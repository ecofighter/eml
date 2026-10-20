//! handler への特殊化 (docs/spec/core-ir.md の「handler への特殊化」)。本体が既知の関数になった `handle` を、経路の
//! 関数の handler ごとの複製の呼び出しに替える。形は handler の節の形で決まり、2つある (`Form`)。
//!
//! - 状態を渡す形 (`passing`): どの操作も再開し、どの節もすぐに再開する handler の形。経路は、末尾かどうかによらず
//!   `yields` の関数の直接の呼び出しでたどる。複製は状態を変数で持ち回り、`perform` には節の本体を展開して、その
//!   `resume` を `perform` の後ろのブロックへのジャンプにする。`return` の節は handle の地点で呼ぶ
//! - 末尾の経路の形: それ以外の handler の形。経路は末尾の呼び出しだけでたどる。複製の中の `perform` は、すぐに
//!   再開する節なら節の本体をその場に展開した形にする。それ以外の節なら、継続の関数の閉包を `k` として節の写し
//!   `<g>{k}` を末尾で呼ぶ形にし、`never` の操作なら、後ろを捨てて節の関数を末尾で呼ぶ形にする
//!
//! 複製と継続の関数は、`specialize` と最初の `yields` の付け直しの後のコードから作る。そのコードは名前ごとに決まって
//! いるので、どの handle を特殊化するかも、複製のコードも、handle を含む関数と経路の関数のコードだけで決まる
//! (docs/spec/vm.md の「追記」)。

mod passing;
mod piece;

use std::collections::{BTreeSet, HashMap};

use super::blocks;
use super::known::{Wrapper, fresh, known_closures, wrapper};
use super::specialize::limited;
use crate::liveness::{insert_var, live_after_term, step_back};
use crate::tail::tail_position_call;
use crate::{Call, CoreFn, Program, Rhs, Stmt, Term};
use eml_lir::{Atom, BlockId, FnIdx, LayoutId, Mark, OperationInfo, Repr, VarId, VarInfo};

use passing::Returned;

/// `let x = handle …` の文で、本体が既知の閉包か関数の値であるもの。
#[derive(Debug, Clone, Copy)]
struct Site {
    owner: FnIdx,
    block: BlockId,
    stmt: usize,
}

/// 節の関数の形。Core IR の形で決め、`fn` と `ctl` の書き方には依らない
/// (docs/spec/core-ir.md の「handler の節の形」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClauseForm {
    /// `k` の使いが、末尾の位置の `mask` のない `resume k(v, s)` だけで、どの `return` もその結果を返す節。
    Resuming,
    /// それ以外の節。`k` を値として使う節、`k` を末尾でなく呼ぶ節、`k` を使わない節、`never` の操作の節、本体に
    /// `handle` を持つ節である。再開する操作では、`perform` の地点から節の写し `<g>{k}` を末尾で呼び、`k` には継続の
    /// 関数の閉包を渡す。`never` の操作の節は `k` を受けないので、写さずに節の関数 g そのものを末尾で呼ぶ。
    Other,
}

impl ClauseForm {
    /// 本体に `handle` を持つ節は展開しない。展開した所ごとにその `handle` が写り、節の関数がちょうど1つの `handle`
    /// に属するという EP の前提 (`crates/eml_core_ir/src/evidence/handlers.rs` の `entries`) に反するためである。
    fn of(clause: &CoreFn, operation: &OperationInfo) -> ClauseForm {
        if !operation.resumable {
            return ClauseForm::Other;
        }
        let params = clause.params();
        let k = params[params.len() - 2];
        let mut returns = 0;
        for block in &clause.blocks {
            let handles = block.stmts.iter().any(|stmt| {
                matches!(
                    stmt,
                    Stmt::Let {
                        var: _,
                        rhs: Rhs::Call {
                            call: Call::Handle { .. },
                            mask: _,
                        },
                    }
                )
            });
            if handles {
                return ClauseForm::Other;
            }
            if let Term::Return(_) = block.term {
                let resumed = tail_position_call(block).is_some_and(|(at, call)| {
                    matches!(call, Call::Resume { k: Atom::Var(var), .. } if *var == k)
                        && matches!(&block.stmts[at], Stmt::Let {
                            var: _,
                            rhs: Rhs::Call { call: _, mask },
                        } if mask.is_empty())
                });
                if !resumed {
                    return ClauseForm::Other;
                }
                returns += 1;
            }
        }
        let mut uses = 0;
        let mut count = |atom: Atom| uses += usize::from(atom == Atom::Var(k));
        for block in &clause.blocks {
            block
                .stmts
                .iter()
                .for_each(|stmt| stmt.for_each_atom(&mut count));
            block.term.for_each_atom(&mut count);
        }
        if uses == returns {
            ClauseForm::Resuming
        } else {
            ClauseForm::Other
        }
    }
}

/// 特殊化の形。handler の節の形だけで決まるので、1つの handle はいつも1つの形になる
/// (docs/spec/core-ir.md の「handler への特殊化」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Form {
    /// 経路を末尾の呼び出しでたどり、`perform` ごとに続きを継続の関数に切り出す形 (S13e-2)。
    Tail,
    /// どの操作も再開し、どの節もすぐに再開する handler の形。経路を末尾かどうかによらずたどり、状態を変数で持ち回る
    /// (`passing`)。
    Passing,
}

