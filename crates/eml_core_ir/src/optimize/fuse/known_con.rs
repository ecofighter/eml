//! 既知のコンストラクタ (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「既知のコンストラクタ」)。
//! 同じ関数の中の `con` で作った値への `switch` と `unpack` をその場で解き、合流のブロックの引数を通って届く場合は
//! `jump` の側で決める (docs/future/roadmap.md の「処理系」の項目「jump threading とインライン化」)。解いた `con` の
//! フィールドにある既知の閉包への `apply` は、S13e-1 と同じ規則で直接の呼び出しにする。
//!
//! `con` の変数を開くのは、その `switch` か `unpack` が唯一の使いのときだけである。translate が `Unr` でない `con` の
//! 変数を開く規則と同じで (docs/spec/core-ir.md)、引数の所有権はちょうど1回だけ移る。どの規則も関数自身のコードだけを
//! 読むので、書き換えた後のコードは名前で決まる (docs/spec/vm.md の「追記」)。

use std::collections::{HashMap, HashSet};

use crate::optimize::known::{
    Callee, KnownClosure, apply_rewritable, fresh, known_closures, replace,
};
use crate::{Block, Call, CoreFn, Rhs, Stmt, Term};
use eml_lir::{Atom, BlockId, Case, CasePattern, Ctor, Mark, VarId};

/// `con` が束縛した変数と、そのコンストラクタと引数。
pub(super) fn known_cons(function: &CoreFn) -> HashMap<VarId, (Ctor, Vec<Atom>)> {
    function
        .blocks
        .iter()
        .flat_map(|block| &block.stmts)
        .filter_map(|stmt| match stmt {
            Stmt::Let {
                var,
                rhs: Rhs::Con { ctor, args },
            } => Some((*var, (*ctor, args.clone()))),
            _ => None,
        })
        .collect()
}

/// 変数ごとの、値として使う所の数。
pub(super) fn uses(function: &CoreFn) -> Vec<usize> {
    let mut uses = vec![0usize; function.vars.len()];
    let mut count = |atom: Atom| {
        if let Atom::Var(var) = atom {
            uses[var.0 as usize] += 1;
        }
    };
    for block in &function.blocks {
        block
            .stmts
            .iter()
            .for_each(|stmt| stmt.for_each_atom(&mut count));
        block.term.for_each_atom(&mut count);
    }
    uses
}

/// 唯一の使いが開く所である、既知の `con` の変数。
fn single_con<'a>(
    known: &'a HashMap<VarId, (Ctor, Vec<Atom>)>,
    uses: &[usize],
    atom: Atom,
) -> Option<(VarId, &'a (Ctor, Vec<Atom>))> {
    let Atom::Var(var) = atom else {
        return None;
    };
    (uses[var.0 as usize] == 1)
        .then(|| known.get(&var).map(|con| (var, con)))
        .flatten()
}

/// `pattern` の値で `switch` が進む行き先と、その case のフィールド。`default` のフィールドは空である。
fn decide(
    cases: &[Case],
    default: Option<BlockId>,
    pattern: CasePattern,
) -> Option<(BlockId, Vec<VarId>)> {
    match cases.iter().find(|case| case.pattern == pattern) {
        Some(case) => Some((case.target, case.fields.clone())),
        None => default.map(|target| (target, Vec::new())),
    }
}

/// `arg` を `param` へ渡してよいか (docs/spec/lir.md の「構造の規則」の R7 と「Kind の印の検査」の V2)。定数の印は
/// どの受け手とも互換である。
fn passes(function: &CoreFn, arg: Atom, param: VarId) -> bool {
    let repr = function.repr(param);
    match arg {
        Atom::Var(var) => {
            function.repr(var).compatible(repr)
                && function.mark(var).compatible(function.mark(param))
        }
        constant => constant.fits_compatible(&function.vars, repr),
    }
}

fn non_unr(function: &CoreFn, atom: Atom) -> bool {
    matches!(atom, Atom::Var(var) if function.mark(var) != Mark::Unr)
}

