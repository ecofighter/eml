//! 文字列の物体 (docs/spec/object-model.md の「種類」)。種類 `String` の物体は、ここの操作だけが作り、書く。
//! 長さの語は生の 0 語目で、バイト列はその後ろにあり、ghost のバイト列 `text` に等しい。

use vstd::prelude::*;
#[cfg(verus_only)]
use vstd::string::StringSliceAdditionalSpecFns;
#[cfg(verus_only)]
use vstd::utf8::encode_utf8_valid_utf8;

use super::*;
#[cfg(verus_only)]
use crate::heap::inv::string_wf;
use crate::text::{text_ptr, write_text};

verus! {

impl<M: Mode> Heap<M> {
    pub closed spec fn is_string(&self, w: Word) -> bool {
        self.core.kind(w.bits_spec()) == Kind::String
    }

    /// 文字列のバイト列。
    pub closed spec fn text_of(&self, w: Word) -> Seq<u8> {
        self.core.perm(w.bits_spec()).text
    }

    /// `alloc_string_immortal` の後の状態。`w` は rc 0 の生きている物体で、検査モードの外に出ている参照の数は 0 である。
    pub closed spec fn made_immortal(&self, old: Heap<M>, w: Word) -> bool {
        let h = w.bits_spec();
        &&& self.wf()
        &&& self.dummy == old.dummy
        &&& self.worker == old.worker
        &&& self.core.model@.building is None
        &&& w.is_ptr_spec()
        &&& self.core.word_ok(w)
        &&& !old.core.dom().contains(h)
        &&& self.core.dom() == old.core.dom().insert(h)
        &&& self.core.state(h) == BlockState::Live
        &&& self.core.rc_of(h) == 0
        &&& old.consistent() && old.settleable() ==> {
            &&& self.consistent()
            &&& self.core.frame(old.core)
        }
    }

    /// `s` の UTF-8 のバイト列を持つ文字列を作る。
    pub fn alloc_string(&mut self, s: &str) -> (w: Word)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            !M::CHECKED ==> old(self).settleable(),
        ensures
            final(self).made(*old(self), w),
            final(self).is_string(w),
            final(self).text_of(w) == s.spec_bytes(),
    {
        self.make_string(s, 1)
    }

    /// `alloc_string` と同じで、rc 0 の不死の物体にする。`CodeBase` の文字列の定数が使う。
    pub fn alloc_string_immortal(&mut self, s: &str) -> (w: Word)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            !M::CHECKED ==> old(self).settleable(),
        ensures
            final(self).made_immortal(*old(self), w),
            final(self).is_string(w),
            final(self).text_of(w) == s.spec_bytes(),
    {
        self.make_string(s, 0)
    }

    /// `s` を写した文字列を、rc `rc` の生きている物体として作る。rc 0 なら不死の物体で、検査モードの外に出ている参照の
    /// 数は 0 から始まる (`alloc_header` が書く)。
    fn make_string(&mut self, s: &str, rc: i32) -> (w: Word)
        requires
            old(self).wf(),
            old(self).safe_settleable(),
            !M::CHECKED ==> old(self).settleable(),
            rc == 0 || rc == 1,
        ensures
            rc == 1 ==> final(self).made(*old(self), w),
            rc == 0 ==> final(self).made_immortal(*old(self), w),
            final(self).is_string(w),
            final(self).text_of(w) == s.spec_bytes(),
    {
        // vstd の `str::len` の仕様は長さが usize に収まることを言わないので、スライスの長さで読む
        #[allow(clippy::needless_as_bytes)]
        let bytes = s.as_bytes().len();
        // 長さの1語と、バイト列を語に切り上げた数。`bytes / 8` は usize の 1/8 以下なので、足しても桁あふれしない
        let payload_words = 1 + bytes / 8 + if bytes.is_multiple_of(8) {
            0
        } else {
            1
        };
        self.settle();
        let ghost s1 = *self;
        let meta = Meta::new(0, 0, Kind::String, false);
        let (h, Tracked(perm)) = self.alloc_header(payload_words, rc, meta);
        let tracked mut perm = perm;
        proof {
            self.core.pages.lemma_block_bounds(h@.addr);
        }
        let cap = self.core.pages.cap_of(h);
        proof {
            assert(8 * (cap - 1) >= bytes) by (nonlinear_arith)
                requires
                    cap >= payload_words,
                    payload_words == 1 + bytes / 8 + if bytes % 8 == 0 {
                        0int
                    } else {
                        1int
                    },
            ;
            // 確保器が渡すブロックの ghost のバイト列は、前に使った物体のものでありうる。中身はまだ何も写していない
            perm.text = Seq::empty();
        }
        let tracked mut slot = perm.slots.tracked_remove(0);
        ptr_mut_write(slot_ptr(h, 0), Tracked(&mut slot), Word::from_bits(bytes));
        proof {
            perm.slots.tracked_insert(0, slot);
        }
        write_text(h, cap, 0, s, Tracked(&mut perm));
        let ghost mid = self.core;
        proof {
            check::lemma_alive_code();
            encode_utf8_valid_utf8(s@);
            assert(perm.text =~= s.spec_bytes());
            let c = self.core;
            let a = h@.addr;
            assert(string_wf(perm, c.cap(a)));
            assert(c.perm_wf(perm, a, BlockState::Live, Set::empty(), self.worker));
        }
        self.core.publish(Ghost(h), Tracked(perm), Ghost(BlockState::Live), Ghost(self.worker));
        let w = Word::from_ptr(h);
        proof {
            if old(self).consistent() && old(self).settleable() {
                self.lemma_dummy_kept(s1);
                self.core.lemma_frame_chain(mid, s1.core, old(self).core);
            } else {
                self.lemma_published_safe(s1);
            }
            crate::word::lemma_aligned_is_ptr(h@.addr);
        }
        w
    }

    /// # Safety
    /// `w` はこのヒープの生きている文字列の物体を指す。返す位置を読めるのは、物体が生きていて、`string_push` でその
    /// 物体を書き換えていない間だけである。
    pub unsafe fn string(&self, w: Word) -> (r: Result<*const str, HeapError>)
        requires
            self.wf(),
            w.is_ptr_spec(),
            self.word_in(w),
            !M::CHECKED ==> self.settleable() && self.readable(w) && self.is_string(w),
        ensures
            self.consistent() && self.settleable() && self.readable(w) && self.is_string(w)
                ==> r is Ok,
    {
        let h = self.header_of(w);
        proof {
            if self.consistent() && self.settleable() && self.readable(w) {
                self.core.lemma_readable(w);
            }
        }
        self.checked_live(h)?;
        if M::CHECKED && !matches!(self.core.meta_get(h).kind(), Kind::String) {
            return Err(HeapError::WrongKind);
        }
        proof {
            if M::CHECKED {
                self.core.lemma_safe_at(h@.addr);
            } else {
                self.core.lemma_live_block(h@.addr);
            }
        }
        let len = self.core.slot_get(h, 0).bits();
        let tracked p = self.core.perms.borrow().tracked_borrow(h@.addr);
        Ok(text_ptr(h, len, Tracked(p)))
    }
}

} // verus!
