//! 実行文脈 `Ctx` (docs/spec/lir.md の「実行の文脈」)。ヒープと、今の `evv` と、yield の状態を持ち、基本操作
//! (docs/spec/lir.md の「基本操作」) をメソッドにする。ヒープの spec の関数の中を開けるため、`heap` の子に置く。
//!
//! `Ctx` は ghost の状態を持たず、`evv` のリストや evidence をヒープの上の spec の関数で表す。VM は検証しないコードで、
//! 基本操作の合間にヒープを変えるので、ghost のリストを保ち続けられないためである。

use std::sync::Arc;

use vstd::prelude::*;

use super::Heap;
#[cfg(verus_only)]
use super::chain_err;
#[cfg(verus_only)]
use super::model::BlockState;
use crate::control::{Control, Stop};
use crate::error::HeapError;
use crate::meta::{Kind, Meta};
#[cfg(verus_only)]
use crate::meta::{bits_kind, bits_lin, bits_scan, bits_tag, bits_wf};
use crate::mode::Mode;
use crate::word::Word;
#[cfg(verus_only)]
use crate::word::lemma_unit_bits;
#[cfg(verus_only)]
use proof::*;

// 証明の部品だけを置くので、検証のときだけ読む
#[cfg(verus_only)]
mod proof;

verus! {

/// 検査モードだけで数える仕事の回数。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct CtxStats {
    pub evidence_steps: u64,
    pub yields: u64,
    pub yield_frames: u64,
}

/// yield の状態。`target` が `()` なら yield していない。Rust の enum は配置が決まらないので、平らな記録にして、
/// ネイティブのコードが決まった位置の語を1回比べれば yield 中かを読めるようにする。
#[repr(C)]
pub(crate) struct YieldState {
    /// 宛先の evidence。
    pub(crate) target: Word,
    /// 節の本体のクロージャ。
    pub(crate) clause: Word,
    /// 組み立て中の継続 κ。
    pub(crate) cont: Word,
    /// 中断か。
    pub(crate) abort: bool,
}

impl YieldState {
    pub(crate) open spec fn wf(&self) -> bool {
        self.target.bits_spec() == 1 ==> {
            &&& self.clause.bits_spec() == 1
            &&& self.cont.bits_spec() == 1
            &&& !self.abort
        }
    }

    fn none() -> (r: YieldState)
        ensures
            r.target.bits_spec() == 1,
            r.wf(),
    {
        YieldState { target: Word::UNIT, clause: Word::UNIT, cont: Word::UNIT, abort: false }
    }
}

/// `w` が物体 `b` を指せば 1、そうでなければ 0。RC のモデルでの増減を、語ごとに足して書くのに使う。
pub open spec fn points(w: Word, b: usize) -> int {
    if w.is_ptr_spec() && w.bits_spec() == b {
        1
    } else {
        0
    }
}

/// `ws` のうち物体 `b` を指す語の数。
spec fn points_in(ws: Seq<Word>, b: usize) -> int
    decreases ws.len(),
{
    if ws.len() == 0 {
        0
    } else {
        points_in(ws.drop_last(), b) + points(ws.last(), b)
    }
}

/// `b` は生きているブロック (書き終えた構築中の物体を含む) である。
spec fn live<M: Mode>(h: Heap<M>, b: usize) -> bool {
    h.core.has(b) && h.core.settled_state(b) == BlockState::Live
}

/// ブロック `b` は、`h0` と同じ `meta`、rc の符号、持ち主を持つ。`lin` ビットと `owned` はこれで決まる。
spec fn same_head<M: Mode>(h1: Heap<M>, h0: Heap<M>, b: usize) -> bool {
    &&& h1.core.perm(b).meta == h0.core.perm(b).meta
    &&& (h1.core.rc_of(b) > 0) == (h0.core.rc_of(b) > 0)
    &&& (h1.core.rc_of(b) < 0) == (h0.core.rc_of(b) < 0)
    &&& h1.core.rc_of(b) > 0 ==> h1.core.model@.owner[b] == h0.core.model@.owner[b]
}

/// `h0` から `h1` への変化。両方で生きているブロックは、`meta`、rc の符号、持ち主を変えず、`x` のほかは語も変えない。
/// `h0` で生きているブロックのうち、不死か、外の参照の数に `d` を足して 1 以上残るものは、同じブロックのまま生きて
/// いて、外の参照の数がちょうど `d` 変わる。
spec fn moved<M: Mode>(h1: Heap<M>, h0: Heap<M>, d: spec_fn(usize) -> int, x: Set<usize>) -> bool {
    let v0 = h0.core.settled_view();
    let v1 = h1.core.settled_view();
    &&& h1.dummy == h0.dummy
    &&& h1.region == h0.region
    &&& h1.worker == h0.worker
    &&& h1.core.env == h0.core.env
    &&& forall|b: usize|
        #![trigger live(h1, b), live(h0, b)]
        live(h0, b) && live(h1, b) ==> {
            &&& same_head(h1, h0, b)
            &&& !x.contains(b) ==> h1.core.perm(b).slots == h0.core.perm(b).slots
        }
    &&& forall|b: usize|
        #![trigger live(h0, b)]
        live(h0, b) && (v0.rc_of(b) == 0 || v0.ext(b) + d(b) >= 1) ==> {
            &&& live(h1, b)
            &&& h1.core.block(b) == h0.core.block(b)
            &&& v0.rc_of(b) != 0 ==> v1.ext(b) == v0.ext(b) + d(b)
        }
}

/// `h0` で生きているブロックは、`h1` でも同じブロックのまま生きている。借りた語も読めるままである。
spec fn no_free<M: Mode>(h1: Heap<M>, h0: Heap<M>) -> bool {
    forall|b: usize| #[trigger] live(h0, b) ==> live(h1, b) && h1.core.block(b) == h0.core.block(b)
}

/// `h0` で生きている物体の外の参照の数は、どれもちょうど `d` 変わる。物体を空けない変化 (`dup`、確保、`set_field`)
/// はこれを満たすので、外の参照を増やす変化の後にも変化をつなげられる。
spec fn exact<M: Mode>(h1: Heap<M>, h0: Heap<M>, d: spec_fn(usize) -> int) -> bool {
    forall|b: usize|
        #![trigger live(h0, b)]
        live(h0, b) && h0.core.settled_view().rc_of(b) != 0 ==> h1.core.settled_view().ext(b)
            == h0.core.settled_view().ext(b) + d(b)
}

/// `h1` で生きているブロックは、`h0` でも生きていた。
spec fn no_alloc<M: Mode>(h1: Heap<M>, h0: Heap<M>) -> bool {
    forall|b: usize| #[trigger] live(h1, b) ==> live(h0, b)
}

/// `w` は即値か、`lin` ビットがなく持ち主が今の worker の物体を指す。`dup` できる。
spec fn dupable<M: Mode>(h: Heap<M>, w: Word) -> bool {
    w.is_ptr_spec() ==> !h.lin_of(w) && h.owned(w)
}

/// `w` は即値か、持ち主が今の worker の物体を指し、共有なら `lin` ビットがない。参照を持てば `decref` できる。
spec fn droppable<M: Mode>(h: Heap<M>, w: Word) -> bool {
    &&& h.owned(w)
    &&& w.is_ptr_spec() && h.rc_value(w) < 0 ==> !h.lin_of(w)
}

spec fn held_word<M: Mode>(h: Heap<M>, w: Word) -> bool {
    h.readable(w) && droppable(h, w)
}

/// `w` は読める `Data` の物体を指し、`lin` ビットがなく、走査する語を数の語なしに `scan` 個持つ。
spec fn data_obj<M: Mode>(h: Heap<M>, w: Word, scan: u8) -> bool {
    &&& w.is_ptr_spec()
    &&& h.readable(w)
    &&& h.kind_of(w) == Kind::Data
    &&& h.scan_of(w) == scan
    &&& !h.lin_of(w)
    &&& h.owned(w)
}

