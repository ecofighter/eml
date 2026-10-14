//! ヒープの証明の部品。確保器の変化の枠の補題、新しいブロックをヒープに入れる手順、構築中の物体を生きている物体に
//! する手順を置く。`publish` と `settle` は ghost の値だけを変える関数で、実行の時には何もしない。

use vstd::prelude::*;

use super::Core;
#[cfg(verus_only)]
use super::inv::{p_code, p_init, p_owner, p_scanned, p_word, prefix_kept};
use super::model::BlockState;
#[cfg(verus_only)]
use super::model::{
    Model, Pos, indeg, lemma_indeg_pos, lemma_indeg_remove, lemma_indeg_union, lemma_indeg_witness,
    lemma_indeg_zero,
};
#[cfg(verus_only)]
use crate::meta::HEADER_WORDS;
use crate::meta::{BlockPerm, Header};
#[cfg(verus_only)]
use crate::rc::{magnitude, pinned};
#[cfg(verus_only)]
use crate::word::Word;

verus! {

impl Core {
    /// `old` の生きている物体のうち、不死か、外の参照を持ち続けるものは、同じ中身で生きている。外の参照の数は、`w` だけ
    /// `delta` 変わる。
    pub(crate) open spec fn evolved(&self, old: Core, w: usize, delta: int) -> bool {
        &&& self.env == old.env
        &&& forall|b: usize|
            #![trigger old.dom().contains(b)]
            old.dom().contains(b) && old.settled_state(b) == BlockState::Live && (old.rc_of(b) == 0
                || old.ext(b) + (if b == w {
                delta
            } else {
                0
            }) >= 1) ==> {
                &&& self.dom().contains(b)
                &&& self.state(b) == BlockState::Live
                &&& self.perm(b).meta == old.perm(b).meta
                &&& self.perm(b).slots == old.perm(b).slots
                &&& self.perm(b).text == old.perm(b).text
                &&& self.block(b) == old.block(b)
                &&& self.cap(b) == old.cap(b)
                &&& (old.rc_of(b) == 0) == (self.rc_of(b) == 0)
                &&& old.rc_of(b) != 0 ==> self.ext(b) == old.ext(b) + if b == w {
                    delta
                } else {
                    0
                }
                &&& self.model@.owner[b] == old.model@.owner[b]
            }
    }

    /// 空ける途中でも隔離したのでもないブロック。解放の連鎖の止まることの尺度に使う。
    pub(crate) open spec fn active(&self) -> Set<usize> {
        self.dom().filter(|b: usize| self.state(b) != BlockState::Freed)
    }

    /// 空ける途中のブロックはない。
    pub(crate) open spec fn quiet(&self) -> bool {
        forall|b: usize| #[trigger] self.dom().contains(b) ==> self.state(b) != BlockState::Dying
    }

    /// `settle` の後の状態。構築中の物体は生きている物体になる。
    pub(crate) open spec fn settled_state(&self, b: usize) -> BlockState {
        if self.model@.building == Some(b) {
            BlockState::Live
        } else {
            self.state(b)
        }
    }

    /// 構築中の物体を生きている物体にした見方。権限は同じで、構築中の物体の「残した」語のうち物体を指すものが辺になる。
    pub(crate) open spec fn settled_view(&self) -> Core {
        match self.model@.building {
            Some(b) => Core {
                model: Ghost(
                    Model {
                        state: self.model@.state.insert(b, BlockState::Live),
                        edges: self.model@.edges.union_prefer_right(self.kept_edges(b)),
                        kept: self.model@.kept.filter(|e: Pos| e.0 != b),
                        building: None,
                        ..self.model@
                    },
                ),
                ..*self
            },
            None => *self,
        }
    }

    /// `old` を生きている物体にした見方のブロックは、どれも同じ権限と形と数を持ち続ける。
    pub(crate) open spec fn frame(&self, old: Core) -> bool {
        let v = old.settled_view();
        &&& forall|b: usize| #[trigger]
            old.dom().contains(b) ==> {
                &&& self.dom().contains(b)
                &&& self.perm(b) == old.perm(b)
                &&& self.state(b) == v.state(b)
                &&& self.block(b) == old.block(b)
                &&& self.cap(b) == old.cap(b)
                &&& self.model@.refs[b] == old.model@.refs[b]
                &&& self.model@.stuck[b] == old.model@.stuck[b]
                &&& self.model@.owner[b] == old.model@.owner[b]
            }
        &&& self.model@.edges == v.model@.edges
        &&& self.model@.kept == v.model@.kept
        &&& self.env == old.env
    }

    /// `self` で生きているブロック (構築中の物体を含む) は、`old` でも生きていて、`meta`、語、文字列のバイト、rc の
    /// 符号、持ち主が同じである。`evolved` は外の参照を持ち続ける物体しか言わないので、リストの後ろの節点のように辺だけで
    /// 持たれる物体の中身は、これで追う。解放の連鎖は何も確保せず、生き残る物体の語に書かず、rc を動かしても生き残る物体の
    /// 符号を変えないので、モデルが合わなくても成り立つ。
    pub(crate) open spec fn kept_live(&self, old: Core) -> bool {
        forall|b: usize|
            #![trigger self.settled_state(b)]
            self.has(b) && self.settled_state(b) == BlockState::Live ==> {
                &&& old.has(b)
                &&& old.settled_state(b) == BlockState::Live
                &&& self.kept_block(old, b)
            }
    }

    /// ブロック `b` は、`old` と同じ `meta`、語、文字列のバイト、rc の符号、持ち主を持つ。持ち主は rc が正の物体
    /// (`owned` が読む物体) についてだけ言う。
    pub(crate) open spec fn kept_block(&self, old: Core, b: usize) -> bool {
        &&& self.perm(b).meta == old.perm(b).meta
        &&& self.perm(b).slots == old.perm(b).slots
        &&& self.perm(b).text == old.perm(b).text
        &&& (self.rc_of(b) > 0) == (old.rc_of(b) > 0)
        &&& (self.rc_of(b) < 0) == (old.rc_of(b) < 0)
        &&& self.rc_of(b) > 0 ==> self.model@.owner[b] == old.model@.owner[b]
    }

    pub(crate) proof fn lemma_kept_live_trans(&self, mid: Core, old: Core)
        requires
            self.kept_live(mid),
            mid.kept_live(old),
        ensures
            self.kept_live(old),
    {
    }

    /// 構築中の物体を生きている物体にした見方は、どのブロックの中身も変えない。
    pub(crate) proof fn lemma_kept_live_settled(&self)
        ensures
            self.settled_view().kept_live(*self),
    {
    }

    pub(crate) proof fn lemma_kept_live_refl(&self)
        ensures
            self.kept_live(*self),
    {
    }

    /// 不死の物体の数だけを替えても、どのブロックの中身も rc も変えない。
    pub(crate) proof fn lemma_kept_live_recounted(&self, old: Core)
        requires
            self.recounted(old),
        ensures
            self.kept_live(old),
    {
        assert forall|b: usize| #[trigger]
            self.has(b) && self.settled_state(b) == BlockState::Live implies {
            &&& old.has(b)
            &&& old.settled_state(b) == BlockState::Live
            &&& self.kept_block(old, b)
        } by {
            if self.dom().contains(b) {
                assert(old.dom().contains(b));
            }
        }
    }

    /// 権限とブロックの状態と持ち主を変えない変化 (token の表だけを替えるなど) は、どのブロックの中身も変えない。
    pub(crate) proof fn lemma_kept_live_same(&self, old: Core)
        requires
            self.perms == old.perms,
            self.env == old.env,
            self.model@.state == old.model@.state,
            self.model@.building == old.model@.building,
            self.model@.owner == old.model@.owner,
        ensures
            self.kept_live(old),
    {
    }

    /// ブロック `h` のほかの権限と状態を変えず、ブロックを足さない変化は、`kept_live` を満たす。`h` は、後で生きて
    /// いるなら前も生きていて、中身が同じであればよい。
    pub(crate) proof fn lemma_kept_live_at(&self, old: Core, h: usize)
        requires
            old.sound(),
            self.env == old.env,
            self.dom().subset_of(old.dom()),
            self.model@.building is None,
            old.model@.building is None,
            forall|b: usize|
                b != h && #[trigger] self.dom().contains(b) ==> self.perms@[b] == old.perms@[b],
            forall|b: usize| b != h ==> #[trigger] self.state(b) == old.state(b),
            forall|b: usize| #[trigger]
                self.dom().contains(b) ==> self.model@.owner[b] == old.model@.owner[b],
            self.dom().contains(h) && self.state(h) == BlockState::Live ==> {
                &&& old.state(h) == BlockState::Live
                &&& self.kept_block(old, h)
            },
        ensures
            self.kept_live(old),
    {
        assert forall|b: usize| #[trigger]
            self.has(b) && self.settled_state(b) == BlockState::Live implies {
            &&& old.has(b)
            &&& old.settled_state(b) == BlockState::Live
            &&& self.kept_block(old, b)
        } by {
            if self.dom().contains(b) {
                assert(old.dom().contains(b));
                if b != h {
                    assert(self.perms@[b] == old.perms@[b]);
                    assert(self.state(b) == old.state(b));
                }
            } else {
                // 付けた塊のブロックは、前もヒープのブロックでなかった。塊の物体は rc 0 なので、持ち主を読まない
                old.lemma_apart(b);
                old.lemma_region_at(b);
            }
        }
    }

    /// 確保器と、ブロックの集まりと、付けた塊が同じなら、どの語の妥当さも変わらない。
    pub(crate) proof fn lemma_same_words(&self, old: Core)
        requires
            self.pages == old.pages,
            self.dom() == old.dom(),
            self.env == old.env,
        ensures
            forall|w: Word| #[trigger] self.word_ok(w) == old.word_ok(w),
    {
    }

    /// 枠をつなぐ。`settle` の後 (`s1`)、確保器からブロックを取った後 (`mid`)、そのブロックを入れた後 (`self`) の3つの変化を
    /// 合わせると、`s0` からの枠になる。
    pub(crate) proof fn lemma_frame_chain(&self, mid: Core, s1: Core, s0: Core)
        requires
            s1.frame(s0),
            s1.dom() == s0.dom(),
            s1.model@.building is None,
            mid.perms == s1.perms,
            mid.model == s1.model,
            mid.env == s1.env,
            forall|b: usize| #[trigger]
                s1.dom().contains(b) ==> mid.block(b) == s1.block(b) && mid.cap(b) == s1.cap(b),
            self.frame(mid),
        ensures
            self.frame(s0),
    {
        assert forall|b: usize| #[trigger] s0.dom().contains(b) implies {
            &&& self.dom().contains(b)
            &&& self.perm(b) == s0.perm(b)
            &&& self.state(b) == s0.settled_state(b)
            &&& self.block(b) == s0.block(b)
            &&& self.cap(b) == s0.cap(b)
            &&& self.model@.refs[b] == s0.model@.refs[b]
            &&& self.model@.stuck[b] == s0.model@.stuck[b]
            &&& self.model@.owner[b] == s0.model@.owner[b]
        } by {
            assert(s1.dom().contains(b));
            assert(mid.dom().contains(b));
        }
    }

    /// 確保器が変わっても、ヒープのブロックの形が同じなら、不変条件の残りは変わらない。
    pub(crate) proof fn lemma_pages_frame(&self, s0: Core)
        requires
            s0.rest_wf(),
            self.perms == s0.perms,
            self.model == s0.model,
            self.env == s0.env,
            self.pages.wf(),
            self.pages.prefixed() == s0.pages.prefixed(),
            self.env_wf(),
            forall|b: usize| #[trigger]
                s0.dom().contains(b) ==> self.block(b) == s0.block(b) && self.cap(b) == s0.cap(b)
                    && self.pages.carved_block(b),
        ensures
            self.rest_wf(),
            forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w),
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::refs_wf);
        reveal(Core::building_wf);
        assert forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w) by {
            if w.is_ptr_spec() && s0.dom().contains(w.bits_spec()) {
                assert(self.block(w.bits_spec()) == s0.block(w.bits_spec()));
            }
        }
        assert forall|h: usize| #[trigger] self.dom().contains(h) implies self.block_wf(h) by {
            assert(s0.block_wf(h));
            let p = self.perm(h);
            assert forall|i: int| #[trigger]
                p_scanned(p, i) && s0.slot_word(p, h, i) implies self.slot_word(p, h, i) by {
                assert(self.word_ok(p_word(p, i)) == s0.word_ok(p_word(p, i)));
            }
            assert forall|i: int| #[trigger]
                self.model@.written[h].contains(i) && s0.slot_word(p, h, i) implies self.slot_word(
                p,
                h,
                i,
            ) by {
                assert(self.word_ok(p_word(p, i)) == s0.word_ok(p_word(p, i)));
            }
        }
    }

    /// 空のヒープは、不変条件を満たす。
    pub(crate) proof fn lemma_empty(&self)
        requires
            self.pages.wf(),
            self.pages.prefixed() == self.checked(),
            self.pages.in_use() == Set::<usize>::empty(),
            self.perms@ == Map::<usize, BlockPerm>::empty(),
            self.model@.state == Map::<usize, BlockState>::empty(),
            self.model@.refs == Map::<usize, nat>::empty(),
            self.model@.edges == Map::<(usize, int), usize>::empty(),
            self.model@.stuck == Map::<usize, bool>::empty(),
            self.model@.owner == Map::<usize, u32>::empty(),
            self.model@.written == Map::<usize, Set<int>>::empty(),
            self.model@.kept == Set::<Pos>::empty(),
            self.model@.building is None,
            self.env@.region is None,
        ensures
            self.full(),
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::refs_wf);
        reveal(Core::building_wf);
        assert(self.dom() =~= self.pages.in_use());
    }

    /// 確保器からブロックを取った後も、ヒープの不変条件は保たれ、取ったブロックはまだヒープの外にある。
    pub(crate) proof fn lemma_alloc(&self, s0: Core, words: usize, h: *mut Header, perm: BlockPerm)
        requires
            s0.dom() == s0.pages.in_use(),
            s0.pages.wf(),
            s0.pages.prefixed() == s0.checked(),
            self.perms == s0.perms,
            self.model == s0.model,
            self.env == s0.env,
            self.pages.alloc_post(s0.pages, words, h, perm),
            !s0.region_has(h@.addr),
        ensures
            s0.full() ==> self.pending(h@.addr),
            s0.safe_wf() ==> self.safe_pending(h@.addr),
            h == self.block(h@.addr),
            self.pages.carved_block(h@.addr),
            perm.wf(h, self.cap(h@.addr), self.checked()),
            self.cap(h@.addr) + HEADER_WORDS >= words,
            forall|b: usize| #[trigger]
                s0.dom().contains(b) ==> self.block(b) == s0.block(b) && self.cap(b) == s0.cap(b)
                    && self.pages.carved_block(b),
            forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w),
    {
        assert forall|b: usize| #[trigger] s0.dom().contains(b) implies self.block(b) == s0.block(b)
            && self.cap(b) == s0.cap(b) && self.pages.carved_block(b) by {
            assert(s0.pages.in_use().contains(b));
        }
        assert forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w) by {
            if w.is_ptr_spec() && s0.dom().contains(w.bits_spec()) {
                assert(self.block(w.bits_spec()) == s0.block(w.bits_spec()));
            }
        }
        if s0.full() || s0.safe_wf() {
            assert forall|x: usize|
                self.region_has(x) implies !#[trigger] self.pages.in_use().contains(x) by {
                assert(s0.region_has(x));
            }
        }
        if s0.full() {
            self.lemma_pages_frame(s0);
        }
        if s0.safe_wf() {
            self.lemma_safe_pages_frame(s0);
        }
        assert(self.dom().insert(h@.addr) =~= self.pages.in_use());
    }

    /// 書き終えた構築中の物体を、生きている物体にする。
    #[inline(always)]
    pub(crate) fn settle(&mut self)
        requires
            (old(self).full() && old(self).settleable()) || (old(self).checked() && old(
                self,
            ).safe_wf() && old(self).safe_settleable()),
        ensures
            old(self).full() && old(self).settleable() ==> final(self).full(),
            old(self).checked() && old(self).safe_wf() && old(self).safe_settleable()
                ==> final(self).safe_wf(),
            final(self).model@.building is None,
            final(self).frame(*old(self)),
            final(self).model == old(self).settled_view().model,
            *final(self) == old(self).settled_view(),
            final(self).dom() == old(self).dom(),
            final(self).pages == old(self).pages,
            forall|w: Word| #[trigger] final(self).word_ok(w) == old(self).word_ok(w),
    {
        proof {
            let s0 = *self;
            if let Some(b) = self.model@.building {
                self.model = Ghost(s0.settled_view().model@);
                assert(*self == s0.settled_view());
                if s0.full() && s0.settleable() {
                    self.lemma_settle(s0, b);
                }
                if s0.checked() && s0.safe_wf() && s0.safe_settleable() {
                    self.lemma_safe_settle(s0, b);
                }
            }
        }
    }

    /// 見方の辺は、構築中の物体の「残した」語の辺を足したものなので、行き先ごとの数は減らない。
    pub(crate) proof fn lemma_view_indeg(&self)
        requires
            self.full(),
        ensures
            forall|t: usize| #[trigger] self.settled_view().indeg(t) >= self.indeg(t),
            self.model@.building matches Some(b) ==> forall|t: usize| #[trigger]
                self.settled_view().indeg(t) == self.indeg(t) + indeg(self.kept_edges(b), t),
            self.model@.building matches Some(b) ==> self.model@.edges.dom().disjoint(
                self.kept_edges(b).dom(),
            ),
    {
        if let Some(b) = self.model@.building {
            reveal(Core::blocks_wf);
            reveal(Core::edges_wf);
            reveal(Core::building_wf);
            let k = self.kept_edges(b);
            // 構築中の物体の辺は書いた語だけから出るので、「残した」語の位置には辺がない
            assert(self.model@.edges.dom().disjoint(k.dom())) by {
                assert forall|e: Pos| #[trigger]
                    k.dom().contains(e) implies !self.model@.edges.dom().contains(e) by {
                    if self.model@.edges.dom().contains(e) {
                        assert(self.edge_wf(e));
                        assert(self.block_wf(b));
                    }
                }
            }
            assert forall|t: usize| #[trigger]
                self.settled_view().indeg(t) == self.indeg(t) + indeg(k, t) by {
                lemma_indeg_union(self.model@.edges, k, t);
            }
        }
    }

    /// 書き終えた構築中の物体 `b` を生きている物体にした見方も、不変条件を満たす。
    pub(crate) proof fn lemma_settle(&self, s0: Core, b: usize)
        requires
            s0.full(),
            s0.settleable(),
            s0.model@.building == Some(b),
            self.perms == s0.perms,
            self.pages == s0.pages,
            self.env == s0.env,
            self.model == s0.settled_view().model,
        ensures
            self.full(),
            self.frame(s0),
            forall|t: usize| #[trigger] self.indeg(t) == s0.indeg(t) + indeg(s0.kept_edges(b), t),
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::refs_wf);
        reveal(Core::building_wf);
        let k = s0.kept_edges(b);
        let p = s0.perm(b);
        assert(s0.state(b) == BlockState::Building);
        assert(s0.block_wf(b));
        s0.lemma_view_indeg();
        assert forall|t: usize| #[trigger] self.indeg(t) == s0.indeg(t) + indeg(k, t) by {
            assert(self.indeg(t) == s0.settled_view().indeg(t));
        }
        // 「残した」語の行き先は、生きている物体か b 自身である
        assert forall|t: usize| #[trigger] indeg(k, t) > 0 implies s0.state(t) == BlockState::Live
            || t == b by {
            let e = lemma_indeg_witness(k, t);
            assert(s0.kept_ok(b, p_word(p, e.1)));
        }
        assert(self.model@.state.dom() =~= self.dom());
        assert forall|h: usize| #[trigger] self.dom().contains(h) implies self.block_wf(h) by {
            assert(s0.block_wf(h));
            let q = self.perm(h);
            if h == b {
                assert forall|i: int| #[trigger] p_scanned(q, i) implies self.slot_word(
                    q,
                    b,
                    i,
                ) by {
                    assert(s0.scanned(b, i));
                    if s0.model@.written[b].contains(i) {
                        assert(s0.slot_word(q, b, i));
                    } else {
                        assert(s0.model@.kept.contains((b, i)));
                        assert(s0.kept_wf((b, i)));
                        assert(s0.kept_ok(b, p_word(q, i)));
                        if p_word(q, i).is_ptr_spec() {
                            assert(k.dom().contains((b, i)));
                        }
                    }
                }
            } else {
                if s0.state(h) == BlockState::Live {
                    assert forall|i: int| #[trigger] p_scanned(q, i) implies self.slot_word(
                        q,
                        h,
                        i,
                    ) by {
                        assert(s0.slot_word(q, h, i));
                    }
                }
                if s0.state(h) == BlockState::Building {
                    assert(s0.model@.building == Some(h));
                }
            }
        }
        assert forall|e: Pos| #[trigger] self.model@.kept.contains(e) implies self.kept_wf(e) by {
            assert(s0.model@.kept.contains(e));
            assert(s0.kept_wf(e));
            if s0.state(e.0) == BlockState::Building {
                assert(s0.model@.building == Some(e.0));
            }
        }
        assert forall|e: Pos| #[trigger] self.model@.edges.dom().contains(e) implies self.edge_wf(
            e,
        ) by {
            if s0.model@.edges.dom().contains(e) {
                assert(s0.edge_wf(e));
                let t = s0.model@.edges[e];
                if s0.state(t) == BlockState::Building {
                    assert(s0.model@.building == Some(t));
                }
                if s0.state(e.0) == BlockState::Building {
                    assert(s0.model@.building == Some(e.0));
                }
            } else {
                assert(k.dom().contains(e));
                assert(e.0 == b);
                assert(s0.model@.kept.contains(e));
                assert(s0.kept_wf(e));
                assert(s0.kept_ok(b, p_word(p, e.1)));
                lemma_indeg_pos(k, e);
            }
        }
        assert forall|t: usize| #[trigger] self.dom().contains(t) implies self.counted_wf(t) by {
            assert(s0.counted_wf(t));
            if indeg(k, t) > 0 {
                if s0.state(t) == BlockState::Building {
                    assert(s0.model@.building == Some(t));
                }
            }
        }
    }

    /// 確保器から取ったブロック `h` を、状態 `st` の物体としてヒープに入れる。ペイロードは書いてあり、ポインタの語はない。
    #[inline(always)]
    pub(crate) fn publish(
        &mut self,
        Ghost(h): Ghost<*mut Header>,
        Tracked(perm): Tracked<BlockPerm>,
        Ghost(st): Ghost<BlockState>,
        Ghost(owner): Ghost<u32>,
    )
        requires
            old(self).pending(h@.addr) || old(self).checked(),
            old(self).checked() ==> old(self).safe_pending(h@.addr),
            old(self).model@.building is None,
            h == old(self).block(h@.addr),
            old(self).pages.carved_block(h@.addr),
            old(self).perm_wf(perm, h@.addr, st, Set::empty(), owner),
            st == BlockState::Live || st == BlockState::Building,
            perm.rc.value() == 0 || perm.rc.value() == 1,
        ensures
            final(self).model@.tokens == old(self).model@.tokens,
            old(self).pending(h@.addr) ==> final(self).full(),
            final(self).checked() ==> final(self).safe_wf(),
            final(self).env == old(self).env,
            final(self).dom() == old(self).dom().insert(h@.addr),
            final(self).perm(h@.addr) == perm,
            final(self).state(h@.addr) == st,
            final(self).model@.refs[h@.addr] == if perm.rc.value() == 1 {
                1nat
            } else {
                0nat
            },
            final(self).model@.written[h@.addr] == Set::<int>::empty(),
            final(self).model@.owner[h@.addr] == owner,
            final(self).model@.building == if st == BlockState::Building {
                Some(h@.addr)
            } else {
                None
            },
            old(self).pending(h@.addr) ==> final(self).frame(*old(self)),
            final(self).pages == old(self).pages,
            old(self).pending(h@.addr) ==> final(self).indeg(h@.addr) == 0,
            forall|w: Word| old(self).word_ok(w) ==> #[trigger] final(self).word_ok(w),
            forall|b: usize|
                b != h@.addr ==> #[trigger] final(self).perms@[b] == old(self).perms@[b],
            forall|b: usize| b != h@.addr ==> #[trigger] final(self).state(b) == old(self).state(b),
            forall|b: usize|
                b != h@.addr ==> #[trigger] final(self).model@.owner[b] == old(
                    self,
                ).model@.owner[b],
            !old(self).has(h@.addr),
    {
        proof {
            let s0 = *self;
            let hs = h@.addr;
            self.perms.borrow_mut().tracked_insert(hs, perm);
            self.model = Ghost(
                Model {
                    state: self.model@.state.insert(hs, st),
                    refs: self.model@.refs.insert(
                        hs,
                        if perm.rc.value() == 1 {
                            1nat
                        } else {
                            0nat
                        },
                    ),
                    stuck: self.model@.stuck.insert(hs, false),
                    owner: self.model@.owner.insert(hs, owner),
                    written: self.model@.written.insert(hs, Set::empty()),
                    building: if st == BlockState::Building {
                        Some(hs)
                    } else {
                        None
                    },
                    ..self.model@
                },
            );
            // 確保器が渡したブロックは、付けた塊のブロックでない
            assert(s0.pages.in_use().contains(hs));
            assert(!s0.region_has(hs));
            if s0.pending(hs) {
                self.lemma_publish(s0, hs);
            }
            assert forall|w: Word| s0.word_ok(w) implies #[trigger] self.word_ok(w) by {}
            if self.checked() {
                self.lemma_perm_safe(s0, perm, hs, st, owner);
                reveal(Core::building_wf);
                reveal(Core::safe_blocks_wf);
                assert forall|b: usize| #[trigger]
                    self.dom().contains(b) && self.state(b)
                        == BlockState::Building implies self.model@.building == Some(b) by {
                    if b != hs {
                        assert(s0.dom().contains(b));
                    }
                }
                assert(self.building_wf());
                self.lemma_safe_publish(s0, hs);
            }
        }
    }

    proof fn lemma_publish(&self, s0: Core, h: usize)
        requires
            s0.pending(h),
            !s0.region_has(h),
            s0.model@.building is None,
            s0.pages.carved_block(h),
            self.pages == s0.pages,
            self.env == s0.env,
            self.perms@ == s0.perms@.insert(h, self.perm(h)),
            s0.perm_wf(self.perm(h), h, self.state(h), Set::empty(), self.model@.owner[h]),
            self.state(h) == BlockState::Live || self.state(h) == BlockState::Building,
            self.rc_of(h) == 0 || self.rc_of(h) == 1,
            self.model@.state == s0.model@.state.insert(h, self.state(h)),
            self.model@.refs == s0.model@.refs.insert(
                h,
                if self.rc_of(h) == 1 {
                    1nat
                } else {
                    0nat
                },
            ),
            self.model@.stuck == s0.model@.stuck.insert(h, false),
            self.model@.owner.dom() == s0.model@.owner.dom().insert(h),
            forall|b: usize| b != h ==> #[trigger] self.model@.owner[b] == s0.model@.owner[b],
            self.model@.written == s0.model@.written.insert(h, Set::empty()),
            self.model@.edges == s0.model@.edges,
            self.model@.kept == s0.model@.kept,
            self.model@.building == if self.state(h) == BlockState::Building {
                Some(h)
            } else {
                None
            },
        ensures
            self.full(),
            self.frame(s0),
            self.indeg(h) == 0,
            forall|w: Word| s0.word_ok(w) ==> #[trigger] self.word_ok(w),
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::refs_wf);
        reveal(Core::building_wf);
        // 新しいブロックを指す辺はない
        assert forall|e: (usize, int)| #[trigger]
            s0.model@.edges.dom().contains(e) implies s0.model@.edges[e] != h by {
            assert(s0.edge_wf(e));
        }
        assert(super::model::targets(self.model@.edges, h) =~= Set::empty());
        assert(self.dom() =~= self.pages.in_use());
        assert(self.model@.state.dom() =~= self.dom());
        assert(self.model@.refs.dom() =~= self.dom());
        assert(self.model@.stuck.dom() =~= self.dom());
        assert(self.model@.owner.dom() =~= self.dom());
        assert(self.model@.written.dom() =~= self.dom());
        assert forall|w: Word| s0.word_ok(w) implies #[trigger] self.word_ok(w) by {}
        // 新しいブロックの語は「残した」語でない
        assert forall|e: Pos| #[trigger] self.model@.kept.contains(e) implies self.kept_wf(e) && e.0
            != h by {
            assert(s0.kept_wf(e));
            assert(self.perms@[e.0] == s0.perms@[e.0]);
        }
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.block_wf(b) by {
            let p = self.perm(b);
            if b == h {
                assert forall|i: int| #[trigger]
                    p_scanned(p, i) && s0.slot_word(p, h, i) implies self.slot_word(p, h, i) by {}
            } else {
                assert(s0.block_wf(b));
                assert forall|i: int| #[trigger]
                    p_scanned(p, i) && s0.slot_word(p, b, i) implies self.slot_word(p, b, i) by {}
                assert forall|i: int| #[trigger]
                    self.model@.written[b].contains(i) && s0.slot_word(
                        p,
                        b,
                        i,
                    ) implies self.slot_word(p, b, i) by {}
            }
        }
        assert forall|e: (usize, int)| #[trigger]
            self.model@.edges.dom().contains(e) implies self.edge_wf(e) by {
            assert(s0.edge_wf(e));
        }
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.counted_wf(b) by {
            if b != h {
                assert(s0.counted_wf(b));
            }
        }
    }

    /// 物体 `h` の rc と数を替える。`dying` なら、最後の参照を手放したので空ける途中にする。
    pub(crate) proof fn lemma_count_step(&self, s0: Core, h: usize, dying: bool)
        requires
            s0.full(),
            s0.dom().contains(h),
            s0.state(h) == BlockState::Live,
            self.pages == s0.pages,
            self.env == s0.env,
            self.perms@.dom() == s0.perms@.dom(),
            forall|b: usize| b != h ==> #[trigger] self.perms@[b] == s0.perms@[b],
            self.perm(h).meta == s0.perm(h).meta,
            self.perm(h).slots == s0.perm(h).slots,
            self.perm(h).text == s0.perm(h).text,
            prefix_kept(self.perm(h), s0.perm(h)),
            self.perm(h).prefix is Some ==> p_code(self.perm(h)) == if dying {
                0u32
            } else {
                1u32
            },
            self.perm(h).prefix is Some && !dying && self.rc_of(h) > 0 ==> s0.rc_of(h) > 0,
            self.perm(h).rc.ptr() == s0.perm(h).rc.ptr(),
            self.perm(h).rc.is_init(),
            self.model@.state == s0.model@.state.insert(
                h,
                if dying {
                    BlockState::Dying
                } else {
                    BlockState::Live
                },
            ),
            self.model@.refs.dom() == s0.model@.refs.dom(),
            forall|b: usize| b != h ==> #[trigger] self.model@.refs[b] == s0.model@.refs[b],
            self.model@.stuck.dom() == s0.model@.stuck.dom(),
            forall|b: usize| b != h ==> #[trigger] self.model@.stuck[b] == s0.model@.stuck[b],
            self.model@.edges == s0.model@.edges,
            self.model@.owner == s0.model@.owner,
            self.model@.written == s0.model@.written,
            self.model@.kept == s0.model@.kept,
            self.model@.building == s0.model@.building,
            dying ==> s0.indeg(h) == 0,
            !dying && self.rc_of(h) != 0 ==> {
                let rc = self.rc_of(h);
                &&& self.model@.refs[h] >= s0.indeg(h)
                &&& self.model@.stuck[h] ==> pinned(rc)
                &&& !self.model@.stuck[h] ==> !pinned(rc) && magnitude(rc) == self.model@.refs[h]
                    && self.model@.refs[h] > 0
            },
        ensures
            self.full(),
            forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w),
            forall|b: usize| #[trigger] self.indeg(b) == s0.indeg(b),
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::refs_wf);
        reveal(Core::building_wf);
        assert(self.model@.state.dom() =~= self.dom());
        assert forall|e: Pos| #[trigger] self.model@.kept.contains(e) implies self.kept_wf(e) by {
            assert(s0.kept_wf(e));
        }
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.block_wf(b) by {
            assert(s0.block_wf(b));
            let p = self.perm(b);
            let q = s0.perm(b);
            if b == h {
                assert forall|i: int| #[trigger] p_scanned(p, i) implies self.slot_word(p, b, i)
                    && p_init(p, i) by {
                    assert(p_scanned(q, i));
                    assert(s0.slot_word(q, b, i));
                }
            } else {
                assert(p == q);
            }
        }
        assert forall|e: (usize, int)| #[trigger]
            self.model@.edges.dom().contains(e) implies self.edge_wf(e) by {
            assert(s0.edge_wf(e));
            if dying && s0.model@.edges[e] == h {
                lemma_indeg_zero(s0.model@.edges, h, e);
            }
        }
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.counted_wf(b) by {
            assert(s0.counted_wf(b));
        }
    }

    /// 空ける途中の親の辺 `e` を外す。
    pub(crate) proof fn lemma_edge_removed(&self, s0: Core, e: (usize, int))
        requires
            s0.full(),
            s0.model@.edges.dom().contains(e),
            s0.state(e.0) == BlockState::Dying,
            self.pages == s0.pages,
            self.env == s0.env,
            self.perms == s0.perms,
            self.model@ == (Model { edges: s0.model@.edges.remove(e), ..s0.model@ }),
        ensures
            self.full(),
            self.indeg(s0.model@.edges[e]) + 1 == s0.indeg(s0.model@.edges[e]),
            forall|b: usize| b != s0.model@.edges[e] ==> #[trigger] self.indeg(b) == s0.indeg(b),
            forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w),
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::refs_wf);
        reveal(Core::building_wf);
        lemma_indeg_remove(s0.model@.edges, e);
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.block_wf(b) by {
            assert(s0.block_wf(b));
            let p = self.perm(b);
            if s0.state(b) == BlockState::Live {
                assert(b != e.0);
                assert forall|i: int| #[trigger] p_scanned(p, i) implies self.slot_word(
                    p,
                    b,
                    i,
                ) by {
                    assert(s0.slot_word(p, b, i));
                }
            }
            if s0.state(b) == BlockState::Building {
                assert(b != e.0);
                assert forall|i: int| #[trigger]
                    self.model@.written[b].contains(i) implies p_scanned(p, i) && self.slot_word(
                    p,
                    b,
                    i,
                ) by {
                    assert(s0.slot_word(p, b, i));
                }
            }
        }
        assert forall|f: (usize, int)| #[trigger]
            self.model@.edges.dom().contains(f) implies self.edge_wf(f) by {
            assert(s0.edge_wf(f));
        }
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.counted_wf(b) by {
            assert(s0.counted_wf(b));
        }
    }

    /// 速いモードで、空ける途中のブロック `h` を確保器に返した後の状態。
    pub(crate) proof fn lemma_block_released(&self, s0: Core, h: usize)
        requires
            s0.full(),
            s0.dom().contains(h),
            s0.state(h) == BlockState::Dying,
            s0.indeg(h) == 0,
            forall|j: int| !(#[trigger] s0.model@.edges.dom().contains((h, j))),
            self.env == s0.env,
            self.perms@ == s0.perms@.remove(h),
            self.model@ == (Model {
                state: s0.model@.state.remove(h),
                refs: s0.model@.refs.remove(h),
                stuck: s0.model@.stuck.remove(h),
                owner: s0.model@.owner.remove(h),
                written: s0.model@.written.remove(h),
                ..s0.model@
            }),
            self.pages.wf(),
            self.pages.prefixed() == s0.pages.prefixed(),
            self.pages.in_use() == s0.pages.in_use().remove(h),
            forall|b: usize| #[trigger]
                self.pages.in_use().contains(b) ==> self.pages.same_block(s0.pages, b),
        ensures
            self.full(),
            self.dom() == s0.dom().remove(h),
            forall|b: usize| #[trigger] self.indeg(b) == s0.indeg(b),
            forall|w: Word|
                s0.word_ok(w) && (w.is_ptr_spec() ==> w.bits_spec() != h)
                    ==> #[trigger] self.word_ok(w),
            forall|b: usize|
                b != h && s0.dom().contains(b) ==> #[trigger] self.block(b) == s0.block(b)
                    && self.cap(b) == s0.cap(b),
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::refs_wf);
        reveal(Core::building_wf);
        assert(self.dom() =~= s0.dom().remove(h));
        assert(self.dom() =~= self.pages.in_use());
        assert forall|b: usize| b != h && s0.dom().contains(b) implies #[trigger] self.block(b)
            == s0.block(b) && self.cap(b) == s0.cap(b) by {
            assert(self.pages.in_use().contains(b));
        }
        assert forall|w: Word|
            s0.word_ok(w) && (w.is_ptr_spec() ==> w.bits_spec()
                != h) implies #[trigger] self.word_ok(w) by {
            if w.is_ptr_spec() && s0.dom().contains(w.bits_spec()) {
                assert(self.pages.in_use().contains(w.bits_spec()));
            }
        }
        assert(self.model@.state.dom() =~= self.dom());
        assert(self.model@.refs.dom() =~= self.dom());
        assert(self.model@.stuck.dom() =~= self.dom());
        assert(self.model@.owner.dom() =~= self.dom());
        assert(self.model@.written.dom() =~= self.dom());
        // 残ったブロックの語は、h を指さない
        assert forall|e: (usize, int)| #[trigger]
            s0.model@.edges.dom().contains(e) implies s0.model@.edges[e] != h && e.0 != h by {
            lemma_indeg_zero(s0.model@.edges, h, e);
        }
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.block_wf(b) by {
            assert(s0.block_wf(b));
            assert(self.pages.in_use().contains(b));
            let p = self.perm(b);
            if s0.state(b) == BlockState::Live {
                assert forall|i: int| #[trigger] p_scanned(p, i) implies self.slot_word(
                    p,
                    b,
                    i,
                ) by {
                    assert(s0.slot_word(p, b, i));
                    if p_word(p, i).is_ptr_spec() {
                        assert(s0.edge_wf((b, i)));
                    }
                }
            }
            if s0.state(b) == BlockState::Building {
                assert forall|i: int| #[trigger]
                    self.model@.written[b].contains(i) implies p_scanned(p, i) && self.slot_word(
                    p,
                    b,
                    i,
                ) by {
                    assert(s0.slot_word(p, b, i));
                    if p_word(p, i).is_ptr_spec() {
                        assert(s0.edge_wf((b, i)));
                    }
                }
            }
        }
        assert forall|e: (usize, int)| #[trigger]
            self.model@.edges.dom().contains(e) implies self.edge_wf(e) by {
            assert(s0.edge_wf(e));
            if s0.dom().contains(s0.model@.edges[e]) {
                assert(self.pages.in_use().contains(s0.model@.edges[e]));
            }
        }
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.counted_wf(b) by {
            assert(s0.counted_wf(b));
        }
    }

    /// 検査モードで、空ける途中のブロック `h` を隔離した後の状態。権限はヒープに残し、前置きの状態だけを替える。
    pub(crate) proof fn lemma_block_freed(&self, s0: Core, h: usize)
        requires
            s0.full(),
            s0.dom().contains(h),
            s0.state(h) == BlockState::Dying,
            s0.indeg(h) == 0,
            forall|j: int| !(#[trigger] s0.model@.edges.dom().contains((h, j))),
            self.env == s0.env,
            self.pages == s0.pages,
            self.perms@.dom() == s0.perms@.dom(),
            forall|b: usize| b != h ==> #[trigger] self.perms@[b] == s0.perms@[b],
            self.perm(h).wf(self.block(h), self.cap(h), self.checked()),
            self.perm(h).prefix matches Some(q) ==> q.first.is_init() && q.state.is_init(),
            self.perm(h).prefix is Some ==> p_code(self.perm(h)) == 3,
            self.perm(h).rc.is_init(),
            self.perm(h).meta == s0.perm(h).meta,
            self.model@ == (Model {
                state: s0.model@.state.insert(h, BlockState::Freed),
                ..s0.model@
            }),
        ensures
            self.full(),
            self.dom() == s0.dom(),
            forall|b: usize| #[trigger] self.indeg(b) == s0.indeg(b),
            forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w),
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::refs_wf);
        reveal(Core::building_wf);
        assert(self.model@.state.dom() =~= self.dom());
        assert forall|e: (usize, int)| #[trigger]
            s0.model@.edges.dom().contains(e) implies s0.model@.edges[e] != h && e.0 != h by {
            lemma_indeg_zero(s0.model@.edges, h, e);
        }
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.block_wf(b) by {
            assert(s0.block_wf(b));
            if b != h {
                let p = self.perm(b);
                if s0.state(b) == BlockState::Live {
                    assert forall|i: int| #[trigger] p_scanned(p, i) implies self.slot_word(
                        p,
                        b,
                        i,
                    ) by {
                        assert(s0.slot_word(p, b, i));
                    }
                }
                if s0.state(b) == BlockState::Building {
                    assert forall|i: int| #[trigger]
                        self.model@.written[b].contains(i) implies p_scanned(p, i)
                        && self.slot_word(p, b, i) by {
                        assert(s0.slot_word(p, b, i));
                    }
                }
            }
        }
        assert forall|e: (usize, int)| #[trigger]
            self.model@.edges.dom().contains(e) implies self.edge_wf(e) by {
            assert(s0.edge_wf(e));
        }
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.counted_wf(b) by {
            assert(s0.counted_wf(b));
        }
    }
}

} // verus!
