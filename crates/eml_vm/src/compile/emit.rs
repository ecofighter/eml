//! 関数ごとの命令の生成 (docs/spec/vm.md の「命令ごとの実行」)。定数の atom は、命令の前に一時のスロットへ読み込む。

use eml_extern::Extern;
use eml_lir::unwind::UnwindPoint;
use eml_lir::{
    Atom, BlockId, Call, Ctor, FnIdx, LayoutCtor, LirFn, Loc, Program, Repr, Rhs, Shape, Stmt,
    Term, VarId,
};
use eml_rt::{MaskItem, Mode};

use super::constants::Constants;
use super::slots::{Slots, allocate};
use super::{Links, Out};
use crate::code::{CompileError, FnEntry, Unwind};
use crate::op::{KEEP, NONE, Op, Pc, RAW, Slot};

pub(crate) fn function<M: Mode>(
    program: &Program,
    index: FnIdx,
    function: &LirFn,
    points: &[UnwindPoint],
    links: &Links,
    constants: &mut Constants<'_, M>,
    out: &mut Out,
) -> Result<FnEntry, CompileError> {
    let slots = allocate(function)?;
    let start = out.pc();
    let mut emitter = Emitter {
        program,
        function,
        index,
        links,
        constants,
        out,
        slots,
        temps: 0,
        max_temps: 0,
        block_pcs: vec![0; function.blocks.len()],
        goto_patches: Vec::new(),
        operand_patches: Vec::new(),
        self_tail_patches: Vec::new(),
        used: used_vars(function),
        elided: elided_decrefs(function),
        points: points.iter().peekable(),
    };
    for block in 0..function.blocks.len() {
        emitter.block_pcs[block] = emitter.out.pc();
        emitter.block(BlockId(block as u32));
    }
    debug_assert!(
        emitter.points.next().is_none(),
        "every unwind point is a statement of the function"
    );
    emitter.patch();
    for &at in &emitter.self_tail_patches {
        emitter.out.operands[at] = start;
    }
    let too_many = || CompileError::TooManySlots {
        function: function.name.clone(),
    };
    let frame_size =
        u16::try_from(usize::from(emitter.slots.colors) + usize::from(emitter.max_temps))
            .map_err(|_| too_many())?;
    Ok(FnEntry {
        name: function.name.clone(),
        start,
        end: emitter.out.pc(),
        arity: u16::try_from(function.params().len()).map_err(|_| too_many())?,
        frame_size,
        ret_rc: function.ret.is_rc(),
    })
}

/// どこかで使う変数。SSA なので、使う変数は定義の後で生きていて、使わない変数の定義は死んでいる。死んでいるフィールドは
/// 読まず、死んでいる `drop` の結果は書かない。
fn used_vars(function: &LirFn) -> Vec<bool> {
    let mut used = vec![false; function.vars.len()];
    let mut mark = |atom: Atom| {
        if let Atom::Var(var) = atom {
            used[var.0 as usize] = true;
        }
    };
    for block in &function.blocks {
        for stmt in &block.stmts {
            match stmt {
                Stmt::Dup(var) | Stmt::Decref(var) => mark(Atom::Var(*var)),
                Stmt::Release { value, fields, .. } => {
                    mark(Atom::Var(*value));
                    fields
                        .iter()
                        .flatten()
                        .for_each(|&var| mark(Atom::Var(var)));
                }
                _ => stmt.for_each_atom(&mut mark),
            }
        }
        block.term.for_each_atom(&mut mark);
    }
    used
}

