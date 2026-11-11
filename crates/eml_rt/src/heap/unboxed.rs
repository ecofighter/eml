//! 生の要素の配列の物体 (docs/spec/object-model.md の「種類」)。種類 `Raw` の物体は、ここの操作だけが作り、書く。
//! 語 0 が要素の数 n で、語 1..=n が生の 8 バイトの要素である。要素は走査しないので、どのビット列を置いてもよい。

use vstd::prelude::*;

#[cfg(verus_only)]
use super::inv::raw_wf;
use super::*;
#[cfg(verus_only)]
use crate::meta::{field_ptr, slot_offset};

verus! {

impl Core {
    /// `old` から、`Raw` の物体 `h` の語 0 (要素の数) より後ろの語だけが変わった。変わった語も書いてある。
    pub(crate) open spec fn elements_changed(&self, old: Core, h: usize) -> bool {
        let p = self.perm(h);
        let q = old.perm(h);
        &&& self.pages == old.pages
        &&& self.env == old.env
        &&& self.model == old.model
        &&& self.perms@.dom() == old.perms@.dom()
        &&& old.dom().contains(h)
        &&& forall|b: usize| b != h ==> #[trigger] self.perms@[b] == old.perms@[b]
        &&& p.rc == q.rc
        &&& p.meta == q.meta
        &&& p.prefix == q.prefix
        &&& p.text == q.text
        &&& p.slots.dom() == q.slots.dom()
        &&& p.slots[0] == q.slots[0]
        &&& forall|j: int| #[trigger]
            p.slots.contains_key(j) ==> p.slots[j].ptr() == q.slots[j].ptr()
        &&& forall|j: int| p_init(q, j) ==> #[trigger] p_init(p, j)
    }

    /// 要素の語を書く前と後は、どちらも `elements_changed` でつながる。
    pub(super) proof fn lemma_elements_trans(&self, mid: Core, old: Core, h: usize)
        requires
            mid.elements_changed(old, h),
            self.elements_changed(mid, h),
        ensures
            self.elements_changed(old, h),
    {
        let p = self.perm(h);
        let q = old.perm(h);
        assert forall|j: int| #[trigger] p.slots.contains_key(j) implies p.slots[j].ptr()
            == q.slots[j].ptr() by {
            assert(mid.perm(h).slots.contains_key(j));
        }
        assert forall|j: int| p_init(q, j) implies #[trigger] p_init(p, j) by {
            assert(p_init(mid.perm(h), j));
        }
    }

    /// 要素の語を書いた後も、不変条件は保たれる。`Raw` の物体は走査する語を持たないので、辺は変わらない。語 0 は
    /// 変わらず、どの語も書いてあるままなので、`raw_wf` も保たれる。
    proof fn lemma_unboxed_set(&self, s0: Core, h: usize)
        requires
            s0.full(),
            s0.state(h) == BlockState::Live,
            s0.kind(h) == Kind::Raw,
            self.elements_changed(s0, h),
        ensures
            self.full(),
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::refs_wf);
        reveal(Core::building_wf);
        let p = self.perm(h);
        let q = s0.perm(h);
        assert(s0.block_wf(h));
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.block_wf(b) by {
            assert(s0.block_wf(b));
            if b == h {
                assert forall|j: int| #[trigger]
                    p.slots.contains_key(j) <==> 0 <= j < self.cap(h) by {
                    assert(p.slots.contains_key(j) == q.slots.contains_key(j));
                }
                assert forall|j: int| 0 <= j < self.cap(h) implies (#[trigger] p.slots[j]).ptr()
                    == q.slots[j].ptr() by {
                    assert(p.slots.contains_key(j));
                }
                assert forall|j: int| 0 <= j < self.cap(h) implies p.slots.contains_key(j) && (
                #[trigger] p.slots[j]).is_init() by {
                    assert(p_init(q, j));
                    assert(p_init(p, j));
                }
                assert(raw_wf(p, self.cap(h)));
                assert forall|j: int| #[trigger] p_scanned(p, j) implies self.slot_word(
                    p,
                    h,
                    j,
                ) by {}
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

    /// 検査モードで、要素の語を書いた後も安全の条件は保たれる。
    proof fn lemma_safe_unboxed_set(&self, s0: Core, h: usize)
        requires
            s0.safe_wf(),
            s0.state(h) == BlockState::Live,
            s0.kind(h) == Kind::Raw,
            self.elements_changed(s0, h),
        ensures
            self.safe_wf(),
    {
        reveal(Core::safe_blocks_wf);
        reveal(Core::building_wf);
        let p = self.perm(h);
        let q = s0.perm(h);
        assert(s0.safe_block_wf(h));
        assert forall|j: int| #[trigger] p.slots.contains_key(j) <==> 0 <= j < self.cap(h) by {
            assert(p.slots.contains_key(j) == q.slots.contains_key(j));
        }
        assert forall|j: int| 0 <= j < self.cap(h) implies (#[trigger] p.slots[j]).ptr()
            == q.slots[j].ptr() by {
            assert(p.slots.contains_key(j));
        }
        assert forall|j: int| 0 <= j < self.cap(h) implies p.slots.contains_key(j) && (
        #[trigger] p.slots[j]).is_init() by {
            assert(p_init(q, j));
            assert(p_init(p, j));
        }
        assert(raw_wf(p, self.cap(h)));
        assert(self.safe_block_wf(h));
        // 生きている物体は「残した」語を持たない
        assert forall|j: int| #[trigger]
            self.model@.kept.contains((h, j)) implies self.safe_kept_wf((h, j)) by {
            assert(s0.safe_kept_wf((h, j)));
        }
        if s0.registered(h) {
            s0.lemma_registered(h);
        }
        self.lemma_safe_step(s0, h);
    }

    /// 生きている `Raw` の物体 `h` の j 語目 (j は 1 以上) に `v` を書く。
    fn put_element(&mut self, h: *mut Header, j: usize, v: Word)
        requires
            old(self).sound(),
            old(self).dom().contains(h@.addr),
            h == old(self).block(h@.addr),
            old(self).state(h@.addr) == BlockState::Live,
            old(self).kind(h@.addr) == Kind::Raw,
            1 <= j < old(self).cap(h@.addr),
        ensures
            final(self).elements_changed(*old(self), h@.addr),
            old(self).full() ==> final(self).full(),
            old(self).safe_wf() ==> final(self).safe_wf(),
            p_word(final(self).perm(h@.addr), j as int) == v,
            forall|k: int|
                k != j ==> #[trigger] final(self).perm(h@.addr).slots[k] == old(self).perm(
                    h@.addr,
                ).slots[k],
    {
        let ghost s0 = *self;
        self.slot_put(h, j, v);
        proof {
            let p = self.perm(h@.addr);
            let q = s0.perm(h@.addr);
            assert forall|k: int| p_init(q, k) implies #[trigger] p_init(p, k) by {
                if k != j {
                    assert(p.slots[k] == q.slots[k]);
                }
            }
            assert(self.elements_changed(s0, h@.addr));
            if s0.full() {
                self.lemma_unboxed_set(s0, h@.addr);
            }
            if s0.safe_wf() {
                self.lemma_safe_unboxed_set(s0, h@.addr);
            }
        }
    }
}

