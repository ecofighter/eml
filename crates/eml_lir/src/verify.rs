//! LIR の不変条件の検査 (docs/spec/lir.md の「verifier」)。ブロックの列の形 (R1〜R4)、変数の定義と支配 (R5、R6)、
//! `unpack` の Repr、`jump` と `return` の Repr の互換、extern の引数と結果の Repr、`box` と `unbox` のオペランドと
//! 束縛の Repr (R7)、データの配置 (R8) と、引き継いだ検査 (直接呼び出しと extern の引数の数、case の種類)、基本操作と
//! 節の呼び出しの形を確かめる (`verify_scopes`)。R7 と R8 には境界の検査も入る。呼び出しの引数と結果、`closure` の
//! 引数、関数の値の対象が一様であること、`tail` の結果、`tobj` のフィールドを比べる。box の挿入の前の入口の段では、
//! 境界を確かめずに、`box`、`unbox`、`tail` と、Perceus が入れるものがまだないことを確かめる (`verify_lowered`)。
//! Perceus の後は、RC の対象の所有の多重集合 (R6) も確かめる (`verify`)。`switch`、`unpack`、`unbox` は値を読む
//! だけである。`switch` と `unpack` のフィールドは値から借りて始まり、自分か持ち主が所有を持つ間だけ有効である。
//! 段の分け方は docs/spec/lir.md の「verifier」による。Core IR の verifier と共有する部品は `check` にある。
//!
//! R8 は、`con`、タグの `switch`、`unpack`、`release` を、その命令が指す配置と比べる。配置を持つ `switch` はタグの
//! case を持つものだけで、リテラルの `switch` では scrutinee の Repr を比べる。値がどの配置で作られたかは
//! 追わない。それを保証するのは translate の型なので、配置の違う値を読む IR もこの検査を通りうる
//! (docs/spec/lir.md の「データの配置」)。
//!
//! 辺の検査、支配木、本体の検査は、それぞれブロックを番号の順に1回たどるだけで、反復も生存解析も使わない。辺は
//! 前向きなので、ブロックに着いたときには入る辺がすべて出そろっている。支配木は辺1本につき深さの対数の手間で
//! 育つ。所有の多重集合は辺ごとに写して比べるので、その手間は辺の数と所有の大きさの積に比例する
//! (docs/spec/lir.md の「verifier」)。
//!
//! 印は Repr を比べる所と同じ所で比べる (V2)。入口の段では `jump` と `return` だけを、範囲の段と所有の段では境界の
//! すべてを比べる。`con` と `closure` が束縛する変数の印が引数の印の結び以上であることはどの段でも確かめ (作る所の
//! 検査)、`dup` と `decref` の対象が `unr` であることは所有の段で確かめる (V1)。段の分け方は
//! docs/spec/lir.md の「Kind の印の検査」による。

use std::collections::{BTreeMap, HashMap};

use crate::check::{
    BlockShape, Context, Dominators, FnSummary, Tables, TermShape, boxed_scalars, clause_function,
    field_count, shape,
};
use crate::syntax::{operation_text, prim_text};
use crate::{
    Atom, Block, BlockId, Call, CasePattern, Ctor, FnIdx, Layout, LirFn, Mark, OpRet, Prim,
    Program, Repr, Rhs, Stmt, Term, VarId, VerifyError, operation,
};

/// Perceus の後の LIR を確かめる。形と範囲に加えて、RC の対象の変数の所有が釣り合うことを確かめる。
pub fn verify(program: &Program) -> Result<(), VerifyError> {
    verify_at(program, Level::Ownership)
}

/// Perceus より前の LIR を確かめる。形と範囲と境界を確かめ、RC の命令がまだないことを確かめる。
pub fn verify_scopes(program: &Program) -> Result<(), VerifyError> {
    verify_at(program, Level::Scopes)
}

/// box の挿入の前の LIR を確かめる。範囲の段と同じ形と範囲を確かめ、境界は比べない。box の挿入と縮約が作るもの
/// (`box`、`unbox`、`tail`) と、Perceus が入れるもの (`dup`、`decref`、`release`) がまだないことを確かめる。
/// box の挿入は、末尾呼び出しと変換のない入力を前提にするためである (docs/spec/lir.md の「パスの順番」)。
pub fn verify_lowered(program: &Program) -> Result<(), VerifyError> {
    verify_at(program, Level::Lowered)
}

/// 検査の段。段はパスの順に直線に並ぶ。入口の段の LIR には、box の挿入と縮約が作るものがまだない。Perceus より前の
/// LIR には、所有を確かめる材料 (`dup`、`decref`、`release`) がまだない。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Level {
    Lowered,
    Scopes,
    Ownership,
}

fn verify_at(program: &Program, level: Level) -> Result<(), VerifyError> {
    // 配置の Repr はコンストラクタをすべてたどって決まるので、命令ごとでなく表ごとに1回だけ求める
    let layout_reprs: Vec<Repr> = program.layouts.iter().map(Layout::repr).collect();
    let functions: Vec<FnSummary> = program
        .functions
        .iter()
        .map(|function| {
            FnSummary::new(
                &function.name,
                function.clause,
                param_reprs(function),
                function.ret,
            )
        })
        .collect();
    let tables = Tables {
        layouts: &program.layouts,
        layout_reprs: &layout_reprs,
        effects: &program.effects,
        strings: &program.strings,
        functions: &functions,
    };
    // `clause` の印の関数の ABI は、参照ごとでなく関数ごとに1回だけ確かめる
    for function in &program.functions {
        clause_function(
            &program.effects,
            &function.name,
            function.clause,
            function
                .params()
                .iter()
                .map(|&param| (param, &function.vars[param.0 as usize])),
            function.ret,
            function.ret_mark,
            level >= Level::Scopes,
        )
        .map_err(|message| VerifyError {
            function: function.name.clone(),
            message,
        })?;
    }
    for function in &program.functions {
        shape(&block_shapes(function))
            .and_then(|dominators| Checker::new(program, tables, function, level, dominators).run())
            .map_err(|message| VerifyError {
                function: function.name.clone(),
                message,
            })?;
    }
    Ok(())
}

