//! 物体のグラフ (docs/spec/object-model.md の「`share`」と「`transfer_back`」)。たどりは物体への読み書きを `Graph` の裏に置いて書き、
//! Verus がたどりを trait のメソッドの約束の上で証明する。ヒープの上の `Graph` は `Objects` で、たどりの間ヒープの
//! ブロックの権限を預かる。

use std::marker::PhantomData;
use std::mem;
use vstd::hash_set::HashSetWithView;
use vstd::prelude::*;
use vstd::raw_ptr::ptr_ref;

use super::check::ALIVE_STATE;
#[cfg(verus_only)]
use super::inv::{
    p_code, p_count, p_extended, p_first, p_init, p_meta, p_owner, p_scanned, p_word, state_code,
};
#[cfg(verus_only)]
use super::model::BlockState;
use super::{Core, Env, count_owned, move_owned};
use crate::cell::{rc_load, rc_store, u32_load, u32_store};
use crate::meta::{
    BlockPerm, Header, Meta, meta_ptr, prefix_first_ptr, prefix_state_ptr, rc_ptr, slot_ptr,
};
#[cfg(verus_only)]
use crate::meta::{bits_lin, bits_wf};
use crate::mode::Mode;
#[cfg(verus_only)]
use crate::word::Word;

verus! {

/// グラフ全体を集合と写像で表した見方。trait のメソッドの約束を、この値の言葉で書くために置く
// 仕様だけで使う型。Verus の外のビルドでも型は残るので、使わないことの警告を止める
#[cfg_attr(not(verus_keep_ghost), allow(dead_code))]
#[verifier::reject_recursive_types(N)]
pub ghost struct GView<N> {
    pub nodes: Set<N>,
    pub live: Map<N, bool>,
    pub rc: Map<N, i32>,
    pub lin: Map<N, bool>,
    pub owner: Map<N, u32>,
    pub children: Map<N, Seq<N>>,
}

impl<N> GView<N> {
    pub open spec fn with_rc(self, n: N, v: i32) -> Self {
        GView {
            nodes: self.nodes,
            live: self.live,
            rc: self.rc.insert(n, v),
            lin: self.lin,
            owner: self.owner,
            children: self.children,
        }
    }

    pub open spec fn with_owner(self, n: N, v: u32) -> Self {
        GView {
            nodes: self.nodes,
            live: self.live,
            rc: self.rc,
            lin: self.lin,
            owner: self.owner.insert(n, v),
            children: self.children,
        }
    }
}

/// 各メソッドの約束だけを頼りにたどりを証明するので、グラフの大きさにも実装にも依らない結論になる
pub(crate) trait Graph: Sized {
    type Node: Copy + PartialEq;

    spec fn gv(&self) -> GView<Self::Node>;

    /// たどりが変えない部分 (`gv` に現れない部分を含む) が `old` と同じである。
    spec fn kept(&self, old: Self) -> bool;

    proof fn lemma_kept_refl(&self)
        ensures
            self.kept(*self),
    ;

    proof fn lemma_kept_trans(&self, mid: Self, old: Self)
        requires
            mid.kept(old),
            self.kept(mid),
        ensures
            self.kept(old),
    ;

    /// 検査モードの前置きが生存を示すか。速いモードと、前置きのないグラフのモデルでは常に真である。
    fn live(&self, node: Self::Node) -> (r: bool)
        requires
            self.gv().nodes.contains(node),
        ensures
            r == self.gv().live[node],
    ;

    fn rc(&self, node: Self::Node) -> (r: i32)
        requires
            self.gv().nodes.contains(node),
        ensures
            r == self.gv().rc[node],
    ;

    /// 局所でない物体を局所にはしない。それは所有者も書く `flip` の仕事である
    fn set_rc(&mut self, node: Self::Node, rc: i32)
        requires
            old(self).gv().nodes.contains(node),
            old(self).gv().rc[node] != 0,
            rc > 0 ==> old(self).gv().rc[node] > 0,
        ensures
            final(self).gv() == old(self).gv().with_rc(node, rc),
            final(self).kept(*old(self)),
            final(self).seen() == old(self).seen(),
    ;

    fn lin(&self, node: Self::Node) -> (r: bool)
        requires
            self.gv().nodes.contains(node),
        ensures
            r == self.gv().lin[node],
    ;

    fn set_owner(&mut self, node: Self::Node, owner: u32)
        requires
            old(self).gv().nodes.contains(node),
            old(self).gv().rc[node] != 0,
        ensures
            final(self).gv() == old(self).gv().with_owner(node, owner),
            final(self).kept(*old(self)),
            final(self).seen() == old(self).seen(),
    ;

    /// 共有の物体を rc 1 の局所の物体にし、所有者を `owner` にする。rc と所有者を1つの操作で書くので、検査モードの
    /// 前置きの所有者は、局所の物体のどれでも持ち主に等しいままである
    fn flip(&mut self, node: Self::Node, owner: u32)
        requires
            old(self).gv().nodes.contains(node),
            old(self).gv().rc[node] < 0,
        ensures
            final(self).gv() == old(self).gv().with_rc(node, 1).with_owner(node, owner),
            final(self).kept(*old(self)),
            final(self).seen() == old(self).seen(),
    ;

    /// 検査モードか。所有者を読めるのは検査モードだけである。
    spec fn checked(&self) -> bool;

    /// 読める所有者が `gv` の所有者に等しい。
    spec fn owners_known(&self) -> bool;

    /// `kept` は、検査モードかと、読める所有者が等しいことを保つ。
    proof fn lemma_kept_known(&self, old: Self)
        requires
            self.kept(old),
        ensures
            self.checked() == old.checked(),
            old.owners_known() ==> self.owners_known(),
    ;

    /// 検査モードで、生きている局所の節点の所有者を読む。
    fn owner(&self, node: Self::Node) -> (r: u32)
        requires
            self.checked(),
            self.gv().nodes.contains(node),
            self.gv().live[node],
            self.gv().rc[node] > 0,
        ensures
            self.owners_known() ==> r == self.gv().owner[node],
    ;

    /// 子のうち物体を指すものを `out` に積む。
    fn push_children(&self, node: Self::Node, out: &mut Vec<Self::Node>)
        requires
            self.gv().nodes.contains(node),
            self.gv().live[node],
        ensures
            final(out)@ == old(out)@ + self.gv().children[node],
    ;

    /// 訪問済みの節点の集まり。
    spec fn seen(&self) -> Set<Self::Node>;

    /// 節点を訪問済みにし、初めてなら真を返す。節点の型の `==` が値の等しさであることを信頼せずに済むように、
    /// 比べ方を実装に任せ、結果を仕様の集合で約束させる
    fn see(&mut self, node: Self::Node) -> (fresh: bool)
        requires
            old(self).gv().nodes.contains(node),
        ensures
            final(self).gv() == old(self).gv(),
            final(self).kept(*old(self)),
            final(self).seen() == old(self).seen().insert(node),
            fresh == !old(self).seen().contains(node),
    ;
}

/// グラフの形。根と子はどれも節点で、各写像の定義域は節点の集合である。たどりが節点の外に触れないための前提である
pub open spec fn shaped<N>(g: GView<N>, root: N) -> bool {
    &&& g.nodes.contains(root)
    &&& g.live.dom() == g.nodes
    &&& g.rc.dom() == g.nodes
    &&& g.lin.dom() == g.nodes
    &&& g.owner.dom() == g.nodes
    &&& g.children.dom() == g.nodes
    &&& forall|p: N, k: int|
        g.nodes.contains(p) && 0 <= k < g.children[p].len() ==> g.nodes.contains(
            #[trigger] g.children[p][k],
        )
}

pub open spec fn edge<N>(g: GView<N>, p: N, n: N) -> bool {
    exists|k: int| 0 <= k < g.children[p].len() && #[trigger] g.children[p][k] == n
}

/// 根から、途中の物体がどれも `ex` を満たす経路。たどりは `ex` を満たす物体だけを展開するので、届く物体は
/// この経路で定める
pub open spec fn is_path<N>(g: GView<N>, ex: spec_fn(N) -> bool, root: N, path: Seq<N>) -> bool {
    &&& path.len() > 0
    &&& path[0] == root
    &&& forall|i: int|
        0 <= i < path.len() - 1 ==> g.nodes.contains(#[trigger] path[i]) && ex(path[i]) && edge(
            g,
            path[i],
            path[i + 1],
        )
}

pub open spec fn reached<N>(g: GView<N>, ex: spec_fn(N) -> bool, root: N, n: N) -> bool {
    exists|path: Seq<N>| #[trigger] is_path(g, ex, root, path) && path.last() == n
}

pub open spec fn same_shape<N>(g0: GView<N>, g: GView<N>) -> bool {
    &&& g.nodes == g0.nodes
    &&& g.live == g0.live
    &&& g.lin == g0.lin
    &&& g.children == g0.children
    &&& g.rc.dom() == g0.rc.dom()
    &&& g.owner.dom() == g0.owner.dom()
}

pub(crate) proof fn lemma_reached_root<N>(g: GView<N>, ex: spec_fn(N) -> bool, root: N)
    ensures
        reached(g, ex, root, root),
{
    let path = seq![root];
    assert(is_path(g, ex, root, path));
}

/// 展開のたびに、積み場の物体がどれも届く物体であることを保つ
pub(crate) proof fn lemma_reach_child<N>(g: GView<N>, ex: spec_fn(N) -> bool, root: N, p: N, k: int)
    requires
        reached(g, ex, root, p),
        g.nodes.contains(p),
        ex(p),
        0 <= k < g.children[p].len(),
    ensures
        reached(g, ex, root, g.children[p][k]),
{
    let path = choose|path: Seq<N>| #[trigger] is_path(g, ex, root, path) && path.last() == p;
    let c = g.children[p][k];
    let longer = path.push(c);
    assert forall|i: int| 0 <= i < longer.len() - 1 implies g.nodes.contains(#[trigger] longer[i])
        && ex(longer[i]) && edge(g, longer[i], longer[i + 1]) by {
        if i < path.len() - 1 {
            assert(longer[i] == path[i] && longer[i + 1] == path[i + 1]);
        } else {
            assert(longer[i] == p && longer[i + 1] == c);
        }
    }
    assert(is_path(g, ex, root, longer));
}

/// 展開する物体の条件を弱めても、届く物体は届く。
pub(crate) proof fn lemma_reached_weaken<N>(
    g: GView<N>,
    ex: spec_fn(N) -> bool,
    wider: spec_fn(N) -> bool,
    root: N,
    n: N,
)
    requires
        reached(g, ex, root, n),
        forall|m: N| #[trigger] ex(m) ==> wider(m),
    ensures
        reached(g, wider, root, n),
{
    let path = choose|path: Seq<N>| #[trigger] is_path(g, ex, root, path) && path.last() == n;
    assert(is_path(g, wider, root, path));
}

/// 語 `w` が指すヘッダのポインタ。`Word::as_ptr` が返す値である。
pub(crate) open spec fn ptr_of(w: Word) -> *mut Header {
    vstd::raw_ptr::ptr_mut_from_data::<Header>(
        vstd::raw_ptr::PtrData {
            addr: w.bits_spec(),
            provenance: w.ptr_spec()@.provenance,
            metadata: (),
        },
    )
}

/// 権限 `p` のブロックの、走査する語の i 番目より前の語のうち、物体を指すものの列。
pub(crate) open spec fn kids(p: BlockPerm, first: int, i: int) -> Seq<*mut Header>
    decreases i,
{
    if i <= 0 {
        Seq::empty()
    } else {
        let w = p_word(p, first + i - 1);
        kids(p, first, i - 1) + if w.is_ptr_spec() {
            seq![ptr_of(w)]
        } else {
            Seq::empty()
        }
    }
}

/// 検査モードの前置きが生存を示す。
pub(crate) open spec fn prefix_alive(p: BlockPerm) -> bool {
    p.prefix matches Some(q) && q.state.value() & 3 == ALIVE_STATE
}

/// 節点 `a` の権限。ヒープのブロックなら預かった権限 `perms`、付けた塊のブロックなら塊の権限である。
pub(crate) open spec fn node_perm(base: Core, perms: Map<usize, BlockPerm>, a: usize) -> BlockPerm {
    if base.in_region(a) {
        base.perm(a)
    } else {
        perms[a]
    }
}

/// たどりの節点にするブロック。生きている物体で、権限は物体の形をしている。たどりはこの形を変えない。付けた塊の
/// 物体も節点で、rc 0 なのでたどりはそこで止まる。
pub(crate) open spec fn node_ok(
    base: Core,
    perms: Map<usize, BlockPerm>,
    checked: bool,
    a: usize,
) -> bool {
    let p = node_perm(base, perms, a);
    let st = base.state(a);
    &&& base.has(a)
    &&& if checked {
        st != BlockState::Building
    } else {
        st == BlockState::Live
    }
    &&& base.in_region(a) ==> base.region_wf()
    &&& !base.in_region(a) ==> perms.dom().contains(a)
    &&& base.block(a)@.addr == a
    &&& a % 8 == 0
    &&& a >= 8
    &&& a + 8 * (1 + base.cap(a)) <= usize::MAX
    &&& p.wf(base.block(a), base.cap(a), checked)
    &&& p.rc.is_init()
    &&& p.meta.is_init()
    &&& bits_wf(p_meta(p))
    &&& p.prefix matches Some(q) ==> q.first.is_init() && q.state.is_init()
    &&& p.prefix is Some ==> p_code(p) == state_code(st)
    &&& st != BlockState::Freed ==> {
        &&& p_extended(p) ==> p_init(p, 0)
        &&& p_first(p) + p_count(p) <= base.cap(a)
    }
    &&& st != BlockState::Freed ==> forall|i: int| #[trigger] p_scanned(p, i) ==> p_init(p, i)
}

/// 節点のヘッダの集合。
pub(crate) open spec fn nodes_of(base: Core, perms: Map<usize, BlockPerm>, checked: bool) -> Set<
    *mut Header,
> {
    base.dom().union(base.region_blocks()).filter(|a: usize| node_ok(base, perms, checked, a)).map(
        |a: usize| base.block(a),
    )
}

/// ヒープの物体のグラフの見方。`base` はたどりの前のヒープで、ブロックの位置を決める。
// 中身を開くと `share` の証明の量化子が増えすぎるので、使う所でだけ開く
#[verifier::opaque]
pub(crate) open spec fn graph_of(
    base: Core,
    perms: Map<usize, BlockPerm>,
    owner: Map<usize, u32>,
    checked: bool,
) -> GView<*mut Header> {
    let ns = nodes_of(base, perms, checked);
    GView {
        nodes: ns,
        live: Map::new(
            ns,
            |n: *mut Header| !checked || prefix_alive(node_perm(base, perms, n@.addr)),
        ),
        rc: Map::new(ns, |n: *mut Header| node_perm(base, perms, n@.addr).rc.value()),
        lin: Map::new(ns, |n: *mut Header| bits_lin(p_meta(node_perm(base, perms, n@.addr)))),
        owner: Map::new(ns, |n: *mut Header| owner[n@.addr]),
        children: Map::new(
            ns,
            |n: *mut Header|
                if base.state(n@.addr) == BlockState::Freed {
                    Seq::empty()
                } else {
                    let p = node_perm(base, perms, n@.addr);
                    kids(p, p_first(p), p_count(p))
                },
        ),
    }
}

/// 節点は、形の正しいブロックのヘッダである。
pub(crate) proof fn lemma_node(
    base: Core,
    perms: Map<usize, BlockPerm>,
    checked: bool,
    n: *mut Header,
)
    requires
        nodes_of(base, perms, checked).contains(n),
    ensures
        node_ok(base, perms, checked, n@.addr),
        n == base.block(n@.addr),
{
    let f = |a: usize| base.block(a);
    let s = base.dom().union(base.region_blocks()).filter(
        |a: usize| node_ok(base, perms, checked, a),
    );
    s.lemma_map_contains(f, n);
    let a = choose|a: usize| s.contains(a) && n == f(a);
    assert(node_ok(base, perms, checked, a));
}

/// 形の正しいブロックのヘッダは、節点である。
pub(crate) proof fn lemma_is_node(base: Core, perms: Map<usize, BlockPerm>, checked: bool, a: usize)
    requires
        node_ok(base, perms, checked, a),
    ensures
        nodes_of(base, perms, checked).contains(base.block(a)),
{
    let f = |b: usize| base.block(b);
    let s = base.dom().union(base.region_blocks()).filter(
        |b: usize| node_ok(base, perms, checked, b),
    );
    base.lemma_region_blocks();
    s.lemma_map_contains(f, base.block(a));
    assert(s.contains(a));
}

/// `as_ptr` の結果は `ptr_of` である。
pub(crate) proof fn lemma_ptr_of(w: Word, h: *mut Header)
    requires
        h@.addr == w.bits_spec(),
        h@.provenance == w.ptr_spec()@.provenance,
    ensures
        h == ptr_of(w),
{
    vstd::raw_ptr::ptrs_mut_eq_sized(h);
    vstd::raw_ptr::ptrs_mut_eq_sized(ptr_of(w));
}

/// 検査モードの前置きの所有者は、たどりの前か今 rc が正の節点で、持ち主に等しい。
pub(crate) open spec fn owners_ok(
    base: Core,
    perms: Map<usize, BlockPerm>,
    owner: Map<usize, u32>,
    checked: bool,
) -> bool {
    forall|a: usize| #[trigger]
        node_ok(base, perms, checked, a) && base.state(a) == BlockState::Live && (base.rc_of(a) > 0
            || (!base.in_region(a) && perms[a].rc.value() > 0)) && perms[a].prefix is Some
            ==> p_owner(perms[a]) == owner[a]
}

/// 2つの権限の写像で、たどりが変えない部分 (rc の値と、節点の前置きの所有者の値を除くすべて) が同じである。
pub(crate) open spec fn same_frame(
    base: Core,
    p1: Map<usize, BlockPerm>,
    p0: Map<usize, BlockPerm>,
    checked: bool,
) -> bool {
    &&& p1.dom() == p0.dom()
    &&& forall|a: usize|
        #![trigger p1[a]]
        p0.dom().contains(a) ==> {
            let x = p1[a];
            let y = p0[a];
            &&& x.meta == y.meta
            &&& x.slots == y.slots
            &&& x.text == y.text
            &&& x.rc.ptr() == y.rc.ptr()
            &&& x.rc.is_init() == y.rc.is_init()
            &&& (x.prefix is Some) == (y.prefix is Some)
            &&& x.prefix matches Some(q1) ==> {
                let q0 = y.prefix->0;
                &&& q1.state == q0.state
                &&& q1.first.ptr() == q0.first.ptr()
                &&& q1.first.is_init() == q0.first.is_init()
            }
            &&& !node_ok(base, p0, checked, a) ==> x == y
        }
}

/// 子の列は、権限のペイロードの語だけで決まる。
pub(crate) proof fn lemma_kids_same(x: BlockPerm, y: BlockPerm, first: int, i: int)
    requires
        x.slots == y.slots,
    ensures
        kids(x, first, i) == kids(y, first, i),
    decreases i,
{
    if i > 0 {
        lemma_kids_same(x, y, first, i - 1);
    }
}

/// 形が同じなら、節点の集合と、節点の生存、`lin`、子は変わらない。
pub(crate) proof fn lemma_same_frame_graph(
    base: Core,
    p1: Map<usize, BlockPerm>,
    p0: Map<usize, BlockPerm>,
    o1: Map<usize, u32>,
    o0: Map<usize, u32>,
    checked: bool,
)
    requires
        same_frame(base, p1, p0, checked),
    ensures
        nodes_of(base, p1, checked) == nodes_of(base, p0, checked),
        forall|a: usize| #[trigger] node_ok(base, p1, checked, a) == node_ok(base, p0, checked, a),
        graph_of(base, p1, o1, checked).live == graph_of(base, p0, o0, checked).live,
        graph_of(base, p1, o1, checked).lin == graph_of(base, p0, o0, checked).lin,
        graph_of(base, p1, o1, checked).children == graph_of(base, p0, o0, checked).children,
{
    reveal(graph_of);
    assert forall|a: usize| #[trigger]
        node_ok(base, p1, checked, a) == node_ok(base, p0, checked, a) by {
        if p0.dom().contains(a) {
            let x = p1[a];
            let y = p0[a];
            assert forall|i: int| #[trigger] p_scanned(x, i) == p_scanned(y, i) by {}
        }
    }
    assert(base.dom().union(base.region_blocks()).filter(|a: usize| node_ok(base, p1, checked, a))
        =~= base.dom().union(base.region_blocks()).filter(
        |a: usize| node_ok(base, p0, checked, a),
    ));
    let ns = nodes_of(base, p0, checked);
    let g1 = graph_of(base, p1, o1, checked);
    let g0 = graph_of(base, p0, o0, checked);
    assert forall|n: *mut Header| #[trigger] ns.contains(n) implies g1.live[n] == g0.live[n]
        && g1.lin[n] == g0.lin[n] && g1.children[n] == g0.children[n] by {
        lemma_node(base, p0, checked, n);
        let a = n@.addr;
        let x = node_perm(base, p1, a);
        let y = node_perm(base, p0, a);
        lemma_kids_same(x, y, p_first(y), p_count(y));
        assert(p_first(x) == p_first(y));
        assert(p_count(x) == p_count(y));
    }
    assert(g1.live =~= g0.live);
    assert(g1.lin =~= g0.lin);
    assert(g1.children =~= g0.children);
}

/// ヒープの物体のグラフ。`share` と `transfer_back` のたどりの間、ヒープのブロックの権限と、検査モードの ID ごとの数と訪問済みの
/// 集まりを預かる。節点は生きている物体のヘッダである。
// 数と訪問済みの欄は速いモードでも大きさを持つが、空のまま触れない。ほかの欄はどれも ghost と権限で、実行の時には
// 大きさを持たない
#[cfg_attr(not(verus_keep_ghost), allow(dead_code))]
pub(crate) struct Objects<M> {
    pub(crate) perms: Tracked<Map<usize, BlockPerm>>,
    /// 検査モードの ID ごとの数 (`Heap` の `owned`)。rc の符号か、rc が正の物体の持ち主が変わると数え直す。
    /// 速いモードでは空のまま触れない。
    pub(crate) owned: Vec<u64>,
    /// たどりが rc を負にした物体の数 (`Stats` の `shared`)。検査モードだけで数え、速いモードでは 0 のまま触れない。
    /// 証明する不変条件の外にある
    pub(crate) shared: u64,
    /// 検査モードで訪れた節点の番地 (`Heap` の `seen`)。速いモードでは `None` のまま触れない。
    pub(crate) seen: Option<HashSetWithView<usize>>,
    /// たどりの間ヒープから預かる、付けた塊の権限の写し。塊の節点はここから読む。
    pub(crate) env: Tracked<Env>,
    /// 物体の持ち主。速いモードでも ghost の値として持ち、たどりの後でモデルに戻す。
    pub(crate) owner: Ghost<Map<usize, u32>>,
    /// たどりの前のヒープ。
    pub(crate) base: Ghost<Core>,
    pub(crate) _mode: PhantomData<M>,
}

impl<M> Objects<M> {
    /// 訪れた番地。集まりをまだ作っていなければ空である。
    pub(crate) open spec fn seen_addrs(&self) -> Set<usize> {
        match self.seen {
            Some(set) => set@,
            None => Set::empty(),
        }
    }

    /// 節点を決めるヒープ。たどりの前のヒープに、預かった塊の権限を戻したものである。
    pub(crate) open spec fn view(&self) -> Core {
        Core { env: self.env, ..self.base@ }
    }

    /// 節点 `h` の権限を借りる。ヒープのブロックなら預かった権限から、塊のブロックなら塊の権限から借りる。どちらから
    /// 借りるかは ghost の条件で決まる。
    proof fn tracked_node(tracked &self, h: usize, checked: bool) -> (tracked p: &BlockPerm)
        requires
            node_ok(self.view(), self.perms@, checked, h),
        ensures
            *p == node_perm(self.view(), self.perms@, h),
    {
        if self.view().in_region(h) {
            self.env.borrow().tracked_region().tracked_perm(h, self.view().checked())
        } else {
            self.perms.borrow().tracked_borrow(h)
        }
    }
}

impl<M: Mode> Graph for Objects<M> {
    type Node = *mut Header;

    open spec fn gv(&self) -> GView<*mut Header> {
        graph_of(self.view(), self.perms@, self.owner@, M::CHECKED)
    }

    open spec fn kept(&self, old: Self) -> bool {
        &&& self.base == old.base
        &&& self.env == old.env
        &&& same_frame(self.view(), self.perms@, old.perms@, M::CHECKED)
        &&& old.owner@.dom().subset_of(self.owner@.dom())
        &&& forall|a: usize|
            #![trigger self.owner@.dom().contains(a)]
            self.owner@.dom().contains(a) ==> old.owner@.dom().contains(a) || (node_ok(
                self.view(),
                old.perms@,
                M::CHECKED,
                a,
            ) && !self.view().in_region(a))
        &&& forall|a: usize|
            #![trigger self.owner@[a]]
            !node_ok(self.view(), old.perms@, M::CHECKED, a) ==> self.owner@[a] == old.owner@[a]
        &&& owners_ok(self.view(), old.perms@, old.owner@, M::CHECKED) ==> owners_ok(
            self.view(),
            self.perms@,
            self.owner@,
            M::CHECKED,
        )
    }

    proof fn lemma_kept_refl(&self) {
    }

    proof fn lemma_kept_trans(&self, mid: Self, old: Self) {
        lemma_same_frame_graph(
            self.view(),
            mid.perms@,
            old.perms@,
            mid.owner@,
            old.owner@,
            M::CHECKED,
        );
        lemma_same_frame_graph(
            self.view(),
            self.perms@,
            mid.perms@,
            self.owner@,
            mid.owner@,
            M::CHECKED,
        );
    }

    fn live(&self, h: *mut Header) -> (r: bool) {
        proof {
            reveal(graph_of);
            lemma_node(self.view(), self.perms@, M::CHECKED, h);
        }
        if !M::CHECKED {
            return true;
        }
        let tracked p = self.tracked_node(h@.addr, M::CHECKED);
        let tracked q = p.prefix.tracked_borrow();
        let state = u32_load(prefix_state_ptr(h), Tracked(&q.state));
        state & 3 == ALIVE_STATE
    }

    fn rc(&self, h: *mut Header) -> (r: i32) {
        proof {
            reveal(graph_of);
            lemma_node(self.view(), self.perms@, M::CHECKED, h);
        }
        let tracked p = self.tracked_node(h@.addr, M::CHECKED);
        rc_load(rc_ptr(h), Tracked(&p.rc))
    }

    fn set_rc(&mut self, h: *mut Header, rc: i32) {
        let ghost old_self = *self;
        proof {
            reveal(graph_of);
            lemma_node(self.view(), self.perms@, M::CHECKED, h);
            // rc が 0 でない節点は、塊の物体でない
            if self.view().in_region(h@.addr) {
                self.view().lemma_region_rc(h@.addr);
            }
        }
        if M::CHECKED {
            let tracked p = self.perms.borrow().tracked_borrow(h@.addr);
            let before = rc_load(rc_ptr(h), Tracked(&p.rc));
            let tracked q = p.prefix.tracked_borrow();
            let owner = u32_load(prefix_first_ptr(h), Tracked(&q.first));
            if before > 0 && rc < 0 {
                self.shared = self.shared.wrapping_add(1);
            }
            if before > 0 && rc <= 0 {
                count_owned(&mut self.owned, owner, false);
            } else if before <= 0 && rc > 0 {
                count_owned(&mut self.owned, owner, true);
            }
        }
        let tracked mut p = self.perms.borrow_mut().tracked_remove(h@.addr);
        rc_store(rc_ptr(h), Tracked(&mut p.rc), rc);
        proof {
            reveal(graph_of);
            self.perms.borrow_mut().tracked_insert(h@.addr, p);
            assert(self.perms@.dom() =~= old_self.perms@.dom());
            lemma_same_frame_graph(
                self.view(),
                self.perms@,
                old_self.perms@,
                self.owner@,
                old_self.owner@,
                M::CHECKED,
            );
            let ns = nodes_of(self.view(), old_self.perms@, M::CHECKED);
            assert forall|n: *mut Header| #[trigger] ns.contains(n) implies self.gv().rc[n]
                == old_self.gv().with_rc(h, rc).rc[n] by {
                lemma_node(self.view(), old_self.perms@, M::CHECKED, n);
            }
            assert(self.gv().rc =~= old_self.gv().with_rc(h, rc).rc);
            assert(self.gv().owner =~= old_self.gv().owner);
            if owners_ok(self.view(), old_self.perms@, old_self.owner@, M::CHECKED) {
                assert forall|a: usize| #[trigger]
                    node_ok(self.view(), self.perms@, M::CHECKED, a) && self.view().state(a)
                        == BlockState::Live && (self.view().rc_of(a) > 0 || (!self.view().in_region(
                        a,
                    ) && self.perms@[a].rc.value() > 0))
                        && self.perms@[a].prefix is Some implies p_owner(self.perms@[a])
                    == self.owner@[a] by {
                    assert(node_ok(self.view(), old_self.perms@, M::CHECKED, a));
                    if a == h@.addr {
                        assert(old_self.gv().rc[h] == old_self.perms@[a].rc.value());
                    }
                }
            }
        }
    }

    fn lin(&self, h: *mut Header) -> (r: bool) {
        proof {
            reveal(graph_of);
            lemma_node(self.view(), self.perms@, M::CHECKED, h);
        }
        let tracked p = self.tracked_node(h@.addr, M::CHECKED);
        let bits = *ptr_ref(meta_ptr(h), Tracked(&p.meta));
        Meta::from_bits(bits).lin()
    }

    fn set_owner(&mut self, h: *mut Header, owner: u32) {
        let ghost old_self = *self;
        proof {
            reveal(graph_of);
            lemma_node(self.view(), self.perms@, M::CHECKED, h);
            // rc が 0 でない節点は、塊の物体でない
            if self.view().in_region(h@.addr) {
                self.view().lemma_region_rc(h@.addr);
            }
        }
        if M::CHECKED {
            let tracked p = self.perms.borrow().tracked_borrow(h@.addr);
            if rc_load(rc_ptr(h), Tracked(&p.rc)) > 0 {
                let tracked q = p.prefix.tracked_borrow();
                let before = u32_load(prefix_first_ptr(h), Tracked(&q.first));
                move_owned(&mut self.owned, before, owner);
            }
            let tracked mut p = self.perms.borrow_mut().tracked_remove(h@.addr);
            let tracked mut q = p.prefix.tracked_unwrap();
            u32_store(prefix_first_ptr(h), Tracked(&mut q.first), owner);
            proof {
                reveal(graph_of);
                p.prefix = Some(q);
                self.perms.borrow_mut().tracked_insert(h@.addr, p);
                assert(self.perms@.dom() =~= old_self.perms@.dom());
            }
        }
        proof {
            self.owner = Ghost(self.owner@.insert(h@.addr, owner));
            lemma_same_frame_graph(
                self.view(),
                self.perms@,
                old_self.perms@,
                self.owner@,
                old_self.owner@,
                M::CHECKED,
            );
            let ns = nodes_of(self.view(), old_self.perms@, M::CHECKED);
            assert forall|n: *mut Header| #[trigger] ns.contains(n) implies self.gv().owner[n]
                == old_self.gv().with_owner(h, owner).owner[n] && self.gv().rc[n]
                == old_self.gv().rc[n] by {
                lemma_node(self.view(), old_self.perms@, M::CHECKED, n);
            }
            assert(self.gv().owner =~= old_self.gv().with_owner(h, owner).owner);
            assert(self.gv().rc =~= old_self.gv().rc);
            if owners_ok(self.view(), old_self.perms@, old_self.owner@, M::CHECKED) {
                assert forall|a: usize| #[trigger]
                    node_ok(self.view(), self.perms@, M::CHECKED, a) && self.view().state(a)
                        == BlockState::Live && (self.view().rc_of(a) > 0 || (!self.view().in_region(
                        a,
                    ) && self.perms@[a].rc.value() > 0))
                        && self.perms@[a].prefix is Some implies p_owner(self.perms@[a])
                    == self.owner@[a] by {
                    assert(node_ok(self.view(), old_self.perms@, M::CHECKED, a));
                }
            }
        }
    }

    fn flip(&mut self, h: *mut Header, owner: u32) {
        let ghost old_self = *self;
        proof {
            reveal(graph_of);
            lemma_node(self.view(), self.perms@, M::CHECKED, h);
            // rc が 0 でない節点は、塊の物体でない
            if self.view().in_region(h@.addr) {
                self.view().lemma_region_rc(h@.addr);
            }
        }
        let tracked mut p = self.perms.borrow_mut().tracked_remove(h@.addr);
        rc_store(rc_ptr(h), Tracked(&mut p.rc), 1);
        if M::CHECKED {
            // rc が負の物体は数に入っていないので、前置きの古い所有者は読まずに、新しい所有者の数だけを増やす
            count_owned(&mut self.owned, owner, true);
            let tracked mut q = p.prefix.tracked_unwrap();
            u32_store(prefix_first_ptr(h), Tracked(&mut q.first), owner);
            proof {
                p.prefix = Some(q);
            }
        }
        proof {
            reveal(graph_of);
            self.perms.borrow_mut().tracked_insert(h@.addr, p);
            assert(self.perms@.dom() =~= old_self.perms@.dom());
            self.owner = Ghost(self.owner@.insert(h@.addr, owner));
            lemma_same_frame_graph(
                self.view(),
                self.perms@,
                old_self.perms@,
                self.owner@,
                old_self.owner@,
                M::CHECKED,
            );
            let ns = nodes_of(self.view(), old_self.perms@, M::CHECKED);
            let want = old_self.gv().with_rc(h, 1).with_owner(h, owner);
            assert forall|n: *mut Header| #[trigger] ns.contains(n) implies self.gv().owner[n]
                == want.owner[n] && self.gv().rc[n] == want.rc[n] by {
                lemma_node(self.view(), old_self.perms@, M::CHECKED, n);
            }
            assert(self.gv().owner =~= want.owner);
            assert(self.gv().rc =~= want.rc);
            if owners_ok(self.view(), old_self.perms@, old_self.owner@, M::CHECKED) {
                assert forall|a: usize| #[trigger]
                    node_ok(self.view(), self.perms@, M::CHECKED, a) && self.view().state(a)
                        == BlockState::Live && (self.view().rc_of(a) > 0 || (!self.view().in_region(
                        a,
                    ) && self.perms@[a].rc.value() > 0))
                        && self.perms@[a].prefix is Some implies p_owner(self.perms@[a])
                    == self.owner@[a] by {
                    assert(node_ok(self.view(), old_self.perms@, M::CHECKED, a));
                }
            }
        }
    }

    open spec fn checked(&self) -> bool {
        M::CHECKED
    }

    open spec fn owners_known(&self) -> bool {
        owners_ok(self.view(), self.perms@, self.owner@, M::CHECKED)
    }

    proof fn lemma_kept_known(&self, old: Self) {
    }

    fn owner(&self, h: *mut Header) -> (r: u32) {
        proof {
            reveal(graph_of);
            lemma_node(self.view(), self.perms@, M::CHECKED, h);
            // rc が 0 でない節点は、塊の物体でない
            if self.view().in_region(h@.addr) {
                self.view().lemma_region_rc(h@.addr);
            }
            // 生きている節点は、前置きの状態が生存で、構築中でないので生きている物体である

            assert(self.view().state(h@.addr) == BlockState::Live);
        }
        let tracked p = self.tracked_node(h@.addr, M::CHECKED);
        let tracked q = p.prefix.tracked_borrow();
        u32_load(prefix_first_ptr(h), Tracked(&q.first))
    }

    fn push_children(&self, h: *mut Header, out: &mut Vec<*mut Header>) {
        proof {
            reveal(graph_of);
            lemma_node(self.view(), self.perms@, M::CHECKED, h);
            // たどりは生きている節点だけを展開する。前置きの状態はブロックの状態を表すので、その節点は生きている物体である
            assert(self.view().state(h@.addr) == BlockState::Live);
        }
        let tracked p = self.tracked_node(h@.addr, M::CHECKED);
        let bits = *ptr_ref(meta_ptr(h), Tracked(&p.meta));
        let m = Meta::from_bits(bits);
        let (first, n) = if m.scan() == Meta::EXTENDED_SCAN {
            let w = *ptr_ref(slot_ptr(h, 0), Tracked(p.slots.tracked_borrow(0)));
            (1usize, w.bits())
        } else {
            (0usize, m.scan() as usize)
        };
        let mut i: usize = 0;
        while i < n
            invariant
                out@ == old(out)@ + kids(*p, first as int, i as int),
                i <= n,
                first == p_first(*p),
                n == p_count(*p),
                node_ok(self.view(), self.perms@, M::CHECKED, h@.addr),
                self.view().state(h@.addr) == BlockState::Live,
                *p == node_perm(self.view(), self.perms@, h@.addr),
                h == self.view().block(h@.addr),
            decreases n - i,
        {
            proof {
                assert(p_scanned(*p, (first + i) as int));
            }
            let w = *ptr_ref(
                slot_ptr(h, first + i),
                Tracked(p.slots.tracked_borrow((first + i) as int)),
            );
            let ghost before = out@;
            if w.is_ptr() {
                let c = w.as_ptr();
                proof {
                    lemma_ptr_of(w, c);
                }
                out.push(c);
            }
            proof {
                assert(out@ =~= old(out)@ + kids(*p, first as int, i + 1));
            }
            i += 1;
        }
    }

    open spec fn seen(&self) -> Set<*mut Header> {
        self.gv().nodes.filter(|n: *mut Header| self.seen_addrs().contains(n@.addr))
    }

    fn see(&mut self, h: *mut Header) -> (fresh: bool) {
        let ghost old_self = *self;
        proof {
            reveal(graph_of);
            lemma_node(self.view(), self.perms@, M::CHECKED, h);
        }
        // 集まりは初めて使うときに作る。速いモードはここを通らないので、たどりのたびに作る費用を払わない
        let mut taken = None;
        mem::swap(&mut taken, &mut self.seen);
        let mut set = match taken {
            Some(set) => set,
            None => HashSetWithView::new(),
        };
        let fresh = set.insert(h.addr());
        self.seen = Some(set);
        proof {
            // 番地の同じ節点は同じヘッダなので、番地を足すと節点がちょうど1つ足される
            assert forall|n: *mut Header| #[trigger]
                self.seen().contains(n) == old_self.seen().insert(h).contains(n) by {
                if self.gv().nodes.contains(n) {
                    lemma_node(self.view(), self.perms@, M::CHECKED, n);
                }
            }
            assert(self.seen() =~= old_self.seen().insert(h));
        }
        fresh
    }
}

/// 走査する語のうち物体を指すもののヘッダがどれも `pred` を満たすなら、子の列の要素もどれも満たす。
pub(crate) proof fn lemma_kids(p: BlockPerm, first: int, i: int, pred: spec_fn(*mut Header) -> bool)
    requires
        forall|j: int|
            #![trigger p_word(p, j)]
            first <= j < first + i && p_word(p, j).is_ptr_spec() ==> pred(ptr_of(p_word(p, j))),
    ensures
        forall|k: int| 0 <= k < kids(p, first, i).len() ==> pred(#[trigger] kids(p, first, i)[k]),
    decreases i,
{
    if i > 0 {
        lemma_kids(p, first, i - 1, pred);
        let prev = kids(p, first, i - 1);
        assert forall|k: int| 0 <= k < kids(p, first, i).len() implies pred(
            #[trigger] kids(p, first, i)[k],
        ) by {
            if k < prev.len() {
                assert(kids(p, first, i)[k] == prev[k]);
            } else {
                assert(kids(p, first, i)[k] == ptr_of(p_word(p, first + i - 1)));
            }
        }
    }
}

/// 不変条件を満たすヒープの生きている物体は節点で、そのグラフは形の前提を満たす。
pub(crate) proof fn lemma_shaped(s0: Core, a: usize)
    requires
        s0.full() || s0.checked(),
        s0.checked() ==> s0.safe_wf(),
        s0.model@.building is None,
        s0.has(a),
        !s0.checked() ==> s0.state(a) == BlockState::Live,
    ensures
        shaped(graph_of(s0, s0.perms@, s0.model@.owner, s0.checked()), s0.block(a)),
        forall|b: usize|
            #![trigger s0.state(b)]
            s0.dom().contains(b) && s0.state(b) == BlockState::Live ==> node_ok(
                s0,
                s0.perms@,
                s0.checked(),
                b,
            ),
{
    reveal(graph_of);
    reveal(Core::building_wf);
    let checked = s0.checked();
    // 検査モードの節点は構築中でないブロックのすべてで、そのどれもが安全の条件を満たす (`Core::safe_wf`)
    assert forall|b: usize|
        s0.has(b) && (if checked {
            s0.state(b) != BlockState::Building
        } else {
            s0.state(b) == BlockState::Live
        }) implies #[trigger] node_ok(s0, s0.perms@, checked, b) by {
        s0.lemma_shape(b);
        let p = s0.perm(b);
        if !s0.dom().contains(b) {
            s0.lemma_region_at(b);
        } else if checked {
            s0.lemma_safe_at(b);
            if s0.state(b) != BlockState::Freed {
                assert forall|i: int| #[trigger] p_scanned(p, i) implies p_init(p, i) by {
                    assert(s0.safe_slot(p, i));
                }
            }
        } else {
            s0.lemma_at(b);
            assert forall|i: int| #[trigger] p_scanned(p, i) implies p_init(p, i) by {
                assert(s0.slot_word(p, b, i));
            }
        }
    }
    assert forall|b: usize|
        #![trigger s0.state(b)]
        s0.dom().contains(b) && s0.state(b) == BlockState::Live implies node_ok(
        s0,
        s0.perms@,
        checked,
        b,
    ) by {}
    let g = graph_of(s0, s0.perms@, s0.model@.owner, checked);
    lemma_is_node(s0, s0.perms@, checked, a);
    assert forall|n: *mut Header, k: int|
        g.nodes.contains(n) && 0 <= k < g.children[n].len() implies g.nodes.contains(
        #[trigger] g.children[n][k],
    ) by {
        lemma_node(s0, s0.perms@, checked, n);
        let b = n@.addr;
        let p = s0.perm(b);
        assert(s0.state(b) != BlockState::Freed);
        let pred = |x: *mut Header| g.nodes.contains(x);
        assert forall|j: int|
            #![trigger p_word(p, j)]
            p_first(p) <= j < p_first(p) + p_count(p) && p_word(p, j).is_ptr_spec() implies pred(
            ptr_of(p_word(p, j)),
        ) by {
            assert(p_scanned(p, j));
            let t = p_word(p, j).bits_spec();
            if !s0.dom().contains(b) {
                s0.lemma_region_at(b);
            } else if checked {
                s0.lemma_safe_at(b);
                assert(s0.safe_slot(p, j));
            } else {
                s0.lemma_at(b);
                reveal(Core::edges_wf);
                assert(s0.slot_word(p, b, j));
                assert(s0.edge_wf((b, j)));
            }
            assert(s0.has(t));
            s0.lemma_shape(t);
            vstd::raw_ptr::ptrs_mut_eq_sized(s0.block(t));
            vstd::raw_ptr::ptrs_mut_eq_sized(ptr_of(p_word(p, j)));
            assert(ptr_of(p_word(p, j)) == s0.block(t));
            assert(s0.has(t) && (if checked {
                s0.state(t) != BlockState::Building
            } else {
                s0.state(t) == BlockState::Live
            }));
            lemma_is_node(s0, s0.perms@, checked, t);
        }
        lemma_kids(p, p_first(p), p_count(p), pred);
    }
}

