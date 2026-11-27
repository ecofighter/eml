# What an engine may load: `Trusted` carries its unwind tables

## Decision

`eml_lir::Trusted` becomes "a program plus the unwind table of every function, made under a promise". Its constructors are the only places that verify or compute tables; no engine verifies, and no engine has an unverified entry.

- `unsafe fn Trusted::new(program) -> Result<Trusted, VerifyError>` runs the ownership-stage verifier in every build and keeps the tables of that one walk. Promise P1, unchanged.
- `unsafe fn Trusted::unverified(program) -> Trusted` does not verify and builds a table for every function of any parsed program. New promise P6. No cargo feature.
- Both engines take `&Trusted` exactly as today and read `program.unwinds()`.

What disappears: `eml_vm`'s feature `testing` and its self dev-dependency, the three `load_unverified` functions, `CodeBase::walked` / `load_with` / `load_verified`, `CompileError::Invalid`, `RunFault::Invalid`, the native load's verify-and-panic, `eml_lir::unwind::verified_for`, both `eml_vm` checks in `nix/crate-graph.sh`, and the exception sentences in lir.md, vm.md and native.md. Nothing is added to `eml_native`, `eml_test_support`'s features or `crate-graph.sh`.

The design is the one native code keeps when it is the only engine (section "End state"). Everything it touches in `eml_vm` is a deletion, so removing `eml_vm` later leaves no orphan: no feature, no back door, no script check.

One plan text disagrees with the roadmap and is corrected here: `docs/plans/2026-11-17-runtime/01-design.md` §13.2 and `docs/plans/2026-11-17-vm-retire.md` say a test that needs IR the verifier rejects is "deleted or restated as a verifier test", while docs/future/roadmap.md's `vm-retire` entry requires that "検査モードが誤りを見つけることを確かめるどのテストもネイティブで誤りを出し" and that the feature `testing` is gone. Only an unverified constructor without a feature satisfies both.

## Answers

### 1. The shape

| Question | Answer |
|---|---|
| Who verifies | `Trusted::new`, once, in every build. |
| Who computes tables | The constructor: `new` from the verifying walk, `unverified` from `unwind::points` plus a fallback. |
| What engines take | `&Trusted`; tables through `Trusted::unwinds() -> &[Unwinds]`, indexed by `FnIdx`. |
| Can `testing`, `load_unverified` and the crate-graph check go | Yes, all of them. |

Tables of a refused function:

- `unwind::points` is not total. `walk` stops at the first violation and returns `Err` for the whole function, so a refused function has no table at all, not a partial one [verified: `crates/eml_lir/src/verify.rs` `unwind_points` / `walk`].
- Today the VM substitutes `Unwinds { entry: [], points: [] }`, and native lowering panics on a point statement without a row (`FnLower::point`, `lower/unwind.rs` L156). That is the origin of the "no point statements" constraint.
- A total table exists and is syntactic. Which statements are points is decided by `unwind::point(stmt, c_functions)` alone, the same function the recorder uses (`verify.rs` L795). The fallback gives a refused function an empty entry row and, at every point statement, a row with `owned: []`, `tokens: []` and the statement's own `landing`.
- Rule in one sentence: a frame of a refused function releases nothing when an abort passes it. Releasing nothing is always memory-safe; what the frame held shows up as a leak in checked mode.
- So the "no point statements" constraint is not needed, and neither engine special-cases a refused function [verified by prototype: in a scratch copy, `Trusted::unverified` on a program whose `main` leaks and has a call produced one empty row at the call for `main` and the real tables for the two valid functions; `Trusted::new` returned `Err` on the same program].

The VM's walk skip on REPL appends (`verified_for` with `walked`) is dropped. Measured in a scratch copy (dev profile, `eml_test_support::lir` against `unwind::verified`, mean of 20):

| Program | Functions | Whole pipeline | Full verifying walk |
|---|---|---|---|
| `bench/orders.em` | 7 | 12.8 ms | 34 us |
| `bench/list.em` | 4 | 5.3 ms | 16 us |
| `tests/ui/run/scripts/wc/main.em` | 20 | 11.9 ms | 89 us |

