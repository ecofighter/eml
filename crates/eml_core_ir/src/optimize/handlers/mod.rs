//! handler への特殊化 (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「S13e-2 handler への
//! 特殊化」)。本体が既知の関数になった `handle` のうち、`perform` までの経路が末尾呼び出しだけでできたものを、経路の
//! 関数の handler ごとの複製の呼び出しに替える。複製の中の `perform` は、節の本体をその場に展開した形にする。
//!
//! 複製と継続の関数は、`specialize` と最初の `yields` の付け直しの後のコードから作る。そのコードは名前ごとに決まって
//! いるので、どの handle を特殊化するかも、複製のコードも、handle を含む関数と経路の関数のコードだけで決まる
//! (docs/spec/vm.md の「追記」)。

mod piece;

use std::collections::HashMap;

use super::known::{fresh, known_closures};
use super::specialize::limited;
use crate::tail::tail_position_call;
use crate::{Call, CoreFn, Program, Rhs, Stmt, Term};
use eml_lir::{Atom, BlockId, FnIdx, OperationInfo, VarId, VarInfo};

/// `let x = handle …` の文で、本体が既知の閉包か関数の値であるもの。
#[derive(Debug, Clone, Copy)]
struct Site {
    owner: FnIdx,
    block: BlockId,
    stmt: usize,
}

/// 節の関数の形。Core IR の形で決め、`fn` と `ctl` の書き方には依らない
/// (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「節の形」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClauseForm {
    /// `k` の使いが、末尾の位置の `mask` のない `resume k(v, s)` だけで、どの `return` もその結果を返す節。
    Resuming,
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
    stateful: bool,
}

impl Shape {
    fn ret(&self) -> FnIdx {
        self.clauses
            .last()
            .expect("a handler has a return clause")
            .0
    }
}

