//! ヒープの証明の部分 (`Core`) の読み書き。ブロックの欄を権限の上で読み書きし、RC の遷移のモデルの側を更新する。

use vstd::prelude::*;
#[cfg(verus_only)]
use vstd::raw_ptr::PointsTo;
use vstd::raw_ptr::{ptr_mut_write, ptr_ref};

use super::Core;
#[cfg(verus_only)]
use super::inv::{
    p_code, p_count, p_first, p_init, p_owner, p_scanned, p_word, prefix_kept, prefix_recounted,
    state_code,
};
use super::model::Pos;
#[cfg(verus_only)]
use super::model::{BlockState, Model, lemma_indeg_pos};
#[cfg(verus_only)]
use crate::alloc::{FREE_META, PageHeader, header_cap, header_ok, page_base};
use crate::cell::{
    rc_decref_shared, rc_dup_shared, rc_is_last, rc_load, rc_store, u32_load, u32_store,
};
#[cfg(verus_only)]
use crate::meta::{BlockPerm, field_ptr};
use crate::meta::{Header, Meta, meta_ptr, prefix_first_ptr, prefix_state_ptr, rc_ptr, slot_ptr};
#[cfg(verus_only)]
use crate::meta::{bits_kind, bits_scan, bits_tag};
#[cfg(verus_only)]
use crate::rc::{Obj, decref_next, dup_next, lemma_decref, lemma_dup};
use crate::word::Word;