/// たどりの前後で、ヒープの基準のブロックの位置と状態が同じなら、物体のグラフも同じ形で読める。
pub(crate) proof fn lemma_graph_base(
    b1: Core,
    b0: Core,
    perms: Map<usize, BlockPerm>,
    owner: Map<usize, u32>,
    checked: bool,
)
    requires
        b1.dom() == b0.dom(),
        b1.pages == b0.pages,
        b1.env == b0.env,
        b1.model@.state == b0.model@.state,
    ensures
        graph_of(b1, perms, owner, checked) == graph_of(b0, perms, owner, checked),
{
    reveal(graph_of);
    assert forall|a: usize| #[trigger]
        node_ok(b1, perms, checked, a) == node_ok(b0, perms, checked, a) by {}
    assert(b1.dom().union(b1.region_blocks()).filter(|a: usize| node_ok(b1, perms, checked, a))
        =~= b0.dom().union(b0.region_blocks()).filter(|a: usize| node_ok(b0, perms, checked, a)));
    assert(nodes_of(b1, perms, checked) =~= nodes_of(b0, perms, checked)) by {
        let s = b0.dom().union(b0.region_blocks()).filter(
            |a: usize| node_ok(b0, perms, checked, a),
        );
        assert forall|n: *mut Header|
            nodes_of(b1, perms, checked).contains(n) == nodes_of(b0, perms, checked).contains(
                n,
            ) by {
            s.lemma_map_contains(|a: usize| b1.block(a), n);
            s.lemma_map_contains(|a: usize| b0.block(a), n);
        }
    }
    let g1 = graph_of(b1, perms, owner, checked);
    let g0 = graph_of(b0, perms, owner, checked);
    assert(g1.children =~= g0.children) by {
        assert forall|n: *mut Header| #[trigger]
            g1.children.dom().contains(n) implies g1.children[n] == g0.children[n] by {
            assert(b1.state(n@.addr) == b0.state(n@.addr));
        }
    }
    assert(g1.live =~= g0.live);
    assert(g1.rc =~= g0.rc);
    assert(g1.lin =~= g0.lin);
    assert(g1.owner =~= g0.owner);
}

} // verus!
