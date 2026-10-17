//! handle、`perform`、`resume`、`mask` の変換と、節ごとの in-place の入口
//! (docs/spec/core-ir.md の「handle と `prompt$`」「再開と状態」
//! 「`perform` と節の3つの形」「`mask` の包み」)。入口を先にすべて決めてから、関数ごとに命令を置き換える。handle の場所は
//! 入口の関数を `ev_set_clause` に入れるので、どの節がどの形になるかを先に知る必要があるためである。

use std::collections::{HashMap, HashSet};
use std::mem;

use crate::builder::FnBuilder;
use crate::tail::tail_position_call;
use crate::{Call, CoreFn, Program, Rhs, Stmt, Term};
use eml_extern::Purity;
use eml_lir::{
    Atom, BlockId, FnIdx, Mark, MaskItem, OpRet, OperationInfo, Prim, Repr, VarId, VarInfo,
    operation,
};

use super::split::Annotations;
use super::{Helpers, call, captured_mark, direct, dummy, named};

/// translate の節の関数 g ごとの in-place の入口。すぐに再開する節では g 自身を書き換えて入口にし、ほかの節では
/// yield を始める入口 `g$entry` を足す。返す表は g から入口への対応である。
pub(super) fn entries(
    program: &mut Program,
    count: usize,
    helpers: &mut Helpers,
) -> HashMap<FnIdx, FnIdx> {
    let mut clauses = Vec::new();
    for function in &program.functions[..count] {
        for stmt in function.blocks.iter().flat_map(|block| &block.stmts) {
            if let Some((effect, values)) = handled(stmt) {
                for (op, &value) in values.iter().enumerate() {
                    clauses.push((clause_target(function, value), effect, op as u32));
                }
            }
        }
    }
    let mut entries = HashMap::new();
    for (clause, effect, op) in clauses {
        // 同じ g を2回書き換えると、入口の引数と状態の取り出しが重なる。translate は節ごとに別の g を作る
        debug_assert!(
            !entries.contains_key(&clause),
            "a clause function is handled once"
        );
        let operation = operation(&program.effects, effect, op);
        let entry = if runs_in_place(program, program.function(clause), operation) {
            into_entry(program, clause, effect, op);
            clause
        } else {
            let entry = yielding_entry(program, helpers, clause, effect, op);
            program.functions.push(entry);
            FnIdx(program.functions.len() as u32 - 1)
        };
        entries.insert(clause, entry);
    }
    entries
}

/// 関数の handle、`perform`、`resume`、`mask` を置き換え、yield の地点の扱いを変える所を返す。ブロックは分けずに
/// 文を足すだけなので、ブロックの番号は変わらない。
pub(super) fn lower(
    program: &mut Program,
    index: FnIdx,
    entries: &HashMap<FnIdx, FnIdx>,
    helpers: &mut Helpers,
) -> Annotations {
    let mut annotations = Annotations::default();
    let source = program.function(index);
    if !source
        .blocks
        .iter()
        .flat_map(|block| &block.stmts)
        .any(lowered)
    {
        return annotations;
    }
    let handles = source
        .blocks
        .iter()
        .flat_map(|block| &block.stmts)
        .any(|stmt| handled(stmt).is_some());
    let mut function = source.clone();
    let prompt = handles.then(|| helpers.prompt(program));
    // 節の値のクロージャは handle のためだけに作られるので、作る関数を入口に替える
    let clause_closures: HashSet<VarId> = function
        .blocks
        .iter()
        .flat_map(|block| &block.stmts)
        .filter_map(handled)
        .flat_map(|(_, values)| values)
        .filter_map(|value| match value {
            Atom::Var(var) => Some(*var),
            _ => None,
        })
        .collect();
    let yields = function.yields;
    let mut blocks = mem::take(&mut function.blocks);
    for (index, block) in blocks.iter_mut().enumerate() {
        let id = BlockId(index as u32);
        let mut out = Out {
            vars: &mut function.vars,
            stmts: Vec::with_capacity(block.stmts.len()),
        };
        for stmt in mem::take(&mut block.stmts) {
            let Stmt::Let { var, rhs } = stmt else {
                out.stmts.push(stmt);
                continue;
            };
            match rhs {
                Rhs::MakeClosure(target, captures) if clause_closures.contains(&var) => {
                    out.stmts.push(Stmt::Let {
                        var,
                        rhs: Rhs::MakeClosure(entries[&target], captures),
                    });
                }
                Rhs::Call {
                    call:
                        Call::Handle {
                            effect,
                            init,
                            body,
                            clauses,
                            ret,
                        },
                    mask: _,
                } => {
                    let prompt = prompt.expect("`prompt$` is made for a function with a handle");
                    let entries = clauses.iter().map(|&value| match value {
                        Atom::Fn(clause) => Atom::Fn(entries[&clause]),
                        value => value,
                    });
                    out.handle(
                        &mut annotations,
                        id,
                        var,
                        effect,
                        init,
                        body,
                        entries,
                        ret,
                        prompt,
                    );
                }
                Rhs::Call {
                    call:
                        Call::Perform {
                            effect,
                            op,
                            resumable: _,
                            args,
                        },
                    mask: _,
                } => out.perform(var, effect, op, args),
                Rhs::Call {
                    call: Call::Resume { k, arg, state },
                    mask,
                } => {
                    // 再開した計算の yield は、`k` の外側の row、つまりこの関数の row を通る
                    let apply = Call::Apply {
                        callee: k,
                        args: vec![arg, state],
                        yields,
                    };
                    out.masked_call(&mut annotations, id, var, apply, mask);
                }
                Rhs::Call { call, mask } if !mask.is_empty() => {
                    debug_assert!(yields, "a function with a mask yields");
                    out.masked_call(&mut annotations, id, var, call, mask);
                }
                rhs => out.stmts.push(Stmt::Let { var, rhs }),
            }
        }
        block.stmts = out.stmts;
    }
    function.blocks = blocks;
    program.functions[index.0 as usize] = function;
    annotations
}

