//! 本体の型を書き出すときの、本体の線形性の Kind 変数の値
//! (docs/spec/types.md の「書き出すときの規則」と docs/implementation/architecture.md の「`eml_types` の内部」)。
//! 値は、SCC で合わせた制約のグラフの成分の上で、メンバーの宣言自身の変数で止まりながら下にたどって決める。

use std::collections::HashMap;
use std::rc::Rc;

use super::KindVar;
use super::eval::{SchemeFacts, scheme_facts};
use super::problem::KindScheme;
use super::solve::{LinSolution, kept_components};
use crate::store::Lin;
use crate::ty::Linearity;

/// 成分から下にたどって届くもの。
#[derive(Clone)]
enum Below {
    /// `Lin` に畳まれた宣言自身の変数に届く。下に定数の `Lin` がない成分からたどるので、誤りのあるプログラムでだけ
    /// 起こる。
    Lin,
    /// 届いた宣言自身の変数。成分ごとに番号のいちばん小さい変数を持ち、`Unr` に畳まれた変数は除く。小さい順に並ぶ。
    Vars(Rc<[u32]>),
}

/// SCC の1つのメンバーの、表の Kind 変数から書き出す値への写像。`joins` は初めて現れた順に番号を振るので、書き出す順に
/// 引く。
pub(crate) struct BodyKinds<'s> {
    solution: &'s LinSolution,
    /// 表の番号から合わせた番号へのずれ。
    offset: usize,
    lin_vars: usize,
    /// 宣言自身の変数を含む成分と、その中の変数の `Shape` の番号 (小さい順)。
    kept: HashMap<usize, Vec<usize>>,
    /// `kept` のうち、下向きに何もない成分。
    kept_inert: Vec<usize>,
    /// `kept` の成分の番号の最大。
    last_kept: usize,
    facts: SchemeFacts,
    /// 成分ごとの覚え。止まる所がメンバーごとに違うので、メンバーごとに作り直す。
    below: HashMap<usize, Below>,
    joins: Vec<Vec<u32>>,
    numbers: HashMap<Rc<[u32]>, u32>,
}