/// `con` の引数を、開いた先のフィールドに移してよいか。フィールドを持たない行き先では引数を捨てることになる。
/// 開く前は Perceus が `con` の値を親から手放せたが、開いた後に残るのは引数の変数だけで、`Unr` でない変数は
/// `decref` できない (docs/spec/lir.md の「Kind の印の検査」の V1)。そこで、`Unr` でない引数は使われるフィールドに
/// 移るときだけ開く。
fn opens(function: &CoreFn, uses: &[usize], args: &[Atom], fields: &[VarId]) -> bool {
    if fields.is_empty() {
        return !args.iter().any(|&arg| non_unr(function, arg));
    }
    fields.len() == args.len()
        && args.iter().zip(fields).all(|(&arg, &field)| {
            passes(function, arg, field) && (!non_unr(function, arg) || uses[field.0 as usize] > 0)
        })
}

/// 同じ関数の中の既知の `con` への `switch` と `unpack` を解く。`switch` は選ばれる行き先への `jump` にし、行き先の
/// ブロックの引数を case のフィールドにする。`unpack` はブロックをそこで分け、残りを `con` の引数を受ける新しい
/// ブロックにする。値の置き換えは `merge_jumps` に任せる。Repr と印が同じでない引数は置き換えずに `jump` で渡す
/// ので、どちらの形も verifier の検査をそのまま通る。開いた `con` の `let` は消す。何か解いたかを返す。
pub(super) fn resolve_known(function: &mut CoreFn) -> bool {
    let known = known_cons(function);
    if known.is_empty() {
        return false;
    }
    let uses = uses(function);
    let mut opened = HashSet::new();
    for index in 0..function.blocks.len() {
        let Term::Switch {
            scrutinee,
            layout,
            cases,
            default,
        } = &function.blocks[index].term
        else {
            continue;
        };
        let Some((var, (ctor, args))) = single_con(&known, &uses, *scrutinee) else {
            continue;
        };
        if *layout != Some(ctor.layout) {
            continue;
        }
        let Some((target, fields)) = decide(cases, *default, CasePattern::Tag(ctor.tag)) else {
            continue;
        };
        if !opens(function, &uses, args, &fields) {
            continue;
        }
        // 行き先は `switch` の行き先でなくなり、入る辺はこの `jump` の1本だけになる (R3 と R4)
        let args = if fields.is_empty() {
            Vec::new()
        } else {
            args.clone()
        };
        function.blocks[target.0 as usize].params = fields;
        function.blocks[index].term = Term::Jump { target, args };
        opened.insert(var);
    }
    let mut splits: Vec<Vec<(usize, Vec<Atom>)>> = vec![Vec::new(); function.blocks.len()];
    for (index, block) in function.blocks.iter().enumerate() {
        for (position, stmt) in block.stmts.iter().enumerate() {
            let Stmt::Unpack {
                value,
                ctor,
                fields,
            } = stmt
            else {
                continue;
            };
            let Some((var, (built, args))) = single_con(&known, &uses, Atom::Var(*value)) else {
                continue;
            };
            if built == ctor && opens(function, &uses, args, fields) {
                splits[index].push((position, args.clone()));
                opened.insert(var);
            }
        }
    }
    if opened.is_empty() {
        return false;
    }
    if splits.iter().any(|block| !block.is_empty()) {
        split_at_unpacks(function, splits);
    }
    drop_cons(function, &opened);
    true
}

