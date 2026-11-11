//! stubのない対象。ネイティブのコードはここでは動かず、`eml_enter`を呼ぶとプロセスが止まる (`extern "C"`の中のpanic)。
//! 走らせる側は先に`SUPPORTED`を見て断る (docs/spec/native.md の「対象」)。
#![allow(unsafe_code)]
use eml_rt::{Ctx, Fast};

use crate::enter::Thunk;

pub const SUPPORTED: bool = false;

#[repr(C)]
pub struct JmpBuf {
    pub prev: *mut JmpBuf,
}

/// # Safety
/// `ctx`はこのworkerの生きている`Ctx`で、呼ぶ側はそれへの`&`も`&mut`も持たない。`thunk`は`entry`と`argv`を受ける
/// 既定の規約の関数で、`argv`はその引数と結果の語の数だけ読み書きできる (docs/spec/native.md の「`eml_enter`」)。
pub unsafe extern "C" fn eml_enter(
    _ctx: *mut Ctx<Fast>,
    _thunk: Thunk,
    _entry: *const u8,
    _argv: *mut u64,
) -> u64 {
    unreachable!("native code is not supported on this target")
}

pub(crate) unsafe extern "C" fn longjmp(_ctx: *mut Ctx<Fast>, _value: u64) -> ! {
    unreachable!("native code is not supported on this target")
}
