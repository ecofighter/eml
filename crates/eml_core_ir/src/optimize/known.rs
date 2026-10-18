//! 既知の閉包への `apply` の書き換えと、translate の包む関数の命令への戻し
//! (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「書き換え」)。

use std::collections::{HashMap, HashSet};

use crate::{Call, CoreFn, Rhs, Stmt, Term};
use eml_lir::{Atom, Ctor, FnIdx, Mark, MaskItem, Repr, VarId, VarInfo};

/// 呼ぶ相手と、すでに渡した先頭の引数が分かっている関数の値。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KnownClosure {
    pub(crate) function: FnIdx,
    pub(crate) captures: Vec<Atom>,
}

/// 同じ関数の中で `closure` が束縛した変数。関数の値 `&f` は変数を通さずに使う所に現れるので、使う所で
/// `KnownClosure { function: f, captures: vec![] }` とみなす (`resolve`)。
pub(crate) fn known_closures(function: &CoreFn) -> HashMap<VarId, KnownClosure> {
    function
        .blocks
        .iter()
        .flat_map(|block| &block.stmts)
        .filter_map(|stmt| match stmt {
            Stmt::Let {
                var,
                rhs: Rhs::MakeClosure(target, captures),
            } => Some((
                *var,
                KnownClosure {
                    function: *target,
                    captures: captures.clone(),
                },
            )),
            _ => None,
        })
        .collect()
}

/// translate の包む関数 (`op$`、`con$`、`cont$`、`cont$state`) が包んでいる命令。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wrapper {
    Perform {
        effect: u32,
        op: u32,
        resumable: bool,
    },
    Con(Ctor),
    /// 引数は `(k, v)` か、`stateful` なら `(k, v, s)` である。
    Resume {
        stateful: bool,
    },
}

/// 引数をそのままの順で1つの命令に渡して結果を返すだけの内部の関数なら、その命令。名前でなく形で見分ける。
/// 内部の関数に限るのは、トップレベルの関数の呼び出しを命令に開くのはインライン化で、この規則の受け持ちでないため
/// である。
pub(crate) fn wrapper(function: &CoreFn) -> Option<Wrapper> {
    if !function.internal {
        return None;
    }
    let [block] = &function.blocks[..] else {
        return None;
    };
    let [Stmt::Let { var, rhs }] = &block.stmts[..] else {
        return None;
    };
    if block.term != Term::Return(Atom::Var(*var)) {
        return None;
    }
    let params: Vec<Atom> = block.params.iter().map(|&param| Atom::Var(param)).collect();
    match rhs {
        Rhs::Call {
            call:
                Call::Perform {
                    effect,
                    op,
                    resumable,
                    args,
                },
            mask,
        } if mask.is_empty() && *args == params => Some(Wrapper::Perform {
            effect: *effect,
            op: *op,
            resumable: *resumable,
        }),
        Rhs::Con { ctor, args } if *args == params => Some(Wrapper::Con(*ctor)),
        Rhs::Call {
            call: Call::Resume { k, arg, state },
            mask,
        } if mask.is_empty() => match params[..] {
            [pk, pv] if (*k, *arg, *state) == (pk, pv, Atom::Unit) => {
                Some(Wrapper::Resume { stateful: false })
            }
            [pk, pv, ps] if (*k, *arg, *state) == (pk, pv, ps) => {
                Some(Wrapper::Resume { stateful: true })
            }
            _ => None,
        },
        _ => None,
    }
}

/// 書き換えが呼ぶ相手について読むもの。書き換えの間はほかの関数を借りられないので、先に表にしておく。
pub(crate) struct Callee {
    pub(crate) arity: usize,
    pub(crate) ret: Repr,
    pub(crate) ret_mark: Mark,
    pub(crate) wrapper: Option<Wrapper>,
}

impl Callee {
    pub(crate) fn of(function: &CoreFn) -> Self {
        Callee {
            arity: function.params().len(),
            ret: function.ret,
            ret_mark: function.ret_mark,
            wrapper: wrapper(function),
        }
    }
}

/// 変数を使う所の種類。`unr` でない閉包を書き換えてよいかを、使う所をすべて見て決めるために数える。
enum Use {
    /// `apply` の呼ばれる値。引数の数と、`mask` が空でないか。
    Callee {
        args: usize,
        masked: bool,
    },
    Other,
}

