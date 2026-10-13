//! LIR の `release` (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「核の操作」)。判定は `is_last` で、
//! rc が −1 の物体も最後の参照として、残すフィールドを `dup` せずに受け取る。共有の親から `lin` の子が届く形
//! (spec の「`share`」) で、正しいプログラムの `File` を `dup` しないためである。

use std::mem;
use std::ptr::NonNull;

use super::{Heap, scan_slot};
use crate::error::HeapError;
use crate::meta::{self, Header};
use crate::mode::Mode;
use crate::rc;
use crate::word::Word;

impl<M: Mode> Heap<M> {
    /// `keep` は走査する語ごとに、その子の参照を呼ぶ側に残すかを持つ。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている物体を指し、呼ぶ側がその参照を1つ持っている。
    pub unsafe fn release(&mut self, w: Word, tag: u16, keep: &[bool]) -> Result<(), HeapError> {
        let h = w.header();
        unsafe { self.check_release(h, tag, keep.len())? };
        if rc::is_last(unsafe { meta::rc(h) }) {
            unsafe { self.release_children(h, keep)? };
            if M::CHECKED {
                self.stats.rc_decrements += 1;
            }
            unsafe { self.free_box(h) };
            Ok(())
        } else {
            for (i, &kept) in keep.iter().enumerate() {
                if kept {
                    unsafe { self.dup(scan_slot(h, i).read())? };
                }
            }
            unsafe { self.decref(w) }
        }
    }

    /// 残さない子を手放す。残す子の参照は、呼ぶ側が箱から受け継ぐ。
    pub(super) unsafe fn release_children(
        &mut self,
        h: NonNull<Header>,
        keep: &[bool],
    ) -> Result<(), HeapError> {
        let mut work = mem::take(&mut self.work);
        let result = unsafe { self.release_unkept(h, keep, &mut work) };
        work.clear();
        self.work = work;
        result
    }

    unsafe fn release_unkept(
        &mut self,
        h: NonNull<Header>,
        keep: &[bool],
        work: &mut Vec<Word>,
    ) -> Result<(), HeapError> {
        for i in (0..keep.len()).rev() {
            if keep[i] {
                continue;
            }
            let child = unsafe { scan_slot(h, i).read() };
            if child.is_ptr() && unsafe { self.release_ref(child.header())? } {
                work.push(child);
            }
        }
        unsafe { self.drain(work) }
    }
}
