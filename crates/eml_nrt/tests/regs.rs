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

#[cfg(all(target_arch = "x86_64", windows))]
mod imp {
    use super::*;

    /// 呼ばれる側が保存するレジスタをすべて壊してから`eml_fatal(ctx, 7, 9)`へ飛ぶthunk。
    #[unsafe(naked)]
    pub unsafe extern "C" fn clobber_thunk(ctx: *mut Ctx<Fast>, entry: *const u8, argv: *mut u64) {
        core::arch::naked_asm!(
            "mov rbx, -1", "mov rbp, -1", "mov rdi, -1", "mov rsi, -1", "mov r12, -1", "mov r13, -1", "mov r14, -1", "mov r15, -1",
            "pcmpeqd xmm6, xmm6", "pcmpeqd xmm7, xmm7", "pcmpeqd xmm8, xmm8", "pcmpeqd xmm9, xmm9", "pcmpeqd xmm10, xmm10", "pcmpeqd xmm11, xmm11", "pcmpeqd xmm12, xmm12", "pcmpeqd xmm13, xmm13", "pcmpeqd xmm14, xmm14", "pcmpeqd xmm15, xmm15",
            "mov edx, 7",
            "mov r8d, 9",
            "jmp {fatal}",
            fatal = sym eml_nrt::eml_fatal,
        )
    }

    /// 呼ばれる側が保存するレジスタに決まった値を入れて`eml_enter`を呼び、戻った後に違っていたレジスタのビットを返す。
    /// `eml_enter`の結果は`*out`に書く。5つ目の引数`out`は、入ったときの`[rsp + 40]`にある。
    #[unsafe(naked)]
    pub unsafe extern "C" fn enter_with_patterns(
        ctx: *mut Ctx<Fast>,
        thunk: eml_nrt::Thunk,
        entry: *const u8,
        argv: *mut u64,
        out: *mut u64,
    ) -> u64 {
        core::arch::naked_asm!(
            "push rbx", "push rbp", "push rdi", "push rsi", "push r12", "push r13", "push r14", "push r15",
            "sub rsp, 200",
            "movaps [rsp + 32], xmm6", "movaps [rsp + 48], xmm7", "movaps [rsp + 64], xmm8", "movaps [rsp + 80], xmm9", "movaps [rsp + 96], xmm10", "movaps [rsp + 112], xmm11", "movaps [rsp + 128], xmm12", "movaps [rsp + 144], xmm13", "movaps [rsp + 160], xmm14", "movaps [rsp + 176], xmm15",
            "mov rbx, 19", "mov rbp, 20", "mov rdi, 21", "mov rsi, 22", "mov r12, 23", "mov r13, 24", "mov r14, 25", "mov r15, 26",
            "movq xmm6, rbx", "movq xmm7, rbp", "movq xmm8, rdi", "movq xmm9, rsi", "movq xmm10, r12", "movq xmm11, r13", "movq xmm12, r14", "movq xmm13, r15", "movq xmm14, rbx", "movq xmm15, rbp",
            "call {enter}",
            "xor ecx, ecx",
            "xor edx, edx", "cmp rbx, 19", "setne dl", "or rcx, rdx",
            "xor edx, edx", "cmp rbp, 20", "setne dl", "shl rdx, 1", "or rcx, rdx",
            "xor edx, edx", "cmp rdi, 21", "setne dl", "shl rdx, 2", "or rcx, rdx",
            "xor edx, edx", "cmp rsi, 22", "setne dl", "shl rdx, 3", "or rcx, rdx",
            "xor edx, edx", "cmp r12, 23", "setne dl", "shl rdx, 4", "or rcx, rdx",
            "xor edx, edx", "cmp r13, 24", "setne dl", "shl rdx, 5", "or rcx, rdx",
            "xor edx, edx", "cmp r14, 25", "setne dl", "shl rdx, 6", "or rcx, rdx",
            "xor edx, edx", "cmp r15, 26", "setne dl", "shl rdx, 7", "or rcx, rdx",
            "movq r10, xmm6", "xor edx, edx", "cmp r10, 19", "setne dl", "shl rdx, 16", "or rcx, rdx",
            "movq r10, xmm7", "xor edx, edx", "cmp r10, 20", "setne dl", "shl rdx, 17", "or rcx, rdx",
            "movq r10, xmm8", "xor edx, edx", "cmp r10, 21", "setne dl", "shl rdx, 18", "or rcx, rdx",
            "movq r10, xmm9", "xor edx, edx", "cmp r10, 22", "setne dl", "shl rdx, 19", "or rcx, rdx",
            "movq r10, xmm10", "xor edx, edx", "cmp r10, 23", "setne dl", "shl rdx, 20", "or rcx, rdx",
            "movq r10, xmm11", "xor edx, edx", "cmp r10, 24", "setne dl", "shl rdx, 21", "or rcx, rdx",
            "movq r10, xmm12", "xor edx, edx", "cmp r10, 25", "setne dl", "shl rdx, 22", "or rcx, rdx",
            "movq r10, xmm13", "xor edx, edx", "cmp r10, 26", "setne dl", "shl rdx, 23", "or rcx, rdx",
            "movq r10, xmm14", "xor edx, edx", "cmp r10, 19", "setne dl", "shl rdx, 24", "or rcx, rdx",
            "movq r10, xmm15", "xor edx, edx", "cmp r10, 20", "setne dl", "shl rdx, 25", "or rcx, rdx",
            "mov r10, [rsp + 304]",
            "mov [r10], rax",
            "mov rax, rcx",
            "movaps xmm6, [rsp + 32]", "movaps xmm7, [rsp + 48]", "movaps xmm8, [rsp + 64]", "movaps xmm9, [rsp + 80]", "movaps xmm10, [rsp + 96]", "movaps xmm11, [rsp + 112]", "movaps xmm12, [rsp + 128]", "movaps xmm13, [rsp + 144]", "movaps xmm14, [rsp + 160]", "movaps xmm15, [rsp + 176]",
            "add rsp, 200",
            "pop r15", "pop r14", "pop r13", "pop r12", "pop rsi", "pop rdi", "pop rbp", "pop rbx",
            "ret",
            enter = sym eml_nrt::eml_enter,
        )
    }
    /// 自分のフレームポインタのレジスタを4つ目の引数にして`walk_frames`へ飛ぶthunk。
    #[unsafe(naked)]
    pub unsafe extern "C" fn fp_thunk(ctx: *mut Ctx<Fast>, entry: *const u8, argv: *mut u64) {
        core::arch::naked_asm!("mov r9, rbp", "jmp {walk}", walk = sym crate::stub::walk_frames)
    }

    /// 今のフレームポインタのレジスタを`*out`に書いてから`eml_enter`へ飛ぶ。5つ目の引数`out`は、入ったときの
    /// `[rsp + 40]`にある。
    #[unsafe(naked)]
    pub unsafe extern "C" fn enter_noting_fp(
        ctx: *mut Ctx<Fast>,
        thunk: eml_nrt::Thunk,
        entry: *const u8,
        argv: *mut u64,
        out: *mut u64,
    ) -> u64 {
        core::arch::naked_asm!("mov rax, [rsp + 40]", "mov [rax], rbp", "jmp {enter}", enter = sym eml_nrt::eml_enter)
    }
}

pub use imp::{clobber_thunk, enter_noting_fp, enter_with_patterns, fp_thunk};