/// `function` の中の既知の閉包への `apply` と、包む関数への直接の呼び出しを書き換える。ブロックを番号の順に、文を
/// 前から見るので、書き換えで生まれた `closure` も、それを使う所より先に既知になる。
pub(crate) fn rewrite(function: &mut CoreFn, callees: &[Callee]) {
    let uses = uses(function);
    let mut known = known_closures(function);
    // `unr` でない閉包は経路ごとに1回しか使われない。書き換えない使う所が残ると、捕獲した値を閉包と直接の呼び出しの
    // 2か所で消費し、閉包そのものも手放せなくなる (V1)。そこで、使う所がすべて書き換わるときだけ書き換える。そうすれば
    // 閉包の `closure` は使われなくなり、縮約が消す (docs/spec/lir.md の「縮約」)
    let safe = |var: VarId, closure: &KnownClosure, function: &CoreFn| {
        function.mark(var) == Mark::Unr
            || uses.get(&var).is_some_and(|uses| {
                uses.iter().all(|use_| match *use_ {
                    Use::Callee { args, masked } => rewritable(closure, args, masked, callees),
                    Use::Other => false,
                })
            })
    };
    known.retain(|&var, closure| safe(var, closure, function));
    let mut rewritten = HashSet::new();
    for index in 0..function.blocks.len() {
        let stmts = std::mem::take(&mut function.blocks[index].stmts);
        let mut out = Vec::with_capacity(stmts.len());
        for stmt in stmts {
            let Stmt::Let { var, rhs } = stmt else {
                out.push(stmt);
                continue;
            };
            let Some(replaced) = replace(function, &known, callees, var, &rhs) else {
                out.push(Stmt::Let { var, rhs });
                continue;
            };
            if let Rhs::Call {
                call:
                    Call::Apply {
                        callee: Atom::Var(callee),
                        ..
                    },
                mask: _,
            } = rhs
            {
                rewritten.insert(callee);
            }
            for stmt in replaced {
                if let Stmt::Let {
                    var,
                    rhs: Rhs::MakeClosure(target, captures),
                } = &stmt
                {
                    let closure = KnownClosure {
                        function: *target,
                        captures: captures.clone(),
                    };
                    if safe(*var, &closure, function) {
                        known.insert(*var, closure);
                    }
                }
                out.push(stmt);
            }
        }
        function.blocks[index].stmts = out;
    }
    drop_unused_closures(function, &rewritten);
}

/// 書き換えた閉包のうち、使う所が残らなかったものの `closure` を消す。残すと、`unr` でない閉包の捕獲を閉包と直接の
/// 呼び出しの2か所で消費する IR になり、box の挿入も値として参照された関数に `f$boxed` を作る
/// (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「パスの後に直すもの」)。定義は使う所を
/// 支配し、辺は前へ向かうので、後ろから1回たどれば、消した `closure` だけが捕獲していた、書き換えた閉包も消える。
fn drop_unused_closures(function: &mut CoreFn, rewritten: &HashSet<VarId>) {
    if rewritten.is_empty() {
        return;
    }
    let mut uses = vec![0usize; function.vars.len()];
    for block in &function.blocks {
        let mut count = |atom: Atom| {
            if let Atom::Var(var) = atom {
                uses[var.0 as usize] += 1;
            }
        };
        block
            .stmts
            .iter()
            .for_each(|stmt| stmt.for_each_atom(&mut count));
        block.term.for_each_atom(&mut count);
    }
    for block in function.blocks.iter_mut().rev() {
        let mut kept = Vec::with_capacity(block.stmts.len());
        for stmt in block.stmts.drain(..).rev() {
            if let Stmt::Let {
                var,
                rhs: Rhs::MakeClosure(_, captures),
            } = &stmt
                && rewritten.contains(var)
                && uses[var.0 as usize] == 0
            {
                for capture in captures {
                    if let Atom::Var(capture) = capture {
                        uses[capture.0 as usize] -= 1;
                    }
                }
                continue;
            }
            kept.push(stmt);
        }
        kept.reverse();
        block.stmts = kept;
    }
}

/// 変数ごとの使う所。
fn uses(function: &CoreFn) -> HashMap<VarId, Vec<Use>> {
    let mut uses: HashMap<VarId, Vec<Use>> = HashMap::new();
    let other = |atom: Atom, uses: &mut HashMap<VarId, Vec<Use>>| {
        if let Atom::Var(var) = atom {
            uses.entry(var).or_default().push(Use::Other);
        }
    };
    for block in &function.blocks {
        for stmt in &block.stmts {
            match stmt {
                Stmt::Let {
                    var: _,
                    rhs:
                        Rhs::Call {
                            call:
                                Call::Apply {
                                    callee,
                                    args,
                                    yields: _,
                                    prompt: _,
                                },
                            mask,
                        },
                } => {
                    if let Atom::Var(var) = callee {
                        uses.entry(*var).or_default().push(Use::Callee {
                            args: args.len(),
                            masked: !mask.is_empty(),
                        });
                    }
                    args.iter().for_each(|&arg| other(arg, &mut uses));
                }
                _ => stmt.for_each_atom(|atom| other(atom, &mut uses)),
            }
        }
        block.term.for_each_atom(|atom| other(atom, &mut uses));
    }
    uses
}

