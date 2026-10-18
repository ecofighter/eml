//! 融合の段。handler への特殊化の後のプログラムの関数の関係 (直接の呼び出しと値としての参照) を凍らせ (`Graph`)、
//! 呼ばれる関数と値として参照される関数から先に、関数を1つずつ1回だけ書き換える。規則は、小さな再帰しない関数の
//! インライン化、定数で分ける `switch` の畳み込み、1本の `jump` だけが入るブロックのつなぎ、既知のコンストラクタ、
//! 既知の閉包への `apply` の書き換え、既知の閉包をフィールドに持つ `con` を受ける関数の複製、使われない純粋な `let` の
//! 削除である (docs/spec/core-ir.md の「融合」)。
//!
//! 展開する本体は、呼び出し先の書き換えた後のコードから写す。呼び出し先のコードはそのときにはもう決まっていて、名前で
//! 決まる。展開した本体の中の呼び出しは、呼び出し先を書き換えたときに展開するかどうかを決めてあるので、呼ぶ側では
//! 展開し直さない。書き換えで生まれた呼び出し (既知の閉包への `apply` と、複製へ向けた呼び出し) だけを、呼ぶ側で
//! 新しく展開する。どの関数のコードも、その関数と呼び出し先のコードだけで決まり、プログラムのほかの関数によらない
//! (docs/spec/vm.md の「追記」)。展開を重ねても、展開した本体の大きさは `INLINE_LIMIT` を超えないので、コードは
//! 呼び出しの連なりの深さの積で育たない。

mod copies;
mod inline;
mod known_con;

use std::collections::{HashMap, HashSet};

use crate::tail::pure;
use crate::{Call, CoreFn, Program, Rhs, Stmt};
use eml_lir::{Atom, FnIdx, Layout, VarId};

use super::blocks;
use super::known::Callee;
use super::specialize::Key;

/// 書き換えで生まれた呼び出し (既知の閉包への `apply` を書き換えた呼び出しと、複製に向けた呼び出し) を展開する回数の
/// 上限。展開した本体がまた既知の閉包への `apply` を持ち込むと、自分を引数に受けて呼ぶ閉包のような形で展開が終わら
/// ないためである。回数は関数のコードだけで決まるので、名前が同じ関数のコードは変わらない (docs/spec/vm.md の「追記」)。
const MAX_INLINE_ROUNDS: usize = 8;

/// `fuse` に入った時点の関数の関係。
struct Graph {
    /// 直接の呼び出しの関係で、2つ以上の関数の SCC に入るか、自分を呼ぶ関数。展開しない。
    recursive: Vec<bool>,
    /// 直接の呼び出しと値としての参照 (`closure` の関数と関数の値 `&f`) を辺にした SCC の番号。番号の小さい SCC
    /// ほど先に書き換える。
    scc: Vec<usize>,
    /// 書き換える順。どの関数も、値の参照も含めて届く SCC の外の関数より後に来る。同じ SCC の中は、直接の呼び出しの
    /// 呼ばれる関数が先に来る。
    order: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Pending,
    Active,
    Done,
}

/// 書き換えの状態。関数の表は、作った複製を後ろに足しながら持つ。書き換えている関数は表から外して空の関数を置く。
pub(super) struct Fuser<'a> {
    functions: &'a mut Vec<CoreFn>,
    layouts: &'a [Layout],
    callees: Vec<Callee>,
    graph: Graph,
    /// `fuse` に入った時点の関数の数。これより後ろは融合の段が作った複製である。
    base: usize,
    status: Vec<Status>,
    /// 元の関数は直接の呼び出しの関係で再帰するか。複製は、参照の関係で輪に入るか (`refer`)。
    recursive: Vec<bool>,
    /// 複製の輪の union-find。輪に入った複製は、同じ輪の複製と同じ代表を持つ。
    groups: Vec<usize>,
    /// 代表ごとの、輪の複製の数。
    sizes: Vec<usize>,
    /// 書き換えた後のコードを展開してよいか。書き換え終えたときに決める。
    inlinable: Vec<bool>,
    /// 引数ごとの、`con` の形について特殊化してよい位置 (`copies::con_params`)。書き換え終えたときに決める。
    con_params: Vec<copies::ConParams>,
    /// 鍵の入れ子の深さ。元の関数は 0 である。
    depths: Vec<usize>,
    copies: HashMap<Key, FnIdx>,
    /// 書き換えている関数。外側から順に並ぶ。
    stack: Vec<usize>,
    /// いま書き換えている元の関数の SCC。
    current: usize,
}