fn param_reprs(function: &LirFn) -> impl ExactSizeIterator<Item = Repr> + '_ {
    function.params().iter().map(|&param| function.repr(param))
}

/// 辺の規則の検査 (`shape`) に渡す、ブロックの形。
fn block_shapes(function: &LirFn) -> Vec<BlockShape> {
    function
        .blocks
        .iter()
        .map(|block| BlockShape {
            params: block.params.len(),
            term: match &block.term {
                Term::Return(_) | Term::TailCall(_) => TermShape::Exit,
                Term::Jump { target, args } => TermShape::Jump {
                    target: *target,
                    args: args.len(),
                },
                Term::Switch { .. } => TermShape::Switch {
                    targets: block.term.successors().collect(),
                },
            },
        })
        .collect()
}

/// 呼び出しの結果を受ける所。`let` の束縛か、`tail` の呼び出し元の `ret` である。
#[derive(Clone, Copy)]
enum Receiver {
    Bound(VarId),
    Tail,
}

/// フィールドの出どころ。分解した値、コンストラクタ、位置である。`release` が名前を書いた変数を確かめるのに使う。
/// フィールドの数は、分解と `release` のどちらも配置で確かめるので持たない (R8)。
#[derive(Clone, Copy, PartialEq, Eq)]
struct Origin {
    value: VarId,
    ctor: Ctor,
    slot: usize,
}

/// 定義と使用の位置。`index` はブロックの中の文の番号で、ブロックの引数と case のフィールドは -1、終端は文の数である。
#[derive(Clone, Copy)]
struct Site {
    block: u32,
    index: i64,
}

/// RC の対象の変数ごとの、所有している参照の数。0 になった変数は除く。
type Owned = BTreeMap<VarId, u32>;

struct Checker<'a> {
    program: &'a Program,
    function: &'a LirFn,
    level: Level,
    dominators: Dominators,
    /// 所有にも命令の型にも関わらない検査の状態。
    cx: Context<'a>,
    defs: Vec<Option<Site>>,
    at: Site,
    /// まだ着いていないブロックの入口の所有。`switch` の行き先は1本の辺から、合流するブロックは最初の `jump` から
    /// 作る。
    pending: HashMap<u32, Owned>,
    /// フィールドの持ち主と出どころ。持ち主は、分解した値が束縛の時点で所有を持っていればその値、なければその値の
    /// 持ち主である。変数は1回だけ定義されるので (R5)、経路ごとではなく関数に1つの表にする
    /// (docs/spec/lir.md の「verifier」)。
    owners: Vec<Option<VarId>>,
    origins: Vec<Option<Origin>>,
}

impl<'a> Checker<'a> {
    fn new(
        program: &'a Program,
        tables: Tables<'a>,
        function: &'a LirFn,
        level: Level,
        dominators: Dominators,
    ) -> Self {
        Checker {
            program,
            function,
            level,
            dominators,
            // 境界の検査は、box の挿入の後の段 (範囲の段と所有の段) だけで行う
            cx: Context::new(tables, &function.vars, level >= Level::Scopes),
            defs: vec![None; function.vars.len()],
            at: Site {
                block: 0,
                index: -1,
            },
            pending: HashMap::new(),
            owners: vec![None; function.vars.len()],
            origins: vec![None; function.vars.len()],
        }
    }

    fn run(mut self) -> Result<(), String> {
        let function = self.function;
        self.clause_values()?;
        for (index, block) in function.blocks.iter().enumerate() {
            let index = index as u32;
            let mut owned = if index == 0 {
                Owned::new()
            } else {
                self.pending
                    .remove(&index)
                    .expect("R3 and R4 give every block an edge from an earlier block")
            };
            self.at = Site {
                block: index,
                index: -1,
            };
            for &param in &block.params {
                self.define(&mut owned, param, self.at)?;
            }
            self.check_block(index, block, owned)?;
        }
        Ok(())
    }

    /// `clause` の印の関数の値 (`&g` と `closure g(..)` の変数) は、`ev_set_clause` の2つ目の引数にだけ使える。入口の
    /// 関数は操作の ABI で呼ばれ、`apply` の一様な規約に合わないためである
    /// (docs/spec/lir.md の「入口の関数の印」)。変数は1回だけ
    /// 定義されるので (R5)、先に `closure` をすべて集めてから使いを見る。
    fn clause_values(&self) -> Result<(), String> {
        let blocks = &self.function.blocks;
        let closures: HashMap<VarId, FnIdx> = blocks
            .iter()
            .flat_map(|block| &block.stmts)
            .filter_map(|stmt| match stmt {
                Stmt::Let {
                    var,
                    rhs: Rhs::MakeClosure(target, _),
                } if self.cx.clause_of(*target).is_some() => Some((*var, *target)),
                _ => None,
            })
            .collect();
        let mut result = Ok(());
        let mut check = |atom: Atom| {
            let target = match atom {
                Atom::Fn(target) => target,
                Atom::Var(var) => match closures.get(&var) {
                    Some(&target) => target,
                    None => return,
                },
                Atom::Int(_) | Atom::Unit | Atom::Tag(_) => return,
            };
            if result.is_ok()
                && let Some(clause) = self.cx.clause_of(target)
            {
                result = Err(format!(
                    "`{}` is a clause of `{clause}`, but it is used as a function value",
                    self.program.functions[target.0 as usize].name
                ));
            }
        };
        for block in blocks {
            for stmt in &block.stmts {
                match stmt {
                    Stmt::Let {
                        var: _,
                        rhs: Rhs::Prim(Prim::EvSetClause { .. }, args),
                    } => args
                        .iter()
                        .enumerate()
                        .filter(|&(index, _)| index != 1)
                        .for_each(|(_, &atom)| check(atom)),
                    _ => stmt.for_each_atom(&mut check),
                }
            }
            block.term.for_each_atom(&mut check);
        }
        result
    }

