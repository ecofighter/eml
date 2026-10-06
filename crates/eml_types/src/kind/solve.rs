//! 段2: SCC ごとに Kind の問題をまとめて解き、違反を集め、各宣言のスキームに残す制約を求める
//! (docs/spec/types.md の「推論」)。型の表は使わず、番号の上の束だけを扱う。

use std::collections::{HashMap, HashSet};

use super::problem::{Bounds, Decl, KindProblem, KindScheme, OwnVars};
use super::{Bound, CarriedInner, Carry, KindOrigin, KindReason, KindVar, Level};
use crate::ty::{Linearity, Multiplicity};

/// 1つの SCC を解いた結果。`schemes` は `members` と同じ順に並ぶ。
pub(crate) struct Solution {
    pub schemes: Vec<KindScheme>,
    /// 由来のある違反の由来。並べ替えと重複除去は、呼ぶ側がモジュール全体でまとめて行う。
    pub violated: Vec<KindOrigin>,
}

/// `members` は1つの SCC の宣言とその問題、`schemes` は前の SCC と本体のない宣言のスキームである。
pub(crate) fn solve_scc(
    members: &[(Decl, &KindProblem)],
    schemes: &HashMap<Decl, KindScheme>,
) -> Solution {
    let merged = merge(members, schemes);
    let (lin, lin_violated) = solve(&merged.lin);
    let (mult, mult_violated) = solve(&merged.mult);
    let carry_violated = violated_carries(&merged.carries, &lin, &mult);
    let violated = lin_violated
        .iter()
        .filter_map(|&index| merged.lin.origins[index].clone())
        .chain(
            mult_violated
                .iter()
                .filter_map(|&index| merged.mult.origins[index].clone()),
        )
        .chain(
            carry_violated
                .iter()
                .filter_map(|&index| merged.carries[index].origin.clone()),
        )
        .collect();
    let lin_graph = Graph::new(
        &merged.lin,
        merged.own.iter().map(|own| own.lin.clone()).collect(),
    );
    let mult_graph = Graph::new(
        &merged.mult,
        merged.own.iter().map(|own| own.mult.clone()).collect(),
    );
    let schemes = (0..merged.own.len())
        .map(|member| KindScheme {
            lin: residual(&lin_graph, member),
            mult: residual(&mult_graph, member),
            carries: carry_residual(&merged.carries, &lin_graph, &mult_graph, member),
        })
        .collect();
    Solution { schemes, violated }
}

/// SCC の問題を1つの番号の空間に並べ、具体化の記録を展開したもの。
struct Merged {
    lin: Bounds<Linearity>,
    mult: Bounds<Multiplicity>,
    carries: Vec<Carry>,
    /// 各宣言の自分の Kind 変数。まとめた後の番号である。
    own: Vec<OwnVars>,
}

fn merge(members: &[(Decl, &KindProblem)], schemes: &HashMap<Decl, KindScheme>) -> Merged {
    let mut offsets = Vec::new();
    let (mut lin_vars, mut mult_vars) = (0, 0);
    for (_, problem) in members {
        offsets.push((lin_vars, mult_vars));
        lin_vars += problem.lin.vars;
        mult_vars += problem.mult.vars;
    }
    let position: HashMap<Decl, usize> = members
        .iter()
        .enumerate()
        .map(|(index, (decl, _))| (*decl, index))
        .collect();
    let own = members
        .iter()
        .zip(&offsets)
        .map(|((_, problem), &(l, m))| OwnVars {
            lin: problem.own.lin.iter().map(|&v| shift_var(v, l)).collect(),
            mult: problem.own.mult.iter().map(|&v| shift_var(v, m)).collect(),
        })
        .collect();
    let mut merged = Merged {
        lin: Bounds {
            vars: lin_vars,
            ..Bounds::default()
        },
        mult: Bounds {
            vars: mult_vars,
            ..Bounds::default()
        },
        carries: Vec::new(),
        own,
    };
    for ((_, problem), &(l, m)) in members.iter().zip(&offsets) {
        merged.copy_own(problem, (l, m));
        for instance in &problem.instances {
            let lin: Vec<KindVar> = instance.lin.iter().map(|&v| shift_var(v, l)).collect();
            let mult: Vec<KindVar> = instance.mult.iter().map(|&v| shift_var(v, m)).collect();
            match position.get(&instance.decl) {
                Some(&callee) => merged.equate(&lin, &mult, callee),
                None => {
                    let scheme = schemes
                        .get(&instance.decl)
                        .expect("a callee is solved before its callers");
                    merged.copy_scheme(scheme, &lin, &mult, instance.origin.as_ref());
                }
            }
        }
    }
    merged
}

impl Merged {
    /// 宣言の自分の制約を、番号をずらして足す。
    fn copy_own(&mut self, problem: &KindProblem, (l, m): (usize, usize)) {
        for (&(lower, upper), origin) in problem.lin.constraints.iter().zip(&problem.lin.origins) {
            self.lin
                .require(shift(lower, l), shift(upper, l), origin.clone());
        }
        for (&(lower, upper), origin) in problem.mult.constraints.iter().zip(&problem.mult.origins)
        {
            self.mult
                .require(shift(lower, m), shift(upper, m), origin.clone());
        }
        for carry in &problem.carries {
            self.carries.push(Carry {
                lin: shift(carry.lin, l),
                mult: shift(carry.mult, m),
                origin: carry.origin.clone(),
            });
        }
    }