pub(crate) fn fuse(program: &mut Program) {
    let graph = graph(&program.functions);
    // 呼ぶ相手の引数の数、Repr、印、包む命令は、融合で変わらない
    let callees: Vec<Callee> = program.functions.iter().map(Callee::of).collect();
    let count = program.functions.len();
    let order = graph.order.clone();
    let recursive = graph.recursive.clone();
    let mut cx = Fuser {
        functions: &mut program.functions,
        layouts: &program.layouts,
        callees,
        graph,
        base: count,
        status: vec![Status::Pending; count],
        recursive,
        groups: (0..count).collect(),
        sizes: vec![1; count],
        inlinable: vec![false; count],
        con_params: vec![copies::ConParams::default(); count],
        depths: vec![0; count],
        copies: HashMap::new(),
        stack: Vec::new(),
        current: 0,
    };
    for index in order {
        cx.current = cx.graph.scc[index];
        cx.fuse_at(index);
    }
}

/// 書き換えている間だけ関数の表に置く空の関数。書き換えている関数は再帰しないので、自分を展開しようとはしない。
fn placeholder() -> CoreFn {
    CoreFn {
        name: String::new(),
        internal: false,
        yields: false,
        clause: None,
        vars: Vec::new(),
        ret: eml_lir::Repr::Unit,
        ret_mark: eml_lir::Mark::Unr,
        blocks: Vec::new(),
    }
}

