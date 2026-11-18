# Stage async-root: `main` is the root task; `Async.run` is removed

- Created: 2026-11-17 (design-level plan).
- Base: the `Close engine-trust` commit. Facts and line numbers in 01 describe main @ 4a2954c1; the refresh re-checks them.
- Status: design-level. `Start async-root` replaces this file with an executable plan in the model of `2026-11-17-engine-trust.md` (deleted at its Close; read it with `git show 97cd579c:docs/plans/2026-11-17-engine-trust.md`) (header, goal and S-n, scope, background with `file:line`, prerequisites, decisions and criteria, constraints, task table and units with exact completion commands, gates, test changes by kind with kind 1 one by one, execution order, verification with baseline, risks).
- Appendices: `docs/plans/2026-11-17-runtime/`; 00 wins; the design is 01 §5 (D-AR-1..D-AR-7), with 01 §2.1 (root task), §3 E2, §16 S-21, §18 G-2.

## 1. Goal and completion conditions

- Goal: a `main` whose row has `Async` runs as the root task of its run; `Async.run`, `run_raw`, `run_result` and everything only the nested root used are deleted. All of it in the VM's existing scheduler; native code is unchanged except that it keeps rejecting the task rows.
- Completion conditions:
  - G-2: `grep -rn 'Async\.run\b\|run_raw\|run_result\|AsyncRunRaw\|WaitToken' crates std tests bench docs/spec docs/implementation docs/future` → empty (at 4a2954c1 `docs/future` has 22 such lines in 5 files [V: plan verification]; a sentence of a rejected alternative that must name the removed function is listed by AR-1 and excluded by path and line in the executable plan).
  - S-21: one root scope per run; the run ends only with the root task finished and the scope empty; every `async/` UI file passes at N = 1, 2, 8 and under both seeds.
  - A `main` without `Async` runs as before: every `RunStats` snapshot of a program without `Async` is byte-identical.
  - Every program, test and document that used `Async.run` is rewritten (D-AR-7 scope); the rewrite script's manual class is empty at Close.
  - `bench/run.sh` rows within +0.5 % of the base for programs that did not use `Async.run` (01 §19 "Gates").

## 2. Scope

- In: D-AR-1..D-AR-7; the VM's root entry and end of run; the REPL's `Async` inputs (D-AR-5); deletions of D-AR-4; docs of AR-8.
- Out: native task rows (`jit-tasks`); the soft limit (`task-limits`); any scheduler reshaping of the VM (UJ-17: only what the root entry needs).

## 3. Received from earlier stages

- `engine-trust`: engines take `&Trusted`; the REPL appends with `extend`; `run_shown` is `unsafe` (D-AR-5 changes what `run_shown` takes the result from).

## 4. Decisions (01 §5)

- D-AR-1: `main : Unit -> <r> Unit` with `r` a subset of `{IO, Panic, Std.Async.Async}`; the entry check gives a diagnostic otherwise (code chosen at the refresh; E-codes of `eml_types` `codes`).
- D-AR-2: with `Async` in `main`'s row, translate emits `main$root : Unit -> <IO, Async> Unit = Std.Async.bottom main ()`; worker 0 opens the root scope, makes the root cell and queues `Entry { kind: Start }` with `main$root`'s static closure; worker 0's loop ends when the root task has finished and the root scope is empty.
- D-AR-3: a root that ends `Io` / `Panic` gives the runtime error an uncaught failure of a synchronous `main` gives, naming `main`, with no position; `exit`, a stop bit and an unwaited signal act on the root task as they act on `run`'s root today.
- D-AR-4: deleted: `Async.run`, `run_raw`, `run_result`, their rows, the VM's `AsyncRunRaw` arm, the `Task` that waits for a root, pinned nested roots; in `cells.rs` `Take::Run`, the root `opener`, `WaitToken`, `Runnable::Token`, `Woken::Token`, `Registry::wait`.
- D-AR-5: a REPL input whose row has `Async` runs as the root task of its run (its wrapper plays `main`); `run_shown` takes the root cell's result.
- D-AR-6: every task starts at class `Task` with the whole depth; no shared depth budget.
- D-AR-7: the rewrite by a one-off script (its text in the executable plan, not in the tree) of `main () = Async.run body` and a first statement `use Async.run`; every other use is printed for manual rewrite. Scope [V: V3]: 188 `.em` / `.repl` files; 29 Rust test files with 265 uses; `crates/eml_vm/tests/miri/blocking.lir`; 10 `bench/` programs; 8 check-fail files; declarations.md, expressions.md, examples.md; testing.md, status.md, benchmarks.md.
- Criteria: G-2; S-21; the `bench/run.sh` gate of 01 §19.