    fn check_block(&mut self, index: u32, block: &Block, mut owned: Owned) -> Result<(), String> {
        for (at, stmt) in block.stmts.iter().enumerate() {
            self.at = Site {
                block: index,
                index: at as i64,
            };
            self.check_stmt(&mut owned, stmt)?;
        }
        self.at = Site {
            block: index,
            index: block.stmts.len() as i64,
        };
        self.check_term(owned, &block.term)
    }

    fn check_stmt(&mut self, owned: &mut Owned, stmt: &Stmt) -> Result<(), String> {
        match stmt {
            Stmt::Let { var, rhs } => {
                self.check_rhs(owned, *var, rhs)?;
                match rhs {
                    Rhs::Call(call) => {
                        let never = self.never_returns(call);
                        self.cx.note_never(*var, never);
                    }
                    Rhs::MakeClosure(target, args) => {
                        self.cx.note_closure(*var, *target, args.len());
                    }
                    Rhs::Prim(Prim::EvNew(effect), _) => {
                        self.cx.note_evidence(*var, *effect);
                    }
                    Rhs::Extern {
                        ext,
                        args: _,
                        at: _,
                    } => {
                        self.cx.note_never(*var, ext.row().ret == OpRet::Never);
                    }
                    Rhs::Prim(_, _)
                    | Rhs::ConstString(_)
                    | Rhs::Con { ctor: _, args: _ }
                    | Rhs::Drop(_)
                    | Rhs::Box(_)
                    | Rhs::Unbox(_) => {}
                }
                self.define(owned, *var, self.at)
            }
            Stmt::Unpack {
                value,
                ctor,
                fields,
            } => {
                let repr = self.function.repr(*value);
                if repr != Repr::Obj {
                    return Err(format!(
                        "`{}` ({}) is unpacked, but only obj can be",
                        self.cx.name(*value),
                        repr.name()
                    ));
                }
                let (layout, layout_repr, constructor) = self.cx.ctor(*ctor, "an unpack")?;
                if layout_repr != Repr::Obj {
                    return Err(format!(
                        "`{}` (obj) is unpacked as `{}`, which is {}",
                        self.cx.name(*value),
                        layout.name,
                        layout_repr.name()
                    ));
                }
                if layout.constructors.len() != 1 {
                    return Err(format!(
                        "an unpack of `{}` names `{}`, which has {} constructors",
                        self.cx.name(*value),
                        layout.name,
                        layout.constructors.len()
                    ));
                }
                field_count(
                    format!(
                        "an unpack of `{}` as `{}` #{}",
                        self.cx.name(*value),
                        layout.name,
                        ctor.tag
                    ),
                    fields.len(),
                    constructor,
                )?;
                self.cx.field_reprs(layout, ctor.tag, constructor, fields)?;
                self.read(owned, *value, "unpacked")?;
                self.bind_fields(owned, *value, *ctor, fields, self.at)
            }
            Stmt::Dup(var) => {
                self.rc_allowed(*var, "duplicated")?;
                self.shareable(*var, "duplicated")?;
                self.read(owned, *var, "duplicated")?;
                if !self.function.repr(*var).is_rc() {
                    return Err(format!(
                        "`{}` is duplicated but is not reference counted",
                        self.cx.name(*var)
                    ));
                }
                *owned.entry(*var).or_insert(0) += 1;
                Ok(())
            }
            Stmt::Decref(var) => {
                self.rc_allowed(*var, "released")?;
                self.shareable(*var, "released")?;
                self.give_up(owned, *var, "released")
            }
            Stmt::Release {
                value,
                ctor,
                fields,
            } => {
                self.rc_allowed(*value, "released with its fields")?;
                self.release(owned, *value, *ctor, fields)
            }
        }
    }

