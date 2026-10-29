//! 小さな再帰しない関数のインライン化 (docs/spec/core-ir.md の「インライン化」)。展開する関数かどうかは、
//! 呼び出し先の書き換えた後のコードだけで決め、展開するかどうかは、それに呼び出しの場所の形を足して決める。
//! プログラム全体の大きさの予算は使わない。全体の予算を使うと、
//! 同じ名前の関数のコードがプログラムの残りに依って変わるためである。

use crate::tail::tail_position_call;
use crate::{Block, Call, CoreFn, Rhs, Stmt, Term};
use eml_lir::{Atom, BlockId, FnIdx, Mark, Repr, Structs, VarId, VarInfo};

use crate::optimize::blocks::Renamer;

/// 展開する関数の文の数の上限。ブロックの終端も1つの文と数える。文を持たないブロックが多い関数 (定数を返す枝の
/// 多い `match`) も、展開すれば終端の数だけコードが増えるためである。
pub(super) const INLINE_LIMIT: usize = 40;

/// 書き換えた後の `function` を、ほかの関数に展開してよいか。大きさが上限以下で、handle、`resume`、`clause` を
/// 持たない関数である。再帰しないことは呼ぶ側が確かめる。handle を持つ関数を展開しないのは、EP が節の関数が
/// ちょうど1つの handle に属することを前提にするためである (`crates/eml_core_ir/src/evidence/handlers.rs` の
/// `entries`)。
pub(super) fn inlinable(function: &CoreFn) -> bool {
    size(function) <= INLINE_LIMIT
        && !function
            .blocks
            .iter()
            .flat_map(|block| &block.stmts)
            .any(|stmt| {
                matches!(
                    stmt,
                    Stmt::Let {
                        var: _,
                        rhs: Rhs::Call {
                            call: Call::Handle { .. } | Call::Resume { .. } | Call::Clause { .. },
                            mask: _,
                        },
                    }
                )
            })
}

/// 関数の文の数。ブロックの終端も1つと数える (`INLINE_LIMIT`)。
pub(super) fn size(function: &CoreFn) -> usize {
    function
        .blocks
        .iter()
        .map(|block| block.stmts.len() + 1)
        .sum()
}

/// 展開する呼び出し。`index` はブロックの中の文の位置で、`tail` は本体の `return` をそのまま残すかである。
struct Site {
    index: usize,
    callee: FnIdx,
    tail: bool,
}

