# Appendix 01: native-runtime contract (long-term guidelines)

Source: the contract designer's revision r3, with the wording fixes of 00-decisions.md applied (N-9/1.2 data-word services, N-17 scope, N-21 `Never`, N-37 bridge exemption). Where 00-decisions.md and this file differ, 00-decisions.md wins.

# S1 response (r3): the native-runtime contract for the jit stages

This is the contract in full (Part 1 only). It replaces r2. The rulings behind the changes are in `X-ruling-r1.md` in the same directory.

Tags: [V] = verified here (method given or in section 7), [I] = inferred, [R] = taken from the research files R1-R4 without re-checking, [D: <design> <section>] = taken from a downstream design's own verified check, not re-run here. Names are fixed where downstream designs must agree (crate names, stage names, symbol prefixes, `Ctx` field names); other names are illustrative.

## Changelog r2 -> r3

User premises folded in:
- `Ctx` layout (the user first fixed `heap` as the first field, then withdrew that and asked for the layout that is best for native code): **`Native` is the first field and `heap` the second**, as in r2, now with the measurements and the reasoning (1.2(c), N-4). The eml_rt bench gate stays rt-layout's with one exception for the layout task (N-29); **the user confirms that exception with the plan.**
- `sched-split` is listed in roadmap 「段の列」 (5.5(a)).
- Native performance first, through clean design; measurement from the first native stage; profiler readiness and a roadmap item after native-aot (new 1.7; N-34 to N-40).
- The jit-stage decisions take precedence over the current text of roadmap.md and native.md (5.5(a)).

Rulings folded in:
- Failure protocol of `eml_x_`: internal-class errors and caught panics end in the shim's fatal path (1.2, 1.3, 1.4, N-9). **This rewords an item the contract calls fixed; the user confirms it with the plan.**
- Engine-held payloads (`panic` message, re-raise requests) are marked by `Fault::Engine`, so the fault word is non-zero iff a failure is parked (1.2(c), 1.4, N-9).
- `Native` keeps six fields; the task-limit state lives in eml_nrt's worker struct (1.2(c), 5.3, N-5, N-17).
- Stack geometry `hard` / `real` (1.3, N-42).
- `eml_enter` returns `u64` (0 or code and site); `eml_fatal(ctx, code: i64, site: i64)` records nothing (1.3, N-12).
- 45 inline rows, `FloatMod` called; every `Rhs::Prim` rejected in jit-core (1.2, 1.4, 5.3).
- Non-async rows are partitioned inline / entry / `Par`; engine-specific rows are `exit` and `panic`; one shared row function per row from `extern_rows!` (N-21, N-22, N-27).
- User C functions are `eml_c$<name>` imports (N-7, N-9).
- Function values: header-as-descriptor objects and generic PAPs (1.3, 5.3).
- Run-fail files are compared after a checked VM run (1.6, N-25).
- Engine differences are a closed list in spec/native.md 「VMとの違い」, including the nested `Async.run` rule and `BuriedRun` (N-43).
- N-3's check is a unit test; N-29 applies to any eml_rt edit; the fast-mode `Err` list gains `RootMisuse` and `Unrecoverable`.
- sched-split's dev-only `testing` feature is allowed under N-41.

Criteria: N-1 to N-33 keep their numbers (17 amended, none retired); N-34 to N-43 are new.

---

## 1. Conclusion

### 1.1 Crate graph (questions 1 and 2)

Two new crates. Final state:

```
eml_cli ─► eml_vm ─────────────────────────► eml_sched ─► eml_rt, eml_io, eml_extern, eml_cabi, [eml_tls], [eml_gui]
        ─► eml_native ─► eml_nrt ──────────► eml_sched
                      ─► eml_lir, eml_rt (consts, see N-31), eml_extern, eml_cabi, cranelift-*, libloading
```

| Crate | Holds | Must not depend on |
|---|---|---|
| `eml_rt` (verified) | `Ctx` with the native fields and their offset consts; layout / `Meta` / `Word` / `rc` exports. No `extern "C"`, no asm, no `catch_unwind` | anything but vstd |
| `eml_sched` (new; created by stage `sched-split`; the name multicore.md 「層の構成」 reserves) | From sched-split: the task-cell core (`cells.rs`, whole file with its tests), the run-error types (`RuntimeError`, the fault enum, `SourceLocation`), `RunConfig` / `Finished` / `RunStats` / `Clock` / `DriverKind`, `Park`. From jit-core: the row list `extern_rows!` and the shared row functions of the seed rows. From jit-externs: every non-async row function, `tls.rs` and `gui.rs` (whole files), the shared user-C pieces. From jit-tasks: worker table, inboxes, monitor, jobs, the `Exec` trait, the task rows' bodies | Cranelift, eml_lir, eml_vm, eml_nrt, eml_native |
| `eml_nrt` (new; native runtime; `libeml_rt.a` is built from it in native-aot) | every `extern "C"` entry generated code calls; asm stubs, `JmpBuf`, `eml_fatal`; the `Image` descriptor; the native run driver; from jit-tasks the native `Exec` | Cranelift, eml_lir, eml_core_ir, eml_vm, eml_native |
| `eml_native` (code generator + JIT host) | `unsupported` (the E0004 list), LIR -> CLIF lowering, (fn, k) adapters, default-cc thunks, `NativeMachine`; the existing bridges and `Linker` | eml_vm |
| `eml_vm` | unchanged role | Cranelift, eml_native, eml_nrt |

