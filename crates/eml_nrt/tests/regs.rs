//! レジスタのテストとフレームの記録のテストが使う、ABIごとのasmの補助 (docs/implementation/testing.md の「`eml_nrt`の検証」)。
#![cfg(not(miri))]
use eml_rt::{Ctx, Fast};

#[cfg(all(target_arch = "aarch64", not(windows)))]
mod imp {
    use super::*;

    /// 呼ばれる側が保存するレジスタをすべて壊してから`eml_fatal(ctx, 7, 9)`へ飛ぶthunk。
    #[unsafe(naked)]
    pub unsafe extern "C" fn clobber_thunk(ctx: *mut Ctx<Fast>, entry: *const u8, argv: *mut u64) {
        core::arch::naked_asm!(
            "mov x19, #-1", "mov x20, #-1", "mov x21, #-1", "mov x22, #-1", "mov x23, #-1",
            "mov x24, #-1", "mov x25, #-1", "mov x26, #-1", "mov x27, #-1", "mov x28, #-1",
            "mov x29, #-1",
            "fmov d8, x19", "fmov d9, x19", "fmov d10, x19", "fmov d11, x19",
            "fmov d12, x19", "fmov d13, x19", "fmov d14, x19", "fmov d15, x19",
            "mov x1, #7",
            "mov x2, #9",
            "b {fatal}",
            fatal = sym eml_nrt::eml_fatal,
        )
    }

    /// 呼ばれる側が保存するレジスタに決まった値を入れて`eml_enter`を呼び、戻った後に違っていたレジスタのビットを返す。
    /// `eml_enter`の結果は`*out`に書く。
    #[unsafe(naked)]
    pub unsafe extern "C" fn enter_with_patterns(
        ctx: *mut Ctx<Fast>,
        thunk: eml_nrt::Thunk,
        entry: *const u8,
        argv: *mut u64,
        out: *mut u64,
    ) -> u64 {
        core::arch::naked_asm!(
            "sub sp, sp, #176",
            "stp x19, x20, [sp, #0]", "stp x21, x22, [sp, #16]", "stp x23, x24, [sp, #32]",
            "stp x25, x26, [sp, #48]", "stp x27, x28, [sp, #64]", "stp x29, x30, [sp, #80]",
            "stp d8, d9, [sp, #96]", "stp d10, d11, [sp, #112]", "stp d12, d13, [sp, #128]",
            "stp d14, d15, [sp, #144]",
            "str x4, [sp, #160]",
            "mov x19, #19", "mov x20, #20", "mov x21, #21", "mov x22, #22", "mov x23, #23",
            "mov x24, #24", "mov x25, #25", "mov x26, #26", "mov x27, #27", "mov x28, #28",
            "mov x29, #29",
            "fmov d8, x19", "fmov d9, x20", "fmov d10, x21", "fmov d11, x22",
            "fmov d12, x23", "fmov d13, x24", "fmov d14, x25", "fmov d15, x26",
            "bl {enter}",
            "mov x9, #0",
            "cmp x19, #19", "cset x10, ne", "orr x9, x9, x10",
            "cmp x20, #20", "cset x10, ne", "orr x9, x9, x10, lsl #1",
            "cmp x21, #21", "cset x10, ne", "orr x9, x9, x10, lsl #2",
            "cmp x22, #22", "cset x10, ne", "orr x9, x9, x10, lsl #3",
            "cmp x23, #23", "cset x10, ne", "orr x9, x9, x10, lsl #4",
            "cmp x24, #24", "cset x10, ne", "orr x9, x9, x10, lsl #5",
            "cmp x25, #25", "cset x10, ne", "orr x9, x9, x10, lsl #6",
            "cmp x26, #26", "cset x10, ne", "orr x9, x9, x10, lsl #7",
            "cmp x27, #27", "cset x10, ne", "orr x9, x9, x10, lsl #8",
            "cmp x28, #28", "cset x10, ne", "orr x9, x9, x10, lsl #9",
            "cmp x29, #29", "cset x10, ne", "orr x9, x9, x10, lsl #10",
            "fmov x11, d8", "cmp x11, #19", "cset x10, ne", "orr x9, x9, x10, lsl #16",
            "fmov x11, d9", "cmp x11, #20", "cset x10, ne", "orr x9, x9, x10, lsl #17",
            "fmov x11, d10", "cmp x11, #21", "cset x10, ne", "orr x9, x9, x10, lsl #18",
            "fmov x11, d11", "cmp x11, #22", "cset x10, ne", "orr x9, x9, x10, lsl #19",
            "fmov x11, d12", "cmp x11, #23", "cset x10, ne", "orr x9, x9, x10, lsl #20",
            "fmov x11, d13", "cmp x11, #24", "cset x10, ne", "orr x9, x9, x10, lsl #21",
            "fmov x11, d14", "cmp x11, #25", "cset x10, ne", "orr x9, x9, x10, lsl #22",
            "fmov x11, d15", "cmp x11, #26", "cset x10, ne", "orr x9, x9, x10, lsl #23",
            "ldr x4, [sp, #160]",
            "str x0, [x4]",
            "mov x0, x9",
            "ldp x19, x20, [sp, #0]", "ldp x21, x22, [sp, #16]", "ldp x23, x24, [sp, #32]",
            "ldp x25, x26, [sp, #48]", "ldp x27, x28, [sp, #64]", "ldp x29, x30, [sp, #80]",
            "ldp d8, d9, [sp, #96]", "ldp d10, d11, [sp, #112]", "ldp d12, d13, [sp, #128]",
            "ldp d14, d15, [sp, #144]",
            "add sp, sp, #176",
            "ret",
            enter = sym eml_nrt::eml_enter,
        )
    }
    /// 自分のフレームポインタのレジスタを4つ目の引数にして`walk_frames`へ飛ぶthunk。
    #[unsafe(naked)]
    pub unsafe extern "C" fn fp_thunk(ctx: *mut Ctx<Fast>, entry: *const u8, argv: *mut u64) {
        core::arch::naked_asm!("mov x3, x29", "b {walk}", walk = sym crate::stub::walk_frames)
    }