    /// 同じ SCC の宣言の参照は、多相化する前の Kind 変数を共有するのと同じ解にする (docs/spec/types.md の「推論」)。
    /// 変数どうしの制約は違反にならないので、由来は付けない。
    fn equate(&mut self, lin: &[KindVar], mult: &[KindVar], callee: usize) {
        debug_assert_eq!(lin.len(), self.own[callee].lin.len());
        debug_assert_eq!(mult.len(), self.own[callee].mult.len());
        for (&v, &w) in lin.iter().zip(&self.own[callee].lin) {
            self.lin.require(Bound::Var(v), Bound::Var(w), None);
            self.lin.require(Bound::Var(w), Bound::Var(v), None);
        }
        for (&v, &w) in mult.iter().zip(&self.own[callee].mult) {
            self.mult.require(Bound::Var(v), Bound::Var(w), None);
            self.mult.require(Bound::Var(w), Bound::Var(v), None);
        }
    }

    /// 前の SCC の宣言のスキームを、具体化した変数について足す。参照した位置の由来 (`Passed`) は、呼んだ関数の中の
    /// 持ち越しを指す `CarriedThrough` にする。呼んだ側の違反が、呼んだ関数の中の呼び出しを指せるようにするため
    /// (docs/spec/diagnostics.md の E3006)。
    fn copy_scheme(
        &mut self,
        scheme: &KindScheme,
        lin: &[KindVar],
        mult: &[KindVar],
        origin: Option<&KindOrigin>,
    ) {
        let rename_lin = |bound: Bound<Linearity>| match bound {
            Bound::Var(v) => Bound::Var(lin[v.index()]),
            constant => constant,
        };
        let rename_mult = |bound: Bound<Multiplicity>| match bound {
            Bound::Var(v) => Bound::Var(mult[v.index()]),
            constant => constant,
        };
        for &(lower, upper) in &scheme.lin {
            self.lin
                .require(rename_lin(lower), rename_lin(upper), origin.cloned());
        }
        for &(lower, upper) in &scheme.mult {
            self.mult
                .require(rename_mult(lower), rename_mult(upper), origin.cloned());
        }
        for carry in &scheme.carries {
            let origin = origin.map(|origin| match &origin.reason {
                KindReason::Passed(name) => KindOrigin {
                    range: origin.range,
                    reason: KindReason::CarriedThrough {
                        name: name.clone(),
                        inner: carry.origin.as_ref().map(CarriedInner::of),
                    },
                },
                _ => origin.clone(),
            });
            self.carries.push(Carry {
                lin: rename_lin(carry.lin),
                mult: rename_mult(carry.mult),
                origin,
            });
        }
    }
}

fn shift_var(var: KindVar, offset: usize) -> KindVar {
    KindVar::from_index(var.index() + offset)
}

fn shift<T>(bound: Bound<T>, offset: usize) -> Bound<T> {
    match bound {
        Bound::Var(v) => Bound::Var(shift_var(v, offset)),
        constant => constant,
    }
}

/// 最小解と、満たせなかった制約 (定数の上限を超えたもの) の番号。上がった変数をワークリストに入れ、上向きの辺に沿って
/// 伝える。束の高さが小さいので、各変数が上がる回数は高々その高さで、時間は制約の数に比例する。
pub(crate) fn solve<T: Level>(bounds: &Bounds<T>) -> (Vec<T>, Vec<usize>) {
    let mut values = vec![T::BOTTOM; bounds.vars];
    let mut upward: Vec<Vec<usize>> = vec![Vec::new(); bounds.vars];
    let mut work = Vec::new();
    for &(lower, upper) in &bounds.constraints {
        match (lower, upper) {
            (Bound::Var(a), Bound::Var(b)) => upward[a.index()].push(b.index()),
            (Bound::Const(c), Bound::Var(b)) if c > values[b.index()] => {
                values[b.index()] = c;
                work.push(b.index());
            }
            _ => {}
        }
    }
    while let Some(v) = work.pop() {
        for &w in &upward[v] {
            if values[v] > values[w] {
                values[w] = values[v];
                work.push(w);
            }
        }
    }
    let value = |bound: Bound<T>| match bound {
        Bound::Const(c) => c,
        Bound::Var(v) => values[v.index()],
    };
    let violated = bounds
        .constraints
        .iter()
        .enumerate()
        .filter(|(_, (lower, upper))| matches!(upper, Bound::Const(c) if value(*lower) > *c))
        .map(|(index, _)| index)
        .collect();
    (values, violated)
}