/// handle を特殊化する。handle を含む関数はその場で書き換え、複製と継続の関数は後ろに足す。
pub(crate) fn handlers(program: &mut Program) {
    let count = program.functions.len();
    let handlers: Vec<Handler> = (0..count)
        .flat_map(|owner| sites(program, FnIdx(owner as u32)))
        .filter_map(|site| eligible(program, site))
        .collect();
    if handlers.is_empty() {
        return;
    }
    let mut copies = Copies {
        program,
        table: HashMap::new(),
        created: Vec::new(),
        pending: Vec::new(),
    };
    let calls: Vec<(FnIdx, Vec<Atom>)> = handlers
        .iter()
        .map(|handler| {
            let shape = Shape {
                effect: handler.effect,
                clauses: handler
                    .clauses
                    .iter()
                    .map(|clause| (clause.function, clause.captures.len()))
                    .collect(),
                stateful: handler.init != Atom::Unit,
            };
            let copy = copies.copy(handler.body.function, &shape);
            let mut args = handler.body.captures.clone();
            args.push(Atom::Unit);
            args.extend(handler.clauses.iter().flat_map(|clause| &clause.captures));
            if shape.stateful {
                args.push(handler.init);
            }
            (copy, args)
        })
        .collect();
    while let Some((index, function, shape)) = copies.pending.pop() {
        copies.build(index, function, &shape);
    }
    let created = std::mem::take(&mut copies.created);
    for (handler, (copy, args)) in handlers.iter().zip(calls) {
        let owner = &mut program.functions[handler.site.owner.0 as usize];
        let Stmt::Let { var: _, rhs } =
            &mut owner.blocks[handler.site.block.0 as usize].stmts[handler.site.stmt]
        else {
            unreachable!("a site is a `let`");
        };
        *rhs = Rhs::Call {
            call: Call::Direct(copy, args),
            mask: Vec::new(),
        };
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
fn eligible(program: &Program, site: Site) -> Option<Handler> {
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
    if program.function(ret.function).params().len() != ret.captures.len() + 2 {
        return None;
    }
    for (clause, operation) in operations.iter().zip(&info.operations) {
        let function = program.function(clause.function);
        if function.params().len() != clause.captures.len() + operation.params.len() + 2
            || ClauseForm::of(function, operation) != ClauseForm::Resuming
        {
            return None;
        }
    }
    let path = path(program, body.function, *effect)?;
    if !performs_fit(program, &path, *effect, operations) {
        return None;
    }
    Some(Handler {
        site,
        effect: *effect,
        init: *init,
        body,
        clauses,
        closures,
    })
}

/// handle の本体から、末尾の位置の既知の直接の呼び出しだけでたどれる `yields` の関数 (経路の関数)。経路の条件
/// (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「対象」) に反する関数があれば `None` を返す。
/// 経路の外の `perform` が H に届くかは、呼ぶ相手と `apply` の `yields` の印で安全側に判断できる。yield しうる操作を
/// 持つエフェクトの `perform` は、それを含む関数の印に必ず表れるためである (docs/spec/types.md の「row の具体化の
/// 記録」)。すべての操作が `never` のエフェクト (`IO` を含む) は、中断が巻き戻しで安いので対象にしない。
fn path(program: &Program, body: FnIdx, effect: u32) -> Option<Vec<FnIdx>> {
    let info = &program.effects[effect as usize];
    if info.operations.iter().all(|operation| !operation.resumable) {
        return None;
    }
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
                        if tail != Some(at) {
                            return None;
                        }
                        if !path.contains(target) {
                            path.push(*target);
                        }
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
fn performs_fit(program: &Program, path: &[FnIdx], effect: u32, clauses: &[KnownFn]) -> bool {
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
            let params = &program.function(clause.function).params()[clause.captures.len()..];
            let vars = &program.function(clause.function).vars;
            args.len() + 2 == params.len()
                && args.iter().zip(params).all(|(&arg, &param)| {
                    let info = &vars[param.0 as usize];
                    arg.fits(&function.vars, info.repr)
                        && match arg {
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
    /// `program.functions` の後ろに足す関数。番号を取った順に並ぶ。
    created: Vec<Option<CoreFn>>,
    pending: Vec<(FnIdx, FnIdx, Shape)>,
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

    /// `<f>{H=<ret>}`。上限を超えた名前は、S13e-1 の複製と同じくハッシュの形にする。
    fn name(&self, function: FnIdx, shape: &Shape) -> String {
        let name = &self.program.function(function).name;
        let ret = &self.program.function(shape.ret()).name;
        limited(name, format!("{name}{{H={ret}}}"))
    }

    /// 複製 `f{H}` を作る。引数は f の引数、節と `return` の節の捕獲 (節の順)、状態つきなら状態の順にする。
    ///
    /// H 宛ての `perform` は、どれもそこで複製を終える。`perform` より後ろは継続の関数 `f{H}$hN` に切り出し、節の
    /// 本体を展開して、その `resume` を `f{H}$hN` の末尾呼び出しにする。状態をいつも関数の引数で渡すので、ブロックの
    /// 間で状態を持ち回る必要がなく、組も確保しない。費用は `perform` ごとの末尾呼び出し1回である。
    ///
    /// `perform` は後ろから順に書き換える。前の `perform` の続きを切り出すときには、その中の後ろの `perform` がすでに
    /// 展開してあるので、継続の関数も同じ番号の継続の関数を呼ぶ。N は f の中の H 宛ての `perform` の、ブロックと文の
    /// 順の通し番号である。接尾辞を EP の `$kN` と分けるのは、`f{H}` がほかのエフェクトで yield するとき、EP が
    /// `f{H}$kN` を作り、名前が重なるためである。継続の関数は同じ複製の中からだけ直接呼ばれるので、内部の関数にする。
    fn build(&mut self, index: FnIdx, function: FnIdx, shape: &Shape) {
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
        let state_atom = state.map_or(Atom::Unit, Atom::Var);
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
                    args.extend([value, state_atom]);
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

        for (number, &(block, stmt)) in performs.iter().enumerate().rev() {
            let continuation = self.reserve();
            let (live, mut cut) = piece::cut(&code, block, stmt, state);
            cut.name = format!("{}$h{number}", code.name);
            let Stmt::Let {
                var: _,
                rhs:
                    Rhs::Call {
                        call: Call::Perform { op, args, .. },
                        mask: _,
                    },
            } = &code.blocks[block.0 as usize].stmts[stmt]
            else {
                unreachable!("a targeted statement is a perform");
            };
            let (clause, _) = shape.clauses[*op as usize];
            let mut clause_args = captures[*op as usize].clone();
            clause_args.extend(args.iter().copied());
            let live: Vec<Atom> = live.into_iter().map(Atom::Var).collect();
            let stateful = shape.stateful;
            piece::inline_clause(
                &mut code,
                (block, stmt),
                program.function(clause),
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
            self.place(continuation, cut);
        }
        piece::prune(&mut code);
        self.place(index, code);
    }
}