/// 再開する操作の、それ以外の節の `k` の出現が、`resume k(..)` の呼ばれる値、`k` を包む `cont$` か `cont$state` の
/// 捕獲 (`stateful` と合う方)、`drop k` だけか。この3つは、`k` を継続の関数の閉包に替えた写し (`piece::clause_with_k`)
/// でもそのまま書ける。生の継続を入れ子の関数が捕獲する形は、その関数の `resume` も書き換えることになるので扱わない。
fn keeps_k(program: &Program, clause: &CoreFn, stateful: bool) -> bool {
    let params = clause.params();
    let k = Atom::Var(params[params.len() - 2]);
    let mut uses = 0;
    let mut fitting = 0;
    for block in &clause.blocks {
        for stmt in &block.stmts {
            stmt.for_each_atom(|atom| uses += usize::from(atom == k));
            let Stmt::Let { var: _, rhs } = stmt else {
                continue;
            };
            fitting += usize::from(match rhs {
                Rhs::Call {
                    call: Call::Resume { k: callee, .. },
                    mask: _,
                } => *callee == k,
                Rhs::MakeClosure(function, captures) => {
                    captures[..] == [k] && resumes(program, *function) == Some(stateful)
                }
                Rhs::Drop(atom) => *atom == k,
                _ => false,
            });
        }
        block
            .term
            .for_each_atom(|atom| uses += usize::from(atom == k));
    }
    uses == fitting
}

/// `function` が生の継続を包む関数 (`cont$` か `cont$state`) なら、状態つきの方か。
fn resumes(program: &Program, function: FnIdx) -> Option<bool> {
    match wrapper(program.function(function)) {
        Some(Wrapper::Resume { stateful }) => Some(stateful),
        _ => None,
    }
}

/// 特殊化する handle。節と `return` の節の捕獲は、handle を含む関数の値である。
struct Handler {
    site: Site,
    effect: u32,
    init: Atom,
    body: KnownFn,
    /// 操作の順の節と、最後に `return` の節。
    clauses: Vec<KnownFn>,
    /// handle のためだけに作られた `closure` の変数。handle を消すと使われなくなる。
    closures: Vec<VarId>,
    /// 操作の順の節の形。
    forms: Vec<ClauseForm>,
    form: Form,
}

impl Handler {
    /// 状態のない handler の `init` は `()` である (docs/spec/core-ir.md)。`from ()` の状態つきの handler も、状態の
    /// ない handler と同じに扱う。状態の値が `()` なので、渡しても省いても同じである。
    fn stateful(&self) -> bool {
        self.init != Atom::Unit
    }
}

/// handle に渡された関数と捕獲。
#[derive(Clone)]
struct KnownFn {
    function: FnIdx,
    captures: Vec<Atom>,
}

/// 複製が受ける handler の形。複製の名前の鍵は `return` の節の関数 (`ret`) である。translate は handle ごとに
/// `return` の節を作り、S13e-1 は持ち主の名前を鍵に入れて複製するので、`ret` が handle を1つに決める。
#[derive(Clone)]
struct Shape {
    effect: u32,
    /// 操作の順の節と、最後に `return` の節の関数と、その捕獲の数。
    clauses: Vec<(FnIdx, usize)>,
    /// 操作の順の節の形。
    forms: Vec<ClauseForm>,
    stateful: bool,
    form: Form,
}

impl Shape {
    fn ret(&self) -> FnIdx {
        self.clauses
            .last()
            .expect("a handler has a return clause")
            .0
    }
}

/// handle を特殊化する。handle を含む関数はその場で書き換え、複製と継続の関数は後ろに足す。`translated` は、
/// `yields` の印を付け直す前の、関数ごとの印である (`path`)。
pub(crate) fn handlers(program: &mut Program, translated: &[bool]) {
    let count = program.functions.len();
    let handlers: Vec<Handler> = (0..count)
        .flat_map(|owner| sites(program, FnIdx(owner as u32)))
        .filter_map(|site| eligible(program, translated, site))
        .collect();
    if handlers.is_empty() {
        return;
    }
    // 組の配置は、複製が `program` を借りる前に足しておく
    let pair = handlers
        .iter()
        .any(|handler| handler.form == Form::Passing && handler.stateful())
        .then(|| passing::pair_layout(program));
    let mut copies = Copies {
        program,
        table: HashMap::new(),
        clause_copies: HashMap::new(),
        created: Vec::new(),
        pending: Vec::new(),
        pair,
    };
    let calls: Vec<FnIdx> = handlers
        .iter()
        .map(|handler| {
            let shape = Shape {
                effect: handler.effect,
                clauses: handler
                    .clauses
                    .iter()
                    .map(|clause| (clause.function, clause.captures.len()))
                    .collect(),
                forms: handler.forms.clone(),
                stateful: handler.stateful(),
                form: handler.form,
            };
            copies.copy(handler.body.function, &shape)
        })
        .collect();
    while let Some((index, function, shape)) = copies.pending.pop() {
        copies.build(index, function, &shape);
    }
    let created = std::mem::take(&mut copies.created);
    // 状態を渡す形の地点は文が増えるので、関数ごとに後ろの地点から置き換え、前の地点の位置を保つ
    let mut rewrites: Vec<(Site, Vec<Stmt>)> = handlers
        .iter()
        .zip(calls)
        .map(|(handler, copy)| (handler.site, site_stmts(program, handler, copy, pair)))
        .collect();
    rewrites.sort_by_key(|(site, _)| std::cmp::Reverse((site.owner.0, site.block.0, site.stmt)));
    for (site, stmts) in rewrites {
        let owner = &mut program.functions[site.owner.0 as usize];
        owner.blocks[site.block.0 as usize]
            .stmts
            .splice(site.stmt..=site.stmt, stmts);
    }
    for handler in &handlers {
        let owner = &mut program.functions[handler.site.owner.0 as usize];
        for block in &mut owner.blocks {
            block.stmts.retain(|stmt| match stmt {
                Stmt::Let {
                    var,
                    rhs: Rhs::MakeClosure(..),
                } => !handler.closures.contains(var),
                _ => true,
            });
        }
    }
    program.functions.extend(
        created
            .into_iter()
            .map(|function| function.expect("every reserved copy is built")),
    );
}

