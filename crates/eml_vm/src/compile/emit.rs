//! 関数ごとの命令の生成 (docs/spec/vm.md の「命令ごとの実行」)。定数の atom は、命令の前に一時のスロットへ読み込む。

use eml_extern::Extern;
use eml_lir::{Atom, BlockId, Call, LirFn, Program, Repr, Rhs, Stmt, Term, VarId};
use eml_rt::Mode;

use super::Out;
use super::constants::Constants;
use super::slots::{Slots, allocate};
use crate::code::{CompileError, FnEntry};
use crate::op::{Op, Pc, Slot};

pub(crate) fn function<M: Mode>(
    program: &Program,
    function: &LirFn,
    strings: &[u32],
    constants: &mut Constants<M>,
    out: &mut Out,
) -> Result<FnEntry, CompileError> {
    let slots = allocate(function)?;
    let start = out.pc();
    let mut emitter = Emitter {
        program,
        function,
        strings,
        constants,
        out,
        slots,
        temps: 0,
        max_temps: 0,
        block_pcs: vec![0; function.blocks.len()],
        goto_patches: Vec::new(),
        operand_patches: Vec::new(),
        used: used_vars(function),
    };
    for index in 0..function.blocks.len() {
        emitter.block_pcs[index] = emitter.out.pc();
        emitter.block(BlockId(index as u32));
    }
    emitter.patch();
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

pub(crate) struct Emitter<'a, M: Mode> {
    pub(crate) program: &'a Program,
    pub(crate) function: &'a LirFn,
    pub(crate) strings: &'a [u32],
    pub(crate) constants: &'a mut Constants<M>,
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
    /// Task 4 のフィールドの読み込みが使う
    #[allow(dead_code)]
    pub(crate) used: Vec<bool>,
}

impl<M: Mode> Emitter<'_, M> {
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
                value: small(f.0),
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
        for stmt in &block.stmts {
            self.temps = 0;
            self.stmt(id, stmt);
        }
        self.temps = 0;
        self.term(id, &block.term);
    }

    fn stmt(&mut self, _block: BlockId, stmt: &Stmt) {
        match stmt {
            Stmt::Let { var, rhs } => self.rhs(*var, rhs),
            // Task 4 と Task 5 で足し、Task 6 でこの腕を消す
            other => panic!("the VM does not compile {other:?} yet"),
        }
    }

    fn rhs(&mut self, var: VarId, rhs: &Rhs) {
        let dst = self.slot(var);
        match rhs {
            Rhs::Call {
                call: Call::Direct(f, args),
                saved: _,
            } => {
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
                    self.out.locations.push((pc, *loc));
                }
            }
            Rhs::ConstString(index) => {
                let index = self.strings[*index as usize];
                self.out.push(Op::LoadConst { dst, index });
            }
            // Task 4 と Task 5 で足し、Task 6 でこの腕を消す
            other => panic!("the VM does not compile {other:?} yet"),
        }
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
            [f.0, slots.len() as u32]
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
                self.out.push(Op::TailCall { at });
            }
            Term::Jump { target, args } => self.jump(block, *target, args),
            // Task 4 と Task 5 で足し、Task 6 でこの腕を消す
            other => panic!("the VM does not compile {other:?} yet"),
        }
    }

    /// ブロック引数への並列な代入と、行き先への `Goto`。変数の代入を先に並べ、定数はその後に書く。定数の書き込み先が、
    /// ほかの代入がまだ読む値のスロットでありうるためである。次のブロックへ進むだけなら `Goto` を出さない。
    pub(crate) fn jump(&mut self, from: BlockId, target: BlockId, args: &[Atom]) {
        let params = self.function.block(target).params.clone();
        let mut moves = Vec::new();
        let mut constants = Vec::new();
        for (&param, &arg) in params.iter().zip(args) {
            let dst = self.slot(param);
            match arg {
                Atom::Var(var) => moves.push((dst, self.slot(var))),
                constant => constants.push((dst, constant, self.repr(param))),
            }
        }
        let scratch = self.temp();
        for (dst, src) in super::moves::sequentialize(&moves, scratch) {
            self.out.push(Op::Move { dst, src });
        }
        for (dst, constant, repr) in constants {
            self.load(dst, constant, repr);
        }
        self.goto(from, target);
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
        _ => return None,
    })
}