## 5. Units (in order)

1. AR-1 `Start async-root`: plan; the script; the kind-1 list (U-5: the manual class and the Rust kind-1 list, including what it finds in `pinned.rs`); the baseline.
2. AR-2: the entry check and `main$root` in translate (Core IR text tests).
3. AR-3: the VM's root entry and end of run (D-AR-2, D-AR-3, D-AR-6).
4. AR-4: the REPL (D-AR-5).
5. AR-5: `.em`, `.repl` and `bench/` programs through the script and the manual list.
6. AR-6: Rust-embedded programs and `crates/eml_vm/tests/miri/blocking.lir`.
7. AR-7: deletions (D-AR-4).
8. AR-8: docs (section 7 of this plan).
9. AR-9 `Close async-root`.

## 6. Test changes by kind (scope)

- Kind 1 (listed one by one by AR-1): `nested_run`, `nested_run_cancelled`, `cancel_reaches_nested_run`, `limit_depth_nested_run`, `handle_outlives_run`; `run_on_main_returns` and `run_on_main_panics` rewritten; `memory::a_task_in_a_nested_run_is_flagged`; `workers::a_start_after_a_nested_run_places_on_another_worker`; what AR-1 finds in `pinned.rs`; the script's manual class.
- Kind 2: positions inside `std/Async.em`; positions on a rewritten line; errors that passed through `run_result`; `RunStats` snapshots of programs that called `Async.run`; REPL transcripts; the native UI snapshots' counts (`async row` changes with the kind-1 deletions and rewrites).
- A kind 1 or kind 2 change found during execution that the executable plan does not list stops the unit for a plan amendment approved by the user.
- Kind 3: rewritten files whose expectations are byte-identical.

## 7. docs/spec sections written

stdlib.md 「`Std.Async`」「起動」「メインのworker」; vm.md 「`Async` のタスク」「待ちの2種類」; declarations.md (`main`'s row); types.md (the entry rule for a `main` whose row has `Async`); expressions.md; examples.md; docs/future/async.md decision 2; docs/future/multicore.md 「`Async.run` と入れ子」 deleted; the other `Async.run` mentions of docs/future (ffi.md 1, native.md 3, roadmap.md including L251, the rest of multicore.md and async.md); testing.md, status.md, benchmarks.md (the rewritten programs).

## 8. Acceptance checks

- CLAUDE.md's gates; `cargo nextest run` passes with the kind-1 list applied.
- G-2's grep empty.
- `async/` UI files at N = 1, 2, 8 and two seeds pass (the harness's existing worker and seed runs).
- `bench/run.sh` within +0.5 % for programs that never used `Async.run`; `bench/run.sh`, `bench/rt.sh`, `bench/scripts.sh` tables replaced at Close.
- The roadmap's `async-root` item is deleted; the REPL fork-table item is untouched (it belongs to `par-total`).

## 9. Risks with defaults

| Item | Default |
|---|---|
| U-5: the script's manual class is large | rewrite by hand in AR-5 / AR-6, one commit per directory if needed |
| A program needs a nested root (a library that called `Async.run` internally) | rewrite it to take `Async` in its row; none expected in `std/` [I] |
| A `pinned.rs` test asserts a nested root's pinning | rewrite over `Async.start_on_main` or delete (kind 1, listed) |
| D-AR-1's diagnostic code | a new E2xxx in `eml_types::codes`, or the existing entry-row code if one fits (AR-1 decides) |

## 10. Refresh at the stage's first commit

Re-check against `Close engine-trust`: D-AR-7's counts (rerun the greps of V3), the `cells.rs` items of D-AR-4 (their only production use is the nested root wait [V: V5]), and the positions of `std/Async.em`. Write the script into the plan; record the baseline (full run count, `RunStats` snapshots unaffected by the change).

Performance gate procedure (01 §19 "Gates"; the executable plan writes it out): `bench/run.sh` prints one table of medians and the build directory changes counts (benchmarks.md L162), so base and head are built and measured alternately in one worktree (`git switch --detach <stage base>`, `bench/run.sh`, `git switch <head>`, `bench/run.sh`, three pairs, the median per row). The compared column is 実行の分 (instructions minus `empty`'s). Rows compared: every row of `bench/run.sh` whose program never used `Async.run` (at the base none of `bench/*.em` does [I: AR-1 greps]); `bench/async/` is not gated. A row over +0.5 % stops the stage; the orchestrator brings the table to the user.