/// handle の地点 `let x = handle …` を置き換える文。末尾の経路の形では、本体の複製の結果がそのまま x になる。
/// 状態を渡す形では、複製は `return` の節を呼ばないので、複製の結果 (`passing::Returned`) を開いてから、ここで
/// `return` の節を結果と状態で呼ぶ。
fn site_stmts(
    program: &mut Program,
    handler: &Handler,
    copy: FnIdx,
    pair: Option<LayoutId>,
) -> Vec<Stmt> {
    let owner = &program.functions[handler.site.owner.0 as usize];
    let Stmt::Let { var: x, rhs: _ } =
        owner.blocks[handler.site.block.0 as usize].stmts[handler.site.stmt]
    else {
        unreachable!("a site is a `let`");
    };
    let (ret, operations) = handler
        .clauses
        .split_last()
        .expect("a handler has a return clause");
    let mut args = handler.body.captures.clone();
    args.push(Atom::Unit);
    let call = |function, args| Rhs::Call {
        call: Call::Direct(function, args),
        mask: Vec::new(),
    };
    if handler.form == Form::Tail {
        args.extend(handler.clauses.iter().flat_map(|clause| &clause.captures));
        if handler.stateful() {
            args.push(handler.init);
        }
        return vec![Stmt::Let {
            var: x,
            rhs: call(copy, args),
        }];
    }
    args.extend(operations.iter().flat_map(|clause| &clause.captures));
    let stateful = handler.stateful();
    if stateful {
        args.push(handler.init);
    }
    let body = program.function(handler.body.function);
    let shape = passing::returned(body, stateful);
    let state = stateful.then(|| passing::state_info(program.function(ret.function)));
    let result = passing::result_info(body, shape, state.as_ref());
    let body_result = VarInfo {
        name: "t".to_string(),
        repr: body.ret,
        mark: body.ret_mark,
    };
    let vars = &mut program.functions[handler.site.owner.0 as usize].vars;
    let mut stmts = Vec::new();
    let (value, state) = match shape {
        Returned::Plain => {
            let value = fresh(vars, body_result);
            stmts.push(Stmt::Let {
                var: value,
                rhs: call(copy, args),
            });
            (Atom::Var(value), Atom::Unit)
        }
        Returned::StateOnly => {
            let state = fresh(vars, state.expect("a state"));
            stmts.push(Stmt::Let {
                var: state,
                rhs: call(copy, args),
            });
            (Atom::Unit, Atom::Var(state))
        }
        Returned::Pair => {
            let received = fresh(vars, result);
            let value = fresh(vars, body_result);
            let state = fresh(vars, state.expect("a state"));
            stmts.push(Stmt::Let {
                var: received,
                rhs: call(copy, args),
            });
            stmts.push(Stmt::Unpack {
                value: received,
                ctor: eml_lir::Ctor {
                    layout: pair.expect("a stateful passing handler has the pair layout"),
                    tag: eml_lir::TUPLE,
                },
                fields: vec![value, state],
            });
            (Atom::Var(value), Atom::Var(state))
        }
    };
    let mut ret_args = ret.captures.clone();
    ret_args.extend([value, state]);
    stmts.push(Stmt::Let {
        var: x,
        rhs: call(ret.function, ret_args),
    });
    stmts
}

/// 関数の中の、本体が既知の handle。
fn sites(program: &Program, owner: FnIdx) -> Vec<Site> {
    let function = program.function(owner);
    let known = known_closures(function);
    let mut sites = Vec::new();
    for (index, block) in function.blocks.iter().enumerate() {
        for (stmt, statement) in block.stmts.iter().enumerate() {
            if let Stmt::Let {
                var: _,
                rhs:
                    Rhs::Call {
                        call: Call::Handle { body, .. },
                        mask: _,
                    },
            } = statement
                && (matches!(body, Atom::Fn(_))
                    || matches!(body, Atom::Var(var) if known.contains_key(var)))
            {
                sites.push(Site {
                    owner,
                    block: BlockId(index as u32),
                    stmt,
                });
            }
        }
    }
    sites
}