/// `function` の直接の呼び出しのうち、展開できるものを展開する。`functions` の呼び出し先は書き換えた後の
/// コードで、`chosen` は呼び出しの結果の変数と呼び出し先から、その呼び出しを展開してよいかを答える。`limit` は
/// 展開した後の関数の大きさの上限で、呼び出しをブロックの順、文の順に見て、呼び出し先の大きさが残りに収まる
/// 呼び出しだけを展開する。どれを展開するかは関数と呼び出し先のコードだけで決まる。何か展開したかを返す。
///
/// 展開する場所は、書き換える前の関数の文から先に決める。展開した本体の中の呼び出しは、呼び出し先を書き換えた
/// ときに決めてあるので、ここでは見ない。そのため、ブロックの新しい番号も先に決まり、新しいブロックの列を1回で
/// 組み立てられる。手間は、元の関数と展開した本体の大きさの和に比例する。
///
/// 呼び出しのあるブロックは、呼び出しの前の文に本体のエントリのブロックを続けたものになる。本体のほかのブロックは、その
/// 直後に元の順のまま入れる。本体の変数は新しい変数にし、引数は実引数で置き換える。呼び出しの後の文と終端は、呼び出しの
/// 結果の変数を引数にした新しいブロックに移し、本体の `return r` をそのブロックへの `jump` にする。末尾の位置の
/// 呼び出しで `stays_tail` が成り立てば、本体の `return` をそのまま残し、後ろの純粋な `let` (使われない) と
/// `return` は捨てる。元のブロックは元の順に並ぶので、辺はどれも番号の大きいブロックへ向かう
/// (docs/spec/lir.md の「構造の規則」の R2)。extern の呼び出しは位置 (`at`) を持ったまま写すので、実行時エラーの
/// 位置は変わらない。
pub(super) fn inline_calls(
    function: &mut CoreFn,
    functions: &[CoreFn],
    structs: &Structs,
    chosen: &dyn Fn(VarId, FnIdx) -> bool,
    limit: usize,
) -> bool {
    let mut room = limit.saturating_sub(size(function));
    let sites: Vec<Vec<Site>> = (0..function.blocks.len())
        .map(|block| {
            sites(
                function,
                BlockId(block as u32),
                functions,
                structs,
                chosen,
                &mut room,
            )
        })
        .collect();
    if sites.iter().all(Vec::is_empty) {
        return false;
    }
    let mut starts = Vec::with_capacity(sites.len());
    let mut next = 0u32;
    for block in &sites {
        starts.push(next);
        next += 1;
        for site in block {
            next += functions[site.callee.0 as usize].blocks.len() as u32 - 1;
            next += u32::from(!site.tail);
        }
    }
    let original = |target: BlockId| BlockId(starts[target.0 as usize]);
    let old = std::mem::take(&mut function.blocks);
    let mut out: Vec<Block> = Vec::with_capacity(next as usize);
    for (block, sites) in old.into_iter().zip(&sites) {
        let Block {
            params,
            stmts,
            mut term,
        } = block;
        let mut stmts = stmts.into_iter();
        let mut consumed = 0;
        let mut current = Some(Block {
            params,
            stmts: Vec::new(),
            term: Term::Return(Atom::Unit),
        });
        for site in sites {
            let mut host = current
                .take()
                .expect("a tail site is the last site of its block");
            host.stmts
                .extend(stmts.by_ref().take(site.index - consumed));
            consumed = site.index + 1;
            let Some(Stmt::Let {
                var: result,
                rhs:
                    Rhs::Call {
                        call: Call::Direct(_, args),
                        mask: _,
                    },
            }) = stmts.next()
            else {
                unreachable!("a site is a direct call");
            };
            let callee = &functions[site.callee.0 as usize];
            let first = out.len() as u32;
            let rest = BlockId(first + callee.blocks.len() as u32);
            let mut copied = copy_body(
                &mut function.vars,
                callee,
                &args,
                first,
                (!site.tail).then_some(rest),
            );
            let entry = copied.remove(0);
            host.stmts.extend(entry.stmts);
            host.term = entry.term;
            out.push(host);
            out.extend(copied);
            if !site.tail {
                current = Some(Block {
                    params: vec![result],
                    stmts: Vec::new(),
                    term: Term::Return(Atom::Unit),
                });
            }
        }
        if let Some(mut last) = current {
            last.stmts.extend(stmts);
            term.for_each_successor_mut(|target| *target = original(*target));
            last.term = term;
            out.push(last);
        }
    }
    debug_assert_eq!(out.len(), next as usize);
    function.blocks = out;
    true
}

/// `block` の中で展開する呼び出し。
fn sites(
    function: &CoreFn,
    block: BlockId,
    functions: &[CoreFn],
    structs: &Structs,
    chosen: &dyn Fn(VarId, FnIdx) -> bool,
    room: &mut usize,
) -> Vec<Site> {
    let stmts = &function.block(block).stmts;
    (0..stmts.len())
        .filter_map(|index| {
            let Stmt::Let {
                var,
                rhs:
                    Rhs::Call {
                        call: Call::Direct(target, args),
                        mask,
                    },
            } = &stmts[index]
            else {
                return None;
            };
            // `mask` は呼び出し先の本体のエフェクトに掛かるので、展開すると掛ける所がなくなる
            if !mask.is_empty() || !chosen(*var, *target) {
                return None;
            }
            let callee = &functions[target.0 as usize];
            let at = (block, index);
            let grown = size(callee);
            if grown > *room || !fits(function, at, callee, args, structs) {
                return None;
            }
            *room -= grown;
            Some(Site {
                index,
                callee: *target,
                tail: stays_tail(function, at, callee, args, structs),
            })
        })
        .collect()
}

/// 呼び出しの場所で、展開した本体が Repr と印の検査を通るか。
///
/// - yield しうる本体は yield しうる関数にだけ展開する。`yields` の付け直しは、印の立っていない関数を yield しない
///   ものとして扱うためである (`crate::optimize::yields::recompute_yields`)
/// - 引数を置き換えるので、変数の実引数は引数と同じ Repr と印を持たなければならない。印を下げて置き換えると、本体が
///   その引数を `lin` の受け手に渡す所で `unr` と `lin` が出会う (docs/spec/lir.md の「Kind の印の検査」の V2)。
///   定数は、引数の Repr の値になれればよい
/// - 本体の `return` の値は、呼び出しの結果の変数へ渡す `jump` の実引数になる。末尾の位置の呼び出しなら、呼ぶ側の
///   `return` の値にしてもよい。どちらかの受け手と互換でなければ展開しない
fn fits(
    function: &CoreFn,
    at: (BlockId, usize),
    callee: &CoreFn,
    args: &[Atom],
    structs: &Structs,
) -> bool {
    if callee.yields && !function.yields {
        return false;
    }
    let params_fit = callee.params().iter().zip(args).all(|(&param, &arg)| {
        let info = &callee.vars[param.0 as usize];
        match arg {
            Atom::Var(var) => function.repr(var) == info.repr && function.mark(var) == info.mark,
            constant => constant.fits(&callee.vars, info.repr),
        }
    });
    if !params_fit {
        return false;
    }
    let Stmt::Let { var: result, .. } = function.block(at.0).stmts[at.1] else {
        unreachable!("a call binds its result");
    };
    returns_pass(
        function,
        callee,
        args,
        (function.repr(result), function.mark(result)),
        structs,
    ) || stays_tail(function, at, callee, args, structs)
}