impl Fuser<'_> {
    /// 関数1つを書き換える。元の関数は、まず直接の呼び出しを1回の走査で展開する。複製の本体は呼び出し先の書き換えた
    /// 後のコードなので、その中の呼び出しは呼び出し先で決めてあり、展開し直さない。そのあと片付け、既知の
    /// コンストラクタ、既知の閉包、複製の規則を、どれも当たらなくなるまで繰り返す。
    fn fuse_at(&mut self, index: usize) {
        let mut function = std::mem::replace(&mut self.functions[index], placeholder());
        self.status[index] = Status::Active;
        self.stack.push(index);
        if index < self.base {
            let chosen = |_: VarId, target: FnIdx| {
                let target = target.0 as usize;
                self.status[target] == Status::Done && self.inlinable[target]
            };
            inline::inline_calls(&mut function, self.functions, &chosen);
        } else {
            self.refer_all(&function);
        }
        self.rewrite(&mut function);
        self.stack.pop();
        self.status[index] = Status::Done;
        // 複製は、自分を直接呼ぶときだけ再帰する。値として自分を参照するだけの複製 (`Iter` の次の状態に自分の閉包を
        // 入れる `map` の写し) は、書き換え終えればコードが決まり、展開しても展開した本体は展開し直さない
        let recursive = if index < self.base {
            self.recursive[index]
        } else {
            references(&function, false).contains(&index)
        };
        self.inlinable[index] = !recursive && inline::inlinable(&function);
        self.con_params[index] = copies::con_params(&function, FnIdx(index as u32), &self.callees);
        self.functions[index] = function;
    }

    /// 規則を、どれも当たらなくなるまで繰り返す。ほとんどの規則は、`switch` や `unpack` を通る経路か `apply` か文を
    /// 減らすか、呼び出しを1回だけ複製へ向ける。引数の余る既知の閉包への `apply` の書き換えは `apply` を残すが、残る
    /// `apply` は呼び出しの結果に向き、それが既知の閉包になるのは呼び出しを展開したときだけである。展開は
    /// `MAX_INLINE_ROUNDS` 回までなので、繰り返しは止まる (docs/spec/core-ir.md の「融合が止まること」)。
    fn rewrite(&mut self, function: &mut CoreFn) {
        let mut rounds = 0;
        loop {
            let mut changed = blocks::fold_constant_switches(function);
            blocks::prune(function);
            // つなげたブロックの引数を定数で置き換えると、つなげなかったブロックの `switch` も定数で分けることがある
            if blocks::merge_jumps(function) {
                changed = true;
                blocks::fold_constant_switches(function);
                blocks::prune(function);
            }
            changed |= known_con::resolve_known(function);
            changed |= known_con::thread_jumps(function);
            blocks::prune(function);
            let (rewritten, mut produced) = known_con::call_known_closures(function, &self.callees);
            changed |= rewritten;
            let redirected = copies::redirect(function, self);
            changed |= !redirected.is_empty();
            produced.extend(redirected);
            if !produced.is_empty() && rounds < MAX_INLINE_ROUNDS {
                let produced: HashSet<VarId> = produced.into_iter().collect();
                let chosen = |var: VarId, target: FnIdx| {
                    produced.contains(&var) && self.inlinable_from_here(target.0 as usize)
                };
                if inline::inline_calls(function, self.functions, &chosen) {
                    changed = true;
                    rounds += 1;
                    // 展開した本体が参照する複製も、この関数から参照する
                    self.refer_all(function);
                }
            }
            changed |= drop_dead_pure_lets(function);
            if !changed {
                break;
            }
        }
    }

    /// いま書き換えている関数から複製 `target` への参照を記録する。参照で輪ができたら、輪の複製を1つの組にまとめる。
    /// 輪ができるのは、`target` を書き換えているか、`target` の組に書き換えている複製があるときである。書き換えている
    /// 複製は、書き換えの途中で作った複製から参照の連なりで届くので、そこからいまの関数までの書き換えている複製が
    /// どれも輪に入る。値として自分を参照するだけの複製も、1つの複製の組になる。
    ///
    /// 参照は書き換えている関数だけが作り、書き換え終えた関数のコードは変わらないので、輪は、輪を閉じる参照を作った
    /// ときに必ず見つかる。書き換えている複製を含む組は、輪のどの複製を先に作ったかで、まだ書き換えていない所が
    /// 変わる。そこで、その組の複製は読まない (`settled`)。書き換えている複製を含まなくなった組は、もう変わらない。
    /// そのため、どの複製のコードも名前で決まる (docs/spec/vm.md の「追記」)。
    fn refer(&mut self, target: usize) {
        if target < self.base {
            return;
        }
        let root = self.root(target);
        let from = if self.status[target] == Status::Active {
            self.stack.iter().position(|&active| active == target)
        } else if self.recursive[target] {
            self.active_member(root)
        } else {
            None
        };
        let Some(from) = from else {
            return;
        };
        for at in from..self.stack.len() {
            let member = self.stack[at];
            if member >= self.base {
                self.recursive[member] = true;
                let (member, root) = (self.root(member), self.root(target));
                if member != root {
                    self.groups[member] = root;
                    self.sizes[root] += self.sizes[member];
                }
            }
        }
    }

    fn root(&self, mut function: usize) -> usize {
        while self.groups[function] != function {
            function = self.groups[function];
        }
        function
    }

    /// 代表が `root` の組の、書き換えている複製のうち一番外側のもののスタックの位置。
    fn active_member(&self, root: usize) -> Option<usize> {
        (0..self.stack.len()).find(|&at| {
            let active = self.stack[at];
            active >= self.base && self.root(active) == root
        })
    }

    /// `function` のコードが参照する複製を、どれも参照する (`refer`)。
    fn refer_all(&mut self, function: &CoreFn) {
        for target in references(function, true) {
            self.refer(target);
        }
    }

    /// 書き換えで生まれた呼び出しや鍵の閉包の関数として、コードを読んでよい関数か (`settled`) で、展開してよいもの。
    /// 2つ以上の複製の輪に入る複製は、展開しない。決定性のためには要らない。書き換えている複製を含まなくなった組は、
    /// もう育たないので、そのコードは名前で決まる。展開すると、展開した本体が輪の次の複製の閉包を持ち込み、引く側の
    /// 複製が輪の複製の数だけできる。標準ライブラリの `Iter` の段はどれも自分だけを参照するので、安全側にそこまでに
    /// とどめた (docs/spec/core-ir.md の「融合が止まること」)。
    fn inlinable_from_here(&self, target: usize) -> bool {
        self.settled(target)
            && self.inlinable[target]
            && (target < self.base || self.sizes[self.root(target)] == 1)
    }

    /// 書き換え終えていて、そのコードが名前で決まる関数か。元の関数は、いま書き換えている関数と違う SCC に入るときだけ
    /// である。同じ SCC の中の書き換えの順はプログラムによって変わるので、同じ SCC の関数を読むと、コードが順によって
    /// 変わる。ただし直接の呼び出しの呼ばれる関数は先に書き換えるので、最初の展開 (`fuse_at`) はそれを読んでよい。
    /// 複製は、組に書き換えている複製がないときだけである (`refer`)。
    fn settled(&self, target: usize) -> bool {
        self.status[target] == Status::Done
            && if target < self.base {
                self.graph.scc[target] != self.current
            } else {
                !self.recursive[target] || self.active_member(self.root(target)).is_none()
            }
    }
}