/// この変換が置き換える文。
fn lowered(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Let {
            var: _,
            rhs:
                Rhs::Call {
                    call: Call::Handle { .. } | Call::Perform { .. } | Call::Resume { .. },
                    mask: _,
                },
        } => true,
        Stmt::Let {
            var: _,
            rhs: Rhs::Call { call: _, mask },
        } => !mask.is_empty(),
        _ => false,
    }
}

/// handle の文なら、そのエフェクトと節の値。
fn handled(stmt: &Stmt) -> Option<(u32, &Vec<Atom>)> {
    match stmt {
        Stmt::Let {
            var: _,
            rhs:
                Rhs::Call {
                    call:
                        Call::Handle {
                            effect,
                            init: _,
                            body: _,
                            clauses,
                            ret: _,
                        },
                    mask: _,
                },
        } => Some((*effect, clauses)),
        _ => None,
    }
}

/// 節の値 `&g` か、同じ関数の中の `closure g(..)` の変数から g を読む。translate はほかの形で節を渡さない。
fn clause_target(function: &CoreFn, value: Atom) -> FnIdx {
    let closure = match value {
        Atom::Fn(target) => Some(target),
        Atom::Var(value) => function
            .blocks
            .iter()
            .flat_map(|block| &block.stmts)
            .find_map(|stmt| match stmt {
                Stmt::Let {
                    var,
                    rhs: Rhs::MakeClosure(target, _),
                } if *var == value => Some(*target),
                _ => None,
            }),
        Atom::Int(_) | Atom::Unit | Atom::Tag(_) => None,
    };
    closure.unwrap_or_else(|| {
        panic!(
            "internal error: a clause of a handler in `{}` is neither `&g` nor a closure of the same function",
            function.name
        )
    })
}

