# Appendix 04: jit-externs (design level)

(Where this file and 00-decisions.md differ, 00-decisions.md wins.)

Section C of 00-decisions.md is folded into this text; 00 remains the record.

# S3 response (r4): design-level plan for stage `jit-externs`, and the row-list macro that jit-core introduces

Part 1 only. Applies X-resolutions-r1 (R-1, R-2, R-4 for S3; R-3, R-5, R-6, R-13 where they touch this design) on top of r3, whose changes from r2 (contract-r3 ruling) are kept.

## Changelog from r3

1. **R-1, entry identifiers.**
   - Each variant of a list line names its entry identifier with `as`: `fn IntShow as eml_x_IntShow | ByteDisplay as eml_x_ByteDisplay (ctx, n: int) -> obj { .. }`.
   - The `entries` expansion passes `eml_x_<V>` to `entries!` as the function identifier. Kind tokens, `never` included, go straight to `entries!` (S2a r3 accepts them).
   - There is no token muncher for kinds and no `symbol =` request. The requests to S2a are closed, and r3's A-2 and U-S3-7 are removed.
   - A unit test checks that every entry identifier is `eml_x_` + its variant (N-8).
   - Grammar 1.2 and the D-1 example are rewritten. The grammar was compiled against S2a's r3 `entries!`, see 7.10.