/// フィールドのない `#N` の行き先にだけ入るブロックと、そこで `decref` を出さない scrutinee。行き先では scrutinee は
/// 即値と分かるので、`decref` は何もしない (docs/spec/vm.md の「RC」)。
fn elided_decrefs(function: &LirFn) -> Vec<Option<VarId>> {
    let mut preds = vec![0usize; function.blocks.len()];
    for block in &function.blocks {
        for target in block.term.successors() {
            preds[target.0 as usize] += 1;
        }
    }
    let mut elided = vec![None; function.blocks.len()];
    for block in &function.blocks {
        if let Term::Switch {
            scrutinee: Atom::Var(var),
            layout: Some(_),
            cases,
            default: _,
        } = &block.term
            && function.repr(*var) == Repr::TObj
        {
            for case in cases {
                if case.fields.is_empty() && preds[case.target.0 as usize] == 1 {
                    elided[case.target.0 as usize] = Some(*var);
                }
            }
        }
    }
    elided
}

pub(crate) struct Emitter<'a, 'c, M: Mode> {
    pub(crate) program: &'a Program,
    pub(crate) function: &'a LirFn,
    /// 今の関数の番号。自己末尾呼び出しを見分ける。
    pub(crate) index: FnIdx,
    /// プログラムの番号から `CodeBase` の番号への写し。関数、文字列、エフェクト、ファイルの番号は、命令に書く前に
    /// これで引き直す (docs/spec/vm.md の「追記」)。
    pub(crate) links: &'a Links,
    pub(crate) constants: &'a mut Constants<'c, M>,
    pub(crate) out: &'a mut Out,
    pub(crate) slots: Slots,
    /// 今の命令が使った一時のスロットの数。命令ごとに 0 に戻す。
    pub(crate) temps: u16,
    pub(crate) max_temps: u16,
    pub(crate) block_pcs: Vec<Pc>,
    /// 行き先のブロックを後で書く `Goto` の位置。
    pub(crate) goto_patches: Vec<(Pc, BlockId)>,
    /// 行き先のブロックを後で書くオペランドの位置。
    pub(crate) operand_patches: Vec<(usize, BlockId)>,
    /// 自己末尾呼び出しのオペランドの位置。先頭の関数の番号を、関数を書き終えた後で入口の命令の位置に書き換える。
    pub(crate) self_tail_patches: Vec<usize>,
    /// どこかで使う変数 (`used_vars`)。
    pub(crate) used: Vec<bool>,
    /// ブロックごとの、`decref` を出さない scrutinee (`elided_decrefs`)。
    pub(crate) elided: Vec<Option<VarId>>,
    /// まだ書いていない巻き戻しの地点。ブロックの順、文の順に並ぶ。
    pub(crate) points: std::iter::Peekable<std::slice::Iter<'a, UnwindPoint>>,
}

