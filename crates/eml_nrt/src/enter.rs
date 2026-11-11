//! Rustからemlのコードに入る道と、致命的な誤りでエントリまで切る道 (docs/spec/native.md の「`eml_enter`」、
//! 「`eml_fatal`」)。
#![allow(unsafe_code)]

use eml_rt::{Ctx, Fast, Fault, HeapError};

use crate::arch;
pub use crate::arch::eml_enter;

/// `eml_enter`が呼ぶ、既定の規約の関数。
pub type Thunk = unsafe extern "C" fn(ctx: *mut Ctx<Fast>, entry: *const u8, argv: *mut u64);

/// エントリまで切った誤り。`code`は0でない。中身のありかは符号ごとに決まる (docs/spec/native.md の「`eml_fatal`」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fatal {
    pub code: u32,
    pub site: u32,
}

/// エントリの中でRustのpanicを捕まえた。中身はない。
pub const FATAL_PANICKED: u32 = 1;
/// `Fault`が`Ctx`に置いてある (`Ctx::take_fault`)。
pub const FATAL_FAULT: u32 = 2;
/// 誤りはworkerの構造体に記録してある (`worker::record`)。
pub const FATAL_RECORDED: u32 = 3;

/// 包みの本体が返す誤り。`park`は中身を`Ctx`に置き、`eml_fatal`に渡す符号を返す。`Fault`と`HeapError`の実装は
/// ここにだけ置く。
pub trait Raise {
    fn park(self, ctx: &mut Ctx<Fast>) -> Fatal;
}

impl Raise for Fault {
    fn park(self, ctx: &mut Ctx<Fast>) -> Fatal {
        let previous = ctx.set_fault(self);
        debug_assert!(previous.is_none(), "a fault was already parked");
        Fatal {
            code: FATAL_FAULT,
            site: 0,
        }
    }
}

impl Raise for HeapError {
    fn park(self, ctx: &mut Ctx<Fast>) -> Fatal {
        Fault::Heap(self).park(ctx)
    }
}

/// 誤りをもう記録したエントリが、符号をそのまま返す。
impl Raise for Fatal {
    fn park(self, _ctx: &mut Ctx<Fast>) -> Fatal {
        self
    }
}

/// `eml_enter`を呼び、結果の語を`Result`に直す。
///
/// # Safety
/// `eml_enter`と同じ。
pub unsafe fn enter(
    ctx: *mut Ctx<Fast>,
    thunk: Thunk,
    entry: *const u8,
    argv: *mut u64,
) -> Result<(), Fatal> {
    // SAFETY: 呼ぶ側の約束
    let packed = unsafe { eml_enter(ctx, thunk, entry, argv) };
    if packed == 0 {
        Ok(())
    } else {
        Err(Fatal {
            code: (packed >> 32) as u32,
            site: packed as u32,
        })
    }
}

/// `Ctx`がどのエントリの外にもあり、ほかのスレッドへ移せる状態か (docs/spec/object-model.md の「`unsafe` の核」のT18)。
pub fn detached(ctx: &Ctx<Fast>) -> bool {
    ctx.jmp().is_null() && ctx.runtime().is_null()
}

crate::entries! {
    pub(crate) const ENTRIES;

    /// いちばん内側の`eml_enter`まで切る。何も記録しない。利用者のRustを走らせず、落とすものを持たないので`plain`である。
    /// panicしうるのは内部の不変条件の確かめだけで、`extern "C"`の中なのでプロセスを止める (docs/spec/native.md の「包みの本体」)。
    pub plain fn eml_fatal(ctx, code: i64, site: i64) -> ! {
        let code = code as u32;
        // 0を通すと`enter`がふつうに戻ったと読むので、releaseでも確かめる。冷たい道なので速い道の費用はない
        assert!(code != 0, "a fatal code is not 0");
        debug_assert!(
            (0..=i64::from(u32::MAX)).contains(&site),
            "a fault site fits in 32 bits"
        );
        // SAFETY: 呼ぶ側は生きている`Ctx`を渡す
        debug_assert!(!unsafe { (*ctx).jmp() }.is_null(), "eml_fatal outside eml_enter");
        // SAFETY: 呼ぶ側は`eml_enter`の中にいて、この枠は落とすものを持たない
        unsafe { arch::longjmp(ctx, (u64::from(code) << 32) | u64::from(site as u32)) }
    }
}