/// `site` の handle を特殊化してよいなら、その中身。判断は handle を含む関数と経路の関数のコードだけで決まる。
fn eligible(program: &Program, translated: &[bool], site: Site) -> Option<Handler> {
    let owner = program.function(site.owner);
    let Stmt::Let {
        var: _,
        rhs:
            Rhs::Call {
                call:
                    Call::Handle {
                        effect,
                        init,
                        body,
                        clauses,
                        ret,
                    },
                mask,
            },
    } = &owner.blocks[site.block.0 as usize].stmts[site.stmt]
    else {
        unreachable!("a site is a handle");
    };
    if !mask.is_empty() {
        return None;
    }
    let known = known_closures(owner);
    let mut uses: HashMap<VarId, usize> = HashMap::new();
    for block in &owner.blocks {
        let mut count = |atom: Atom| {
            if let Atom::Var(var) = atom {
                *uses.entry(var).or_default() += 1;
            }
        };
        block
            .stmts
            .iter()
            .for_each(|stmt| stmt.for_each_atom(&mut count));
        block.term.for_each_atom(&mut count);
    }
    // 捕獲を複製の引数に開くので、閉包をほかの所でも使うと、`unr` でない捕獲を2か所で消費することになる
    let mut closures = Vec::new();
    let mut resolve = |atom: Atom| match atom {
        Atom::Fn(function) => Some(KnownFn {
            function,
            captures: Vec::new(),
        }),
        Atom::Var(var) if uses.get(&var) == Some(&1) => {
            let closure = known.get(&var)?;
            closures.push(var);
            Some(KnownFn {
                function: closure.function,
                captures: closure.captures.clone(),
            })
        }
        _ => None,
    };
    let body = resolve(*body)?;
    let clauses = clauses
        .iter()
        .chain([ret])
        .map(|&atom| resolve(atom))
        .collect::<Option<Vec<_>>>()?;

    let info = &program.effects[*effect as usize];
    if program.function(body.function).params().len() != body.captures.len() + 1 {
        return None;
    }
    let (ret, operations) = clauses.split_last().expect("a handler has a return clause");
    let ret_fn = program.function(ret.function);
    if ret_fn.params().len() != ret.captures.len() + 2 {
        return None;
    }
    let stateful = *init != Atom::Unit;
    let state = &ret_fn.vars[ret_fn.params()[ret_fn.params().len() - 1].0 as usize];
    let mut forms = Vec::with_capacity(operations.len());
    for (clause, operation) in operations.iter().zip(&info.operations) {
        let function = program.function(clause.function);
        // `never` の操作の節は `k` を受けない (docs/spec/core-ir.md の「`never` の中断」)
        let continuation = usize::from(operation.resumable);
        if function.params().len()
            != clause.captures.len() + operation.params.len() + continuation + 1
        {
            return None;
        }
        // 複製は状態を `return` の節の状態の引数の形で受け、そのまま節に渡す
        let param = &function.vars[function.params()[function.params().len() - 1].0 as usize];
        if stateful && (param.repr, param.mark) != (state.repr, state.mark) {
            return None;
        }
        let form = ClauseForm::of(function, operation);
        // それ以外の節の結果は、そのまま複製の結果になる
        let returns_handled =
            function.ret == ret_fn.ret && function.ret_mark.compatible(ret_fn.ret_mark);
        if form == ClauseForm::Other
            && (!returns_handled || (operation.resumable && !keeps_k(program, function, stateful)))
        {
            return None;
        }
        forms.push(form);
    }
    // すぐに再開する節だけの handler は、経路がすべて末尾の呼び出しでも状態を渡す形にする。1つの handler に2つの
    // 形を持たないためである
    let form = if forms.iter().all(|&form| form == ClauseForm::Resuming) {
        Form::Passing
    } else {
        Form::Tail
    };
    let path = path(program, translated, body.function, *effect, form)?;
    if !performs_fit(program, &path, *effect, operations, &forms) {
        return None;
    }
    if form == Form::Passing
        && !passing::resumes_fit(
            program,
            &path,
            *effect,
            operations,
            stateful.then_some(state),
        )
    {
        return None;
    }
    Some(Handler {
        site,
        effect: *effect,
        init: *init,
        body,
        clauses,
        closures,
        forms,
        form,
    })
}

/// handle の本体から、既知の直接の呼び出しでたどれる `yields` の関数 (経路の関数)。末尾の経路の形 (`Form::Tail`) は
/// 末尾の位置の呼び出しだけを、状態を渡す形は末尾かどうかによらずすべての呼び出しをたどる。経路の条件
/// (docs/spec/core-ir.md の「handler への特殊化」) に反する関数があれば `None` を返す。
/// 経路の外の `perform` が H に届くかは、呼ぶ相手と `apply` の `yields` の印で安全側に判断できる。yield しうる操作を
/// 持つエフェクトの `perform` は、それを含む関数の印に必ず表れるためである (docs/spec/types.md の「row の具体化の
/// 記録」)。すべての操作が `never` のエフェクト (`IO` を含む) は、中断が巻き戻しで安いので対象にしない。
///
/// ただし、付け直した印は再開する操作の `perform` だけを yield の地点に数えるので、`never` の操作だけを起こす関数は
/// 印を失う。扱うエフェクトが `never` の操作を持つときは、呼ぶ相手を translate の印 (`translated`) で見て、印の
/// 立った経路の外の関数を呼ぶ handle を残す。translate の印は関数の row から決まり、エフェクトのラベルは再開する操作を
/// 持てば yield しうるので、その `perform` を見落とさない。印は名前ごとに決まるので、判断も名前ごとに決まる。
fn path(
    program: &Program,
    translated: &[bool],
    body: FnIdx,
    effect: u32,
    form: Form,
) -> Option<Vec<FnIdx>> {
    let info = &program.effects[effect as usize];
    if info.operations.iter().all(|operation| !operation.resumable) {
        return None;
    }
    let aborts = info.operations.iter().any(|operation| !operation.resumable);
    let mut path = vec![body];
    let mut next = 0;
    while let Some(&function) = path.get(next) {
        next += 1;
        for block in &program.function(function).blocks {
            let tail = tail_position_call(block).map(|(at, _)| at);
            for (at, stmt) in block.stmts.iter().enumerate() {
                let Stmt::Let {
                    var: _,
                    rhs: Rhs::Call { call, mask },
                } = stmt
                else {
                    continue;
                };
                if !mask.is_empty() {
                    return None;
                }
                match call {
                    Call::Handle { .. } | Call::Resume { .. } | Call::Clause { .. } => return None,
                    Call::Apply { yields: true, .. } => return None,
                    Call::Direct(target, _) if program.function(*target).yields => {
                        if form == Form::Tail && tail != Some(at) {
                            return None;
                        }
                        if !path.contains(target) {
                            path.push(*target);
                        }
                    }
                    Call::Direct(target, _) if aborts && translated[target.0 as usize] => {
                        return None;
                    }
                    Call::Direct(..) | Call::Apply { .. } | Call::Perform { .. } => {}
                }
            }
        }
    }
    Some(path)
}

