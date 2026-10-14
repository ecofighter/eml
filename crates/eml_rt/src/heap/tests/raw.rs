//! テストがヒープの操作の外から読み書きするヘッダと前置きの欄。核は権限の上でしか触れないので、テストだけが生の
//! ポインタで触れる。

use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use crate::Word;
use crate::meta::{Meta, meta_ptr, slot_ptr};

/// rc の欄。
pub(super) unsafe fn rc(w: Word) -> i32 {
    unsafe { AtomicI32::from_ptr(w.as_ptr().cast::<i32>()) }.load(Ordering::Relaxed)
}

/// ヘッダの `meta`。
pub(super) unsafe fn meta(w: Word) -> Meta {
    Meta::from_bits(unsafe { meta_ptr(w.as_ptr()).read() })
}

/// ヘッダの `meta` を書き換える。
pub(super) unsafe fn set_meta(w: Word, m: Meta) {
    unsafe { meta_ptr(w.as_ptr()).write(m.bits()) }
}

/// ペイロードの i 語目を書き換える。`set_field` は共有の物体に書けないので、共有の物体の輪を切るのに使う。
pub(super) unsafe fn set_slot(w: Word, i: usize, v: Word) {
    unsafe { slot_ptr(w.as_ptr(), i).write(v) }
}

/// 検査モードの前置きの `first` の欄 (所有者 ID か不死の物体の数)。
pub(super) unsafe fn prefix_first(w: Word) -> u32 {
    let h = w.as_ptr();
    let p = h.cast::<u32>().with_addr(h.addr() - 8);
    unsafe { AtomicU32::from_ptr(p) }.load(Ordering::Relaxed)
}
