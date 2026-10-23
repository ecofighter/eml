//! LIR の不変条件の検査 (docs/spec/lir.md の「verifier」)。ブロックの列の形 (R1〜R4)、変数の定義と支配 (R5、R6)、
//! `unpack` の Repr、`jump` と `return` の Repr の互換、extern の引数と結果の Repr、`box` と `unbox` のオペランドと
//! 束縛の Repr (R7)、データの配置 (R8) と、引き継いだ検査 (直接呼び出しと extern の引数の数、case の種類)、基本操作と
//! 節の呼び出しの形を確かめる (`verify_scopes`)。R7 と R8 には境界の検査も入る。呼び出しの引数と結果、`closure` の
//! 引数、`tobj` のフィールドを比べる。`tail` の結果は印だけを比べる。box の挿入の前の入口の段では、
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
    self, BlockShape, Context, Dominators, FnSummary, Tables, TermShape, boxed_scalars,
    clause_function, field_count, shape,
};
use crate::liveness;
use crate::reuse;
use crate::syntax::{layout_text, operation_text, prim_text, repr_text, rets_text};
use crate::unwind::{self, UnwindPoint};
use crate::{
    Atom, Block, BlockId, Call, CasePattern, Ctor, FnIdx, Layout, LirFn, Mark, OpRet, Prim,
    Program, Repr, Reuse, Rhs, Stmt, Term, VarId, VerifyError, operation,
};

/// Perceus の後の LIR を確かめる。形と範囲に加えて、RC の対象の変数の所有が釣り合うことを確かめる。
pub fn verify(program: &Program) -> Result<(), VerifyError> {
    verify_at(program, Level::Ownership)
}

/// 開くパス (flatten) より前の LIR を確かめる。形と範囲と境界を確かめ、RC の命令がまだないことを確かめる。構造体の
/// 変数を受け、複数の値の `let` と `return` と構造体の `unbox` を拒む。
pub fn verify_scopes(program: &Program) -> Result<(), VerifyError> {
    verify_at(program, Level::Scopes)
}

/// 開くパスの後で Perceus より前の LIR (縮約の後も) を確かめる。範囲の段の検査に加えて、構造体がどこにも残らないことを確かめ、
/// 複数の値の `let` と `return` と構造体の `unbox` を受ける
/// (docs/spec/lir.md の「verifier」)。
pub fn verify_flattened(program: &Program) -> Result<(), VerifyError> {
    verify_at(program, Level::Flattened)
}

/// box の挿入の前の LIR を確かめる。範囲の段と同じ形と範囲を確かめ、境界は比べない。box の挿入と縮約が作るもの
/// (`box`、`unbox`、`tail`) と、Perceus が入れるもの (`dup`、`decref`、`release`) がまだないことを確かめる。
/// box の挿入は、末尾呼び出しと変換のない入力を前提にするためである (docs/spec/lir.md の「パスの順番」)。
pub fn verify_lowered(program: &Program) -> Result<(), VerifyError> {
    verify_at(program, Level::Lowered)
}

/// 検査の段。段はパスの順に直線に並ぶ。入口の段の LIR には、box の挿入と縮約が作るものがまだない。開くパスより前の
/// LIR には構造体が残り、複数の値の命令がまだない。Perceus より前の LIR には、所有を確かめる材料 (`dup`、`decref`、`release`) がまだない。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Level {
    Lowered,
    Scopes,
    Flattened,
    Ownership,
}

/// Perceus の後の関数を所有の段で確かめながら、巻き戻しの地点ごとの所有を書き留める (`unwind::points_all`)。
/// 表と「中断しうる」の不動点は、プログラムに1回だけ求める。
pub(crate) fn unwind_points(program: &Program) -> Vec<Result<Vec<UnwindPoint>, VerifyError>> {
    let aborts = unwind::may_abort(program);
    with_tables(program, |tables| {
        program
            .functions
            .iter()
            .map(|function| {
                shape(&block_shapes(function))
                    .and_then(|dominators| {
                        let mut checker =
                            Checker::new(program, tables, function, Level::Ownership, dominators);
                        checker.unwind = Some(Recorder {
                            aborts: &aborts,
                            points: Vec::new(),
                        });
                        checker.run()
                    })
                    .map(|points| points.expect("the checker was given a list"))
                    .map_err(|message| VerifyError {
                        function: function.name.clone(),
                        message,
                    })
            })
            .collect()
    })
}

fn verify_at(program: &Program, level: Level) -> Result<(), VerifyError> {
    if level >= Level::Flattened {
        no_struct_layouts(program)?;
    }
    with_tables(program, |tables| {
        // `clause` の印の関数の ABI と `split` は、参照ごとでなく関数ごとに1回だけ確かめる
        for function in &program.functions {
            rets(program, function, level, function.clause.is_some()).map_err(|message| {
                VerifyError {
                    function: function.name.clone(),
                    message,
                }
            })?;
            clause_function(
                &program.effects,
                &function.name,
                function.clause,
                function
                    .params()
                    .iter()
                    .map(|&param| (param, &function.vars[param.0 as usize])),
                function.rets[0].0,
                function.rets[0].1,
                level >= Level::Scopes,
            )
            .and_then(|()| check::split(function.split, function.params().len()))
            .and_then(|()| {
                if level >= Level::Scopes {
                    variant_name(program, function)
                } else {
                    Ok(())
                }
            })
            .map_err(|message| VerifyError {
                function: function.name.clone(),
                message,
            })?;
        }
        for function in &program.functions {
            shape(&block_shapes(function))
                .and_then(|dominators| {
                    Checker::new(program, tables, function, level, dominators).run()
                })
                .map_err(|message| VerifyError {
                    function: function.name.clone(),
                    message,
                })?;
        }
        // 値の参照の規則は、参照の形 (`closure` の引数の数など) を本体の検査が確かめた後に見る
        if level >= Level::Scopes {
            for function in &program.functions {
                value_references(program, function).map_err(|message| VerifyError {
                    function: function.name.clone(),
                    message,
                })?;
            }
        }
        Ok(())
    })
}