/// ブロックを、開く `unpack` の位置で分ける。前の部分は `con` の引数を渡す `jump` で終わり、後ろの部分はフィールドを
/// 引数に持つ新しいブロックになる。新しいブロックは元のブロックの直後に並べるので、辺はどれも番号の大きいブロックへ
/// 向かう (R2)。後ろの部分は元のブロックが支配していた所なので、元の定義はそのまま見える (R6)。
fn split_at_unpacks(function: &mut CoreFn, splits: Vec<Vec<(usize, Vec<Atom>)>>) {
    let mut starts = Vec::with_capacity(splits.len());
    let mut next = 0u32;
    for block in &splits {
        starts.push(next);
        next += 1 + block.len() as u32;
    }
    let old = std::mem::take(&mut function.blocks);
    let mut out = Vec::with_capacity(next as usize);
    for ((index, block), splits) in old.into_iter().enumerate().zip(splits) {
        let Block {
            params,
            stmts,
            mut term,
        } = block;
        term.for_each_successor_mut(|target| *target = BlockId(starts[target.0 as usize]));
        let mut current = Block {
            params,
            stmts: Vec::new(),
            term: Term::Return(Atom::Unit),
        };
        let mut piece = starts[index];
        let mut splits = splits.into_iter().peekable();
        for (position, stmt) in stmts.into_iter().enumerate() {
            match splits.next_if(|(at, _)| *at == position) {
                Some((_, args)) => {
                    let Stmt::Unpack { fields, .. } = stmt else {
                        unreachable!("a split is at an unpack");
                    };
                    piece += 1;
                    current.term = Term::Jump {
                        target: BlockId(piece),
                        args,
                    };
                    out.push(std::mem::replace(
                        &mut current,
                        Block {
                            params: fields,
                            stmts: Vec::new(),
                            term: Term::Return(Atom::Unit),
                        },
                    ));
                }
                None => current.stmts.push(stmt),
            }
        }
        current.term = term;
        out.push(current);
    }
    function.blocks = out;
}

/// 開いた `con` の `let` を消す。唯一の使いが開く所だったので、値はもう使われない。
fn drop_cons(function: &mut CoreFn, opened: &HashSet<VarId>) {
    for block in &mut function.blocks {
        block.stmts.retain(
            |stmt| !matches!(stmt, Stmt::Let { var, rhs: Rhs::Con { .. } } if opened.contains(var)),
        );
    }
}

/// 合流のブロック `M(p…)` の始まりの形。
enum Entry {
    /// 文を持たず、`i` 番目の引数で分ける `switch` で終わる。ほかの引数は使われない。`i` 番目の引数は、`switch` の
    /// ほかに行き先のブロックで使われていてもよい (`thread_switch`)。
    Switch(usize),
    /// `i` 番目の引数の `unpack` で始まる。
    Unpack(usize),
}

fn entry(uses: &[usize], block: &Block) -> Option<Entry> {
    let only_use = |atom: Atom| match atom {
        Atom::Var(var) if uses[var.0 as usize] == 1 => {
            block.params.iter().position(|&param| param == var)
        }
        _ => None,
    };
    match (&block.stmts[..], &block.term) {
        ([], Term::Switch { scrutinee, .. }) => {
            let Atom::Var(scrutinee) = *scrutinee else {
                return None;
            };
            let position = block.params.iter().position(|&param| param == scrutinee)?;
            let others_unused = block
                .params
                .iter()
                .enumerate()
                .all(|(at, &param)| at == position || uses[param.0 as usize] == 0);
            others_unused.then_some(Entry::Switch(position))
        }
        ([Stmt::Unpack { value, .. }, ..], _) => only_use(Atom::Var(*value)).map(Entry::Unpack),
        _ => None,
    }
}

