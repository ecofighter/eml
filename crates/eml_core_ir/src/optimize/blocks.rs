//! ブロックの並びを整える部品。handler への特殊化 (`handlers`) と融合 (`fuse`) が共有する。どれも構造の規則
//! (docs/spec/lir.md の「構造の規則」) を保ったまま、定数で分ける `switch`、辺の入らないブロック、1本の `jump` だけが
//! 入るブロックを片付ける。

use std::collections::{HashMap, HashSet};

use crate::{Block, CoreFn, Stmt, Term};
use eml_lir::{Atom, BlockId, CasePattern, VarId};

/// 入口から届かないブロックを落とし、残ったブロックに元の順のまま番号を振り直す。展開や `switch` の畳み込みで
/// 切り離したブロックは辺が入らなくなるので、残すと構造の規則に反する (docs/spec/lir.md の「構造の規則」)。
pub(crate) fn prune(function: &mut CoreFn) {
    let mut kept = vec![BlockId::ENTRY];
    kept.extend(reachable(&function.blocks, &function.blocks[0].term));
    if kept.len() == function.blocks.len() {
        return;
    }
    let renumber: HashMap<BlockId, BlockId> = kept
        .iter()
        .enumerate()
        .map(|(position, &index)| (index, BlockId(position as u32)))
        .collect();
    let mut blocks: Vec<Block> = kept
        .iter()
        .map(|&index| std::mem::replace(&mut function.blocks[index.0 as usize], empty()))
        .collect();
    for block in &mut blocks {
        block
            .term
            .for_each_successor_mut(|target| *target = renumber[target]);
    }
    function.blocks = blocks;
}

fn empty() -> Block {
    Block {
        params: Vec::new(),
        stmts: Vec::new(),
        term: Term::Return(Atom::Unit),
    }
}

/// `term` の行き先から届くブロックを、番号の昇順に返す。届く部分だけをたどるので、関数の残りの大きさに依らない。
fn reachable(blocks: &[Block], term: &Term) -> Vec<BlockId> {
    let mut seen = HashSet::new();
    let mut pending: Vec<BlockId> = term.successors().collect();
    while let Some(block) = pending.pop() {
        if seen.insert(block) {
            pending.extend(blocks[block.0 as usize].term.successors());
        }
    }
    let mut reached: Vec<BlockId> = seen.into_iter().collect();
    reached.sort_unstable_by_key(|block| block.0);
    reached
}

/// 引数を置き換えて定数になった `switch` の行き先。展開した節や関数の引数が定数 (`True`、引数のないコンストラクタ、
/// 整数) なら、本体の `switch` の値も定数になる。定数で分ける `switch` は verifier が断るので (docs/spec/lir.md の
/// 「verifier」)、合う case か `default` へのジャンプにする。定数はフィールドを持たないので、合う case はフィールドを
/// 束縛しない。`switch` の行き先は引数を持たないので、ジャンプは値を渡さない。ほかの case のブロックには辺が入らなく
/// なり、`prune` が落とす。
pub(crate) fn constant_target(term: &Term) -> Option<BlockId> {
    let Term::Switch {
        scrutinee,
        layout: _,
        cases,
        default,
    } = term
    else {
        return None;
    };
    let pattern = match *scrutinee {
        Atom::Tag(tag) => CasePattern::Tag(tag),
        Atom::Int(value) => CasePattern::Int(value),
        Atom::Var(_) | Atom::Unit | Atom::Fn(_) => return None,
    };
    let matched = cases.iter().find(|case| case.pattern == pattern);
    debug_assert!(
        matched.is_none_or(|case| case.fields.is_empty()),
        "a constant has no fields"
    );
    Some(
        matched
            .map(|case| case.target)
            .or(*default)
            .expect("a switch on a constant has a matching case or a default"),
    )
}

/// 定数で分ける `switch` を、合う行き先へのジャンプにする (`constant_target`)。1つでも畳んだかを返す。辺の入らなく
/// なったブロックは、呼ぶ側が `prune` で落とす。
pub(crate) fn fold_constant_switches(function: &mut CoreFn) -> bool {
    let mut folded = false;
    for block in &mut function.blocks {
        if let Some(target) = constant_target(&block.term) {
            block.term = Term::Jump {
                target,
                args: Vec::new(),
            };
            folded = true;
        }
    }
    folded
}