The walk is about 4 us per function, under 1% of the lowering the REPL already repeats for the whole program on every input (vm.md 「追記」). The skip buys nothing measurable and is the only reason the tables could not live in `Trusted`.

Alternatives rejected:

- **`Trusted` with a flag and a `tables(walked)` method the engines call**: keeps the skip, but verification stays at each load and the type still establishes nothing.
- **A verifier walk that continues after a violation**: ownership after a violation is undefined, and it would be a large change to `Checker` for the sake of tests.
- **Engines refusing an unverified `Trusted` in fast mode**: existing tests run refused LIR in fast mode on purpose (`run.rs` L143, `machine.rs` L646), and it would put the concept back into the engines.

### 2. Soundness and honesty

- **Classification**: `Trusted` is "a type whose only constructors establish the fact", and both constructors are `unsafe fn` with `# Safety`. The design is stronger than today's: today `Trusted` establishes only P1 and each engine must remember to verify (two copies, one returning `Invalid`, one panicking). After the change the verifier-checked part is established by the type.
- **Compared with `Trusted::new`**: same class and same kind of promise, a statement about the program that holds for whatever engine later loads it. P6 is P1 plus "also the rules the verifier checks, except the violations put in on purpose, each of which the engine and mode it runs on reports before an operation relies on it, or which only leaves objects unreleased".
- **Honest with the spec**: lir.md 「実行の前提」 today says programs reach an engine only through a verifying load, "except" a feature-gated back door of one crate. The new text has no exception by crate or feature: a program reaches an engine only as a `Trusted`, and `Trusted` has two constructors with two written promises. native.md's "checked mode is not a sandbox" and lir.md's "undefined in fast mode" stay true as written.
- **No mechanical "tests only" check**: I recommend none. `Trusted::new` has the same rule (D-4: one function of `eml_cli`) with no check, and `unsafe` plus `# Safety` is the terminal class of the repo's rule. `crates/eml_cli/tests/surface.rs` already checks that every `pub unsafe fn` has `# Safety`.
- **Behaviour changes**:
  - A compiler bug that produces refused LIR now panics in `eml_cli::lower_trusted` in every build (`internal error: invalid LIR: ...`), where a release build used to return `RunFault::Invalid` from `execute` or `Outcome::Broken` from the REPL. Debug builds already panic for this in `eml_lir::passes_until`.
  - An abort through a frame of a refused function stopped at a point used to be an internal error on the VM (vm.md 「巻き戻しの表」); it now releases nothing on both engines. I found no test that pins the old behaviour [inferred from a grep of `crates/eml_vm/tests`; the full suite was not run].

### 3. Should these failures live only in `eml_rt` / `eml_nrt` tests

No. Keeping them end-to-end is the better design, not just the cheaper one.

- Ownership is fully verifier-checked, so use after free, a second decref and a leak cannot be written as verified LIR. Only `WrongKind` and possibly `LinDup` under a `poly` mark can.
- Entry-level tests show that `Heap` reports the error. They do not show that generated code reaches the audit before the access and takes the fatal path with the same `RunFault` as the VM. That wiring is what NC-4 adds.
- About 25 `eml_vm` tests already use refused LIR for exactly this (`run.rs`, `data.rs`, `evidence.rs`, `par.rs`, `machine.rs`). When the VM retires, checked native code is the only checked engine, and these facts need a home there.
- The stand-in route (`visit` / `call_visited` handing generated code a freed object) would work for some cases, but it cannot reuse the LIR of the existing VM tests when they move to native code, and it is more unsafe test code than ten lines of LIR.

## End state: native code as the only engine

Premises from the user: `eml_vm` will be removed; negative tests of checked mode stand on native code alone; the stage order and green tests at every step are not constraints.

### What the roadmap already records