/// 末尾の位置の呼び出しで、本体の `return` をそのまま呼ぶ側の `return` にしてよいか。
fn stays_tail(
    function: &CoreFn,
    (block, index): (BlockId, usize),
    callee: &CoreFn,
    args: &[Atom],
    structs: &Structs,
) -> bool {
    tail_position_call(function.block(block)).is_some_and(|(at, _)| at == index)
        && returns_pass(
            function,
            callee,
            args,
            (function.ret, function.ret_mark),
            structs,
        )
}

/// 本体のどの `return` の値も、Repr が `repr`、印が `mark` の受け手に渡せるか (R7 と V2)。戻らない呼び出しの束縛は、
/// verifier と同じくどの受け手にも渡せる (docs/spec/lir.md の「Kind の印の検査」)。構造体は verifier と同じく
/// `Structs::compatible` で比べる。
fn returns_pass(
    function: &CoreFn,
    callee: &CoreFn,
    args: &[Atom],
    (repr, mark): (Repr, Mark),
    structs: &Structs,
) -> bool {
    let params = callee.params();
    let never = never_bound(callee);
    callee.blocks.iter().all(|block| {
        let Term::Return(returned) = block.term else {
            return true;
        };
        match returned {
            Atom::Var(var) if never.contains(&var) => true,
            Atom::Var(var) => match params.iter().position(|&param| param == var) {
                Some(position) => match args[position] {
                    Atom::Var(arg) => {
                        structs.compatible(function.repr(arg), repr)
                            && Mark::passes_to(function.mark(arg), mark)
                    }
                    constant => constant.fits_compatible(&function.vars, structs, repr),
                },
                None => {
                    structs.compatible(callee.repr(var), repr)
                        && Mark::passes_to(callee.mark(var), mark)
                }
            },
            constant => constant.fits_compatible(&callee.vars, structs, repr),
        }
    })
}

/// 戻らない呼び出し (`never` の操作の `perform` と、戻らない extern) が束縛する変数。
fn never_bound(function: &CoreFn) -> Vec<VarId> {
    function
        .blocks
        .iter()
        .flat_map(|block| &block.stmts)
        .filter_map(|stmt| match stmt {
            Stmt::Let {
                var,
                rhs:
                    Rhs::Call {
                        call:
                            Call::Perform {
                                resumable: false, ..
                            },
                        mask: _,
                    },
            } => Some(*var),
            Stmt::Let {
                var,
                rhs: Rhs::Extern { ext, .. },
            } if ext.row().ret == eml_extern::OpRet::Never => Some(*var),
            _ => None,
        })
        .collect()
}

/// `callee` の本体を、`vars` に新しい変数を足して写す。写したブロックは `first` から順に番号を振る。エントリのブロックは
/// 呼ぶ側のブロックにつながり、引数を持たない。`rest` があれば本体の `return r` を `jump rest(r)` にし、なければ
/// `return` のまま残す。
fn copy_body(
    vars: &mut Vec<VarInfo>,
    callee: &CoreFn,
    args: &[Atom],
    first: u32,
    rest: Option<BlockId>,
) -> Vec<Block> {
    let mut copy = Renamer::new(vars, &callee.vars);
    for (&param, &arg) in callee.params().iter().zip(args) {
        copy.substitute(param, arg);
    }
    callee
        .blocks
        .iter()
        .enumerate()
        .map(|(position, block)| {
            let params = copy.params(position, block);
            let stmts = block.stmts.iter().map(|stmt| copy.stmt(stmt)).collect();
            let term = match (&block.term, rest) {
                (&Term::Return(returned), Some(rest)) => Term::Jump {
                    target: rest,
                    args: vec![copy.atom(returned)],
                },
                (term, _) => copy.term(term, |id| BlockId(first + id.0)),
            };
            Block {
                params,
                stmts,
                term,
            }
        })
        .collect()
}