    /// 今のフレームポインタのレジスタを`*out`に書いてから`eml_enter`へ飛ぶ。
    #[unsafe(naked)]
    pub unsafe extern "C" fn enter_noting_fp(
        ctx: *mut Ctx<Fast>,
        thunk: eml_nrt::Thunk,
        entry: *const u8,
        argv: *mut u64,
        out: *mut u64,
    ) -> u64 {
        core::arch::naked_asm!("str x29, [x4]", "b {enter}", enter = sym eml_nrt::eml_enter)
    }
}

#[cfg(all(target_arch = "x86_64", not(windows)))]
mod imp {
    use super::*;

    /// 呼ばれる側が保存するレジスタをすべて壊してから`eml_fatal(ctx, 7, 9)`へ飛ぶthunk。
    #[unsafe(naked)]
    pub unsafe extern "C" fn clobber_thunk(ctx: *mut Ctx<Fast>, entry: *const u8, argv: *mut u64) {
        core::arch::naked_asm!(
            "mov rbx, -1", "mov rbp, -1", "mov r12, -1", "mov r13, -1", "mov r14, -1", "mov r15, -1",
            "mov esi, 7",
            "mov edx, 9",
            "jmp {fatal}",
            fatal = sym eml_nrt::eml_fatal,
        )
    }

    /// 呼ばれる側が保存するレジスタに決まった値を入れて`eml_enter`を呼び、戻った後に違っていたレジスタのビットを返す。
    /// `eml_enter`の結果は`*out`に書く。
    #[unsafe(naked)]
    pub unsafe extern "C" fn enter_with_patterns(
        ctx: *mut Ctx<Fast>,
        thunk: eml_nrt::Thunk,
        entry: *const u8,
        argv: *mut u64,
        out: *mut u64,
    ) -> u64 {
        core::arch::naked_asm!(
            "push rbx", "push rbp", "push r12", "push r13", "push r14", "push r15",
            "push r8",
            "mov rbx, 19", "mov rbp, 20", "mov r12, 21", "mov r13, 22", "mov r14, 23", "mov r15, 24",
            "call {enter}",
            "xor ecx, ecx",
            "xor edx, edx", "cmp rbx, 19", "setne dl", "or rcx, rdx",
            "xor edx, edx", "cmp rbp, 20", "setne dl", "shl rdx, 1", "or rcx, rdx",
            "xor edx, edx", "cmp r12, 21", "setne dl", "shl rdx, 2", "or rcx, rdx",
            "xor edx, edx", "cmp r13, 22", "setne dl", "shl rdx, 3", "or rcx, rdx",
            "xor edx, edx", "cmp r14, 23", "setne dl", "shl rdx, 4", "or rcx, rdx",
            "xor edx, edx", "cmp r15, 24", "setne dl", "shl rdx, 5", "or rcx, rdx",
            "pop r8",
            "mov [r8], rax",
            "mov rax, rcx",
            "pop r15", "pop r14", "pop r13", "pop r12", "pop rbp", "pop rbx",
            "ret",
            enter = sym eml_nrt::eml_enter,
        )
    }
    /// 自分のフレームポインタのレジスタを4つ目の引数にして`walk_frames`へ飛ぶthunk。
    #[unsafe(naked)]
    pub unsafe extern "C" fn fp_thunk(ctx: *mut Ctx<Fast>, entry: *const u8, argv: *mut u64) {
        core::arch::naked_asm!("mov rcx, rbp", "jmp {walk}", walk = sym crate::stub::walk_frames)
    }

    /// 今のフレームポインタのレジスタを`*out`に書いてから`eml_enter`へ飛ぶ。
    #[unsafe(naked)]
    pub unsafe extern "C" fn enter_noting_fp(
        ctx: *mut Ctx<Fast>,
        thunk: eml_nrt::Thunk,
        entry: *const u8,
        argv: *mut u64,
        out: *mut u64,
    ) -> u64 {
        core::arch::naked_asm!("mov [r8], rbp", "jmp {enter}", enter = sym eml_nrt::eml_enter)
    }
}

pub use imp::{clobber_thunk, enter_noting_fp, enter_with_patterns, fp_thunk};