/// `Raw` のブロック `h` の要素 0 の位置 (語 0 が要素の数なので、ペイロードの語 1)。C に貸す位置で、ここからは何も
/// 読み書きせず、権限も変えない (docs/spec/object-model.md の「`unsafe` の核」の T16)。
#[verifier::external_body]
#[inline]
pub(crate) fn unboxed_payload(h: *mut Header, Tracked(perm): Tracked<&BlockPerm>) -> *mut u64
    requires
        perm.slots.contains_key(0),
        perm.slots[0].ptr() == field_ptr::<Word>(h, slot_offset(0)),
{
    slot_ptr(h, 1).cast::<u64>()
}

impl<M: Mode> Heap<M> {
    /// `w` は読める `Raw` の物体で、tag は 0 である。
    pub closed spec fn is_unboxed(&self, w: Word) -> bool {
        let h = w.bits_spec();
        &&& w.is_ptr_spec()
        &&& self.core.readable(w)
        &&& self.core.kind(h) == Kind::Raw
        &&& bits_tag(self.core.meta_bits(h)) == 0
    }

    /// `Raw` の物体 `w` の要素の数。
    pub closed spec fn unboxed_len_spec(&self, w: Word) -> int {
        self.word_at(w, 0).bits_spec() as int
    }

    /// `Raw` の物体 `w` の i 番目の要素のビット列。
    pub closed spec fn unboxed_at(&self, w: Word, i: int) -> u64 {
        self.word_at(w, 1 + i).bits_spec() as u64
    }

    /// `unboxed_put` と `unboxed_copy` の後の状態。`w` の要素の語だけが変わり、ほかは変わらない。
    pub closed spec fn unboxed_written(&self, old: Heap<M>, w: Word) -> bool {
        &&& self.dummy == old.dummy
        &&& self.region == old.region
        &&& self.worker == old.worker
        &&& self.core.elements_changed(old.core, w.bits_spec())
    }

    /// 生の要素の配列の spec の中身。`Ctx` の操作の証明が、物体の権限と結び付けるのに使う。
    pub(super) proof fn lemma_unboxed_spec(&self, w: Word)
        ensures
            self.is_unboxed(w) == (w.is_ptr_spec() && self.core.readable(w) && self.core.kind(
                w.bits_spec(),
            ) == Kind::Raw && bits_tag(self.core.meta_bits(w.bits_spec())) == 0),
            self.unboxed_len_spec(w) == p_word(self.core.perm(w.bits_spec()), 0).bits_spec() as int,
            forall|i: int| #[trigger]
                self.unboxed_at(w, i) == p_word(
                    self.core.perm(w.bits_spec()),
                    1 + i,
                ).bits_spec() as u64,
    {
    }

    /// `unboxed_written` の中身。
    pub(super) proof fn lemma_unboxed_written(&self, old: Heap<M>, w: Word)
        ensures
            self.unboxed_written(old, w) == (self.dummy == old.dummy && self.region == old.region
                && self.worker == old.worker && self.core.elements_changed(
                old.core,
                w.bits_spec(),
            )),
    {
    }