/// `jump` の実引数で解ける合流のブロックの入口を、`jump` の側で決める (docs/future/roadmap.md の「処理系」の
/// 項目「jump threading とインライン化」)。`jump M(a…)` の行き先 `M(p…)` が `switch p_i` か `unpack p_i` で始まり、
/// `p_i` をほかで使わず、`a_i` が `Tag` や `Int` の定数か、唯一の使いがこの `jump` である既知の `con` なら、`switch`
/// か `unpack` を飛ばす。
///
/// ブロックの分け方は「R3 を守る作り方」に従う。`switch` で選ばれる行き先 C は、`jump N(f')` だけを持つ辺のブロック
/// と、元の本体と case のフィールドを引数に持つ合流のブロック N に分け、`jump` を `jump N(fs)` に付け替える。C は
/// `switch` の行き先のまま引数を持たず (R3)、N は C の直後に置く (R2)。N の本体が使う値は、C のフィールドのほかは
/// M を支配するブロックの定義なので、`jump` のブロックでも見える (R6)。`unpack` で始まる M は、`unpack` と
/// `jump N(…)` だけを残し、残りを M の引数とフィールドを受ける N に移す。M が支配していた所はどれも N が支配するので、
/// N の引数に元の変数を使えば、本体の使いは書き換えなくてよい。
///
/// すでにこの形に分けた C と M (`jump` だけで終わる) は、新しいブロックを作らずに `jump` の行き先へ直接つなぐ。届かなく
/// なった M と C は、呼ぶ側が `prune` で落とす。何かつなぎ替えたかを返す。
pub(super) fn thread_jumps(function: &mut CoreFn) -> bool {
    let known = known_cons(function);
    let uses = uses(function);
    let count = function.blocks.len();
    let mut added: Vec<(usize, Block)> = Vec::new();
    let mut opened = HashSet::new();
    let mut changed = false;
    for index in 0..count {
        let Term::Jump { target, args } = &function.blocks[index].term else {
            continue;
        };
        let (target, args) = (target.0 as usize, args.clone());
        // この回で足したブロックへ向かう `jump` は、次の回で見る
        if target >= count {
            continue;
        }
        let Some(form) = entry(&uses, &function.blocks[target]) else {
            continue;
        };
        let threaded = match form {
            Entry::Switch(position) => {
                thread_switch(function, &uses, &known, &mut added, target, &args, position)
            }
            Entry::Unpack(position) => {
                thread_unpack(function, &uses, &known, &mut added, target, &args, position)
            }
        };
        if let Some((jump, con)) = threaded {
            function.blocks[index].term = jump;
            opened.extend(con);
            changed = true;
        }
    }
    drop_cons(function, &opened);
    insert_after(function, added);
    changed
}

/// `switch` で始まる `M` への `jump` を、選ばれる行き先の本体へつなぐ。新しい `jump` と、開いた `con` の変数を返す。
fn thread_switch(
    function: &mut CoreFn,
    uses: &[usize],
    known: &HashMap<VarId, (Ctor, Vec<Atom>)>,
    added: &mut Vec<(usize, Block)>,
    target: usize,
    args: &[Atom],
    position: usize,
) -> Option<(Term, Option<VarId>)> {
    let Term::Switch {
        layout,
        cases,
        default,
        ..
    } = &function.blocks[target].term
    else {
        unreachable!("the entry is a switch");
    };
    let (pattern, con, values) = match args[position] {
        Atom::Tag(tag) => (CasePattern::Tag(tag), None, Vec::new()),
        Atom::Int(value) => (CasePattern::Int(value), None, Vec::new()),
        atom => {
            let (var, (ctor, values)) = single_con(known, uses, atom)?;
            if *layout != Some(ctor.layout) {
                return None;
            }
            (CasePattern::Tag(ctor.tag), Some(var), values.clone())
        }
    };
    let (chosen, fields) = decide(cases, *default, pattern)?;
    if con.is_none() && !fields.is_empty() {
        return None;
    }
    if !scrutinee_skipped(function, uses, target, chosen) {
        return None;
    }
    // フィールドを持たない行き先へ進む `con` は、引数を捨てる (`opens`)
    if fields.is_empty() && values.iter().any(|&value| non_unr(function, value)) {
        return None;
    }
    let values = if fields.is_empty() {
        Vec::new()
    } else {
        values
    };
    let chosen = chosen.0 as usize;
    let block = &function.blocks[chosen];
    if let (
        [],
        Term::Jump {
            target: next,
            args: forwarded,
        },
    ) = (&block.stmts[..], &block.term)
    {
        let substitution: HashMap<VarId, Atom> =
            fields.iter().copied().zip(values.iter().copied()).collect();
        return forward(
            function,
            added,
            *next,
            forwarded,
            &substitution,
            &values,
            con,
        );
    }
    if !fields.is_empty() && !opens(function, uses, &values, &fields) {
        return None;
    }
    let copies: Vec<VarId> = fields
        .iter()
        .map(|&field| copy_var(function, field))
        .collect();
    let join = BlockId((function.blocks.len() + added.len()) as u32);
    let body = std::mem::replace(
        &mut function.blocks[chosen],
        Block {
            params: Vec::new(),
            stmts: Vec::new(),
            term: Term::Jump {
                target: join,
                args: copies.iter().map(|&copy| Atom::Var(copy)).collect(),
            },
        },
    );
    if let Term::Switch { cases, .. } = &mut function.blocks[target].term
        && let Some(case) = cases
            .iter_mut()
            .find(|case| case.target.0 as usize == chosen)
    {
        case.fields = copies;
    }
    added.push((
        chosen,
        Block {
            params: fields,
            stmts: body.stmts,
            term: body.term,
        },
    ));
    Some((
        Term::Jump {
            target: join,
            args: values,
        },
        con,
    ))
}