impl<'a, M: Mode> Emitter<'a, '_, M> {
    pub(crate) fn slot(&self, var: VarId) -> Slot {
        self.slots.of[var.0 as usize]
    }

    pub(crate) fn repr(&self, var: VarId) -> Repr {
        self.function.repr(var)
    }

    /// 一時のスロットを1つ取る。変数のスロットの後に置き、命令ごとに使い回す。
    pub(crate) fn temp(&mut self) -> Slot {
        let slot = self.slots.colors + self.temps;
        self.temps += 1;
        self.max_temps = self.max_temps.max(self.temps);
        slot
    }

    /// 値をスロットに置く。変数はそのスロットで、定数は一時のスロットに読み込む。`repr` は値を受ける位置の Repr で、
    /// `#N` の書き方 (生の値か即値か) を決める。
    pub(crate) fn atom(&mut self, atom: Atom, repr: Repr) -> Slot {
        match atom {
            Atom::Var(var) => self.slot(var),
            constant => {
                let slot = self.temp();
                self.load(slot, constant, repr);
                slot
            }
        }
    }

    /// 定数を `dst` に書く命令を出す。`#N` は、`enum` の位置では生の値、`tobj` の位置では即値である。
    pub(crate) fn load(&mut self, dst: Slot, constant: Atom, repr: Repr) {
        let small = |n: u32| i32::try_from(n).expect("a tag or function index fits in i32");
        let op = match constant {
            Atom::Unit => Op::LoadImm { dst, value: 0 },
            Atom::Fn(f) => Op::LoadImm {
                dst,
                value: small(self.links.functions[f.0 as usize]),
            },
            Atom::Tag(n) if repr == Repr::TObj => Op::LoadImm {
                dst,
                value: small(n),
            },
            Atom::Tag(n) => Op::LoadRaw {
                dst,
                value: small(n),
            },
            Atom::Int(n) => match i32::try_from(n) {
                Ok(value) => Op::LoadRaw { dst, value },
                Err(_) => Op::LoadConst {
                    dst,
                    index: self.constants.raw(n),
                },
            },
            Atom::Var(_) => unreachable!("load takes a constant"),
        };
        self.out.push(op);
    }

    fn block(&mut self, id: BlockId) {
        let block = self.function.block(id);
        for (i, stmt) in block.stmts.iter().enumerate() {
            self.temps = 0;
            self.stmt(id, stmt);
            if let Some(point) = self.points.next_if(|p| (p.block, p.stmt) == (id, i)) {
                self.unwind(point);
            }
        }
        self.temps = 0;
        self.term(id, &block.term);
    }

    fn stmt(&mut self, block: BlockId, stmt: &Stmt) {
        match stmt {
            Stmt::Let { var, rhs } => self.rhs(*var, rhs),
            Stmt::Dup(var) => {
                let src = self.slot(*var);
                self.out.push(Op::Dup { src });
            }
            Stmt::Decref(var) => {
                if self.elided[block.0 as usize] != Some(*var) {
                    let src = self.slot(*var);
                    self.out.push(Op::Decref { src });
                }
            }
            Stmt::Unpack {
                value,
                ctor,
                fields,
            } => {
                let layout_ctor = self.layout_ctor(*ctor);
                let shape = layout_ctor.shape();
                let bound = self.field_operands(layout_ctor, &shape, fields);
                let at = self.out.operands(
                    [ctor.tag, shape.scanned, (bound.len() / 2) as u32]
                        .into_iter()
                        .chain(bound),
                );
                let src = self.slot(*value);
                self.out.push(Op::Unpack { src, at });
            }
            Stmt::Release {
                value,
                ctor,
                fields,
            } => {
                let layout_ctor = self.layout_ctor(*ctor);
                let shape = layout_ctor.shape();
                let offset = crate::data::count_word(shape.scanned);
                let entries: Vec<u32> = fields
                    .iter()
                    .zip(&shape.positions)
                    .zip(&layout_ctor.fields)
                    .filter(|(_, repr)| repr.is_rc())
                    .map(|((field, &position), _)| {
                        (position + offset) | if field.is_some() { KEEP } else { 0 }
                    })
                    .collect();
                let at = self.out.operands(
                    [ctor.tag, shape.scanned, entries.len() as u32]
                        .into_iter()
                        .chain(entries),
                );
                let src = self.slot(*value);
                self.out.push(Op::Release { src, at });
            }
        }
    }

    /// 巻き戻しの表の行を足す。地点の命令は文の最後に書くので、今の位置がその命令の次の位置である
    /// (docs/superpowers/specs/2026-10-17-s13c-unwinding-design.md の「巻き戻しの表」)。
    fn unwind(&mut self, point: &UnwindPoint) {
        let entry = Unwind {
            slots: point
                .owned
                .iter()
                .map(|&(var, count)| (self.slot(var), count))
                .collect(),
            landing: point.landing.map(|var| self.slot(var)),
        };
        let pc = self.out.pc();
        self.out.unwinds.push((pc, entry));
    }

    /// コンストラクタの配置。命令ごとに1回引き、形 (`LayoutCtor::shape`) とともに使い回す。
    fn layout_ctor(&self, ctor: Ctor) -> &'a LayoutCtor {
        &self.program.layouts[ctor.layout.0 as usize].constructors[ctor.tag as usize]
    }

    /// 使うフィールドの [位置 | RAW, スロット] の並び。死んでいるフィールドは読まない。
    fn field_operands(
        &self,
        layout_ctor: &LayoutCtor,
        shape: &Shape,
        fields: &[VarId],
    ) -> Vec<u32> {
        let offset = crate::data::count_word(shape.scanned);
        let mut items = Vec::new();
        for ((&field, &position), &repr) in
            fields.iter().zip(&shape.positions).zip(&layout_ctor.fields)
        {
            if self.used[field.0 as usize] {
                let raw = if repr.is_rc() { 0 } else { RAW };
                items.push((position + offset) | raw);
                items.push(u32::from(self.slot(field)));
            }
        }
        items
    }

    fn rhs(&mut self, var: VarId, rhs: &Rhs) {
        let dst = self.slot(var);
        match rhs {
            Rhs::Call(Call::Direct(f, args)) => {
                let at = self.call_operands(*f, args);
                self.out.push(Op::Call { dst, at });
            }
            Rhs::Extern { ext, args, at } => {
                let row = ext.row();
                let slots: Vec<Slot> = args
                    .iter()
                    .zip(row.params)
                    .map(|(&arg, &repr)| self.atom(arg, repr))
                    .collect();
                let op = arithmetic(*ext, dst, &slots).unwrap_or_else(|| {
                    let at = self.out.operands(
                        [*ext as u32, slots.len() as u32]
                            .into_iter()
                            .chain(slots.iter().map(|&s| u32::from(s))),
                    );
                    Op::Extern { dst, at }
                });
                let pc = self.out.push(op);
                if let Some(loc) = at {
                    let file = self.links.files[loc.file as usize];
                    self.out.locations.push((pc, Loc { file, ..*loc }));
                }
            }
            Rhs::ConstString(index) => {
                let index = self.links.strings[*index as usize];
                self.out.push(Op::LoadConst { dst, index });
            }
            Rhs::Con { ctor, args } => {
                let layout_ctor = self.layout_ctor(*ctor);
                let shape = layout_ctor.shape();
                let offset = crate::data::count_word(shape.scanned);
                let lin = self.function.mark(var) == eml_lir::Mark::Lin;
                let mut items = vec![
                    ctor.tag,
                    shape.scanned,
                    shape.raw,
                    u32::from(lin),
                    args.len() as u32,
                ];
                for ((&arg, &position), &repr) in
                    args.iter().zip(&shape.positions).zip(&layout_ctor.fields)
                {
                    let slot = self.atom(arg, repr);
                    let raw = if repr.is_rc() { 0 } else { RAW };
                    items.push((position + offset) | raw);
                    items.push(u32::from(slot));
                }
                let at = self.out.operands(items);
                self.out.push(Op::Con { dst, at });
            }
            Rhs::Drop(atom) => {
                if let Atom::Var(v) = atom
                    && self.repr(*v).is_rc()
                {
                    let src = self.slot(*v);
                    self.out.push(Op::Decref { src });
                }
                if self.used[var.0 as usize] {
                    self.out.push(Op::LoadImm { dst, value: 0 });
                }
            }
            Rhs::Box(atom) => {
                let op = match *atom {
                    Atom::Int(n) => Op::BoxConst {
                        dst,
                        index: self.constants.boxed_int(n),
                    },
                    Atom::Var(v) if self.repr(v) == Repr::Enum => Op::BoxEnum {
                        dst,
                        src: self.slot(v),
                    },
                    Atom::Var(v) => Op::BoxInt {
                        dst,
                        src: self.slot(v),
                    },
                    other => panic!("a box of {other:?} does not pass the verifier"),
                };
                self.out.push(op);
            }
            Rhs::Unbox(atom) => {
                let Atom::Var(v) = atom else {
                    panic!("an unbox of a constant does not pass the verifier")
                };
                let src = self.slot(*v);
                self.out.push(Op::Unbox { dst, src });
            }
            Rhs::MakeClosure(f, args) => {
                let callee = self.program.function(*f);
                let slots: Vec<Slot> = args
                    .iter()
                    .zip(callee.params())
                    .map(|(&arg, &param)| self.atom(arg, callee.repr(param)))
                    .collect();
                let lin = self.function.mark(var) == eml_lir::Mark::Lin;
                let at = self.out.operands(
                    [
                        self.links.functions[f.0 as usize],
                        u32::from(lin),
                        slots.len() as u32,
                    ]
                    .into_iter()
                    .chain(slots.iter().map(|&s| u32::from(s))),
                );
                self.out.push(Op::Closure { dst, at });
            }
            Rhs::Call(call @ (Call::Apply { .. } | Call::Clause { .. })) => {
                let at = self.value_call_operands(call);
                let op = if matches!(call, Call::Clause { .. }) {
                    Op::Clause { dst, at }
                } else {
                    Op::Apply { dst, at }
                };
                self.out.push(op);
            }
            Rhs::Prim(prim, args) => {
                let row = prim.row();
                let slots: Vec<Slot> = args
                    .iter()
                    .zip(row.params)
                    .map(|(&arg, &repr)| self.atom(arg, repr))
                    .collect();
                let op = self.prim(prim, dst, &slots);
                self.out.push(op);
            }
        }
    }

    fn prim(&mut self, prim: &eml_lir::Prim, dst: Slot, a: &[Slot]) -> Op {
        use eml_lir::Prim;
        let op_index = |o: u32| u16::try_from(o).expect("operations fit in 16 bits");
        match prim {
            Prim::EvvGet => Op::EvvGet { dst },
            Prim::EvvSet => Op::EvvSet { dst, l: a[0] },
            Prim::EvvPush => Op::EvvPush {
                dst,
                l: a[0],
                e: a[1],
            },
            Prim::EvvMask(items) => {
                let start = self.out.mask_lists.len() as u32;
                // 引き直した番号でも昇順に保つ。項目は (エフェクト, keep, skip) を 1 つの単位として動かす
                // (docs/spec/lir.md の「基本操作」)
                let mut items: Vec<MaskItem> = items
                    .iter()
                    .map(|item| MaskItem {
                        effect: self.effect(item.effect),
                        keep: item.keep,
                        skip: item.skip,
                    })
                    .collect();
                items.sort_unstable_by_key(|item| item.effect);
                let len = items.len() as u32;
                self.out.mask_lists.extend(items);
                let at = self.out.operands([start, len, u32::from(a[0])]);
                Op::EvvMask { dst, at }
            }
            Prim::EvNew(e) => {
                let n_ops = self.program.effects[*e as usize].operations.len() as u32;
                let at = self.out.operands([
                    self.effect(*e).into(),
                    n_ops,
                    u32::from(a[0]),
                    u32::from(a[1]),
                ]);
                Op::EvNew { dst, at }
            }
            Prim::EvSetClause { effect: _, op } => {
                let at = self.out.operands([u32::from(a[0]), *op, u32::from(a[1])]);
                Op::EvSetClause { dst, at }
            }
            Prim::EvRenew => Op::EvRenew {
                dst,
                ev: a[0],
                state: a[1],
            },
            Prim::EvLookup(e) => Op::EvLookup {
                dst,
                effect: self.effect(*e),
            },
            Prim::EvClause { effect: _, op } => Op::EvClause {
                dst,
                ev: a[0],
                op: op_index(*op),
            },
            Prim::EvEvv => Op::EvEvv { dst, ev: a[0] },
            Prim::EvTakeState => Op::EvTakeState { dst, ev: a[0] },
            Prim::EvSetState => Op::EvSetState {
                dst,
                ev: a[0],
                s: a[1],
            },
            Prim::Yielding => Op::Yielding { dst },
            Prim::YieldStatus => Op::YieldStatus { dst, ev: a[0] },
            Prim::YieldTo => {
                let at = self.out.operands(a.iter().map(|&s| u32::from(s)));
                Op::YieldTo { dst, at }
            }
            Prim::YieldAbort => Op::YieldAbort {
                ev: a[0],
                clause: a[1],
            },
            Prim::YieldContTake => Op::YieldContTake { dst },
            Prim::YieldContSet => Op::YieldContSet { dst, k: a[0] },
            Prim::YieldFinish => Op::YieldFinish { dst },
            Prim::Dummy => Op::Dummy { dst },
            Prim::RootGet(root) => Op::RootGet { dst, root: *root },
            Prim::RootSet(root) => {
                let at = self.out.operands([*root, u32::from(a[0])]);
                Op::RootSet { dst, at }
            }
        }
    }

    /// `apply` と `clause` のオペランド [呼ばれる値, n, 引数…]。`apply` の位置は一様 (`tobj`)、`clause` の引数は操作の
    /// 表の Repr で、先頭の evidence は `obj` である (docs/spec/lir.md の「節の呼び出し」)。
    fn value_call_operands(&mut self, call: &Call) -> u32 {
        let (callee, args, reprs): (Atom, &[Atom], Vec<Repr>) = match call {
            Call::Apply { callee, args, .. } => (*callee, args, vec![Repr::TObj; args.len()]),
            Call::Clause {
                effect,
                op,
                callee,
                args,
            } => {
                let operation = eml_lir::operation(&self.program.effects, *effect, *op);
                let reprs = std::iter::once(Repr::Obj)
                    .chain(operation.params.iter().copied())
                    .collect();
                (*callee, args, reprs)
            }
            Call::Direct(..) => unreachable!("a direct call has its own instruction"),
        };
        let callee = self.atom(callee, Repr::TObj);
        let slots: Vec<Slot> = args
            .iter()
            .zip(reprs)
            .map(|(&arg, repr)| self.atom(arg, repr))
            .collect();
        self.out.operands(
            [u32::from(callee), slots.len() as u32]
                .into_iter()
                .chain(slots.iter().map(|&s| u32::from(s))),
        )
    }

    /// 直接の呼び出しのオペランド [関数の番号, 引数の数, 引数のスロット…]。
    pub(crate) fn call_operands(&mut self, f: eml_lir::FnIdx, args: &[Atom]) -> u32 {
        let callee = self.program.function(f);
        let slots: Vec<Slot> = args
            .iter()
            .zip(callee.params())
            .map(|(&arg, &param)| self.atom(arg, callee.repr(param)))
            .collect();
        self.out.operands(
            [self.links.functions[f.0 as usize], slots.len() as u32]
                .into_iter()
                .chain(slots.iter().map(|&s| u32::from(s))),
        )
    }

    fn term(&mut self, block: BlockId, term: &Term) {
        match term {
            Term::Return(atom) => {
                let src = self.atom(*atom, self.function.ret);
                self.out.push(Op::Ret { src });
            }
            Term::TailCall(Call::Direct(f, args)) => {
                let at = self.call_operands(*f, args);
                if *f == self.index {
                    self.self_tail_patches.push(at as usize);
                    self.out.push(Op::TailSelf { at });
                } else {
                    self.out.push(Op::TailCall { at });
                }
            }
            Term::TailCall(call) => {
                let at = self.value_call_operands(call);
                let op = if matches!(call, Call::Clause { .. }) {
                    Op::TailClause { at }
                } else {
                    Op::TailApply { at }
                };
                self.out.push(op);
            }
            Term::Jump { target, args } => self.jump(block, *target, args),
            Term::Switch {
                scrutinee,
                layout,
                cases,
                default,
            } => self.switch(block, *scrutinee, *layout, cases, *default),
        }
    }

    /// ブロック引数への並列な代入と、行き先への `Goto`。変数の代入を先に並べ、定数はその後に書く。定数の書き込み先が、
    /// ほかの代入がまだ読む値のスロットでありうるためである。次のブロックへ進むだけなら `Goto` を出さない。
    pub(crate) fn jump(&mut self, from: BlockId, target: BlockId, args: &[Atom]) {
        let function = self.function;
        let params = &function.block(target).params;
        let mut moves = Vec::new();
        let mut constants = Vec::new();
        for (&param, &arg) in params.iter().zip(args) {
            let dst = self.slot(param);
            match arg {
                Atom::Var(var) => moves.push((dst, self.slot(var))),
                constant => constants.push((dst, constant, self.repr(param))),
            }
        }
        // 作業用のスロットは、代入が輪になるときだけ取る
        let ordered = super::moves::sequentialize(&moves, || self.temp());
        for (dst, src) in ordered {
            self.out.push(Op::Move { dst, src });
        }
        for (dst, constant, repr) in constants {
            self.load(dst, constant, repr);
        }
        self.goto(from, target);
    }

    fn switch(
        &mut self,
        block: BlockId,
        scrutinee: Atom,
        layout: Option<eml_lir::LayoutId>,
        cases: &[eml_lir::Case],
        default: Option<BlockId>,
    ) {
        use eml_lir::CasePattern;
        let Atom::Var(var) = scrutinee else {
            // 定数の scrutinee は、コード生成の時点で行き先を決める
            let target = cases
                .iter()
                .find(|case| match (case.pattern, scrutinee) {
                    (CasePattern::Tag(t), Atom::Tag(n)) => t == n,
                    (CasePattern::Int(k), Atom::Int(n)) => k == n,
                    _ => false,
                })
                .map(|case| case.target)
                .or(default)
                .expect("a constant switch has a matching case or a default");
            self.goto(block, target);
            return;
        };
        // case のない `switch` は値を比べずに `default` へ進むので、キーのない表を作らない
        let Some(first) = cases.first().map(|case| case.pattern) else {
            let target = default.expect("a switch without cases has a default");
            self.goto(block, target);
            return;
        };
        let base = self.out.operands.len();
        let mut patches = Vec::new();
        if let Some(default) = default {
            patches.push((1, default));
        }
        let src = self.slot(var);
        let (items, op) = match (first, layout) {
            (CasePattern::Int(_) | CasePattern::String(_), _) => {
                let mut items = vec![cases.len() as u32, NONE];
                for case in cases {
                    let (lo, hi) = match case.pattern {
                        CasePattern::Int(n) => (n as u32, (n >> 32) as u32),
                        CasePattern::String(index) => (self.links.strings[index as usize], 0),
                        CasePattern::Tag(_) => unreachable!("one switch compares one kind of case"),
                    };
                    patches.push((items.len() + 2, case.target));
                    items.extend([lo, hi, 0]);
                }
                let at = base as u32;
                let op = if matches!(first, CasePattern::String(_)) {
                    Op::SwitchString { src, at }
                } else {
                    Op::SwitchInt { src, at }
                };
                (items, op)
            }
            (CasePattern::Tag(_), Some(layout)) => {
                // 配置のタグは 0 から m - 1 に収まるので、飛び先の表はいつも密である
                let ctors = &self.program.layouts[layout.0 as usize].constructors;
                let m = ctors.len();
                let mut items = vec![m as u32, NONE];
                items.extend(std::iter::repeat_n(NONE, m));
                for case in cases {
                    let CasePattern::Tag(tag) = case.pattern else {
                        unreachable!("one switch compares one kind of case")
                    };
                    let (check, fields) = if case.fields.is_empty() {
                        (NONE, Vec::new())
                    } else {
                        let layout_ctor = &ctors[tag as usize];
                        let shape = layout_ctor.shape();
                        let fields = self.field_operands(layout_ctor, &shape, &case.fields);
                        (shape.scanned, fields)
                    };
                    items[2 + tag as usize] = items.len() as u32;
                    patches.push((items.len(), case.target));
                    items.extend([0, check, (fields.len() / 2) as u32]);
                    items.extend(fields);
                }
                let at = base as u32;
                let op = match self.repr(var) {
                    Repr::Enum => Op::SwitchEnum { src, at },
                    Repr::Obj => Op::SwitchObj { src, at },
                    _ => Op::SwitchTObj { src, at },
                };
                (items, op)
            }
            (CasePattern::Tag(_), None) => {
                unreachable!("a switch with tag cases names its layout (docs/spec/lir.md)")
            }
        };
        for (offset, target) in patches {
            self.operand_patches.push((base + offset, target));
        }
        self.out.operands(items);
        self.out.push(op);
    }

    /// プログラムのエフェクトの番号から実行時の番号。
    fn effect(&self, e: u32) -> u16 {
        self.links.effects[e as usize]
    }

    pub(crate) fn goto(&mut self, from: BlockId, target: BlockId) {
        if target.0 != from.0 + 1 {
            let pc = self.out.push(Op::Goto { target: 0 });
            self.goto_patches.push((pc, target));
        }
    }

    fn patch(&mut self) {
        for &(pc, block) in &self.goto_patches {
            self.out.code[pc as usize] = Op::Goto {
                target: self.block_pcs[block.0 as usize],
            };
        }
        for &(index, block) in &self.operand_patches {
            self.out.operands[index] = self.block_pcs[block.0 as usize];
        }
    }
}