/// 一様な位置で値を受ける Repr (`obj`、`tobj`、`unit`) か。
fn is_uniform(repr: Repr) -> bool {
    matches!(repr, Repr::Obj | Repr::TObj | Repr::Unit)
}

/// 捕獲を `captured` 個受けた後の引数と、結果がどれも一様か。
fn uniform_beyond(function: &LirFn, captured: usize) -> bool {
    function.rets.len() == 1
        && is_uniform(function.rets[0].0)
        && function
            .params()
            .iter()
            .skip(captured)
            .all(|&param| is_uniform(function.repr(param)))
}

/// `<f>$uniform` という名前の関数は版で、`<f>` があるときだけ存在し、すべて一様な ABI を持つ
/// (docs/spec/lir.md の「関数の ABI」)。
fn variant_name(program: &Program, function: &LirFn) -> Result<(), String> {
    let Some(base) = function.name.strip_suffix("$uniform") else {
        return Ok(());
    };
    if !program.functions.iter().any(|other| other.name == base) {
        return Err(format!("the variant has no function `{base}`"));
    }
    if function.split.is_some() || function.clause.is_some() || !uniform_beyond(function, 0) {
        return Err("the variant does not have the uniform ABI".to_string());
    }
    Ok(())
}

/// 値として参照する関数は、捕獲より後ろの引数と結果が一様である。`clause` の印の関数は操作の ABI で呼ばれるので
/// 除く (docs/spec/lir.md の「関数の ABI」)。表にない番号は本体の検査が報告する。
fn value_references(program: &Program, function: &LirFn) -> Result<(), String> {
    let check = |target: FnIdx, captured: usize| -> Result<(), String> {
        match program.functions.get(target.0 as usize) {
            Some(callee) if callee.clause.is_none() && !uniform_beyond(callee, captured) => {
                Err(format!(
                    "`{}` is used as a value with {captured} captures, but its later parameters and its result are not all uniform",
                    callee.name
                ))
            }
            _ => Ok(()),
        }
    };
    let mut result = Ok(());
    let mut atom = |atom: Atom| {
        if let Atom::Fn(target) = atom
            && result.is_ok()
        {
            result = check(target, 0);
        }
    };
    for block in &function.blocks {
        for stmt in &block.stmts {
            if let Stmt::Let {
                var: _,
                rhs: Rhs::MakeClosure(target, args),
            } = stmt
            {
                check(*target, args.len())?;
            }
            stmt.for_each_atom(&mut atom);
        }
        block.term.for_each_atom(&mut atom);
    }
    result
}

/// 開いた後の段では、配置のフィールドに構造体は来ない。構造体の型を宣言したフィールドは `tobj` である
/// (docs/spec/lir.md の「verifier」)。
fn no_struct_layouts(program: &Program) -> Result<(), VerifyError> {
    for layout in &program.layouts {
        for constructor in &layout.constructors {
            if constructor
                .fields
                .iter()
                .any(|repr| matches!(repr, Repr::Struct(_)))
            {
                return Err(VerifyError {
                    function: layout.name.clone(),
                    message: format!(
                        "the constructor `{}` has a struct field, but no struct remains after flatten",
                        constructor.name
                    ),
                });
            }
        }
    }
    Ok(())
}

/// 開いた後の段では、変数の表のどこにも構造体は残らない。開くパスは開いた構造体の変数を表から外す。
fn no_struct_vars(program: &Program, function: &LirFn) -> Result<(), String> {
    match function
        .vars
        .iter()
        .enumerate()
        .find(|(_, info)| matches!(info.repr, Repr::Struct(_)))
    {
        Some((index, info)) => Err(format!(
            "`{}.{index}` is {}, but no struct remains after flatten",
            info.name,
            repr_text(&program.layouts, &program.structs, info.repr)
        )),
        None => Ok(()),
    }
}

/// 関数の結果の並びは空でない。2つ以上の値を返す関数は開いた後の段だけにあり、開いた後の結果に構造体は残らない。
/// `clause` の印の関数は操作の ABI で値を1つ返す。
fn rets(program: &Program, function: &LirFn, level: Level, clause: bool) -> Result<(), String> {
    let text = || rets_text(&program.layouts, &program.structs, &function.rets);
    match function.rets.len() {
        0 => return Err("the function returns no values".to_string()),
        1 => {}
        _ if level < Level::Flattened => {
            return Err(format!("the function returns {} before flatten", text()));
        }
        _ if clause => {
            return Err(format!(
                "the function is a clause, but it returns {}",
                text()
            ));
        }
        _ => {}
    }
    if level >= Level::Flattened
        && function
            .rets
            .iter()
            .any(|(repr, _)| matches!(repr, Repr::Struct(_)))
    {
        return Err(format!(
            "the function returns {}, but no struct remains after flatten",
            text()
        ));
    }
    if level >= Level::Flattened {
        no_struct_vars(program, function)?;
    }
    Ok(())
}

