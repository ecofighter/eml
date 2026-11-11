# Appendix 02: jit-rt and sched-split (executable design)

(Where this file and 00-decisions.md differ, 00-decisions.md wins. A line number is a hint; the named item decides. After the advisor review r1, the jit-rt and sched-split plans add: the A1 test `the_code_generator_features_run` and the eml_nrt test `a_thread_with_the_worker_stack_recurses` (advice I-1), cross clippy `--all-targets` in N2, N3 and S3, checkpoints as orchestrator steps, and S0's commit; the plans are the executable text.)

# S2a response (r4): executable design of `jit-rt` and `sched-split`

This is the revised deliverable in full (self-contained: an implementer needs neither the contract document nor the planning directory).

## Changelog r3 -> r4

- **`entries!` result kind `never`** (resolution R-3): a `never` result expands to a Rust function returning `()` and a descriptor row with no result; implemented and tested in the probe (D-8, Appendix A.2).
- **Row list and `entries!`** (R-1): section 2 states that the row list passes the entry identifier `eml_x_<Variant>` and the kind tokens (`obj`, `tobj`, `unit`, `int`, `enum`, `float`, `never`) straight to `entries!`.
- **Fatal codes** (R-2): 1 and 2 are jit-rt's; 3 (`FATAL_RECORDED`) is reserved for jit-core; `impl Raise for HeapError` is defined once, in jit-rt (D-7).
- **T0, native.md L41** (R-15): the generic partial-application sentence is written by T0 only.
- **spec/native.md 「エントリ」 outline** (R-17): gains the subsection 「失敗の手順」 (failure protocols by prefix, the fast-mode `Err` list).
- **Miri budget** (R-18): task R1 measures the `trusted::` commands and updates the budget text of testing.md and `crates/eml_rt/CLAUDE.md`.

Tags: [V] verified (method given or in section 7), [I] inferred.
Base commit for both stages: `432fffb4`. All line numbers refer to that commit.
Every source file the implementer creates is reproduced in Appendix A. The same changes exist as unified diffs for the reviewer in `/private/tmp/claude-501/-Users-arakaki-Projects-eml/81d3cddb-3cbd-4406-a565-d9e03f21aa4d/scratchpad/jit/S2a-probe-rt.patch` and `/private/tmp/claude-501/-Users-arakaki-Projects-eml/81d3cddb-3cbd-4406-a565-d9e03f21aa4d/scratchpad/jit/S2a-probe-sched.patch`; the plan does not depend on them.
Comments in the appendix are placeholders: the implementer rewrites every comment and doc in Japanese by the project rule and cites only `docs/spec` / `docs/implementation` sections that exist in the same commit.

---

## 0. Settled decisions and rulings this design follows

- **Ctx layout (user, contract N-4):** `Native` is `#[repr(C)]`, non-generic, the first field of `#[repr(C)] Ctx<M>`, and `heap` is the field right after it; const asserts pin `offset_of!(Ctx<M>, native) == 0` in both modes and every `CTX_*` const <= 120 and a multiple of 8.
- **Bench gate (contract N-29):** for every gated `bench/rt.sh` row the median delta over 5 alternating pairs against the commit before the task is <= +0.1 %. One exception, for task R1 only: `handle`, `show-int`, `show-int-long`, `slice-short` may rise by at most +0.6 % (one `add` per out-of-line `Heap` call made from a `Ctx` method), on condition that the sum of the instruction deltas over all gated rows is negative; otherwise the task stops and goes to the user with the table and the `heap`-first alternative (measured in r2: no row regresses, `yield` -2.7 % instead of about -10 %).
- **`sched-split` is listed** in roadmap 「段の列」 (user).
- **Native performance first, through clean design; profiler readiness** (user premise): section 1.3.
- **Rulings on this design's earlier challenges:**
  - N-3 is checked by the unit test `trust::the_core_has_no_native_boundary` (the grep is retired: at base it prints a `catch_unwind` of `alloc.rs`'s in-file test module, L3655 [V]).
  - `eml_sched` has the dev-only feature `testing`; it gates items only, is enabled only by dev-dependencies, and is deleted entirely by jit-tasks (N-41).
  - `RunStats` loses `#[non_exhaustive]`; one `#[verifier::rlimit(20)]` for the box-value reroute: both conform.
  - `eml_enter` returns `u64` (0, or code and site); `eml_fatal(ctx, code: i64, site: i64)` records nothing.
  - `Native` has six fields; per-worker state that only Rust reads lives in the native runtime's worker struct behind `runtime` (N-5).
  - sched-split has no `gui` / `tls` feature; the two `cfg_attr(not(feature = "gui"), ..)` attributes of `cells.rs` are deleted.

---

## 1. Conclusion

### 1.1 jit-rt

**D-1 `Native` and its place in `Ctx`.** In `crates/eml_rt/src/heap/ctx.rs`, inside `verus!`:
```rust
#[repr(transparent)] pub struct StopPtr(*const AtomicU32);
#[repr(transparent)] pub struct JmpPtr(*mut ());
#[repr(transparent)] pub struct RtPtr(*mut ());

#[repr(C)]
pub struct Native {        // offsets (also the offsets inside Ctx)
    yielding: YieldState,  //  0: target 0, clause 8, cont 16, abort 24
    fault: u64,            // 32   0 = none, 1 = a Fault is parked in Ctx.fault_value
    stop: StopPtr,         // 40   == Arc::as_ptr(&ctx.word)
    stack_limit: usize,    // 48   0 until the native runtime sets it
    jmp: JmpPtr,           // 56   innermost JmpBuf, null outside eml_enter
    runtime: RtPtr,        // 64   opaque per-worker pointer owned by the native runtime, null by default
}                          // size 72

#[repr(C)]
pub struct Ctx<M: Mode> {
    native: Native, heap: Heap<M>, evv: Word, roots: Vec<Option<Word>>, control: Arc<Control>,
    word: Arc<AtomicU32>, max_depth: usize, depth_limit: usize, max_evv: usize, stats: CtxStats,
    host: HostRef, fault_value: Option<Fault>,
}
```
`yielding` moves into `Native` (not mirrored). `heap` is at offset 72; `fault_value` is last. `Native`, `StopPtr`, `JmpPtr`, `RtPtr` are `pub` items of the private module and are not re-exported from the crate root.
- Why: generated code reads `Native` after every non-tail call, at every function entry and after every `faults` row; at offsets 0..64 each read has the shortest encoding on every target, and the yield words are in `ldp` / `stp` reach for the runtime's own yield operations. The price is one `add` to form `&mut ctx.heap` in an out-of-line `Heap` call (1.3). The three pointers need `unsafe impl Send` wrappers because raw pointers make `Ctx` `!Send`.
- Rejected: `heap` first (no shared-runtime row regresses, but every `Native` read on x86-64 gets a 32-bit displacement and `yield` gains 2.7 % instead of about 10 %; it is the fallback N-29 names); `usize` fields for `jmp` / `runtime` (int-to-pointer casts fail Miri's strict provenance in eml_nrt); a `fault` kind encoding (generated code only tests non-zero).
- Conformance: the const asserts and pin test of R1; `grep -rn 'fn set_stop\|fn set_jmp' crates/eml_rt/src` empty.

**D-2 Offset consts.** Outside `verus!`, next to `Ctx`, re-exported from `heap/mod.rs` and `lib.rs`:
`CTX_YIELD_TARGET` (0), `CTX_FAULT` (32), `CTX_STOP` (40), `CTX_STACK_LIMIT` (48), `CTX_JMP` (56), each `offset_of!(Ctx<Fast>, native) + offset_of!(Native, f) [+ offset_of!(YieldState, target)]`. Const asserts: `offset_of!(Ctx<Fast>, native) == 0` and `offset_of!(Ctx<Checked>, native) == 0`; `offset_of!(Ctx<M>, heap) == size_of::<Native>()` for both modes; `size_of::<Native>() == 72`; every `CTX_*` const `<= 120` and `.is_multiple_of(8)` (a `while` loop over an array of the five, in a `const` block).
`CTX_YIELD_ABORT` is added by jit-effects with its first reader. `runtime` has no const: only Rust reads it.

**D-3 Accessors and fault parking** (safe `pub fn`s inside `verus!`; each `&mut` one `ensures old(self).wf() ==> final(self).wf()`, `old(self).held_ok() ==> final(self).held_ok()`, `heap_spec` / `evv_spec` / `yield_spec` unchanged, proved by one new `proof fn lemma_same_words`):
```rust
pub fn set_fault(&mut self, fault: Fault) -> (previous: Option<Fault>)   // parks `fault`, returns what was parked before; ensures fault_parked()
pub fn take_fault(&mut self) -> Option<Fault>                             // ensures !fault_parked()
pub closed spec fn fault_parked(&self) -> bool                            // self.native.fault != 0
pub fn stack_limit(&self) -> usize
pub fn set_stack_limit(&mut self, limit: usize)
pub fn runtime(&self) -> *mut ()
pub fn set_runtime(&mut self, runtime: *mut ())
pub fn jmp(&self) -> *mut ()                      // no setter: only the entry stubs write it, through CTX_JMP
pub fn stop_word(&self) -> *const AtomicU32       // no setter
```
`fault` and `fault_value` are written only by these two functions and `for_worker`. Contract: a wrapper parks at most one fault per call and generated code tests the word right after; `set_fault` returning `Some` means that contract was broken, and callers `debug_assert!` on `None`.
- Rejected: the pair as a conjunct of `Ctx::wf` (touches every proof for a fact no proof needs); a `requires !fault_parked()` (callers are unverified code; the returned value is checkable at run time).

**D-4 T18.** One trust row, one `trust.rs` entry `("heap/ctx.rs: external_body stop_of", "T18")` for
```rust
#[verifier::external_body]
fn stop_of(word: &Arc<AtomicU32>) -> StopPtr { StopPtr(Arc::as_ptr(word)) }
```
plus the three `unsafe impl Send` (an allowed spelling outside `verus!`, `trust.rs` L254-259). Null pointers are `core::ptr::null_mut()` (no trust item) [V]. Row text for object-model.md 「`unsafe` の核」:

| Field | Pointee owner | Valid while | Written by | When the `Ctx` crosses threads |
|---|---|---|---|---|
| `stop` | the `Arc<AtomicU32>` in `Ctx.word` (single owner; `stop == Arc::as_ptr(&word)`) | the `Ctx` lives | `for_worker` only; `word` is private and never reassigned; no setter exists | unchanged; an `AtomicU32` is `Sync` |
| `jmp` | the `eml_enter` frame that linked it | between that frame's link and unlink | the entry stubs of the native runtime only | null (no enter is active) |
| `runtime` | the native runtime's per-worker struct | from worker start to worker end | the native runtime (`set_runtime`) at worker start / end | null (cleared before the `Ctx` is handed back) |

Miri column: "the pointer read on another thread after the `Ctx` moved; `runtime` round trip; a parked fault is taken once and is not leaked by a dropped `Ctx`". Test: Appendix A.1.

**D-5 rt-layout exports** (mechanics: spec functions keep the literals of object-model.md; exec bodies use the consts; Verus proves exec == spec; a unit test pins each value):
- `Meta` (`meta.rs` L153-161): `pub const TAG_MASK: u32 = 0xFFFF; SCAN_SHIFT: u32 = 16; SCAN_MASK: u32 = 0xFF; KIND_SHIFT: u32 = 24; KIND_MASK: u32 = 0xF; LIN: u32 = 1 << 31;` (`KIND_SHIFT`, `KIND_MASK`, `LIN` exist as private consts). `Meta::new` uses `SCAN_SHIFT`; `tag()` becomes `(self.0 & Meta::TAG_MASK) as u16`; `scan()` becomes `((self.0 >> Meta::SCAN_SHIFT) & Meta::SCAN_MASK) as u8`; `lemma_consts` gains the three new equalities.
- `Word` (`word.rs` L45-47): `pub const IMM_TAG: usize = 1; pub const IMM_SHIFT: u32 = 1;` used by `imm_bits`, `is_imm_bits`, `is_ptr_bits`, `imm_value_bits` (L243-275). `Word::UNIT`, `Word::EMPTY`, `MIN_INT`, `MAX_INT` exist.
- `rc` (`rc.rs`, `lib.rs` L30): `pub mod rc;`; `LOCAL_MAX`, `DUP_LIMIT` become `pub` (`DROP_LIMIT` is); four new consts carry the fast-path rule so generated code repeats no rc number: `DUP_FAST_BIAS: u32 = 1`, `DUP_FAST_SPAN: u32 = (LOCAL_MAX - 1) as u32`, `DECREF_FAST_BIAS: u32 = 2`, `DECREF_FAST_SPAN: u32 = (LOCAL_MAX - 2) as u32`; `dup_step` / `decref_step` (L93, L113) use them. Inline rule for jit-core: `(rc as u32).wrapping_sub(BIAS) < SPAN` => store `rc ± 1`, else out of line (one subtract, one compare, one branch, one store).
- `layout`: `pub const BOX_VALUE_WORD: usize = 0; pub const BOX_WORDS: usize = BOX_VALUE_WORD + 1;` with the site table of R3.
- Rejected: literals in the `Meta` / `Word` spec functions replaced by consts (meta.rs L302 states the opposite rule on purpose); a source-scan test for the box word like `only_layout_spells_the_count_word_rule` (the literal is `0`, and `String`, `Bytes`, `Raw`, `Foreign` and the free list legitimately use slot 0 at 60+ sites; the check is the fixed residual list of R3).

**D-6 `eml_nrt` crate and the stub ABI.** New crate `crates/eml_nrt` (dependency in jit-rt: `eml_rt` only; the `eml_sched` edge arrives in jit-core with its first use).
```
crates/eml_nrt/
  Cargo.toml                 autotests = false, [[test]] integration
  src/lib.rs                 #![deny(unsafe_code)]; re-exports; TABLES; entries()
  src/enter.rs               Thunk, Fatal, FATAL_PANICKED, FATAL_FAULT, Raise, enter(), detached(), the entry `eml_fatal`
  src/entries.rs             Kind, Entry, entries! (+ __entry!, __kind!, __result!), unit tests `entries::tests`
  src/arch/mod.rs            cfg selection
  src/arch/aarch64.rs        AAPCS64 (macOS, Linux)
  src/arch/x86_64_sysv.rs    SysV x86_64
  src/arch/x86_64_win64.rs   Win64 x86_64
  src/arch/unsupported.rs    every other target (incl. aarch64 Windows): SUPPORTED = false, eml_enter panics
  src/trust.rs               #[cfg(test)] unsafe-file list (copy of eml_native's scanner)
  examples/nrt_bench.rs      cost of the stub and of the two entry forms (bench/rt.sh)
  tests/main.rs, tests/stub.rs, tests/regs.rs
```
```rust
pub type Thunk = unsafe extern "C" fn(ctx: *mut Ctx<Fast>, entry: *const u8, argv: *mut u64);
/// 0 = returned (results in argv[0..]); otherwise (code << 32) | site with code != 0
pub unsafe extern "C" fn eml_enter(ctx: *mut Ctx<Fast>, thunk: Thunk, entry: *const u8, argv: *mut u64) -> u64;
pub unsafe fn enter(ctx, thunk, entry, argv) -> Result<(), Fatal>;   // Rust wrapper over eml_enter
pub struct Fatal { pub code: u32, pub site: u32 }
pub const FATAL_PANICKED: u32 = 1;   // a shim caught a Rust panic; no payload
pub const FATAL_FAULT: u32 = 2;      // a Fault is parked in the Ctx: ctx.take_fault() returns Some
pub fn detached(ctx: &Ctx<Fast>) -> bool;   // jmp and runtime are null
pub const SUPPORTED: bool;
```
- `JmpBuf` is part of the stub's own frame: `#[repr(C)] { prev: *mut JmpBuf, ctx: *mut Ctx<Fast>, <callee-saved registers> }`. Saved sets: AAPCS64 x19-x28, x29, x30, d8-d15; SysV rbx, rbp, r12-r15; Win64 rbx, rbp, rdi, rsi, r12-r15, xmm6-xmm15 (full 128 bits, `movaps`). On Win64 the buffer sits 32 bytes (shadow space) above the stack pointer. Every asm offset is a `const` operand derived from `size_of` / `offset_of!` of `JmpBuf` and `eml_rt::CTX_JMP`; no layout number is written in the asm text.
- Link / unlink: on entry `buf.prev = ctx.jmp; ctx.jmp = &buf`; on normal return and in the longjmp half `ctx.jmp = buf.prev`. Nested enters chain through `prev`.
- The stub calls `thunk(ctx, entry, argv)` with the C convention and knows nothing about `tail`.
- **Frame record:** each stub sets up a standard frame record before the call (aarch64: `x29` points at the saved `x29, x30` pair inside the buffer; x86-64: `push rbp; mov rbp, rsp`), so a frame-pointer walk goes from generated code through the thunk and the stub into the Rust caller (1.3).
- The longjmp half is `pub(crate) unsafe extern "C" fn longjmp(ctx, value: u64) -> !` in each arch file; only `eml_fatal` calls it.
- Not saved: MXCSR / FPCR control bits (generated code and eml_rt do not change them).
- Rejected: `JmpBuf` inside `Ctx` (nesting needs one per enter); OS / CRT `setjmp` (SEH, signal masks); a build script with an external assembler (`#[unsafe(naked)]` + `naked_asm!` is stable on 1.99.1 [V]); a thread-local for the fatal payload (no thread-locals in the native runtime).

**D-7 `eml_fatal`, the fatal rule, and how a caller reads the error.** `eml_fatal` is an entry declared by the macro: `plain fn eml_fatal(ctx, code: i64, site: i64) -> !`; it debug-asserts `code != 0`, `0 <= site <= u32::MAX`, `ctx.jmp` non-null, then `arch::longjmp(ctx, (code as u32 as u64) << 32 | site as u32 as u64)`. It records nothing.
- Fatal rule: a longjmp crosses only `tail` frames, thunks, and the Rust frames of the one entry that detected the error plus `eml_fatal`; those Rust frames hold no value with a destructor at the call.
- Re-entry rule: a Rust entry that re-enters eml code passes the raw `ctx` and holds no `&` / `&mut Ctx`, no reference to the worker struct and no `Drop` value across `eml_enter` or `eml_fatal`.
- Because `eml_fatal` records nothing, the detector moves the error to where its code says (below) before it raises, and re-raising across a nested enter is `eml_fatal` with the same code and site.
- A wrapper body never names `eml_fatal`: in a `shim` the call comes from the macro after the `catch_unwind` closure has returned.
- **Reading the error (answers S4 I-1, S2b I-3):** the Rust code that called `enter` gets `Err(Fatal { code, site })` and reads the payload by code. `FATAL_PANICKED`: none (an internal error). `FATAL_FAULT`: `ctx.take_fault()` is `Some(fault)`; a `HeapError` arrives as `Fault::Heap(e)`. Code 3 is reserved for jit-core's `FATAL_RECORDED` (the worker struct behind `ctx.runtime` holds the recorded error); jit-core defines the const next to these two in `enter.rs`. Any further code is defined there by the stage that raises it, together with where its payload is. The same holds for a nested `enter` inside an entry: the inner call returns the `Fatal` to that entry, which decides. jit-rt provides no `take_fatal -> RuntimeError`: building a `RuntimeError` needs the function name of `site`, which only jit-core's driver has.
- `pub trait Raise { fn park(self, ctx: &mut Ctx<Fast>) -> Fatal; }` is the shim's `Err` arm. jit-rt implements it for `Fault` (`set_fault`, `FATAL_FAULT`, site 0), `HeapError` (as `Fault::Heap`), and `Fatal` (returns itself; used by tests and by entries that already recorded their error). These are the only definitions of the first two; later stages add impls for their own error types and do not redefine these.
- `eml_fatal(` appears in eml_nrt code only in `entries.rs` (`__entry!` shim arm, twice) and `enter.rs` (its declaration).

**D-8 Entry macro and table format.** `eml_nrt::entries!` (`#[macro_export]`, so test crates declare echo entries with the same macro):
```rust
eml_nrt::entries! {
    /// table doc
    pub const NAME;                                   // expands to: pub const NAME: &[eml_nrt::Entry]
    /// plain: the body is the function body; `ctx` is the raw *mut Ctx<Fast>.
    pub plain fn eml_fatal(ctx, code: i64, site: i64) -> ! { ... }
    /// shim: the body runs inside catch_unwind with `ctx: &mut Ctx<Fast>` and has type Result<R, E: Raise>.
    pub shim fn eml_rt_example(ctx, a: i64, b: f64) -> i64 { ... Ok(v) }
}
pub enum Kind { I64, F64 }
pub struct Entry { pub symbol: &'static str, pub address: fn() -> *const u8, pub params: &'static [Kind] /* after ctx */, pub result: Option<Kind> }
pub fn entries() -> impl Iterator<Item = &'static Entry>   // over `const TABLES: &[&[Entry]]` in lib.rs
```
- Parameter and result types are matched as tokens: `i64`, `f64`, the row kind names `obj`, `tobj`, `unit`, `int`, `enum` (all `Kind::I64`, Rust `i64`) and `float` (`Kind::F64`, `f64`), and for the result also `!`, `never`, or nothing; anything else is a compile error, so every parameter is `i64` or `f64` by construction. Result-less entries are allowed (`result: None`). `-> never` is the row kind of a row that never continues (`exit`, `panic`, `fail_io`): the entry is a Rust function returning `()` that returns normally, its descriptor row has no result, and generated code branches unconditionally to the cold block after the call. `-> !` is for an entry that really does not return (`eml_fatal`).
- An entry's symbol is its function identifier (`stringify!`); there is no override. A list that derives entries from extern rows spells the identifier `eml_x_<Variant>` itself and passes `#[allow(non_snake_case)]` through the attribute position.
- Shim body rule, written once in `__entry!`'s `shim` arm: `match catch_unwind(AssertUnwindSafe(|| { let ctx = unsafe { &mut *ctx }; body })) { Ok(Ok(v)) => v, Ok(Err(e)) => { let f = Raise::park(e, unsafe { &mut *ctx }); eml_fatal(ctx, f.code as i64, f.site as i64) } Err(p) => { drop(p); eml_fatal(ctx, FATAL_PANICKED as i64, 0) } }`. No `&mut Ctx` lives across `eml_fatal`. (A payload whose `Drop` panics aborts the process at the `extern "C"` boundary; accepted.)
- `shim` costs nothing on the fast path (1.3), so every entry that can run arbitrary Rust is a `shim`. `plain` exists for `eml_fatal` (it is the fatal path itself: it never returns and holds no destructor); no other `plain` entry is planned, and adding one needs a cannot-panic argument written in spec/native.md.
- `address` is a `fn() -> *const u8` (a non-capturing closure): the table is a plain `const`.
- A stage adds a table by declaring it in its module and appending it to `TABLES`.
- Symbol names: `eml_rt_*`, `eml_x_*`, `eml_nrt_*`, `eml_enter`, `eml_fatal`, `eml_run_pending` (a unit test checks `entries()`; test-only entries declared outside eml_nrt use `eml_t_*`).
- No `#[no_mangle]` in the jit stages (the JIT registers addresses with `JITBuilder::symbol`); native-aot adds it in the macro, one place.
- Rejected: `inventory` / `linkme` (new dependency, linker sections on three OSes); one central macro invocation for all entries; echo entries in the production table; a third macro form without `catch_unwind` (1.3: nothing to gain).

**D-9 `eml_native` in jit-rt.** `Cargo.toml` gains `eml_lir`, `eml_nrt`, `eml_rt`. New `src` files (generic over `cranelift_module::Module`; none names `JITModule`):
- `src/mem.rs` — the memory-access module, the only file of `eml_native/src` that names `eml_rt` (jit-core's JIT host file reaches `HostRef` / `Control` through eml_nrt's re-exports; 00-decisions C-9). In jit-rt: `use eml_lir::Shape; use eml_rt::layout; const _: () = assert!(Shape::REUSE_SCANNED_LIMIT as usize <= layout::INLINE_SCAN_LIMIT);`.
- `src/abi.rs` — `pub type Built<T> = Result<T, Box<ModuleError>>`; `pub fn isa(pic: bool) -> Option<OwnedTargetIsa>` — **the one Cranelift flag constructor for eml code, adapters, helpers and thunks** (00 U-13 adds `JIT_REGION_BYTES` and `jit_builder`, which builds every JIT module from this ISA and one reserved region): `opt_level=speed`, `is_pic=<pic>`, `preserve_frame_pointers=true`, `enable_probestack=true`, `probestack_strategy=inline`; `pub fn clif(Kind) -> Type`; `pub fn tail_signature(params: &[Kind], rets: &[Kind]) -> Signature` (`tail (ctx, leaves...) -> leaves...`; the one `CallConv::Tail`); `pub fn entry_signature(&Entry, CallConv) -> Signature`; `pub fn declare_entry<M: Module>(&mut M, &Entry) -> Built<FuncId>` (the one `Linkage::Import`).
- `src/thunk.rs` — `pub fn define_thunk<M: Module>(&mut M, name: &str, params: &[Kind], rets: &[Kind]) -> Built<FuncId>`: default-cc `(ctx, entry, argv)`; loads parameter i from `argv + 8*i` as `i64` or `f64`, `call_indirect`s `entry` with `tail_signature(params, rets)`, stores result j to `argv + 8*j`. Per kind sequence, not per arity.
- `lib.rs`: `pub mod abi; mod mem; pub mod thunk;`. `trust.rs` `FILES` gains the three files (none has `unsafe`).
- `bridge.rs` keeps its own `new_module()` (L131-147): C bridges are default-convention only and need neither frame pointers nor stack probes. No per-function flag override and no size threshold that lowers `opt_level` exists.
- `abi` and `thunk` are `pub` because the ABI tests are integration tests (nothing in `src/` may name `JITModule` except `bridge.rs` and jit-core's JIT host).

**D-10 Tests added by jit-rt.**
| Where | Tests | Runs on |
|---|---|---|
| `crates/eml_rt/src/heap/tests/trusted.rs` | `the_native_pointers_follow_the_ctx_across_threads` (T18) | all, Miri both models [V] |
| `crates/eml_rt/src/heap/tests/ctx.rs` | `the_native_offsets_are_pinned` | all |
| `crates/eml_rt/src/trust.rs` | `the_core_has_no_native_boundary` | all |
| `meta.rs`, `word.rs`, `rc.rs`, `layout.rs`, `heap/tests` | pin tests of the new consts; box read-back | all |
| `crates/eml_nrt/src/entries.rs` `entries::tests` | `entries_run_and_describe_themselves`, `the_table_has_only_named_prefixes` | all, Miri both models [V] |
| `crates/eml_nrt/src/trust.rs` | three scanner tests | all |
| `crates/eml_nrt/tests/stub.rs` | `a_thunk_returns_through_enter`, `the_stub_keeps_the_frame_record_chain`; in `mod fatal`: `a_fatal_from_a_deep_frame_returns_from_enter`, `enters_nest_and_unlink`, `callee_saved_registers_survive_a_fatal`, `a_shim_cuts_to_enter_on_an_error_and_on_a_panic`, `a_heap_error_is_parked_before_the_cut` | four CI targets; not Miri |
| `crates/eml_native/tests/abi.rs` | `mixed_int_and_float_arguments_reach_an_entry`, `tail_calls_and_multiple_results_run`; in `mod fatal`: `a_fatal_cuts_through_tail_frames` | four CI targets; not Miri |
- The register test uses two test-only naked functions per ABI in `tests/regs.rs`: `enter_with_patterns` loads a distinct value into every callee-saved register, calls `eml_enter`, and returns a bitmask of registers that differ; `clobber_thunk` overwrites all of them and jumps to `eml_fatal(ctx, 7, 9)`. Dropping two restores from the longjmp half makes it fail [V].
- The frame-record test (N-35) uses two more naked helpers per ABI in `tests/regs.rs`: `enter_noting_fp` stores the frame-pointer register to `*out` and jumps to `eml_enter`; `fp_thunk` passes its own frame-pointer register as a fourth argument to the Rust function `walk_frames`, which follows frame records while the frames are live and reports whether, within three records, it reaches the value `enter_noting_fp` stored. Replacing the stub's `add x29, sp, ..` by a `nop` makes it fail [V].
- The echo test declares `eml_t_echo_ii`, `eml_t_echo6` (i64/f64 alternating), `eml_t_echo9` (f64/i64 alternating), `eml_t_echo18` (alternating; stack arguments on every ABI) with `entries!`, registers them next to `eml_nrt::entries()`, builds for each a `tail` caller and a thunk from the descriptor only, and compares what the entry saw (written through `ctx.runtime()`) with what was sent. It asserts that every (parameter kinds, result kind) of `eml_nrt::entries()` has an echo entry. Cost of that rule later: one echo function per distinct shape; rows have at most 5 parameters, so at most 63 parameter shapes exist. The stage that adds the std rows (jit-externs) may generate the echoes from its row list.
- Fatal-path tests live in modules named `fatal` so the ASan job skips them (`-- --skip fatal::`): a longjmp leaves ASan's stack poison behind.

**D-11 CI steps** (`.github/workflows/ci.yml`; final form after jit-rt, the task that makes each edit is named in section 5):
- step `dependency edges of eml_native` (L95-102):
  ```
  test "$(cargo tree -p eml_vm -e normal --target all | grep -c -E 'cranelift|eml_native|eml_nrt')" -eq 0
  test "$(cargo tree -p eml_nrt -e normal --target all | grep -c -E 'cranelift|eml_lir|eml_core_ir|eml_vm|eml_native')" -eq 0
  test "$(cargo tree -p eml_native -e normal --target all | grep -c -E 'eml_vm')" -eq 0
  test "$(cargo tree -p eml_lir -e normal --target all | grep -c -E 'eml_rt')" -eq 0
  test -z "$(grep -rn unsafe crates/eml_extern/src)"
  ```
  (all four counts are 0 on the probe [V]). sched-split appends `test "$(cargo tree -p eml_sched -e normal --target all | grep -c -E 'cranelift|eml_lir|eml_vm|eml_native|eml_nrt')" -eq 0` and `test "$(cargo tree -p eml_cli -e normal,build -f '{p} {f}' | grep -c 'eml_sched.*testing')" -eq 0` (both 0 on the probe [V]).
- clippy row L80: `cargo clippy -p eml_io -p eml_vm -p eml_nrt --all-targets -- -D warnings`.
- miri job: new step after the `eml_io` one, `nix develop .#miri -c cargo miri test -p eml_nrt --lib -- entries::`.
- asan job: `cargo test ... -p eml_native -- --skip fatal::`; new step `cargo test ... -p eml_nrt -- --skip fatal::`.
- The test job needs no edit: `cargo nextest run` runs the stub and ABI tests on ubuntu-24.04, ubuntu-24.04-arm, macos-15, windows-2025.

**D-12 CI gate procedure (the stub and ABI tests must be green on the four targets before the stage closes).** CI runs on pushes to `main`, pull requests and `workflow_dispatch` only (ci.yml L4-8), so a branch push alone runs nothing.
1. The stage is developed on the branch `plan/jit-rt`. Local pre-push checks are part of each task's completion criteria (host tests; `nix develop .#cross -c cargo build -p eml_nrt --target <t>` for the three other targets, which assembles their stubs).
2. Checkpoint G1 after task N3, G2 after task A1, G3 on the closing commit. At a checkpoint the executor **asks the user for permission to push**, then:
   ```
   git push -u origin plan/jit-rt
   gh workflow run CI --ref plan/jit-rt
   gh run list --workflow CI --branch plan/jit-rt --limit 1 --json databaseId,headSha,status
   gh run watch <databaseId> --exit-status
   ```
   Pass: the run's `headSha` is the checkpoint commit and `gh run watch` exits 0.
3. Red run: `gh run view <databaseId> --log-failed`. If the failure is in this stage's code, fix it in a **new commit** on the branch (`Fix the <ABI> stub: <what>`; no amend, no force-push), re-run the local checks, and repeat step 2 (push needs the user's permission each time). The fix commits stay in the history; the Close commit message lists them.
4. If a target cannot be made green (for example Cranelift's `tail` on Windows, or a platform that rejects the hand-written `ret`): stop and report target, test and log to the user. No `cfg` exclusion of a stub or ABI test without a user decision recorded in the plan.
5. Diagnostic hint for step 3: a crash only in `fatal::` tests with exit `0xC0000409` (Windows) or SIGILL / SIGSEGV at the `ret` of the longjmp half points at a shadow-stack / branch-protection policy (assumption A-2), not at a wrong register list.

**D-13 Unsupported targets.** `eml_nrt::SUPPORTED` is `true` on aarch64 non-Windows and x86_64 (SysV, Win64), `false` elsewhere; there `eml_enter` exists and panics. jit-core's `--native` refusal reads the const.

**D-14 `Ctx::bytes_empty` is infallible** (requested by jit-externs: a `Pure` row must not fail in fast mode). It allocates the empty object with `alloc_bytes_room(0)` without the per-request budget check, like `array_empty`; its postcondition becomes `r is Ok` (the `Result` type stays, callers are unchanged). The memory limit is still enforced by the page accounting and the stop bit at the next safepoint.

### 1.2 sched-split

**D-20 Crate shape.** `crates/eml_sched` (`[lib] doctest = false`, `autotests = false`, `[[test]] integration`), dependencies `eml_io`, `eml_rt`, and `libc` under `cfg(unix)`; `[features] testing = []` (D-23). No `tls` / `gui` / `gui-native` in this stage; jit-externs adds them with the first arm that needs them (`tls = ["dep:eml_tls", "eml_io/tls"]`, `gui = ["dep:eml_gui"]`, `gui-native = ["gui", "eml_gui/native"]`, forwarded from eml_vm, eml_nrt, eml_native and eml_cli).
```
crates/eml_sched/src/lib.rs      pub mod cells; pub mod config; pub mod doorbell; pub mod driver; pub mod error; pub mod park;
            src/cells.rs         moved whole from eml_vm/src/cells.rs
            src/error.rs         moved whole (VmFault, KEPT_PENDING, SourceLocation, RuntimeError)
            src/park.rs          moved whole (Progress, PollSource, Pending, Park, ParkFactory, DefaultPark, Guard)
            src/doorbell.rs      moved whole (Doorbell)
            src/driver.rs        moved whole (JOB_QUIESCE_LIMIT, Clock, MadeDriver, RunClock, DriverKind, Signal, VirtualSignal, floor_ms, ceil_ms) + NO_DRIVER from eml_vm/src/sched.rs L286-287
            src/config.rs        new file: RunStats, Finished, RunConfig (+ Default, with_*) from eml_vm/src/lib.rs L62-176, L190-285; Sched (+ seed) from sched.rs L101-121; Seat from workers.rs L206-213
            tests/main.rs, tests/imports.rs
```
- Why whole files: `error.rs` needs `cells::CellFault`; `RunConfig` needs `Sched`, `Clock`, `Doorbell`, `VirtualSignal`, `Seat`, `ParkFactory`; `Finished` needs `RunStats`; `RunClock::now` needs `NO_DRIVER`.
- Stays in eml_vm (until jit-tasks): `Policy`, `Scheduler`, `Memory`, `Normal`, `Sequential`, `Seeded`, `TaskId`, `workers.rs`, `monitor.rs`, `jobs.rs`, `counters.rs`, `RunCounts`, `WORKER_STACK`, `execute`, `Machine`.
- Must not depend on Cranelift, `eml_lir`, `eml_vm`, `eml_nrt`, `eml_native` (CI line of D-11).

**D-21 eml_vm keeps its paths.** `crates/eml_vm/src/lib.rs` replaces `mod cells; mod doorbell; mod driver; mod error; mod park;` by `pub(crate) use eml_sched::{cells, doorbell, driver, error, park};` and adds `pub use eml_sched::config::{Finished, RunConfig, RunStats};`. `sched.rs` adds `pub use eml_sched::config::Sched;` and `pub(crate) use eml_sched::driver::NO_DRIVER;`. `workers.rs` adds `pub use eml_sched::config::Seat;`. Every `crate::cells::…`, `crate::driver::…`, `crate::VmFault` and every `pub use` line of `lib.rs` (L45-56) keeps resolving; `eml_cli` and `eml_test_support` need no source change except D-24's sites. `eml_vm` and through it `eml_cli` re-export the same public names as today. `RuntimeError` and its `Display` exist once, in `eml_sched/src/error.rs`.

**D-22 Visibility rule.** In the six moved files every `pub(crate)` becomes `pub` (cells 247, driver 11, park 2, error 1 [V]); nothing else changes visibility. This gives the native runtime the public constructors and fields it needs (`RuntimeError`, `VmFault`, `SourceLocation`, `Finished`'s fields, `RunStats::default()`, `RunConfig::default()` + `with_*`, `Clock`, `RunClock`, `DriverKind`). Check: `grep -c 'pub(crate)' crates/eml_sched/src/*.rs` is 0 for every file.

**D-23 Test fixtures.** In `cells.rs` these `#[cfg(test)]` items become `#[cfg(any(test, feature = "testing"))]` — 25 attributes [V]: `type Hook`, the `hook` field of `Mail` and its initialiser, `Mail::set_hook`, `Mail::run_hook`, `struct Unposted`, `impl Post for Unposted`, and the functions `solo`, `running_task`, `test_ctx`, `mode_ctx`, `terminate_waiter`, `sleeping_task`, `member_task`, `owned_task`, `task_on`, `placed_task`, `stream_on`, `arrival_of`, `flag_cancel`, `limit_task`, `limit_flag`, `op_left`, `test_chan`, `send_now`, `park_receiver`. `crates/eml_vm/Cargo.toml` `[dev-dependencies]` gains `eml_sched = { workspace = true, features = ["testing"] }`.
The one `cfg!(test)` expression of the file (in `Registry::host_bin`, `if bin.is_none() && cfg!(test) {`) stays `cfg!(test)`: widening it would make the library under eml_vm's integration tests and the UI tests more lenient than a release build. The five `jobs::tests` that relied on it get the bin from their fixture (`crates/eml_vm/src/jobs.rs` `settled`): after `let mut driver = TestDriver::new(None);` add `registry.lock().set_bin(0, Arc::new(cells::HostBin::new(driver.bin(), false)));` [V].
The feature gates items only: no `cfg!(feature = "testing")` expression exists. With it on, the library differs from a release build by one always-`None` field (`Mail.hook`; `run_hook` is called only from `#[cfg(test)]` code of eml_vm) and by unused public functions. It is enabled only by dev-dependencies, the CI edge step checks that the `eml` binary's dependency tree does not have it (D-11), and jit-tasks, which moves `sched.rs`, `workers.rs`, `monitor.rs` and `jobs.rs` into `eml_sched`, deletes it entirely; a test that still needs a fixture then is rewritten on the public API in that stage.

**D-24 `RunStats` loses `#[non_exhaustive]`** (`RunConfig` keeps it). Five "default then assign" sites become struct-update expressions with the same fields and values: `crates/eml_test_support/src/lib.rs` `channel_independent` (L317-323), `driver_independent` (L331-339); `crates/eml_cli/tests/ui.rs` L532-535, L575-578, L612-616.

**D-25 The import test moves with the file.** `crates/eml_vm/tests/tasks.rs` `the_task_cells_import_only_std_and_the_runtime` (L276-293) is deleted there and recreated unchanged (same name and body, `include_str!("../src/cells.rs")`) in `crates/eml_sched/tests/imports.rs`.

**D-26 Commands that name the moved tests.** `cargo miri test -p eml_vm --lib -- cells::a_read cells::a_write counters::` splits into `cargo miri test -p eml_sched --lib -- cells::a_read cells::a_write` and `cargo miri test -p eml_vm --lib -- counters::` (ci.yml L238-241, CLAUDE.md L51, testing.md L197, L202, L706, L787). TSan's `-p eml_vm --lib counters::` is unchanged. Clippy gains the row `cargo clippy -p eml_sched --all-targets -- -D warnings` (the build with `testing` off).

### 1.3 Native cost model, forgone optimizations, profiler hooks

Cost of what jit-rt lays down, on the native fast path:

| Path | Cost | Why this is the floor |
|---|---|---|
| Reading a `Native` field from generated code (yield check, fault check, safepoint) | one load at `ctx + CTX_*`, offsets 0..56: an 8-bit displacement on x86-64 (none for `yielding.target`), one scaled `ldr` on aarch64 | the field is at the head of the object `ctx` points at |
| A runtime entry (or a VM `Ctx` method) calling an out-of-line `Heap` operation | one `add` to form `&mut ctx.heap` (offset 72) | the price of `Native` first; measured below |
| Calling an entry | a direct C call with `ctx` and `i64` / `f64` in registers; no wrapper layer between generated code and the entry | descriptor-built import |
| `shim` vs `plain` entry | 0 instructions difference on the fast path: 10^8 calls cost 818,705,818 vs 818,687,270 instructions (`nrt_bench`, release, aarch64) [V] | the landing pad is off the fast path |
| `eml_enter` round trip | about 52 instructions including the Rust wrapper and a trivial thunk (10^7 round trips: 538.7 M instructions; measured before the frame record, which adds 1 instruction on aarch64 and 3 on x86-64) [V]. Paid once per entry from Rust (run start; later task resumes and re-entries), never per eml call | saves and restores 20 registers on aarch64 |
| `eml_fatal` | cold | - |
| Inline `dup` / `decref` rule exported in D-5 | subtract, compare, branch, store | one-compare range test, as eml_rt's own |

Effect of the layout on the shared runtime (`bench/rt.sh`, instructions retired, same worktree):
- `Native` first, r1 (medians of 3 per row): `yield` -10.6 %, `map-reuse` -4.2 %; `show-int` +0.45 %, `slice-short` +0.33 %, `show-int-long` +0.17 %, `handle` +0.11 %, `list` +0.03 %, `dup` unchanged [V].
- The full jit-rt probe of this revision against the base with the gate script and the exception: exit 0 over 2 alternating pairs [V]: `handle` +0.156 %, `show-int` +0.480 %, `show-int-long` +0.191 %, `slice-short` +0.362 % (allowed, each <= +0.6 %); every other gated row <= +0.038 % (`concat-copy`); `yield` -10.849 %, `map-reuse` -4.275 %, `list` +0.002 %; sum over the gated rows -47,927,761 instructions.
- The trade: native speed and code density (the three checks generated code makes most) and the yield path, against one instruction per out-of-line heap call in four extern rows. The VM pays the same `add`; VM rows do not decide.

Forgone, for a simple design:
- `heap` first: it removes the `add` but lengthens every `Native` read on x86-64 and loses most of the `yield` gain; kept only as the fallback of the gate's exception.
- A `catch_unwind`-free fallible macro form: measured gain is zero.
- Saving fewer registers in the stub or skipping the frame record: the stub is not on a hot path.
- Hand-written asm beyond the three stubs.

Profiler readiness (the profiler itself is a roadmap item after `native-aot`; T0 writes it):
- jit-rt lays down: `preserve_frame_pointers=true` in the one flag constructor (`abi::isa`); the stubs keep the frame-record chain (D-6, tested by `the_stub_keeps_the_frame_record_chain`), so a sampling profiler can walk generated code -> thunk -> stub -> Rust without unwind tables; runtime entry names and addresses (the descriptor table). There is no stack switching, and the fatal longjmp is the only non-returning transfer.
- Later stages: stable symbol name per function, `NativeMachine::symbols()`, code address and size per function, the site table (jit-core); task attribution (jit-tasks); the symbol table of AOT binaries (native-aot).

Measurement added by jit-rt: `crates/eml_nrt/examples/nrt_bench.rs` and three rows in `bench/rt.sh` (`enter`, `entry-plain`, `entry-shim`), recorded in benchmarks.md under a new heading 「ネイティブの数値」 at Close; no gate on them. The eml_rt gate script `bench/rt-gate.sh` (task R0) is the committed form of the bench gate.

---

## 2. Interfaces provided (final names, for consolidation)

| Consumer's assumption | Final interface |
|---|---|
| S2b I-1 (entry macro and table) | `eml_nrt::entries!` with forms `plain` / `shim`; `eml_nrt::entries() -> impl Iterator<Item = &'static Entry>` (an iterator, not a slice); `Entry { symbol, address: fn() -> *const u8, params: &'static [Kind], result: Option<Kind> }`; `Kind::{I64, F64}`; result-less entries allowed; a module's table is `const NAME: &[Entry]` appended to `TABLES` in `eml_nrt/src/lib.rs` |
| S2b I-2 (eml_rt exports) | `eml_rt::{CTX_YIELD_TARGET, CTX_FAULT, CTX_STOP, CTX_STACK_LIMIT, CTX_JMP}`; `Word::{IMM_TAG, IMM_SHIFT, UNIT, EMPTY, MIN_INT, MAX_INT}` (`pub const`s; `UNIT` / `EMPTY` are `Word` values, read with `.bits()`); `Meta::{TAG_MASK, SCAN_SHIFT, SCAN_MASK, KIND_SHIFT, KIND_MASK, LIN}`; `eml_rt::rc::{LOCAL_MAX, DUP_LIMIT, DROP_LIMIT, DUP_FAST_BIAS, DUP_FAST_SPAN, DECREF_FAST_BIAS, DECREF_FAST_SPAN}`; `eml_rt::layout::{BOX_VALUE_WORD, BOX_WORDS}`; `Ctx::{set_fault -> Option<Fault>, take_fault, stack_limit, set_stack_limit, runtime, set_runtime, jmp, stop_word}` |
| S2b I-3, S4 I-1 (reading a fatal) | D-7: `enter() -> Result<(), Fatal { code, site }>`; `FATAL_PANICKED = 1` (no payload), `FATAL_FAULT = 2` (`ctx.take_fault()`); other codes and their payload location are defined by the raising stage in `eml_nrt/src/enter.rs`; `Raise` implemented for `Fault`, `HeapError`, `Fatal`. No `take_fatal` |
| S2b I-4 (thunk) | `eml_native::thunk::define_thunk<M: Module>(&mut M, name, params: &[Kind], rets: &[Kind]) -> Built<FuncId>` in `src/thunk.rs`, already per kind sequence |
| S2b I-5 (flag constructor) | `eml_native::abi::isa(pic: bool)`; `bridge.rs` keeps its own |
| File names in eml_native | `src/abi.rs` (`Built`, `isa`, `clif`, `tail_signature`, `entry_signature`, `declare_entry`), `src/thunk.rs`, `src/mem.rs`; jit-rt creates them with the contents of D-9; no `lower/sig.rs`, `lower/entries.rs` |
| S3 request (a): per-entry symbol override | **Not added.** The entry's function identifier is its symbol; each list line of the row list spells `eml_x_<Variant>` (S3's fallback form), with `#[allow(non_snake_case)]` passed through the attribute position. One rule for every entry, and native-aot's `#[no_mangle]` needs nothing more |
| S3 request (b): row kind tokens | **Accepted and implemented in jit-rt's macro** [V]: `obj`, `tobj`, `unit`, `int`, `enum` map to `Kind::I64` / `i64`, `float` to `Kind::F64` / `f64`, in parameter and result position; `never` in result position gives a function returning `()` and `result: None`. No token muncher is needed in `extern_rows!` |
| What the row list hands to `entries!` | per variant, the entry identifier it spells itself (`fn IntShow as eml_x_IntShow(ctx, a: int) -> obj { .. }`; one `as` per variant on a multi-variant line) as the function identifier, the kind tokens unchanged, `#[allow(non_snake_case)]` in the attribute position. `entries!` checks nothing about the name; the test that every entry identifier equals `eml_x_` + the variant name belongs to the row list's stage |
| Fatal codes | 1 `FATAL_PANICKED`, 2 `FATAL_FAULT` (jit-rt); 3 `FATAL_RECORDED` (defined by jit-core in `eml_nrt/src/enter.rs`) |
| `Heap::is_immortal`, `Fault::Engine` | not in jit-rt; jit-core adds them with their first user under the eml_rt gates |
| S2b I-6 (eml_sched visibility) | D-22; modules `eml_sched::{cells, config, doorbell, driver, error, park}`; `config::{RunStats, Finished, RunConfig, Sched, Seat}`; `error::{VmFault, RuntimeError, SourceLocation}`; `driver::{Clock, RunClock, DriverKind, Signal, VirtualSignal}`; `park::{Park, ParkFactory, Pending, Progress, PollSource, DefaultPark}` |
| S2b I-7 (`SUPPORTED`) | `eml_nrt::SUPPORTED`; eml_nrt compiles on other targets through `arch/unsupported.rs` [I: not built for aarch64-pc-windows-msvc] |
| `Ctx::bytes_empty` infallible (requested by jit-externs) | D-14, task R4 |
| Features of eml_sched | sched-split: `testing` only. jit-externs: `tls = ["dep:eml_tls", "eml_io/tls"]`, `gui = ["dep:eml_gui"]`, `gui-native = ["gui", "eml_gui/native"]` |
| `Native` field set | six fields; none reserved for later stages |
| N-32's "debug assert where a `Ctx` changes thread" | jit-rt provides `eml_nrt::detached(&Ctx<Fast>) -> bool`; the assert site is jit-core's run driver (after the worker's `eml_enter` returns and `runtime` is cleared) and jit-tasks' worker hand-back. Owner: S2b, then S4 |

## 3. Alternatives considered and rejected

- Split `Native` into "generated-code fields" and "runtime fields" structs: one more offset layer for no reader.
- `fault_value` as a `repr(C)` word encoding: `Fault` is a 24-byte Rust enum with `Box`es; a second encoding needs its own proofs.
- `eml_fatal` hand-declared outside the macro: two ways to declare an entry.
- Stub tests through Cranelift in eml_nrt: eml_nrt must stay Cranelift-free.
- Register test by "Rust locals survive": probabilistic.
- `cfg(sanitize = "address")` to skip fatal tests: unstable; a module name plus `--skip` needs nothing.
- One commit for the crate with all three stubs: a failure on one ABI would block the other two and the docs (now N1 / N2 / N3).
- A commit's own CI result as its completion criterion: it exists only after a push (now checkpoints, D-12).
- Amending and force-pushing after a red run: rewrites pushed history; fix commits are simpler and keep the record.
- sched-split as one commit: three (S1-S3) so each compiles and passes alone; the order is forced by imports.
- Keeping `RunStats` in eml_vm: `Finished` contains it.
- `Ctx::bytes_empty` keeping the budget check and its row marked `faults`: changes LIR / Core IR snapshots of every program using `Bytes.empty` for a check the allocator repeats.

## 4. Assumptions made

| # | Assumption | If false |
|---|---|---|
| A-1 [I] | The SysV and Win64 stubs and their test helpers behave as written. They assemble for `x86_64-unknown-linux-gnu` and `x86_64-pc-windows-msvc` [V]; they have not run. aarch64 Linux uses the file that runs on macOS [V assembled] | Checkpoint G1 is red on that target; D-12 step 3 |
| A-2 [I] | No CI target enforces shadow stacks (CET, GCS) or branch protection for Rust binaries by default (rustc does not pass `/CETCOMPAT` on windows-msvc and does not enable `branch-protection` on aarch64 Linux), so the hand-written `ret` of the longjmp half is legal | `fatal::` tests crash on that target (D-12 step 5); goes to the user |
| A-3 [V aarch64 macOS; I elsewhere] | A panic caught by `catch_unwind` inside a shim does not need unwind info for the frames above the shim (stub, JIT) | That target's `a_shim_cuts_to_enter_on_an_error_and_on_a_panic` fails; fallback: shims return a checked status and generated code calls `eml_fatal` (changes only the `eml_rt_*` protocol); user decision |
| A-4 [I] | With resolver 3, `eml_sched/testing` is not enabled by `cargo build -p eml_cli` / `nix build` | The fixtures are compiled into the binary (dead code, no behaviour change) |
| A-5 [I] | The three sched-split commits each compile in the given order (only the final state was built) | Merge S1+S2 or S2+S3 and say so in the commit message |
| A-6 [I] | Verus's resource limits behave on the implementer's machine as here | Apply the same remedy (`#[verifier::rlimit(20)]`, no spec change) to the lemma that fails and name it in the commit message |
| A-7 [V 2 pairs; I for 5] | R1's gate with the exception passes over 5 pairs as it does over 2 (four rows between +0.1 % and +0.6 %, negative sum, no other row above +0.1 %) | N-29's fallback: the task stops and goes to the user with the table and the `heap`-first alternative |
| A-8 [I] | `gh workflow run CI --ref <branch>` starts the workflow for an unmerged branch (the workflow has `workflow_dispatch` and exists on `main`) | Open a draft pull request from the branch instead (`pull_request` also triggers CI); needs the user's permission like the push |

## 5. Implications for implementation

Order: plan commit -> T0 -> jit-rt (R0, R1, R2, R3, R4, N1, N2, N3, [G1], A1, [G2], Close, [G3]) -> sched-split (S1, S2, S3, Close). The two stages do not depend on each other.

**The gates** (every task): `cargo nextest run` ends with `N tests run: N passed` (0 failed), `cargo clippy --all-targets -- -D warnings` prints no `error` / `warning`, `cargo fmt --check` prints nothing.
**The eml_rt gates** (tasks R1-R4):
- `cargo verus verify -p eml_rt` -> last result line `verification results:: <n> verified, 0 errors`;
- `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` -> no output;
- `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::` and the same with ` -Zmiri-tree-borrows` appended -> `test result: ok`;
- the bench gate: `bench/rt-gate.sh <commit before the task>` exits 0 (5 alternating pairs; every row except `empty`, `println-*`, `read-*`, `stdout-*` has a median delta <= +0.1 %). **Task R1 alone** runs `bench/rt-gate.sh --allow 'handle show-int show-int-long slice-short' --allow-bound 0.6 <commit before R1>` instead: exit 0 means those four rows are <= +0.6 %, every other gated row <= +0.1 %, and the printed sum over the gated rows is negative. Run on macOS with a clean tree and nothing else running; it takes about 5 x 2 runs of `bench/rt.sh`. If it exits 1, run it once more; a second failure stops the task and goes to the user with both tables (for R1: with the `heap`-first alternative). The table goes into the task's commit message.

Pitfall for every task: `eml_cli`'s `citations::every_cited_heading_exists` fails when a comment or doc cites a heading that does not exist [V]. Write the cited section in the same commit.

### 5.0 Task T0 — `Revise the native design texts` (first commit after the plan commit)

Edits `docs/future/` only. Text intent per site (the implementer writes Japanese).

roadmap.md 「段の列」: replace the three bullets `jit-core`, `jit-externs`, `jit-effects` (L31-33) by these eight, in this order. "Old sentence" refers to the sentences of those three bullets.

| New bullet | Content | Old sentences that move here | 前提 |
|---|---|---|---|
| `jit-rt` | eml_rt: the `#[repr(C)]` fields native code reads (`yielding`, `fault`, stop pointer, `stack_limit`, `jmp`, `runtime`) as one struct at the head of `Ctx` (the heap follows it), with `offset_of!` consts next to `Ctx`, a trust row for the pointers, fault parking; `Bytes.empty` infallible. New crate `eml_nrt`: `eml_enter` / `eml_fatal` stubs for AAPCS64, SysV x86_64 and Win64 with frame records, the entry macro and descriptor table. `eml_native`: edges to `eml_rt`, `eml_lir`, `eml_nrt`, the one Cranelift flag constructor, the ABI tests on the four CI targets | from old `jit-core`: "AAPCS64とx86_64のSysVの`eml_enter`と`eml_fatal`" (now three ABIs), "ネイティブのコードが読む`Ctx`の欄の配置", the whole export passage from "`eml_rt`から、出すコードが読むものを" through "`eml_vm`の`compile/constants.rs`の箱の書き込みをそこへ付け替える", and "`eml_native`は`eml_rt`と`eml_lir`への辺を持つようになり … コンパイル時の`assert`で確かめる" (add `eml_nrt` to the edges) | 借用パラメータ |
| `sched-split` | `eml_sched` is created with `cells.rs`, the run-error types, `RunConfig` / `Finished` / `Clock` / `DriverKind`, `Park`; VM only; tests unchanged | none (new) | なし |
| `jit-core` | first-order LIR -> CLIF (scalars, data, direct calls, tail calls, RC, constants), unwind checks and cleanup blocks, entry safepoints, the fault path, a seed set of extern rows, heap entries, `eml run --native`, the native UI tests | from old `jit-core`: the first sentence without "クロージャと`apply`", "安全点", "扱わない形はE0004で断り、UIテストの印 … を導入する" with the mark spelled `-- native: skip (<reason>)`, and "出すコードは、物体のメモリのロードとストアのずれやマスクに整数リテラルを使わない" | `jit-rt`, `sched-split` |
| `jit-apply` | closures, function values (static closures), `apply` and tail `apply`, entries for the `(function, k)` that occur, generic partial-application objects, excess application | "クロージャと`apply`" from old `jit-core` | `jit-core` |
| `jit-externs` | old `jit-externs` bullet unchanged, plus: the `Std.Tls` / `Std.Gui` rows that are not task operations move to `eml_sched` with the `tls` / `gui` / `gui-native` features forwarded; non-blocking user C rows | old `jit-externs` whole; "利用者のCの行をネイティブのコードから呼ぶこと" from old `jit-core` | `jit-apply` |
| `jit-effects` | effect operations in native code | from old `jit-effects`: "yield、戻り値で伝える中断、handler" and the two evidence-role sentences ("evidenceの欄の役割 … に移す。同じ段で … 役割の番号を消す") | `jit-externs` |
| `jit-tasks` | the task layer in native code; the worker table, inboxes, monitor and jobs move to `eml_sched` behind `Exec`; nested `Async.run` levels end in LIFO order (`BuriedRun`); the limit classes live in the runtime's per-worker state | from old `jit-effects`: "`Exec`と`IoDriver`の上の`Async`のエントリ、ネイティブの`Async.run`の入れ子、親ごとに1つのworkerの上の`par`、致命的な誤りの手順、安全点での上限の中断", "この段で、E0004で断る形がなくなる", "`eml_run_pending`とメインのworkerの致命的な誤りの道も入れる"; blocking user C rows | `jit-effects` |
| `jit-repl` | the REPL on native code: append by name, roots, recovery after a fatal error | none (new) | `jit-tasks` |

Other sites:

| File, place | New content |
|---|---|
| roadmap.md L34 `native-aot` | delete "Win64のエントリのstub、"; 前提 `jit-tasks`; add: AOT binaries get `/STACK:8388608` on Windows |
| roadmap.md L38 `native-par` | 前提: `native-inline`, `std-par`, `jit-tasks` |
| roadmap.md L337 | the `eml_sched` crate arrives in two steps: `sched-split` (cells, run interface), `jit-tasks` (worker table, inboxes, monitor, `Exec`) |
| roadmap.md 「その後の項目」 | new item 「ネイティブのプロファイラ」. Scope: a sampling profiler for native runs (`eml run --native`, later AOT binaries): sample each worker's (pc, fp), walk frame records, resolve through the JIT symbol map and the entry table, report per eml function, per source line where a position is known, and per task; write a `perf` map / jitdump on Linux and an equivalent symbol table for macOS and Windows tools; statement positions in LIR for line-level attribution (today only extern statements carry a position). 前提: `native-aot`. 後に回した理由: it needs the finished backend and the symbol table of AOT binaries. 見直す条件: when `native-aot` closes |
| native.md decision #1 (L19) | code generator `eml_native`; runtime entries, entry stubs and run driver in `eml_nrt` (no Cranelift; `libeml_rt.a` is built from it); shared engine-neutral layer `eml_sched`; edges `eml_native -> eml_nrt -> eml_sched -> eml_rt`; `eml_vm` depends on none of Cranelift, `eml_native`, `eml_nrt` |
| native.md 「呼び出し規約とエントリ」 L41 | entries exist for the `(function, k)` that occur in the program; partial application builds a generic partial-application object instead of one entry per `k`. (T0 is the only task that rewrites this sentence; no later stage edits it again) |
| native.md 「呼び出し規約とエントリ」 L44 (`eml_run_pending` paragraph, L46) | `eml_run_pending` becomes a C symbol in `native-aot`, with its first C caller; until then the main worker's loop is a Rust function |
| native.md 「呼び出し規約とエントリ」 L44 | `eml_enter(ctx, thunk, entry, argv)` returns 0 or the code and site of the fatal error. The stub calls a default-convention thunk made by Cranelift, which loads the arguments from `argv`, calls the `tail` function and stores the results to `argv`; reason: Cranelift's `tail` convention assigns other argument registers and clobbers all registers, so asm must not call it. The `JmpBuf` is in the stub's frame and linked from `Ctx`. All three stubs exist from `jit-rt` and keep a frame record |
| native.md 「extern の失敗」 L64-66 | the wrapper parks the `Fault` in `Ctx` and generated code tests one fault word; a failure whose payload the engine holds (a panic message, a re-raise request) is parked as `Fault::Engine` with the payload in the runtime's per-worker state; the cold path calls one shared routine; rows that never return (`exit`, `panic`, `fail_io`) need no test; internal-class errors of a row are fatal |
| native.md 「評価全体を止める誤り」 L73 | `eml_fatal(ctx, code, site)`; between the error and the stub there are `tail` frames, thunks, and the destructor-free Rust frames of the one entry that detected the error plus `eml_fatal` |
| native.md 「タスクごとの上限」 L79-83 | the class is not a `Ctx` field: it lives in the runtime's per-worker state, with one writer of `stack_limit`; the decision table is implemented once in a runtime service and generated code branches on its answer; the limits are `hard` and `real = hard + LIMIT_HEADROOM_BYTES` (replacing "the real limit minus the headroom") |
| native.md 「安全点とスタック」 L91, L95, L98 and decision #9 | entry safepoint only while LIR edges are forward. Stack geometry: per thread, with `base` the lowest address of the stack, `hard = base + 1 MiB` on workers and `stack_base + 2 MiB` on the main worker; `real = hard + LIMIT_HEADROOM_BYTES` (2 MiB); `stack_limit` is `real` in normal running. The fields native code reads are one `#[repr(C)]` struct at the head of `Ctx`, the heap after it: `yielding`, `fault`, stop pointer, `stack_limit`, `jmp`, `runtime` (the per-worker pointer that replaces thread-locals); a field is added with its first reader in generated code or asm |
| native.md 「並列の実行」 L128 | nested `Async.run` levels end in LIFO order; a run that can proceed only by ending an outer level first ends with the `BuriedRun` deadlock error |
| native.md 「extern のエントリ」 L102-103 | entries live in `eml_nrt`, declared by one macro that emits the function and its descriptor row; name prefixes `eml_rt_`, `eml_x_`, `eml_nrt_`; parameters `ctx` then `i64` / `f64`, at most one result |
| native.md 「メインのworkerの対応」 L139 | the shared core `cells.rs` is in `eml_sched` |
| native.md 「AOT と配布」 L152 | `libeml_rt.a` is built from `eml_nrt` (with `eml_sched`, `eml_rt`) |
| native.md 「テスト」 L178-180 | mark `-- native: skip (<reason>)`; separate native directory tests; a snapshot of the rejected forms; a leak census after each native run |
| native.md 「未決の論点」 L199 | the Windows `tail` item is settled by `jit-rt`'s tests on the four CI targets; keep the trampoline / no-native fallback as what happens if they fail |
| native.md, a new paragraph under 「目的と範囲」 | the VM is the reference engine for checking and development; where a choice trades native speed against VM speed, native speed wins unless it needs special cases or duplicated logic; frame pointers and name-keyed symbols are kept for a future profiler |
| multicore.md 「層の構成」 L49-63 | diagram: `eml_vm  Exec を実装する (VM)` / `eml_nrt  Exec を実装する (ネイティブ)`, `eml_native` is the code generator above `eml_nrt`; dependency sentence `eml_rt <- eml_sched <- eml_vm`, `eml_sched <- eml_nrt <- eml_native`; L63: `eml_sched` arrives in two steps |

Leave roadmap L571 and ffi.md L128 as they are.
Test change kind: none.
Completion (exact):
- `grep -c '^- `jit-' docs/future/roadmap.md` -> `7`; `grep -c '^- `sched-split`' docs/future/roadmap.md` -> `1`;
- `grep -n '// native: skip\|Win64のエントリのstub' docs/future/roadmap.md docs/future/native.md` -> no output; `grep -c 'ネイティブのプロファイラ' docs/future/roadmap.md` -> `1` or more; `grep -c 'BuriedRun' docs/future/native.md` -> `1` or more;
- `cargo nextest run -p eml_cli --test integration citations::` -> all passed;
- `git diff --stat HEAD~1 | grep -v 'docs/future/' | grep -c '|'` -> `0`.

### 5.1 Stage jit-rt

**Prerequisites and baseline** (record in the plan before R0): branch `plan/jit-rt` from the plan + T0 commits on `432fffb4`; `cargo nextest run` -> `4878 tests run: 4878 passed, 22 skipped` [V]; `cargo verus verify -p eml_rt` -> record `<n>`.

#### R0 — `Add the eml_rt bench gate script` (1.3)

Files: new `bench/rt-gate.sh` (Appendix A.4, mode 755); `docs/implementation/benchmarks.md` 「命令の数の測り方」 (the paragraph at L162 gains: the gate is `bench/rt-gate.sh BASE`, what it prints (per row the median delta in % and in instructions, and the sum over the gated rows), which rows it does not judge, and the `--allow` / `--allow-bound` options: listed rows are judged against the given bound and the sum must then be negative); `CLAUDE.md` Commands (one line after `bench/rt.sh`); `crates/eml_rt/CLAUDE.md` first bullet (the gate is part of an eml_rt change).
The script hard-codes no row list besides the rows it does not judge.
Test change kind: none. Completion: the gates; `bash -n bench/rt-gate.sh` -> no output; `bench/rt-gate.sh HEAD 1` (base = HEAD itself, one pair) prints a table of 41 rows and the line `門の行の差の和: ...`, and leaves `git branch --show-current` on `plan/jit-rt` with a clean tree. Its exit status is not judged here: with one pair of identical builds the row `list` showed +0.154 % once [V]; the gate proper uses 5 pairs.

#### R1 — `Put the fields native code reads at the head of Ctx` (D-1..D-4)

Files: `crates/eml_rt/src/heap/ctx.rs` (struct L819-836, `for_worker` L1065-1100, `stats` L1138), `heap/ctx/{arrays,bytes,io,proc,proof,recover,roots,strings,unboxed}.rs`, `heap/mod.rs` L64, `lib.rs` L44, `trust.rs`, `heap/tests/trusted.rs`, `heap/tests/ctx.rs`; `docs/spec/object-model.md`; `crates/eml_rt/CLAUDE.md`.

TDD order:
1. Tests first (they do not compile until step 2): the T18 test of Appendix A.1 before `a_notice_from_another_thread_is_seen` (`trusted.rs` L1529); in `heap/tests/ctx.rs`:
   ```rust
   #[test]
   fn the_native_offsets_are_pinned() {
       use crate::{CTX_FAULT, CTX_JMP, CTX_STACK_LIMIT, CTX_STOP, CTX_YIELD_TARGET};
       assert_eq!((CTX_YIELD_TARGET, CTX_FAULT, CTX_STOP, CTX_STACK_LIMIT, CTX_JMP), (0, 32, 40, 48, 56));
   }
   ```
   in `trust.rs`:
   ```rust
   #[test]
   fn the_core_has_no_native_boundary() {
       for (file, text) in FILES {
           let s = prepare(text);
           for word in ["extern", "no_mangle", "naked", "naked_asm", "global_asm", "asm", "catch_unwind"] {
               let len = word.chars().count();
               assert!(
                   !(0..s.len()).any(|i| word_at(&s, i, word) && !s.get(i + len).is_some_and(|&c| is_ident(c))),
                   "{file} uses {word}"
               );
           }
       }
   }
   ```
   (`word_at` checks only the leading boundary; without the trailing check `pub mod externs;` matches [V].)
2. `ctx.rs`, inside `verus!`, **before** the doc comment of `Ctx` (L819-820): the three wrappers, `Native`, `stop_of`. `Ctx` gets `#[repr(C)]`, `native: Native` as the first field (before `heap`), loses `yielding`, gains `fault_value: Option<Fault>` last. Add `use crate::externs::Fault;`.
3. Rename the field accesses: `perl -pi -e 's/\.yielding\b(?!\()/.native.yielding/g' $(git ls-files 'crates/eml_rt/src/heap/*.rs' 'crates/eml_rt/src/heap/ctx/*.rs' 'crates/eml_rt/src/heap/tests/*.rs')` (195 replacements in 10 files [V]; the method `yielding()` is untouched). Check: `grep -rnE '\.yielding\b' crates/eml_rt/src | grep -vE '\.native\.yielding|\.yielding\(' ` -> no output.
4. `for_worker` (the only struct expression that names the field, L1076-1092): `let stop = stop_of(&word);` and `native: Native { yielding: YieldState::none(), fault: 0, stop, stack_limit: 0, jmp: JmpPtr(core::ptr::null_mut()), runtime: RtPtr(core::ptr::null_mut()) }`, `fault_value: None`.
5. After `stats()`: the lemma and the accessors.
   ```rust
   proof fn lemma_same_words(&self, s0: Ctx<M>)
       requires self.heap == s0.heap, self.evv == s0.evv, self.native.yielding == s0.native.yielding, self.max_evv == s0.max_evv,
       ensures s0.wf() ==> self.wf(), s0.held_ok() ==> self.held_ok(),
   {
       assert(self.heap.core.settled_view() == s0.heap.core.settled_view());
       assert forall|b: usize| #[trigger] self.need(seq![], b) == s0.need(seq![], b) by {}
       if s0.holds_args(seq![]) { assert(self.holds_args(seq![])); }
       if s0.held_ok() { assert(self.held_ok()); }
   }
   pub fn set_fault(&mut self, fault: Fault) -> (previous: Option<Fault>)
       ensures /* the five frame clauses of D-3 */ final(self).fault_parked(),
   {
       let previous = self.fault_value.take();
       self.fault_value = Some(fault);
       self.native.fault = 1;
       proof { self.lemma_same_words(*old(self)); }
       previous
   }
   pub fn take_fault(&mut self) -> (r: Option<Fault>)
       ensures /* same */ !final(self).fault_parked(),
   {
       self.native.fault = 0;
       let r = self.fault_value.take();
       proof { self.lemma_same_words(*old(self)); }
       r
   }
   ```
   `set_stack_limit` and `set_runtime` assign the field and end with the same proof block; the getters return the field (`self.native.runtime.0`, `self.native.jmp.0`, `self.native.stop.0`).
6. After `} // verus!` (L3640): `unsafe impl Send for StopPtr {}` (and `JmpPtr`, `RtPtr`); the five consts and the const asserts of D-2 (`core::mem::offset_of!`):
   ```rust
   const _: () = {
       assert!(core::mem::offset_of!(Ctx<Fast>, native) == 0);
       assert!(core::mem::offset_of!(Ctx<Checked>, native) == 0);
       assert!(core::mem::offset_of!(Ctx<Fast>, heap) == core::mem::size_of::<Native>()
           && core::mem::offset_of!(Ctx<Checked>, heap) == core::mem::size_of::<Native>());
       assert!(core::mem::size_of::<Native>() == 72);
       let consts = [CTX_YIELD_TARGET, CTX_FAULT, CTX_STOP, CTX_STACK_LIMIT, CTX_JMP];
       let mut i = 0;
       while i < consts.len() {
           assert!(consts[i] <= 120 && consts[i].is_multiple_of(8));
           i += 1;
       }
   };
   ```
7. `heap/mod.rs` L64 and `lib.rs` L44: re-export the five consts.
8. `trust.rs`: `TRUSTED` gains `("heap/ctx.rs: external_body stop_of", "T18")`; the number list (L543-545) gains `"T18"`.
9. `verusfmt --verus-only $(git ls-files 'crates/eml_rt/src/*.rs')` (write mode; the rename makes 7 files need reformatting [V]), then `cargo fmt`.

Docs in the same commit (`docs/spec/object-model.md`):
- 「欄」 (L812-825): `yielding` is inside `native`; new rows `native` (fields and who writes each: `yielding` the yield operations; `fault` `set_fault` / `take_fault`; `stop` `for_worker` only; `stack_limit` `set_stack_limit`; `jmp` the entry stubs of the native runtime only; `runtime` `set_runtime`) and `fault_value`. State: `Ctx` is `#[repr(C)]`; `native` is at offset 0 in both modes and `heap` is the field right after it; the offsets native code reads are the `pub const`s `CTX_YIELD_TARGET`, `CTX_FAULT`, `CTX_STOP`, `CTX_STACK_LIMIT`, `CTX_JMP` next to `Ctx`; `eml_rt` contains no `extern "C"`, asm or `catch_unwind` (`trust.rs` checks).
- New subsection 「置いた失敗」 under 「実行文脈 `Ctx`」: the contract of `set_fault` / `take_fault` (D-3).
- 「語の位置」 last bullet (L91): "`Ctx`の欄のずれ" points to 「欄」.
- 「`unsafe` の核」: the outside-`verus!` list (L1390) gains the `CTX_*` consts and their asserts; row T18 with the table and Miri column of D-4.
- `docs/implementation/testing.md` 「Miri」 (L197) and the budget sentence of `crates/eml_rt/CLAUDE.md`: measure `/usr/bin/time -p` (`user`) of both `trusted::` Miri commands on a built tree before and after this task. The new test's added `user` time must be within the per-test rule (5 s under Tree Borrows for both modes together; alone it runs in 0.35 s under Miri [V], and it is a fast-mode-only test). If either command's total is over half its budget (the budgets are 280 s Stacked Borrows and 365 s Tree Borrows, so the thresholds are 140 s and 182.5 s), apply the rule already written there (re-measure on an idle machine and set the budget to about 3x) and write the new numbers in both files; otherwise the numbers stay and the paragraph's list of what `trusted` covers gains the T18 test. The measured times and the machine go into the commit message.
- `crates/eml_rt/CLAUDE.md`: "T0-T17" -> "T0-T18"; one bullet: the fields native code reads are `Native` at the head of `Ctx`, read only through the `CTX_*` consts; eml_rt holds no `extern "C"`, asm or `catch_unwind` (docs/spec/object-model.md の「欄」).

Test change kind: none (new tests only). Left to the implementer: Japanese comments and the wording of the spec rows.
Completion criteria:
- the gates and the eml_rt gates, with the bench gate in its R1 form (`--allow 'handle show-int show-int-long slice-short' --allow-bound 0.6`); Verus on the probe: `1229 verified, 0 errors` with `Native` alone [V];
- `cargo nextest run -p eml_rt -E 'test(the_native) | test(the_core_has_no_native_boundary) | test(the_trust_list)'` -> `4 tests run: 4 passed`;
- `grep -rn 'fn set_stop\|fn set_jmp' crates/eml_rt/src` -> no output.
Pitfalls: struct-update specs (`Ctx { depth_limit: limit, ..*self }`, L897-920; `proof.rs` L2311-2312) keep working [V]. `trusted` tests use the memory host (`host(&Arc::default())`), not `StdHost`.

#### R2 — `Export the Meta, Word and rc encodings` (D-5)

Files: `crates/eml_rt/src/meta.rs` (L153-161, `new` L183, `tag` L187-192, `scan` L194-200, `lemma_consts` L392-400), `word.rs` (L45-47, L243-275), `rc.rs` (L10, L14, L93, L113), `lib.rs` L30.
TDD: pin tests first. `meta.rs` tests: `assert_eq!((Meta::TAG_MASK, Meta::SCAN_SHIFT, Meta::SCAN_MASK, Meta::KIND_SHIFT, Meta::KIND_MASK, Meta::LIN), (0xFFFF, 16, 0xFF, 24, 0xF, 0x8000_0000));` and a round trip of `Meta::new(7, 3, Kind::Data, true)` through `tag/scan/kind/lin`. `word.rs`: `assert_eq!((Word::IMM_TAG, Word::IMM_SHIFT), (1, 1))` and `Word::imm(n).bits() == ((n << Word::IMM_SHIFT) as usize | Word::IMM_TAG)` for `n` in `[0, 1, -1, Word::MIN_INT, Word::MAX_INT]`. `rc.rs`: `assert_eq!((rc::DUP_FAST_BIAS, rc::DUP_FAST_SPAN, rc::DECREF_FAST_BIAS, rc::DECREF_FAST_SPAN), (1, 0x7FFF_FFFE, 2, 0x7FFF_FFFD))` and, for rc in `[i32::MIN, DROP_LIMIT - 1, DROP_LIMIT, -2, -1, 0, 1, 2, LOCAL_MAX - 1, LOCAL_MAX]`, "the fast-path predicate is true iff `dup_step` / `decref_step` returns `Store`".
Exact edits: as D-5. `tag()` needs `proof { lemma_consts(); let x = self.0; assert((x & 0xFFFF) as u16 == x as u16) by (bit_vector); }`; `scan()` needs `proof { lemma_consts(); let x = self.0; assert(((x >> 16) & 0xFF) as u8 == (x >> 16) as u8) by (bit_vector); }` [V]. The spec functions are not edited.
Docs (object-model.md): 「ヘッダ」 names the six `Meta` consts; 「語」 names `Word::IMM_TAG`, `IMM_SHIFT`, `UNIT`, `EMPTY`; 「局所の速い道」 names the four fast-path consts and `LOCAL_MAX` / `DUP_LIMIT` / `DROP_LIMIT` in `eml_rt::rc`; 「語の位置」 L91 says where each item it excludes is exported.
Test change kind: none. Completion: the gates and the eml_rt gates (probe with R1+R2: `1240 verified, 0 errors` [V]); `cargo nextest run -p eml_rt -E 'test(/meta::|word::|rc::/)'` -> 0 failed. If the bench gate fails on a row, revert `tag()` / `scan()` to the cast forms (keeping the consts and `new`) and run the gate again.

#### R3 — `Put the box value word in eml_rt::layout` (D-5)

Replace the literal by `crate::layout::BOX_VALUE_WORD` / `BOX_WORDS` (in ghost positions `crate::layout::BOX_VALUE_WORD as int`):
| Site | Edit |
|---|---|
| `layout.rs` after `INLINE_SCAN_LIMIT` (L38) | the two consts |
| `heap/mod.rs` L820-825 (dummy maker) | `alloc_header(BOX_WORDS, 0, ..)`; `tracked_remove` / `tracked_insert` index; `slot_ptr(h, BOX_VALUE_WORD)` |
| `heap/mod.rs` L1690-1700 (`alloc_int`) | the first `1` of `alloc_header(1, 1, Meta::new(0, 0, Kind::BigInt, false))` (the second is the rc); remove / insert index; `slot_ptr` |
| `heap/mod.rs` L1742-1748 (`alloc_float`) | same three |
| `heap/mod.rs` L4440, L4484 (`unbox_int`, `unbox_float`) | `slot_get(h, BOX_VALUE_WORD)` |
| `heap/recover.rs` L812, L831 (the `Kind::BigInt` and `Kind::Float` arms) | `slot_get(h, BOX_VALUE_WORD)` |
| invariants: `heap/safe.rs` L134-135, L993, L1007, L1109-1110; `heap/inv.rs` L343, L456-457; `frozen.rs` L135; `heap/field.rs` L105 | `p_init(.., 0)` -> `p_init(.., crate::layout::BOX_VALUE_WORD as int)` (`cap >= 1` stays, as for `COUNT_WORD` at safe.rs L131) |
| proof steps under `if ...boxes_value()`: `heap/safe.rs` L421, L659, L660, L662, L1018 | `contains_key(0)`, `p_init(q, 0)`, `p.slots[0] == q.slots[0]` -> the const [V] |
| `heap/share.rs` L591 `lemma_moves_parent` | `#[verifier::rlimit(20)]` above the `proof fn` |
| `crates/eml_vm/src/compile/constants.rs` L8, L94-95, L113-114 | import `layout`; `alloc_immortal(layout::BOX_WORDS, ..)`, `set_raw(w, layout::BOX_VALUE_WORD, ..)` |

Residual literal slot-0 sites are **not** box sites and stay: `foreign.rs` (vtable word), `text.rs`, `heap/string.rs`, `heap/unboxed.rs`, `heap/inv.rs` L119 (`body_wf`: the length word of `String` / `Bytes` / `Raw`), `heap/recover.rs` L798 (`Raw` length), `alloc.rs` (free-list link).
TDD: `layout.rs` tests gain `assert_eq!((BOX_VALUE_WORD, BOX_WORDS), (0, 1));`; `heap/tests` gains a both-modes test that reads `alloc_int(i64::MAX)` and `alloc_float(bits)` back with `raw(w, layout::BOX_VALUE_WORD)`.
Docs: 「語の位置」: new item "箱: `BOX_VALUE_WORD`、`BOX_WORDS`"; "`BigInt`と`Float`の箱の値の語" leaves the "not in this module" list.
Test change kind: none. Completion: the gates and the eml_rt gates (probe: `1242 verified, 0 errors` [V]);
- `grep -n 'alloc_immortal(1,\|set_raw(w, 0,' crates/eml_vm/src/compile/constants.rs` -> no output;
- `grep -rnE 'p_init\([a-z_.()]*, 0\)|contains_key\(0\)|slots\[0\]|slot_get\([a-z]+, 0\)|slot_ptr\([a-z]+, 0\)|tracked_(remove|insert)\(0' crates/eml_rt/src/heap/safe.rs crates/eml_rt/src/frozen.rs crates/eml_rt/src/heap/field.rs crates/eml_rt/src/heap/mod.rs` -> no output; the same pattern over `crates/eml_rt/src/heap/inv.rs` -> exactly the line `&&& p_init(p, 0)` of `body_wf`; over `crates/eml_rt/src/heap/recover.rs` -> exactly the line `let n = self.core.slot_get(h, 0).bits();` (the `Raw` arm) [V on the probe].
Pitfall: without the `rlimit` attribute Verus reports `resource limit (rlimit) exceeded` at `lemma_moves_parent`, not at an edited line; `spinoff_prover` does not help [V].

#### R4 — `Make Bytes.empty infallible` (D-14)

File: `crates/eml_rt/src/heap/ctx/bytes.rs` L405-422. New function (verified as written [V]):
```rust
pub fn bytes_empty(&mut self) -> (r: Result<Word, Fault>)
    requires old(self).wf(),
    ensures
        final(self).wf(),
        final(self).evv_spec() == old(self).evv_spec(),
        final(self).yield_spec() == old(self).yield_spec(),
        r is Ok,
        r matches Ok(w) ==> final(self).is_bytes(w) && final(self).text_of(w).len() == 0,
        r matches Ok(w) ==> (old(self).held_ok() ==> {
            &&& final(self).held_ok()
            &&& final(self).holds(w)
            &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
        }),
{
    let ghost s0 = *self;
    let w = self.heap.alloc_bytes_room(0);
    proof {
        if M::CHECKED { assert(self.heap.words_kept(s0.heap)); self.lemma_words_kept(s0); }
        if s0.held_ok() { self.lemma_made_ok(s0, w); }
    }
    Ok(w)
}
```
(the body of `bytes_with_capacity`, L1052-1062, without the checks). TDD: first add to `heap/tests/bytes.rs`, registered in its `both_modes!` list [V: fails before the change at `unwrap`, passes after in both modes]:
```rust
fn an_empty_bytes_ignores_the_request_budget<M: Mode>() {
    let limits = Limits { max_bytes: 0, ..Limits::default() };
    let mut ctx = Ctx::<M>::new(Arc::new(Control::new(limits)), host(&Arc::default()));
    assert_eq!(unsafe { ctx.bytes_make(0, 0) }, Err(Fault::Stopped(Stop::MemoryLimit)));
    let w = ctx.bytes_empty().unwrap();
    assert_eq!(body(ctx.heap(), w), Vec::<u8>::new());
    unsafe { ctx.heap_mut().decref(w).unwrap() };
}
```
Docs: object-model.md 「バイト列の操作」 (and L1167 if it lists `Bytes.empty` among operations that check the budget): `Bytes.empty` does not check the request budget, like `MutArray.empty`; stdlib.md 「実行時エラー」 only if it names `Bytes.empty`.
Test change kind: none expected (the whole suite passes with the change [V]); if a `heap/tests/expected.txt` row changes, it is kind 2 with scope "rows of `bytes_empty` under an exhausted budget" and is listed in the commit message.
Completion: the gates and the eml_rt gates; `grep -n 'r is Ok,' crates/eml_rt/src/heap/ctx/bytes.rs | wc -l` is one more than before the task.

#### N1 — `Add eml_nrt with the AAPCS64 entry stub` (D-6, D-7, D-8, D-13)

Files: root `Cargo.toml` (`eml_nrt = { path = "crates/eml_nrt" }` after `eml_native`); `crates/eml_nrt` as Appendix A.2 **except**: `src/arch/x86_64_sysv.rs`, `src/arch/x86_64_win64.rs` and the two `x86_64` blocks of `tests/regs.rs` are not created yet; `src/arch/mod.rs` selects `aarch64.rs` under `all(target_arch = "aarch64", not(windows))` and `unsupported.rs` under `not(all(target_arch = "aarch64", not(windows)))`; `src/trust.rs` lists only the files that exist; `tests/main.rs` is
```rust
#[cfg(all(not(miri), target_arch = "aarch64", not(windows)))]
mod regs;
#[cfg(all(not(miri), target_arch = "aarch64", not(windows)))]
mod stub;
```
Also: `bench/rt.sh` (build line gains `-p eml_nrt` and `--example nrt_bench`; before the ratios block: a heading line and `measure "$examples/nrt_bench" enter entry-plain entry-shim`); `.github/workflows/ci.yml` (dependency step: the eml_vm line becomes `'cranelift|eml_native|eml_nrt'`, the eml_nrt line is added, the eml_native line stays as it is until A1; clippy row; miri step; asan step for eml_nrt — D-11).
TDD order: `src/entries.rs` unit tests and `tests/stub.rs` first, then `enter.rs`, `arch/aarch64.rs`.
Docs in the same commit:
- New `docs/spec/native.md` (位置づけ: 規範; opening lines: the contract between generated native code and the native runtime, one section per implemented part). Section 「エントリ」 with subsections whose names code comments cite:
  - 「エントリの ABI」: C default convention; `ctx: *mut Ctx<Fast>` first; other parameters exactly `i64` or `f64`; at most one result, `i64` or `f64`; no struct by value; the symbol names of D-8.
  - 「記述子の表」: `entries!`, `Entry`, `Kind`, `entries()`, `TABLES`; generated code imports by symbol and builds the signature from the row only.
  - 「包みの本体」: the `plain` and `shim` forms; the shim body rule; `Raise` and its three impls; a `plain` entry needs a cannot-panic argument written here; the list of `plain` entries (`eml_fatal`: it never returns and holds no destructor); `shim` has no fast-path cost.
  - 「失敗の手順」: the protocol by prefix. `eml_rt_*`: fatal-only (an `Err` or a caught panic is recorded and the shim ends in `eml_fatal`; generated code checks nothing). `eml_x_*`: a row that can fail parks its failure in the `Ctx` and returns a placeholder, and generated code tests the fault word; any other error and a caught panic are internal and end in the shim's fatal path; a wrapper body never names `eml_fatal`. `eml_nrt_*`: a service returns either an action word that generated code branches on, or a plain data word when its only failure path is the fatal path. In this stage the section states the rule and says that the only entry is `eml_fatal`; each later stage adds what it implements. The fast-mode `Err` list: in fast mode a heap operation returns `Err` only as `WrongKind` (a `foreign*` kind or type mismatch), `StaleToken` (`reuse_in_place` on an empty token), `RootMisuse` and `Unrecoverable` (root and recovery operations); none is recoverable by generated code, which is why heap entries are fatal-only.
  - 「`eml_enter`」: signature and result word; `Thunk` and what a thunk does; `JmpBuf` and its link through `Ctx`; the saved register set per ABI; the frame record (each stub sets up a standard frame record before calling the thunk, so a frame-pointer walk goes generated code -> thunk -> stub -> Rust caller; nothing may use the frame-pointer register for another purpose or switch stacks inside eml code); nesting; the re-entry rule; `enter`, `detached`; which ABIs have a stub (the list grows in N2, N3).
  - 「`eml_fatal`」: signature; `Fatal`; the code table (1 `FATAL_PANICKED`, 2 `FATAL_FAULT`; later stages append their codes here) and how the caller of `enter` reads each; "records nothing"; the fatal rule; where `eml_fatal(` may appear.
  - 「対象」: `SUPPORTED`.
- `docs/README.md` 「文書の地図」: row for `spec/native.md`; 「読む順」: `eml_nrt` -> spec/native.md.
- `docs/implementation/architecture.md`: repository list (after L47) and crate table (after L84): `eml_nrt` (the entries generated code calls and the entry stubs; depends on `eml_rt`; no Cranelift, `eml_lir`, `eml_vm`, `eml_native`); 「crate の依存関係」 L96: `eml_vm`, `eml_io`, `eml_nrt` depend on `eml_rt`; L103: the CI step also checks eml_nrt; new section 「`eml_nrt` の内部」 after 「`eml_native` の内部」 (module list; `unsafe` only in the files `trust.rs` lists).
- `docs/implementation/testing.md`: new section 「`eml_nrt` の検証」 after 「`eml_native` の検証」 (L257): the unsafe-file list; each stub test and what it checks; they do not run under Miri; fatal-path tests are in modules named `fatal`, which the ASan job skips, and why; Miri runs `entries::` under both models, no budget (two tests, under 1 s). 「Miri」 L197-202, 「CI」 L706, 「よく使うコマンド」 L787: the command.
- `docs/implementation/benchmarks.md` 「命令の数の測り方」: the three `nrt_bench` rows and what each measures; a new heading 「ネイティブの数値」 that will hold the native numbers (in this stage: the three rows, filled at Close).
- `CLAUDE.md`: Architecture block row `eml_nrt`; Commands: `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_nrt --lib -- entries::`; the non-default-features comment (`eml_nrt` in the `--all-targets` row); the ASan comment (`-p eml_nrt -- --skip fatal::`); topic table row "Native runtime entries | docs/spec/native.md の「エントリ」".
Test change kind: none.
Completion criteria (on an aarch64 macOS or Linux host):
- the gates; `cargo nextest run -p eml_nrt` -> `12 tests run: 12 passed` (7 stub, 2 entries, 3 trust) [V];
- both Miri runs of `cargo miri test -p eml_nrt --lib -- entries::` -> `test result: ok. 2 passed` [V];
- `grep -rn 'thread_local' crates/eml_nrt/src` -> no output; `grep -rnw 'Checked' crates/eml_nrt/src` -> no output; `grep -rnE 'CTX_[A-Z_]+\s*(:|=)' crates/eml_nrt/src` -> no output; `grep -rn 'eml_fatal(' crates/eml_nrt/src | grep -v '//'` -> 3 lines: `entries.rs` (2), `enter.rs` (1);
- `nix develop .#cross -c cargo build -p eml_nrt --target aarch64-unknown-linux-gnu` -> `Finished`; the two cross clippy commands of CLAUDE.md -> no output;
- `cargo build --release -p eml_nrt --example nrt_bench && for c in enter entry-plain entry-shim; do target/release/examples/nrt_bench $c; done` prints `10000000`, `100000000`, `100000000`.
On an x86_64 host the stub tests are not compiled in this commit; the executor then runs N1's host criteria after N2 (Linux) or N3 (Windows) and says so in N1's commit message.
Pitfalls: under Miri a function's address differs per cast; no test compares `(entry.address)()` with `f as *const u8` [V]. `naked_asm!` strings must not be raw strings (the `trust.rs` scanner rejects them). Use `.is_multiple_of(16)` in const asserts (clippy `manual_is_multiple_of`) [V]. Each arch file starts with its own `#![allow(unsafe_code)]`; `arch/mod.rs` has none. `pub unsafe fn`s need a `# Safety` section (clippy).

#### N2 — `Add the SysV x86_64 entry stub`

Files: `crates/eml_nrt/src/arch/x86_64_sysv.rs` (Appendix A.2); `arch/mod.rs` gains the `all(target_arch = "x86_64", not(windows))` arm and narrows `unsupported`; `src/trust.rs` lists the file; `tests/regs.rs` gains the SysV block; `tests/main.rs` cfg becomes `all(not(miri), any(all(target_arch = "aarch64", not(windows)), all(target_arch = "x86_64", not(windows))))`. Docs: spec/native.md 「`eml_enter`」 register set and frame layout for SysV; architecture.md 「`eml_nrt` の内部」.
Completion: the gates; `nix develop .#cross -c cargo build -p eml_nrt --target x86_64-unknown-linux-gnu` -> `Finished` [V]; on an x86_64 Linux or macOS host `cargo nextest run -p eml_nrt` -> `12 tests run: 12 passed` (otherwise this is checked at G1) [I].

#### N3 — `Add the Win64 entry stub`

Files: `crates/eml_nrt/src/arch/x86_64_win64.rs` (Appendix A.2); `arch/mod.rs` final form (Appendix); `trust.rs`; `tests/regs.rs` Win64 block; `tests/main.rs` final form:
```rust
#[cfg(not(miri))]
mod regs;
#[cfg(not(miri))]
mod stub;
```
Docs: spec/native.md 「`eml_enter`」 Win64 register set, shadow space, `movaps` alignment; 「対象」 final list.
Completion: the gates; `nix develop .#cross -c cargo build -p eml_nrt --target x86_64-pc-windows-msvc` -> `Finished` [V]; `grep -rn 'cfg(not(windows))\|cfg(unix)\|target_arch' crates/eml_nrt/tests/main.rs crates/eml_nrt/tests/stub.rs` -> no output.

**G1** (D-12): CI on the N3 commit green on the four targets.

#### A1 — `Give eml_native its runtime edges and the ABI tests` (D-9, D-10, D-11)

Files: `crates/eml_native/Cargo.toml` (L12-20: add `eml_lir.workspace = true`, `eml_nrt.workspace = true`, `eml_rt.workspace = true`; rewrite the comment that says it depends only on `eml_extern` and `eml_cabi`), `src/lib.rs`, `src/trust.rs`, new `src/abi.rs`, `src/mem.rs`, `src/thunk.rs`, `tests/abi.rs`, `tests/main.rs` (`mod abi;`) — Appendix A.3. `ci.yml`: the eml_native line becomes `grep -c -E 'eml_vm'`, the `eml_lir` line is added; asan step `-p eml_native -- --skip fatal::`.
TDD order: `tests/abi.rs` first.
Docs in the same commit:
- spec/native.md 「`eml_enter`」: the thunk is made by `eml_native` per (parameter kinds, result kinds); eml functions have the signature `tail (ctx, leaves...) -> leaves...` and are entered only through a thunk; the Cranelift flags of the one constructor and why each (`preserve_frame_pointers`: stack walking; inline stack probes: frames larger than a page, required on Windows); no per-function override.
- architecture.md L47, L84, L103: `eml_native` depends on `eml_extern`, `eml_cabi`, `eml_lir`, `eml_rt` (layout consts only, named only in `mem.rs`), `eml_nrt`; `eml_vm` depends on none of Cranelift, `eml_native`, `eml_nrt`; the checks of the CI step. 「`eml_native` の内部」 (L411-418): `abi.rs`, `mem.rs`, `thunk.rs`; `JITModule` only in `bridge.rs`; `bridge.rs` keeps its own flags.
- testing.md 「`eml_native` の検証」: the three ABI tests; the echo-coverage rule; the ASan skip.
- object-model.md 「語の位置」 where the `REUSE_SCANNED_LIMIT` assert is described: `eml_native` (`mem.rs`) next to `eml_vm` (`data.rs` L15).
- `CLAUDE.md`: Architecture row of `eml_native`; the ASan comment.
Test change kind: none.
Completion criteria:
- the gates; `cargo nextest run -p eml_native -E 'test(/abi::/)'` -> `3 tests run: 3 passed` [V aarch64 macOS];
- the five lines of D-11's dependency step each exit 0 [V];
- `grep -rln 'eml_rt' crates/eml_native/src` -> `crates/eml_native/src/mem.rs`; `grep -rln 'JITModule' crates/eml_native/src` -> `crates/eml_native/src/bridge.rs`; `grep -rn 'Linkage::Import' crates/eml_native/src` -> 1 line (`abi.rs`); `grep -rn 'CallConv::Tail' crates/eml_native/src` -> 1 line (`abi.rs`); `grep -rn 'preserve_frame_pointers' crates/eml_native/src` -> 1 line (`abi.rs`); `grep -rln 'settings::builder' crates/eml_native/src` -> `abi.rs` and `bridge.rs`; `grep -rnE 'ins\(\)\.(load|store|uload|sload)' crates/eml_native/src` -> lines of `bridge.rs` and `thunk.rs` only; `grep -rnE 'eml_rt::(Ctx|Heap)|Ctx<' crates/eml_native/src` -> no output; `grep -rn 'thread_local' crates/eml_native/src` -> no output [V];
- `nix develop -c nix/third-party-licenses.sh --check` passes.
Pitfalls: `InstBuilder::iadd_imm` is deprecated in Cranelift 0.137 — use `iadd_imm_s` [V]. A `pub fn` returning `ModuleResult` trips clippy `result_large_err` — return `Built<T>` [V].

**G2** (D-12): CI on the A1 commit green on the four targets.

#### Close of jit-rt

`Close jit-rt`, one commit (roadmap 「進め方」):
1. spec: `grep -nE 'jit-|sched-split|native-aot|native-inline' docs/spec/native.md docs/spec/object-model.md` -> no output; read the lines this stage added for "now ..., later ..." forecasts and remove them.
2. Measure: `bench/scripts.sh` (resize rows in a separate `Resize bench rows` commit if any leaves its range), then `bench/run.sh`, `bench/rt.sh`, `bench/scripts.sh` on the last code commit; replace the three tables of benchmarks.md 「今の数値」 (the rt table now has the three `nrt_bench` rows); fill 「ネイティブの数値」 with the three `nrt_bench` rows; add a titled finding 「`Ctx` の欄の順」 with R1's gate table and the cause (`Native` first: the yield path and `map-reuse` gain, and an out-of-line `Heap` call from a `Ctx` method costs one `add`, which shows in `handle`, `show-int`, `show-int-long`, `slice-short`; `heap` first measured: no row regresses, `yield` about -2.7 %).
3. status.md: one line: the native runtime has the entry stubs and no code generator; `Bytes.empty` no longer checks the request budget.
4. architecture.md: verify N1 / A1's edits.
5. roadmap.md: delete the `jit-rt` bullet; in `jit-core`'s 前提 replace `jit-rt` by what it left (the entry stubs and the native fields of `Ctx`).
6. overview.md / README: only rows that changed.
7. CLAUDE.md files: verify.
8. Delete the plan after checking every D-n of jit-rt is in `docs/`.
9. Commit message: documents; "no existing test changed; new tests only" (or R4's kind-2 rows); bench numbers; the fix commits of G1 / G2 if any; deferred items.
**G3**: CI on the closing commit green on the four targets; then the branch is merged.

### 5.2 Stage sched-split

**Prerequisites and baseline** (branch `plan/sched-split`). Record `cargo nextest run` total and these per-binary counts (`cargo nextest list -p eml_vm -p eml_sched 2>/dev/null | awk '{print $1}' | sort | uniq -c`), and the fingerprint `git ls-files 'crates/*/tests/**/*.snap' 'tests/ui' 'crates/eml_vm/tests/bench.rs' | xargs shasum | shasum`:

| binary | base | after S1 | after S2 | after S3 |
|---|---:|---:|---:|---:|
| `eml_vm` (lib) | 201 | 119 | 113 | 113 |
| `eml_vm::integration` | 579 | 578 | 578 | 578 |
| `eml_sched` (lib) | - | 82 | 88 | 88 |
| `eml_sched::integration` | - | 1 | 1 | 1 |
| sum | 780 | 780 | 780 | 780 |

([V]: base 201 / 579 and final 113 / 88; the probe left the import test in eml_vm, so the 578 / 1 split and the S1, S2 columns are [I] from the test counts per file: `cells.rs` 82, `doorbell.rs` 3, `park.rs` 2, `driver.rs` 1, `error.rs` 0.) Workspace total stays `4878 passed, 22 skipped` when the stage runs before jit-rt; after jit-rt it is jit-rt's final total.

#### S1 — `Move the task cells and the run errors to eml_sched` (D-20..D-23, D-25, D-26)

1. Root `Cargo.toml`: `eml_sched = { path = "crates/eml_sched" }`. `crates/eml_sched/Cargo.toml` as D-20 (this commit: dependency `eml_rt` only; `eml_io` and `libc` arrive in S2). `crates/eml_vm/Cargo.toml`: `eml_sched.workspace = true` in `[dependencies]`, `eml_sched = { workspace = true, features = ["testing"] }` in `[dev-dependencies]`.
2. `git mv crates/eml_vm/src/cells.rs crates/eml_vm/src/error.rs crates/eml_sched/src/`. `crates/eml_sched/src/lib.rs`: crate doc and `pub mod cells; pub mod error;`.
3. In both files: `sed -i 's/pub(crate)/pub/g'`. `error.rs` keeps `use crate::cells::CellFault;`.
4. `cells.rs`: delete the two `cfg_attr(not(feature = "gui"), ...)` attributes; apply D-23's attribute changes (compile-driven: both `cargo check -p eml_sched` and `cargo check -p eml_sched --features testing` pass, then `cargo test -p eml_vm --lib --no-run`). Leave `cfg!(test)`.
5. `crates/eml_vm/src/lib.rs`: remove `mod cells;` and `mod error;`, add `pub(crate) use eml_sched::{cells, error};`. `crates/eml_vm/src/jobs.rs`: the fixture line of D-23.
6. Move the import test (D-25): `crates/eml_sched/tests/main.rs` (`mod imports;`), `tests/imports.rs`.
7. Commands (D-26) in `ci.yml`, `CLAUDE.md`, testing.md; the CI dependency step gains the two lines of D-11 (the eml_sched edges and "the `eml` binary has no `eml_sched/testing`").
Docs: architecture.md (repository list, crate table: `eml_sched` — the task-cell core and the run interface shared by the engines; depends on `eml_rt`, `eml_io`; must not depend on Cranelift, `eml_lir`, `eml_vm`, `eml_nrt`, `eml_native`; 「crate の依存関係」: `eml_vm -> eml_sched`, the dev-only feature `testing`, what it gates and that jit-tasks deletes it (D-23); L326 where it lists `cells.rs`); vm.md L719 (`cells.rs` is in `eml_sched`), L792, L905, L958 (fix the crate where stated), 「置き場所」 for the moved types; testing.md L197, L202, L284-285, L706, L787; multicore.md L63 (`eml_sched` holds the cells and the run interface; scheduler and worker table are still in `eml_vm`).
Test changes — kind 3 only: (a) `the_task_cells_import_only_std_and_the_runtime` moves from `crates/eml_vm/tests/tasks.rs` to `crates/eml_sched/tests/imports.rs`, body unchanged; (b) the 82 unit tests of `cells.rs` move with the file; (c) `crates/eml_vm/src/jobs.rs` fixture `settled` gains one setup line, expected values unchanged.
Completion: the gates; the "after S1" column; the fingerprint equals the baseline; `grep -c 'cfg(any(test, feature = "testing"))' crates/eml_sched/src/cells.rs` -> `25`; `grep -c 'cfg!(test)' crates/eml_sched/src/cells.rs` -> `1`; `grep -rn 'feature = "testing"' crates/eml_sched/src | grep -c 'cfg!'` -> `0`; `cargo tree -p eml_cli -e normal,build -f '{p} {f}' | grep -c 'eml_sched.*testing'` -> `0` [V]; `grep -c 'feature = "gui"' crates/eml_sched/src/cells.rs` -> `0`; `test "$(cargo tree -p eml_sched -e normal --target all | grep -c -E 'cranelift|eml_lir|eml_vm|eml_native|eml_nrt')" -eq 0`; `cargo clippy -p eml_sched --all-targets -- -D warnings` and the same with `--features testing` -> no output [V]; both Miri runs of `cargo miri test -p eml_sched --lib -- cells::a_read cells::a_write` -> `test result: ok`.

#### S2 — `Move the clock, the doorbell and Park to eml_sched` (D-20, D-21)

`git mv` `park.rs`, `doorbell.rs`, `driver.rs`; `pub(crate)` -> `pub`; `eml_sched/Cargo.toml` gains `eml_io.workspace = true` and `[target.'cfg(unix)'.dependencies] libc.workspace = true`; in `driver.rs` `use crate::error::VmFault;` replaces `use crate::VmFault;`, `use crate::sched::NO_DRIVER;` is deleted and the const with its doc comment (`eml_vm/src/sched.rs` L286-287) is placed **before** the `#[cfg(test)] mod tests` of `driver.rs` (clippy `items_after_test_module` [V]); `eml_vm/src/sched.rs` gains `pub(crate) use eml_sched::driver::NO_DRIVER;`; `eml_vm/src/lib.rs` adds the three names to the `pub(crate) use eml_sched::{...}` line; `eml_sched/src/lib.rs` gains the three `pub mod`s.
Docs: architecture.md; vm.md 「時計と driver」 / 「メインのworkerの待ち」 where they name the files.
Test changes — kind 3: the 6 unit tests of `driver.rs` (1), `park.rs` (2), `doorbell.rs` (3) move with their files.
Completion: the gates; the "after S2" column; fingerprint unchanged.

#### S3 — `Move RunConfig and Finished to eml_sched` (D-20, D-21, D-24)

New `crates/eml_sched/src/config.rs` with, moved verbatim: `RunStats` + `fields()` (without `#[non_exhaustive]`), `Finished`, `RunConfig` + `Default` + `with_*` (`eml_vm/src/lib.rs` L62-176, L190-285), `Sched` + `seed()` (`sched.rs` L101-121), `Seat` (`workers.rs` L206-213); imports `std::sync::Arc`, `std::time::Duration`, `eml_rt::{Control, Limits}`, `crate::doorbell::Doorbell`, `crate::driver::{Clock, JOB_QUIESCE_LIMIT, VirtualSignal}`, `crate::park::ParkFactory`. `eml_vm`: the re-export lines of D-21 (drop imports the compiler reports unused). The five sites of D-24.
Docs: vm.md 「置き場所」 and L410 (the selection enum `Sched` and the run configuration live in `eml_sched`; `Policy` and `Scheduler` stay in `eml_vm`'s `sched.rs`); architecture.md 「CLI と lib API」 if it names `eml_vm` as the definition site.
Test changes — kind 3, listed: the five sites of D-24 (same fields and values).
Completion: the gates; the "after S3" column; workspace total as the baseline [V: `4878 passed, 22 skipped` on the probe]; fingerprint unchanged; `grep -c 'pub(crate)' crates/eml_sched/src/*.rs` -> every count `0`; `grep -rn 'for RuntimeError' crates/*/src | grep Display` -> 1 line, in `crates/eml_sched/src/error.rs`; `git diff 432fffb4 --stat -- crates/eml_cli/src crates/eml_test_support/src` lists only `eml_test_support/src/lib.rs`; `cargo clippy -p eml_cli --no-default-features --features run -- -D warnings`, `... --features run,gui ...`, `cargo clippy -p eml_io -p eml_vm --all-targets -- -D warnings` -> no output [V].

#### Close of sched-split

`Close sched-split`: 1. spec: `grep -nE 'jit-|sched-split' docs/spec/vm.md` -> no output; 2. bench: `crates/eml_vm` changed, so the three scripts run and the tables are replaced; this stage has no native path, so a VM row that moves is recorded, not gated (the only mechanism is cross-crate inlining of `cells` functions); 3. status.md: no change; 4. architecture.md: verify; 5. roadmap.md: delete the `sched-split` bullet, fix `jit-core`'s 前提, L337 says the first step is done; 6. overview / README: only if they list crates; 7. CLAUDE.md: Architecture block row `eml_sched`, Commands (Miri line split, clippy row); 8. delete the plan; 9. commit message: the move list, "kind 3 only" with the items of S1-S3, the `testing` feature and its end (jit-tasks), `RunStats` without `#[non_exhaustive]`, bench.
The stage touches no ABI-specific code; its CI run is the ordinary one after the merge (or, with the user's permission, D-12 step 2 on `plan/sched-split`).

## 6. Unresolved items

| # | Item | Default | Who / when |
|---|---|---|---|
| U-1 | ASan and the longjmp | skip modules named `fatal` in the ASan job | jit-core may call `__asan_handle_no_return` under a custom cfg |
| U-2 | Fatal codes beyond 1 and 2, and the meaning of `site` values | defined by jit-core in `eml_nrt/src/enter.rs`, listed in spec/native.md 「`eml_fatal`」 | S2b |
| U-3 | The Win64 register test compares the low 64 bits of xmm6-xmm15 (the stub saves 128) | accept | revisit on a Win64 bug |
| U-4 | `eml_nrt` -> `eml_sched` edge | added by jit-core with its first use | S2b |
| U-5 | The N-32 debug assert at the hand-back site | jit-core's run driver, then jit-tasks' worker hand-back, using `eml_nrt::detached` | S2b, S4 |
| U-6 | `#[no_mangle]` on entries | none until native-aot | native-aot |
| U-7 | Removing `eml_sched/testing` | deleted entirely by jit-tasks | S4 |
| U-8 | Echo functions per entry shape once the std rows arrive | hand-written per shape, or generated from the row list | S3 |

## 7. Additional checks

All on aarch64-apple-darwin (macOS 27.2), rustc 1.99.1, Cranelift 0.137.1, in the scratch clone; the repository was not modified.

1. [V] `Native` as D-1..D-4, `Native` first: `cargo verus verify -p eml_rt` -> 0 errors (`1229 verified` with `Native` alone; `1242 verified, 0 errors` for the full r3 state, re-run after the field order went back). Without `lemma_same_words` the setters' postconditions fail. The N-4 const asserts compile; `size_of::<Heap<Fast>>()` = 1232, `size_of::<Ctx<Fast>>()` = 1456. The T18 test passes natively and under Miri with `-Zmiri-strict-provenance`, with and without `-Zmiri-tree-borrows`. `trust::the_trust_list_is_exactly_the_trusted_items` needs the T18 entry; `the_core_has_no_native_boundary` passes.
2. [V] sched-split probe (final state of S1+S2+S3): `cargo nextest run` -> `4878 tests run: 4878 passed, 22 skipped`; workspace clippy clean; `-p eml_cli --no-default-features --features run`, `--features run,gui`, `-p eml_io -p eml_vm -p eml_sched --all-targets`, `-p eml_sched --all-targets [--features testing]` clean; `cargo tree -p eml_sched` shows `eml_io`, `eml_rt`, `libc`. Intermediate findings: 54 compile errors before the `testing` feature; 5 `jobs::tests` failures ("a host has no bin for its handles") from the `cfg!(test)` branch, fixed by the fixture line with `cfg!(test)` left local (780 eml_vm + eml_sched tests pass); `E0639` on `RunStats`; clippy `field_reassign_with_default` at the five sites.
3. [V] Exports: R2 alone verifies on top of R1; R3's invariant reroute in `frozen.rs`, `heap/inv.rs` or `heap/safe.rs` (each alone) makes `lemma_moves_parent` exceed its rlimit, `heap/field.rs` alone does not, `spinoff_prover` does not fix it, `rlimit(20)` does; the five proof-step sites of `safe.rs` reroute without further change; `bytes_empty` as in R4 verifies with no other edit; the R3 residual greps give the stated lines. `cargo nextest run -p eml_rt -p eml_vm` passes with all of it.
4. [V] Bench. (a) `Native` first, r1: medians of 3 per row (1.3). (b) `heap` first, r2: 2 alternating pairs, plain gate exit 0, largest gated delta +0.006 %, `yield` -2.732 %. (c) `Native` first, the full r3 probe (R1-R4, eml_nrt) vs base with `bench/rt-gate.sh --allow 'handle show-int show-int-long slice-short' --allow-bound 0.6 432fffb4 2`: exit 0 over 2 alternating pairs [V]: `handle` +0.156 %, `show-int` +0.480 %, `show-int-long` +0.191 %, `slice-short` +0.362 % (allowed, each <= +0.6 %); every other gated row <= +0.038 % (`concat-copy`); `yield` -10.849 %, `map-reuse` -4.275 %, `list` +0.002 %; sum over the gated rows -47,927,761 instructions. The `compare` function run on r1's single pair gives the same four rows over +0.1 % and a sum of -45,368,627 instructions.
5. [V] eml_nrt as Appendix A.2: `cargo nextest run -p eml_nrt` -> 12 passed, including the register test, the frame-record test (it fails when the stub's `add x29, sp, ..` is replaced by `nop`), both shim arms, the parked heap error, an entry declared with the row kind tokens and one with `-> never` (returns `()`, descriptor result `None`); removing two restores from the longjmp half fails the register test (checked in r1; the r2 frame-record change leaves the restore list as is). `nix develop .#cross -c cargo build -p eml_nrt --target <t>` finishes for `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`, `aarch64-unknown-linux-gnu` with the r2 stubs; the x86 test helpers, including the frame-record ones, were assembled for both x86 targets by compiling them temporarily inside the library. Miri: `entries::` 2 passed under both models. `nrt_bench`: `enter` 538,721,724, `entry-plain` 818,687,270, `entry-shim` 818,705,818, `empty` 20,092,931 instructions.
6. [V] eml_native as Appendix A.3: the three ABI tests pass on aarch64 macOS (10^7 self `return_call`s, `return_call` to an 11-leaf callee, 3-value return through the thunk, fatal from 0 / 1 / 100 `tail` frames, echo of 2, 6, 9 and 18 mixed parameters built from the descriptor). A1's greps give the stated results with the files `abi.rs`, `mem.rs`, `thunk.rs`. Workspace clippy clean; `cargo nextest run` -> 4898 run, 4897 passed, 1 failed: `citations::every_cited_heading_exists`, because the probe's comments cite doc sections the probe did not write.
7. [V] N-3's grep at base prints `alloc.rs:3655`. CI triggers are `push` to `main`, `pull_request`, `workflow_dispatch` (ci.yml L4-8).
8. [V] N-41's two checks on the sched-split probe: both counts 0.
9. [V] `bench/rt-gate.sh HEAD 1` end to end on the probe (with the `nrt_bench` rows in `bench/rt.sh`): 44 rows, the three new rows within +0.014 %, branch restored by the exit trap (a first version of the trap referenced a local variable and failed; the appendix has the fixed one). `list` read +0.154 % in that single identical-build pair, which is why one pair is not a verdict. `an_empty_bytes_ignores_the_request_budget` passes in both modes.
10. Not checked: execution of the SysV and Win64 stubs and of the ABI tests on x86_64 Linux, aarch64 Linux and Windows (A-1); the three sched-split commits and N1 / N2's intermediate states individually; 5-pair bench medians (A-7); `bench/run.sh`, `bench/scripts.sh`; ASan and TSan jobs; `nix build`; the `gh` commands of D-12 (A-8).

---

## Appendix A: reference sources

These are the files of the probe (final state of jit-rt; all tests named in section 5.1 pass with exactly this text on aarch64 macOS; the x86_64 files are assembled, not run — A-1). Recreate them as written, except comments and docs, which are rewritten in Japanese and must cite existing sections. Tasks N1 and N2 create subsets as section 5.1 says.

### A.1 The T18 test (`crates/eml_rt/src/heap/tests/trusted.rs`, before `a_notice_from_another_thread_is_seen`)

```rust
// T18: `Native` のポインタの欄。`stop` は `Ctx` が持つ止める語を指し、`Ctx` が別のスレッドへ移った後も読める。`jmp` と
// `runtime` は作ったときは null で、`runtime` は置いたポインタをそのまま返す。置いた失敗は1回だけ取り出せ、取り出さずに
// 落とした `Ctx` も失敗を漏らさない
#[test]
fn the_native_pointers_follow_the_ctx_across_threads() {
    let control = Arc::new(Control::new(Limits::default()));
    let mut ctx = Ctx::<Fast>::new(control.clone(), host(&Arc::default()));
    assert!(ctx.jmp().is_null() && ctx.runtime().is_null());
    // SAFETY: `stop_word` は `ctx` が生きている間の、その止める語を指す
    assert_eq!(unsafe { &*ctx.stop_word() }.load(Ordering::Relaxed), 0);
    let mut slot = Box::new(7u64);
    ctx.set_runtime((&raw mut *slot).cast());
    // SAFETY: 上で置いた `slot` を指す
    unsafe { *ctx.runtime().cast::<u64>() += 1 };
    ctx.set_runtime(std::ptr::null_mut());
    assert_eq!(*slot, 8);
    ctx.set_stack_limit(4096);
    assert_eq!(ctx.stack_limit(), 4096);
    assert_eq!(ctx.set_fault(Fault::Output("lost".into())), None);
    assert_eq!(
        ctx.set_fault(Fault::Output("first".into())),
        Some(Fault::Output("lost".into()))
    );
    assert_eq!(ctx.take_fault(), Some(Fault::Output("first".into())));
    assert_eq!(ctx.take_fault(), None);
    control.interrupt();
    let moved = std::thread::spawn(move || {
        // SAFETY: 同じく、移った先でも `ctx` の止める語を指す
        let bits = unsafe { &*ctx.stop_word() }.load(Ordering::Relaxed);
        let _ = ctx.set_fault(Fault::Output("left behind".into()));
        (bits, ctx.poll(0))
    })
    .join()
    .unwrap();
    assert_eq!(moved, (Control::INTERRUPT, Err(crate::Stop::Interrupted)));
}
```
(`host`, `Fault`, `Arc`, `Ordering`, `Control`, `Limits`, `Ctx`, `Fast` are already imported in that file.)

### A.2 `crates/eml_nrt`

`crates/eml_nrt/Cargo.toml`
```toml
[package]
name = "eml_nrt"
version.workspace = true
edition.workspace = true
license.workspace = true
autotests = false

[lib]
doctest = false

[dependencies]
eml_rt.workspace = true

[[test]]
name = "integration"
path = "tests/main.rs"
```

`crates/eml_nrt/src/lib.rs`
```rust
//! ネイティブのコードが呼ぶランタイム (docs/spec/native.md の「エントリ」)。
#![deny(unsafe_code)]

mod arch;
mod enter;
mod entries;
#[cfg(test)]
mod trust;

pub use arch::{JmpBuf, SUPPORTED};
#[doc(hidden)]
pub use eml_rt as __rt;
pub use enter::{
    FATAL_FAULT, FATAL_PANICKED, Fatal, Raise, Thunk, detached, eml_enter, eml_fatal, enter,
};
pub use entries::{Entry, Kind};

/// エントリの表。module ごとの表をここに並べる。
const TABLES: &[&[Entry]] = &[enter::ENTRIES];

/// 生成したコードが呼べるすべてのエントリ。
pub fn entries() -> impl Iterator<Item = &'static Entry> {
    TABLES.iter().flat_map(|table| table.iter())
}
```

`crates/eml_nrt/src/enter.rs`
```rust
//! probe: enter / fatal
#![allow(unsafe_code)]

use eml_rt::{Ctx, Fast, Fault, HeapError};

use crate::arch;
pub use crate::arch::eml_enter;

/// `eml_enter` が呼ぶ、既定の規約の関数。
pub type Thunk = unsafe extern "C" fn(ctx: *mut Ctx<Fast>, entry: *const u8, argv: *mut u64);

/// エントリまで切った誤り。`code` は 0 でない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fatal {
    pub code: u32,
    pub site: u32,
}

/// エントリの中で Rust の panic を捕まえた。中身はない。
pub const FATAL_PANICKED: u32 = 1;
/// `Fault` が `Ctx` に置いてある (`Ctx::take_fault`)。
pub const FATAL_FAULT: u32 = 2;

/// 包みが返す誤り。`park` は中身を `Ctx` に置き、`eml_fatal` に渡す符号を返す。
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

impl Raise for Fatal {
    fn park(self, _ctx: &mut Ctx<Fast>) -> Fatal {
        self
    }
}

/// `eml_enter` を呼び、結果を `Result` に直す。
///
/// # Safety
/// `eml_enter` と同じ。
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

/// `Ctx` がエントリの外にあり、スレッドを移せる状態か。
pub fn detached(ctx: &Ctx<Fast>) -> bool {
    ctx.jmp().is_null() && ctx.runtime().is_null()
}

crate::entries! {
    pub(crate) const ENTRIES;

    /// いちばん内側の `eml_enter` まで切る。
    pub plain fn eml_fatal(ctx, code: i64, site: i64) -> ! {
        let code = code as u32;
        debug_assert!(code != 0, "a fatal code is not 0");
        debug_assert!(
            (0..=i64::from(u32::MAX)).contains(&site),
            "a fault site fits in 32 bits"
        );
        // SAFETY: 呼ぶ側は生きている `Ctx` を渡す
        debug_assert!(!unsafe { (*ctx).jmp() }.is_null(), "eml_fatal outside eml_enter");
        // SAFETY: 呼ぶ側は `eml_enter` の中にいて、この枠は落とすものを持たない
        unsafe { arch::longjmp(ctx, (u64::from(code) << 32) | u64::from(site as u32)) }
    }
}
```

`crates/eml_nrt/src/entries.rs`
```rust
//! probe: the entry macro and the descriptor row
#![allow(unsafe_code)]

/// エントリの引数と結果の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    I64,
    F64,
}

/// エントリの記述子の行。
#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub symbol: &'static str,
    pub address: fn() -> *const u8,
    /// `ctx` の後の引数。
    pub params: &'static [Kind],
    pub result: Option<Kind>,
}

#[doc(hidden)]
#[macro_export]
macro_rules! __kind {
    (i64) => {
        $crate::Kind::I64
    };
    (f64) => {
        $crate::Kind::F64
    };
    (obj) => {
        $crate::Kind::I64
    };
    (tobj) => {
        $crate::Kind::I64
    };
    (unit) => {
        $crate::Kind::I64
    };
    (int) => {
        $crate::Kind::I64
    };
    (enum) => {
        $crate::Kind::I64
    };
    (float) => {
        $crate::Kind::F64
    };
}

/// 引数と結果の Rust の型。`i64` と `f64` のほかに、行の表が書く Repr の名前も受ける。
#[doc(hidden)]
#[macro_export]
macro_rules! __ty {
    (!) => {
        !
    };
    (never) => {
        ()
    };
    (f64) => {
        f64
    };
    (float) => {
        f64
    };
    (i64) => {
        i64
    };
    (obj) => {
        i64
    };
    (tobj) => {
        i64
    };
    (unit) => {
        i64
    };
    (int) => {
        i64
    };
    (enum) => {
        i64
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __result {
    () => {
        None
    };
    (!) => {
        None
    };
    (never) => {
        None
    };
    ($t:tt) => {
        Some($crate::__kind!($t))
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __entry {
    (plain [$(#[$m:meta])*] $v:vis $name:ident $ctx:ident ($($p:ident : $t:tt),*) ($($r:tt)?) $body:block) => {
        $(#[$m])*
        #[allow(clippy::missing_safety_doc)]
        $v unsafe extern "C" fn $name(
            $ctx: *mut $crate::__rt::Ctx<$crate::__rt::Fast>
            $(, $p: $crate::__ty!($t))*
        ) $(-> $crate::__ty!($r))? $body
    };
    (shim [$(#[$m:meta])*] $v:vis $name:ident $ctx:ident ($($p:ident : $t:tt),*) ($($r:tt)?) $body:block) => {
        $(#[$m])*
        #[allow(clippy::missing_safety_doc)]
        $v unsafe extern "C" fn $name(
            ctx: *mut $crate::__rt::Ctx<$crate::__rt::Fast>
            $(, $p: $crate::__ty!($t))*
        ) $(-> $crate::__ty!($r))? {
            let outcome = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                // SAFETY: 呼ぶ側は、この worker の生きている `Ctx` を渡す。この借用は閉包の中で終わる
                let $ctx: &mut $crate::__rt::Ctx<$crate::__rt::Fast> = unsafe { &mut *ctx };
                $body
            }));
            match outcome {
                Ok(Ok(value)) => value,
                Ok(Err(error)) => {
                    // SAFETY: 上と同じ。借用は `park` の間だけである
                    let fatal = $crate::Raise::park(error, unsafe { &mut *ctx });
                    // SAFETY: この枠は落とすものを持たない
                    unsafe { $crate::eml_fatal(ctx, fatal.code as i64, fatal.site as i64) }
                }
                Err(payload) => {
                    drop(payload);
                    // SAFETY: この枠は落とすものを持たない
                    unsafe { $crate::eml_fatal(ctx, $crate::FATAL_PANICKED as i64, 0) }
                }
            }
        }
    };
}

/// エントリの関数と記述子の表を一緒に宣言する。
#[macro_export]
macro_rules! entries {
    (
        $(#[$tm:meta])* $tv:vis const $table:ident;
        $(
            $(#[$m:meta])*
            $v:vis $form:ident fn $name:ident($ctx:ident $(, $p:ident : $t:tt)* $(,)?) $(-> $r:tt)? $body:block
        )*
    ) => {
        $(
            $crate::__entry! { $form [$(#[$m])*] $v $name $ctx ($($p: $t),*) ($($r)?) $body }
        )*
        $(#[$tm])*
        $tv const $table: &[$crate::Entry] = &[
            $(
                $crate::Entry {
                    symbol: stringify!($name),
                    address: || $name as *const u8,
                    params: &[$($crate::__kind!($t)),*],
                    result: $crate::__result!($($r)?),
                },
            )*
        ];
    };
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use eml_rt::{Buffering, Control, Ctx, Fast, HostRef, Limits, StdHost};

    use crate::{Fatal, Kind};

    crate::entries! {
        const SAMPLE;

        plain fn eml_t_plain(ctx, a: i64, b: f64) -> f64 {
            // SAFETY: テストは生きている `Ctx` を渡す
            let limit = unsafe { (*ctx).stack_limit() };
            a as f64 + b + limit as f64
        }

        shim fn eml_t_shim(ctx, a: i64) -> i64 {
            ctx.set_stack_limit(a as usize);
            Ok::<i64, Fatal>(a + 1)
        }

        #[allow(non_snake_case)]
        shim fn eml_t_Named(ctx, a: obj, b: float, c: enum) -> int {
            ctx.set_stack_limit(a as usize);
            Ok::<i64, Fatal>(a + b as i64 + c)
        }

        #[allow(non_snake_case)]
        shim fn eml_t_Never(ctx, a: tobj) -> never {
            ctx.set_stack_limit(a as usize);
            Ok::<(), Fatal>(())
        }

        shim fn eml_t_unit(ctx, a: i64) {
            ctx.set_stack_limit(a as usize);
            Ok::<(), Fatal>(())
        }
    }

    fn ctx() -> Box<Ctx<Fast>> {
        let host = StdHost::new(
            Box::new(std::io::sink()),
            Box::new(std::io::sink()),
            std::path::PathBuf::new(),
            Buffering::Block,
        );
        let control = Arc::new(Control::new(Limits::default()));
        Box::new(Ctx::new(control, HostRef::new(Arc::new(host))))
    }

    #[test]
    fn entries_run_and_describe_themselves() {
        let mut ctx = ctx();
        let raw: *mut Ctx<Fast> = &mut *ctx;
        // SAFETY: `raw` はこのテストが持つ `Ctx` で、ほかに借用はない
        unsafe {
            assert_eq!(eml_t_shim(raw, 4), 5);
            assert_eq!(eml_t_Named(raw, 1, 2.0, 3), 6);
            eml_t_Never(raw, 9);
            eml_t_unit(raw, 6);
            assert_eq!(eml_t_plain(raw, 1, 0.5), 7.5);
        }
        let rows: Vec<_> = SAMPLE
            .iter()
            .map(|e| (e.symbol, e.params, e.result))
            .collect();
        assert_eq!(
            rows,
            [
                ("eml_t_plain", &[Kind::I64, Kind::F64][..], Some(Kind::F64)),
                ("eml_t_shim", &[Kind::I64][..], Some(Kind::I64)),
                (
                    "eml_t_Named",
                    &[Kind::I64, Kind::F64, Kind::I64][..],
                    Some(Kind::I64)
                ),
                ("eml_t_Never", &[Kind::I64][..], None),
                ("eml_t_unit", &[Kind::I64][..], None),
            ]
        );
    }

    #[test]
    fn the_table_has_only_named_prefixes() {
        for entry in crate::entries() {
            let s = entry.symbol;
            assert!(
                ["eml_rt_", "eml_x_", "eml_nrt_"]
                    .iter()
                    .any(|p| s.starts_with(p))
                    || ["eml_enter", "eml_fatal", "eml_run_pending"].contains(&s),
                "{s}"
            );
        }
        let mut names: Vec<_> = crate::entries().map(|e| e.symbol).collect();
        let all = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), all);
    }
}
```

`crates/eml_nrt/src/arch/mod.rs`
```rust
//! ISA と ABI ごとの stub を選ぶ (docs/spec/native.md の「エントリ」)。

#[cfg(all(target_arch = "aarch64", not(windows)))]
#[path = "aarch64.rs"]
mod imp;
#[cfg(all(target_arch = "x86_64", not(windows)))]
#[path = "x86_64_sysv.rs"]
mod imp;
#[cfg(all(target_arch = "x86_64", windows))]
#[path = "x86_64_win64.rs"]
mod imp;
#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
#[path = "unsupported.rs"]
mod imp;
#[cfg(all(target_arch = "aarch64", windows))]
#[path = "unsupported.rs"]
mod imp;

pub(crate) use imp::longjmp;
pub use imp::{JmpBuf, SUPPORTED, eml_enter};
```

`crates/eml_nrt/src/arch/aarch64.rs`
```rust
//! probe: AAPCS64
#![allow(unsafe_code)]
use core::mem::{offset_of, size_of};

use eml_rt::{CTX_JMP, Ctx, Fast};

use crate::enter::Thunk;

pub const SUPPORTED: bool = true;

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

/// # Safety
/// `ctx` はこの worker の生きている `Ctx` で、呼ぶ側はそれへの `&` も `&mut` も持たない。`thunk` は `entry` と `argv` を
/// 受ける既定の規約の関数で、`argv` はその引数と結果の語の数だけ読み書きできる (docs/spec/native.md の「エントリ」)。
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
```

`crates/eml_nrt/src/arch/x86_64_sysv.rs`
```rust
//! probe: SysV x86_64
#![allow(unsafe_code)]
use core::mem::{offset_of, size_of};

use eml_rt::{CTX_JMP, Ctx, Fast};

use crate::enter::Thunk;

pub const SUPPORTED: bool = true;

#[repr(C)]
pub struct JmpBuf {
    pub prev: *mut JmpBuf,
    ctx: *mut Ctx<Fast>,
    /// rbx, rbp, r12, r13, r14, r15
    r: [u64; 6],
}

/// 戻り番地の 8 バイトと合わせて `call` の前の rsp を 16 の倍数にする詰め物を足した枠の大きさ。
const FRAME: usize = size_of::<JmpBuf>();
const PREV: usize = offset_of!(JmpBuf, prev);
const CTX: usize = offset_of!(JmpBuf, ctx);
const R: usize = offset_of!(JmpBuf, r);
const _: () = assert!(FRAME.is_multiple_of(16));

/// # Safety
/// `ctx` はこの worker の生きている `Ctx` で、呼ぶ側はそれへの `&` も `&mut` も持たない。`thunk` は `entry` と `argv` を
/// 受ける既定の規約の関数で、`argv` はその引数と結果の語の数だけ読み書きできる (docs/spec/native.md の「エントリ」)。
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
```

`crates/eml_nrt/src/arch/x86_64_win64.rs`
```rust
//! probe: Win64 x86_64
#![allow(unsafe_code)]
use core::mem::{offset_of, size_of};

use eml_rt::{CTX_JMP, Ctx, Fast};

use crate::enter::Thunk;

pub const SUPPORTED: bool = true;

#[repr(C, align(16))]
pub struct JmpBuf {
    pub prev: *mut JmpBuf,
    ctx: *mut Ctx<Fast>,
    /// rbx, rbp, rdi, rsi, r12, r13, r14, r15
    r: [u64; 8],
    /// xmm6..xmm15
    xmm: [[u64; 2]; 10],
}

/// 呼ばれる側の影の領域。`JmpBuf` はその上に置く。
const SHADOW: usize = 32;
const FRAME: usize = SHADOW + size_of::<JmpBuf>();
const PREV: usize = SHADOW + offset_of!(JmpBuf, prev);
const CTX: usize = SHADOW + offset_of!(JmpBuf, ctx);
const R: usize = SHADOW + offset_of!(JmpBuf, r);
const XMM: usize = SHADOW + offset_of!(JmpBuf, xmm);
const _: () = assert!(FRAME.is_multiple_of(16) && XMM.is_multiple_of(16));

/// # Safety
/// `ctx` はこの worker の生きている `Ctx` で、呼ぶ側はそれへの `&` も `&mut` も持たない。`thunk` は `entry` と `argv` を
/// 受ける既定の規約の関数で、`argv` はその引数と結果の語の数だけ読み書きできる (docs/spec/native.md の「エントリ」)。
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
```

`crates/eml_nrt/src/arch/unsupported.rs`
```rust
//! probe: no stub
#![allow(unsafe_code)]
use eml_rt::{Ctx, Fast};

use crate::enter::Thunk;

pub const SUPPORTED: bool = false;

#[repr(C)]
pub struct JmpBuf {
    pub prev: *mut JmpBuf,
}

/// # Safety
/// `ctx` はこの worker の生きている `Ctx` で、呼ぶ側はそれへの `&` も `&mut` も持たない。`thunk` は `entry` と `argv` を
/// 受ける既定の規約の関数で、`argv` はその引数と結果の語の数だけ読み書きできる (docs/spec/native.md の「エントリ」)。
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
```

`crates/eml_nrt/examples/nrt_bench.rs`
```rust
//! probe: cost of the entry stub and of the two entry forms
use std::hint::black_box;
use std::sync::Arc;

use eml_nrt::{Fatal, enter};
use eml_rt::{Buffering, Control, Ctx, Fast, HostRef, Limits, StdHost};

eml_nrt::entries! {
    const BENCH;

    plain fn eml_t_plain(ctx, a: i64) -> i64 {
        let _ = ctx;
        a + 1
    }

    shim fn eml_t_shim(ctx, a: i64) -> i64 {
        let _ = ctx;
        Ok::<i64, Fatal>(a + 1)
    }
}

unsafe extern "C" fn add_one(_ctx: *mut Ctx<Fast>, _entry: *const u8, argv: *mut u64) {
    unsafe { *argv += 1 };
}

fn main() {
    let case = std::env::args().nth(1).unwrap_or_default();
    let host = StdHost::new(
        Box::new(std::io::sink()),
        Box::new(std::io::sink()),
        std::path::PathBuf::new(),
        Buffering::Block,
    );
    let mut ctx = Box::new(Ctx::<Fast>::new(
        Arc::new(Control::new(Limits::default())),
        HostRef::new(Arc::new(host)),
    ));
    let raw: *mut Ctx<Fast> = &mut *ctx;
    let plain = black_box(BENCH[0].address)();
    let shim = black_box(BENCH[1].address)();
    type F = unsafe extern "C" fn(*mut Ctx<Fast>, i64) -> i64;
    let mut x = 0i64;
    match case.as_str() {
        "empty" => {}
        "enter" => {
            let mut argv = [0u64];
            for _ in 0..10_000_000 {
                unsafe {
                    enter(raw, black_box(add_one), std::ptr::null(), argv.as_mut_ptr()).unwrap()
                };
            }
            x = argv[0] as i64;
        }
        "entry-plain" => {
            let f = unsafe { std::mem::transmute::<*const u8, F>(plain) };
            for _ in 0..100_000_000 {
                x = unsafe { f(raw, x) };
            }
        }
        "entry-shim" => {
            let f = unsafe { std::mem::transmute::<*const u8, F>(shim) };
            for _ in 0..100_000_000 {
                x = unsafe { f(raw, x) };
            }
        }
        other => panic!("a case: empty, enter, entry-plain, entry-shim; got {other:?}"),
    }
    println!("{x}");
}
```

`crates/eml_nrt/tests/main.rs`
```rust
mod regs;
mod stub;
```

`crates/eml_nrt/tests/stub.rs`
```rust
//! probe: stub tests
#![cfg(not(miri))]
use std::sync::Arc;

use eml_nrt::{FATAL_PANICKED, Fatal, JmpBuf, detached, eml_fatal, enter};
use eml_rt::{Buffering, Control, Ctx, Fast, HostRef, Limits, StdHost};

use crate::regs::{clobber_thunk, enter_noting_fp, enter_with_patterns, fp_thunk};

fn ctx() -> Box<Ctx<Fast>> {
    let host = StdHost::new(
        Box::new(std::io::sink()),
        Box::new(std::io::sink()),
        std::path::PathBuf::new(),
        Buffering::Block,
    );
    let control = Arc::new(Control::new(Limits::default()));
    Box::new(Ctx::new(control, HostRef::new(Arc::new(host))))
}

unsafe extern "C" fn add_one(ctx: *mut Ctx<Fast>, _entry: *const u8, argv: *mut u64) {
    unsafe {
        assert!(!(*ctx).jmp().is_null());
        *argv += 1;
    }
}

#[test]
fn a_thunk_returns_through_enter() {
    let mut ctx = ctx();
    let mut argv = [41u64];
    let r = unsafe { enter(&mut *ctx, add_one, std::ptr::null(), argv.as_mut_ptr()) };
    assert_eq!(r, Ok(()));
    assert_eq!(argv[0], 42);
    assert!(detached(&ctx));
}

/// `fp_thunk` が渡すフレームポインタから記録をたどり、`argv[1]` の値 (`eml_enter` を呼んだときのフレームポインタ) に
/// 3つ以内で着けば `argv[0]` に 1 を書く。
pub unsafe extern "C" fn walk_frames(
    _ctx: *mut Ctx<Fast>,
    _entry: *const u8,
    argv: *mut u64,
    fp: *const u64,
) {
    unsafe {
        let wanted = *argv.add(1);
        let mut record = fp;
        for _ in 0..3 {
            let previous = *record;
            if previous == wanted {
                *argv = 1;
                return;
            }
            record = previous as *const u64;
        }
    }
}

/// stub はフレームの記録をつなぐ。thunk が見るフレームポインタから記録をたどると、`eml_enter` を呼んだときの
/// フレームポインタに着く。
#[test]
fn the_stub_keeps_the_frame_record_chain() {
    let mut ctx = ctx();
    let mut argv = [0u64, 0];
    let before: *mut u64 = &mut argv[1];
    let status = unsafe {
        enter_noting_fp(
            &mut *ctx,
            fp_thunk,
            std::ptr::null(),
            argv.as_mut_ptr(),
            before,
        )
    };
    assert_eq!((status, argv[0]), (0, 1));
    assert!(detached(&ctx));
}

mod fatal {
    use super::*;

    #[inline(never)]
    unsafe extern "C" fn dive(ctx: *mut Ctx<Fast>, n: u64) -> u64 {
        if n == 0 {
            unsafe { eml_fatal(ctx, 7, 9) }
        }
        unsafe { dive(ctx, n - 1) + std::hint::black_box(1) }
    }

    unsafe extern "C" fn dive_thunk(ctx: *mut Ctx<Fast>, _entry: *const u8, argv: *mut u64) {
        unsafe {
            *argv = dive(ctx, *argv);
        }
    }

    #[test]
    fn a_fatal_from_a_deep_frame_returns_from_enter() {
        let mut ctx = ctx();
        for depth in [0u64, 1, 100] {
            let mut argv = [depth];
            let r = unsafe { enter(&mut *ctx, dive_thunk, std::ptr::null(), argv.as_mut_ptr()) };
            assert_eq!(r, Err(Fatal { code: 7, site: 9 }));
            assert_eq!(argv[0], depth);
            assert!(detached(&ctx));
        }
        let mut argv = [1u64];
        assert_eq!(
            unsafe { enter(&mut *ctx, add_one, std::ptr::null(), argv.as_mut_ptr()) },
            Ok(())
        );
    }

    unsafe extern "C" fn nest(ctx: *mut Ctx<Fast>, _entry: *const u8, argv: *mut u64) {
        unsafe {
            let outer = (*ctx).jmp().cast::<JmpBuf>();
            let mut inner = [3u64];
            let r = enter(ctx, dive_thunk, std::ptr::null(), inner.as_mut_ptr());
            assert_eq!(r, Err(Fatal { code: 7, site: 9 }));
            assert_eq!((*ctx).jmp().cast::<JmpBuf>(), outer);
            let mut inner = [10u64];
            let r = enter(ctx, link_thunk, outer.cast(), inner.as_mut_ptr());
            assert_eq!(r, Ok(()));
            *argv = inner[0];
            if *argv.add(1) != 0 {
                eml_fatal(ctx, 5, 6)
            }
        }
    }

    unsafe extern "C" fn link_thunk(ctx: *mut Ctx<Fast>, entry: *const u8, argv: *mut u64) {
        unsafe {
            let me = (*ctx).jmp().cast::<JmpBuf>();
            assert_eq!((*me).prev.cast::<u8>().cast_const(), entry);
            *argv += 1;
        }
    }

    #[test]
    fn enters_nest_and_unlink() {
        let mut ctx = ctx();
        let mut argv = [0u64, 0];
        assert_eq!(
            unsafe { enter(&mut *ctx, nest, std::ptr::null(), argv.as_mut_ptr()) },
            Ok(())
        );
        assert_eq!(argv[0], 11);
        assert!(detached(&ctx));
        let mut argv = [0u64, 1];
        assert_eq!(
            unsafe { enter(&mut *ctx, nest, std::ptr::null(), argv.as_mut_ptr()) },
            Err(Fatal { code: 5, site: 6 })
        );
        assert!(detached(&ctx));
    }

    #[test]
    fn callee_saved_registers_survive_a_fatal() {
        let mut ctx = ctx();
        let mut argv = [0u64];
        let mut out = 0u64;
        let mask = unsafe {
            enter_with_patterns(
                &mut *ctx,
                clobber_thunk,
                std::ptr::null(),
                argv.as_mut_ptr(),
                &mut out,
            )
        };
        assert_eq!(mask, 0, "registers differ after a fatal: {mask:#x}");
        assert_eq!(out, (7 << 32) | 9);
        assert!(detached(&ctx));
        let mask = unsafe {
            enter_with_patterns(
                &mut *ctx,
                add_one,
                std::ptr::null(),
                argv.as_mut_ptr(),
                &mut out,
            )
        };
        assert_eq!((mask, out, argv[0]), (0, 0, 1));
    }

    eml_nrt::entries! {
        const SHIMS;

        shim fn eml_t_checked(ctx, x: i64) -> i64 {
            let _ = ctx.stack_limit();
            if x == 1 {
                return Err(Fatal { code: 21, site: 4 });
            }
            if x == 2 {
                panic!("a shim panics");
            }
            Ok(x * 2)
        }
    }

    unsafe extern "C" fn shim_thunk(ctx: *mut Ctx<Fast>, _entry: *const u8, argv: *mut u64) {
        unsafe {
            *argv = eml_t_checked(ctx, *argv as i64) as u64;
        }
    }

    #[test]
    fn a_shim_cuts_to_enter_on_an_error_and_on_a_panic() {
        assert_eq!(SHIMS[0].symbol, "eml_t_checked");
        let mut ctx = ctx();
        let mut argv = [5u64];
        assert_eq!(
            unsafe { enter(&mut *ctx, shim_thunk, std::ptr::null(), argv.as_mut_ptr()) },
            Ok(())
        );
        assert_eq!(argv[0], 10);
        let mut argv = [1u64];
        assert_eq!(
            unsafe { enter(&mut *ctx, shim_thunk, std::ptr::null(), argv.as_mut_ptr()) },
            Err(Fatal { code: 21, site: 4 })
        );
        let mut argv = [2u64];
        assert_eq!(
            unsafe { enter(&mut *ctx, shim_thunk, std::ptr::null(), argv.as_mut_ptr()) },
            Err(Fatal {
                code: FATAL_PANICKED,
                site: 0
            })
        );
        assert!(detached(&ctx));
        let mut argv = [5u64];
        assert_eq!(
            unsafe { enter(&mut *ctx, shim_thunk, std::ptr::null(), argv.as_mut_ptr()) },
            Ok(())
        );
    }

    eml_nrt::entries! {
        const HEAP_SHIMS;

        shim fn eml_t_heap(ctx, x: i64) -> i64 {
            let _ = ctx.stack_limit();
            if x == 0 {
                return Err(eml_rt::HeapError::StaleToken);
            }
            Ok(x)
        }
    }

    unsafe extern "C" fn heap_thunk(ctx: *mut Ctx<Fast>, _entry: *const u8, argv: *mut u64) {
        unsafe {
            *argv = eml_t_heap(ctx, *argv as i64) as u64;
        }
    }

    #[test]
    fn a_heap_error_is_parked_before_the_cut() {
        assert_eq!(HEAP_SHIMS.len(), 1);
        let mut ctx = ctx();
        let mut argv = [0u64];
        assert_eq!(
            unsafe { enter(&mut *ctx, heap_thunk, std::ptr::null(), argv.as_mut_ptr()) },
            Err(Fatal {
                code: eml_nrt::FATAL_FAULT,
                site: 0
            })
        );
        assert_eq!(
            ctx.take_fault(),
            Some(eml_rt::Fault::Heap(eml_rt::HeapError::StaleToken))
        );
        assert!(detached(&ctx));
    }
}
```

`crates/eml_nrt/tests/regs.rs`
```rust
//! probe: per-ABI asm helpers for the callee-saved register test
#![cfg(not(miri))]
use eml_rt::{Ctx, Fast};

#[cfg(all(target_arch = "aarch64", not(windows)))]
mod imp {
    use super::*;

    /// 呼ばれる側が保存するレジスタをすべて壊してから `eml_fatal(ctx, 7, 9)` へ飛ぶ thunk。
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

    /// 呼ばれる側が保存するレジスタに決まった値を入れて `eml_enter` を呼び、戻った後に違っていたレジスタのビットを返す。
    /// `eml_enter` の結果は `*out` に書く。
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
    /// 自分のフレームポインタのレジスタを4つ目の引数にして `walk_frames` へ飛ぶ thunk。
    #[unsafe(naked)]
    pub unsafe extern "C" fn fp_thunk(ctx: *mut Ctx<Fast>, entry: *const u8, argv: *mut u64) {
        core::arch::naked_asm!("mov x3, x29", "b {walk}", walk = sym crate::stub::walk_frames)
    }

    /// 今のフレームポインタのレジスタを `*out` に書いてから `eml_enter` へ飛ぶ。
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
    /// 自分のフレームポインタのレジスタを4つ目の引数にして `walk_frames` へ飛ぶ thunk。
    #[unsafe(naked)]
    pub unsafe extern "C" fn fp_thunk(ctx: *mut Ctx<Fast>, entry: *const u8, argv: *mut u64) {
        core::arch::naked_asm!("mov rcx, rbp", "jmp {walk}", walk = sym crate::stub::walk_frames)
    }

    /// 今のフレームポインタのレジスタを `*out` に書いてから `eml_enter` へ飛ぶ。
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

    /// 5つ目の引数 `out` は、入ったときの `[rsp + 40]` にある。
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
    /// 自分のフレームポインタのレジスタを4つ目の引数にして `walk_frames` へ飛ぶ thunk。
    #[unsafe(naked)]
    pub unsafe extern "C" fn fp_thunk(ctx: *mut Ctx<Fast>, entry: *const u8, argv: *mut u64) {
        core::arch::naked_asm!("mov r9, rbp", "jmp {walk}", walk = sym crate::stub::walk_frames)
    }

    /// 今のフレームポインタのレジスタを `*out` に書いてから `eml_enter` へ飛ぶ。
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
```

`crates/eml_nrt/src/trust.rs`: copy `crates/eml_native/src/trust.rs` from `fn is_ident` (L21) to the end unchanged, with this head:
```rust
//! `unsafe` の一覧のテスト (docs/implementation/testing.md の「`eml_nrt` の検証」)。`unsafe` を書いてよいファイルをここで
//! 固定する。走査は `eml_native` の `trust.rs` と同じである。

/// 調べるファイル。`src/` のこの module 以外をすべて載せる (`every_source_file_is_listed` が確かめる)。
const FILES: &[(&str, &str)] = &[
    ("lib.rs", include_str!("lib.rs")),
    ("enter.rs", include_str!("enter.rs")),
    ("entries.rs", include_str!("entries.rs")),
    ("arch/mod.rs", include_str!("arch/mod.rs")),
    ("arch/aarch64.rs", include_str!("arch/aarch64.rs")),
    ("arch/x86_64_sysv.rs", include_str!("arch/x86_64_sysv.rs")),
    ("arch/x86_64_win64.rs", include_str!("arch/x86_64_win64.rs")),
    ("arch/unsupported.rs", include_str!("arch/unsupported.rs")),
];

/// 調べないファイル。この module だけである。
const SKIPPED: &[&str] = &["trust.rs"];

/// `unsafe` を持ち、先頭に `#![allow(unsafe_code)]` を置くファイル。
const UNSAFE_FILES: &[&str] = &[
    "enter.rs",
    "entries.rs",
    "arch/aarch64.rs",
    "arch/x86_64_sysv.rs",
    "arch/x86_64_win64.rs",
    "arch/unsupported.rs",
];
```

### A.3 `crates/eml_native`

`crates/eml_native/src/lib.rs` module lines become: `pub mod abi; mod bridge; mod error; mod library; mod linker; mod mem; pub mod thunk; #[cfg(test)] mod trust;`. `crates/eml_native/src/trust.rs` `FILES` gains `("abi.rs", include_str!("abi.rs"))`, `("mem.rs", include_str!("mem.rs"))`, `("thunk.rs", include_str!("thunk.rs"))`; `UNSAFE_FILES` is unchanged. `crates/eml_native/tests/main.rs` gains `mod abi;`.

`crates/eml_native/src/mem.rs`
```rust
//! probe: the one module that names eml_rt's layout
use eml_lir::Shape;
use eml_rt::layout;

const _: () = assert!(Shape::REUSE_SCANNED_LIMIT as usize <= layout::INLINE_SCAN_LIMIT);
```

`crates/eml_native/src/abi.rs`
```rust
//! probe: signatures
use cranelift_codegen::ir::{AbiParam, Signature, types};
use cranelift_codegen::isa::{CallConv, OwnedTargetIsa};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_module::{FuncId, Linkage, Module, ModuleError};
use eml_nrt::{Entry, Kind};

/// Cranelift の module の誤り。大きいので箱に入れて返す。
pub type Built<T> = Result<T, Box<ModuleError>>;

/// eml のコードを出す ISA。`pic` は AOT だけが立てる。
pub fn isa(pic: bool) -> Option<OwnedTargetIsa> {
    let mut flags = settings::builder();
    flags.set("opt_level", "speed").ok()?;
    flags
        .set("is_pic", if pic { "true" } else { "false" })
        .ok()?;
    flags.set("preserve_frame_pointers", "true").ok()?;
    flags.set("enable_probestack", "true").ok()?;
    flags.set("probestack_strategy", "inline").ok()?;
    cranelift_native::builder()
        .ok()?
        .finish(settings::Flags::new(flags))
        .ok()
}

pub fn clif(kind: Kind) -> types::Type {
    match kind {
        Kind::I64 => types::I64,
        Kind::F64 => types::F64,
    }
}

/// eml の関数のシグネチャ: `tail (ctx, 葉...) -> 葉...`。
pub fn tail_signature(params: &[Kind], rets: &[Kind]) -> Signature {
    let mut sig = Signature::new(CallConv::Tail);
    sig.params.push(AbiParam::new(types::I64));
    sig.params
        .extend(params.iter().map(|&k| AbiParam::new(clif(k))));
    sig.returns
        .extend(rets.iter().map(|&k| AbiParam::new(clif(k))));
    sig
}

/// エントリのシグネチャ。記述子だけから作る。
pub fn entry_signature(entry: &Entry, call_conv: CallConv) -> Signature {
    let mut sig = Signature::new(call_conv);
    sig.params.push(AbiParam::new(types::I64));
    sig.params
        .extend(entry.params.iter().map(|&k| AbiParam::new(clif(k))));
    sig.returns
        .extend(entry.result.map(|k| AbiParam::new(clif(k))));
    sig
}

/// エントリを import として宣言する。
pub fn declare_entry<M: Module>(module: &mut M, entry: &Entry) -> Built<FuncId> {
    let sig = entry_signature(entry, module.isa().default_call_conv());
    Ok(module.declare_function(entry.symbol, Linkage::Import, &sig)?)
}
```

`crates/eml_native/src/thunk.rs`
```rust
//! probe: default-cc thunk
use cranelift_codegen::ir::{AbiParam, InstBuilder, MemFlagsData, Signature, types};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{FuncId, Linkage, Module};
use eml_nrt::Kind;

use crate::abi::{Built, clif, tail_signature};

/// `thunk(ctx, entry, argv)`: 引数を `argv` から読み、`tail` の `entry` を呼び、結果を `argv` に書く。
pub fn define_thunk<M: Module>(
    module: &mut M,
    name: &str,
    params: &[Kind],
    rets: &[Kind],
) -> Built<FuncId> {
    let mut sig = Signature::new(module.isa().default_call_conv());
    for _ in 0..3 {
        sig.params.push(AbiParam::new(types::I64));
    }
    let id = module.declare_function(name, Linkage::Local, &sig)?;
    let mut ctx = module.make_context();
    ctx.func.signature = sig;
    let mut fctx = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
    let block = b.create_block();
    b.append_block_params_for_function_params(block);
    b.switch_to_block(block);
    let (c, entry, argv) = (
        b.block_params(block)[0],
        b.block_params(block)[1],
        b.block_params(block)[2],
    );
    let flags = MemFlagsData::trusted();
    let mut args = vec![c];
    for (i, &kind) in params.iter().enumerate() {
        args.push(b.ins().load(clif(kind), flags, argv, 8 * i as i32));
    }
    let callee = b.import_signature(tail_signature(params, rets));
    let call = b.ins().call_indirect(callee, entry, &args);
    let results = b.inst_results(call).to_vec();
    for (i, value) in results.into_iter().enumerate() {
        b.ins().store(flags, value, argv, 8 * i as i32);
    }
    b.ins().return_(&[]);
    b.seal_all_blocks();
    b.finalize(module.target_config());
    module.define_function(id, &mut ctx)?;
    module.clear_context(&mut ctx);
    Ok(id)
}
```

`crates/eml_native/tests/abi.rs`
```rust
//! probe: ABI tests
#![cfg(not(miri))]
use std::sync::Arc;

use cranelift_codegen::ir::{InstBuilder, types};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module, default_libcall_names};
use eml_native::abi::{declare_entry, isa, tail_signature};
use eml_native::thunk::define_thunk;
use eml_nrt::Kind::{self, F64, I64};
use eml_nrt::{Entry, Fatal, Thunk, detached, enter};
use eml_rt::{Buffering, Control, Ctx, Fast, HostRef, Limits, StdHost};

eml_nrt::entries! {
    const ECHO;

    plain fn eml_t_echo_ii(ctx, a: i64, b: i64) {
        let out = unsafe { (*ctx).runtime() }.cast::<u64>();
        unsafe {
            *out = a as u64;
            *out.add(1) = b as u64;
        }
    }

    plain fn eml_t_echo6(ctx, a: i64, b: f64, c: i64, d: f64, e: i64, f: f64) -> f64 {
        let out = unsafe { (*ctx).runtime() }.cast::<u64>();
        for (i, bits) in [a as u64, b.to_bits(), c as u64, d.to_bits(), e as u64, f.to_bits()]
            .into_iter()
            .enumerate()
        {
            unsafe { *out.add(i) = bits };
        }
        -6.5
    }

    plain fn eml_t_echo9(
        ctx, a: f64, b: i64, c: f64, d: i64, e: f64, f: i64, g: f64, h: i64, i: f64,
    ) -> i64 {
        let out = unsafe { (*ctx).runtime() }.cast::<u64>();
        for (k, bits) in [
            a.to_bits(), b as u64, c.to_bits(), d as u64, e.to_bits(), f as u64, g.to_bits(),
            h as u64, i.to_bits(),
        ]
        .into_iter()
        .enumerate()
        {
            unsafe { *out.add(k) = bits };
        }
        -9
    }

    plain fn eml_t_echo18(ctx, a: i64, b: f64, c: i64, d: f64, e: i64, f: f64, g: i64, h: f64, i: i64, j: f64, k: i64, l: f64, m: i64, n: f64, o: i64, p: f64, q: i64, r: f64) -> f64 {
        let out = unsafe { (*ctx).runtime() }.cast::<u64>();
        for (k, bits) in [a as u64, b.to_bits(), c as u64, d.to_bits(), e as u64, f.to_bits(), g as u64, h.to_bits(), i as u64, j.to_bits(), k as u64, l.to_bits(), m as u64, n.to_bits(), o as u64, p.to_bits(), q as u64, r.to_bits()].into_iter().enumerate() {
            unsafe { *out.add(k) = bits };
        }
        -18.5
    }
}

fn ctx() -> Box<Ctx<Fast>> {
    let host = StdHost::new(
        Box::new(std::io::sink()),
        Box::new(std::io::sink()),
        std::path::PathBuf::new(),
        Buffering::Block,
    );
    let control = Arc::new(Control::new(Limits::default()));
    Box::new(Ctx::new(control, HostRef::new(Arc::new(host))))
}

fn module() -> JITModule {
    let mut builder = JITBuilder::with_isa(isa(false).unwrap(), default_libcall_names());
    for entry in eml_nrt::entries().chain(ECHO) {
        builder.symbol(entry.symbol, (entry.address)());
    }
    JITModule::new(builder)
}

/// `name(ctx, 葉...)` を宣言して `body` で本体を作る。
fn define(
    m: &mut JITModule,
    name: &str,
    params: &[Kind],
    rets: &[Kind],
    body: impl FnOnce(&mut JITModule, &mut FunctionBuilder, FuncId),
) -> FuncId {
    let sig = tail_signature(params, rets);
    let id = m.declare_function(name, Linkage::Local, &sig).unwrap();
    let mut ctx = m.make_context();
    ctx.func.signature = sig;
    let mut fctx = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
    let block = b.create_block();
    b.append_block_params_for_function_params(block);
    b.switch_to_block(block);
    body(m, &mut b, id);
    b.seal_all_blocks();
    b.finalize(m.target_config());
    m.define_function(id, &mut ctx).unwrap();
    m.clear_context(&mut ctx);
    id
}

fn thunk(m: &JITModule, id: FuncId) -> Thunk {
    unsafe { std::mem::transmute::<*const u8, Thunk>(m.get_finalized_function(id)) }
}

fn params(b: &FunctionBuilder) -> Vec<cranelift_codegen::ir::Value> {
    let block = b.current_block().unwrap();
    b.block_params(block).to_vec()
}

/// 記述子から作った呼び出しが、macro で宣言した Rust のエントリに引数を1つずつ渡し、結果を受け取る。
#[test]
fn mixed_int_and_float_arguments_reach_an_entry() {
    for entry in eml_nrt::entries() {
        assert!(
            ECHO.iter()
                .any(|echo| echo.params == entry.params && echo.result == entry.result),
            "no echo entry has the kinds of {}",
            entry.symbol
        );
    }
    let mut m = module();
    let mut callers = Vec::new();
    for entry in ECHO {
        let rets: Vec<Kind> = entry.result.into_iter().collect();
        let callee = declare_entry(&mut m, entry).unwrap();
        let name = format!("call${}", entry.symbol);
        let id = define(&mut m, &name, entry.params, &rets, |m, b, _| {
            let args = params(b);
            let callee = m.declare_func_in_func(callee, b.func);
            let call = b.ins().call(callee, &args);
            let results = b.inst_results(call).to_vec();
            b.ins().return_(&results);
        });
        let th = define_thunk(
            &mut m,
            &format!("thunk${}", entry.symbol),
            entry.params,
            &rets,
        )
        .unwrap();
        callers.push((entry, id, th));
    }
    m.finalize_definitions().unwrap();
    let mut ctx = ctx();
    for (entry, id, th) in callers {
        let n = entry.params.len();
        let mut argv: Vec<u64> = entry
            .params
            .iter()
            .enumerate()
            .map(|(i, kind)| match kind {
                I64 => 0x0101_0000_0000_0000u64.wrapping_mul(i as u64 + 1) + i as u64,
                F64 => (i as f64 + 0.25).to_bits(),
            })
            .collect();
        let sent = argv.clone();
        let mut seen = vec![0u64; n];
        ctx.set_runtime(seen.as_mut_ptr().cast());
        let r = unsafe {
            enter(
                &mut *ctx,
                thunk(&m, th),
                m.get_finalized_function(id),
                argv.as_mut_ptr(),
            )
        };
        ctx.set_runtime(std::ptr::null_mut());
        assert_eq!(r, Ok(()), "{}", entry.symbol);
        assert_eq!(seen, sent, "{}", entry.symbol);
        match entry.symbol {
            "eml_t_echo6" => assert_eq!(f64::from_bits(argv[0]), -6.5),
            "eml_t_echo9" => assert_eq!(argv[0] as i64, -9),
            "eml_t_echo18" => assert_eq!(f64::from_bits(argv[0]), -18.5),
            _ => {}
        }
        assert!(detached(&ctx));
    }
}

/// 自分への `return_call` はスタックを伸ばさず、スタック渡しの引数が多い関数への `return_call` と、複数の値の戻りが通る。
#[test]
fn tail_calls_and_multiple_results_run() {
    let mut m = module();
    let wide = define(&mut m, "wide", &[I64; 11], &[I64, F64, I64], |_, b, _| {
        let p = params(b);
        let mut sum = p[1];
        for x in &p[2..] {
            sum = b.ins().iadd(sum, *x);
        }
        let half = b.ins().f64const(0.5);
        let one = b.ins().iconst(types::I64, 1);
        b.ins().return_(&[sum, half, one]);
    });
    let count = define(
        &mut m,
        "count",
        &[I64, I64],
        &[I64, F64, I64],
        |m, b, me| {
            let p = params(b);
            let (c, n, acc) = (p[0], p[1], p[2]);
            let done = b.create_block();
            let more = b.create_block();
            b.ins().brif(n, more, &[], done, &[]);
            b.switch_to_block(more);
            let n1 = b.ins().iadd_imm_s(n, -1);
            let a1 = b.ins().iadd(acc, n);
            let me = m.declare_func_in_func(me, b.func);
            b.ins().return_call(me, &[c, n1, a1]);
            b.switch_to_block(done);
            let wide = m.declare_func_in_func(wide, b.func);
            let mut args = vec![c, acc];
            for i in 0..10 {
                args.push(b.ins().iconst(types::I64, i));
            }
            b.ins().return_call(wide, &args);
        },
    );
    let th = define_thunk(&mut m, "thunk$count", &[I64, I64], &[I64, F64, I64]).unwrap();
    m.finalize_definitions().unwrap();
    let mut ctx = ctx();
    let n = 10_000_000u64;
    let mut argv = [n, 0, 0];
    let r = unsafe {
        enter(
            &mut *ctx,
            thunk(&m, th),
            m.get_finalized_function(count),
            argv.as_mut_ptr(),
        )
    };
    assert_eq!(r, Ok(()));
    assert_eq!(argv[0], n * (n + 1) / 2 + 45);
    assert_eq!(f64::from_bits(argv[1]), 0.5);
    assert_eq!(argv[2], 1);
}

mod fatal {
    use super::*;

    /// `tail` のフレームを n 枚積んでから `eml_fatal` を呼ぶと、`eml_enter` から誤りが返る。
    #[test]
    fn a_fatal_cuts_through_tail_frames() {
        let mut m = module();
        let fatal_entry: &Entry = eml_nrt::entries()
            .find(|e| e.symbol == "eml_fatal")
            .unwrap();
        let fatal = declare_entry(&mut m, fatal_entry).unwrap();
        let deep = define(&mut m, "deep", &[I64], &[I64], |m, b, me| {
            let p = params(b);
            let (c, n) = (p[0], p[1]);
            let zero = b.create_block();
            let more = b.create_block();
            b.ins().brif(n, more, &[], zero, &[]);
            b.switch_to_block(zero);
            let fatal = m.declare_func_in_func(fatal, b.func);
            let code = b.ins().iconst(types::I64, 7);
            let site = b.ins().iconst(types::I64, 9);
            b.ins().call(fatal, &[c, code, site]);
            b.ins()
                .trap(cranelift_codegen::ir::TrapCode::unwrap_user(1));
            b.switch_to_block(more);
            let n1 = b.ins().iadd_imm_s(n, -1);
            let me = m.declare_func_in_func(me, b.func);
            let call = b.ins().call(me, &[c, n1]);
            let r = b.inst_results(call)[0];
            let r1 = b.ins().iadd_imm_s(r, 1);
            b.ins().return_(&[r1]);
        });
        let th = define_thunk(&mut m, "thunk$deep", &[I64], &[I64]).unwrap();
        m.finalize_definitions().unwrap();
        let mut ctx = ctx();
        for depth in [0u64, 1, 100] {
            let mut argv = [depth];
            let r = unsafe {
                enter(
                    &mut *ctx,
                    thunk(&m, th),
                    m.get_finalized_function(deep),
                    argv.as_mut_ptr(),
                )
            };
            assert_eq!(r, Err(Fatal { code: 7, site: 9 }));
            assert!(detached(&ctx));
        }
    }
}
```

### A.4 Bench scripts

`bench/rt-gate.sh`
```sh
#!/usr/bin/env bash
# eml_rt を変えたコミットの門 (docs/implementation/benchmarks.md の「命令の数の測り方」)。1つの作業ツリーで BASE と今の
# HEAD を交互に checkout して bench/rt.sh を流し、行ごとに命令の数の差 (%) の中央値と、門の行の命令の数の差の和を出す。
# 揺れる行 (empty、println-*、read-*、stdout-*) は見ない。ほかの行の中央値がどれも +0.1 % 以下なら 0 で終わる。
# `--allow '行 行…' --allow-bound 上限` を付けると、挙げた行は上限 (%) まで許し、その代わり和が負であることを求める。
# 使い方: bench/rt-gate.sh [--allow '行 行…' --allow-bound 上限] BASE [組の数 (既定 5)]
set -euo pipefail

main() {
  local allow="" bound="0.1"
  while [[ "${1:-}" == --* ]]; do
    case "$1" in
      --allow) allow="$2" ;;
      --allow-bound) bound="$2" ;;
      *) echo "bench/rt-gate.sh: 知らない引数 $1" >&2; exit 2 ;;
    esac
    shift 2
  done
  local root base pairs new back out i
  root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  base="$(git -C "$root" rev-parse --verify "$1^{commit}")"
  pairs="${2:-5}"
  if [[ -n "$(git -C "$root" status --porcelain)" ]]; then
    echo "bench/rt-gate.sh: 作業ツリーに変更がある" >&2
    exit 1
  fi
  new="$(git -C "$root" rev-parse HEAD)"
  back="$(git -C "$root" symbolic-ref -q --short HEAD || echo "$new")"
  out="$(mktemp -d)"
  # 終わりには main の局所変数がもうないので、ここで値を埋めておく
  # shellcheck disable=SC2064
  trap "git -C '$root' checkout -q '$back'" EXIT
  for i in $(seq "$pairs"); do
    git -C "$root" checkout -q "$base"
    "$root/bench/rt.sh" >"$out/base.$i"
    git -C "$root" checkout -q "$new"
    "$root/bench/rt.sh" >"$out/new.$i"
  done
  compare "$out" "$pairs" "$allow" "$bound"
}

# compare <表を置いたディレクトリ> <組の数> <許す行> <許す行の上限>
compare() {
  awk -v pairs="$2" -v allow="$3" -v bound="$4" '
    function row(line,    f) {
      split(line, f, "|")
      gsub(/[` ]/, "", f[2]); gsub(/ /, "", f[3])
      name = f[2]; count = f[3]
      return name != "" && count ~ /^[0-9]+$/
    }
    function median(a, n,    i, j, t) {
      for (i = 1; i <= n; i++) for (j = i + 1; j <= n; j++) if (a[j] < a[i]) { t = a[i]; a[i] = a[j]; a[j] = t }
      return a[int((n + 1) / 2)]
    }
    BEGIN { split(allow, names, " "); for (k in names) allowed[names[k]] = 1 }
    FNR == 1 { split(FILENAME, parts, "/"); split(parts[length(parts)], tag, "."); side = tag[1]; pair = tag[2] }
    row($0) { if (!((side, pair, name) in seen)) { seen[side, pair, name] = 1; v[side, pair, name] = count; if (side == "base" && pair == 1) order[++rows] = name } }
    END {
      bad = 0; sum = 0
      print "| 行 | 差の中央値 (%) | 差の中央値 (命令) | 門 |"
      print "|---|---:|---:|---|"
      for (r = 1; r <= rows; r++) {
        name = order[r]; n = 0
        for (p = 1; p <= pairs; p++)
          if ((("new", p, name) in v)) {
            n++
            abs[n] = v["new", p, name] - v["base", p, name]
            pct[n] = 100 * abs[n] / v["base", p, name]
          }
        if (n == 0) continue
        m = median(pct, n); ma = median(abs, n)
        gated = !(name == "empty" || name ~ /^println-/ || name ~ /^read-/ || name ~ /^stdout-/)
        limit = (name in allowed) ? bound : 0.1
        verdict = gated ? (m <= limit ? ((name in allowed) && m > 0.1 ? "許した" : "ok") : "超えた") : "見ない"
        if (gated) { sum += ma; if (m > limit) bad = 1 }
        printf "| `%s` | %+.3f | %+d | %s |\n", name, m, ma, verdict
      }
      printf "\n門の行の差の和: %+d 命令\n", sum
      if (allow != "" && sum >= 0) { print "和が負でない"; bad = 1 }
      exit bad
    }' "$1"/base.* "$1"/new.*
}

main "$@"
exit
```

`bench/rt.sh` edits (task N1): the build line becomes
```sh
cargo build --release -p eml_rt -p eml_nrt --example rt_bench --example rt_extern_bench \
  --example nrt_bench \
  --manifest-path "$root/Cargo.toml" >&2
```
and before the line `echo "eml / 参照の比 ...` these lines are added:
```sh
echo "ネイティブのランタイムの計測 (nrt_bench)"
echo
measure "$examples/nrt_bench" enter entry-plain entry-shim
echo
```