/// 実行時のエフェクトの番号を命令の `u16` にする。evidence の tag はエフェクトの番号 + 1 なので、番号は 65535 より
/// 小さい (docs/spec/object-model.md の「evidence の3つの物体」)。速いモードの `ev_new` はこれを確かめないので、
/// 番号を振る `CodeBase::load` が panic で確かめる。
pub(crate) fn effect_number(e: u32) -> u16 {
    u16::try_from(e)
        .ok()
        .filter(|&e| e < u16::MAX)
        .expect("effect numbers are below 65535 (docs/spec/object-model.md)")
}

/// 算術と比較の extern の専用の命令 (docs/spec/vm.md の「基本操作と extern」)。
fn arithmetic(ext: Extern, dst: Slot, args: &[Slot]) -> Option<Op> {
    if let (Extern::IntNeg, [a]) = (ext, args) {
        return Some(Op::IntNeg { dst, a: *a });
    }
    let [a, b] = *args else { return None };
    Some(match ext {
        Extern::IntAdd => Op::IntAdd { dst, a, b },
        Extern::IntSub => Op::IntSub { dst, a, b },
        Extern::IntMul => Op::IntMul { dst, a, b },
        Extern::IntDiv => Op::IntDiv { dst, a, b },
        Extern::IntMod => Op::IntRem { dst, a, b },
        Extern::IntEq => Op::IntEq { dst, a, b },
        Extern::IntNe => Op::IntNe { dst, a, b },
        Extern::IntLt => Op::IntLt { dst, a, b },
        Extern::IntLe => Op::IntLe { dst, a, b },
        Extern::IntGt => Op::IntGt { dst, a, b },
        Extern::IntGe => Op::IntGe { dst, a, b },
        Extern::IntCompare => Op::IntCompare { dst, a, b },
        Extern::BoolEq => Op::BoolEq { dst, a, b },
        Extern::BoolNe => Op::BoolNe { dst, a, b },
        Extern::Println
        | Extern::Open
        | Extern::ReadAll
        | Extern::ReadLine
        | Extern::Close
        | Extern::IntShow
        | Extern::IntNeg
        | Extern::StrConcat
        | Extern::StrEq
        | Extern::StrNe
        | Extern::StrCompare
        | Extern::StrShow
        | Extern::Eprintln
        | Extern::StrByteLength
        | Extern::StrByteAt
        | Extern::StrSlice
        | Extern::StrFindFrom
        | Extern::EnvArgCount
        | Extern::EnvArg
        | Extern::EnvHas
        | Extern::EnvVar
        | Extern::StdinReadAll
        | Extern::StdinReadLine
        | Extern::WriteText
        | Extern::ProcStatus
        | Extern::Exit => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::effect_number;

    /// 番号 65534 までは変換し、65535 はコード生成で断る。番号 65535 のエフェクトを持つプログラムを作る統合テストは
    /// 遅いので、変換の関数を直に確かめる。
    #[test]
    #[should_panic(expected = "effect numbers are below 65535")]
    fn effect_65535_is_rejected_by_the_code_generator() {
        assert_eq!(effect_number(65534), 65534);
        effect_number(65535);
    }
}
