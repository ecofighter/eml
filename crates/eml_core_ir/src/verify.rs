//! Core IR の不変条件の検査 (docs/spec/core-ir.md の「verifier」)。ブロックの列の形 (R1〜R4)、変数の定義と支配 (R5、R6)、
//! `unpack` の Repr、`jump` と `return` の Repr の互換、extern の引数と結果の Repr (R7)、データの配置 (R8) と、
//! 引き継いだ検査 (直接呼び出しと extern の引数の数、case の種類) を確かめる。境界 (呼び出しの引数と結果、関数の値の
//! 対象が一様であること、`tobj` のフィールド) は、box の挿入の後の LIR の段だけで比べる。
//! 段は2つある。translate の直後は、EP が作る基本操作、節の呼び出し、`clause` の印がまだないことと、エフェクトの
//! 命令の形 (`mask` の順、`handle` の節の数、再開できるかどうか) を確かめる (`verify_translated`)。EP の後は、
//! `handle`、`perform`、`resume`、`mask` が残っていないことと、基本操作と節の呼び出しの形を確かめる
//! (`verify_evidence`)。段の分け方は docs/spec/core-ir.md の「verifier」による。
//! 印はどちらの段でも、`jump` と `return` だけで比べる (V2)。`con` と `closure` が束縛する変数の印が、引数の印の
//! 結び以上であることも確かめる (docs/spec/lir.md の「Kind の印の検査」)。
//! LIR の verifier と共有する部品は `eml_lir::check` にある。
//!
//! R8 は、`con`、タグの `switch`、`unpack` を、その命令が指す配置と比べる。配置を持つ `switch` はタグの case を
//! 持つものだけで、リテラルの `switch` では scrutinee の Repr を比べる。値がどの配置で作られたかは追わない。それを
//! 保証するのは translate の型なので、配置の違う値を読む IR もこの検査を通りうる (docs/spec/lir.md の
//! 「データの配置」)。
//!
//! 辺の検査、支配木、本体の検査は、それぞれブロックを番号の順に1回たどるだけで、反復も生存解析も使わない。辺は
//! 前向きなので、ブロックに着いたときには入る辺がすべて出そろっている。支配木は辺1本につき深さの対数の手間で
//! 育つ (docs/spec/core-ir.md の「verifier」)。

use std::collections::HashMap;

use crate::liveness;
use crate::tail::tail_position_call;
use crate::{Block, Call, CoreFn, Program, Rhs, Stmt, Term};
use eml_lir::check::{
    BlockShape, Context, Dominators, FnSummary, Tables, TermShape, field_count, shape,
};
use eml_lir::syntax::{effect_text, operation_text, prim_text};
use eml_lir::{
    Atom, BlockId, CasePattern, EffectInfo, FnIdx, Layout, MaskItem, OpRet, Prim, Repr, VarId,
    VerifyError, operation,
};

/// EP の後の IR を確かめる。変換の段と同じ形と範囲を確かめ、`handle`、`perform`、`resume`、`mask` が残っていない
/// ことと、基本操作と節の呼び出しの形を確かめる (docs/spec/core-ir.md の「verifier」)。
pub fn verify_evidence(program: &Program) -> Result<(), VerifyError> {
    verify_at(program, Level::Evidence)
}

/// translate の直後の IR を確かめる。形と範囲を確かめ、EP が作るもの (基本操作、節の呼び出し、`clause` の印) が
/// まだないことと、エフェクトの命令の形を確かめる。
pub fn verify_translated(program: &Program) -> Result<(), VerifyError> {
    verify_at(program, Level::Translated)
}

/// 検査の段。translate の直後の IR には、EP が作るものがまだない。EP の後の IR には、エフェクトの命令がもうない。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    Translated,
    Evidence,
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
    // `clause` の印の関数の ABI と役割は、参照ごとでなく関数ごとに1回だけ確かめる
    for function in &program.functions {
        clause_function(program, function, level)
            .and_then(|()| eml_lir::check::role(function.role, function.params().len()))
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

fn param_reprs(function: &CoreFn) -> impl ExactSizeIterator<Item = Repr> + '_ {
    function.params().iter().map(|&param| function.repr(param))
}

