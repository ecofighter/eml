# Appendix 06: the extern / C-call boundary (jit-externs refresh)

(Written 2026-11-13 by the designer "S0" during the refresh of jit-externs; reviewed once; the user approved amendments M-1..M-7 and the default of U-1, entries for all rows.)

**Read with appendix 07 and 00 §F.** Appendix 07 (the crate graph) moves the pieces this text places in `eml_sched` (the list `extern_rows!`, `rows::call`, `ffi`, `tls`, `gui`, `constants`) to the new crate `eml_rows`, and `abi::c_signature` to the new crate `eml_clink` (07 AM-10, AM-11); it also drops the `shared_rows` re-export and the feature forwarding of 5.3's last bullet. Read every `eml_sched` path of the list, `ffi`, `tls`, `gui` and `constants` below as `eml_rows`. Where this text, 07 and 00 differ, 00 §F wins, then 07. The executable plan `docs/plans/2026-11-11-jit-externs.md` restates the conformance criteria of section 8 with those paths.

Later resolutions that change this text (recorded in 00 §F.4; the plan's D-1 states the criteria as amended): G-1 and G-24 are reworded, because the group `pending` exists from the list reshape (unit 2) to the last commit of unit 5, not "only inside unit 5", and the std-row unit may use a second transitional group, `NOT_YET_NATIVE`; G-12's check also asserts that no lending call precedes a narrowing `brif` of the same C call; U-3's residue is 31 rows (the 27 of U-3 plus `ShowBytesLength`, `ShowBytesGet`, `ProcStageCount`, `ProcWaitStage`); the std-row unit also takes `Prim::EmptyMutArray` out of `Form::Prim`, so the exact counts are 320 / 41 after it and 326 + new files / 43 at close; U-2 and U-6 are decided (`CInt::code` / `from_code`; `LoadError::LinkMismatch`).

Names in this text: "04", "01", "00" are the appendices of this directory; "S1" / "S2" are the unit designs folded into the executable plan; "R2" / "R3" were the refresh's fact-finding notes, whose results the plan's section 3 states.


Revision of r1 after review round 1. The response table for the findings is at the end.

Tags: [V] verified (method given, or item of section 7), [I] inferred. Base fddd44f2. "04" = `docs/plans/2026-11-11-jit/04-jit-externs.md`, "01" = `01-contract.md`, "00" = `00-decisions.md`.

## 1. Conclusion

**Revise, not replace.** 04's core is the right design and stays: one list of row bodies in eml_sched, each body compiled once as a generic function, `catch_unwind` shims in eml_nrt, failures parked in `Ctx`, direct calls of user C functions. What changes is everything around that core that encodes one engine's concerns in the shared list or builds machinery ahead of a consumer.

The architecture:

- **One list, one line form.** `extern_rows!` in `crates/eml_sched/src/rows.rs` names all 334 rows once: `rows { fn V | W (ctx, a: kind, ..) -> kind { body } .. }` (230 rows: the 45 inline ones and 185 others), `engine { Panic, Exit }` (2), `lir { Par }` (1), `task { 101 names }`; 230 + 2 + 1 + 101 = 334. No `as eml_x_V`, no `inline` mark, no `generic` / `dedicated` split.
- **Two expansions instead of four.** `defs` (eml_sched) and `entries` (eml_nrt). `names`, `vm_arms`, `LISTED`, `Listed`, `Section` are deleted.
- **The VM calls a function, not a macro.** `defs` also emits `rows::call(ext, ctx, &args) -> Result<Value, RunFault>`, one exhaustive `match` with no wildcard. `run_extern` becomes the `Panic` / `Exit` prelude plus that call. Dedicated op handlers call `rows::<First>` directly.
- **Entry names are derived.** `entries!` takes a table prefix (`const ROWS = "eml_x_";`); the symbol is `concat!(prefix, stringify!(name))`.
- **Every row with a shared body has an entry**: 230 generated plus hand-written `Exit` and `Panic`, 232 in all (= 334 - 101 async - `Par`; HEAD's "187 called" already counts `Exit` and `Panic` [V: `lower/rows.rs:91-103`], so 187 + 45 = 232). eml_native's lowering table alone decides `Inline` (45) or `Called` (187). The entry set follows from `Extern::is_async()`. Cost and benefit of the 45 entries no generated code calls: 5.3.
- **Failure and panic protocol: unchanged** (N-9, `Fault::Engine`, `park_panic`, `worker::record`).
- **User C: direct `call` of the import `eml_c$<name>`**, bound with `JITBuilder::symbol` at load after `Linked::fits` (X-5). No `symbol_lookup_fn`, no shared map.
- **No C descriptors in data (Q-1).** The cold helpers take the function's name as an ordinary `s:` string datum; a handle type's dtor reaches `eml_x_c_handle_new` as `func_addr` of the import `eml_cd$<handle type name>`, in a parameter typed `Option<Dtor>`. No `c:` / `h:` data symbols and no pointer into machine tables in this stage; jit-tasks adds `c:` for `blocking` calls (5.12 shows the path). The only new datum is the empty `MutArray`, `a:mut`.
- **Shared C pieces shrink** to `eml_sched::ffi::{CHandle, HandleCell, Raise, handle_new}`. `CTables` (the by-name merge) is not moved; native loads once.
- **`Std.Tls` / `Std.Gui` move by file with `GateHost`**, as 04 D-7.
- **Row coverage (D-10)**: no classifier tool. The `ui::run_native` snapshot lists the non-async std rows no accepted file contains; new UI files close the gaps.
- **`fail_io`**: a `never` line of the list, shared. Engine rows are exactly `Exit` and `Panic`.

## 2. Rationale

- A `macro_rules!` callback list is the only stable-Rust way to keep body, signature and binding of a row in one place and still stamp one `extern "C"` function per row. Bodies need Rust parameter types, so the kinds are written on the line and const-checked against `Extern::V.row()`; that is a typed signature, not a second table.
- 04's list carries three facts that are not about the row: which VM op runs it (`generic` / `dedicated`), whether native expands it (`inline`), and the entry identifier (`as`). Each costs grammar, a table field and a tying test. None is needed: the VM's dedicated ops name their row function directly, eml_native owns the inline table, and the symbol is a function of the variant name.
- `vm_arms` makes eml_vm a third parser of the list grammar and regenerates "today's `Internal` texts". A generated `call` in eml_sched gives the same monomorphised code through a 3-method trait, keeps the exhaustiveness check at compile time, and gives tests one way to run any row by its `Extern` value. eml_native's N-27 test has a hand-written 27-arm `shared_of` match today for exactly that reason [V: `crates/eml_native/tests/rows.rs:173-206`].
- `c:` / `h:` descriptors put raw pointers to machine-owned tables into generated data and add an `unsafe` dereference in eml_nrt, a datum kind and a lifetime rule. The cold paths need only a name (a string constant already has a datum) and the dtor address (an import already has a relocation).
- `symbol_lookup_fn` with `Arc<RwLock<HashMap>>` exists to add callees after the module is made. `NativeMachine::load` makes the builder with `linked` in hand and loads once [V: `jit.rs:210-231`], so `builder.symbol` is enough. The stage that makes loads repeat (jit-repl) adds what it needs.

## 3. Alternatives considered and rejected

- **Generate the list from `eml_extern`'s table (build script / proc macro).** The table holds const expressions, not bodies; bodies cannot live in a dependency-free crate. A generator adds a build step or a `syn` crate to remove one signature line per row that the compiler already checks.
- **Kind tokens in `eml_extern`'s `table!`, bodies keyed by variant.** `macro_rules!` cannot join two lists by name; needs one macro per variant.
- **Trait per row / function-item generics for the shims.** A generic `extern "C"` shim over a zero-sized function type needs `mem::zeroed` of a fn item; a hack.
- **Plain functions plus a separate binding list.** Two places per row.
- **The VM calls the native shims.** They are `Fast`-only, C ABI and park faults; the VM is generic over `Mode` and returns `Result`.
- **`rows::call(ext, ctx, &[Word])`.** Bakes the VM's slot encoding of `float` into eml_sched and copies arguments.
- **A second result or out-pointer instead of parking.** Parking is one load and one branch on `Ctx`'s first cache line, and the protocol "as seen from generated code" is fixed by 01 §1.4.
- **One lowering path for std rows and C rows.** They share the import declaration and the after-call handling; C adds marshalling before and after. Forcing one descriptor makes std rows carry empty conversion tables.
- **Leave Tls / Gui in eml_vm until jit-tasks.** Keeps a fourth bodiless group and hand arms in the VM for one stage, and loads jit-tasks further.
- **`inline` mark kept, no entries for the 45 rows.** Recorded as the fallback of U-1 (section 6).
- **Shared C pieces in eml_cabi.** jit-tasks puts submit / take there next to cells; eml_sched::ffi is where they end up anyway.

## 4. Assumptions

| # | Assumption | If false |
|---|---|---|
| A-1 [V: 7.1] | The simplified grammar expands without recursion (three fixed phases) into functions, `call`, `SHARED` and, in eml_nrt, one `entries!` table with one entry per variant | - |
| A-2 [I] | That expansion stays acceptable at 230 rows (no recursion, so no `recursion_limit`; compile time not measured) | split the list into family lists that one outer macro concatenates |
| A-3 [V: 7.3, aarch64 macOS; I on x86-64 SysV / Win64 / aarch64 Linux] | `func_addr` of a `Linkage::Import` function registered with `JITBuilder::symbol` gives its address under `is_pic=false` in the 128 MiB region | the dtor address goes into a datum `h:<handle type name>` (8-byte raw address filled at load); `eml_x_c_handle_new` is unchanged |
| A-4 [V: 7.3, same target] | A direct `call` of an import with `AbiParam::new(I32).sext()` parameters and result follows the C ABI | explicit `sextend` before the call, as the bridge's callers do |
| A-5 [I] | No test pins the `RunFault::Internal` texts of the dedicated / `Par` / async arms of `run_extern` | kind 2 for those texts |
| A-6 [I, from 04 1.0] | The 25 non-async Tls / Gui rows and `ChanToCell` / `AsyncJobCell` cannot be reached natively without an async row | they need UI files like any other row |
| A-7 [V: 7.2] | Adding entries for more kind sequences fails `abi::mixed_int_and_float_arguments_reach_an_entry` until an echo entry exists for each | - |
| A-8 [V: read `crates/eml_cabi/src/lib.rs:27-73`] | `Linked::calls()` / `dtors()` are in `Program::c_functions` / `c_handles` order once `fits` holds | - |

## 5. Implications for implementation

### 5.1 The list (`crates/eml_sched/src/rows.rs`)

```
list    := "rows" "{" line* "}" "engine" "{" names "}" "lir" "{" names "}" "task" "{" names "}"
line    := "fn" Variant ("|" Variant)* "(" ctxpat ("," ident ":" kind)* ")" "->" ret block
kind    := obj | tobj | unit | int | enum | float          ret := kind | never
```

- Types in a body are those at HEAD (`arg_ty!`, `ret_ty!`): `obj` / `tobj` -> `Word`; `unit` / `int` / `enum` -> `i64` as argument; `float` -> `f64`; result `unit` -> `()`, `never` -> `Infallible`. Body type `Result<T, RunFault>`.
- An arm of `run_extern` that matches on `ext` only to pick constants (`Open | FileOutCreate | FileOutAppend`, the read / write / close / flush families, `CharShow | CharDisplay`, the `Float` rounding rows) becomes one line per variant with the constants written out, e.g. `fn Open(ctx, path: obj) -> obj { Ok(ctx.fs_open(path, Open::Read)?) }`. No helper and no second `match`.
- The 10 non-async Tls rows are already one function each (`tls::push_tls(ctx, conn, bytes)`, `vm.rs:4477-4507`); each is one line calling it.
- The 15 non-async Gui rows keep the toolkit dispatcher of HEAD, `gui::arm<M>(ctx, ext: Extern, args: &[Word]) -> Result<Word, RunFault>` (`gui.rs:643`), unchanged: it reads `ext.row()` for the position of the `Ui` and for whether it is consumed, so it is a function of the row, and its inner `match` is over toolkit operations. Each Gui row is one line that passes its own variant and converts by kind: `fn GuiPollRaw(ctx, ui: obj) -> int { Ok(gui::arm(ctx, Extern::GuiPollRaw, &[ui])?.bits() as i64) }`; an `int` argument goes in as `Word::from_bits(n as usize)`, which is the encoding `arm` reads today [V: `gui.rs:636-648`, `vm.rs:4511-4531`]. The exhaustiveness argument is about `call`, which stays one `match` with one arm per row.
- `FailIo`: `fn FailIo(ctx, kind: enum, message: obj) -> never { .. }`, body moved from `vm.rs:4535-4541`.
- `ChanToCell | AsyncJobCell` are ordinary lines (`Ok(a)`).
- Matching rule [V: 7.1]: the parameter group must be matched as `( $($sig:tt)* )`, then re-packed as one `tt` in a second phase before it is used per variant (`$sig:tt` right after `$(| $rest)*` is ambiguous; a depth-2 variable inside the per-variant repetition is a lockstep error).

`defs` emits, in eml_sched:

```rust
pub unsafe fn <First><M: Mode>(ctx: &mut Ctx<M>, ..) -> Result<T, RunFault>      // #[inline], shared `# Safety`
const _: () = assert!(kinds_match(Extern::<V>.row(), ..));                         // per variant
pub const SHARED: &[Extern];                                                        // every variant of `rows`, list order
pub trait Args { fn word(&self, i: usize) -> Word; fn int(&self, i: usize) -> i64; fn float(&self, i: usize) -> f64; }
pub enum Value { Word(Word), Int(i64), Float(f64) }
pub unsafe fn call<M: Mode>(ext: Extern, ctx: &mut Ctx<M>, args: &impl Args) -> Result<Value, RunFault>;
```

`call`: one arm per line reading arguments by kind (`obj|tobj` -> `word`, `unit|int|enum` -> `int`, `float` -> `float`) and wrapping the result (`unit` -> `Value::Word(Word::UNIT)`, `never` -> `match v {}`); one arm per name group returning `RunFault::Internal`. No `_` arm, so a row added to `eml_extern` without a list entry does not compile.

Code size: the row functions are `#[inline]` and `call` is instantiated once per `Mode`, which is what `run_extern` is today (one function per mode holding every arm). The stage records the clean dev build time of eml_vm before unit 5 and after its last commit in that commit's message; if it rises by more than 20 %, `call` gets `#[inline(never)]` and the row functions of lines whose body is more than a single `Ctx` / `externs` call lose `#[inline]` (the dedicated ops' rows are all single calls and keep it).

During unit 5 only, the list has a fifth name group `pending { .. }` holding the non-async rows whose body is still an arm of `run_extern`. `call` has an arm for it like the other name groups, so the list names all 334 rows and `call` has no wildcard at every commit. The last commit of unit 5 deletes the group from the grammar.

Deleted: `names!`, `LISTED`, `Listed`, `Section`, `section_of!`. eml_nrt keeps `pub use eml_sched::rows as shared_rows` and drops the `LISTED` re-export.

### 5.2 The VM

- `run_extern` (`vm.rs:4159-4900`): keep the `Panic` and `Exit` branches; the rest is `match unsafe { rows::call(ext, ctx, &At(task, slots)) }? { Value::Word(w) => task.set(dst, w), Value::Int(n) => task.set_raw(dst, n), Value::Float(x) => task.set_float(dst, x) }` with a private `At` implementing `Args` over `task.get` / `raw` / `float` [V: 7.1, the seed rows ran this way under `ui::run` / `ui::run_fail`]. The generic arms touch nothing but `ctx` and those accessors [V: 7.4].
- Dedicated handlers (`vm.rs:1203-1253`, `1326-1381`) call `unsafe { rows::<First>::<R::M>(ctx, ..) }?` instead of `externs::*` / `ctx.*`. The `Float` unboxed rows convert bits in their own list line.
  - The 45 inline rows and `FloatMod` are list lines with an unused context (`_`), as at HEAD (`rows.rs:195-242`); the handler passes `ctx` and the `#[inline]` function drops it.
  - Error type: the row function returns `Result<T, RunFault>`; `externs::int_add(..)?` in the handler already converts `ArithFault` to `RunFault::Extern(Fault::Arith(..))` through the same `From` impl (`eml_sched/src/error.rs:61-65`), and `step_failed` takes a `RunFault` (`vm.rs:4033-4039`) [V]. So `step_failed` and the handlers' result type do not change.
  - Not measured: the arithmetic handlers after the change (7.1 ran the generic path only). Expected identical code after inlining [I]; `bench/run.sh` before and after goes into the commit message, no gate (U-4).
- During unit 5, `run_extern`'s `match` keeps the arms of the `pending` rows and ends in `_ => rows::call(..)`; the last commit removes the `match`.
- `emit.rs:1062` comment points at the list instead of `run_extern`'s match.
- VM cost: accepted (U-4); `bench/run.sh` recorded at Close, no gate. `RunStats` byte-identical.

### 5.3 eml_nrt entries

- `entries!` (`entries.rs`) gains the arm `$tv const $table:ident = $prefix:literal;` with `symbol: concat!($prefix, stringify!($name))`; the prefix-less arm forwards with `""` [V: 7.1].
- **Why all 230 get an entry** (M-1). Cost: 45 shims that generated code never calls (compiled, registered with `builder.symbol` at each load, exported in a later `libeml_rt.a`), and an exception to "entries arrive with their consumer". They add no echo entry beyond what the other 185 rows need [I: their machine shapes `(I64) -> I64`, `(I64, I64) -> I64`, `(F64) -> F64`, `(F64, F64) -> F64`, `(F64, F64) -> I64` occur among `IntAnd`, `ByteWrap`, `FloatSqrt`, `FloatPow`, `FloatCompare`], and no Miri test of their own (the `entries::x_*` tests are per kind, 5.9). Benefit: one line form and a non-recursive expansion (a per-line skip inside one `entries!` table needs an accumulating muncher over 230 lines and a raised `recursion_limit`); no mark, no `Listed.inline`, no test tying two sets; native-inline moves a row by editing eml_native only, and an inline expansion can use the row's entry as its slow path. The two other routes: (b) HEAD's `inline fn` prefix read only by `entries` (U-1's fallback; costs the muncher, the mark and the tying test); (c) a skip set inside eml_nrt, which needs the same muncher plus a continuation-passing membership macro, puts the set in a crate that cannot see the expanders (eml_native depends on eml_nrt, not the reverse) and so needs the tying test as well. (c) is rejected; the user chooses between (a) and (b) with M-1.
- `rows.rs`: `eml_sched::extern_rows!(entries)` builds `ROWS` with prefix `"eml_x_"`: one `shim fn <Variant>` per variant of `rows` with the body of HEAD (`Ok` -> value, `Err(RunFault::Extern(f)) if FAULTS` -> `set_fault`, else `worker::record`), then hand-written `Exit` and `Panic`. The Rust items are `eml_nrt::rows::<Variant>`; the symbols are `eml_x_<Variant>`.
- New entry kind token `dtor` (Rust `Option<Dtor>`, machine `I64`, argument only), next to `ptr`. `Dtor` is `eml_cabi::Dtor` (`unsafe extern "C" fn(*mut c_void)`), re-exported as `eml_sched::ffi::Dtor`, so eml_nrt gains no crate edge. `Option` of a function pointer has the pointer's ABI with null as `None`, so no cast or `transmute` exists anywhere in Rust: the conversion from the machine word is the entry's ABI, as it is for `Word`.
- `crates/eml_nrt/src/c.rs`, table `C` (no prefix):
  - `shim fn eml_x_c_narrow(ctx, name: obj, arg: int, int: int, value: int) -> never`: parks `Raise::Narrowing { .. }.text(name)` through `worker::park_panic`. `int` is a code of `CInt` defined once in eml_extern.
  - `shim fn eml_x_c_handle_ptr(ctx, handle: obj) -> i64`: `foreign::<CHandle>`; an `Err` is the internal class.
  - `shim fn eml_x_c_handle_new(ctx, name: obj, dtor: dtor, ptr: i64) -> obj`: calls `eml_sched::ffi::handle_new(ctx, ptr, dtor)` (5.5); `None` (null `ptr`) -> `park_panic(Raise::NullHandle.text(name))` and the placeholder.
    `# Safety` (doc comment on the entry and the same sentence in spec/native.md 「Cの関数の呼び出し」): `dtor` is `None` or a function that may be called as `void f(void*)` on `ptr` for the life of the process; generated code passes 0 or the address of the import `eml_cd$<handle type name>`, which `load` binds only to an element of `Linked::dtors()` after `Linked::fits` held. The entry is `unsafe extern "C"` like every entry; its body has no `unsafe` cast. Owner: `crates/eml_nrt/src/c.rs`, added in unit 8 and listed in eml_nrt's `trust.rs`.
  - `name` is borrowed (an immortal string); the helpers read it with `Heap::string` and release nothing.
- `heap.rs`: `eml_rt_text_payload`, `eml_rt_unboxed_ptr`, `eml_rt_unboxed_ptr_mut` `(ctx, w: obj) -> i64`, fatal-only, returning the exposed address (as `ffi.rs:117-121` does).
- eml_nrt and eml_native forward `tls`, `gui`, `gui-native` to eml_sched (01 §1.1).

### 5.4 eml_native

- `lower/rows.rs`: `lowering(ext)` = `Inline(i)` if in `INLINE`; `Called` if `!ext.is_async() && ext != Extern::Par`; else `None`. `CALLED` is deleted. `called_row` is unchanged.
- `unsupported.rs`: `Form::StdRow` and `Form::CRow` deleted; `Rhs::Prim(Prim::EmptyMutArray)` is no longer `Form::Prim`. `eml_cli::rejected_what` loses the two texts.
- `lower/consts.rs`: `Datum::EmptyMutArray`, symbol `a:mut` (`names.rs`); `DATUM_EMPTY_MUT_ARRAY` in `image.rs`; `fill_data` / `constants::build` call `eml_sched::constants::empty_mut_array(heap)` (`alloc_array_immortal(true)`, moved from `compile/constants.rs:112-118`; the VM calls the same function).
- `abi.rs`:
  ```rust
  pub fn c_signature(signature: &CSignature, call_conv: CallConv) -> Signature;   // no `.sext()` anywhere (Q-3 fired at CP-C); callers `ireduce` I32 arguments and `sextend` an I32 result
  pub enum Import<'a> { Entry(&'a Entry), C(&'a CFunction), Dtor(&'a CHandleType) }
  pub fn declare_import<M: Module>(module: &mut M, import: Import<'_>) -> Built<FuncId>;   // the one `Linkage::Import`
  ```
  `bridge.rs` builds its callee signature with `abi::c_signature` (it keeps its own flag constructor, N-34).
- `names.rs`: `c_function(name) = "eml_c$<name>"`, `c_dtor(handle) = "eml_cd$<handle type name>"`.
- `jit.rs` `load` uses its `linked` parameter (today `_linked`). Its callers already pass the `Linked` made from the same program: `eml run` builds one `linked` from `program.c_functions` / `c_handles` and gives it to either engine (`eml_cli/src/main.rs:~160-170`, `lib.rs` `execute_on`), and the harness does the same (`eml_test_support::link`, `ui.rs:242`) [V: the refresh's fact-finding, item X-6]. No caller changes; the REPL has no native engine. `load`: `if !linked.fits(&program.c_functions, &program.c_handles)` -> a load error; then `builder.symbol(c_function(..), call.callee)` per function and `builder.symbol(c_dtor(..), dtor)` per handle type with a dtor, next to the entry symbols.
- `lower/c.rs`, for `Rhs::Extern(ExternRef::C(id))`, in this order:
  1. Arguments as words of the row's Reprs.
  2. For each `CParam::Int(int)` with `int.narrows()`, in parameter order: `I32`: `sextend(ireduce.i32(v)) == v`; `SizeT`: `v >= 0`. Cold edge: `eml_x_c_narrow(ctx, <s: datum of the name>, k + 1, code, v)`, then the `Never` path with `raise = Some((site, row.purity))`.
  3. Pointer and handle arguments, in parameter order: the three `eml_rt_*` entries and `eml_x_c_handle_ptr`. `NoArg` passes nothing.
  4. `call` of `eml_c$<name>` (`I32` arguments through `ireduce`).
  5. Result: `Void` -> `Word::UNIT`; `I32` -> `sextend`; `I64` / `SizeT` raw; `F64`; `Handle(h)` -> `eml_x_c_handle_new(ctx, name, func_addr(eml_cd$<h>) or 0, ptr)`, then the fault load (`After::Faults`).
  Steps 3 and 4 are emitted by one function, `FnLower::lend_and_call`, that takes the already checked scalar values and whose body contains no builder call other than `call_entry` of the four lending entries and the `call` of the import. Generated code has no safepoint except at function entry (N-17) and no unwind point inside a statement, so nothing else can land between them.
  A statement may reach its unwind cleanup from several cold edges (each narrowing check, the handle fault), as `After::Arith` already does with two.
- Cost model: std `Called` row: one `call` (address load + indirect branch in the JIT), plus one load of `ctx.native.fault` and one branch iff `faults`. Scalar C call: 2-3 instructions and a branch per narrowing argument, one `call`, a `sextend` for `I32` results; nothing after the call. Each pointer or handle argument: one entry call. Handle result: one entry call, one load, one branch.
- Forgone (N-38): `plain` wrappers per row; inlining a handle's C pointer; dropping `unit` parameters from the entry ABI; passing the site to wrappers; a libm `fmod` import; inline payload offsets for C pointer arguments (native-inline).

### 5.5 Shared pieces in eml_sched

- `ffi.rs`: `CHandle` (private fields; `new(ptr, dtor)`, `ptr()`, and a clone of the cell for a `blocking` job), `HandleCell` with its `Drop`, `Raise { Narrowing { arg, int, value }, NullHandle }` with `text(&self, name: &str)`. Moved from `eml_vm/src/ffi.rs:21-69`. eml_sched gains the edge to eml_cabi (`Dtor`, re-exported as `ffi::Dtor`), which 01 §1.1 already draws.
- `ffi::handle_new<M: Mode>(ctx: &mut Ctx<M>, ptr: i64, dtor: Option<Dtor>) -> Option<Word>`: `None` for 0 without building anything; else builds the `CHandle` first and then calls `alloc_foreign(handle, true)`. It is the `Handle` arm of the VM's `unmarshal` (`ffi.rs:252-262`) moved; the VM calls it too, so the two engines cannot differ on this path.
- `constants.rs`: `empty_mut_array`.
- `tls.rs`, `gui.rs`: 04 D-7 unchanged (whole files, `GateHost` with exactly the six calls of `gui.rs:36-55, 463-538`, features as 01 §1.1).
- Not moved: `c_links` / `renumbered` (`compile/mod.rs`), `marshal`, `unmarshal`, the `blocking` path.

### 5.6 Soundness (settled here)

- **Borrowed pointers.** A C row borrows every `Obj` argument (`c.rs:157-160` [V]); LIR keeps a borrowed object alive in the caller past the statement; objects never move. All narrowing checks run before the first pointer is taken. Between the first pointer entry and the `call`, generated code calls only the three pointer entries and `eml_x_c_handle_ptr`, which read and neither allocate, free nor change a count; there is no unwind point in between. `eml_x_c_handle_new` allocates after C has returned. This is the window of T16 for the synchronous call; the VM's `marshal` takes the same pointers in the same order. A C function may not keep a lent pointer past its return (declarations.md 「`extern "C"`」: 「ポインタは呼び出しの間だけ有効である」 [V: L252]); native lends for the same window as the VM and adds no case. The order is held by structure (`lend_and_call`, 5.4) and checked mechanically (G-12). T16's text gains "native code" next to the VM as a lender; no eml_rt edit.
- **`MutUnboxedArray`.** Unique and owned by the worker by its `Lin` type; `unboxed_ptr_mut`'s fast-mode precondition is the VM's.
- **Handles.** `eml_x_c_handle_ptr` reads the cell of a borrowed, live object. The dtor crosses the entry as `Option<Dtor>` (token `dtor`), never as an integer and with no cast, so the rule of 「エントリのABI」 holds and the Miri `entries::` tests pass a real function. Its value is 0 or the address `load` registered from `Linked::dtors()`, which `Linked::new`'s contract makes callable for the life of the process (the `# Safety` text is in 5.3).
- **A handle is never lost.** `alloc_foreign` returns a `Word`, not a `Result`: it cannot fail with a limit (limits are stop bits read at safepoints) [V: `eml_rt/src/heap/mod.rs:4156`]. `handle_new` checks null before it builds anything, so no path parks a panic while a cell exists. It builds the `CHandle` before allocating, so if the allocation panics the unwinding drops the `CHandle` and the dtor runs once before the shim's fatal path. The VM runs the same function.
- **Callee addresses.** Registered only after `fits`; `Linked::new` is the `unsafe` constructor that vouches for them. Generated code holds them as relocations, not immediates (N-13).
- **No re-entry.** `CParam` has no callback form, so no entry runs under a C frame and the fatal longjmp never crosses one. Adding a callback parameter needs a re-entry design first.
- **Unwinding.** A C function must not unwind or `longjmp` across the call (spec text; the same as the VM's bridge). A Rust `extern "C"` fixture that panics aborts.
- **Panics of the helpers.** They are `shim`s: a caught Rust panic is the fatal path; the user-level `Panic` (narrowing, null handle) is `park_panic` + `Fault::Engine`, identical to `eml_x_Panic`, so jit-effects' `try_panic` catches both by changing `eml_nrt_failed` only.
- **Stack.** C runs on the worker stack with at least 1 MiB below `hard` (N-42). A C function that needs more hits the guard page and the process dies of the OS's stack-overflow signal; no eml state is observed afterwards. The VM is the same in kind: its bridge runs C on the VM worker's stack with whatever is left of it.
- **Stop requests.** A stop word set by another thread, `exit` on another worker or a signal is seen at the next safepoint, after C returns; nothing interrupts a synchronous C call. Equal to the VM, whose loop also polls only between instructions.
- **No advisor question is left open**; nothing here needs an `unsafe` boundary that safe code can reach: every new `unsafe` item is an entry called only by generated code or by `load`.

### 5.7 jit-core / jit-apply code replaced

| Item | Fate |
|---|---|
| seed lines in `extern_rows!` | rewritten in the new form (drop `as`, `inline`, the two sections) |
| `names!`, `LISTED`, `Listed`, `Section` | deleted |
| eml_nrt `entries` muncher, `rows_table!` | replaced by the three-phase expansion; `eml_x_Exit` / `eml_x_Panic` become `Exit` / `Panic` in the prefixed table |
| `entries!` | gains the prefix arm and the `dtor` token |
| `rows::{INLINE, lowering}` | `INLINE` kept; `lowering` by rule; `CALLED` deleted |
| `Form::StdRow`, `Form::CRow` | deleted |
| `abi::declare_entry` | becomes `declare_import(Import::Entry(..))` |
| `shared_of` in `crates/eml_native/tests/rows.rs` | replaced by `shared_rows::call` |

### 5.8 Rough unit order

1. Plan commit.
2. **List reshape** on the 54 existing rows: 5.1 (with the `pending` group naming the 176 non-async rows still in `run_extern`: 233 - 54 - `Par`, `Exit`, `Panic`), 5.3 (prefix, expansion), eml_native tests retargeted, echo entries for the kind sequences the 45 new entries add. Native snapshots unchanged (208 / 8). The `SHARED` test in this unit and through unit 5 asserts: no duplicates, no async row, not `Par` / `Exit` / `Panic`, and every `SHARED` row has an entry `eml_x_<Variant>`.
3. Move `Std.Tls`. 4. Move `Std.Gui`. (Kind 3.)
5. **All bodies into the list**, by family over several commits: in each, arms leave `run_extern`, the same variants leave `pending` and enter `rows` as lines, echo entries follow. Every commit is green: the list is total and `call` has no wildcard throughout (5.1); a variant removed from `run_extern` without a line still sits in `pending` and fails the first UI test that reaches it with `Internal`. The last commit deletes `pending`, replaces `run_extern`'s `match` by the call, and strengthens the `SHARED` test to equality with `Extern::ALL` minus async rows, `Par`, `Exit`, `Panic` (230). Dedicated ops call `rows::`. Infallibility test (04 D-9b) written over `call`. Native still rejects by `CALLED`, so snapshots stay 208 / 8.
6. Shared C pieces and `empty_mut_array` to eml_sched (kind 3).
7. **Native std rows**: `lowering` by rule, `Form::StdRow` deleted, `a:mut`; partition test; new UI files for unlowered rows (written in this unit; their number is the length of the snapshot's list minus the residue of U-3, known after the unit's first snapshot run); snapshots. The unit starts by re-running the per-file form count (the plan's procedure FL, section 7.1); the floors are the re-measured "accepted" plus "only `std row`" (today `run` 208 + 109 = 317, `run-fail` 8 + 32 = 40).
8. **Native user C**. First step: the test `abi::an_import_gives_its_address` (7.3's probe, kept) on the four CI targets, which settles A-3 and A-4 before any lowering is written. Then 5.4, `c.rs`, `Form::CRow` deleted, codegen tests, one CLIF pin; snapshots. Floors: unit 7's result plus the files whose forms are within {`std row`, `c row`} (today `run` >= 323, `run-fail` >= 42). Histogram keys only `par`, `async row`, `blocking c`, `prim`, `clause`, `prompt`.
9. Native measurement (N-39). 10. Close.

Acceptance beyond 04's list: `grep -n 'Extern::' ` inside `run_extern` shows only `Panic` and `Exit`; `grep -rn 'LISTED\|vm_arms\| as eml_x_' crates` is empty; `ui::run_native` stays within 5 s alone (measure at unit 7; 1.0 s for 208 files today [V: 7.1]).

### 5.9 Test changes by kind (scope)

- **Kind 1** (E-8; the plan lists each): eml_sched `rows::tests::{the_listed_rows_are_the_seed_and_the_inline_rows, a_names_row_has_no_entry}` deleted; eml_nrt `entries::tests::every_row_entry_is_named_after_its_variant` and `rows::fatal::a_row_error_of_the_internal_class_is_recorded` rewritten; eml_native `unsupported::{std_row_is_rejected, c_row_is_rejected, the_called_rows_are_the_seed, the_inline_table_is_the_marked_set}` deleted, `{the_first_function_and_row_are_reported, forms_are_sorted_and_distinct, inline_and_called_rows_are_supported, every_called_row_has_an_entry}` rewritten without `StdRow` / `CALLED`. `-- native: skip` marks: none expected.
- **Kind 2**: the two native snapshots (counts, histogram keys, the new list of unlowered rows); the `Internal` texts of bodiless rows if any test pins them (A-5).
- **Kind 3**: every VM test, REPL transcript and `bench.rs` snapshot; the Tls / Gui / ffi moves.
- **New**: eml_sched `SHARED` equals `Extern::ALL` minus async rows, `Par`, `Exit`, `Panic`; infallibility of non-`faults` rows with an `obj` result; eml_nrt Miri `entries::x_*`, one row per kind the glue handles (results `obj`, `unit`, `int`, `enum`, `float`, `never`; parameters `obj`, `int`, `float`, `unit`; a parking row and a non-`faults` row), plus the three C helpers incl. dtor run once. The glue is generated per kind from tokens that are const-checked against the row's Reprs, and `faults` is read from the table, so a row cannot have glue of its own to get wrong; eml_native partition test, C codegen tests (04 §5 list), CLIF pin of a C row with a narrowing parameter and a handle result; UI files for unlowered rows.

### 5.10 `docs/` to write

spec/native.md: 「行の下ろし方」 (lowering by rule, entry per shared row), 「externの失敗」 (unchanged protocol, the C helpers), 「エントリのABI」 (`dtor`), new 「Cの関数の呼び出し」 (5.4 order, 5.6 window, stack), the `a:mut` datum, 「断る形」. spec/vm.md 「基本操作と extern」 (`rows::call`, dedicated ops), 「GUIのPark」, 「C の関数の呼び出し」. spec/object-model.md T16 (native as a lender). implementation/architecture.md (eml_sched `rows` / `ffi` / `tls` / `gui` / `constants`, features), testing.md (feature combos, the unlowered-rows list, Miri budget), status.md. CLAUDE.md: the eml_sched, eml_vm, eml_nrt, eml_native architecture lines and the clippy combo line. future/native.md, future/ffi.md: retargeted.

### 5.11 Contract amendments

| # | Where | Old | New | Why |
|---|---|---|---|---|
| M-1 | 01 N-21; 01 §1.2 "No entry is declared ahead of a caller" | a row is `Inline` and has no entry, or has exactly one entry, or is `Par`; counts 45 / 187 / 1 | every non-async row except `Par` has exactly one entry `eml_x_<Variant>` whose descriptor equals the row's Reprs (232: 230 generated from the list, `Exit` and `Panic` by hand); the row-lowering table marks 45 of them `Inline`, whose entries generated code does not call; `Par` has none; no other `eml_x_` symbol except `eml_x_c_*`. The plan states the counts 45 / 187 / 1. 01 §1.2 gains: "Row entries are generated from the list, one per row with a shared body, and are the exception to this rule" | cost: 45 uncalled shims; benefit: one line form, no mark and no tying test, the entry set follows from the table, native-inline edits eml_native only (5.3). If refused: U-1 |
| M-2 | 01 N-22 | the VM's `run_extern` arms call the row function; check `LISTED` equals `Extern::ALL`, the VM's match exhaustive | the VM's generic extern instruction calls `eml_sched::rows::call`; dedicated handlers call the row function; check: `call`'s match has no wildcard, the `SHARED` test, `run_extern` names only `Panic` and `Exit` | 5.1, 5.2 |
| M-3 | 00 R-1, R-4, C-1, C-3, C-4 | `as` identifiers and their test; `inline fn` lines and the "marked set" test; `LISTED` / `Listed` re-exported | symbol = table prefix + variant name; no mark; eml_nrt re-exports `eml_sched::rows` only | 5.1, 5.3 |
| M-4 | 01 N-7 | the only other import is a user C function; the `Linkage::Import` function takes a descriptor row or a `c_signature` result | "The only other imports are a user C function, symbol `eml_c$<canonical eml name>`, default cc, signature built from its `CSignature` by one helper (`c_signature`, which `bridge.rs` also uses), and a handle type's dtor, symbol `eml_cd$<handle type name>`, signature `(i64)`, of which generated code takes only the address. Check: `Linkage::Import` hits one function, `declare_import`, which takes a descriptor row, a C function or a handle type" | the dtor reaches `eml_x_c_handle_new` without a data descriptor (5.3, 5.4) |
| M-5 | 00 C-6 | "jit-externs adds `c:<Module.f>` for a user-C function (its contents are jit-externs' `CEntry` pointer; jit-tasks' `CSubmit`/`CTake` wrappers read the same `c:` symbol)" | "jit-externs adds `a:mut` (the empty `MutArray`). jit-tasks adds `c:<Module.f>` with its first reader, the `CSubmit` / `CTake` wrappers; its contents are a pointer to that function's row of the `Image`'s C table" | Q-1; the synchronous path needs a name and an address, not a descriptor; 5.12 gives jit-tasks' path |
| M-6 | 00 E-7 | `Listed.entry == None` for task rows changes in jit-tasks | the partition test's async clause changes in jit-tasks | M-1 |
| M-7 | 04 D-8 / A-5 (X-5); 05's use of it (not a criterion of 01) | callees resolved by `symbol_lookup_fn` over an `Arc<RwLock<HashMap>>` shared with the machine | callees and dtors registered with `JITBuilder::symbol` in `load`; jit-repl replaces this registration when it makes loads repeat (5.12) | `load` builds the builder and loads once |

No soundness criterion is dropped: N-9, N-10, N-12, N-13, U-13 stand as written.

### 5.12 Effects on later stages

- **jit-effects**: none beyond 05; C-row panics already travel as `Fault::Engine`.
- **jit-tasks**: task lines use the same line form (`fn AsyncSleep(cx, ms: int) -> unit { .. }`, no `as`); it adds its own dispatcher for `async_op` and its entries arm; `GateHost` as 05 uses it; `CHandle`'s cell clone is already public API for submit.
  - **Blocking C** needs, at run time on a pool thread, the function's `CFunction` (parameter kinds to copy or pin) and its `CCall` (bridge and callee). The path, with nothing in this stage in its way: `load` already holds `program.c_functions` and `linked.calls()` in the same order after `fits`; jit-tasks appends a C table to `Image` (rows `{ CFunction, CCall }`, built by eml_native at load, owned by the machine like `Image.functions`), adds the datum `c:<Module.f>` whose slot `fill_data` sets to the address of that function's row, and `eml_x_c_submit(ctx, cfn, argv)` / `eml_x_c_take(..)` read it, as 05 D-13 writes. No by-name map is involved: the datum is defined by the same load that knows the function's table index, and its symbol is by name (R-6). The map M-7 drops served only import resolution after the module exists.
  - The synchronous helpers may stay on the name datum or move to `c:` then; jit-tasks decides.
- **jit-repl**: `NativeMachine`'s `asked` flag (one load per machine) is what makes `builder.symbol` at load sufficient. jit-repl removes that flag and creates the module before the first load, so it must **replace** the registration of 5.4, not add to it: `eml_c$` / `eml_cd$` names resolve through `symbol_lookup_fn` over a machine-owned by-name map filled before each load's `finalize_definitions`, and `c_links` / `renumbered` move to `eml_sched::ffi` with their second user. 05's jit-repl change list gains this item at its refresh.
- **native-aot**: `eml_c$` and `eml_cd$` are plain undefined symbols to bind at link; nothing in generated code is JIT-specific.
- **native-inline**: adds expanders to `INLINE`; list and entries untouched.

## 6. Unresolved items

| # | Item | Default |
|---|---|---|
| U-1 | Entries for the 45 inline rows (M-1) need the user's approval | entries for all. If refused: the 45 lines keep HEAD's `inline fn` prefix, read only by `entries`, which then stays HEAD's accumulating muncher (with `#![recursion_limit]` raised in eml_nrt); eml_native keeps the test "`INLINE` equals the marked set" through a `pub const INLINE_MARKED: &[Extern]` that `defs` emits; G-1 allows the prefix and G-7 counts 187. Nothing else changes |
| U-7 | A-3 fails on a CI target (found by unit 8's first step) | the dtor address comes from a raw-address datum `h:<handle type name>` filled at load on every target; G-14's `h:` clause yields; recorded as a plan revision, no user decision needed (no contract text names it) |
| U-2 | Encoding of `CInt` in `eml_x_c_narrow` | `CInt::code` / `from_code` in eml_extern |
| U-3 | Exact residue of unlowered rows | the 25 Tls / Gui rows, `ChanToCell`, `AsyncJobCell`; re-measured at the refresh (the probe then suggested about 41 other rows need UI files; the plan's D-32 gives 53 rows to cover and a residue of 31) |
| U-4 | `ui::run_native` total time with 115 more files | measured in unit 7; over 5 s goes to the user |
| U-5 | Binding `eml_c$` / `eml_cd$` in AOT | native-aot |
| U-6 | Name of the load error for a `Linked` that does not fit | a new `LoadError` variant |

## 7. Additional checks

1. [V] Scratch worktree at fddd44f2 (removed): rewrote `eml_sched/src/rows.rs` to the grammar of 5.1 with `Args` / `Value` / `call` / `SHARED`; `entries!` with the prefix arm; the non-recursive `entries` expansion giving 54 + 2 entries named `eml_x_<Variant>`; `run_extern` routing the 54 listed rows through `rows::call`. `cargo test -p eml_sched --lib rows::` and `-p eml_nrt --lib entries::` (26 tests) pass; `ui::run`, `ui::run_fail`, `ui::run_native` (1.06 s), `ui::run_fail_native` pass with snapshots unchanged; `cargo clippy -p eml_sched -p eml_nrt -p eml_vm -- -D warnings` clean. Found on the way: the two macro matching rules of 5.1 and that an optional table prefix must be its own macro arm.
2. [V] Same tree, `cargo nextest run -p eml_native`: 152 of 153 pass; the one failure is `abi::mixed_int_and_float_arguments_reach_an_entry` (A-7).
3. [V] A cranelift test in that tree (aarch64 macOS, `abi::jit_builder()`): `func_addr` of an import named `eml_cd$M.H` equals the registered address, and a direct `call` of `eml_c$M.add` with `(i32 sext, f64, i32 sext) -> i32 sext` returns the right value for a negative argument.
4. [V] `sed -n 4192,4713p vm.rs | grep`: the generic arms use only `task.get / raw / float / set / set_raw / set_float` and `ctx`.
5. [V] (r2) Read `vm.rs:4033-4045` (`step_failed` takes `RunFault`), `eml_sched/src/error.rs:61-65`, `gui.rs:636-660` (`arm`), the Gui rows' Reprs (`eml_extern/src/lib.rs:2171-2240`), `eml_rt/src/heap/mod.rs:4156` (`alloc_foreign` returns `Word`), declarations.md L252.
6. [V] Read: `eml_vm/src/ffi.rs`, `gui.rs:25-112, 455-560`, `eml_native/src/{bridge.rs, abi.rs, jit.rs:160-235, unsupported.rs, lower/rows.rs}`, `eml_extern/src/c.rs`, `eml_cabi/src/lib.rs`, `eml_rt` `text_payload` / `unboxed_ptr[_mut]` / `foreign` / `alloc_foreign`, object-model.md T16.
7. Not checked: the full 230-row list (A-2); the arithmetic handlers through `rows::` (5.2); `func_addr` on the other three targets (A-3); any C lowering end to end; compile time.

## 8. Conformance criteria

- **G-1** MUST: at stage close `extern_rows!` has exactly the sections `rows`, `engine`, `lir`, `task` (`pending` exists only inside unit 5) and one line form, `fn V (| V)* (ctx, a: kind, ..) -> ret { body }`. MUST NOT: an entry identifier, an `inline` or VM-op mark, or any other per-engine fact in the list. Why: the list is engine-neutral. Applies: design, eml_sched. Check: `grep -n ' as eml_x_\|inline fn\|generic {\|dedicated {' crates/eml_sched/src/rows.rs` is empty.
- **G-2** MUST: the list is expanded in exactly two places, `defs` in eml_sched and `entries` in eml_nrt. MUST NOT: eml_vm or eml_native invoke `extern_rows!`. Check: `grep -rn 'extern_rows!(' crates/*/src` hits those two.
- **G-3** MUST: `eml_sched::rows::call` is one `match` over `Extern` with no wildcard arm at every commit from the list reshape on, and every variant appears in the list once. Why: a new row must fail to compile until listed. Check: read the expansion; `grep -n '_ =>' ` in the `call` arm of `defs` is empty; at close the test `SHARED == Extern::ALL` minus async rows, `Par`, `Exit`, `Panic` (230 rows).
- **G-4** MUST: `run_extern` handles `Panic` and `Exit` itself and passes every other row to `rows::call` through an `Args` implementation; each dedicated op handler calls its `rows::<First>` function. MUST NOT: eml_vm name `eml_rt::externs::*` or a `Ctx` row method for a listed row. Check: `grep -c 'Extern::' ` within `run_extern` counts only `Panic` and `Exit`; `grep -n 'externs::' crates/eml_vm/src/vm.rs` shows no row operation; `bench.rs` snapshots unchanged.
- **G-5** MUST: `LISTED`, `Listed`, `Section`, `names!`, `vm_arms`, `CALLED` do not exist. Check: `grep -rn 'LISTED\|vm_arms\|pub const CALLED' crates` is empty.
- **G-6** MUST: a row entry's symbol comes from the table prefix `"eml_x_"` and the variant name inside `entries!`. MUST NOT: a hand-spelled `eml_x_<Variant>` identifier in eml_sched or eml_nrt source outside tests and the `eml_x_c_*` helpers. Check: `grep -rn 'fn eml_x_' crates/eml_nrt/src` hits only `c.rs`.
- **G-7** MUST: an entry `eml_x_<Variant>` exists for exactly the rows with `!is_async()` and not `Par`, with a descriptor equal to the row's Reprs (`Never` -> no result). Check: the partition test over `Extern::ALL` in `crates/eml_native/tests`; 232 entries at the base table (230 generated, `Exit`, `Panic`), 45 `Inline` / 187 `Called` / `Par`.
- **G-8** MUST: `eml_native::rows::lowering` is `Inline` for `INLINE`, `Called` for every other non-async row except `Par`, and `None` otherwise; `Form::StdRow` and `Form::CRow` are deleted. Check: `grep -rn 'StdRow\|CRow' crates` is empty; the native snapshots' histogram keys.
- **G-9** MUST: the row shim keeps N-9 as written: park `RunFault::Extern` iff the row `faults`, record every other `Err`, never name `eml_fatal`. Check: `grep -n 'eml_fatal' crates/eml_nrt/src/rows.rs crates/eml_nrt/src/c.rs` is empty; the Miri `entries::x_*` tests.
- **G-10** MUST: a non-blocking C row lowers to a direct default-cc `call` of the import `eml_c$<canonical eml name>` with a signature from `abi::c_signature`, which `bridge.rs` also uses. MUST NOT: `call_indirect`, a bridge, or an argument array on that path. Check: the CLIF pin; `grep -rn 'fn c_signature' crates/eml_native/src` hits one function; `grep -rn 'Linkage::Import' crates/eml_native/src` hits one function (`declare_import`).
- **G-11** MUST: `NativeMachine::load` registers C callees and dtors with `JITBuilder::symbol` only after `Linked::fits` returned true. MUST NOT: `symbol_lookup_fn`, or a process address as an immediate. Check: `grep -rn 'symbol_lookup_fn' crates/eml_native/src` is empty; a test loads a program with a `Linked` of another program and gets the load error.
- **G-12** MUST: in the lowering of a C row, every narrowing check precedes the first pointer or handle entry; between that entry and the `call` only `eml_rt_text_payload`, `eml_rt_unboxed_ptr`, `eml_rt_unboxed_ptr_mut` and `eml_x_c_handle_ptr` are called; nothing is loaded after the `call` unless the result is a handle, where `fault` is loaded after `eml_x_c_handle_new`. The lending calls and the import `call` are emitted by one function, `lend_and_call`. Why: T16's window. Check: a test in `crates/eml_native/tests/c.rs` that takes the kept CLIF of every C-calling function its fixtures compile and asserts that between the first call of a lending entry and the call of `eml_c$..` every `call` names one of the four lending entries; the CLIF pin of a row with a narrowing parameter, a pointer parameter and a handle result.
- **G-13** MUST: the three pointer entries and `eml_x_c_handle_ptr` neither allocate, free, nor change a reference count. Check: review of their bodies (each is one `Heap` read); a Miri `entries::` test per entry.
- **G-14** MUST: the C helpers receive the function's name as a string-datum word and the dtor as the `dtor` parameter (`Option<Dtor>`); `eml_x_c_handle_new` carries the `# Safety` text of 5.3; a handle is made only by `eml_sched::ffi::handle_new`, in both engines. MUST NOT: a `c:` data symbol or a pointer to a machine-owned table in generated code or data in this stage; a dtor passed as an integer or made by a cast or `transmute`; an `h:` data symbol, unless U-7 applies. Check: `grep -rn '"c:\|"h:' crates/eml_native/src` is empty; `grep -n 'transmute\| as \*' crates/eml_nrt/src/c.rs` is empty; `grep -rn 'alloc_foreign' crates/eml_vm/src/ffi.rs crates/eml_nrt/src/c.rs` is empty.
- **G-15** MUST: a C-row panic (narrowing, null handle) is parked with `worker::park_panic` and raised through `eml$raise` with the row's purity, and its text comes from `eml_sched::ffi::Raise::text`, which the VM also calls. Check: `grep -rn 'does not fit in C\|returned a null handle' crates/*/src` hits one file; the `run-fail/ffi` files agree natively.
- **G-16** MUST: `CParam` gains no form through which C can call eml code without a re-entry design reviewed first. Check: review; `CParam`'s variants at close equal those at fddd44f2.
- **G-17** MUST: `CHandle`, `HandleCell`, `Raise`, `handle_new`, `empty_mut_array`, `tls.rs`, `gui.rs` are defined once, in eml_sched, and used by both engines. MUST NOT: move `c_links` / `renumbered` or the `blocking` path in this stage. Check: `grep -rn 'struct CHandle\|struct HandleCell\|enum Raise' crates/*/src` hits eml_sched only; `git diff <base> --stat -- crates/eml_vm/src/compile/mod.rs` shows no move.
- **G-18** MUST: the empty `MutArray` is a datum named `a:mut`, built by `eml_sched::constants::empty_mut_array` in both engines. Check: `grep -rn 'alloc_array_immortal' crates/*/src` outside eml_rt hits that function only.
- **G-19** MUST: the `ui::run_native` snapshot lists the non-async std rows that no accepted file contains, and at close that list is exactly the residue the plan names by rule (U-3). MUST NOT: a separate classifier binary or an `agree` test in eml_native as the coverage mechanism for a row a UI file can reach. Check: the snapshot.
- **G-20** MUST: the floors are re-measured at the start of unit 7 by the rule of 5.8 (today `run` >= 317 / `run-fail` >= 40 after the std-row unit and >= 323 / >= 42 at close), with every `-- native: skip` mark listed as kind 1. Check: the snapshots; `grep -rn 'native: skip' tests/ui | wc -l` equals 8 plus the plan's list.
- **G-21** MUST: the plan carries amendments M-1 to M-7 as user-confirmed items before the unit that relies on each, and the kind-1 list of 5.9 test by test. Check: the plan's sections.
- **G-22** MUST NOT: an eml_rt edit in this stage. Check: `git diff <plan commit> --stat -- crates/eml_rt` is empty.
- **G-23** MUST: every unit's VM-side change is kind 3 (UI output, REPL transcripts, `RunStats` byte-identical). Check: no pending snapshot outside the two native snapshots; `cargo nextest run -p eml_vm --test integration bench::`.
- **G-24** MUST: every commit of unit 5 passes `cargo nextest run`, with the list total over `Extern::ALL` through the `pending` group. MUST NOT: a wildcard arm in `call`, or a weakened test left after the unit's last commit. Check: `grep -n 'pending' crates/eml_sched/src/rows.rs crates/eml_nrt/src/rows.rs` is empty at close.
- **G-25** MUST: the plan shows M-1 to the user with its cost (45 uncalled shims, the exception to 01 §1.2) and the alternative of U-1 side by side. Check: the plan's amendment section.

---

## Response to review round 1

| Finding | Verdict | Reason and change |
|---|---|---|
| A-1 counts | **Accepted** | r1 was wrong. `Exit` and `Panic` are inside HEAD's 187 called rows. Correct: `rows` 230 (45 + 185), `engine` 2, `lir` 1, `task` 101 = 334; entries 232 = 230 generated + 2 by hand; lowering 45 `Inline` / 187 `Called` / `Par`. Fixed in 1, 5.3, 5.8, M-1, G-3, G-7, A-2 |
| A-2 dedicated ops | **Accepted** | 5.2 now states: the inline rows are lines with an unused `_` context (as at HEAD); both routes produce `RunFault` through the same `From<ArithFault>`, and `step_failed` takes `RunFault`, so it does not change [V]; the arithmetic path was not measured and is recorded, not gated |
| A-3 helper shape | **Partially accepted** | Specified in 5.1: selector matches become constants per line (no helper); Tls rows are already one function each; Gui keeps HEAD's `gui::arm` with the conversions written per line. Rejected: that `arm`'s inner `match` defeats the exhaustiveness argument. That argument is about `call`, which has one arm per row; `arm` dispatches toolkit operations and is unchanged code |
| B-1 green intermediate commits | **Accepted** | r1's transitional `_` arms are replaced by a `pending` name group that exists only in unit 5: the list is total and `call` wildcard-free at every commit; the `SHARED` test is the weak form until the last commit (5.1, 5.8, G-3, G-24) |
| B-2 `load` callers, jit-repl | **Partially accepted** | 5.4 cites the callers: both already pass the `Linked` made from the same program, so no caller unit is needed and `fits` is the guard. 5.12 and M-7 now say jit-repl must replace the registration and that `asked` is the change point. Rejected: that A-8 lacked a native-side check; `load`'s parameter and its callers exist at HEAD |
| B-3 the dtor cast | **Accepted, and the cast removed** | The token is now `dtor`, typed `Option<Dtor>`; the ABI does the conversion, so no `transmute` exists and no crate owns one. `Dtor` is re-exported by `eml_sched::ffi`, so eml_nrt gains no edge. The `# Safety` text, its owner (`c.rs`, unit 8, `trust.rs`) and the spec sentence are in 5.3; G-14 greps for casts |
| B-4 code size | **Partially accepted** | Rejected as a blow-up: `call` per mode holds what `run_extern` per mode holds today. Accepted: the build time is recorded and a 20 % threshold with a stated remedy is in 5.1 |
| C-1 ordering guard | **Accepted** | `lend_and_call` emits the lending calls and the import call with nothing else; a test scans the CLIF of every C fixture function for the window (G-12). No safepoint or unwind point can fall inside a statement (N-17). Retention is already forbidden by declarations.md L252; native adds no case |
| C-2 handle leak | **Partially accepted** | The premise is wrong: `alloc_foreign` returns `Word` and cannot fail on a limit [V]; null is checked before anything is built. Accepted as a divergence risk: the handle is now made by one shared `ffi::handle_new` that both engines call, which builds the `CHandle` before allocating so a panic in allocation still runs the dtor once (5.5, 5.6, G-14) |
| C-3 entries for inline rows | **Partially accepted** | Kept (a), entries for all, as the default, now with cost and benefit side by side in 5.3 and M-1 and a criterion that the user sees both (G-25). The middle route (a skip set in eml_nrt) is rejected with reasons: one `entries!` table cannot drop items without an accumulating muncher, membership needs a continuation-passing macro, and eml_nrt cannot see eml_native's expanders, so the tying test returns. U-1's fallback is spelled out in full. Correction to the finding: the 45 entries add no echo entry and no Miri test of their own |
| C-4 blocking C | **Partially accepted** | Decision kept; the path for jit-tasks is now shown (5.12): an `Image` C table of `{ CFunction, CCall }` built at load from `program.c_functions` and `linked.calls()`, and the datum `c:<Module.f>` pointing at the row. Rejected: that it needs the by-name map. That map resolved import symbols after module creation; the datum is defined by the load that knows the index. M-5 reworded to give `c:` to jit-tasks outright |
| C-5 fallback contradicts G-14 | **Accepted** | Unit 8 starts with the import test on the four CI targets; U-7 names the fallback and who decides; G-14 has the escape clause |
| C-6 guard page, stop requests | **Accepted** | 5.6 states both and that the VM is equal in kind: a guard-page hit in C kills the process on either engine; a stop is seen after C returns on either engine |
| C-7 coverage | **Partially accepted** | Rejected: per-row glue errors. The glue is generated per kind from const-checked tokens and `faults` comes from the table; the Miri tests now cover every kind (5.9). Accepted: floors are re-measured at the start of unit 7 by a stated rule, and the UI files have an owner (unit 7) and a way to get their count (5.8, G-20) |