- docs/future/roadmap.md, `vm-retire` (L38): before `eml_vm` is deleted, every UI file passes on checked and fast native code, "検査モードが誤りを見つけることを確かめるどのテストもネイティブで誤りを出し", and "`rows::call`、`SHARED`、`GateHost`、feature `testing`はなくなり".
- `jit-repl` (L37): `eml run` and `eml repl` use native code; `--vm` selects the VM until `eml_vm` is deleted.
- `jit-tasks` (L35): native code accepts every program; `unsupported` and E0004 go.
- `native-checked` (L32): the UI tests compare every checked native run with the checked VM.
- `docs/plans/2026-11-17-vm-retire.md` D-VR-1 (b): "every checked-mode negative test fires natively with the VM's error class".

So native code must run today's `run_unverified` tests without a `testing` feature by `vm-retire` at the latest. `Trusted::unverified` is that mechanism, and NC-4 is the first stage that needs it, so it is introduced now (unit NC-3b).

### Why this is the end-state shape

- Native code loads a whole program once. It needs one table per function and nothing else from the verifier; `Trusted::unwinds()` is exactly that.
- The only reason verification lived in the engines was the VM's REPL walk skip (`verified_for`). It is VM-only and measurably worthless, so it is deleted now instead of surviving until `vm-retire`.
- After `eml_vm` is deleted, the remaining code is `Trusted` (two constructors), `unwind::points`, `unwind::unowned` and the native load that reads the tables. Nothing in `eml_lir`, `eml_native`, `eml_test_support` or `crate-graph.sh` changes at that point.
- `jit-repl`'s native append gets its tables the same way: each input's `Trusted` has every table.

### The native-checked plan under this premise

| Item | Verdict | Change |
|---|---|---|
| S-NC-8 (`tests/checked.rs`) | Keep; it is the core. | Each test pins its expected `RunFault` / `RuntimeError` / counters as a literal on checked native code. No test's assertion is "equal to the VM". Drop `agree_unverified`; keep only `native_unverified`. |
| S-NC-11 (`eml_test_support::both`, `run_lir_both`, `run_unverified_both`) | Drop. | It is a VM-comparison helper living in `eml_vm/tests`, which `vm-retire` would delete again. Instead move the tests (next row). |
| The 12 `evidence.rs` and 6 `reuse.rs` tests | Move to `crates/eml_native/tests` in NC-5, expectations byte-identical. | They run on checked native code and, when that passes, on fast native code (the native form of `run_lir`). A test whose program native code still refuses stays in `eml_vm/tests` and is listed with the stage that moves it. This is `vm-retire`'s kind-3 move done early. |
| NC-5's CLI work (`--native --debug-heap`, `--native --stats`) | Keep. | Unchanged. |
| NC-5's UI comparison with the checked VM (`ui::run_native_checked`, `ui::run_fail_native_checked`) | Keep as a transitional extra. | It is harness code only, already scheduled for deletion by D-VR-3. While the VM exists it is the one corpus-wide oracle for the counters, and it is what licenses re-pinning `bench.rs` on native runs (D-VR-2). Do not add anything to the product for it. |
| `cli::native_prints_the_vms_stats` | Reword. | Pin the native `stats:` line itself; the VM line beside it is the transitional extra. |

### Now versus the VM-removal stage

Now (NC-3b, NC-4, NC-5):