2. **R-2, fatal codes.** `FATAL_RECORDED` is **3** (jit-core). S2a's codes are 1 = caught panic and 2 = `FATAL_FAULT`. `impl Raise for HeapError` stays S2a's (D-3, A-4, 1.3).
3. **R-4, inline marker.** A line prefixed `inline` gives the shared `rows::` function (the VM and N-27's differential test call it) and a `names` row marked inline, but no entry. eml_native's inline-expander table is tested equal to the set of `inline` lines (jit-core's test, 00 C-1). D-1, D-9c, 1.2 and the expansion table no longer refer to S2b's `Inline` set from eml_nrt.
4. **R-3, `never`.** A `never` entry returns normally with no result (Rust `()`; the descriptor has no result). Generated code branches unconditionally to the cold block after the call. Applied to D-3 and D-9c.
5. **R-6, no load-order numbers in code.** The C helpers no longer take a C-function or handle-type number. Generated code passes the address of a per-name data symbol (`symbol_value`, a relocation) that the load fills with a pointer to the machine's entry for that name (D-8, 1.3). Fault sites follow S2b's function-relative scheme.
6. **R-5 / R-13 noted.** S4 drops its separate `async_rows!` and uses the `task` section (D-12, unchanged). The floor ≥ 323 / ≥ 42 supersedes S1's "about 308" (D-10).
7. **1.2:** S2b's K9 snippets use this grammar (R-1). The r3 sentence about an "r1 spelling" in K9 is removed.

Tags: [V] = verified (method in section 7), [I] = inferred. Code citations are against base 432fffb4 (scratch clone `scratch-S3`). A line number is a hint; the named item decides. This document uses the names the other r1 / r2 designs and contract r3 fixed: `RunFault` (S2b renames `VmFault` in jit-core), `eml_nrt::entries!` with its `plain` / `shim` forms, `Raise`, `Fatal`, `eml_fatal(ctx, code, site)` (S2a), `Form`, `Image`, `Worker`, `eml$raise`, `eml_nrt_failed`, `After::{Faults, Never}` (S2b), `worker::park_panic`, `worker::record`, `FATAL_RECORDED`, `Fault::Engine` (S2b r2, contract r3 1.4). Names marked *illustrative* may change at the executable refresh.

---

# Part 1: revised deliverable

## 1. Conclusion

### 1.0 Facts the decisions rest on [V: 7.1, 7.2, 7.4]

| Set of `Extern` rows | Count | Native lowering at jit-externs close | VM today |
|---|---|---|---|
| all | 334 | | |
| `is_async()` | 101 | rejected (`Form::AsyncRow`) until jit-tasks | `Op::Async` |
| non-async, generic VM instruction | 162 | `Call eml_x_<V>` | `run_extern` arms (vm.rs L4186-4707) |
| non-async, dedicated VM op, inline natively | 45 | `Inline` (S2b's table: VM `arithmetic()` minus `FloatMod`) | `Op::IntAdd`, ... |
| non-async, dedicated VM op, called natively | 23 | `Call eml_x_<V>` | `Op::ArrayGet`, ..., `Op::FloatRem` |
| engine-specific | 2 | `eml_x_Panic`, `eml_x_Exit` (S2b, hand-written) | `run_extern` prelude |
| not an extern call in LIR | 1 | `par` (jit-tasks) | `par` |

The 23 dedicated rows called natively are 22 container rows plus `FloatMod`. Properties of the 233 non-async rows:
- `faults` is true exactly when purity is not `Pure`.
- `writes` is false and `held` is empty on every row.
- 79 rows borrow an argument and 36 have `with_args`.
- 3 rows return `Never`.
- At most 5 parameters; 15 ABI shapes.
- All 25 `Std.Tls` / `Std.Gui` rows are `Effectful`.

### 1.1 Decisions

**D-1. One row list for every row: `extern_rows!` in `eml_sched/src/rows.rs`.** The list names all 334 rows exactly once, in five sections. The two sections with bodies use the grammar of 1.2. Each consumer passes its own expansion macro:

```rust
// eml_sched/src/rows.rs (illustrative spelling of the section heads; the grammar is fixed in 1.2)
#[macro_export]
macro_rules! extern_rows { ($site:path) => { $site! {
    generic {      // the VM's generic extern instruction; native Call
        fn Println as eml_x_Println (ctx, s: obj) -> unit { ctx.println(Stream::Out, s)?; Ok(()) }
        fn IntShow as eml_x_IntShow | ByteDisplay as eml_x_ByteDisplay (ctx, n: int) -> obj { Ok(ctx.int_show(n)) }
        fn GuiEndRaw as eml_x_GuiEndRaw (ctx, ui: obj) -> obj { gui::arm(ctx, Extern::GuiEndRaw, &[ui]) }
        fn FailIo as eml_x_FailIo (ctx, kind: enum, message: obj) -> never { fail_io(ctx, kind, message) }
        ...
    }
    dedicated {    // a dedicated VM op calls the function; native `inline` lines are CLIF, the others Call
        inline fn IntDiv | ByteDiv (_, a: int, b: int) -> int { Ok(externs::int_div(a, b)?) }   // from jit-core (00 C-1)
        fn ArrayGet as eml_x_ArrayGet | MutArrayRead as eml_x_MutArrayRead (ctx, a: obj, i: int) -> tobj { Ok(ctx.array_get(a, i)?) }
        ...
    }
    engine { Panic, Exit }              // hand-written in both engines
    lir { Par }                         // never an extern call
    task { AsyncStartRaw, ..., GuiIsGuiTaskRaw }   // 101 names; bodies arrive in jit-tasks (D-12)
} } }
```

There are four expansions, all of them `macro_rules!` written next to their consumer:

| Expansion | Crate | What it emits | Lands in |
|---|---|---|---|
| `defs` | eml_sched (`rows.rs`) | One `#[inline] #[allow(unused_unsafe)] pub unsafe fn <First><M: Mode>(ctx: &mut Ctx<M>, ..) -> Result<R, RunFault>` per line (`inline` lines included), named after the line's first variant, with the shared `# Safety` doc and the body inside `unsafe { }`. For every variant of the line, a `const` assertion that the line's kinds equal `Extern::<V>.row()`'s `params` / `ret`. | jit-core |
| `entries` | eml_nrt (`rows.rs`) | One entry per variant of every line not marked `inline`, through S2a's `entries!` `shim` form. The function identifier is the variant's `as` identifier (`eml_x_<V>`), the kind tokens are passed as written, and the body is D-3's. The expansion is a muncher (line by line, then variant by variant) that skips `inline` lines. | jit-core |
| `vm_arms` | eml_vm (`vm.rs`) | The whole `match ext` of `run_extern`. `generic` lines become arms that read operands by kind, call `rows::<First>`, apply `?`, and write `dst` by kind. `dedicated`, `engine`, `lir` and `task` lines become today's `Internal` arms with today's texts. The match is exhaustive and has no wildcard. | jit-externs |
| `names` | eml_sched (`rows.rs`) | `pub const LISTED: &[Listed]` with `Listed { row: Extern, section: Section, entry: Option<&'static str>, inline: bool }` (`entry` = the `as` identifier, `None` on `inline` lines and in `engine` / `lir` / `task`), used by the tests of D-9. eml_nrt re-exports `eml_sched::rows` and `LISTED`, and eml_native's tests reach them through eml_nrt (00 C-3); there is no eml_native → eml_sched edge | jit-core (the struct, the seed and inline rows: 00 C-4), jit-externs (all) |

Why:
- N-22 asks that each row's implementation exist once and that both engines get their code from one macro over the table.
- Compiling each body once, as a named function, keeps both engines' sites as plumbing. It also lets the compiler check the kinds and, through the exhaustive VM match, the completeness of the list.

Rejected:
- Splicing bodies at each site: the code is compiled twice and errors are unreadable.
- Putting body tokens into `eml_extern`'s `table!`: the crate is dependency-free, and its rows are const expressions, not tokens.
- A wildcard arm for the async rows: it loses the exhaustiveness check that emit.rs L1063 relies on.

Conformance: N-22 is met by the three expansion sites plus the dedicated-op call sites of D-2. N-21 and N-8 are met by the tests of D-9.

**D-2. The dedicated VM ops call the shared functions.** Each dedicated op handler in `vm.rs` (L1203.., L1326-1360) replaces its direct `externs::*` / `Ctx::*` call with `rows::<First>::<R::M>(ctx, ..)?`. Here `<First>` is the line whose variants are the rows that op covers. A `dedicated` line groups exactly the rows emit.rs maps to one op, and the 45 rows S2b inlines are the `inline` lines that jit-core already listed (R-4, 00 C-1):
- `inline IntEq | CharEq | ByteEq`
- `inline IntDiv | ByteDiv`
- `ArrayGet | MutArrayRead`
- `UnboxedArrayGetInt | MutUnboxedArrayReadInt`, and the `Float` pair, whose body is `rows::UnboxedArrayGetInt(..).map(|b| f64::from_bits(b as u64))`
- and so on

Every row thus has one body that both engines run. `Inline` rows run as CLIF in native, and S2b's N-27 differential test already compares each inline expander with the row's `rows::` function from jit-core on (03 K17); once this stage makes the dedicated VM ops call the same functions, the VM's semantics is the reference. The calls are `#[inline]`, generic and cross-crate, so I expect the same machine code [I]. `bench/run.sh` is recorded at close and does not gate: the native-performance premise accepts VM cost for one shared definition. `RunStats` and every VM expected value stay byte-identical (kind 3).
Rejected: a differential test over the 22 container rows plus an N-22 challenge. Two hand-written mappings would still drift; one function is simpler.

**D-3. The wrapper protocol.**
- **Symbol.** `eml_x_<Variant>` (`format!("eml_x_{e:?}")`). This is the same rule as S2b's seed rows; it is injective because variant names are unique.
- **ABI.** `ctx` first. `obj | tobj | unit | int | enum` → `i64`; `float` → `f64`. Result: the same, or nothing for `never`: the entry returns normally with no result and the descriptor has `result: None` (R-3); generated code branches unconditionally to the site's cold block after the call.
- **Body.** Inside S2a's `shim` (where `ctx: &mut Ctx<Fast>`):

```rust
match rows::<First>::<Fast>(ctx, args..) {
    Ok(v) => Ok(to_word(v)),                                        // Word -> bits, () -> Word::UNIT bits
    Err(RunFault::Extern(f)) if FAULTS => { ctx.set_fault(f); Ok(PLACEHOLDER) }   // Word::UNIT bits / 0.0
    Err(e) => Err(e),                                               // impl Raise for RunFault -> eml_fatal
}
```

Here `FAULTS` is `const { Extern::<V>.row().faults }`. For `never` rows the body is `Infallible`, so the `Ok` arm is `match n {}`, and the parking arm returns `Ok(())`. With S2a's shim, the remaining `Err(e)` and any caught panic go to `Raise::park` and then `eml_fatal`. `impl Raise for RunFault` (jit-core's) is `worker::record(ctx, self, None)` (`worker::record` takes `Option<At>`, 00 C-2): it records `RuntimeError::Fault { fault: e, function: "<native code>", at: None }` in the worker struct and returns `Fatal { code: FATAL_RECORDED (= 3), site: 0 }`. Codes 1 (caught panic) and 2 (`FATAL_FAULT`) are S2a's (R-2); all three are defined in `crates/eml_nrt/src/enter.rs` (00 C-5). This is the internal-error class (S1 U7). The wrapper body never names `eml_fatal`, so `grep eml_fatal crates/eml_nrt/src/rows.rs` stays empty.

What reaches the fatal arm [V: 7.4]:
- The row bodies produce only `RunFault::Extern(..)`, either from `Fault` or from `From<HeapError>` (`error.rs:55-59`).
- The 16 `RunFault::Internal` sites in `tls.rs` / `gui.rs` are implementation bugs.
- No row returns `Stopped`, `Driver`, `Park` or `Deadlock`. `Fault::Stopped` (for example an interrupted stdin read) travels inside `Extern` and is parked. `eml_nrt_failed` then classifies it exactly as VM `failed` does.
- A non-`faults` row never errs in fast mode (D-5).

So the fatal arm holds only bugs and caught panics.

**D-4. Native cost of a std row** (performance premise). For a `Call` row, the fast path is:
1. Arguments are already in registers.
2. One `call` of the imported entry. For cranelift-jit's non-colocated imports this is an address load plus an indirect branch [I].
3. In the wrapper: the prologue, `catch_unwind`, which costs nothing on the non-panicking path because Rust unwinding is table-based [D: S2a's `entry-shim` / `entry-plain` rows], and the inlined `rows::` body, which calls the eml_rt method.
4. Back in generated code: for `faults` rows, **one load of `ctx.native.fault` and one branch**; for non-`faults` rows, nothing.

There is no `yielding.target` check after an extern, because S2b only checks after `Rhs::Call`. No argument is spilled for the ABI: every parameter is an `i64` / `f64` register argument (at most 5 + `ctx`).
- This is minimal for "one Rust function per row". The remaining costs are the call and the wrapper frame. native-inline removes them for the rows that are worth it (D-11).
- **`catch_unwind` cost**: S2a measured `shim` against `plain` entries at 818,705,818 vs 818,687,270 instructions per 10^8 calls (contract r3 1.7), i.e. no difference on the fast path. So every row wrapper is a `shim`, and N-10 lists no exception.
- **Measurement in this stage (record, not a gate)**: an `#[ignore = "timing"]` test `rows::extern_call_cost` reports ns per call of `eml_x_StrByteLength` from JIT code, and the number goes into spec/native.md 「extern の行」. At Close, the native rows of `bench/scripts.sh` and a re-run of `bench/native.sh` (N-39).
- **No per-row `plain` wrappers** (forgone, D-13): it would be a special case per row for a cost the shim very likely does not have.

**D-5. Invariant: a non-`faults` row cannot fail in fast mode.**
- [V: 7.2, 7.4] Every `Ctx` method a `Pure` row calls ensures `!M::CHECKED ==> r is Ok` or `r is Ok`. Every `eml_rt::externs` function a `Pure` row calls returns a plain value (no `Result`).
- `ProcStageCount` can fail only with `WrongKind` on a wrongly typed handle, which the types exclude.
- The one real exception is `bytes_empty` (`Std.Bytes.empty`). It is `bytes_make(0, 0)`, which fails with `Fault::Stopped(Stop::MemoryLimit)` when `over_budget(&control, 16)` (`eml_rt/src/heap/ctx/bytes.rs:937-938`). `array_empty` and `unboxed_empty` cannot fail (`ensures r is Ok`).
- jit-rt makes `bytes_empty` infallible (`ensures r is Ok`; S2a task R4, verified without proof work). jit-externs therefore edits no eml_rt file (N-29 does not apply to it).
- Effect on the VM, checked for R4: behaviour changes only when a run exhausts its budget exactly at `Std.Bytes.empty`. No UI test exercises that: none of the 5 `max-bytes` UI files uses `Bytes.empty` [V: grep]. No eml_rt test asserts it: `heap/tests/bytes.rs:692` expects `Ok`, and the `MemoryLimit` cases in `heap/tests/externs.rs:1505-1513` are file reads [V].
- D-9b pins the invariant.

**D-6. Rows that call back closures: none outside `Par`.** No row body runs eml code [V: 7.4]:
- Every body calls an eml_rt method, an `eml_rt::externs` function, `tls::*` or `gui::arm`.
- `tls.rs` and `gui.rs` import neither `apply` nor `vm`.
- The `tobj` parameters of the array makers and writers are stored, not called.

`Par` takes two closures that may yield, but it is LIR `par` (jit-tasks). So no `eml_x_` entry re-enters eml code in jit-externs, and N-12's list of re-entering entries gains nothing.

**D-7. `Std.Tls` and `Std.Gui` move to eml_sched.**
- `tls.rs` moves whole. `gui.rs` moves whole: `GUI_SLOT`, `Slot`, `UiToken`, `GuiPark`, `park`, `arm`, `is_gui_task`, `forget_opening`, the `off` modules and the unit tests.
- Why whole files: the arms share private types with the async helpers (`TlsConn`, `Slot`). Splitting would make every field `pub` across crates.
- The four functions that take `&mut Scheduler<P>` (`want`, `open_ui`, `abandon`, `lower_holds`) move with unchanged bodies, generic over `GateHost` (illustrative): `own_driver() -> Result<(), RunFault>`, `open_gate() -> i64`, `release_gate(i64) -> Result<(), RunFault>`, `swap_holds(bool) -> bool`, `holds() -> bool`, `wake(u32)`. Those are exactly the calls they make today (`gui.rs:36-55, 462-534`) [V]. eml_vm implements `GateHost` for `Scheduler<P>`. jit-tasks may replace the trait by its `WorkerCore` (S4's D-7), at a cost of about 10 lines.
- `GUI_SLOT` stays a `thread_local!` in eml_sched. The toolkit is bound to the main OS thread, `GuiPark` shares the slot, and N-5 forbids thread-locals only in eml_nrt and eml_native.
- Features, as S2a's D-20 wrote them: eml_sched `tls = ["dep:eml_tls", "eml_io/tls"]`, `gui = ["dep:eml_gui"]`, `gui-native = ["gui", "eml_gui/native"]`. eml_vm forwards `tls = ["eml_sched/tls"]`, `gui = ["eml_sched/gui"]` and `gui-native = ["gui", "eml_sched/gui-native"]`, and keeps `eml_tls` / `eml_gui` only as dev-dependencies under the same features if its tests name them. eml_nrt and eml_native forward the three names downward. eml_cli's lines are those of S2a's D-20.
- `eml_vm::gui_park` and `eml_vm::eml_gui` become re-exports from eml_sched.
- The 25 native entries run the same `rows::` functions the VM's UI tests run. Natively they become reachable only with async rows (jit-tasks), so status.md says exactly that (D-10).

**D-8. Non-blocking user C rows: direct calls.** A `Rhs::Extern(ExternRef::C(id))` lowers in five steps:
1. **Narrowing checks, inline, before anything else.** `I32`: `sextend(ireduce.i32(v)) == v`; `SizeT`: `v >= 0`. The cold path calls `eml_x_c_narrow(ctx, cdesc, arg_1based, value)`, which parks the panic through `park_panic` (step 5), then `eml$raise(ctx, function, site, Panics)` (`function` = the calling function's code address, `site` its local site index; 00 C-2) and the site's cleanup. A C row with a narrowing parameter has an unwind point (`c.rs:244, 321`) [V].
2. **Argument words.**
   - `Int` / `Float`: passed as is.
   - `NoArg`: passed as no argument.
   - Text / unboxed pointers: `eml_rt_text_payload(ctx, w)`, `eml_rt_unboxed_ptr(ctx, w)`, `eml_rt_unboxed_ptr_mut(ctx, w)`. These are fatal-only heap entries that expose provenance (T16). In fast mode they return `Err` only for a wrong kind (`heap/string.rs:811-821`, `heap/unboxed.rs:690-739`) [V].
   - Handle pointer: `eml_x_c_handle_ptr(ctx, w)`.
3. **The call.** A `call` with the default convention to the module import `eml_c$<CFunction.name>`. That name is the eml canonical name `Module.f`, which is also the key of the VM's C table (`compile/mod.rs:210`). The signature comes from one helper, `abi::c_signature(&CSignature)` (`I32` → `ireduce` and an `I32` param; `I64`; `F64`).
   - The JIT resolves `eml_c$` names with `JITBuilder::symbol_lookup_fn` over an `Arc<RwLock<HashMap<String, usize>>>` shared with the `NativeMachine`. Each `load` inserts the callees of the C functions that are new by name before it compiles. Cranelift caches each lookup by name [V: `cranelift-jit-0.137.1/src/backend.rs:153-159, 210-216`], which matches "same name, same code".
   - Two eml names that bind one C symbol become two imports with one address.
   - **No load-order numbers (R-6).** `cdesc` is the address (`symbol_value`, a relocation by name) of a per-name data symbol of the C function, 8 bytes, filled at load with a pointer to the machine's `CEntry` (its `CFunction`: name and parameter kinds for the message). `hdesc` is the same for the handle type: a pointer to its `CHandleEntry` (name, dtor). The data symbols follow S2b's per-name data-symbol scheme (R-6, 00 C-6): the C function's is `c:<Module.f>` (jit-tasks' `CSubmit` / `CTake` wrappers read the same symbol); the handle type's takes a prefix of its own in the same scheme, named by content only (*illustrative*: `h:<handle type name>`; fixed at the executable refresh). `DatumRow` gains each kind here. On the fast path they cost nothing; the helper does one load on the cold or allocating path.
4. **Result.**
   - `Void` → `Word::UNIT`.
   - `I32` → `sextend`; `SizeT` / `I64` raw; `F64` as the `f64` leaf.
   - `Handle(h)` → `eml_x_c_handle_new(ctx, hdesc, ptr)`. On null it parks the panic through `park_panic`, which sets `Fault::Engine`, and returns the placeholder. The `ctx.native.fault` load that follows (the row faults because it has a handle result) therefore sees a non-zero word and takes the raise path.
5. **Panics.** `eml_x_c_narrow` and the null case allocate the message with `ctx.fresh_string(&CRaise::text(..))` and call `eml_nrt::worker::park_panic(ctx, word)`, exactly as `eml_x_Panic` does. `park_panic` stores the word in the worker struct and calls `ctx.set_fault(Fault::Engine)`. `eml_nrt_failed` starts with `take_fault()`, finds `Engine`, takes the word, and reports `panicked: {text}` with the site (from jit-effects it delivers the word to `try_panic`). This is the VM's `ffi::raise` (`ffi.rs:457-472`), which allocates the same string.

Soundness of the pointers (review C-6):
- C rows borrow every argument, and LIR keeps borrowed objects alive in the caller until after the statement.
- eml objects never move.
- The entries between taking a pointer and the call are reads: they neither allocate nor free.
- A fatal in between abandons the call without leaking or freeing anything.
- No `&mut Ctx` or `Drop` value is live in generated code.
- Stack: C runs on the eml worker stack. Contract r3 1.3 (N-42) keeps `hard = base + 1 MiB` and `stack_limit = real = hard + 2 MiB`, so below the deepest eml frame there are at least 3 MiB in normal running, of which 1 MiB stays below `hard` in every state, for one eml frame and any Rust entry or C function. A C function that needs more than that at the eml depth limit overflows into the guard page, as a Rust entry would. This is written in spec/native.md.

Cost of a scalar-only C call:
- 2-3 instructions per narrowing argument, one direct `call`, and a `sextend` for `I32` results.
- Each pointer argument adds one entry call, until native-inline exports and inlines the payload offset.
- Each handle argument adds one entry call permanently (D-13).
- The VM's path, by contrast, allocates a `Vec` per call and goes through a bridge (`ffi.rs:100-148, 267-296`).

`blocking` C stays rejected (`Form::BlockingC`) until jit-tasks.

**D-9. Shared C pieces and table tests.**
- **`eml_sched::ffi`** receives:
  - `CHandle`, `HandleCell` (`ffi.rs:21-48`);
  - `CRaise` with its texts (`ffi.rs:50-69`);
  - `narrows(CParam, value)`;
  - `CTables` with `link(&self, functions: &[CFunction], handles: &[CHandleType], linked: &Linked) -> Result<CLinks, CMismatch { function: String }>` and `commit(CLinks)`.

  `CTables` is the by-name merge `c_links` + `renumbered` (`compile/mod.rs:166-220`), rewritten over eml_extern slices so eml_sched gains no eml_lir edge (N-2). eml_vm maps `CMismatch` to `CompileError::Mismatch`, and `NativeMachine::load` maps it to its own `LoadError`. Kind 3.
- **D-9a, coverage = N-22's check** (eml_sched `rows::tests`): `LISTED` equals `Extern::ALL`, with each row once. The exhaustive VM match also enforces this at compile time.
- **D-9b, infallibility** (eml_sched `rows::tests`, fast `Ctx` with an exhausted `Control` budget): every non-`faults` row whose result is `obj` (22 rows) returns `Ok` when called with valid arguments. The arguments come from one table: 1-2 small strings / bytes / arrays per kind. The test runs within 1 s alone. Scalar rows need no runtime test, because they return plain values (D-5).
- **D-9c, lowering = N-21's partition test** (`crates/eml_native/tests/rows.rs`, needs both tables, `LISTED` reached through eml_nrt's re-export (00 C-3); the plan states the counts **45 / 187 / 1** at the base commit): for each `e` in `Extern::ALL` with `!e.is_async() && e != Par`, exactly one holds: `e` is on an `inline` line and has no entry (45), or exactly one entry `eml_x_{e:?}` exists whose parameter kinds are `abi(params)` and whose result is `abi(ret)` (`Never` → no result) (187). The same test asserts that every `as` identifier in `LISTED` is `eml_x_` + its variant (N-8). No other `eml_x_` symbol exists except the `eml_x_c_*` helpers. That eml_native's inline-expander table equals the set of `inline` rows in `LISTED` (R-4) is jit-core's test and stands (00 C-1).

**D-10. `Unsupported` and acceptance.**
- jit-externs deletes S2b's `Form::StdRow` and `Form::CRow`.
- The `Prim` form stops covering `Prim::EmptyMutArray`, which lowers as a new constant kind `Const::EmptyMutArray`: an immortal empty `MutArray` built once per image. The maker moves from eml_vm `compile/constants.rs:125-131` into `eml_sched::constants` next to S2b's box makers.
- **Floor** (review B-4): S2b orders forms "last stage first", so a file whose first form is `StdRow` or `CRow` has no other rejected form. At close, `tests/ui/run` accepted-and-identical must therefore be at least jit-apply's 208 + `std row` 113 + `c row` 2 = **323**, and `run-fail` at least 8 + 32 + 2 = **42**, minus `-- native: skip` marks listed one by one in the plan (kind 1), each with a reason that names a difference listed in spec/native.md 「VMとの違い」 (N-43). jit-externs expects none; a new difference would be a user-visible semantics item of the plan. These figures are S2b's measured r1 numbers [V per S2b 7.1]; the refresh re-measures them. The floor supersedes S1 5.3's "about 308" (R-13).
- The histogram may contain only `Par`, `AsyncRow`, `BlockingC`, `Prim`, `Clause`, `Prompt`.
- **Row coverage**: S2b's classifier (`crates/eml_cli/examples/forms.rs`) gains a `--rows` listing of the `eml_x_` rows that accepted UI files reach. Every listed row except the 25 Tls/Gui rows must be reached by an accepted UI file or by an agree test in `crates/eml_native/tests/rows.rs` (one small program per family that runs on both engines). The uncovered list goes into the plan and must be empty at close.

**D-11. Candidates for native-inline** (cost of a `Call` row today: one call + wrapper + one fault load):
- The 22 container rows (`*Length`: one load; `*Get` / `*Set`: bounds compare + load / store with a cold fault path).
- `StrByteLength` / `BytesLength` / `ShowBytesLength`.
- `ChanToCell` / `AsyncJobCell` (S2b's existing `Move` expander, useful from jit-tasks).
- The C pointer arguments (payload offset consts from eml_rt under rt-layout P4).

Kept as calls: string and bytes compare / concat / show (real work, memory-bound), I/O rows, and `FloatMod`. `fmod` needs a libm import; doing that through the C-call path for a std row would be a special case.

**D-12. Async rows in the same macro (answer to S4).** Yes, one macro.
- jit-externs lists the 101 async rows by name in the `task` section: the VM arm is today's `Internal` text, and there is no entry. S4 uses this section and has no separate list (R-5).
- jit-tasks replaces `task { names }` with lines of the same form `fn V as eml_x_V (cx, a: kind, ..) -> kind { body }`. The context `cx` has the type S4 defines (`(core, ctx, cur)` in S4's D-8), and the result is S4's `RowEnd` / `RowFault`.
- jit-tasks adds the section's arms to the four expansions (`defs`, `entries`, `vm_arms` targets `async_op` instead of `run_extern`, `names`).
- The grammar is the same; only the section's context and result convention differ. S4 owns those.
- `CSubmit` / `CTake` are not `Extern` rows and stay outside the list. Their marshal / unmarshal helpers move to `eml_sched::ffi` in jit-tasks, next to D-9's types.

**D-13. Forgone or deferred optimizations** (user qualification):
- Per-row `plain` wrappers without `catch_unwind`: a special case per row with a cannot-panic argument to keep, for a cost the measurement of D-4 will very likely show as zero. If it does not, S1 U4's procedure applies to a whole class, not a row.
- Inlining a handle's C pointer: needs the `Foreign` payload layout (`Arc<HandleCell>`) in generated code.
- Dropping `unit` parameters from the entry ABI: the descriptor would no longer equal the row's Reprs.
- Passing the site to wrappers so internal-class errors carry a location: an ABI cost for bugs.
- A libm `fmod` import for `FloatMod`.

**D-14. Profiler readiness** (contract r3 1.7; the roadmap item is the contract's, N-40): `eml_x_*`, `eml_rt_*` and C callees are ordinary named machine functions with normal frames, so a sampling profiler attributes time to row names, and calls to C appear under their library symbols. This stage adds no hook. The entry names and addresses come from jit-rt's descriptor table (1.7). The JIT symbol map and the frame-pointer policy belong to jit-rt / jit-core (N-34 to N-36). If a profiler stage wants frame pointers in the Rust entries on Linux, it builds eml_nrt with `-C force-frame-pointers=yes`; nothing here prevents it.

### 1.2 The `extern_rows!` grammar (complete; introduced by jit-core)

```
list      := section*
section   := "generic" "{" line* "}" | "dedicated" "{" line* "}"
           | "engine" "{" names "}" | "lir" "{" names "}" | "task" "{" names "}"
line      := "fn" Variant "as" Entry ("|" Variant "as" Entry)* "(" ctxpat ("," param)* ")" "->" ret block
           | "inline" "fn" Variant ("|" Variant)* "(" ctxpat ("," param)* ")" "->" ret block   // no entry, so no `as` (03 K9, 00 C-1)
Entry     := ident                           // must be `eml_x_` + Variant (unit test, N-8)
ctxpat    := ident | "_"                       // the name the body uses for `&mut Ctx<M>` (hygiene: it comes from the list)
param     := ident ":" kind
kind      := "obj" | "tobj" | "unit" | "int" | "enum" | "float"
ret       := kind | "never"
names     := Variant ("," Variant)* ","?
```

- **Types inside the body.** `obj | tobj | unit` → `Word`; `int | enum` → `i64`; `float` → `f64`. The body's type is `Result<T, RunFault>`, where `T` is `Word` (`obj` / `tobj`), `()` (`unit`), `i64`, `f64`, or `core::convert::Infallible` (`never`). Errors convert with `?` from `Fault`, `HeapError`, `ArithFault` and `CellFault` (the existing `From` impls).
- **`inline` lines** (R-4) give the `rows::` function and a `names` row marked inline, but no entry, so they name no `as` identifier. A row moves between `Call` and `Inline` (native-inline) by adding or removing the prefix (and the `as` identifiers) together with its expander.
- **Multi-row lines.** All variants of a line share the parameter and result kinds. Each variant is const-checked against its own row. The function is named after the first variant, and other lines may call it (for example the `Float` unboxed pair).
- **Slices.** A body that needs a slice builds it itself (`&[a, ui]`). The grammar has no variadic parameters, because rows have fixed arity.
- **`unsafe`.** Every function is an `unsafe fn` with one shared `# Safety` contract: "arguments are words of the row's Reprs, owned or borrowed by LIR's rules (lir.md 「extern の借用」)". `defs` wraps the body in `unsafe { }` (edition 2024 `unsafe_op_in_unsafe_fn`) and puts `#[allow(unused_unsafe)]` on the function, because most bodies have no unsafe operation.
- **Bodies are blocks, not closures.** Multi-statement rows are written directly. Long bodies call a named function in eml_sched (`tls::*`, `gui::arm`, `fail_io`).
- **This grammar is final** (S3 owns it; S2b's K9 snippets use it, R-1).
- **Parsing.** The parameter group is matched as a parenthesised token group (`($($sig:tt)*)`), so `|` before it is unambiguous. Every expansion (`defs`, `entries`, `names`, `vm_arms`) walks a section line by line with two arms, one starting with the literal `inline fn` and one with `fn`. An optional-identifier matcher for the mark is ambiguous, because `fn` also matches as an identifier; this agrees with S2b's prototype. The muncher form also lets `entries` drop `inline` lines and emit one item per variant without nested-repetition depth errors.
- **`engine` rows.** `Exit` and `Panic` are only named in the list. Their entries are hand-written in eml_nrt with `entries!` directly (`-> never`, S2b K9).
- **Introduced in jit-core** (eml_sched `rows.rs` + eml_nrt `rows.rs`):
  - the macro with this grammar;
  - the `defs`, `entries` and `names` expansions;
  - the `generic` lines of the 8 seed rows `Println`, `Eprintln`, `IntShow` (jit-externs widens it to `IntShow as eml_x_IntShow | ByteDisplay as eml_x_ByteDisplay`), `StrShow`, `FloatShow`, `StrConcat`, `StrEq`, `StrCompare`, and the `dedicated` line `FloatMod`;
  - the 45 inline rows as `inline` lines, with the test that eml_native's inline-expander table equals the marked set, and N-27's differential test against their `rows::` functions (00 C-1; 03 K9, K17);
  - `LISTED` with the `Listed` struct (00 C-4), and eml_nrt's re-exports of `eml_sched::rows` and `LISTED` (00 C-3);
  - `impl Raise for RunFault` = `worker::record(ctx, self, None)` (`FATAL_RECORDED` = 3, in `enter.rs`; 00 C-2, C-5);
  - a seed-subset version of D-9c (with the `as`-identifier check).
- **jit-externs adds** the remaining non-inline lines, the `engine` / `lir` / `task` sections, the `vm_arms` expansion and the D-2 call sites, and D-9a / D-9b.
- The VM is untouched in jit-core: its hand-written arms for the seed rows coexist with the `defs` for one stage gap. N-22 is a jit-externs completion item.

### 1.3 Interfaces provided (S3 is the owner)

| Interface | File | Shape |
|---|---|---|
| `extern_rows!` | `crates/eml_sched/src/rows.rs` | grammar of 1.2; `#[macro_export]`; called as `eml_sched::extern_rows!(path::to::site)` |
| row functions | `eml_sched::rows::<First>` | `pub unsafe fn <First><M: Mode>(ctx: &mut Ctx<M>, ..) -> Result<T, RunFault>` |
| `LISTED`, `Listed`, `Section` | `eml_sched::rows`, re-exported by eml_nrt (00 C-3) | `pub const LISTED: &[Listed]`, `Listed { row, section, entry, inline }`, `pub enum Section { Generic, Dedicated, Engine, Lir, Task }` (introduced by jit-core, 00 C-4) |
| `eml_x_<Variant>` | `crates/eml_nrt/src/rows.rs` | D-3; table `pub const ROWS` appended to `TABLES` |
| `impl Raise for RunFault` (jit-core) | `crates/eml_nrt/src/rows.rs` | `worker::record(ctx, self, None)` → `Fatal { code: FATAL_RECORDED (= 3), site: 0 }` (00 C-2) |
| C helpers | `crates/eml_nrt/src/c.rs` | `eml_x_c_narrow(ctx, cdesc, arg, value)` (parks via `park_panic`), `eml_x_c_handle_ptr(ctx, w) -> i64`, `eml_x_c_handle_new(ctx, hdesc, ptr) -> i64` (shim; `cdesc` / `hdesc` are data-symbol addresses, D-8); `eml_rt_text_payload`, `eml_rt_unboxed_ptr`, `eml_rt_unboxed_ptr_mut` (shim, fatal-only) |
| C lowering | `crates/eml_native/src/lower/c.rs`; `c_signature` and the C import in `crates/eml_native/src/abi.rs` | D-8 |
| shared C | `eml_sched::ffi::{CHandle, HandleCell, CRaise, CTables, CLinks, CMismatch}` | D-9 |
| `GateHost` | `eml_sched::gui` | D-7 |
| empty `MutArray` maker | `eml_sched::constants::empty_mut_array` | D-10 |

### 1.4 Requests to other owners

- **To S2a:** none open. S2a r3's `entries!` takes the row kind tokens and `never`, and the function identifier is the symbol, which the list's `as` identifiers supply (R-1).
- **To S2b (jit-core):**
  - K9 uses the grammar of 1.2 (`fn V as eml_x_V (ctx, a: kind) -> kind { body }`).
  - K9 adds `impl Raise for RunFault` as `worker::record(ctx, self, None)`, with `FATAL_RECORDED = 3` (R-2, 00 C-2).
  - The per-name data-symbol scheme of R-6 also carries the C descriptors of D-8 (`c:<Module.f>`, 00 C-6).
  - K9 lists the 45 inline rows as `inline` lines and owns the inline-table test (00 C-1); `LISTED` uses the `Listed` struct (00 C-4); eml_nrt re-exports `eml_sched::rows` and `LISTED` (00 C-3).
  - The wrapper protocol is N-9's (D-3), and `park_panic` sets `Fault::Engine` (ruling S2b-1), which the C helpers reuse.
  - S2b r2 already adopts the macro, `RunFault`, `park_panic` and `worker::record`.
- **Echo shapes (S2a U-8):** jit-externs adds one hand-written echo entry per parameter / result shape that the new rows introduce. They are not generated from the row list, so the ABI test stays independent of the code it checks.
- **To S4:** see D-12. Optionally `GateHost` is replaced by `WorkerCore` in jit-tasks.

### 1.5 Challenges: all ruled (contract r3)

- **CH-1 (adopted; a fixed item, confirmed by the user).** Contract r3 N-9 「What a wrapper does」:
  - park `RunFault::Extern(f)` iff the row `faults`;
  - engine-held payloads go to the worker struct, with `Fault::Engine` in `Ctx`;
  - every other `Err` and a caught panic are the internal class and end in the shim's fatal path;
  - the wrapper body never names `eml_fatal`.

  The jit-core and jit-externs plans each state it as a contract rewording that the user confirmed.
- **CH-2 (adopted, N-21).**
  - `Par` cannot have one (LIR verifier, `eml_lir/src/check.rs:1067`).
  - The 45 `Inline` rows get none: S1 1.2 says no entry is declared ahead of a caller, and the VM / N-27 use the `rows::` functions instead.
  - Test: D-9c (45 / 187 / 1).
- **CH-3 (adopted, N-22).** `fail_io` is shared (`never` line). The engine-specific rows are `exit` and `panic`.
- **CH-4 (adopted, N-7).** User C functions are imported as `eml_c$<name>` with a signature from `CSignature`. The one `Linkage::Import` function in `crates/eml_native/src/abi.rs` takes a descriptor row or a `c_signature` result.
- **CH-5 (adopted, N-9).** Narrowing faults are inline (like `After::Arith`). The fault load follows `eml_x_c_handle_new`, not the C call.
- **CH-6 (resolved).** N-29 applies to every task that touches `crates/eml_rt`; `bytes_empty` is jit-rt's R4, so jit-externs does not touch eml_rt.
- (r1's CH-7 is withdrawn: S2a's sched-split deletes the two `cfg_attr(feature = "gui")` lines.)

## 2. Rationale

- **The list in eml_sched.** It is the lowest crate both engines depend on that may name eml_rt, eml_io, eml_tls and eml_gui.
- **Every row in the list.** The VM's exhaustive match stays exhaustive. Adding a row to `eml_extern` without a list line is a compile error in eml_vm, exactly as the comment at emit.rs L1063 relies on today.
- **Dedicated ops call the shared functions.** That is the only way two engines with different dispatch share one body.
- **Fatal for the residue.** After D-5 the residue holds only bugs and caught panics. The VM's worker-level `catch_unwind` treats them the same way (`workers.rs:943`).
- **Direct C calls.** FFI speed is the purpose of native. Inline narrowing keeps scalar C calls free of Rust, and only the conversions that need runtime knowledge become entries.

## 3. Alternatives considered and rejected

- A second wrapper mechanism for the seed rows (U1).
- Hand-written VM arms with equivalence tests (N-22).
- One `eml_x_dispatch(ctx, row, argv)` entry: an argument array and a match per call.
- A module data table of C callee addresses: needs `call_indirect` (against N-16), and AOT would repeat the `dlsym` loop. The option stays open for native-aot (U-S3-1).
- `GUI_SLOT` behind `ctx.runtime`: eml_vm would depend on eml_nrt's runtime type.
- Moving only the arm functions of `tls.rs` / `gui.rs`: every private field of the shared types becomes `pub` across crates.

## 4. Assumptions made

| # | Assumption | If false |
|---|---|---|
| A-1 [I] | jit-core introduces the macro as in 1.2 (request to S2b) | jit-externs' first native unit converts S2b's `seed_rows!` (9 lines) |
| A-2 [V: S2a r3 D-8; 7.10] | `entries!` accepts `obj tobj unit int enum float` and `never` and uses the function identifier as the symbol | - |
| A-3 [V: S2b r2 D-6; contract r3 1.4] | `eml_x_Panic` / `eml_x_Exit` exist from jit-core. `worker::park_panic(ctx, Word)` stores the word and sets `Fault::Engine`. `eml_nrt_failed` starts with `take_fault()`. `worker::record(ctx, RunFault, Option<At>) -> Fatal` exists (00 C-2) | the C helpers follow whatever `eml_x_Panic` uses |
| A-4 [R-2] | `FATAL_RECORDED` (= 3) and `worker::record` exist from jit-core | - |
| A-5 [I] | `NativeMachine` can own the `Arc<RwLock<..>>` callee map passed to `symbol_lookup_fn` before `JITModule` creation; S2b's per-name data symbols (R-6) can be defined per C function / handle type and filled at load | callees go into a per-name data symbol plus `call_indirect` (U-S3-1) |
| A-6 [V: S2b r1 D-15] | The native harness passes the VM's `Linked` (with fixtures) to both engines | fixture files get `-- native: skip` (kind 1) |
| A-7 [V: S2a r1 D-20] | sched-split moved `cells`, `error`, `park`, `driver` and no gui feature is needed before jit-externs | - |
| A-8 [V: 7.1] | `held` is empty and `writes` is false on non-async std rows | - |
| A-10 [V: S2a r2 R4] | jit-rt makes `Ctx::bytes_empty` infallible | D-5's invariant fails for one row; D-9b fails, and the row gets `faults: true` in the plan (kind 2) |
| A-9 [V: S2a r1 D-10] | jit-rt's ABI test requires an echo entry for every parameter-kind sequence in `entries()`; jit-externs adds echoes for the new sequences of the 15 shapes | - |

## 5. Implications for implementation (stage `jit-externs`)

### Interfaces consumed
- From sched-split: eml_sched (`cells`, `error`, `park`, `driver`, `config`).
- From jit-rt: `entries!`, `Raise`, `Fatal`, `Kind`, `entries()`, the ABI echo test, `Ctx::set_fault`, the infallible `bytes_empty` (R4), `bench/rt.sh` / `bench/native.sh` row conventions (N-39).
- From jit-core: the macro, the seed rows and the 45 `inline` lines of 1.2, `LISTED` / `Listed` and eml_nrt's re-exports of `eml_sched::rows` and `LISTED` (00 C-1, C-3, C-4); `worker::{park_panic, record}`, `Fault::Engine`; `bench/native.sh`; spec/native.md 「VMとの違い」; S2b's `rows.rs` `INLINE`, `After`, `eml$raise(ctx, function, site, purity)`, `eml_nrt_failed(ctx, function, site, purity)` (00 C-2), `Worker`, `Image`, `Form`, `unsupported`, `consts.rs`, the native harness and snapshots, `crates/eml_cli/examples/forms.rs`, `agree(text)`.

### Work units (commit-sized, in order)
1. **Plan commit** (the executable plan replaces `docs/plans/2026-11-11-jit-externs.md`). It records the re-measured floor of D-10 and the uncovered-row list.
2. **Move `Std.Tls`** (D-7): features, forwarding, re-exports. Kind 3. Feature-combo clippy including `-p eml_sched`.
3. **Move `Std.Gui`** (D-7): `GateHost`; `--features eml_cli/gui-native`; the `.#cross` Windows clippy with `gui-native`. Kind 3.
4. **The full list and `vm_arms`** (D-1): all lines and sections; `run_extern` becomes prelude + `extern_rows!(vm_arms)`; D-9a / D-9b. Kind 3: all UI tests, REPL transcripts and the `bench.rs` `RunStats` snapshots unchanged.
5. **Dedicated ops call `rows::`** (D-2). Kind 3. Record `bench/run.sh` before and after; no gate.
6. **Shared C pieces** (D-9) and the empty-`MutArray` maker (D-10). Kind 3.
7. **Native entries for all rows** (D-3): `extern_rows!(entries)` for every non-inline line, D-9c, hand-written echo entries for new ABI shapes, Miri `entries::x_*`:
   - a parking row: `Std.FileIn.open` of a missing file on the memory Host (`IntDiv` is inline and has no entry);
   - a non-`faults` row: `StrConcat`;
   - a `float` row: `FloatPow`;
   - a `unit` parameter: `FloatInfinity`;
   - `FailIo` parks `Raised`.

   Then `rows::extern_call_cost` (`#[ignore]`, a record).
8. **Native lowering of std rows** (D-4, D-10): delete `Form::StdRow`; `Const::EmptyMutArray` and `Prim::EmptyMutArray` leave `Form::Prim`; the `fail_io` `Never` path; agree tests for uncovered families; snapshots (kind 2) against the floor; spec/native.md 「extern の行」 (with D-5's invariant).
9. **Native user C** (D-8): `eml_c$` imports; `c_signature`; inline narrowing; the six entries; `Form::CRow` deleted; codegen tests and one CLIF pin (a C row with a narrowing parameter, N-9); snapshots (kind 2); spec/native.md 「C の関数の呼び出し」.
10. **Native measurement (N-39)**: the scripts of `tests/ui/run/scripts/` that run natively get their native row next to the VM and Python rows of `bench/scripts.sh`; `bench/native.sh` (from jit-core) picks up the `bench/*.em` programs that are no longer rejected by itself. Both are records, not gates.
11. **Close**: docs; benchmarks.md 「今の数値」 (eml_vm touched) and 「ネイティブの数値」 (`bench/native.sh`, the native rows of `bench/scripts.sh`); the roadmap close steps, delete the plan.

### Existing code revised

| Where (432fffb4) | Change | Cost |
|---|---|---|
| `crates/eml_vm/src/vm.rs:4153-4894` | prelude kept; match generated | ~600 lines → ~40 + list lines |
| `crates/eml_vm/src/vm.rs` dedicated op handlers (L1203-1360 and the float / compare ops) | call `rows::` | ~40 one-line edits |
| `crates/eml_vm/src/tls.rs` (535 lines), `gui.rs` (819 lines) | moved; 4 functions generic over `GateHost` | imports, ~30 lines, one trait impl |
| `crates/eml_vm/src/ffi.rs:21-69, 80-98`, `compile/mod.rs:166-220`, `compile/constants.rs:125-131` | moved to eml_sched over slices; error mapped | ~80 lines moved, ~10 changed |
| `Cargo.toml` of eml_sched, eml_vm, eml_nrt, eml_native, eml_cli | features | small |
| jit-core files (N-18 list) | `eml_nrt/src/rows.rs` (list grows), `eml_native` `unsupported.rs` (`StdRow`, `CRow` deleted; `Prim` narrowed), `lower/consts.rs` (`EmptyMutArray`), `lower/rows.rs` (nothing: the default is `Call`), `abi.rs` (`c_signature`; the one `Linkage::Import` function also takes a `c_signature` result), new `lower/c.rs` | as listed |

### Tests and change kinds
- **VM:** kind 3 only.
- **New tests:**
  - D-9a, D-9b (eml_sched);
  - D-9c, Miri `entries::x_*` (eml_nrt, eml_native);
  - C codegen tests in `crates/eml_native/tests/c.rs` with fixtures through `Linker::with_symbols`: `int32_t` fits and misfits (uncaught: the VM's text and site); negative `size_t`; void; an `int32_t` result of -1; 9 alternating `F64` / `I64` parameters; `Text`; `UnboxedArray`; `MutUnboxedArray` written by C; a handle round trip with the dtor run once (census clean); a null handle; a `pure` C row with a misfit (uncaught here; the caught-by-`try_panic` variant is added by jit-effects, a note for S4);
  - agree tests for row families not reached by accepted UI files;
  - one CLIF pin (narrowing + call + handle fault load).
- **Kind 2:** the two native snapshots.
- **Kind 1:** any `-- native: skip` mark, listed one by one with a reason citing 「VMとの違い」 (N-43); none expected.
- Every test ≤ 1 s alone; the Miri budget is recorded.

### `docs/` written
- **spec/native.md:**
  - 「extern の行」: the list and its sections, the four expansions, the symbol rule, D-3's table, D-5's invariant, the exceptions, the cost model and the measured call cost;
  - 「C の関数の呼び出し」: D-8 with the pointer-lifetime and stack paragraphs.
- **spec/vm.md:** 「基本操作と extern」 (arms and dedicated ops call `rows::`); L480 (`GuiPark` in eml_sched); L1080 (the VM's conversion is `ffi.rs`; native follows the same table in lowering).
- **spec/object-model.md:** 「extern」, if it names `run_extern`.
- **implementation/architecture.md:** L99, L326; eml_sched's `rows`, `tls`, `gui`, `ffi`, `constants`; features.
- **implementation/testing.md:** feature combos with `-p eml_sched`; L681; the new tests; the Miri budget.
- **implementation/status.md:** every non-async std row and non-blocking C run natively; `Std.Tls` / `Std.Gui` rows only become reachable with async rows.
- **CLAUDE.md:** the architecture lines (eml_sched, eml_vm, eml_gui) and the clippy feature-combo line.
- **future/native.md** 「extern のエントリ」 and **future/ffi.md** L30, L86, L106, L125: reduced or retargeted.

### Acceptance criteria
- `cargo nextest run` green. Pending snapshots only in the two native snapshots. `cargo nextest run -p eml_vm --test integration bench::` unchanged.
- Native `run` accepted ≥ 323 and `run-fail` accepted ≥ 42 (minus the listed marks). The histogram contains only `Par`, `AsyncRow`, `BlockingC`, `Prim`, `Clause`, `Prompt`. The uncovered-row list is empty.
- `cargo nextest run -p eml_sched -E 'test(rows::)'`, `-p eml_nrt -E 'test(rows::) | test(c::)'` and `-p eml_native -E 'test(rows::) | test(c::)'` green.
- `cargo clippy --all-targets -- -D warnings`; `cargo clippy -p eml_sched -p eml_vm -p eml_io -p eml_nrt --all-targets --no-default-features -- -D warnings`, and the same with `--features tls` and with `--features gui`; `cargo build --features eml_cli/gui-native`; the two `.#cross` Windows commands.
- `cargo nextest run -p eml_io -p eml_vm -E 'test(tls_off::) | test(gui_off::)'` green.
- N-2's greps (the eml_sched line also with `--features tls,gui`).
- `grep -rn 'thread_local' crates/eml_nrt/src crates/eml_native/src` is empty.
- `grep -rn 'eml_fatal' crates/eml_nrt/src/rows.rs crates/eml_nrt/src/c.rs` is empty.
- `grep -rn 'Linkage::Import' crates/eml_native/src` hits one function.
- `git diff <plan commit> --stat -- crates/eml_rt` is empty (N-29 does not apply).
- `cargo nextest run --profile timing -j 1` passes. benchmarks.md 「ネイティブの数値」 replaced at Close (N-39).

### Risks and fallbacks

| Risk | Fallback |
|---|---|
| `macro_rules!` errors in a 334-row list | Bodies are short; long ones call named functions; the const assertions name the variant |
| The VM slows because of the cross-crate calls of D-2 | Accepted (premise); `#[inline]` is the default; the number is recorded |
| `entries!` cannot take kinds or symbols | 1.4 fallback |
| `I32` C ABI extension rules differ (Apple arm64) | Explicit `ireduce` / `sextend`; fixtures on all four targets |
| C needs more stack than the headroom at the eml depth limit | Documented. Same guard-page behaviour as a Rust entry |

## 6. Unresolved items

| # | Item | Default | Who / when |
|---|---|---|---|
| U-S3-1 | How AOT binds `eml_c$<name>` | JIT: `symbol_lookup_fn` | native-aot |
| U-S3-2 | Inline container rows and C pointer arguments | entries now (D-11) | native-inline |
| U-S3-4 | `GateHost` versus `WorkerCore` | `GateHost` until jit-tasks | S4 |
| U-S3-5 | `fail_io` shared (CH-3) | shared | orchestrator ruling |
| U-S3-6 | C tables across REPL appends | `CTables` by name already; transcripts | S4 (jit-repl) |

## 7. Additional checks

1. [V] `crates/eml_extern/examples/s3rows.rs` (scratch clone) over the table: the counts of 1.0; output in `scratchpad/jit/s3rows.tsv`. The ABI shapes were counted with awk.
2. [V] `scratchpad/jit/fastok.py` over `eml_rt/src/heap/ctx/*.rs`: which `Result` methods ensure fast-mode `Ok`. I read `bytes_empty` → `bytes_make` (`bytes.rs:906-938`, `over_budget`), `array_empty` (`arrays.rs:2504-2511`), `unboxed_empty` (`unboxed.rs:606-613`) and `proc_stage_count` (`proc.rs:403-415`).
3. [V] `scratchpad/jit/probe-s3/m.rs`: a callback list macro expanded into `extern "C"` entries with `export_name = concat!(..)` plus a descriptor array and a `match`. It runs.
4. [V] Read `run_extern`: 162 rows in its generic arms, 70 in the dedicated `Internal` list (incl. `Panic` / `Exit`), and `Par`. Every `Pure` row in the generic arms either calls an infallible `externs` function or a fast-`Ok` `Ctx` method (awk over the arms). `RunFault` constructors in `tls.rs` / `gui.rs`: 16 `Internal`, 1 `Extern`. In the generic arms: one `Extern` (`FailIo`). `error.rs:55-59` (`From<HeapError>` → `Extern`). All 25 Tls/Gui rows are `Effectful`.
5. [V] emit.rs L1063-1103 (op grouping) and L460-475 (container ops); `vm.rs:1203, 1326-1360`; `eml_lir/src/prim.rs:42, 201, 217` and emit.rs L782 (`EmptyMutArray` is a constant); `compile/constants.rs:125-131`.
6. [V] `ffi.rs`, `c.rs`, `compile/mod.rs:166-220`, `cranelift-jit-0.137.1/src/backend.rs:127-159, 210-216`.
7. [V] No `max-bytes` UI file uses `Bytes.empty`. eml_rt tests do not expect `bytes_empty` to fail.
8. [V] Read S2a r1 (D-6..D-11, D-20), S2b r1 (1.0, D-6, D-10..D-15, 5.3) and S4 r1 (D-7, D-8, I-6) for the interfaces used here.
9. [V] (r3) Read X-ruling-r1 sections 1 and 3, S1-response-r3 N-7 to N-43 and 1.3 / 1.7, S2a r2 D-7 / D-8 / U-8 / R4, S2b r2 D-6 and the interface table (`worker::record`, `park_panic`, `FATAL_RECORDED`, K9).
10. [V] (r4) Grammar probe against S2a's `entries!`: `scratchpad/jit/scratch-S3r4` (a clone of `scratch-S2a` at `1c23eebd` plus S2a's uncommitted `never` support in `entries.rs`). `crates/eml_nrt/src/rowsprobe.rs` holds a 4-line list in the grammar of 1.2 (a two-variant `generic` line with `as`, a `never` line, a `float` line with `_` as the context, and an `inline` `dedicated` line), with three expansions: `defs`, `names` (a muncher marking `inline`) and `entries` (a muncher over lines and variants skipping `inline`, calling `crate::entries!`). Results:
    - `cargo test -p eml_nrt --lib rowsprobe` passes. The table holds exactly `eml_x_IntShow`, `eml_x_ByteDisplay`, `eml_x_FailIo` (no result) and `eml_x_Sqrt` (`F64`), and no inline entry. Every `as` identifier equals `eml_x_` + the variant.
    - `cargo clippy -p eml_nrt --lib --tests -- -D warnings` is clean, once `defs` carries the `# Safety` doc and `#[allow(unused_unsafe)]`.
    - Findings folded into 1.2: (a) `$sig:tt` after `$(| $v as $e)*` is ambiguous and must be matched as `($($sig:tt)*)`; (b) an optional-identifier mark before `fn` is ambiguous, so the expansions walk lines with an `inline fn` arm and a `fn` arm (the probe's `defs` used a literal `$(inline)?`, which compiled, but 1.2 makes the two-arm walk the rule for every expansion); (c) the placeholder needs a per-kind macro (`Word` has no `Default`).
11. Not checked: execution of any wrapper or C lowering; the real eml_sched list (the probe stands in for it inside eml_nrt).