- **Features.** Native itself is not feature-gated (it rides on `eml_cli/run`, `eml_test_support/run`). The existing `tls`, `gui`, `gui-native` features are forwarded, not multiplied: eml_sched gets `tls = ["dep:eml_tls", "eml_io/tls"]`, `gui = ["dep:eml_gui"]`, `gui-native = ["gui", "eml_gui/native"]` (the definitions eml_vm has today [V: `crates/eml_vm/Cargo.toml`]) in the stage that moves the first arm needing them (jit-externs); eml_vm, eml_nrt and eml_native each forward the three names to the crate below; eml_cli's `tls` / `gui` / `gui-native` name `eml_vm/<f>` and `eml_native/<f>`. The feature-combo clippy list of CLAUDE.md keeps its names and gains `-p eml_sched -p eml_nrt` in the "without tls and gui" `--all-targets` row. N-2's `cargo tree` greps run on the default feature set. sched-split itself adds no `tls` / `gui` feature (it deletes the two `cfg_attr(not(feature = "gui"), ..)` attributes of `cells.rs`, whose items become `pub`). One dev-only feature exists for a time: `eml_sched/testing` (fixtures and hooks that eml_vm's unit tests used through `#[cfg(test)]` before the move), under the conditions of N-41.
- eml_native depending on eml_nrt (and through it eml_io) is intended: the JIT host must run what it compiles. "Never mix" is about eml_vm.

### 1.2 Runtime access path (question 1)

**(a) Inline in generated code** (values only from eml_rt consts, rt-layout G-5/G-12):

| Operation | Inline | Out of line |
|---|---|---|
| immediate test, immediate value, `()` compare | `Word` consts | - |
| constructor tag of `obj`/`tobj` | header meta load, `Meta` tag mask | - |
| field / raw word read and write | `layout::slot_byte_offset(layout::data_first(..) + position)` | - |
| `dup` | local fast path (`rc` consts): load, range compare, store | `eml_rt_dup_slow` |
| `decref` | local non-last fast path (rc > 1) | `eml_rt_decref` (rc == 1, shared, immortal, stuck) |
| `unbox` of `int` / `float` | immediate path; box value word from `layout` | - |
| scalar rows: the rows eml_vm's `compile/emit.rs` `arithmetic()` maps to dedicated ops [V: read L1064-1103] that have a CLIF operation: 45 rows; `FloatMod` has none (no float remainder instruction, no `fmod` libcall [D: S2b 9.7]) and is a called row | CLIF ops with overflow / zero checks, cold branch to the fault path | - |
| allocation | none in the jit-* stages (see below); an `int` that fits the immediate is made inline (shift, compare, branch) | `eml_rt_alloc_data`, `eml_rt_alloc_int` (big ints), `eml_rt_alloc_float` |
| `release`, tokens, `con` reuse, `free_block`, closures, strings, `switch` on strings | none | `eml_rt_*` entries |
| yield / abort check | load `ctx.native.yielding.target`, compare with `Word::UNIT` bits | cleanup block |
| safepoint | load stop-word pointer, load word, `get_stack_pointer` vs `ctx.native.stack_limit` | `eml_nrt_safepoint` |

Row lowering is chosen by one table in eml_native: `Inline(expander) | Call(entry symbol)`, default `Call`. native-inline adds byte / array rows there. Allocation is not a row; an inline allocation fast path is a later addition to the memory-access module (N-15) once eml_rt exports the allocator facts under rt-layout P4(b). Nothing here blocks it. Risk recorded: one call per allocation counts against the 1.5x-Python goal and is measured in native-inline (`logstat`).

**(b)(d) Entry points** all live in eml_nrt, declared through one macro that emits the `unsafe extern "C" fn` and its descriptor row `(symbol, address, parameter kinds, result kind)` together. Generated code imports by symbol name; the JIT registers the table with `JITBuilder::symbol`; native-aot exports the same names.

| Prefix | What | Failure protocol |
|---|---|---|
| `eml_rt_<op>` | thin shims over the eml_rt heap / `Ctx` API | **fatal-only**: an `Err(HeapError)` or a caught panic is recorded and the shim calls `eml_fatal`; generated code checks nothing |
| `eml_x_<row>` | one per called `eml_extern::Extern` row (`eml_x_<Variant>`) and per user-C helper (`eml_x_c_*`) | `faults` rows: park the `Fault` in `Ctx`, return a placeholder; generated code tests `ctx.native.fault`. Other rows: plain call. `OpRet::Never` rows (`exit`, `panic`, `fail_io` [V: 7.6]): no hot continuation, see 1.4. Internal-class errors (an `Err` of a non-`faults` row, a non-`Extern` `RunFault`, a caught panic) take the shim's fatal path; the wrapper body never names `eml_fatal` (N-9) |
| `eml_nrt_<service>` | cold-path services (`eml_nrt_failed`, `eml_nrt_safepoint`, later task services) | return an action word that generated code branches on, or a plain data word when the service's only failure path is the fatal path; or do not return |
| `eml_enter`, `eml_fatal`, later `eml_run_pending` | native.md names | - |

- **ABI of every entry**: C default convention; `ctx: *mut Ctx<Fast>` first; every other parameter exactly `i64` or `f64`; at most one result, `i64` or `f64`; no struct by value, no narrower integer, no `bool`. Multi-result operations (`con_at`) get an out-pointer or two entries. No row has a struct Repr; the widest has 5 parameters; 38 rows have a `float` [V: 7.6]. An entry may have no result or be `!`. User C functions are not entries: they are imported as `eml_c$<canonical eml name>` with a signature built from `CSignature` by one helper (N-7).
- **Entries arrive with their consumer.** A stage adds exactly the entries its own lowering or tests call: jit-rt the macro, the table and test-only echo entries; jit-core the heap entries, seed rows and the two services; jit-apply the closure entries; jit-effects the prim entries; and so on. No entry is declared ahead of a caller.
- **Why fatal-only is sound for heap shims.** In fast mode eml_rt operations return `Err` only as follows [V: `crates/eml_rt/src/error.rs` L1-3, object-model.md L398-401]: `WrongKind` from the operations that compare kind or type in both modes (`foreign*`, and the payload readers used for C arguments), `StaleToken` from `reuse_in_place` on an empty token, `RootMisuse` from `root_get` / `root_set`, and `Unrecoverable` from `recover`. Generated code tests the token for empty before the call; `RootMisuse` and `WrongKind` are compiler or row-implementation bugs (internal class); `Ctx::recover` is called by the driver, never by generated code. No soft limit or recoverable condition reaches a shim as `Err` (the memory limits are stop bits and the allocator counters). spec/native.md 「エントリ」 carries this list.
- **Shim body rule.** `match catch_unwind(|| op) { Ok(Ok(v)) => v, Ok(Err(e)) => fatal(ctx, e), Err(p) => { drop(p); fatal(ctx, Internal) } }`. `eml_fatal` is called after the closure has returned, never inside it; a panic wins over any `Err` because the closure did not return one. Works on aarch64 macOS for both arms [V: 7.9].

**(c) `Ctx`.** Native code instantiates `Ctx<Fast>` only (decision #3). `Ctx<M>` becomes `#[repr(C)]` with one non-generic `#[repr(C)]` first field; `heap` is the field after it:

```rust
#[repr(C)]
pub struct Native {
    yielding: YieldState,   // { target, clause, cont: Word, abort: bool }  moved here, not mirrored
    fault: u64,             // 0 = none; non-zero = a Fault is parked in Ctx.fault_value
    stop: StopPtr,          // *const AtomicU32
    stack_limit: usize,     // compared with SP; usize::MAX is the Pending poison (jit-tasks)
    jmp: JmpPtr,            // innermost JmpBuf; the JmpBuf type belongs to eml_nrt
    runtime: RtPtr,         // opaque per-worker pointer owned by eml_nrt
}
#[repr(C)]
pub struct Ctx<M: Mode> { native: Native, heap: Heap<M>, evv: Word, /* ... */ fault_value: Option<Fault> }

pub const CTX_YIELD_TARGET: usize = offset_of!(Ctx<Fast>, native) + offset_of!(Native, yielding) + offset_of!(YieldState, target);
// likewise CTX_FAULT, CTX_STOP, CTX_STACK_LIMIT, CTX_JMP; CTX_YIELD_ABORT arrives with its first reader (jit-effects)
```

- **Why `Native` first** (the user left the position of `heap` open and asked for the layout that is best for native code). Measured on the shared runtime, which native code calls as much as the VM does (`bench/rt.sh`, instructions retired, base / `Native` first / `heap` first [D: S2a r1 C-1, medians of 3; one more full pair agrees, S2a r1 7.4]):

  | row | `Native` first | `heap` first |
  |---|---:|---:|
  | `yield` | -10.6 % (-39.1 M) | -2.5 % |
  | `map-reuse` | -4.2 % (-29.8 M) | -0.2 % |
  | `show-int` | +0.45 % (+9.3 M) | -0.04 % |
  | `slice-short` | +0.33 % (+9.1 M) | -0.03 % |
  | `show-int-long` | +0.17 % (+8.7 M) | -0.03 % |
  | `handle` | +0.11 % (+0.7 M) | -0.01 % |
  | `list` (allocation) | +0.03 % | +0.01 % |
  | `dup`, `perform` | within 0.05 % | within 0.05 % |

  - In generated code the fields are read after every non-tail call (`yielding.target`), at every function entry (stop pointer, `stack_limit`) and after every `faults` row. At offsets 0..64 each read has the shortest encoding on every target: an 8-bit displacement on x86-64 (3 to 4 bytes less per read than behind the 1232-byte `Heap`; same uops) and one scaled `ldr` on aarch64, where a pair of words is also in `ldp` / `stp` reach. Behind the heap the aarch64 read is still one `ldr`, so generated code on aarch64 gains nothing from either order; x86-64 gains code density from `Native` first.
  - In the runtime, `Native` first makes the yield operations shorter (the yield words are in pair reach; this is the `yield` row, and resumable effects are a hot path of native code too) and costs one `add` to form `&mut ctx.heap` in an out-of-line `Heap` call made from a `Ctx` method. That `add` is +0.03 % on the allocation row, the runtime call native code makes most, and at most +0.45 % on rows with several `Heap` calls per extern.
  - Not examined: why `map-reuse` gains 4.2 % (it reproduced in both measurements; the mechanism is inferred to be the same pair reach on `Ctx` words) [I]. The 5-pair run of the layout task is the check (A7).
  - Net: the gains are an order of magnitude larger than the losses, and the losses fall on the paths native-inline removes first (extern rows worth inlining). `heap` first would be the choice only if the per-row +0.1 % gate were the goal; the goal is native speed, so the gate gets the one exception of N-29. No trick is involved: it is a field order.
  - `size_of::<Native>()` is 72, so `heap` is at offset 72 and `CTX_YIELD_TARGET` = 0 [D: S2a r1 C-1].
- `Native` has these six fields and nothing is reserved. A field is added only with its first reader in generated code or asm (N-5); the task-limit state (`limit_class`, the real limit, the worker id) has no such reader and lives in eml_nrt's worker struct.
- `Fault` gains one variant, `Fault::Engine`: "the engine holds the payload of this failure" (a panic message word, a re-raise request; 1.4). It arrives in jit-core with its first producer (`panic`); the VM never produces it and maps it to an internal error.
- Verified: with `Native` embedded, `#[repr(C)]` on `Ctx<M>` and the ~40 `.yielding` spellings rewritten, `cargo verus verify -p eml_rt` gives 0 errors with no proof body edited for the `Native` change [V: 7.1, `Native` first, nine fields; D: S2a r1 7.1, `Native` first, six fields]. The box-value export costs one proof attribute, `#[verifier::rlimit(20)]` on `heap/share.rs` `lemma_moves_parent` [D: S2a 7.3]. Adding a field later costs the same kind of run.
- Raw-pointer fields make `Ctx` `!Send` and eml_vm stops compiling [V: 7.1]. The three pointer fields are `#[repr(transparent)]` wrappers with `unsafe impl Send`, covered by one new trust row **T18**, which states for each pointer its pointee's owner, lifetime and the state at a thread crossing:

  | Field | Pointee owner | Valid while | Written by | When `Ctx` crosses threads |
  |---|---|---|---|---|
  | `stop` | the `Arc<AtomicU32>` in `Ctx.word` (single owner; `stop == Arc::as_ptr(&word)`) | the `Ctx` lives | `for_worker` only; `word` is private and never reassigned, and no setter for `stop` exists | unchanged; an `AtomicU32` is `Sync` |
  | `jmp` | the `eml_enter` frame that linked it | between that frame's link and unlink | eml_nrt's asm stubs only (`set_jmp` is not offered to anyone else) | null (no enter is active) |
  | `runtime` | eml_nrt's per-worker struct | from worker start to worker end | eml_nrt at worker start / end | null (cleared before the `Ctx` is handed back) |

  eml_nrt asserts the last column where it sends or returns a `Ctx`.
- eml_rt offers safe accessors for what it does not interpret (`set_stack_limit`, `runtime` / `set_runtime`, `set_fault(Fault)`, `take_fault() -> Option<Fault>`); `fault` and `fault_value` change only together.
- Generated code reads four fields: `yielding.target` (and `abort` from jit-effects), `fault`, `stop`, `stack_limit`. eml_nrt reads the rest.
- `runtime` is the one field beyond native.md's list; it replaces thread-locals (decision #5). In jit-core it points at the worker struct that holds the task's `exit: Option<i64>` (the VM's `Task.exit`). It is also where every per-worker value without a generated-code reader hangs (N-5).

### 1.3 Entry, fatal, and where things live (question 2)

- **Stubs.** `eml_enter`, the longjmp half of `eml_fatal`, and `JmpBuf` are in `eml_nrt/src/arch/{aarch64,x86_64_sysv,x86_64_win64}.rs` as `#[unsafe(naked)]` functions with `naked_asm!` (stable on the pinned 1.99.1; runs [V: 7.2]). No external assembler, no build script. Each stub sets up a standard frame record before it calls the thunk (N-35).
- **Shape.**
  ```rust
  // 0 = returned (result words in *argv); otherwise code << 32 | site, code != 0
  unsafe extern "C" fn eml_enter(ctx: *mut Ctx<Fast>, thunk: *const u8, entry: *const u8, argv: *mut u64) -> u64;
  unsafe extern "C" fn eml_fatal(ctx: *mut Ctx<Fast>, code: i64, site: i64) -> !;   // records nothing; asm longjmp
  // Rust wrapper: eml_nrt::enter(ctx, thunk, entry, argv) -> Result<(), Fatal { code: u32, site: u32 }>
  ```
  The stub saves native.md's callee-saved set into a `JmpBuf` in its own frame, links it, and calls `thunk(ctx, entry, argv)` with the C convention. `thunk` is a Cranelift-made **default-cc** function per arity that loads the arguments and `call_indirect`s the `tail`-cc adapter `enter$k`. The stub never calls `tail` code directly: Cranelift's `tail` convention assigns other argument registers and clobbers all registers [V: 7.3].
- **Fatal rule.** A longjmp crosses only `tail` frames, default-cc thunks, and the Rust frames of the one `eml_rt_*` / `eml_nrt_*` / `eml_x_*` entry that detected the error plus `eml_fatal`; those Rust frames hold no value with a destructor at the call. `eml_fatal` records nothing: the detector moves the error into the worker struct first (the codes and where each one's payload lives are listed in spec/native.md 「`eml_fatal`」), so re-raising across a nested enter is `eml_fatal` with the same code and site. In a shim the call comes from the macro after the `catch_unwind` closure has returned; a wrapper body never names `eml_fatal`, and only internal-class errors of an `eml_x_*` entry reach it (N-9).
- **Re-entry rule.** A Rust entry that re-enters eml code passes the raw `ctx` and holds no `&`/`&mut Ctx` across `eml_enter`.
- **CLIF lowering**: eml_native, generic over `cranelift_module::Module`; only the JIT host file names `JITModule`. Every external reference is a module symbol or module data; no process address is an immediate (decision #1's "same CLIF").
- **Constants**: read from a module data table that eml_nrt fills before first entry (a frozen region, as the VM builds) [I: native.md 「AOT と配布」 default].
- **`Image`**: a `#[repr(C)]` descriptor defined in eml_nrt, built by eml_native: thunks, function table, names, fault-site table `(function, Loc)`, constant initialisers, effect numbers of `Prelude.Panic` / `Prelude.IO`; later stages append what they need (runtime entries in jit-tasks, ABI id in native-aot). eml_nrt learns about a program only from `Image`.
- **Run driver**: `eml_native::NativeMachine::{new, load(&Program, &Linked) -> Loaded, run(entry), close}` and `eml_native::execute(program, linked, config, host) -> Result<Finished, RuntimeError>`, same shape as eml_vm's. `load` compiles functions that are new by name. The loop is `eml_nrt::run(&Image, ..)`: the calling thread is the main worker, worker 0 is a spawned thread, `main` is entered through `eml_enter`, the end follows VM `Finish` (exit code, `yield_finish`, `evv_set(())`, flush, `check_finished`). Code memory is freed at `close` after all workers are joined (`JITModule::free_memory` exists [V: 7.3]).
- **Stacks** (decision #9). Worker threads are spawned with the 1 GiB reservation. Per thread, with `base` the lowest address of the stack: `hard = base + 1 MiB` on workers and `stack_base + 2 MiB` on the main worker; `real = hard + LIMIT_HEADROOM_BYTES` (2 MiB, the VM's `LIMIT_HEADROOM` of 2^18 words in bytes). `stack_limit` is `real` in normal running, `hard` while a task is aborting after a limit (jit-tasks) and `usize::MAX` as the `Pending` poison (jit-tasks). Below `hard` there is always 1 MiB (2 MiB on the main worker) for one eml frame and any Rust entry or C function. eml_nrt computes the values at thread start from the known spawn size; jit-core sets `stack_limit = real` (N-42). The main worker runs no eml code until jit-tasks (pinned tasks); jit-tasks takes its `stack_base` from an OS query, which leaves 4 MiB usable on an 8 MiB main thread. The Windows `/STACK:8388608` flag for the `eml` binary already exists [V: `crates/eml_cli/build.rs` L10]; native-aot adds it for AOT binaries. (native.md's "aborting = real limit minus headroom" with a real limit of `base + 1 MiB` would put the limit below the stack; this geometry replaces it.)
- **Task layer (jit-tasks)**: eml_vm implements `Exec` with its `Task` stacks; eml_nrt implements `Exec` with continuation closures in run queues, a nested loop for `Async.run`, per-child `JmpBuf`s for `par`. eml_nrt contains no copy of anything in eml_sched. Nested `Async.run` levels end in LIFO order: a level whose run has finished under an inner level waits for the inner one, and if the run can proceed only by ending the outer level first, the shared deadlock verdict reports `RunFault::BuriedRun` (never a hang; N-43). The limit classes and their one transition function live in eml_nrt (`limits.rs`); generated code sees only `stack_limit`.
- **Function values (jit-apply; U5 settled).** A native function value is always an object whose header is its descriptor: tag = the number of argument leaves still missing, scanned word 0 = the code address of its entry as an immediate, then the captures. `Atom::Fn(f)` is an immortal static closure built at load. An exact `apply` is two loads, a compare and a `call_indirect`; adapters exist for the `(f, k)` that occur in the program; partial application builds a generic PAP object (`eml$pap$m$r`), so no adapter per `k` and no descriptor load is needed. eml_rt and eml_sched never decode a function value; the immediate function number of object-model.md is the VM's form.
- **User C functions (jit-externs).** A non-blocking C row is a direct default-cc `call` of the import `eml_c$<name>`; narrowing checks are inline; pointer and handle arguments come from entries; panics raised around the call use the engine-held payload of 1.4.

### 1.4 Effects / interrupt contract across stages (question 3)

Principle: **a hot check in generated code, a per-site cold block of size O(owned variables), every decision in a shared routine** (an eml_nrt service or a module-level CLIF function emitted once per load).

| Piece | jit-core lays down | Later stage adds |
|---|---|---|
| Function signature | `tail` cc, `(ctx: i64, leaf params...) -> leaf rets...`, leaves `i64` / `f64` | nothing |
| Unwind point that is a call | load `yielding.target`, compare, cold branch; cold block from `UnwindPoint` (`decref` owned by count, `drop_token`, return placeholders) | jit-effects: behind the same test, the `abort` load; an abort goes to the cleanup (with the `landing` test and `ev_take_state` for a passed landing), a yield returns to the hot path where the LIR's own `yielding` check takes the slow path. That side is not marked cold: yields are the normal path of resumable effects |
| Lowering structure | one lowering function, "after unwind point", returns (hot continuation, cold block) | jit-effects extends that function only |
| Abort return values | constants (`Word::UNIT` bits, 0, 0.0); callers in an abort never read them | jit-effects: a landing writes the real dummy (`eml_rt_dummy`) |
| Faulting row | test `ctx.native.fault`; cold: `call eml$raise(ctx, site, purity)`, then the site's unwind cold block. `eml$raise` is one module-level `tail` function | jit-core body: `eml_nrt_failed` (classifies as VM `failed`, finds no handler, `eml_fatal`). jit-effects: when `eml_nrt_failed` returns a clause, call it with the clause ABI and return |
| `Never` rows | `panic`: the wrapper parks the message word in the worker struct and `Fault::Engine` in `Ctx`; `fail_io`: the shared row function returns the fault and the wrapper parks it; generated code calls `eml$raise` unconditionally, then the unwind cold block. `exit`: the wrapper stores the code in the task state behind `ctx.runtime` and calls `Ctx::exit_abort()` (sets `yielding.target`); generated code jumps unconditionally to the site's unwind cold block. No load after a `Never` row; the callers up the stack see the abort through their own `yielding.target` checks | nothing |
| Engine-held payload | `fault != 0` iff a failure is parked. When the payload is not a `Fault` (a panic message word; later a re-raise request), it is in the worker struct and `Ctx` holds `Fault::Engine`. `eml_nrt_failed` starts with `take_fault()` and, on `Engine`, takes the payload from the worker struct | jit-externs: C-row panics (narrowing, null handle). jit-tasks: re-raise requests of rows that re-enter eml code |
| Inline arithmetic faults | cold: `eml_rt_fault_arith(ctx, kind)` then `eml$raise` | nothing |
| Safepoint | every function entry: stop word, SP vs `stack_limit`; cold: `a = eml_nrt_safepoint(ctx, fn, is_ep_helper)`, `Continue` -> hot path | jit-core: fatal for every non-`Continue` case. jit-tasks: native.md 「タスクごとの上限」 implemented in the service over the classes kept in the worker struct (EP-helper rule, `LIMIT_HEADROOM_BYTES`), returns `Raise`, routed to `eml$raise`; nothing is added to the hot path |
| Entry adapter reached by an abort | returns placeholder with the state set; the driver ends like `Finish` | nothing |
| Every `Rhs::Prim`, `Call::Clause`, `Apply.prompt`, `yields` | rejected (E0004); no accepted UI file contains a prim [D: S2b 9.1] | `empty_mut_array`: jit-externs. The evidence, yield, `dummy` and root prims: jit-effects. `child_abort`: jit-tasks |
| `Call::Apply`, `MakeClosure`, `Atom::Fn` | rejected | jit-apply |
| `Call::Par`, `ChildAbort`, async-class rows, blocking C | rejected | jit-tasks |

- LIR edges only go forward [V: lir.md L816] and a self `return_call` re-enters the entry, so the entry safepoint is the only one today. A loopification pass would add the back-edge safepoint with the same lowering function.
- Evidence roles (bound for jit-effects): generated code never reads or writes an evidence object; roles do not appear in eml_native. jit-effects picks constructors or role consts on the `Ctx` surface; the fork-evidence builders (`fork_evv`, the `Panic` tail, the re-raise mask) become one implementation used by eml_vm and eml_nrt.

**What is fixed and what may be revised.** Fixed by this contract (changing one is a contract change that goes back to the user): the eml function signature shape; `ctx` first; the C-ABI kinds of entries; the three failure protocols by prefix, as seen from generated code (what it tests after which call: nothing after `eml_rt_*`, the fault word after a `faults` `eml_x_*` row, the action word of `eml_nrt_*`); `eml_enter` through a default-cc thunk; the fatal and re-entry rules; "decisions in shared routines". Revisable by any later stage: the signature of a particular entry or service (the result word of `eml_enter` included), the `Image` fields, the `Native` field set, the body of a lowering function. The procedure is: the stage's plan lists the change, the same commit updates docs/spec/native.md and every caller, and the descriptor test (N-7) passes. There is no compatibility to keep; JIT code and runtime are one build. *Changed in r3 and to be confirmed by the user with the plan:* the `eml_x_` protocol and the fatal rule now say that an `eml_x_*` shim ends in the fatal path for internal-class errors and caught panics (r2 said "`eml_x_*` wrappers never call `eml_fatal`", which contradicted N-10's `catch_unwind` rule).

### 1.5 Stage split (question 4)

Eight consecutive stages replace the roadmap's three (5.3 has scope and completion):

`jit-rt` -> `sched-split` -> `jit-core` -> `jit-apply` -> `jit-externs` -> `jit-effects` -> `jit-tasks` -> `jit-repl`

`jit-rt` and `sched-split` are independent of each other; the order between them is free. Later prerequisites: native-aot needs `jit-tasks`; native-inline needs native-aot; native-par needs native-inline, `std-par`, `jit-tasks`.

### 1.6 Tests and verification (question 5)

- **Engine selection.** `eml_test_support::execute_on` and the harness's `execute_in` take an engine (`Vm` / `Native`).
- **Native UI pass as its own tests.** Two new directory tests, `ui::run_native` and `ui::run_fail_native`, each with 1 s per file and 5 s in all (testing.md's list of five becomes seven; nextest overrides likewise). Per file: a mark -> skipped; compile; `eml_native::unsupported(&program)` non-empty -> count as rejected; otherwise run the VM in fast mode and native (1 worker) in the same process and compare stdout, stderr, written files and the result (exit code or error text) byte for byte. A `run-fail` file first runs on the VM in checked mode in the same test (a VM bug must not run unchecked code in the test process); it is compared only when that run fails with an error `fast_mode_reports`, and is counted as `checked only` otherwise. `ui::run` / `ui::run_fail` are unchanged, so their budget is untouched.
- **Budget gate.** jit-core's plan has, as the task that first runs UI files natively, a measurement of the slowest file in dev with `cargo nextest run -j 1`; over 1 s blocks the stage. Evidence so far: Cranelift compiles 183k CLIF instructions (1000 functions) in 278 ms dev / 208 ms release [V: 7.4]; UI programs have a median of 118 and a maximum of 7514 LIR statements [V: 7.5]; real lowering is not measured [I].
- **Skip mark.** `-- native: skip (<reason>)` in the entry file's first lines, parsed with the existing marks; it takes precedence over rejection. Introduced in jit-core with a testing.md section; each marked file is listed in the plan (kind 1) and its reason names a difference of spec/native.md 「VMとの違い」 (N-43). jit-core marks the 8 `max-depth` files [D: S2b 9.5]; the 5 `max-bytes` files are not marked. A file that is accepted but cannot be made identical for a reason that is not a lowering bug gets the mark by a plan revision; more than 10 such files in one stage goes back to the user.
- **Rejection snapshot.** One insta snapshot per native directory test: number run natively, number rejected, histogram by first rejected form. Stage plans state the expected numbers (kind 2). At jit-tasks close rejected is 0 and the counting code is deleted.
- **Leak census.** Every native run in tests ends with `Heap::live_objects()` (both modes, verified [V: 7.7]) and asserts no object with rc != 0 is left after a successful run. The condition is clean on the VM: 1759 fast-mode heap closes over `ui::run` and `ui::run_fail` (every worker's heap), 0 with a counted object left [V: 7.10]. This is native's replacement for checked mode as the RC safety net.
- **Codegen unit tests** (`crates/eml_native/tests`): LIR text -> JIT -> `eml_enter` -> result word + census. CLIF text snapshots only for a fixed small set that pins the cost model (call + unwind check, entry safepoint, inline `dup` / `decref`, tag switch, a `faults` row and a non-`faults` row).
- **Differential tests of inline code**: each `Inline` row against its reference implementation (the shared row function once the row is in the `extern_rows!` list, the `eml_rt::externs` function before that) over an edge-value matrix; inline `dup` / `decref` against `Heap::dup` / `decref` per rc class; inline `int` boxing against `Heap::alloc_int` at the 63-bit boundaries.
- **Stub tests** (`crates/eml_nrt/tests`, all four CI targets = AAPCS64 on macOS and Linux, SysV x64, Win64), with Rust `extern "C"` functions as targets: normal return, fatal from depth n, nested enters, callee-saved integer and float registers intact after a fatal, `ctx.jmp` restored, fatal out of a `catch_unwind` shim on both arms (Err and panic).
- **ABI tests** (`crates/eml_native/tests`, all four CI targets): (1) `tail` / `return_call` (self, and to a callee with more stack arguments), multi-value return, longjmp through `tail` frames; (2) for each distinct parameter-kind sequence in the entry descriptor table, plus fixed mixed sequences of 6 and 9 parameters alternating `i64` / `f64`, a JIT caller built from the descriptor calls a Rust echo entry declared by the same macro and every argument and the result round-trip.
- **Miri**: runs eml_nrt's entries called from Rust (`eml_nrt entries::`, both borrow models, budget recorded in testing.md) and eml_rt `trusted::` for T18. Tests that use asm or JIT code are `#[cfg(not(miri))]`. **ASan**: eml_nrt tests and the native UI tests, without fatal-path tests until `eml_fatal` calls `__asan_handle_no_return` (U6). **TSan**: from jit-tasks, the native runs of the thread-crossing categories.
- **Before any native stage closes**: all CLAUDE.md gates; rejection snapshot equals the plan's numbers; census clean; stub and ABI tests green on the four CI targets; `RunStats` snapshots untouched unless the plan lists them (a VM change that a native-favouring choice needs is listed as kind 2, not avoided; N-38); every task that edits eml_rt passes Verus, verusfmt, Miri `trusted::` and the eml_rt bench gate (N-29); the native numbers are re-measured and benchmarks.md 「ネイティブの数値」 is replaced (N-39).

### 1.7 Performance and profiler readiness

Premise: native performance is a first-class design criterion; the VM is the reference engine for checking and development, and a VM slowdown is acceptable when it buys native speed or a cleaner native design. Performance is pursued through clean design only (data layout, calling convention, simple single-sourced inline fast paths); an optimisation that needs special cases, duplicated logic, fragile invariants or asm beyond the entry stubs is forgone and listed (N-38).

**Cost model of what this contract fixes** (fast path; the stage designs refine it per construct, the CLIF pins of 1.6 record it):

| Path | Fast path | Later |
|---|---|---|
| direct call | one PC-relative `call`; arguments and `ctx` in registers (N-37) | - |
| tail call | `return_call`, a jump; a self tail call passes the entry safepoint | - |
| unwind check after a non-tail call | 1 load (`yielding.target`), compare with a constant, 1 branch not taken | minimal for decision #6 |
| entry safepoint | 3 loads (stop pointer, stop word, `stack_limit`), SP read, 2 compares, 1 branch | a single-load form and leaf elision are forgone (cross-thread writes, a special case); revisit in native-inline with numbers |
| `dup` / `decref` of a local object | immediate test, 1 load, range compare, 1 store | last reference, shared, stuck: 1 call |
| field read / write, tag | 1 load / store at a constant offset; tag: 1 load + mask | - |
| allocation | 1 entry call + 1 store per field; `int` boxing inline | inline allocation path: native-inline, once eml_rt exports the allocator facts |
| called row | 1 default-cc `call` of the entry (an address load + indirect branch in the JIT; a PLT-free direct call in AOT); `faults` rows add 1 load + 1 branch | container rows inline: native-inline |
| `shim` vs `plain` entry | no difference on the fast path: 818,705,818 vs 818,687,270 instructions per 10^8 calls [D: S2a 1.3] | so U4 stays "`catch_unwind` everywhere" |
| reading a `Native` field | 1 load at `ctx + CTX_*`, offsets 0..64: 8-bit displacement on x86-64, scaled `ldr` on aarch64 (N-4) | - |
| a runtime entry calling an out-of-line `Heap` operation | 1 `add` to form `&mut ctx.heap` (offset 72) | the price of `Native` first; +0.03 % on the allocation row (1.2(c)) |
| `eml_enter` round trip | about 52 instructions [D: S2a 1.3], paid per entry from Rust (run start, task resume, `par` child), never per eml call | one enter per worker loop is forgone (a Rust loop under a `JmpBuf`) |
| exact `apply` | 2 loads (meta, code word), compare, `call_indirect`, the unwind check | - |
| `perform` | 2 entry calls (`ev_lookup`, `ev_clause`), 1 load, 1 indirect call | evidence stays out of generated code (N-24); a per-site cache is forgone |

**Measurement** (N-39): recorded from the first native stage, never a gate on final targets. The final targets stay those of docs/future/native.md 「目的と範囲」 and are judged in native-inline.

**Profiler readiness.** The profiler is future work (one roadmap item after native-aot, N-40). What it needs is kept possible and cheap by these hooks:

| Hook | Stage | Criterion |
|---|---|---|
| frame pointers preserved in all generated code | jit-rt (the one flag constructor) | N-34 |
| the stubs keep the frame-record chain; no stack switching; the fatal longjmp is the only non-returning transfer | jit-rt | N-35 |
| runtime entry names and addresses (the descriptor table) | jit-rt | N-7 |
| stable symbol name per function; `NativeMachine::symbols()`; `Image.functions[i].{code, size}` | jit-core | N-36 |
| site -> `(function, file, line, column)` (the fault-site table) | jit-core | N-36 |
| task attribution: the published current-task word is written at each native entry start and end | jit-tasks | (review) |
| symbol table of AOT binaries | native-aot | - |

Scope of the roadmap item 「ネイティブのプロファイラ」: a sampling profiler for native runs (`eml run --native`, later AOT binaries): sample each worker's (pc, fp), walk frame records, resolve through the symbol map and the entry table, report per eml function, per source line where a position is known, and per task; write a `perf` map / jitdump on Linux and an equivalent symbol table for macOS and Windows tools; statement positions in LIR for line-level attribution (today only extern statements carry a `Loc`). Not implemented by the jit stages.

---

## 2. Rationale

- **eml_nrt separate from eml_native.** native-aot links runtime + stubs into `libeml_rt.a` with no compiler on the user's machine [R: native.md 「AOT と配布」]; entries, stubs and driver must build without Cranelift and eml_lir.
- **Entries outside eml_rt.** eml_rt's rules (all code in `verus!`, no `unsafe` outside, only vstd) exclude `catch_unwind` shims, asm and rows that need eml_io / eml_gui. The shim layer is unverified caller code, the same trust class as eml_vm.
- **`Native` as a non-generic `repr(C)` first field, `heap` second.** Same offsets in both modes, declared order, `offset_of!` supplies every number, no proof cost; the shortest encodings for the reads generated code makes most, and a shorter yield path in the runtime, for one `add` per out-of-line heap call (measured in 1.2(c)).
- **Fatal-only heap entries.** Heap errors stop the evaluation in the VM too, and fast mode produces only the two `Err`s named in 1.2.
- **Decisions in shared routines.** `failed`, the limit table and stop-bit priority exist once; later stages replace a routine body instead of emitted shapes.
- **Entries and fields arrive with their consumer.** A signature fixed without a caller is where rework comes from; with one build for JIT code and runtime, changing a signature is cheap, so nothing needs to be frozen early.
- **sched-split first.** The run-error types are needed by jit-core, and the `Std.Tls` / `Std.Gui` non-async arms needed by jit-externs import `cells` types and `DriverKind` [V: 7.11]. Moving `cells.rs` once, before any native code depends on it, removes both orderings problems; it is a VM-only stage whose tests stay byte-identical.
- **`exit` in jit-core.** The only abort that needs no handler; it exercises the cleanup blocks in the stage that creates them.
- **Seed rows in jit-core.** 304 of 308 effect-free UI programs call `println` [V: 7.5]. The seed uses the final wrapper mechanism on an allow-list, so jit-externs widens the list.
- **`is_async()` as the externs / tasks boundary.** Its definition is "rows that touch the registry, the run's driver kind or the current task" [V: `eml_extern/src/lib.rs` L2245-2250]. In `run_extern` every non-async arm takes only `ctx` and operand words; the only ones outside `Ctx` methods / `externs::` are 10 `Std.Tls` and 15 `Std.Gui` rows (`tls::f(ctx, words)`, `gui::arm(ctx, ext, args)`) [V: 7.11].
- **Win64 in jit-rt.** CI runs Windows on every push; native.md's open Windows item is settled in the first stage instead of the sixth.
- **Separate native UI tests.** Own time budget and own failure report; the existing five tests keep theirs.

- **Internal-class errors of a row end in the fatal path.** A non-`faults` row has no unwind point, so a parked fault would be seen only at the end of the run; stopping at the failing statement is the only useful report for a bug. User-reportable failures still travel by the fault word.
- **One marker for engine-held payloads.** A panic message is an eml string word and a re-raise request is engine state; neither is an eml_rt `Fault`. `Fault::Engine` keeps "the fault word is non-zero iff a failure is parked" true for every source, so generated code and `eml_nrt_failed` have one rule.
- **Limit state in the worker struct.** Generated code compares SP with `stack_limit` only; the class and the real limit are read by one Rust service. Keeping them out of `Native` keeps eml_rt out of jit-tasks.
- **Generic PAP objects.** The apply fast path stays at two loads and the runtime routine at a few lines; the cost is one more indirect jump when a PAP is called.
- **Profiler hooks now, profiler later.** Frame pointers, frame records in the stubs and name-keyed symbols cost nothing to keep and are expensive to retrofit.

## 3. Alternatives considered and rejected

- **`extern "C"` entries inside eml_rt**: trust-list discipline, `catch_unwind` and asm in the verified crate, rows needing eml_io / eml_gui.
- **Stubs and driver in eml_native behind a feature**: AOT would need a Cranelift-free build of a Cranelift crate.
- **A const-only facade crate (`eml_layout`) between eml_rt and eml_native**: the facts are defined in eml_rt's `verus!` code (rt-layout P2/P3); a facade is a second surface. A grep (N-31) gives the same guarantee.
- **`Ctx` stays `repr(Rust)`**: offsets differ per mode and may be large; no declared order.
- **A wrapper struct in eml_nrt holding the hot fields**: rt-layout G-12 (fields are `Ctx` fields).
- **Stub calls the `tail` adapter directly**: depends on Cranelift's private register assignment [V: 7.3].
- **Checked return from every heap entry**: cost on the hottest calls for errors fast mode does not produce.
- **`limit_class` and fault-class trees emitted per function**: larger code; each later stage would rewrite emitted shapes.
- **All entries and all nine fields declared in jit-rt** (r1): frozen without consumers.
- **eml_sched created inside jit-core with only the error types** (r1): `error.rs` depends on `cells::CellFault`, so it forces a cells refactor into a codegen stage.
- **Moving the 25 Tls / Gui non-async rows to jit-tasks**: breaks the `is_async()` boundary for no gain once `cells.rs` is in eml_sched.
- **Native pass inside `ui::run`** (r1): shares the existing 1 s per file.
- **A compiled-std cache across tests**: needs a shared JIT module and cross-module linking; not needed with a separate budget.
- **Thread-local runtime pointer**: decision #5. **Native checked mode**: decision #3.
- **CLIF text snapshots as the main codegen test**: churn, no behaviour.

- **`heap` first, `Native` second**: every gated `bench/rt.sh` row within +0.006 %, but the `yield` row gains 2.5 % instead of 10.6 % and `map-reuse` 0.2 % instead of 4.2 %, and every `Native` read on x86-64 carries a 32-bit displacement. It optimises for the VM-era per-row gate, not for native code.
- **`limit_class` / `stack_real` / `worker` in `Native`** (r2): no reader in generated code or asm.
- **Parking every `Err` of every row and checking at the end of the run**: loses the stop at the failing statement for internal errors.
- **A `Fault` variant that holds a `Word`** for the panic message: puts an RC-counted word inside an eml_rt value that the heap does not scan.
- **One adapter per `(f, k)` for every `k`** (native.md L41): needs a descriptor load or lookup on every partial application.
- **Rewriting eml_vm's 54 fixture uses on the public API inside sched-split**: turns a move-only stage into a test rewrite; the dev-only feature is bounded by N-41 instead.
- **Baton threads or fibers for nested `Async.run`**: a per-thread driver rework or a stack switch (which N-35 forbids) for an ordering native.md already gives up.
- **Performance special cases** (folded yield check, fused `perform` entry, `catch_unwind`-free entries, single-load safepoint, a pinned `ctx` register, a `fork_evv` memo): listed as forgone in the stage designs (N-38).

## 4. Assumptions made

| # | Assumption | If false |
|---|---|---|
| A1 [I] | `tail` / `return_call`, the stubs and the 1 GiB reserved stack work on x86_64 Linux, aarch64 Linux and x86_64 Windows as on macOS arm64 | jit-rt does not close (5.3). The failing case goes to the user with native.md's two options for Windows (trampoline for self tail calls, or no native on Windows); a Linux failure questions decision #4 |
| A2 [I, V on aarch64 macOS: 7.9] | A longjmp over destructor-free Rust frames (one entry + `eml_fatal`), including out of a function that used `catch_unwind`, is sound on all three ABIs | jit-rt's stub tests fail on that target. Then heap entries get a checked return and generated code calls `eml_fatal`; only the `eml_rt_*` protocol changes |
| A3 [I] | The native pass keeps each UI file under 1 s in dev | The budget gate in jit-core blocks; remedy is decided there with numbers (first candidate: lower Cranelift `opt_level` for functions over a size threshold is *not* allowed; splitting slow files' native pass into `#[ignore]` with reason is) |
| A4 [D: S3 7.4] | No non-async row calls back an eml closure: every body calls an eml_rt method, an `eml_rt::externs` function, `tls::*` or `gui::arm`; `Par` is LIR `par` | That row re-enters through `eml_enter` (re-entry rule); if the closure can yield, the row is reimplemented in jit-tasks |
| A5 [V: 7.10] | The census condition holds for correct runs | - |
| A6 [D: S2a 7.2, probed] | `cells.rs`, `error.rs`, `park.rs`, `driver.rs`'s `Clock` / `DriverKind` and `RunConfig` / `Finished` move to eml_sched with visibility and import edits, plus two things the probe found: eml_vm's unit tests use `#[cfg(test)]` fixtures of `cells.rs` (54 uses), which become the dev-only feature of N-41, and `RunStats` loses `#[non_exhaustive]` (eml_vm builds it with a struct expression; five test sites become struct-update expressions, kind 3) | sched-split grows by the untangling it needs; it stays VM-only and byte-identical |
| A7 [D: S2a r1 C-1 and 7.4, medians of 3 and one full pair; the 5-pair run in the task decides] | With `Native` first the `bench/rt.sh` deltas are those of 1.2(c): four rows between +0.11 % and +0.45 %, `yield` and `map-reuse` down by 10.6 % and 4.2 %, the sum negative | N-29's exception does not apply; the task stops and goes to the user with the table and the `heap`-first alternative (all gated rows within +0.006 % [D: S2a r2 7.4]) |
| A8 [settled: S3 D-7] | `gui.rs` moves whole and `GUI_SLOT` stays a `thread_local!` in eml_sched: it is toolkit state bound to the main OS thread, not `Ctx` state | N-5 covers only eml_nrt and eml_native |
| A9 [I] | No UI program has the nested `Async.run` inversion of N-43 (7 files have two or more `Async.run`; two were read) | The file gets the mark (kind 1); if the difference is unacceptable to the user, the fallback is baton threads, a user decision |
| A10 [I] | `catch_unwind` in an entry costs no instruction on the non-panicking path on x86-64 and Windows as measured on aarch64 macOS | `bench/native.sh` shows it; then a cannot-panic entry may become `plain` with its argument written in spec/native.md (U4) |

## 5. Implications for implementation

### 5.1 Constraints on every downstream stage

- Lowering reads only the function's LIR after `reuse`, the layouts it names and extern rows. A function's code depends on its name only.
- `eml_native::unsupported(&Program) -> Vec<Unsupported>` is the single statement of what is rejected; `Session` turns it into E0004 (one per distinct form, deterministic order); lowering panics on a rejected function. Stages shrink `Unsupported`; jit-tasks deletes it.
- `eml run --native` conflicts with `--debug-heap` and `--stats` at the clap level.
- `RuntimeError` and its `Display` are defined once (eml_sched). Native supplies the function name and `Loc` through the fault-site table and the function id passed to cold services.
- Object memory and `Ctx` are touched in eml_native only through one module that wraps `load` / `store` and imports the eml_rt consts.
- Adding a std row stays "table row + implementation + std declaration", where the implementation is one line of the `extern_rows!` list. Engine-specific by nature, and the only exceptions: `exit` and `panic` (`fail_io` is a shared row whose result is `never`).

### 5.2 What the early stages lay down

1. jit-rt: `Native` (six fields, the head of `Ctx`), consts, T18; fault parking; the entry macro (`plain` and `shim` forms) and descriptor table; `eml_enter` with thunks and frame records, `eml_fatal(ctx, code, site)`, nested `JmpBuf`; the one flag constructor; the ABI tests; the eml_rt bench gate script and the `nrt_bench` rows.
2. sched-split: eml_sched with the run interface and `cells.rs`.
3. jit-core: `Image`; the worker struct behind `ctx.runtime` (exit code, recorded error, engine-held payload); `Fault::Engine`; `eml$raise`, `eml_nrt_failed`, `eml_nrt_safepoint` with action words (`Continue`, `Unwind`, `Raise`); the stack geometry consts; the "after unwind point" lowering function; the row-lowering table; `extern_rows!` with the seed rows; `NativeMachine::load` by name and `symbols()`; the native UI tests, mark, snapshot, census; spec/native.md 「VMとの違い」; `bench/native.sh`.

### 5.3 Stage list

| Stage | Scope | Completion condition |
|---|---|---|
| **jit-rt** | eml_rt: `Native` first, `heap` second (yielding, fault, stop, stack_limit, jmp, runtime), consts, T18, fault parking; the rt-layout exports assigned to jit-core (`Meta` bits, `Word` consts, `rc` consts, box value word with eml_rt's sites and eml_vm `compile/constants.rs` rerouted); `Ctx::bytes_empty` infallible (so that no non-`faults` row can fail in fast mode). eml_nrt: stubs for AAPCS64, SysV x64, Win64 with frame records; `JmpBuf`; `eml_fatal`; entry macro and table. eml_native: edges to eml_rt, eml_lir, eml_nrt; the flag constructor (N-34); the `REUSE_SCANNED_LIMIT <= INLINE_SCAN_LIMIT` const assert; the ABI tests. CI edge step rewritten (N-2). `bench/rt-gate.sh`, `nrt_bench` | Verus / verusfmt / Miri / bench gate with the layout task's exception (N-29); stub and ABI tests green on macOS, Linux x86_64, Linux aarch64 and Windows. **A red target blocks the close**: no cfg-exclusion without a user decision (A1). No VM snapshot changes. `nrt_bench` rows recorded (N-39) |
| **sched-split** | eml_sched created; `cells.rs` (with its import test), `error.rs` types, `RunConfig` / `Finished` / `RunStats` / `Clock` / `DriverKind`, `Park` move from eml_vm; eml_vm and eml_cli re-export; the dev-only `testing` feature (N-41). VM-only | every test and snapshot byte-identical (kind 3 only); N-2's eml_sched line; `cargo tree -p eml_sched` has no eml_vm; N-41's checks |
| **jit-core** | Lowering of scalars, data (`con`, `unpack`, `switch`, boxes, `release`, tokens, reuse), direct calls, `LetMany` / `ReturnMany`, tail calls, RC, constants; unwind checks, cleanup blocks; entry safepoints; fault path; the 45 inline scalar rows; seed rows through `extern_rows!` (U1); heap entries; eml_rt: `Heap::is_immortal` (census) and `Fault::Engine`; fault enum renamed `RunFault`; `Image`, worker struct, `NativeMachine`, driver; `--native`; native UI tests, mark, snapshot, census; `bench/native.sh` | Rejected forms are exactly: closures / `apply` / `Atom::Fn`, every `Rhs::Prim`, `clause`, `prompt`, `par`, async-class rows, blocking C, std rows without an entry, user C rows. **At least 150 `tests/ui/run` files run natively and match the VM** (174 of 635 are accepted by the classifier at the base commit [D: S2b 9.1]); the snapshot pins the exact numbers; budget gate passed; N-19's abort test; N-36 and N-37; eml_rt gates for the eml_rt task; native numbers recorded |
| **jit-apply** | `MakeClosure`, `Atom::Fn` (static closures), `Call::Apply` and tail apply, closure entries `(f, k)` for the `k` that occur, generic PAP objects for partial application, excess application; closure entries; object-model.md and spec/native.md state the native form of a function value | closure forms leave `Unsupported`; **at least 185 `run` files run natively** (208 accepted by the classifier [D: S2b 9.1]); census tests for each apply path (closure freed on last use, captures dup'ed otherwise, partial, partial of partial, excess, abort inside an excess application); native numbers recorded |
| **jit-externs** | `extern_rows!` lists all 334 rows; the VM's `run_extern` `match` and dedicated op handlers and eml_nrt's wrappers come from it; `empty_mut_array`; `tls.rs` and `gui.rs` moved to eml_sched with the feature forwarding of 1.1; non-blocking user C rows called directly (inline narrowing checks, handle and pointer entries, engine-held panics) | the rejection histogram contains no form other than: the effect / root / `child_abort` prims, `clause`, `prompt`, async-class rows, blocking C, `par`. N-21's partition test; N-22's coverage test; the infallibility test of non-`faults` rows. `RunStats` snapshots byte-identical unless the plan lists a change. (Expected: about 308 `run` files native; the snapshot is the record.) Native numbers recorded, with native rows in `bench/scripts.sh` for the scripts that run |
| **jit-effects** | all effect prims (`yielding` / `yield_status` inline, one `eml_rt_<prim>` entry each otherwise), effect numbers and mask lists as module data keyed by name, `Call::Clause`, landing, yield slow path, `eml$raise` calling clauses (`try_panic`, `try_io`), root prims; `CTX_YIELD_ABORT`; evidence roles onto the `Ctx` surface with eml_vm and the eml_rt lemmas rerouted (rt-layout G-10) | the histogram contains only async-class rows, blocking C, `par` (and `child_abort`); UI categories `effects`, `unwind`, `panic` have no rejected file; eml_rt gates; native numbers of the effect benches recorded |
| **jit-tasks** | worker table, inboxes, monitor, jobs and the `Exec` trait into eml_sched (first commits, VM byte-identical); the `testing` feature deleted (N-41); native `Exec`: the 101 async-class rows, continuation waits, nested `Async.run` (LIFO levels, `BuriedRun`), `par` on the parent's worker with per-child `JmpBuf`, `compute`, blocking C, the limit classes and their one transition function in the worker struct (no new `Native` field, no eml_rt edit expected), task limits and `ResourceLimit`, the stop procedure across workers, main worker (`hard = stack_base + 2 MiB`) and its pending loop as a Rust function, signals | rejected is 0; the counting code and `Unsupported` are deleted; every UI file is identical natively at 1 worker or carries the mark with a reason of N-43; order-independent `async` / `chan` / net files also at 2 and 8; TSan job extended; stdlib.md and spec/native.md state the nested-run rule; native numbers of `par_tree`, `bench/async/scale.sh`, `bench/net.sh` recorded |
| **jit-repl** | native REPL: append by name, roots, recovery after a fatal, transcripts in native mode | REPL transcripts identical to the VM's fast-mode pass |

### 5.4 What later stages rely on

- **native-aot**: eml_nrt is Cranelift-free; lowering is generic over `Module`; symbol-only references; `Image` is `repr(C)` data; the ABI id is a const computed from eml_rt's consts, `Native`'s offsets and the entry descriptor table; the Win64 stub already exists. The C symbol `eml_run_pending` is added there, with its first C caller; called rows become direct calls (no address load).
- **native-inline**: the row-lowering table; lir.md's borrowed-parameter contract; the one memory-access module (also the place for an inline allocation path).
- **native-par**: every worker enters any closure through `eml_enter` + thunk with its own `Ctx`; per-child `JmpBuf`s and join nodes from jit-tasks; no thread-locals.

### 5.5 Documents

**(a) The doc-revision task right after the plan commit** changes design texts only (`docs/future/`). The decisions of this contract and of the jit-stage designs take precedence over the current text of docs/future/roadmap.md and docs/future/native.md (user premise); the task rewrites those texts to these contents:

| File and place | Change |
|---|---|
| roadmap.md 「段の列」 L31-33 | Replace the `jit-core`, `jit-externs`, `jit-effects` bullets by eight bullets with the scopes of 5.3. The rt-layout export list of the old jit-core bullet goes to `jit-rt`; the user-C sentence to `jit-externs`; "この段で、E0004で断る形がなくなる", `eml_run_pending`, `Async`, `par`, limits to `jit-tasks`; the evidence-role sentence stays in `jit-effects` |
| roadmap.md L34 `native-aot` | Delete "Win64のエントリのstub"; prerequisite `jit-tasks`; add `/STACK` for AOT binaries |
| roadmap.md L38 `native-par` | prerequisite: `native-inline`, `std-par`, `jit-tasks` |
| roadmap.md L337 (`eml_sched` の crate) | Point to `sched-split` (cells, run interface) and `jit-tasks` (worker table, monitor, `Exec`) |
| roadmap.md L571 and ffi.md L128 | keep (`jit-externs` still names the right stage) |
| native.md decision #1 | Code generator `eml_native`; runtime entries, stubs and driver in `eml_nrt`; shared layer `eml_sched`; edges of 1.1 |
| native.md 「呼び出し規約とエントリ」 L41, L44 | L41: adapters exist for the `(function, k)` that occur; partial application builds a generic PAP object. L44: `eml_enter(ctx, thunk, entry, argv)` returns 0 or the code and site of the fatal error; the stub calls a default-cc thunk that calls the `tail` function; reason (the `tail` register assignment); all three stubs keep a frame record; `eml_run_pending` becomes a C symbol in native-aot |
| native.md 「extern の失敗」 L64-66 | fault word + parked `Fault`; `Fault::Engine` for payloads the engine holds; `eml$raise` and `eml_nrt_failed` as shared routines; `Never` rows; internal-class errors of a row are fatal |
| native.md 「評価全体を止める誤り」 L73 | the fatal rule of 1.3 (which Rust frames may be crossed); heap entries are fatal-only |
| native.md 「タスクごとの上限」 L79-83 | the class is not a `Ctx` field: it lives in the runtime's per-worker state with one writer of `stack_limit`; the decision table is implemented in `eml_nrt_safepoint`; generated code branches on its action word; the limits are `hard` and `real = hard + LIMIT_HEADROOM_BYTES` as in 1.3 (replacing "real minus headroom") |
| native.md 「安全点とスタック」 L91, L95, L98 and decision #9 | entry safepoint only while LIR edges are forward; the stack geometry of 1.3 (workers `hard = base + 1 MiB`, main worker `hard = stack_base + 2 MiB`, `real` 2 MiB above); the `Native` field list (six fields with `runtime`, at the head of `Ctx`, the heap after it), fields arrive with their first reader in generated code or asm |
| native.md 「extern のエントリ」 L102-103 | wrappers live in eml_nrt; the three prefixes and protocols; the entry ABI kinds |
| native.md 「メインのworkerの対応」 row 1 | the shared core is `eml_sched` |
| native.md 「AOT と配布」 L152 | `libeml_rt.a` is built from eml_nrt (with eml_sched, eml_rt) |
| native.md 「テスト」 L178-180 | mark `-- native: skip (<reason>)`; separate native directory tests; rejection snapshot with histogram; census |
| native.md 「未決の論点」 L199 | the Windows `tail` item is settled by jit-rt's tests; keep the fallback text as the option if they fail |
| multicore.md 「層の構成」 L49-63 | diagram and dependency sentence: `eml_nrt` implements `Exec`, `eml_native` is the code generator; eml_sched arrives in two steps |
| native.md 「並列の実行」 L128 | nested `Async.run` levels end in LIFO order; a run that needs the inversion ends with the `BuriedRun` deadlock error |
| native.md 「目的と範囲」 (new paragraph) | the VM is the reference engine for checking and development; where a choice trades native speed against VM speed, native speed wins unless it needs special cases or duplicated logic; frame pointers and name-keyed symbols are kept for a future profiler |
| roadmap.md 「その後の項目」 | new item 「ネイティブのプロファイラ」: scope of 1.7; 前提 `native-aot`; 後に回した理由: it needs the finished backend and the symbol table of AOT binaries; 見直す条件: when `native-aot` closes (N-40) |
| roadmap.md 「段の列」 | `sched-split` is listed (user premise), with 前提 なし; `jit-core`'s 前提 is `jit-rt`, `sched-split` |

**(b) Normative and implementation docs, written with the code:**

| Stage | Writes |
|---|---|
| jit-rt | object-model.md: 「実行文脈 `Ctx`」 (`Native` fields, who writes each), 「語の位置」 (new exports), 「`unsafe` の核」 (T18 with the table of 1.2). New `docs/spec/native.md`: 「エントリ」 (entry ABI, prefixes, protocols, `eml_enter` / `eml_fatal`, fatal rule, re-entry rule, shim body rule). architecture.md: crate graph, eml_nrt internals, CI step. testing.md: stub and ABI tests, the eml_nrt Miri command. Both CLAUDE.md files |
| sched-split | architecture.md (eml_sched, dependency edges, features), vm.md and testing.md where they name the moved files; multicore.md L63 ("`eml_sched` はまだない") |
| jit-core | spec/native.md: 「関数の形」, 「巻き戻しの地点」, 「安全点」, 「extern の失敗」, 「`Image`」, 「断る形」; testing.md: 「ネイティブで流す」 (the two tests, mark, snapshot, census, the seven-test list in 「性能のテスト」); status.md; diagnostics.md |
| jit-apply | object-model.md 「クロージャ」: the immediate function number is the VM's form; spec/native.md 「関数の値」: the native form |
| jit-tasks | stdlib.md 「`Std.Async`」 and spec/native.md 「VMとの違い」: the nested-run rule and `BuriedRun` |
| each later stage | the spec/native.md section for what it implements (a new difference from the VM goes into 「VMとの違い」 in the same commit); the matching part of docs/future/native.md is deleted or reduced to what is still future |

Code comments cite docs/spec/native.md, never docs/future or docs/plans.

## 6. Decisions on existing text, and unresolved items

### 6.1 Decisions that replace existing text (r1's challenges; accepted in direction by the user)

| # | Old text | Decision |
|---|---|---|
| C1 | Decision #1 implies stubs in eml_native | Stubs, entries, driver in eml_nrt |
| C2 | native.md L44: stub calls the `tail` adapter | Stub calls a default-cc thunk |
| C3 | native.md L73: only `tail` frames between error and stub | Plus destructor-free Rust frames of one entry and `eml_fatal` |
| C4 | native.md L81-83: `limit_class` branches in generated code | One implementation in `eml_nrt_safepoint` |
| C5 | Roadmap: Win64 stub in native-aot | In jit-rt |
| C6 | Roadmap: user C rows in jit-core | In jit-externs (blocking in jit-tasks) |
| C7 | Roadmap: three stages | Eight (1.5) |
| C8 | native.md: `// native: skip` | `-- native: skip (<reason>)` |
| C9 | rt-layout's bench gate (+0.1 % per gated row) for every eml_rt task | Kept, with one exception for the task that puts `Native` at the head of `Ctx` (N-29); to be confirmed by the user with the plan |
| C10 | Contract r2 and native.md L79: `limit_class`, `stack_real`, `worker` as `Ctx` fields | In eml_nrt's worker struct; `Native` keeps six fields |
| C11 | native.md L80, L95, decision #9: one limit, "aborting = real minus headroom" | `hard` and `real = hard + 2 MiB` (1.3) |
| C12 | native.md L41: an entry per `(function, k)` for partial application | Adapters for occurring `(f, k)`; generic PAP objects |
| C13 | object-model.md L27, L31: `&f` is an immediate function number | VM-specific; a native function value is always an object |
| C14 | Contract r2: "`eml_x_*` wrappers never call `eml_fatal`" | Internal-class errors and caught panics of an `eml_x_*` shim are fatal; the wrapper body never names `eml_fatal` (to be confirmed by the user, 1.4) |
| C15 | Contract r2: `panic` parks a `Fault` | The message word is engine-held; `Fault::Engine` marks it |
| C16 | Contract r2: one entry per non-async row; exceptions `exit`, `panic`, `fail_io` | Partition inline / entry / `Par` (N-21); exceptions `exit`, `panic` (N-22) |
| C17 | native.md L128: nested `Async.run` order may invert | It inverts to LIFO, and a run that depends on the VM's order ends with `BuriedRun` (user-visible; N-43) |
| C18 | native.md L44, L144: `eml_run_pending` exists from the task stage | A Rust function in jit-tasks; the C symbol arrives in native-aot |

### 6.2 Unresolved, with defaults

- **U1 seed rows of jit-core** (settled by S2b). Called rows: `println`, `eprintln`, `Show Int.show`, `Show String.show`, `Show Float.show`, `++`, `Eq String.==`, `Ord String.compare`, `Float` remainder, `exit`, `panic`; the first nine through `extern_rows!`, the last two hand-written in eml_nrt. No second wrapper mechanism; the 150-file bound must hold.
- **U2 E0004 anchor.** Default: the entry file's `main` name; message names the form and the first function that has it.
- **U3 `Finished` for native.** Default: the shared type with VM-only counters zero.
- **U4 `catch_unwind` in entries.** Default: everywhere, with no exception list; the measured fast-path cost is zero on aarch64 macOS (1.7). An entry may become `plain` only with a measurement on a CI target that shows a cost and a "cannot panic" argument written in spec/native.md (A10).
- **U5 closure contents in native** (settled by S2b, 1.3 「Function values」). Every scanned word is a valid `Word`; written in spec/native.md; eml_rt and eml_sched never decode it.
- **U6 ASan and our longjmp.** Default: fatal-path tests excluded from the ASan job until `eml_fatal` calls `__asan_handle_no_return` under the sanitizer cfg.
- **U7 function name for fatal-only heap errors.** Default: none (internal-error class; already outside `fast_mode_reports`).
- **U8 aarch64 Windows.** Not in CI. Default: stub with native-aot; `--native` refused there until then (machine-fact error, exit 2).
- **U9 fault-enum name** (settled). `VmFault` moves to eml_sched under its name in sched-split (pure move) and is renamed `RunFault` in jit-core, the stage that first builds it natively (kind 3; no snapshot contains the name [D: S2b 9.4]).

## 7. Additional checks

Scratch clone: `/private/tmp/claude-501/-Users-arakaki-Projects-eml/81d3cddb-3cbd-4406-a565-d9e03f21aa4d/scratchpad/jit/scratch-S1` (modified; the repository is untouched).

1. [V] `Native` experiment in `crates/eml_rt/src/heap/ctx.rs` (nine fields, `#[repr(C)]` first field of `#[repr(C)] Ctx<M>`, `.yielding` accesses rewritten by script, `offset_of!` consts, const asserts for offset 0 and equal offsets in both modes): `cargo verus verify -p eml_rt` 1220 verified, 0 errors, 41 s, no proof edited. With raw-pointer fields eml_vm fails to build (`workers.rs:883`, `lib.rs:1088`: not `Send`); with `usize` fields the workspace builds.
2. [V] `crates/eml_native/tests/probe.rs` on aarch64-apple-darwin, Cranelift 0.137.1, rustc 1.99.1: naked `eml_enter` / longjmp with a linked `JmpBuf`; default-cc thunk -> `call_indirect` of a `tail` function; 10^7 self `return_call`s without stack growth (24 ms); `return_call` to a 12-parameter callee; `tail` multi-value return `(i64, f64, i64)`; entry safepoint with `get_stack_pointer` stopping a 10^8-deep recursion; Rust `eml_fatal` from 100 `tail` frames; canary intact. `ins().return_call(func_ref, &args)` is the signature.
3. [V] Read `cranelift-codegen-0.137.1` `isa/x64/abi.rs` L1073-1080 (`Tail` argument registers rax, rcx, rdx, rsi, rdi, r8, r9, r10), L890 and `isa/aarch64/abi.rs` L1194 (`(Tail, true) => ALL_CLOBBERS`), L178 (x0 reserved); `cranelift-jit-0.137.1/src/backend.rs` L369-373 (`is_pic` only on x86_64), L206 (`free_memory`).
4. [V] `probe::compile_time`: 1000 functions, 183,000 CLIF instructions: 278 ms dev, 208 ms release, one thread.
5. [V] `crates/eml_cli/examples/{count,rows}.rs` over the 611 single-file `tests/ui/run` programs: LIR statements mean 455, median 118, p90 1171, max 7514; 308 programs without effect prim / clause / `par` / `yields` / async-named row; 264 of them without closure or `apply`; **160 closure-free and 33 closure-using effect-free programs use only the default seed rows plus the `arithmetic()`-style rows**; most frequent missing rows: `Std.FileIn.open` 43, `close` 38, `Std.String.byte_length` 17. Classification is by row name and by `Prelude.{Num,Eq,Ord,Div} {Int,Float,Char,Byte,Bool}.*` for inline rows, so it is approximate.
6. [V] `crates/eml_extern/examples/n.rs`: 334 rows; `is_async()` 101; `faults` 227; `Pure` 107; rows with a `float` 38; struct Reprs 0; `Never`: `exit`, `fail_io`, `panic`; widest row 5 parameters.
7. [V] `crates/eml_rt/src/heap/walk.rs` L275: `Heap<M>::live_objects` is in `impl<M: Mode>` with a verified postcondition.
8. [V] docs/spec/lir.md L816 (forward edges); docs/future/multicore.md L49-65; `crates/eml_cli/build.rs` L10 (`/STACK:8388608`); docs/spec/object-model.md L398-401 (the two fast-mode `Err` sources).
9. [V] `probe::fatal_from_a_catch_unwind_entry` (aarch64 macOS): a `tail` function calls a Rust `extern "C"` shim whose body is a `catch_unwind`; the shim calls `eml_fatal` after the closure returned `Err`, and after a caught panic (payload dropped first); both return to `eml_enter` with the code; the next enter works; `ctx.jmp` is null afterwards.
10. [V] Census on the VM: `Machine<Fast>` given `at_close` / `at_run_end` hooks that call `reclaim`, `live_objects` and count objects whose rc word is not 0; `cargo test -p eml_cli --test integration ui::run` (matches `ui::run` and `ui::run_fail`): 1759 heap closes, 0 with a counted object. (Three `run-fail` snapshots differed only because child `eml` processes printed the probe line.)
11. [V] `crates/eml_vm/src/vm.rs` `run_extern` (L4153-4800): arms outside `ctx.*` / `externs::*` are `tls::*(ctx, words)` (L4471-4501) and `gui::arm(ctx, ext, args)` (L4525); `tls.rs` imports `cells::{Found, LINE_LIMIT, READ_ROOM}`, `driver::DriverKind`, `error::VmFault`; `gui.rs` imports `cells::{CellRef, Raise}`, `sched::{Policy, Scheduler}`, and has a `thread_local!` `GUI_SLOT`; `error.rs` imports `cells::CellFault` (a two-variant enum, cells.rs L188) for one `From` impl; `park.rs` imports only `eml_rt::IoWaker`; `driver.rs` imports eml_io, `VmFault`, `park::PollSource`, `sched::NO_DRIVER`.
12. Not checked: execution of 2 and 9 on x86_64 and Windows; real backend compile time; the sched-split move itself; rows calling back closures; Miri on entries; `bench/rt.sh` with the new fields.
13. [V] (r3) `cranelift-module-0.137.0/src/module.rs` L189-194, L928-950: `Linkage::is_final` is true for `Local`, `Hidden`, `Export` and `declare_func_in_func` sets `colocated` from it (N-37).
14. [V] (r3) `crates/eml_rt/src/error.rs` L1-3: the fast-mode `Err` list (`WrongKind`, `StaleToken`, `RootMisuse`, `Unrecoverable`). `crates/eml_lir/src/prim.rs` L10-45: the prims are the evidence and yield prims, `Dummy`, `RootGet` / `RootSet`, `EmptyMutArray`, `ChildAbort`. `crates/eml_vm/src/vm.rs` L159: `LIMIT_HEADROOM = 1 << 18` words (2 MiB). docs/future/native.md L41, L44, L79-83, L95, L128 and decision #9 read for the texts 5.5(a) replaces.
15. [D] Taken from the downstream designs' own checks and not re-run here: S2a 7.1-7.8 (Verus with `heap` first, the sched-split probe, the bench pairs, the stub and ABI tests on aarch64 macOS, `shim` vs `plain` cost), S2b 9.1-9.8 (the classifier numbers, the Cranelift probe, `is_immortal`, the rename count, the marks, the libcall list), S3 7.1-7.4 (row counts, infallibility of `Pure` rows, no callbacks), S4 section 7. Still not executed anywhere: the SysV and Win64 stubs and the ABI tests on x86_64 and Windows; the 5-pair bench gate; real lowering time.

## 8. Conformance criteria

D = the downstream designs, C = code from the named stage. "(review)" marks a criterion with no mechanical check. IDs are stable: N-1 to N-33 keep their r2 numbers (amended in r3: N-3, N-4, N-5, N-7, N-8, N-9, N-10, N-12, N-13, N-17, N-21, N-22, N-25, N-27, N-29, N-31, N-32; none retired), N-34 to N-43 are new in r3.

- **N-1** MUST: crates and edges are those of 1.1; native runtime code goes to eml_nrt, shared engine-neutral code to eml_sched. Applies: D, C. Check: the design names a crate per new file; N-2.
- **N-2** MUST: CI step `dependency edges of eml_native`: `cargo tree -p eml_vm -e normal --target all | grep -c -E 'cranelift|eml_native|eml_nrt'` = 0; `-p eml_nrt` with `'cranelift|eml_lir|eml_core_ir|eml_vm|eml_native'` = 0; `-p eml_native` with `'eml_vm'` = 0; `-p eml_sched` with `'cranelift|eml_lir|eml_vm|eml_native|eml_nrt'` = 0; `-p eml_lir` with `'eml_rt'` = 0; the existing `eml_extern` `unsafe` grep. Applies: C jit-rt (eml_sched line from sched-split).
- **N-3** MUST NOT: eml_rt contain `extern "C"`, `no_mangle`, `naked`, `catch_unwind` or asm outside test code. Check: the unit test `trust::the_core_has_no_native_boundary` in `crates/eml_rt/src/trust.rs` passes (it uses the trust scanner, which blanks comments, strings and `#[cfg(test)]` modules). The r2 grep is retired: at base it prints the `catch_unwind` of the in-file test module of `alloc.rs` (L3655).
- **N-4** MUST: `Native` is `#[repr(C)]`, non-generic, the first field of `#[repr(C)] Ctx<M>`, and `heap` is the field right after it; every read from generated code or eml_nrt is at the fixed offset given by the `offset_of!`-derived `CTX_*` consts next to `Ctx`; const asserts pin (a) `offset_of!(Ctx<M>, native) == 0` and equal offsets for `Fast` and `Checked`, (b) every `CTX_*` const <= 120 and a multiple of 8 (an 8-bit displacement on x86-64, a scaled `ldr` and the `ldp` / `stp` reach on aarch64). Check: the const asserts compile; `grep -rnE 'CTX_[A-Z_]+\s*(:|=)' crates/eml_native/src crates/eml_nrt/src` is empty (no local definition).
- **N-5** MUST: state that generated code or asm reads per worker lives in `Native` with a const; a field (and its const) is added in the stage of its first reader in generated code or asm, by the procedure of 1.4. MUST: per-worker state that only Rust code reads (limit class, real limit, level and child records, parked payloads, the recorded error) lives in eml_nrt's worker struct behind `ctx.native.runtime`, not in `Native`. MUST NOT: thread-locals or thread-keyed side tables in eml_nrt or eml_native. Check: `grep -rn 'thread_local' crates/eml_nrt/src crates/eml_native/src` is empty.
- **N-6** MUST: native code instantiates only `Ctx<Fast>`; `--native` with `--debug-heap` or `--stats` is a usage error. Check: `grep -rnw 'Checked' crates/eml_nrt/src crates/eml_native/src` is empty; a `cli.rs` test.
- **N-7** MUST: every runtime function generated code calls outside its module is declared by the one entry macro in eml_nrt (function + descriptor together); parameters are `ctx` then `i64` / `f64`; the result is one `i64` / `f64`, nothing, or `!`. The only other import is a user C function: symbol `eml_c$<canonical eml name>`, default cc, signature built from its `CSignature` by one helper (`c_signature`). MUST NOT: hand-written `Signature`s for entries in eml_native. Check: a unit test iterates the descriptor table and rejects any other kind; `grep -rn 'Linkage::Import' crates/eml_native/src` hits one function, which takes a descriptor row or a `c_signature` result; the ABI test (2) of 1.6 on the four CI targets.
- **N-8** MUST: symbol prefixes `eml_rt_`, `eml_x_`, `eml_nrt_`, plus `eml_enter`, `eml_fatal` and the reserved `eml_run_pending` (defined by the stage that has its first C caller, native-aot; until then the main worker's loop is a Rust function). `eml_x_` names are `eml_x_<Variant>` (the `Extern` variant name, so injective) and the user-C helpers `eml_x_c_*`. Check: from jit-core a test ties every called row to an entry of that name whose descriptor equals the row's Reprs; from jit-externs the partition test of N-21.
- **N-9** MUST: failure protocols by prefix as in 1.2 and 1.4.
  - *What generated code checks.* After an `eml_x_` call it loads `ctx.native.fault` iff the row has `faults` and is not `Never`; after a `Never` row it loads nothing; after an `eml_rt_` or `eml_nrt_` call it loads nothing (a service returns an action word, or a data word when its only failure path is the fatal path). It loads `yielding.target` after a statement iff the statement is an `eml_lir::unwind::points` point that is a `Rhs::Call`; jit-effects adds the `abort` load only on the not-taken side of that test. For a user C row: narrowing checks are inline before the call, with a cold call of `eml_x_c_narrow` followed by `eml$raise` (the shape of an inline arithmetic fault); nothing is loaded after the C call itself; a row that returns a handle loads `fault` after `eml_x_c_handle_new`.
  - *What a wrapper does.* A `faults` row parks `RunFault::Extern(f)` with `set_fault(f)` and returns the placeholder. A failure whose payload the engine holds (a panic message word, a re-raise request) is parked as the payload in the worker struct plus `set_fault(Fault::Engine)`, so `fault != 0` iff a failure is parked and `eml_nrt_failed` starts with `take_fault()`. Every other `Err` (any `Err` of a non-`faults` row, a `RunFault` that is not `Extern`) and a caught panic are the internal class and end in the shim's fatal path (the error is recorded before the `Err` reaches the shim).
  - MUST NOT: a wrapper body name `eml_fatal`; a failure outside the internal class reach an `eml_x_` shim's fatal path; an `eml_rt_*` entry park a fault and return (the one exception is `eml_rt_fault_arith`, whose whole work is to park).
  - Check: CLIF pins for a `faults` row, a non-`faults` row, a `Never` row and a call (jit-externs adds a C row with a narrowing parameter); `grep -n 'eml_fatal' crates/eml_nrt/src/rows.rs crates/eml_nrt/src/c.rs` is empty; the test that every non-`faults` row with an `obj` result returns `Ok` under an exhausted budget (jit-externs); the run driver asserts that no fault is parked when a run ends.
- **N-10** MUST: every entry that can run arbitrary Rust wraps its body in `catch_unwind` and calls `eml_fatal` only after the closure returned (shim body rule). Check: the macro is the only place that writes the pattern; exceptions (U4) are listed in spec/native.md. No exception is listed at present (U4).
- **N-11** MUST: `eml_enter`'s only callee is a default-cc thunk; asm has no knowledge of the `tail` convention; stubs for AAPCS64, SysV x86_64 and Win64 x86_64 exist from jit-rt and save native.md's register sets. Check: the stub tests on the four CI targets, including the callee-saved-register test.
- **N-12** MUST: a longjmp crosses only `tail` frames, thunks, and destructor-free Rust frames of one entry plus `eml_fatal`; `eml_fatal` records nothing, so the detector moves the error into the worker struct before it raises, and a re-raise is `eml_fatal` with the same code and site. MUST NOT: a Rust entry hold `&Ctx` / `&mut Ctx`, a `&` / `&mut` to the worker struct, or a `Drop` value across `eml_enter` or `eml_fatal`. Check: `eml_fatal(` appears in eml_nrt only in the entry macro's file, `enter.rs` and the named re-raising services (grep, list in the design); the rest (review), with the rule in spec/native.md 「エントリ」; the design lists every re-entering entry.
- **N-13** MUST: lowering is generic over `cranelift_module::Module`; external references are module symbols or data. MUST NOT: a process address as an immediate (a code address reaches a closure word through `func_addr` or through `Image` at load); `JITModule` outside the JIT host file and `bridge.rs`. Check: `grep -rln 'JITModule' crates/eml_native/src` returns at most those two files.
- **N-14** MUST: eml_nrt takes program facts only from `Image` (`#[repr(C)]`). Check: N-2 (no eml_lir edge).
- **N-15** MUST: eml_native touches object memory and `Ctx` through one module that wraps `load` / `store` and takes offsets, masks and tags from eml_rt consts; the const assert `Shape::REUSE_SCANNED_LIMIT as usize <= layout::INLINE_SCAN_LIMIT` exists. Check: `grep -rnE 'ins\(\)\.(load|store|uload|sload)' crates/eml_native/src` hits only that module, the thunk builder and `bridge.rs`; absence of layout literals in that module (review).
- **N-16** MUST: every eml function has the signature `tail (ctx, leaves...) -> leaves...`; tail-position calls are `return_call` / `return_call_indirect`; entries and C functions are called with the default cc by `call`. MUST NOT: pass a `tail` function pointer to C. Check: `grep -rn 'CallConv::Tail' crates/eml_native/src` hits one signature helper.
- **N-17** MUST: every eml function's entry has the safepoint (runtime adapters and the `eml$apply`/`eml$pap` helpers are not eml functions and have none); its cold side is one call to `eml_nrt_safepoint` returning an action. MUST NOT: generated code read any limit state other than the stop word and `stack_limit`. Check: CLIF pin; `grep -rnE 'CTX_(LIMIT_CLASS|STACK_REAL|WORKER)' crates` is empty (no such const exists).
- **N-18** MUST: jit-core emits unwind checks, cleanup blocks, the fault path through `eml$raise` and the safepoint for every function it accepts. MUST: later stages add behaviour by changing the bodies of `eml$raise`, the services and the "after unwind point" function, and by lowering newly accepted forms. The items "fixed by this contract" in 1.4 MUST NOT change without going back to the user; anything else changes by the procedure of 1.4. Applies: D (S3, S4 list every jit-core function, entry or field they change), C. Check: the design's change list.
- **N-19** MUST: jit-core implements `exit` as in 1.4 and has a test that drives an abort through at least two frames holding owned values and a token and passes the census. Check: named test in `crates/eml_native/tests`.
- **N-20** MUST: `eml_native::unsupported` is the only source of native E0004; lowering panics on a rejected function; each plan lists the `Unsupported` variants it removes; jit-tasks removes the type. MUST NOT: a rejection that depends on anything but the program's LIR and the entry table. Check: one `not_yet_supported` call in eml_cli's native path.
- **N-21** MUST: the externs / tasks boundary is `Extern::is_async()` (233 / 101); `CSubmit` / `CTake` are on the tasks side. At jit-externs close the non-async rows are partitioned: a row is in the `Inline` set of the row-lowering table and has no entry, or it has exactly one entry `eml_x_<Variant>` whose descriptor equals the row's Reprs (`Never` -> no result: the entry returns normally and generated code branches to the cold block after it), or it is `Par` (LIR `par`, never an extern call, no entry). No other `eml_x_` symbol exists except the `eml_x_c_*` helpers. Check: the partition test over `Extern::ALL` in `crates/eml_native/tests`; the stage's plan states its counts (45 / 187 / 1 at the base commit).
- **N-22** MUST: a row's implementation exists once, as the shared row function that the `extern_rows!` list in eml_sched defines; the VM's `run_extern` arms and dedicated op handlers, eml_nrt's wrappers and the differential tests of N-27 all call it (the list holds the seed rows from jit-core and every row from jit-externs; the task rows' bodies arrive in jit-tasks). The engine-specific rows are exactly `exit` and `panic`. MUST NOT: eml_nrt duplicate logic of eml_sched or eml_vm. Check: the coverage test (`LISTED` equals `Extern::ALL`, each row once); the VM's `match` is exhaustive with no wildcard; duplication (review).
- **N-23** MUST: `RuntimeError` and its `Display` are defined once, in eml_sched. Check: `grep -rn 'for RuntimeError' crates/*/src | grep Display` hits one line.
- **N-24** MUST: generated code never reads or writes evidence object fields; the fork-evidence builders have one implementation. Check: the memory-access module of N-15 exports no evidence accessor (review of one file); rt-layout G-10's grep for role literals in eml_vm is empty after jit-effects.
- **N-25** MUST: the native UI tests compare with the VM fast-mode result in-process; a file is identical, rejected, or carries `-- native: skip (<reason>)`. For a `run-fail` file a checked VM run comes first in the same test, and the file is compared only when that run fails with an error `fast_mode_reports`; otherwise it is counted as `checked only`. Marks are listed in the plan as kind 1, and each reason names a difference listed in spec/native.md 「VMとの違い」 (N-43); numbers run / skipped / rejected and the histogram are an insta snapshot; the lower bounds of 5.3 hold. MUST NOT: skip by category or file name in harness code. Check: `grep -rn 'native: skip' tests/ui | wc -l` equals the plan's list; the snapshot.
- **N-26** MUST: every native run in tests ends with the census (no object with rc != 0 after success). Check: the harness and the `crates/eml_native/tests` helper call one shared function.
- **N-27** MUST: each `Inline` row has a differential test against its reference implementation: the shared row function of N-22 once the row is in the list, the `eml_rt::externs` function before that; inline `dup` / `decref` one per rc class; inline `int` boxing at the 63-bit boundaries. Check: a test iterates the row-lowering table's `Inline` entries and fails on a row without a matrix.
- **N-28** MUST: every test in the ordinary run finishes within 1 s alone in dev; each native UI file within 1 s (budget gate in jit-core). MUST: tests using asm or JIT code are `#[cfg(not(miri))]`; eml_nrt `entries::` runs under Miri with a recorded budget. Check: `cargo nextest run --profile timing -j 1` in CI.
- **N-29** MUST: a stage that edits eml_rt passes Verus, verusfmt, Miri `trusted::` and the eml_rt bench gate, and follows rt-layout G-9 / G-10. The bench gate is rt-layout's: for every gated `bench/rt.sh` row the median delta over 5 alternating pairs against the commit before the task is <= +0.1 % (committed as `bench/rt-gate.sh` in jit-rt). One exception, for the one task that puts `Native` at the head of `Ctx`: the rows `handle`, `show-int`, `show-int-long` and `slice-short` may rise by at most +0.6 % (one `add` per out-of-line `Heap` call made from a `Ctx` method), on condition that the sum of the instruction deltas over all gated rows of that run is negative; the table and the cause go into benchmarks.md as a titled finding. If the condition fails, the task stops and goes to the user with the table and the `heap`-first alternative. Applies: every task whose diff touches `crates/eml_rt` (expected: jit-rt, jit-core, jit-effects; any other stage that turns out to need an eml_rt edit lists it in its plan). Check: the gate's table in the commit message of each such task.
- **N-30** MUST: spec text is written in `docs/spec/` in the commit that implements the rule; code comments cite it. Check: `citations.rs`; `grep -rn 'docs/future\|docs/plans' crates/eml_nrt crates/eml_native crates/eml_sched` is empty.
- **N-31** MUST: in eml_native, `eml_rt` is named only in the memory-access module (consts and `const fn`s of `layout`, `Meta`, `Word`, `rc`, the `CTX_` consts) (00-decisions C-9: the JIT host file uses eml_nrt's re-exports of `HostRef`, `Control` and names no `eml_rt` item). MUST NOT: eml_native call a non-const eml_rt function. Why: run-time use of eml_rt goes through eml_nrt's entries. Check: `grep -rln 'eml_rt' crates/eml_native/src` returns only `mem.rs`; `grep -rnE 'eml_rt::(Ctx|Heap)|Ctx<' crates/eml_native/src` is empty.
- **N-32** MUST: T18 in object-model.md 「`unsafe` の核」 states owner, lifetime, writer and thread-crossing state for `stop`, `jmp`, `runtime` as in 1.2; no setter for `stop` exists; eml_nrt asserts `jmp` and `runtime` are null where a `Ctx` changes thread. Check: read the row; `grep -n 'fn set_stop' crates/eml_rt/src -r` is empty; a debug assert with `eml_nrt::detached(&ctx)` at the run driver's hand-back site (jit-core) and at the worker hand-back (jit-tasks).
- **N-33** MUST: jit-rt closes only with stub and ABI tests green on all four CI targets; excluding a target needs a user decision recorded in the plan. Check: CI run of the closing commit; `grep -rn 'cfg(not(windows))\|cfg(unix)' crates/eml_nrt/tests crates/eml_native/tests` shows no stub or ABI test.
- **N-34** MUST: eml code, adapters, module-level helpers and thunks are compiled with the flags of one constructor in eml_native, which sets `preserve_frame_pointers=true` and `opt_level=speed`; `bridge.rs` keeps its own constructor for the C bridges. MUST NOT: a per-function flag override or a size threshold that lowers `opt_level`. Applies: C jit-rt on. Check: `grep -rn 'preserve_frame_pointers' crates/eml_native/src` hits one line, inside that constructor; `grep -rln 'settings::builder' crates/eml_native/src` returns that file and `bridge.rs`.
- **N-35** MUST: each `eml_enter` stub keeps the frame-pointer chain: it sets up a standard frame record before calling the thunk (aarch64: `x29` points at a saved `x29, x30` pair; x86-64: `push rbp; mov rbp, rsp`), so a frame-pointer walk goes generated code -> thunk -> stub -> the Rust caller. MUST NOT: any design that makes such a walk impossible (generated code or a stub using the frame-pointer register for another purpose, switching stacks inside eml code, a control transfer other than call / return / tail call / the fatal longjmp). Applies: D, C jit-rt on. Check: a stub test on the four CI targets in which the callee follows the frame records from its own frame and finds, within three records, the frame-pointer value the test read before calling `enter`; the MUST NOT part (review).
- **N-36** MUST: every function the JIT host defines (eml function, adapter, module-level helper, thunk) has a module symbol name derived only from names and shapes (decision #10's key, never a load order or an address), and `NativeMachine::symbols()` returns `(name, address, size)` for each; `Image.functions[i]` carries the code address and size of function `i`; the fault-site table maps a site to `(function, file, line, column)`. Applies: C jit-core on. Check: a test in `crates/eml_native/tests` loads a program and asserts that every `Image.functions` row has a symbol whose range holds its code address, that symbol ranges do not overlap, and that loading the same program in a fresh machine gives the same names.
- **N-37** MUST: a direct call of a known eml function, adapter or module-level helper is a `call` / `return_call` to a function declared with a final linkage (`Local` or `Export`), which Cranelift marks `colocated` (PC-relative, no address load) [V: 7.13]; `call_indirect` is used only where the callee is a run-time value (`apply`, `Call::Clause`, a closure's code word, the thunk's `entry`). MUST NOT: a function table or a data-table load on a direct call. Applies: C jit-core on. Check: the CLIF pin `a_call_and_its_unwind_check` shows a `colocated` callee; `grep -rn 'call_indirect' crates/eml_native/src | grep -v src/bridge.rs` hits only the apply / clause / PAP lowering and the thunk builder (the FFI bridge in `bridge.rs` predates the backend and is exempt).
- **N-38** MUST (review): each native stage's design and plan carry (a) a native cost model: for every hot path the stage lays down, its fast-path loads, branches and calls, and why that is the floor or which later stage lowers it; (b) the list of forgone optimisations with the reason; (c) for every choice that trades native speed against VM speed, VM simplicity or code shared with the VM, the trade and the choice. Native speed wins that trade unless the native gain needs a special case, duplicated logic, a fragile invariant, or asm beyond the entry stubs; then the optimisation is forgone and listed. MUST NOT: keep a slower or more complex native path in order to leave a `bench/run.sh` row, a `RunStats` snapshot or VM code unchanged; the VM change goes into the plan as a kind-2 item instead. MUST NOT: a per-call cost semantics does not require (a second load of a `Native` field at one site, a wrapper layer between generated code and an entry, an argument array for a fixed-arity call). Applies: D, plans of every native stage. Check: the sections exist; the CLIF pins of 1.6 are the mechanical record for the pinned paths.
- **N-39** MUST: native measurement starts with the first native stage and is a record, not a gate on final targets. jit-rt records the `enter`, `entry-plain` and `entry-shim` rows of `bench/rt.sh` (`crates/eml_nrt/examples/nrt_bench.rs`); jit-core adds `bench/native.sh` (every `bench/*.em` under `eml run` and `eml run --native`, release build: instructions retired, wall time, the native / VM ratio, `rejected` for a program native refuses); a stage that makes a script of `tests/ui/run/scripts/` or a `bench/async` / `bench/net` program runnable natively adds its native row next to the VM and Python rows of the existing script (`bench/scripts.sh`, `bench/async/scale.sh`, `bench/net.sh`). Every native stage's Close runs these and replaces docs/implementation/benchmarks.md 「ネイティブの数値」. MUST NOT: fail a stage on a native / VM or native / Python ratio. Applies: C jit-rt on. Check: `test -x bench/native.sh` from jit-core; the Close commit's diff changes the tables under that heading (review of the Close commit).
- **N-40** MUST: the doc-revision task adds one item 「ネイティブのプロファイラ」 to docs/future/roadmap.md 「その後の項目」 with the scope of 1.7, 前提 `native-aot`, 後に回した理由 and 見直す条件 (when `native-aot` closes); the designs name no other prerequisite for it. Each design states which stage lays down which hook of 1.7. Check: `grep -c 'ネイティブのプロファイラ' docs/future/roadmap.md` >= 1 after that task.
- **N-41** MUST: the dev-only feature `eml_sched/testing` (sched-split) gates only test fixtures and test hooks (items; no `cfg!(feature = "testing")` branch in non-test logic), is enabled only by dev-dependencies, and is deleted by jit-tasks, the stage that moves the files whose unit tests use it; tests that still need a fixture then are rewritten on the public API in that stage. No other new cargo feature is introduced by the jit stages. Check: `grep -rn 'feature = "testing"' crates/eml_sched/src | grep -c 'cfg!'` = 0; in the CI edge step, `cargo tree -p eml_cli -e normal,build -f '{p} {f}' | grep -c 'eml_sched.*testing'` = 0; after jit-tasks `grep -rn 'testing' crates/eml_sched/Cargo.toml` is empty.
- **N-42** MUST: the stack geometry of 1.3 is defined once (`hard` margins and `LIMIT_HEADROOM_BYTES` as consts), and `stack_limit` is `real` from jit-core on, so the depth a program reaches does not change when jit-tasks adds the classes; after thread start `stack_limit` has one writer (jit-core: none; jit-tasks: the one transition function). Check: `grep -rn 'set_stack_limit(' crates/eml_nrt/src` hits the thread-start site and, from jit-tasks, one function in `limits.rs`.
- **N-43** MUST: the observable differences between a native run and a VM fast-mode run of the same program are exactly those listed in docs/spec/native.md 「VMとの違い」, each with its reason: (1) no word-count depth limit (`max-depth`); the limit is the thread's stack (jit-core); (2) an `Async.run` that would finish under a nested `Async.run` on the same worker waits for the inner one, and a run that can only proceed by that inversion ends with `RunFault::BuriedRun` instead of continuing; nested entries share the thread's depth budget (jit-tasks; also stated in stdlib.md 「`Std.Async`」); (3) `--debug-heap`, `--stats` and the `Seeded` policy do not exist natively. A stage that needs another difference adds it to its plan as a user-visible semantics item, and the same commit writes the section. MUST NOT: a difference that shows as a hang or as a wrong result; a `-- native: skip` or `# native: skip` reason that names no listed difference. Applies: D, C jit-core on. Check: each mark's reason against the section (review at plan approval); the section exists from jit-core.

## 9. Revisions of the existing codebase, by stage

| # | Revision | Stage | Cost | Status |
|---|---|---|---|---|
| R-1 | eml_rt: `#[repr(C)] Ctx<M>`, `Native` first field (six fields), `heap` second, consts, T18 | jit-rt | ~40 mechanical renames in 10 files; no proof body edited; one trust row, `trust.rs` entry, one `trusted::` test | proof cost [V: 7.1]; bench deltas [D: S2a r1 C-1]; the 5-pair run with N-29's exception is in the task |
| R-2 | eml_rt: `Meta` / `Word` / `rc` visibility; box value word in `layout`, eml_rt and eml_vm `compile/constants.rs` rerouted; `Ctx::bytes_empty` infallible | jit-rt | as rt-layout scoped; one `#[verifier::rlimit(20)]` on `lemma_moves_parent` | [D: S2a 7.3] |
| R-3 | eml_rt: fault parking, accessors for uninterpreted fields | jit-rt | new safe functions | [I] |
| R-4 | eml_vm -> eml_sched: `cells.rs`, error types, `RunConfig` / `Finished` / `RunStats` / `Clock` / `DriverKind`, `Park` | sched-split | file moves, `pub(crate)` -> `pub` on what eml_vm uses, import edits, re-exports; the dev-only `testing` feature for 54 fixture uses (N-41); `RunStats` loses `#[non_exhaustive]`; tests byte-identical | [D: S2a 7.2, probed] |
| R-5 | Harness: engine parameter; `ui::run_native`, `ui::run_fail_native`; mark; snapshot; census; nextest overrides | jit-core | harness code; marked files kind 1 | [I] |
| R-6 | eml_cli: `Session` native check (E0004), `run --native` | jit-core | one method, clap flag and conflicts | [I] |
| R-7 | eml_vm: `run_extern` arms and dedicated op handlers call the shared row functions of `extern_rows!`; `tls.rs` / `gui.rs` to eml_sched; feature forwarding | jit-externs (the list and the seed rows' functions: jit-core) | the ~600-line `match` replaced; `RunStats` snapshots byte-identical | [I] |
| R-8 | eml_rt + eml_vm: evidence roles on the `Ctx` surface; one fork-evidence builder | jit-effects | evidence lemmas restated (not measured); `vm.rs` / `compile/forks.rs` edits | [R] |
| R-9 | eml_vm -> eml_sched: worker table, inboxes, monitor, jobs; `Exec` trait; the `testing` feature deleted | jit-tasks | VM refactor behind `Exec`, byte-identical; no eml_rt edit expected | [I] |
| R-10 | eml_rt: `Heap::is_immortal`, `Fault::Engine` (the VM maps it to an internal error) | jit-core | two small additions under the eml_rt gates | `is_immortal` [D: S2b 9.3]; `Fault::Engine` [I] |
| R-11 | eml_rt: `CTX_YIELD_ABORT` | jit-effects | one const with its const asserts | [I] |
| R-12 | Fault enum `VmFault` -> `RunFault` | jit-core | 400 textual occurrences in 33 files, none in a snapshot (kind 3) | [D: S2b 9.4] |

Considered and not chosen: an infallible fast-mode heap API (second spelling to verify); source spans on `LirFn` for the E0004 anchor (IR text snapshots change for a diagnostic that disappears); replacing the VM's `depth_limit` by the stack fields (different quantities); a fast-mode live counter (work on every alloc / free; the walk exists).