/// `closure` に `args` 個の引数を渡す `apply` を書き換えられるか。`replace_apply` と同じ場合分けである。
fn rewritable(closure: &KnownClosure, args: usize, masked: bool, callees: &[Callee]) -> bool {
    let callee = &callees[closure.function.0 as usize];
    let given = closure.captures.len() + args;
    match given.cmp(&callee.arity) {
        std::cmp::Ordering::Less => true,
        std::cmp::Ordering::Equal => !(masked && is_perform(callee.wrapper)),
        std::cmp::Ordering::Greater => !masked,
    }
}

fn is_perform(wrapper: Option<Wrapper>) -> bool {
    matches!(wrapper, Some(Wrapper::Perform { .. }))
}

/// `let var = rhs` を置き換える文。書き換えないなら `None`。
fn replace(
    function: &mut CoreFn,
    known: &HashMap<VarId, KnownClosure>,
    callees: &[Callee],
    var: VarId,
    rhs: &Rhs,
) -> Option<Vec<Stmt>> {
    let Rhs::Call { call, mask } = rhs else {
        return None;
    };
    match call {
        Call::Direct(target, args) => {
            let callee = &callees[target.0 as usize];
            let rhs = instruction(callee.wrapper?, args.clone(), mask)?;
            Some(vec![Stmt::Let { var, rhs }])
        }
        Call::Apply {
            callee,
            args,
            yields,
            prompt,
        } => {
            let closure = match *callee {
                Atom::Fn(target) => KnownClosure {
                    function: target,
                    captures: Vec::new(),
                },
                Atom::Var(callee) => known.get(&callee)?.clone(),
                _ => return None,
            };
            let callee = &callees[closure.function.0 as usize];
            if !rewritable(&closure, args.len(), !mask.is_empty(), callees) {
                return None;
            }
            let mut given = closure.captures;
            given.extend(args.iter().copied());
            if given.len() < callee.arity {
                // 部分適用はエフェクトを起こさないので `mask` を落とす (docs/spec/core-ir.md の「変換の規則」)
                return Some(vec![Stmt::Let {
                    var,
                    rhs: Rhs::MakeClosure(closure.function, given),
                }]);
            }
            let rest = given.split_off(callee.arity);
            let rhs = saturated(closure.function, callee, given, mask);
            if rest.is_empty() {
                return Some(vec![Stmt::Let { var, rhs }]);
            }
            // 余った引数は、translate の既知の呼び出しと同じく、結果の関数の値に `apply` する。`mask` が空のときだけ
            // ここに来るので、分けても `mask` の掛かる範囲は変わらない
            let result = VarId(function.vars.len() as u32);
            function.vars.push(VarInfo {
                name: "t".to_string(),
                repr: callee.ret,
                mark: callee.ret_mark,
            });
            Some(vec![
                Stmt::Let { var: result, rhs },
                Stmt::Let {
                    var,
                    rhs: Rhs::Call {
                        call: Call::Apply {
                            callee: Atom::Var(result),
                            args: rest,
                            yields: *yields,
                            prompt: *prompt,
                        },
                        mask: Vec::new(),
                    },
                },
            ])
        }
        _ => None,
    }
}

/// 引数のそろった呼び出し。包む関数は直接呼ばず、包んでいる命令に戻す。`op$` と `con$` を直接呼ばないことで、box の
/// 挿入がそれらをいつもその場で一様にする (docs/spec/core-ir.md の「REPL の入力」)。
fn saturated(target: FnIdx, callee: &Callee, args: Vec<Atom>, mask: &[MaskItem]) -> Rhs {
    callee
        .wrapper
        .and_then(|wrapper| instruction(wrapper, args.clone(), mask))
        .unwrap_or_else(|| Rhs::Call {
            call: Call::Direct(target, args),
            mask: mask.to_vec(),
        })
}

/// 包む関数の命令に `args` を渡す右辺。`perform` は `mask` を持てないので、`mask` があれば `None` を返す。
fn instruction(wrapper: Wrapper, args: Vec<Atom>, mask: &[MaskItem]) -> Option<Rhs> {
    match wrapper {
        Wrapper::Perform {
            effect,
            op,
            resumable,
        } => mask.is_empty().then(|| Rhs::Call {
            call: Call::Perform {
                effect,
                op,
                resumable,
                args,
            },
            mask: Vec::new(),
        }),
        // `con` はエフェクトを起こさないので `mask` は要らない
        Wrapper::Con(ctor) => Some(Rhs::Con { ctor, args }),
        Wrapper::Resume { stateful: _ } => {
            let mut args = args.into_iter();
            let k = args.next().expect("a continuation wrapper takes k");
            let arg = args.next().expect("a continuation wrapper takes a value");
            let state = args.next().unwrap_or(Atom::Unit);
            Some(Rhs::Call {
                call: Call::Resume { k, arg, state },
                mask: mask.to_vec(),
            })
        }
    }
}