fn with_tables<R>(program: &Program, f: impl FnOnce(Tables<'_>) -> R) -> R {
    // 配置の Repr はコンストラクタをすべてたどって決まるので、命令ごとでなく表ごとに1回だけ求める
    let layout_reprs: Vec<Repr> = program.layouts.iter().map(Layout::repr).collect();
    let functions: Vec<FnSummary> = program
        .functions
        .iter()
        .map(|function| FnSummary {
            name: &function.name,
            clause: function.clause,
        })
        .collect();
    let tables = Tables {
        layouts: &program.layouts,
        layout_reprs: &layout_reprs,
        structs: &program.structs,
        effects: &program.effects,
        strings: &program.strings,
        functions: &functions,
    };
    f(tables)
}

/// 辺の規則の検査 (`shape`) に渡す、ブロックの形。
pub(crate) fn block_shapes(function: &LirFn) -> Vec<BlockShape> {
    function
        .blocks
        .iter()
        .map(|block| BlockShape {
            params: block.params.len(),
            term: match &block.term {
                Term::Return(_) | Term::ReturnMany(_) | Term::TailCall(_) => TermShape::Exit,
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

/// 呼び出しの結果を受ける所。`let` の束縛か、複数の値の `let` の束縛の並びか、`tail` の呼び出し元の結果である。
#[derive(Clone, Copy)]
enum Receiver<'v> {
    Bound(VarId),
    Many(&'v [VarId]),
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

/// RC の対象の変数ごとの、所有している参照の数。0 になった変数は除く。所有の段では、経路でまだ使っていない token も
/// 1 として持つ。token も経路ごとにちょうど1回使うので、所有と同じ写し方で `switch` の行き先へ渡せる
/// (docs/spec/object-model.md の「reuse」)。
type Owned = BTreeMap<VarId, u32>;

/// 道ごとの、構築子が分かっている変数。タグの case の行き先はその scrutinee の、`unpack` はその値の構築子を知る。
/// 変数は1回だけ定義され、データは変わらないので、事実は道の上で消えない。合流するブロックでは、入るすべての道で
/// 分かっているものだけが残る。reuse のパスと同じ求め方である (crates/eml_lir/src/reuse.rs)。フィールドのない
/// 構築子は箱でなく即値で、手放すことがないので数えない。
type Known = HashMap<VarId, Ctor>;

struct Checker<'a> {
    program: &'a Program,
    function: &'a LirFn,
    level: Level,
    dominators: Dominators,
    /// 所有にも命令の型にも関わらない検査の状態。
    cx: Context<'a>,
    defs: Vec<Option<Site>>,
    at: Site,
    /// 着地の印の検査が使う入口の生存集合。印がなければ求めない。
    live_in: std::cell::OnceCell<Vec<Vec<VarId>>>,
    /// まだ着いていないブロックの入口の所有。`switch` の行き先は1本の辺から、合流するブロックは最初の `jump` から
    /// 作る。
    pending: HashMap<u32, Owned>,
    /// 今の道の `Known` と、まだ着いていないブロックの入口の `Known`。`pending` と同じ辺で写す。
    known: Known,
    pending_known: HashMap<u32, Known>,
    /// フィールドの持ち主と出どころ。持ち主は、分解した値が束縛の時点で所有を持っていればその値、なければその値の
    /// 持ち主である。変数は1回だけ定義されるので (R5)、経路ごとではなく関数に1つの表にする
    /// (docs/spec/lir.md の「verifier」)。
    owners: Vec<Option<VarId>>,
    origins: Vec<Option<Origin>>,
    /// token ごとの、元の値とそのコンストラクタ。`owners` と同じく、変数は1回だけ定義されるので関数に1つの表にする。
    token_sources: Vec<Option<(VarId, Ctor)>>,
    /// 構造体の `unbox` が読む箱。その `release` だけが `unboxed` の印を持ち、いつも持つ。VM は印の付いた `release`
    /// を、ダミーを受けても何もしない命令にするためである
    /// (docs/spec/lir.md の「ダミーの値」)。変数は1回だけ
    /// 定義されるので (R5)、関数に1つの表にする。
    unboxed: Vec<bool>,
    /// 巻き戻しの地点ごとの所有を書き留める先。`unwind_points` だけが与える。
    unwind: Option<Recorder<'a>>,
}

/// 巻き戻しの地点を決める関数ごとの「中断しうる」(`unwind::may_abort`) と、書き留めた地点。
struct Recorder<'a> {
    aborts: &'a [bool],
    points: Vec<UnwindPoint>,
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
            live_in: std::cell::OnceCell::new(),
            pending: HashMap::new(),
            known: Known::new(),
            pending_known: HashMap::new(),
            owners: vec![None; function.vars.len()],
            origins: vec![None; function.vars.len()],
            token_sources: vec![None; function.vars.len()],
            unboxed: function.struct_boxes(),
            unwind: None,
        }
    }

    fn run(mut self) -> Result<Option<Vec<UnwindPoint>>, String> {
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
            self.known = self.pending_known.remove(&index).unwrap_or_default();
            self.at = Site {
                block: index,
                index: -1,
            };
            for &param in &block.params {
                self.define(&mut owned, param, self.at)?;
            }
            self.check_block(index, block, owned)?;
        }
        Ok(self.unwind.map(|recorder| recorder.points))
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

    fn check_block(&mut self, index: u32, block: &Block, mut owned: Owned) -> Result<(), String> {
        let function = self.function;
        for (at, stmt) in block.stmts.iter().enumerate() {
            self.at = Site {
                block: index,
                index: at as i64,
            };
            self.check_stmt(&mut owned, stmt)?;
            if let Some(Recorder { aborts, points }) = &mut self.unwind
                && let Some((vars, landing)) = unwind::point(stmt, aborts)
            {
                // 束縛した変数は命令が戻ってから現れるので、巻き戻しで手放すものに入れない
                let (tokens, owned): (Vec<_>, Vec<_>) = owned
                    .iter()
                    .filter(|&(held, _)| !vars.contains(held))
                    .map(|(&var, &count)| (var, count))
                    .partition(|&(held, _)| function.repr(held) == Repr::Token);
                points.push(UnwindPoint {
                    block: BlockId(index),
                    stmt: at,
                    owned,
                    tokens: tokens.into_iter().map(|(token, _)| token).collect(),
                    landing,
                });
            }
        }
        self.at = Site {
            block: index,
            index: block.stmts.len() as i64,
        };
        self.check_term(owned, block)
    }

    fn check_stmt(&mut self, owned: &mut Owned, stmt: &Stmt) -> Result<(), String> {
        let mut atoms = Vec::new();
        stmt.for_each_atom(|atom| atoms.push(atom));
        self.no_token_values(&atoms)?;
        match stmt {
            Stmt::Let { var, rhs } => {
                self.check_rhs(owned, *var, rhs)?;
                match rhs {
                    Rhs::Call(call) => {
                        let never = self.never_returns(call);
                        self.cx.note_never(*var, never);
                        // 継続を2回再開すると、yield の後の `con` が同じ箱を2回使うので、token は yield しうる呼び出しを
                        // またがない (docs/spec/object-model.md の「reuse」)
                        if reuse::may_yield(self.program, self.function, call) {
                            self.no_token_live(owned, || {
                                format!(
                                    "live after {}, which may yield",
                                    self.call_text(call, false)
                                )
                            })?;
                        }
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
                    | Rhs::Con { .. }
                    | Rhs::Drop(_)
                    | Rhs::Box(_)
                    | Rhs::Unbox(_) => {}
                }
                self.define(owned, *var, self.at)
            }
            Stmt::LetMany { vars, call } => {
                if self.level < Level::Flattened {
                    return Err(format!(
                        "a let of {} values is formed before flatten",
                        vars.len()
                    ));
                }
                self.check_call(owned, call, Receiver::Many(vars))?;
                if reuse::may_yield(self.program, self.function, call) {
                    self.no_token_live(owned, || {
                        format!(
                            "live after {}, which may yield",
                            self.call_text(call, false)
                        )
                    })?;
                }
                for &var in vars {
                    self.define(owned, var, self.at)?;
                }
                Ok(())
            }
            Stmt::Unpack {
                value,
                ctor,
                fields,
            } => {
                self.cx.check_unpack(*value, *ctor, fields)?;
                self.read(owned, *value, "unpacked")?;
                self.known.insert(*value, *ctor);
                self.bind_fields(
                    owned,
                    *value,
                    *ctor,
                    fields.iter().copied().enumerate(),
                    self.at,
                )
            }
            Stmt::UnboxStruct {
                value,
                ctor,
                fields,
            } => {
                if self.level < Level::Flattened {
                    return Err(format!(
                        "an unbox of `{}` as `{}` is formed before flatten",
                        self.cx.name(*value),
                        layout_text(&self.program.layouts, ctor.layout)
                    ));
                }
                self.check_unbox_struct(*value, *ctor, fields)?;
                self.read(owned, *value, "unboxed")?;
                self.known.insert(*value, *ctor);
                let fields = fields
                    .iter()
                    .enumerate()
                    .filter_map(|(slot, field)| field.map(|field| (slot, field)));
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
                token,
                unboxed,
            } => {
                self.rc_allowed(*value, "released with its fields")?;
                match (*unboxed, self.unboxed[value.0 as usize]) {
                    (false, true) => {
                        return Err(format!(
                            "`{}` is unboxed as a struct, but its release is not marked unboxed",
                            self.cx.name(*value)
                        ));
                    }
                    (true, false) => {
                        return Err(format!(
                            "`{}` is released as unboxed, but it is not unboxed as a struct",
                            self.cx.name(*value)
                        ));
                    }
                    _ => {}
                }
                self.release(owned, *value, *ctor, fields, token.is_some())?;
                match *token {
                    Some(token) => self.define_token(owned, token, *value, *ctor),
                    None => Ok(()),
                }
            }
            Stmt::DropToken(token) => {
                self.rc_allowed(*token, "dropped")?;
                self.use_token(owned, *token)
            }
        }
    }

    fn is_token(&self, var: VarId) -> bool {
        self.function.repr(var) == Repr::Token
    }

    /// token は、token を受ける `con` と `drop_token` でだけ使う。値の位置 (呼び出し、`apply`、`clause`、extern と
    /// `prim` の引数、閉包の捕獲、`con` のフィールド、`jump` の実引数、`return` の値、`switch` の scrutinee) には
    /// 現れない (docs/spec/object-model.md の「reuse」)。
    fn no_token_values(&self, atoms: &[Atom]) -> Result<(), String> {
        match atoms.iter().find_map(|&atom| match atom {
            Atom::Var(var) if self.is_token(var) => Some(var),
            _ => None,
        }) {
            Some(token) => Err(format!(
                "the token `{}` is used as a value",
                self.cx.name(token)
            )),
            None => Ok(()),
        }
    }

    /// 経路でまだ使っていない token がないこと。`state` は誤りの文で token の状態を言う句である。
    fn no_token_live(&self, owned: &Owned, state: impl FnOnce() -> String) -> Result<(), String> {
        match owned.keys().find(|&&var| self.is_token(var)) {
            Some(&token) => Err(format!(
                "the token `{}` is {}",
                self.cx.name(token),
                state()
            )),
            None => Ok(()),
        }
    }

    /// token を定義する `release` の token。元の値とコンストラクタを、token を受ける `con` の検査のために覚える。
    fn define_token(
        &mut self,
        owned: &mut Owned,
        token: VarId,
        value: VarId,
        ctor: Ctor,
    ) -> Result<(), String> {
        if !self.is_token(token) {
            return Err(format!(
                "`{}` ({}) is bound to a release, which gives a token",
                self.cx.name(token),
                self.function.repr(token).name()
            ));
        }
        self.define_once(token, self.at)?;
        owned.insert(token, 1);
        self.token_sources[token.0 as usize] = Some((value, ctor));
        Ok(())
    }

    /// token を使う。経路でまだ使っていない token に限る。
    fn use_token(&self, owned: &mut Owned, token: VarId) -> Result<(), String> {
        self.visible(token)?;
        if !self.is_token(token) {
            return Err(format!(
                "`{}` ({}) is used as a token",
                self.cx.name(token),
                self.function.repr(token).name()
            ));
        }
        if owned.remove(&token).is_none() {
            return Err(format!("the token `{}` is used twice", self.cx.name(token)));
        }
        Ok(())
    }

    /// token を受ける `con` の検査。箱の物理的な語の数は、token の元のコンストラクタと同じである。ヘッダを書かない
    /// `con` は、元と同じ配置の同じタグで、`lin` ビットの条件 (元の変数が `unr` か、`con` の束縛が `lin`) を満たす。
    /// 書かない引数は、元の値を分解して束縛した変数で、元と `con` で物理的な位置と語の種類が同じである。箱は中身を
    /// 消さないので、その位置には同じ語が残っている (docs/spec/object-model.md の「reuse」と「`lin` ビット」)。
    fn check_reuse(
        &self,
        owned: &mut Owned,
        var: VarId,
        ctor: Ctor,
        args: &[Atom],
        reuse: &Reuse,
    ) -> Result<(), String> {
        let token = reuse.token;
        self.rc_allowed(token, "reused")?;
        self.use_token(owned, token)?;
        let (source, from) = self.token_sources[token.0 as usize]
            .expect("only a release defines a token, and it records the source");
        // どちらのコンストラクタも、`check_con` と `release` が配置で確かめてある
        let (layout, _, constructor) = self.cx.ctor(ctor, "a con")?;
        let (from_layout, _, from_constructor) = self.cx.ctor(from, "a release")?;
        let (shape, from_shape) = (constructor.shape(), from_constructor.shape());
        let what = format!("a con of `{}` #{}", layout.name, ctor.tag);
        let token_name = self.cx.name(token);
        let words = |words: Option<u32>| {
            words.map_or("255 or more scanned words".to_string(), |words| {
                format!("{words} words")
            })
        };
        match (shape.reuse_words(), from_shape.reuse_words()) {
            (Some(needed), Some(held)) if needed == held => {}
            (needed, held) => {
                return Err(format!(
                    "{what} has {}, but the token `{token_name}` holds {}",
                    words(needed),
                    words(held)
                ));
            }
        }
        if reuse.in_place {
            if ctor != from {
                return Err(format!(
                    "{what} rebuilds the token `{token_name}` in place, but the token comes from `{}` #{}",
                    from_layout.name, from.tag
                ));
            }
            let (source_mark, mark) = (self.function.mark(source), self.function.mark(var));
            if source_mark != Mark::Unr && mark != Mark::Lin {
                return Err(format!(
                    "{what} rebuilds the token `{token_name}` in place, but `{}` is {} and `{}` is {}",
                    self.cx.name(source),
                    source_mark.name(),
                    self.cx.name(var),
                    mark.name()
                ));
            }
        }
        if reuse.skip.len() != args.len() {
            return Err(format!(
                "{what} has {} arguments, but {} skip marks",
                args.len(),
                reuse.skip.len()
            ));
        }
        for (index, (&arg, _)) in args
            .iter()
            .zip(&reuse.skip)
            .enumerate()
            .filter(|(_, (_, skip))| **skip)
        {
            let kept = match arg {
                Atom::Var(field) => self.origins[field.0 as usize].is_some_and(|origin| {
                    origin.value == source
                        && origin.ctor == from
                        && from_constructor.fields[origin.slot].is_rc()
                            == constructor.fields[index].is_rc()
                        && from_shape.positions[origin.slot] == shape.positions[index]
                }),
                Atom::Int(_) | Atom::Float(_) | Atom::Unit | Atom::Tag(_) | Atom::Fn(_) => false,
            };
            if !kept {
                return Err(format!(
                    "argument {index} of {what} is skipped, but {} is not at the same position in the box of the token `{token_name}`",
                    self.cx.atom_text(arg)
                ));
            }
        }
        Ok(())
    }

    fn check_term(&mut self, mut owned: Owned, block: &Block) -> Result<(), String> {
        let term = &block.term;
        let mut atoms = Vec::new();
        term.for_each_atom(|atom| atoms.push(atom));
        self.no_token_values(&atoms)?;
        // token は合流するブロックへ流れず、関数からも出ない (docs/spec/object-model.md の「reuse」)
        match term {
            Term::Return(_) | Term::ReturnMany(_) => {
                self.no_token_live(&owned, || "still live at a return".to_string())?
            }
            Term::TailCall(_) => {
                self.no_token_live(&owned, || "still live at a tail call".to_string())?;
            }
            Term::Jump { target, args: _ } => {
                self.no_token_live(&owned, || format!("still live at a jump to b{}", target.0))?
            }
            Term::Switch { .. } => {}
        }
        match term {
            Term::Return(atom) => {
                self.consume(&mut owned, *atom)?;
                self.check_returned(std::slice::from_ref(atom))?;
                self.nothing_owned(&owned)
            }
            Term::ReturnMany(atoms) => {
                if self.level < Level::Flattened {
                    return Err(format!(
                        "a return of {} values is formed before flatten",
                        atoms.len()
                    ));
                }
                for &atom in atoms {
                    self.consume(&mut owned, atom)?;
                }
                self.check_returned(atoms)?;
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
                self.merge_known(*target);
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
                self.cx.switch_scrutinee(*scrutinee)?;
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
                    let mut known = self.known.clone();
                    if let (Atom::Var(value), Some(layout), CasePattern::Tag(tag)) =
                        (*scrutinee, *layout, case.pattern)
                    {
                        let ctor = Ctor { layout, tag };
                        if !case.fields.is_empty() {
                            known.insert(value, ctor);
                        }
                        self.bind_fields(
                            &mut entry,
                            value,
                            ctor,
                            case.fields.iter().copied().enumerate(),
                            site,
                        )?;
                    }
                    self.pending.insert(case.target.0, entry);
                    self.pending_known.insert(case.target.0, known);
                }
                if let Some(default) = default {
                    self.pending.insert(default.0, owned);
                    self.pending_known.insert(default.0, self.known.clone());
                }
                Ok(())
            }
        }
    }

    /// 構造体の `unbox` の値は箱 (`obj` か `tobj`) で、配置はコンストラクタを1つ持つ。名前を書いたフィールドの変数は、
    /// 配置の宣言した Repr と互換である。構造体の型を宣言したフィールドは `tobj` で、続く `unbox` が受け直す
    /// (docs/spec/lir.md の「ブロックの列」)。
    fn check_unbox_struct(
        &self,
        value: VarId,
        ctor: Ctor,
        fields: &[Option<VarId>],
    ) -> Result<(), String> {
        let repr = self.function.repr(value);
        if !matches!(repr, Repr::Obj | Repr::TObj) {
            return Err(format!(
                "`{}` ({}) is unboxed as a struct, but only obj and tobj can be",
                self.cx.name(value),
                self.cx.repr_text(repr)
            ));
        }
        let (layout, _, constructor) = self.cx.ctor(ctor, "an unbox")?;
        if layout.constructors.len() != 1 {
            return Err(format!(
                "an unbox of `{}` names `{}`, which has {} constructors",
                self.cx.name(value),
                layout.name,
                layout.constructors.len()
            ));
        }
        field_count(
            format!("an unbox of `{}` as `{}`", self.cx.name(value), layout.name),
            fields.len(),
            constructor,
        )?;
        for (slot, (field, &declared)) in fields.iter().zip(&constructor.fields).enumerate() {
            let Some(field) = *field else { continue };
            let field_repr = self.function.repr(field);
            if !self.cx.compatible(field_repr, declared) {
                return Err(format!(
                    "field {slot} of an unbox of `{}` is `{}` ({}), but the layout has {}",
                    self.cx.name(value),
                    self.cx.name(field),
                    self.cx.repr_text(field_repr),
                    declared.name()
                ));
            }
        }
        Ok(())
    }

    /// `return` の値の並びは、関数の結果の並びと長さが同じで、値ごとに Repr が互換で印も互換である
    /// (docs/spec/lir.md の「末尾の位置の呼び出し」)。
    fn check_returned(&self, atoms: &[Atom]) -> Result<(), String> {
        let rets = &self.function.rets;
        if atoms.len() != rets.len() {
            return Err(format!(
                "a return gives {} values, but the function returns {}",
                atoms.len(),
                rets_text(&self.program.layouts, &self.program.structs, rets)
            ));
        }
        for (&atom, &(repr, mark)) in atoms.iter().zip(rets) {
            if !self.cx.passes(atom, repr) {
                return Err(format!(
                    "{} is returned from a function that returns {}",
                    self.cx.typed_atom_text(atom),
                    self.cx.repr_text(repr)
                ));
            }
            if !self.cx.mark_passes(atom, mark) {
                return Err(format!(
                    "{} is returned from a function that returns {}",
                    self.cx.marked_atom_text(atom),
                    mark.name()
                ));
            }
        }
        Ok(())
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

    /// 合流するブロックの入口の `Known` は、入るすべての `jump` の道で分かっているものの共通部分である。
    fn merge_known(&mut self, target: BlockId) {
        let known = std::mem::take(&mut self.known);
        match self.pending_known.get_mut(&target.0) {
            Some(first) => first.retain(|var, ctor| known.get(var) == Some(ctor)),
            None => {
                self.pending_known.insert(target.0, known);
            }
        }
    }

    /// `jump` の実引数は、行き先の引数と Repr が互換で (R7)、印も互換である (V2)。
    fn check_passed(&self, target: BlockId, arg: Atom, param: VarId) -> Result<(), String> {
        let expected = self.function.repr(param);
        if self.cx.passes(arg, expected) {
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
                self.cx.repr_text(self.function.repr(var)),
                self.cx.name(param),
                self.cx.repr_text(expected)
            ),
            _ => format!(
                "a jump to b{} passes {} to `{}` ({})",
                target.0,
                self.cx.atom_text(arg),
                self.cx.name(param),
                self.cx.repr_text(expected)
            ),
        })
    }

    /// 変数を多くとも1回定義し (R5)、RC の対象なら所有を1つ持つ。token は token を定義する `release` でだけ定義する
    /// (`define_token`)。
    fn define(&mut self, owned: &mut Owned, var: VarId, site: Site) -> Result<(), String> {
        if self.is_token(var) {
            return Err(format!(
                "`{}` is a token, but only a release defines one",
                self.cx.name(var)
            ));
        }
        self.define_once(var, site)?;
        if self.level == Level::Ownership && self.function.repr(var).is_rc() {
            owned.insert(var, 1);
        }
        Ok(())
    }

    /// 変数を多くとも1回定義する (R5)。
    fn define_once(&mut self, var: VarId, site: Site) -> Result<(), String> {
        let slot = &mut self.defs[var.0 as usize];
        if slot.is_some() {
            return Err(format!("`{}` is defined twice", self.cx.name(var)));
        }
        *slot = Some(site);
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
    /// `fields` はフィールドの位置と変数の組で、構造体の `unbox` は読まない位置を飛ばす。
    fn bind_fields(
        &mut self,
        owned: &mut Owned,
        value: VarId,
        ctor: Ctor,
        fields: impl Iterator<Item = (usize, VarId)>,
        site: Site,
    ) -> Result<(), String> {
        let owner = if owned.contains_key(&value) {
            Some(value)
        } else {
            self.owners[value.0 as usize]
        };
        for (slot, field) in fields {
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
    /// x を同じ配置とタグで分解したときの、同じ位置のフィールドである (docs/spec/lir.md)。残す位置のない `release` は
    /// `decref` と同じなので、token を定義する形でだけ認める。
    ///
    /// 箱の子は `L #t` の形で手放され、token の語の数も `L #t` で決まるので、違う構築子なら fast mode で記憶がおかしくなる
    /// (docs/spec/lir.md の「verifier」)。残すフィールドがあれば、その出どころが、x を `L #t` として分解したことを示す。
    /// 残すフィールドのない `release` (token だけを作る形) には示すものがないので、その道で x の構築子が `L #t` だと
    /// 分かっていることを求める。出どころで示せる `release` には求めない。両立しない2つの `switch` を通る死んだ道では、
    /// 分かっている構築子が出どころの構築子と食い違うことがあり、Perceus はそこにも `release` を出すためである。
    fn release(
        &self,
        owned: &mut Owned,
        value: VarId,
        ctor: Ctor,
        fields: &[Option<VarId>],
        defines_token: bool,
    ) -> Result<(), String> {
        if !defines_token && fields.iter().all(Option::is_none) {
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
        if fields.iter().all(Option::is_none) && self.known.get(&value) != Some(&ctor) {
            return Err(format!(
                "`{}` is released as `{}` #{}, but its constructor is not known on this path",
                self.cx.name(value),
                layout.name,
                ctor.tag
            ));
        }
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

    /// RC の命令は Perceus だけが、token の命令は Perceus の後の reuse のパスだけが入れる。
    fn rc_allowed(&self, var: VarId, what: &str) -> Result<(), String> {
        if self.level != Level::Ownership {
            return Err(format!("`{}` is {what} before Perceus", self.cx.name(var)));
        }
        Ok(())
    }

    /// 借用の印 `borrowed` が立つ引数を読む。
    fn read_borrowed(
        &self,
        owned: &mut Owned,
        args: &[Atom],
        borrowed: &[bool],
        lent: &str,
    ) -> Result<(), String> {
        for (&arg, &borrowed) in args.iter().zip(borrowed) {
            match arg {
                Atom::Var(arg) if borrowed => self.read(owned, arg, lent)?,
                // 変数でない値は所有を持たないので、消費と同じ検査 (関数の番号) だけを通す
                _ if borrowed => self.consume(owned, arg)?,
                _ => {}
            }
        }
        Ok(())
    }

    /// 借りる変数を同じ命令が消費もするなら、消費の数より多い参照を持っているはずである。借りる引数は命令の間じゅう
    /// 読まれるので、足りないと消費が最後の参照を渡して、読みが解放された物体に当たる
    /// (docs/spec/lir.md の「extern の借用」)。
    fn lent_apart_from_consumed(
        &self,
        owned: &Owned,
        args: &[Atom],
        borrowed: &[bool],
        lent: &str,
    ) -> Result<(), String> {
        if self.level != Level::Ownership {
            return Ok(());
        }
        for (&arg, _) in args.iter().zip(borrowed).filter(|&(_, &borrowed)| borrowed) {
            let Atom::Var(var) = arg else { continue };
            let Some(&held) = owned.get(&var) else {
                continue;
            };
            let consumed = args
                .iter()
                .zip(borrowed)
                .filter(|&(&other, &borrowed)| !borrowed && other == arg)
                .count();
            if consumed > 0 && held as usize <= consumed {
                return Err(format!(
                    "`{}` is {lent} and consumed by the same instruction",
                    self.cx.name(var)
                ));
            }
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
            Atom::Fn(_) | Atom::Int(_) | Atom::Float(_) | Atom::Unit | Atom::Tag(_) => {
                return Ok(());
            }
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
                    for (index, (&arg, &param)) in args.iter().zip(target.params()).enumerate() {
                        let expected = target.repr(param);
                        if !self.cx.passes(arg, expected) {
                            return Err(format!(
                                "argument {index} of a closure of `{}` is {}, but the function takes {}",
                                target.name,
                                self.cx.typed_atom_text(arg),
                                self.cx.repr_text(expected)
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
            Rhs::Extern { ext, args, at: _ } => {
                self.cx.check_extern(var, *ext, args)?;
                // 借用の引数は基本操作と同じく読む使いである (docs/spec/lir.md の「extern の借用」)
                let lent = format!("lent to `{}`", ext.row().name);
                self.read_borrowed(owned, args, ext.row().borrowed, &lent)?;
                self.lent_apart_from_consumed(owned, args, ext.row().borrowed, &lent)?;
            }
            Rhs::Prim(prim, args) => {
                self.cx.check_prim(var, prim, args)?;
                // 借用の引数は `unbox` のオペランドと同じく読む使いで、所有の数を変えない
                // (docs/spec/lir.md の「基本操作」)
                let lent = format!("lent to `prim {}`", prim_text(&self.program.effects, prim));
                self.read_borrowed(owned, args, prim.row().borrowed, &lent)?;
                self.lent_apart_from_consumed(owned, args, prim.row().borrowed, &lent)?;
            }
            Rhs::ConstString(index) => {
                if *index as usize >= self.program.strings.len() {
                    return Err(format!(
                        "a constant refers to string {index}, which does not exist"
                    ));
                }
            }
            Rhs::Con { ctor, args, reuse } => {
                self.cx.check_con(var, *ctor, args)?;
                if let Some(reuse) = reuse {
                    self.check_reuse(owned, var, *ctor, args, reuse)?;
                }
            }
            Rhs::Drop(_) => {}
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
                    Atom::Int(_) | Atom::Float(_) => true,
                    Atom::Unit | Atom::Tag(_) | Atom::Fn(_) => false,
                };
                if !boxable {
                    return Err(format!(
                        "{} is boxed, but only {} values and Int and Float constants can be",
                        self.cx.typed_atom_text(*atom),
                        boxed_scalars("and")
                    ));
                }
                // 構造体の箱は配置の `con` と同じ形のヒープの物体なので `obj` である
                // (docs/spec/lir.md の「値の表現」)
                let (boxed, of) = match *atom {
                    Atom::Var(operand)
                        if matches!(self.function.repr(operand), Repr::Struct(_)) =>
                    {
                        (Repr::Obj, "a box of a struct")
                    }
                    _ => (Repr::TObj, "a box"),
                };
                let repr = self.function.repr(var);
                if repr != boxed {
                    return Err(format!(
                        "`{}` ({}) is bound to {of}, which is {}",
                        self.cx.name(var),
                        self.cx.repr_text(repr),
                        boxed.name()
                    ));
                }
            }
            // `obj` はつねにヒープの物体を指すので、スカラーを入れた値にならない。構造体の箱は `obj` なので、構造体は
            // `obj` からも取り出す
            Rhs::Unbox(atom) => {
                if self.level < Level::Scopes {
                    return Err(format!(
                        "{} is unboxed before the boxing pass",
                        self.cx.atom_text(*atom)
                    ));
                }
                let gives_struct = matches!(self.function.repr(var), Repr::Struct(_));
                let operand = match *atom {
                    Atom::Var(operand)
                        if self.function.repr(operand) == Repr::TObj
                            || (gives_struct && self.function.repr(operand) == Repr::Obj) =>
                    {
                        operand
                    }
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
                Some(target.rets.clone())
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
                    OpRet::Repr(repr) => Some(vec![(repr, operation.ret_mark)]),
                    OpRet::Never => None,
                }
            }
            // `apply` の結果は一様な `tobj` である。関数の値は型変数の位置なので、結果の印はいつも `poly` で、どの受け手
            // とも合う (docs/spec/lir.md の「関数の ABI」)
            Call::Apply { .. } => Some(vec![(Repr::TObj, Mark::Poly)]),
            // 組は VM が子の一様な結果を2つの `tobj` のフィールドに入れて作るので、結果は `obj` である。子の値の型は
            // 型変数の位置なので、印は `apply` と同じく `poly` である (docs/spec/lir.md の「LIR の命令」)
            Call::Par { pair, .. } => {
                if matches!(receiver, Receiver::Tail) {
                    return Err(
                        "a par is a tail call, but the join needs the parent's frame".to_string(),
                    );
                }
                self.cx.check_par_pair(*pair)?;
                Some(vec![(Repr::Obj, Mark::Poly)])
            }
        };
        if self.cx.boundaries {
            self.check_operands(call)?;
            if let Some(result) = result {
                self.check_result(call, &result, receiver)?;
            }
        }
        self.consume_all(owned, |f| call.for_each_atom(f))?;
        match call.prompt() {
            Some(marker) => self.check_prompt(owned, call, marker),
            None => Ok(()),
        }
    }

    /// 着地の印 `prompt v` の検査。`v` は呼び出しの後も生きている `obj` の変数でなければならない。所有の段では、
    /// 呼び出しの直後にも所有していなければならない (docs/spec/lir.md の「着地の印」)。
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

    /// 呼び出しのオペランドを左から比べる (R7)。直接の呼び出しは、呼ばれる関数の引数と比べる。関数値の適用は、呼ぶ
    /// 値と引数を `tobj` と比べる。呼び出しの形は確かめてある。印も同じ所で比べる (V2)。関数値の
    /// 適用の位置の印はいつも `poly` なので、印を比べない。
    fn check_operands(&self, call: &Call) -> Result<(), String> {
        // 誤りの文で呼び出しを指す句 (`an apply`) は呼び出しの形で決まるので、比べる処理を形ごとに作る
        let operands = |taker: &'static str| {
            move |role: &dyn Fn() -> String, atom: Atom| -> Result<(), String> {
                if self.cx.passes(atom, Repr::TObj) {
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
                    if !self.cx.passes(arg, expected) {
                        return Err(format!(
                            "argument {index} of `{}` is {}, but the function takes {}",
                            target.name,
                            self.cx.typed_atom_text(arg),
                            self.cx.repr_text(expected)
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
            // 呼ぶ値と引数はどれも `tobj` と比べる (docs/spec/lir.md の「関数の ABI」)
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
            // 子は `()` を適用する関数の値なので、`apply` の呼ぶ値と同じく `tobj` と比べ、印は比べない
            // (docs/spec/lir.md の「Kind の印の検査」)
            Call::Par {
                left,
                right,
                pair: _,
                at: _,
            } => {
                let uniform = operands("a par");
                uniform(&|| "argument 0".to_string(), *left)?;
                uniform(&|| "argument 1".to_string(), *right)?;
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
                if !self.cx.passes(*callee, Repr::TObj) {
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
                    if !self.cx.passes(arg, repr) {
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
            Call::Direct(_, _) | Call::Apply { .. } | Call::Par { .. } => false,
        }
    }

    /// 呼び出しの結果 (直接の呼び出しなら呼ばれる関数の `ret`、節の呼び出しなら操作の `ret`、`apply` なら `tobj`)
    /// は、束縛する変数と互換である (R7)。`tail` の結果は呼び出し元の `rets` と互換である
    /// (docs/spec/lir.md の「末尾の位置の呼び出し」)。
    /// 結果の印は、束縛する変数か呼び出し元の戻り値の印と互換である (V2)。
    fn check_result(
        &self,
        call: &Call,
        results: &[(Repr, Mark)],
        receiver: Receiver,
    ) -> Result<(), String> {
        let rets = |results: &[(Repr, Mark)]| {
            rets_text(&self.program.layouts, &self.program.structs, results)
        };
        match receiver {
            Receiver::Bound(var) => {
                let repr = self.function.repr(var);
                let [(result, result_mark)] = *results else {
                    return Err(format!(
                        "`{}` ({}) is bound to {}, which returns {}",
                        self.cx.name(var),
                        self.cx.repr_text(repr),
                        self.call_text(call, false),
                        rets(results)
                    ));
                };
                self.check_bound(call, var, result, result_mark)
            }
            Receiver::Many(vars) => {
                if vars.len() != results.len() {
                    return Err(format!(
                        "{} values are bound to {}, which returns {}",
                        vars.len(),
                        self.call_text(call, false),
                        rets(results)
                    ));
                }
                for (&var, &(result, result_mark)) in vars.iter().zip(results) {
                    self.check_bound(call, var, result, result_mark)?;
                }
                Ok(())
            }
            Receiver::Tail => {
                // 末尾呼び出しの結果はそのまま呼び出し元の結果になるので、並びも Repr も `rets` と合う
                // (docs/spec/lir.md の「末尾の位置の呼び出し」)
                let rets = &self.function.rets;
                let fits = results.len() == rets.len()
                    && results
                        .iter()
                        .zip(rets)
                        .all(|(&(result, _), &(ret, _))| self.cx.compatible(result, ret));
                if !fits {
                    return Err(format!(
                        "{} returns {}, but this function returns {}",
                        self.call_text(call, true),
                        rets_text(&self.program.layouts, &self.program.structs, results),
                        rets_text(&self.program.layouts, &self.program.structs, rets)
                    ));
                }
                for (&(_, result_mark), &(_, ret_mark)) in results.iter().zip(rets) {
                    // 呼び出し元を "this function" と書き、呼ばれる側を指す "the function" と分ける
                    if !Mark::passes_to(result_mark, ret_mark) {
                        return Err(format!(
                            "{} returns {}, but this function returns {}",
                            self.call_text(call, true),
                            result_mark.name(),
                            ret_mark.name()
                        ));
                    }
                }
                Ok(())
            }
        }
    }

    /// 呼び出しの結果の1つの値を束縛する変数は、Repr が互換で、印も互換である。
    fn check_bound(
        &self,
        call: &Call,
        var: VarId,
        result: Repr,
        result_mark: Mark,
    ) -> Result<(), String> {
        let repr = self.function.repr(var);
        if !self.cx.compatible(repr, result) {
            return Err(format!(
                "`{}` ({}) is bound to {}, which returns {}",
                self.cx.name(var),
                self.cx.repr_text(repr),
                self.call_text(call, false),
                self.cx.repr_text(result)
            ));
        }
        self.cx
            .bound_mark(var, result_mark, || self.call_text(call, false))
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
            Call::Par { .. } => ("a", "par".to_string()),
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
        Call::Par { .. } => "a tail par",
    }
}

/// 所有の多重集合を、参照の数だけ変数を並べた昇順の列にする。
fn flatten(owned: &Owned) -> Vec<VarId> {
    owned
        .iter()
        .flat_map(|(&var, &count)| std::iter::repeat_n(var, count as usize))
        .collect()
}