/// 入口の表。読める tag 0 の `Data` の物体で、どの欄も `dup` できる。
spec fn table_ok<M: Mode>(h: Heap<M>, t: Word) -> bool {
    &&& t.is_ptr_spec()
    &&& h.readable(t)
    &&& h.kind_of(t) == Kind::Data
    &&& h.tag_of(t) == 0
    &&& !h.lin_of(t)
    &&& h.owned(t)
    &&& forall|i: int| 0 <= i < h.count_of(t) ==> dupable(h, #[trigger] h.scanned_word(t, i))
}

/// evidence の浅い形 (docs/spec/object-model.md の「evidence の3つの物体」)。[入口の表, 状態, 外側の evv] を持つ。
/// 外側の evv がリストとして正しいことは入れない。入れると節点の述語と互いに再帰し、検査モードで確かめる費用が
/// handler の入れ子の深さについて指数になるためである。
spec fn ev_ok<M: Mode>(h: Heap<M>, e: Word) -> bool {
    &&& data_obj(h, e, 3)
    &&& h.tag_of(e) != 0
    &&& table_ok(h, h.scanned_word(e, 0))
    &&& droppable(h, h.scanned_word(e, 1))
    &&& dupable(h, h.scanned_word(e, 2))
}

/// `evv` の節点の形。[evidence, 次の節点] を持つ。
spec fn node_ok<M: Mode>(h: Heap<M>, n: Word) -> bool {
    data_obj(h, n, 2) && h.tag_of(n) != 0
}

/// `w` から節点をたどり、`fuel` 歩以内に `()` に着くなら、(tag, evidence) の組の列。輪になったリストや形の違う
/// 節点では `None` になる。
spec fn path<M: Mode>(h: Heap<M>, w: Word, fuel: nat) -> Option<Seq<(u16, Word)>>
    decreases fuel,
{
    if w.bits_spec() == 1 {
        Some(Seq::empty())
    } else if fuel == 0 {
        None
    } else {
        let e = h.scanned_word(w, 0);
        if node_ok(h, w) && ev_ok(h, e) && h.tag_of(e) == h.tag_of(w) {
            match path(h, h.scanned_word(w, 1), (fuel - 1) as nat) {
                Some(s) => Some(seq![(h.tag_of(w), e)] + s),
                None => None,
            }
        } else {
            None
        }
    }
}

spec fn seq_of<M: Mode>(h: Heap<M>, w: Word) -> Seq<(u16, Word)> {
    match path(h, w, choose|f: nat| #[trigger] path(h, w, f) is Some) {
        Some(s) => s,
        None => Seq::empty(),
    }
}

spec fn listed<M: Mode>(h: Heap<M>, w: Word, max: usize) -> bool {
    &&& exists|f: nat| #[trigger] path(h, w, f) is Some
    &&& seq_of(h, w).len() <= max
}

/// エフェクトの evidence と節点の tag (docs/spec/object-model.md の「evidence の3つの物体」)。65535 の tag は 0 になり、
/// tag 0 の evidence はないので、見つからない。
spec fn effect_tag(effect: u16) -> u16 {
    if effect == u16::MAX {
        0
    } else {
        (effect + 1) as u16
    }
}

/// 先頭から見て、tag が `tag` の最初の evidence。
spec fn find(s: Seq<(u16, Word)>, tag: u16) -> Option<Word>
    decreases s.len(),
{
    if s.len() == 0 {
        None
    } else if s[0].0 == tag {
        Some(s[0].1)
    } else {
        find(s.drop_first(), tag)
    }
}

/// 実行文脈。VM はこれだけを持ち回す。`evv` と yield の状態の語は、`Ctx` が持つ数えた参照である。`Ctx` を落としても
/// これらの語には触れず、ヒープと一緒に消える。
pub struct Ctx<M: Mode> {
    heap: Heap<M>,
    evv: Word,
    yielding: YieldState,
    /// 安全点の確かめがヒープの中をたどらずに読めるよう、ヒープとは別に持つ。
    control: Arc<Control>,
    max_depth: usize,
    /// `evv` の長さの上限。リストをたどるループは、これを超えたら止まる。
    max_evv: usize,
    stats: CtxStats,
}

impl<M: Mode> Ctx<M> {
    pub closed spec fn heap_spec(&self) -> Heap<M> {
        self.heap
    }

    pub closed spec fn evv_spec(&self) -> Word {
        self.evv
    }

    /// yield の状態。宛先、節の本体、κ、中断か、の組である。
    pub closed spec fn yield_spec(&self) -> (Word, Word, Word, bool) {
        (self.yielding.target, self.yielding.clause, self.yielding.cont, self.yielding.abort)
    }

    pub open spec fn is_yielding(&self) -> bool {
        self.yield_spec().0.bits_spec() != 1
    }

    /// κ の欄が空。関数 0 の値も `()` と同じ語なので、空に見える。
    pub open spec fn cont_empty(&self) -> bool {
        self.yield_spec().2.bits_spec() == 1
    }

    /// `evv` が空で、yield していない。
    pub open spec fn finished(&self) -> bool {
        self.evv_spec().bits_spec() == 1 && !self.is_yielding()
    }

    pub closed spec fn evv_limit(&self) -> usize {
        self.max_evv
    }

    /// `e` は evidence の形をしている。
    pub closed spec fn ev_shape(&self, e: Word) -> bool {
        ev_ok(self.heap, e)
    }

    /// `w` から `fuel` 歩以内に `()` に着くなら、(tag, evidence) の組の列。
    pub closed spec fn evv_path(&self, w: Word, fuel: nat) -> Option<Seq<(u16, Word)>> {
        path(self.heap, w, fuel)
    }

    /// `w` のリストの (tag, evidence) の組の列。`evv_wf(w)` のときに意味を持つ。
    pub closed spec fn evv_seq(&self, w: Word) -> Seq<(u16, Word)> {
        seq_of(self.heap, w)
    }

    /// `w` は `evv` のリストとして正しく、長さが上限以下である。
    pub closed spec fn evv_wf(&self, w: Word) -> bool {
        listed(self.heap, w, self.max_evv)
    }

    /// 今の `evv` で、エフェクト `effect` の最初の evidence。tag はエフェクトの番号 + 1 なので、65535 は見つからない。
    pub closed spec fn lookup(&self, effect: u16) -> Option<Word> {
        find(self.evv_seq(self.evv), effect_tag(effect))
    }

    pub closed spec fn ev_tag(&self, e: Word) -> u16 {
        self.heap.tag_of(e)
    }

    pub closed spec fn ev_table(&self, e: Word) -> Word {
        self.heap.scanned_word(e, 0)
    }

    pub closed spec fn ev_state(&self, e: Word) -> Word {
        self.heap.scanned_word(e, 1)
    }

    pub closed spec fn ev_outer(&self, e: Word) -> Word {
        self.heap.scanned_word(e, 2)
    }

    /// 入口の数。
    pub closed spec fn ev_ops(&self, e: Word) -> int {
        self.heap.count_of(self.ev_table(e))
    }

    pub closed spec fn ev_entry(&self, e: Word, op: int) -> Word {
        self.heap.scanned_word(self.ev_table(e), op)
    }

    /// `old` からの RC のモデルでの変化。`old` で生きている物体のうち、不死か、外の参照の数に `d` を足して 1 以上
    /// 残るものは生きていて、外の参照の数がちょうど `d` 変わる。両方で生きている物体は、`meta`、rc の符号、持ち主を
    /// 変えず、`x` のほかは走査する語も変えない。`d` と `x` は物体を番地で受ける。
    pub closed spec fn evolved_by(
        &self,
        old: Ctx<M>,
        d: spec_fn(usize) -> int,
        x: Set<usize>,
    ) -> bool {
        moved(self.heap, old.heap, d, x)
    }

    /// `Ctx` の語と `ws` の語のうち、物体 `b` を指すものの数。
    spec fn need(&self, ws: Seq<Word>, b: usize) -> int {
        points(self.evv, b) + points(self.yielding.target, b) + points(self.yielding.clause, b)
            + points(self.yielding.cont, b) + points_in(ws, b)
    }

    /// `Ctx` の語と `ws` の語を合わせて、語ごとに参照を1つ持つ。どの語も即値か生きている物体を指し、`decref` で
    /// 手放せる。同じ物体を指す語が並べば、物体の外の参照の数はその数以上ある。`Ctx` の語を数えるのは、
    /// `evv_set(evv_get())` のように `Ctx` の持つ物体を引数に渡すとき、引数の参照が `Ctx` の参照と別にあることを
    /// 言うためである。
    pub closed spec fn holds_args(&self, ws: Seq<Word>) -> bool {
        let v = self.heap.core.settled_view();
        &&& held_word(self.heap, self.evv)
        &&& held_word(self.heap, self.yielding.target)
        &&& held_word(self.heap, self.yielding.clause)
        &&& held_word(self.heap, self.yielding.cont)
        &&& forall|i: int| 0 <= i < ws.len() ==> held_word(self.heap, #[trigger] ws[i])
        &&& forall|b: usize| #[trigger]
            self.need(ws, b) > 0 && v.rc_of(b) != 0 ==> v.ext(b) >= self.need(ws, b)
    }

    /// 呼ぶ側が `w` の参照を、`Ctx` の参照と別に1つ持つ。
    pub closed spec fn holds(&self, w: Word) -> bool {
        self.holds_args(seq![w])
    }

    /// モデルが合っていて、`Ctx` の持つ語を `Ctx` が持っている。速いモードの基本操作の前提であり、検査モードでは、
    /// これが成り立てば基本操作は `Ok` を返す。
    pub closed spec fn held_ok(&self) -> bool {
        &&& self.heap.consistent()
        &&& self.heap.settleable()
        &&& self.evv_wf(self.evv)
        &&& self.holds_args(seq![])
    }

    /// 不変条件。検査モードでは安全の条件で、`Err` の後も保つ。速いモードでは `held_ok` も含む。
    pub closed spec fn wf(&self) -> bool {
        &&& self.heap.wf()
        &&& self.heap.safe_settleable()
        &&& self.yielding.wf()
        &&& self.heap.word_in(self.evv)
        &&& self.heap.word_in(self.yielding.target)
        &&& self.heap.word_in(self.yielding.clause)
        &&& self.heap.word_in(self.yielding.cont)
        &&& !M::CHECKED ==> self.held_ok()
    }

    /// `evv` が空で、yield していない文脈。ページはセッションの `control` に数える。
    pub fn new(control: Arc<Control>) -> (r: Ctx<M>)
        ensures
            r.wf(),
            r.held_ok(),
            r.finished(),
    {
        let limits = control.limits();
        let heap = Heap::with_control(control.clone());
        let r = Ctx {
            heap,
            evv: Word::UNIT,
            yielding: YieldState::none(),
            control,
            max_depth: limits.max_depth,
            max_evv: limits.max_evv,
            stats: CtxStats { evidence_steps: 0, yields: 0, yield_frames: 0 },
        };
        proof {
            lemma_unit_bits();
            lemma_points_in(seq![], 0);
            r.lemma_unit_listed(r.evv);
            assert forall|b: usize| #[trigger] r.need(seq![], b) > 0 implies false by {
                lemma_points_in(seq![], b);
            }
        }
        r
    }

    pub fn heap(&self) -> (r: &Heap<M>)
        ensures
            *r == self.heap_spec(),
    {
        &self.heap
    }

    /// # Safety
    /// ヒープを入れ替えない。`Ctx` が持つ語 (`evv` と yield の状態) を手放したり書き換えたりせず、それらの語からたどれる
    /// 物体 (`evv` の節点、evidence、入口の表) も書き換えない。`Ctx` のメソッドを次に呼ぶ前に、作っている物体を
    /// 書き終える。
    pub unsafe fn heap_mut(&mut self) -> &mut Heap<M> {
        &mut self.heap
    }

    /// VM がリークを確かめる前に呼ぶ (docs/spec/lir.md の「`evv` の不変条件」)。
    pub fn check_finished(&self) -> (r: bool)
        ensures
            r == self.finished(),
    {
        self.evv.bits() == 1 && self.yielding.target.bits() == 1
    }

    pub fn stats(&self) -> CtxStats {
        self.stats
    }

    /// yield 中か中断中なら 1、そうでなければ 0。
    pub fn yielding(&self) -> (r: i64)
        ensures
            r == if self.is_yielding() {
                1i64
            } else {
                0i64
            },
    {
        if self.yielding.target.bits() == 1 {
            0
        } else {
            1
        }
    }

    /// 0: yield していない、1: `ev` 宛ての yield、2: `ev` 宛ての中断、3: ほかの宛先の yield か中断。宛先は語の値で
    /// 比べる。yield の状態が宛先の参照を持つので、比べる間に番地が使い回されることはない。
    pub fn yield_status(&self, ev: Word) -> (r: i64)
        ensures
            r == if !self.is_yielding() {
                0i64
            } else if self.yield_spec().0.bits_spec() != ev.bits_spec() {
                3i64
            } else if self.yield_spec().3 {
                2i64
            } else {
                1i64
            },
    {
        let target = self.yielding.target.bits();
        if target == 1 {
            0
        } else if target != ev.bits() {
            3
        } else if self.yielding.abort {
            2
        } else {
            1
        }
    }

    /// yield か中断を始める。`ev`、`clause`、`k` の参照を `Ctx` が持つ。
    fn start_yield(&mut self, ev: Word, clause: Word, k: Word, abort: bool) -> (r: Result<
        (),
        HeapError,
    >)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(ev),
            old(self).heap_spec().word_in(clause),
            old(self).heap_spec().word_in(k),
            !M::CHECKED ==> !old(self).is_yielding() && ev.is_ptr_spec() && old(self).holds_args(
                seq![ev, clause, k],
            ),
        ensures
            final(self).wf(),
            r is Err <==> M::CHECKED && (old(self).is_yielding() || !ev.is_ptr_spec()),
            r is Err ==> r == Err::<(), HeapError>(HeapError::YieldMisuse) && *final(self) == *old(
                self,
            ),
            r is Ok ==> final(self).yield_spec() == (ev, clause, k, abort),
            final(self).heap_spec() == old(self).heap_spec(),
            final(self).evv_spec() == old(self).evv_spec(),
    {
        if M::CHECKED && (self.yielding.target.bits() != 1 || !ev.is_ptr()) {
            return Err(HeapError::YieldMisuse);
        }
        proof {
            crate::word::lemma_imm_and_ptr_are_disjoint(ev.bits_spec());
            lemma_unit_bits();
        }
        self.yielding = YieldState { target: ev, clause, cont: k, abort };
        if M::CHECKED {
            self.stats.yields = self.stats.yields.wrapping_add(1);
        }
        proof {
            if !M::CHECKED {
                let s0 = *old(self);
                let ws = seq![ev, clause, k];
                assert(held_word(s0.heap, ws[0]) && held_word(s0.heap, ws[1]) && held_word(
                    s0.heap,
                    ws[2],
                ));
                lemma_moved_refl(self.heap);
                assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies s0.need(ws, b)
                    + 0 >= self.need(seq![], b) by {
                    lemma_points_in(seq![], b);
                    lemma_points_in(ws, b);
                }
                self.lemma_holds_moved(s0, ws, seq![], |b: usize| 0int, Set::empty());
            }
        }
        Ok(())
    }

    /// yield を始める。`ev` は宛先の evidence、`clause` は節の本体、`k` は最初の κ で、どれも消費する。
    ///
    /// # Safety
    /// 3つの語は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がその参照を1つずつ持つ。`ev` は物体を指す。
    /// yield していない (検査モードは `YieldMisuse` を返す)。
    pub unsafe fn yield_to(&mut self, ev: Word, clause: Word, k: Word) -> (r: Result<(), HeapError>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(ev),
            old(self).heap_spec().word_in(clause),
            old(self).heap_spec().word_in(k),
            !M::CHECKED ==> !old(self).is_yielding() && ev.is_ptr_spec() && old(self).holds_args(
                seq![ev, clause, k],
            ),
        ensures
            final(self).wf(),
            r is Err <==> M::CHECKED && (old(self).is_yielding() || !ev.is_ptr_spec()),
            r is Err ==> r == Err::<(), HeapError>(HeapError::YieldMisuse) && *final(self) == *old(
                self,
            ),
            r is Ok ==> final(self).yield_spec() == (ev, clause, k, false),
            final(self).heap_spec() == old(self).heap_spec(),
            final(self).evv_spec() == old(self).evv_spec(),
    {
        self.start_yield(ev, clause, k, false)
    }

    /// 中断を始める。引数と前提は `yield_to` と同じである。
    ///
    /// # Safety
    /// `yield_to` と同じ。
    pub unsafe fn yield_abort(&mut self, ev: Word, clause: Word, k: Word) -> (r: Result<
        (),
        HeapError,
    >)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(ev),
            old(self).heap_spec().word_in(clause),
            old(self).heap_spec().word_in(k),
            !M::CHECKED ==> !old(self).is_yielding() && ev.is_ptr_spec() && old(self).holds_args(
                seq![ev, clause, k],
            ),
        ensures
            final(self).wf(),
            r is Err <==> M::CHECKED && (old(self).is_yielding() || !ev.is_ptr_spec()),
            r is Err ==> r == Err::<(), HeapError>(HeapError::YieldMisuse) && *final(self) == *old(
                self,
            ),
            r is Ok ==> final(self).yield_spec() == (ev, clause, k, true),
            final(self).heap_spec() == old(self).heap_spec(),
            final(self).evv_spec() == old(self).evv_spec(),
    {
        self.start_yield(ev, clause, k, true)
    }

    /// κ を取り出し、欄を `()` にする。κ の参照は呼ぶ側に移る。ヒープに触れないので安全な関数にする。yield して
    /// いなければ、検査モードは `YieldMisuse` を返し、速いモードは空の欄の `()` を返す。
    pub fn yield_cont_take(&mut self) -> (r: Result<Word, HeapError>)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            r is Err <==> M::CHECKED && !old(self).is_yielding(),
            r is Err ==> r == Err::<Word, HeapError>(HeapError::YieldMisuse) && *final(self)
                == *old(self),
            r matches Ok(k) ==> {
                &&& k == old(self).yield_spec().2
                &&& final(self).yield_spec().0 == old(self).yield_spec().0
                &&& final(self).yield_spec().1 == old(self).yield_spec().1
                &&& final(self).cont_empty()
                &&& final(self).yield_spec().3 == old(self).yield_spec().3
            },
            final(self).heap_spec() == old(self).heap_spec(),
            final(self).evv_spec() == old(self).evv_spec(),
    {
        if M::CHECKED && self.yielding.target.bits() == 1 {
            return Err(HeapError::YieldMisuse);
        }
        let k = self.yielding.cont;
        self.yielding.cont = Word::UNIT;
        proof {
            lemma_unit_bits();
            if !M::CHECKED {
                let s0 = *old(self);
                lemma_moved_refl(self.heap);
                assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies s0.need(
                    seq![],
                    b,
                ) + 0 >= self.need(seq![], b) by {
                    lemma_points_in(seq![], b);
                }
                self.lemma_holds_moved(s0, seq![], seq![], |b: usize| 0int, Set::empty());
            }
        }
        Ok(k)
    }

    /// κ を入れる。`k` は消費する。
    ///
    /// # Safety
    /// `k` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がその参照を1つ持つ。yield 中で、κ の欄が空で
    /// ある (検査モードは `YieldMisuse` を返す)。
    pub unsafe fn yield_cont_set(&mut self, k: Word) -> (r: Result<(), HeapError>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(k),
            !M::CHECKED ==> old(self).is_yielding() && old(self).cont_empty() && old(self).holds(k),
        ensures
            final(self).wf(),
            r is Err <==> M::CHECKED && !(old(self).is_yielding() && old(self).cont_empty()),
            r is Err ==> r == Err::<(), HeapError>(HeapError::YieldMisuse) && *final(self) == *old(
                self,
            ),
            r is Ok ==> final(self).yield_spec() == (
                old(self).yield_spec().0,
                old(self).yield_spec().1,
                k,
                old(self).yield_spec().3,
            ),
            final(self).heap_spec() == old(self).heap_spec(),
            final(self).evv_spec() == old(self).evv_spec(),
    {
        if M::CHECKED && (self.yielding.target.bits() == 1 || self.yielding.cont.bits() != 1) {
            return Err(HeapError::YieldMisuse);
        }
        self.yielding.cont = k;
        if M::CHECKED {
            self.stats.yield_frames = self.stats.yield_frames.wrapping_add(1);
        }
        proof {
            lemma_unit_bits();
            if !M::CHECKED {
                let s0 = *old(self);
                let ws = seq![k];
                assert(held_word(s0.heap, ws[0]));
                lemma_moved_refl(self.heap);
                assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies s0.need(ws, b)
                    + 0 >= self.need(seq![], b) by {
                    lemma_points_in(seq![], b);
                    lemma_points_in(ws, b);
                }
                self.lemma_holds_moved(s0, ws, seq![], |b: usize| 0int, Set::empty());
            }
        }
        Ok(())
    }

    /// yield を終える。節の本体を返し、宛先の参照を手放し、yield の状態をなしに戻す。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。yield 中で、κ の欄が空である (検査モードは `YieldMisuse` を返す)。検査モードで
    /// 宛先を手放す連鎖が誤りを見つけたら、`Err` を返した後は `Ctx` を落とすことしかできない
    /// (docs/spec/object-model.md の「検査モードの約束」)。
    pub unsafe fn yield_finish(&mut self) -> (r: Result<Word, HeapError>)
        requires
            old(self).wf(),
            !M::CHECKED ==> old(self).is_yielding() && old(self).cont_empty(),
        ensures
            final(self).wf(),
            !M::CHECKED ==> r is Ok,
            M::CHECKED && !(old(self).is_yielding() && old(self).cont_empty()) ==> r == Err::<
                Word,
                HeapError,
            >(HeapError::YieldMisuse) && *final(self) == *old(self),
            old(self).is_yielding() && old(self).cont_empty() ==> {
                &&& !final(self).is_yielding()
                &&& final(self).evv_spec() == old(self).evv_spec()
                &&& final(self).heap_spec().kept_live(old(self).heap_spec())
                &&& old(self).held_ok() ==> r is Ok || chain_err(r)
                &&& old(self).held_ok() && r is Ok ==> r == Ok::<Word, HeapError>(
                    old(self).yield_spec().1,
                ) && final(self).heap_spec().evolved(
                    old(self).heap_spec(),
                    old(self).yield_spec().0,
                    -1,
                )
            },
    {
        if M::CHECKED && (self.yielding.target.bits() == 1 || self.yielding.cont.bits() != 1) {
            return Err(HeapError::YieldMisuse);
        }
        let target = self.yielding.target;
        let clause = self.yielding.clause;
        self.yielding = YieldState::none();
        proof {
            lemma_unit_bits();
            if old(self).held_ok() {
                (*old(self)).lemma_held_dup_ok(target, seq![]);
            }
        }
        let r = unsafe { self.heap.decref(target) };
        proof {
            let s0 = *old(self);
            if M::CHECKED {
                self.lemma_words_kept(s0);
            }
            if s0.held_ok() && r is Ok {
                lemma_decref_moved(self.heap, s0.heap, target);
                let d = |b: usize| -points(target, b);
                assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies s0.need(
                    seq![],
                    b,
                ) + d(b) >= self.need(seq![], b) by {
                    lemma_points_in(seq![], b);
                }
                self.lemma_holds_moved(s0, seq![], seq![], d, Set::empty());
                self.lemma_evv_kept(s0, d, Set::empty());
            }
        }
        match r {
            Ok(()) => Ok(clause),
            Err(e) => Err(e),
        }
    }

    /// 今の `evv` を `dup` して返す。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている (`Ctx` の持つ語は生きている)。
    pub unsafe fn evv_get(&mut self) -> (r: Result<Word, HeapError>)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            !M::CHECKED ==> r is Ok,
            r matches Ok(w) ==> w == old(self).evv_spec(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            old(self).held_ok() ==> r is Ok && final(self).heap_spec().evolved(
                old(self).heap_spec(),
                old(self).evv_spec(),
                1,
            ) && final(self).heap_spec().bumped(old(self).heap_spec(), old(self).evv_spec()),
    {
        let w = self.evv;
        proof {
            if old(self).held_ok() {
                self.lemma_held_dup_ok(w, seq![]);
            }
        }
        let r = unsafe { self.heap.dup(w) };
        proof {
            let s0 = *old(self);
            if M::CHECKED {
                self.lemma_words_kept(s0);
            }
            if s0.held_ok() {
                lemma_dup_moved(self.heap, s0.heap, w);
                let d = |b: usize| points(w, b);
                assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies s0.need(
                    seq![],
                    b,
                ) + d(b) >= self.need(seq![], b) by {
                    lemma_points_in(seq![], b);
                }
                self.lemma_holds_moved(s0, seq![], seq![], d, Set::empty());
                self.lemma_evv_kept(s0, d, Set::empty());
            }
        }
        match r {
            Ok(()) => Ok(w),
            Err(e) => Err(e),
        }
    }

    /// `evv` を `l` に替え、元の `evv` を手放す。`l` は消費する。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`l` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がその参照を
    /// 1つ持つ。速いモードでは、`l` は `evv` のリストとして正しい。検査モードで元の `evv` を手放す連鎖が誤りを
    /// 見つけたら、`Err` を返した後は `Ctx` を落とすことしかできない (docs/spec/object-model.md の「検査モードの約束」)。
    pub unsafe fn evv_set(&mut self, l: Word) -> (r: Result<(), HeapError>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(l),
            !M::CHECKED ==> old(self).evv_wf(l) && old(self).holds(l),
        ensures
            final(self).wf(),
            !M::CHECKED ==> r is Ok,
            final(self).evv_spec() == l,
            final(self).yield_spec() == old(self).yield_spec(),
            final(self).heap_spec().kept_live(old(self).heap_spec()),
            old(self).held_ok() && old(self).evv_wf(l) && old(self).holds(l) ==> r is Ok
                || chain_err(r),
            old(self).held_ok() && old(self).evv_wf(l) && old(self).holds(l) && r is Ok
                ==> final(self).heap_spec().evolved(
                old(self).heap_spec(),
                old(self).evv_spec(),
                -1,
            ),
    {
        let old_evv = self.evv;
        self.evv = l;
        proof {
            let s0 = *old(self);
            if s0.held_ok() && s0.evv_wf(l) && s0.holds(l) {
                s0.lemma_held_dup_ok(old_evv, seq![l]);
            }
        }
        let r = unsafe { self.heap.decref(old_evv) };
        proof {
            let s0 = *old(self);
            if M::CHECKED {
                assert(s0.heap.word_in(l));
                self.lemma_words_kept(s0);
            }
            if s0.held_ok() && s0.evv_wf(l) && s0.holds(l) && r is Ok {
                lemma_decref_moved(self.heap, s0.heap, old_evv);
                let d = |b: usize| -points(old_evv, b);
                let ws = seq![l];
                assert(held_word(s0.heap, ws[0]));
                assert forall|b: usize| #[trigger] self.need(seq![], b) > 0 implies s0.need(ws, b)
                    + d(b) >= self.need(seq![], b) by {
                    lemma_points_in(seq![], b);
                    lemma_points_in(ws, b);
                }
                self.lemma_holds_moved(s0, ws, seq![], d, Set::empty());
                lemma_listed_kept(self.heap, s0.heap, d, Set::empty(), l, s0.max_evv);
            }
        }
        r
    }

    /// エフェクト `effect` の evidence を作る。入口の表は欄が `n_ops` 個で、どれも `()` である。tag はエフェクトの
    /// 番号 + 1 にする (docs/spec/object-model.md の「evidence の3つの物体」)。`state` と `outer` は消費する。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`state` と `outer` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側が
    /// その参照を1つずつ持つ。速いモードでは、`effect` は 65535 より小さく (検査モードは `BadEvidence` を返す)、
    /// `outer` は `evv` のリストとして正しい。
    pub unsafe fn ev_new(&mut self, effect: u16, n_ops: usize, state: Word, outer: Word) -> (r:
        Result<Word, HeapError>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(state),
            old(self).heap_spec().word_in(outer),
            !M::CHECKED ==> effect < u16::MAX && old(self).evv_wf(outer) && old(self).holds_args(
                seq![state, outer],
            ),
        ensures
            final(self).wf(),
            r is Err ==> *final(self) == *old(self),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            !M::CHECKED ==> r is Ok,
            M::CHECKED && effect == u16::MAX ==> *final(self) == *old(self) && r == Err::<
                Word,
                HeapError,
            >(HeapError::BadEvidence),
            old(self).held_ok() && effect < u16::MAX && old(self).evv_wf(outer) && old(
                self,
            ).holds_args(seq![state, outer]) ==> (r matches Ok(e) && {
                &&& final(self).held_ok()
                &&& final(self).holds(e)
                &&& final(self).ev_shape(e)
                &&& final(self).ev_tag(e) == effect + 1
                &&& final(self).ev_state(e) == state
                &&& final(self).ev_outer(e) == outer
                &&& final(self).ev_ops(e) == n_ops
                &&& forall|i: int|
                    0 <= i < n_ops ==> (#[trigger] final(self).ev_entry(e, i)).bits_spec() == 1
                &&& final(self).evolved_by(
                    *old(self),
                    |b: usize| -points(state, b) - points(outer, b),
                    Set::empty(),
                )
            }),
    {
        if M::CHECKED && effect == u16::MAX {
            return Err(HeapError::BadEvidence);
        }
        let ghost s0 = *self;
        let ghost h0 = self.heap;
        let ghost c = s0.held_ok() && s0.evv_wf(outer) && s0.holds_args(seq![state, outer]);
        let ghost z = |b: usize| 0int;
        let ghost none = Set::<usize>::empty();
        let table = self.heap.alloc_data(0, n_ops, 0, false);
        let ghost h1 = self.heap;
        let ev = self.heap.alloc_data(effect + 1, 3, 0, false);
        let ghost h2 = self.heap;
        let ghost x = none.insert(ev.bits_spec());
        proof {
            lemma_words_trans(h2, h1, h0);
            lemma_unextended(h2, ev);
            assert(h2.scanned_at(ev, 0));
            if c {
                lemma_alloc_moved(h1, h0, table);
                lemma_alloc_moved(h2, h1, ev);
                lemma_moved_chain(h2, h1, h0, z, z, z, none, none, none);
                // 先に作った入口の表は、外の参照を1つ持ったまま生き残る
                assert(live(h1, table.bits_spec()));
                lemma_arg_moved(h2, h1, z, none, table);
                assert(h2.scanned_word(ev, 0).bits_spec() == 1);
            }
        }
        (unsafe { self.heap.set_field(ev, 0, table) })?;
        let ghost h3 = self.heap;
        proof {
            if M::CHECKED {
                assert(h2.scanned_at(ev, 1) && h2.scanned_at(ev, 2));
                lemma_set_passes_kept(h3, h2, ev, 0, table, 1);
                lemma_set_passes_kept(h3, h2, ev, 0, table, 2);
            }
            if c {
                lemma_set_unit_moved(h3, h2, ev, 0, table);
                let d = |b: usize| -points(table, b);
                lemma_moved_chain(h3, h2, h0, z, d, z, none, x, none);
                s0.lemma_arg_counted(seq![state, outer], 0);
                lemma_points_in(seq![state, outer], state.bits_spec());
                lemma_arg_moved(h3, h0, z, none, state);
            }
        }
        (unsafe { self.heap.set_field(ev, 1, state) })?;
        let ghost h4 = self.heap;
        let ghost d4 = |b: usize| -points(state, b);
        proof {
            if M::CHECKED {
                lemma_set_passes_kept(h4, h3, ev, 1, state, 2);
            }
            if c {
                lemma_unextended(h3, ev);
                assert(h3.word_at(ev, 1) == h2.word_at(ev, 1));
                assert(h2.scanned_word(ev, 1).bits_spec() == 1);
                lemma_set_unit_moved(h4, h3, ev, 1, state);
                lemma_moved_chain(h4, h3, h0, z, d4, d4, none, x, none);
                s0.lemma_arg_counted(seq![state, outer], 1);
                lemma_points_in(seq![state, outer], outer.bits_spec());
                lemma_arg_moved(h4, h0, d4, none, outer);
            }
        }
        (unsafe { self.heap.set_field(ev, 2, outer) })?;
        proof {
            lemma_words_trans(h3, h2, h0);
            lemma_words_trans(self.heap, h4, h3);
            lemma_words_trans(self.heap, h3, h0);
            self.lemma_words_kept(s0);
            if c {
                self.lemma_ev_new_ok(s0, h1, h2, h3, h4, table, ev, effect, n_ops, state, outer);
            }
        }
        Ok(ev)
    }

    /// `l` の先頭に evidence `e` の節点を足したリストを返す。節点の tag は `e` の tag と同じにする
    /// (docs/spec/object-model.md の「evidence の3つの物体」)。`l` と `e` は消費する。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`l` と `e` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がその
    /// 参照を1つずつ持つ。速いモードでは、`l` は長さが上限より短い `evv` のリストで、`e` は evidence である。検査モードは、
    /// 確保の前に `e` の形と `l` の長さを確かめ、違えば `BadEvidence` を返す。
    pub unsafe fn evv_push(&mut self, l: Word, e: Word) -> (r: Result<Word, HeapError>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(l),
            old(self).heap_spec().word_in(e),
            !M::CHECKED ==> old(self).evv_wf(l) && old(self).evv_seq(l).len() < old(
                self,
            ).evv_limit() && old(self).ev_shape(e) && old(self).holds_args(seq![l, e]),
        ensures
            final(self).wf(),
            r is Err ==> *final(self) == *old(self),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).evv_wf(l) && old(self).evv_seq(l).len() < old(
                self,
            ).evv_limit() && old(self).ev_shape(e) && old(self).holds_args(seq![l, e]) ==> (
            r matches Ok(n) && {
                &&& final(self).held_ok()
                &&& final(self).holds(n)
                &&& final(self).evv_wf(n)
                &&& final(self).evv_seq(n) == seq![(old(self).ev_tag(e), e)] + old(self).evv_seq(l)
                &&& final(self).evolved_by(
                    *old(self),
                    |b: usize| -points(l, b) - points(e, b),
                    Set::empty(),
                )
            }),
    {
        let ghost s0 = *self;
        let ghost h0 = self.heap;
        let ghost c = s0.held_ok() && s0.evv_wf(l) && s0.evv_seq(l).len() < s0.max_evv
            && s0.ev_shape(e) && s0.holds_args(seq![l, e]);
        if M::CHECKED {
            self.checked_ev(e)?;
            self.checked_room(l)?;
        }
        proof {
            if c {
                lemma_unextended(h0, e);
            }
        }
        let m = (unsafe { self.heap.meta(e) })?;
        proof {
            m.lemma_fields();
        }
        let node = self.heap.alloc_data(m.tag(), 2, 0, false);
        let ghost h1 = self.heap;
        let ghost z = |b: usize| 0int;
        let ghost none = Set::<usize>::empty();
        let ghost x = none.insert(node.bits_spec());
        let ghost ws = seq![l, e];
        proof {
            lemma_words_trans(h1, h0, h0);
            lemma_unextended(h1, node);
            assert(h1.scanned_at(node, 0));
            if c {
                lemma_alloc_moved(h1, h0, node);
                s0.lemma_arg_counted(ws, 1);
                lemma_points_in(ws, e.bits_spec());
                lemma_arg_moved(h1, h0, z, none, e);
                assert(h1.scanned_word(node, 0).bits_spec() == 1);
                assert(h1.scanned_word(node, 1).bits_spec() == 1);
            }
        }
        (unsafe { self.heap.set_field(node, 0, e) })?;
        let ghost h2 = self.heap;
        let ghost d2 = |b: usize| -points(e, b);
        proof {
            if M::CHECKED {
                assert(h1.scanned_at(node, 1));
                lemma_set_passes_kept(h2, h1, node, 0, e, 1);
            }
            if c {
                lemma_set_unit_moved(h2, h1, node, 0, e);
                lemma_moved_chain(h2, h1, h0, z, d2, d2, none, x, none);
                s0.lemma_arg_counted(ws, 0);
                lemma_points_in(ws, l.bits_spec());
                lemma_arg_moved(h2, h0, d2, none, l);
                lemma_unextended(h2, node);
                assert(h2.word_at(node, 1) == h1.word_at(node, 1));
            }
        }
        (unsafe { self.heap.set_field(node, 1, l) })?;
        proof {
            lemma_words_trans(h2, h1, h0);
            lemma_words_trans(self.heap, h2, h0);
            self.lemma_words_kept(s0);
            if c {
                self.lemma_evv_push_ok(s0, h1, h2, node, l, e);
            }
        }
        Ok(node)
    }

    /// evidence の外側の `evv` を `dup` して返す。外側の `evv` がリストとして正しいことは EP の不変条件で、この操作は
    /// 語そのものだけを約束する。`ev` は借りる。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`ev` は即値か、このヒープか付けた塊の生きている物体を指す。速いモードでは、`ev`
    /// は evidence である (検査モードは形を確かめ、違えば `BadEvidence` を返す)。
    pub unsafe fn ev_evv(&mut self, ev: Word) -> (r: Result<Word, HeapError>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(ev),
            !M::CHECKED ==> old(self).ev_shape(ev),
        ensures
            final(self).wf(),
            r is Err ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).ev_shape(ev) ==> (r matches Ok(w) && {
                &&& w == old(self).ev_outer(ev)
                &&& final(self).held_ok()
                &&& final(self).holds(w)
                &&& final(self).ev_shape(ev)
                &&& final(self).evolved_by(*old(self), |b: usize| points(w, b), Set::empty())
            }),
    {
        let ghost s0 = *self;
        if M::CHECKED {
            self.checked_ev(ev)?;
        }
        proof {
            if s0.held_ok() && s0.ev_shape(ev) {
                lemma_unextended(s0.heap, ev);
            }
        }
        let w = (unsafe { self.heap.field(ev, 2) })?;
        proof {
            if s0.held_ok() && s0.ev_shape(ev) {
                s0.heap.lemma_readable_scanned(ev, 2);
            }
        }
        (unsafe { self.heap.dup(w) })?;
        proof {
            if M::CHECKED {
                self.lemma_words_kept(s0);
            }
            if s0.held_ok() && s0.ev_shape(ev) {
                self.lemma_dupped_ok(s0, w, ev);
            }
        }
        Ok(w)
    }

    /// 入口の表の欄 `op` を `dup` して返す。`ev` は借りる。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`ev` は即値か、このヒープか付けた塊の生きている物体を指す。速いモードでは、`ev`
    /// は evidence で、`op` は入口の数より小さい (検査モードは確かめ、違えば `BadEvidence` を返す)。
    pub unsafe fn ev_clause(&mut self, ev: Word, op: usize) -> (r: Result<Word, HeapError>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(ev),
            !M::CHECKED ==> old(self).ev_shape(ev) && op < old(self).ev_ops(ev),
        ensures
            final(self).wf(),
            r is Err ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).ev_shape(ev) && op < old(self).ev_ops(ev) ==> (
            r matches Ok(w) && {
                &&& w == old(self).ev_entry(ev, op as int)
                &&& final(self).held_ok()
                &&& final(self).holds(w)
                &&& final(self).ev_shape(ev)
                &&& final(self).evolved_by(*old(self), |b: usize| points(w, b), Set::empty())
            }),
            M::CHECKED && old(self).held_ok() && old(self).ev_shape(ev) && op >= old(self).ev_ops(
                ev,
            ) ==> r == Err::<Word, HeapError>(HeapError::BadEvidence),
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.ev_shape(ev);
        proof {
            if c {
                lemma_unextended(s0.heap, ev);
                s0.heap.lemma_readable_scanned(ev, 0);
            }
        }
        let t = if M::CHECKED {
            let t = self.checked_ev(ev)?;
            let n = (unsafe { self.heap.scanned_len(t) })?;
            if op >= n {
                return Err(HeapError::BadEvidence);
            }
            t
        } else {
            (unsafe { self.heap.field(ev, 0) })?
        };
        let w = (unsafe { self.heap.scanned(t, op) })?;
        proof {
            if c && op < s0.ev_ops(ev) {
                s0.heap.lemma_readable_scanned(t, op as int);
            }
        }
        (unsafe { self.heap.dup(w) })?;
        proof {
            if M::CHECKED {
                self.lemma_words_kept(s0);
            }
            if c && op < s0.ev_ops(ev) {
                self.lemma_dupped_ok(s0, w, ev);
            }
        }
        Ok(w)
    }

    /// evidence の状態を返し、欄を `()` にする。状態の参照は呼ぶ側に移る。`ev` は借りる。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`ev` は即値か、このヒープか付けた塊の生きている物体を指す。速いモードでは、`ev`
    /// は evidence で、ほかのスレッドが読めない局所の物体である (検査モードは形を確かめて `BadEvidence` を、書けない
    /// 物体には `set_field` の `NotWritable` を返す)。
    pub unsafe fn ev_take_state(&mut self, ev: Word) -> (r: Result<Word, HeapError>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(ev),
            !M::CHECKED ==> old(self).ev_shape(ev) && old(self).heap_spec().rc_value(ev) > 0,
        ensures
            final(self).wf(),
            r is Err ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).ev_shape(ev) && old(self).heap_spec().rc_value(ev) > 0
                ==> (r matches Ok(s) && {
                &&& s == old(self).ev_state(ev)
                &&& final(self).ev_state(ev).bits_spec() == 1
                &&& final(self).ev_table(ev) == old(self).ev_table(ev)
                &&& final(self).ev_outer(ev) == old(self).ev_outer(ev)
                &&& final(self).held_ok()
                &&& final(self).holds(s)
                &&& final(self).ev_shape(ev)
                &&& final(self).evolved_by(
                    *old(self),
                    |b: usize| points(s, b),
                    Set::empty().insert(ev.bits_spec()),
                )
            }),
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.ev_shape(ev) && s0.heap.rc_value(ev) > 0;
        if M::CHECKED {
            self.checked_ev(ev)?;
        }
        self.heap.settle();
        let ghost h1 = self.heap;
        proof {
            lemma_unextended(h1, ev);
        }
        let s = (unsafe { self.heap.field(ev, 1) })?;
        let unit = Word::UNIT;
        proof {
            lemma_unit_bits();
            if c {
                self.lemma_settled_ev(s0, ev);
            }
        }
        (unsafe { self.heap.set_field(ev, 1, unit) })?;
        proof {
            lemma_words_trans(self.heap, h1, s0.heap);
            self.lemma_words_kept(s0);
            if c {
                self.lemma_take_state_ok(s0, h1, ev, s, unit);
            }
        }
        Ok(s)
    }

    /// evidence の状態の欄を `s` にし、元の状態を手放す。`s` は消費し、`ev` は借りる。元の状態を手放す連鎖が `ev`
    /// 自身に届いても (docs/spec/lir.md の「evidence」の、状態の欄を通る輪)、呼ぶ側が `ev` を持つので `ev` は空かない。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`ev` と `s` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側が `s`
    /// の参照と、それと別に `ev` の参照を1つずつ持つ。速いモードでは、`ev` は局所の evidence である (検査モードは形を
    /// 確かめて `BadEvidence` を、書けない物体には `NotWritable` を返す)。検査モードで元の状態を手放す連鎖が誤りを
    /// 見つけたら、`Err` を返した後は `Ctx` を落とすことしかできない (docs/spec/object-model.md の「検査モードの約束」)。
    pub unsafe fn ev_set_state(&mut self, ev: Word, s: Word) -> (r: Result<(), HeapError>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(ev),
            old(self).heap_spec().word_in(s),
            !M::CHECKED ==> old(self).ev_shape(ev) && old(self).heap_spec().rc_value(ev) > 0 && old(
                self,
            ).holds_args(seq![ev, s]),
        ensures
            final(self).wf(),
            r is Err ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap() || exists|mid: Heap<M>|
                #![trigger mid.field_set(old(self).heap_spec().settled_heap(), ev, 1, s)]
                final(self).heap_spec().kept_live(mid) && mid.field_set(
                    old(self).heap_spec().settled_heap(),
                    ev,
                    1,
                    s,
                ),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).ev_shape(ev) && old(self).heap_spec().rc_value(ev) > 0
                && old(self).holds_args(seq![ev, s]) ==> r is Ok || chain_err(r),
            old(self).held_ok() && old(self).ev_shape(ev) && old(self).heap_spec().rc_value(ev) > 0
                && old(self).holds_args(seq![ev, s]) && r is Ok ==> {
                &&& final(self).ev_state(ev) == s
                &&& final(self).ev_table(ev) == old(self).ev_table(ev)
                &&& final(self).ev_outer(ev) == old(self).ev_outer(ev)
                &&& final(self).held_ok()
                &&& final(self).holds(ev)
                &&& final(self).ev_shape(ev)
                &&& final(self).evolved_by(
                    *old(self),
                    |b: usize| -points(s, b),
                    Set::empty().insert(ev.bits_spec()),
                )
            },
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.ev_shape(ev) && s0.heap.rc_value(ev) > 0 && s0.holds_args(
            seq![ev, s],
        );
        if M::CHECKED {
            self.checked_ev(ev)?;
        }
        self.heap.settle();
        let ghost h1 = self.heap;
        proof {
            lemma_unextended(h1, ev);
        }
        let old_state = (unsafe { self.heap.field(ev, 1) })?;
        proof {
            if c {
                self.lemma_settled_ev(s0, ev);
                s0.lemma_arg_counted(seq![ev, s], 1);
                lemma_points_in(seq![ev, s], s.bits_spec());
                lemma_settle_moved(h1, s0.heap);
                lemma_arg_moved(h1, s0.heap, |b: usize| 0int, Set::empty(), s);
            }
        }
        (unsafe { self.heap.set_field(ev, 1, s) })?;
        let ghost h2 = self.heap;
        proof {
            if c {
                self.lemma_set_state_mid(s0, h1, ev, s, old_state);
            }
        }
        let r = unsafe { self.heap.decref(old_state) };
        proof {
            if M::CHECKED {
                lemma_words_trans(h2, h1, s0.heap);
                lemma_words_trans(self.heap, h2, s0.heap);
                self.lemma_words_kept(s0);
            }
            if c && r is Ok {
                self.lemma_set_state_ok(s0, h1, h2, ev, s, old_state);
            }
        }
        r
    }

    /// 入口の表の欄 `op` に節の本体 `c` を入れる。`c` は消費し、`ev` は借りる。欄が 255 個以上の表は、先頭に数の語を
    /// 持つので、書く位置を1つずらす (docs/spec/object-model.md の「物体の形」)。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`ev` と `c` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側が `c`
    /// の参照を1つ持つ。`c` は `lin` ビットを持たない。速いモードでは、`ev` は evidence で、`op` は入口の数より小さく、
    /// 欄はまだ `()` で、入口の表はほかのスレッドが読めない局所の物体である (検査モードは確かめ、違えば `BadEvidence`
    /// を、書けない表には `NotWritable` を返す)。
    pub unsafe fn ev_set_clause(&mut self, ev: Word, op: usize, c: Word) -> (r: Result<
        (),
        HeapError,
    >)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(ev),
            old(self).heap_spec().word_in(c),
            !M::CHECKED ==> {
                &&& old(self).ev_shape(ev)
                &&& op < old(self).ev_ops(ev)
                &&& old(self).ev_entry(ev, op as int).bits_spec() == 1
                &&& old(self).heap_spec().rc_value(old(self).ev_table(ev)) > 0
                &&& old(self).holds(c)
                &&& c.is_ptr_spec() ==> !old(self).heap_spec().lin_of(c)
            },
        ensures
            final(self).wf(),
            r is Err ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            !M::CHECKED ==> r is Ok,
            ({
                &&& old(self).held_ok()
                &&& old(self).ev_shape(ev)
                &&& op < old(self).ev_ops(ev)
                &&& old(self).ev_entry(ev, op as int).bits_spec() == 1
                &&& old(self).heap_spec().rc_value(old(self).ev_table(ev)) > 0
                &&& old(self).holds(c)
                &&& c.is_ptr_spec() ==> !old(self).heap_spec().lin_of(c)
            }) ==> r is Ok && {
                &&& final(self).ev_entry(ev, op as int) == c
                &&& forall|i: int|
                    i != op ==> #[trigger] final(self).ev_entry(ev, i) == old(self).ev_entry(ev, i)
                &&& final(self).ev_ops(ev) == old(self).ev_ops(ev)
                &&& final(self).held_ok()
                &&& final(self).ev_shape(ev)
                &&& final(self).evolved_by(
                    *old(self),
                    |b: usize| -points(c, b),
                    Set::empty().insert(old(self).ev_table(ev).bits_spec()),
                )
            },
            M::CHECKED && old(self).held_ok() && old(self).ev_shape(ev) && (op >= old(self).ev_ops(
                ev,
            ) || old(self).ev_entry(ev, op as int).bits_spec() != 1) ==> r == Err::<(), HeapError>(
                HeapError::BadEvidence,
            ),
    {
        let ghost s0 = *self;
        let ghost k = s0.held_ok() && s0.ev_shape(ev) && op < s0.ev_ops(ev) && s0.ev_entry(
            ev,
            op as int,
        ).bits_spec() == 1 && s0.heap.rc_value(s0.ev_table(ev)) > 0 && s0.holds(c) && (
        c.is_ptr_spec() ==> !s0.heap.lin_of(c));
        proof {
            if s0.held_ok() && s0.ev_shape(ev) {
                lemma_unextended(s0.heap, ev);
                s0.heap.lemma_readable_scanned(ev, 0);
            }
        }
        let t = if M::CHECKED {
            let t = self.checked_ev(ev)?;
            let n = (unsafe { self.heap.scanned_len(t) })?;
            if op >= n {
                return Err(HeapError::BadEvidence);
            }
            let cur = (unsafe { self.heap.scanned(t, op) })?;
            if cur.bits() != 1 {
                return Err(HeapError::BadEvidence);
            }
            t
        } else {
            (unsafe { self.heap.field(ev, 0) })?
        };
        let tm = (unsafe { self.heap.meta(t) })?;
        proof {
            tm.lemma_fields();
        }
        let i = if tm.scan() == Meta::EXTENDED_SCAN {
            op + 1
        } else {
            op
        };
        self.heap.settle();
        let ghost h1 = self.heap;
        proof {
            if k {
                self.lemma_settled_ev(s0, ev);
                s0.lemma_arg_counted(seq![c], 0);
                lemma_points_in(seq![c], c.bits_spec());
                lemma_settle_moved(h1, s0.heap);
                lemma_arg_moved(h1, s0.heap, |b: usize| 0int, Set::empty(), c);
                self.lemma_entry_slot(s0, ev, op as int, i as int);
            }
        }
        (unsafe { self.heap.set_field(t, i, c) })?;
        proof {
            lemma_words_trans(self.heap, h1, s0.heap);
            self.lemma_words_kept(s0);
            if k {
                self.lemma_set_clause_ok(s0, h1, ev, op as int, i as int, c);
            }
        }
        Ok(())
    }

    /// `ev` と同じ入口の表を指し、状態を `state` に、外側の `evv` を今の `evv` にした新しい evidence を返す。tag は
    /// `ev` と同じである。`state` は消費し、`ev` は借りる。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`ev` と `state` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側が
    /// `state` の参照を1つ持つ。速いモードでは、`ev` は evidence である (検査モードは形を確かめ、違えば
    /// `BadEvidence` を返す)。
    pub unsafe fn ev_renew(&mut self, ev: Word, state: Word) -> (r: Result<Word, HeapError>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(ev),
            old(self).heap_spec().word_in(state),
            !M::CHECKED ==> old(self).ev_shape(ev) && old(self).holds(state),
        ensures
            final(self).wf(),
            r is Err ==> *final(self) == *old(self),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).ev_shape(ev) && old(self).holds(state) ==> (
            r matches Ok(e) && {
                &&& final(self).held_ok()
                &&& final(self).holds(e)
                &&& final(self).ev_shape(e)
                &&& final(self).ev_shape(ev)
                &&& final(self).ev_tag(e) == old(self).ev_tag(ev)
                &&& final(self).ev_table(e) == old(self).ev_table(ev)
                &&& final(self).ev_state(e) == state
                &&& final(self).ev_outer(e) == old(self).evv_spec()
                &&& final(self).evolved_by(*old(self), |b: usize| -points(state, b), Set::empty())
            }),
    {
        let ghost s0 = *self;
        let ghost h0 = self.heap;
        let ghost c = s0.held_ok() && s0.ev_shape(ev) && s0.holds(state);
        let ghost z = |b: usize| 0int;
        let ghost none = Set::<usize>::empty();
        proof {
            if c {
                lemma_unextended(h0, ev);
                h0.lemma_readable_scanned(ev, 0);
            }
        }
        let t = if M::CHECKED {
            self.checked_ev(ev)?
        } else {
            (unsafe { self.heap.field(ev, 0) })?
        };
        let m = (unsafe { self.heap.meta(ev) })?;
        proof {
            m.lemma_fields();
        }
        let l = self.evv;
        proof {
            if c {
                s0.lemma_held_dup_ok(l, seq![state]);
            }
        }
        // 2つの `dup` を確かめてから、どちらかを行う。後の `dup` が誤れば、先の `dup` の後にヒープを変えたまま
        // `Err` を返すことになるためである
        if M::CHECKED {
            self.heap.dup_check(t)?;
            self.heap.dup_check(l)?;
        }
        (unsafe { self.heap.dup(t) })?;
        let ghost h1 = self.heap;
        let ghost d1 = |b: usize| points(t, b);
        proof {
            if c {
                lemma_dup_moved(h1, h0, t);
                lemma_listed_head(h0, l, s0.max_evv);
                lemma_readable_kept(h1, h0, l);
                lemma_word_kept(h1, h0, d1, none, l);
            }
        }
        (unsafe { self.heap.dup(l) })?;
        let ghost h2 = self.heap;
        let ghost d2 = |b: usize| points(t, b) + points(l, b);
        let e = self.heap.alloc_data(m.tag(), 3, 0, false);
        let ghost h3 = self.heap;
        let ghost x = none.insert(e.bits_spec());
        proof {
            lemma_unextended(h3, e);
            assert(h3.scanned_at(e, 0));
            if c {
                lemma_dup_moved(h2, h1, l);
                lemma_moved_chain(h2, h1, h0, d1, |b: usize| points(l, b), d2, none, none, none);
                lemma_alloc_moved(h3, h2, e);
                lemma_moved_chain(h3, h2, h0, d2, z, d2, none, none, none);
                lemma_ext_nonneg_word(h0, t);
                lemma_arg_moved(h3, h0, d2, none, t);
                assert(h3.scanned_word(e, 0).bits_spec() == 1);
            }
        }
        (unsafe { self.heap.set_field(e, 0, t) })?;
        let ghost h4 = self.heap;
        let ghost d4 = |b: usize| points(l, b);
        proof {
            if M::CHECKED {
                assert(h3.scanned_at(e, 1) && h3.scanned_at(e, 2));
                lemma_set_passes_kept(h4, h3, e, 0, t, 1);
                lemma_set_passes_kept(h4, h3, e, 0, t, 2);
            }
            if c {
                lemma_set_unit_moved(h4, h3, e, 0, t);
                lemma_moved_chain(h4, h3, h0, d2, |b: usize| -points(t, b), d4, none, x, none);
                s0.lemma_arg_counted(seq![state], 0);
                lemma_points_in(seq![state], state.bits_spec());
                lemma_arg_moved(h4, h0, d4, none, state);
                lemma_unextended(h4, e);
                assert(h4.word_at(e, 1) == h3.word_at(e, 1));
                assert(h3.scanned_word(e, 1).bits_spec() == 1);
            }
        }
        (unsafe { self.heap.set_field(e, 1, state) })?;
        let ghost h5 = self.heap;
        let ghost d5 = |b: usize| points(l, b) - points(state, b);
        proof {
            if M::CHECKED {
                lemma_set_passes_kept(h5, h4, e, 1, state, 2);
            }
            if c {
                lemma_set_unit_moved(h5, h4, e, 1, state);
                lemma_moved_chain(h5, h4, h0, d4, |b: usize| -points(state, b), d5, none, x, none);
                s0.lemma_evv_counted(seq![state]);
                lemma_points_in(seq![state], l.bits_spec());
                lemma_arg_moved(h5, h0, d5, none, l);
                lemma_unextended(h5, e);
                assert(h5.word_at(e, 2) == h3.word_at(e, 2));
                assert(h3.scanned_word(e, 2).bits_spec() == 1);
            }
        }
        (unsafe { self.heap.set_field(e, 2, l) })?;
        proof {
            if M::CHECKED {
                lemma_words_trans(h2, h1, h0);
                lemma_words_trans(h3, h2, h0);
                lemma_words_trans(h4, h3, h0);
                lemma_words_trans(h5, h4, h0);
                lemma_words_trans(self.heap, h5, h0);
                self.lemma_words_kept(s0);
            }
            if c {
                self.lemma_ev_renew_ok(s0, h1, h2, h3, h4, h5, ev, t, l, e, state);
            }
        }
        Ok(e)
    }

    /// 今の `evv` を先頭からたどり、エフェクト `effect` の最初の evidence を `dup` して返す。速いモードの1歩は、
    /// 節点の `meta` を作っておいた値と1回比べ、次の節点を読むだけである (docs/spec/lir.md の「evidence を探す」)。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。速いモードでは、`evv` にエフェクト `effect` の evidence がある。検査モードは、
    /// なければ `NoHandler` を、形の違う節点や上限を超える長さには `BadEvidence` を返す。
    pub unsafe fn ev_lookup(&mut self, effect: u16) -> (r: Result<Word, HeapError>)
        requires
            old(self).wf(),
            !M::CHECKED ==> old(self).lookup(effect) is Some,
        ensures
            final(self).wf(),
            r is Err ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).lookup(effect) is Some ==> (r matches Ok(e) && {
                &&& e == old(self).lookup(effect)->0
                &&& final(self).held_ok()
                &&& final(self).holds(e)
                &&& final(self).ev_shape(e)
                &&& final(self).ev_tag(e) == effect + 1
                &&& final(self).evolved_by(*old(self), |b: usize| points(e, b), Set::empty())
            }),
            M::CHECKED && old(self).held_ok() && old(self).lookup(effect) is None ==> r == Err::<
                Word,
                HeapError,
            >(HeapError::NoHandler),
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok();
        let tag: u16 = if effect == u16::MAX {
            0
        } else {
            effect + 1
        };
        let want = Meta::new(tag, 2, Kind::Data, false);
        proof {
            want.lemma_fields();
        }
        let wanted = want.bits();
        let ghost f = choose|f: nat| #[trigger] path(s0.heap, s0.evv, f) is Some;
        let ghost s = seq_of(s0.heap, s0.evv);
        proof {
            if c {
                lemma_seq_of(s0.heap, s0.evv, f);
                assert(s.subrange(0, s.len() as int) =~= s);
            }
        }
        let mut node = self.evv;
        let mut steps: usize = 0;
        let ghost mut i: int = 0;
        loop
            invariant
                self.wf(),
                self.heap == s0.heap,
                self.evv == s0.evv,
                self.yielding == s0.yielding,
                self.max_evv == s0.max_evv,
                self.heap.word_in(node),
                steps <= self.max_evv,
                s0 == *old(self),
                c == s0.held_ok(),
                s == seq_of(s0.heap, s0.evv),
                tag == effect_tag(effect),
                wanted == want.bits_spec(),
                bits_wf(wanted),
                bits_tag(wanted) == tag,
                bits_scan(wanted) == 2,
                bits_kind(wanted) == Kind::Data,
                !bits_lin(wanted),
                c ==> {
                    &&& 0 <= i <= s.len()
                    &&& i <= f
                    &&& path(self.heap, node, (f - i) as nat) == Some(s.subrange(i, s.len() as int))
                    &&& find(s, tag) == find(s.subrange(i, s.len() as int), tag)
                    &&& M::CHECKED ==> steps == i
                    &&& s.len() <= self.max_evv
                },
                !M::CHECKED ==> c && find(s.subrange(i, s.len() as int), tag) is Some,
            ensures
                self.wf(),
                self.heap == s0.heap,
                self.evv == s0.evv,
                self.yielding == s0.yielding,
                self.max_evv == s0.max_evv,
                self.heap.word_in(node),
                node.is_ptr_spec(),
                c ==> {
                    &&& node_ok(self.heap, node)
                    &&& ev_ok(self.heap, self.heap.scanned_word(node, 0))
                    &&& find(s, tag) == Some(self.heap.scanned_word(node, 0))
                    &&& self.heap.tag_of(self.heap.scanned_word(node, 0)) == tag
                },
            decreases
                    if M::CHECKED {
                        self.max_evv - steps
                    } else {
                        s.len() - i
                    },
        {
            let ghost rest = s.subrange(i, s.len() as int);
            proof {
                if c && node.bits_spec() != 1 {
                    let k = (f - i) as nat;
                    assert(k > 0);
                    let next = path(self.heap, self.heap.scanned_word(node, 1), (k - 1) as nat);
                    assert(next is Some);
                    let x = (self.heap.tag_of(node), self.heap.scanned_word(node, 0));
                    assert(rest =~= seq![x] + next->0);
                    assert(rest.drop_first() =~= s.subrange(i + 1, s.len() as int));
                    assert((seq![x] + next->0).drop_first() =~= next->0);
                    assert(rest.len() >= 1);
                    assert(i < s.len());
                    assert(node.is_ptr_spec());
                }
                if c && node.bits_spec() == 1 {
                    assert(rest =~= Seq::<(u16, Word)>::empty());
                    assert(find(rest, tag) is None);
                }
                lemma_unit_bits();
            }
            if M::CHECKED {
                if node.bits() == 1 {
                    proof {
                        if c {
                            assert(rest.len() == 0);
                        }
                    }
                    return Err(HeapError::NoHandler);
                }
                if steps >= self.max_evv || !node.is_ptr() {
                    return Err(HeapError::BadEvidence);
                }
                self.stats.evidence_steps = self.stats.evidence_steps.wrapping_add(1);
            }
            let m = (unsafe { self.heap.meta(node) })?;
            let bits = m.bits();
            if bits == wanted {
                break;
            }
            proof {
                // 節点の形が正しければ、`meta` の値が違うのは tag が違うときだけである
                if c && bits_tag(bits) == bits_tag(wanted) {
                    lemma_meta_eq(bits, wanted);
                }
            }
            if M::CHECKED && (!Self::data_meta(m, 2) || m.tag() == 0) {
                return Err(HeapError::BadEvidence);
            }
            proof {
                lemma_unextended(self.heap, node);
            }
            node = (unsafe { self.heap.field(node, 1) })?;
            if M::CHECKED {
                steps += 1;
            }
            proof {
                i = i + 1;
            }
        }
        proof {
            if c {
                lemma_unextended(self.heap, node);
                s0.heap.lemma_readable_scanned(node, 0);
            }
        }
        let e = (unsafe { self.heap.field(node, 0) })?;
        (unsafe { self.heap.dup(e) })?;
        proof {
            if M::CHECKED {
                self.lemma_words_kept(s0);
            }
            if c && s0.lookup(effect) is Some {
                self.lemma_dupped_ok(s0, e, e);
            }
        }
        Ok(e)
    }
}

// 検査モードの確かめ。状態を変える前に呼ぶ
impl<M: Mode> Ctx<M> {
    /// `m` は、`lin` ビットのない `Data` の物体で、走査する語を `scan` 個持つ物体の `meta` である。
    fn data_meta(m: Meta, scan: u8) -> (r: bool)
        ensures
            r == (bits_kind(m.bits_spec()) == Kind::Data && bits_scan(m.bits_spec()) == scan
                && !bits_lin(m.bits_spec())),
    {
        proof {
            m.lemma_fields();
        }
        matches!(m.kind(), Kind::Data) && m.scan() == scan && !m.lin()
    }

    /// `ev` が evidence の形かを確かめ、入口の表を返す。表の欄の値は、読む操作が確かめる。
    fn checked_ev(&self, ev: Word) -> (r: Result<Word, HeapError>)
        requires
            M::CHECKED,
            self.wf(),
            self.heap.word_in(ev),
        ensures
            r matches Ok(t) ==> {
                &&& ev.is_ptr_spec()
                &&& self.heap.scan_of(ev) == 3
                &&& t == self.heap.scanned_word(ev, 0)
                &&& t.is_ptr_spec()
                &&& self.heap.word_in(t)
            },
            self.held_ok() && self.ev_shape(ev) ==> r == Ok::<Word, HeapError>(
                self.heap.scanned_word(ev, 0),
            ),
    {
        if !ev.is_ptr() {
            return Err(HeapError::BadEvidence);
        }
        let m = (unsafe { self.heap.meta(ev) })?;
        proof {
            m.lemma_fields();
        }
        if !Self::data_meta(m, 3) || m.tag() == 0 {
            return Err(HeapError::BadEvidence);
        }
        proof {
            lemma_unextended(self.heap, ev);
        }
        let t = (unsafe { self.heap.field(ev, 0) })?;
        if !t.is_ptr() {
            return Err(HeapError::BadEvidence);
        }
        let tm = (unsafe { self.heap.meta(t) })?;
        proof {
            tm.lemma_fields();
        }
        if !matches!(tm.kind(), Kind::Data) || tm.lin() || tm.tag() != 0 {
            return Err(HeapError::BadEvidence);
        }
        Ok(t)
    }

    /// `l` が `max_evv` より短いリストかを、節点をたどって確かめる。輪になったリストも、上限の歩数で止まる。
    fn checked_room(&self, l: Word) -> (r: Result<(), HeapError>)
        requires
            M::CHECKED,
            self.wf(),
            self.heap.word_in(l),
        ensures
            self.held_ok() && self.evv_wf(l) && self.evv_seq(l).len() < self.max_evv ==> r is Ok,
    {
        let ghost c = self.held_ok() && self.evv_wf(l) && self.evv_seq(l).len() < self.max_evv;
        let ghost f = choose|f: nat| #[trigger] path(self.heap, l, f) is Some;
        let ghost s = seq_of(self.heap, l);
        proof {
            if c {
                lemma_seq_of(self.heap, l, f);
            }
        }
        let mut n = l;
        let mut count: usize = 0;
        while n.bits() != 1
            invariant
                M::CHECKED,
                self.wf(),
                self.heap.word_in(n),
                count <= self.max_evv,
                c == (self.held_ok() && self.evv_wf(l) && self.evv_seq(l).len() < self.max_evv),
                c ==> {
                    &&& count <= f
                    &&& count <= s.len()
                    &&& path(self.heap, n, (f - count) as nat) == Some(
                        s.subrange(count as int, s.len() as int),
                    )
                    &&& s.len() < self.max_evv
                    &&& self.heap.consistent()
                    &&& self.heap.settleable()
                },
            decreases self.max_evv - count,
        {
            proof {
                if c {
                    let k = (f - count) as nat;
                    let rest = s.subrange(count as int, s.len() as int);
                    assert(k > 0);
                    assert(node_ok(self.heap, n));
                    let next = path(self.heap, self.heap.scanned_word(n, 1), (k - 1) as nat);
                    assert(next is Some);
                    assert(rest =~= seq![(self.heap.tag_of(n), self.heap.scanned_word(n, 0))]
                        + next->0);
                    assert(rest.len() >= 1);
                }
            }
            if self.max_evv - count <= 1 || !n.is_ptr() {
                return Err(HeapError::BadEvidence);
            }
            let m = (unsafe { self.heap.meta(n) })?;
            proof {
                m.lemma_fields();
            }
            if !Self::data_meta(m, 2) || m.tag() == 0 {
                return Err(HeapError::BadEvidence);
            }
            proof {
                lemma_unextended(self.heap, n);
                if c {
                    let k = (f - count) as nat;
                    let rest = s.subrange(count as int, s.len() as int);
                    let next = path(self.heap, self.heap.scanned_word(n, 1), (k - 1) as nat);
                    let x = (self.heap.tag_of(n), self.heap.scanned_word(n, 0));
                    assert(rest =~= seq![x] + next->0);
                    assert(rest.drop_first() =~= s.subrange(count + 1, s.len() as int));
                    assert((seq![x] + next->0).drop_first() =~= next->0);
                }
            }
            n = (unsafe { self.heap.field(n, 1) })?;
            count += 1;
        }
        // 足す節点の分も上限に入る。空のリストに足すときはループに入らないので、ここで確かめる
        if count >= self.max_evv {
            return Err(HeapError::BadEvidence);
        }
        Ok(())
    }
}

} // verus!
// 安全点の確かめは何も約束しないので、`verus!` の外に書く (docs/spec/lir.md の「実行の文脈」)。止める語は「止めよ」の
// ビットだけを伝えるので、Relaxed で読む
impl<M: Mode> Ctx<M> {
    /// 安全点の確かめ。`depth` は VM のタスクのスタックの語の数である。
    #[inline]
    pub fn poll(&self, depth: usize) -> Result<(), Stop> {
        let bits = self.control.stop_bits();
        if depth > self.max_depth || bits != 0 {
            return Err(stop_reason(bits));
        }
        Ok(())
    }
}

/// 止める理由。割り込みを確保量より先に、確保量を深さより先に報告する。ビットは速い道で読んだ値を受け、読み直さない。
/// 読み直すと、その間にほかのスレッドが `clear` したとき、深さの上限を超えていなくても `DepthLimit` を返すためである。
#[cold]
fn stop_reason(bits: u32) -> Stop {
    if bits & Control::INTERRUPT != 0 {
        Stop::Interrupted
    } else if bits & Control::MEMORY != 0 {
        Stop::MemoryLimit
    } else {
        Stop::DepthLimit
    }
}
