//! AAPCS64 (macOSとLinux) のエントリのstubとlongjmpの側 (docs/spec/native.md の「`eml_enter`」)。
#![allow(unsafe_code)]
use core::mem::{offset_of, size_of};

use eml_rt::{CTX_JMP, Ctx, Fast};

use crate::enter::Thunk;

pub const SUPPORTED: bool = true;

/// 今のスタックポインタ。スタックの底は、局所変数のアドレスでなくこの値から求める。アドレスを外に出す局所変数は、
/// AddressSanitizerの偽のスタックに移りうるためである。安全点が比べるのも同じスタックポインタである。
#[inline(always)]
pub(crate) fn stack_pointer() -> usize {
    let sp: usize;
    // SAFETY: レジスタを1つ読むだけで、記憶にもフラグにも触れない
    unsafe {
        core::arch::asm!("mov {}, sp", out(reg) sp, options(nomem, nostack, preserves_flags))
    };
    sp
}

/// `eml_enter`のフレームの中のバッファ。入れ子の`eml_enter`はそれぞれ自分のバッファを持ち、`prev`でつながる。
#[repr(C)]
pub struct JmpBuf {
    pub prev: *mut JmpBuf,
    ctx: *mut Ctx<Fast>,
    /// x19..x28, x29, x30
    x: [u64; 12],
    /// d8..d15
    d: [u64; 8],
}

const SIZE: usize = size_of::<JmpBuf>();
const PREV: usize = offset_of!(JmpBuf, prev);
const X: usize = offset_of!(JmpBuf, x);
const D: usize = offset_of!(JmpBuf, d);
const _: () = assert!(SIZE.is_multiple_of(16) && offset_of!(JmpBuf, ctx) == PREV + 8);

/// 呼ばれる側が保存するレジスタをすべて保存し、`ctx`の`jmp`にバッファをつなぎ、標準のフレームの記録を作ってから
/// `thunk(ctx, entry, argv)`を呼ぶ。thunkから戻れば0を、`eml_fatal`で切られればその符号とsiteを返す。
///
/// # Safety
/// `ctx`はこのworkerの生きている`Ctx`で、呼ぶ側はそれへの`&`も`&mut`も持たない。`thunk`は`entry`と`argv`を受ける
/// 既定の規約の関数で、`argv`はその引数と結果の語の数だけ読み書きできる (docs/spec/native.md の「`eml_enter`」)。
#[unsafe(naked)]
pub unsafe extern "C" fn eml_enter(
    ctx: *mut Ctx<Fast>,
    thunk: Thunk,
    entry: *const u8,
    argv: *mut u64,
) -> u64 {
    core::arch::naked_asm!(
        "sub sp, sp, #{size}",
        "stp x19, x20, [sp, #{x}]",
        "stp x21, x22, [sp, #{x} + 16]",
        "stp x23, x24, [sp, #{x} + 32]",
        "stp x25, x26, [sp, #{x} + 48]",
        "stp x27, x28, [sp, #{x} + 64]",
        "stp x29, x30, [sp, #{x} + 80]",
        "add x29, sp, #{x} + 80",
        "stp d8, d9, [sp, #{d}]",
        "stp d10, d11, [sp, #{d} + 16]",
        "stp d12, d13, [sp, #{d} + 32]",
        "stp d14, d15, [sp, #{d} + 48]",
        "ldr x9, [x0, #{jmp}]",
        "stp x9, x0, [sp, #{prev}]",
        "mov x9, sp",
        "str x9, [x0, #{jmp}]",
        "mov x9, x1",
        "mov x1, x2",
        "mov x2, x3",
        "blr x9",
        "ldp x9, x10, [sp, #{prev}]",
        "str x9, [x10, #{jmp}]",
        "ldp x29, x30, [sp, #{x} + 80]",
        "add sp, sp, #{size}",
        "mov x0, #0",
        "ret",
        size = const SIZE,
        prev = const PREV,
        x = const X,
        d = const D,
        jmp = const CTX_JMP,
    )
}

/// いちばん内側の`eml_enter`が保存したレジスタを戻し、そのバッファをつなぎから外し、`value`を返してその`eml_enter`から
/// 戻る。呼ぶのは`eml_fatal`だけである。
#[unsafe(naked)]
pub(crate) unsafe extern "C" fn longjmp(ctx: *mut Ctx<Fast>, value: u64) -> ! {
    core::arch::naked_asm!(
        "ldr x9, [x0, #{jmp}]",
        "mov sp, x9",
        "ldp x19, x20, [sp, #{x}]",
        "ldp x21, x22, [sp, #{x} + 16]",
        "ldp x23, x24, [sp, #{x} + 32]",
        "ldp x25, x26, [sp, #{x} + 48]",
        "ldp x27, x28, [sp, #{x} + 64]",
        "ldp x29, x30, [sp, #{x} + 80]",
        "ldp d8, d9, [sp, #{d}]",
        "ldp d10, d11, [sp, #{d} + 16]",
        "ldp d12, d13, [sp, #{d} + 32]",
        "ldp d14, d15, [sp, #{d} + 48]",
        "ldr x9, [sp, #{prev}]",
        "str x9, [x0, #{jmp}]",
        "add sp, sp, #{size}",
        "mov x0, x1",
        "ret",
        size = const SIZE,
        prev = const PREV,
        x = const X,
        d = const D,
        jmp = const CTX_JMP,
    )
}