verus! {

impl Core {
    /// ブロック `h` の位置と大きさの事実。
    pub(crate) proof fn lemma_at(&self, h: usize)
        requires
            self.full(),
            self.dom().contains(h),
        ensures
            self.block_wf(h),
            self.pages.carved_block(h),
            self.block(h)@.addr == h,
            h % 8 == 0,
            h >= 8,
            h + 8 * (1 + self.cap(h)) <= usize::MAX,
    {
        reveal(Core::blocks_wf);
        assert(self.block_wf(h));
        self.pages.lemma_block_bounds(h);
    }

    /// ブロック `h` の権限を借りる。このヒープのブロックなら自分の権限から、塊のブロックなら塊の権限の写しから借りる。
    /// どちらから借りるかは ghost の条件で決まるので、実行の時の分岐はない。塊の権限からは読むことしかできない。
    pub(crate) proof fn tracked_perm(tracked &self, h: usize) -> (tracked p: &BlockPerm)
        requires
            self.region_wf(),
            self.has(h),
        ensures
            *p == self.perm(h),
    {
        if self.dom().contains(h) {
            self.perms.borrow().tracked_borrow(h)
        } else {
            self.env.borrow().tracked_region().tracked_perm(h, self.checked())
        }
    }

    /// 確保器が切り出したブロック `h` は、付けた塊のブロックでない。`h` のページの頭の権限は確保器が持ち、塊のページの頭の
    /// 権限とは別のものだからである。
    pub(crate) proof fn lemma_not_region(tracked &mut self, h: usize)
        requires
            old(self).pages.wf(),
            old(self).pages.carved_block(h),
            old(self).region_wf(),
        ensures
            *final(self) == *old(self),
            !final(self).region_has(h),
    {
        if self.env@.region is Some {
            self.pages.lemma_carved_page(h);
            let tracked r = self.env.borrow().tracked_region();
            self.pages.lemma_page_apart(page_base(h), r, self.checked());
            if r.has(h) {
                r.lemma_block(h, self.checked());
            }
        }
    }

    #[inline]
    pub(crate) fn rc_get(&self, h: *mut Header) -> (r: i32)
        requires
            self.sound(),
            self.has(h@.addr),
            h == self.block(h@.addr),
        ensures
            r == self.rc_of(h@.addr),
    {
        proof {
            self.lemma_shape(h@.addr);
        }
        let tracked p = self.tracked_perm(h@.addr);
        rc_load(rc_ptr(h), Tracked(&p.rc))
    }

    #[inline]
    pub(crate) fn meta_get(&self, h: *mut Header) -> (r: Meta)
        requires
            self.sound(),
            self.has(h@.addr),
            h == self.block(h@.addr),
        ensures
            r.bits_spec() == self.meta_bits(h@.addr),
            r.kind_spec() == self.kind(h@.addr),
            r.scan_spec() == bits_scan(self.meta_bits(h@.addr)),
            r.lin_spec() == self.lin(h@.addr),
            r.tag_spec() == bits_tag(self.meta_bits(h@.addr)),
    {
        proof {
            self.lemma_shape(h@.addr);
        }
        let tracked p = self.tracked_perm(h@.addr);
        let bits = *ptr_ref(meta_ptr(h), Tracked(&p.meta));
        Meta::from_bits(bits)
    }

    /// 切り出したブロック `h` のヘッダの `meta`。ヒープのブロックでなければ空きリストにあるので、空いた印である。
    #[inline]
    pub(crate) fn carved_meta(&self, h: *mut Header) -> (r: u32)
        requires
            self.sound(),
            self.pages.carved_block(h@.addr),
            h == self.pages.block_ptr(h@.addr),
        ensures
            self.dom().contains(h@.addr) ==> r == self.meta_bits(h@.addr),
            !self.dom().contains(h@.addr) ==> r == FREE_META,
    {
        proof {
            if self.dom().contains(h@.addr) {
                self.lemma_shape(h@.addr);
            }
        }
        let tracked held = if self.dom().contains(h@.addr) {
            Some(self.perms.borrow().tracked_borrow(h@.addr))
        } else {
            None
        };
        self.pages.read_meta(h, Tracked(held))
    }

    /// 語を読む。
    #[inline]
    pub(crate) fn slot_get(&self, h: *mut Header, i: usize) -> (r: Word)
        requires
            self.sound(),
            self.has(h@.addr),
            h == self.block(h@.addr),
            p_init(self.perm(h@.addr), i as int),
        ensures
            r == p_word(self.perm(h@.addr), i as int),
    {
        proof {
            self.lemma_shape(h@.addr);
            assert(self.perm(h@.addr).slots.contains_key(i as int));
        }
        let tracked p = self.tracked_perm(h@.addr);
        *ptr_ref(slot_ptr(h, i), Tracked(p.slots.tracked_borrow(i as int)))
    }

    /// 走査する語の始まりと数。
    #[inline]
    pub(crate) fn scan_get(&self, h: *mut Header) -> (r: (usize, usize))
        requires
            self.sound(),
            self.has(h@.addr),
            h == self.block(h@.addr),
            self.state(h@.addr) != BlockState::Freed,
        ensures
            r.0 == p_first(self.perm(h@.addr)),
            r.1 == p_count(self.perm(h@.addr)),
            r.0 + r.1 <= self.cap(h@.addr),
    {
        proof {
            self.lemma_shape(h@.addr);
        }
        let m = self.meta_get(h);
        if m.scan() == Meta::EXTENDED_SCAN {
            (1, self.slot_get(h, 0).bits())
        } else {
            (0, m.scan() as usize)
        }
    }

    /// rc が 1 か -1 か。
    #[inline]
    pub(crate) fn rc_last(&self, h: *mut Header) -> (r: bool)
        requires
            self.sound(),
            self.has(h@.addr),
            h == self.block(h@.addr),
        ensures
            r == (self.rc_of(h@.addr) == 1 || self.rc_of(h@.addr) == -1),
    {
        proof {
            self.lemma_shape(h@.addr);
        }
        let tracked p = self.tracked_perm(h@.addr);
        rc_is_last(rc_ptr(h), Tracked(&p.rc))
    }

    /// ブロック `h` のページの頭の権限を借りる。`tracked_perm` と同じく、借りる先を ghost の条件で選ぶ。
    pub(crate) proof fn tracked_header(tracked &self, h: usize) -> (tracked r: &PointsTo<
        PageHeader,
    >)
        requires
            self.sound(),
            self.has(h),
        ensures
            r.ptr()@.addr == page_base(h),
            r.ptr()@.provenance == self.block(h)@.provenance,
            r.is_init(),
            header_ok(r.value(), self.pages.prefixed()),
            header_cap(r.value(), self.pages.prefixed()) == self.cap(h),
    {
        self.lemma_shape(h);
        if self.in_region(h) {
            let tracked r = self.env.borrow().tracked_region();
            r.lemma_block(h, self.checked());
            r.lemma_owner(h, self.checked());
            assert(r.owner(h).headers.contains_key(page_base(h)));
            r.tracked_header(h, self.checked())
        } else {
            self.pages.tracked_block_header(h)
        }
    }

    /// ペイロードの語の数。
    #[inline]
    pub(crate) fn cap_get(&self, h: *mut Header) -> (r: usize)
        requires
            self.sound(),
            self.has(h@.addr),
            h == self.block(h@.addr),
        ensures
            r == self.cap(h@.addr),
    {
        let tracked hdr = self.tracked_header(h@.addr);
        self.pages.cap_in(h, Tracked(hdr))
    }

    /// i 語目に `v` を書く。モデルはまだ替えない。
    #[inline]
    pub(crate) fn slot_put(&mut self, h: *mut Header, i: usize, v: Word)
        requires
            old(self).sound(),
            old(self).dom().contains(h@.addr),
            h == old(self).block(h@.addr),
            i < old(self).cap(h@.addr),
        ensures
            final(self).slot_changed(*old(self), h@.addr, i as int),
            p_word(final(self).perm(h@.addr), i as int) == v,
            final(self).model == old(self).model,
    {
        let ghost s0 = *self;
        proof {
            self.lemma_shape(h@.addr);
        }
        let tracked mut p = self.perms.borrow_mut().tracked_remove(h@.addr);
        let tracked mut slot = p.slots.tracked_remove(i as int);
        ptr_mut_write(slot_ptr(h, i), Tracked(&mut slot), v);
        proof {
            p.slots.tracked_insert(i as int, slot);
            self.perms.borrow_mut().tracked_insert(h@.addr, p);
            assert(self.perms@.dom() =~= s0.perms@.dom());
            assert(self.perm(h@.addr).slots.dom() =~= s0.perm(h@.addr).slots.dom());
        }
    }

    /// 検査モードの前置きの状態の欄を読む。
    #[inline]
    pub(crate) fn prefix_state_get(&self, h: *mut Header) -> (r: u32)
        requires
            self.sound(),
            self.has(h@.addr),
            h == self.block(h@.addr),
            self.checked(),
        ensures
            self.perm(h@.addr).prefix is Some,
            r == self.perm(h@.addr).prefix->0.state.value(),
    {
        proof {
            self.lemma_shape(h@.addr);
        }
        let tracked p = self.tracked_perm(h@.addr);
        let tracked q = p.prefix.tracked_borrow();
        u32_load(prefix_state_ptr(h), Tracked(&q.state))
    }

    /// 検査モードの前置きの `first` の欄 (所有者 ID か不死の物体の数) を読む。
    #[inline]
    pub(crate) fn prefix_first_get(&self, h: *mut Header) -> (r: u32)
        requires
            self.sound(),
            self.has(h@.addr),
            h == self.block(h@.addr),
            self.checked(),
        ensures
            self.perm(h@.addr).prefix is Some,
            r == p_owner(self.perm(h@.addr)),
    {
        proof {
            self.lemma_shape(h@.addr);
        }
        let tracked p = self.tracked_perm(h@.addr);
        let tracked q = p.prefix.tracked_borrow();
        u32_load(prefix_first_ptr(h), Tracked(&q.first))
    }

    /// 検査モードの前置きの状態は、ブロックの状態を表す。
    pub(crate) proof fn lemma_code_state(&self, h: usize)
        requires
            self.sound(),
            self.has(h),
            self.checked(),
        ensures
            self.perm(h).prefix is Some,
            p_code(self.perm(h)) == state_code(self.state(h)),
            self.full() && (self.state(h) == BlockState::Live || self.state(h)
                == BlockState::Building) && self.rc_of(h) > 0 ==> p_owner(self.perm(h))
                == self.model@.owner[h],
    {
        self.lemma_shape(h);
        if !self.dom().contains(h) {
            self.lemma_region_at(h);
        } else if self.full() {
            self.lemma_at(h);
        }
    }

    /// 不死の物体の外に出ている参照の数 (前置きの `first` の欄) のほかは変わらない。
    pub(crate) open spec fn recounted(&self, old: Core) -> bool {
        &&& self.pages == old.pages
        &&& self.env == old.env
        &&& self.model == old.model
        &&& self.perms@.dom() == old.perms@.dom()
        &&& forall|b: usize| #[trigger]
            old.dom().contains(b) ==> {
                &&& self.perm(b).rc == old.perm(b).rc
                &&& self.perm(b).meta == old.perm(b).meta
                &&& self.perm(b).slots == old.perm(b).slots
                &&& self.perm(b).text == old.perm(b).text
                &&& prefix_recounted(self.perm(b), old.perm(b), old.rc_of(b) == 0)
            }
    }

    /// 不死の物体の数を替えても、不変条件は保たれる。数は rc が 0 の物体の前置きにしかなく、ほかの不変条件は読まない。
    pub(crate) proof fn lemma_recounted(&self, s0: Core)
        requires
            s0.full(),
            self.recounted(s0),
        ensures
            self.full(),
            forall|w: Word| #[trigger] self.word_ok(w) == s0.word_ok(w),
            forall|b: usize|
                #![trigger s0.dom().contains(b)]
                s0.dom().contains(b) ==> s0.rc_of(b) != 0 ==> self.perm(b) == s0.perm(b),
    {
        reveal(Core::blocks_wf);
        reveal(Core::edges_wf);
        reveal(Core::refs_wf);
        reveal(Core::building_wf);
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.block_wf(b) by {
            assert(s0.block_wf(b));
            let p = self.perm(b);
            let q = s0.perm(b);
            if s0.rc_of(b) != 0 {
                assert(p == q);
            }
            if s0.state(b) == BlockState::Live {
                assert forall|i: int| #[trigger] p_scanned(p, i) implies self.slot_word(
                    p,
                    b,
                    i,
                ) by {
                    assert(p_scanned(q, i));
                    assert(s0.slot_word(q, b, i));
                }
            }
            if s0.state(b) == BlockState::Building {
                assert forall|i: int| #[trigger]
                    self.model@.written[b].contains(i) implies p_scanned(p, i) && self.slot_word(
                    p,
                    b,
                    i,
                ) && !self.model@.kept.contains((b, i)) by {
                    assert(s0.slot_word(q, b, i));
                }
            }
            if s0.state(b) == BlockState::Dying || s0.state(b) == BlockState::Token {
                assert forall|i: int| #[trigger] p_scanned(p, i) implies p_init(p, i) by {
                    assert(p_scanned(q, i));
                }
            }
        }
        assert forall|e: Pos| #[trigger] self.model@.kept.contains(e) implies self.kept_wf(e) by {
            assert(s0.kept_wf(e));
        }
        assert forall|e: Pos| #[trigger] self.model@.edges.dom().contains(e) implies self.edge_wf(
            e,
        ) by {
            assert(s0.edge_wf(e));
        }
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.counted_wf(b) by {
            assert(s0.counted_wf(b));
        }
        assert forall|b: usize|
            #![trigger s0.dom().contains(b)]
            s0.dom().contains(b) && s0.rc_of(b) != 0 implies self.perm(b) == s0.perm(b) by {
            assert(self.perm(b) == s0.perm(b));
        }
    }

    pub(crate) proof fn lemma_recounted_refl(&self)
        requires
            self.sound(),
        ensures
            self.recounted(*self),
    {
        assert forall|b: usize| #[trigger] self.dom().contains(b) implies prefix_recounted(
            self.perm(b),
            self.perm(b),
            self.rc_of(b) == 0,
        ) by {
            self.lemma_shape(b);
        }
    }

    /// 不死の物体の数を替えても、`evolved` は変わらない。
    pub(crate) proof fn lemma_recounted_evolved(&self, mid: Core, old: Core, w: usize, delta: int)
        requires
            mid.evolved(old, w, delta),
            self.recounted(mid),
        ensures
            self.evolved(old, w, delta),
    {
        assert forall|b: usize|
            #![trigger old.dom().contains(b)]
            old.dom().contains(b) && old.settled_state(b) == BlockState::Live && (old.rc_of(b) == 0
                || old.ext(b) + (if b == w {
                delta
            } else {
                0
            }) >= 1) implies {
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
        } by {
            assert(mid.dom().contains(b));
        }
    }

    /// 参照を手放した後に不死の物体の数を減らしても、`released` は変わらない。
    pub(crate) proof fn lemma_recounted_released(
        &self,
        mid: Core,
        old: Core,
        h: usize,
        edge: Option<Pos>,
    )
        requires
            mid.full(),
            mid.released(old, h, edge, false),
            mid.dom().contains(h),
            self.recounted(mid),
            forall|b: usize| b != h ==> #[trigger] self.perms@[b] == mid.perms@[b],
        ensures
            self.full(),
            self.released(old, h, edge, false),
    {
        self.lemma_recounted(mid);
        assert(mid.dom().contains(h));
        assert forall|b: usize| b != h implies #[trigger] self.state(b) == old.state(b) by {
            assert(mid.state(b) == old.state(b));
        }
        self.lemma_recounted_evolved(
            mid,
            old,
            h,
            if edge is None {
                -1
            } else {
                0
            },
        );
    }

    /// 検査モードの前置きの状態の欄を書く。モデルはまだ替えない。
    #[inline]
    pub(crate) fn prefix_state_put(&mut self, h: *mut Header, v: u32)
        requires
            old(self).perms@.dom().contains(h@.addr),
            old(self).perm(h@.addr).prefix is Some,
            old(self).perm(h@.addr).prefix->0.state.ptr() == field_ptr::<u32>(h, -4),
            h@.addr >= 8,
        ensures
            final(self).pages == old(self).pages,
            final(self).env == old(self).env,
            final(self).model == old(self).model,
            final(self).perms@.dom() == old(self).perms@.dom(),
            forall|b: usize|
                b != h@.addr ==> #[trigger] final(self).perms@[b] == old(self).perms@[b],
            final(self).perm(h@.addr).rc == old(self).perm(h@.addr).rc,
            final(self).perm(h@.addr).meta == old(self).perm(h@.addr).meta,
            final(self).perm(h@.addr).slots == old(self).perm(h@.addr).slots,
            final(self).perm(h@.addr).text == old(self).perm(h@.addr).text,
            final(self).perm(h@.addr).prefix is Some,
            final(self).perm(h@.addr).prefix->0.first == old(self).perm(h@.addr).prefix->0.first,
            final(self).perm(h@.addr).prefix->0.state.ptr() == old(self).perm(
                h@.addr,
            ).prefix->0.state.ptr(),
            final(self).perm(h@.addr).prefix->0.state.is_init(),
            final(self).perm(h@.addr).prefix->0.state.value() == v,
    {
        let ghost s0 = *self;
        let tracked mut p = self.perms.borrow_mut().tracked_remove(h@.addr);
        let tracked mut q = p.prefix.tracked_unwrap();
        u32_store(prefix_state_ptr(h), Tracked(&mut q.state), v);
        proof {
            p.prefix = Some(q);
            self.perms.borrow_mut().tracked_insert(h@.addr, p);
            assert(self.perms@.dom() =~= s0.perms@.dom());
        }
    }

    /// 検査モードの前置きの `first` の欄を書く。モデルはまだ替えない。
    #[inline]
    pub(crate) fn prefix_first_put(&mut self, h: *mut Header, v: u32)
        requires
            old(self).perms@.dom().contains(h@.addr),
            old(self).perm(h@.addr).prefix is Some,
            old(self).perm(h@.addr).prefix->0.first.ptr() == field_ptr::<u32>(h, -8),
            h@.addr >= 8,
        ensures
            final(self).pages == old(self).pages,
            final(self).env == old(self).env,
            final(self).model == old(self).model,
            final(self).perms@.dom() == old(self).perms@.dom(),
            forall|b: usize|
                b != h@.addr ==> #[trigger] final(self).perms@[b] == old(self).perms@[b],
            final(self).perm(h@.addr).rc == old(self).perm(h@.addr).rc,
            final(self).perm(h@.addr).meta == old(self).perm(h@.addr).meta,
            final(self).perm(h@.addr).slots == old(self).perm(h@.addr).slots,
            final(self).perm(h@.addr).text == old(self).perm(h@.addr).text,
            final(self).perm(h@.addr).prefix is Some,
            final(self).perm(h@.addr).prefix->0.state == old(self).perm(h@.addr).prefix->0.state,
            final(self).perm(h@.addr).prefix->0.first.ptr() == old(self).perm(
                h@.addr,
            ).prefix->0.first.ptr(),
            final(self).perm(h@.addr).prefix->0.first.is_init(),
            final(self).perm(h@.addr).prefix->0.first.value() == v,
    {
        let ghost s0 = *self;
        let tracked mut p = self.perms.borrow_mut().tracked_remove(h@.addr);
        let tracked mut q = p.prefix.tracked_unwrap();
        u32_store(prefix_first_ptr(h), Tracked(&mut q.first), v);
        proof {
            p.prefix = Some(q);
            self.perms.borrow_mut().tracked_insert(h@.addr, p);
            assert(self.perms@.dom() =~= s0.perms@.dom());
        }
    }

    /// rc を書く。モデルはまだ替えない。
    #[inline]
    pub(crate) fn rc_put(&mut self, h: *mut Header, v: i32)
        requires
            old(self).sound(),
            old(self).dom().contains(h@.addr),
            h == old(self).block(h@.addr),
        ensures
            final(self).rc_changed(*old(self), h@.addr),
            final(self).perm(h@.addr).prefix == old(self).perm(h@.addr).prefix,
            final(self).rc_of(h@.addr) == v,
    {
        proof {
            self.lemma_shape(h@.addr);
        }
        let tracked mut p = self.perms.borrow_mut().tracked_remove(h@.addr);
        rc_store(rc_ptr(h), Tracked(&mut p.rc), v);
        proof {
            self.perms.borrow_mut().tracked_insert(h@.addr, p);
        }
    }

    /// 共有の数を1つ進める (`dup`)。
    #[inline]
    pub(crate) fn rc_up_shared(&mut self, h: *mut Header)
        requires
            old(self).sound(),
            old(self).dom().contains(h@.addr),
            h == old(self).block(h@.addr),
            old(self).rc_of(h@.addr) > i32::MIN,
        ensures
            final(self).rc_changed(*old(self), h@.addr),
            final(self).perm(h@.addr).prefix == old(self).perm(h@.addr).prefix,
            final(self).rc_of(h@.addr) == old(self).rc_of(h@.addr) - 1,
    {
        proof {
            self.lemma_shape(h@.addr);
        }
        let tracked mut p = self.perms.borrow_mut().tracked_remove(h@.addr);
        rc_dup_shared(rc_ptr(h), Tracked(&mut p.rc));
        proof {
            self.perms.borrow_mut().tracked_insert(h@.addr, p);
        }
    }

    /// 共有の数を1つ戻し (`decref`)、最後の参照なら真を返す。
    #[inline]
    pub(crate) fn rc_down_shared(&mut self, h: *mut Header) -> (r: bool)
        requires
            old(self).sound(),
            old(self).dom().contains(h@.addr),
            h == old(self).block(h@.addr),
            old(self).rc_of(h@.addr) < 0,
        ensures
            final(self).rc_changed(*old(self), h@.addr),
            final(self).perm(h@.addr).prefix == old(self).perm(h@.addr).prefix,
            final(self).rc_of(h@.addr) == old(self).rc_of(h@.addr) + 1,
            r == (old(self).rc_of(h@.addr) == -1),
    {
        proof {
            self.lemma_shape(h@.addr);
        }
        let tracked mut p = self.perms.borrow_mut().tracked_remove(h@.addr);
        let r = rc_decref_shared(rc_ptr(h), Tracked(&mut p.rc));
        proof {
            self.perms.borrow_mut().tracked_insert(h@.addr, p);
        }
        r
    }

    /// `old` から、ブロック `h` の rc の値だけが変わった。
    pub(crate) open spec fn rc_changed(&self, old: Core, h: usize) -> bool {
        &&& self.pages == old.pages
        &&& self.env == old.env
        &&& self.model == old.model
        &&& self.perms@.dom() == old.perms@.dom()
        &&& forall|b: usize| b != h ==> #[trigger] self.perms@[b] == old.perms@[b]
        &&& self.perm(h).meta == old.perm(h).meta
        &&& self.perm(h).slots == old.perm(h).slots
        &&& self.perm(h).text == old.perm(h).text
        &&& prefix_kept(self.perm(h), old.perm(h))
        &&& self.perm(h).rc.ptr() == old.perm(h).rc.ptr()
        &&& self.perm(h).rc.is_init()
    }

    /// RC の遷移のモデルで、`h` の参照を手放すと最後の参照になるか。
    pub(crate) open spec fn last_by_model(&self, h: usize, up: bool) -> bool {
        !up && self.rc_of(h) != 0 && decref_next(
            Obj { rc: self.rc_of(h), count: self.model@.refs[h], stuck: self.model@.stuck[h] },
        ).freed
    }

    /// RC の遷移のモデルの側。`s0` は遷移の前の状態で、メモリの rc はもう書いてある。`up` なら `dup`、そうでなければ
    /// `decref` で、`edge` は解放の連鎖が手放す辺である。`dying` は、実行が最後の参照と決めたことである。`c` は、
    /// モデルが合っていてモデルの前提を守った道で、そのときはモデルの遷移をたどる。そうでない道 (検査モードだけ) では、
    /// 状態だけを `dying` に合わせる。
    #[inline(always)]
    pub(crate) fn count_ghost(
        &mut self,
        Ghost(s0): Ghost<Core>,
        Ghost(h): Ghost<usize>,
        Ghost(up): Ghost<bool>,
        Ghost(edge): Ghost<Option<Pos>>,
        Ghost(c): Ghost<bool>,
        Ghost(dying): Ghost<bool>,
    )
        requires
            c ==> s0.full(),
            !c ==> s0.checked(),
            s0.checked() ==> s0.safe_wf(),
            s0.has(h),
            s0.state(h) == BlockState::Live,
            s0.model@.building is None,
            old(self).rc_changed(s0, h),
            up ==> edge is None,
            up ==> !dying,
            s0.rc_of(h) == 0 ==> !dying,
            c ==> dying == s0.last_by_model(h, up),
            c ==> (edge matches Some(e) ==> s0.model@.edges.dom().contains(e) && s0.model@.edges[e]
                == h && s0.state(e.0) == BlockState::Dying),
            c ==> (s0.rc_of(h) != 0 ==> {
                let s = Obj {
                    rc: s0.rc_of(h),
                    count: s0.model@.refs[h],
                    stuck: s0.model@.stuck[h],
                };
                &&& !up && edge is None ==> s0.ext(h) >= 1
                &&& up ==> old(self).rc_of(h) == dup_next(s).obj.rc
                &&& !up ==> old(self).rc_of(h) == decref_next(s).obj.rc
            }),
            c ==> (s0.rc_of(h) == 0 ==> old(self).rc_of(h) == 0),
            c && up ==> old(self).perm(h).prefix == s0.perm(h).prefix,
            old(self).perm(h).prefix is Some ==> p_code(old(self).perm(h)) == if dying {
                0u32
            } else {
                1u32
            },
        ensures
            final(self).model@.tokens == s0.model@.tokens,
            c ==> final(self).full(),
            final(self).checked() ==> final(self).safe_wf(),
            final(self).pages == s0.pages,
            final(self).env == s0.env,
            final(self).perms == old(self).perms,
            final(self).dom() == s0.dom(),
            forall|w: Word| #[trigger] final(self).word_ok(w) == s0.word_ok(w),
            final(self).state(h) == if dying {
                BlockState::Dying
            } else {
                BlockState::Live
            },
            c && dying ==> final(self).edges_complete(h),
            c && s0.quiet() && !dying ==> final(self).quiet(),
            forall|b: usize| b != h ==> #[trigger] final(self).state(b) == s0.state(b),
            c ==> final(self).model@.edges == match edge {
                Some(e) => s0.model@.edges.remove(e),
                None => s0.model@.edges,
            },
            final(self).model@.state == if s0.dom().contains(h) {
                s0.model@.state.insert(
                    h,
                    if dying {
                        BlockState::Dying
                    } else {
                        BlockState::Live
                    },
                )
            } else {
                s0.model@.state
            },
            final(self).model@.building == s0.model@.building,
            final(self).model@.written == s0.model@.written,
            final(self).model@.kept == s0.model@.kept,
            final(self).model@.owner == s0.model@.owner,
            c ==> final(self).evolved(
                s0,
                h,
                if up {
                    1int
                } else if edge is None {
                    -1int
                } else {
                    0int
                },
            ),
            c && up ==> final(self).bumped(s0, h),
    {
        self.count_model(Ghost(s0), Ghost(h), Ghost(up), Ghost(edge), Ghost(c), Ghost(dying));
        proof {
            if !s0.dom().contains(h) {
                s0.lemma_region_at(h);
                self.lemma_region_count(s0, h, up, edge, c);
            } else if c {
                if up {
                    s0.lemma_at(h);
                }
                self.lemma_count_extra(s0, h, up, edge);
                if up && s0.rc_of(h) != 0 {
                    let s = Obj {
                        rc: s0.rc_of(h),
                        count: s0.model@.refs[h],
                        stuck: s0.model@.stuck[h],
                    };
                    reveal(Core::refs_wf);
                    assert(s0.counted_wf(h));
                    lemma_dup(s);
                }
            }
            if self.checked() && s0.dom().contains(h) {
                self.lemma_safe_counted(s0, h, dying);
            }
        }
    }

    /// 塊の物体の参照を動かした後。塊の物体は数えないので、モデルは手放した辺を外すだけで、ヒープの物体はどれも変わらない
    /// (docs/spec/object-model.md の「凍結した領域」)。
    proof fn lemma_region_count(&self, s0: Core, h: usize, up: bool, edge: Option<Pos>, c: bool)
        requires
            c ==> s0.full(),
            !c ==> s0.checked(),
            s0.checked() ==> s0.safe_wf(),
            s0.in_region(h),
            s0.model@.building is None,
            up ==> edge is None,
            c ==> (edge matches Some(e) ==> s0.model@.edges.dom().contains(e) && s0.model@.edges[e]
                == h && s0.state(e.0) == BlockState::Dying),
            self.pages == s0.pages,
            self.env == s0.env,
            self.perms@.dom() == s0.perms@.dom(),
            forall|b: usize| b != h ==> #[trigger] self.perms@[b] == s0.perms@[b],
            c ==> self.model@ == match edge {
                Some(e) => Model { edges: s0.model@.edges.remove(e), ..s0.model@ },
                None => s0.model@,
            },
            !c ==> self.model == s0.model,
        ensures
            c ==> self.full(),
            self.checked() ==> self.safe_wf(),
            c && s0.quiet() ==> self.quiet(),
            c ==> self.evolved(
                s0,
                h,
                if up {
                    1int
                } else if edge is None {
                    -1int
                } else {
                    0int
                },
            ),
            c && up ==> self.bumped(s0, h),
    {
        assert(self.perms@ =~= s0.perms@);
        let s1 = Core { model: self.model, ..s0 };
        assert(*self == s1);
        if c {
            if let Some(e) = edge {
                s1.lemma_edge_removed(s0, e);
            }
        }
        if self.checked() {
            reveal(Core::safe_blocks_wf);
            reveal(Core::safe_tokens_wf);
            reveal(Core::building_wf);
            assert forall|b: usize| #[trigger] s1.dom().contains(b) implies s1.safe_block_wf(b) by {
                assert(s0.safe_block_wf(b));
            }
            assert forall|e: Pos| #[trigger] s1.model@.kept.contains(e) implies s1.safe_kept_wf(
                e,
            ) by {
                assert(s0.safe_kept_wf(e));
            }
            assert forall|i: int|
                0 <= i < s1.model@.tokens.len() && (
                #[trigger] s1.model@.tokens[i]) is Some implies s1.token_entry(
                s1.model@.tokens[i]->0,
            ) by {
                assert(s0.token_entry(s0.model@.tokens[i]->0));
            }
        }
        if c && s0.quiet() {
            assert forall|b: usize| #[trigger] s1.dom().contains(b) implies s1.state(b)
                != BlockState::Dying by {
                assert(s0.dom().contains(b));
            }
        }
    }

    proof fn lemma_count_extra(&self, s0: Core, h: usize, up: bool, edge: Option<Pos>)
        requires
            s0.full(),
            s0.dom().contains(h),
            s0.state(h) == BlockState::Live,
            s0.model@.building is None,
            self.pages == s0.pages,
            self.env == s0.env,
            self.perms@.dom() == s0.perms@.dom(),
            forall|b: usize| b != h ==> #[trigger] self.perms@[b] == s0.perms@[b],
            self.perm(h).meta == s0.perm(h).meta,
            self.perm(h).slots == s0.perm(h).slots,
            self.perm(h).text == s0.perm(h).text,
            up ==> edge is None,
            edge matches Some(e) ==> s0.model@.edges.dom().contains(e) && s0.model@.edges[e] == h
                && s0.state(e.0) == BlockState::Dying,
            s0.rc_of(h) != 0 && !up && edge is None ==> s0.ext(h) >= 1,
            s0.rc_of(h) == 0 ==> self.rc_of(h) == 0,
            ({
                let s = Obj {
                    rc: s0.rc_of(h),
                    count: s0.model@.refs[h],
                    stuck: s0.model@.stuck[h],
                };
                let dying = !up && s0.rc_of(h) != 0 && decref_next(s).freed;
                &&& s0.rc_of(h) != 0 && up ==> self.rc_of(h) == dup_next(s).obj.rc
                &&& s0.rc_of(h) != 0 && !up ==> self.rc_of(h) == decref_next(s).obj.rc
                &&& self.state(h) == if dying {
                    BlockState::Dying
                } else {
                    BlockState::Live
                }
                &&& s0.rc_of(h) != 0 && !dying ==> self.ext(h) == s0.ext(h) + if up {
                    1int
                } else if edge is None {
                    -1int
                } else {
                    0int
                }
            }),
            forall|b: usize| b != h ==> #[trigger] self.state(b) == s0.state(b),
            forall|b: usize| b != h ==> #[trigger] self.ext(b) == s0.ext(b),
            self.model@.owner == s0.model@.owner,
            self.model@.edges == match edge {
                Some(e) => s0.model@.edges.remove(e),
                None => s0.model@.edges,
            },
        ensures
            ({
                let s = Obj {
                    rc: s0.rc_of(h),
                    count: s0.model@.refs[h],
                    stuck: s0.model@.stuck[h],
                };
                let dying = !up && s0.rc_of(h) != 0 && decref_next(s).freed;
                &&& dying ==> self.edges_complete(h)
                &&& s0.quiet() && !dying ==> self.quiet()
            }),
            self.evolved(
                s0,
                h,
                if up {
                    1int
                } else if edge is None {
                    -1int
                } else {
                    0int
                },
            ),
    {
        let rc = s0.rc_of(h);
        let s = Obj { rc, count: s0.model@.refs[h], stuck: s0.model@.stuck[h] };
        let dying = !up && rc != 0 && decref_next(s).freed;
        let delta = if up {
            1int
        } else if edge is None {
            -1int
        } else {
            0int
        };
        reveal(Core::blocks_wf);
        reveal(Core::refs_wf);
        assert(s0.block_wf(h));
        assert(s0.counted_wf(h));
        if dying {
            lemma_decref(s);
            if let Some(e) = edge {
                lemma_indeg_pos(s0.model@.edges, e);
            }
            assert forall|i: int|
                #![trigger self.scanned(h, i)]
                self.scanned(h, i) && p_word(
                    self.perm(h),
                    i,
                ).is_ptr_spec() implies self.model@.edges.dom().contains((h, i)) by {
                assert(s0.scanned(h, i));
                assert(s0.slot_word(s0.perm(h), h, i));
                if let Some(e) = edge {
                    assert(e.0 != h);
                }
            }
        }
        if s0.quiet() && !dying {
            assert forall|b: usize| #[trigger] self.dom().contains(b) implies self.state(b)
                != BlockState::Dying by {
                assert(s0.dom().contains(b));
            }
        }
        assert forall|b: usize|
            #![trigger s0.dom().contains(b)]
            s0.dom().contains(b) && s0.settled_state(b) == BlockState::Live && (s0.rc_of(b) == 0
                || s0.ext(b) + (if b == h {
                delta
            } else {
                0
            }) >= 1) implies {
            &&& self.dom().contains(b)
            &&& self.state(b) == BlockState::Live
            &&& self.perm(b).meta == s0.perm(b).meta
            &&& self.perm(b).slots == s0.perm(b).slots
            &&& self.perm(b).text == s0.perm(b).text
            &&& self.block(b) == s0.block(b)
            &&& self.cap(b) == s0.cap(b)
            &&& (s0.rc_of(b) == 0) == (self.rc_of(b) == 0)
            &&& s0.rc_of(b) != 0 ==> self.ext(b) == s0.ext(b) + if b == h {
                delta
            } else {
                0
            }
            &&& self.model@.owner[b] == s0.model@.owner[b]
        } by {
            if b == h && rc != 0 {
                if up {
                    lemma_dup(s);
                } else {
                    lemma_decref(s);
                }
                if let Some(e) = edge {
                    lemma_indeg_pos(s0.model@.edges, e);
                }
            }
        }
    }

    /// `count_ghost` の本体。
    #[inline(always)]
    fn count_model(
        &mut self,
        Ghost(s0): Ghost<Core>,
        Ghost(h): Ghost<usize>,
        Ghost(up): Ghost<bool>,
        Ghost(edge): Ghost<Option<Pos>>,
        Ghost(c): Ghost<bool>,
        Ghost(dying): Ghost<bool>,
    )
        requires
            c ==> s0.full(),
            s0.has(h),
            s0.state(h) == BlockState::Live,
            s0.model@.building is None,
            old(self).rc_changed(s0, h),
            up ==> edge is None,
            up ==> !dying,
            c ==> dying == s0.last_by_model(h, up),
            c ==> (edge matches Some(e) ==> s0.model@.edges.dom().contains(e) && s0.model@.edges[e]
                == h && s0.state(e.0) == BlockState::Dying),
            c ==> (s0.rc_of(h) != 0 ==> {
                let s = Obj {
                    rc: s0.rc_of(h),
                    count: s0.model@.refs[h],
                    stuck: s0.model@.stuck[h],
                };
                &&& !up && edge is None ==> s0.ext(h) >= 1
                &&& up ==> old(self).rc_of(h) == dup_next(s).obj.rc
                &&& !up ==> old(self).rc_of(h) == decref_next(s).obj.rc
            }),
            c ==> (s0.rc_of(h) == 0 ==> old(self).rc_of(h) == 0),
            old(self).perm(h).prefix is Some ==> p_code(old(self).perm(h)) == if dying {
                0u32
            } else {
                1u32
            },
        ensures
            final(self).model@.tokens == s0.model@.tokens,
            final(self).pages == s0.pages,
            final(self).perms == old(self).perms,
            final(self).dom() == s0.dom(),
            final(self).env == s0.env,
            forall|w: Word| #[trigger] final(self).word_ok(w) == s0.word_ok(w),
            final(self).model@.state == if s0.dom().contains(h) {
                s0.model@.state.insert(
                    h,
                    if dying {
                        BlockState::Dying
                    } else {
                        BlockState::Live
                    },
                )
            } else {
                s0.model@.state
            },
            final(self).model@.written == s0.model@.written,
            final(self).model@.kept == s0.model@.kept,
            final(self).model@.building == s0.model@.building,
            c ==> final(self).full(),
            c && dying ==> final(self).indeg(h) == 0,
            c && s0.rc_of(h) != 0 && !dying ==> final(self).ext(h) == s0.ext(h) + if up {
                1int
            } else if edge is None {
                -1int
            } else {
                0int
            },
            forall|b: usize| b != h ==> #[trigger] final(self).state(b) == s0.state(b),
            c ==> forall|b: usize|
                b != h ==> #[trigger] final(self).model@.refs[b] == s0.model@.refs[b],
            c ==> forall|b: usize|
                b != h && (edge matches Some(e) ==> b != s0.model@.edges[e])
                    ==> #[trigger] final(self).indeg(b) == s0.indeg(b),
            c ==> forall|b: usize| b != h ==> #[trigger] final(self).ext(b) == s0.ext(b),
            c ==> final(self).model@.refs.dom() == s0.model@.refs.dom(),
            c && up ==> final(self).model@.refs[h] == s0.model@.refs[h] + if s0.rc_of(h) != 0 {
                1nat
            } else {
                0nat
            },
            c && up ==> final(self).model@.state == s0.model@.state,
            final(self).model@.owner == s0.model@.owner,
            c ==> final(self).model@.edges == match edge {
                Some(e) => s0.model@.edges.remove(e),
                None => s0.model@.edges,
            },
            !s0.dom().contains(h) && c ==> final(self).model@ == match edge {
                Some(e) => Model { edges: s0.model@.edges.remove(e), ..s0.model@ },
                None => s0.model@,
            },
            !s0.dom().contains(h) && !c ==> final(self).model == s0.model,
    {
        proof {
            if !s0.dom().contains(h) {
                // 塊の物体は数えないので、辺を外すほかはモデルを変えない
                if c {
                    s0.lemma_region_at(h);
                    if let Some(e) = edge {
                        self.model = Ghost(Model { edges: s0.model@.edges.remove(e), ..s0.model@ });
                        let s1 = Core { model: self.model, ..s0 };
                        s1.lemma_edge_removed(s0, e);
                        assert(self.perms@ =~= s0.perms@);
                        assert(*self == s1);
                    } else {
                        self.model = Ghost(s0.model@);
                        assert(self.perms@ =~= s0.perms@);
                        assert(*self == s0);
                    }
                } else {
                    self.model = Ghost(s0.model@);
                }
            } else if c {
                let s1 = match edge {
                    Some(e) => Core {
                        model: Ghost(Model { edges: s0.model@.edges.remove(e), ..s0.model@ }),
                        ..s0
                    },
                    None => s0,
                };
                if let Some(e) = edge {
                    s1.lemma_edge_removed(s0, e);
                }
                let rc = s0.rc_of(h);
                let s = Obj { rc, count: s0.model@.refs[h], stuck: s0.model@.stuck[h] };
                reveal(Core::refs_wf);
                reveal(Core::blocks_wf);
                assert(s0.counted_wf(h));
                let (count, stuck, dying) = if rc == 0 {
                    (s.count, s.stuck, false)
                } else if up {
                    lemma_dup(s);
                    (dup_next(s).obj.count, dup_next(s).obj.stuck, false)
                } else {
                    lemma_decref(s);
                    (decref_next(s).obj.count, decref_next(s).obj.stuck, decref_next(s).freed)
                };
                self.model = Ghost(
                    Model {
                        state: s1.model@.state.insert(
                            h,
                            if dying {
                                BlockState::Dying
                            } else {
                                BlockState::Live
                            },
                        ),
                        refs: s1.model@.refs.insert(h, count),
                        stuck: s1.model@.stuck.insert(h, stuck),
                        ..s1.model@
                    },
                );
                assert(self.model@.refs.dom() =~= s1.model@.refs.dom());
                assert(self.model@.stuck.dom() =~= s1.model@.stuck.dom());
                if up {
                    assert(self.model@.state =~= s0.model@.state);
                }
                if self.rc_of(h) > 0 {
                    // 正の rc は、正の rc から動いたものだけである
                    assert(rc > 0);
                }
                self.lemma_count_step(s1, h, dying);
            } else {
                self.model = Ghost(
                    Model {
                        state: s0.model@.state.insert(
                            h,
                            if dying {
                                BlockState::Dying
                            } else {
                                BlockState::Live
                            },
                        ),
                        ..s0.model@
                    },
                );
            }
        }
    }
}

} // verus!