    fn check_term(&mut self, mut owned: Owned, term: &Term) -> Result<(), String> {
        match term {
            Term::Return(atom) => {
                self.consume(&mut owned, *atom)?;
                if !self.cx.passes(*atom, self.function.ret)? {
                    return Err(format!(
                        "{} is returned from a function that returns {}",
                        self.cx.typed_atom_text(*atom),
                        self.function.ret.name()
                    ));
                }
                if !self.cx.mark_passes(*atom, self.function.ret_mark) {
                    return Err(format!(
                        "{} is returned from a function that returns {}",
                        self.cx.marked_atom_text(*atom),
                        self.function.ret_mark.name()
                    ));
                }
                self.nothing_owned(&owned)
            }
            Term::TailCall(call) => {
                if self.level < Level::Scopes {
                    return Err(format!("{} is formed before contract", tail_text(call)));
                }
                self.check_call(&mut owned, call, Receiver::Tail)?;
                self.nothing_owned(&owned)
            }
            Term::Jump { target, args } => {
                let params = &self.function.block(*target).params;
                for (&arg, &param) in args.iter().zip(params) {
                    self.check_passed(*target, arg, param)?;
                }
                for &arg in args {
                    self.consume(&mut owned, arg)?;
                }
                self.merge(*target, owned)
            }
            Term::Switch {
                scrutinee,
                layout,
                cases,
                default,
            } => {
                if cases.is_empty() && default.is_none() {
                    return Err("a switch has no targets".to_string());
                }
                self.cx.switch_cases(cases)?;
                self.cx.literal_default(cases, *default)?;
                if cases.iter().any(|case| !case.fields.is_empty()) {
                    self.cx.fields_allowed(*scrutinee)?;
                }
                self.cx
                    .switch_layout(*scrutinee, *layout, cases, *default)?;
                // `switch` は scrutinee を読むだけなので、どの行き先も scrutinee の所有を引き継ぐ
                match *scrutinee {
                    Atom::Var(var) => self.read(&owned, var, "switched on")?,
                    atom => self.consume(&mut owned, atom)?,
                }
                // 行き先は辺を1本しか持たないので (R3)、ここで入口の状態を決める。フィールドは行き先の先頭で定義する
                for case in cases {
                    let mut entry = owned.clone();
                    let site = Site {
                        block: case.target.0,
                        index: -1,
                    };
                    // 配置とタグは `switch_layout` が確かめた
                    if let (Atom::Var(value), Some(layout), CasePattern::Tag(tag)) =
                        (*scrutinee, *layout, case.pattern)
                    {
                        let ctor = Ctor { layout, tag };
                        self.bind_fields(&mut entry, value, ctor, &case.fields, site)?;
                    }
                    self.pending.insert(case.target.0, entry);
                }
                if let Some(default) = default {
                    self.pending.insert(default.0, owned);
                }
                Ok(())
            }
        }
    }

    /// 合流するブロックに入る `jump` は、渡す値を除いて、どれも同じ多重集合を所有する。その集合が入口の所有になる (R6)。
    fn merge(&mut self, target: BlockId, owned: Owned) -> Result<(), String> {
        let Some(first) = self.pending.get(&target.0) else {
            self.pending.insert(target.0, owned);
            return Ok(());
        };
        if *first != owned {
            return Err(format!(
                "a jump to b{} owns {} but an earlier jump to it owns {}",
                target.0,
                self.owned_names(&owned),
                self.owned_names(first)
            ));
        }
        Ok(())
    }

    /// `jump` の実引数は、行き先の引数と Repr が互換で (R7)、印も互換である (V2)。
    fn check_passed(&self, target: BlockId, arg: Atom, param: VarId) -> Result<(), String> {
        let expected = self.function.repr(param);
        if self.cx.passes(arg, expected)? {
            let mark = self.function.mark(param);
            if self.cx.mark_passes(arg, mark) {
                return Ok(());
            }
            return Err(format!(
                "a jump to b{} passes {} to `{}` ({})",
                target.0,
                self.cx.marked_atom_text(arg),
                self.cx.name(param),
                mark.name()
            ));
        }
        Err(match arg {
            Atom::Var(var) => format!(
                "a jump to b{} passes `{}` ({}) to `{}` ({})",
                target.0,
                self.cx.name(var),
                self.function.repr(var).name(),
                self.cx.name(param),
                expected.name()
            ),
            _ => format!(
                "a jump to b{} passes {} to `{}` ({})",
                target.0,
                self.cx.atom_text(arg),
                self.cx.name(param),
                expected.name()
            ),
        })
    }

    /// 変数を多くとも1回定義し (R5)、RC の対象なら所有を1つ持つ。
    fn define(&mut self, owned: &mut Owned, var: VarId, site: Site) -> Result<(), String> {
        let slot = &mut self.defs[var.0 as usize];
        if slot.is_some() {
            return Err(format!("`{}` is defined twice", self.cx.name(var)));
        }
        *slot = Some(site);
        if self.level == Level::Ownership && self.function.repr(var).is_rc() {
            owned.insert(var, 1);
        }
        Ok(())
    }

    /// 定義の位置が今の位置を支配する (R6)。
    fn visible(&self, var: VarId) -> Result<(), String> {
        let dominated = self.defs[var.0 as usize].is_some_and(|site| {
            if site.block == self.at.block {
                site.index < self.at.index
            } else {
                self.dominators.dominates(site.block, self.at.block)
            }
        });
        if !dominated {
            return Err(format!("`{}` is used outside its scope", self.cx.name(var)));
        }
        Ok(())
    }