/// 使われない純粋な `let` を消す。展開した本体が返す値を呼ぶ側が捨てる所で、値を作るだけの
/// 文が残るためである。純粋の規則は EP と同じ `crate::tail::pure` で、消しても評価の順と実行時エラーは変わらない。
/// 定義は使う位置を支配し、辺は番号の大きいブロックへ向かうので、ブロックを後ろから、文を後ろから見れば、`let` を
/// 見る時点でその変数の使いはすべて見終わっている。消すたびに使いの数を減らせば、1回で、消した `let` だけが使って
/// いた束縛も消える (LIR の縮約の `remove_dead_lets` と同じ形)。
fn drop_dead_pure_lets(function: &mut CoreFn) -> bool {
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
    let mut dropped = false;
    for block in function.blocks.iter_mut().rev() {
        let mut keep = vec![true; block.stmts.len()];
        for (position, stmt) in block.stmts.iter().enumerate().rev() {
            if let Stmt::Let { var, rhs } = stmt
                && uses[var.0 as usize] == 0
                && (pure(rhs) || drops_constant(rhs))
            {
                keep[position] = false;
                rhs.for_each_atom(|atom| {
                    if let Atom::Var(used) = atom {
                        uses[used.0 as usize] -= 1;
                    }
                });
            }
        }
        if keep.contains(&false) {
            dropped = true;
            let mut keep = keep.into_iter();
            block.stmts.retain(|_| keep.next() == Some(true));
        }
    }
    dropped
}

/// 定数の `drop`。定数はヒープの参照を持たないので、捨てても何も起きない。translate が葉で捨てる `Unr` でない値は、
/// 既知のコンストラクタを解くと、フィールドのない定数 (`None` の `#0`) に置き換わる。
fn drops_constant(rhs: &Rhs) -> bool {
    matches!(rhs, Rhs::Drop(atom) if !matches!(atom, Atom::Var(_)))
}