/// 破れた持ち越しの制約の番号。持ち越しの制約はどちらの束の最小解も動かさないので、解いた後に確かめればよい。
pub(crate) fn violated_carries(
    carries: &[Carry],
    lin: &[Linearity],
    mult: &[Multiplicity],
) -> Vec<usize> {
    let lin_value = |bound: Bound<Linearity>| match bound {
        Bound::Const(c) => c,
        Bound::Var(v) => lin[v.index()],
    };
    let mult_value = |bound: Bound<Multiplicity>| match bound {
        Bound::Const(c) => c,
        Bound::Var(v) => mult[v.index()],
    };
    carries
        .iter()
        .enumerate()
        .filter(|(_, carry)| {
            lin_value(carry.lin) == Linearity::Lin && mult_value(carry.mult) == Multiplicity::Multi
        })
        .map(|(index, _)| index)
        .collect()
}

/// 変数どうしの制約のグラフを強連結成分に縮めたもの。残す制約は成分の DAG の上で求める。同じ循環に入った変数は等しいので、
/// 関数ごとに循環を1周せずに済む。
struct Graph<T> {
    /// 変数ごとの成分の番号。
    component: Vec<usize>,
    /// 成分の変数に付いた定数の上限の最小と、定数の下限の最大。
    upper: Vec<Option<T>>,
    lower: Vec<Option<T>>,
    /// SCC の宣言ごとの自分の変数 (`Shape` の番号の順)。
    own: Vec<Vec<KindVar>>,
    /// 上向き (下限から上限へ) と下向きの辺。
    upward: Edges,
    downward: Edges,
}

/// 1つの向きの辺と、たどるときに入る意味のない隣を見分けるための情報。
///
/// その向きにたどった先 (成分自身を含む) に定数がなく、成分自身のほかにどの宣言の変数を含む成分もない成分を
/// 「何もない成分」と呼ぶ。
/// 何もない成分は、ある宣言の残す成分でなければ、その宣言の残す制約 (docs/spec/types.md の「推論」) に何も足さない。
/// 環状の相互再帰では、すべての `μ` を含む成分から、各宣言の内側の矢印の Kind 変数の成分へ辺が出る。そのまま
/// たどると宣言ごとに宣言の数だけ辺を見ることになるので、何もない成分への辺は、その成分が残す成分のときだけ見る。
struct Edges {
    /// 成分ごとの隣。番号の順に並べ、重複は除いてある。
    all: Vec<Vec<usize>>,
    /// 成分ごとの、何もない成分かどうか。
    inert: Vec<bool>,
    /// 成分ごとの、何もない成分ではない隣。
    active: Vec<Vec<usize>>,
    /// 成分ごとの、何もない成分で、どこかの宣言の変数を含む隣。誰の変数も含まない何もない成分は、どの宣言にも
    /// 何も足さないので持たない。
    owned_inert: Vec<Vec<usize>>,
}

impl Edges {
    /// `all` は成分ごとの1つの向きの隣 (番号の順)、`bounds` はその向きの定数、`owners` は成分ごとの変数を含む宣言の
    /// 数である。`descending` は、隣の成分の番号が自分より大きいことを表す。
    fn new<T>(
        all: Vec<Vec<usize>>,
        bounds: &[Option<T>],
        owners: &[usize],
        descending: bool,
    ) -> Edges {
        let count = all.len();
        // 隣の成分を先に決めるため、隣の番号の側から順に見る
        let order: Vec<usize> = if descending {
            (0..count).rev().collect()
        } else {
            (0..count).collect()
        };
        let mut inert = vec![false; count];
        for component in order {
            debug_assert!(
                all[component]
                    .iter()
                    .all(|&next| (next > component) == descending)
            );
            inert[component] = bounds[component].is_none()
                && all[component]
                    .iter()
                    .all(|&next| inert[next] && owners[next] == 0);
        }
        let active = all
            .iter()
            .map(|list| list.iter().copied().filter(|&next| !inert[next]).collect())
            .collect();
        let owned_inert = all
            .iter()
            .map(|list| {
                list.iter()
                    .copied()
                    .filter(|&next| inert[next] && owners[next] > 0)
                    .collect()
            })
            .collect();
        Edges {
            all,
            inert,
            active,
            owned_inert,
        }
    }
}