/// 経路の関数の H 宛ての `perform` の引数が、節の引数と同じ Repr で、印も受け渡せる。節の本体は引数を置き換えて
/// 展開するので、Repr が違うと、box の挿入が直す呼び出しの境目がないまま、違う Repr の値を使うことになるためである。
/// 展開する節では、引数の印が節の引数の印を超えないことも求める。`unr` の引数の所に `poly` の値を置くと、展開した
/// 本体がその値を何度も使い、`unr` でない値を写すことになるためである (docs/spec/lir.md の「Kind の印の検査」の V1)。
/// それ以外の節は呼び出しで引数を渡すので、印は受け渡せれば足りる。
fn performs_fit(
    program: &Program,
    path: &[FnIdx],
    effect: u32,
    clauses: &[KnownFn],
    forms: &[ClauseForm],
) -> bool {
    path.iter().all(|&function| {
        let function = program.function(function);
        targeted(function, effect).all(|(block, stmt)| {
            let Stmt::Let {
                var: _,
                rhs:
                    Rhs::Call {
                        call: Call::Perform { op, args, .. },
                        mask: _,
                    },
            } = &function.blocks[block.0 as usize].stmts[stmt]
            else {
                unreachable!("a targeted statement is a perform");
            };
            let clause = &clauses[*op as usize];
            let inlined = forms[*op as usize] == ClauseForm::Resuming;
            let operation = &program.effects[effect as usize].operations[*op as usize];
            let params = &program.function(clause.function).params()[clause.captures.len()..];
            let vars = &program.function(clause.function).vars;
            args.len() + usize::from(operation.resumable) + 1 == params.len()
                && args.iter().zip(params).all(|(&arg, &param)| {
                    let info = &vars[param.0 as usize];
                    arg.fits(&function.vars, info.repr)
                        && match arg {
                            Atom::Var(var) if inlined => info.mark.at_least(function.mark(var)),
                            Atom::Var(var) => function.mark(var).compatible(info.mark),
                            _ => true,
                        }
                })
        })
    })
}

/// `effect` の `perform` の位置。ブロックと文の順に並ぶ。
fn targeted(function: &CoreFn, effect: u32) -> impl Iterator<Item = (BlockId, usize)> + '_ {
    function
        .blocks
        .iter()
        .enumerate()
        .flat_map(move |(index, block)| {
            block
                .stmts
                .iter()
                .enumerate()
                .filter_map(move |(at, stmt)| match stmt {
                    Stmt::Let {
                        var: _,
                        rhs:
                            Rhs::Call {
                                call: Call::Perform { effect: e, .. },
                                mask: _,
                            },
                    } if *e == effect => Some((BlockId(index as u32), at)),
                    _ => None,
                })
        })
}

/// 複製の表。複製と継続の関数は、作る前に番号を取っておき、書き換える前のプログラムから作る。
struct Copies<'a> {
    program: &'a Program,
    table: HashMap<(FnIdx, FnIdx), FnIdx>,
    /// 節の関数ごとの、`k` を閉包で受ける写し `<g>{k}`。
    clause_copies: HashMap<FnIdx, FnIdx>,
    /// `program.functions` の後ろに足す関数。番号を取った順に並ぶ。
    created: Vec<Option<CoreFn>>,
    pending: Vec<(FnIdx, FnIdx, Shape)>,
    /// 状態つきの handler を状態を渡す形で特殊化するときの、組の配置 (`passing::pair_layout`)。
    pair: Option<LayoutId>,
}

