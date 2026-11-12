# Appendix 03: jit-core and jit-apply (executable design)

(Where this file and 00-decisions.md differ, 00-decisions.md wins.)

Section C of 00-decisions.md is folded into this text; 00 remains the record. Task numbers here are this appendix's own; the stage plans (docs/plans/2026-11-11-jit-core.md, -jit-apply.md) map them in their 「付録03」 column, and where a plan's task text differs from this file (fixtures, checkpoints, task splits), the plan wins.

# S2b response (r4): executable design and plan for jit-core and jit-apply

The deliverable in full, written for an implementer who has neither the contract document nor the planning directory.

**Changes from r3** (the conformance resolutions applied; everything else is r3's text):
1. **No program-wide number in generated code** (D-5, D-6, D-9, D-13, D-17, D-19; K6, K7, K10, K11, K13, K15, A1, A2). Constants and other load-time data are per-name data symbols (`s:<text>`, `i:<n>`, `x:<bits>`, `v:<function>`), not slots of one table; a fault site is the pair (the function's own code address, a site index local to the function); the safepoint passes the function's own code address. `Image` has no global site table and no data table; `FnRow` carries its sites and its static-closure slot. Hot-path costs are unchanged.
2. `extern_rows!` lines name each entry identifier (`fn V as eml_x_V(..)`), kind tokens go straight to `entries!`, inline rows are `inline fn` lines listed from K9 on; a prototype of this grammar compiles [V 9.9].
3. Fatal codes: 1 and 2 are jit-rt's (`FATAL_PANICKED`, `FATAL_FAULT`); jit-core adds `FATAL_RECORDED = 3`; `impl Raise for HeapError` is jit-rt's and is not redefined; the driver reads all three.
4. A `never` row's entry returns normally with no result (`eml_x_Exit`, `eml_x_Panic` return nothing).
5. `eml run --native` with `--workers` other than 1 or with `--gui` is a usage error until jit-tasks.
6. eml_native names the run-interface types through eml_nrt's re-exports; `eml_rt` is named in eml_native only in `mem.rs`.
7. The stack-geometry consts are defined in `crates/eml_nrt/src/run.rs` only.
8. A5 no longer edits docs/future/native.md L41 (the doc-revision task of jit-rt's plan does).
9. The stack tests of K6 and K13 run on an explicit 64 MiB worker stack.

---

## 0. What the two stages inherit (contract rules, inlined)

The stages run after `jit-rt` and `sched-split`. These rules are fixed by the long-term contract and by those two stages; the plan below relies on them and must not change them.

**Crates.** `eml_rt` (verified runtime core), `eml_sched` (engine-neutral run types: `RuntimeError`, the fault enum, `SourceLocation`, `RunConfig`, `Finished`, `RunStats`, `Clock`, `Park`, `cells`), `eml_nrt` (native runtime: every function generated code calls, the asm entry stubs, the run driver), `eml_native` (Cranelift code generator and JIT host), `eml_vm` (the VM). Allowed edges: `eml_native -> eml_nrt, eml_lir, eml_rt, eml_extern, eml_cabi, cranelift-*`; `eml_nrt -> eml_sched, eml_rt, eml_extern`; `eml_vm -> eml_sched`. Forbidden: `eml_vm -> cranelift / eml_native / eml_nrt`; `eml_nrt -> cranelift / eml_lir / eml_core_ir / eml_vm / eml_native`; `eml_native -> eml_vm`; `eml_sched -> cranelift / eml_lir / eml_vm / eml_native / eml_nrt`.

**What jit-rt provides** (names as in its plan):
- `eml_rt`: `Ctx<M>` is `#[repr(C)]` with the non-generic `#[repr(C)] Native` as its first field and `heap` right after it (`yielding: YieldState { target, clause, cont, abort }`, `fault: u64` (non-zero iff a failure is parked: a `Fault`, or from jit-core `Fault::Engine` with the payload held by the engine), `stop: *const AtomicU32`, `stack_limit: usize`, `jmp`, `runtime`). Offset consts next to `Ctx` (`offset_of!`-derived, each <= 120 and a multiple of 8): `CTX_YIELD_TARGET`, `CTX_FAULT`, `CTX_STOP`, `CTX_STACK_LIMIT`, `CTX_JMP`. Accessors: `set_fault(Fault)`, `take_fault() -> Option<Fault>`, `stack_limit()`, `set_stack_limit(usize)`, `runtime() -> *mut ()`, `set_runtime(*mut ())`. Exports: `Meta::{TAG_MASK, SCAN_SHIFT, SCAN_MASK, KIND_SHIFT, KIND_MASK, LIN}`, `Word::{IMM_TAG, IMM_SHIFT, UNIT, EMPTY}`, `pub mod rc` with `DUP_FAST_BIAS`, `DUP_FAST_SPAN`, `DECREF_FAST_BIAS`, `DECREF_FAST_SPAN` (inline rule: `(rc as u32).wrapping_sub(BIAS) < SPAN` => store `rc ± 1`), `layout::BOX_VALUE_WORD`, and the existing `layout::{RC_BYTE, META_BYTE, slot_byte_offset, data_first, data_raw_first, data_meta, INLINE_SCAN_LIMIT}`.
- `eml_nrt`: `entries!` macro. `pub const NAME;` opens a table; `pub plain fn sym(ctx, a: i64, ..) -> R { body }` has the raw `ctx: *mut Ctx<Fast>`; `pub shim fn sym(ctx, a: i64, b: f64) -> R { body }` runs `body: Result<R, E: Raise>` inside `catch_unwind` with `ctx: &mut Ctx<Fast>`; parameter and result types are `i64`, `f64`, the row-kind tokens (`obj`, `tobj`, `unit`, `int`, `enum` -> `i64`; `float` -> `f64`), nothing, `never` (a result only: the Rust function returns `()` and the descriptor has no result), or `!`. An entry's symbol is its function identifier. A table is appended to `TABLES` in `lib.rs`; `eml_nrt::entries()` iterates `Entry { symbol, address, params: &[Kind], result: Option<Kind> }`, `Kind::{I64, F64}`. `pub trait Raise { fn park(self, ctx: &mut Ctx<Fast>) -> Fatal; }` with impls for `Fault` (`set_fault`, `FATAL_FAULT`), `HeapError` (as `Fault::Heap`) and `Fatal`; `Fatal { code: u32, site: u32 }`; `FATAL_PANICKED = 1` (a caught panic, no payload), `FATAL_FAULT = 2` (`ctx.take_fault()` holds the fault). `eml_nrt::enter(ctx, thunk, entry, argv) -> Result<(), Fatal>` (wrapper of the stub `eml_enter`), `eml_fatal(ctx, code, site) -> !` (site 0 = none), `detached(&Ctx) -> bool`, `SUPPORTED: bool`.
- `eml_native`: `src/mem.rs` (holds only the const assert `Shape::REUSE_SCANNED_LIMIT <= layout::INLINE_SCAN_LIMIT`), `src/abi.rs` (`isa(pic) -> Option<OwnedTargetIsa>`, the one flag constructor for eml code: `opt_level=speed`, `preserve_frame_pointers=true`, inline stack probes; `bridge.rs` keeps its own for the C bridges; `clif(Kind) -> Type`; `tail_signature(params: &[Kind], rets: &[Kind]) -> Signature`, the only `CallConv::Tail`; `declare_entry(module, &Entry) -> Built<FuncId>`, the only `Linkage::Import`), `src/thunk.rs` (`define_thunk(module, name, params, rets)`: a default-cc `(ctx, entry, argv)` function that loads the arguments from `argv`, `call_indirect`s the `tail` function `entry`, stores the results to `argv`).

**Rules.**
- *Entry ABI*: every function generated code calls outside its module is declared by `entries!`; C default convention; `ctx` first; other parameters `i64` / `f64`; at most one `i64` / `f64` result.
- *Failure protocols by prefix*: `eml_rt_*` (thin shims over the eml_rt heap / `Ctx` API) are fatal-only: an `Err` or a caught panic ends in `eml_fatal`; generated code checks nothing after them. `eml_x_<Variant>` (one per extern row): a row with `faults` parks `RunFault::Extern(f)` with `ctx.set_fault(f)` and returns the placeholder, and generated code tests `ctx.native.fault` right after; a failure whose payload the engine holds (a panic message word) is parked as the payload in the worker struct plus `set_fault(Fault::Engine)`; every other `Err` (any `Err` of a row without `faults`, a `RunFault` that is not `Extern`) and a caught panic are the internal class and end in the shim's fatal path, the error being recorded before the `Err` reaches the shim; the wrapper body never names `eml_fatal`. Rows without `faults` are plain calls. `eml_nrt_*` (services) return an action word that generated code branches on, or a plain data word when their only failure path is the fatal path.
- *Shim body rule*: `eml_fatal` is called only after the `catch_unwind` closure has returned (the macro's `shim` arm writes this once).
- *Fatal rule*: a fatal error longjmps to the innermost `eml_enter`; it crosses only `tail` frames, thunks, and the Rust frames of the one entry that detected it; those frames hold no value with a destructor at that point. So an error value is recorded (moved into the worker struct) **before** the `Err` is returned to the shim.
- *Re-entry rule*: Rust code that enters eml code holds no `&Ctx` / `&mut Ctx` across `enter`.
- *Generated code*: every eml function is `tail` cc `(ctx, leaves..) -> leaves..`; tail calls are `return_call` / `return_call_indirect`; a direct call of a known function, adapter or helper is a `call` / `return_call` of a function declared `Linkage::Local` (Cranelift marks it colocated: PC-relative, no address load), and `call_indirect` appears only where the callee is a run-time value; all eml code is compiled with the flags of `abi::isa`; external references are module symbols or module data, never a process address as an immediate; `JITModule` is named only in the JIT host file and `bridge.rs`; object memory and `Ctx` are touched only through `mem.rs`; `eml_rt` is named in eml_native only in `mem.rs` (consts and const fns); the run-interface types the JIT host's API names (`RunConfig`, `Finished`, `RuntimeError`, `RunFault`, `HostRef`) come through eml_nrt's re-exports, so eml_native has no edge to eml_sched; a function's code depends only on its own LIR and the names it refers to: generated code embeds no number that depends on the rest of the program or on a load order (function ids, global site numbers, table indices); no thread-locals in eml_nrt / eml_native; native code instantiates only `Ctx<Fast>`.
- *Differences from the VM*: the observable differences between a native run and a VM fast-mode run are exactly those listed in docs/spec/native.md 「VMとの違い」, each with its reason; a `-- native: skip` reason names one of them.
- *What later stages may change*: bodies of `eml$raise`, of the services, of `after_unwind_point`, entry signatures and `Image` fields (by listing the change in their plan). Not the items above.

---

## 1. Conclusion

### 1.0 Numbers (classification facts and expectations)

Facts [V 9.1], produced by the rules of D-12 applied to every UI entry at 432fffb4; from task K5 on the committed tests `ui::run_native` / `ui::run_fail_native` reproduce them (`cargo nextest run -p eml_cli --test integration -E 'test(=ui::run_native) | test(=ui::run_fail_native)'`):

| | jit-core | jit-apply |
|---|---|---|
| `tests/ui/run` (635): accepted / skipped / rejected | 174 / 6 / 455 | 208 / 6 / 421 |
| run histogram (first form) | async row 128, c row 2, closure 19, fn value 15, par 22, prim 156, std row 113 | async row 128, c row 2, par 22, prim 156, std row 113 |
| `tests/ui/run-fail` (81): accepted / skipped / rejected | 7 / 2 / 72 | 8 / 2 / 71 |
| run-fail histogram | async row 33, c row 2, closure 1, par 1, prim 3, std row 32 | async row 33, c row 2, par 1, prim 3, std row 32 |
| `bench/*.em` (42) accepted | 25 | 27 |

"Accepted" is a classification; that every accepted file runs natively with VM-identical output is the **expectation** the stage works to (not yet measured). Completion bar: accepted-and-identical >= 150 (jit-core) and >= 185 (jit-apply); the snapshot records the actual numbers. A file that is accepted but cannot be made identical for a reason that is not a lowering bug gets `-- native: skip (<reason>)` by a plan revision (kind 1), which lowers `accepted` by one; more than 10 such files in a stage goes back to the user.

### 1.1 jit-core decisions

**D-1 Module structure of `eml_native`.**
```
crates/eml_native/src/
  lib.rs
  bridge.rs error.rs library.rs linker.rs trust.rs     (existing)
  abi.rs  thunk.rs                                      (jit-rt; unchanged)
  mem.rs            the memory-access module (jit-rt created it; D-7 fills it)
  unsupported.rs    Form, Unsupported, unsupported(&Program)                    (D-12)
  jit.rs            the JIT host file: NativeMachine, execute, symbol table     (D-13, D-18)
  lower/
    mod.rs          program<M: Module>(..): declare all, define all, helpers; signature_of(&LirFn)
    names.rs        module symbol names (D-3)
    function.rs     FnLower: blocks, block params, statements, terminators, atoms, drop (K14 Inline RC)
    call.rs         direct call, LetMany, tail call
    unwind.rs       after_unwind_point (D-4), cleanup blocks, placeholders, eml$raise, the function's site list
    safepoint.rs    entry safepoint (D-5)
    rc.rs           inline dup / decref
    data.rs         con, reuse, unpack, case fields, switch, box, unbox, UnboxStruct, release, tokens (D-8)
    consts.rs       per-name load-time data symbols (D-9)
    rows.rs         INLINE and CALLED row lists, inline expanders, call lowering (D-10)
    apply.rs        (jit-apply) closures, adapters, apply, eml$apply$m, eml$pap$m$r
```
Final shared names (agreed with jit-rt's plan): the memory-access module is `src/mem.rs`; the `tail` signature helper and the import function are `abi::tail_signature` and `abi::declare_entry`; no `lower/sig.rs`, `lower/entries.rs` or `lower/mem.rs`. Rejected: one `emit.rs` (the grep rules would need exceptions); a trait per construct (no second implementor).

**D-2 Values.** LIR is SSA with block parameters (docs/spec/lir.md R5, R6 [V]). Each `VarId` maps to one CLIF `Value` (`Vec<Option<Value>>`); LIR block parameters are CLIF block parameters; no `cranelift_frontend::Variable`; `seal_all_blocks()` once. CLIF type by Repr: `Float` -> `F64`; `Obj`, `TObj`, `Unit`, `Int`, `Enum`, `Token` -> `I64`; `unit` is the word `Word::UNIT`. Atoms are materialised per use by the Repr of the position (the call sites of VM `compile/emit.rs` L203-256 [V]): `Int(n)` -> `iconst`; `Float(bits)` -> `f64const`; `Unit` -> `iconst UNIT`; `Tag(n)` -> `iconst n` at an `enum` position, `iconst mem::imm_bits(n)` at `obj` / `tobj`; `Fn(f)` -> jit-apply.

**D-3 Functions, calls, names.**
- Every LIR function: `abi::tail_signature(kinds of params, kinds of rets)` through `lower::signature_of`. `Call::Direct` non-tail -> `call`; `LetMany` -> `call`, all results bound; `Term::TailCall(Direct)` -> `return_call` (a self tail call re-enters the entry and its safepoint); `Return` / `ReturnMany` -> `return_`. LIR-compatible result Reprs have equal CLIF types [I].
- Module symbol names (`names.rs`): LIR function `f:<name>`; closure adapter `c<k>:<name>`; `eml$raise`; data symbols `s:<text>`, `i:<n>`, `x:<bits>` (D-9); jit-apply `v:<name>`, `eml$apply$<m>`, `eml$pap$<m>$<r>`. LIR names may hold any character; the kind prefix ends at the first `:`. All `Linkage::Local` (final linkage: direct calls are colocated). The names depend only on LIR names and shapes, never on a load order or an address; the same strings are the profiler's symbol names (D-18).
- The run entry (`Program.entry`) must have exactly one result leaf that is not `float`; else `LoadError::Entry`. All 708 unmarked UI entries return `unit` [V 9.1].
- One `load` per `NativeMachine` in these stages (a second call returns `LoadError::Loaded`); appending by name is jit-repl's (per-name data symbols need no growth rule: a new load declares only the symbols that are new).

**D-4 The "after unwind point" function** (`lower/unwind.rs`; later stages extend this function and nothing around it):
```rust
pub(super) enum After { Call, Faults { site: u32, purity: Purity }, Arith { site: u32 }, Never { raise: Option<(u32, Purity)> } }   // site: index in the function's own site list
pub(super) struct Point<'a> { pub owned: &'a [(VarId, u32)], pub tokens: &'a [VarId], pub landing: Option<VarId> }
/// Emitted right after the instruction of an unwind point; leaves the builder in the hot continuation.
pub(super) fn after_unwind_point(fx: &mut FnLower, point: &Point, after: After);
```
| `After` | Used for | Hot code | Cold code (`set_cold_block`) |
|---|---|---|---|
| `Call` | non-tail `Rhs::Call` / `LetMany` | `t = mem::yield_target(ctx); brif (t != UNIT) cleanup, hot` | `cleanup` |
| `Faults` | called row with `faults && ret != Never` | `f = mem::fault(ctx); brif f raise, hot` | `raise: call eml$raise(ctx, self, site, purity); jump cleanup` |
| `Arith` | inline faulting row | none here; the expander branches to `arith(kind)` | `arith(kind): call eml_rt_fault_arith(ctx, kind); call eml$raise(ctx, self, site, Panics); jump cleanup` |
| `Never` | `exit` (`raise: None`), `panic` (`Some`) | unconditional | `[call eml$raise(ctx, self, site, purity);] jump cleanup`; the builder then switches to a fresh unreachable block and binds the instruction's variable to its placeholder so the dead LIR continuation lowers |

`self` is `func_addr` of the function being lowered (a relocated symbol, materialised only in cold blocks); `site` is the index of the statement in the function's own site list (D-6).
`cleanup` (one per point): for each `(v, n)` in `owned`, `n` times `call eml_rt_decref(ctx, v)`; for each token `call eml_rt_drop_token(ctx, t)`; `return_` placeholders for the function's `rets` (`Float` -> `0.0`; `Obj` / `TObj` / `Unit` -> `UNIT`; `Int` / `Enum` / `Token` -> `0`). `landing` is asserted `None`. Points come from `eml_lir::unwind::points(program, |_| true)` (unwind.rs L44-49 [V]); an `Err` entry makes lowering panic. The `fault` load exists only in `Faults`, the `yielding.target` load only in `Call`, no load in `Never`.

**D-5 Entry safepoint** (`lower/safepoint.rs`): first code of every LIR function (adapters and the `eml$..` helpers are trampolines ending in a tail call to a function that has one):
```
entry:  bits = mem::stop_bits(ctx)          // load the stop-word pointer, load.i32 the word
        c = bor (icmp ne bits, 0), (icmp ult get_stack_pointer, mem::stack_limit(ctx));  brif c, slow, body
slow:   (cold)  a = call eml_nrt_safepoint(ctx, self, is_ep_helper);  brif (a == ACTION_CONTINUE) body, leave
leave:  (cold)  cleanup of Point { owned: Unwinds.entry, tokens: [], landing: None }
```
`self` is the function's own code address (`func_addr`); the service finds the `FnRow` whose `[code, code + size)` holds it (a sorted index built once per run; cold path only). `is_ep_helper = eml_lir::ep_helper(name)`. jit-core's `eml_nrt_safepoint`: `ctx.poll(0)`; `Err(Stop::Notice)` -> `control.take_notice(0)`, poll again; any other stop -> record `Fault { RunFault::Stopped(stop), function, at: None }`, fatal; no stop bit and the service's own frame address below `ctx.stack_limit()` -> the same with `Stop::DepthLimit`; else `ACTION_CONTINUE`. It returns no other action in jit-core, so `leave` is emitted and not executed. Stop priority is `eml_rt`'s (`Ctx::poll` -> `stop_reason`); the VM analogue is `polled_stop`, vm.rs L525-560 [V].
**Stack geometry** (the consts `WORKER_STACK`, `STACK_HARD_MARGIN`, `LIMIT_HEADROOM_BYTES` are defined once, in `crates/eml_nrt/src/run.rs`): with `base` the lowest address of the worker's stack (address of a local at thread start minus the spawn size), `hard = base + STACK_HARD_MARGIN` (1 MiB) and `real = hard + LIMIT_HEADROOM_BYTES` (2 MiB, the VM's `LIMIT_HEADROOM` of 2^18 words in bytes). jit-core sets `stack_limit = real` once at thread start (the only `set_stack_limit` call in eml_nrt) and never changes it; `hard` is used from jit-tasks on (a task aborting after a limit). Below `real` there are always 3 MiB for one eml frame and any runtime entry.
**Limits natively**: `Limits.max_depth` is not read (its unit is VM stack words); the depth limit is the worker stack, 1 GiB - 3 MiB of native frames; `max_bytes` / `soft_bytes` act through the allocator and the stop word as in the VM. This is entry (1) of spec/native.md 「VMとの違い」 ("no word-count depth limit; the limit is the thread's stack"); entry (3) is "`--debug-heap`, `--stats` and the `Seeded` policy do not exist natively". Both are written by jit-core and repeated in status.md.

**D-6 Fault path.**
- Sites are local to a function: `FnRow.sites: [SiteRow { file, line, column }]`, one per `Rhs::Extern` of that function lowered with `Faults`, `Arith` or `Never { raise: Some }`, in statement order (so the index depends only on the function's LIR); `file = NO_FILE` when the statement has no `at`. Generated code identifies a site as `(self, site)`; no global site number exists. `Fatal.site` is always 0 in these stages (the error is recorded with its position before the fatal).
- `eml$raise`: one module-level `tail` function `(ctx, function, site, purity)`, no result; jit-core body `call eml_nrt_failed(ctx, function, site, purity); return`.
- `eml_nrt_failed(ctx, function, site, purity) -> action`, jit-core body: `fault = ctx.take_fault()`; `Some(Fault::Engine)` -> take the payload from the worker struct (`worker.panic`): `text = ctx.copy_string(message, usize::MAX)`, fatal with `RunFault::Panicked(text)`; `Some(other)` -> fatal with `RunFault::Extern(other)`; `None` -> fatal with `RunFault::Internal("a failure was raised with nothing parked")`; `function` and `at` from the site. No handler can exist in jit-core (every `Rhs::Prim` is rejected), so the VM's `failed` (vm.rs L4083-4115 [V]) reduces to its uncaught arm.
- Recording: `eml_nrt::worker::record(ctx: &mut Ctx<Fast>, fault: RunFault, at: Option<At>) -> Fatal` (`At { function: usize /* a code address */, site: Option<u32> }`) builds the `RuntimeError::Fault` (function name from the `FnRow`, position from its site; `None` -> `function = "<native code>"`, no position), moves it into the worker struct, returns `Fatal { code: FATAL_RECORDED (= 3), site: 0 }` (`FATAL_RECORDED` is defined in `crates/eml_nrt/src/enter.rs` next to codes 1 and 2; C-5). Services are `shim` entries whose error type is `Fatal`, so the macro performs the `eml_fatal`. The driver reads a `Fatal` by its code: `FATAL_RECORDED` -> the recorded error; `FATAL_FAULT` (a heap entry's `Err`, parked by jit-rt's `Raise` impls) -> `RunFault::Extern(ctx.take_fault())` with `function = "<native code>"`; `FATAL_PANICKED` -> `RunFault::Internal("a runtime entry panicked")`.
- `exit`: `eml_x_Exit(ctx, code)`: `worker.exit = Some(code)`, `Ctx::exit_abort()`; a `never` entry: it returns normally with no result; lowered with `After::Never { raise: None }`.
- `panic`: `eml_x_Panic(ctx, message)`: `eml_nrt::worker::park_panic(ctx, Word)` stores the message word in `worker.panic` and calls `ctx.set_fault(Fault::Engine)` (a later `try_panic` clause receives the message as a word, so the payload is engine-held; the marker keeps "`fault != 0` iff a failure is parked" true); a `never` entry, no result; lowered with `After::Never { raise: Some(site, row purity) }`. `park_panic` is the interface jit-externs' C-row panics and jit-tasks reuse.
- `Fault::Engine` is a new unit variant of `eml_rt::Fault` (task K3): "the engine holds the payload of this failure". `Fault::panics()` is false and `io_error_kind()` is `None` for it; its `Display` is `internal error: a failure whose payload the engine holds`; the VM never produces it, and `RuntimeError::is_broken` counts `RunFault::Extern(Fault::Engine)` (an internal error). `Fault` stays 24 bytes.

**D-7 Memory access and inline RC** (`src/mem.rs`, `lower/rc.rs`). `mem.rs` exports only functions that take the builder, plus consts computed from eml_rt's; no other file of `eml_native/src` writes `ins().load` / `store` (thunk.rs, bridge.rs excepted) or names `eml_rt` (C-9: `jit.rs` names no `eml_rt` item; it uses eml_nrt's re-exports):
```rust
pub const UNIT: i64; pub const EMPTY_TOKEN: i64; pub const fn imm_bits(n: i64) -> i64;
pub fn is_imm(b, w) -> Value; pub fn imm_value(b, w) -> Value; pub fn make_imm(b, v) -> Value;
pub fn imm_round_trips(b, v) -> Value;                       // (v << IMM_SHIFT) >>s IMM_SHIFT == v
pub fn load_rc(b, obj) -> Value /*I32*/; pub fn store_rc(b, obj, rc);
pub fn dup_is_local(b, rc) -> Value; pub fn decref_is_local(b, rc) -> Value; pub fn is_one_ref(b, rc) -> Value;
pub fn load_tag(b, obj) -> Value; pub fn is_big_int(b, obj) -> Value;
pub fn token_of_box(b, obj) -> Value; pub fn box_of_token(b, tok) -> Value; // fast-mode token = the box word (object-model.md 「検査モードの token」); both are the identity
pub fn load_field(b, obj, scanned: u32, position: u32, ty) -> Value; pub fn store_field(b, obj, scanned: u32, position: u32, v);
pub fn load_box(b, obj, ty) -> Value;
pub fn yield_target(b, ctx) -> Value; pub fn fault(b, ctx) -> Value; pub fn stop_bits(b, ctx) -> Value; pub fn stack_limit(b, ctx) -> Value;
pub fn data_word(b, slot: GlobalValue, ty) -> Value;         // load of a per-name data symbol
pub fn spill(b, values) -> Value;                            // (jit-apply) explicit stack slot as argv
pub fn closure_code(b, clo) -> Value;                        // (jit-apply)
```
Inline `dup(w)` (the immediate test only for a `tobj` word; an `obj` word is always a pointer): `brif is_imm(w) done; rc = load_rc; brif dup_is_local(rc) fast, other; fast: store_rc(rc + 1); other: brif rc slow, done; slow: call eml_rt_dup_slow(ctx, w)`. Inline `decref(w)`: the same with `decref_is_local` (`rc` in `2..LOCAL_MAX`), `store_rc(rc - 1)`, slow call `eml_rt_decref` (rc 1, shared, stuck). `brif rc` is the immortal test (rc 0 means "not counted", `eml_rt::rc`'s `dup_spec` / `decref_spec`); it is a zero test, so no rc number is written, and it keeps constants and static closures free of calls. `is_one_ref` is `rc == 1`.

**D-8 Data lowering.** Positions come from `LayoutCtor::shape()` (eml_lir lib.rs L800-822 [V]) through `mem::load_field` / `store_field`.

| LIR | Lowering |
|---|---|
| `Rhs::ConstString(i)` | `mem::data_word` of `Datum::String` |
| `Rhs::Box(int var)` | `brif mem::imm_round_trips(v) imm, big; imm: mem::make_imm(v); big: call eml_rt_alloc_int(ctx, v)` (`Heap::alloc_int`'s rule, heap/mod.rs L1669 [V via R3]) |
| `Rhs::Box(float var)` / `Box(enum var)` | `call eml_rt_alloc_float(ctx, v)` / `mem::make_imm` |
| `Rhs::Box(Int n)` | fits 63 bits: `iconst imm_bits(n)`; else `data_word` of `Datum::BigInt(n)` |
| `Rhs::Box(Float x)` / `Box(Tag n)` | `data_word` of `Datum::Float(bits)` / `iconst imm_bits(n)` |
| `Rhs::Unbox` to `int` / `enum` | `brif is_imm(w) a, b; a: imm_value; b: load_box(I64)`; to `float`: `load_box(F64)` (the dummy reads 0, data.rs L413-437 [V]) |
| `Rhs::Con` | `w = call eml_rt_alloc_data(ctx, tag, scanned, raw, lin)`; `store_field` for every field (`lin` = the bound variable's mark is `Lin`, emit.rs L560 [V]) |
| `Rhs::Con` with `reuse` | `brif (tok == EMPTY_TOKEN) fresh, reuse; fresh:` as `Con`; `reuse: w = mem::box_of_token(tok)` if `in_place` (no call, no header write), else `call eml_rt_con_at(ctx, tok, tag, scanned, raw, lin)`; store the fields not marked `skip` (data.rs L371-405 [V]) |
| `Rhs::Drop(a)` | RC variable: inline `decref`; result `UNIT` |
| `Stmt::Dup` / `Decref` | inline |
| `Stmt::Unpack`, case fields | `load_field` per bound field |
| `Stmt::UnboxStruct` | `brif mem::is_big_int(w) dummy, real; dummy:` RC leaves = `w`, raw leaves = 0 / `0.0`; `real:` `load_field`s (data.rs `unbox_struct` [V]) |
| `Stmt::Release` | `rc = load_rc(x); brif is_one_ref(rc) last, ask; ask: l = call eml_rt_is_last(ctx, x); brif l last, shared; last:` inline `decref` of every scanned field whose entry is `None`, `call eml_rt_free_block(ctx, x)`; `shared:` inline `dup` of every kept RC field variable, inline `decref(x)` (data.rs L245-286 [V]). The `fields` name variables already bound by `unpack` / case fields (lir.md R5) |
| `Stmt::Release` with `token` | `brif is_one_ref(rc) unique, other; unique:` drop the non-kept children, `tok = mem::token_of_box(x)` (no call); `other:` the `ask` path, `tok = EMPTY_TOKEN` (data.rs L322-355 [V]) |
| `Release { unboxed: true }` | first `brif mem::is_big_int(x) skip, go`; `skip:` the token (if any) = `EMPTY_TOKEN` |
| `Stmt::DropToken` | `call eml_rt_drop_token(ctx, tok)` |
| `Term::Switch` on `enum` / `obj` / `tobj` | tag = the value / `mem::load_tag` / `is_imm ? imm_value : load_tag`; `cranelift_frontend::Switch`; default = the LIR default or a `trap` block; case fields load at the head of the target |
| `Term::Switch` on `int` / strings | `Switch` over the constants / linear `r = call eml_x_StrEq(ctx, s, data_word(<the case's string>)); brif r case, next` |
| constant scrutinee | `jump` chosen at compile time (emit.rs L928-948) |
| `Term::Jump` | `jump target(args)` |
| `Rhs::Prim`, `MakeClosure`, `Atom::Fn`, `Call::Apply / Clause / Par` | `unreachable!` (rejected, D-12) |

**D-9 Load-time data.** Every load-time value a function needs is its own module data object: writable, zero-initialised, 8 bytes, `Linkage::Local`, named by its content only: a string constant `s:<text>`, a boxed big `int` `i:<decimal>`, a boxed `float` `x:<16 hex digits of the bits>`. Generated code reads `mem::data_word(symbol_value(<symbol>), ty)`: one relocated address and one load [V 9.2 for the mechanism]. `consts.rs` declares a symbol on first use (the module's name table dedups equal content) and records `Datum -> DataId`; after `finalize_definitions` the JIT host puts each slot's address in `Image.data: [DatumRow { kind, bits, text, slot: *mut u64 }]`, and eml_nrt fills `*slot` before the first entry. So a function's code names what it uses and embeds no index. The objects are immortals in a frozen region, built as the VM builds them (constants.rs L51-139 [V]); the two immortal box makers move from eml_vm `compile/constants.rs` L86-116 to `eml_sched::constants` so both engines use one implementation. Later kinds follow the same rule: jit-apply `v:<function name>` (the function's static closure, D-19); jit-externs the empty `MutArray`; jit-effects `e:<effect name>` (the slot holds the machine number as a raw integer).
Rejected: one table indexed by a per-program number (the index is a load-order number inside the code); object addresses as immediates (a process address in code).

**D-10 Rows.** `rows.rs` holds two lists and `lowering(ext)`:
- `INLINE: &[(Extern, Inline)]`, 45 rows:

| `Inline` | Rows | CLIF |
|---|---|---|
| `Add` / `Sub` / `Mul` | `IntAdd`, `IntSub`, `IntMul` | `sadd_overflow` / `ssub_overflow` / `smul_overflow`; overflow -> `arith(INTEGER_OVERFLOW)` [V 9.2 aarch64] |
| `Div` / `Rem` | `IntDiv`, `ByteDiv` / `IntMod`, `ByteMod` | `b == 0` -> `arith(DIVISION_BY_ZERO)`; `a == MIN && b == -1` -> `arith(INTEGER_OVERFLOW)`; `sdiv` / `srem` (externs.rs L688-721 [V]) |
| `Neg` | `IntNeg` | `a == MIN` -> overflow; `ineg` |
| `Cmp(IntCC)` | `Int` / `Char` / `Byte` x `Eq Ne Lt Le Gt Ge` (18) | `icmp`, `uextend` |
| `Compare` | `IntCompare`, `CharCompare`, `ByteCompare` | `(a > b) + (a >= b)` |
| `Move` | `CharToInt`, `ByteToInt`, `CharOfByte` | the argument |
| `FAdd` / `FSub` / `FMul` / `FDiv` / `FNeg` | `FloatAdd`..`FloatDiv`, `FloatNeg` | the op, then `select(fcmp uno r r, f64const NAN, r)` (eml_rt `canonical_nan`, externs.rs L933-950 [V]; [V 9.2]) |
| `FCmp(FloatCC)` | `FloatEq Ne Lt Le Gt Ge` | `fcmp` (`Ne` = `NotEqual`), `uextend` |
| `BoolEq` / `BoolNe` | same | `icmp` |

  This is VM `arithmetic()` (emit.rs L1064-1103 [V]) minus `FloatMod` (section 3).
- `CALLED: &[Extern]`, 11 rows in jit-core: `Println`, `Eprintln`, `IntShow`, `StrShow`, `FloatShow`, `StrConcat`, `StrEq`, `StrCompare`, `FloatMod`, `Exit`, `Panic`. A called row lowers as: arguments by the row's parameter Reprs, `call eml_x_<Variant>` (`format!("eml_x_{ext:?}")`), then `After::Faults` iff `row.faults && ret != Never`; `After::Never` for a `Never` row; nothing otherwise. A test ties `CALLED` to eml_nrt's table (every called row has an entry whose descriptor equals the row's Reprs).
- **Wrappers** use the one row-list mechanism of the stage `jit-externs`, introduced here with the seed rows: `extern_rows!` in `crates/eml_sched/src/rows.rs` lists the 9 engine-neutral rows (all but `Exit`, `Panic`) with kinds and body, in the grammar jit-externs' plan owns (line form `fn V as eml_x_V(ctx, a: kind, ..) -> kind { body }`, multi-variant `fn A as eml_x_A | B as eml_x_B (..)`, an `inline fn V(..)` line for a row native lowers inline; kinds `obj | tobj | unit | int | enum | float`, result also `never`); from K9 on the 45 inline rows are `inline` lines of the list, and a test holds `rows::INLINE` equal to the marked set; its `defs` expansion gives one `pub unsafe fn <First><M: Mode>(ctx: &mut Ctx<M>, ..) -> Result<T, RunFault>` per line in eml_sched, its `names` expansion the list `LISTED: &[Listed]` (C-4; `Listed { row: Extern, section: Section, entry: Option<&'static str>, inline: bool }` as appendix 04 defines it), which eml_nrt re-exports together with the row functions (C-3), and its `entries` expansion in `crates/eml_nrt/src/rows.rs` the `eml_x_` shims for the lines that are not `inline` (K9 has the grammar and the lines). The VM keeps its hand-written arms until jit-externs adds the third expansion. `Exit` and `Panic` are engine-specific and hand-written in eml_nrt.
- **Wrapper protocol** (section 0's rule, spelled out): `Ok(v)` -> return `v`; `Err(RunFault::Extern(f))` from a row with `faults` -> `ctx.set_fault(f)`, return the placeholder; any other `Err`, and any `Err` from a row without `faults` -> the shim's last arm calls `worker::record(ctx, e, None)` and returns its `Fatal` (`Raise` has no impl for `RunFault`, and no `Raise` impl may need the worker struct), i.e. fatal; a caught panic -> the shim's fatal. A non-`faults` row has no unwind point and cannot fail in fast mode (of the 9 seed rows only `Println` and `Eprintln` have `faults`; the other 7 are `Pure` [V: eml_extern lib.rs L360, L447, L514, L527, L553-577]), so nothing is ever parked without a test following it.

**D-11 Entries jit-core adds** (eml_nrt, all `shim`; kinds `i64` unless noted):

| Symbol | Parameters -> result | Body | Table (file) |
|---|---|---|---|
| `eml_rt_alloc_data` | `tag, scanned, raw, lin` -> word | `Heap::alloc_data` | `heap.rs` |
| `eml_rt_alloc_int` / `eml_rt_alloc_float` | `n` / `x: f64` -> word | `Heap::alloc_int` / `alloc_float(x.to_bits())` | `heap.rs` |
| `eml_rt_dup_slow` / `eml_rt_decref` | `w` -> none | `Heap::dup` / `decref` | `heap.rs` |
| `eml_rt_is_last` | `w` -> 0/1 | `Heap::is_last` | `heap.rs` |
| `eml_rt_free_block` | `w` -> none | `Heap::free_block` | `heap.rs` |
| `eml_rt_con_at` | `token, tag, scanned, raw, lin` -> word | `Heap::con_at(token, scanned + raw, data_meta(..)).0` | `heap.rs` |
| `eml_rt_drop_token` | `token` -> none | `Heap::drop_token` | `heap.rs` |
| `eml_rt_fault_arith` | `kind` -> none | `ctx.set_fault(Fault::Arith(..))`; `ARITH_DIVISION_BY_ZERO = 0`, `ARITH_INTEGER_OVERFLOW = 1` | `heap.rs` |
| `eml_nrt_failed` | `function, site, purity` -> action | D-6 | `services.rs` |
| `eml_nrt_safepoint` | `function, ep_helper` -> action | D-5 | `services.rs` |
| 9 x `eml_x_<Variant>` + `eml_x_Exit`, `eml_x_Panic` | the row's kinds (`float` = `f64`; `never` = no result) | D-10, D-6 | `rows.rs` |

`catch_unwind` stays in every entry (settled by the contract: its fast-path cost is zero, and `decref` can run arbitrary `Foreign` finalizers). `eml_fatal(` is written in eml_nrt only in jit-rt's macro file and `enter.rs`. No entry of these two stages re-enters eml code.

**D-12 `Unsupported` and E0004.**
```rust
// crates/eml_native/src/unsupported.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Form {              // declared order = report order = histogram key order
    Par,                     // Call::Par                                         (lifted by jit-tasks)
    AsyncRow,                // ExternRef::Std(e), e.is_async()                   (jit-tasks)
    BlockingC,               // ExternRef::CSubmit / CTake                        (jit-tasks)
    Prim,                    // any Rhs::Prim   (empty_mut_array: jit-externs; evidence, yield, dummy, root prims: jit-effects; child_abort: jit-tasks)
    Clause,                  // Call::Clause, or LirFn.clause.is_some()           (jit-effects)
    Prompt,                  // Call::Apply { prompt: Some(_) }                   (jit-effects)
    StdRow,                  // non-async std row in neither rows::INLINE nor rows::CALLED   (jit-externs)
    CRow,                    // ExternRef::C                                      (jit-externs)
    Closure,                 // Rhs::MakeClosure                                  (jit-apply)
    FnValue,                 // Atom::Fn anywhere                                 (jit-apply)
    Apply,                   // Call::Apply                                       (jit-apply)
}
pub struct Unsupported { pub form: Form, pub function: String, pub row: Option<&'static str> }
/// One element per distinct form, sorted by `Form`; `function` = the first function in `program.functions` order with
/// the form; `row` = the first row name for `AsyncRow` / `StdRow`.
pub fn unsupported(program: &Program) -> Vec<Unsupported>;
impl Form { pub fn label(self) -> &'static str }   // "par", "async row", "blocking c", "prim", "clause", "prompt", "std row", "c row", "closure", "fn value", "apply"
```
A file's histogram bucket is `unsupported(p)[0].form`. Because the jit-apply forms are last in the order, a file is in one of their buckets only if nothing else blocks it, so jit-apply's numbers are jit-core's with those buckets moved to `accepted` [V 9.1]. For the other stages a file can move from one bucket to another while still rejected (e.g. `prim` -> `std row`); every stage's snapshot change is a kind-2 change stated in its plan.
`eml_cli::Session::compile_native(&self) -> Compiled<eml_lir::Program>` (feature `run`): `compile()`, then one `Diagnostic::not_yet_supported(entry file, range of the name `main` in its declaration, message)` per element; with any, `program = None`. Message: ``native code does not support {what} yet (first in `{function}`)``; `what`: `` `par` ``, ``the task row `{row}` ``, `blocking C functions`, `effect operations`, `operation clauses`, `handler landings`, ``the std row `{row}` ``, `C functions`, `closures`, `function values`, `calls of function values`. It is the only `not_yet_supported` call in `crates/eml_cli/src`. Lowering `unreachable!`s on a rejected form.

**D-13 `Image`, driver, `NativeMachine`.**
```rust
// crates/eml_nrt/src/image.rs
#[repr(C)] pub struct Str { pub ptr: *const u8, pub len: usize }
#[repr(C)] pub struct Slice<T> { pub ptr: *const T, pub len: usize }
#[repr(C)] pub struct SiteRow { pub file: u32, pub line: u32, pub column: u32 }                       // file == NO_FILE: none
#[repr(C)] pub struct FnRow { pub name: Str, pub code: *const u8, pub size: u32, pub sites: Slice<SiteRow> }   // code/size: D-18
#[repr(C)] pub struct DatumRow { pub kind: u32, pub bits: u64, pub text: Str, pub slot: *mut u64 }    // DATUM_STRING=0, DATUM_BIG_INT=1, DATUM_FLOAT=2
#[repr(C)] pub struct Image { pub functions: Slice<FnRow>, pub files: Slice<Str>, pub data: Slice<DatumRow> }
pub struct Start { pub thunk: Thunk, pub entry: *const u8, pub argv: *mut u64, pub result_rc: bool }
#[derive(Clone, Copy)] pub enum Census { Skip, Check }
pub struct Options { pub census: Census, pub stack: usize }            // Default: Skip, run::WORKER_STACK
pub const ACTION_CONTINUE: i64 = 0; pub const ACTION_UNWIND: i64 = 1; pub const ACTION_RAISE: i64 = 2;
// crates/eml_nrt/src/run.rs
pub const WORKER_STACK: usize = 1 << 30; pub const STACK_HARD_MARGIN: usize = 1 << 20; pub const LIMIT_HEADROOM_BYTES: usize = 2 << 20;   // the only definition (D-5)
pub fn run(image: &Image, start: &Start, config: &RunConfig, host: HostRef, options: Options) -> Result<Finished, RuntimeError>;
// crates/eml_nrt/src/worker.rs
pub struct Worker { image: *const Image, exit: Option<i64>, panic: Option<Word>, error: Option<RuntimeError> }
pub struct At { pub function: usize, pub site: Option<u32> }
pub fn record(ctx: &mut Ctx<Fast>, fault: RunFault, at: Option<At>) -> Fatal; pub fn park_panic(ctx: &mut Ctx<Fast>, message: Word);
```
`run`: `control.begin_run()`; spawn `eml-worker-0` with `options.stack`; on it `Ctx::<Fast>::for_worker(control, host, 0)`; `set_stack_limit(real)` with `real = (address of a local at thread start - options.stack) + STACK_HARD_MARGIN + LIMIT_HEADROOM_BYTES` (D-5); build the data (`constants::build`), attach the frozen region, write each `*DatumRow.slot`; put `Worker` behind `ctx.set_runtime`; `eml_nrt::enter(ctx_ptr, start.thunk, start.entry, start.argv)` holding only the raw pointer; then `set_runtime(null)` and `debug_assert!(eml_nrt::detached(&ctx))`.
- `Ok(())`: if `ctx.yielding() != 0`: with `worker.exit = Some(code)` -> `yield_finish`, `evv_set(Word::UNIT)`, `control.exit(code)`; without -> `RunFault::Internal("an abort reached the bottom of the task")`. Else `decref` the result iff `result_rc`. Then `flush` (an `Err(fault)` is `RuntimeError::Fault { RunFault::Extern(fault), function: "<native code>", at: None }`), `check_finished` (false -> `Internal("the evidence list or a yield is left at the end")`), a parked fault left (`ctx.take_fault().is_some()`; this covers a parked panic through `Fault::Engine`) -> `Internal("a fault is left parked at the end")`, then the census. `Finished { stats: RunStats::default(), exit: control.exit_code(), all counters 0 }`.
- `Err(fatal)`: read the error by `fatal.code` as D-6 states (`FATAL_RECORDED`, `FATAL_FAULT`, `FATAL_PANICKED`), then `flush` (its error ignored).
- **Census** (`Census::Check`, after `Ok` only), exactly:
  ```rust
  pub fn census(ctx: &mut Ctx<Fast>) -> Vec<(String, u64)> {          // crates/eml_nrt/src/census.rs
      ctx.reclaim();
      let heap = unsafe { ctx.heap_mut() };
      let mut counts = BTreeMap::<&'static str, u64>::new();
      for w in heap.live_objects() {
          if !heap.is_immortal(w) { *counts.entry(unsafe { heap.meta(w) }.map_or("?", |m| m.kind().name())).or_default() += 1; }
      }
      counts.into_iter().map(|(kind, n)| (kind.to_string(), n)).collect()
  }
  ```
  non-empty -> `Err(RuntimeError::Leak(list))` (shown as `memory leak: objects were not freed: 1 Data, ...`). `Heap::live_objects` exists (heap/walk.rs L275 [V]); `Heap::is_immortal` is new in eml_rt (task K3):
  ```rust
  /// inside verus!, impl<M: Mode> Heap<M>, heap/walk.rs, before `collect_live`
  pub fn is_immortal(&self, w: Word) -> (r: bool)
      requires self.wf(), w.is_ptr_spec(), self.word_in(w), self.live_block(w.bits_spec()),
      ensures r == (self.rc_value(w) == 0),
  { let h = self.header_of(w); self.core.rc_get(h) == 0 }
  ```
  With exactly this text `cargo verus verify -p eml_rt` gives 1221 verified, 0 errors (1220 at base) [V 9.3]; its precondition is `live_objects`' postcondition.
eml_nrt re-exports `RunConfig`, `Finished`, `RuntimeError`, `RunFault`, `HostRef` (`pub use` in `lib.rs`); `jit.rs` names them as `eml_nrt::..`.
```rust
// crates/eml_native/src/jit.rs
pub struct NativeMachine { /* JITModule, pools behind Image, symbol table, RunConfig, HostRef */ }
pub struct Loaded { pub entry: u32 }
pub enum LoadError { Entry { function: String }, Loaded, Module(String) }
pub enum Arg { I64(i64), F64(f64) }
impl NativeMachine {
    pub fn new(config: &RunConfig, host: HostRef) -> NativeMachine;
    pub fn load(&mut self, program: &Program, linked: &Linked) -> Result<Loaded, LoadError>;
    pub fn run(&mut self, loaded: Loaded, options: Options) -> Result<Finished, RuntimeError>;
    pub fn symbols(&self) -> &[Symbol];                                   // D-18
    #[doc(hidden)] pub fn call(&mut self, name: &str, args: &[Arg], options: Options) -> Result<Vec<Arg>, RuntimeError>;
    #[doc(hidden)] pub fn clif(&self, name: &str) -> Option<&str>;        // kept only when asked for at `new_with_clif`
    pub fn close(self);                                                   // JITModule::free_memory after run returned
}
pub fn execute(program: &Program, linked: &Linked, config: &RunConfig, host: HostRef, options: Options) -> Result<Finished, RuntimeError>;
```
`execute` maps a `LoadError` to `RuntimeError::Fault { RunFault::Internal(..), function: entry name, at: None }` (as `eml_vm::execute`, lib.rs L293-307 [V]). The fault enum `VmFault` is renamed `RunFault` (it is now built by two engines; 400 textual occurrences, none in a snapshot [V 9.4]).

**D-14 CLI.** `eml run --native` (`#[arg(long, conflicts_with_all = ["debug_heap", "stats"])]`). The run branch (main.rs L92-170 [V]) calls `session.compile_native()` and `eml_cli::execute_on(Engine::Native, ..)`. Until jit-tasks, `--native` together with `--gui` or a set `EML_GUI` (C-11), with an explicit `--workers N` where `N != 1`, or, without `--workers`, with `EML_WORKERS` other than 1 (C-7), is a usage error with exit 2 (`--gui`: `conflicts_with`; `--workers`: `error: --native runs one worker; --workers must be 1`; the `EML_WORKERS` and `EML_GUI` messages likewise name no stage, C-10); without either flag or variable the native run uses one worker, which spec/native.md 「VMとの違い」 states with entry (3). Exit codes as the VM's. `!eml_nrt::SUPPORTED` -> `error: native code is not supported on this machine`, exit 2.
```rust
// crates/eml_cli/src/lib.rs (feature run)
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Engine { Vm, Native }
pub fn execute_on(engine: Engine, program: &eml_lir::Program, linked: &Linked, config: &RunConfig, host: HostRef) -> Result<Finished, RuntimeError>;   // Native: Options::default()
```

**D-15 Native UI harness.**
- `eml_test_support::execute_native(program, linked, config, host) -> Result<Finished, RuntimeError>` = `eml_native::execute(.., Options { census: Census::Check, ..Default::default() })`: every native run in tests goes through it (the eml_native test helpers and ui.rs).
- Mark `-- native: skip (<reason>)`: `Marks.native_skip`, parsed in `leading_marks` like `workers: skip`, allowed on `run` / `run-fail` entries only. The reason names an entry of spec/native.md 「VMとの違い」; the mark takes precedence over rejection.
- Tests `ui::run_native`, `ui::run_fail_native`, one shared function. Per entry file: (1) mark -> `skipped`; (2) compile; `unsupported` non-empty -> `rejected[forms[0]]`; (3) `accepted`; on Windows a `windows: skip` file stops here; (4) run-fail only: checked VM run, must be `Err`; `!fast_mode_reports(error)` -> `checked only`, stop (ui.rs L297-299: a VM bug must not run unchecked code in the test process [V]); (5) VM fast (`debug_heap = false`, `Sched::Normal`, 1 worker) and native, same marks, same `eml_test_support::link(program)`, a fresh host and `Control` each; `assert_agrees(.., Compare::Nothing)`: stdout, stderr, written files, result text.
- The file walk of these tests keeps `windows: skip` files (steps 1-3 are OS-independent), so the snapshot is the same on every OS. Snapshot per test (insta file snapshot), not asserted when `EML_UI_FILTER` is set:
  ```
  accepted 174
  skipped 6
  rejected 455
    async row 128
    c row 2
    closure 19
    fn value 15
    par 22
    prim 156
    std row 113
  ```
  (`run_fail_native` has `checked only 0` after `accepted`.)
- Kind-1 list, mark reason `the depth limit is the thread's stack, not a word count` (entry (1) of 「VMとの違い」): `tests/ui/run/async/limit_depth_after_cancel.em`, `limit_depth_caught.em`, `limit_depth_fails_scope.em`, `limit_depth_in_shield.em`, `limit_depth_nested_run.em`, `limit_depth_repeated.em`, `tests/ui/run-fail/async/limit_depth_par_child.em`, `limit_depth_uncaught.em` (all files with a `max-depth` mark [V 9.5]). The divergence is a listed one and permanent (D-5). The 5 `max-bytes` files are not marked.
- nextest: the two override filters (.config/nextest.toml L8, L17 [V]) gain `| test(=ui::run_native) | test(=ui::run_fail_native)`.

**D-16 Tests.**
- Codegen unit tests (`crates/eml_native/tests`, each file `#![cfg(not(miri))]`): LIR text -> verifier -> VM checked run (`eml_test_support::execute`) and native run (`execute_native`) -> same stdout and result text; census clean. Helpers `tests/common/native.rs`: `agree(text)`, `agree_lowered(text)` (through `passes_until(Reuse)`), `call(text, name, args)`, `clif(text, function)`.
- CLIF pins (`tests/clif.rs`, inline insta), exactly: `a_call_and_its_unwind_check`, `the_entry_safepoint`, `inline_dup_and_decref`, `a_tag_switch`, `a_faulting_row`, `a_row_that_does_not_fault`, `a_never_row`; jit-apply adds `an_apply_site`. They are the record of the cost model (D-17).
- `rows::every_inline_row_agrees_with_its_shared_function` iterates `rows::INLINE`; a row without a matrix fails. `rc::inline_dup_and_decref_agree_with_the_heap`: a stand-in start thunk on the run's worker makes the objects in the run's heap and calls the function through the thunk from `entry_of`; local 1, local 2, shared (negative rc), stuck (`LOCAL_MAX`, written directly), immortal, immediate, each compared with a twin under `Heap::dup` / `Heap::decref` (C-8). `data::boxing_an_int_agrees_with_the_heap` at the 63-bit boundaries.
- `abort::exit_unwinds_two_frames_holding_owned_values_and_a_token`.
- `reuse::inline_token_moves_agree_with_the_heap`, `reuse::release_of_a_shared_or_stuck_value_gives_an_empty_token`, `reuse::an_in_place_reuse_calls_no_entry`: the inline token moves against `Heap::into_token` / `Heap::reuse_in_place` and `Heap`'s twin for shared (-1, -2) and stuck parents; the CLIF of an in-place reuse names neither deleted entry.
- `machine::symbols_cover_every_function`: every `Image.functions` row has a symbol whose range holds its code address, symbol ranges do not overlap, and loading the same program in a fresh machine gives the same names. The pin `a_call_and_its_unwind_check` shows a `colocated` callee.
- eml_nrt tests: the driver and services with Rust `extern "C"` stand-ins for thunk and entry.

**D-17 Native cost model** (fast path; "load" = memory load; pinned by the CLIF pins and recorded by `bench/native.sh`):

| Construct | Fast path | Not paid | Left for a later stage / forgone |
|---|---|---|---|
| direct call | `call` rel; arguments in registers; `ctx` is one register move | no frame setup beyond Cranelift's; no count / kind check | `tail` cc keeps the platform's callee-saved set for an ordinary `call` (only an exceptional `try_call` edge clobbers everything: cranelift-codegen 0.137 `get_regs_clobbered_by_call`, aarch64/abi.rs L1192-1209, x64/abi.rs L885-900; prologue saves in aarch64/abi.rs L1392-1425, x64/abi.rs L932-935), so a value live across a call sits in a callee-saved register when one is free and is spilled only past that (decision "eml calls use `tail`"); a pinned `ctx` register is not used (decision "`ctx` is the first argument") |
| tail call | `return_call` = jump; a self tail call is a loop that passes the safepoint | no frame growth | - |
| unwind check after a non-tail call | 1 load (`ctx.yielding.target`, offset 0: an 8-bit displacement on x86-64, one scaled `ldr` on aarch64) + compare-with-constant + branch never taken | no table lookup, no second flag | minimal for "aborts travel by return value" |
| entry safepoint | 3 loads (stop pointer, stop word, `stack_limit`) + SP read + 2 compares + `or` + 1 branch | no call | a single-load form (stop requests poison `stack_limit`) and a leaf exemption are forgone here: both need cross-thread writes or a special case against "every entry has the safepoint"; revisit in native-inline with numbers |
| `dup` / `decref` of a local object | imm test + 1 load + range compare + 1 store, 2 branches | no call; immortals (constants, static closures) take 1 more branch and no call | last reference, shared, stuck: 1 call |
| field read / write | 1 load / store at a constant offset | no bounds or kind check | - |
| allocation (`con`, `float` box, big `int`) | 1 call (`eml_rt_alloc_data` ..; inside, one `add` to reach `ctx.heap`, the price of `Native` first) + 1 store per field | - | inline allocation fast path: native-inline, after eml_rt exports the allocator facts |
| `int` box / unbox | box: shift, shift, compare, branch, `or`; unbox: test, branch, shift | no call for 63-bit ints | - |
| `switch` on tags | `enum`: the value; `obj`: 1 load + mask; `tobj`: imm test + that; then `Switch` (compare chain or `br_table`) | - | fusing an inline compare with the `switch` that follows needs a side table of "boolean sources": forgone unless Cranelift's mid-end already folds it (the bench shows) |
| `release` | 1 load + compare; unique: per dropped child an inline `decref`, then 1 call (`free_block`) or none (the box becomes the token) | no generic walk | inline free: native-inline |
| reuse `con` | token test + stores of non-skipped fields; in place: no call; another constructor: 1 call (`eml_rt_con_at`) | no allocation | - |
| constants | 1 load from the constant's own data symbol (one relocated address per distinct constant a function uses; no index arithmetic) | no RC call on use (immortal) | - |
| inline row | the CLIF op (+ overflow / zero branch, or NaN select) | no call, no fault load | - |
| called row | 1 default-cc `call` into the shim (argument moves, `catch_unwind` with no cost on the normal path) + the row; `faults` rows add 1 load + branch | no argument array, no dispatch | container rows inline: native-inline |
| apply, exact count (jit-apply) | 2 loads (meta, code word) + mask + compare + shift + `call_indirect` (+ the unwind check) | no function table, no descriptor load, no count passed | - |
| closure entry `c<k>` | `k` loads + rc load / compare; unique: 1 call (`free_block`); shared: inline `dup`s + `decref`; then a jump to the function | static closures (`k = 0`): a jump only | inline free: native-inline |
| partial application | 1 direct call (`eml$apply$m`) + 1 call (`eml_nrt_pap`: 1 allocation); calling the PAP: `m + 1` loads, rc as above, 1 indirect jump more than an exact call | no descriptor lookup | - |

Every `Native` field generated code reads (`yielding.target`, `fault`, the stop pointer, `stack_limit`) is at an offset <= 120, so each read has the shortest encoding on every target. Native-versus-VM trade: none of these choices slows the VM; K1-K3 leave VM instruction counts unchanged.
Name-fixed code: a function passes its own code address (cold blocks only) where the runtime needs to know "which function / which site"; the runtime's address -> `FnRow` lookup is a binary search on the cold path. No hot path pays for it.
Forgone optimisations (clean design first): compare-and-branch fusion table; leaf-function safepoint elision; single-load safepoint; `ctx` in a pinned register; dropping `catch_unwind` from hot entries (no measurable gain expected); packing arity and code into one word.

**D-18 Measurement and profiler hooks.**
- `bench/native.sh` (jit-core): for every `bench/*.em`, `eml run` and `eml run --native` in the release build, `instructions retired` and wall time (medians of 3, `EML_WORKERS=1`, the method of `bench/run.sh`), one Markdown table with the native / VM ratio; a program `--native` rejects is printed as `rejected`. The table is recorded in docs/implementation/benchmarks.md under a new heading 「ネイティブの数値」 at each native stage's Close. It is a record, not a gate. 25 of the 42 programs are accepted at jit-core, 27 at jit-apply [V 9.1].
- Hooks laid down in jit-core: frame pointers are preserved (`abi::isa`); every function, adapter and helper has a stable module symbol name (D-3); `NativeMachine::symbols() -> &[Symbol { name: String, address: *const u8, size: u32 }]` and `Image.functions[i].{code, size}` give the code-address -> name map for every function the JIT host defines, thunks included (sizes from `Module::define_function`'s result); fault sites map an address-independent site number to a source position. Nothing in the design prevents frame-pointer stack walking: eml frames, thunks and the stub keep the FP chain.
- The profiler itself is the roadmap item 「ネイティブのプロファイラ」 (前提 `native-aot`) that the doc-revision task of jit-rt's plan writes; these stages add nothing to the roadmap for it.

### 1.2 jit-apply decisions

**D-19 Closure contents.** A native function value is always an object; the object header is its descriptor:
- header tag = the number of argument leaves still missing, `n` (1..=65535);
- scanned word 0 = the code pointer of its entry as an immediate (`mem::make_imm(func_addr)`);
- then the RC captures (scanned) and the other captures (raw) in the function's parameter order: the shape of `LayoutCtor { fields: [TObj] ++ capture Reprs }`;
- `lin` from the bound variable's mark.
`Atom::Fn(f)` is the word of a **static closure**: an immortal object `{ tag = params(f), [code of c0:<f>] }` built at load in the frozen region (`Datum::Fn(f)`, data symbol `v:<function name>`, `DATUM_FN = 3`, `DatumRow` gains `code: *const u8`), read with `mem::data_word`. `FnRow` gains `value: *mut u64` (the slot of the function's static closure, null if none), so the runtime can obtain the function value of a function known only by name; `consts.fn_value(FnIdx) -> DataId` declares the symbol, and later stages call it for their by-name roots (`fork$E.op`, the `Prelude.Panic` / `Prelude.IO` clause functions).
No eml_rt code reads the tag of a closure: the 20 non-test `.tag()` uses in `crates/eml_rt/src` are evidence / evv-node / entry-table checks (`heap/ctx.rs`), the `release` layout check (`heap/check.rs` L493, never applied to closures), array / `Raw` checks that require tag 0 together with an extended scan or `Kind::Raw`, and copies of the whole `Meta` (`recover.rs` L699, `string.rs` L2265) [V 9.6]. eml_nrt asserts `Word::int(code as i64).is_some()` when it writes a code word.
Rejected: the VM's `imm(f | k << 32)` + a function table (three dependent loads per apply); arity and code packed in one immediate (address-width dependent); a second head word for `n` (8 bytes per closure); function values as immediates pointing to static descriptors (two decodings at every site).

**D-20 Closure entries.** For each `(f, k)` that occurs (`MakeClosure(f, caps)` with `k = caps.len() >= 1`, core-ir.md L291 [V]; `Atom::Fn(f)` and `fn_value` roots with `k = 0`) the adapter `c<k>:<f>` has the signature `tail (ctx, clo, params(f)[k..]) -> rets(f)`: `k >= 1`: load the captures by Repr; `brif is_one_ref(rc) last, ask; ask: call eml_rt_is_last ..; last: call eml_rt_free_block(ctx, clo); shared:` inline `dup` of the RC captures, inline `decref(clo)`; `return_call f:<f>(ctx, caps.., args..)` (VM `take_captures`, apply.rs [V]). `k = 0`: `return_call f:<f>(ctx, args..)`.
For a function with `clause.is_none()` lowering asserts that `params[k..]` are all uniform (`obj` / `tobj` / `unit`) and `rets` is one uniform leaf, so the adapter's signature is `uniform(n)` = `(ctx, I64, n x I64) -> I64` (the verifier's rule for value-referenced functions; the assertion turns a violation into a compile-time panic instead of a signature mismatch). For a clause function (jit-effects) the same adapter is typed by the operation's ABI and is reached only from `Call::Clause`; no second slot is needed because a clause function is never applied uniformly (lir.md: clause functions are referenced only by `prim ev_set_clause`).

**D-21 Apply.** At `Call::Apply { callee, args }`, `m = args.len() >= 1`:
```
n = mem::load_tag(callee);  brif (n == m) fast, slow
fast: r = call_indirect uniform(m) mem::closure_code(callee)(ctx, callee, args..);  jump join(r)
slow: r = call eml$apply$m(ctx, callee, args..);  jump join(r)
join: after_unwind_point(point, After::Call)
```
(tail position: `return_call_indirect` / `return_call eml$apply$m`). `eml$apply$m`: `tail (ctx, callee, a_1..a_m) -> I64`, emitted for every `m` in `1..=max(1, largest m of the program)`; `eml$apply$1` therefore always exists and is the entry later stages use to run a closure on `()` (`eml$apply_unit`):
```
n = load_tag(callee)
n == m:  return_call_indirect code(ctx, callee, a_1..a_m)
n >  m:  r = call eml_nrt_pap(ctx, callee, mem::spill([a_1..a_m]), m);  return r
n <  m:  Switch on n over 1..m:  r = call_indirect uniform(n) code(ctx, callee, a_1..a_n)
                                 after_unwind_point(Point { owned: a_{n+1}..a_m, once each }, After::Call)
                                 return_call eml$apply$(m-n)(ctx, r, a_{n+1}..a_m)
         default: trap
```
The check after the first part is the VM's `ApplyRest` frame dropping its rest arguments (apply.rs `rest_arguments` [V]); only the last arrow of one `apply` can yield (core-ir.md L444 [V]). Size: `eml$apply$m` has `m - 1` arms; over the UI programs the largest `m` is 2 in all but three programs (3, 29, 30) [V 9.1]; for `m = 30` the helpers total about 15k CLIF instructions (~30 ms in the dev profile [I, from 9.2's rate]).

**D-22 Partial application: generic PAP objects.** `eml_nrt_pap(ctx, callee, argv, m) -> word`: `n = tag(callee)`, `r = n - m`; `r <= 0` or no adapter for `(m, r)` -> fatal `Internal("applying a value that is not a function")`; else allocates `{ tag = r, scanned: [code of eml$pap$m$r, callee, a_1..a_m], lin = callee's lin }` (the VM copies `lin`, apply.rs L80-84 [V]). `eml$pap$m$r`: `tail (ctx, pap, b_1..b_r) -> I64`: load `callee`, `a_1..a_m`; last reference -> `free_block(pap)`, else `dup` each and `decref(pap)`; `return_call_indirect code(callee)(ctx, callee, a.., b..)` (`n(callee) = m + r` by construction). Emitted for `m in 1..A`, `r in 1..=A-m`, `A` = the largest `n` of any closure creation of the program; `Image.paps: Slice<PapRow { args: u32, rest: u32, code: *const u8 }>`. Over the UI programs `A <= 4` (at most 6 adapters) [V 9.1]; the count is `A(A-1)/2`, e.g. 1225 small functions for a 50-parameter function value (~0.1 s [I]); no cap.
(The older design sentence, one entry per `(function, k)` for partial application, is replaced; section 3.) Why not the VM's form (a new closure of the same `f` with `k + m` captures): it needs an adapter for every `k` up to the arity of every function used as a value, and the runtime must reach `(f, k)`, its raw-capture count and the next adapter from the closure, i.e. a descriptor load on every apply or a lookup. Generic PAPs keep the apply fast path at two loads and the runtime routine at ten lines; the cost is one more indirect jump when a PAP is called.

**D-23** `Closure`, `FnValue`, `Apply` leave `Form` (8 variants remain). New entry: `eml_nrt_pap` (`shim`, table `closures.rs`).

**D-24 Tests of jit-apply.** `apply::a_closure_is_freed_on_its_last_use`, `a_shared_closure_dups_its_captures`, `a_static_function_value_takes_no_rc`, `raw_captures_keep_their_bits`, `a_closure_with_255_captures`, `a_tail_apply_runs_in_constant_stack`, `an_apply_result_is_checked_for_an_abort`, `a_partial_application_waits_for_the_rest` (called once and twice), `a_partial_application_of_a_partial_application`, `an_excess_application_applies_the_result`, `an_abort_inside_an_excess_application_drops_the_rest`, `a_lin_closure_stays_lin_through_a_partial_application`; pin `clif::an_apply_site`.

---

## 2. Interfaces provided (for the other stages' plans)

| Interface | Where | For |
|---|---|---|
| `mem.rs` function list of D-7; `abi.rs`, `thunk.rs` used as jit-rt defines them | `crates/eml_native/src` | all |
| `after_unwind_point(fx, &Point, After)`, `After::{Call, Faults, Arith, Never}`; jit-effects adds its abort / landing split at the top of the cold side and an `AbortPrim` kind | `lower/unwind.rs` | jit-effects |
| Entry safepoint with `leave` = cleanup of `Unwinds.entry`; routing `ACTION_RAISE` to `eml$raise` is added there by jit-tasks | `lower/safepoint.rs` | jit-tasks |
| `eml$raise(ctx, function, site, purity)`; `eml_nrt_failed(ctx, function, site, purity) -> action`; `eml_nrt_safepoint(ctx, function, ep_helper) -> action`; `function` is a code address, `site` indexes that function's `FnRow.sites` | eml_native / `eml_nrt/src/services.rs` | jit-externs, jit-effects, jit-tasks |
| `worker::record(ctx, RunFault, Option<At>) -> Fatal` (accepts any `RunFault`; `FATAL_RECORDED = 3`); `worker::park_panic(ctx, Word)` (payload in the worker struct + `Fault::Engine`); `Worker` is a plain struct that stages extend | `eml_nrt/src/worker.rs` | jit-externs (C panics: `ctx.fresh_string(text)` then `park_panic`), jit-tasks |
| `rows::{INLINE, CALLED, lowering}`; `Form::{StdRow, CRow}`; called-row lowering from the descriptor | `lower/rows.rs`, `unsupported.rs` | jit-externs widens `CALLED` to every listed row |
| `extern_rows!` (final grammar, `as eml_x_V`, `inline` lines) with the `defs`, `names` and `entries` expansions for the 9 seed rows and the 45 inline rows; the row shims record internal-class errors with `worker::record` | `eml_sched/src/rows.rs`, `eml_nrt/src/rows.rs` | jit-externs adds the remaining rows and the `vm_arms` expansion |
| Load-time data as per-name data symbols: `Datum` kinds, `consts.<kind>() -> DataId`, `DatumRow.slot`, eml_nrt fills the slots before the first entry | `lower/consts.rs`, `eml_nrt/src/{image,constants}.rs` | jit-externs (`EmptyMutArray`), jit-effects (effect numbers by name, mask lists) |
| Function value of a function by name: `consts.fn_value(FnIdx)`, `FnRow.value`; adapter `c0:<f>` typed for clause functions | `lower/{consts,apply}.rs` | jit-effects (`fork$E.op`, `panicked`, `io_failed`), jit-tasks |
| Closure decoding: `mem::load_tag`, `mem::closure_code`; no other descriptor | `mem.rs` | jit-effects (`Call::Clause` = `call_indirect <op ABI> closure_code(callee)(ctx, callee, ev, args..)`) |
| `eml$apply$m` always present for `m = 1` | `lower/apply.rs` | jit-tasks (`eml$apply_unit`) |
| `Image` (`repr(C)`, grows per stage), `Start`, `Options`, `run` | `eml_nrt/src/{image,run}.rs` | jit-tasks rewrites `run` around its worker table |
| Harness: `Engine`, `execute_native`, `-- native: skip`, the two tests and their snapshot; the same `Linked` goes to both engines | eml_cli tests, eml_test_support | all |
| `bench/native.sh`; `NativeMachine::symbols()` | bench, `jit.rs` | all native stages |

**Requests.**
- To jit-rt (S2a): the ABI echo test asserts that every parameter-kind sequence in `eml_nrt::entries()` has an echo entry: jit-core's new sequences are `[]`, `[I]`, `[F]`, `[I,I]`, `[F,F]`, `[I,I,I]`, `[I,I,I,I]`, `[I,I,I,I,I]`; the tasks that add entries add missing echoes in that test.
- To jit-externs (S3): none open on the grammar (K9 uses the final one); `FloatMod` needs an `eml_x_` entry that generated code calls (it is not `Inline`); the fault enum is named `RunFault` from jit-core on; C-row panics go through `worker::park_panic`, which leaves the fault word non-zero.
- To jit-effects (S4): a function's code may not embed machine numbers, so effect numbers are `e:<effect name>` data symbols (D-9); the function-value descriptor is the closure header (D-19); a `clause` slot is not added, the adapter of a clause function is typed (D-20); `eml_nrt_failed` and `eml$raise` keep the jit-core signatures until that stage changes them.

## 3. Rulings applied

The six challenges of r2 were ruled on; the design follows the rulings and holds no open challenge.

| Item | Ruling | In this plan |
|---|---|---|
| `FloatMod` is not inline | accepted: 45 inline rows, `FloatMod` a called row (`LibCall` has no `fmod` [V 9.7]) | D-10 |
| Generic PAP objects instead of an entry per `(f, k)` | accepted; docs/future/native.md L41 is rewritten only by the doc-revision task of jit-rt's plan | D-22 |
| The `panic` message word is engine-held | accepted with the marker `Fault::Engine` | D-6, K3 |
| A checked VM run precedes the comparison of a run-fail file | accepted | D-15 |
| object-model.md's immediate function number is VM-specific | accepted | D-19, A5 |
| Every `Rhs::Prim` is rejected until its stage | accepted; by stage: `empty_mut_array` jit-externs; evidence, yield, `dummy`, root prims jit-effects; `child_abort` jit-tasks | D-12 |
| Wrapper errors of the internal class are fatal through the shim | contract amended | section 0, D-10 |
| `Native` first, `heap` second | user decision | section 0, D-17 |
| Completion bounds 150 / 185; seed list; rename to `RunFault` | accepted as written | 1.0, D-10, K1 |

## 4. Rationale

- **Classifier as committed code first.** `unsupported` and the counting half of the harness land before any lowering (K4, K5), so the plan's numbers are checked by a test from the second week of the stage and never by a scratch tool.
- **Decisions in shared routines.** `eml$raise`, the services, `eml$apply$m` hold decisions; sites pass their function's address and a local index; later stages change bodies.
- **A function's name fixes its code.** Everything a function refers to is a symbol named by content or by LIR name, and everything it tells the runtime about itself is relative to its own address, so the same LIR gives the same code in any program and any load order (what the reuse key of the later stages relies on).
- **Performance through layout, not special cases.** The header-as-descriptor closure, the immortal zero test, inline boxing and per-name data symbols remove loads and calls without a second code path; the optimisations that would need side tables or contract exceptions are listed as forgone (D-17).
- **Budget.** A module with 40 functions (12 unwind checks each) is created, compiled and freed in 7.0 ms in the dev profile; a 1 GiB-stack thread costs 23 us; the largest accepted program has 1592 LIR statements [V 9.1, 9.2].

## 5. Alternatives considered and rejected (beyond those in D-n)

- Porting the VM's LIR-text tests wholesale (duplicates facts; the UI pass covers programs).
- Comparing native output with committed `ui::run` snapshots (they hold checked-mode output plus diagnostics).
- Harness auto-skip on `max-depth` (a skip the file does not state).
- `eml_rt_release(ctx, x, tag, keep_mask)` as one entry (the mask does not fit a word; the VM also expands `release`).
- `unsupported` reading eml_nrt's entry table directly (the answer would change commit by commit inside the stage; `rows::CALLED` plus the tie test gives the same guarantee).
- Census by reading the rc word with a raw pointer in eml_nrt (int-to-pointer under Miri's strict provenance; a layout read outside eml_rt).
- A second hand-written wrapper list for the seed rows (one mechanism only).

## 6. Assumptions

| # | Assumption | If false |
|---|---|---|
| A-1 [I] | On x86_64 (SysV, Win64) and aarch64 Linux: `is_pic=false` data-symbol reads and `func_addr` (both under the region rule of `abi::jit_builder`, 00 U-13), `return_call_indirect`, `sadd/ssub/smul_overflow` on `I64` work as on aarch64 macOS [V 9.2] | The task's tests fail on that CI target. `smul_overflow`: use `smulhi(a, b) != sshr(imul(a, b), 63)`. Data symbol: the task stops and the question goes to the user with the failing output (the alternative, a table pointer in `Native`, is a contract change) |
| A-2 [I] | Every accepted file is VM-identical natively | 1.0's rule (mark by plan revision; bar is >= 150 / >= 185) |
| A-3 [I; V macOS] | A 1 GiB reservation per native run works on the CI Linux and Windows machines at the test binary's thread count, and the 3 MiB below `real` cover the deepest runtime entry | `Options.stack`: `execute_native` passes 64 MiB (decided in K6 if its `run::the_worker_stack_is_reserved` fails on CI; recorded in testing.md); the margins are the two consts of D-5 |
| A-4 [I] | jit-rt and sched-split close with the names of section 0 | The first task that meets a different name adapts and notes it in its commit message; no logic depends on a name |
| A-5 [I] | jit-rt's `entries!` accepts the kind tokens and `never` as section 0 states | K9's `entries` expansion maps the tokens itself with one helper macro before calling `entries!`; nothing else changes |
| A-6 [I] | `catch_unwind` adds no instruction on the normal path of the hot entries (the contract's settled default) | `bench/native.sh` shows it; the question goes back to the contract owner with the numbers |

## 7. Implications for implementation

Conventions for every task: one commit; tests are written first; Japanese docs and comments (PLaMo); "Spec" = sections of `docs/spec/native.md` written in the same commit and cited by the code; a new source file of eml_native is added to `trust.rs` `FILES` (only `jit.rs` joins `UNSAFE_FILES`), of eml_nrt to its `trust.rs`. Unless a task says otherwise its completion includes `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` clean. Standing greps (expected output in comments), run in every task from K10 on:
```sh
grep -rn 'thread_local' crates/eml_nrt/src crates/eml_native/src                          # empty
grep -rnw 'Checked' crates/eml_nrt/src crates/eml_native/src                              # empty
grep -rnE 'CTX_[A-Z_]+\s*(:|=)' crates/eml_native/src crates/eml_nrt/src                  # empty
grep -rn 'Linkage::Import' crates/eml_native/src | wc -l                                  # 1  (abi.rs)
grep -rn 'CallConv::Tail' crates/eml_native/src | wc -l                                   # 1  (abi.rs)
grep -rln 'JITModule' crates/eml_native/src | sort                                        # crates/eml_native/src/bridge.rs, crates/eml_native/src/jit.rs
grep -rlnw 'eml_rt' crates/eml_native/src | sort                                          # crates/eml_native/src/mem.rs
grep -rn 'eml_sched' crates/eml_native/src crates/eml_native/Cargo.toml                    # empty
grep -rnE 'ins\(\)\.(load|store|uload|sload)' crates/eml_native/src | grep -v -e src/mem.rs -e src/thunk.rs -e src/bridge.rs   # empty
grep -rnE 'eml_rt::(Ctx|Heap)|Ctx<' crates/eml_native/src                                 # empty
grep -rn 'docs/future\|docs/plans' crates/eml_nrt crates/eml_native crates/eml_sched      # empty
grep -rn 'eml_fatal(' crates/eml_nrt/src | grep -v -e src/entries.rs -e src/enter.rs      # empty
grep -rn 'set_stack_limit(' crates/eml_nrt/src | wc -l                                    # 1  (run.rs, thread start)
grep -rn 'preserve_frame_pointers' crates/eml_native/src | wc -l                          # 1  (abi.rs)
grep -rln 'settings::builder' crates/eml_native/src | sort                                # crates/eml_native/src/abi.rs, crates/eml_native/src/bridge.rs
grep -rln 'call_indirect' crates/eml_native/src | sort                                    # crates/eml_native/src/bridge.rs, crates/eml_native/src/thunk.rs (jit-apply adds crates/eml_native/src/lower/apply.rs)
```

### 7.1 Stage jit-core

**Base**: the later of `Close jit-rt` and `Close sched-split`. Prerequisite checks:
```sh
for c in eml_nrt eml_lir eml_rt; do cargo tree -p eml_native -e normal --target all | grep -c "^.*$c v" ; done   # three numbers, each >= 1
for n in CTX_YIELD_TARGET CTX_FAULT CTX_STOP CTX_STACK_LIMIT; do grep -rn "pub const $n" crates/eml_rt/src | wc -l; done   # 1 1 1 1
ls crates/eml_sched/src/error.rs crates/eml_sched/src/config.rs crates/eml_native/src/abi.rs crates/eml_native/src/thunk.rs crates/eml_native/src/mem.rs docs/spec/native.md   # all exist
cargo nextest run                                                                                                 # green
```
Baselines recorded in the plan: `bench/run.sh` and `bench/rt.sh` tables; wall time of `cargo nextest run -p eml_cli --test integration -E 'test(=ui::run) | test(=ui::run_fail)'`.

**K0 Plan commit.** `docs/plans/<date>-jit-core.md`: sections 0, 1.0, 1.1, 2, 7.1 of this document. Test changes: kind 1 = the 8 marks of D-15 (K5); kind 2 = none (new snapshots only); kind 3 = K1 (rename), K2 (VM constants through the shared makers), K10 (`eml_test_support::execute_on` callers), K19 (`execute_in` callers).

**K1 Rename `VmFault` to `RunFault`.** `crates/eml_sched/src/error.rs` and every user (30 Rust files of eml_vm / eml_cli at 432fffb4, `docs/spec/vm.md`, `docs/implementation/{testing,architecture}.md` [V 9.4]). Mechanical. Completion: `grep -rn 'VmFault' crates docs/spec docs/implementation CLAUDE.md | wc -l` -> `0`; `cargo nextest run` green; `git diff --stat HEAD~1 -- '*.snap'` empty.

**K2 Immortal box makers in `eml_sched::constants`.** New `crates/eml_sched/src/constants.rs` (eml_sched already depends on eml_rt and names `Mode`, `Heap`):
```rust
pub fn immortal_big_int<M: Mode>(heap: &mut Heap<M>, n: i64) -> Word;   // the None arm of eml_vm compile/constants.rs boxed_int
pub fn immortal_float<M: Mode>(heap: &mut Heap<M>, bits: u64) -> Word;  // the body of boxed_float's allocation
```
eml_vm `compile/constants.rs` calls them. Tests first (eml_sched, both modes): the object is immortal and `unbox_int` / `unbox_float` read the value. Completion: `cargo nextest run -p eml_sched -p eml_vm` green; `git diff HEAD~1 -- crates/eml_vm/tests/bench.rs` empty; `grep -c 'alloc_immortal' crates/eml_vm/src/compile/constants.rs` -> `0`.

**K3 eml_rt: `Heap::is_immortal` and `Fault::Engine`.** `crates/eml_rt/src/heap/walk.rs`: the function of D-13, exactly. `crates/eml_rt/src/externs.rs` (`Fault`, L200-224; `io_error_kind` L229-244; `panics` L250-262; `Display` L1278-): the unit variant `Engine` as D-6 states. `crates/eml_sched/src/error.rs` `RuntimeError::is_broken`: add `RunFault::Extern(Fault::Engine)`. Tests first: `heap/tests` (both modes) `an_immortal_object_is_told_from_a_counted_one` (a fresh `alloc_data` object -> false; `dummy()` and an `alloc_string_immortal` -> true, each taken from `live_objects`); in the test module that holds `faults_stay_small`: `an_engine_held_failure_is_neither_a_panic_nor_an_io_error` (`panics()` false, `io_error_kind()` `None`, the `Display` text, size still 24); a `ctx` test: `set_fault(Fault::Engine)` then `take_fault()` returns it. Spec: object-model.md 「生存の列挙」 (one sentence) and the `Fault` table of 「extern」 (one row). Completion: `cargo verus verify -p eml_rt` -> `verification results:: <base + 1> verified, 0 errors`; `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` clean; `cargo nextest run -p eml_rt -p eml_sched -p eml_vm` green with no snapshot change; the Miri `trusted::` command green (no new trust item); the eml_rt bench gate: every gated row of `bench/rt.sh` within +0.1 % (median of 5 alternating pairs against the commit before). Pitfall: every exhaustive `match` on `Fault` in eml_vm / eml_cli gains the arm (the compiler lists them); behaviour for the existing variants does not change.

**K4 `unsupported` and the row lists.** Files: `crates/eml_native/src/unsupported.rs`, `lower/mod.rs` (module declarations only), `lower/rows.rs` (`Inline`, `INLINE`, `CALLED`, `lowering`; no expanders), `lib.rs`, `tests/unsupported.rs`. Tests first: `<form>_is_rejected` for the 11 labels (`par_is_rejected`, `async_row_is_rejected`, `blocking_c_is_rejected`, `prim_is_rejected`, `clause_is_rejected`, `prompt_is_rejected`, `std_row_is_rejected`, `c_row_is_rejected`, `closure_is_rejected`, `fn_value_is_rejected`, `apply_is_rejected`), `forms_are_sorted_and_distinct`, `the_first_function_and_row_are_reported`, `inline_and_called_rows_are_supported`, `the_inline_table_is_the_vm_arithmetic_list_without_float_mod` (a literal list of 45 variants), `the_called_rows_are_the_seed` (the 11 of D-10). Spec: 「断る形」. Completion: `cargo nextest run -p eml_native unsupported::` -> 16 passed.

**K5 The counting half of the native UI tests.** Files: `crates/eml_cli/tests/common/mod.rs` (`Marks.native_skip` L109-123, `marks_of` L139, `leading_marks`; a walk that keeps `windows: skip` files), `tests/ui.rs` (`run_native`, `run_fail_native` doing steps 1-3 of D-15 and the snapshot), the 8 files (kind 1), two snapshot files, `.config/nextest.toml` L8, L17, `docs/spec/native.md` (new section 「VMとの違い」 with entry (1) of D-5), `docs/implementation/testing.md` (new section 「ネイティブで流す」: the tests, the mark and that its reason names an entry of 「VMとの違い」, the snapshot; 「性能のテスト」: seven tests), CLAUDE.md Testing bullet. Tests first: `common` unit tests `a_native_mark_is_read`, `a_native_mark_outside_a_run_entry_panics`. Completion:
```sh
cargo nextest run -p eml_cli --test integration -E 'test(=ui::run_native) | test(=ui::run_fail_native)'   # 2 passed
grep -rn 'native: skip' tests/ui | wc -l                                                                    # 8
sed -n '/^---$/,$p' crates/eml_cli/tests/snapshots/integration__ui__run_native.snap | tail -n +2           # the block of D-15
sed -n '/^---$/,$p' crates/eml_cli/tests/snapshots/integration__ui__run_fail_native.snap | tail -n +2      # accepted 7 / checked only 0 / skipped 2 / rejected 72 / async row 33 / c row 2 / closure 1 / par 1 / prim 3 / std row 32
```
If the numbers differ from 1.0 the cause is a UI file added since 432fffb4: list the files, update the plan's numbers in this commit.

**K6 eml_nrt: image, worker, driver, census.** Files: `crates/eml_nrt/Cargo.toml` (`eml_sched`, `eml_extern`), `src/{image.rs, worker.rs, run.rs, census.rs, constants.rs}`, `lib.rs` (the re-exports `RunConfig`, `Finished`, `RuntimeError`, `RunFault`, `HostRef`), `src/enter.rs` (`FATAL_RECORDED = 3` next to jit-rt's codes; C-5), `tests/run.rs`. Tests first, Rust `extern "C"` stand-ins for thunk and entry: `run::returns_and_finishes`, `run::the_stack_limit_is_three_mebibytes_above_the_base` (the stand-in reads `ctx.stack_limit()` and its own frame address: the limit is below the frame and within `options.stack - 3 MiB` of it), `run::a_parked_panic_left_at_the_end_is_reported` (the stand-in calls `park_panic` and returns: `a fault is left parked at the end`), `run::a_recorded_error_comes_back` (stand-in calls a test shim returning `Err(record(..))`), `run::a_heap_error_comes_back_as_its_fault` (a test shim returning `Err(HeapError::..)`: `FATAL_FAULT`, reported in `<native code>`), `run::an_entry_panic_is_an_internal_error`, `run::a_code_address_finds_its_function` (first, middle and last byte of a row's range; an address outside every range gives `<native code>`), `run::an_abort_without_exit_is_an_internal_error`, `run::the_census_reports_a_counted_object` (`memory leak: objects were not freed: 1 Data`), `run::data_slots_hold_immortal_objects`, `run::the_worker_stack_is_reserved` (`Options.stack` = 64 MiB; a stand-in recursing through 32 MiB of frames returns; expected under 50 ms [I]; the 1 GiB default is exercised by every other test, which only reserves it); unit tests `census::`, `constants::` (no `enter`; run under Miri). Tests calling `enter` are in `#![cfg(not(miri))]` files; fatal ones in a module named `fatal`. Spec: 「`Image`」, 「実行の始めと終わり」 (driver order, `Finished`, the census), 「安全点」 (the stack geometry of D-5 and its two consts). Completion: `cargo nextest run -p eml_nrt` green; `grep -rn 'set_stack_limit(' crates/eml_nrt/src | wc -l` -> `1`; `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_nrt --lib -- entries:: census:: constants::` green (and with `-Zmiri-tree-borrows`); the two `user` times go to testing.md 「Miri」. Pitfalls: move the `RuntimeError` into `Worker` before returning the `Fatal`; `Image` gets `unsafe impl Sync` with the reason (read-only after `run` filled `data_table`).

**K7 eml_nrt services.** Files: `src/services.rs` (table `SERVICES`: `eml_nrt_safepoint`, `eml_nrt_failed`), `lib.rs` `TABLES`, `tests/services.rs`. Tests first: `services::a_stop_bit_is_fatal_with_the_function` (``interrupted in `f` ``), `services::a_notice_is_taken_and_the_run_continues`, `services::a_parked_fault_is_reported_at_its_site` (the stand-in passes its `FnRow`'s code address and site 0: `division by zero\n  at dir/file.em:3:16`), `services::a_site_without_a_position_names_the_function`, `services::a_parked_panic_reports_its_text` (through `park_panic`: the fault word is non-zero before the service and zero after), `services::a_raise_with_nothing_parked_is_an_internal_error`. Spec: 「安全点」 (service half, the native limits paragraph of D-5), 「extern の失敗」 (service half). Completion: `cargo nextest run -p eml_nrt services::` -> 6 passed.

**K8 eml_nrt heap entries.** Files: `src/heap.rs` (table `HEAP`: the 10 `eml_rt_*` of D-11), `lib.rs`, unit tests `entries::heap_*` (called from Rust, run under Miri), the echo additions of section 2's request in `crates/eml_native/tests/abi.rs`. Tests first: one per entry (`entries::heap_alloc_data_gives_a_data_object`, ..., `entries::heap_con_at_rewrites_the_header`), `entries::heap_fault_arith_parks`. Spec: 「エントリ」 gains the table of heap entries. Completion: `cargo nextest run -p eml_nrt entries::` green; the Miri command of K6 green; `cargo nextest run -p eml_native abi::` green.

**K9 The row list, the seed wrappers and the inline lines.** The grammar of `extern_rows!` is owned by the plan of `jit-externs` (appendix 04 D-9, its `names` row and the multi-variant line form); jit-core introduces the macro with it, including the 45 inline rows as `inline fn` lines and the test that the inline expander table equals the marked set (C-1):
```
list    := section*
section := "generic" "{" line* "}" | "dedicated" "{" line* "}" | "engine" "{" names "}" | "lir" "{" names "}" | "task" "{" names "}"
line    := "fn" Variant "as" Entry ("|" Variant "as" Entry)* "(" ctxpat ("," ident ":" kind)* ")" "->" ret block
         | "inline" "fn" Variant ("|" Variant)* "(" ctxpat ("," ident ":" kind)* ")" "->" ret block
ctxpat  := ident | "_"            kind := "obj" | "tobj" | "unit" | "int" | "enum" | "float"            ret := kind | "never"
```
`Entry` is always `eml_x_` + the variant name, written out because `macro_rules!` cannot build an identifier. An `inline` line gives the shared function and no entry. In a body `obj | tobj | unit` are `Word`, `int | enum` are `i64`, `float` is `f64`; the body is a block of type `Result<T, RunFault>` with `T` = `Word`, `()` (`unit`), `i64`, `f64`, or `Infallible` (`never`); errors convert with `?`. Each function is `pub unsafe fn <First><M: Mode>(ctx: &mut Ctx<M>, ..)`, named after the line's first variant; `defs` wraps the body in `unsafe { }`.
Files:
- `crates/eml_sched/src/rows.rs`: `#[macro_export] macro_rules! extern_rows { ($site:path) => { $site! { .. } } }` with the list below, and the expansions `defs` (the functions, plus per variant a const assertion that the line's kinds equal `Extern::<V>.row()`'s `params` / `ret`) and `names` (`pub const LISTED: &[Listed]` with `Listed { row: Extern, section: Section, entry: Option<&'static str>, inline: bool }` exactly as appendix 04 defines it (C-4; `entry` = the `as` identifier, `None` for an `inline` line), `pub enum Section { Generic, Dedicated, Engine, Lir, Task }`). The jit-core list (the bodies are the calls the same rows make in the VM: `run_extern`, eml_vm `vm.rs` L4186-4707, and for the inline rows the `eml_rt::externs` function the dedicated VM op calls; the VM stays untouched; copy each call):
  ```
  generic {
      fn Println as eml_x_Println(ctx, s: obj) -> unit { ctx.println(Stream::Out, s)?; Ok(()) }
      fn Eprintln as eml_x_Eprintln(ctx, s: obj) -> unit { ctx.println(Stream::Err, s)?; Ok(()) }
      fn IntShow as eml_x_IntShow(ctx, n: int) -> obj { Ok(ctx.int_show(n)) }
      fn StrShow as eml_x_StrShow(ctx, s: obj) -> obj { .. }
      fn FloatShow as eml_x_FloatShow(ctx, x: float) -> obj { .. }
      fn StrConcat as eml_x_StrConcat(ctx, a: obj, b: obj) -> obj { .. }
      fn StrEq as eml_x_StrEq(ctx, a: obj, b: obj) -> enum { .. }
      fn StrCompare as eml_x_StrCompare(ctx, a: obj, b: obj) -> enum { .. }
  }
  dedicated {
      fn FloatMod as eml_x_FloatMod(_, a: float, b: float) -> float { Ok(externs::float_rem(a, b)) }
      inline fn IntAdd(_, a: int, b: int) -> int { Ok(externs::int_add(a, b)?) }
      inline fn IntDiv | ByteDiv(_, a: int, b: int) -> int { Ok(externs::int_div(a, b)?) }
      inline fn IntEq | CharEq | ByteEq(_, a: int, b: int) -> enum { Ok(externs::int_eq(a, b)) }
      ..            // the 45 rows of rows::INLINE (D-10), grouped as the VM's `arithmetic()` groups them (emit.rs L1064-1103)
  }
  ```
  (`IntShow` stands alone here; jit-externs widens the line to `IntShow as eml_x_IntShow | ByteDisplay as eml_x_ByteDisplay` and adds the other lines and the `engine` / `lir` / `task` sections.)
- `crates/eml_nrt/src/rows.rs`: the `entries` expansion (table `ROWS`, appended to `TABLES`): for each variant of a line that is not `inline`, one `shim` entry whose function identifier is the line's `Entry` and whose parameter and result types are the line's kind tokens, passed to `entries!` as they are; its body is
  ```rust
  match rows::<First>::<Fast>(ctx, args..) {
      Ok(v) => Ok(to_word(v)),                                                      // Word -> bits, () -> Word::UNIT bits
      Err(RunFault::Extern(f)) if FAULTS => { ctx.set_fault(f); Ok(PLACEHOLDER) }   // FAULTS = const { Extern::<V>.row().faults }
      Err(e) => Err(e),
  }
  ```
  where the last arm is in fact `Err(e) => Err(unsafe { worker::record(ctx, e, None) })`, returning its `Fatal` (`FATAL_RECORDED`, reported with `function = "<native code>"`; `Raise` has no impl for `RunFault`, and no `Raise` impl may need the worker struct); and the hand-written `shim` entries `eml_x_Exit(ctx, code: int) -> never`, `eml_x_Panic(ctx, message: obj) -> never` (D-6; they return normally). The file never names `eml_fatal`. `crates/eml_nrt/src/lib.rs` re-exports the row functions of `eml_sched::rows` and `LISTED` (C-3; no eml_native -> eml_sched edge and no dev-dependency exemption).
- `crates/eml_native/tests/unsupported.rs`: `every_called_row_has_an_entry` (for each `rows::CALLED` row an entry named `eml_x_<Variant>` exists and its descriptor equals the row's kinds: `obj | tobj | unit | int | enum` -> `I64`, `float` -> `F64`, `never` -> no result); `the_inline_table_is_the_marked_set` (`rows::INLINE`'s rows = the `LISTED` rows with `inline`, read through the eml_nrt re-export; C-3).
Tests first: eml_sched `rows::tests::println_writes_a_line`, `rows::tests::the_listed_rows_are_the_seed_and_the_inline_rows` (9 + 45 rows with section and flag); eml_nrt `entries::every_row_entry_is_named_after_its_variant` (each `eml_x_` symbol of `ROWS` equals `format!("eml_x_{:?}", variant)` for a listed non-inline variant or `Exit` / `Panic`), `entries::x_println_parks_a_write_failure` (fault word non-zero, placeholder returned), `entries::x_str_concat_returns`, `entries::x_float_mod`, `entries::x_exit_starts_the_abort_and_returns`, `entries::x_panic_parks_the_message_and_returns` (worker slot set, `take_fault()` is `Fault::Engine`); in `mod fatal` (`cfg(not(miri))`): `fatal::a_row_error_of_the_internal_class_is_recorded` (a test-only list line whose body returns `Err(RunFault::Internal(..))` ends `enter` with `FATAL_RECORDED` and that error). Spec: 「extern の失敗」 (the wrapper protocol), 「行の下ろし方」 (the `inline` mark). Completion: `cargo nextest run -p eml_sched -p eml_nrt -p eml_native` green; `grep -c 'eml_fatal' crates/eml_nrt/src/rows.rs` -> `0`. Pitfall [V 9.9]: an optional `$($mark:ident)? fn` matcher is ambiguous in `macro_rules!` (`fn` is an ident to the matcher); expansions walk a section line by line with two arms, one starting with the literal `inline fn`, one with `fn`; kind tokens are matched as `tt` (`enum` is a keyword).

**K10 JIT host and straight-line functions.** Files: `lower/{mod, names, function}.rs` (params, atoms, `Return`, `ReturnMany`, `Jump`, block params, `Switch` on `int` / `enum` / constants), `mem.rs` (`UNIT`, `EMPTY_TOKEN`, `imm_bits`), `jit.rs` (`NativeMachine::{new, new_with_clif, load, call, run, clif, symbols, close}`, `execute`; run-interface types named as `eml_nrt::..`; `FnRow` rows with `code` / `size`; the module registers `eml_nrt::entries()` with `JITBuilder::symbol`), `Cargo.toml` dev-deps (`eml_test_support`, `eml_rt`, `insta`), `crates/eml_test_support/src/lib.rs` (`execute_native`; `execute_on(.., engine)` L376-386 and its callers, kind 3), `crates/eml_cli/src/lib.rs` (`Engine`, `execute_on`), `tests/{common/native.rs, machine.rs}`. No calls and no safepoint yet. Tests first: `machine::a_function_returns_its_argument`, `machine::many_values_return_in_order` (int, float, int), `machine::a_jump_passes_block_arguments`, `machine::an_int_switch_takes_the_default`, `machine::an_entry_with_two_results_is_refused`, `machine::a_second_load_is_refused`, `machine::symbols_cover_every_function` (D-16). Spec: 「関数の形」 (signature, value types, names, one load per machine). Completion: `cargo nextest run -p eml_native machine::` -> 7 passed; the standing greps. Pitfalls: Cranelift 0.137 deprecates `iadd_imm`, `icmp_imm`, `band_imm` (use the `_s` / `_u` forms; clippy fails otherwise); declare every function before defining any.

**K11 Called rows and the fault path.** Files: `lower/unwind.rs` (`Point`, `cleanup`, placeholders, `After::{Faults, Never}`, `eml$raise`, the per-function site list -> `FnRow.sites`), `lower/rows.rs` (call lowering), `mem.rs` (`fault`), `lower/function.rs` (`Rhs::Extern` dispatch), `tests/{rows.rs, abort.rs}`, pins `clif::a_faulting_row` (`Println`), `clif::a_row_that_does_not_fault` (`IntShow`), `clif::a_never_row` (`Exit`). Tests first (`agree`): `rows::println_of_a_shown_int`, `rows::float_show_and_float_mod`, `rows::string_rows_borrow_their_arguments` (`StrShow`, `StrEq`, `StrCompare`, then the argument is used again), `rows::a_write_failure_is_fatal_with_its_text` (a host whose stdout fails), `abort::exit_in_the_entry_gives_its_code`, `abort::exit_drops_what_the_frame_owns` (a shown string owned across `exit`; census), `abort::an_uncaught_panic_reports_the_message` (`panicked: boom\n  at ...`), `rows::the_same_function_has_the_same_code_in_two_programs` (a function with a faulting row and a `Never` row, loaded once alone and once after ten other functions with their own sites: the two `clif` texts are equal). Spec: 「巻き戻しの地点」 (cleanup, placeholders, the `Faults` / `Never` rows of D-4's table), 「extern の失敗」 (generated half). Completion: `cargo nextest run -p eml_native rows:: abort:: clif::` -> 11 passed.

**K12 Calls.** Files: `lower/call.rs` (`Call::Direct`, `LetMany`, `TailCall`), `lower/unwind.rs` (`After::Call`), `mem.rs` (`yield_target`), `tests/calls.rs`, pin `clif::a_call_and_its_unwind_check` (the callee is shown `colocated`). Tests first: `calls::a_direct_call_returns_its_result`, `calls::many_values_come_back` (`LetMany`), `calls::a_self_tail_call_runs_in_constant_stack` (10^6 iterations), `calls::a_tail_call_to_a_callee_with_more_stack_arguments` (12 parameters), `calls::float_parameters_and_results`, `calls::an_exit_in_a_callee_unwinds_the_caller` (the caller owns two shown strings, one with two references; census), `calls::an_exit_two_frames_down`. Spec: 「関数の形」 (calls, tail calls), 「巻き戻しの地点」 (`Call` row). Completion: `cargo nextest run -p eml_native calls:: clif::` green.

**K13 Entry safepoint.** Files: `lower/safepoint.rs`, `mem.rs` (`stop_bits`, `stack_limit`), `tests/machine.rs`, pin `clif::the_entry_safepoint`. Tests first: `machine::deep_recursion_hits_the_stack_limit` (``the call depth limit is exceeded in `f` ``; `Options.stack` = 64 MiB, so about 10^6 frames; expected under 100 ms in the dev profile [I]), `machine::a_stop_bit_stops_at_the_next_entry` (`Control::interrupt()` before the run), `machine::a_notice_does_not_stop_the_run`. Spec: 「安全点」 (emitted half). Completion: `cargo nextest run -p eml_native machine:: clif::` green; the K11 and K12 pins are re-accepted in this commit only for the added entry block (stated in the commit message).

**K14 Inline RC.** Files: `lower/rc.rs`, `mem.rs` (`is_imm`, `load_rc`, `store_rc`, `dup_is_local`, `decref_is_local`), `lower/function.rs` (`Stmt::Dup`, `Stmt::Decref`, `Rhs::Drop`), `jit.rs` (the hidden `NativeMachine::entry_of` and the `Drop` `SAFETY` comment), `crates/eml_nrt/src/run.rs` (one sentence of `run`'s `# Safety` widened to stand-in start thunks), `tests/rc.rs`, `tests/common/native.rs` (the helper that calls a function from a stand-in), pin `clif::inline_dup_and_decref`, testing.md 「`eml_native` の検証」 (one sentence on stand-in tests). Tests first: `rc::inline_dup_and_decref_agree_with_the_heap` (a stand-in start thunk on the run's worker calls the thunk taken with `entry_of`; the stand-in makes the objects in the run's heap and passes them to a function that dups / decrefs its argument; the test compares the object's rc word with that of a twin given the same operation by `Heap::dup` / `Heap::decref`, for local 1, local 2, a shared object (negative rc), a stuck object (`LOCAL_MAX`, rc written directly), an immortal string made by the test, an immediate; C-8), `rc::a_last_decref_frees` (census), `rc::drop_releases_one_reference`. Spec: 「RC」. Completion: `cargo nextest run -p eml_native rc:: clif::` green.

**K15 Data I: constructors, fields, switches, boxes, constants.** Files: `lower/data.rs` (`Con` without reuse, `Unpack`, case fields, tag `Switch`, `Box`, `Unbox`, `ConstString`, string `Switch`), `lower/consts.rs`, `mem.rs` (`load_tag`, `load_field`, `store_field`, `load_box`, `make_imm`, `imm_value`, `imm_round_trips`, `data_word`), `jit.rs` (data symbols, `Image.data` with slot addresses), `tests/data.rs`, pin `clif::a_tag_switch`. Tests first: `data::con_and_unpack_round_trip_every_field_kind`, `data::a_constructor_with_255_scanned_fields`, `data::switch_on_enum_obj_and_tobj`, `data::box_and_unbox_int_float_enum`, `data::boxing_an_int_agrees_with_the_heap` (`2^62 - 1`, `2^62`, `-2^62`, `-2^62 - 1`, `MIN`, `MAX`), `data::a_big_int_constant_and_a_float_box_constant`, `data::equal_constants_share_a_symbol`, `data::a_function_with_constants_has_the_same_code_in_two_programs` (as K11's test, with string, big-int and float-box constants), `data::a_string_switch_compares_in_order`. Spec: 「値と定数」 (boxes, the data table), 「データ」. Completion: `cargo nextest run -p eml_native data:: clif::` green.

**K16 Data II: release, tokens, reuse, struct boxes.** Files: `lower/data.rs` (the rest of D-8), `mem.rs` (`is_one_ref`, `is_big_int`, `token_of_box`, `box_of_token`), `crates/eml_nrt/src/heap.rs` and `entries.rs` (delete the entries `eml_rt_into_token` and `eml_rt_reuse_in_place`, which generated code no longer calls), `tests/{reuse.rs, abort.rs}`. Tests first: `reuse::release_frees_a_last_reference_and_drops_children`, `reuse::release_of_a_shared_value_dups_the_kept_fields`, `reuse::a_token_is_reused_in_place`, `reuse::a_token_of_another_constructor_is_rewritten`, `reuse::an_empty_token_allocates`, `reuse::skipped_fields_keep_their_words`, `reuse::drop_token_frees_the_box`, `reuse::unbox_struct_reads_leaves`, `reuse::release_unboxed_releases`, `reuse::a_big_int_box_counts_as_the_dummy` (a hand-built `BigInt` box passed as the struct box), `abort::exit_unwinds_two_frames_holding_owned_values_and_a_token` (hand LIR: `main` owns an object across `call f`; `f` holds an owned object and a live token across `call g`; `g` calls `exit(3)`; the test first asserts with `eml_lir::unwind::points` that `f`'s point has non-empty `owned` and `tokens` and `main`'s non-empty `owned`; then exit code 3, VM-equal output, census clean), `reuse::inline_token_moves_agree_with_the_heap`, `reuse::release_of_a_shared_or_stuck_value_gives_an_empty_token`, `reuse::an_in_place_reuse_calls_no_entry`. Spec: 「データ」 (release, reuse, dummy boxes, the token moves), 「ヒープのエントリ」 (the two entries removed). Completion: `cargo nextest run -p eml_native reuse:: abort::` green.

**K17 Inline rows.** Files: `lower/rows.rs` (expanders), `lower/unwind.rs` (`After::Arith`), `tests/rows.rs`. Tests first: `rows::every_inline_row_agrees_with_its_shared_function` (for each `INLINE` row a one-row LIR function called through `NativeMachine::call`; ints `0, 1, -1, 2, MAX, MIN, MAX-1, MIN+1, 2^32, -2^32, 3037000500`; floats `0.0, -0.0, 1.5, -2.25, INF, -INF, NAN, a negative NaN with payload, MIN_POSITIVE, MAX`; results compared bit for bit with the row's shared function `eml_sched::rows::<First>`, reached through eml_nrt's re-export (C-3; the one the VM's side will call); a faulting pair is expected to give the fault's text), `rows::an_overflow_reports_its_position`, `rows::a_fault_without_a_position_names_the_function`, `rows::an_overflow_drops_what_the_frame_owns` (no census after a fatal; the test checks the error only). Spec: 「行の下ろし方」. Completion: `cargo nextest run -p eml_native rows::` green; `cargo nextest run -p eml_native -j 1` shows no `SLOW` line.

**K18 `Session::compile_native` and `eml run --native`.** Files: `crates/eml_cli/src/lib.rs` (after `compile`, L203-206), `src/main.rs` (`Command::Run` L32-50; run branch L92-170), `tests/{api.rs, cli.rs}`, `docs/spec/diagnostics.md` (L56 row, 「未対応の構文と E0004」), `docs/spec/native.md` 「VMとの違い」 entry (3) with the one-worker rule including `EML_WORKERS` and `EML_GUI` (C-7, C-11), `docs/implementation/architecture.md` 「CLI と lib API」. Tests first: `api::compile_native_reports_each_rejected_form_once` (inline snapshot: a program with a handler/`prompt` and a `par` gives E0004 for each such form once at `main`; use only forms that stay rejected past jit-apply, as the jit-core plan's K20 says), `api::compile_native_keeps_an_accepted_program`, `cli::native_runs_a_program`, `cli::native_rejects_with_e0004_and_exit_1`, `cli::native_conflicts_with_debug_heap_and_stats` (exit 2), `cli::native_refuses_workers_other_than_one_and_gui` (exit 2 for `--workers 2`, for `EML_WORKERS=2` without `--workers` (C-7), for `--gui` and for a set `EML_GUI` (C-11); `--workers 1` is accepted; no message names a stage, C-10), `cli::native_exit_code_is_the_low_byte`. Completion: `cargo nextest run -p eml_cli --test integration -E 'test(cli::native) | test(api::compile_native)'` -> 7 passed; `grep -rn 'not_yet_supported' crates/eml_cli/src | wc -l` -> `1`. Left to the implementer: reading the range of `main`'s name from HIR (the range other diagnostics put on a function name; else the item's range).

**K19 The running half of the native UI tests.** Files: `crates/eml_cli/tests/ui.rs` (`execute_in` L199-263 gains `engine`, callers pass `Engine::Vm`, kind 3; steps 4-5 of D-15), `tests/common/mod.rs` (`EML_UI_TIMES`: `run_all` prints `<ms> <path>` per file), testing.md 「ネイティブで流す」 (the comparison, the census, the checked run for run-fail). Completion: the K5 command -> 2 passed with **no snapshot change** (`git status --short crates/eml_cli/tests/snapshots` empty); `RUST_TEST_THREADS=1 EML_UI_TIMES=1 cargo nextest run -p eml_cli --test integration -E 'test(=ui::run_native) | test(=ui::run_fail_native)' --no-capture 2>&1 | sort -n | tail -1` shows a time below `1000`; the slowest five lines go into the plan file in this commit. A differing file: fix the lowering; a mark only by 1.0's rule.

**K20 Native bench and CI.** Files: `bench/native.sh` (D-18), `docs/implementation/benchmarks.md` (heading 「ネイティブの数値」, the method, the first table), `.github/workflows/ci.yml`: in the miri job the eml_nrt step's filter becomes `-- entries:: census:: constants::`; in the asan job nothing changes if jit-rt's `-p eml_nrt -- --skip fatal::` and `-p eml_native -- --skip fatal::` steps exist, else they are added with those exact arguments; CLAUDE.md Commands (`bench/native.sh`, `run --native`). Completion: `test -x bench/native.sh`; it prints 42 rows, 25 with numbers and 17 `rejected`; the commit's CI run is green on ubuntu-24.04, ubuntu-24.04-arm, macos-15, windows-2025 including `cargo nextest run --profile timing -j 1`. A red target: fix in a follow-up commit of this stage; if the cause is an item of A-1 / A-3 use its named fallback; anything else stops the stage and goes to the user. The stage does not close with a red target.

**K21 `Close jit-core`** (roadmap 「進め方」): (1) spec/native.md has no stage names or "later" notes; (2) K1 / K2 touched eml_vm and K3 eml_rt: run `bench/scripts.sh`, `bench/run.sh`, `bench/rt.sh` on the last code commit and replace the three tables of benchmarks.md 「今の数値」; re-run `bench/native.sh`; (3) status.md: what `--native` runs and rejects, the differences of 「VMとの違い」, one worker and no `--gui` natively; (4) architecture.md: 「`eml_native` の内部」 (D-1), eml_nrt internals, testing.md 「`eml_native` の検証」; (5) roadmap.md: remove the jit-core bullet; to 「その後の項目」 with 後に回した理由 / 見直す条件: `--stack-size`, the forgone optimisations of D-17; (6) overview.md / README.md map if changed; (7) CLAUDE.md Architecture lines; (8) delete the plan; (9) message: docs moved, test changes (kind 1: the 8 marks and reason; kind 3: K1, K2, K10, K19), the timing and bench numbers, deferred items.

**Order**: K0, K1, K2, K3, K4, K5, K6, K7, K8, K9, K10, K11, K12, K13, K14, K15, K16, K17, K18, K19, K20, K21. Each task uses only code of earlier tasks.

**Completion bar**: the two snapshots (1.0's numbers or the revised ones by its rule, `accepted` >= 150); `Form` has exactly the 11 variants; the 7 pins, the abort test, the inline-row test and the symbol test pass; K3's eml_rt gates; K19's timing; K20's CI; native numbers recorded (the `bench/native.sh` table in benchmarks.md); the standing greps.

### 7.2 Stage jit-apply

**Base**: `Close jit-core`. Checks: `cargo nextest run` green; `grep -c 'Closure\|FnValue' crates/eml_native/src/unsupported.rs` > 0. Baselines: K19's timing command; `bench/native.sh`.

**A0 Plan commit.** D-19..D-24, section 2's rows for closures, 7.2. Test changes: kind 1 = three unit tests deleted in A5 (`unsupported::closure_is_rejected`, `fn_value_is_rejected`, `apply_is_rejected`; the variants no longer exist); kind 2 = the two native snapshots (`closure` and `fn value` buckets move to `accepted`).

**A1 Runtime side.** Files: eml_nrt `image.rs` (`DATUM_FN`, `DatumRow.code`, `FnRow.value`, `PapRow`, `Image.paps`), `constants.rs` (static closures: `alloc_immortal(1, layout::data_meta(n, 1, false))`, `set_field(first, Word::imm(code))`, with the `Word::int` assert), `closures.rs` (table `CLOSURES`: `eml_nrt_pap`), tests. Tests first: `entries::a_static_closure_is_immortal_with_its_arity_tag`, `entries::pap_holds_the_callee_and_arguments` (tag `r`, scanned `2 + m`, `lin` copied), `entries::pap_without_an_adapter_is_fatal` and `entries::pap_of_a_saturated_callee_is_fatal` (in `mod fatal`, `cfg(not(miri))`). Spec: 「クロージャ」 (object form, static closures, PAP form, who may decode). Completion: `cargo nextest run -p eml_nrt` green; the Miri command green.

**A2 Function values and exact application.** Files: `lower/apply.rs` (`c0` adapters, the apply site, `eml$apply$m` with all three arms; the `n > m` arm calls `eml_nrt_pap`, which is fatal until A4 emits PAP adapters), `mem.rs` (`closure_code`, `spill`), `lower/consts.rs` (`Datum::Fn`, `fn_value`, symbol `v:<name>`), `lower/function.rs` (`Atom::Fn`, `Call::Apply`), `jit.rs`, `tests/apply.rs`, pin `clif::an_apply_site`. Tests load programs with `NativeMachine::load` directly (`unsupported` still rejects them, so no UI number moves). Tests first: `apply::a_static_function_value_takes_no_rc`, `apply::a_tail_apply_runs_in_constant_stack`, `apply::an_apply_result_is_checked_for_an_abort`, `apply::an_excess_application_applies_the_result`, `apply::an_abort_inside_an_excess_application_drops_the_rest`. Spec: 「クロージャ」 (entries, the apply site, `eml$apply$m`). Completion: `cargo nextest run -p eml_native apply:: clif::` green.

**A3 Closures with captures.** Files: `lower/apply.rs` (`MakeClosure`, `c<k>` adapters, the uniformity assertion of D-20), `lower/function.rs`, `tests/apply.rs`. Tests first: `apply::a_closure_is_freed_on_its_last_use`, `a_shared_closure_dups_its_captures`, `raw_captures_keep_their_bits`, `a_closure_with_255_captures`. Completion: `cargo nextest run -p eml_native apply::` green.

**A4 Partial application.** Files: `lower/apply.rs` (`eml$pap$m$r`, the bound `A`), `jit.rs` (`Image.paps`), `tests/apply.rs`. Tests first: `apply::a_partial_application_waits_for_the_rest`, `a_partial_application_of_a_partial_application`, `a_lin_closure_stays_lin_through_a_partial_application`. Spec: 「クロージャ」 (PAP adapters). Completion: `cargo nextest run -p eml_native apply::` -> 12 passed.

**A5 The forms leave `Unsupported`.** Files: `unsupported.rs`, `tests/unsupported.rs` (the three deletions), the two snapshots (kind 2), `docs/spec/object-model.md` L27, L31 and 「クロージャ」 (the function-value immediate and the packed head word are the VM's; native's are in spec/native.md), spec/native.md 「断る形」, diagnostics.md. Completion:
```sh
cargo nextest run                                                                          # green
sed -n '/^---$/,$p' crates/eml_cli/tests/snapshots/integration__ui__run_native.snap | tail -n +2        # accepted 208 / skipped 6 / rejected 421 / async row 128 / c row 2 / par 22 / prim 156 / std row 113
sed -n '/^---$/,$p' crates/eml_cli/tests/snapshots/integration__ui__run_fail_native.snap | tail -n +2   # accepted 8 / checked only 0 / skipped 2 / rejected 71 / async row 33 / c row 2 / par 1 / prim 3 / std row 32
grep -c 'Closure\|FnValue' crates/eml_native/src/unsupported.rs                            # 0
```
(1.0's rule applies to a file that cannot be made identical.) K19's timing command: slowest file below 1000.

**A6 `Close jit-apply`.** (1) spec final pass; (2) no VM measurement (nothing under `crates/eml_vm`, `eml_rt`, `eml_lir`, `eml_core_ir`, `std/`, `bench/*.em` changed); `bench/native.sh` re-run and the 「ネイティブの数値」 table replaced (27 rows with numbers); (3) status.md; (4) architecture.md (`lower/apply.rs`, `eml_nrt/closures.rs`); (5) roadmap.md: remove the bullet; (6)-(7) only if changed; (8) delete the plan; (9) message: kind 1 and kind 2 changes with reasons, timing and bench numbers.

**Order**: A0..A6. **Completion bar**: snapshots (`accepted` >= 185); the 12 `apply::` tests and the pin; `Form` has 8 variants; CI green on four targets; native numbers recorded.

## 8. Unresolved items

| Item | Default | Who / when |
|---|---|---|
| `--stack-size` | not offered; 1 GiB | user, by jit-tasks |
| `--workers N` (N != 1) and `--gui` with `--native` | usage error | jit-tasks lifts it |
| The 5 `max-bytes` files natively | not marked | jit-tasks, when their forms are accepted |
| Name `RunFault` | as written | plan review |
| Single-load safepoint, leaf safepoint elision, compare-branch fusion | forgone | native-inline, with `bench/native.sh` numbers |
| Appending loads | one load per machine | jit-repl |
| x86_64 behaviour of A-1's items | CI of the task decides; fallbacks named | implementer |

## 9. Additional checks

1. **Classifier** [V] (`scratch-S2b/crates/eml_cli/examples/forms.rs`, D-12's rules, 635 run + 81 run-fail + 42 bench programs, all compile): the numbers of 1.0; entry result Repr `unit` in all 708 unmarked entries; constructs in jit-core-accepted run files: reuse `con` 19 files, token `release` 19, `LetMany` 13, `UnboxStruct` 10, `release unboxed` 4, string `switch` 3, `prim` 0; statement counts of the 208 files accepted at jit-apply: median 23, p90 133, max 1592, sum 13,401; per program over the 629 unmarked run programs: largest missing-argument count `A` = 0 (282), 1 (44), 2 (138), 3 (164), 4 (1); largest apply argument count 0 (288), 1 (36), 2 (302), 3 (1), 29 (1), 30 (1); seed variants: without `panic` run-fail drops to 6; with `Std.FileIn.open` + `close` 206 / 242.
2. **Cranelift probe** [V] (`scratch-S2b/crates/eml_native/tests/probe2.rs`, aarch64-apple-darwin, Cranelift 0.137.1, dev profile, 3 tests pass): data symbol read from a `tail` function and filled through `get_finalized_data`; a closure with head `func_addr << 1 | 1` and the tag in the meta word applied by tag compare + `return_call_indirect`; `Switch`; a `(i64, f64)` call followed by the unwind check with a cold cleanup block; an explicit stack slot as `argv` to a default-cc Rust function; `sadd/ssub/smul_overflow`, `sdiv`, `srem` against Rust over an 11 x 11 matrix; `fadd` + NaN select. Module + 40 functions + finalize + `free_memory`: 7.0 ms per round (200 rounds); 1 GiB-stack thread spawn + join: 23 us.
3. **`Heap::is_immortal`** [V] (`Fault::Engine` was not built [I]): added as in D-13 to `heap/walk.rs`; `cargo verus verify -p eml_rt`: `1221 verified, 0 errors` (57 s).
4. **Rename** [V]: `grep -rn VmFault` over Rust, snapshots and docs: 400 lines, 33 files, 0 in `.snap`.
5. **Marks** [V]: `max-depth` on exactly the 8 files of D-15; `max-bytes` on 5 (4 `async`, `run/limits/soft_limit_without_a_task.em`, the only accepted file with any mark); `windows:` on 3, none accepted.
6. **Tag readers** [V]: `grep -rn '\.tag()' crates/eml_rt/src | grep -v /tests/`: 20 sites, classified in D-19.
7. **Libcalls** [V]: `cranelift-codegen-0.137.1/src/ir/libcall.rs` `enum LibCall` has no `fmod`.
8. **LIR of a token across `exit`** [V]: `tests/ui/run/unwind/reuse_token_exit.em`: `run` has unwind points with `tokens: [r.29]`; `exit` is followed by `return t.4`.
9. **Row-list grammar** [V] (`scratch` `S2b-work.ZaL0/m/m.rs`, rustc 1.99.1): a `macro_rules!` list with `fn V as eml_x_V | W as eml_x_W (ctx, a: int) -> obj { .. }`, `inline fn A | B(_, ..) -> int { .. }`, a `never` line and an `enum` kind expands through a `defs` consumer (functions) and an entry-name consumer (`eml_x_` identifiers only for non-inline lines); the optional-ident form of the `inline` mark is rejected as ambiguous, the two-arm line walker compiles and runs.
10. **Not checked**: per-name data symbols beyond the single data object of 9.2, and `func_addr` of the function being defined [I: the same relocation kinds as 9.2's data symbol and `func_addr`]; x86_64 and Windows (A-1, A-3); real lowering time; identity of the accepted files (A-2); the Miri time of the new eml_nrt unit tests; the exact final names of jit-rt / sched-split / the `extern_rows!` grammar (A-4, A-5).
