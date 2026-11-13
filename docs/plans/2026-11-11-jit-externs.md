# Stage jit-externs: every non-async extern row and every non-`blocking` user C function runs natively; the crate graph gets its strata

- Created: 2026-11-11; refreshed into this executable plan: 2026-11-13
- Base: main @ fddd44f2 (jit-apply closed, the signal-delivery fix merged). Every `file:line` below describes that commit. Line numbers are hints; the named item decides.
- Status: plan finalized. Every item of section 10 has a default. This file is in English by the user's decision U-G; `docs/`, code comments and CLAUDE.md stay Japanese / as CLAUDE.md requires.
- Appendices (English, `docs/plans/2026-11-11-jit/`): `00-decisions.md` wins over all others (its section F records this refresh); `01-contract.md` is the long-term contract (criteria N-1..N-43); `06-extern-boundary.md` and `07-crate-graph.md` are the designs of this stage and supersede `04-jit-externs.md`. This plan is self-contained for implementation; the appendices hold the full rationale. Where an appendix and this plan differ, this plan and 00 section F win.

## 1. Goal and success criteria

- Goal:
  1. The workspace has three strata (runtime closure, compilers and engines, artifacts) checked by one script, `nix/crate-graph.sh`. Two new crates: `eml_rows` (what the built-ins are at run time) and `eml_clink` (linking user C).
  2. `extern_rows!` in `eml_rows` names all 334 rows once. Every non-async row except `Par` has one shared body that both engines run; the VM reaches it through `eml_rows::rows::call`, native code through the entry `eml_x_<Variant>`.
  3. Native code calls every non-async std row and every non-`blocking` user C function. `Form::StdRow` and `Form::CRow` are deleted.
- Success criteria (checks in section 9):
  - S-1: the native snapshots' histogram keys are only `par`, `async row`, `blocking c`, `prim`, `clause`, `prompt`.
  - S-2: `tests/ui/run` accepts exactly `326 + F` files natively (F = the `run/` files this stage adds) and `tests/ui/run-fail` exactly 43, each agreeing with the VM. Floors: 323 / 42 (G-20).
  - S-3: `rows::the_rows_are_partitioned` (45 / 187 / 1 / 101; 232 entries), `rows::tests::shared_lists_each_row_with_a_body_once` (230) and `rows::tests::rows_that_do_not_fault_return_ok_on_an_exhausted_budget` pass.
  - S-4: the `unlowered` section of the `ui::run_native` snapshot is exactly the 31-row residue of D-32.
  - S-5: every VM expected value is byte-identical: UI outputs, REPL transcripts, `RunStats` snapshots (kind 3).
  - S-6: `bash nix/crate-graph.sh --check` exits 0; `eml_vm`, `eml_nrt`, `eml_native` declare no feature.
  - S-7: native numbers are recorded; `bench/scripts.sh` has 4 `(native)` rows.
  - S-8: CI is `success` on the four targets for the Close commit (CP-Z).

## 2. Scope

- In scope: the tasks of section 7; the docs each task names; the amendments of D-3.
- Out of scope (owner):
  - bodies of the 101 async rows, `par`, `blocking` C, `CSubmit` / `CTake`, the datum `c:<Module.f>`, the worker-table move to `eml_sched`, deleting `GateHost` and `eml_sched/testing`: jit-tasks.
  - effect / root / evidence prims, `clause`, `prompt`, catching a C-row panic with `try_panic`: jit-effects.
  - `c_links` / `renumbered`, C tables across REPL appends, repeated native loads: jit-repl.
  - inline expansion of container rows and of C pointer arguments: native-inline.
  - `eml_rtlib`, the runtime-library variants and notices, binding `eml_c$` / `eml_cd$` at link, removing `TestDriver` from apps: native-aot. This stage only writes the decision (U-F) into `docs/`.
  - any edit of `crates/eml_rt` (G-22). If one turns out to be needed, stop and report.

## 3. Background context

All facts are at fddd44f2. [V] = verified at the refresh; [I] = inferred.

Rows and the list:
- `Extern::ALL` has 334 rows (`crates/eml_extern/src/lib.rs:359`); `is_async()` is true for 101 (`:2251`). 334 = 45 inline + 187 called + `Par` + 101 task. The 187 include `Exit` and `Panic` [V].
- `extern_rows!` lives in `crates/eml_sched/src/rows.rs`. `LISTED` has 54 rows: 8 generic (`Println`, `Eprintln`, `IntShow`, `StrShow`, `FloatShow`, `StrConcat`, `StrEq`, `StrCompare`), `FloatMod`, and the 45 inline rows; its engine / lir / task sections are empty. Grammar at base: `as eml_x_<V>` identifiers, an `inline fn` mark, sections `generic` / `dedicated`; `arg_ty!(unit)` is `i64`; `entries!` has the kind tokens `obj tobj unit int enum float`, `ptr`, an extra `token` kind, and `never`. There is no `vm_arms` expansion [V].
- The `entries` expansion of the list is in `crates/eml_nrt/src/rows.rs` (an accumulating muncher, `rows_table!`); `eml_x_Exit` / `eml_x_Panic` are hand-written. `eml_nrt` re-exports the list as `shared_rows` and `LISTED`, `Listed`, `Section` (`crates/eml_nrt/src/lib.rs`). `eml_nrt` has 11 row entries today.
- `eml_native::lower::rows::CALLED` has 11 rows (`crates/eml_native/src/lower/rows.rs:91-103`); `INLINE` has 45.

The VM:
- `run_extern` is `crates/eml_vm/src/vm.rs:4159-4900`: no wildcard, 334 rows. Its generic arms use only `task.get / raw / float / set / set_raw / set_float` and `ctx` [V]. `FailIo`'s body is `vm.rs:4535-4541`; the Tls rows are `vm.rs:4477-4507`; the Gui rows `vm.rs:4511-4531`.
- The dedicated op handlers are `vm.rs:1203-1253` and `1326-~1381`. `step_failed` takes a `RunFault` (`vm.rs:4033-4039`); `From<ArithFault> for RunFault` is `crates/eml_sched/src/error.rs:61-65`.
- `crates/eml_vm/src/tls.rs` (538 lines) has no function taking `&mut Scheduler<P>`. `crates/eml_vm/src/gui.rs` (821 lines): `abandon` :36, `lower_holds` :51, `on::want` :463, `on::open_ui` :489 (two stubs in `mod off`), called from `async_op` (`vm.rs:3650-3667`); `arm` is `gui.rs:643`. `eml_vm/src` has `feature = "…"` only in `lib.rs`, `gui.rs`, `tls.rs` [V].
- `crates/eml_vm/src/ffi.rs`: `CHandle { Arc<HandleCell { ptr, dtor }> }` :28-46, `Raise` :50, the `Handle` arm of `unmarshal` :252-262, the argument numbering of the narrowing panic :80-90, pointer exposure :117-121. `c_links` is `compile/mod.rs:185-221`, `renumbered` :275-298. `empty_mut_array` is `compile/constants.rs:112-118` (`alloc_array_immortal(true)`).
- `crates/eml_vm/src/hosts.rs:610-632` holds `stdio_host`, `script_host`, `unmasked`; the rest of the file is the capture fixtures.
- `crates/eml_vm/src/lib.rs:37-56` re-exports, among its own items, `Doorbell`, `Clock`, `Signal`, `VirtualSignal`, `Bridge`, `CCall`, `Dtor`, `Linked`, `eml_gui`, `Masked`, `Control`, `HostRef`, `Limits`, `Stop`, `Finished`, `RunConfig`, `RunStats`, `RunFault`, `RuntimeError`, `SourceLocation`, `gui_park`, the `park` items; `sched.rs:17` re-exports `Sched`, `workers.rs:25` `Seat`.

Native code:
- `NativeMachine::load` (`crates/eml_native/src/jit.rs:210-231`) builds the `JITBuilder` locally, registers entries with `builder.symbol`, ignores its `_linked` parameter, and loads once per machine. Both callers already pass the `Linked` made from the same program (`eml_cli` `execute_on`; the harness, `crates/eml_cli/tests/ui.rs:242` `eml_test_support::link`) [V].
- The one `Linkage::Import` is `abi::declare_entry` (`crates/eml_native/src/abi.rs:78-82`). Data symbols are 8-byte slots (`lower/consts.rs:72`); `Datum` is `{String, BigInt, Float, Fn}`; a new datum kind touches `lower/consts.rs`, `jit.rs` `Tables::new`, `crates/eml_nrt/src/run.rs` `fill_data` (:130-170) and `crates/eml_nrt/src/constants.rs`.
- Every `Rhs::Prim` is `Form::Prim` today, `Prim::EmptyMutArray` included. `Form` order (a file is counted under its first form): `Par`, `AsyncRow`, `BlockingC`, `Prim`, `Clause`, `Prompt`, `StdRow`, `CRow` (`crates/eml_native/src/unsupported.rs:11-20`).
- `eml_cabi::Linked { functions, calls, handles, dtors }` (`crates/eml_cabi/src/lib.rs:27-73`): `calls()` / `dtors()` are in `Program::c_functions` / `c_handles` order once `fits` holds. `CHandleType { name, dtor, links }` is `crates/eml_extern/src/c.rs:112`; a C row borrows every `Obj` argument (`c.rs:157-160`).
- `eml_native/src/{linker,library,bridge,error}.rs` reference only each other [V]. Four placement tests exempt `bridge.rs` (`crates/eml_native/tests/placement.rs:265, 299, 411, 420`); `placement::eml_native_has_no_edge_to_eml_sched` is `:283`.
- `Ctx::bytes_empty` is infallible (`crates/eml_rt/src/bytes.rs:407`); `alloc_foreign` returns a `Word` (`crates/eml_rt/src/heap/mod.rs:4156`).

Counts and tests:
- Native snapshots (`crates/eml_cli/tests/snapshots/integration__ui__run_native.snap`, `…run_fail_native.snap`): run accepted 208 / skipped 6 / rejected 422 (async row 129, c row 2, par 22, prim 156, std row 113); run-fail accepted 8 / checked only 0 / skipped 2 / rejected 71 (async row 33, c row 2, par 1, prim 3, std row 32). Files whose only forms are std row / C functions: run 115, run-fail 34.
- 8 files carry `-- native: skip` (the `limit_depth_*` files: 6 in `run/async`, 2 in `run-fail/async`).
- The acceptance classifier is `native_entries` in `crates/eml_cli/tests/ui.rs:~805` over `eml_native::unsupported`. No classifier binary exists.
- FFI UI files: `run/ffi` 9, `run/async/blocking` 5, `run-fail/ffi` 2, `run-fail/async/blocking` 2; REPL `ffi` transcripts 2. Fixtures are Rust `extern "C"` functions bound by address.
- Test counts: `eml_sched` `rows::tests` 3 and `constants::` 4; `eml_native` `bridge::` 9 + `linker::` 16, `trust::` 3; `eml_vm` lib `tls::` 11 + `gui::` 2, integration `gui::` 18; `tls_off` / `gui_off` 3 tests in all [V].
- `bench/*.em`: 42 programs; `--native` accepts 27 and rejects 15, 10 of them for std rows only.
- CI (`.github/workflows/ci.yml`, workflow `CI`): jobs `test` (ubuntu-24.04, ubuntu-24.04-arm, macos-15, windows-2025; `defaults.run.shell: bash`), `gui-native`, `fmt`, `verus`, `licenses`, `miri`, `asan`, `tsan`. There is no cross job. The step `dependency edges of eml_native` holds five `cargo tree` greps, the `eml_sched.*testing` grep and the `eml_extern` `unsafe` grep.
- Doc anchors: spec/vm.md L491 「GUIのPark」, L1091 (「橋渡しの関数」); architecture.md L103, L333 (the `eml_vm` bullet), L428, L442, L445; testing.md L739 「GUIの手での確認」; spec/native.md L116, L204, L223, L225; future/ffi.md L30, L102, L107, L119, L125, L128.
- The test `tls::a_real_socket_handshake_rejects_the_expired_fixtures` was slow and failed under load because `target/debug/deps` held about 448k stale files: on macOS the first `SecPolicyCreateSSL` of a process reads the directory of the executable. The dev profile has been cleaned (`cargo clean --profile dev`); CI was never affected.

## 4. Prerequisites