/// すぐに再開し、本体に yield の地点がない節か (docs/spec/core-ir.md の
/// 「`perform` と節の3つの形」)。EP の前の命令で、yield の地点になるものがないことを確かめる。`k` の使いが末尾の
/// `resume` の呼ばれる値だけなら、どの道でも `k` を1回だけ、最後に再開する。
fn runs_in_place(program: &Program, clause: &CoreFn, operation: &OperationInfo) -> bool {
    if !operation.resumable {
        return false;
    }
    let params = clause.params();
    let k = params[params.len() - 2];
    let mut returns = 0;
    for block in &clause.blocks {
        let tail = tail_position_call(block);
        match block.term {
            Term::Return(_) => {
                let resumed = matches!(
                    tail,
                    Some((_, Call::Resume { k: Atom::Var(var), arg: _, state: _ })) if *var == k
                );
                if !resumed {
                    return false;
                }
                returns += 1;
            }
            Term::Jump { .. } | Term::Switch { .. } => {}
        }
        let tail = tail.map(|(at, _)| at);
        for (at, stmt) in block.stmts.iter().enumerate() {
            let yield_point = match stmt {
                Stmt::Let {
                    var: _,
                    rhs: Rhs::Call { call, mask },
                } => {
                    !mask.is_empty()
                        || match call {
                            // 末尾の位置の `resume` は、上で `k` の再開だと確かめた
                            Call::Resume { .. } => tail != Some(at),
                            Call::Perform { .. } | Call::Clause { .. } => true,
                            Call::Handle { .. } => clause.yields,
                            Call::Direct(target, _) => program.function(*target).yields,
                            Call::Apply {
                                callee: _,
                                args: _,
                                yields,
                            } => *yields,
                        }
                }
                // in-place の節は `perform` の場所の `evv` で動くので、IO の extern を持つと、handle の本体の中の
                // `try_io` が節の失敗を捕まえてしまう (docs/spec/core-ir.md の「IO の extern」)
                Stmt::Let {
                    var: _,
                    rhs:
                        Rhs::Extern {
                            ext,
                            args: _,
                            at: _,
                        },
                } => ext.row().purity == Purity::Effectful,
                _ => continue,
            };
            if yield_point {
                return false;
            }
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
    uses == returns
}

/// すぐに再開する節の関数 g を、その場で入口にする。引数を「捕獲、evidence、操作の引数」にし、状態は入口の先頭で
/// evidence の欄から取り出す。末尾の `resume k(v, s)` は、状態を欄に戻して v を返す形にする。
fn into_entry(program: &mut Program, clause: FnIdx, effect: u32, op: u32) {
    let operation = operation(&program.effects, effect, op);
    let operation_params = operation.params.len();
    let ret_mark = operation.ret_mark;
    let resumes: Vec<(usize, usize)> = program
        .function(clause)
        .blocks
        .iter()
        .enumerate()
        .filter_map(|(index, block)| tail_position_call(block).map(|(at, _)| (index, at)))
        .collect();
    let function = &mut program.functions[clause.0 as usize];
    let params = function.params().to_vec();
    let captures = params.len() - operation_params - 2;
    let state = params[params.len() - 1];
    let evidence = fresh(&mut function.vars, "e", Repr::Obj, Mark::Unr);
    let mut values = Vec::new();
    for (index, at) in resumes {
        let unit = fresh(
            &mut function.vars,
            "u",
            Repr::Unit,
            Prim::EvSetState.row().ret_mark,
        );
        let block = &mut function.blocks[index];
        let Stmt::Let {
            var: result,
            rhs:
                Rhs::Call {
                    call:
                        Call::Resume {
                            k: _,
                            arg,
                            state: resumed,
                        },
                    mask: _,
                },
        } = block.stmts[at]
        else {
            unreachable!("every return of a clause run in place returns a tail resume");
        };
        // 末尾の位置の呼び出しの後には、結果を読まない純粋な `let` だけが残りうる。結果の変数は下で消えるので、
        // 後ろの文が読んでいれば変換の誤りである
        debug_assert!(
            block.stmts[at + 1..].iter().all(|stmt| {
                let mut reads = false;
                stmt.for_each_atom(|atom| reads |= atom == Atom::Var(result));
                !reads
            }),
            "no statement after the tail resume reads its result"
        );
        block.stmts[at] = Stmt::Let {
            var: unit,
            rhs: Rhs::Prim(Prim::EvSetState, vec![Atom::Var(evidence), resumed]),
        };
        block.term = Term::Return(arg);
        values.push(arg);
    }
    // 返す値の Repr は操作の結果の型から決まる。定数より変数の Repr を選ぶのは、`#N` が `enum` にも `tobj` にも
    // 収まるためである。操作の ABI に合わせるのは box の挿入である。印は Repr と違って操作の表から直接写す。取り出す
    // 状態の変数は、translate が節の状態の引数に付けた印のままにする。その印は `poly` より正確で、`poly` と互換なので、
    // ほかの状態の変数と同じ `poly` に揃えなくても V2 を通る (docs/spec/core-ir.md の「EP が足す変数の印」)
    let ret = values
        .iter()
        .find_map(|value| match value {
            Atom::Var(var) => Some(function.repr(*var)),
            _ => None,
        })
        .unwrap_or_else(|| atom_repr(values[0]));
    let entry = &mut function.blocks[BlockId::ENTRY.0 as usize];
    let mut entry_params = params[..captures].to_vec();
    entry_params.push(evidence);
    entry_params.extend_from_slice(&params[captures..captures + operation_params]);
    entry.params = entry_params;
    entry.stmts.insert(
        0,
        Stmt::Let {
            var: state,
            rhs: Rhs::Prim(Prim::EvTakeState, vec![Atom::Var(evidence)]),
        },
    );
    function.ret = ret;
    function.ret_mark = ret_mark;
    function.name.push_str("$entry");
    function.clause = Some((effect, op));
}

/// 再開する節と `never` の節の入口 `g$entry`。節のクロージャを作って yield か中断を始め、ダミーを返す。`prompt$` が
/// 後でそのクロージャを、再開する節なら `(k, st)`、`never` の節なら `(st)` で呼ぶ。
fn yielding_entry(
    program: &mut Program,
    helpers: &mut Helpers,
    clause: FnIdx,
    effect: u32,
    op: u32,
) -> CoreFn {
    let id = helpers.id(program);
    let operation = operation(&program.effects, effect, op).clone();
    let function = program.function(clause);
    // g は捕獲と操作の引数の後に、再開する節なら `k` と状態を、`never` の節なら状態だけを受ける
    let trailing = if operation.resumable { 2 } else { 1 };
    let params = function.params();
    let captures = params.len() - operation.params.len() - trailing;
    let mut b = FnBuilder::new();
    let mut args = Vec::new();
    for &param in &params[..captures] {
        args.push(Atom::Var(b.param(function.vars[param.0 as usize].clone())));
    }
    let evidence = b.param(named("e", Repr::Obj, Mark::Unr));
    for (&param, &repr) in params[captures..].iter().zip(&operation.params) {
        let VarInfo { name, mark, .. } = &function.vars[param.0 as usize];
        args.push(Atom::Var(b.param(named(name, repr, *mark))));
    }
    // `closure` は引数を1つ以上とるので、捕獲も操作の引数もなければ関数の値にする
    let body = if args.is_empty() {
        Atom::Fn(clause)
    } else {
        let mark = captured_mark(|var| b.mark(var), &args);
        Atom::Var(builder_bind(
            &mut b,
            named("c", Repr::TObj, mark),
            Rhs::MakeClosure(clause, args),
        ))
    };
    let start = if operation.resumable {
        Prim::YieldTo
    } else {
        Prim::YieldAbort
    };
    builder_bind(
        &mut b,
        named("u", Repr::Unit, start.row().ret_mark),
        Rhs::Prim(start, vec![Atom::Var(evidence), body, Atom::Fn(id)]),
    );
    let ret = match operation.ret {
        OpRet::Repr(repr) => repr,
        OpRet::Never => Repr::Obj,
    };
    let value = dummy(ret, |info, rhs| builder_bind(&mut b, info, rhs));
    b.terminate(Term::Return(value));
    // 入口はいつも yield か中断を始めるので、g の印によらず `yields` を立てる (docs/spec/core-ir.md の「`perform` と節の3つの形」)。
    // 戻り値の印は操作の表から写す。`never` の操作の表の印は `poly` である
    let mut entry = b.finish(
        format!("{}$entry", function.name),
        true,
        true,
        ret,
        operation.ret_mark,
    );
    entry.clause = Some((effect, op));
    entry
}

fn builder_bind(b: &mut FnBuilder, info: VarInfo, rhs: Rhs) -> VarId {
    let var = b.var(info);
    b.emit(Stmt::Let { var, rhs });
    var
}

fn fresh(vars: &mut Vec<VarInfo>, name: &str, repr: Repr, mark: Mark) -> VarId {
    vars.push(named(name, repr, mark));
    VarId(vars.len() as u32 - 1)
}

/// 定数がそのまま収まる Repr。
fn atom_repr(atom: Atom) -> Repr {
    match atom {
        Atom::Int(_) => Repr::Int,
        Atom::Unit => Repr::Unit,
        Atom::Tag(_) => Repr::Enum,
        Atom::Fn(_) | Atom::Var(_) => Repr::TObj,
    }
}

/// 置き換えた後のブロックの文。
struct Out<'a> {
    vars: &'a mut Vec<VarInfo>,
    stmts: Vec<Stmt>,
}

impl Out<'_> {
    fn bind(&mut self, name: &str, repr: Repr, mark: Mark, rhs: Rhs) -> VarId {
        let var = fresh(self.vars, name, repr, mark);
        self.stmts.push(Stmt::Let { var, rhs });
        var
    }

    /// 結果の印は基本操作の表の印である。evidence、`evv`、入口のクロージャは `unr` になる。
    fn prim(&mut self, name: &str, repr: Repr, prim: Prim, args: Vec<Atom>) -> VarId {
        let mark = prim.row().ret_mark;
        self.bind(name, repr, mark, Rhs::Prim(prim, args))
    }

    /// 新しい evidence に入口を入れて `evv` の先頭に足し、本体を呼んで、その結果を `prompt$` に渡す。本体の
    /// `apply` は yield の地点にしない。yield したかは `prompt$` が見る。
    #[expect(clippy::too_many_arguments, reason = "the parts of one handle")]
    fn handle(
        &mut self,
        annotations: &mut Annotations,
        block: BlockId,
        result: VarId,
        effect: u32,
        init: Atom,
        body: Atom,
        entries: impl Iterator<Item = Atom>,
        ret: Atom,
        prompt: FnIdx,
    ) {
        let outer = self.prim("w", Repr::TObj, Prim::EvvGet, Vec::new());
        let evidence = self.prim(
            "ev",
            Repr::Obj,
            Prim::EvNew(effect),
            vec![init, Atom::Var(outer)],
        );
        for (op, entry) in entries.enumerate() {
            self.prim(
                "u",
                Repr::Unit,
                Prim::EvSetClause {
                    effect,
                    op: op as u32,
                },
                vec![Atom::Var(evidence), entry],
            );
        }
        let pushed = self.prim(
            "w",
            Repr::TObj,
            Prim::EvvPush,
            vec![Atom::Var(outer), Atom::Var(evidence)],
        );
        self.prim("u", Repr::Unit, Prim::EvvSet, vec![Atom::Var(pushed)]);
        annotations
            .not_yield_points
            .insert((block, self.stmts.len()));
        // 本体の結果は `File` でありうる
        let value = self.bind(
            "x",
            Repr::TObj,
            Mark::Poly,
            call(Call::Apply {
                callee: body,
                args: vec![Atom::Unit],
                yields: true,
            }),
        );
        self.stmts.push(Stmt::Let {
            var: result,
            rhs: direct(prompt, vec![Atom::Var(evidence), ret, Atom::Var(value)]),
        });
    }

    /// 今の `evv` から evidence を探し、その入口を節の呼び出しで呼ぶ。
    fn perform(&mut self, result: VarId, effect: u32, op: u32, args: Vec<Atom>) {
        let evidence = self.prim("ev", Repr::Obj, Prim::EvLookup(effect), Vec::new());
        let entry = self.prim(
            "c",
            Repr::TObj,
            Prim::EvClause { effect, op },
            vec![Atom::Var(evidence)],
        );
        let mut clause_args = vec![Atom::Var(evidence)];
        clause_args.extend(args);
        self.stmts.push(Stmt::Let {
            var: result,
            rhs: call(Call::Clause {
                effect,
                op,
                callee: Atom::Var(entry),
                args: clause_args,
            }),
        });
    }

    /// `mask` のある呼び出しは、`evv` から飛ばすエフェクトを除いてから呼び、戻ったら元の `evv` に戻す。呼ばれる関数が
    /// `yields` でなくても yield の地点にするのは、遅い道で κ を `mask$` で包むためである。
    fn masked_call(
        &mut self,
        annotations: &mut Annotations,
        block: BlockId,
        result: VarId,
        target: Call,
        mask: Vec<MaskItem>,
    ) {
        if mask.is_empty() {
            self.stmts.push(Stmt::Let {
                var: result,
                rhs: call(target),
            });
            return;
        }
        let outer = self.prim("w", Repr::TObj, Prim::EvvGet, Vec::new());
        let masked = self.prim(
            "w",
            Repr::TObj,
            Prim::EvvMask(mask.clone()),
            vec![Atom::Var(outer)],
        );
        self.prim("u", Repr::Unit, Prim::EvvSet, vec![Atom::Var(masked)]);
        let at = (block, self.stmts.len());
        annotations.masked.insert(at, mask);
        annotations.forced.insert(at);
        self.stmts.push(Stmt::Let {
            var: result,
            rhs: call(target),
        });
        self.prim("u", Repr::Unit, Prim::EvvSet, vec![Atom::Var(outer)]);
    }
}