impl Copies<'_> {
    fn reserve(&mut self) -> FnIdx {
        self.created.push(None);
        FnIdx((self.program.functions.len() + self.created.len() - 1) as u32)
    }

    fn place(&mut self, index: FnIdx, function: CoreFn) {
        let slot = index.0 as usize - self.program.functions.len();
        self.created[slot] = Some(function);
    }

    /// 経路の関数 `function` の、handler `shape` の複製の番号。
    fn copy(&mut self, function: FnIdx, shape: &Shape) -> FnIdx {
        let key = (function, shape.ret());
        if let Some(&copy) = self.table.get(&key) {
            return copy;
        }
        let copy = self.reserve();
        self.table.insert(key, copy);
        self.pending.push((copy, function, shape.clone()));
        copy
    }

    /// 節の関数 `clause` の、`k` を閉包で受ける写し `<g>{k}` の番号。節の関数はちょうど1つの handle に属するので、
    /// 写しの鍵は節の関数だけで、状態つきかどうかもそれで決まる。
    fn clause_copy(&mut self, clause: FnIdx, stateful: bool) -> FnIdx {
        if let Some(&copy) = self.clause_copies.get(&clause) {
            return copy;
        }
        let copy = self.reserve();
        self.clause_copies.insert(clause, copy);
        let program = self.program;
        let function = program.function(clause);
        let name = limited(&function.name, format!("{}{{k}}", function.name));
        let code = piece::clause_with_k(function, name, stateful, |wrapper| {
            resumes(program, wrapper).is_some()
        });
        self.place(copy, code);
        copy
    }

    /// `<f>{H=<ret>}`。上限を超えた名前は、S13e-1 の複製と同じくハッシュの形にする。
    fn name(&self, function: FnIdx, shape: &Shape) -> String {
        let name = &self.program.function(function).name;
        let ret = &self.program.function(shape.ret()).name;
        limited(name, format!("{name}{{H={ret}}}"))
    }

    /// 複製 `f{H}` を作る。形は handler の形で決まる。
    fn build(&mut self, index: FnIdx, function: FnIdx, shape: &Shape) {
        match shape.form {
            Form::Tail => self.build_tail(index, function, shape),
            Form::Passing => passing::build(self, index, function, shape),
        }
    }

    /// 末尾の経路の形の複製 `f{H}` を作る。引数は f の引数、節と `return` の節の捕獲 (節の順)、状態つきなら状態の
    /// 順にする。
    ///
    /// H 宛ての `perform` は、どれもそこで複製を終える。再開する操作では、`perform` より後ろを継続の関数 `f{H}$hN`
    /// に切り出す。すぐに再開する節は本体を展開して、その `resume` を `f{H}$hN` の末尾呼び出しにする。それ以外の節は、
    /// `f{H}$hN` の閉包を `k` として節の写し `<g>{k}` に渡し、末尾で呼ぶ。`never` の操作は後ろを捨て、節の関数を末尾で
    /// 呼ぶ。状態をいつも関数の引数で渡すので、ブロックの間で状態を持ち回る必要がなく、組も確保しない。展開した節の
    /// 費用は `perform` ごとの末尾呼び出し1回である。
    ///
    /// `perform` は後ろから順に書き換える。前の `perform` の続きを切り出すときには、その中の後ろの `perform` がすでに
    /// 展開してあるので、継続の関数も同じ番号の継続の関数を呼ぶ。N は f の中の H 宛ての `perform` の、ブロックと文の
    /// 順の通し番号である。接尾辞を EP の `$kN` と分けるのは、`f{H}` がほかのエフェクトで yield するとき、EP が
    /// `f{H}$kN` を作り、名前が重なるためである。継続の関数は同じ複製の中からだけ、直接呼ばれるか値として使われる。
    /// どちらになるかは `perform` の操作の節の形で決まり、節の形は名前で決まる。`k` として渡す継続の関数は値として
    /// 生まれるので内部の関数にし、末尾で呼ぶだけの継続の関数は内部の関数にしない (docs/spec/lir.md の「一様な関数」)。
    ///
    /// 続きは合流のブロックで止め、合流のブロック J からの続きは合流のブロックの関数 `f{H}$hjJ` にする
    /// (`piece::region`)。J は f でのブロックの番号である。接尾辞を EP の `$jJ` と分けるのも、`$hN` と同じ理由で
    /// ある。生存はブロックを後ろから1回だけたどって求め、`perform` はそのたどる途中で書き換える。辺はすべて番号の
    /// 大きいブロックへ向かうので、`perform` の後ろと合流のブロックの生存は、そこへ着いたときには書き換えた後のコード
    /// で決まっている。`perform` ごとに続きを末尾までたどると、`perform` の数の2乗の手間になるためである。
    fn build_tail(&mut self, index: FnIdx, function: FnIdx, shape: &Shape) {
        let program = self.program;
        let ret = program.function(shape.ret());
        let mut code = program.function(function).clone();
        code.name = self.name(function, shape);
        code.internal = false;
        code.ret = ret.ret;
        code.ret_mark = ret.ret_mark;

        let mut captures: Vec<Vec<Atom>> = Vec::new();
        for &(clause, count) in &shape.clauses {
            let clause = program.function(clause);
            let vars = clause.params()[..count]
                .iter()
                .map(|&param| {
                    Atom::Var(fresh(&mut code.vars, clause.vars[param.0 as usize].clone()))
                })
                .collect();
            captures.push(vars);
        }
        let state = shape.stateful.then(|| {
            let param = ret.params()[ret.params().len() - 1];
            fresh(&mut code.vars, ret.vars[param.0 as usize].clone())
        });
        let all_captures: Vec<Atom> = captures.iter().flatten().copied().collect();
        let params = &mut code.blocks[0].params;
        params.extend(all_captures.iter().map(|atom| match atom {
            Atom::Var(var) => *var,
            _ => unreachable!("captures are parameters"),
        }));
        params.extend(state);
        let returned = VarInfo {
            name: "t".to_string(),
            repr: ret.ret,
            mark: ret.ret_mark,
        };

        let performs: Vec<(BlockId, usize)> = targeted(&code, shape.effect).collect();
        for block in 0..code.blocks.len() {
            let Term::Return(value) = code.blocks[block].term else {
                continue;
            };
            let tail = match tail_position_call(&code.blocks[block]) {
                Some((at, Call::Direct(target, args))) if program.function(*target).yields => {
                    Some((at, *target, args.clone()))
                }
                _ => None,
            };
            let var = fresh(&mut code.vars, returned.clone());
            let rhs = match tail {
                Some((at, target, mut args)) => {
                    code.blocks[block].stmts.truncate(at);
                    args.extend(all_captures.iter().copied());
                    args.extend(state.map(Atom::Var));
                    Call::Direct(self.copy(target, shape), args)
                }
                None => {
                    let mut args = captures.last().expect("a return clause").clone();
                    args.extend([value, state.map_or(Atom::Unit, Atom::Var)]);
                    Call::Direct(shape.ret(), args)
                }
            };
            let block = &mut code.blocks[block];
            block.stmts.push(Stmt::Let {
                var,
                rhs: Rhs::Call {
                    call: rhs,
                    mask: Vec::new(),
                },
            });
            block.term = Term::Return(Atom::Var(var));
        }

        let mut copy = Rewrite {
            joins: piece::joins(&code.blocks),
            live_in: vec![Vec::new(); code.blocks.len()],
            code,
            captures,
            all_captures,
            state,
            join_functions: HashMap::new(),
            pending: Vec::new(),
        };
        let mut next = performs.len();
        for block in (0..copy.joins.len()).rev() {
            let block = BlockId(block as u32);
            let mut live = copy.live_out(block);
            let mut end = copy.code.block(block).stmts.len();
            while next > 0 && performs[next - 1].0 == block {
                next -= 1;
                let stmt = performs[next].1;
                let source = copy.code.block(block);
                for later in source.stmts[stmt + 1..end].iter().rev() {
                    step_back(later, &mut live);
                }
                let Stmt::Let { var: result, .. } = source.stmts[stmt] else {
                    unreachable!("a targeted statement is a `let`");
                };
                live.remove(&result);
                if let Some(state) = state {
                    live.remove(&state);
                }
                self.perform(
                    &mut copy,
                    shape,
                    next,
                    (block, stmt),
                    live.into_iter().collect(),
                );
                live = copy.live_out(block);
                for later in copy.code.block(block).stmts[stmt..].iter().rev() {
                    step_back(later, &mut live);
                }
                end = stmt;
            }
            for earlier in copy.code.block(block).stmts[..end].iter().rev() {
                step_back(earlier, &mut live);
            }
            copy.live_in[block.0 as usize] = live.into_iter().collect();
        }
        while let Some(join) = copy.pending.pop() {
            let mut params = copy.code.block(join).params.clone();
            params.extend(copy.join_live(join));
            params.extend(state);
            let mut body = self.region(&mut copy, (join, 0), params);
            body.name = format!("{}$hj{}", copy.code.name, join.0);
            let function = copy.join_functions[&join];
            self.place(function, body);
        }
        blocks::prune(&mut copy.code);
        self.place(index, copy.code);
    }

    /// `copy` の `block` の `stmt` 番目の H 宛ての `perform` を書き換える。`number` はその通し番号で、`live` は
    /// `perform` の後で生きている変数 (結果と状態を除き、番号の昇順) である。
    fn perform(
        &mut self,
        copy: &mut Rewrite,
        shape: &Shape,
        number: usize,
        (block, stmt): (BlockId, usize),
        live: Vec<VarId>,
    ) {
        let program = self.program;
        let Stmt::Let {
            var: result,
            rhs:
                Rhs::Call {
                    call: Call::Perform { op, args, .. },
                    mask: _,
                },
        } = &copy.code.block(block).stmts[stmt]
        else {
            unreachable!("a targeted statement is a perform");
        };
        let result = *result;
        let op = *op as usize;
        let (clause, _) = shape.clauses[op];
        let mut clause_args = copy.captures[op].clone();
        clause_args.extend(args.iter().copied());
        let state_atom = copy.state.map_or(Atom::Unit, Atom::Var);
        let operation = &program.effects[shape.effect as usize].operations[op];
        if !operation.resumable {
            // `never` の操作: 節の関数を末尾で呼び、`perform` より後ろは捨てる。中断で巻き戻したフレームの値は、
            // ここで手放す
            clause_args.push(state_atom);
            let released = dead(&copy.code, &copy.all_captures, &live, &clause_args);
            let call = Call::Direct(clause, clause_args);
            end(
                &mut copy.code,
                (block, stmt),
                released,
                None,
                call,
                program.function(clause),
            );
            return;
        }
        let continuation = self.reserve();
        let mut params = live.clone();
        params.push(result);
        params.extend(copy.state);
        let mut body = self.region(copy, (block, stmt + 1), params);
        body.name = format!("{}$h{number}", copy.code.name);
        body.internal = shape.forms[op] == ClauseForm::Other;
        self.place(continuation, body);
        let live: Vec<Atom> = live.into_iter().map(Atom::Var).collect();
        if shape.forms[op] == ClauseForm::Other {
            // `k` を値として使う節: 続きの閉包を `k` にして、節の写しを末尾で呼ぶ。閉包の印は節の `k` の印で、
            // `once` の操作なら `lin` になる
            let source = program.function(clause);
            let k = source.vars[source.params()[source.params().len() - 2].0 as usize].mark;
            debug_assert!(
                live.iter().all(|&atom| match atom {
                    Atom::Var(var) => k.at_least(copy.code.mark(var)),
                    _ => true,
                }),
                "a `multi` continuation captures only `unr` values"
            );
            // 捕獲のない閉包は関数の値で書く (docs/spec/core-ir.md)
            let closure = (!live.is_empty()).then(|| {
                let var = fresh(
                    &mut copy.code.vars,
                    VarInfo {
                        name: "k".to_string(),
                        repr: Repr::TObj,
                        mark: k,
                    },
                );
                (var, Rhs::MakeClosure(continuation, live.clone()))
            });
            let value = closure
                .as_ref()
                .map_or(Atom::Fn(continuation), |&(var, _)| Atom::Var(var));
            clause_args.extend([value, state_atom]);
            let mut passed = live;
            passed.extend(clause_args.iter().copied());
            let released = dead(&copy.code, &copy.all_captures, &[], &passed);
            // 写しの戻り値は節の関数の戻り値のままである
            let call = Call::Direct(self.clause_copy(clause, shape.stateful), clause_args);
            end(
                &mut copy.code,
                (block, stmt),
                released,
                closure,
                call,
                program.function(clause),
            );
            return;
        }
        // 節の捕獲は、展開した本体と、継続の関数に渡す生きている変数の両方に現れうる。同じ値を2か所で使ってよいのは、
        // 操作の節の捕獲が `Unr` に限られるためである (E3001、docs/spec/linearity.md)
        debug_assert!(
            copy.captures[op].iter().all(|&atom| match atom {
                Atom::Var(var) => copy.code.mark(var) == Mark::Unr,
                _ => true,
            }),
            "an operation clause captures only `unr` values"
        );
        let stateful = shape.stateful;
        let clause = program.function(clause);
        let base = copy.code.blocks.len();
        piece::inline_clause(
            &mut copy.code,
            (block, stmt),
            clause,
            &clause_args,
            state_atom,
            |value, state| {
                let mut args = live.clone();
                args.push(value);
                if stateful {
                    args.push(state);
                }
                Rhs::Call {
                    call: Call::Direct(continuation, args),
                    mask: Vec::new(),
                }
            },
        );
        // 足したブロックは合流のブロックにしない。この `perform` の地点からだけ届くので1つの続きにしか入らず、節が
        // 状態を使った後なので、状態を渡す合流のブロックの関数の形にも合わないためである。H 宛ての `perform` を
        // 含まないので、生存はここで決めてよい
        copy.joins.resize(copy.code.blocks.len(), false);
        copy.live_in.resize(copy.code.blocks.len(), Vec::new());
        for added in (base..copy.code.blocks.len()).rev() {
            let added = BlockId(added as u32);
            let mut live = copy.live_out(added);
            for stmt in copy.code.block(added).stmts.iter().rev() {
                step_back(stmt, &mut live);
            }
            copy.live_in[added.0 as usize] = live.into_iter().collect();
        }
    }

    /// `copy` の `block` の `start` 番目の文からの続きを関数にする (`piece::region`)。合流のブロック J への `jump` は、
    /// J の関数を、`jump` の実引数、J の入口で生きている変数、状態の順で末尾で呼ぶ形にする。
    fn region(&mut self, copy: &mut Rewrite, at: (BlockId, usize), params: Vec<VarId>) -> CoreFn {
        let Rewrite {
            code,
            joins,
            state,
            join_functions,
            pending,
            ..
        } = copy;
        let live_in = &copy.live_in;
        piece::region(code, at, params, joins, |join, args| {
            let function = *join_functions.entry(join).or_insert_with(|| {
                pending.push(join);
                self.reserve()
            });
            let mut args = args.to_vec();
            args.extend(join_live(code, live_in, join, *state).map(Atom::Var));
            args.extend(state.map(Atom::Var));
            Call::Direct(function, args)
        })
    }
}