    /// RC の対象の変数の、所有している参照の数。0 なら、`what` (使う、捨てる) ことはできない。持ち主が所有を持つ
    /// フィールドは、借りているだけなので所有を渡せない。
    fn count<'s>(
        &self,
        owned: &'s mut Owned,
        var: VarId,
        what: &str,
    ) -> Result<&'s mut u32, String> {
        self.visible(var)?;
        if !self.function.repr(var).is_rc() {
            return Err(format!(
                "`{}` is {what} but is not reference counted",
                self.cx.name(var)
            ));
        }
        if !owned.contains_key(&var) {
            return Err(match self.owners[var.0 as usize] {
                Some(owner) if owned.contains_key(&owner) => format!(
                    "`{}` is {what} but is only borrowed from `{}`",
                    self.cx.name(var),
                    self.cx.name(owner)
                ),
                _ => format!("`{}` is {what} after it was moved", self.cx.name(var)),
            });
        }
        Ok(owned.get_mut(&var).expect("checked above"))
    }

    /// 値を読む (`switch` の scrutinee、`unpack` と `unbox` の値、`dup`)。所有の検査の段では、RC の対象の変数は
    /// 有効でなければならない。つまり、自分か持ち主が所有を持つ。所有を持つ経路は実際の参照を持つので物体は生きていて、
    /// data は書き換わらないので、そこからたどれる物体もすべて生きている (docs/spec/lir.md の「verifier」)。
    fn read(&self, owned: &Owned, var: VarId, what: &str) -> Result<(), String> {
        self.visible(var)?;
        if self.level != Level::Ownership
            || !self.function.repr(var).is_rc()
            || owned.contains_key(&var)
        {
            return Ok(());
        }
        match self.owners[var.0 as usize] {
            Some(owner) if owned.contains_key(&owner) => Ok(()),
            Some(owner) => Err(format!(
                "`{}` is {what} after its owner `{}` was given up",
                self.cx.name(var),
                self.cx.name(owner)
            )),
            None => Err(format!(
                "`{}` is {what} after it was moved",
                self.cx.name(var)
            )),
        }
    }

    /// `case` か `unpack` のフィールドを定義する。RC の対象のフィールドは所有を持たずに始まる (借りる)。持ち主と
    /// 出どころは定義のときに1回だけ決める。
    fn bind_fields(
        &mut self,
        owned: &mut Owned,
        value: VarId,
        ctor: Ctor,
        fields: &[VarId],
        site: Site,
    ) -> Result<(), String> {
        let owner = if owned.contains_key(&value) {
            Some(value)
        } else {
            self.owners[value.0 as usize]
        };
        for (slot, &field) in fields.iter().enumerate() {
            self.define(owned, field, site)?;
            owned.remove(&field);
            self.owners[field.0 as usize] = owner;
            self.origins[field.0 as usize] = Some(Origin { value, ctor, slot });
        }
        Ok(())
    }

    /// 参照を1つ手放す。所有しなくなった変数は表から除き、写す状態を小さく保つ。
    fn give_up(&self, owned: &mut Owned, var: VarId, what: &str) -> Result<(), String> {
        let count = self.count(owned, var, what)?;
        *count -= 1;
        if *count == 0 {
            owned.remove(&var);
        }
        Ok(())
    }

    /// `release x L #t(p1, .., pn)`。x の参照を1つ手放し、名前を書いた変数が参照を1つずつ受け取る。名前を書いた変数は、
    /// x を同じ配置とタグで分解したときの、同じ位置のフィールドである (docs/spec/lir.md)。
    fn release(
        &self,
        owned: &mut Owned,
        value: VarId,
        ctor: Ctor,
        fields: &[Option<VarId>],
    ) -> Result<(), String> {
        if fields.iter().all(Option::is_none) {
            return Err(format!(
                "a release of `{}` keeps no field",
                self.cx.name(value)
            ));
        }
        // 数を先に配置で確かめるので、出どころは数を比べない
        let (layout, _, constructor) = self.cx.ctor(ctor, "a release")?;
        field_count(
            format!(
                "a release of `{}` as `{}` #{}",
                self.cx.name(value),
                layout.name,
                ctor.tag
            ),
            fields.len(),
            constructor,
        )?;
        for (slot, field) in fields.iter().enumerate() {
            let Some(field) = *field else { continue };
            self.visible(field)?;
            if !self.function.repr(field).is_rc() {
                return Err(format!(
                    "`{}` is kept but is not reference counted",
                    self.cx.name(field)
                ));
            }
            if self.origins[field.0 as usize] != Some(Origin { value, ctor, slot }) {
                return Err(format!(
                    "`{}` is not field {slot} of `{}` as `{}` #{}",
                    self.cx.name(field),
                    self.cx.name(value),
                    layout.name,
                    ctor.tag
                ));
            }
        }
        self.give_up(owned, value, "released")?;
        for &field in fields.iter().flatten() {
            *owned.entry(field).or_insert(0) += 1;
        }
        Ok(())
    }

    /// `dup` と `decref` の対象は `unr` である (V1)。`lin` と `poly` の値は共有してはならず、ちょうど1回消費する
    /// (docs/spec/lir.md の「Kind の印の検査」)。
    fn shareable(&self, var: VarId, what: &str) -> Result<(), String> {
        let mark = self.function.mark(var);
        if mark == Mark::Unr {
            return Ok(());
        }
        Err(format!(
            "`{}` ({}) is {what}, but only unr can be",
            self.cx.name(var),
            mark.name()
        ))
    }

    /// RC の命令は Perceus だけが入れる。
    fn rc_allowed(&self, var: VarId, what: &str) -> Result<(), String> {
        if self.level != Level::Ownership {
            return Err(format!("`{}` is {what} before Perceus", self.cx.name(var)));
        }
        Ok(())
    }

    /// 値を使う。所有の検査の段では、RC の対象なら所有を1つ渡す。
    fn consume(&self, owned: &mut Owned, atom: Atom) -> Result<(), String> {
        let var = match atom {
            Atom::Var(var) => var,
            Atom::Fn(target) if target.0 as usize >= self.program.functions.len() => {
                return Err(format!(
                    "a function value refers to the unknown function #{}",
                    target.0
                ));
            }
            Atom::Fn(_) | Atom::Int(_) | Atom::Unit | Atom::Tag(_) => return Ok(()),
        };
        if self.level != Level::Ownership || !self.function.repr(var).is_rc() {
            return self.visible(var);
        }
        self.give_up(owned, var, "used")
    }

    fn consume_all(
        &self,
        owned: &mut Owned,
        for_each: impl FnOnce(&mut dyn FnMut(Atom)),
    ) -> Result<(), String> {
        let mut result = Ok(());
        for_each(&mut |atom| {
            if result.is_ok() {
                result = self.consume(owned, atom);
            }
        });
        result
    }

    fn check_rhs(&self, owned: &mut Owned, var: VarId, rhs: &Rhs) -> Result<(), String> {
        match rhs {
            Rhs::Call(call) => {
                return self.check_call(owned, call, Receiver::Bound(var));
            }
            Rhs::MakeClosure(index, args) => {
                let target = self.function_at(*index)?;
                if args.is_empty() {
                    return Err(format!(
                        "a closure of `{0}` has no arguments; use `&{0}`",
                        target.name
                    ));
                }
                let params = target.params().len();
                if args.len() >= params {
                    return Err(format!(
                        "a closure of `{}` has {} arguments, but it must have fewer than {params}",
                        target.name,
                        args.len()
                    ));
                }
                if self.cx.boundaries {
                    self.cx.function_value(*index)?;
                    for (index, (&arg, &param)) in args.iter().zip(target.params()).enumerate() {
                        let expected = target.repr(param);
                        if !self.cx.passes(arg, expected)? {
                            return Err(format!(
                                "argument {index} of a closure of `{}` is {}, but the function takes {}",
                                target.name,
                                self.cx.typed_atom_text(arg),
                                expected.name()
                            ));
                        }
                        let mark = target.mark(param);
                        if !self.cx.mark_passes(arg, mark) {
                            return Err(format!(
                                "argument {index} of a closure of `{}` is {}, but the function takes {}",
                                target.name,
                                self.cx.marked_atom_text(arg),
                                mark.name()
                            ));
                        }
                    }
                }
                self.cx.check_built(
                    var,
                    args,
                    || format!("a closure of `{}`", target.name),
                    "captures",
                )?;
            }
            Rhs::Extern { ext, args, at: _ } => self.cx.check_extern(var, *ext, args)?,
            Rhs::Prim(prim, args) => {
                self.cx.check_prim(var, prim, args)?;
                // 借用の引数は `unbox` のオペランドと同じく読む使いで、所有の数を変えない
                // (docs/spec/lir.md の「基本操作」)
                let lent = format!("lent to `prim {}`", prim_text(&self.program.effects, prim));
                for (&arg, &borrowed) in args.iter().zip(prim.row().borrowed) {
                    match arg {
                        Atom::Var(arg) if borrowed => self.read(owned, arg, &lent)?,
                        // 変数でない値は所有を持たないので、消費と同じ検査 (関数の番号) だけを通す
                        _ if borrowed => self.consume(owned, arg)?,
                        _ => {}
                    }
                }
            }
            Rhs::ConstString(index) => {
                if *index as usize >= self.program.strings.len() {
                    return Err(format!(
                        "a constant refers to string {index}, which does not exist"
                    ));
                }
            }
            Rhs::Con { ctor, args } => self.cx.check_con(var, *ctor, args)?,
            Rhs::Drop(atom) => self.cx.fn_atom(*atom)?,
            // `unit` の値、`()`、`#N`、`&f`、参照は命令なしで `tobj` に収まるので、`box` しない。1つの値の書き方を1つに保つ
            // (docs/spec/lir.md の「値の表現」)
            Rhs::Box(atom) => {
                if self.level < Level::Scopes {
                    return Err(format!(
                        "{} is boxed before the boxing pass",
                        self.cx.atom_text(*atom)
                    ));
                }
                let boxable = match *atom {
                    Atom::Var(operand) => self.function.repr(operand).needs_box(),
                    Atom::Int(_) => true,
                    Atom::Unit | Atom::Tag(_) | Atom::Fn(_) => false,
                };
                if !boxable {
                    return Err(format!(
                        "{} is boxed, but only {} values and Int constants can be",
                        self.cx.typed_atom_text(*atom),
                        boxed_scalars("and")
                    ));
                }
                let repr = self.function.repr(var);
                if repr != Repr::TObj {
                    return Err(format!(
                        "`{}` ({}) is bound to a box, which is tobj",
                        self.cx.name(var),
                        repr.name()
                    ));
                }
            }
            // `obj` はつねにヒープの物体を指すので、スカラーを入れた値にならない
            Rhs::Unbox(atom) => {
                if self.level < Level::Scopes {
                    return Err(format!(
                        "{} is unboxed before the boxing pass",
                        self.cx.atom_text(*atom)
                    ));
                }
                let operand = match *atom {
                    Atom::Var(operand) if self.function.repr(operand) == Repr::TObj => operand,
                    _ => {
                        return Err(format!(
                            "{} is unboxed, but only tobj can be",
                            self.cx.typed_atom_text(*atom)
                        ));
                    }
                };
                let repr = self.function.repr(var);
                if !repr.needs_box() {
                    return Err(format!(
                        "`{}` ({}) is bound to an unbox, which gives {}",
                        self.cx.name(var),
                        repr.name(),
                        boxed_scalars("or")
                    ));
                }
                return self.read(owned, operand, "unboxed");
            }
        }
        self.consume_all(owned, |f| rhs.for_each_consumed(f))
    }

    /// 呼び出しは、形を確かめた後に、オペランドの Repr を左から比べ、結果を `receiver` と比べてから、範囲と所有を
    /// 確かめる (docs/spec/lir.md の「verifier」)。
    fn check_call(&self, owned: &mut Owned, call: &Call, receiver: Receiver) -> Result<(), String> {
        if matches!(receiver, Receiver::Tail) && call.prompt().is_some() {
            return Err(format!(
                "{} has a prompt marker, but a tail call leaves no frame to land on",
                self.call_text(call, true)
            ));
        }
        let result = match call {
            Call::Direct(target, args) => {
                if let Some(clause) = self.cx.clause_of(*target) {
                    return Err(format!(
                        "`{}` is a clause of `{clause}`, but it is called directly",
                        self.program.functions[target.0 as usize].name
                    ));
                }
                let target = self.function_at(*target)?;
                let params = target.params().len();
                if args.len() != params {
                    return Err(format!(
                        "a direct call to `{}` passes {} arguments, but it takes {params}",
                        target.name,
                        args.len()
                    ));
                }
                Some((target.ret, target.ret_mark))
            }
            Call::Clause {
                effect,
                op,
                callee: _,
                args,
            } => {
                let info = self.cx.effect(*effect)?;
                let Some(operation) = info.operations.get(*op as usize) else {
                    return Err(format!(
                        "a clause names operation {op} of `{}`, which has {} operations",
                        info.name,
                        info.operations.len()
                    ));
                };
                let takes = 1 + operation.params.len();
                if args.len() != takes {
                    return Err(format!(
                        "{} is given {} arguments, but it takes {takes} (the evidence and {} arguments of the operation)",
                        self.call_text(call, false),
                        args.len(),
                        operation.params.len()
                    ));
                }
                // `never` の操作の節は値を返さないので、結果は位置でない
                match operation.ret {
                    OpRet::Repr(repr) => Some((repr, operation.ret_mark)),
                    OpRet::Never => None,
                }
            }
            // 関数の値は型変数の位置なので、結果の印はいつも `poly` で、どの受け手とも合う
            Call::Apply { .. } => Some((Repr::TObj, Mark::Poly)),
        };
        if self.cx.boundaries {
            self.check_operands(call)?;
            if let Some(result) = result {
                self.check_result(call, result, receiver)?;
            }
        }
        self.consume_all(owned, |f| call.for_each_atom(f))?;
        match call.prompt() {
            Some(marker) => self.check_prompt(owned, call, marker),
            None => Ok(()),
        }
    }

    /// 着地の印 `prompt v` の検査。`v` は呼び出しの後も生きている `obj` の変数でなければならない。所有の段では、
    /// 呼び出しの直後にも所有していなければならない (docs/superpowers/specs/2026-10-17-s13c-unwinding-design.md の「着地の印」)。
    fn check_prompt(&self, owned: &Owned, call: &Call, marker: VarId) -> Result<(), String> {
        let what = self.call_text(call, false);
        self.visible(marker)?;
        if self.function.repr(marker) != Repr::Obj {
            return Err(format!(
                "`{}` is the prompt marker of {what}, but it is {}, not obj",
                self.cx.name(marker),
                self.function.repr(marker).name()
            ));
        }
        if !self.used_after(marker) {
            return Err(format!(
                "`{}` is the prompt marker of {what}, but it is not used after the call",
                self.cx.name(marker)
            ));
        }
        if self.level == Level::Ownership && !owned.contains_key(&marker) {
            return Err(format!(
                "`{}` is the prompt marker of {what}, but it is not owned after the call",
                self.cx.name(marker)
            ));
        }
        Ok(())
    }

    /// 現在の位置より後ろの文か終端が `var` を使うか。辺は前向きなので、後ろのブロックと同じブロックの後ろの文を
    /// 見れば足りる。到達できないブロックの使いも数える近似で、生存解析は使わない。
    fn used_after(&self, var: VarId) -> bool {
        let mut found = false;
        let mut check = |atom: Atom| found |= atom == Atom::Var(var);
        let blocks = &self.function.blocks;
        let current = &blocks[self.at.block as usize];
        let skip = (self.at.index + 1) as usize;
        current
            .stmts
            .iter()
            .skip(skip)
            .for_each(|stmt| stmt.for_each_atom(&mut check));
        current.term.for_each_atom(&mut check);
        for block in &blocks[self.at.block as usize + 1..] {
            block
                .stmts
                .iter()
                .for_each(|stmt| stmt.for_each_atom(&mut check));
            block.term.for_each_atom(&mut check);
        }
        found
    }

    /// 呼び出しのオペランドを左から比べる (R7)。直接の呼び出しは、呼ばれる関数の引数と比べる。関数値の適用の
    /// オペランドは一様な位置なので、`tobj` と比べる。呼び出しの形は確かめてある。印も同じ所で比べる (V2)。関数値の
    /// 適用の位置の印はいつも `poly` なので、印を比べない。
    fn check_operands(&self, call: &Call) -> Result<(), String> {
        // 誤りの文で呼び出しを指す句 (`an apply`) は呼び出しの形で決まるので、比べる処理を形ごとに作る
        let operands = |taker: &'static str| {
            move |role: &dyn Fn() -> String, atom: Atom| -> Result<(), String> {
                if self.cx.passes(atom, Repr::TObj)? {
                    return Ok(());
                }
                Err(format!(
                    "{} of {} is {}, but {taker} takes tobj",
                    role(),
                    self.call_text(call, false),
                    self.cx.typed_atom_text(atom)
                ))
            }
        };
        match call {
            Call::Direct(target, args) => {
                let target = &self.program.functions[target.0 as usize];
                for (index, (&arg, &param)) in args.iter().zip(target.params()).enumerate() {
                    let expected = target.repr(param);
                    if !self.cx.passes(arg, expected)? {
                        return Err(format!(
                            "argument {index} of `{}` is {}, but the function takes {}",
                            target.name,
                            self.cx.typed_atom_text(arg),
                            expected.name()
                        ));
                    }
                    let mark = target.mark(param);
                    if !self.cx.mark_passes(arg, mark) {
                        return Err(format!(
                            "argument {index} of `{}` is {}, but the function takes {}",
                            target.name,
                            self.cx.marked_atom_text(arg),
                            mark.name()
                        ));
                    }
                }
            }
            Call::Apply {
                callee,
                args,
                yields: _,
                prompt: _,
            } => {
                let uniform = operands("an apply");
                uniform(&|| "the callee".to_string(), *callee)?;
                for (index, &arg) in args.iter().enumerate() {
                    uniform(&|| format!("argument {index}"), arg)?;
                }
            }
            // 節の呼び出しは操作の ABI で呼ぶので、先頭の evidence は `obj`、残りは操作の表の Repr と比べる
            Call::Clause {
                effect,
                op,
                callee,
                args,
            } => {
                let operation = operation(&self.program.effects, *effect, *op);
                let what = self.call_text(call, false);
                if !self.cx.passes(*callee, Repr::TObj)? {
                    return Err(format!(
                        "the callee of {what} is {}, but a clause takes tobj",
                        self.cx.typed_atom_text(*callee)
                    ));
                }
                // 呼ばれる入口のクロージャは `ev_clause` が `dup` して返すので `unr` である
                // (docs/spec/lir.md の「Kind の印の検査」)
                if !self.cx.mark_passes(*callee, Mark::Unr) {
                    return Err(format!(
                        "the callee of {what} is {}, but a clause takes unr",
                        self.cx.marked_atom_text(*callee)
                    ));
                }
                let expected = std::iter::once((Repr::Obj, Mark::Unr)).chain(
                    operation
                        .params
                        .iter()
                        .copied()
                        .zip(operation.param_marks.iter().copied()),
                );
                for (index, (&arg, (repr, mark))) in args.iter().zip(expected).enumerate() {
                    let taker = if index == 0 {
                        "a clause"
                    } else {
                        "the operation"
                    };
                    if !self.cx.passes(arg, repr)? {
                        return Err(format!(
                            "argument {index} of {what} is {}, but {taker} takes {}",
                            self.cx.typed_atom_text(arg),
                            repr.name()
                        ));
                    }
                    if !self.cx.mark_passes(arg, mark) {
                        return Err(format!(
                            "argument {index} of {what} is {}, but {taker} takes {}",
                            self.cx.marked_atom_text(arg),
                            mark.name()
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// 結果を受けない呼び出し。`never` の操作の `clause` は戻らない。
    fn never_returns(&self, call: &Call) -> bool {
        match call {
            Call::Clause { effect, op, .. } => {
                operation(&self.program.effects, *effect, *op).ret == OpRet::Never
            }
            Call::Direct(_, _) | Call::Apply { .. } => false,
        }
    }

    /// 呼び出しの結果 (直接の呼び出しなら呼ばれる関数の `ret`、節の呼び出しなら操作の `ret`、`apply` なら `tobj`) は、
    /// 束縛する変数か、`tail` なら呼び出し元の `ret` と互換である (R7)。結果の印も同じく互換である (V2)。
    fn check_result(
        &self,
        call: &Call,
        (result, result_mark): (Repr, Mark),
        receiver: Receiver,
    ) -> Result<(), String> {
        match receiver {
            Receiver::Bound(var) => {
                let repr = self.function.repr(var);
                if !repr.compatible(result) {
                    return Err(format!(
                        "`{}` ({}) is bound to {}, which returns {}",
                        self.cx.name(var),
                        repr.name(),
                        self.call_text(call, false),
                        result.name()
                    ));
                }
                self.cx
                    .bound_mark(var, result_mark, || self.call_text(call, false))
            }
            Receiver::Tail => {
                // 呼び出し元を "this function" と書き、呼ばれる側を指す "the function" と分ける
                let ret = self.function.ret;
                if !result.compatible(ret) {
                    return Err(format!(
                        "{} returns {}, but this function returns {}",
                        self.call_text(call, true),
                        result.name(),
                        ret.name()
                    ));
                }
                let ret_mark = self.function.ret_mark;
                if result_mark.compatible(ret_mark) {
                    return Ok(());
                }
                Err(format!(
                    "{} returns {}, but this function returns {}",
                    self.call_text(call, true),
                    result_mark.name(),
                    ret_mark.name()
                ))
            }
        }
    }

    /// 境界の検査の文言で呼び出しを指す言い方。`let` では `` `g` ``、``a clause of `Ask.ask` `` の形、`tail` では
    /// ``a tail call to `g` ``、``a tail clause of `Ask.ask` `` の形にする。呼び出しの形は確かめてある。
    fn call_text(&self, call: &Call, tail: bool) -> String {
        let (article, noun) = match call {
            Call::Direct(target, _) => {
                let name = &self.program.functions[target.0 as usize].name;
                return if tail {
                    format!("a tail call to `{name}`")
                } else {
                    format!("`{name}`")
                };
            }
            Call::Apply { .. } => ("an", "apply".to_string()),
            Call::Clause {
                effect,
                op,
                callee: _,
                args: _,
            } => (
                "a",
                format!(
                    "clause of `{}`",
                    operation_text(&self.program.effects, *effect, *op)
                ),
            ),
        };
        if tail {
            format!("a tail {noun}")
        } else {
            format!("{article} {noun}")
        }
    }

    fn function_at(&self, target: FnIdx) -> Result<&'a LirFn, String> {
        self.program
            .functions
            .get(target.0 as usize)
            .ok_or_else(|| format!("a call refers to the unknown function #{}", target.0))
    }

    fn nothing_owned(&self, owned: &Owned) -> Result<(), String> {
        match owned.keys().next() {
            Some(&var) => Err(format!(
                "`{}` is still owned at the end of the function",
                self.cx.name(var)
            )),
            None => Ok(()),
        }
    }

    fn owned_names(&self, owned: &Owned) -> String {
        self.cx.names(&flatten(owned))
    }
}

/// 入口の段が拒む `tail` の言い方。
fn tail_text(call: &Call) -> &'static str {
    match call {
        Call::Direct(_, _) => "a tail call",
        Call::Apply { .. } => "a tail apply",
        Call::Clause { .. } => "a tail clause",
    }
}

/// 所有の多重集合を、参照の数だけ変数を並べた昇順の列にする。
fn flatten(owned: &Owned) -> Vec<VarId> {
    owned
        .iter()
        .flat_map(|(&var, &count)| std::iter::repeat_n(var, count as usize))
        .collect()
}