/// `jump` が `M` の `switch` を飛ばして `chosen` の本体へ進むとき、`switch` の値を使う所がほかの行き先のブロックに
/// しかないか。行き先へ入る辺は `switch` の辺1本だけなので (docs/spec/lir.md の「構造の規則」の R3)、ほかの行き先の
/// 使いは `M` を通る経路にしか現れない。translate はフィールドを束縛しない葉で `Unr` でない値を `drop` するので
/// (docs/spec/core-ir.md)、`None` の葉が `Option` の値を捨てていても `Some` の `con` を `jump` の側で解けるように、
/// ほかの行き先の使いは許す。
fn scrutinee_skipped(function: &CoreFn, uses: &[usize], target: usize, chosen: BlockId) -> bool {
    let block = &function.blocks[target];
    let Term::Switch {
        scrutinee: Atom::Var(scrutinee),
        cases,
        default,
        ..
    } = &block.term
    else {
        return false;
    };
    let mut targets: Vec<BlockId> = cases
        .iter()
        .map(|case| case.target)
        .chain(*default)
        .collect();
    targets.sort_unstable_by_key(|target| target.0);
    targets.dedup();
    let mut elsewhere = 0;
    for other in targets {
        let used = uses_in(&function.blocks[other.0 as usize], *scrutinee);
        if other == chosen && used > 0 {
            return false;
        }
        elsewhere += used;
    }
    uses[scrutinee.0 as usize] == 1 + elsewhere
}

/// ブロックの文と終端が `var` を使う数。
fn uses_in(block: &Block, var: VarId) -> usize {
    let mut count = 0;
    let mut add = |atom: Atom| count += usize::from(atom == Atom::Var(var));
    block
        .stmts
        .iter()
        .for_each(|stmt| stmt.for_each_atom(&mut add));
    block.term.for_each_atom(&mut add);
    count
}

/// `unpack` で始まる `M` への `jump` を、`unpack` の後ろの部分へつなぐ。
fn thread_unpack(
    function: &mut CoreFn,
    uses: &[usize],
    known: &HashMap<VarId, (Ctor, Vec<Atom>)>,
    added: &mut Vec<(usize, Block)>,
    target: usize,
    args: &[Atom],
    position: usize,
) -> Option<(Term, Option<VarId>)> {
    let (var, (ctor, values)) = single_con(known, uses, args[position])?;
    let block = &function.blocks[target];
    let Stmt::Unpack {
        ctor: unpacked,
        fields,
        ..
    } = &block.stmts[0]
    else {
        unreachable!("the entry is an unpack");
    };
    if unpacked != ctor {
        return None;
    }
    if let (
        [_],
        Term::Jump {
            target: next,
            args: forwarded,
        },
    ) = (&block.stmts[..], &block.term)
    {
        let substitution: HashMap<VarId, Atom> = block
            .params
            .iter()
            .copied()
            .zip(args.iter().copied())
            .chain(fields.iter().copied().zip(values.iter().copied()))
            .collect();
        return forward(
            function,
            added,
            *next,
            forwarded,
            &substitution,
            values,
            Some(var),
        );
    }
    if !opens(function, uses, values, fields) {
        return None;
    }
    let values = values.clone();
    let fields = fields.clone();
    let join = BlockId((function.blocks.len() + added.len()) as u32);
    let params = function.blocks[target].params.clone();
    let renamed: Vec<VarId> = params
        .iter()
        .enumerate()
        .map(|(at, &param)| {
            if at == position {
                param
            } else {
                copy_var(function, param)
            }
        })
        .collect();
    let copies: Vec<VarId> = fields
        .iter()
        .map(|&field| copy_var(function, field))
        .collect();
    let mut body = std::mem::replace(
        &mut function.blocks[target],
        Block {
            params: renamed.clone(),
            stmts: vec![Stmt::Unpack {
                value: params[position],
                ctor: *ctor,
                fields: copies.clone(),
            }],
            term: Term::Jump {
                target: join,
                args: renamed
                    .iter()
                    .enumerate()
                    .filter(|&(at, _)| at != position)
                    .map(|(_, &param)| Atom::Var(param))
                    .chain(copies.iter().map(|&copy| Atom::Var(copy)))
                    .collect(),
            },
        },
    );
    body.stmts.remove(0);
    let others = args
        .iter()
        .enumerate()
        .filter(|&(at, _)| at != position)
        .map(|(_, &arg)| arg);
    let mut join_params: Vec<VarId> = params
        .iter()
        .enumerate()
        .filter(|&(at, _)| at != position)
        .map(|(_, &param)| param)
        .collect();
    join_params.extend(fields);
    added.push((
        target,
        Block {
            params: join_params,
            stmts: body.stmts,
            term: body.term,
        },
    ));
    let jumped = others.chain(values).collect();
    Some((
        Term::Jump {
            target: join,
            args: jumped,
        },
        Some(var),
    ))
}