impl<T: Level> Graph<T> {
    /// `own` は SCC の宣言ごとの自分の変数である。
    fn new(bounds: &Bounds<T>, own: Vec<Vec<KindVar>>) -> Graph<T> {
        let mut edges = vec![Vec::new(); bounds.vars];
        for &(lower, upper) in &bounds.constraints {
            if let (Bound::Var(a), Bound::Var(b)) = (lower, upper) {
                edges[a.index()].push(b.index());
            }
        }
        let (component, count) = components(&edges);
        let mut upward = vec![Vec::new(); count];
        let mut downward = vec![Vec::new(); count];
        for (a, targets) in edges.iter().enumerate() {
            for &b in targets {
                let (from, to) = (component[a], component[b]);
                if from != to {
                    upward[from].push(to);
                    downward[to].push(from);
                }
            }
        }
        for list in upward.iter_mut().chain(downward.iter_mut()) {
            list.sort_unstable();
            list.dedup();
        }
        let mut upper: Vec<Option<T>> = vec![None; count];
        let mut lower: Vec<Option<T>> = vec![None; count];
        for &(l, u) in &bounds.constraints {
            match (l, u) {
                (Bound::Var(v), Bound::Const(c)) => {
                    let slot = &mut upper[component[v.index()]];
                    *slot = Some(slot.map_or(c, |old| old.min(c)));
                }
                (Bound::Const(c), Bound::Var(v)) => {
                    let slot = &mut lower[component[v.index()]];
                    *slot = Some(slot.map_or(c, |old| old.max(c)));
                }
                _ => {}
            }
        }
        // 成分ごとの、変数を含む宣言の数
        let mut owners = vec![0; count];
        let mut last_owner: Vec<Option<usize>> = vec![None; count];
        for (member, vars) in own.iter().enumerate() {
            for var in vars {
                let c = component[var.index()];
                if last_owner[c] != Some(member) {
                    last_owner[c] = Some(member);
                    owners[c] += 1;
                }
            }
        }
        // Tarjan は、成分からたどれる成分に番号を振り終えてからその成分に番号を振る。上向きの隣は番号が小さく、
        // 下向きの隣は番号が大きい
        let upward = Edges::new(upward, &upper, &owners, false);
        let downward = Edges::new(downward, &lower, &owners, true);
        Graph {
            component,
            upper,
            lower,
            own,
            upward,
            downward,
        }
    }

    /// `start` の成分から辺をたどる。`up` なら上向き、そうでなければ下向きである。`kept` の成分には入らずにそこで止まり、
    /// 出会った `kept` の成分と、通った成分 (`start` を含む) の定数のうち、上向きなら上限の最小、下向きなら下限の最大を
    /// 返す。止まった成分の先は、その成分の残す制約が受け持つ。
    fn reach(
        &self,
        start: usize,
        kept: &HashMap<usize, Vec<usize>>,
        up: bool,
    ) -> (Vec<usize>, Option<T>) {
        let (edges, bounds) = if up {
            (&self.upward, &self.upper)
        } else {
            (&self.downward, &self.lower)
        };
        let pick = |a: T, b: T| if up { a.min(b) } else { a.max(b) };
        // 何もない成分のうち残す成分は、入らずに止まる成分として見つける。ほかの何もない成分は、入っても定数も
        // 残す成分も見つからないので見ない
        let mut kept_inert: Vec<usize> = kept.keys().copied().filter(|&c| edges.inert[c]).collect();
        kept_inert.sort_unstable();
        let mut found = Vec::new();
        let mut constant = bounds[start];
        let mut seen = HashSet::from([start]);
        let mut work = vec![start];
        while let Some(component) = work.pop() {
            // 何もない隣と何もない残す成分の少ないほうから探す。多くの宣言の残す成分に辺が出る成分でも、宣言ごとに
            // 見る量は自分の変数の数で済む
            let owned = &edges.owned_inert[component];
            if owned.len() <= kept_inert.len() {
                for &next in owned {
                    if kept.contains_key(&next) && seen.insert(next) {
                        found.push(next);
                    }
                }
            } else {
                for &next in &kept_inert {
                    if edges.all[component].binary_search(&next).is_ok() && seen.insert(next) {
                        found.push(next);
                    }
                }
            }
            for &next in &edges.active[component] {
                if !seen.insert(next) {
                    continue;
                }
                if kept.contains_key(&next) {
                    found.push(next);
                    continue;
                }
                if let Some(c) = bounds[next] {
                    constant = Some(constant.map_or(c, |old| pick(old, c)));
                }
                work.push(next);
            }
        }
        (found, constant)
    }
}