- The `Trusted` design, with every deletion in `eml_vm`, `eml_sched::RunFault`, `eml_lir::unwind` and `crate-graph.sh` listed in the change list.
- S-NC-8 as native-only assertions.
- The move of the natively accepted `evidence.rs` / `reuse.rs` tests.
- An inventory in the plan of every `run_unverified` test of `eml_vm` with its fate, so that D-VR-1 (b) becomes a check of a list. Starting point [inferred from the tests' doc comments; the implementer confirms each]:

| Test site | Fact | Fate |
|---|---|---|
| `run.rs` L135, L143 | A forgotten object is a leak (checked), fast mode ignores it. | Native now: `checked::a_leak` holds it; delete the VM copy at `vm-retire`. |
| `run.rs` L229, L252 | A heap fault outside / inside an extern, with its position. | Native now if the lowering carries the position; else listed for NC-5. |
| `run.rs` L296, `data.rs` L513 | A dup of a `lin` value. | Native now: `fatal::a_dup_of_a_lin_object`. |
| `data.rs` L534 | An unbox of a data object is `WrongKind`. | Native now: the `box` audit test. |
| `data.rs` L207, L236, L349, L377 | Checked-mode data checks. | Native now where an S-NC-8 test states the same fact; else moved in NC-5. |
| `evidence.rs` L393, L412 | An evidence left at the end. | Moved in NC-5 with `evidence.rs`. |
| `apply.rs` L228, `calls.rs` L349, `structs.rs` L392 | Internal errors of the bytecode interpreter. | Die with `eml_vm`, unless the native `code` / `data` audit has the same fact (then one native test). |
| `par.rs` L647 | A leaking `par` child is a `TransferLeak` at its join. | Moves at `jit-tasks`. |
| `machine.rs` L646 | A `lin` value at a session root is refused by recovery. | Moves at `jit-repl`. |

Left for later stages:

- `jit-tasks` and `jit-repl`: each moves the `eml_vm` tests it makes runnable to `crates/eml_native/tests` at that stage. This replaces "the test files switched to both engines" in `docs/plans/2026-11-17-vm-retire.md` §3.
- `vm-retire`: deletes `eml_vm`, the UI comparison harness, the bytecode-specific tests and the VM copies already restated natively. Its VR-2 shrinks to deletions.

### Stage order

For the load and verify design no reordering is needed, and no test goes red: the VM side of NC-3b is deletion only, and the design is identical whether `eml_vm` is deleted sooner or later.

Recommended order (unchanged stages, reshaped contents):

| Stage | Removes |
|---|---|
| `native-checked` (NC-3b, NC-4, reshaped NC-5, NC-6, NC-7) | `eml_vm`'s `testing`, `load_unverified`, `verified_for`, `CompileError::Invalid`, `RunFault::Invalid`, the two `eml_vm` checks of `crate-graph.sh`; `both` from the plan. |
| `jit-tasks` | `unsupported` and E0004; the `eml_vm` task, `par`, channel and worker tests move to native. |
| `jit-repl` | The VM as the default engine; the REPL and root tests move to native. |
| `vm-retire` | `eml_vm`, `--vm`, the UI comparison harness, `rows::call`, `SHARED`, `GateHost`, `eml_sched`'s `testing`. |

The alternative the user's third premise allows is to delete `eml_vm` right after `native-checked`, before `jit-tasks`. I do not recommend it on the evidence of this question, and it does not block NC-3b either way.

- It would remove early: `eml_vm`, the parity harness, `Engine`, `--native`, 「VMとの違い」, and the constraint that `jit-tasks` shares `eml_sched` and `eml_rows` with a second engine.
- Red until `jit-tasks` closes: every UI file native code refuses today (`par`, the task rows, `blocking` C), which includes all of `tests/ui/{run,run-fail}/{par,async}`, the seed and worker-count runs, and the `eml_vm` task, channel, worker, signal, I/O and pinned tests, which would have no engine.
- Red until `jit-repl` closes: `repl::transcripts` and the REPL itself.
- Also lost for that period: `bench.rs` snapshots until re-pinned natively, whole-run Miri, and the differential oracle for the scheduler during the stage that rewrites it.
- This is a product outage for two stages, not only red tests. Whether the simpler `jit-tasks` is worth it is a question for the `jit-tasks` design, which I did not examine.

## Change list

Unit: a new unit **NC-3b `Make Trusted carry the verified unwind tables`**, committed before NC-4, because NC-4's completion criteria (S-NC-8's `fatal::` tests) depend on it. It touches `jit.rs` `load` and `lower/mod.rs` `program`, which also carry staged NC-4 hunks; if splitting them is not practical, commit it right after NC-4 and move the unverified S-NC-8 tests into it.

### `eml_lir`

`crates/eml_lir/src/trusted.rs`:

```rust
pub struct Trusted { program: Program, unwinds: Vec<Unwinds> }   // Debug, Clone; fields private

impl Trusted {
    /// # Safety: P1
    pub unsafe fn new(program: Program) -> Result<Trusted, VerifyError>;
    /// # Safety: P6
    pub unsafe fn unverified(program: Program) -> Trusted;
    pub fn unwinds(&self) -> &[Unwinds];
}
impl Deref for Trusted { type Target = Program; }
```

- `new` calls `unwind::verified(&program)?`.
- `unverified` calls `unwind::points(&program)` and replaces each `Err` with `unwind::unowned(function, &program.c_functions)`.

`crates/eml_lir/src/unwind.rs`:

- Add `pub(crate) fn unowned(function: &LirFn, c: &[CFunction]) -> Unwinds`: empty `entry`; for each block in order and each statement in order where `point(stmt, c)` is `Some((_, landing))`, an `UnwindPoint { block, stmt, owned: vec![], tokens: vec![], landing }`.
- `verified` becomes `pub(crate)`.
- Delete `verified_for`.
- `points` loses `wanted`: `pub fn points(program: &Program) -> Vec<Result<Unwinds, VerifyError>>`. Every caller that remains passes `|_| true` (`eml_vm/tests/bench.rs` L1816, `eml_native/tests/{abort,effects,rows}.rs`, `eml_lir/tests/unwind.rs`).
- Update the module and function doc comments, which name the VM's `testing` load.

`crates/eml_lir/src/verify.rs`: `unwind_points`, `verified` and `verify_at` lose the `walked` / `wanted` parameter; every function is walked.

### `eml_vm`

- `Cargo.toml`: delete `[features] testing = []` with its comment and the dev-dependency `eml_vm = { path = ".", features = ["testing"] }`.
- `src/compile/mod.rs`:
  - Delete `load_unverified`, `load_verified`, `walked` and the `tables` closure of `load_with`.
  - `load` and `extend` call one private function taking `&Trusted`.
  - `write` and the debug comparison take `program.unwinds()` (`&[Unwinds]`); the two `.expect(..)` on `Option` go (L221, L459).
- `src/lib.rs`: delete `Machine::load_unverified` and `AnyMachine::load_unverified`, and the `CompileError::Invalid` arm of `compile_error`.
- `src/code.rs`: delete `CompileError::Invalid`.
- `crates/eml_sched/src/error.rs`: delete `RunFault::Invalid` (L21, L44, L148).

### `eml_native`