    /// `unboxed_written` は続けて書いた後もつながる。`Ctx` の操作が要素を1つずつ書くループで使う。
    pub(super) proof fn lemma_unboxed_written_trans(&self, mid: Heap<M>, old: Heap<M>, w: Word)
        requires
            mid.unboxed_written(old, w),
            self.unboxed_written(mid, w),
        ensures
            self.unboxed_written(old, w),
    {
        self.core.lemma_elements_trans(mid.core, old.core, w.bits_spec());
    }

    /// 生きているか構築中の `Raw` の物体は生きていて、`raw_wf` を満たす。`Raw` は構築中の物体にならない。
    pub(super) proof fn lemma_unboxed_block(&self, h: usize)
        requires
            self.core.sound(),
            self.core.has(h),
            self.core.kind(h) == Kind::Raw,
            self.core.state(h) == BlockState::Live || self.core.state(h) == BlockState::Building,
        ensures
            self.core.state(h) == BlockState::Live,
            raw_wf(self.core.perm(h), self.core.cap(h)),
            self.core.block(h)@.addr == h,
    {
        self.core.lemma_shape(h);
        if self.core.dom().contains(h) {
            if self.core.full() {
                self.core.lemma_at(h);
            } else {
                self.core.lemma_safe_at(h);
            }
        }
    }