/// 書き換えている複製 `f{H}` と、そのブロックの表。
struct Rewrite {
    code: CoreFn,
    /// 節の順の、節の捕獲の引数。最後が `return` の節である。
    captures: Vec<Vec<Atom>>,
    all_captures: Vec<Atom>,
    state: Option<VarId>,
    /// ブロックごとの、合流のブロックか (`piece::joins`)。
    joins: Vec<bool>,
    /// ブロックごとの入口の生存集合。後ろのブロックから、書き換えた後のコードで求める。
    live_in: Vec<Vec<VarId>>,
    /// 番号を取った合流のブロックの関数 `f{H}$hjJ`。
    join_functions: HashMap<BlockId, FnIdx>,
    /// 番号を取ったが、中身をまだ作っていない合流のブロック。
    pending: Vec<BlockId>,
}

impl Rewrite {
    /// ブロックの終端の前で生きている変数。
    fn live_out(&self, block: BlockId) -> BTreeSet<VarId> {
        let term = &self.code.block(block).term;
        let mut live = live_after_term(&self.code, term, &self.live_in);
        term.for_each_atom(|atom| insert_var(&mut live, atom));
        live
    }

    fn join_live(&self, join: BlockId) -> impl Iterator<Item = VarId> + '_ {
        join_live(&self.code, &self.live_in, join, self.state)
    }
}

