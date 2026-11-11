//! SysV x86_64 (Windowsでないx86_64) のエントリのstubとlongjmpの側 (docs/spec/native.md の「`eml_enter`」)。
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
        core::arch::asm!("mov {}, rsp", out(reg) sp, options(nomem, nostack, preserves_flags))
    };
    sp
}

/// `eml_enter`のフレームの中のバッファ。入れ子の`eml_enter`はそれぞれ自分のバッファを持ち、`prev`でつながる。
#[repr(C)]
pub struct JmpBuf {
    pub prev: *mut JmpBuf,
    ctx: *mut Ctx<Fast>,
    /// rbx, rbp, r12..r15。rbpの欄にはstub自身のフレームポインタが入る。呼び出し元のrbpはバッファのすぐ上の
    /// フレームの記録にあり、ふつうに戻るときもlongjmpの側も`pop rbp`で戻す。
    r: [u64; 6],
}

/// `push rbp`の後でrspは16の倍数になり、16の倍数の大きさの枠を取ってもそのままなので、thunkを呼ぶ`call`で
/// SysVの求める16バイトの揃えが保たれる。
const FRAME: usize = size_of::<JmpBuf>();
const PREV: usize = offset_of!(JmpBuf, prev);
const CTX: usize = offset_of!(JmpBuf, ctx);
const R: usize = offset_of!(JmpBuf, r);
const _: () = assert!(FRAME.is_multiple_of(16));

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
        "push rbp",
        "mov rbp, rsp",
        "sub rsp, {frame}",
        "mov [rsp + {r}], rbx",
        "mov [rsp + {r} + 8], rbp",
        "mov [rsp + {r} + 16], r12",
        "mov [rsp + {r} + 24], r13",
        "mov [rsp + {r} + 32], r14",
        "mov [rsp + {r} + 40], r15",
        "mov rax, [rdi + {jmp}]",
        "mov [rsp + {prev}], rax",
        "mov [rsp + {ctx}], rdi",
        "mov [rdi + {jmp}], rsp",
        "mov rax, rsi",
        "mov rsi, rdx",
        "mov rdx, rcx",
        "call rax",
        "mov rdi, [rsp + {ctx}]",
        "mov rax, [rsp + {prev}]",
        "mov [rdi + {jmp}], rax",
        "add rsp, {frame}",
        "pop rbp",
        "xor eax, eax",
        "ret",
        frame = const FRAME,
        prev = const PREV,
        ctx = const CTX,
        r = const R,
        jmp = const CTX_JMP,
    )
}

/// いちばん内側の`eml_enter`が保存したレジスタを戻し、そのバッファをつなぎから外し、`value`を返してその`eml_enter`から
/// 戻る。呼ぶのは`eml_fatal`だけである。
#[unsafe(naked)]
pub(crate) unsafe extern "C" fn longjmp(ctx: *mut Ctx<Fast>, value: u64) -> ! {
    core::arch::naked_asm!(
        "mov rsp, [rdi + {jmp}]",
        "mov rbx, [rsp + {r}]",
        "mov rbp, [rsp + {r} + 8]",
        "mov r12, [rsp + {r} + 16]",
        "mov r13, [rsp + {r} + 24]",
        "mov r14, [rsp + {r} + 32]",
        "mov r15, [rsp + {r} + 40]",
        "mov rax, [rsp + {prev}]",
        "mov [rdi + {jmp}], rax",
        "add rsp, {frame}",
        "pop rbp",
        "mov rax, rsi",
        "ret",
        frame = const FRAME,
        prev = const PREV,
        r = const R,
        jmp = const CTX_JMP,
    )
}