| Needed | For | How to check | If missing |
|---|---|---|---|
| Base commit | all | `git rev-parse --short HEAD` on `main` → `fddd44f2` | stop and report (the plan's facts need a refresh) |
| Nix devShell (cargo-nextest, cargo-insta, cargo-about) | all | `cargo nextest --version` → prints a version | stop and report |
| `.#miri` shell | G-a, G-b, U2a, U2b, U5f, U5g, U5j, U6, 7a, 8b, 8d | `nix develop .#miri -c cargo miri --version` → prints a version | stop and report |
| `.#cross` shell | GS lines 9, 15, 17; U4; U5i; 8d | `nix develop .#cross -c cargo --version` → prints a version | record those lines as skipped in the commit message; CP-A's Windows CI covers the default features (Q-6) |
| PLaMo (`plamo-translate` MCP tool or CLI) | every Japanese doc and comment | `plamo-translate --help` → exit 0 | follow the `natural-japanese:natural-japanese` skill (CLAUDE.md) |
| shellcheck, actionlint | G-a, G-b, U3, U4, 9 | `nix shell nixpkgs#shellcheck -c shellcheck --version` → prints a version | stop and report |
| macOS | U1 baseline, U5a (`bench/run.sh`), 9, 10 (measurement) | `uname` → `Darwin` | U5a: skip the record and say so in the message; 9 and 10: the runs and the table replacement move to a commit `Measure jit-externs` right after Close |
| `gh`, and the user's permission before each push | CP-A, CP-B, CP-C, CP-7, CP-Z (orchestrator) | `gh auth status` → logged in | ask the user; the stage does not close without CP-Z. The user allowed (2026-11-13) pushing `plan/jit-externs` at every checkpoint and after CI fixes without asking each time; merging into `main` is still asked |
| A clean dev profile | U1 | `ls -f target/debug/deps \| wc -l` → well under 200,000 | ask the user before `cargo clean --profile dev` (it deletes the build cache) |

This stage does not edit `crates/eml_rt`, so Verus, verusfmt and `bench/rt-gate.sh` are not needed.

## 5. Decisions

User decisions of the refresh (2026-11-13; recorded in 00 section F):

| ID | Decision |
|---|---|
| U-A | "If redesigning the architecture overall makes it elegant, do so aggressively. Let Fable do the redesign." → the extern / C boundary was redesigned (appendix 06, D-1) |
| U-B | The slow TLS test is caused by stale files in `target/debug/deps`; clean the dev profile and rebuild in the stage's first task, verify the test runs under 1 s, and write the explanatory paragraph in testing.md 「性能のテスト」 in its own commit (U1, UT) |
| U-C | Approved M-1 (entries for all 232 non-async rows except `Par`, the 45 inline rows included) and M-2..M-7 (D-3) |
| U-D | "Also aggressively redesign cross-crate dependencies and splits, aiming for an elegant design." → the crate graph was redesigned (appendix 07, D-2) |
| U-E | "Organize dependencies with a particular focus on licensing and portability during native app generation, speed and size." → the primary criterion of D-2 |
| U-F | Approved AM-1..AM-13 and AM-12b; the graph tasks G-a..G-d run at the head of this stage; a shipped app's `tls` variant is chosen by import (`Std.Tls` / `Std.Http` in the module graph), its `gui` axis by profile |
| U-G | This plan may be written in English |

### 5.1 Long-term guidelines (changing these requires re-examination)

If implementation conflicts with any line of this section, stop and report. A soundness or memory-safety problem beyond what D-1 settles (an `unsafe` boundary safe code can reach, a broken precondition, aliasing, a lifetime across the fatal path or a thread) goes to a strong advisor before any fix (CLAUDE.md).

#### D-1: the extern / C-call boundary (appendix 06)

- Decision:
  - **One list, one line form.** `extern_rows!` in `crates/eml_rows/src/rows.rs`: `rows { fn V (| W)* (ctx, a: kind, ..) -> ret { body } .. }` (230 rows), `engine { Panic, Exit }`, `lir { Par }`, `task { 101 names }`. No entry identifier, no `inline` mark, no `generic` / `dedicated` split.
  - **Two expansions.** `defs` (in `eml_rows`) emits the row functions, the const checks against `Extern::<V>.row()`, `SHARED`, `Args`, `Value` and `call`. `entries` (in `eml_nrt`) emits one `shim` entry per variant.
  - **The VM calls a function.** `run_extern` is the `Panic` / `Exit` prelude plus `eml_rows::rows::call`, an exhaustive `match` with no wildcard. Dedicated op handlers call `rows::<First>` directly.
  - **Entries.** The symbol is the table prefix `"eml_x_"` plus the variant name. Every non-async row except `Par` has one: 232 = 230 generated + `Exit` + `Panic` by hand. `eml_native`'s `INLINE` table alone decides `Inline` (45) or `Called` (187).
  - **Failure protocol unchanged** (N-9): park `RunFault::Extern` iff the row `faults`, record every other `Err`; user-level panics go through `park_panic` and `Fault::Engine`.
  - **User C.** A direct default-cc `call` of the import `eml_c$<canonical eml name>`, registered with `JITBuilder::symbol` in `load` after `Linked::fits`. A handle type's dtor is the import `eml_cd$<handle type name>`, of which generated code takes only the address. No `c:` / `h:` data symbol, no pointer into machine tables, no `symbol_lookup_fn`. The only new datum is `a:mut`.
  - **Shared C pieces.** `eml_rows::ffi::{CHandle, HandleCell, Raise, Dtor, handle_new}`; `eml_rows::constants::empty_mut_array`. `Std.Tls` and `Std.Gui` move by file to `eml_rows` with the trait `GateHost`.
  - **Coverage.** The `ui::run_native` snapshot lists the lowered std rows no accepted file contains; UI files close the gaps. No classifier tool.
  - **Soundness of the C call** (06 section 5.6): all narrowing checks run before the first pointer is taken; between the first lending entry and the `call` only the four lending entries run (they read, and neither allocate, free nor change a count); no safepoint or unwind point lies inside a statement (N-17); `eml_x_c_handle_new` allocates after C has returned; `handle_new` checks null before building anything and builds the `CHandle` before allocating, so a handle is never lost and its dtor runs once; callee addresses are registered only after `fits`; `CParam` has no callback form, so no entry runs under a C frame; a C function must not unwind across the call or keep a lent pointer past its return.
- Rationale: a `macro_rules!` callback list is the only stable-Rust way to keep a row's body, signature and binding in one place and still stamp one `extern "C"` function per row. The engine-specific facts (which VM op, whether native expands inline, the entry identifier) each cost grammar and a tying test and none is needed. A generated `call` keeps exhaustiveness at compile time and gives tests one way to run any row. Descriptors in data would put raw pointers to machine-owned tables into generated data; the cold paths need only a name and an address.
- Rejected alternatives: generating the list from `eml_extern`'s table (bodies cannot live in a dependency-free crate); the VM calling the native shims (`Fast`-only, C ABI); `rows::call(ext, ctx, &[Word])` (bakes the VM's float slot encoding in); an out-pointer in place of parking (01 section 1.4 fixes the protocol); the `inline` mark with no entries for the 45 rows (needs a muncher, a mark and a tying test; the user chose entries for all, U-C); shared C pieces in `eml_cabi`; `symbol_lookup_fn` over a shared map (loads do not repeat until jit-repl).
- Scope of impact: `eml_rows`, `eml_nrt`, `eml_vm` (`vm.rs`, `ffi.rs`), `eml_native`, `eml_clink`, `eml_extern` (`CInt::code` only).
- Revisit when: a `CParam` callback form is wanted (needs a re-entry design first); jit-repl makes loads repeat (it replaces the `builder.symbol` registration); jit-tasks adds `c:<Module.f>`.
- Conformance checks (G-1..G-25; the commands are in section 7, G-grep):
  - **G-1** MUST: at stage close `extern_rows!` has exactly the sections `rows`, `engine`, `lir`, `task` and the one line form. MUST NOT: an entry identifier, an `inline` or VM-op mark, or another per-engine fact in the list. Check: `grep -n ' as eml_x_\|inline fn\|generic {\|dedicated {' crates/eml_rows/src/rows.rs` → empty.
  - **G-2** MUST: the list is expanded in exactly two places, `defs` in `eml_rows` and `entries` in `eml_nrt`. MUST NOT: `eml_vm`, `eml_native` or `eml_sched` invoke `extern_rows!`. Check: `grep -rln 'extern_rows!(' crates/*/src` → those two files.
  - **G-3** MUST: `eml_rows::rows::call` is one `match` over `Extern` with no wildcard arm at every commit from U2a on, and every variant is in the list once. Check: the `_ =>` count inside `defs` is 0; from U5j the `SHARED` test (230 rows).
  - **G-4** MUST: `run_extern` handles `Panic` and `Exit` itself and passes every other row to `rows::call` through an `Args` implementation; each dedicated op handler calls its `rows::<First>` function. MUST NOT: `eml_vm` name `eml_rt::externs::*` or a `Ctx` row method for a listed row. Check: the U5a, U5f, U5j lines of G-grep; `bench::` snapshots unchanged.
  - **G-5** MUST NOT exist: `LISTED`, `Listed`, `Section`, `names!`, `vm_arms`, `CALLED`. Check: G-grep (U2a; `CALLED` from 7b).
  - **G-6** MUST: a row entry's symbol comes from the prefix and the variant name inside `entries!`. MUST NOT: a hand-spelled `eml_x_<Variant>` identifier in `eml_rows` or `eml_nrt` source outside tests and the `eml_x_c_*` helpers. Check: `grep -rn 'fn eml_x_' crates/eml_rows/src crates/eml_nrt/src` → empty; from 8b only `crates/eml_nrt/src/c.rs`.
  - **G-7** MUST: an entry `eml_x_<Variant>` exists for exactly the rows with `!is_async()` that are not `Par`, with a descriptor equal to the row's Reprs (`Never` → no result). Check: `rows::the_rows_are_partitioned` (7b).
  - **G-8** MUST: `lowering` is `Inline` for `INLINE`, `Called` for every other non-async row except `Par`, `None` otherwise; `Form::StdRow` and `Form::CRow` are deleted. Check: `grep -rn 'StdRow\|CRow' crates` → empty (from 8d); the snapshots' histogram keys.
  - **G-9** MUST: the row shim keeps N-9 as written and never names `eml_fatal`. Check: `grep -n 'eml_fatal' crates/eml_nrt/src/rows.rs crates/eml_nrt/src/c.rs` → empty; the Miri `entries::` tests.
  - **G-10** MUST: a non-blocking C row lowers to a direct default-cc `call` of `eml_c$<name>` with a signature from `eml_clink::c_signature`, which the bridges also use. MUST NOT: `call_indirect`, a bridge, or an argument array on that path. Check: `clif::a_c_row`; `grep -rn 'fn c_signature' crates/*/src` → one hit, under `crates/eml_clink/src`; `Linkage::Import` only in `abi.rs` (`declare_import`).
  - **G-11** MUST: `load` registers C callees and dtors with `JITBuilder::symbol` only after `Linked::fits` returned true. MUST NOT: `symbol_lookup_fn`, or a process address as an immediate. Check: `grep -rn 'symbol_lookup_fn' crates/eml_native/src` → empty; `c::a_linked_of_another_program_is_refused`.
  - **G-12** MUST: in the lowering of a C row every narrowing check precedes the first lending entry; between that entry and the `call` only `eml_rt_text_payload`, `eml_rt_unboxed_ptr`, `eml_rt_unboxed_ptr_mut`, `eml_x_c_handle_ptr` are called; nothing is loaded after the `call` unless the result is a handle, where `fault` is loaded after `eml_x_c_handle_new`. The order is held by the structure of one function, `lend_and_call`. Check: `c::the_lending_window_holds_only_lending_entries`, which also asserts that no lending call precedes a narrowing `brif` of the same C call; `clif::a_c_row`.
  - **G-13** MUST: the three pointer entries and `eml_x_c_handle_ptr` neither allocate, free, nor change a reference count. Check: review (each body is one `Heap` read); one Miri `entries::` test per entry.
  - **G-14** MUST: the C helpers receive the function's name as a string-datum word and the dtor as the `dtor` parameter (`Option<Dtor>`); `eml_x_c_handle_new` carries the `# Safety` text of task 8b; a handle is made only by `eml_rows::ffi::handle_new`, in both engines. MUST NOT: a `c:` data symbol or a pointer to a machine-owned table in generated code or data; a dtor passed as an integer or made by a cast or `transmute`; an `h:` data symbol unless Q-2 fired. Check: `grep -rn '"c:\|"h:' crates/eml_native/src` → empty; `grep -n 'transmute\| as \*' crates/eml_nrt/src/c.rs` → empty; `grep -rn 'alloc_foreign' crates/eml_vm/src/ffi.rs crates/eml_nrt/src/c.rs` → empty.
  - **G-15** MUST: a C-row panic (narrowing, null handle) is parked with `worker::park_panic`, raised through `eml$raise` with the row's purity, and its text comes from `eml_rows::ffi::Raise::text`, which the VM also calls. Check: `grep -rln 'does not fit in C\|returned a null handle' crates/*/src` → `crates/eml_rows/src/ffi.rs`; the `run-fail/ffi` files agree natively.
  - **G-16** MUST NOT: `CParam` gain a form through which C can call eml code. Check: its variants at close equal those at fddd44f2 (7).
  - **G-17** MUST: `CHandle`, `HandleCell`, `Raise`, `handle_new`, `empty_mut_array`, `tls.rs`, `gui.rs` are defined once, in `eml_rows`, and used by both engines. MUST NOT: move `c_links` / `renumbered` or the `blocking` path in this stage. Check: G-grep (U6); `git diff --exit-code fddd44f2 -- crates/eml_vm/src/compile/mod.rs` → exit 0.
  - **G-18** MUST: the empty `MutArray` is a datum named `a:mut`, built by `eml_rows::constants::empty_mut_array` in both engines. Check: `alloc_array_immortal` outside `eml_rt` only in `crates/eml_rows/src/constants.rs`.
  - **G-19** MUST: the `ui::run_native` snapshot lists the lowered std rows that no accepted file contains, and at close that list is exactly D-32's residue. MUST NOT: a classifier binary, or an `agree` test in `eml_native` as the coverage mechanism for a row a UI file can reach. Check: the snapshot.
  - **G-20** MUST: the floors are re-measured at the start of 7b (procedure FL): `run` ≥ 317 / `run-fail` ≥ 40 after 7b, ≥ 323 / ≥ 42 at close; every `-- native: skip` mark is listed as kind 1. Check: the snapshots; `grep -rn 'native: skip' tests/ui | wc -l` → 8.
  - **G-21** MUST: this plan carries M-1..M-7 as user-confirmed (D-3) and the kind-1 list test by test (section 6).
  - **G-22** MUST NOT: an `eml_rt` edit in this stage. Check: `git diff --stat fddd44f2 -- crates/eml_rt` → empty.
  - **G-23** MUST: every VM-side change is kind 3. Check: K; `cargo nextest run -p eml_vm --test integration bench::`.
  - **G-24** MUST: every commit of the stage passes `cargo nextest run`. Transitional groups exist only between their opening and closing commits, each listed here: `pending` in the list from U2a to U5i (U5j deletes it); `NOT_YET_NATIVE` in `unsupported.rs` inside 7b (7b1 to 7b's last commit), only if 7b is split. While `pending` exists the list is total over `Extern::ALL` through it. MUST NOT: a wildcard arm in `call`, or a weakened test left after the closing commit. Check: `grep -n 'pending' crates/eml_rows/src/rows.rs crates/eml_nrt/src/rows.rs` → empty from U5j; `grep -rn 'NOT_YET_NATIVE' crates` → empty from 7b's last commit.
  - **G-25** MUST: M-1 was shown to the user with its cost (45 uncalled shims, the exception to 01 section 1.2) beside the alternative (the `inline fn` mark). Done: U-C, recorded in D-3 and 00 section F.
- Kept in the repository: spec/native.md 「記述子の表」「行の下ろし方」「externの失敗」 (U2a, 7b), 「エントリのABI」「ヒープのエントリ」 (8b), 「値と定数」 (7a), 「Cの関数の呼び出し」「断る形」 (8d); spec/vm.md 「基本操作と extern」 (U5j, U6), 「C の関数の命令」 (8d); spec/object-model.md T16 (8d); testing.md (U2a, U5j, 7b, 7e, 8d); the placement, partition and window tests themselves.

#### D-2: the crate graph and the shipped-app rules (appendix 07)

- Decision:
  - **Strata.** Runtime closure (what a shipped native app links): `eml_rt`, `eml_extern`, `eml_io`, `eml_cabi`, `eml_sched`, `eml_rows`, `eml_nrt`, and behind features `eml_tls`, `eml_gui`. Compilers and engines: the front end, `eml_lir`, `eml_core_ir`, `eml_vm`, `eml_native`, `eml_clink`. Artifacts: `eml_cli` (and from native-aot `eml_rtlib`); `eml_test_support` is dev-only. An edge never points from the runtime closure to a compiler or engine crate.
  - **Rule 1 (strata).** `eml_rt`, `eml_extern`, `eml_tls`, `eml_gui` depend on nothing in the workspace.
  - **Rule 2 (features).** A non-artifact crate declares a feature only if its own `src/` or `tests/` has a `cfg` on it. Only an artifact crate declares a feature whose content is another crate's feature. `eml_rows` declares `tls`, `gui`, `gui-native`; `eml_cli` switches them on; `eml_vm`, `eml_nrt`, `eml_native`, `eml_sched` have none of them. `eml_rows/tls` does not enable `eml_io/tls` (the `tls.localhost` terminator is a test facility of the tool).
  - **Rule 3 (re-exports).** A crate re-exports from another workspace crate only types of its own public signatures; no function, module, constant or macro (`#[doc(hidden)] pub use eml_rt as __rt` excepted). The kept list is the `REEXPORTS` table of task G-d.
  - **Rule 4.** `eml_native`'s `[dependencies]` never name `eml_sched`, `eml_rows`, `eml_io` or `eml_vm`.
  - **Rule 5.** `#[cfg(test)]` code in the `src/` of `eml_rows`, `eml_sched`, `eml_nrt`, `eml_io` names no engine crate and not `eml_test_support` (the dev cycle would link a second copy of the crate under test). Such a test is an integration test in `tests/`.
  - **Dead-strip rules.** Code reachable from the run path of `eml_nrt` never reads the entry descriptor tables, and no table or entry carries `#[used]`; `eml_nrt` never calls `rows::call` or another dispatcher over `Extern`.
  - **Shipped apps (decided now, built in native-aot; U-F).** The runtime static library is built by the artifact crate `eml_rtlib` in four variants `{base, tls, gui, tls+gui}`. The profile decides the `gui` axis; whether the module graph contains `Std.Tls` (directly or through `Std.Http`) decides the `tls` axis. Each variant has one notice file with three parts: the variant's crates; the Rust standard library with the crates it vendors; for `gui` variants the system-library section.
  - Target direct edges: the base table and the per-task deltas of section 7.1 ("Script tables").
- Rationale: without `rows.rs`, `eml_sched` needs only `eml_rt`, `eml_io` and libc; row bodies are the meaning of the extern table and change for other reasons than the task layer. Four contract criteria carried a "`bridge.rs` is exempt" clause; the C linker shares no code with the code generator. Features decide what an app carries, so they are declared once and switched on by whoever defines an artifact. Licensing by construction needs variants, not linker luck.
- Rejected alternatives: one general "runtime" crate (five optional edges and three features in front of the scheduler); merging `eml_cabi` into `eml_extern`, `eml_clink` or `eml_rows::ffi`; forwarding features on `eml_vm`; moving all of `hosts.rs` to `eml_io` (600 lines of fixtures into every app); a Rust test that parses every `Cargo.toml` (Cargo's own resolution in a script needs no parser); one static library relying on dead-stripping for Tls.
- Scope of impact: every `Cargo.toml`, `ci.yml`, architecture.md, CLAUDE.md, later native stages.
- Revisit when: native-aot designs `eml_rtlib` (G-C15..G-C17); jit-tasks moves the worker table (G-C18); `eml build` adds an object and link driver (splitting `eml_cli`).
- Conformance checks (G-C1..G-C19; "script" = `bash nix/crate-graph.sh --check`, CI step `crate graph`):
  - **G-C1** MUST: the workspace members are the 18 crates of fddd44f2 plus `eml_rows` and `eml_clink` (from native-aot `eml_rtlib`). Check: the script's edge table has one row per directory of `crates/`. From G-a.
  - **G-C2** MUST: every direct workspace edge (normal, optional, target-specific) is in the script's table. MUST NOT: a new edge without editing the table in the same commit. Check: the script. From G-a.
  - **G-C3** MUST NOT: `eml_vm` reach Cranelift, `eml_native`, `eml_nrt` or `eml_clink`; `eml_native` reach `eml_vm`; `eml_lir` reach `eml_rt`; `eml_sched` reach `eml_extern`, `eml_rows`, `eml_cabi`, `eml_tls`, `eml_gui`, Cranelift, `eml_lir`, `eml_vm`, `eml_nrt`, `eml_native`. Check: the script's closure rows.
  - **G-C4** MUST NOT: `eml_native`'s `[dependencies]` name `eml_sched`, `eml_rows`, `eml_io` or `eml_vm`; `eml_native/src` name `eml_sched` or `eml_rows`. Check: `placement::eml_native_has_no_edge_to_the_shared_runtime`, which reads the manifest from `[dependencies]` to the next line starting with `[`; `grep -rnw 'eml_rows\|eml_sched' crates/eml_native/src` → empty.
  - **G-C5** MUST: among non-artifact crates `tls`, `gui`, `gui-native` are declared by `eml_rows` only; `eml_io` declares `tls`, `eml_gui` `native`, `eml_sched` `testing` until jit-tasks; `eml_vm`, `eml_nrt`, `eml_native`, `eml_clink`, `eml_cabi`, `eml_rt`, `eml_extern` declare none. MUST NOT: a feature whose only content is another crate's feature outside an artifact crate (`eml_test_support`'s stage features are the one listed exception). Check: the script's feature table. From U4.
  - **G-C6** MUST: a `pub use eml_<crate>::…` outside `eml_cli` and `eml_test_support` names only types, and only those of the `REEXPORTS` table. Check: the script. From G-d.
  - **G-C7** MUST: (a) `eml_vm/src`, `eml_nrt/src`, `eml_native/src` have no `feature = `; (b) rule 5; (c) the feature-off run really has the features off. Check: (a), (b) G-grep (U4); (c) the two `cargo tree … -i eml_rows` lines of GS 13, also in CI's step `feature resolution of tls`. From U4 (the tls half from U3).
  - **G-C8** MUST: in `eml_native/src`, `JITModule` is named only in `jit.rs`, `settings::builder` only in `abi.rs`, `ins().load|store` only in `mem.rs` and `thunk.rs`, `call_indirect` only in `lower/apply.rs` and `thunk.rs`. MUST NOT: any file of `eml_native` listed as an exemption. Check: the four placement tests; `grep -c bridge crates/eml_native/tests/placement.rs` → 0. From G-b.
  - **G-C9** MUST: `stdio_host` and `script_host` are defined in `eml_io`, over `eml_rt::StdHost`. MUST NOT: a test fixture (`CaptureHost`, `Capture`, `Written`, `RecordingHost`) defined in a crate of the runtime closure outside `#[cfg(test)]`; `eml_io::TestDriver` is the recorded exception until native-aot. Check: G-grep (G-c), in the comment-dropping form (`crates/eml_io/src/mask.rs:180` names `CaptureHost` in a `///` comment).
  - **G-C10** MUST NOT: the runtime closure contain a compiler, an engine, a loader or a tool crate, in any variant. Check: the script, for the feature sets none, `eml_rows/tls`, `eml_rows/gui-native`, `eml_rows/tls,eml_rows/gui-native` (the rows arrive with G-a, U3, U4).
  - **G-C11** MUST: the base variant carries no Tls and no Gui crate; the `tls` variant no Gui crate; the `gui` variant no Tls crate. Check: the script. From U4.
  - **G-C12** MUST NOT: `eml_rows/tls` enable `eml_io/tls`. Check: the script. From U3.
  - **G-C13** MUST: the base runtime closure builds without a C or C++ toolchain. Check: the script (`cargo tree -p eml_nrt -e normal,build` has no `cc`, `cmake`, `cxx-build`, `bindgen`, `nasm`, `pkg-config`); locally GS line 17 (the three cross targets). No CI cross job is added. From G-a.
  - **G-C13b** MUST: the Tls runtime closure builds natively on the four CI targets. Check: CI test job, every OS: `cargo build -p eml_nrt --features eml_rows/tls`. From U3.
  - **G-C14** MUST NOT: non-test code of `eml_nrt` call `eml_rows::rows::call` or match on `Extern` to choose a row function; code reachable from `eml_nrt::run` read `entries()` or a descriptor table; a table or an entry carry `#[used]`. Check: `grep -rn 'rows::call\|#\[used\]' crates/eml_nrt/src` → empty (a hit inside a `#[cfg(test)]` module is allowed and named in the commit message). Readers of `entries()` and the tables this stage adds, all tests: `crates/eml_native/tests/rows.rs`, the test module of `crates/eml_nrt/src/entries.rs`, `crates/eml_native/tests/abi.rs`; `eml_native`'s `jit.rs` and `lower/mod.rs` stay the tool-side readers. From U2a; binds jit-tasks.
  - **G-C15** (native-aot) MUST: one artifact crate with independent features `tls` and `gui`, LTO inside the library, `panic = "unwind"`; `eml build` passes the linker's dead-strip option; four variants.
  - **G-C16** (native-aot) MUST: an app links exactly the variant the selection rule gives; a hello-world app has no `aws_lc|rustls|eml_x_Tls|eml_x_Gui` symbol and no `eml_x_` symbol of a row it does not call; a `gui` app links Qt 6 / AppKit dynamically.
  - **G-C17** (native-aot) MUST: each shipped variant has a notice file with the three parts, compared in CI; no notice names a crate its variant lacks.
  - **G-C18** (jit-tasks) MUST: delete `eml_sched/testing` and `GateHost`, remove `eml_vm`'s edge to `eml_io`, put the task row bodies in `eml_rows`.
  - **G-C19** MUST: G-a..G-d, U3, U4 and U6 are kind 3: no UI output, REPL transcript, `RunStats` or native snapshot changes, and `crates/eml_rt` is not edited. Check: GS lines 1, 2, 11.
- Kept in the repository: architecture.md 「crate の依存関係」 (strata, rules 1-5, the script; G-a, G-b, G-d, U3, U4), CLAUDE.md's Architecture block and one invariant bullet (G-a, G-d, U4), `nix/crate-graph.sh` and CI step `crate graph` (G-a), testing.md 「feature の組み合わせの確認」 (U3, U4), distribution.md 「AOTの出力」 / future/native.md 「AOT と配布」 / the roadmap's native-aot, jit-tasks and jit-repl items (G-a, U4; checked at 10).

#### D-3: contract amendments (user-approved, U-C and U-F; old → new)

| # | Where | Old | New |
|---|---|---|---|
| M-1 | 01 N-21, section 1.2 | a row is `Inline` with no entry, or has one entry, or is `Par` | every non-async row except `Par` has exactly one entry `eml_x_<Variant>` (232); 45 are `Inline` and their entries are not called by generated code; no other `eml_x_` symbol except `eml_x_c_*`. Row entries are the exception to "no entry ahead of a caller". Cost shown to the user: 45 uncalled shims; alternative: keep the `inline fn` mark |
| M-2 | 01 N-22 | `run_extern`'s arms call the row function; `LISTED` equals `Extern::ALL` | the generic extern instruction calls `eml_rows::rows::call`, dedicated handlers the row function; `call` has no wildcard; the `SHARED` test; `run_extern` names only `Panic` and `Exit` |
| M-3 | 00 R-1, R-4, C-1, C-3, C-4 | `as` identifiers, `inline fn` lines, the "marked set" test, `LISTED` / `Listed` re-exported | symbol = prefix + variant name; no mark; `eml_nrt` re-exports nothing of the list |
| M-4 | 01 N-7 | the only other import is a user C function | imports besides entries: a user C function `eml_c$<name>` (signature from `c_signature`) and a handle type's dtor `eml_cd$<handle type name>` (signature `(i64)`, address only); `Linkage::Import` in one function, `declare_import` |
| M-5 | 00 C-6 | jit-externs adds `c:<Module.f>` | jit-externs adds `a:mut`; jit-tasks adds `c:<Module.f>` with its first reader |
| M-6 | 00 E-7 | `Listed.entry == None` for task rows changes in jit-tasks | the partition test's async clause changes in jit-tasks |
| M-7 | 04 D-8 / A-5; 05's use | callees through `symbol_lookup_fn` over a shared map | callees and dtors registered with `JITBuilder::symbol` in `load`; jit-repl replaces the registration |
| AM-1 | 01 section 1.1 | `eml_sched` holds the list, Tls, Gui, the C pieces | `eml_rows` holds them; `eml_clink` the linker and bridges; `eml_io` the real-host constructors |
| AM-2 | 01 section 1.1 "Features" | `eml_sched` defines the three features; engines forward them | `eml_rows` defines them; nothing forwards; `eml_cli` names `eml_rows/<f>` and `eml_io/tls` |
| AM-3 | N-1 | shared engine-neutral code to `eml_sched` | task layer and run interface to `eml_sched`; row bodies and values of built-ins to `eml_rows`; OS-facing code an app needs to `eml_io`; linking of user C to `eml_clink` |
| AM-4 | N-2 | CI step `dependency edges of eml_native` | CI step `crate graph` = `nix/crate-graph.sh --check` |
| AM-5 | N-13, N-15, N-34, N-37 | "… and `bridge.rs`" / "(`bridge.rs` exempt)" | the same rules without the clause |
| AM-6 | N-7 (after M-4) | `c_signature`, which `bridge.rs` also uses | `eml_clink::c_signature`, which the bridges also use |
| AM-7 | N-22, N-23, N-30 | the list in `eml_sched`; N-30's grep over three crates | in `eml_rows`; N-23 unchanged; N-30's grep adds `crates/eml_rows crates/eml_clink` |
| AM-8 | N-41 | no other new cargo feature | adds: `tls`, `gui`, `gui-native` move from `eml_vm` to `eml_rows`; a move, not a new feature |
| AM-9 | 01 section 5.3; 05 D-7, D-8, D-13, D-21 | Tls / Gui, async bodies, submit / take in `eml_sched`; `Exec: GateHost` | in `eml_rows`, no forwarding; `GateHost` deleted when `WorkerCore` moves |
| AM-10 | 06 G-1..G-4, G-6, G-14, G-15, G-17, G-18 | paths in `eml_sched` | the same criteria with `eml_rows` (as written in D-1); G-2 adds `eml_sched` to the crates that must not invoke the list |
| AM-11 | 06 G-10, 5.1, 5.3 | `c_signature` in `eml_native`; `shared_rows` kept; features forwarded | one `fn c_signature`, in `eml_clink`; `shared_rows` deleted; no forwarding |
| AM-12 | future/native.md 「AOT と配布」, roadmap native-aot, distribution.md 「AOTの出力」 | two libraries; "what is linked is decided by the profile alone" | four variants of `eml_rtlib`; `gui` by profile, `tls` by the presence of `Std.Tls` in the module graph; one notice per variant |
| AM-12b | the same two docs | the runtime notice is the crates' licences | it also carries the Rust standard library's licences and those of the crates it vendors |
| AM-13 | vm.md 「置き場所」 | `eml_vm` re-exports the run interface, the hosts and `Control` | rule 3; `stdio_host` / `script_host` are `eml_io`'s |

No soundness criterion is dropped: N-9, N-10, N-12, N-13, U-13 stand. Kept in the repository: the spec and architecture sections named under D-1 and D-2; 00 section F (until jit-repl's Close deletes the appendix directory).

### 5.2 Short-to-mid-term design

May be adjusted during implementation for good reason; record any adjustment in the commit message.

| ID | Decision | Source |
|---|---|---|
| D-10 | The name group `pending` exists from U2a through U5i; U5j deletes it. It holds the 176 non-async rows whose body is still an arm of `run_extern` | S1-D1; G-24 |
| D-11 | During U2b..U5i `run_extern` keeps the `Panic` / `Exit` prelude and HEAD's arms of the pending rows that have a body (154 at U2b), and ends in `_ => rows::call(..)`. `call`'s name-group arms return `RunFault::Internal` with: `lir` "`Std.Par.par` is not an extern instruction"; `task` "Async externs run through Op::Async"; `engine` "Panic and Exit run in the engine"; `pending` "the row has no shared body yet". No test pins these texts [V] | S1-D2 |
| D-12 | `#[deny(unreachable_patterns)]` on `call`, so a variant listed twice does not compile | S1-D3 |
| D-13 | U5 moves the 176 rows in families of 9 + 15 + 20 + 19 + 36 + 40 + 12 + 25, in the order of the U5 table (the echo dependencies fix it) | S1-D4 |
| D-14 | `Op::UnboxedGet` / `Op::UnboxedSet` call the Int row function. The Float unboxed lines are reached only through `call` and native code; `rows::tests::unboxed_float_rows_keep_their_bits` (U5f) stands in for a VM call of them | S1-D5; X I-3 |
| D-15 | Features: `eml_rows`: `tls = ["dep:eml_tls"]` (U3), `gui = ["dep:eml_gui"]`, `gui-native = ["gui", "eml_gui/native"]` (U4). `eml_cli`: `tls = ["run", "eml_rows/tls", "eml_io/tls", "dep:eml_tls"]`, `gui = ["run", "eml_rows/gui"]`, `gui-native = ["gui", "eml_rows/gui-native"]`. Nothing forwards | S1-D6; AM-2, AM-8 |
| D-16 | `GateHost` is a trait in `eml_rows::gui`, outside `on` / `off`: `own_driver(&mut self) -> Result<(), RunFault>`, `open_gate(&mut self) -> i64`, `release_gate(&mut self, gate: i64) -> Result<(), RunFault>`, `swap_holds(&mut self, on: bool) -> bool`, `holds(&self) -> bool`, `wake(&mut self, worker: u32)`. `impl<P: Policy> GateHost for Scheduler<P>` is in `crates/eml_vm/src/sched.rs` | S1-D7 |
| D-17 | Build time: T-B-vm and T-B-nrt are measured at U1, U2b, U5f, U5j with the rule of section 7 (T-B) | S1-D9 |
| D-18 | Echo entries: U2a adds `f_f`, `ff_i`; U5e `i_f`, `if_i`; U5f `ii_f`, `iif_i`; 8b `iiii` | S1-D10; D-S2-12 |
| D-19 | Per-kind glue tests `entries::tests::x_*`: U2a `x_int_add_parks_an_overflow`, `x_str_eq_returns_an_enum`; U5e `x_float_infinity_takes_unit`; U5h `x_fail_io_parks_the_raised_failure` | S1-D11 |
| D-20 | Infallibility set, computed by the rule "`!faults` and an `obj` / `tobj` result" (20 rows): `IntShow`, `CharShow`, `CharDisplay`, `ByteDisplay`, `FloatShow`, `StrConcat`, `StrShow`, `StrPush`, `BytesEmpty`, `BytesAppend`, `BytesOfString`, `BytesPush`, `ArrayThaw`, `MutArrayEmpty`, `MutArrayFreeze`, `UnboxedArrayThaw`, `MutUnboxedArrayEmpty`, `MutUnboxedArrayFreeze`, `MutUnboxedArrayFillFloat`, `MutUnboxedArrayFillInt` | S1-D12 |
| D-21 | After U5i the Tls row helpers and `gui::arm` are `pub(crate)` in `eml_rows`; what `eml_vm`'s async path uses stays `pub`. `eml_rows::ffi::handle_new` is an `unsafe fn` whose `# Safety` is `Ctx::heap_mut`'s | S1-D13, D14 |
| D-22 | The test arm of `entries`: `@table [vis] NAME "prefix" [path] [extra] lines`; test row `IntShow`, table `TEST_ROWS`, prefix `"eml_t_"`. U2 is split into U2a (list, entries) and U2b (VM routing) | S1-D16, D17 |
| D-23 | The graph tasks G-a..G-d run as one block right after U1, so U2a..U6 are written once against the final crate names. `eml_vm::eml_gui` and `eml_vm::gui_park` leave in U4, not G-d (`eml_cli` reaches `eml_gui::{Backend, Script}` only through them). `eml_vm` keeps re-exporting exactly `Linked`, `RunConfig`, `HostRef`, `Finished`, `RuntimeError`. `eml_test_support` gains `eml_clink` (G-b), `eml_sched` (G-d), `eml_rt` (U4) and no `eml_io`. The Tls run of `eml_rows` alone is `cargo nextest run -p eml_rows --features tls` | S1-D18..D22; 07 errata F-2..F-6 |
| D-24 | Checkpoints (orchestrator, a push each): CP-A after U4 (the first push), CP-B = CI of U6's commit, CP-C = CI of 8a's commit, CP-7 after 7e, CP-Z after Close. 8b starts only after CP-C and CP-7 passed | S1-D15; D-S2-8; X Cf-5, Cf-1 (round 2) |
| D-30 | Exact counts. 7b also takes `Prim::EmptyMutArray` out of `Form::Prim`, which accepts 4 more files than the floors (`run/var/array_in_condition.em`, `run/var/exit_inside.em`, `run/var/sieve.em`, `run-fail/var/index_out_of_range.em`). After 7b: `run` 320, `run-fail` 41. After 8d: `run` 326 + F, `run-fail` 43 | D-S2-1 |
| D-31 | The unlowered list: only `ui::run_native`'s snapshot carries it, as a section `unlowered <n>` then one `  <Variant>` line per row of `Extern::ALL` with `lowering(row).is_some()` that no accepted `run/` file contains | D-S2-2 |
| D-32 | 7b's list has 84 rows; 53 are covered by UI files (7c-7e); the residue is 31: the 10 non-async `Tls*` rows, the 15 non-async `Gui*` rows, `AsyncJobCell`, `ChanToCell` (only in std functions that also hold a task row); `ShowBytesLength`, `ShowBytesGet` (only via `instance Show Bytes`, which calls `try_panic`, `std/Prelude.em:456-458`); `ProcStageCount`, `ProcWaitStage` (only via `Proc.wait_all`, `std/Proc.em:209-226`) | D-S2-3 |
| D-33 | A windows-marked accepted `run/` file is counted on every OS and its rows collected; it is not run on Windows. The assertion "an accepted file has no windows mark" holds for `run-fail/` only | D-S2-4 |
| D-34 | Floors are re-measured with procedure FL on a copy of `tests/ui` | D-S2-5 |
| D-35 | `bench/scripts.sh` gets `(native)` rows for `spectral-norm`, `mandelbrot`, `fannkuch-redux`, `sieve`; `nbody`, the 5 scripts and `https-get` stay rejected | D-S2-6 |
| D-36 | 7b lands as one commit if at most 5 files disagree on the first full native run; otherwise in stages behind the transitional list `NOT_YET_NATIVE` | D-S2-7 |
| D-37 | C codegen tests use `eml_test_support`'s fixtures; only the CLIF pin uses a local `eml_clink::Linker`. Names: `LoadError::LinkMismatch` ("the linked C functions are not the program's"); `CInt::code` (`I64` 0, `I32` 1, `SizeT` 2) and `CInt::from_code`. The name argument of `eml_x_c_narrow` is the string datum `s:<CFunction.name>`; `arg` is 1-based over all `CFunction.params`, `NoArg` included, as the VM counts. `entries!` gains the kind token `dtor` (Rust `Option<eml_rows::ffi::Dtor>`, machine `I64`, argument only). `eml_nrt/src/c.rs` calls no dispatcher; `eml_native/src` names neither `eml_rows` nor `eml_sched`; the `eml_cli` harness gains the dev-dependency `eml_extern` | D-S2-9..13 |
| D-38 | The slow TLS test is an environment matter: the dev profile is clean before U1; UT records the cause in testing.md. No test or code change | S3; U-B |
| D-39 | Doc ownership: architecture.md and CLAUDE.md change in the commit that changes the structure, and Close only checks. U4 owns future/ffi.md L30 / L125. G-a writes the roadmap items for jit-tasks and jit-repl and the 「その後の項目」 `eml_sched` item; U4 writes AM-12 / AM-12b and the native-aot item. The `eml_vm` bullet of architecture.md (L333): each task edits the sentence it makes false (G-c hosts, U3 tls, U4 gui, U5j "the match arms call `Ctx` methods" and "`vm.rs` has no cfg") | X Cf-1, Cf-2, Cf-3, Cf-6 |

Source IDs are for traceability only: `S1-D*` and `D-S2-*` are the decision numbers of the two task designs folded into this plan, `S3` is the investigation of the TLS test, `X` the cross-check resolutions (00 section F.4).

## 6. Implementation constraints and conventions

- One task is one commit (7b may be 7b1..7b6 by D-36; a fix after a checkpoint is its own reviewed commit). Work test-first. A commit is made only after its completion criteria hold.
- Code comments, `docs/` and CLAUDE.md edits are written in Japanese (CLAUDE.md): draft in English, translate with PLaMo (`form: plain`, identifiers and section names in `glossary`, earlier parts of the same document as `history`), review the result, and put no space between Japanese and Latin characters or inline code. Comments say why, not what.
- Code and `docs/` never cite `docs/plans/`, this plan's IDs (D-x, G-x, task IDs), the appendices or `docs/future/` (N-30). Code comments cite `docs/spec/` or `docs/implementation/` sections. A cited heading that does not exist fails `citations::every_cited_heading_exists`.
- The spec section is written in the same commit that implements its rule. Docs describe the current state: no stage names, dates or "for now … later" text.
- A new test file is declared in its crate's `tests/main.rs`. A new source file of `eml_native`, `eml_nrt`, `eml_clink` or `eml_io` goes into that crate's `trust.rs` `FILES`.
- Every test of the ordinary run finishes within 1 s alone; no outcome depends on machine speed.
- Never bend the design to keep a test unchanged. A test change not listed below is not agreed: stop and report.
- Every task that changes a workspace edge, a `[features]` table or a cross-crate `pub use eml_…` edits the tables of `nix/crate-graph.sh` in the same commit (G-C2).
- A new external dependency or a `Cargo.lock` change that alters the licence list: regenerate with `nix develop -c nix/third-party-licenses.sh` in the same commit.
- Run ad-hoc `.em` programs under `timeout 20`. Stop every background task before finishing.
- Shell heredocs whose text contains backticks use a quoted delimiter (`<<'EOF'`).
- Accept snapshots with `cargo insta accept`, never `cargo insta review` (interactive).
- A pushed commit is never amended or force-pushed.
- No special case, duplicated logic or optimisation that needs a fragile invariant (N-38). Deliberately forgone here: `plain` wrappers per row; inlining a handle's C pointer; dropping `unit` parameters from the entry ABI; a libm `fmod` import; inline payload offsets for C pointer arguments.

### Test changes (docs/implementation/testing.md 「テストの変更の運用」)

**Kind 1 (pass / fail), test by test:**

| Task | Test | Change | Reason |
|---|---|---|---|
| U2a | `eml_rows` `rows::tests::the_listed_rows_are_the_seed_and_the_inline_rows` | deleted | `LISTED` and the sections no longer exist |
| U2a | `eml_rows` `rows::tests::a_names_row_has_no_entry` | deleted | `names!` and `Listed` no longer exist |
| U2a | `eml_nrt` `entries::tests::every_row_entry_is_named_after_its_variant` | rewritten over `SHARED` | the `as` grammar is gone |
| U2a | `eml_nrt` `rows::fatal::a_row_error_of_the_internal_class_is_recorded` | rewritten over the `@table` arm; assertions unchanged | the `as` grammar and the `@lines` muncher are gone |
| U2a | `eml_native` `unsupported::the_inline_table_is_the_marked_set` | deleted | no mark exists (M-1, M-3) |
| 7b | `crates/eml_cli/tests/ui.rs` `native_entries`: the assertion "an accepted file has no windows mark" | holds for `run-fail/` only (D-33) | acceptance and rows are OS-independent; `run/std/proc_relative_program/main.em` is windows-marked and becomes accepted |
| 7b | `unsupported::std_row_is_rejected` | deleted | `Form::StdRow` is gone |
| 7b | `unsupported::the_called_rows_are_the_seed` | deleted | `CALLED` is gone |
| 7b | `unsupported::forms_are_sorted_and_distinct` | rewritten without `Float.truncate`: asserts `Par`, `Prim`, `Prompt`, sorted | std rows are accepted |
| 7b | `unsupported::the_first_function_and_row_are_reported` | rewritten on task rows (`Form::AsyncRow`, `row: Some(<eml name>)`) | std rows are accepted |
| 7b | `unsupported::inline_and_called_rows_are_supported` | rewritten by rule (every non-async row except `Par` is `Inline` or `Called`; async rows and `Par` are `None`; a `Float.truncate` program has no rejection) | lowering is by rule |
| 7b | `unsupported::every_called_row_has_an_entry` | replaced by `rows::the_rows_are_partitioned` | M-1 |
| 8d | `unsupported::c_row_is_rejected` | deleted | `Form::CRow` is gone |

`-- native: skip` marks: none expected. A mark is allowed only for a difference N-43 lists (its item 1), through a plan revision that names the file; any other difference stops the task and goes to the user.

**Kind 2 (expected values), scope:** the two native snapshots `integration__ui__run_native.snap` and `integration__ui__run_fail_native.snap` in 7b (or 7b1..7b6), `run_native` in each of 7c-7e, both in 8d: the counts, the histogram keys (`std row` leaves in 7b, `c row` in 8d) and the `unlowered` section. Nothing else.

**Kind 3 (mechanical, byte-identical expected values; the commit message is the record):**
- G-a: `eml_sched/tests/constants.rs` and `rows::tests` move to `eml_rows`; the placement test is renamed and strengthened; `placement::code_cites_only_spec_and_implementation` scans `eml_rows` too.
- G-b: 25 integration tests move to `eml_clink`; four placement tests are renamed and lose `bridge.rs`; the `trust` tables change; the citation placement test scans `eml_clink` too.
- G-c: `spawns.rs` `EXPECTED` paths.
- G-d: imports in `eml_vm/tests`, `eml_native/tests`, `eml_cli`, `eml_test_support`.
- U2a: the `x_*` tests use the renamed Rust items; `eml_native/tests/rows.rs` uses `eml_rows::rows::call`.
- U3: 11 tls unit tests and `tls_off` move to `eml_rows`. U4: 2 gui unit tests, 18 integration `gui::` tests and `gui_off` move to `eml_rows`; `RecordingHost` moves to `eml_test_support`.
- 8d: `crates/eml_native/tests/common/native.rs` links the program.
- Every VM test, REPL transcript and `bench.rs` snapshot stays byte-identical throughout.

**New tests:** `eml_clink` `trust::` (3); the tests the cards of U2a, U5e, U5f, U5h, U5j, U6, 8a, 7a, 7b, 8b, 8c, 8d name; the UI files of 7c-7e; one regression test per native disagreement fixed in 7b.

## 7. Implementation tasks

### 7.1 Shared commands

**G (gate; from the repository root, after `git add -A`):**
1. `cargo nextest run` → exit 0; the last line reads `Summary [ <t>s] <N> tests run: <N> passed`, optionally ` (<k> slow)`, then `, <M> skipped`, with no `failed`
2. `cargo clippy --all-targets -- -D warnings` → exit 0
3. `cargo fmt --check` → exit 0
4. from G-a on: `bash nix/crate-graph.sh --check` → exit 0

**GS (the graph and VM tasks' gate).** These lines hold in every task from G-a to U6, unless a line names other tasks.
```sh
# 1  native snapshots unchanged (208 / 8)
git diff --exit-code fddd44f2 -- crates/eml_cli/tests/snapshots/integration__ui__run_native.snap crates/eml_cli/tests/snapshots/integration__ui__run_fail_native.snap   # exit 0
# 2  RunStats unchanged; no pending snapshot (G-C19)
cargo nextest run -p eml_vm --test integration bench::                                   # 0 failed
git status --porcelain | grep -c 'snap'                                                   # 0
# 3  tls / gui off: 3 tests run: 3 passed
cargo nextest run -p eml_io -p eml_vm -E 'test(tls_off::) | test(gui_off::)'             # G-a..U2b
cargo nextest run -p eml_io -p eml_vm -p eml_rows -E 'test(tls_off::) | test(gui_off::)' # U3
cargo nextest run -p eml_io -p eml_rows -E 'test(tls_off::) | test(gui_off::)'           # U4 on
# 4-8  feature combos
cargo clippy -p eml_cli --no-default-features --features run -- -D warnings              # exit 0
cargo clippy -p eml_cli --no-default-features --features run,gui -- -D warnings          # exit 0
cargo clippy -p eml_io -p eml_rows -p eml_vm -p eml_nrt --all-targets -- -D warnings     # exit 0
cargo clippy -p eml_sched --all-targets -- -D warnings                                    # exit 0
cargo clippy -p eml_native --all-targets -- -D warnings                                   # exit 0
cargo clippy -p eml_clink --all-targets -- -D warnings                                    # exit 0, from G-b
# 9  Windows cross clippy
nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run -- -D warnings   # exit 0
# 10 graph script (holds the N-41 grep)
bash nix/crate-graph.sh --check                                                           # exit 0
# 11 G-22 / G-C19
git diff --stat fddd44f2 -- crates/eml_rt                                                 # empty
# 12 from U3
cargo clippy -p eml_rows --all-targets --features tls -- -D warnings                      # exit 0
# 13 from U3: the lines of CI's step "feature resolution of tls", and G-C13b
test "$(cargo tree --workspace -e features -i eml_rows | grep -c 'eml_rows feature "tls"')" -ge 1 && echo ok   # ok
test "$(cargo tree -p eml_rows -p eml_io -p eml_vm -p eml_nrt -e normal,dev,build --target all | grep -c -E 'eml_tls|aws-lc|ring v')" -eq 0 && echo ok   # ok
test "$(cargo tree -p eml_io -p eml_rows -e features -i eml_rows | grep -c -E 'eml_rows feature "(tls|gui)"')" -eq 0 && echo ok   # ok (G-C7c)
cargo build -p eml_nrt --features eml_rows/tls                                            # exit 0 (G-C13b)
# 14 U3 only: the moved tls tests run in the ordinary run
cargo nextest run --workspace -E 'package(eml_rows) & test(/^tls::/)'                    # 11 passed
# 15 from U4
cargo clippy -p eml_rows --all-targets --features gui -- -D warnings                      # exit 0
cargo clippy -p eml_rows --all-targets --features tls,gui -- -D warnings                  # exit 0
cargo clippy -p eml_rows --all-targets --features gui-native -- -D warnings               # exit 0 (macOS: AppKit)
nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run,eml_cli/gui-native -- -D warnings   # exit 0
# 16 from U4 (replaces 14): 13 lib tests (11 tls, 2 gui) + 18 integration gui tests
cargo nextest run --workspace -E 'package(eml_rows) & test(/^(tls|gui)::/)'              # 31 passed
# 17 from G-a: G-C13, the cross half
for t in x86_64-pc-windows-msvc x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu; do nix develop .#cross -c cargo clippy --target $t -p eml_nrt -- -D warnings || echo FAIL $t; done   # no FAIL
```

**G-grep.** Each block holds from the task named until Close, unless it says otherwise. A trailing `| grep -vE '^[^:]+:[0-9]+:\s*//'` drops comment lines.
```sh
# always (the placement lines of the earlier native stages; values at fddd44f2 in brackets where G-b changes them)
grep -rn 'thread_local' crates/eml_nrt/src crates/eml_native/src                          # empty
grep -rnw 'Checked' crates/eml_nrt/src crates/eml_native/src                              # empty
grep -rnE 'CTX_[A-Z_]+\s*(:|=)' crates/eml_native/src crates/eml_nrt/src                  # empty
grep -rn 'Linkage::Import' crates/*/src | cut -d: -f1 | sort -u                           # crates/eml_native/src/abi.rs
grep -rn 'CallConv::Tail' crates/eml_native/src | wc -l                                   # 1
grep -rlnw 'eml_rt' crates/eml_native/src | sort                                          # crates/eml_native/src/mem.rs
grep -rn 'eml_sched' crates/eml_native/src crates/eml_native/Cargo.toml                    # empty
grep -rnE 'eml_rt::(Ctx|Heap)|Ctx<' crates/eml_native/src                                 # empty
grep -rn 'eml_fatal(' crates/eml_nrt/src | grep -v -e src/entries.rs -e src/enter.rs      # empty
grep -rn 'set_stack_limit(' crates/eml_nrt/src | wc -l                                    # 1
grep -rn 'preserve_frame_pointers' crates/eml_native/src | wc -l                          # 1
grep -rn 'symbol_lookup_fn' crates/eml_native/src                                         # empty
grep -rn '"c:\|"h:' crates/eml_native/src                                                 # empty (if Q-2 fired: only "h:")
grep -rn 'native: skip' tests/ui | wc -l                                                  # 8
grep -rn 'docs/future\|docs/plans' crates/eml_nrt crates/eml_native crates/eml_sched      # empty
# G-a on
grep -rn 'eml_sched::rows\|eml_sched::constants\|eml_sched::extern_rows' crates           # empty
grep -rn 'docs/future\|docs/plans' crates/eml_rows                                        # empty (N-30, AM-7)
# G-b on (G-C8) [before G-b each file list also has crates/eml_native/src/bridge.rs]
grep -c bridge crates/eml_native/tests/placement.rs                                       # 0
grep -n libloading crates/eml_native/Cargo.toml                                           # empty
grep -rn 'docs/future\|docs/plans' crates/eml_clink                                       # empty
grep -rln 'JITModule' crates/eml_native/src | sort                                        # crates/eml_native/src/jit.rs
grep -rln 'settings::builder' crates/eml_native/src | sort                                # crates/eml_native/src/abi.rs
grep -rln 'call_indirect' crates/eml_native/src | sort                                    # crates/eml_native/src/lower/apply.rs, crates/eml_native/src/thunk.rs
grep -rnE 'ins\(\)\.(load|store|uload|sload)' crates/eml_native/src | grep -v -e src/mem.rs -e src/thunk.rs   # empty
# G-c on (G-C9)
grep -rn 'fn stdio_host\|fn script_host' crates/*/src | cut -d: -f1 | sort -u             # crates/eml_io/src/hosts.rs
grep -rn 'CaptureHost\|RecordingHost' crates/{eml_rt,eml_io,eml_sched,eml_rows,eml_nrt,eml_cabi,eml_extern}/src | grep -vE '^[^:]+:[0-9]+:\s*//'   # empty
# G-d on (G-C4, G-C6)
grep -rn 'shared_rows' crates                                                              # empty
grep -rnE '^\s*pub use (cells|doorbell|driver|error|park)::' crates/eml_vm/src            # empty (also in the script)
grep -rnw 'eml_rows\|eml_sched' crates/eml_native/src                                     # empty
# U2a on (G-1, G-2, G-3, G-5, G-6, G-C14)
grep -rn 'LISTED\|vm_arms\| as eml_x_\|inline fn\|section_of!\|names!' crates/*/src         # empty
grep -n ' as eml_x_\|inline fn\|generic {\|dedicated {' crates/eml_rows/src/rows.rs       # empty
grep -rln 'extern_rows!(' crates/*/src | sort                                             # crates/eml_nrt/src/rows.rs, crates/eml_rows/src/rows.rs
sed -n '/^macro_rules! defs/,/^}/p' crates/eml_rows/src/rows.rs | grep -c '_ =>'          # 0
grep -rn 'fn eml_x_' crates/eml_rows/src crates/eml_nrt/src | cut -d: -f1 | sort -u       # empty; from 8b: crates/eml_nrt/src/c.rs
grep -rn 'rows::call\|#\[used\]' crates/eml_nrt/src                                      # empty (a hit inside a #[cfg(test)] module is allowed, G-C14)
grep -n 'eml_fatal' crates/eml_nrt/src/rows.rs                                            # empty (G-9)
# U3 on
test ! -e crates/eml_vm/src/tls.rs && echo gone                                          # gone
grep -rnE 'Scheduler<|Policy>|crate::sched' crates/eml_rows/src                           # empty
# U4 on (G-C7)
test ! -e crates/eml_vm/src/gui.rs && echo gone                                          # gone
grep -n 'eml_tls\|eml_gui\|^\[features\]' crates/eml_vm/Cargo.toml                       # empty
grep -rn 'feature = "' crates/eml_vm/src crates/eml_nrt/src crates/eml_native/src         # empty (G-C7a)
grep -rn 'eml_vm\|eml_native\|eml_test_support' crates/eml_rows/src crates/eml_sched/src crates/eml_nrt/src crates/eml_io/src | grep -vE '^[^:]+:[0-9]+:\s*//'   # empty (G-C7b)
grep -rnE 'eml_vm.{0,6}gui\.rs|eml_vm.{0,6}tls\.rs' docs/spec docs/implementation docs/future CLAUDE.md   # empty
# U5a on
sed -n '/Op::IntAdd { dst, a, b }/,/Op::SwitchEnum/p' crates/eml_vm/src/vm.rs | grep -c 'externs::'   # 0  (the pattern names the handler's fields so it does not also match step_failed's `Op::IntAdd { dst, .. }`)
# U5f on
grep -nE 'ctx\.(array_get|array_length|array_set|unboxed_get|unboxed_length|unboxed_set|string_byte_at|string_byte_length|bytes_get|bytes_length)\(' crates/eml_vm/src/vm.rs   # empty
# U5j on (G-4, G-24)
awk '/^fn run_extern/,/^}/' crates/eml_vm/src/vm.rs | grep -o 'Extern::[A-Za-z]*' | sort -u   # exactly: Extern::ALL, Extern::Exit, Extern::Panic
grep -nE 'externs::[a-z_]+\(' crates/eml_vm/src/vm.rs                                     # empty
grep -n 'pending' crates/eml_rows/src/rows.rs crates/eml_nrt/src/rows.rs                 # empty
# U6 on (G-14, G-15, G-17, G-18)
grep -rnwE 'struct (CHandle|HandleCell)' crates/*/src | cut -d: -f1 | sort -u             # crates/eml_rows/src/ffi.rs
grep -rln 'does not fit in C\|returned a null handle' crates/*/src                        # crates/eml_rows/src/ffi.rs
grep -rln 'alloc_array_immortal' crates/*/src | grep -v '^crates/eml_rt/'                 # crates/eml_rows/src/constants.rs
grep -n 'alloc_foreign' crates/eml_vm/src/ffi.rs                                          # empty
git diff --exit-code fddd44f2 -- crates/eml_vm/src/compile/mod.rs                         # exit 0 (G-17)
diff <(git show fddd44f2:crates/eml_extern/src/c.rs | sed -n '/pub enum CParam/,/^}/p') <(sed -n '/pub enum CParam/,/^}/p' crates/eml_extern/src/c.rs)   # empty (G-16)
sed -n '/pub enum CParam/,/^}/p' crates/eml_extern/src/c.rs | grep -c '^    [A-Z]'         # 7 (G-16)
# G-a..U6 only (8b edits eml_extern)
git diff --exit-code fddd44f2 -- crates/eml_extern                                        # exit 0
# from 7b's last commit (G-5, G-8, G-24)
grep -rn 'pub const CALLED\|StdRow\|NOT_YET_NATIVE' crates                                # empty
# 8b on (G-9, G-14)
grep -n 'eml_fatal' crates/eml_nrt/src/c.rs                                               # empty
grep -n 'transmute\| as \*' crates/eml_nrt/src/c.rs                                       # empty
grep -rn 'alloc_foreign' crates/eml_vm/src/ffi.rs crates/eml_nrt/src/c.rs                 # empty
# 8c on (G-10)
grep -rn 'fn c_signature' crates/*/src | cut -d: -f1                                      # one line, under crates/eml_clink/src
# 8d on (G-8)
grep -rn 'CRow' crates                                                                     # empty
# whole stage (G-22)
git diff --stat fddd44f2 -- crates/eml_rt                                                 # empty
```

**Script tables.** The direct-edge table at fddd44f2 [V]:
```
eml_cabi: eml_extern
eml_cli: eml_core_ir eml_diagnostics eml_hir eml_lir eml_native eml_syntax eml_tls eml_types eml_vm
eml_core_ir: eml_diagnostics eml_extern eml_hir eml_lir eml_types
eml_diagnostics:
eml_extern:
eml_gui:
eml_hir: eml_diagnostics eml_extern eml_syntax
eml_io: eml_rt eml_tls
eml_lir: eml_extern
eml_native: eml_cabi eml_extern eml_lir eml_nrt eml_rt
eml_nrt: eml_extern eml_rt eml_sched
eml_rt:
eml_sched: eml_extern eml_io eml_rt
eml_syntax: eml_diagnostics
eml_test_support: eml_cli eml_core_ir eml_diagnostics eml_hir eml_lir eml_native eml_nrt eml_syntax eml_types eml_vm
eml_tls:
eml_types: eml_diagnostics eml_extern eml_hir
eml_vm: eml_cabi eml_extern eml_gui eml_io eml_lir eml_rt eml_sched eml_tls
```
Deltas, each written in the task's commit:

| Task | Edges | Features | `REEXPORTS` (from G-d) |
|---|---|---|---|
| G-a | new `eml_rows: eml_extern eml_rt eml_sched`; `eml_sched` − extern; `eml_nrt` + rows; `eml_vm` + rows | none | - |
| G-b | new `eml_clink: eml_cabi eml_extern`; `eml_cli` + clink; `eml_test_support` + clink | `eml_cli` `run` + `"dep:eml_clink"`; `eml_test_support` `run` + `"dep:eml_clink"` | - |
| G-c | `eml_cli` + io | `eml_cli` `run` + `"dep:eml_io"` | - |
| G-d | `eml_cli` + sched; `eml_test_support` + sched | `eml_cli` `run` + `"dep:eml_sched"`; `eml_test_support` `run` + `"dep:eml_sched"` | the list is created (below) |
| U3 | `eml_rows` + tls; `eml_vm` − tls; `eml_cli` + rows | `eml_rows`: `tls = ["dep:eml_tls"]`; `eml_vm` − `tls`; `eml_cli` `tls = ["run", "eml_rows/tls", "eml_io/tls", "dep:eml_tls"]`, `run` + `"dep:eml_rows"` | - |
| U4 | `eml_rows` + gui; `eml_vm` − gui; `eml_test_support` + rt | `eml_rows`: `gui = ["dep:eml_gui"]`, `gui-native = ["gui", "eml_gui/native"]`; `eml_vm` has no `[features]`; `eml_cli` `gui = ["run", "eml_rows/gui"]`, `gui-native = ["gui", "eml_rows/gui-native"]`; `eml_test_support` `run` + `"dep:eml_rt"` | − `crates/eml_vm/src/lib.rs: pub use eml_gui;` + `crates/eml_rows/src/gui.rs: pub use eml_gui::{Backend, Script};` |
| U6 | `eml_rows` + cabi | none | + `crates/eml_rows/src/ffi.rs: pub use eml_cabi::Dtor;` |
| 8c | `eml_native` + clink | none | - |

Rows that differ from the base table after 8c (every other row as at base):
```
eml_cli: eml_clink eml_core_ir eml_diagnostics eml_hir eml_io eml_lir eml_native eml_rows eml_sched eml_syntax eml_tls eml_types eml_vm
eml_clink: eml_cabi eml_extern
eml_native: eml_cabi eml_clink eml_extern eml_lir eml_nrt eml_rt
eml_nrt: eml_extern eml_rows eml_rt eml_sched
eml_rows: eml_cabi eml_extern eml_gui eml_rt eml_sched eml_tls
eml_sched: eml_io eml_rt
eml_test_support: eml_cli eml_clink eml_core_ir eml_diagnostics eml_hir eml_lir eml_native eml_nrt eml_rt eml_sched eml_syntax eml_types eml_vm
eml_vm: eml_cabi eml_extern eml_io eml_lir eml_rows eml_rt eml_sched
```
The `REEXPORTS` list as G-d creates it (`pub use eml_` statements in `crates/*/src` outside `eml_cli` and `eml_test_support`, line numbers stripped):
```
crates/eml_clink/src/lib.rs: pub use eml_cabi::Linked;
crates/eml_lir/src/lib.rs: pub use eml_extern::{FloatBits, Mark, OpRet, Repr, StructId};
crates/eml_native/src/lib.rs: pub use eml_nrt::image::Options;
crates/eml_nrt/src/lib.rs: pub use eml_rt as __rt;
crates/eml_nrt/src/lib.rs: pub use eml_rt::HostRef;
crates/eml_nrt/src/lib.rs: pub use eml_sched::config::{Finished, RunConfig};
crates/eml_nrt/src/lib.rs: pub use eml_sched::error::{RunFault, RuntimeError};
crates/eml_sched/src/lib.rs: pub use eml_rt::{Control, HostRef, Limits, Stop};
crates/eml_vm/src/lib.rs: pub use eml_cabi::Linked;
crates/eml_vm/src/lib.rs: pub use eml_gui;
crates/eml_vm/src/lib.rs: pub use eml_rt::HostRef;
crates/eml_vm/src/lib.rs: pub use eml_sched::config::{Finished, RunConfig};
crates/eml_vm/src/lib.rs: pub use eml_sched::error::RuntimeError;
```
The implementer pastes the script's no-argument output; if the pasted `eml_lir`, `eml_native` or `eml_nrt` lines differ from this list only in items that existed at fddd44f2, the pasted text wins and the commit message says so.

**T-B (build time).** On a built tree with no other build running. Each number is the median of three `real` values; the values go into the commit message (U1: into section 9).

| When | T-B-vm: clean | T-B-vm: build | T-B-nrt: clean | T-B-nrt: build |
|---|---|---|---|---|
| U1 (base) | `eml_sched eml_vm` | `-p eml_vm --features eml_vm/tls,eml_vm/gui` | `eml_sched eml_nrt` | `-p eml_nrt` |
| U2b | `eml_sched eml_rows eml_vm` | `-p eml_vm --features eml_vm/tls,eml_vm/gui` | `eml_sched eml_rows eml_nrt` | `-p eml_nrt` |
| U5f, U5j | `eml_sched eml_rows eml_vm` | `-p eml_vm --features eml_rows/tls,eml_rows/gui` | `eml_sched eml_rows eml_nrt` | `-p eml_nrt` |

```sh
cargo build <build args>        # warm the dependencies once
for i in 1 2 3; do cargo clean <clean as -p list> >/dev/null 2>&1; /usr/bin/time -p cargo build <build args> 2>&1 | grep '^real'; done
```
Rule: if T-B-vm / base > 1.20 at U5f or U5j, that commit puts `#[inline(never)]` on `call`, removes `#[inline]` from every line whose body is more than one `Ctx` / `externs` call (the dedicated ops' rows keep it), and re-measures. Still above 1.20: stop and report with both numbers. If T-B-nrt / base > 1.20 at U5j: report to the user before U6 (no remedy; the uncalled entries are M-1's approved cost).

**P (pending check, U2b..U5i).** U2a fixes the `pending` block format: `pending {` alone on its line; one variant per line with a trailing comma; no comments inside; `}` alone on its line.
```sh
diff <(awk '/^fn run_extern/,/^}/' crates/eml_vm/src/vm.rs | grep -o 'Extern::[A-Za-z]*' | sed 's/^Extern:://' | grep -vx -e ALL -e Panic -e Exit | sort -u) \
     <(awk '/^ *pending \{$/{f=1;next} f&&/^ *\}/{f=0} f' crates/eml_rows/src/rows.rs | tr -d ' ,' | sort -u)
```
Expected: U2b..U5e exactly 22 lines `> <Row>` (U5f's container rows) and no `<` line; U5f..U5i empty.

**Snapshot readers** (at fddd44f2 `counts run_native` prints `accepted 208/skipped 6/rejected 422/  async row 129/  c row 2/  par 22/  prim 156/  std row 113` [V]):
```sh
S=crates/eml_cli/tests/snapshots
counts() { awk '/^---$/{n++; next} n>=2 && /^unlowered/{exit} n>=2' "$S/integration__ui__$1.snap" | paste -sd/ -; }
rows()   { awk '/^unlowered/{f=1; next} f {print $1}' "$S/integration__ui__run_native.snap" | paste -sd' ' -; }
```

**T (slowest native UI file):**
```sh
RUST_TEST_THREADS=1 EML_UI_TIMES=1 cargo nextest run -p eml_cli --test integration -E 'test(=ui::run_native) | test(=ui::run_fail_native)' --no-capture 2>&1 | sort -n | tail -1
```
Pass: the first field is below `1000`.

**B (budget of `ui::run_native`):**
```sh
cargo nextest run -p eml_cli --test integration -E 'test(=ui::run_native)' -j 1
```
Pass: `PASS [ <t>s] (1/1) eml_cli::integration ui::run_native` with `<t>` below 5.000.

**K (snapshots, from 7b on).** After `cargo insta accept`:
- `find . -path ./target -prune -o \( -name '*.snap.new' -o -name '*.pending-snap' \) -print` → prints nothing.
- `git status --porcelain crates/eml_cli/tests/snapshots | grep -v '^??'` → only ` M` on `integration__ui__run_native.snap` and `integration__ui__run_fail_native.snap`.
- `??` appears only for the new UI files' `integration__ui__run@…` snapshots.
- Any other ` M` is a kind-3 violation (G-23): revert it and find the cause.

**FL (floor measurement; U1 and the first step of 7b).** `cargo build -p eml_cli`, save the script as `$TMPDIR/forms.sh` (not committed), run `bash $TMPDIR/forms.sh "$PWD" "$TMPDIR/forms"`.
```bash
#!/usr/bin/env bash
# usage: forms.sh REPO OUTDIR ; OUTDIR/run.txt, OUTDIR/run-fail.txt: one "<forms>|<entry>" line per entry
set -euo pipefail
export LC_ALL=C
repo="$1"; out="$2"
eml="$repo/target/debug/eml"
rm -rf "$out/ui"; mkdir -p "$out"; cp -R "$repo/tests/ui" "$out/ui"
for top in run run-fail; do
  { find "$out/ui/$top" -mindepth 2 -maxdepth 2 -name '*.em'; find "$out/ui/$top" -mindepth 3 -maxdepth 3 -name main.em; } | sort |
  while read -r f; do
    rel="${f#"$out/ui/"}"
    if head -n 5 "$f" | grep -q '^-- native: skip ('; then echo "skip|$rel"; continue; fi
    forms=$( (cd "$(dirname "$f")" && timeout 20 "$eml" run --native "$(basename "$f")" < /dev/null 2>&1 > /dev/null || true) |
      { grep -o 'native code does not support .* yet' || true; } |
      sed -e 's/^native code does not support //' -e 's/ yet$//' -e 's/^the std row .*/std row/' -e 's/^the task row .*/async row/' |
      sort -u | paste -sd, - )
    echo "${forms:-none}|$rel"
  done > "$out/$top.txt"
done
for top in run run-fail; do
  echo "$top: entries $(wc -l < "$out/$top.txt") none $(grep -c '^none|' "$out/$top.txt") only-std $(grep -c '^std row|' "$out/$top.txt") std-or-c $(grep -cE '^(std row|C functions|C functions,std row)\|' "$out/$top.txt") skip $(grep -c '^skip|' "$out/$top.txt")"
done
```
Expected at fddd44f2 and through U6 [V at fddd44f2]:
```
run: entries 636 none 208 only-std 109 std-or-c 115 skip 6
run-fail: entries 81 none 8 only-std 32 std-or-c 34 skip 2
```
The 4 `a:mut` files: `grep -E 'var/(array_in_condition|exit_inside|sieve|index_out_of_range)' $TMPDIR/forms/*.txt` → 4 lines with forms `effect operations,std row` or `effect operations`.
If a number differs: list `git diff --name-only --diff-filter=A fddd44f2 HEAD -- tests/ui` and the added marks; if they explain the difference, correct the counts of 7b (and S-2) in 7b's commit; otherwise stop and report.

**CP (CI checkpoint; the orchestrator's procedure, not an implementer task; makes no commit).** `<branch>` = `plan/jit-externs`.
1. Ask the user's permission for the push, then (one CI run takes about 18 minutes):
   ```
   git push -u origin plan/jit-externs
   gh workflow run CI --ref plan/jit-externs
   sha=$(git rev-parse HEAD)
   gh run list --workflow CI --branch plan/jit-externs --json databaseId,headSha,status --jq ".[] | select(.headSha == \"$sha\") | .databaseId"
   gh run view <databaseId> --json status,conclusion
   ```
   While `gh run list` prints nothing, wait and repeat (select by the pushed commit's `headSha`). Poll `gh run view` until `status` is `completed`, or run `gh run watch <databaseId> --exit-status` as a background task. Pass: the run's `headSha` is the checkpoint's commit and `conclusion` is `success`. That includes the four targets (ubuntu-24.04, ubuntu-24.04-arm, macos-15, windows-2025), `cargo nextest run --profile timing -j 1`, and from the task that adds each: the step `crate graph` (also under Git Bash on Windows), `feature resolution of tls` with the G-C7c guard, `cargo build -p eml_nrt --features eml_rows/tls` on every OS, the `gui-native` job, the ASan job with `eml_clink`, Miri and TSan.
2. On failure: read `gh run view <databaseId> --log-failed`. Each fix is its own reviewed task and its own new commit (never amend or force-push). Repeat step 1, asking permission before each push. If the cause is outside this stage's code, stop and return to the user.

| Checkpoint | Commit | Settles |
|---|---|---|
| CP-A | U4 (first push: U1..U4) | the graph tasks and the feature move on four targets |
| CP-B | U6 | the list and the VM routing under Miri, ASan, TSan |
| CP-C | 8a | A-3 and A-4 (Q-2, Q-3) before any C lowering |
| CP-7 | 7e | the ~150 newly native files on four OSes |
| CP-Z | 10 (Close) | the merge gate; after it passes the branch is merged |

### 7.2 Task table

No task runs in parallel with another. "After" is the task that must be committed first.

| ID | Task | Based on | After |
|---|---|---|---|
| U1 | `Start jit-externs`: branch, baseline, plan and appendices | D-3, D-38 | - |
| UT | the testing.md paragraph on the macOS verifier cost | D-38 | U1 |
| G-a | `eml_rows` and `nix/crate-graph.sh` | D-2 (G-C1..G-C4, G-C13), AM-3, AM-4, AM-7, AM-10 | UT |
| G-b | `eml_clink` | D-2 (G-C8, G-C10), AM-5, AM-6 | G-a |
| G-c | the real hosts move to `eml_io` | D-2 (G-C9) | G-b |
| G-d | re-exports and direct edges | D-2 (G-C6), AM-13, D-23 | G-c |
| U2a | list reshape and entries on the 54 rows | D-1 (G-1..G-3, G-5, G-6, G-9), D-10, D-12, D-18, D-19, D-22; G-C14 | G-d |
| U2b | the VM routes through `rows::call` | D-1 (G-3), D-11 | U2a |
| U3 | move `Std.Tls` to `eml_rows` | D-15; G-C12, G-C13b, G-C7c | U2b |
| U4 | move `Std.Gui` with `GateHost` | D-15, D-16, D-23, D-39; G-C5, G-C7, G-C10, G-C11; AM-12, AM-12b | U3 |
| CP-A | CI of U4's commit | D-24 | U4 |
| U5a..U5j | all bodies into the list (10 commits) | D-1 (G-3, G-4, G-24), D-11, D-13, D-14, D-17, D-20, D-21 | CP-A |
| U6 | shared C pieces and `empty_mut_array` | D-1 (G-14..G-18), D-21 | U5j |
| CP-B | CI of U6's commit | D-24 | U6 |
| 8a | import test on every target | D-1 (G-10, G-11); Q-2, Q-3 | CP-B |
| CP-C | CI of 8a's commit | D-24 | 8a |
| 7a | the empty `MutArray` datum `a:mut` | D-1 (G-18), M-5 | 8a (CP-C may run meanwhile) |
| 7b | lower every non-async std row (1 commit, or 7b1..7b6) | D-1 (G-7, G-8, G-19, G-20), D-30, D-31, D-33, D-34, D-36; M-1, M-6 | 7a |
| 7c, 7d, 7e | UI files for the 53 rows | D-32; G-19 | 7b |
| CP-7 | CI of 7e's commit | D-24 | 7e |
| 8b | C helper and pointer entries, `dtor` token, echo | D-1 (G-6, G-9, G-13, G-14), D-37; G-C14 | CP-7 and CP-C |
| 8c | `eml_clink::c_signature`, one import declarer | D-1 (G-10), M-4, AM-6; G-C2 | 8b |
| 8d | call user C functions natively | D-1 (G-8, G-10..G-16), D-37, M-7 | 8c |
| 9 | native rows in `bench/scripts.sh` | D-35; N-39 | 8d |
| 10 | `Close jit-externs` | all | 9 |
| CP-Z | CI of the Close commit, then merge | D-24 | 10 |

Order note. The two unit designs disagreed on where 8a's checkpoint sits (one let 8a ride CP-B). Resolved: CP-B is the CI of U6's commit only and 8a has its own push, CP-C, because CP-B must show the list work green before a test whose failure triggers a plan revision (Q-2, Q-3) lands. 8a precedes 7a so that CP-C's answer is known before 8b.

Every card's completion includes G, plus the GS lines in force (G-a..U6), the G-grep lines in force, and K (from 7b on). G-a..G-d, U2b, U3, U4, U5a..U5j, U6, 8c are kind 3.

### U1: `Start jit-externs`

- Purpose: the stage's first commit (roadmap 「進め方」): this plan replaces the design-level file, the appendix directory gains 06 and 07, and the baseline is recorded.
- Targets: `docs/plans/2026-11-11-jit-externs.md`; `docs/plans/2026-11-11-jit/{00-decisions.md, 04-jit-externs.md, 06-extern-boundary.md, 07-crate-graph.md}`. Appendices 01 and 05 are not edited: 00 section F overrides them.
- Work:
  1. `git switch -c plan/jit-externs fddd44f2`. `git diff --stat fddd44f2 main` → empty (if `main` moved, stop and report).
  2. The dev profile is already clean. `cargo build --all-targets` → exit 0. `ls -f target/debug/deps | wc -l` → well under 200,000.
  3. The TLS test (D-38):
     - `cargo nextest run -p eml_cli -p eml_tls -j 1 -E 'test(=tls::a_real_socket_handshake_rejects_the_expired_fixtures) | test(/^platform::/)'` → every test `PASS`, each under 1 s, no `SLOW`.
     - `cargo nextest run -p eml_cli -E 'test(=tls::a_real_socket_handshake_rejects_the_expired_fixtures)' --stress-count 12 -j 12` → 12 passed, no `WouldBlock` panic.
  4. Record the baseline in section 9 (macOS, clean tree):
     - the `cargo nextest run` summary line (expected: no `failed`);
     - T-B-vm and T-B-nrt, three runs each and the medians (U1 row of T-B);
     - `cargo nextest run -p eml_io -p eml_vm -E 'test(tls_off::) | test(gui_off::)'` → `3 passed`;
     - `cargo nextest run --workspace -E 'package(eml_vm) & test(/^(tls|gui)::/)'` → `31 passed`;
     - the `user` time (`/usr/bin/time -p`) of the two `eml_nrt` Miri runs (`nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_nrt --lib -- entries:: census:: constants::`, and with `-Zmiri-tree-borrows` added) and of the two `eml_vm` runs (`… cargo miri test -p eml_vm --test integration miri::`, same two settings);
     - `counts run_native`, `counts run_fail_native` → the values of section 3;
     - T's slowest line and B's time;
     - FL's two lines → as given under FL;
     - `bench/native.sh` → 42 rows: 27 with numbers, 15 `rejected`, no `differs`;
     - `grep -rn 'native: skip' tests/ui | wc -l` → 8.
  5. Copy the plan and the four appendix files into place and commit as `Start jit-externs`.
- Left to the implementer: none.
- Condition: always.
- Completion criteria:
  - steps 2-4 print what is written; a different baseline number is recorded with the difference stated, and a difference in a count of section 3 stops the stage.
  - `git diff --name-only HEAD~1 HEAD | sort` → exactly the five target files.
  - `cargo nextest run -p eml_cli --test integration -E 'test(/^citations::/)'` → no `failed`.
- Pitfalls: if step 3 is slow again, `target/debug/deps` has grown; do not change the test (D-38).

### UT: the testing.md paragraph on the macOS verifier cost

- Purpose: record the cause of the slow TLS test so it is not investigated again (U-B, D-38).
- Targets: `docs/implementation/testing.md` only, section 「性能のテスト」: one paragraph after the paragraph that starts 「CI は、`.config/nextest.toml` の `timing` の profile で…」.
- Work: translate this draft with PLaMo (`form: plain`; the section's earlier paragraphs as `history`; glossary: `SecPolicyCreateSSL`, `target/debug/deps`, `cargo clean --profile dev`, `eml_tls`, `platform::`, `eml_cli`, `tls::`):

  > On macOS, the first certificate check through the OS verifier in a process (`SecPolicyCreateSSL`) reads the directory that holds the executable. Test binaries sit in `target/debug/deps`, so when stale files from incremental builds pile up there (hundreds of thousands), the tests that reach the OS verifier (`eml_tls`'s `platform::` and `eml_cli`'s `tls::`) take seconds each. `tls::` then exceeds its server's 5-second watchdog and fails. `cargo clean --profile dev` restores the time. CI builds into a fresh tree and does not see this.

  Commit message: the cause (the CFBundle directory scan; about 448k entries) and that no test or code changed.
- Left to the implementer: the Japanese wording.
- Completion criteria:
  - `git diff --name-only HEAD~1 HEAD` → `docs/implementation/testing.md`.
  - `cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` → no `failed`.

### G-a: `eml_rows` and the graph script

- Purpose: move the row list and the load-time constants out of the task layer, and put the crate graph under one check.
- Targets:
  - New crate `crates/eml_rows`: `Cargo.toml` shaped like `eml_sched`'s (`autotests = false`; `[lib] doctest = false`; `[[test]] name = "integration", path = "tests/main.rs"`); `[dependencies]` `eml_extern`, `eml_rt`, `eml_sched`; `src/lib.rs` with `pub mod constants; pub mod rows;` and a module doc whose first sentence says the crate is what the built-ins are at run time for both engines; `tests/main.rs` with `mod constants;`.
  - Root `Cargo.toml`: `eml_rows = { path = "crates/eml_rows" }` in `[workspace.dependencies]` (the members glob `crates/*` takes the crate as is).
  - `git mv`: `crates/eml_sched/src/rows.rs` → `crates/eml_rows/src/rows.rs`; `crates/eml_sched/src/constants.rs` → `crates/eml_rows/src/constants.rs`; `crates/eml_sched/tests/constants.rs` → `crates/eml_rows/tests/constants.rs`. In `rows.rs`, `crate::error::RunFault` → `eml_sched::error::RunFault`; in `tests/constants.rs`, `eml_sched::constants` → `eml_rows::constants`.
  - `eml_sched`: drop `eml_extern` from `[dependencies]`; `lib.rs` loses `pub mod constants; pub mod rows;` and its module doc no longer names `constants.rs`; `tests/main.rs` loses `mod constants;`.
  - `eml_nrt`: add `eml_rows`; `rows.rs`: `eml_sched::extern_rows!(entries)` → `eml_rows::extern_rows!(entries)`, `[eml_sched::rows]` → `[eml_rows::rows]`; `constants.rs` and the test import at `entries.rs:759` read from `eml_rows`; `lib.rs`: `pub use eml_rows::rows as shared_rows;` and `pub use eml_rows::rows::{LISTED, Listed, Section};` (transitional; G-d deletes them).
  - `eml_vm`: add `eml_rows`; `src/compile/constants.rs` imports from `eml_rows::constants`.
  - `crates/eml_native/tests/placement.rs`: rename `eml_native_has_no_edge_to_eml_sched` to `eml_native_has_no_edge_to_the_shared_runtime`. It asserts: `eml_nrt`'s manifest names `eml_sched` and `eml_rows`; the text of `eml_native`'s manifest from `[dependencies]` to the next line starting with `[` names none of `eml_sched`, `eml_rows`, `eml_io`, `eml_vm`; `files_with(&native(), name, true)` is empty for `eml_sched` and for `eml_rows`. Add `eml_rows` to the crate list of `placement::code_cites_only_spec_and_implementation`.
  - New `nix/crate-graph.sh` (executable, bash, Japanese comments), specified below.
  - `.github/workflows/ci.yml`: the step `dependency edges of eml_native` is renamed `crate graph`, its `run:` becomes `bash nix/crate-graph.sh --check`, its comment cites architecture.md 「crate の依存関係」. In the step `cargo clippy of the non-default features`, `-p eml_io -p eml_vm -p eml_nrt --all-targets` becomes `-p eml_io -p eml_rows -p eml_vm -p eml_nrt --all-targets`.
  - Docs: architecture.md 「crate の依存関係」 (the dependency block; the `eml_sched` and `eml_nrt` bullets; a new `eml_rows` bullet; a bullet for the three strata and rules 1, 4, 5 of D-2; a bullet for the script and how a change of edge or feature edits it), L442 / L445 (`eml_sched::rows`); spec/native.md L116 (`eml_sched::constants`), L204, L223 (the paths only); testing.md 「`eml_sched`のテスト」 (the constants and row tests are `eml_rows`'), the CI section (step `crate graph`), 「feature の組み合わせの確認」 (the clippy row); future/multicore.md 「層の構成」 (the diagram and its first two bullets); future/roadmap.md (below); CLAUDE.md (the `eml_sched` line without rows / constants, a new `eml_rows` line, the `eml_nrt` line; one new invariant bullet: strata, the script, rule 5; Commands: `bash nix/crate-graph.sh [--check]` and the clippy combo comment with `eml_rows`).
  - future/roadmap.md, written here (D-39):
    - the jit-externs text: 「`eml_sched`へ移す」 → `eml_rows`; name the two new crates;
    - jit-tasks: the worker table, inboxes, monitor and jobs go to `eml_sched`; task row bodies and the `ffi` submit / take go to `eml_rows`; `GateHost` is deleted once `WorkerCore` is in `eml_sched`; `eml_sched/testing` is deleted; the capture fixtures leave `eml_vm/src` when no unit test there needs them; `eml_vm` drops `eml_io`; `eml_nrt` gains `eml_cabi` (and `eml_io` if its workers spawn masked threads); the two dead-strip rules of D-2 bind the async entries;
    - jit-repl: `c_links` and `renumbered` go to `eml_rows::ffi`; recovery to `eml_sched`;
    - 「その後の項目」, the item on `eml_sched` as a crate: its text says the rows are in `eml_rows`; its condition is unchanged;
    - jit-effects, native-inline, par-steal, std-par, native-par: no change.
- `nix/crate-graph.sh`:
  - Usage `nix/crate-graph.sh [--check]`. Header `set -euo pipefail`, then `cd "$(dirname "$0")/.."`. Every text read from a manifest or from `cargo tree` goes through `tr -d '\r'`.
  - It holds three tables as shell variables, `EDGES`, `FEATURES` and (from G-d) `REEXPORTS`, and computes three texts:
    - edges: for each directory `crates/*/` in sorted order, the crate name `c`, `: `, then the space-joined result of `cargo tree -p "$c" -e normal --depth 1 --prefix none --all-features --target all | grep '^eml_' | sed 's/ .*//' | grep -vx "$c" | sort -u`. A crate with no edge prints `c:`.
    - features: for each crate whose `Cargo.toml` has a `[features]` table, a line `[c]`, then the lines between `[features]` and the next line starting with `[`, without blank lines and without lines whose first non-blank character is `#`.
    - reexports (from G-d): `grep -rnE '^\s*pub use eml_' crates/*/src | grep -vE '^crates/eml_(cli|test_support)/' | sed -E 's/:[0-9]+:[[:space:]]*/: /' | sort`.
  - Without an argument it prints the three computed texts under the headings `EDGES`, `FEATURES`, `REEXPORTS`, for pasting into the tables.
  - With `--check` it runs every check below, prints every failure, and exits 1 if any failed:
    - `diff` of each computed text against its table (G-C1, G-C2, G-C5, G-C6);
    - from G-d: `grep -rnE '^\s*pub use (cells|doorbell|driver|error|park)::' crates/eml_vm/src` prints nothing;
    - closure checks, each a count of a line-anchored pattern over `cargo tree … --target all --prefix none`:

      | From | Tree | Pattern | Expected |
      |---|---|---|---|
      | G-a | `-p eml_vm -e normal` | `^(cranelift[a-z-]*\|eml_native\|eml_nrt\|eml_clink) v` | 0 |
      | G-a | `-p eml_native -e normal` | `^eml_vm v` | 0 |
      | G-a | `-p eml_lir -e normal` | `^eml_rt v` | 0 |
      | G-a | `-p eml_sched -e normal` | `^(eml_extern\|eml_rows\|eml_cabi\|eml_tls\|eml_gui\|cranelift[a-z-]*\|eml_lir\|eml_vm\|eml_nrt\|eml_native) v` | 0 (G-C3) |
      | G-a | `-p eml_nrt -e normal,build` | `^(cc\|cmake\|cxx-build\|bindgen\|nasm\|pkg-config) v` | 0 (G-C13) |
      | G-a | `-p eml_nrt -e normal` | TOOL | 0 (G-C10) |
      | G-a | `-p eml_cli -e normal,build -f '{p} {f}'` (as in today's CI step, without `--prefix none`) | `eml_sched.*testing` | 0 (N-41) |
      | G-a | `grep -rn unsafe crates/eml_extern/src` | any line | none |
      | U3 | `-p eml_nrt -e normal --features eml_rows/tls` | TOOL | 0 |
      | U3 | `-p eml_nrt -e features --features eml_rows/tls -i eml_io` | `eml_io feature "tls"` | 0 (G-C12) |
      | U3 | `-p eml_cli -e features -i eml_io` | `eml_io feature "tls"` | ≥ 1 (G-C12) |
      | U4 | `-p eml_nrt -e normal` with `--features eml_rows/gui-native`, and with `--features eml_rows/tls,eml_rows/gui-native` | TOOL | 0 |
      | U4 | `-p eml_nrt -e normal`, no features | TLS, GUI | 0, 0 (G-C11) |
      | U4 | `-p eml_nrt -e normal --features eml_rows/tls` | GUI; `^eml_tls v` | 0; ≥ 1 |
      | U4 | `-p eml_nrt -e normal --features eml_rows/gui-native` | TLS; `^eml_gui v` | 0; ≥ 1 |

      TOOL = `^(cranelift[a-z-]*|libloading|eml_lir|eml_core_ir|eml_types|eml_hir|eml_syntax|eml_diagnostics|eml_vm|eml_native|eml_clink|clap|rustyline|ariadne|logos|rowan) v`. TLS = `^(eml_tls|rustls[a-z-]*|aws-lc-rs|aws-lc-sys|ring|webpki-root-certs|security-framework[a-z-]*|schannel|openssl-probe|jni[a-z-]*) v`. GUI = `^(eml_gui|taffy|slotmap|cxx[a-z-]*|link-cplusplus|objc2[a-z-]*|windows-(core|collections|reference|time|implement|interface|result|strings)|qt-build-utils) v`. (The table writes `|` as `\|` only because of Markdown.) The rows "U3" and "U4" are added by those tasks.
- Work (TDD):
  1. `cargo nextest run -p eml_sched` and `cargo nextest run -p eml_native --test integration placement::`; note the passed counts.
  2. Rename and strengthen the placement test. It fails until `eml_nrt`'s manifest names `eml_rows`.
  3. Create the crate, move the files, edit paths and manifests.
  4. Write the script. Paste its no-argument output into the tables and compare with the base table of 7.1 plus G-a's deltas: equal line by line.
- Left to the implementer: the script's internal structure; Japanese wording.
- Completion criteria:
  - G; GS lines 1-11 and 17; G-grep (always, G-a).
  - `cargo nextest run -p eml_rows` → `7 tests run: 7 passed` (`rows::tests` 3, `constants::` 4).
  - `cargo nextest list -p eml_sched | grep -cE 'rows::|constants::'` → 0.
  - `cargo nextest run -p eml_native --test integration -E 'test(=placement::eml_native_has_no_edge_to_the_shared_runtime)'` → 1 passed.
  - Negative check on a scratch copy, not committed: add `eml_vm.workspace = true` to `eml_nrt`'s `[dependencies]` → `--check` exits 1, its diff names `eml_nrt` and the TOOL row fails. Delete one feature line of `eml_io` → `--check` exits 1.
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_nrt --lib -- entries:: census:: constants::` → `test result: ok`.
  - `nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- .github/workflows/ci.yml` → exit 0 (below: "actionlint").
  - `nix shell nixpkgs#shellcheck -c shellcheck nix/crate-graph.sh` → exit 0.
  - `nix develop -c nix/third-party-licenses.sh --check` → exit 0 (below: "licences check").
- Pitfalls: the placement test must scope to `[dependencies]`, or G-d's dev-dependency breaks it. The script must run under Git Bash (Q-9).

### G-b: `eml_clink`

- Purpose: the C linker leaves the code generator, so the placement rules of `eml_native` hold without exemptions (AM-5).
- Targets:
  - New crate `crates/eml_clink`: `[dependencies]` `eml_cabi`, `eml_extern`, the `cranelift-*` crates `bridge.rs` imports (`codegen`, `frontend`, `jit`, `module`, plus `native` if `bridge.rs` uses it), `libloading`. `src/lib.rs` starts with `#![deny(unsafe_code)]`, has a module doc, declares `mod bridge; mod error; mod library; mod linker;` and `#[cfg(test)] mod trust;`, and re-exports `pub use bridge::Bridges; pub use error::LinkError; pub use library::{candidates, running_os}; pub use linker::Linker; pub use eml_cabi::Linked;`.
  - `git mv`: `crates/eml_native/src/{linker,library,bridge,error}.rs` → `crates/eml_clink/src/`; `crates/eml_native/tests/{linker,bridge}.rs` → `crates/eml_clink/tests/`; every item of `crates/eml_native/tests/common/mod.rs` except `pub mod native;` → `crates/eml_clink/tests/common/mod.rs` (`eml_native/tests/common/mod.rs` keeps `pub mod native;` and its doc line).
  - New `crates/eml_clink/src/trust.rs`, a copy of `eml_native`'s scanner with `FILES` = lib, bridge, error, library, linker; `UNSAFE_FILES` = bridge, library, linker; tests `unsafe_stays_in_the_listed_files`, `every_source_file_is_listed`, `the_scanner_skips_comments_and_strings`. `eml_native`'s `trust.rs` loses the four files and its `UNSAFE_FILES` becomes `["jit.rs"]`.
  - `eml_native`: `lib.rs` loses the four modules and their five re-exports; `Cargo.toml` loses `libloading`; `tests/main.rs` loses `mod bridge; mod linker;`.
  - `crates/eml_native/tests/placement.rs`: the four tests lose `bridge.rs` and are renamed `jit_module_is_named_only_in_jit`, `memory_is_touched_only_in_mem_and_thunk`, `flags_are_built_only_in_abi`, `indirect_calls_are_only_in_apply_and_thunk`; their messages drop "bridge.rs". Add `eml_clink` to the crate list of `placement::code_cites_only_spec_and_implementation`.
  - `eml_cli`: add `eml_clink` (optional; `run` + `"dep:eml_clink"`); `src/lib.rs` and `src/repl.rs` take `Linker` / `LinkError` from it. `eml_test_support`: add `eml_clink` under `run`; take `Linker` from it.
  - `ci.yml`, ASan job: a step `cargo test -p eml_clink` in the form of the `eml_native` step (`nix develop .#miri -c cargo test -Zbuild-std --target ${{ matrix.target }} -p eml_clink`).
  - The script: G-b's deltas.
  - Docs: architecture.md 「crate の依存関係」 (block; `eml_native` and `eml_cabi` bullets; a new `eml_clink` bullet), 「`eml_native` の内部」 (no linker, no exemption), a new 「`eml_clink`の内部」, 「外部 crate」 (libloading is `eml_clink`'s); spec/vm.md 「ライブラリと記号」 and 「橋渡しの関数」 where they name `eml_native`; testing.md 「`eml_native` の検証」 (the bridge, linker and trust tests are `eml_clink`'s; the ASan list); future/ffi.md L102 (`eml_cabi`'s users); CLAUDE.md (the `eml_native` line without "C library/symbol resolution" and bridges; a new `eml_clink` line; the `eml_cabi` line: `eml_clink` builds `Bridge`, `Dtor`, `CCall`, `Linked`; the ASan command's list gains `-p eml_clink`).
- Work (TDD): the spec is the 25 moved tests (`bridge::` 9, `linker::` 16) and the trust tests. Run them before the move (`-p eml_native`) and after it (`-p eml_clink`).
- Left to the implementer: wording.
- Completion criteria:
  - G; GS including the `eml_clink` clippy line; G-grep (G-b).
  - `cargo nextest run -p eml_clink` → `28 tests run: 28 passed` (25 integration + 3 trust).
  - `cargo nextest run -p eml_native -E 'test(/^trust::/) | test(/^placement::/)'` → no `failed`.
  - `cargo tree -p eml_native -e normal --target all | grep -c libloading` → 0.
  - `bash nix/crate-graph.sh --check` → exit 0, with the row `eml_clink: eml_cabi eml_extern`.
  - ASan: `nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=address CARGO_TARGET_DIR=target/asan cargo test -Zbuild-std --target $(rustc -vV | sed -n 's/^host: //p') -p eml_clink` → `test result: ok`; the same with `-p eml_native -- --skip fatal::` → `test result: ok`.
  - actionlint → exit 0; licences check → exit 0; `nix build` → exit 0.

### G-c: the real hosts move to `eml_io`

- Purpose: the host constructors a shipped app needs live in the runtime closure; the capture fixtures stay in `eml_vm` (G-C9).
- Targets:
  - New `crates/eml_io/src/hosts.rs` with `stdio_host`, `script_host` and the private `unmasked`, moved from `eml_vm/src/hosts.rs:610-632`. No `unsafe`. `eml_io/src/lib.rs`: `mod hosts;` and `pub use hosts::{script_host, stdio_host};`. `("hosts.rs", include_str!("hosts.rs"))` goes into `eml_io`'s `trust.rs` `FILES`.
  - `eml_vm`: `lib.rs` drops `script_host` and `stdio_host` from `pub use hosts::…`; `hosts.rs` keeps `CaptureHost` and the other fixtures.
  - `eml_cli`: add `eml_io` (optional; `run` + `"dep:eml_io"`); `src/lib.rs` re-exports `stdio_host` / `script_host` from `eml_io` (`terminal.rs` and `main.rs` are unchanged through that facade).
  - `crates/eml_vm/tests/spawns.rs`, `EXPECTED` (kind 3): `("eml_vm/src/hosts.rs", "Unmasked::new", 2)` becomes 1; `("eml_vm/src/hosts.rs", "with_around_spawn", 2)` is removed; add `("eml_io/src/hosts.rs", "Unmasked::new", 1)` and `("eml_io/src/hosts.rs", "with_around_spawn", 2)`; the doc comment names `eml_io`'s `hosts.rs`. (`CRATES` already holds `eml_io`.)
  - The script: G-c's deltas.
  - Docs: architecture.md 「`eml_io` の内部」 (the two constructors; the fixtures stay in `eml_vm`), the `eml_io` bullet, and in the `eml_vm` bullet the sentence on `hosts.rs`; spec/vm.md 「置き場所」, the sentence on where the hosts are; CLAUDE.md, the `eml_io` line.
- Left to the implementer: wording.
- Completion criteria:
  - G; GS; G-grep (G-c).
  - `cargo nextest run -p eml_vm --test integration -E 'test(=spawns::every_thread_but_worker_0_is_masked)'` → 1 passed.
  - `cargo nextest run -p eml_io --lib -E 'test(/^trust::/)'` → no `failed`.
  - `bash nix/crate-graph.sh --check` → exit 0.

### G-d: re-exports and direct edges

- Purpose: rule 3 of D-2 (G-C6, AM-13): a crate re-exports from another workspace crate only the kept types, and `eml_cli` names the crate that defines what it uses.
- Targets:
  - `eml_sched/src/lib.rs`: `pub use eml_rt::{Control, HostRef, Limits, Stop};`.
  - `eml_vm/src/lib.rs`: the foreign re-exports become exactly `pub use eml_cabi::Linked;`, `pub use eml_rt::HostRef;`, `pub use eml_sched::config::{Finished, RunConfig};`, `pub use eml_sched::error::RuntimeError;`, plus `#[cfg(feature = "gui")] pub use eml_gui;` until U4. Delete `pub use doorbell::Doorbell`, `pub use driver::{..}`, `pub use error::{..}`, `pub use park::{..}`, and the `pub use` lines at `sched.rs:17` (`Sched`) and `workers.rs:25` (`Seat`), with their names in `lib.rs`'s `pub use sched::{..}` / `workers::{..}`. The `pub(crate) use eml_sched::{cells, doorbell, driver, error, park};` aliases stay. Uses of deleted names inside `eml_vm/src` through `crate::<Name>` import from the defining crate. `pub use gui::park as gui_park` stays until U4 (it is `eml_vm`'s own item). `eml_vm`'s own items (`Machine`, `AnyMachine`, `capture_host`, `Capture`, `CaptureHost`, `Written`, `CodeBase`, …) are unaffected.
  - `eml_nrt/src/lib.rs`: delete `shared_rows` and `{LISTED, Listed, Section}`.
  - `eml_native/src/lib.rs`: `pub use eml_nrt::SUPPORTED;` becomes `pub const SUPPORTED: bool = eml_nrt::SUPPORTED;` with a doc line; `pub use eml_nrt::image::Options;` stays.
  - `eml_native` gains the dev-dependency `eml_rows`: `tests/rows.rs`: `use eml_nrt::shared_rows as shared;` → `use eml_rows::rows as shared;`; `tests/unsupported.rs:297`: `eml_nrt::LISTED` → `eml_rows::rows::LISTED`. The comment at `src/lower/rows.rs:163` is rewritten without naming a crate (it cites the docs section instead), so G-C4's grep stays empty.
  - `eml_cli`: add `eml_sched` (optional; `run` + `"dep:eml_sched"`). `src/lib.rs`'s facade takes each name from its defining crate: from `eml_sched`: `Clock`, `Control`, `Doorbell`, `Limits`, `RunFault`, `RunStats`, `Sched`, `Signal`, `SourceLocation`, `Stop`, `VirtualSignal`, `ParkFactory`, `Finished`, `HostRef`, `RunConfig`, `RuntimeError`; from `eml_io`: `Masked`, `stdio_host`, `script_host`; from `eml_clink`: `Linked`; from `eml_vm`: `Capture`, `CaptureHost`, `Written`, `capture_host`, `execute`. `src/repl.rs` and `tests/ui.rs` import accordingly. `eml_cli/src` still names no `eml_rt`.
  - `eml_test_support`: add `eml_sched` under `run`; take `Finished`, `HostRef`, `RunConfig`, `RunStats`, `RuntimeError` from it.
  - `crates/eml_vm/tests/*` (25 files name `eml_vm::` items): imports of deleted names come from `eml_sched` / `eml_cabi` / `eml_io` (kind 3).
  - The script: the `REEXPORTS` table of 7.1 and the alias check.
  - Docs: spec/vm.md 「置き場所」 (`eml_vm`'s dependency list; the re-export bullets: `eml_vm` re-exports the types of `execute`'s signature only, `eml_sched` re-exports `Control`, `Limits`, `Stop`, `HostRef`, `eml_cli` takes the run interface from `eml_sched`); spec/native.md L223 (no `shared_rows` / `LISTED` re-export; `eml_native`'s tests read `eml_rows` through a dev-dependency) and L225 (the shared function is `eml_rows::rows`'); testing.md L269 (`eml_rows::rows`); architecture.md (a rule-3 bullet; the `eml_cli` and `eml_test_support` bullets); CLAUDE.md (the `eml_cli` line names `eml_sched` and `eml_io`; the invariant bullet gains "re-exports of signature types only").
- Left to the implementer: wording; the order of the import edits.
- Completion criteria:
  - G; GS; G-grep (G-d).
  - `bash nix/crate-graph.sh --check` → exit 0, with the `REEXPORTS` table equal to 7.1's list.
  - `cargo clippy -p eml_cli --no-default-features --features run -- -D warnings` and the same with `--features core` → exit 0.
  - `cargo nextest run -p eml_native --test integration -E 'test(rows::) | test(unsupported::)'` → no `failed`.
- Pitfalls: `Machine::with_doorbell(Arc<Doorbell>)` still takes a `Doorbell`; its callers import `Doorbell` from `eml_sched`. Do not keep a re-export because a signature names the type: the kept list is exact (D-23).

### U2a: list reshape and entries on the 54 rows

- Purpose: the list form of D-1 on today's 54 rows. The 11 row entries become 56 (54 + `Exit` + `Panic`). The VM is not touched.
- Targets:
  - `crates/eml_rows/src/rows.rs`. The list:
    ```
    list    := "rows" "{" line* "}" "engine" "{" names "}" "lir" "{" names "}" "task" "{" names "}" "pending" "{" names "}"
    line    := "fn" Variant ("|" Variant)* "(" ctxpat ("," ident ":" kind)* ")" "->" ret block
    kind    := obj | tobj | unit | int | enum | float          ret := kind | never
    ```
    - `rows` holds the 54 rows (the 8 generic ones, `FloatMod`, the 45 inline ones, with an unused context where HEAD has one); `engine {Panic, Exit}`; `lir {Par}`; `task {the 101 async rows}`; `pending {the 176 other non-async rows}` in the P format.
    - Body types are HEAD's (`arg_ty!`, `ret_ty!`): `obj` / `tobj` → `Word`; `unit` / `int` / `enum` → `i64` as argument; `float` → `f64`; result `unit` → `()`, `never` → `Infallible`. Body type `Result<T, RunFault>`.
    - `defs` is a three-phase, non-recursive expansion and emits:
      ```rust
      pub unsafe fn <First><M: Mode>(ctx: &mut Ctx<M>, ..) -> Result<T, RunFault>      // #[inline], shared `# Safety`
      const _: () = assert!(kinds_match(Extern::<V>.row(), ..));                         // per variant
      pub const SHARED: &[Extern];                                                        // every variant of `rows`, list order
      pub trait Args { fn word(&self, i: usize) -> Word; fn int(&self, i: usize) -> i64; fn float(&self, i: usize) -> f64; }
      pub enum Value { Word(Word), Int(i64), Float(f64) }
      pub unsafe fn call<M: Mode>(ext: Extern, ctx: &mut Ctx<M>, args: &impl Args) -> Result<Value, RunFault>;
      ```
    - `call` carries `#[deny(unreachable_patterns)]` and has no `_` arm: one arm per line reading arguments by kind (`obj|tobj` → `word`, `unit|int|enum` → `int`, `float` → `float`) and wrapping the result (`unit` → `Value::Word(Word::UNIT)`, `never` → `match v {}`); one arm per name group returning `RunFault::Internal` with D-11's texts.
    - Matching rule: match the parameter group as `( $($sig:tt)* )`, then re-pack it as one `tt` in a second phase before it is used per variant (`$sig:tt` right after `$(| $rest)*` is ambiguous; a depth-2 variable inside the per-variant repetition is a lockstep error).
    - Delete `Section`, `Listed`, `section_of!`, `names!`, `LISTED`. Rewrite the module doc.
  - `crates/eml_nrt/src/entries.rs`: the prefix arm `$(#[$tm:meta])* $tv:vis const $table:ident = $prefix:literal;` with `symbol: concat!($prefix, stringify!($name))`. The arm without a prefix forwards with `""` (an optional prefix must be its own macro arm).
  - `crates/eml_nrt/src/rows.rs`: the `entries` expansion with exactly two forms (D-22):
    - public: `(rows { $($lines:tt)* } engine $e:tt lir $l:tt task $t:tt pending $p:tt)` forwards to `entries!(@table [pub(crate)] ROWS "eml_x_" [eml_rows::rows] [ <hand-written shim fn Exit, shim fn Panic> ] $($lines)*)`. (U5j drops `pending $p:tt`.)
    - `(@table [$($vis:tt)*] $table:ident $prefix:literal [$($path:ident)::+] [$($extra:tt)*] $($lines:tt)*)` runs the three phases over the lines and emits `crate::entries! { $($vis)* const $table = $prefix; <one shim per variant calling $path::<First>> $($extra)* }`.
    - Each shim keeps HEAD's body: `Ok` → the value; `Err(RunFault::Extern(f)) if FAULTS` → `set_fault`; any other `Err` → `worker::record` (N-9). The Rust items are `eml_nrt::rows::<Variant>`; the symbols are `eml_x_<Variant>`.
  - `crates/eml_native/tests/{abi.rs, rows.rs, unsupported.rs}`.
- Work (TDD; the crate that must compile after each step is named):
  1. `eml_rows` tests (they fail to compile): `rows::tests::shared_lists_each_row_with_a_body_once`, weak form (no duplicate, no `is_async()` row, none of `Par` / `Exit` / `Panic`); `rows::tests::call_runs_a_row_by_its_variant` (`StrConcat` → `Value::Word` holding "abcd"; `IntAdd(1, 2)` → `Value::Int(3)`; `FloatMod(7.5, 2.0)` → `Value::Float(1.5)`; `Exit`, `Par`, `AsyncPark` → `Err(RunFault::Internal(_))`; nothing about `pending`).
  2. Reshape the list and `defs`; delete the two kind-1 tests of `eml_rows`. Checkpoint: `cargo nextest run -p eml_rows --lib rows::` → `3 passed`. (`eml_nrt` does not compile yet.)
  3. `eml_nrt`: rewrite `entries::tests::every_row_entry_is_named_after_its_variant` (the `ROWS` symbols are `SHARED` mapped to `eml_x_{:?}`, then `eml_x_Exit`, `eml_x_Panic`; each entry's `(params, result)` equals its row's Reprs: `float` → `F64`, any other → `I64`, `Never` → none). Rewrite `rows::fatal::a_row_error_of_the_internal_class_is_recorded` as `entries!(@table [] TEST_ROWS "eml_t_" [test_rows] [] fn IntShow(ctx, n: int) -> obj { .. })`, where `test_rows::IntShow` returns `Err(RunFault::Internal("a test row failed"))`; the assertions are HEAD's (`FATAL_RECORDED`, site 0, `worker.error`); only the symbol becomes `eml_t_IntShow`. Add `entries::tests::x_int_add_parks_an_overflow` and `entries::tests::x_str_eq_returns_an_enum`. Rename `crate::rows::eml_x_<V>` to `crate::rows::<V>` in the `x_*` tests. Implement the prefix arm and `entries`. Checkpoint: `cargo nextest run -p eml_nrt` → no `failed`.
  4. `eml_native`: delete `unsupported::the_inline_table_is_the_marked_set`; in `tests/rows.rs` replace `Shared` / `shared_of` by `eml_rows::rows::call` through a test `Args` over `&[Arg]`; add the echo entries `eml_t_echo_f_f` and `eml_t_echo_ff_i` in `tests/abi.rs`. Checkpoint: `cargo nextest run -p eml_native` → no `failed`.
  5. Docs: spec/native.md 「記述子の表」 (the table prefix form: the symbol is the identifier, or the prefix followed by it), 「externの失敗」 (one `shim` entry per variant of the `rows` group; `Exit` / `Panic` by hand; no mark, no `as`), 「行の下ろし方」 first paragraph (no mark; `rows::INLINE` rows also have entries, which generated code does not call; no re-export; the `rows::CALLED` sentence stays until 7b); architecture.md (the `eml_rows` bullet; the `rows.rs` sentence in 「`eml_nrt`の内部」); testing.md 「`eml_rows`のテスト」 (the heading renamed from the `eml_sched` part G-a edited), 「`eml_nrt`の検証」 (the rewritten and new tests; `rows::fatal`), 「`eml_native` の検証」; the CLAUDE.md `eml_rows` line.
- Left to the implementer: macro internals within the arms above; the spelling of unused parameters (`_unit: unit`; `_` is not an `ident`); wording.
- Completion criteria:
  - G; GS; G-grep (U2a).
  - `cargo nextest run -p eml_nrt --lib -E 'test(/^entries::tests::x_/) | test(=entries::tests::every_row_entry_is_named_after_its_variant)'` → `8 passed` (below: "the x_ filter").
  - `cargo nextest run -p eml_native --test integration -E 'test(=abi::mixed_int_and_float_arguments_reach_an_entry)'` → 1 passed (it fails until both echo entries exist).
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_nrt --lib -- entries:: census:: constants::` → `test result: ok`; with `-Zmiri-tree-borrows` added → `test result: ok` (below: "both eml_nrt Miri runs").
- Pitfalls: `eml_nrt`'s expansion and `eml_native`'s tests cannot compile against a half-changed grammar, so this commit carries all three crates. If a `clif::` snapshot changes because entries were declared, stop and report (Q-11).

### U2b: the VM routes through `rows::call`

- Purpose: D-11.
- Targets: `crates/eml_vm/src/vm.rs`:
  - `run_extern`: remove the 8 seed arms (`IntShow | ByteDisplay` becomes a `ByteDisplay` arm), the dedicated-`Internal` arm, the `Par` arm and the async arms; add `_ => match unsafe { eml_rows::rows::call(ext, ctx, &At { .. }) }? { Value::Word(w) => task.set(dst, w), Value::Int(n) => task.set_raw(dst, n), Value::Float(x) => task.set_float(dst, x) }`.
  - A private `At` implementing `Args` over `task.get` / `raw` / `float`.
  - The `run_extern` comment.
- Work (TDD): the spec is the UI tests and `bench::` (kind 3).
- Left to the implementer: the fields of `At`.
- Completion criteria:
  - G; GS; G-grep (U2a).
  - P → 22 `>` lines and no `<` line.
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::` → `test result: ok` with `user` ≤ 300 s; with `-Zmiri-tree-borrows` added → ok with `user` ≤ 400 s (below: "both eml_vm Miri runs").
  - T-B measured (U2b row) and written into the commit message.

### U3: move `Std.Tls` to `eml_rows`

- Purpose: move `tls.rs` whole, with the feature declared in `eml_rows` (D-15).
- Targets:
  - `git mv crates/eml_vm/src/tls.rs crates/eml_rows/src/tls.rs`; `pub mod tls;` in `eml_rows`. Its imports come from `eml_sched::{cells, driver, error}`. Make `pub` what `eml_vm` uses: `Roots`, `client_conn`, `push_conn`, `TlsConn`, `SETUP_KIND`, and the 10 row functions (until U5i).
  - `eml_rows/Cargo.toml`: `[features] tls = ["dep:eml_tls"]`; `eml_tls = { workspace = true, optional = true }`; `[dev-dependencies] eml_test_support = { workspace = true, features = ["run"] }`.
  - `git mv crates/eml_vm/tests/tls_off.rs crates/eml_rows/tests/tls_off.rs`, under `#[cfg(not(feature = "tls"))]` in `eml_rows/tests/main.rs`; its doc names `eml_rows`.
  - `eml_vm`: the `tls` feature and the `eml_tls` dependency are removed (`gui` stays until U4); paths become `eml_rows::tls`; the `sched.rs` tests name `eml_rows::tls::Roots`; `tests/main.rs` loses `tls_off`.
  - `eml_cli`: `eml_rows` (optional; `run` + `"dep:eml_rows"`); `tls = ["run", "eml_rows/tls", "eml_io/tls", "dep:eml_tls"]`.
  - `Cargo.lock` as cargo rewrites it. The script: U3's deltas and the closure rows "U3".
  - `ci.yml`: step `cargo nextest run without tls and gui`: `-p eml_io -p eml_vm -p eml_rows`, its comment names `eml_rows`. Step `feature resolution of tls`: its second line becomes `eml_rows feature "tls"` over `-i eml_rows`; its third line's tree becomes `-p eml_rows -p eml_io -p eml_vm -p eml_nrt`; add the G-C7c guard (GS 13, third line); rewrite the comment. Step `cargo clippy of the non-default features`: add `cargo clippy -p eml_rows --all-targets --features tls -- -D warnings`. Test job, every OS: a new step `cargo build -p eml_nrt --features eml_rows/tls` (G-C13b).
  - Docs: architecture.md (the `eml_rows` bullet: tls, rule 2 for `tls`; the `eml_tls` and `eml_io` bullets: `eml_rows/tls` does not enable `eml_io/tls`, the terminator is a tool's test facility; in the `eml_vm` bullet the sentence on `tls.rs`); testing.md 「feature の組み合わせの確認」 (`eml_cli` switches `eml_rows/tls` and `eml_io/tls`; the Tls run of `eml_rows` alone, `cargo nextest run -p eml_rows --features tls`, with `-p eml_io --features eml_io/tls` added only when a test needs the `tls.localhost` terminator; the cargo tree checks; where `tls_off` lives); CLAUDE.md (the `eml_tls` line: `eml_rows` reaches it through its feature `tls`; the `eml_rows` line; the clippy combo comment; the `tls_off` command `-p eml_io -p eml_vm -p eml_rows` until U4).
- Work (TDD): 1. before the move, `cargo nextest run -p eml_vm --lib --features tls -E 'test(/^tls::/)'` → `11 passed`. 2. Move. 3. `cargo nextest run -p eml_rows --lib --features tls -E 'test(/^tls::/)'` → `11 passed`.
- Left to the implementer: wording.
- Completion criteria:
  - G; GS (lines 12-14 from here); G-grep (U3).
  - `bash nix/crate-graph.sh --check` → exit 0. actionlint → exit 0. Licences check → exit 0.
- Pitfalls: if GS line 14 counts 0, a workspace run does not build `eml_rows` with `tls` (Q-8). A unit test inside `tls.rs` must name no engine crate (rule 5); if one does, it becomes an integration test in `crates/eml_rows/tests/`.

### U4: move `Std.Gui` with `GateHost`

- Purpose: move `gui.rs` whole: `GUI_SLOT`, `Slot`, `UiToken`, `GuiPark`, `park`, `arm`, `want`, `open_ui`, `abandon`, `lower_holds`, `is_gui_task`, `forget_opening`, `off`, and its 2 unit tests. The four scheduler functions take a `GateHost` (D-16). After U4 `eml_vm` has no features.
- Targets:
  - `git mv crates/eml_vm/src/gui.rs crates/eml_rows/src/gui.rs`; `pub mod gui;`. The `pub(crate) use on::*` / `off::*` the VM calls become `pub`. The trait `GateHost` goes in `gui.rs`, outside `on` / `off`. `#[cfg(feature = "gui")] pub use eml_gui::{Backend, Script};`. (`eml_rows` may hold the `thread_local!`: `placement::no_thread_locals` scans `eml_nrt` and `eml_native` only.)
  - `impl<P: Policy> GateHost for Scheduler<P>` in `crates/eml_vm/src/sched.rs`.
  - `eml_rows/Cargo.toml`: `gui = ["dep:eml_gui"]`, `gui-native = ["gui", "eml_gui/native"]`; `eml_gui` optional; dev-dependency `eml_vm`.
  - `git mv crates/eml_vm/tests/{gui,gui_off}.rs crates/eml_rows/tests/`, under `#[cfg(feature = "gui")]` / `#[cfg(not(feature = "gui"))]`. `gui.rs` takes `Backend`, `Script`, `stub` from `eml_gui`, `park` from `eml_rows::gui`, the run types from `eml_sched` / `eml_vm`, and `RecordingHost` from `eml_test_support`.
  - `RecordingHost` moves from `crates/eml_vm/tests/common/mod.rs` to `eml_test_support/src/lib.rs` under `run`; `eml_test_support` gains `eml_rt` (`run` + `"dep:eml_rt"`); `eml_vm/tests/seat.rs` imports it from there.
  - `eml_vm`: remove `[features]`, the `eml_gui` dependency, `mod gui`, `pub use eml_gui` and `gui_park`; `tests/main.rs` loses `gui` and `gui_off`; paths become `eml_rows::gui`.
  - `eml_cli`: `gui = ["run", "eml_rows/gui"]`, `gui-native = ["gui", "eml_rows/gui-native"]`; `src/lib.rs`: `pub use eml_rows::gui::{Backend, Script, park as gui_park};` and `ParkFactory` from `eml_sched::park`.
  - `Cargo.lock`. The script: U4's deltas, the `REEXPORTS` edit, the closure rows "U4".
  - `ci.yml`: step `cargo nextest run without tls and gui`: `-p eml_io -p eml_rows`. Step `cargo clippy of the non-default features`: add `-p eml_rows --all-targets --features gui` and `--features tls,gui`. The `gui-native` job is unchanged (it names `eml_cli/gui-native`).
  - Docs:
    - spec/vm.md 「GUIのPark」 (`eml_rows::gui`, `GateHost`);
    - architecture.md: the block; the `eml_rows` bullet (gui, `GateHost`); rule 2 as a bullet; 「`eml_gui`の内部」 where it names `eml_vm`'s `gui.rs`; in the `eml_vm` bullet the sentence on gui;
    - testing.md: 「GUIの手での確認」; 「feature の組み合わせの確認」 (the full feature table; the G-C7c guard; the gated tests live in `crates/eml_rows/tests/`; rule 5);
    - future/ffi.md L30 and L125 (they name `eml_vm`'s `gui.rs`);
    - distribution.md 「AOTの出力」 and future/native.md 「AOT と配布」 (AM-12 under U-F, AM-12b): the runtime static library is built by the artifact crate `eml_rtlib` in four variants `{base, tls, gui, tls+gui}`; the profile decides the `gui` axis; whether the module graph contains `Std.Tls` (directly or through `Std.Http`) decides the `tls` axis, which replaces 「プロファイルだけで決まり、importからは推さない」; each variant has one notice file with three parts: that variant's crates (cargo-about over `eml_rtlib` with its features), the Rust standard library with the crates it vendors, and for `gui` variants the system-library section with GPL-3.0 / LGPL-3.0;
    - future/roadmap.md, the native-aot item: `eml_rtlib` and its profile (LTO inside the library, `panic = "unwind"`), the variants and notices, the selection rule above, interrupt handling and the memory query moving into `eml_io`, `eml_clink::define_bridge`, the driver factory (removing `TestDriver` from apps), exported entries, the symbol, link-mode and size tests (G-C15..G-C17), and the tool's `THIRD-PARTY-LICENSES.md` gaining the standard-library section;
    - CLAUDE.md: the `eml_gui` line (`eml_rows` reaches it through its features `gui` and `gui-native`); the `eml_vm` line (drop "whose gui.rs holds the Std.Gui arms and the main worker's GUI Park"; `eml_rows::gui` holds them); the `eml_cli` line's features; the `eml_rows` line; Commands: the `tls_off` / `gui_off` line (`-p eml_io -p eml_rows`) and the clippy combo comment ("eml_io + eml_rows + eml_vm + eml_nrt --all-targets without tls and gui").
- Work (TDD): the spec is `gui::on::tests::*` (2), the 18 integration `gui::` tests, `gui_off::std_gui_is_not_available` and the UI `gui/` files. Run them before and after the move.
- Left to the implementer: the receiver mutability of `GateHost` within its six calls; wording.
- Completion criteria:
  - G; GS (lines 15-16 from here); G-grep (U4).
  - `bash nix/crate-graph.sh --check` → exit 0, with every closure row through "U4".
  - actionlint → exit 0.
  - `cargo clippy --all-targets --features eml_cli/gui-native -- -D warnings` → exit 0.
  - `cargo nextest run --features eml_cli/gui-native -E 'test(gui::) | test(gui_off::)'` → no `failed`.
  - `nix develop .#cross -c cargo clippy --target x86_64-unknown-linux-gnu --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run -- -D warnings` → exit 0.
  - `grep -c 'eml_rtlib' docs/future/native.md docs/implementation/distribution.md` → ≥ 1 in each; `grep -n 'importからは推さない' docs/future/native.md docs/implementation/distribution.md` → empty.
  - Licences check → exit 0. `nix build` → exit 0.
- Then CP-A.

### U5a..U5j: all bodies into the list

What every commit from U5b to U5i does:
- In one commit a family's arms leave `run_extern`, the same variants leave `pending`, and they enter `rows` as lines. An arm that matches on `ext` only to pick constants (`Open | FileOutCreate | FileOutAppend`, the read / write / close / flush families, `CharShow | CharDisplay`, the `Float` rounding rows) becomes one line per variant with the constants written out, e.g. `fn Open(ctx, path: obj) -> obj { Ok(ctx.fs_open(path, Open::Read)?) }`. No helper and no second `match`.
- `.map_err(Fault::Arith)?` and `?` keep HEAD's conversions.
- Pitfall: a line's parameters are in row order, while the body passes them in HEAD's `Ctx` order. `ReadChunk`: chunk = argument 0, handle = argument 1. The writes: text = argument 0, handle = argument 1. `ProcWaitStage(n: int, p: obj)` → `proc_wait_stage(p, n)`.

Completion of each commit: G; GS; G-grep up to that task; P (U5a..U5e: 22 `>` lines; U5f..U5i: empty); `cargo nextest run -p eml_native --test integration abi::` → no `failed`.

| ID | Rows (count) | Extra targets, tests, checks | Echo |
|---|---|---|---|
| U5a | none moved. `Op::Int{Add,Sub,Mul,Div,Rem,Neg,Eq,Ne,Lt,Le,Gt,Ge,Compare}`, `Op::Float{Add,Sub,Mul,Div,Rem,Neg,Eq,Ne,Lt,Le,Gt,Ge}` and `Op::Bool{Eq,Ne}` call `unsafe { eml_rows::rows::<Row>::<R::M>(ctx, ..) }?` (`IntRem` → `IntMod`, `FloatRem` → `FloatMod`). The handler passes `ctx` and the `#[inline]` function drops it; `step_failed` and the handlers' result type do not change | `bench/run.sh` on the parent and on the commit (macOS, clean tree); the differing rows go into the message; no gate | - |
| U5b | `StrNe, StrCharAt, StrSlice, StrPush, StrFindFrom, CharShow, CharDisplay, ByteDisplay, StringDecode` (9); `ByteDisplay` joins the `IntShow` line | - | - |
| U5c | `BytesEmpty, BytesAppend, BytesAppendSlice, BytesSlice, BytesTruncate, BytesOfString, BytesEq, BytesNe, BytesCompare, BytesFindFrom, BytesMake, BytesWithCapacity, BytesSet, BytesPush, BytesFindByte` (15) | - | - |
| U5d | `CharOfInt, ByteOfInt, ByteNeg, ByteAdd, ByteSub, ByteMul, ByteWrap, IntAnd\|ByteAnd, IntOr\|ByteOr, IntXor\|ByteXor, IntComplement, ByteComplement, IntShl, IntShr, ByteShl, ByteShr, CheckIndex` (20) | - | - |
| U5e | `FloatCompare, FloatShowsMinus, FloatOfInt, FloatTruncate, FloatFloor, FloatCeil, FloatRound, FloatSqrt, FloatAbs, FloatSin, FloatCos, FloatExp, FloatLog, FloatPow, FloatInfinity, FloatNan, FloatToFixed, FloatCanParse, FloatParseRaw` (19) | new test `entries::tests::x_float_infinity_takes_unit`; the x_ filter → `9 passed` | `i_f`, `if_i` |
| U5f | generic: `ArrayMake\|MutArrayMake, MutArrayEmpty, ArrayThaw, MutArrayFreeze, UnboxedArrayMake{Float,Int}\|MutUnboxedArrayMake{Float,Int}` (one line per kind), `MutUnboxedArrayEmpty, MutUnboxedArrayFill{Float,Int}, UnboxedArrayThaw, MutUnboxedArrayFreeze` (14); dedicated: `ArrayLength\|MutArrayLength, ArrayGet\|MutArrayRead, ArraySet\|MutArrayWrite, UnboxedArrayLength\|MutUnboxedArrayLength, UnboxedArrayGet{Float,Int}\|MutUnboxedArrayRead{Float,Int}, UnboxedArraySet{Float,Int}\|MutUnboxedArrayWrite{Float,Int}, StrByteLength, StrByteAt, BytesLength\|ShowBytesLength, BytesGet\|ShowBytesGet` (22) | see U5f below | `ii_f`, `iif_i` |
| U5g | `Open, FileOutCreate, FileOutAppend`; `ReadAll, ReadLine, ReadAllBytes, ReadLineBytes` and the same four prefixed `Stdin` and `Proc` (12); `ReadChunk, StdinReadChunk, ProcReadChunk`; `Close, FileOutCloseRaw, StdinClose, StdoutClose, StderrClose`; `StdinOpen, StdoutOpen, StderrOpen`; `{FileOut,Stdout,Stderr,Proc}Write{,Bytes,Line}` (12); `StdoutFlush, FileOutFlush` (40) | ASan: `nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=address CARGO_TARGET_DIR=target/asan cargo test -Zbuild-std --target $(rustc -vV \| sed -n 's/^host: //p') -p eml_vm --test integration -- io:: workers::` → `test result: ok` | - |
| U5h | `EnvArgCount, EnvArg, EnvHas, EnvLookup, ProcSpawn, ProcWait, ProcStageCount, ProcWaitStage, ProcCloseStdin, FailIo, ChanToCell\|AsyncJobCell` (12). `FailIo` is `fn FailIo(ctx, kind: enum, message: obj) -> never { .. }` with the body of `vm.rs:4535-4541`; `ChanToCell \| AsyncJobCell` is an ordinary line (`Ok(a)`) | new test `entries::tests::x_fail_io_parks_the_raised_failure`; the x_ filter → `10 passed` | - |
| U5i | `Tls{PushTls, WritePlain, PushEof, SendCloseNotify, PullTls, ReadPlain, Status, ErrorMessage, LineReady, TakeLine}` (10; each line is one call of `eml_rows::tls::*`) and the 15 non-async `Gui*` rows (25 in all). Each Gui line passes its own variant to `gui::arm`, unchanged from HEAD: `fn GuiPollRaw(ctx, ui: obj) -> int { Ok(gui::arm(ctx, Extern::GuiPollRaw, &[ui])?.bits() as i64) }`; an `int` argument goes in as `Word::from_bits(n as usize)`; `GuiCloseUi` → `()`. Then narrow the visibility (D-21) | GS line 15's Windows `.#cross` command with `eml_cli/gui-native` → exit 0 | - |
| U5j | close the list: see below | - | - |

**U5f.** The dedicated ops `Op::{ArrayGet, ArrayLength, UnboxedGet, UnboxedLength, UnboxedSet, StrByteAt, StrByteLength, BytesGet, BytesLength, ArraySet}` call the row functions; `Op::UnboxedGet` / `Op::UnboxedSet` call the Int row function (D-14); the Float unboxed lines convert bits in their own list line. New test `eml_rows` `rows::tests::unboxed_float_rows_keep_their_bits`: through `call`, `UnboxedArraySetFloat` then `UnboxedArrayGetFloat` on a 2-element array with a NaN whose payload is not the canonical one; the bits must equal those of the Int rows on the same array. This test stands in for a VM call of the Float unboxed lines, which the VM never makes. Extra completion: `cargo nextest run -p eml_rows --lib rows::` → `4 passed`; both eml_vm Miri runs → ok within 300 s / 400 s; T-B (U5f row) with its rule.

**U5j: close the list.**
- Purpose: delete `pending` from the list, from `defs` and from `entries`'s public arm; `run_extern` becomes the prelude plus `call`.
- Work (TDD):
  1. Strengthen `rows::tests::shared_lists_each_row_with_a_body_once`: as a set, `SHARED` equals `Extern::ALL` filtered by `!is_async()`, minus `Par`, `Exit`, `Panic`: 230 rows, no duplicate.
  2. Add `rows::tests::rows_that_do_not_fault_return_ok_on_an_exhausted_budget`: a fast `Ctx` whose `Control` has `max_bytes` = `soft_bytes` = 0; a positive control asserted before the rows run (`bytes_make(0, 0)` on that `Ctx` → `Err(.. MemoryLimit ..)`); through `call`, every row of D-20's computed set on arguments from one table → `Ok`; the table's rows equal the computed set. Fixtures that set a stop bit while they are built are intended. A row that returns `Err` because of a stop bit is a stop-and-report finding, not a reason to change the test.
  3. Delete `pending`.
  4. Docs: spec/vm.md 「基本操作と extern」 (the generic `Extern` instruction runs `Panic` / `Exit` itself and every other row through `eml_rows::rows::call`; the dedicated instructions call their `#[inline]` row function); the comment at `emit.rs` L1062 points at the list; the `run_extern` comment cites vm.md; testing.md 「`eml_rows`のテスト」 and 「`eml_nrt`の検証」 (the `x_*` list; the re-measured Miri `user`); architecture.md (the `eml_rows` bullet; in the `eml_vm` bullet the sentences "the match arms call `Ctx` methods" and "`vm.rs` has no cfg"); the CLAUDE.md `eml_rows` line.
- Completion criteria:
  - G; GS; all G-grep lines through U5j.
  - `cargo nextest run -p eml_rows --lib rows::` → `5 passed`. The x_ filter → `10 passed`.
  - `cargo nextest run -j 1 -p eml_rows -p eml_nrt -E 'test(rows::) | test(entries::)'` → no `SLOW` line.
  - Both eml_nrt Miri runs → ok, with `user` (`/usr/bin/time -p`) written into testing.md. Both eml_vm Miri runs → ok within 300 s / 400 s.
  - T-B (U5j row) with its rule.

Left to the implementer in U5: the wording of the lines within the row order; nothing else.

### U6: shared C pieces and `empty_mut_array`

- Purpose: `eml_rows::ffi::{CHandle, HandleCell, Raise, Dtor, handle_new}` and `eml_rows::constants::empty_mut_array`, used by both engines.
- Targets:
  - New `crates/eml_rows/src/ffi.rs` with `pub use eml_cabi::Dtor;`: `CHandle` (private fields; `new(ptr, dtor)`, `ptr()`, a clone of the cell for a `blocking` job), `HandleCell` with its `Drop`, `Raise { Narrowing { arg, int, value }, NullHandle }` with `text(&self, name: &str)`, moved from `eml_vm/src/ffi.rs:21-69`; and
    `pub unsafe fn handle_new<M: Mode>(ctx: &mut Ctx<M>, ptr: i64, dtor: Option<Dtor>) -> Option<Word>`: `None` for 0 without building anything; else it builds the `CHandle` first and then calls `alloc_foreign(handle, true)`. `# Safety` is `Ctx::heap_mut`'s.
  - `eml_rows` gains `eml_cabi`; `Cargo.lock`; the script: U6's delta and its `REEXPORTS` line.
  - `crates/eml_vm/src/ffi.rs`: `unmarshal`'s `Handle` arm calls `handle_new` (`None` → `Raise::NullHandle`); `raise` calls `why.text(&function.name)`; the pointer and the cell are read through accessors; the cancelled `blocking` path drops a cell through a public constructor and builds no object.
  - `eml_rows/src/constants.rs`: `pub fn empty_mut_array<M: Mode>(heap: &mut Heap<M>) -> Word` (`alloc_array_immortal(true)`), moved from `eml_vm/src/compile/constants.rs:112-118`, which now calls it. `compile/mod.rs` is not touched (G-17).
  - Docs: spec/vm.md 「基本操作と extern」 first bullet, 「橋渡しの関数」 the `handle_new` sentence; architecture.md, the `eml_rows` bullet; future/ffi.md L102 (`eml_cabi`'s users gain `eml_rows`) and L119 (`CHandle` is `eml_rows::ffi`'s); the CLAUDE.md `eml_rows` line.
- Work (TDD):
  1. `eml_rows` tests, which fail to compile at first and name no engine crate (rule 5): `ffi::tests::a_null_pointer_makes_no_handle` (`handle_new(ctx, 0, Some(count))` → `None`; the dtor count stays 0; the census is empty); `ffi::tests::a_handle_runs_its_dtor_once` (`Some(w)`, then `decref(w)` → count 1); `ffi::tests::the_raise_texts_name_the_function` (texts byte-equal to HEAD's); `constants::tests::the_empty_mut_array_is_immortal_lin_and_empty`.
  2. Move the pieces.
  3. The VM's `ffi::` / `ffi_abandon::` tests and the `run-fail/ffi` UI files are the spec.
- Left to the implementer: the accessor names of `CHandle`; wording.
- Completion criteria:
  - G; GS; G-grep (U6).
  - `cargo nextest run -p eml_rows --lib -E 'test(/^ffi::/) | test(/^constants::/)'` → `4 passed`; the same with `-j 1` → no `SLOW` line.
  - `cargo nextest run -p eml_vm --test integration -E 'test(ffi::) | test(ffi_abandon::)'` → no `failed`.
  - ASan: `nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=address CARGO_TARGET_DIR=target/asan cargo test -Zbuild-std --target $(rustc -vV | sed -n 's/^host: //p') -p eml_vm --test integration -- ffi:: ffi_abandon::` → `test result: ok`. TSan: `nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=thread TSAN_OPTIONS=halt_on_error=1 CARGO_TARGET_DIR=target/tsan cargo test -Zbuild-std --target $(rustc -vV | sed -n 's/^host: //p') -p eml_vm --test integration -- ffi:: ffi_abandon::` → `test result: ok`.
  - `bash nix/crate-graph.sh --check` → exit 0. Licences check → exit 0. `nix build` → exit 0.
- Then CP-B.

### 8a: imports on every target

- Purpose: settle on the four CI targets, before any C lowering exists, that `func_addr` of a `Linkage::Import` function registered with `JITBuilder::symbol` gives its address (Q-2) and that a direct `call` with `AbiParam::new(I32).sext()` follows the C ABI (Q-3). Verified so far on aarch64 macOS only.
- Targets: `crates/eml_native/tests/abi.rs` only.
- Work: the test `abi::an_import_gives_its_address`:
  - build with `eml_native::abi::jit_builder()`, registering local Rust `extern "C"` functions: `symbol("eml_cd$M.H", dtor)` with `dtor: extern "C" fn(*mut c_void)`; `symbol("eml_c$M.add", add)` with `add: (i32, f64, i32) -> i32`; `symbol("eml_c$M.sum12", sum12)` with the shape `(i32, i64, i32, usize, i32 × 6, f64, f64) -> i64`;
  - declare the imports with `Linkage::Import` inside the test (it is not under `src`, so G-grep is unaffected);
  - assert: `func_addr` of `eml_cd$M.H` equals `dtor as usize`; `eml_c$M.add`, with `.sext()` on the `I32` params and result, returns the right value for `(-5, 0.5, -7)`; `eml_c$M.sum12` returns its weighted sum with negative `i32` arguments.
- Left to the implementer: the weights of `sum12`.
- Completion criteria:
  - `cargo nextest run -p eml_native -E 'test(=abi::an_import_gives_its_address)'` → `1 test run: 1 passed`.
  - G; G-grep.
- Then CP-C. If the address assertion is red on a target: Q-2. If a `.sext()` assertion is red: Q-3.

### 7a: the empty `MutArray` datum

- Purpose: `Prim::EmptyMutArray` lowers to a load of the load-time datum `a:mut` (M-5, G-18). `unsupported` still rejects it in this task.
- Targets:
  - `eml_native`: `lower/consts.rs` (`Datum::EmptyMutArray`), `lower/names.rs` (`EMPTY_MUT_ARRAY = "a:mut"`), the `Rhs::Prim(Prim::EmptyMutArray, ..)` arm in `lower/function.rs`, `jit.rs` `Tables::new`.
  - `eml_nrt`: `image.rs` (`DATUM_EMPTY_MUT_ARRAY`); `run.rs` `fill_data` and `constants.rs` call `eml_rows::constants::empty_mut_array(heap)`.
  - spec/native.md 「値と定数」.
- Work (tests first): `data::an_empty_mut_array_is_one_immortal_datum` (`eml_native`; LIR loaded directly with `NativeMachine::load`; two functions give the same word, the array's length is 0, the census is clean); `constants::an_empty_mut_array_datum_is_immortal` (an `eml_nrt` unit test, run under Miri `constants::`).
- Left to the implementer: wording.
- Completion criteria:
  - G; G-grep.
  - `cargo nextest run -p eml_nrt -p eml_native` → exit 0, no `failed`.
  - Both eml_nrt Miri runs → `test result: ok`.
  - `git diff --exit-code HEAD -- crates/eml_cli/tests/snapshots` → exit 0.

### 7b: lower every non-async std row

- Purpose: `lowering` by rule (M-1); `Form::StdRow` deleted; the `unlowered` list (D-31).
- First step: run FL and write its two output lines into section 9 of this plan in this commit.
- Targets:
  - `crates/eml_native/src/lower/rows.rs`: `lowering(ext)` is `Inline(i)` if `ext` is in `INLINE`, else `Called` if `!ext.is_async() && ext != Extern::Par`, else `None`. `CALLED` is deleted; `called_row` is unchanged.
  - `unsupported.rs`: `Form::StdRow` and its arm are deleted; `Rhs::Prim(Prim::EmptyMutArray, ..)` notes nothing.
  - `crates/eml_cli/src/lib.rs` `rejected_what`: the `StdRow` arm is deleted.
  - `crates/eml_cli/tests/ui.rs`: `NativeTally` gains `rows: HashSet<Extern>` (`Extern` is not `Ord`), the union over accepted files of the rows of every `Stmt::Let { rhs: Rhs::Extern { ext: ExternRef::Std(row), .. }, .. }`; `render` for `run` appends the `unlowered` section (D-31, rows in `Extern::ALL` order); the windows rule of D-33.
  - `crates/eml_cli/Cargo.toml` `[dev-dependencies]` gains `eml_extern.workspace = true` (a dev edge; the script's table is unchanged).
  - The kind-1 test edits of section 6 and the partition test.
  - Docs: spec/native.md 「行の下ろし方」 (232 entries: 230 generated plus `Exit` and `Panic`; 45 `Inline` rows whose entries generated code does not call; 187 `Called`; none for `Par` or task rows; no `eml_x_` symbol besides `eml_x_c_*`), 「externの失敗」 ("every `Called` row"), 「断る形」 (without `StdRow` and `CALLED`; `empty_mut_array` is not `Prim`); testing.md 「ネイティブで流す」 (step 3 per D-33; the `unlowered` section), 「`eml_native` の検証」 (the partition test); spec/diagnostics.md only if `grep -n 'std row' docs/spec/diagnostics.md` hits.
- Work (TDD):
  1. The kind-1 edits and the partition test first; they fail against `CALLED`.
  2. `rows.rs`, `unsupported.rs`, `rejected_what`.
  3. The harness.
  4. Full `ui::run_native` and `ui::run_fail_native`.
- The partition test `rows::the_rows_are_partitioned` (`crates/eml_native/tests/rows.rs`), over `Extern::ALL`: async rows and `Par` have `lowering == None` and no entry `eml_x_<Variant>`; `INLINE` rows are `Inline`, all others `Called`; each `Inline` or `Called` row has exactly one entry `eml_x_<Variant>` whose `(params, result)` equals the row's Reprs (`Float` → `F64`, all others `I64`; `Never` → no result); the counts are 45 / 187 / 1 / 101; exactly 232 entry symbols start with `eml_x_` and not with `eml_x_c_`.
- Split rule (D-36). If at most 5 files disagree after step 4, 7b is one commit. Otherwise:
  - 7b1: the new `lowering`, `CALLED` deleted, the harness, the partition test, the kind-1 edits; `const NOT_YET_NATIVE: &[Extern]` in `unsupported.rs` holds every row HEAD's `lowering` did not lower, and `Form::StdRow` stays for those rows; `Prim::EmptyMutArray` leaves `Prim`.
  - 7b2..7b6: one commit per family, each removing its rows from the list. Families by the module of `row().name`: F1 `Std.Int`, `Std.Byte`, `Std.Char`, `Std.Float`, `Std.Bool`, `Std.String` and the `Prelude` scalar / show rows; F2 `Std.Bytes`; F3 `Std.Array`, `Std.MutArray`, `Std.UnboxedArray`, `Std.MutUnboxedArray`, `Prelude.check_index`; F4 `Std.FileIn`, `Std.FileOut`, `Std.Stdin`, `Std.Stdout`, `Std.Stderr`, `Std.Env`, `Std.Fs`, plus `Prelude` I/O, `exit`, `panic`, `fail_io`; F5 `Std.Proc`.
  - The last commit deletes `NOT_YET_NATIVE` and `Form::StdRow`.
  - Each stage commit passes G and K; only the final state carries the exact numbers below.
- Disagreements: a file whose native run differs from the VM is a bug. Fix it in the same commit with a regression test (an `agree` on eml source in `crates/eml_native/tests`). Stop and report if more than 10 files still disagree after one fixing pass, or if a cause cannot be found.
- Left to the implementer: the fixes; the task-row program for `the_first_function_and_row_are_reported`; Japanese wording.
- Completion criteria (final state):
  ```sh
  counts run_native        # accepted 320/skipped 6/rejected 310/  async row 129/  c row 6/  par 22/  prim 153
  counts run_fail_native   # accepted 41/checked only 0/skipped 2/rejected 38/  async row 33/  c row 2/  par 1/  prim 2
  grep '^unlowered' crates/eml_cli/tests/snapshots/integration__ui__run_native.snap   # unlowered 84
  rows
  # ReadChunk ReadAllBytes ReadLineBytes ByteGt ByteToInt ByteOfInt FloatTruncate FloatFloor FloatCeil FloatSqrt FloatAbs FloatSin FloatCos FloatExp FloatLog BoolNe StrPush BytesTruncate BytesWithCapacity BytesSet BytesPush ShowBytesLength ShowBytesGet StringDecode StdinReadChunk StdinReadAllBytes StdinReadLineBytes StderrWriteLine ProcStageCount ProcWaitStage ProcWrite ProcWriteBytes ProcReadChunk ProcReadAllBytes ProcReadLineBytes ProcReadLine ArraySet ArrayThaw UnboxedArrayLength UnboxedArrayThaw MutUnboxedArrayEmpty MutUnboxedArrayLength MutUnboxedArrayFreeze UnboxedArrayMakeFloat UnboxedArrayMakeInt UnboxedArrayGetFloat UnboxedArrayGetInt UnboxedArraySetFloat UnboxedArraySetInt MutUnboxedArrayMakeFloat MutUnboxedArrayMakeInt MutUnboxedArrayReadFloat MutUnboxedArrayReadInt MutUnboxedArrayWriteFloat MutUnboxedArrayWriteInt MutUnboxedArrayFillFloat MutUnboxedArrayFillInt AsyncJobCell ChanToCell TlsPushTls TlsPushEof TlsPullTls TlsWritePlain TlsReadPlain TlsStatus TlsErrorMessage TlsSendCloseNotify TlsLineReady TlsTakeLine GuiCloseUi GuiBeginWindowRaw GuiBeginGridRaw GuiEndRaw GuiNextCellRaw GuiCreateRaw GuiAddItemRaw GuiSetTextRaw GuiSetIntRaw GuiCloseWindowRaw GuiOpenWindowsRaw GuiFlushRaw GuiPollRaw GuiEventIntRaw GuiEventTextRaw
  cargo nextest run -p eml_native -E 'test(=rows::the_rows_are_partitioned)'          # 1 test run: 1 passed
  cargo nextest run -p eml_vm --test integration bench::                               # exit 0, no `failed`
  ```
  Also: G; G-grep (through "7b's last commit"); K; T below 1000; B below 5.000 (else Q-4). If a number differs, apply FL's rule.
- Pitfalls: `FloatTruncate`, `MutUnboxedArrayMakeFloat`, `MutUnboxedArrayReadFloat` occur in the C UI files, which stay rejected until 8d, so they are in the list here. `c row` rises from 2 to 6 because `std row` no longer precedes it.

### 7c, 7d, 7e: UI files for the 53 rows

- Purpose: every row of 7b's `unlowered` list except D-32's residue is contained in an accepted `run/` file (G-19).
- Rules for each file:
  - It goes under `tests/ui/run/<category>/` (testing.md 「分類」) with no marks. Inputs are sibling `.stdin` or `.args` files. A child process is the test file itself, run with arguments (as `run/std/proc_spawn.em` does). Each file runs under 1 s in `ui::run`. Output is deterministic.
  - Avoid handlers, `try_panic`, `try_io`, `par`, `Async`, and the std functions that call `try_panic`: `show` on `Bytes`, `String.from_utf8`, `Array.from_list` / `to_list`, `UnboxedArray.from_list` / `to_list`, `Proc.wait_all`, `.|`.
  - A file name must not match a std module name case-insensitively (E1026).
  - Acceptance check: `timeout 20 eml run --native <file>` at the 7b tree prints no E0004. If it prints one whose text includes `first in Prelude.try_panic@...{0=<X>...}`, the std function `<X>` is what to avoid.
- Trial programs. All 8 shapes below were accepted by a build with 7b's acceptance rule and reach the 53 rows [V]. They are shapes, not finished tests. Each starts, after its imports, with `main : Unit -> <IO, Panic> Unit` and `main () =`.
  ```
  -- float (FloatTruncate Floor Ceil Sqrt Abs Sin Cos Exp Log)
  import Float
    println (show (Float.truncate 2.5 + Float.floor 1.5 + Float.ceil 1.5))
    println (show (Float.sqrt 2.0 + Float.abs (0.0 - 1.0) + Float.sin 1.0 + Float.cos 1.0 + Float.exp 1.0 + Float.log 2.0))

  -- byte (ByteGt ByteToInt ByteOfInt BoolNe)
  import Byte
    let a = Byte.of_int 7
    let b = Byte.of_int 9
    println (show (Byte.to_int a))
    println (show (b > a))
    println (show (True != False))

  -- string (StrPush StringDecode)
  import String
  import Bytes
    println (String.push 'c' "ab")
    println (String.decode (Bytes.of_string "xyz"))

  -- bytes (BytesWithCapacity BytesPush BytesSet BytesTruncate)
  import Bytes
  import Byte
    let b = Bytes.with_capacity 4
    let b = Bytes.push (Byte.of_int 1) b
    let b = Bytes.push (Byte.of_int 2) b
    let b = Bytes.set 0 (Byte.of_int 5) b
    let b = Bytes.truncate 1 b
    println (show (Bytes.length b))
    println (show (Byte.to_int (Bytes.get 0 b)))

  -- arrays (ArraySet ArrayThaw)
  import Array
  import MutArray
    let a = Array.make 3 0
    let a = Array.set a 1 5
    println (show (Array.get a 1))
    let m = Array.thaw a
    let (n, m) = MutArray.length m
    println (show n)
    println (show (Array.length (MutArray.freeze m)))

  -- unboxed (the 19 UnboxedArray* / MutUnboxedArray* rows)
  import UnboxedArray
  import MutUnboxedArray
    let a : UnboxedArray Int = UnboxedArray.make 3 1
    let a = UnboxedArray.set a 0 4
    println (show (UnboxedArray.get a 0 + UnboxedArray.length a))
    let f : UnboxedArray Float = UnboxedArray.make 2 0.5
    let f = UnboxedArray.set f 1 1.5
    println (show (UnboxedArray.get f 1))
    let m = UnboxedArray.thaw f
    let m = MutUnboxedArray.write m 0 2.5
    let (x, m) = MutUnboxedArray.read m 0
    let m = MutUnboxedArray.fill m x
    let (n, m) = MutUnboxedArray.length m
    println (show n)
    println (show (UnboxedArray.length (MutUnboxedArray.freeze m)))
    let i : MutUnboxedArray Int = MutUnboxedArray.make 2 3
    let i = MutUnboxedArray.write i 0 4
    let (y, i) = MutUnboxedArray.read i 0
    let i = MutUnboxedArray.fill i y
    let (k, i) = MutUnboxedArray.length i
    println (show k)
    let e : MutUnboxedArray Float = MutUnboxedArray.empty ()
    let fm : MutUnboxedArray Float = MutUnboxedArray.make 1 0.0
    let (z, fm) = MutUnboxedArray.read fm 0
    println (show z)
    println (show (UnboxedArray.length (MutUnboxedArray.freeze e)))
    println (show (UnboxedArray.length (MutUnboxedArray.freeze fm)))
    println (show (UnboxedArray.length (MutUnboxedArray.freeze i)))

  -- io (ReadChunk ReadAllBytes ReadLineBytes, Stdin* x3, StderrWriteLine)
  import Stdin
  import Stderr
  import FileIn
  import Bytes
    let s = Stdin.open ()
    let (c, s) = Stdin.read_chunk (Bytes.empty ()) s
    let (l, s) = Stdin.read_line_bytes s
    let (r, s) = Stdin.read_all_bytes s
    Stdin.close s
    println (show (Bytes.length c + Bytes.length r))
    let e = Stderr.open ()
    let e = Stderr.write_line "x" e
    Stderr.close e
    let f = FileIn.open "<input file>"
    let (c2, f) = FileIn.read_chunk (Bytes.empty ()) f
    let (l2, f) = FileIn.read_line_bytes f
    let (r2, f) = FileIn.read_all_bytes f
    FileIn.close f
    println (show (Bytes.length c2 + Bytes.length r2))

  -- proc (ProcWrite ProcWriteBytes ProcReadChunk ProcReadAllBytes ProcReadLineBytes ProcReadLine)
  import Proc
  import Bytes
    let c = Proc.spawn <the test file itself as child>
    let c = Proc.write "a" c
    let c = Proc.write_bytes (Bytes.of_string "b\n") c
    let c = Proc.close_stdin c
    let (l, c) = Proc.read_line c
    let (lb, c) = Proc.read_line_bytes c
    let (k, c) = Proc.read_chunk (Bytes.empty ()) c
    let (r, c) = Proc.read_all_bytes c
    println (show (Proc.wait c))
  ```
  Rejected trial [V]: `Proc.wait_all` is rejected with `Prim`, `Clause`, `Prompt`, first in `Prelude.try_panic@[Int]{0=Std.Proc.next_stage$lambda0/1}`; `run/bytes/get_set.em` is rejected through `Prelude.Show Bytes.show`. Hence the four non-task residue rows.
- Commits (file names are proposals):
  - 7c `Cover scalar, string and byte rows natively` (19 rows): `run/float/math_functions.em`; `run/byte/conversions_and_compare.em` (with `BoolNe`); `run/strings/push_and_decode.em`; `run/bytes/build_and_edit.em`.
  - 7d `Cover array rows natively` (21 rows): `run/arrays/set_and_thaw.em`; `run/unboxed/rows_without_lists.em` (may be split by `Int` and `Float`).
  - 7e `Cover I/O rows natively` (13 rows): `run/files/read_bytes.em` with a sibling input file; `run/std/stdin_bytes.em` with `.stdin`; `run/std/stderr_write_line.em`; `run/std/proc_pipes.em`. Doc in 7e: testing.md 「ネイティブで流す」 gains the residue rule: the rows left in the list are contained only in std functions that also contain a task row or a `try_panic` call.
- Work per commit: 1. write the files; 2. `cargo nextest run -p eml_cli --test integration -E 'test(=ui::run) | test(=ui::run_native)'`; 3. `cargo insta accept`; 4. check K.
- Left to the implementer: the file names and categories within testing.md 「分類」; the programs' values; whether `rows_without_lists.em` is split.
- Completion criteria per commit: G; G-grep; K; `rows` lists none of the commit's target rows; `counts run_native` starts `accepted <320 + F>`, F = the `run/` files added since 7b.
- After 7e:
  ```sh
  grep '^unlowered' crates/eml_cli/tests/snapshots/integration__ui__run_native.snap   # unlowered 31
  rows
  # ShowBytesLength ShowBytesGet ProcStageCount ProcWaitStage AsyncJobCell ChanToCell TlsPushTls TlsPushEof TlsPullTls TlsWritePlain TlsReadPlain TlsStatus TlsErrorMessage TlsSendCloseNotify TlsLineReady TlsTakeLine GuiCloseUi GuiBeginWindowRaw GuiBeginGridRaw GuiEndRaw GuiNextCellRaw GuiCreateRaw GuiAddItemRaw GuiSetTextRaw GuiSetIntRaw GuiCloseWindowRaw GuiOpenWindowsRaw GuiFlushRaw GuiPollRaw GuiEventIntRaw GuiEventTextRaw
  ```
  Also T below 1000 and B below 5.000.
- If a row cannot be covered or a residue row turns out to be reached: Q-1.
- Then CP-7.

### 8b: C helper and pointer entries

- Purpose: the entries the C lowering calls, with their Miri tests, before the lowering exists.
- Targets:
  - `crates/eml_extern/src/c.rs`: `CInt::code` (`I64` 0, `I32` 1, `SizeT` 2) and `CInt::from_code`.
  - `crates/eml_nrt/src/entries.rs`: the kind token `dtor` (Rust `Option<eml_rows::ffi::Dtor>`, machine `I64`, argument only), next to `ptr`. `Option` of a function pointer has the pointer's ABI with null as `None`, so no cast and no `transmute` exists anywhere; `eml_nrt` gains no `eml_cabi` edge.
  - New `crates/eml_nrt/src/c.rs`, table `C` with no prefix:
    - `shim fn eml_x_c_narrow(ctx, name: obj, arg: int, int: int, value: int) -> never`: parks `Raise::Narrowing { .. }.text(name)` through `worker::park_panic`; `int` is a `CInt` code.
    - `shim fn eml_x_c_handle_ptr(ctx, handle: obj) -> i64`: `foreign::<CHandle>`; an `Err` is the internal class.
    - `shim fn eml_x_c_handle_new(ctx, name: obj, dtor: dtor, ptr: i64) -> obj`: calls `unsafe { eml_rows::ffi::handle_new(ctx, ptr, dtor) }`; `None` → `park_panic(Raise::NullHandle.text(name))` and the placeholder. Its doc comment carries this `# Safety` (the same sentence goes into spec/native.md in 8d): `dtor` is `None` or a function that may be called as `void f(void*)` on `ptr` for the life of the process; generated code passes 0 or the address of the import `eml_cd$<handle type name>`, which `load` binds only to an element of `Linked::dtors()` after `Linked::fits` held.
    - `name` is borrowed (an immortal string): the helpers read it with `Heap::string` and release nothing. No `rows::call`, no table read (G-C14).
  - `crates/eml_nrt/src/heap.rs`: `eml_rt_text_payload`, `eml_rt_unboxed_ptr`, `eml_rt_unboxed_ptr_mut`, each `(ctx, w: obj) -> i64`, fatal-only, returning the exposed address (as `eml_vm/src/ffi.rs:117-121` does). One `Heap` read each: no allocation, no free, no count change (G-13).
  - `lib.rs` `TABLES` and `trust.rs` (the `c.rs` row).
  - `crates/eml_native/tests/abi.rs`: the echo `eml_t_echo_iiii` for `(I64, I64, I64, I64) -> ()`, plus any other shape `mixed_int_and_float_arguments_reach_an_entry` reports.
  - Docs: spec/native.md 「エントリのABI」 (`dtor`), 「externの失敗」 (the three helpers), 「ヒープのエントリ」 (the pointer entries); architecture.md, the `eml_nrt` bullet (`c.rs`, table `C`); CLAUDE.md, the `eml_nrt` line (the tables gain `C`).
- Work (tests first, `entries::tests::*` in `crates/eml_nrt/src/entries.rs`): `x_c_narrow_parks_the_narrowing_panic` (text = `Raise::Narrowing { arg: 2, int: CInt::I32, value }.text(name)`; fault `Engine`); `x_c_handle_new_runs_the_dtor_once`; `x_c_handle_new_of_null_parks_the_null_handle_panic` (no dtor call); `x_c_handle_ptr_reads_the_pointer`; `rt_text_payload_points_at_the_bytes`; `rt_unboxed_ptr_points_at_the_elements`; `rt_unboxed_ptr_mut_points_at_the_elements`; `entries_take_a_dtor_kind` (the descriptor of `eml_x_c_handle_new` is `(I64, I64, I64) -> I64`). In `eml_extern`: `cint_codes_round_trip`.
- Left to the implementer: wording.
- Completion criteria:
  - G; G-grep (8b).
  - `cargo nextest run -p eml_nrt --lib -E 'test(/x_c_|rt_text_payload|rt_unboxed_ptr|dtor_kind/)'` → `8 tests run: 8 passed`.
  - `cargo nextest run -p eml_extern -E 'test(cint_codes_round_trip)'` → `1 test run: 1 passed`.
  - `cargo nextest run -p eml_native` → exit 0, no `failed`.
  - Both eml_nrt Miri runs → `test result: ok`; the `user` times go into the commit message; if either exceeds 10 s, update the figure in testing.md 「`eml_nrt`の検証」.
- Pitfalls: if Q-2 fired at CP-C, apply its revision here first.

### 8c: `eml_clink::c_signature` and one import declarer

- Purpose: one helper builds a C function's Cranelift signature for the bridges and for native code (M-4, AM-6); one function declares every import.
- Targets:
  - `crates/eml_clink/src/` (the module holding `bridge.rs`'s signature code): `pub fn c_signature(signature: &CSignature, call_conv: CallConv) -> Signature`, with `.sext()` on `I32` params and result. `bridge.rs` builds its callee signature with it.
  - `crates/eml_native/src/abi.rs`:
    ```rust
    pub enum Import<'a> { Entry(&'a Entry), C(&'a CFunction), Dtor(&'a CHandleType) }
    pub fn declare_import<M: Module>(module: &mut M, import: Import<'_>) -> Built<FuncId>;   // the one `Linkage::Import`
    ```
    `declare_import` replaces `declare_entry` and calls `eml_clink::c_signature` for `Import::C`; a dtor's signature is `(i64)`.
  - `lower/mod.rs`: the caller. `lower/names.rs`: `c_function(name) = "eml_c$<name>"`, `c_dtor(handle) = "eml_cd$<handle type name>"`.
  - `crates/eml_native/Cargo.toml`: a normal edge to `eml_clink`; `nix/crate-graph.sh`'s `eml_native` row becomes `eml_cabi eml_clink eml_extern eml_lir eml_nrt eml_rt`.
  - Docs: architecture.md, the `eml_clink` bullet (`c_signature`) and the `eml_native` bullet (`declare_import`, the edge to `eml_clink`); CLAUDE.md, the `eml_native` line (it depends on `eml_clink`) and the `eml_clink` line if it names its contents.
- Work (test first): `c_signature_extends_int32` in `crates/eml_clink/tests/bridge.rs`: `I32` params and result carry `ArgumentExtension::Sext`; other types carry none.
- Left to the implementer: the module of `c_signature` inside `eml_clink`.
- Completion criteria:
  - G; G-grep (8c).
  - `cargo nextest run -p eml_clink -p eml_native` → exit 0, no `failed`.
  - `cargo nextest run -p eml_clink -E 'test(c_signature_extends_int32)'` → `1 test run: 1 passed`.
  - `git diff --exit-code HEAD -- crates/eml_cli/tests/snapshots` → exit 0.
- Pitfalls: if Q-3 fired at CP-C, `c_signature` has no `.sext()` and this test follows the revision.

### 8d: call user C functions natively

- Purpose: a non-`blocking` C row lowers to a direct call; `Form::CRow` is deleted.
- Targets:
  - `jit.rs` `load` uses its `linked` parameter. Before any module work: `if !linked.fits(&program.c_functions, &program.c_handles)` → `Err(LoadError::LinkMismatch)`. Next to the entry symbols: `builder.symbol(c_function(..), call.callee)` per function and `builder.symbol(c_dtor(..), dtor)` per handle type with a dtor. No caller changes.
  - New `lower/c.rs` (added to `eml_native`'s `trust.rs` `FILES`), for `Rhs::Extern(ExternRef::C(id))`, in this order:
    1. Arguments as words of the row's Reprs.
    2. For each `CParam::Int(int)` with `int.narrows()`, in parameter order: `I32`: `sextend(ireduce.i32(v)) == v`; `SizeT`: `v >= 0`. Cold edge: `eml_x_c_narrow(ctx, <the s: datum of the name>, k + 1, code, v)`, then the `Never` path with `raise = Some((site, row.purity))`. `k + 1` is 1-based over all `CFunction.params`, `NoArg` included (D-37).
    3. Pointer and handle arguments, in parameter order: the three `eml_rt_*` entries and `eml_x_c_handle_ptr`. `NoArg` passes nothing.
    4. `call` of `eml_c$<name>` (`I32` arguments through `ireduce`).
    5. Result: `Void` → `Word::UNIT`; `I32` → `sextend`; `I64` / `SizeT` raw; `F64`; `Handle(h)` → `eml_x_c_handle_new(ctx, name, func_addr(eml_cd$<h>) or 0, ptr)`, then the fault load (`After::Faults`).
    Steps 3 and 4 are emitted by one function, `FnLower::lend_and_call`, which takes the already checked scalar values and whose body contains no builder call other than `call_entry` of the four lending entries and the `call` of the import. A statement may reach its unwind cleanup from several cold edges (each narrowing check, the handle fault), as `After::Arith` already does with two.
  - The name datum goes through `lower/consts.rs`'s string datum (the `ConstString` path). No new datum kind.
  - `lower/function.rs`: the `ExternRef::C` arm. `unsupported.rs`: `CRow` is deleted and `ExternRef::C(_) => {}`. `rejected_what` loses `CRow`.
  - `crates/eml_native/tests/common/native.rs`: `call`, `clif` and `visit` link with `eml_test_support::link(&program)` (kind 3); new `agree_source(source: &str)` (`eml_test_support::lir` + `agree_program`); new `clif_linked(text, function, &Linked)`. `tests/main.rs`: `mod c;`.
  - Docs: spec/native.md, new 「Cの関数の呼び出し」 (the order above; the lending window; the stack: C runs on the worker stack with at least 1 MiB below `hard`, and a C function that needs more dies on the guard page; a stop request is seen at the next safepoint after C returns; the imports `eml_c$` / `eml_cd$`, their signatures from `eml_clink::c_signature`, registered with `JITBuilder::symbol` after `fits`; the load error; the `# Safety` sentence of 8b; a C function must not unwind across the call or keep a lent pointer), 「断る形」; spec/vm.md 「C の関数の命令」 (native code lowers the same conversion; cite native.md); spec/object-model.md T16 (native code is a lender in the same window); testing.md 「`eml_native` の検証」; architecture.md (`lower/` gains `c.rs`); CLAUDE.md, the `eml_native` line (user C calls, `lower/c.rs`).
- Work (tests first, `crates/eml_native/tests/c.rs`; each runs `agree_source`: the VM in checked mode against native, same stdout and same result text):
  - `c::an_int32_argument_that_fits`
  - `c::an_int32_argument_that_does_not_fit_panics` (non-`pure`; uncaught, so the VM's text and site)
  - `c::a_pure_row_that_does_not_fit_panics`
  - `c::a_negative_size_t_panics` (sum12, 4th argument -1)
  - `c::twelve_mixed_arguments` (sum12 and add_i32 called in the same function, so one function holds two C calls)
  - `c::an_int32_result_is_sign_extended` (neg, `NoArg`)
  - `c::a_void_function_writes_through_a_lent_array` (fill)
  - `c::a_text_argument` (bytes_sum on `Bytes` and on `String`)
  - `c::an_unboxed_array_argument` (dot)
  - `c::a_handle_round_trip_runs_the_dtor_once` (tag 1; frees(1) = 1; census clean)
  - `c::a_null_handle_panics`
  - `c::a_linked_of_another_program_is_refused` (G-11)
  - `c::the_lending_window_holds_only_lending_entries` (G-12): compile every source above with `NativeMachine::new_with_clif` and its `link`; in each function map `fnN = %<symbol>`. Lending calls are calls of `eml_rt_text_payload`, `eml_rt_unboxed_ptr`, `eml_rt_unboxed_ptr_mut`, `eml_x_c_handle_ptr`. Walk the instructions in layout order with a flag `lent`: set at a lending call, reset after each `call %eml_c$..`. Assert:
    1. no `brif` whose target block calls `%eml_x_c_narrow` is reached while `lent` is set (every narrowing check of a C call precedes that call's first lending call);
    2. the span from a C call's first lending call to its `call %eml_c$..` lies inside one block, and every `call` in it is a lending call;
    3. after `call %eml_c$..`, up to the next `call` or the end of the block, there is no `load` of the fault word (the `v0+32` load of the pins); the exception: when the next call is `%eml_x_c_handle_new`, the instruction after it is that load.
  - The CLIF pin `clif::a_c_row` (inline snapshot), loaded with `clif_linked` and a local `unsafe { eml_clink::Linker::new(Vec::new()).with_symbols(..) }` of two local Rust fns:
    ```
    chandle "M.H" drop "eml_test_c_pin_free" link []
    cfn "M.make" symbol "eml_test_c_pin" link [] (i32, text) -> handle "M.H"
    ```
    (LIR C spellings: `i32 i64 size_t f64 array mut_array text handle "T" unit void`, `crates/eml_lir/src/syntax.rs:252-266, 666-693`.)
- Kind 1: `unsupported::c_row_is_rejected` is deleted.
- Left to the implementer: the eml sources of the `c::` tests over `eml_test_support`'s fixtures; wording.
- Completion criteria:
  ```sh
  counts run_native        # accepted <326 + F>/skipped 6/rejected 304/  async row 129/  par 22/  prim 153
  counts run_fail_native   # accepted 43/checked only 0/skipped 2/rejected 36/  async row 33/  par 1/  prim 2
  grep '^unlowered' crates/eml_cli/tests/snapshots/integration__ui__run_native.snap   # unlowered 31
  cargo nextest run -p eml_native -E 'test(/^c::/)'                                     # 13 tests run: 13 passed
  cargo nextest run -p eml_native -E 'test(=clif::a_c_row)'                             # 1 test run: 1 passed
  cargo nextest run -p eml_clink                                                        # exit 0, no `failed`
  cargo clippy -p eml_cli --no-default-features --features run -- -D warnings          # exit 0
  nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run -- -D warnings   # exit 0
  for p in eml_native eml_nrt eml_clink; do nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=address CARGO_TARGET_DIR=target/asan cargo test -Zbuild-std --target $(rustc -vV | sed -n 's/^host: //p') -p $p -- --skip fatal::; done   # every `test result:` line ok
  ```
  Also: G; G-grep (all lines); K; T below 1000; B below 5.000.
- Pitfalls: a Rust `extern "C"` fixture that panics aborts. A problem with the lending window that the three assertions do not express goes to an advisor before a fix (section 5.1).

### 9: native measurement

- Purpose: N-39: the numeric programs that run natively get rows in `bench/scripts.sh` (D-35).
- Targets (commit `Add native rows to bench/scripts.sh`):
  - `bench/scripts/compare.sh` reads `EML_RUN_FLAGS` (default empty) and inserts it right after `run`.
  - `bench/scripts.sh`: `numeric_row` takes an optional 5th argument `native`; with it the label is `<label> (native)`, `compare.sh` gets `EML_RUN_FLAGS=--native`, and the measured command is `EML_WORKERS=1 "$eml" run --native …`. Four calls after the five VM rows, with the VM rows' sizes:
    - `numeric_row spectral-norm spectral_norm spectral_norm 510 native`
    - `numeric_row mandelbrot mandelbrot mandelbrot 480 native`
    - `numeric_row fannkuch-redux fannkuch_var fannkuch 9 native`
    - `numeric_row sieve sieve_var sieve 3500000 native`
  - benchmarks.md 「スクリプトの計測」 (the `(native)` rows take the VM row's size; the ~300 ms sizing and the resize rule apply to VM rows only) and 「ネイティブの数値」 (one sentence pointing at those rows).
- Left to the implementer: wording.
- Completion criteria:
  - `bash -n bench/scripts.sh && bash -n bench/scripts/compare.sh` → exit 0.
  - `nix shell nixpkgs#shellcheck -c shellcheck bench/scripts.sh bench/scripts/compare.sh` → exit 0.
  - macOS: `bench/scripts.sh > $TMPDIR/scripts.md` → exit 0; the numeric table has 9 rows; `grep -c '(native)' $TMPDIR/scripts.md` → 4.
  - macOS: `bench/native.sh > $TMPDIR/native.md` → 42 program rows; `grep -c '| rejected | rejected |' $TMPDIR/native.md` → 3 (`par_tree`, `nbody`, `ep_float`); `grep -c differs $TMPDIR/native.md` → 0.
  - G.
- Pitfalls: a `differs` row or a `compare.sh` mismatch is a bug: fix it in its own reviewed commit with a regression test. Without macOS the two runs move to `Measure jit-externs`.

### 10: `Close jit-externs`

- Purpose: close the stage (roadmap 「進め方」), one commit.
- Work, in this order:
  1. spec.
     - `grep -rnE 'jit-(externs|effects|tasks|repl)' docs/spec docs/implementation` → empty. Remove any 「今は〜、後で〜」 text from the sections this stage wrote.
     - Amendments; each prints ≥ 1 except the M-3 line, which prints 0:
       ```sh
       sed -n '/^## 行の下ろし方/,/^## /p' docs/spec/native.md | grep -c '232'                     # M-1
       sed -n '/^## 行の下ろし方/,/^## /p' docs/spec/native.md | grep -c 'eml_x_c_'                # M-1
       sed -n '/^### 基本操作と extern/,/^##/p' docs/spec/vm.md | grep -c 'rows::call'            # M-2
       grep -c '`as`の識別子' docs/spec/native.md                                                  # M-3: prints 0
       sed -n '/^## Cの関数の呼び出し/,/^## /p' docs/spec/native.md | grep -c 'eml_cd\$'          # M-4
       sed -n '/^## Cの関数の呼び出し/,/^## /p' docs/spec/native.md | grep -c 'eml_clink::c_signature'   # M-4 / AM-6
       sed -n '/^## 値と定数/,/^## /p' docs/spec/native.md | grep -c 'a:mut'                       # M-5
       grep -c 'the_rows_are_partitioned' docs/implementation/testing.md                          # M-6
       sed -n '/^## Cの関数の呼び出し/,/^## /p' docs/spec/native.md | grep -c 'JITBuilder::symbol' # M-7
       ```
     - Check only: `grep -rnE 'eml_vm.{0,6}gui\.rs|eml_vm.{0,6}tls\.rs' docs/spec docs/implementation docs/future CLAUDE.md` → empty; `grep -nF '`match`の腕で`Ctx`のメソッドを呼ぶ' docs/implementation/architecture.md` → empty; `grep -rn 'shared_rows' docs crates` → only `docs/plans` lines.
  2. Measure. `eml_vm` changed, so: run `bench/scripts.sh` first (a VM row out of range gets its own commit `Resize bench rows`). Then run `bench/run.sh`, `bench/rt.sh`, `bench/scripts.sh` on the clean tree of the last code commit and replace the three tables of benchmarks.md 「今の数値」. Replace 「ネイティブの数値」: the three `nrt_bench` rows and the `bench/native.sh` table; update prose that names rejected programs. Re-read the findings hit by `grep -n 'run_extern\|extern の腕' docs/implementation/benchmarks.md` and re-measure or delete those the stage invalidated.
  3. status.md: native code runs every non-async std row and every non-`blocking` C function; the Tls and Gui rows, `AsyncJobCell` and `ChanToCell` appear only with task rows; `Show Bytes` and `Proc.wait_all` need `try_panic`; L63 loses 「残りのstdの行、利用者のCの関数」.
  4. architecture.md and CLAUDE.md, check only (the structure commits wrote them); fix only what is missing and name it in the commit message:
     ```sh
     grep -n 'LISTED\|seed rows\|shared_rows' CLAUDE.md docs/implementation/architecture.md       # empty
     grep -c 'eml_rows\|eml_clink' CLAUDE.md                                                      # >= 2
     grep -n 'lower/c.rs\|c_signature' docs/implementation/architecture.md                        # >= 1 line each
     ```
  5. roadmap.md and docs/future:
     - delete the `jit-externs` item (L32);
     - in `jit-effects`' 前提, replace `jit-externs` with 「ネイティブのコードのexternの行 ([ネイティブの実行系](../spec/native.md)の「行の下ろし方」)」;
     - rewrite L570: `apply_int` and `apply_float` run natively now;
     - L625 and future/ffi.md L128: drop the stage name (「ネイティブのコードがプラグインをまたいで行ごとに型の付いたエントリを要るとき」);
     - retarget future/ffi.md L107 and future/native.md 「extern のエントリ」 to spec/native.md 「Cの関数の呼び出し」 and 「行の下ろし方」, keeping only what is still future;
     - check only (G-a and U4 wrote them): `grep -c 'eml_rtlib' docs/future/native.md docs/implementation/distribution.md` → ≥ 1 in each; `grep -n 'importからは推さない' docs/future/native.md docs/implementation/distribution.md` → empty; `grep -nE 'eml_sched.{0,20}(rows|ffi|tls\.rs|gui\.rs|extern_rows)' docs/future/*.md` → empty; the roadmap's jit-tasks, jit-repl and native-aot items hold the texts the G-a and U4 cards list.
  6. overview.md and README.md: change only if stale.
  7. Confirm that no decision lives only in this plan (D-1, D-2 "Kept in the repository"; the items of section 10 that were deferred), then delete `docs/plans/2026-11-11-jit-externs.md`. The appendix directory stays until jit-repl's Close.
  8. Commit message: the kind-1 and kind-2 changes with reasons (section 6), the new UI files, the measurements, the build-time record (T-B), where each decision went, and the date of the U-F decision (2026-11-13).
- Left to the implementer: the commit message's wording.
- Completion criteria:
  - G; every G-grep line.
  - `cargo nextest run -p eml_cli --test integration -E 'test(/^citations::/)'` → exit 0, no `failed`.
  - `ls docs/plans | grep -c jit-externs` → `0`.
  - ``grep -c '^- `jit-externs`' docs/future/roadmap.md`` → `0`.
  - `grep -rn 'jit-externs' docs/spec docs/implementation docs/future docs/README.md docs/overview.md CLAUDE.md` → empty.
  - The checks of steps 1, 4 and 5 print what is written.
- Then CP-Z; after it passes the branch is merged.

## 8. Execution order

Strictly sequential; every task uses only the code of the tasks before it.

```
U1 → UT → G-a → G-b → G-c → G-d → U2a → U2b → U3 → U4 → [CP-A]
   → U5a → U5b → U5c → U5d → U5e → U5f → U5g → U5h → U5i → U5j → U6 → [CP-B]
   → 8a → [CP-C]
   → 7a → 7b (or 7b1..7b6) → 7c → 7d → 7e → [CP-7]
   → 8b → 8c → 8d → 9 → 10 → [CP-Z] → merge
```

| Phase | Tasks | What moves | Native UI counts |
|---|---|---|---|
| 0 | U1, UT | plan, baseline, one doc paragraph | 208 / 8 |
| 1 graph | G-a..G-d | crates and edges (kind 3) | 208 / 8 |
| 2 list and VM | U2a..U6 | the list, the VM routing, Tls / Gui / C pieces to `eml_rows` (kind 3 for the VM) | 208 / 8 |
| 3 native std rows | 8a, 7a..7e | lowering by rule; UI coverage | 320 + F / 41 |
| 4 native C | 8b..8d | helpers, imports, the C lowering | 326 + F / 43 |
| 5 close | 9, 10 | measurement, docs | unchanged |

7a..7e may proceed while CP-C's CI runs; 8b waits for both CP-C and CP-7. macOS is needed for U1's baseline, U5a's record, 9 and 10. The checkpoints are the orchestrator's; a failed checkpoint blocks the tasks listed after it.

## 9. Verification plan

- Baseline (U1, at fddd44f2, macOS, clean tree). Known values:

  | Item | Value at base |
  |---|---|
  | `cargo nextest run` | no `failed` on a clean dev profile [I]. Under load with the stale `deps` directory the run was `5106 tests run: 5105 passed, 1 failed, 23 skipped`, the failure being the TLS test of D-38 |
  | `counts run_native` | `accepted 208/skipped 6/rejected 422/  async row 129/  c row 2/  par 22/  prim 156/  std row 113` |
  | `counts run_fail_native` | accepted 8 / checked only 0 / skipped 2 / rejected 71 (async row 33, c row 2, par 1, prim 3, std row 32) |
  | FL | `run: entries 636 none 208 only-std 109 std-or-c 115 skip 6`; `run-fail: entries 81 none 8 only-std 32 std-or-c 34 skip 2` |
  | `tls_off` / `gui_off` | 3 passed |
  | `package(eml_vm) & test(/^(tls\|gui)::/)` | 31 passed |
  | `bench/native.sh` | 42 rows: 27 with numbers, 15 rejected |
  | `native: skip` marks | 8 |
  | T-B (the combined form, one measurement) | 9.36 / 9.52 / 9.47 s |
  | `ui::run_native` / `ui::run_fail_native` (under load) | 1.785 s / 0.783 s |

  U1 writes here: the nextest summary line; T-B-vm and T-B-nrt (three values and the median each); the four Miri `user` times; T's slowest line; B's time. 7b writes here: FL's two lines at its parent commit.

  Baseline values (U1, 2026-11-13, macOS 27.2.1, Mac17,2 Apple M5 arm64, rustc 1.99.1, fddd44f2, clean dev profile: `ls -f target/debug/deps | wc -l` → 3978):

  | Item | Value measured |
  |---|---|
  | `cargo nextest run` | `Summary [58.849s] 5106 tests run: 5106 passed, 23 skipped` |
  | TLS test and `platform::` (`-j 1`) | 4 passed; 0.107 / 0.058 / 0.052 / 0.060 s; no `SLOW` |
  | TLS test `--stress-count 12 -j 12` | 12/12 passed, no `WouldBlock` panic |
  | T-B-vm (clean `eml_sched eml_vm`, build `-p eml_vm --features eml_vm/tls,eml_vm/gui`) | 9.49 / 9.47 / 9.29 s; median 9.47 s |
  | T-B-nrt (clean `eml_sched eml_nrt`, build `-p eml_nrt`) | 1.29 / 1.38 / 1.33 s; median 1.33 s |
  | `tls_off` / `gui_off` | 3 passed |
  | `package(eml_vm) & test(/^(tls\|gui)::/)` | 31 passed |
  | Miri `eml_nrt` (`entries:: census:: constants::`), strict-provenance | `user` 19.93 s (26 passed; this was the first Miri run in the worktree and includes building the nrt test binary, `real` 14.13 s) |
  | Miri `eml_nrt`, strict-provenance + tree-borrows | `user` 2.46 s (26 passed; `real` 3.29 s) |
  | Miri `eml_vm` (`miri::`), strict-provenance | `user` 138.00 s (16 passed; `real` 98.05 s) |
  | Miri `eml_vm`, strict-provenance + tree-borrows | `user` 121.38 s (16 passed; `real` 122.14 s) |
  | `counts run_native` | `accepted 208/skipped 6/rejected 422/  async row 129/  c row 2/  par 22/  prim 156/  std row 113` (equal to section 3) |
  | `counts run_fail_native` | `accepted 8/checked only 0/skipped 2/rejected 71/  async row 33/  c row 2/  par 1/  prim 3/  std row 32` (equal to section 3) |
  | T's slowest line | `116 tests/ui/run/std/long_list_functions.em` |
  | B (`ui::run_native`, `-j 1`) | `PASS [   0.908s]` |
  | FL | `run: entries 636 none 208 only-std 109 std-or-c 115 skip 6`; `run-fail: entries 81 none 8 only-std 32 std-or-c 34 skip 2`; the 4 `a:mut` files print `effect operations,std row` (4 lines) |
  | `bench/native.sh` | 42 rows: 27 with numbers, 15 `rejected`, 0 `differs` |
  | `native: skip` marks | 8 |
- Overall verification (after 10, on the Close commit):
  - G → as defined; every G-grep line → as annotated.
  - `counts run_native` → `accepted <326 + F>/skipped 6/rejected 304/  async row 129/  par 22/  prim 153` (S-1, S-2).
  - `counts run_fail_native` → `accepted 43/checked only 0/skipped 2/rejected 36/  async row 33/  par 1/  prim 2`.
  - `grep '^unlowered' crates/eml_cli/tests/snapshots/integration__ui__run_native.snap` → `unlowered 31`, and `rows` → the 31 names of D-32 (S-4).
  - `cargo nextest run -p eml_native -E 'test(=rows::the_rows_are_partitioned)'` → 1 passed; `cargo nextest run -p eml_rows --lib rows::` → `5 passed` (S-3).
  - `git diff --stat fddd44f2 -- crates/eml_rt` → empty. `git diff --name-status fddd44f2 -- tests/ui crates/eml_cli/tests/repl crates/eml_cli/tests/snapshots | grep -v '^A'` → exactly two lines, `M` on `integration__ui__run_native.snap` and on `integration__ui__run_fail_native.snap`. `cargo nextest run -p eml_vm --test integration bench::` → no `failed`, and `git diff fddd44f2 -- crates/eml_vm/tests/bench.rs` shows import lines only (S-5).
  - `bash nix/crate-graph.sh --check` → exit 0; `grep -c '^\[features\]' crates/eml_vm/Cargo.toml crates/eml_nrt/Cargo.toml crates/eml_native/Cargo.toml` → 0 for each (S-6).
  - benchmarks.md holds the fresh tables and 4 `(native)` rows (S-7).
  - CP-Z → `conclusion: success` for the Close commit's `headSha` (S-8).
- Manual checks by the user: permission to merge into `main` after CP-Z (pushes need no per-push permission, section 4). No other manual check: the native GUI backends are compiled (clippy, the `gui-native` job) and their tests run on the stub, as before this stage.

## 10. Risks and unresolved items

| ID | Item | Impact | How / when it gets resolved | Default if unresolved |
|---|---|---|---|---|
| Q-1 | One of the 53 rows cannot be put in an accepted file, or a residue row turns out to be reached (the Tls / Gui residue was not verified row by row) | 7c-7e, S-4 | the implementer, at the snapshot run | name the std functions that contain the row and the rejected form each brings; move the row between the lists of D-32, the 7e completion and testing.md's residue sentence in the same commit. More than 3 rows move: stop and report |
| Q-2 | 8a's `func_addr` assertion is red on a target | 8b, 8d, G-14 | CP-C | plan revision without a user decision, applied on every target: a datum kind `DATUM_ADDRESS` in `crates/eml_nrt/src/image.rs`; `eml_native`'s `Tables::new` puts the dtor address from `Linked::dtors()` (after `fits`) into the row's `bits`, and `fill_data` copies `bits` into the slot raw, building no object; the datum is named `h:<handle type name>`; `eml_nrt` gains no `eml_cabi` edge; `eml_x_c_handle_new` is unchanged; G-grep allows `"h:`; 8a drops the address assertion |
| Q-3 | 8a's `.sext()` assertions are red on a target | 8c, 8d | CP-C | `eml_clink::c_signature` drops `.sext()`; `lower/c.rs` emits an explicit `sextend` of `I32` arguments and of the result; 8c's test follows |
| Q-4 | B ≥ 5.000 s, or T's slowest file ≥ 1000 ms | 7b, 7e, 8d | measured in those tasks | stop and report T's 5 slowest lines and B to the user; do not split the test or mark files |
| Q-5 | T-B-vm or T-B-nrt rises above 1.20 × base | U5f, U5j | T-B's rule | the remedy of T-B, then stop and report |
| Q-6 | `.#cross` is unavailable on the dev machine | GS 9, 15, 17; U4; U5i; 8d | U1's prerequisite check | record those lines as skipped in each commit message; CP-A's Windows CI covers Windows type-checking of the default features |
| Q-7 | macOS is unavailable | U1 baseline, U5a record, 9, 10 | prerequisite check | U5a: no record; 9 and 10: `Measure jit-externs` right after Close |
| Q-8 | A workspace run does not build `eml_rows`'s tests with `tls` / `gui`, or the gated integration tests do not run from `crates/eml_rows/tests/` through the dev cycle | U3, U4 (GS 14, 16) | U3 | first run them through `--features` (GS 12, 15 forms) and revise the plan; if the dev cycle itself fails, the gated tests go to `crates/eml_cli/tests/` |
| Q-9 | `nix/crate-graph.sh` fails under Git Bash on the Windows runner | CP-A | CP-A | the CI step gets `if: runner.os != 'Windows'` (the graph is OS-independent apart from `--target all`, which already covers Windows) |
| Q-10 | `Task::float` / `set_float` or `f64` passing loses a bit pattern | U5f | `unboxed_float_rows_keep_their_bits` | stop and report (the fix would be a kind change in `eml_extern`) |
| Q-11 | Declaring 232 entries changes a pinned `clif::` snapshot | U2a, U5 | G in those tasks | stop and report (an unlisted kind-2 change) |
| Q-12 | The three-phase expansion is too slow or hits a macro limit at 230 rows | U5 | T-B; compile errors | split the list into family lists that one outer macro concatenates; the grammar and `call` do not change |
| Q-13 | More than 5 files disagree natively on 7b's first full run | 7b | 7b step 4 | D-36's staged landing; more than 10 after one fixing pass: stop and report |
| Q-14 | An earlier task added a `run/` or `run-fail/` file or a mark, so the counts of 7b and 8d shift | 7b, 8d, S-2 | FL at 7b | FL's rule: correct the counts in the same commit if the added files explain the difference; otherwise stop |
| Q-15 | A move changes `THIRD-PARTY-LICENSES.md` | G-a, G-b, U3, U4, U6 | the licences check in those tasks | regenerate in the same commit |
| Q-16 | A Miri run exceeds its budget (300 s / 400 s for `eml_vm` `miri::`) | U2b, U5f, U5j | those tasks | stop and report with the times |
| Q-17 | ~~The user wants fewer pushes~~ Resolved 2026-11-13: five pushes, each without asking (see section 4) | CP-A..CP-7 | - | - |
| Q-18 | A soundness problem beyond D-1 appears (the lending window, the dtor path, the fatal path) | 8b, 8d, U6 | the implementer or a reviewer | stop; consult a strong advisor before settling the fix (CLAUDE.md) |
| Q-19 | Deferred to native-aot, not open here: binding `eml_c$` / `eml_cd$` at link; removing `TestDriver` from apps; the standard-library section of the tool's own notice; dead-stripping on ELF and COFF (verified on aarch64 macOS only) | none in this stage | native-aot's design | recorded in the roadmap's native-aot item by U4 |

## Appendix: main changes from review

Only what prevents relitigation. The full response tables are at the end of appendices 06 and 07.

- Entries for the 45 inline rows were challenged as entries ahead of a caller → kept, with cost and alternative shown to the user, who approved (U-C). A skip set inside `eml_nrt` was rejected: it needs an accumulating muncher and a tying test.
- Transitional wildcard arms during the body moves were rejected → the `pending` group keeps the list total and `call` wildcard-free at every commit. G-1 and G-24 were reworded so that `pending` (U2a..U5i) and `NOT_YET_NATIVE` (inside 7b) are listed transitional groups.
- The dtor crossing the entry as an integer with a cast was rejected → the `dtor` kind token typed `Option<Dtor>`; no `transmute` anywhere.
- A handle leak on a failing allocation was claimed → the premise was wrong (`alloc_foreign` cannot fail on a limit), but both engines now share one `handle_new`, which builds the `CHandle` before allocating.
- The ordering of narrowing checks and lending calls was only argued → it is held by `lend_and_call`'s structure and checked by the window scan, which also asserts that no lending call precedes a narrowing `brif` of the same call; one fixture function holds two C calls.
- Moving all of `hosts.rs` to `eml_io` was rejected → only `stdio_host`, `script_host`, `unmasked` move; the capture fixtures stay in `eml_vm` until jit-tasks.
- A Rust test parsing manifests was replaced by `nix/crate-graph.sh` over `cargo tree`. No CI cross job is added: the "no C build crates" half of G-C13 is in the script, the cross-compile half is local (GS 17).
- "An app without Tls carries no Tls" by dead-stripping was rejected → four library variants; the user chose selection by import (U-F).
- Corrections to appendix 07's checks found while cutting the tasks (its last section): the comment-dropping form of G-C9's grep; `cargo nextest run -p eml_rows --features tls`; `eml_test_support` gains `eml_rt`, not `eml_io`; `eml_vm`'s gui re-exports leave in U4; the placement test scopes to `[dependencies]`; `eml_vm` keeps exactly five foreign re-exports.
- Ownership conflicts between the two unit designs were resolved: U4 owns future/ffi.md L30 / L125; architecture.md and CLAUDE.md change in the structure commits and Close only checks; G-a and U4 write the roadmap and AOT texts and Close only checks them; CP-B is the CI of U6's commit and 8a has its own push CP-C.
- U2 was split into U2a / U2b; a further split was rejected (the grammar change must carry `eml_rows`, `eml_nrt` and `eml_native`'s tests together). One U5 commit was rejected (a bisect could not find a wrong body).
- The 7b counts are exact (320 / 41, then 326 + F / 43), four files above the approved floors, because `Prim::EmptyMutArray` leaves `Form::Prim` in the same task.
- The slow TLS test: lengthening the watchdog, `#[ignore]` and a code workaround were rejected → an environment clean plus one doc paragraph (U-B).