    /// `make_unboxed` が書いたブロックは、生きている物体として正しい。
    proof fn lemma_raw_perm(&self, perm: BlockPerm, h: *mut Header, meta: Meta)
        requires
            h == self.core.block(h@.addr),
            perm.wf(h, self.core.cap(h@.addr), self.core.checked()),
            perm.prefix matches Some(q) ==> q.first.is_init() && q.state.is_init()
                && q.state.value() == ALIVE_STATE && q.first.value() == self.worker,
            perm.rc.is_init() && perm.rc.value() == 1,
            perm.meta.is_init() && perm.meta.value() == meta.bits_spec(),
            meta.kind_spec() == Kind::Raw,
            meta.scan_spec() == 0,
            bits_wf(meta.bits_spec()),
            bits_kind(meta.bits_spec()) == meta.kind_spec(),
            bits_scan(meta.bits_spec()) == meta.scan_spec(),
            p_filled(perm, self.core.cap(h@.addr)),
            p_word(perm, 0).bits_spec() + 1 <= self.core.cap(h@.addr),
        ensures
            self.core.perm_wf(perm, h@.addr, BlockState::Live, Set::empty(), self.worker),
            raw_wf(perm, self.core.cap(h@.addr)),
    {
        check::lemma_alive_code();
        assert(p_count(perm) == 0);
        assert(p_init(perm, 0));
        assert forall|i: int| #[trigger] p_scanned(perm, i) implies self.core.slot_word(
            perm,
            h@.addr,
            i,
        ) by {}
    }

    /// 長さ n の生の要素の配列を作る。要素はどれも `x` のビット列である。`lin` は `lin` ビットである。
    pub fn alloc_unboxed(&mut self, n: usize, x: u64, lin: bool) -> (w: Word)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            !M::CHECKED ==> old(self).settleable(),
        ensures
            final(self).made(*old(self), w),
            final(self).is_unboxed(w),
            final(self).unboxed_len_spec(w) == n,
            final(self).lin_of(w) == lin,
            final(self).owned(w),
            forall|i: int| 0 <= i < n ==> #[trigger] final(self).unboxed_at(w, i) == x,
            final(self).grew(*old(self), w),
            M::CHECKED ==> final(self).words_kept(*old(self)),
    {
        self.make_unboxed(n, x, lin, None)
    }

    /// `alloc_unboxed` の本体。`src` があれば、要素をその `Raw` の物体の語 1..=n から写す。`src` はほかのヒープの
    /// 物体でもよい (回復が使う)。
    pub(super) fn make_unboxed(
        &mut self,
        n: usize,
        x: u64,
        lin: bool,
        src: Option<(&Core, *mut Header)>,
    ) -> (w: Word)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            !M::CHECKED ==> old(self).settleable(),
            src matches Some((c, s)) ==> {
                &&& c.sound()
                &&& c.has(s@.addr)
                &&& s == c.block(s@.addr)
                &&& forall|j: int| 1 <= j <= n ==> #[trigger] p_init(c.perm(s@.addr), j)
            },
        ensures
            final(self).made(*old(self), w),
            final(self).is_unboxed(w),
            final(self).unboxed_len_spec(w) == n,
            final(self).lin_of(w) == lin,
            final(self).owned(w),
            forall|i: int|
                0 <= i < n ==> #[trigger] final(self).unboxed_at(w, i) == match src {
                    Some((c, s)) => p_word(c.perm(s@.addr), 1 + i).bits_spec() as u64,
                    None => x,
                },
            final(self).grew(*old(self), w),
            M::CHECKED ==> final(self).words_kept(*old(self)),
    {
        let payload_words = match n.checked_add(1) {
            Some(m) => m,
            None => too_large(),
        };
        self.settle();
        let ghost s1 = *self;
        let meta = Meta::new(0, 0, Kind::Raw, lin);
        let (h, Tracked(perm)) = self.alloc_header(payload_words, 1, meta);
        let tracked mut perm = perm;
        let ghost cap = self.core.cap(h@.addr);
        let tracked mut slot = perm.slots.tracked_remove(0);
        ptr_mut_write(slot_ptr(h, 0), Tracked(&mut slot), Word::from_bits(n));
        proof {
            perm.slots.tracked_insert(0, slot);
        }
        let ghost start = perm;
        let mut i: usize = 0;
        while i < n
            invariant
                perm.wf(h, cap, M::CHECKED),
                perm.rc == start.rc,
                perm.meta == start.meta,
                perm.prefix == start.prefix,
                perm.text == start.text,
                h@.addr + crate::layout::slot_offset_spec(cap as int) <= usize::MAX,
                n + 1 <= cap,
                0 <= i <= n,
                p_init(perm, 0) && p_word(perm, 0).bits_spec() == n,
                src matches Some((c, s)) ==> {
                    &&& c.sound()
                    &&& c.has(s@.addr)
                    &&& s == c.block(s@.addr)
                    &&& forall|j: int| 1 <= j <= n ==> #[trigger] p_init(c.perm(s@.addr), j)
                },
                forall|j: int|
                    0 <= j < i ==> #[trigger] p_init(perm, 1 + j) && p_word(
                        perm,
                        1 + j,
                    ).bits_spec() as u64 == match src {
                        Some((c, s)) => p_word(c.perm(s@.addr), 1 + j).bits_spec() as u64,
                        None => x,
                    },
            decreases n - i,
        {
            let v = match src {
                Some((c, s)) => {
                    proof {
                        assert(p_init(c.perm(s@.addr), i + 1));
                    }
                    c.slot_get(s, i + 1).bits()
                },
                None => x as usize,
            };
            let ghost want: u64 = match src {
                Some((c, s)) => p_word(c.perm(s@.addr), 1 + i).bits_spec() as u64,
                None => x,
            };
            proof {
                assert(v as u64 == want);
            }
            let ghost before = perm;
            let tracked mut slot = perm.slots.tracked_remove((i + 1) as int);
            ptr_mut_write(slot_ptr(h, i + 1), Tracked(&mut slot), Word::from_bits(v));
            proof {
                perm.slots.tracked_insert((i + 1) as int, slot);
                assert(perm.slots =~= before.slots.insert((i + 1) as int, slot));
                assert forall|j: int| 0 <= j < i + 1 implies #[trigger] p_init(perm, 1 + j)
                    && p_word(perm, 1 + j).bits_spec() as u64 == match src {
                    Some((c, s)) => p_word(c.perm(s@.addr), 1 + j).bits_spec() as u64,
                    None => x,
                } by {
                    if j != i {
                        assert(p_init(before, 1 + j));
                        assert(perm.slots[1 + j] == before.slots[1 + j]);
                    } else {
                        assert(perm.slots[1 + i] == slot);
                        assert(p_word(perm, 1 + i).bits_spec() == v);
                    }
                }
            }
            i += 1;
        }
        let cap_words = {
            let tracked hdr = self.core.pages.tracked_block_header(h@.addr);
            self.core.pages.cap_in(h, Tracked(hdr))
        };
        proof {
            // 確保器から取ったばかりのブロックは塊のブロックでないので、ペイロードの語の数は確保器のものである
            assert(self.core.pages.in_use().contains(h@.addr));
            assert(!self.core.region_has(h@.addr));
            assert(cap_words == cap);
        }
        let ghost written = perm;
        fill_zero(h, Tracked(&mut perm), n + 1, cap_words, Ghost(cap), Ghost(M::CHECKED));
        proof {
            assert(perm.slots[0] == written.slots[0]);
            assert forall|j: int| 0 <= j < cap implies #[trigger] p_init(perm, j) by {
                if 1 <= j < n + 1 {
                    assert(perm.slots[j] == written.slots[j]);
                    assert(p_init(written, 1 + (j - 1)));
                }
            }
            inv::lemma_filled(perm, cap);
        }
        let ghost mid = self.core;
        proof {
            self.lemma_raw_perm(perm, h, meta);
        }
        let ghost filled = perm;
        self.core.publish(Ghost(h), Tracked(perm), Ghost(BlockState::Live), Ghost(self.worker));
        let w = Word::from_ptr(h);
        proof {
            crate::word::lemma_aligned_is_ptr(h@.addr);
            assert forall|i: int| 0 <= i < n implies #[trigger] self.unboxed_at(w, i) == match src {
                Some((c, s)) => p_word(c.perm(s@.addr), 1 + i).bits_spec() as u64,
                None => x,
            } by {
                assert(self.core.perm(h@.addr) == filled);
                assert(p_init(written, 1 + i));
                assert(filled.slots[1 + i] == written.slots[1 + i]);
            }
            if old(self).consistent() && old(self).settleable() {
                self.lemma_dummy_kept(s1);
                self.core.lemma_frame_chain(mid, s1.core, old(self).core);
            } else {
                self.lemma_published_safe(s1);
            }
            assert forall|b: usize|
                #![trigger old(self).core.has(b)]
                old(self).core.has(b) && old(self).core.settled_state(b)
                    == BlockState::Live implies self.grown_block(*old(self), b) by {
                assert(s1.core.has(b));
                assert(b != h@.addr);
            }
        }
        w
    }

    /// 検査モードで、`Raw` で tag 0 の物体でなければ `WrongKind` を返す。速いモードでは何も確かめない。
    #[inline]
    fn checked_unboxed(&self, h: *mut Header) -> (r: Result<(), HeapError>)
        requires
            self.core.sound(),
            self.core.has(h@.addr),
            h == self.core.block(h@.addr),
        ensures
            !M::CHECKED ==> r is Ok,
            M::CHECKED ==> (r is Ok <==> (self.core.kind(h@.addr) == Kind::Raw && bits_tag(
                self.core.meta_bits(h@.addr),
            ) == 0)),
            r is Err ==> r == Err::<(), HeapError>(HeapError::WrongKind),
    {
        if M::CHECKED {
            let m = self.core.meta_get(h);
            if !matches!(m.kind(), Kind::Raw) || m.tag() != 0 {
                return Err(HeapError::WrongKind);
            }
        }
        Ok(())
    }

    /// 要素の数。
    ///
    /// # Safety
    /// `w` はこのヒープか付けた塊の生きている `Raw` の物体を指す (検査モードは確かめ、`Raw` でなければ `WrongKind` を
    /// 返す)。
    pub unsafe fn unboxed_len(&self, w: Word) -> (r: Result<usize, HeapError>)
        requires
            self.wf(),
            w.is_ptr_spec(),
            self.word_in(w),
            !M::CHECKED ==> self.settleable() && self.is_unboxed(w),
        ensures
            r matches Ok(n) ==> n == self.unboxed_len_spec(w),
            self.consistent() && self.settleable() && self.is_unboxed(w) ==> r is Ok,
            M::CHECKED ==> (r is Ok <==> self.is_unboxed(w)),
    {
        let h = self.header_of(w);
        proof {
            if !M::CHECKED || (self.consistent() && self.is_unboxed(w)) {
                self.core.lemma_readable(w);
            }
            if M::CHECKED {
                self.lemma_checked_readable(w);
            }
        }
        self.checked_live(h)?;
        self.checked_unboxed(h)?;
        proof {
            self.lemma_unboxed_block(h@.addr);
        }
        Ok(self.core.slot_get(h, 0).bits())
    }

    /// i 番目の要素のビット列を読む。
    ///
    /// # Safety
    /// `w` はこのヒープか付けた塊の生きている `Raw` の物体を指し、i は要素の数より小さい (検査モードは確かめ、`Raw`
    /// でなければ `WrongKind` を、範囲の外なら `OutOfBounds` を返す)。
    pub unsafe fn unboxed_word(&self, w: Word, i: usize) -> (r: Result<u64, HeapError>)
        requires
            self.wf(),
            w.is_ptr_spec(),
            self.word_in(w),
            !M::CHECKED ==> self.settleable() && self.is_unboxed(w) && i < self.unboxed_len_spec(w),
        ensures
            r matches Ok(x) ==> x == self.unboxed_at(w, i as int),
            self.consistent() && self.settleable() && self.is_unboxed(w) && i
                < self.unboxed_len_spec(w) ==> r is Ok,
            M::CHECKED ==> (r is Ok <==> self.is_unboxed(w) && i < self.unboxed_len_spec(w)),
    {
        let h = self.header_of(w);
        proof {
            if !M::CHECKED || (self.consistent() && self.is_unboxed(w)) {
                self.core.lemma_readable(w);
            }
            if M::CHECKED {
                self.lemma_checked_readable(w);
            }
        }
        self.checked_live(h)?;
        self.checked_unboxed(h)?;
        proof {
            self.lemma_unboxed_block(h@.addr);
        }
        let n = self.core.slot_get(h, 0).bits();
        if M::CHECKED && i >= n {
            return Err(HeapError::OutOfBounds);
        }
        Ok(self.core.slot_get(h, i + 1).bits() as u64)
    }

    /// i 番目の要素に `v` のビット列を書く。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている `Raw` の物体を指し、i は要素の数より小さく、ほかのスレッドはその物体を読めない。
    /// rc が正なら、持ち主は今の worker である (検査モードは確かめ、`Raw` でなければ `WrongKind` を、範囲の外なら
    /// `OutOfBounds` を、ほかのスレッドが読める物体なら `NotWritable` を返す)。
    pub unsafe fn unboxed_put(&mut self, w: Word, i: usize, v: u64) -> (r: Result<(), HeapError>)
        requires
            old(self).wf(),
            w.is_ptr_spec(),
            old(self).word_in(w),
            !M::CHECKED ==> old(self).writable(w) && old(self).is_unboxed(w) && i < old(
                self,
            ).unboxed_len_spec(w) && old(self).owned(w),
        ensures
            final(self).wf(),
            r is Err ==> *final(self) == *old(self),
            r is Ok ==> final(self).unboxed_written(*old(self), w),
            r is Ok ==> {
                &&& final(self).is_unboxed(w) == old(self).is_unboxed(w)
                &&& final(self).writable(w) == old(self).writable(w)
                &&& final(self).owned(w) == old(self).owned(w)
                &&& final(self).lin_of(w) == old(self).lin_of(w)
                &&& final(self).rc_value(w) == old(self).rc_value(w)
                &&& final(self).unboxed_len_spec(w) == old(self).unboxed_len_spec(w)
                &&& final(self).unboxed_at(w, i as int) == v
                &&& forall|j: int|
                    j != i ==> #[trigger] final(self).unboxed_at(w, j) == old(self).unboxed_at(w, j)
            },
            M::CHECKED ==> final(self).words_kept(*old(self)),
            old(self).consistent() && old(self).writable(w) && old(self).is_unboxed(w) && i < old(
                self,
            ).unboxed_len_spec(w) && old(self).owned(w) ==> r is Ok && final(self).consistent(),
    {
        let h = self.header_of(w);
        let ghost c = self.consistent() && self.writable(w) && self.is_unboxed(w) && i
            < self.unboxed_len_spec(w) && self.owned(w);
        proof {
            self.lemma_checks_pass(w);
        }
        self.checked_live(h)?;
        self.checked_unboxed(h)?;
        proof {
            self.lemma_unboxed_block(h@.addr);
        }
        if M::CHECKED {
            let n = self.core.slot_get(h, 0).bits();
            if i >= n {
                return Err(HeapError::OutOfBounds);
            }
            self.checked_unshared(w, h)?;
        }
        let ghost s0 = self.core;
        proof {
            // 書き込める物体 (`unshared`) は、rc 0 の塊の物体でない
            if !s0.dom().contains(h@.addr) {
                s0.lemma_region_at(h@.addr);
            }
        }
        self.core.put_element(h, i + 1, Word::from_bits(v as usize));
        proof {
            if c {
                // ダミーは `BigInt` なので、書いた物体でない
                assert(s0.kind(self.dummy.bits_spec()) == Kind::BigInt);
                assert(self.core.perms@[self.dummy.bits_spec()]
                    == s0.perms@[self.dummy.bits_spec()]);
            }
            assert(self.core.perm(h@.addr).slots[0] == s0.perm(h@.addr).slots[0]);
            assert forall|j: int| j != i implies #[trigger] self.unboxed_at(w, j) == old(
                self,
            ).unboxed_at(w, j) by {
                assert(self.core.perm(h@.addr).slots[1 + j] == s0.perm(h@.addr).slots[1 + j]);
            }
            if M::CHECKED {
                self.lemma_safe_frame(*old(self));
            }
        }
        Ok(())
    }

    /// 要素 0 の位置。C の関数に要素を読ませるために貸す (docs/spec/object-model.md の「配列」)。
    ///
    /// # Safety
    /// `w` はこのヒープか付けた塊の生きている `Raw` の物体を指す。返す位置から要素の数だけ読んでよいのは、物体が生きて
    /// いて、書き換えていない間だけである (検査モードは確かめ、`Raw` でなければ `WrongKind` を返す)。
    pub unsafe fn unboxed_ptr(&self, w: Word) -> (r: Result<*const u64, HeapError>)
        requires
            self.wf(),
            w.is_ptr_spec(),
            self.word_in(w),
            !M::CHECKED ==> self.settleable() && self.is_unboxed(w),
        ensures
            self.consistent() && self.settleable() && self.is_unboxed(w) ==> r is Ok,
            M::CHECKED ==> (r is Ok <==> self.is_unboxed(w)),
    {
        let h = self.header_of(w);
        proof {
            if !M::CHECKED || (self.consistent() && self.is_unboxed(w)) {
                self.core.lemma_readable(w);
            }
            if M::CHECKED {
                self.lemma_checked_readable(w);
            }
        }
        self.checked_live(h)?;
        self.checked_unboxed(h)?;
        proof {
            self.lemma_unboxed_block(h@.addr);
            self.core.lemma_shape(h@.addr);
        }
        let tracked p = self.core.tracked_perm(h@.addr);
        Ok(unboxed_payload(h, Tracked(p)) as *const u64)
    }

    /// 要素 0 の位置。C の関数に要素を書かせるために貸す (docs/spec/object-model.md の「配列」)。
    ///
    /// ヒープの何も変えないので `&self` を取る。`&Ctx` しか持たない呼び手 (VM が C の関数の引数を組むところ) からも
    /// 貸せるようにするためである。書き込みの一意は Rust の借用でなく、下の前提と検査モードの `NotWritable` が守る。
    /// 返す位置の来歴はヒープのページの確保から来るので、`&Heap` への共有の参照を通して書くことにはならない。
    ///
    /// # Safety
    /// 添字を除いて `unboxed_put` と同じである。`w` はこのヒープの生きている `Raw` の物体を指し、ほかのスレッドはその
    /// 物体を読めない。rc が正なら、持ち主は今の worker である (検査モードは確かめ、`Raw` でなければ `WrongKind` を、
    /// ほかのスレッドが読める物体なら `NotWritable` を返す)。返す位置から要素の数だけ読み書きしてよいのは、物体を生かし、
    /// 一意に保ち、このヒープの操作でその物体を読み書きしない間だけである。
    pub unsafe fn unboxed_ptr_mut(&self, w: Word) -> (r: Result<*mut u64, HeapError>)
        requires
            self.wf(),
            w.is_ptr_spec(),
            self.word_in(w),
            !M::CHECKED ==> self.writable(w) && self.is_unboxed(w) && self.owned(w),
        ensures
            M::CHECKED && r is Ok ==> self.is_unboxed(w) && self.unshared(w),
            self.consistent() && self.writable(w) && self.is_unboxed(w) && self.owned(w)
                ==> r is Ok,
    {
        let h = self.header_of(w);
        proof {
            self.lemma_checks_pass(w);
            if M::CHECKED {
                self.lemma_checked_readable(w);
            }
        }
        self.checked_live(h)?;
        self.checked_unboxed(h)?;
        proof {
            self.lemma_unboxed_block(h@.addr);
            self.core.lemma_shape(h@.addr);
        }
        if M::CHECKED {
            self.checked_unshared(w, h)?;
        }
        let tracked p = self.core.tracked_perm(h@.addr);
        Ok(unboxed_payload(h, Tracked(p)))
    }

    /// `src` の要素 0..n を `dst` の要素 0..n に写す。`dst` のほかの要素と `src` は変えない。
    ///
    /// # Safety
    /// `dst` はこのヒープの生きている `Raw` の物体を指し、ほかのスレッドはその物体を読めない。rc が正なら、持ち主は今の
    /// worker である。`src` はこのヒープか付けた塊の生きている `Raw` の物体で、`dst` と別の物体である。n はどちらの要素の
    /// 数も超えない (検査モードは確かめ、`Raw` でなければ `WrongKind` を、n が要素の数を超えれば `OutOfBounds` を、
    /// `dst` をほかのスレッドが読めるなら `NotWritable` を返す)。
    pub unsafe fn unboxed_copy(&mut self, dst: Word, src: Word, n: usize) -> (r: Result<
        (),
        HeapError,
    >)
        requires
            old(self).wf(),
            dst.is_ptr_spec(),
            src.is_ptr_spec(),
            dst.bits_spec() != src.bits_spec(),
            old(self).word_in(dst),
            old(self).word_in(src),
            !M::CHECKED ==> old(self).writable(dst) && old(self).is_unboxed(dst) && old(
                self,
            ).is_unboxed(src) && old(self).owned(dst) && n <= old(self).unboxed_len_spec(dst) && n
                <= old(self).unboxed_len_spec(src),
        ensures
            final(self).wf(),
            r is Err ==> *final(self) == *old(self),
            r is Ok ==> final(self).unboxed_written(*old(self), dst),
            r is Ok ==> {
                &&& final(self).is_unboxed(dst) == old(self).is_unboxed(dst)
                &&& final(self).writable(dst) == old(self).writable(dst)
                &&& final(self).owned(dst) == old(self).owned(dst)
                &&& final(self).lin_of(dst) == old(self).lin_of(dst)
                &&& final(self).rc_value(dst) == old(self).rc_value(dst)
                &&& final(self).is_unboxed(src) == old(self).is_unboxed(src)
                &&& final(self).unboxed_len_spec(dst) == old(self).unboxed_len_spec(dst)
                &&& final(self).unboxed_len_spec(src) == old(self).unboxed_len_spec(src)
                &&& forall|j: int|
                    0 <= j < n ==> #[trigger] final(self).unboxed_at(dst, j) == old(
                        self,
                    ).unboxed_at(src, j)
                &&& forall|j: int|
                    !(0 <= j < n) ==> #[trigger] final(self).unboxed_at(dst, j) == old(
                        self,
                    ).unboxed_at(dst, j)
                &&& forall|j: int| #[trigger]
                    final(self).unboxed_at(src, j) == old(self).unboxed_at(src, j)
            },
            M::CHECKED ==> final(self).words_kept(*old(self)),
            old(self).consistent() && old(self).writable(dst) && old(self).is_unboxed(dst) && old(
                self,
            ).is_unboxed(src) && old(self).owned(dst) && n <= old(self).unboxed_len_spec(dst) && n
                <= old(self).unboxed_len_spec(src) ==> r is Ok && final(self).consistent(),
    {
        let hd = self.header_of(dst);
        let hs = self.header_of(src);
        let ghost c = self.consistent() && self.writable(dst) && self.is_unboxed(dst)
            && self.is_unboxed(src) && self.owned(dst) && n <= self.unboxed_len_spec(dst) && n
            <= self.unboxed_len_spec(src);
        proof {
            self.lemma_checks_pass(dst);
            if !M::CHECKED || c {
                self.core.lemma_readable(src);
            }
            if M::CHECKED {
                self.lemma_checked_readable(src);
            }
        }
        self.checked_live(hd)?;
        self.checked_unboxed(hd)?;
        self.checked_live(hs)?;
        self.checked_unboxed(hs)?;
        proof {
            self.lemma_unboxed_block(hd@.addr);
            self.lemma_unboxed_block(hs@.addr);
        }
        if M::CHECKED {
            if n > self.core.slot_get(hd, 0).bits() || n > self.core.slot_get(hs, 0).bits() {
                return Err(HeapError::OutOfBounds);
            }
            self.checked_unshared(dst, hd)?;
        }
        let ghost s0 = self.core;
        proof {
            // 書き込める物体 (`unshared`) は、rc 0 の塊の物体でない
            if !s0.dom().contains(hd@.addr) {
                s0.lemma_region_at(hd@.addr);
            }
            assert(s0.elements_changed(s0, hd@.addr));
        }
        let mut k: usize = 0;
        while k < n
            invariant
                self.dummy == old(self).dummy,
                self.region == old(self).region,
                self.worker == old(self).worker,
                self.tokens == old(self).tokens,
                self.building == old(self).building,
                s0 == old(self).core,
                s0.sound(),
                s0.dom().contains(hd@.addr),
                hd == s0.block(hd@.addr),
                hs == s0.block(hs@.addr),
                s0.has(hs@.addr),
                hd@.addr != hs@.addr,
                s0.state(hd@.addr) == BlockState::Live,
                s0.kind(hd@.addr) == Kind::Raw,
                raw_wf(s0.perm(hd@.addr), s0.cap(hd@.addr)),
                raw_wf(s0.perm(hs@.addr), s0.cap(hs@.addr)),
                n <= p_word(s0.perm(hd@.addr), 0).bits_spec(),
                n <= p_word(s0.perm(hs@.addr), 0).bits_spec(),
                0 <= k <= n,
                self.core.elements_changed(s0, hd@.addr),
                s0.full() ==> self.core.full(),
                s0.safe_wf() ==> self.core.safe_wf(),
                forall|j: int|
                    1 <= j <= k ==> #[trigger] p_word(self.core.perm(hd@.addr), j).bits_spec()
                        == p_word(s0.perm(hs@.addr), j).bits_spec(),
                forall|j: int|
                    !(1 <= j <= k) ==> #[trigger] self.core.perm(hd@.addr).slots[j] == s0.perm(
                        hd@.addr,
                    ).slots[j],
            decreases n - k,
        {
            let ghost s1 = self.core;
            proof {
                // 写す元は `dst` でないので、書いても変わらない
                assert(self.core.perm(hs@.addr) == s0.perm(hs@.addr));
                assert(p_init(s0.perm(hs@.addr), k + 1));
                assert(self.core.perm(hd@.addr).meta == s0.perm(hd@.addr).meta);
            }
            let x = self.core.slot_get(hs, k + 1).bits();
            self.core.put_element(hd, k + 1, Word::from_bits(x));
            proof {
                self.core.lemma_elements_trans(s1, s0, hd@.addr);
                let p = self.core.perm(hd@.addr);
                let q = s1.perm(hd@.addr);
                assert forall|j: int| 1 <= j <= k + 1 implies #[trigger] p_word(p, j).bits_spec()
                    == p_word(s0.perm(hs@.addr), j).bits_spec() by {
                    if j != k + 1 {
                        assert(p.slots[j] == q.slots[j]);
                        assert(p_word(q, j).bits_spec() == p_word(
                            s0.perm(hs@.addr),
                            j,
                        ).bits_spec());
                    } else {
                        assert(p_word(p, j).bits_spec() == x);
                        assert(x == p_word(s1.perm(hs@.addr), j).bits_spec());
                    }
                }
                assert forall|j: int| !(1 <= j <= k + 1) implies #[trigger] p.slots[j] == s0.perm(
                    hd@.addr,
                ).slots[j] by {
                    assert(p.slots[j] == q.slots[j]);
                }
            }
            k += 1;
        }
        proof {
            let p = self.core.perm(hd@.addr);
            let q = s0.perm(hd@.addr);
            assert(self.core.perm(hs@.addr) == s0.perm(hs@.addr));
            if c {
                // ダミーは `BigInt` なので、書いた物体でない
                assert(s0.kind(self.dummy.bits_spec()) == Kind::BigInt);
                assert(self.core.perms@[self.dummy.bits_spec()]
                    == s0.perms@[self.dummy.bits_spec()]);
            }
            assert(p.slots[0] == q.slots[0]);
            assert forall|j: int| 0 <= j < n implies #[trigger] self.unboxed_at(dst, j) == old(
                self,
            ).unboxed_at(src, j) by {
                assert(p_word(p, 1 + j).bits_spec() == p_word(
                    s0.perm(hs@.addr),
                    1 + j,
                ).bits_spec());
            }
            assert forall|j: int| !(0 <= j < n) implies #[trigger] self.unboxed_at(dst, j) == old(
                self,
            ).unboxed_at(dst, j) by {
                assert(p.slots[1 + j] == q.slots[1 + j]);
            }
            if M::CHECKED {
                self.lemma_safe_frame(*old(self));
            }
        }
        Ok(())
    }
}

} // verus!