/// すでに `jump` だけで終わる C か M へ向かう `jump` を、新しいブロックを作らずにその `jump` の行き先 `next` へ直接
/// つなぐ。`substitution` は、C か M が受け取る値を `jump` の側の値に置き換える。
fn forward(
    function: &CoreFn,
    added: &[(usize, Block)],
    next: BlockId,
    forwarded: &[Atom],
    substitution: &HashMap<VarId, Atom>,
    values: &[Atom],
    con: Option<VarId>,
) -> Option<(Term, Option<VarId>)> {
    let args = substituted(forwarded, substitution);
    let fits = args
        .iter()
        .zip(block_params(function, added, next))
        .all(|(&arg, &param)| passes(function, arg, param))
        && carried(function, values, &args);
    fits.then_some((Term::Jump { target: next, args }, con))
}

/// 直接つなぐ `jump` が、`Unr` でない `con` の引数をどれも渡すか。渡さない引数は捨てることになる (`opens`)。
fn carried(function: &CoreFn, values: &[Atom], forwarded: &[Atom]) -> bool {
    values
        .iter()
        .all(|&value| !non_unr(function, value) || forwarded.contains(&value))
}

fn block_params<'a>(function: &'a CoreFn, added: &'a [(usize, Block)], id: BlockId) -> &'a [VarId] {
    let index = id.0 as usize;
    match function.blocks.get(index) {
        Some(block) => &block.params,
        None => &added[index - function.blocks.len()].1.params,
    }
}

/// `var` と同じ名前、Repr、印の新しい変数。
fn copy_var(function: &mut CoreFn, var: VarId) -> VarId {
    let info = function.vars[var.0 as usize].clone();
    fresh(&mut function.vars, info)
}

fn substituted(atoms: &[Atom], substitution: &HashMap<VarId, Atom>) -> Vec<Atom> {
    atoms
        .iter()
        .map(|&atom| match atom {
            Atom::Var(var) => substitution.get(&var).copied().unwrap_or(atom),
            atom => atom,
        })
        .collect()
}

/// 足したブロックを、それぞれの元のブロック (C か M) の直後に入れて番号を振り直す。足したブロックの仮の番号は、元の
/// ブロックの数から足した順に続く。足したブロックへの辺は元のブロックとその前からしか来ず、足したブロックの辺は元の
/// ブロックの後ろへ向かうので、R2 を保つ。
fn insert_after(function: &mut CoreFn, added: Vec<(usize, Block)>) {
    if added.is_empty() {
        return;
    }
    let count = function.blocks.len();
    let mut after: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (position, (anchor, _)) in added.iter().enumerate() {
        after[*anchor].push(position);
    }
    let mut renumber = vec![BlockId(0); count + added.len()];
    let mut next = 0u32;
    for (index, inserted) in after.iter().enumerate() {
        renumber[index] = BlockId(next);
        next += 1;
        for &position in inserted {
            renumber[count + position] = BlockId(next);
            next += 1;
        }
    }
    let mut added: Vec<Option<Block>> = added.into_iter().map(|(_, block)| Some(block)).collect();
    let old = std::mem::take(&mut function.blocks);
    let mut out = Vec::with_capacity(next as usize);
    for (block, inserted) in old.into_iter().zip(&after) {
        out.push(block);
        for &position in inserted {
            out.push(added[position].take().expect("a block is inserted once"));
        }
    }
    for block in &mut out {
        block
            .term
            .for_each_successor_mut(|target| *target = renumber[target.0 as usize]);
    }
    function.blocks = out;
}

