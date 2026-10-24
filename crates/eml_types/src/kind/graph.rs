//! 番号で表した有向グラフの強連結成分。Kind の解、呼び出しグラフ、一様な位置のグラフが共有する。HIR にも推論の表にも
//! 依存しない。

/// Tarjan の方法で、`edges` (節点ごとの行き先の並び) の強連結成分を見つけた順 (行き先の成分が先) に返す。節点が
/// 多くても Rust のスタックを使わないよう、明示的なスタックでたどる。成分の節点はスタックから外した順に並ぶ。この順は
/// SCC の検査の順と Kind 変数の番号を決めるので、呼ぶ側は並べ替えずに使う。
pub(crate) fn components(edges: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let unvisited = usize::MAX;
    let n = edges.len();
    let mut index = vec![unvisited; n];
    let mut low = vec![0; n];
    let mut on_stack = vec![false; n];
    let mut stack = Vec::new();
    let mut out = Vec::new();
    let mut next = 0;
    for root in 0..n {
        if index[root] != unvisited {
            continue;
        }
        // (節点, 次に見る辺の位置)
        let mut work = vec![(root, 0)];
        index[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(&(v, edge)) = work.last() {
            if edge < edges[v].len() {
                let w = edges[v][edge];
                work.last_mut().expect("read above").1 += 1;
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
                let mut members = Vec::new();
                loop {
                    let w = stack
                        .pop()
                        .expect("the root of a component is on the stack");
                    on_stack[w] = false;
                    members.push(w);
                    if w == v {
                        break;
                    }
                }
                out.push(members);
            }
        }
    }
    out
}

/// 節点ごとの強連結成分の番号 (`components` の並びの位置) と、成分の数。
pub(crate) fn component_ids(edges: &[Vec<usize>]) -> (Vec<usize>, usize) {
    let components = components(edges);
    let mut id = vec![0; edges.len()];
    for (c, members) in components.iter().enumerate() {
        for &v in members {
            id[v] = c;
        }
    }
    (id, components.len())
}