/// 入る辺が1本の `jump` だけのブロックを、その `jump` のブロックの後ろにつなげる。1つでもつなげたかを返す。
/// ブロックの引数は実引数で置き換える。行き先は `jump` のブロックからしか入れないので、引数を使う所はどれも `jump` の
/// ブロックの終わりに支配され、実引数がそこで見える。置き換えるのは、実引数が引数と同じ Repr と印の変数か、引数の
/// Repr の値になれる定数のときだけである。使う所の形がそのまま保たれ、Repr と印の検査の答えが変わらないためである
/// (docs/spec/lir.md の「構造の規則」の R7 と「Kind の印の検査」)。辺は番号の大きいブロックへ向かうので、つなげた後も
/// 元の順のまま番号を振り直せば R2 を保つ。
///
/// 展開した関数の `return` が残りのブロックへの `jump` になる所と、畳んだ `switch` の行き先で、ブロックの境目を消す。
pub(crate) fn merge_jumps(function: &mut CoreFn) -> bool {
    let count = function.blocks.len();
    let mut edges = vec![0usize; count];
    for block in &function.blocks {
        for target in block.term.successors() {
            edges[target.0 as usize] += 1;
        }
    }
    let mut substitution: Vec<Option<Atom>> = vec![None; function.vars.len()];
    let mut removed = vec![false; count];
    let mut merged = false;
    for index in 0..count {
        if removed[index] {
            continue;
        }
        while let Term::Jump { target, args } = &function.blocks[index].term {
            let target = target.0 as usize;
            if edges[target] != 1 {
                break;
            }
            let args: Vec<Atom> = args
                .iter()
                .map(|&arg| resolve(&substitution, arg))
                .collect();
            let params = &function.blocks[target].params;
            if !params
                .iter()
                .zip(&args)
                .all(|(&param, &arg)| substitutable(function, arg, param))
            {
                break;
            }
            for (&param, arg) in params.iter().zip(args) {
                substitution[param.0 as usize] = Some(arg);
            }
            let next = std::mem::replace(&mut function.blocks[target], empty());
            removed[target] = true;
            let host = &mut function.blocks[index];
            host.stmts.extend(next.stmts);
            host.term = next.term;
            merged = true;
        }
    }
    if !merged {
        return false;
    }
    let mut renumber = vec![BlockId(0); count];
    let mut next = 0;
    for (index, &gone) in removed.iter().enumerate() {
        if !gone {
            renumber[index] = BlockId(next);
            next += 1;
        }
    }
    let mut index = 0;
    function.blocks.retain(|_| {
        index += 1;
        !removed[index - 1]
    });
    for block in &mut function.blocks {
        block
            .term
            .for_each_successor_mut(|target| *target = renumber[target.0 as usize]);
        substitute(block, &substitution);
    }
    true
}

fn resolve(substitution: &[Option<Atom>], atom: Atom) -> Atom {
    match atom {
        Atom::Var(var) => substitution[var.0 as usize].unwrap_or(atom),
        atom => atom,
    }
}

fn substitutable(function: &CoreFn, arg: Atom, param: VarId) -> bool {
    match arg {
        Atom::Var(var) => {
            function.repr(var) == function.repr(param) && function.mark(var) == function.mark(param)
        }
        constant => constant.fits(&function.vars, function.repr(param)),
    }
}

/// ブロックの中の値の使いを置き換える。`unpack` の値は `obj` で、`obj` に収まる定数はないので
/// (`Atom::fits`)、置き換え先は変数である。
fn substitute(block: &mut Block, substitution: &[Option<Atom>]) {
    let atom = |atom: &mut Atom| *atom = resolve(substitution, *atom);
    for stmt in &mut block.stmts {
        match stmt {
            Stmt::Let { var: _, rhs } => rhs.for_each_atom_mut(atom),
            Stmt::Unpack { value, .. } => match resolve(substitution, Atom::Var(*value)) {
                Atom::Var(var) => *value = var,
                constant => panic!("an unpacked value is replaced by the constant {constant:?}"),
            },
        }
    }
    block.term.for_each_atom_mut(atom);
}