/// 合流のブロック J の入口で生きている変数から、J の引数と状態を除いたもの。番号の昇順に並ぶ。
fn join_live<'a>(
    code: &'a CoreFn,
    live_in: &'a [Vec<VarId>],
    join: BlockId,
    state: Option<VarId>,
) -> impl Iterator<Item = VarId> + 'a {
    let params = &code.block(join).params;
    live_in[join.0 as usize]
        .iter()
        .copied()
        .filter(move |var| !params.contains(var) && Some(*var) != state)
}

/// `perform` の地点で複製を終える。ブロックの `perform` から後ろを捨て、`released` を `drop` し、`closure` を作って
/// から `call` を末尾で呼ぶ。呼ぶ相手は節の関数かその写しで、結果 (`returned` の形) はそのまま複製の結果になる。
fn end(
    code: &mut CoreFn,
    (block, stmt): (BlockId, usize),
    released: Vec<VarId>,
    closure: Option<(VarId, Rhs)>,
    call: Call,
    returned: &CoreFn,
) {
    let unit = VarInfo {
        name: "u".to_string(),
        repr: Repr::Unit,
        mark: Mark::Unr,
    };
    let mut stmts = Vec::new();
    for var in released {
        stmts.push(Stmt::Let {
            var: fresh(&mut code.vars, unit.clone()),
            rhs: Rhs::Drop(Atom::Var(var)),
        });
    }
    stmts.extend(closure.map(|(var, rhs)| Stmt::Let { var, rhs }));
    let var = fresh(
        &mut code.vars,
        VarInfo {
            name: "t".to_string(),
            repr: returned.ret,
            mark: returned.ret_mark,
        },
    );
    stmts.push(Stmt::Let {
        var,
        rhs: Rhs::Call {
            call,
            mask: Vec::new(),
        },
    });
    let host = &mut code.blocks[block.0 as usize];
    host.stmts.truncate(stmt);
    host.stmts.extend(stmts);
    host.term = Term::Return(Atom::Var(var));
}

/// 複製を `perform` の地点で終えるときに手放す、`unr` でない値。節と `return` の節の捕獲と、`perform` の後で
/// 生きていた変数 (`live`) のうち、末尾の呼び出しに渡さないものである。中断と同じく、もとの意味でも handle の
/// フレームの値はそこで捨てる (docs/spec/core-ir.md の「`never` の中断」)。`unr` の値は Perceus が手放すが、`unr` で
/// ない値には `decref` を出せないので、ここで `drop` にする (docs/spec/lir.md の「Kind の印の検査」の V1)。
fn dead(code: &CoreFn, captures: &[Atom], live: &[VarId], passed: &[Atom]) -> Vec<VarId> {
    let mut released: Vec<VarId> = Vec::new();
    let candidates = captures
        .iter()
        .filter_map(|atom| match atom {
            Atom::Var(var) => Some(*var),
            _ => None,
        })
        .chain(live.iter().copied());
    for var in candidates {
        if code.mark(var) != Mark::Unr
            && !passed.contains(&Atom::Var(var))
            && !released.contains(&var)
        {
            released.push(var);
        }
    }
    released
}