/// EP より前に `clause` の印はない。印の関数の ABI は `eml_lir::check::clause_function` が確かめる。
fn clause_function(program: &Program, function: &CoreFn, level: Level) -> Result<(), String> {
    if let Some((effect, op)) = function.clause
        && level == Level::Translated
    {
        return Err(format!(
            "a clause of `{}` is defined before the evidence pass",
            operation_text(&program.effects, effect, op)
        ));
    }
    eml_lir::check::clause_function(
        &program.effects,
        &function.name,
        function.clause,
        function
            .params()
            .iter()
            .map(|&param| (param, &function.vars[param.0 as usize])),
        function.ret,
        function.ret_mark,
        false,
    )
}

/// 辺の規則の検査 (`shape`) に渡す、ブロックの形。
fn block_shapes(function: &CoreFn) -> Vec<BlockShape> {
    function
        .blocks
        .iter()
        .map(|block| BlockShape {
            params: block.params.len(),
            term: match &block.term {
                Term::Return(_) => TermShape::Exit,
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

/// 定義と使用の位置。`index` はブロックの中の文の番号で、ブロックの引数と case のフィールドは -1、終端は文の数である。
#[derive(Clone, Copy)]
struct Site {
    block: u32,
    index: i64,
}

struct Checker<'a> {
    program: &'a Program,
    function: &'a CoreFn,
    level: Level,
    dominators: Dominators,
    /// 命令の型に関わらない検査の状態。
    cx: Context<'a>,
    defs: Vec<Option<Site>>,
    at: Site,
    /// 着地の印の検査が使う入口の生存集合。印がなければ求めない。
    live_in: std::cell::OnceCell<Vec<Vec<VarId>>>,
}

impl<'a> Checker<'a> {
    fn new(
        program: &'a Program,
        tables: Tables<'a>,
        function: &'a CoreFn,
        level: Level,
        dominators: Dominators,
    ) -> Self {
        Checker {
            program,
            function,
            level,
            dominators,
            cx: Context::new(tables, &function.vars, false),
            defs: vec![None; function.vars.len()],
            at: Site {
                block: 0,
                index: -1,
            },
            live_in: std::cell::OnceCell::new(),
        }
    }

    fn run(mut self) -> Result<(), String> {
        let function = self.function;
        self.clause_values()?;
        for (index, block) in function.blocks.iter().enumerate() {
            let index = index as u32;
            self.at = Site {
                block: index,
                index: -1,
            };
            for &param in &block.params {
                self.define(param, self.at)?;
            }
            self.check_block(index, block)?;
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
                Atom::Int(_) | Atom::Float(_) | Atom::Unit | Atom::Tag(_) => return,
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

    fn check_block(&mut self, index: u32, block: &Block) -> Result<(), String> {
        for (at, stmt) in block.stmts.iter().enumerate() {
            self.at = Site {
                block: index,
                index: at as i64,
            };
            self.check_stmt(stmt)?;
        }
        self.at = Site {
            block: index,
            index: block.stmts.len() as i64,
        };
        self.check_term(&block.term)
    }

    fn check_stmt(&mut self, stmt: &Stmt) -> Result<(), String> {
        match stmt {
            Stmt::Let { var, rhs } => {
                self.check_rhs(*var, rhs)?;
                match rhs {
                    Rhs::Call { call, mask: _ } => {
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
                    | Rhs::Drop(_) => {}
                }
                self.define(*var, self.at)
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
                self.visible(*value)?;
                self.define_all(fields, self.at)
            }
        }
    }

    fn check_term(&mut self, term: &Term) -> Result<(), String> {
        match term {
            Term::Return(atom) => {
                self.use_atom(*atom)?;
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
                Ok(())
            }
            Term::Jump { target, args } => {
                let params = &self.function.block(*target).params;
                for (&arg, &param) in args.iter().zip(params) {
                    self.check_passed(*target, arg, param)?;
                }
                for &arg in args {
                    self.use_atom(arg)?;
                }
                Ok(())
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
                self.use_atom(*scrutinee)?;
                // 行き先は辺を1本しか持たないので (R3)、フィールドはここで行き先の先頭に定義する
                for case in cases {
                    let site = Site {
                        block: case.target.0,
                        index: -1,
                    };
                    // 配置とタグは `switch_layout` が確かめた
                    if let (Atom::Var(_), Some(_), CasePattern::Tag(_)) =
                        (*scrutinee, *layout, case.pattern)
                    {
                        self.define_all(&case.fields, site)?;
                    }
                }
                Ok(())
            }
        }
    }

    /// `jump` の実引数は、行き先の引数と Repr が互換で (R7)、印も互換である (V2)。Core IR の段では、印を比べるのは
    /// `jump` と `return` だけである (docs/spec/lir.md の「Kind の印の検査」)。
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

    /// 変数を多くとも1回定義する (R5)。
    fn define(&mut self, var: VarId, site: Site) -> Result<(), String> {
        let slot = &mut self.defs[var.0 as usize];
        if slot.is_some() {
            return Err(format!("`{}` is defined twice", self.cx.name(var)));
        }
        *slot = Some(site);
        Ok(())
    }

    fn define_all(&mut self, vars: &[VarId], site: Site) -> Result<(), String> {
        for &var in vars {
            self.define(var, site)?;
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

    /// 値を使う。変数は見えていて、関数の値は表にある関数を指す。
    fn use_atom(&self, atom: Atom) -> Result<(), String> {
        match atom {
            Atom::Var(var) => self.visible(var),
            Atom::Fn(target) if target.0 as usize >= self.program.functions.len() => Err(format!(
                "a function value refers to the unknown function #{}",
                target.0
            )),
            Atom::Fn(_) | Atom::Int(_) | Atom::Float(_) | Atom::Unit | Atom::Tag(_) => Ok(()),
        }
    }

    fn use_all(&self, for_each: impl FnOnce(&mut dyn FnMut(Atom))) -> Result<(), String> {
        let mut result = Ok(());
        for_each(&mut |atom| {
            if result.is_ok() {
                result = self.use_atom(atom);
            }
        });
        result
    }

    fn check_rhs(&self, var: VarId, rhs: &Rhs) -> Result<(), String> {
        match rhs {
            Rhs::Call { call, mask } => {
                self.check_mask(call, mask)?;
                self.check_call(call)?;
                return self.check_prompt(call);
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
                self.cx.check_built(
                    var,
                    args,
                    || format!("a closure of `{}`", target.name),
                    "captures",
                )?;
            }
            Rhs::Extern { ext, args, at: _ } => self.cx.check_extern(var, *ext, args)?,
            Rhs::Prim(prim, args) => {
                // 根の基本操作と、`var` の節の空の配列は translate が作る (docs/spec/lir.md の「基本操作」)
                if self.level == Level::Translated
                    && !matches!(
                        prim,
                        Prim::RootGet(_) | Prim::RootSet(_) | Prim::EmptyMutArray
                    )
                {
                    return Err(format!(
                        "`prim {}` is used before the evidence pass",
                        prim_text(&self.program.effects, prim)
                    ));
                }
                self.cx.check_prim(var, prim, args)?
            }
            Rhs::ConstString(index) => {
                if *index as usize >= self.program.strings.len() {
                    return Err(format!(
                        "a constant refers to string {index}, which does not exist"
                    ));
                }
            }
            Rhs::Con { ctor, args } => self.cx.check_con(var, *ctor, args)?,
            Rhs::Drop(_) => {}
        }
        self.use_all(|f| rhs.for_each_atom(f))
    }

    /// 呼び出しは、形を確かめた後に範囲を確かめる (docs/spec/lir.md の「verifier」)。
    fn check_call(&self, call: &Call) -> Result<(), String> {
        // EP は `handle`、`perform`、`resume` を基本操作と節の呼び出しに置き換える
        // (docs/spec/core-ir.md の「verifier」)
        if self.level != Level::Translated {
            let remains = match call {
                Call::Handle { .. } => Some("a handler"),
                Call::Perform { .. } => Some("a perform"),
                Call::Resume { .. } => Some("a resume"),
                Call::Direct(_, _) | Call::Apply { .. } | Call::Clause { .. } => None,
            };
            if let Some(remains) = remains {
                return Err(format!("{remains} remains after the evidence pass"));
            }
        }
        match call {
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
            }
            Call::Handle {
                effect,
                init: _,
                body: _,
                clauses,
                ret,
            } => {
                let info = self.cx.effect(*effect)?;
                if clauses.len() != info.operations.len() {
                    return Err(format!(
                        "a handler of `{}` has clauses for {} operations, but the effect has {}",
                        info.name,
                        clauses.len(),
                        info.operations.len()
                    ));
                }
                // 節の引数の数は範囲の外の `closure` からも数えられるので、先に節が見えることを確かめる
                for &atom in clauses.iter().chain([ret]) {
                    if let Atom::Var(var) = atom {
                        self.visible(var)?;
                    }
                }
                self.check_clause_arities(info, clauses, *ret)?;
            }
            Call::Perform {
                effect,
                op,
                resumable,
                args: _,
            } => {
                let info = self.cx.effect(*effect)?;
                let Some(operation) = info.operations.get(*op as usize) else {
                    return Err(format!(
                        "`perform` names operation {op} of `{}`, which has {} operations",
                        info.name,
                        info.operations.len()
                    ));
                };
                if operation.resumable != *resumable {
                    let (table, call) = if operation.resumable {
                        ("resumes", "never resumes")
                    } else {
                        ("never resumes", "resumes")
                    };
                    return Err(format!(
                        "`{}.{}` {table}, but this perform {call}",
                        info.name, operation.name
                    ));
                }
            }
            Call::Clause {
                effect,
                op,
                callee: _,
                args,
            } => {
                if self.level == Level::Translated {
                    return Err("a clause call is used before the evidence pass".to_string());
                }
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
                        "a clause of `{}` is given {} arguments, but it takes {takes} (the evidence and {} arguments of the operation)",
                        operation_text(&self.program.effects, *effect, *op),
                        args.len(),
                        operation.params.len()
                    ));
                }
            }
            Call::Apply { .. }
            | Call::Resume {
                k: _,
                arg: _,
                state: _,
            } => {}
        }
        self.use_all(|f| call.for_each_atom(f))?;
        // Core IR のパスは引数を変換した変数に替えないので、どの段でも変数と場所を比べる
        match call {
            Call::Apply { args, site, .. } => self.cx.check_site(args, site, true),
            _ => Ok(()),
        }
    }

    /// 着地の印 `prompt v` の検査。印は EP だけが出し、末尾の位置の呼び出しには付かず、`v` は呼び出しの後も
    /// 生きている `obj` の変数でなければならない (docs/spec/lir.md の「着地の印」)。
    fn check_prompt(&self, call: &Call) -> Result<(), String> {
        let Some(marker) = call.prompt() else {
            return Ok(());
        };
        if self.level == Level::Translated {
            return Err("a prompt marker appears before the evidence pass".to_string());
        }
        self.visible(marker)?;
        let block = &self.function.blocks[self.at.block as usize];
        if tail_position_call(block).is_some_and(|(at, _)| at as i64 == self.at.index) {
            return Err(format!(
                "an apply with the prompt marker `{}` is in a tail position, which leaves no frame to land on",
                self.cx.name(marker)
            ));
        }
        let repr = self.function.repr(marker);
        if repr != Repr::Obj {
            return Err(format!(
                "`{}` is the prompt marker of an apply, but it is {}, not obj",
                self.cx.name(marker),
                repr.name()
            ));
        }
        if !self.used_after(marker) {
            return Err(format!(
                "`{}` is the prompt marker of an apply, but it is not used after the call",
                self.cx.name(marker)
            ));
        }
        Ok(())
    }

    /// 呼び出しの直後で `var` が生きているか。生存解析の入口の集合から、終端と後ろの文を逆にたどって求める。
    fn used_after(&self, var: VarId) -> bool {
        let function = self.function;
        let live_in = self.live_in.get_or_init(|| liveness::live_in(function));
        let block = &function.blocks[self.at.block as usize];
        let mut live = liveness::live_after_term(function, &block.term, live_in);
        block
            .term
            .for_each_atom(|atom| liveness::insert_var(&mut live, atom));
        let first = (self.at.index + 1) as usize;
        for stmt in block.stmts[first..].iter().rev() {
            liveness::step_back(stmt, &mut live);
        }
        live.contains(&var)
    }

    /// 結果を受けない呼び出し。`never` の操作の `perform` と `clause` は戻らない。
    fn never_returns(&self, call: &Call) -> bool {
        match call {
            Call::Perform { resumable, .. } => !resumable,
            Call::Clause { effect, op, .. } => {
                operation(&self.program.effects, *effect, *op).ret == OpRet::Never
            }
            Call::Direct(_, _) | Call::Apply { .. } | Call::Handle { .. } | Call::Resume { .. } => {
                false
            }
        }
    }

    /// `mask` はエフェクトの表にある番号を真に昇順に並べた項目で、どの項目も 1 つ以上の handler を飛ばす。extern のエフェクトは表にないので、`mask` にも
    /// 現れない。`handle`、`perform`、`clause` は `mask` を持たない (docs/spec/core-ir.md)。
    fn check_mask(&self, call: &Call, mask: &[MaskItem]) -> Result<(), String> {
        if mask.is_empty() {
            return Ok(());
        }
        // EP は `mask` を `evv_mask` に置き換える (docs/spec/core-ir.md の
        // 「`mask` の包み」)
        if self.level != Level::Translated {
            return Err("a mask remains after the evidence pass".to_string());
        }
        match call {
            Call::Clause { .. } => return Err("a mask on clause".to_string()),
            Call::Handle {
                effect: _,
                init: _,
                body: _,
                clauses: _,
                ret: _,
            } => return Err("a mask on handle".to_string()),
            Call::Perform {
                effect: _,
                op: _,
                resumable: _,
                args: _,
            } => return Err("a mask on perform".to_string()),
            Call::Direct(_, _)
            | Call::Apply { .. }
            | Call::Resume {
                k: _,
                arg: _,
                state: _,
            } => {}
        }
        if let Some(unknown) = mask
            .iter()
            .find(|item| item.effect as usize >= self.program.effects.len())
        {
            return Err(format!(
                "a mask names an unknown effect #{}",
                unknown.effect
            ));
        }
        if !mask.is_sorted_by(|a, b| a.effect < b.effect) {
            return Err("a mask is not in ascending order".to_string());
        }
        if let Some(item) = mask.iter().find(|item| item.skip == 0) {
            return Err(format!(
                "a mask item of `{}` skips no handler",
                effect_text(&self.program.effects, item.effect)
            ));
        }
        Ok(())
    }

    /// 節は捕獲の後に「操作の引数 + `k` (再開する操作) + 状態」を、`return` の節は値と状態を受ける
    /// (docs/spec/core-ir.md)。関数が静的に分からない節は確かめない。
    fn check_clause_arities(
        &self,
        info: &EffectInfo,
        clauses: &[Atom],
        ret: Atom,
    ) -> Result<(), String> {
        for (op, &clause) in info.operations.iter().zip(clauses) {
            let Some(has) = self.parameters_left(clause) else {
                continue;
            };
            let needs = op.params.len() + usize::from(op.resumable) + 1;
            if has != needs {
                let receives = if op.resumable {
                    "for the arguments, `k`, and the state"
                } else {
                    "for the arguments and the state"
                };
                return Err(format!(
                    "the clause for `{}` needs {needs} parameters after its captures ({} {receives}), but it has {has}",
                    op.name,
                    op.params.len()
                ));
            }
        }
        match self.parameters_left(ret) {
            Some(has) if has != 2 => Err(format!(
                "the `return` clause needs 2 parameters after its captures (the value and the state), but it has {has}"
            )),
            _ => Ok(()),
        }
    }

    /// 呼ばれる値の関数が、すでに渡した引数の後にまだ受ける引数の数。関数の値か、同じ関数の中で `closure` で作った
    /// 値のときだけ分かる。変数は1回だけ定義されるので、`closure` の記録は関数全体で引ける。
    fn parameters_left(&self, atom: Atom) -> Option<usize> {
        let (function, passed) = match atom {
            Atom::Fn(function) => (function, 0),
            Atom::Var(var) => self.cx.closure(var)?,
            Atom::Int(_) | Atom::Float(_) | Atom::Unit | Atom::Tag(_) => return None,
        };
        let params = self
            .program
            .functions
            .get(function.0 as usize)?
            .params()
            .len();
        Some(params.saturating_sub(passed))
    }

    fn function_at(&self, target: FnIdx) -> Result<&'a CoreFn, String> {
        self.program
            .functions
            .get(target.0 as usize)
            .ok_or_else(|| format!("a call refers to the unknown function #{}", target.0))
    }
}