- `src/jit.rs` `load`: delete the `eml_lir::unwind::verified` block and its panic; pass the tables from `program.unwinds()`. Fix the doc comment (the sentence "どのビルドも...確かめ、破れていればpanicする" goes; the promise is `Trusted`'s).
- `src/lower/mod.rs` `program`: take the tables as `&[Unwinds]` from the `Trusted`; update the comment at L140.
- No feature, no `load_unverified`.

### `eml_cli` and `eml_test_support`

- `crates/eml_cli/src/lib.rs` `lower_trusted`: `unsafe { Trusted::new(program) }.unwrap_or_else(|error| panic!("internal error: invalid LIR: {error}"))`. Signature unchanged.
- `eml_test_support`: no change. There is no `execute_unverified` and no `both` (S-NC-11 is dropped; see "End state").

### `nix/crate-graph.sh`

- `FEATURES` table: delete the rows `[eml_vm]` / `testing = []` (L70-71).
- Delete L232-239: the `eml_vm.*testing` tree check and the `load_unverified` name check.
- Add nothing.

### Docs (same commit)

Before writing, check that no text of the rejected proposal is left in the working tree (`eml_native`'s `testing`, `NativeMachine::load_unverified`, `eml_test_support::execute_unverified`).

| File | Section | Change |
|---|---|---|
| docs/spec/lir.md | 「verifier」 (L742, L840) | `Trusted::new` verifies in every build and keeps the tables; delete the `verified_for` text and the two engine-load sentences. |
| docs/spec/lir.md | 「巻き戻しの地点」 (L930) | The tables come from `Trusted::unwinds()`. `points` is for tests reading unverified IR. A refused function in an unverified `Trusted` has the release-nothing table. |
| docs/spec/lir.md | 「実行の前提」 | First paragraph: a program reaches an engine only as a `Trusted`; `Trusted::new` refuses what the verifier refuses. Closing paragraph: replace the `eml_vm` / feature sentence with a pointer to P6. |
| docs/spec/lir.md | 「エンジンの約束」 | `Trusted` wraps the program and its tables; two constructors. Add P6 (text below). Delete the "`load_unverified` takes `&Program`" sentence. R-2 drops `Invalid` from its list. |
| docs/spec/vm.md | 「`CodeBase` と `FnEntry`」 (L35), 「追記」 (L67 and the `Machine::load` bullet) | Drop `Invalid`, `verified_for`, the walk skip and the three `load_unverified` functions. The debug comparison and P2 stay. |
| docs/spec/vm.md | 「巻き戻しの表」 (L262, L267) | Tables come from the `Trusted`. Replace the unverified-load bullet with the release-nothing rule; delete the internal-error sentence. |
| docs/spec/native.md | 「関数の形」 (L181), last bullet of 「検査モード」 (L417) | `load` does not verify; a `Trusted` is already verified or made under P6. Tests reach the audits with `Trusted::unverified`. No feature, no constraint on point statements. |
| docs/implementation/architecture.md | 「crate の依存関係」 | Delete the `eml_vm` `testing` bullet; the features sentence names only `eml_sched` for `testing`; the `crate-graph.sh` bullet drops the `eml_vm` feature check and the `load_unverified` check. |
| docs/implementation/architecture.md | 「`eml_native`の内部」 (L449), 「CLI と lib API」 | `load` reads the tables of the `Trusted`; `lower_trusted` panics on a refused program. |
| docs/implementation/testing.md | 「`eml_vm` のテスト」 (L319) | Refused LIR is loaded as `Trusted::unverified` through the helper `unverified`; no feature. |
| docs/implementation/testing.md | The feature table (L769) | Delete the `eml_vm` `testing` row. |
| docs/implementation/testing.md | 「`eml_native` の検証」 | The `tests/checked.rs` bullet names `unverified`. |

P6, for 「エンジンの約束」 (English draft; translate with PLaMo):

> P6: `unsafe fn Trusted::unverified` wraps a program without running the verifier. The program meets every premise of 「実行の前提」, including the rules the verifier checks, except the violations the caller puts in on purpose. Each such violation is one that the engine and mode the value is loaded on reports before an operation relies on it (the errors of [オブジェクトモデル](object-model.md)の「報告する誤り」 and the engines' internal errors), or one that only leaves objects unreleased. A function that passes the ownership walk gets its table; a function the walk refuses gets an empty entry row and, at each unwind point, a row that releases nothing and keeps the statement's landing marker, so an abort through its frame leaves what the frame held unreleased. Only tests call it.

### Tests

Helpers:

- `crates/eml_vm/tests/common/mod.rs`:
  - `trust` unwraps `Trusted::new` (panic with the `VerifyError`).
  - New `pub fn unverified(text: &str) -> Trusted`: parses, asserts `eml_lir::verify(&program).is_err()`, then `unsafe { Trusted::unverified(program) }`.
  - Delete `execute_unverified`; its callers use the existing `execute(&unverified(text), config)`.
  - `run_unverified(text)` keeps its signature.
- `crates/eml_native/tests/common/native.rs`: `trust` unwraps likewise; new `unverified(text)` with the same assertion; `native_unverified(text)` reuses the body of `native_checked` on a `&Trusted`. There is no `agree_unverified`: the tests pin literal expected values.
- A test that trusted a refused program without loading it (for example only to call `unsupported`) passes the plain `Program` instead.

Kind 1 (listed one by one):

| Test | Change | Replacement |
|---|---|---|
| `eml_lir` `unwind::verified_for_walks_only_the_selected_functions` | Deleted; the function is gone. | None. |
| `eml_lir` `unwind::verified_for_applies_the_program_rules_to_every_function` | Deleted. | The program-wide rules are `verify`'s existing tests. |
| `eml_lir` `verify::a_landing_enters_a_bad_function_at_every_level_and_unwalked` | The `verified_for` assertion (L3783-3791) is removed; renamed without `_and_unwalked`. | None. |
| `eml_vm` `machine::a_program_the_verifier_refuses_is_not_loaded` | Deleted. | New `eml_lir` `trusted::new_refuses_what_the_verifier_refuses` (same LIR, asserts the `VerifyError`). |
| `eml_native` `effects::release_check_a_tail_clause_result_mismatch_panics` | Deleted. | See note. |
| `eml_native` `effects::release_check_a_clause_entry_signature_mismatch_panics` | Deleted. | See note. |
| `eml_native` `effects::release_check_a_marked_apply_not_followed_by_prompt_panics` | Deleted. | See note. |
| `eml_native` `effects::release_check_a_program_level_rule_is_refused` | Deleted. | See note. |
| `eml_native` `raise::release_check_an_empty_mask_panics` | Deleted. | See note. |

Note for the five `release_check_*` tests: their fact was "the native load verifies in every build", which becomes `trusted::new_refuses_what_the_verifier_refuses`. For each rule, name the `crates/eml_lir/tests/verify.rs` test that pins it; where none does, move the LIR there as a new verifier test. `load_panic` in `effects.rs` is deleted.

New tests:

- `eml_lir` `trusted::new_refuses_what_the_verifier_refuses`.
- `eml_lir` `trusted::an_unverified_program_has_a_row_at_every_point`: a refused function with a call, a faulting extern and a marked `apply`; asserts the empty entry row, one empty row per point and the landing. Also asserts that a valid function of the same program has its real table.
- The S-NC-8 tests that need refused LIR (`fatal::a_dup_of_a_lin_object`, `fatal::a_second_decref`, `fatal::an_unpack_of_a_freed_box`, `fatal::a_switch_on_a_freed_box`, `fatal::a_release_that_names_another_constructor`, `fatal::an_apply_of_a_clause_closure`, `a_leak`), in NC-4.

Kind 2: none expected. If a `run_unverified` test turns out to pin the VM's internal error for an abort through a refused frame, its expected value changes to the leak; add it to the plan by name.

Kind 3 (mechanical, expected values unchanged):

- `load_unverified(&program, ..)` becomes `load(&unverified(..), ..)` in `machine.rs` L646 and in the unit test `every_function_has_an_entry_row_at_its_start` (`crates/eml_vm/src/vm.rs` L4150, expected `vec![]` unchanged).
- `points(p, |_| true)` becomes `points(p)`.
- A `run_unverified` test whose LIR the verifier accepts fails the new helper assertion and moves to `run_lir`.

### Completion criteria

- `grep -rn 'load_unverified\|verified_for\|execute_unverified' crates nix docs/spec docs/implementation` is empty.
- `grep -rn 'feature = "testing"' crates/eml_vm crates/eml_native crates/eml_lir` is empty; `grep -n testing crates/eml_vm/Cargo.toml` shows only the `eml_sched` dev-dependency line.
- `grep -rn 'unwind::verified\|unwind::points' crates/*/src` hits only `crates/eml_lir/src`.
- `grep -rn 'RunFault::Invalid\|CompileError::Invalid' crates` is empty.
- `bash nix/crate-graph.sh --check` passes.
- `cargo test -p eml_lir`, `-p eml_vm`, `-p eml_native`, `-p eml_cli` and `cargo clippy --all-targets -- -D warnings` pass; the `bench.rs` `RunStats` snapshots are unchanged.

## Plan amendments (`docs/plans/2026-11-17-native-checked.md`)

- **5.4, new S-NC-13**: "`eml_lir::Trusted` carries the program and the unwind table of every function. `unsafe fn Trusted::new` verifies in every build and returns `Err(VerifyError)`; `unsafe fn Trusted::unverified` (P6) takes any parsed program and gives a function the ownership walk refuses a table that releases nothing. Engines do not verify and have no unverified entry. `eml_vm`'s feature `testing`, `load_unverified`, `verified_for`, `CompileError::Invalid`, `RunFault::Invalid` and the two `eml_vm` checks of `nix/crate-graph.sh` are deleted. The REPL append walks every function (about 4 us per function, measured). This is the shape native code keeps as the only engine (docs/future/roadmap.md `vm-retire`)."
- **S-NC-8**: replace "run through the unverified helpers on the checked VM and on checked native code" with "loaded as `Trusted::unverified` through `tests/common/native.rs`'s `unverified` and `native_unverified`, on checked native code". Replace "asserts the same `RunFault` on both engines, compared without `function` and `at`" with "asserts its `RunFault` as a literal". Delete the "Amendment (NC-4, option A ...)" tail.
- **S-NC-11**: replace with "The 12 tests of `evidence.rs` and the 6 of `reuse.rs` that native code accepts move to `crates/eml_native/tests` with byte-identical expectations and run on checked native code, then on fast native code when the checked run passes. A test native code refuses stays in `crates/eml_vm/tests`, listed with the stage that moves it. There is no `eml_test_support::both`."
- **7.1 / 7.2**: add unit NC-3b with the change list above. Delete the two "Amendment (the unverified load ...)" bullets of NC-4 (L304, L318). In NC-5, delete the "Note from NC-4" (L328) and the `both` / `run_lir_both` / `run_unverified_both` work line (L332); add the move and the inventory table of "End state"; the completion line `-p eml_vm ... (evidence|reuse)` becomes the same filter on `-p eml_native`, and N is recounted.
- **7.4**: add the kind-1 table and the kind-3 items above under NC-3b. Under NC-5, the move of `evidence.rs` / `reuse.rs` is kind 3 (expectations byte-identical); a test that needs another expected value natively stops the unit (10.3 O-5).
- **8**: replace the three "amendment of 7.2 NC-4" rows (L504-506) with the docs table above, unit NC-3b.

In other plans (they follow the roadmap text and the user's premise, so they are corrections, listed for the user's approval with this amendment):

- `docs/plans/2026-11-17-runtime/01-design.md` §13.2 (the `load_unverified` cell): "tests that need IR the verifier rejects run on native code through `Trusted::unverified`". §13.3 (`vm-retire` row): drop "`eml_vm`'s `testing` deleted" and "the `load_unverified` check deleted"; both are gone at `native-checked`.
- `docs/plans/2026-11-17-vm-retire.md`: G-21 drops "and the `load_unverified` check"; §3 `jit-tasks` reads "the test files it makes runnable moved to `crates/eml_native/tests`"; §6 kind 1 drops "tests that need IR the verifier rejects ... deleted or restated as verifier tests".
- docs/future/roadmap.md `native-checked` (L32) keeps its UI-comparison sentence. `vm-retire` (L38) keeps "feature `testing`はなくなり", which then means `eml_sched`'s only; no edit is needed until NC-7's Close.

## Additional checks

- `points` returns `Err` for a refused function with no partial table; the fallback is total [verified: read `verify.rs`; prototype of `trusted.rs` and `unowned` compiled and ran in a scratch copy of `eml_lir`].
- Cost of the full verifying walk against the pipeline [verified: measured, table above].
- The roadmap's `vm-retire` entry requires natively firing negative tests and no `testing` feature [verified: docs/future/roadmap.md L38; `docs/plans/2026-11-17-vm-retire.md` D-VR-1 (b)].
- Existing tests run refused LIR in fast mode, so P6 cannot say "checked mode only" [verified: `crates/eml_vm/tests/run.rs` L143, `machine.rs` L646].
- `RunFault::Invalid` and `CompileError::Invalid` are named only in `eml_vm` and `eml_sched` and pinned by one test [verified: grep].
- Native lowering of a refused function needs nothing from the verifier beyond the rows [inferred from `lower/unwind.rs` and `safepoint.rs`; not run, since the engines were not built with the prototype].
- The scratch copy was deleted; no worktree was created and the repository was not changed.