impl<'s> BodyKinds<'s> {
    /// `scheme` は同じ SCC の解が `member` に残したスキームである。畳み込みは古いスキームでなくこれで決める。
    pub fn new(
        solution: &'s LinSolution,
        member: usize,
        scheme: &KindScheme,
        lin_vars: usize,
    ) -> BodyKinds<'s> {
        let kept = kept_components(&solution.graph, member);
        let mut kept_inert: Vec<usize> = kept
            .keys()
            .copied()
            .filter(|&component| solution.graph.downward.inert[component])
            .collect();
        kept_inert.sort_unstable();
        let last_kept = kept.keys().copied().max().unwrap_or(0);
        BodyKinds {
            solution,
            offset: solution.offsets[member],
            lin_vars,
            kept,
            kept_inert,
            last_kept,
            facts: scheme_facts(scheme, lin_vars),
            below: HashMap::new(),
            joins: Vec::new(),
            numbers: HashMap::new(),
        }
    }

    /// 表の Kind 変数 `var` を書き出す値。
    pub fn lin(&mut self, var: KindVar) -> Lin {
        let merged = var.index() + self.offset;
        // 止まる所がなければ集合はいつも空なので、たどらずに最小解を使う。値は下の規則と同じになる
        if self.lin_vars == 0 {
            return constant(self.solution.values[merged]);
        }
        let component = self.solution.graph.component[merged];
        if let Some(shapes) = self.kept.get(&component) {
            return self.fold(shapes[0]);
        }
        // 最小解が `Lin` なのは、下に定数の `Lin` があるときである。止まる成分を通って届くときも、その宣言自身の変数は
        // スキームで `Lin` に畳まれるので、規則の値は同じ `Lin` になる。最小解が `Unr` なら下に定数の `Lin` はなく、
        // たどって求めるのは届く宣言自身の変数の集合だけでよい
        if self.solution.values[merged] == Linearity::Lin {
            return Lin::Lin;
        }
        match self.reach(component) {
            Below::Lin => Lin::Lin,
            Below::Vars(vars) => self.number(vars),
        }
    }

    /// 本体の `Var(lin_vars + k)` が表す集合 `joins[k]`。
    pub fn into_joins(self) -> Vec<Vec<u32>> {
        self.joins
    }

    /// 宣言自身の変数の値。定数だけで決まる変数は、宣言の型と同じく定数に畳む。
    fn fold(&self, var: usize) -> Lin {
        if self.facts.fixed_unr[var] {
            Lin::Unr
        } else if self.facts.forced_lin[var] {
            Lin::Lin
        } else {
            Lin::Var(var as u32)
        }
    }

    fn number(&mut self, vars: Rc<[u32]>) -> Lin {
        match *vars {
            [] => Lin::Unr,
            [var] => Lin::Var(var),
            _ => {
                let next = (self.lin_vars + self.joins.len()) as u32;
                let joins = &mut self.joins;
                let number = *self.numbers.entry(vars).or_insert_with_key(|vars| {
                    joins.push(vars.to_vec());
                    next
                });
                Lin::Var(number)
            }
        }
    }

    /// `start` の成分から下に届くもの。成分の DAG を明示的なスタックで後順にたどり、各成分を1回だけ計算する。長い鎖でも
    /// Rust のスタックを使い切らないようにするため。
    fn reach(&mut self, start: usize) -> Below {
        let mut stack = vec![(start, None)];
        while let Some((component, next)) = stack.pop() {
            if self.below.contains_key(&component) {
                continue;
            }
            let Some(next) = next else {
                let next = self.next(component);
                let unknown: Vec<usize> = next
                    .iter()
                    .copied()
                    .filter(|n| !self.kept.contains_key(n) && !self.below.contains_key(n))
                    .collect();
                stack.push((component, Some(next)));
                stack.extend(unknown.into_iter().map(|n| (n, None)));
                continue;
            };
            let mut lin = false;
            let mut vars = Vec::new();
            for n in next {
                let found = match self.kept.get(&n) {
                    Some(shapes) => match self.fold(shapes[0]) {
                        Lin::Lin => Below::Lin,
                        Lin::Var(var) => Below::Vars(Rc::from([var])),
                        _ => continue,
                    },
                    None => self.below[&n].clone(),
                };
                match found {
                    Below::Lin => lin = true,
                    Below::Vars(found) => vars.extend_from_slice(&found),
                }
            }
            let below = if lin {
                Below::Lin
            } else {
                vars.sort_unstable();
                vars.dedup();
                Below::Vars(vars.into())
            };
            self.below.insert(component, below);
        }
        self.below[&start].clone()
    }

    /// 下向きの隣のうち、止まる成分に届きうるもの。下向きの隣は番号が大きい (`Graph::new`) ので、止まる成分の番号の
    /// 最大より大きい成分からは止まる成分に届かない。そこで止まると、ほかのメンバーの変数の長い鎖の上にある変数でも、
    /// 鎖をたどらずに済む。何もない成分は、止まる成分でなければ何も足さないので見ない。何もない止まる成分は、
    /// `Graph::reach` と同じく、隣と止まる成分の少ないほうから探す。
    fn next(&self, component: usize) -> Vec<usize> {
        let edges = &self.solution.graph.downward;
        let active = &edges.active[component];
        let mut next = active[..active.partition_point(|&n| n <= self.last_kept)].to_vec();
        let owned = &edges.owned_inert[component];
        if owned.len() <= self.kept_inert.len() {
            next.extend(owned.iter().filter(|n| self.kept.contains_key(n)));
        } else {
            next.extend(
                self.kept_inert
                    .iter()
                    .filter(|n| edges.all[component].binary_search(n).is_ok()),
            );
        }
        next
    }
}

fn constant(value: Linearity) -> Lin {
    match value {
        Linearity::Unr => Lin::Unr,
        Linearity::Lin => Lin::Lin,
    }
}