/// Tarjan の方法で強連結成分の番号を振る。変数が多くても Rust のスタックを使わないよう、明示的なスタックでたどる
/// (`scc.rs` と同じ形)。
fn components(edges: &[Vec<usize>]) -> (Vec<usize>, usize) {
    let unvisited = usize::MAX;
    let n = edges.len();
    let mut index = vec![unvisited; n];
    let mut low = vec![0; n];
    let mut on_stack = vec![false; n];
    let mut component = vec![unvisited; n];
    let mut stack = Vec::new();
    let mut next = 0;
    let mut count = 0;
    for root in 0..n {
        if index[root] != unvisited {
            continue;
        }
        let mut work = vec![(root, 0)];
        index[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(&(v, edge)) = work.last() {
            if let Some(&w) = edges[v].get(edge) {
                work.last_mut().unwrap().1 += 1;
                if index[w] == unvisited {
                    index[w] = next;
                    low[w] = next;
                    next += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    work.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
                continue;
            }
            work.pop();
            if let Some(&(parent, _)) = work.last() {
                low[parent] = low[parent].min(low[v]);
            }
            if low[v] == index[v] {
                loop {
                    let w = stack.pop().unwrap();
                    on_stack[w] = false;
                    component[w] = count;
                    if w == v {
                        break;
                    }
                }
                count += 1;
            }
        }
    }
    (component, count)
}

/// 宣言 `member` の自分の変数を含む成分と、その中の自分の変数の `Shape` の番号 (小さい順)。
fn kept_components<T>(graph: &Graph<T>, member: usize) -> HashMap<usize, Vec<usize>> {
    let mut kept: HashMap<usize, Vec<usize>> = HashMap::new();
    for (index, var) in graph.own[member].iter().enumerate() {
        kept.entry(graph.component[var.index()])
            .or_default()
            .push(index);
    }
    kept
}

fn var<T>(index: usize) -> Bound<T> {
    Bound::Var(KindVar::from_index(index))
}

/// 宣言 `member` の自分の変数 (`Shape` の番号の順) について残す制約 (docs/spec/types.md の「推論」)。自分の変数を含む成分を
/// 残す成分とし、残さない成分を通り抜けて最初に出会う残す成分までをたどる。同じ成分に入った自分の変数は等しいので、
/// 等しいことを輪で表し、境界をそのすべてに付ける。結果の変数は `Shape` の番号である。
fn residual<T: Level>(graph: &Graph<T>, member: usize) -> Vec<(Bound<T>, Bound<T>)> {
    let kept = kept_components(graph, member);
    let mut out = Vec::new();
    for (&component, shapes) in &kept {
        if shapes.len() >= 2 {
            for pair in shapes.windows(2) {
                out.push((var(pair[0]), var(pair[1])));
            }
            out.push((var(shapes[shapes.len() - 1]), var(shapes[0])));
        }
        let (targets, upper) = graph.reach(component, &kept, true);
        for target in targets {
            out.push((var(shapes[0]), var(kept[&target][0])));
        }
        let (_, lower) = graph.reach(component, &kept, false);
        for &shape in shapes {
            if let Some(c) = upper {
                out.push((var(shape), Bound::Const(c)));
            }
            // 下限が束の最小元なら何も言わないので省く
            if let Some(c) = lower.filter(|c| *c != T::BOTTOM) {
                out.push((Bound::Const(c), var(shape)));
            }
        }
    }
    // 正規形にする。同じ意味のスキームが同じ値になり、表示の並びも変数の順になる
    out.sort_by_key(|&(lower, upper)| match (lower, upper) {
        (Bound::Var(a), Bound::Var(b)) => (a.index(), 0, b.index()),
        (Bound::Var(a), Bound::Const(_)) => (a.index(), 1, 0),
        (Bound::Const(_), Bound::Var(b)) => (b.index(), 2, 0),
        (Bound::Const(_), Bound::Const(_)) => (usize::MAX, 3, 0),
    });
    out.dedup();
    out
}

/// 表の制約から、`keep` の変数について残す制約を求める。表の単体テストが使う。
#[cfg(test)]
pub(crate) fn residual_of<T: Level>(
    bounds: &Bounds<T>,
    keep: &[KindVar],
) -> Vec<(Bound<T>, Bound<T>)> {
    residual(&Graph::new(bounds, vec![keep.to_vec()]), 0)
}

/// 宣言 `member` のスキームに残す持ち越しの制約 (docs/spec/types.md の「推論」)。両側を下向きにたどり、出会った残す成分の
/// 自分の変数と定数に置き換える。同じ組は1つにまとめ、由来は位置が最も前のものを残す (docs/spec/diagnostics.md の E3006)。
/// 由来ごとに残すと、多相な関数を重ねるたびに制約が増え、同じ違反を何度も報告するためである。
fn carry_residual(
    carries: &[Carry],
    lin_graph: &Graph<Linearity>,
    mult_graph: &Graph<Multiplicity>,
    member: usize,
) -> Vec<Carry> {
    let lin_kept = kept_components(lin_graph, member);
    let mult_kept = kept_components(mult_graph, member);
    let mut best: HashMap<(Bound<Linearity>, Bound<Multiplicity>), Option<&KindOrigin>> =
        HashMap::new();
    for carry in carries {
        let lins = lowers(lin_graph, &lin_kept, carry.lin, Linearity::Lin);
        let mults = lowers(mult_graph, &mult_kept, carry.mult, Multiplicity::Multi);
        for &lin in &lins {
            for &mult in &mults {
                // 両側が定数の組はその本体の中の違反で、解いたときに報告済みである
                if matches!((lin, mult), (Bound::Const(_), Bound::Const(_))) {
                    continue;
                }
                let origin = carry.origin.as_ref();
                best.entry((lin, mult))
                    .and_modify(|kept| {
                        if earlier(origin, *kept) {
                            *kept = origin;
                        }
                    })
                    .or_insert(origin);
            }
        }
    }
    let mut out: Vec<Carry> = best
        .into_iter()
        .map(|((lin, mult), origin)| Carry {
            lin,
            mult,
            origin: origin.cloned(),
        })
        .collect();
    out.sort_by_key(|carry| (bound_key(carry.lin), bound_key(carry.mult)));
    out
}

/// `bound` の下にある残す成分の自分の変数と、下にある定数のうち `top` (値の側なら `Lin`、row の側なら `Multi`)。ほかの
/// 定数は持ち越しの制約を破らないので残さない。
fn lowers<T: Level>(
    graph: &Graph<T>,
    kept: &HashMap<usize, Vec<usize>>,
    bound: Bound<T>,
    top: T,
) -> Vec<Bound<T>> {
    let (vars, constant): (Vec<usize>, Option<T>) = match bound {
        Bound::Const(c) => (Vec::new(), Some(c)),
        Bound::Var(v) => {
            let component = graph.component[v.index()];
            match kept.get(&component) {
                // 残す成分の変数の下限は、その変数の残す制約が受け持つ
                Some(members) => (members.clone(), None),
                None => {
                    let (found, constant) = graph.reach(component, kept, false);
                    let vars = found.iter().flat_map(|c| kept[c].iter().copied()).collect();
                    (vars, constant)
                }
            }
        }
    };
    let mut out: Vec<Bound<T>> = vars.into_iter().map(var).collect();
    if constant == Some(top) {
        out.push(Bound::Const(top));
    }
    out
}

/// 由来の位置の比べ方。範囲の始まり、終わりの順に比べ、由来のないものは由来のあるものより後に置く。
fn earlier(a: Option<&KindOrigin>, b: Option<&KindOrigin>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => (a.range.start(), a.range.end()) < (b.range.start(), b.range.end()),
        (Some(_), None) => true,
        (None, _) => false,
    }
}

/// 正規形の並びの鍵。変数を番号の順に、定数より前に置く。
fn bound_key<T: Level>(bound: Bound<T>) -> (u8, usize, Option<T>) {
    match bound {
        Bound::Var(v) => (0, v.index(), None),
        Bound::Const(c) => (1, 0, Some(c)),
    }
}

#[cfg(test)]
mod tests {
    use eml_diagnostics::TextRange;
    use eml_hir::FunctionId;
    use eml_hir::builtin::Builtin;
    use la_arena::RawIdx;

    use super::*;
    use crate::kind::problem::Instance;
    use crate::kind::{CallKind, CarriedValue, InnerLabel};

    fn lattice<T>() -> Bounds<T> {
        Bounds::default()
    }

    fn function(index: u32) -> Decl {
        Decl::Function(FunctionId::from_raw(RawIdx::from(index)))
    }

    fn range(start: u32, end: u32) -> TextRange {
        TextRange::new(start.into(), end.into())
    }

    #[test]
    fn a_residual_carry_replaces_internal_variables_with_kept_ones() {
        let mut linearity = lattice::<Linearity>();
        let a = linearity.fresh();
        let internal = linearity.fresh();
        linearity.require(Bound::Var(a), Bound::Var(internal), None);
        let mut multiplicity = lattice::<Multiplicity>();
        let e = multiplicity.fresh();
        let inner = multiplicity.fresh();
        multiplicity.require(Bound::Var(e), Bound::Var(inner), None);
        let carries = [Carry {
            lin: Bound::Var(internal),
            mult: Bound::Var(inner),
            origin: None,
        }];
        assert_eq!(
            carry_residual(
                &carries,
                &Graph::new(&linearity, vec![vec![a]]),
                &Graph::new(&multiplicity, vec![vec![e]]),
                0
            ),
            vec![Carry {
                lin: Bound::Var(a),
                mult: Bound::Var(e),
                origin: None,
            }]
        );
    }

    #[test]
    fn a_residual_carry_keeps_one_constant_side() {
        let mut linearity = lattice::<Linearity>();
        let internal = linearity.fresh();
        linearity.require(Bound::Const(Linearity::Lin), Bound::Var(internal), None);
        let mut multiplicity = lattice::<Multiplicity>();
        let e = multiplicity.fresh();
        let carries = [
            Carry {
                lin: Bound::Var(internal),
                mult: Bound::Var(e),
                origin: None,
            },
            Carry {
                lin: Bound::Const(Linearity::Lin),
                mult: Bound::Const(Multiplicity::Multi),
                origin: None,
            },
        ];
        assert_eq!(
            carry_residual(
                &carries,
                &Graph::new(&linearity, vec![vec![]]),
                &Graph::new(&multiplicity, vec![vec![e]]),
                0
            ),
            vec![Carry {
                lin: Bound::Const(Linearity::Lin),
                mult: Bound::Var(e),
                origin: None,
            }]
        );
    }

    #[test]
    fn unconstrained_variables_take_the_bottom() {
        let mut lattice = lattice::<Linearity>();
        let v = lattice.fresh();
        assert_eq!(solve(&lattice).0[v.index()], Linearity::Unr);
    }

    #[test]
    fn lower_bounds_propagate_through_chains() {
        let mut lattice = lattice::<Linearity>();
        let a = lattice.fresh();
        let b = lattice.fresh();
        lattice.require(Bound::Var(a), Bound::Var(b), None);
        lattice.require(Bound::Const(Linearity::Lin), Bound::Var(a), None);
        assert_eq!(
            solve(&lattice),
            (vec![Linearity::Lin, Linearity::Lin], vec![])
        );
    }

    #[test]
    fn an_upper_bound_below_the_solution_is_reported() {
        let mut lattice = lattice::<Multiplicity>();
        let a = lattice.fresh();
        lattice.require(Bound::Const(Multiplicity::Multi), Bound::Var(a), None);
        lattice.require(Bound::Var(a), Bound::Const(Multiplicity::Once), None);
        assert_eq!(solve(&lattice).1, vec![1]);
    }

    #[test]
    fn residual_constraints_pass_through_internal_variables() {
        let mut lattice = lattice::<Linearity>();
        let a = lattice.fresh();
        let b = lattice.fresh();
        let internal = lattice.fresh();
        lattice.require(Bound::Var(a), Bound::Var(internal), None);
        lattice.require(Bound::Var(internal), Bound::Const(Linearity::Unr), None);
        lattice.require(Bound::Var(a), Bound::Var(b), None);
        assert_eq!(
            residual_of(&lattice, &[a, b]),
            vec![
                (Bound::Var(a), Bound::Var(b)),
                (Bound::Var(a), Bound::Const(Linearity::Unr)),
            ]
        );
    }

    #[test]
    fn residual_lower_bounds_above_the_bottom_are_kept() {
        let mut lattice = lattice::<Linearity>();
        let c = lattice.fresh();
        let internal = lattice.fresh();
        lattice.require(Bound::Const(Linearity::Lin), Bound::Var(internal), None);
        lattice.require(Bound::Var(internal), Bound::Var(c), None);
        lattice.require(Bound::Const(Linearity::Unr), Bound::Var(c), None);
        assert_eq!(
            residual_of(&lattice, &[c]),
            vec![(Bound::Const(Linearity::Lin), Bound::Var(c))]
        );
    }

    #[test]
    fn copied_constraints_use_the_new_variables() {
        let mut problem = KindProblem::default();
        let a = problem.lin.fresh();
        let copy = problem.lin.fresh();
        problem.instances.push(Instance {
            decl: Decl::Builtin(Builtin::IntEq),
            lin: vec![copy],
            mult: vec![],
            origin: None,
        });
        let scheme = KindScheme {
            lin: vec![(
                Bound::Const(Linearity::Lin),
                Bound::Var(KindVar::from_index(0)),
            )],
            ..KindScheme::default()
        };
        let schemes = HashMap::from([(Decl::Builtin(Builtin::IntEq), scheme)]);
        let merged = merge(&[(function(0), &problem)], &schemes);
        let values = solve(&merged.lin).0;
        assert_eq!(values[copy.index()], Linearity::Lin);
        assert_eq!(values[a.index()], Linearity::Unr);
    }

    #[test]
    fn a_carry_breaks_only_when_the_value_is_linear_and_the_row_is_multi() {
        let carry = |lin, mult| Carry {
            lin,
            mult,
            origin: None,
        };
        let carries = [
            carry(Bound::Var(KindVar(0)), Bound::Var(KindVar(0))),
            carry(Bound::Const(Linearity::Lin), Bound::Var(KindVar(1))),
            carry(Bound::Var(KindVar(1)), Bound::Const(Multiplicity::Multi)),
        ];
        let lin = [Linearity::Lin, Linearity::Unr];
        let mult = [Multiplicity::Multi, Multiplicity::Once];
        assert_eq!(violated_carries(&carries, &lin, &mult), vec![0]);
    }

    #[test]
    fn a_worklist_reaches_the_end_of_a_chain_given_in_reverse() {
        let mut lattice = lattice::<Multiplicity>();
        let vars: Vec<KindVar> = (0..4).map(|_| lattice.fresh()).collect();
        for pair in vars.windows(2).rev() {
            lattice.require(Bound::Var(pair[0]), Bound::Var(pair[1]), None);
        }
        lattice.require(Bound::Const(Multiplicity::Multi), Bound::Var(vars[0]), None);
        assert_eq!(solve(&lattice).0, vec![Multiplicity::Multi; 4]);
    }

    #[test]
    fn kept_variables_in_one_cycle_get_the_same_bounds() {
        let mut lattice = lattice::<Linearity>();
        let x = lattice.fresh();
        let y = lattice.fresh();
        let internal = lattice.fresh();
        lattice.require(Bound::Var(x), Bound::Var(y), None);
        lattice.require(Bound::Var(y), Bound::Var(internal), None);
        lattice.require(Bound::Var(internal), Bound::Var(x), None);
        lattice.require(Bound::Var(internal), Bound::Const(Linearity::Unr), None);
        assert_eq!(
            residual_of(&lattice, &[x, y]),
            vec![
                (Bound::Var(x), Bound::Var(y)),
                (Bound::Var(x), Bound::Const(Linearity::Unr)),
                (Bound::Var(y), Bound::Var(x)),
                (Bound::Var(y), Bound::Const(Linearity::Unr)),
            ]
        );
    }

    #[test]
    fn a_shared_component_reaches_only_the_components_that_matter_to_each_member() {
        // 環状の相互再帰の形。3つの宣言の μ が1つの成分になり、そこから各宣言の m へ辺が出る。m₁ だけに上限がある
        let mut lattice = lattice::<Linearity>();
        let mus: Vec<KindVar> = (0..3).map(|_| lattice.fresh()).collect();
        let ms: Vec<KindVar> = (0..3).map(|_| lattice.fresh()).collect();
        for i in 0..3 {
            lattice.require(Bound::Var(mus[i]), Bound::Var(mus[(i + 1) % 3]), None);
            lattice.require(Bound::Var(mus[i]), Bound::Var(ms[i]), None);
        }
        lattice.require(Bound::Var(ms[1]), Bound::Const(Linearity::Unr), None);
        let graph = Graph::new(&lattice, (0..3).map(|i| vec![mus[i], ms[i]]).collect());
        let mu = Bound::Var(KindVar::from_index(0));
        let m = Bound::Var(KindVar::from_index(1));
        let unr = Bound::Const(Linearity::Unr);
        // m₁ の成分はほかの宣言には残す成分でないので、通り抜けて上限を集める
        assert_eq!(residual(&graph, 0), vec![(mu, m), (mu, unr)]);
        // 自分の m₁ の成分で止まるので、μ には上限が付かず、m に付く
        assert_eq!(residual(&graph, 1), vec![(mu, m), (m, unr)]);
        assert_eq!(residual(&graph, 2), vec![(mu, m), (mu, unr)]);
    }

    #[test]
    fn a_reference_within_the_scc_becomes_an_equation() {
        // f は自分の a を g に渡し、g は自分の b を2回使う。f の a にも g の b と同じ上限が付く
        let mut f = KindProblem::default();
        let a = f.lin.fresh();
        let passed = f.lin.fresh();
        f.lin.require(Bound::Var(a), Bound::Var(passed), None);
        f.own.lin = vec![a];
        f.instances.push(Instance {
            decl: function(1),
            lin: vec![passed],
            mult: vec![],
            origin: None,
        });
        let mut g = KindProblem::default();
        let b = g.lin.fresh();
        g.lin
            .require(Bound::Var(b), Bound::Const(Linearity::Unr), None);
        g.own.lin = vec![b];
        let solution = solve_scc(&[(function(0), &f), (function(1), &g)], &HashMap::new());
        let unr = vec![(
            Bound::Var(KindVar::from_index(0)),
            Bound::Const(Linearity::Unr),
        )];
        assert_eq!(solution.schemes[0].lin, unr);
        assert_eq!(solution.schemes[1].lin, unr);
        assert!(solution.violated.is_empty());
    }

    #[test]
    fn a_reference_to_an_earlier_scc_copies_its_scheme() {
        let inner = KindOrigin {
            range: range(1, 2),
            reason: KindReason::CarriedAcross {
                value: CarriedValue::Local {
                    name: "x".to_string(),
                    binding: range(0, 1),
                },
                multi: None,
                call: CallKind::Call,
            },
        };
        let keep = KindScheme {
            lin: vec![(
                Bound::Var(KindVar::from_index(0)),
                Bound::Const(Linearity::Unr),
            )],
            mult: vec![],
            carries: vec![Carry {
                lin: Bound::Var(KindVar::from_index(0)),
                mult: Bound::Const(Multiplicity::Multi),
                origin: Some(inner),
            }],
        };
        let mut f = KindProblem::default();
        let a = f.lin.fresh();
        let passed = f.lin.fresh();
        f.lin.require(Bound::Var(a), Bound::Var(passed), None);
        f.own.lin = vec![a];
        f.instances.push(Instance {
            decl: function(9),
            lin: vec![passed],
            mult: vec![],
            origin: Some(KindOrigin {
                range: range(5, 9),
                reason: KindReason::Passed("keep".to_string()),
            }),
        });
        let schemes = HashMap::from([(function(9), keep)]);
        let solution = solve_scc(&[(function(0), &f)], &schemes);
        assert_eq!(
            solution.schemes[0],
            KindScheme {
                lin: vec![(
                    Bound::Var(KindVar::from_index(0)),
                    Bound::Const(Linearity::Unr),
                )],
                mult: vec![],
                carries: vec![Carry {
                    lin: Bound::Var(KindVar::from_index(0)),
                    mult: Bound::Const(Multiplicity::Multi),
                    origin: Some(KindOrigin {
                        range: range(5, 9),
                        reason: KindReason::CarriedThrough {
                            name: "keep".to_string(),
                            inner: Some(CarriedInner {
                                range: range(1, 2),
                                label: InnerLabel::Kept("x".to_string()),
                            }),
                        },
                    }),
                }],
            }
        );
    }

    #[test]
    fn carries_of_one_pair_keep_the_earliest_origin() {
        let mut linearity = lattice::<Linearity>();
        let a = linearity.fresh();
        let mut multiplicity = lattice::<Multiplicity>();
        let e = multiplicity.fresh();
        let origin = |start, end| {
            Some(KindOrigin {
                range: range(start, end),
                reason: KindReason::Unified,
            })
        };
        let carries = [
            Carry {
                lin: Bound::Var(a),
                mult: Bound::Var(e),
                origin: origin(10, 11),
            },
            Carry {
                lin: Bound::Var(a),
                mult: Bound::Var(e),
                origin: None,
            },
            Carry {
                lin: Bound::Var(a),
                mult: Bound::Var(e),
                origin: origin(3, 4),
            },
        ];
        assert_eq!(
            carry_residual(
                &carries,
                &Graph::new(&linearity, vec![vec![a]]),
                &Graph::new(&multiplicity, vec![vec![e]]),
                0
            ),
            vec![Carry {
                lin: Bound::Var(a),
                mult: Bound::Var(e),
                origin: origin(3, 4),
            }]
        );
    }
}