/// 既知の閉包への `apply` を直接の呼び出しにする (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md
/// の「書き換え」)。解いた `con` のフィールドを置き換えると、`closure` で束縛した変数や関数の値への `apply` が新しく
/// 現れるためである。`Unr` でない閉包は、使う所がすべて書き換わるときだけ書き換える。書き換えない使う所が残ると、
/// 捕獲した値を閉包と直接の呼び出しの2か所で消費するためである (S13e-1 と同じ規則)。書き換えた文の数と、書き換えで
/// 生まれた直接の呼び出しの結果の変数を返す。生まれた呼び出しは、呼ぶ側がインライン化の候補にする。
pub(super) fn call_known_closures(function: &mut CoreFn, callees: &[Callee]) -> (bool, Vec<VarId>) {
    let closures = known_closures(function);
    let mut total: HashMap<VarId, usize> = HashMap::new();
    let mut applied: HashMap<VarId, Vec<bool>> = HashMap::new();
    for block in &function.blocks {
        for stmt in &block.stmts {
            stmt.for_each_atom(|atom| {
                if let Atom::Var(var) = atom {
                    *total.entry(var).or_default() += 1;
                }
            });
            if let Stmt::Let {
                var,
                rhs:
                    Rhs::Call {
                        call:
                            Call::Apply {
                                callee: Atom::Var(callee),
                                args,
                                ..
                            },
                        mask,
                    },
            } = stmt
                && let Some(closure) = closures.get(callee)
            {
                let rewritable =
                    apply_rewritable(&function.vars, closure, *var, args, mask, callees);
                applied.entry(*callee).or_default().push(rewritable);
            }
        }
        block.term.for_each_atom(|atom| {
            if let Atom::Var(var) = atom {
                *total.entry(var).or_default() += 1;
            }
        });
    }
    let known: HashMap<VarId, KnownClosure> = closures
        .into_iter()
        .filter(|(var, _)| {
            let Some(applies) = applied.get(var) else {
                return false;
            };
            function.mark(*var) == Mark::Unr
                || (applies.len() == total[var] && applies.iter().all(|&ok| ok))
        })
        .collect();
    let mut changed = false;
    let mut produced = Vec::new();
    for block in &mut function.blocks {
        if !block.stmts.iter().any(|stmt| applies_known(stmt, &known)) {
            continue;
        }
        let mut out = Vec::with_capacity(block.stmts.len());
        for stmt in std::mem::take(&mut block.stmts) {
            let replaced = match &stmt {
                Stmt::Let { var, rhs } if applies_known(&stmt, &known) => {
                    replace(&mut function.vars, &known, callees, *var, rhs)
                }
                _ => None,
            };
            match replaced {
                Some(stmts) => {
                    changed = true;
                    produced.extend(stmts.iter().filter_map(|stmt| match stmt {
                        Stmt::Let {
                            var,
                            rhs:
                                Rhs::Call {
                                    call: Call::Direct(_, _),
                                    mask: _,
                                },
                        } => Some(*var),
                        _ => None,
                    }));
                    out.extend(stmts);
                }
                None => out.push(stmt),
            }
        }
        block.stmts = out;
    }
    (changed, produced)
}

/// 既知の閉包か関数の値への `apply` か。
fn applies_known(stmt: &Stmt, known: &HashMap<VarId, KnownClosure>) -> bool {
    match stmt {
        Stmt::Let {
            var: _,
            rhs:
                Rhs::Call {
                    call: Call::Apply { callee, .. },
                    mask: _,
                },
        } => match callee {
            Atom::Fn(_) => true,
            Atom::Var(var) => known.contains_key(var),
            _ => false,
        },
        _ => false,
    }
}
