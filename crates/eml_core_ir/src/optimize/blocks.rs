//! ブロックの並びを整える部品。handler への特殊化 (`handlers`) と融合 (`fuse`) が共有する。どれも構造の規則
//! (docs/spec/lir.md の「構造の規則」) を保ったまま、定数で分ける `switch`、辺の入らないブロック、1本の `jump` だけが
//! 入るブロックを片付ける。本体を写すときの変数の付け替え (`Renamer`) もここに置く。

use crate::{Block, CoreFn, Stmt, Term};
use eml_lir::{Atom, BlockId, CasePattern, VarId, VarInfo, push_var};

/// エントリから届かないブロックを落とし、残ったブロックに元の順のまま番号を振り直す。展開や `switch` の畳み込みで
/// 切り離したブロックは辺が入らなくなるので、残すと構造の規則に反する (docs/spec/lir.md の「構造の規則」)。融合の
/// 書き換えのたびに呼ぶので、印と番号の表はブロックの数の配列にする。
pub(crate) fn prune(function: &mut CoreFn) {
    let count = function.blocks.len();
    let mut reached = vec![false; count];
    reached[BlockId::ENTRY.0 as usize] = true;
    let mut pending = vec![BlockId::ENTRY];
    let mut kept = 1;
    while let Some(block) = pending.pop() {
        for next in function.blocks[block.0 as usize].term.successors() {
            if !reached[next.0 as usize] {
                reached[next.0 as usize] = true;
                kept += 1;
                pending.push(next);
            }
        }
    }
    if kept == count {
        return;
    }
    let mut renumber = vec![BlockId::ENTRY; count];
    let mut next = 0;
    for (index, &reached) in reached.iter().enumerate() {
        if reached {
            renumber[index] = BlockId(next);
            next += 1;
        }
    }
    let mut reached = reached.into_iter();
    function.blocks.retain(|_| reached.next() == Some(true));
    for block in &mut function.blocks {
        block
            .term
            .for_each_successor_mut(|target| *target = renumber[target.0 as usize]);
    }
}

fn empty() -> Block {
    Block {
        params: Vec::new(),
        stmts: Vec::new(),
        term: Term::Return(Atom::Unit),
    }
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
        // `float` の値で分ける `switch` はない
        Atom::Var(_) | Atom::Float(_) | Atom::Unit | Atom::Fn(_) => return None,
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
/// つなげて定数で分けるようになった `switch` も畳むので、片付けは1回で済む。辺の入らなくなったブロックは、呼ぶ側が
/// `prune` で落とす。
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
            // 引数を定数で置き換えた `switch` は、ここで畳む。畳んだ行き先も1本の `jump` だけが入るので、続けてつなげる
            if let Term::Switch { scrutinee, .. } = &mut host.term {
                *scrutinee = resolve(&substitution, *scrutinee);
            }
            if let Some(kept) = constant_target(&host.term) {
                // 行き先ごとの辺を数えたので、残す行き先への辺も1本を除いて減らす
                let mut kept_once = false;
                host.term.for_each_successor_mut(|dropped| {
                    if *dropped == kept && !kept_once {
                        kept_once = true;
                    } else {
                        edges[dropped.0 as usize] -= 1;
                    }
                });
                host.term = Term::Jump {
                    target: kept,
                    args: Vec::new(),
                };
            }
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

/// 関数の本体を別の関数へ写すときの変数の付け替え。展開 (`fuse`) と節の写し (`handlers`) が使う。写した先の変数は
/// `vars` に足し、`map` は写す元の変数ごとの写した先の値である。
pub(crate) struct Renamer<'a> {
    pub(crate) vars: &'a mut Vec<VarInfo>,
    source: &'a [VarInfo],
    map: Vec<Option<Atom>>,
}

impl<'a> Renamer<'a> {
    pub(crate) fn new(vars: &'a mut Vec<VarInfo>, source: &'a [VarInfo]) -> Self {
        Renamer {
            vars,
            source,
            map: vec![None; source.len()],
        }
    }

    /// 写す元の変数 `var` を、定義を写さずに値 `atom` に置き換える。引数を実引数にするのに使う。
    pub(crate) fn substitute(&mut self, var: VarId, atom: Atom) {
        self.map[var.0 as usize] = Some(atom);
    }

    fn define(&mut self, var: VarId) -> VarId {
        let copy = push_var(self.vars, self.source[var.0 as usize].clone());
        self.map[var.0 as usize] = Some(Atom::Var(copy));
        copy
    }

    /// 定義は使いを支配し、ブロックは番号の順に写すので、使いを写すときには定義が写してある。
    pub(crate) fn atom(&self, atom: Atom) -> Atom {
        match atom {
            Atom::Var(var) => {
                self.map[var.0 as usize].expect("a definition is copied before its uses")
            }
            atom => atom,
        }
    }

    /// `unpack` の値は `obj` で、`obj` に収まる定数はないので (`Atom::fits`)、`fits` を通った実引数は変数である。
    fn var(&self, var: VarId) -> VarId {
        match self.atom(Atom::Var(var)) {
            Atom::Var(var) => var,
            atom => panic!("a variable is replaced by the constant {atom:?}"),
        }
    }

    /// `index` 番目のブロックの引数を写す。エントリのブロックの引数は実引数に置き換えるので、写さない。
    pub(crate) fn params(&mut self, index: usize, block: &Block) -> Vec<VarId> {
        if index == 0 {
            return Vec::new();
        }
        block
            .params
            .iter()
            .map(|&param| self.define(param))
            .collect()
    }

    pub(crate) fn stmt(&mut self, stmt: &Stmt) -> Stmt {
        match stmt {
            Stmt::Let { var, rhs } => {
                let mut rhs = rhs.clone();
                rhs.for_each_atom_mut(|atom| *atom = self.atom(*atom));
                Stmt::Let {
                    var: self.define(*var),
                    rhs,
                }
            }
            Stmt::Unpack {
                value,
                ctor,
                fields,
            } => Stmt::Unpack {
                value: self.var(*value),
                ctor: *ctor,
                fields: fields.iter().map(|&field| self.define(field)).collect(),
            },
        }
    }

    /// 終端を写す。辺が入らなくなる case のブロックも写すので、そのフィールドも定義する。
    pub(crate) fn term(&mut self, term: &Term, block_of: impl Fn(BlockId) -> BlockId) -> Term {
        let mut term = term.clone();
        term.for_each_atom_mut(|atom| *atom = self.atom(*atom));
        term.for_each_successor_mut(|target| *target = block_of(*target));
        if let Term::Switch { cases, .. } = &mut term {
            for case in cases {
                case.fields = case
                    .fields
                    .iter()
                    .map(|&field| self.define(field))
                    .collect();
            }
        }
        term
    }
}