/// 直接の呼び出しの関係で再帰する関数と、値の参照も辺にした SCC と、書き換える順。
fn graph(functions: &[CoreFn]) -> Graph {
    let direct: Vec<Vec<usize>> = functions
        .iter()
        .map(|function| references(function, false))
        .collect();
    let values: Vec<Vec<usize>> = functions
        .iter()
        .map(|function| references(function, true))
        .collect();
    let count = functions.len();
    let mut recursive = vec![false; count];
    let mut position = vec![0; count];
    let mut next = 0;
    for members in components(&direct) {
        if members.len() > 1 || direct[members[0]].contains(&members[0]) {
            for &member in &members {
                recursive[member] = true;
            }
        }
        for member in members {
            position[member] = next;
            next += 1;
        }
    }
    let mut scc = vec![0; count];
    for (index, members) in components(&values).into_iter().enumerate() {
        for member in members {
            scc[member] = index;
        }
    }
    let mut order: Vec<usize> = (0..count).collect();
    order.sort_unstable_by_key(|&function| (scc[function], position[function]));
    Graph {
        recursive,
        scc,
        order,
    }
}

/// 関数のコードが参照する関数。`values` なら `closure` の関数と関数の値 `&f` も含める。
fn references(function: &CoreFn, values: bool) -> Vec<usize> {
    let mut targets = Vec::new();
    for block in &function.blocks {
        for stmt in &block.stmts {
            match stmt {
                Stmt::Let {
                    var: _,
                    rhs:
                        Rhs::Call {
                            call: Call::Direct(target, _),
                            mask: _,
                        },
                } => targets.push(target.0 as usize),
                Stmt::Let {
                    var: _,
                    rhs: Rhs::MakeClosure(target, _),
                } if values => targets.push(target.0 as usize),
                _ => {}
            }
            if values {
                stmt.for_each_atom(|atom| {
                    if let Atom::Fn(target) = atom {
                        targets.push(target.0 as usize);
                    }
                });
            }
        }
        if values {
            block.term.for_each_atom(|atom| {
                if let Atom::Fn(target) = atom {
                    targets.push(target.0 as usize);
                }
            });
        }
    }
    targets.sort_unstable();
    targets.dedup();
    targets
}

/// SCC を Tarjan の方法で求める。Tarjan の方法は SCC を、そこから届く SCC より後に見つけるので、見つけた順に並べれば
/// 呼ばれる関数が先に来る。プログラムの大きさに比例する手間で済み、深い呼び出しの連なりでもスタックを溢れさせない
/// ように、再帰を使わずに書く。
fn components(successors: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let count = successors.len();
    const UNSEEN: usize = usize::MAX;
    let mut visit = vec![UNSEEN; count];
    let mut low = vec![0; count];
    let mut on_stack = vec![false; count];
    let mut stack = Vec::new();
    let mut next = 0;
    let mut found = Vec::new();
    for root in 0..count {
        if visit[root] != UNSEEN {
            continue;
        }
        let mut frames = vec![(root, 0usize)];
        visit[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(frame) = frames.last_mut() {
            let (function, position) = *frame;
            if let Some(&callee) = successors[function].get(position) {
                frame.1 += 1;
                if visit[callee] == UNSEEN {
                    visit[callee] = next;
                    low[callee] = next;
                    next += 1;
                    stack.push(callee);
                    on_stack[callee] = true;
                    frames.push((callee, 0));
                } else if on_stack[callee] {
                    low[function] = low[function].min(visit[callee]);
                }
                continue;
            }
            frames.pop();
            if let Some(&(caller, _)) = frames.last() {
                low[caller] = low[caller].min(low[function]);
            }
            if low[function] == visit[function] {
                let mut members = Vec::new();
                loop {
                    let member = stack.pop().expect("an SCC member is on the stack");
                    on_stack[member] = false;
                    members.push(member);
                    if member == function {
                        break;
                    }
                }
                found.push(members);
            }
        }
    }
    found
}
