//! Win64 x86_64 (Windowsのx86_64) のエントリのstubとlongjmpの側 (docs/spec/native.md の「`eml_enter`」)。
#![allow(unsafe_code)]
use core::mem::{offset_of, size_of};

use eml_rt::{CTX_JMP, Ctx, Fast};

use crate::enter::Thunk;

pub const SUPPORTED: bool = true;

/// `eml_enter`のフレームの中のバッファ。入れ子の`eml_enter`はそれぞれ自分のバッファを持ち、`prev`でつながる。
#[repr(C, align(16))]
pub struct JmpBuf {
    pub prev: *mut JmpBuf,
    ctx: *mut Ctx<Fast>,
    /// rbx, rbp, rdi, rsi, r12..r15。rbpの欄にはstub自身のフレームポインタが入る。呼び出し元のrbpはバッファのすぐ上の
    /// フレームの記録にあり、ふつうに戻るときもlongjmpの側も`pop rbp`で戻す。
    r: [u64; 8],
    /// xmm6..xmm15を128ビットずつ。`movaps`で読み書きするので、どの欄も16バイトに揃う。
    xmm: [[u64; 2]; 10],
}

/// Win64では、呼ぶ側がスタックポインタのすぐ上に、呼ばれる側のための32バイトのshadow spaceを取る。thunkがそこを
/// 使ってよいので、バッファはその上に置く。
const SHADOW: usize = 32;
/// `push rbp`の後でrspは16の倍数になり、16の倍数の大きさの枠を取ってもそのままなので、thunkを呼ぶ`call`で
/// Win64の求める16バイトの揃えが保たれ、xmmの欄も`movaps`の求める16バイトに揃う。
const FRAME: usize = SHADOW + size_of::<JmpBuf>();
const PREV: usize = SHADOW + offset_of!(JmpBuf, prev);
const CTX: usize = SHADOW + offset_of!(JmpBuf, ctx);
const R: usize = SHADOW + offset_of!(JmpBuf, r);
const XMM: usize = SHADOW + offset_of!(JmpBuf, xmm);
const _: () = assert!(FRAME.is_multiple_of(16) && XMM.is_multiple_of(16));

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
        "mov [rsp + {r} + 16], rdi",
        "mov [rsp + {r} + 24], rsi",
        "mov [rsp + {r} + 32], r12",
        "mov [rsp + {r} + 40], r13",
        "mov [rsp + {r} + 48], r14",
        "mov [rsp + {r} + 56], r15",
        "movaps [rsp + {xmm}], xmm6",
        "movaps [rsp + {xmm} + 16], xmm7",
        "movaps [rsp + {xmm} + 32], xmm8",
        "movaps [rsp + {xmm} + 48], xmm9",
        "movaps [rsp + {xmm} + 64], xmm10",
        "movaps [rsp + {xmm} + 80], xmm11",
        "movaps [rsp + {xmm} + 96], xmm12",
        "movaps [rsp + {xmm} + 112], xmm13",
        "movaps [rsp + {xmm} + 128], xmm14",
        "movaps [rsp + {xmm} + 144], xmm15",
        "mov rax, [rcx + {jmp}]",
        "mov [rsp + {prev}], rax",
        "mov [rsp + {ctx}], rcx",
        "lea rax, [rsp + {shadow}]",
        "mov [rcx + {jmp}], rax",
        "mov rax, rdx",
        "mov rdx, r8",
        "mov r8, r9",
        "call rax",
        "mov rcx, [rsp + {ctx}]",
        "mov rax, [rsp + {prev}]",
        "mov [rcx + {jmp}], rax",
        "add rsp, {frame}",
        "pop rbp",
        "xor eax, eax",
        "ret",
        frame = const FRAME,
        shadow = const SHADOW,
        prev = const PREV,
        ctx = const CTX,
        r = const R,
        xmm = const XMM,
        jmp = const CTX_JMP,
    )
}

/// いちばん内側の`eml_enter`が保存したレジスタを戻し、そのバッファをつなぎから外し、`value`を返してその`eml_enter`から
/// 戻る。呼ぶのは`eml_fatal`だけである。
#[unsafe(naked)]
pub(crate) unsafe extern "C" fn longjmp(ctx: *mut Ctx<Fast>, value: u64) -> ! {
    core::arch::naked_asm!(
        "mov rax, [rcx + {jmp}]",
        "lea rsp, [rax - {shadow}]",
        "mov rbx, [rsp + {r}]",
        "mov rbp, [rsp + {r} + 8]",
        "mov rdi, [rsp + {r} + 16]",
        "mov rsi, [rsp + {r} + 24]",
        "mov r12, [rsp + {r} + 32]",
        "mov r13, [rsp + {r} + 40]",
        "mov r14, [rsp + {r} + 48]",
        "mov r15, [rsp + {r} + 56]",
        "movaps xmm6, [rsp + {xmm}]",
        "movaps xmm7, [rsp + {xmm} + 16]",
        "movaps xmm8, [rsp + {xmm} + 32]",
        "movaps xmm9, [rsp + {xmm} + 48]",
        "movaps xmm10, [rsp + {xmm} + 64]",
        "movaps xmm11, [rsp + {xmm} + 80]",
        "movaps xmm12, [rsp + {xmm} + 96]",
        "movaps xmm13, [rsp + {xmm} + 112]",
        "movaps xmm14, [rsp + {xmm} + 128]",
        "movaps xmm15, [rsp + {xmm} + 144]",
        "mov rax, [rsp + {prev}]",
        "mov [rcx + {jmp}], rax",
        "add rsp, {frame}",
        "pop rbp",
        "mov rax, rdx",
        "ret",
        frame = const FRAME,
        shadow = const SHADOW,
        prev = const PREV,
        r = const R,
        xmm = const XMM,
        jmp = const CTX_JMP,
    )
}
