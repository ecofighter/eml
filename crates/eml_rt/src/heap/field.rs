//! フィールドの読み書きの証明の部品。構築中の物体を生きている物体にした見方、生きている物体の語を読む補題、
//! 語を書いた後の不変条件の補題を置く。

use vstd::prelude::*;

use super::Core;
#[cfg(verus_only)]
use super::inv::{p_count, p_extended, p_first, p_init, p_kind, p_scanned, p_word, string_wf};
#[cfg(verus_only)]
use super::model::{BlockState, Model, Pos, lemma_indeg_insert, lemma_indeg_remove};
#[cfg(verus_only)]
use crate::meta::{Kind, bits_kind, bits_scan, bits_wf};
#[cfg(verus_only)]
use crate::word::Word;

verus! {

/// `set_field` の後の辺。i 語目の前の辺を外し、`v` がポインタなら i 語目から `v` への辺を足す。
pub(crate) open spec fn field_edges(edges: Map<Pos, usize>, h: usize, i: int, v: Word) -> Map<
    Pos,
    usize,
> {
    if v.is_ptr_spec() {
        edges.remove((h, i)).insert((h, i), v.bits_spec())
    } else {
        edges.remove((h, i))
    }
}

impl Core {
    /// `old` から、ブロック `h` の i 語目の値だけが変わった。
    pub(crate) open spec fn slot_changed(&self, old: Core, h: usize, i: int) -> bool {
        &&& self.pages == old.pages
        &&& self.env == old.env
        &&& self.perms@.dom() == old.perms@.dom()
        &&& forall|b: usize| b != h ==> #[trigger] self.perms@[b] == old.perms@[b]
        &&& self.perm(h).rc == old.perm(h).rc
        &&& self.perm(h).meta == old.perm(h).meta
        &&& self.perm(h).prefix == old.perm(h).prefix
        &&& self.perm(h).slots.dom() == old.perm(h).slots.dom()
        &&& self.perm(h).text == old.perm(h).text
        &&& forall|j: int| j != i ==> #[trigger] self.perm(h).slots[j] == old.perm(h).slots[j]
        &&& self.perm(h).slots[i].ptr() == old.perm(h).slots[i].ptr()
        &&& p_init(self.perm(h), i)
    }

    /// `set_field` の後の状態。i 語目に `v` を書き、i 語目の辺を `v` に替える。構築中の物体なら、i 語目を書いたことにする。
    pub(crate) open spec fn field_set(&self, old: Core, h: usize, i: int, v: Word) -> bool {
        &&& self.slot_changed(old, h, i)
        &&& p_word(self.perm(h), i) == v
        &&& self.model@ == Model {
            edges: field_edges(old.model@.edges, h, i, v),
            written: if old.state(h) == BlockState::Building {
                old.model@.written.insert(h, old.model@.written[h].insert(i))
            } else {
                old.model@.written
            },
            kept: old.model@.kept.remove((h, i)),
            ..old.model@
        }
    }

    /// 書き終えた構築中の物体を生きている物体にした見方も、不変条件を満たす。
    pub(crate) proof fn lemma_settled_view(&self)
        requires
            self.full(),
            self.settleable(),
        ensures
            self.settled_view().full(),
            self.settled_view().perms == self.perms,
            self.settled_view().pages == self.pages,
            self.settled_view().env == self.env,
            self.settled_view().model@.building is None,
            forall|b: usize| #[trigger] self.settled_view().state(b) == self.settled_state(b),
    {
        if let Some(b) = self.model@.building {
            self.settled_view().lemma_settle(*self, b);
        }
    }

    /// 読める語の指す物体は、生きているか構築中である。
    pub(crate) proof fn lemma_readable(&self, w: Word)
        requires
            self.full(),
            self.readable(w),
            w.is_ptr_spec(),
        ensures
            self.state(w.bits_spec()) == BlockState::Live || self.state(w.bits_spec())
                == BlockState::Building,
    {
        reveal(Core::building_wf);
    }

    /// 生きている物体 (書き終えた構築中の物体を含む) の形。
    pub(crate) proof fn lemma_live_block(&self, h: usize)
        requires
            self.full(),
            self.settleable(),
            self.has(h),
            self.settled_state(h) == BlockState::Live,
        ensures
            p_extended(self.perm(h)) ==> p_init(self.perm(h), 0),
            p_kind(self.perm(h)).boxes_value() ==> p_init(self.perm(h), 0),
            p_kind(self.perm(h)) == Kind::String ==> string_wf(self.perm(h), self.cap(h)),
            p_first(self.perm(h)) + p_count(self.perm(h)) <= self.cap(h),
    {
        if !self.dom().contains(h) {
            self.lemma_region_at(h);
            return;
        }
        self.lemma_settled_view();
        let c = self.settled_view();
        reveal(Core::blocks_wf);
        assert(c.block_wf(h));
    }

    /// 生きている物体 (書き終えた構築中の物体を含む) の走査する語は書いてあり、生きている物体を指す。
    pub(crate) proof fn lemma_live_slot(&self, h: usize, i: int)
        requires
            self.full(),
            self.settleable(),
            self.has(h),
            self.settled_state(h) == BlockState::Live,
            self.scanned(h, i),
        ensures
            p_init(self.perm(h), i),
            self.readable(p_word(self.perm(h), i)),
    {
        if !self.dom().contains(h) {
            self.lemma_region_at(h);
            return;
        }
        self.lemma_settled_view();
        let c = self.settled_view();
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::building_wf);
        assert(c.block_wf(h));
        assert(c.scanned(h, i));
        assert(c.slot_word(c.perm(h), h, i));
        let w = p_word(self.perm(h), i);
        if w.is_ptr_spec() {
            assert(c.edge_wf((h, i)));
            assert(c.state(w.bits_spec()) == BlockState::Live);
        }
    }

    /// `set_field` の後も、不変条件は保たれる。
    pub(crate) proof fn lemma_field_set(&self, s0: Core, h: usize, i: int, v: Word)
        requires
            s0.full(),
            s0.dom().contains(h),
            s0.state(h) == BlockState::Live || s0.state(h) == BlockState::Building,
            s0.scanned(h, i),
            s0.readable(v),
            v.is_ptr_spec() && s0.rc_of(v.bits_spec()) != 0 ==> s0.ext(v.bits_spec()) >= 1,
            self.field_set(s0, h, i, v),
        ensures
            self.full(),
            forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w),
            forall|b: usize, j: int| #[trigger] self.scanned(b, j) == s0.scanned(b, j),
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::refs_wf);
        reveal(Core::building_wf);
        let p = self.perm(h);
        let q = s0.perm(h);
        let e = (h, i);
        assert(s0.block_wf(h));
        // 走査する語の数の語 (`scan` が 255 のときの 0 語目) は、走査する語でないので変わらない
        assert(p_first(p) == p_first(q));
        if p_extended(q) {
            assert(i != 0);
        }
        assert(p_count(p) == p_count(q));
        assert forall|b: usize, j: int| #[trigger] self.scanned(b, j) == s0.scanned(b, j) by {
            if b != h {
                assert(self.perms@[b] == s0.perms@[b]);
            }
        }
        assert(self.model@.written.dom() =~= s0.model@.written.dom());
        // 辺の数の変化。前の辺を外してから新しい辺を足す
        let e1 = s0.model@.edges.remove(e);
        if s0.model@.edges.dom().contains(e) {
            lemma_indeg_remove(s0.model@.edges, e);
        } else {
            assert(e1 =~= s0.model@.edges);
        }
        if v.is_ptr_spec() {
            lemma_indeg_insert(e1, e, v.bits_spec());
        }
        assert forall|b: usize| #[trigger]
            self.indeg(b) <= s0.indeg(b) + if v.is_ptr_spec() && b == v.bits_spec() {
                1int
            } else {
                0int
            } by {}
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.block_wf(b) by {
            assert(s0.block_wf(b));
            if b == h {
                assert forall|j: int| #[trigger]
                    p.slots.contains_key(j) <==> 0 <= j < self.cap(h) by {
                    assert(p.slots.contains_key(j) == q.slots.contains_key(j));
                }
                assert forall|j: int| 0 <= j < self.cap(h) implies (#[trigger] p.slots[j]).ptr()
                    == q.slots[j].ptr() by {}
                if self.state(h) == BlockState::Live {
                    assert forall|j: int| #[trigger] p_scanned(p, j) implies self.slot_word(
                        p,
                        h,
                        j,
                    ) by {
                        if j != i {
                            assert(p_scanned(q, j));
                            assert(s0.slot_word(q, h, j));
                        }
                    }
                } else {
                    assert forall|j: int| #[trigger]
                        self.model@.written[h].contains(j) implies p_scanned(p, j)
                        && self.slot_word(p, h, j) by {
                        if j != i {
                            assert(s0.model@.written[h].contains(j));
                            assert(s0.slot_word(q, h, j));
                        }
                    }
                }
            } else {
                let pb = self.perm(b);
                assert(pb == s0.perm(b));
                if s0.state(b) == BlockState::Live {
                    assert forall|j: int| #[trigger] p_scanned(pb, j) implies self.slot_word(
                        pb,
                        b,
                        j,
                    ) by {
                        assert(s0.slot_word(pb, b, j));
                    }
                }
                if s0.state(b) == BlockState::Building {
                    assert forall|j: int| #[trigger]
                        self.model@.written[b].contains(j) implies p_scanned(pb, j)
                        && self.slot_word(pb, b, j) by {
                        assert(s0.slot_word(pb, b, j));
                    }
                }
            }
        }
        assert forall|f: Pos| #[trigger] self.model@.edges.dom().contains(f) implies self.edge_wf(
            f,
        ) by {
            if f != e {
                assert(s0.edge_wf(f));
                if f.0 != h {
                    assert(self.perms@[f.0] == s0.perms@[f.0]);
                }
            } else {
                assert(v.is_ptr_spec());
            }
        }
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.counted_wf(b) by {
            assert(s0.counted_wf(b));
            if s0.state(b) == BlockState::Dying && v.is_ptr_spec() {
                assert(b != v.bits_spec());
            }
        }
    }

    /// 生の語を書いた後も、不変条件は保たれる。生の語は走査しないので、辺は変わらない。
    pub(crate) proof fn lemma_raw_set(&self, s0: Core, h: usize, i: int)
        requires
            s0.full(),
            s0.dom().contains(h),
            s0.state(h) == BlockState::Live || s0.state(h) == BlockState::Building,
            p_first(s0.perm(h)) + p_count(s0.perm(h)) <= i,
            s0.kind(h) != Kind::String,
            self.slot_changed(s0, h, i),
            self.model == s0.model,
        ensures
            self.full(),
            forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w),
            forall|b: usize, j: int| #[trigger] self.scanned(b, j) == s0.scanned(b, j),
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::refs_wf);
        reveal(Core::building_wf);
        let p = self.perm(h);
        let q = s0.perm(h);
        assert(s0.block_wf(h));
        assert(p_first(p) == p_first(q));
        if p_extended(q) {
            assert(i != 0);
        }
        assert(p_count(p) == p_count(q));
        assert forall|b: usize, j: int| #[trigger] self.scanned(b, j) == s0.scanned(b, j) by {
            if b != h {
                assert(self.perms@[b] == s0.perms@[b]);
            }
        }
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.block_wf(b) by {
            assert(s0.block_wf(b));
            if b == h {
                assert forall|j: int| #[trigger]
                    p.slots.contains_key(j) <==> 0 <= j < self.cap(h) by {
                    assert(p.slots.contains_key(j) == q.slots.contains_key(j));
                }
                assert forall|j: int| 0 <= j < self.cap(h) implies (#[trigger] p.slots[j]).ptr()
                    == q.slots[j].ptr() by {}
                if self.state(h) == BlockState::Live {
                    assert forall|j: int| #[trigger] p_scanned(p, j) implies self.slot_word(
                        p,
                        h,
                        j,
                    ) by {
                        assert(p_scanned(q, j));
                        assert(s0.slot_word(q, h, j));
                    }
                } else {
                    assert forall|j: int| #[trigger]
                        self.model@.written[h].contains(j) implies p_scanned(p, j)
                        && self.slot_word(p, h, j) by {
                        assert(s0.slot_word(q, h, j));
                    }
                }
            } else {
                assert(self.perm(b) == s0.perm(b));
            }
        }
        assert forall|f: Pos| #[trigger] self.model@.edges.dom().contains(f) implies self.edge_wf(
            f,
        ) by {
            assert(s0.edge_wf(f));
            if f.0 != h {
                assert(self.perms@[f.0] == s0.perms@[f.0]);
            }
        }
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.counted_wf(b) by {
            assert(s0.counted_wf(b));
        }
    }

    /// 生きている物体の `meta` を、種類と scan を変えずに書いた後も、不変条件は保たれる (`set_lin`)。語とモデルは
    /// 変わらないので、`meta` から決まる形 (走査する語の位置と数、種類の条件) が同じなら、どの述語も前と同じである。
    pub(crate) proof fn lemma_meta_set(&self, s0: Core, h: usize)
        requires
            s0.full() || s0.safe_wf(),
            s0.dom().contains(h),
            s0.state(h) == BlockState::Live,
            self.meta_changed(s0, h),
            bits_wf(self.meta_bits(h)),
            bits_kind(self.meta_bits(h)) == bits_kind(s0.meta_bits(h)),
            bits_scan(self.meta_bits(h)) == bits_scan(s0.meta_bits(h)),
        ensures
            s0.full() ==> self.full(),
            s0.safe_wf() ==> self.safe_wf(),
            forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w),
            forall|b: usize, j: int| #[trigger] self.scanned(b, j) == s0.scanned(b, j),
            forall|b: usize| #[trigger] self.count(b) == s0.count(b),
            forall|b: usize| #[trigger] self.kind(b) == s0.kind(b),
            forall|b: usize| #[trigger] self.state(b) == s0.state(b),
            forall|b: usize| #[trigger] self.cap(b) == s0.cap(b),
            forall|b: usize| #[trigger] self.has(b) == s0.has(b),
            forall|b: usize| #[trigger] self.block(b) == s0.block(b),
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::refs_wf);
        reveal(Core::building_wf);
        reveal(Core::safe_blocks_wf);
        let p = self.perm(h);
        let q = s0.perm(h);
        assert(p_extended(p) == p_extended(q));
        assert(p_first(p) == p_first(q));
        assert(p_count(p) == p_count(q));
        assert(p_kind(p) == p_kind(q));
        assert forall|b: usize| #[trigger] self.has(b) == s0.has(b) by {}
        assert forall|b: usize, j: int| #[trigger] self.scanned(b, j) == s0.scanned(b, j) by {
            if b != h && s0.dom().contains(b) {
                assert(self.perms@[b] == s0.perms@[b]);
            }
        }
        assert forall|b: usize| #[trigger] self.count(b) == s0.count(b) by {
            if b != h && s0.dom().contains(b) {
                assert(self.perms@[b] == s0.perms@[b]);
            }
        }
        assert forall|b: usize| #[trigger] self.kind(b) == s0.kind(b) by {
            if b != h && s0.dom().contains(b) {
                assert(self.perms@[b] == s0.perms@[b]);
            }
        }
        assert forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w) by {}
        if s0.full() {
            assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.block_wf(b) by {
                assert(s0.block_wf(b));
                if b == h {
                    assert forall|j: int| #[trigger] p_scanned(p, j) implies self.slot_word(
                        p,
                        h,
                        j,
                    ) by {
                        assert(p_scanned(q, j));
                        assert(s0.slot_word(q, h, j));
                    }
                    if p_kind(p) == Kind::String {
                        assert(string_wf(q, s0.cap(h)));
                    }
                } else {
                    assert(self.perm(b) == s0.perm(b));
                }
            }
            assert forall|e: Pos| #[trigger] self.model@.kept.contains(e) implies self.kept_wf(
                e,
            ) by {
                assert(s0.kept_wf(e));
                if e.0 != h {
                    assert(self.perms@[e.0] == s0.perms@[e.0]);
                }
            }
            assert forall|f: Pos| #[trigger]
                self.model@.edges.dom().contains(f) implies self.edge_wf(f) by {
                assert(s0.edge_wf(f));
                if f.0 != h {
                    assert(self.perms@[f.0] == s0.perms@[f.0]);
                }
            }
            assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.counted_wf(
                b,
            ) by {
                assert(s0.counted_wf(b));
            }
        }
        if s0.safe_wf() {
            assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.safe_block_wf(
                b,
            ) by {
                assert(s0.safe_block_wf(b));
                if b == h {
                    assert forall|j: int| #[trigger] p_scanned(p, j) implies self.safe_slot(
                        p,
                        j,
                    ) by {
                        assert(p_scanned(q, j));
                        assert(s0.safe_slot(q, j));
                    }
                    if p_kind(p) == Kind::String {
                        assert(string_wf(q, s0.cap(h)));
                    }
                } else {
                    assert(self.perm(b) == s0.perm(b));
                }
            }
            assert forall|e: Pos| #[trigger] self.model@.kept.contains(e) implies self.safe_kept_wf(
                e,
            ) by {
                assert(s0.safe_kept_wf(e));
                if e.0 != h {
                    assert(self.perms@[e.0] == s0.perms@[e.0]);
                }
            }
            reveal(Core::safe_tokens_wf);
            let t = self.model@.tokens;
            assert forall|i: int|
                0 <= i < t.len() && (#[trigger] t[i]) is Some implies self.token_entry(t[i]->0) by {
                assert(s0.token_entry(t[i]->0));
                if (t[i]->0).0@.addr != h {
                    assert(self.perms@[(t[i]->0).0@.addr] == s0.perms@[(t[i]->0).0@.addr]);
                }
            }
        }
    }
}

} // verus!
