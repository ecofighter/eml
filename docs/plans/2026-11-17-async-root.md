# Stage async-root: `main` is the root task; `Async.run` is removed

- Created: 2026-11-17 (design-level plan); redesigned into this executable plan 2026-11-19.
- Base: main @ 3f007940 (`Merge branch 'plan/engine-trust'`). Every `file:line` below describes that commit; line numbers are hints, the named item decides.
- Status: executable plan, approved by the user 2026-11-19 (all of D-10, the G-2 restatement, the position loss of K2-b, the class of an `Async` `main`, the kind 1 and kind 2 changes of 7.4, S-AR-1's texts). The design is 01 §5 as restated by 00 §D D-10 (2026-11-19). Approving this plan is the agreement for D-10 (it restates D-AR-2, D-AR-3, D-AR-4, G-2 and where E2 holds) and for the test changes of 7.4. English by the user's decision (00 §A UJ-1, UJ-16); `docs/`, code comments and CLAUDE.md stay Japanese via PLaMo (`form: plain`, no space between Japanese and Latin characters or inline code).
- Appendices (`docs/plans/2026-11-17-runtime/`): `00-decisions.md` wins over everything (§D D-10 is this stage's restatement); `01-design.md` §5 (D-AR-1..D-AR-7), §2.1, §3 E2, §16 S-21, §18 G-2 are read through D-10.
- Tags: [V: proto] checked on a prototype in a scratch worktree at 3f007940 (a 93-line patch: `async_effect`, `check_main`, the two-pass input check, `root` in std, the collector, the wrapper, the error name; then the script of 7.5 over every file; then `cargo test --workspace`); [V: read] read in the source; [V: census] counted by the researchers' scripts; [I] inferred.

## 1. Goal and success criteria

- Goal: a `main`, or a REPL input, whose row has `Async` runs as the root task of its run. The entry wrapper that translate already builds calls the std-private `Std.Async.root`, which is what `Async.run` was. The public `Async.run` and nested roots are removed. No engine structure changes: the LIR `Program`, the engines' entry APIs, the scheduler and the `unsafe` surface stay as they are.
- Success criteria:
  - S-1 (G-2 as restated by D-10 (e)): `grep -rn 'Async\.run\b\|run_raw\|run_result\|AsyncRunRaw\|AsyncRunResult' crates std tests bench docs/spec docs/implementation docs/future` → empty.
  - S-2 (S-21): one root scope per run; the run ends only with the root task finished and the root scope empty; every `async/` UI file passes at N = 1, 2, 8 and under both seeds (the harness's existing runs).
  - S-3: a program without `Async` in its entry's row compiles to the same Core IR and LIR and runs with byte-identical `RunStats` (`crates/eml_vm/tests/bench.rs` untouched; `lowering::every_program_lowers_to_the_same_text` passes unchanged).
  - S-4: every program, test, comment and document that used `Async.run` is rewritten; the script's manual class (7.5) is done by hand.
  - S-5: the `bench/run.sh` gate of 10.1 holds on AR-6's commit (every row within +0.5 % of the base).
  - S-6: the REPL runs an input whose row has `Async` as the root task of that input's run; an input without `Async` runs exactly as before.
  - S-7: CI `success` on every job for the Close commit.

## 2. Scope

- In: the type check of `main` and of REPL inputs; the root wrapper in translate; `Std.Async.root`; the name of the root's failure; the rewrite of programs, tests, comments and docs; deleting the public `run` and nested roots; renaming the two rows.
- Out (owner): a root the engine starts, `Program.root`, the split of the wrapper at the wait (`jit-tasks` JT-5, D-10 (b)); deleting `WaitToken` and its relatives (`vm-retire`); the soft memory limit and the limit classes (`task-limits`); `par` children (`par-total`); the new scheduler (`sched-core`).
- No new crate edge, feature, cross-crate re-export, `RuntimeEntry`, `pub` std function, `pub` item of `eml_sched::cells` or `unsafe` block. No change under `crates/eml_rt`.

## 3. Background context (at 3f007940)

Front end:
- `check_main` (`crates/eml_types/src/check/mod.rs:1142-1209`) allows the labels `[program.io(), program.panic()]` (:1166); otherwise E2004, whose text prints the allowed row built from that list ("`main` must have type `Unit -> <IO, Panic> Unit`, with either label optional") [V: read]. `ExternIndex` has `io` and `panic` only (`crates/eml_hir/src/hir.rs:233-236`); `Std.Async.Async` is an ordinary std effect, loaded only when the module is imported. `Program::function_by_canonical_name` (`crates/eml_hir/src/program.rs:423-435`) is the model for finding a std item by name; it finds private functions [V: proto].
- REPL inputs: `check_input` (`crates/eml_types/src/check/input.rs:24`) checks an expression or `let` input under the closed row `<IO, Panic>` (:38-46); `finish_input` (:178) writes the input function's type with the same fixed row (:197); another label is E2015 (`check/report.rs:396`, "... but an input may perform only `IO` and `Panic`") [V: read].
- Entry: `ProgramBuilder::entry` (`crates/eml_core_ir/src/translate/program.rs:715-801`) builds `entry$<name>` (arity 0): it calls the target with `()` (an arity-0 target is called first and the result applied), then `EntryAfter::{Return, Store(root), Shown(..)}`. `instances::collect` (`translate/instances.rs:815-880`) adds the entry instance (:835), the `RuntimeEntry` functions (:840-849, monomorphic) and the display part of an input (:855); `Collector::add(function, type args, row bits)` (:242) adds an instance of a generic function at chosen types [V: read, proto].
- `std/Async.em`: `bottom` (private, generic) L67-72; `pub run` L74-75 (`run f = run_result (run_raw (bottom f))`); the placement comment L77-79; `pub scope` L95-106; `compute` L171; the runtime-interface comment L362-366; `extern run_raw` L368, `extern run_result` L377; 38 `pub` items [V: read].

Runtime:
- `run_raw`'s arm (`crates/eml_vm/src/vm.rs:2802-2828`): opens a root scope (`open_root(parent, caller)`, the only call with a caller, :2810), starts the root task on the calling worker, registers `cells.wait(root, WaitToken(..))`, switches away as `TaskState::WaitingCell(root)`. `run_result` shares the `take_result` arm (:2849-2861) with `Take::Run` (`crates/eml_sched/src/cells.rs:2580-2587`): the root scope must be closed, its first failure wins over the value, the scope is released. A failure goes through `raise_again` (:3898); uncaught, `CodeBase::error` (`crates/eml_vm/src/code.rs:350`) names the function and position of the failing instruction, which is why 17 snapshots print `at <std>/Async.em:75:9` [V: read].
- Called from `main`, `run_raw` runs on the entry `Task` (`cell: None`), opens a parentless root, and the entry `Task` is the waiter; this is the path the root wrapper uses. Nested-root-only parts: the `Some(cell)` branch of the arm (parent scope, inherited `pinned`), `open_root`'s parameters (`cells.rs:1947-1957`), `TaskCell.run` (`cells.rs:307`; set :1954, cleared :2546, read by the cancel mark :2777). `open_root(None, None)` is written 50 times in tests and helpers (47 in `cells.rs`, one each in `eml_vm`'s `workers.rs`, `jobs.rs`, `sched.rs`) [V: grep].
- Every `Async` entry `Task` starts at `Limit::Task` with depth offset 0 (`vm.rs:251-268`); the entry `Task` of `main` runs at `Limit::Stops` (`vm.rs:188-211`). docs/spec/stdlib.md:1156 and vm.md:1028 still say a nested run's body continues the caller's depth [V: read].
- `exit` in a task: `entry_finish` (`vm.rs:4151`) writes the end field and returns `Stopped(Ended)`; `run_entry` maps it to `Ran::Abandoned` with the code (`crates/eml_vm/src/lib.rs:655-672`). Unchanged by this stage.

What the prototype showed [V: proto]:
- `main : Unit -> <IO, Panic, Async> Unit`, `<IO, Async>`, `<Panic, Async>`, `<Async>` and an arity-0 `main = fn () -> ..` run under `--debug-heap` and at 4 workers; a late child is waited for; `exit 3` in a child ends the run with 3; `eml run --native` refuses with E0004 naming a task row, as for any `Async` program.
- REPL under `--debug-heap`: `let x = Async.await (Async.start (fn () -> [1, 2, 3]))` then `x` prints `[1, 2, 3]`; an input without `Async` keeps its position on failure (`division by zero` / `at <input 9>:1:4`); a failing root reads `panicked: boom in \`input$8\``; `let t = Async.start ..` is E2016 (a `Task` is linear), so no handle outlives its input.
- After the script of 7.5 over all 223 files (`changed 217 of 223`; idempotent), `cargo test --workspace` fails exactly these tests: `eml_cli` `ui::run_fail` (26 snapshots), `ui::check_fail` (7 snapshots), `repl::transcripts` (`async/stale_scope`); `eml_vm` `io::a_write_counts_one_completion`, `io::a_read_at_the_peer_eof_returns_the_buffer_unchanged`, `memory::a_task_that_grows_without_switching_after_a_crossing_without_a_candidate_is_flagged`, `miri::a_blocking_call_runs_on_the_test_driver`, `pinned::children_of_a_pinned_task_stay_on_the_main_worker`, `tasks::await_raises_the_panic_of_the_task_again`, `tasks::drain_within_waits_until_the_scope_has_no_task`, `tasks::race_decides_the_winner_in_one_claim`, `tasks::timeout_runs_the_body_in_the_calling_task`. Everything else passes, including `ui::run` (every rewritten `run` file byte-identical), the native tallies, `bench.rs` and the lowering tests. 7.4 gives each a fate.

Uses (census at 3f007940) [V: census, proto]:
- `Async.run` is written in 189 `.em` files (179 under `tests/ui`, 10 under `bench/`; two only in a comment), 9 transcripts (`crates/eml_cli/tests/repl/async/{chan_stale,compute,counter,deadlock,exit_in_task,io_in_flight,run_on_main,stale_scope}.repl`, `gui/not_available.repl`; 21 lines), 25 Rust test files (260 lines) and `crates/eml_vm/tests/miri/blocking.lir`. Docs: spec 51 lines (stdlib.md 24, vm.md 21, expressions.md 3, core-ir.md 2, declarations.md 1), implementation 5 (status.md 2, testing.md 2, benchmarks.md 1), future 22 (multicore.md 10, async.md 6, native.md 3, roadmap.md 2, ffi.md 1).
- Rust programs sit at column 0 of the Rust file inside `"\`-continued ordinary strings (not raw strings), or are escaped one-line strings (`"..\nmain : Unit -> <IO, Panic> Unit\nmain () = .."`).
- No `bench/*.em` at the top level uses `Async`, so no row of `bench/run.sh` and no `RunStats` snapshot of `bench.rs` runs an `Async` program.
- Tests: `cargo nextest list` lists 5302; 23 are `#[ignore]`.

## 4. Prerequisites

| Needed | For | How to check | If missing |
|---|---|---|---|
| The dev shell (Rust 1.99.1, nextest, clippy, insta) | every unit | `cargo --version` → `cargo 1.99.1`; `cargo nextest --version` exit 0 | stop |
| macOS | the gate of 10.1; the Close measurements | `uname` → `Darwin` | gate: stop for the user; Close: `Measure async-root` right after |
| The `.#cross` and `.#miri` shells | checkpoint extras | `nix develop .#cross -c true`, `nix develop .#miri -c true` exit 0 | stop before CP-A |
| `plamo-translate` (MCP tool or CLI) | Japanese text | tool listed or `plamo-translate --help` exit 0 | the `natural-japanese` skill (CLAUDE.md) |
| `python3` (3.8 or later) | the script of 7.5 | `python3 --version` exit 0 | stop |
| `gh` authenticated with push | checkpoints | `gh auth status` exit 0 | ask the user |
| The working tree at the base | AR-1 | `git status --porcelain` lists only `docs/plans/2026-11-17-{async-root,jit-repl,jit-tasks,sched-core,vm-retire}.md` and `docs/plans/2026-11-17-runtime/00-decisions.md` as modified | stop |

A unit that finds it must touch `crates/eml_rt`, add an `unsafe` block or change `crates/eml_cli/tests/surface.rs` stops for the user (CLAUDE.md: a soundness question goes to a strong advisor first).

## 5. Decisions

### 5.1 Long-term guidelines

| ID | Decision | Source |
|---|---|---|
| D-AR-1 | `main : Unit -> <r> Unit` with `r` a subset of `{IO, Panic, Std.Async.Async}`; E2004 otherwise | 01 §5 |
| D-AR-2 | With `Async` in the entry's row, the entry wrapper (`entry$main`, `entry$input$N`; arity 0, called by the engine as today) calls `Std.Async.root@[T]` with the entry function's value, `T` being the entry's result type, and then does what it does today with the result. `root : (Unit -> <IO, Panic, Async> a) -> <IO, Panic> a` is private std code: `root f = root_result (root_start (bottom f))`. The engine starts nothing and no IR type changes. An entry without `Async` is wrapped as today | 00 §D D-10 (a) |
| D-AR-3 | An uncaught failure raised by the `root_result` instruction is the root's failure: the error is `<failure> in \`<name>\`` with no position, `<name>` being the entry function's name without `entry$` (`main`; `input$N` in the REPL). The root scope's first failure wins over the root's value (`Take::Run`, unchanged). `exit`, a stop bit and an unwaited signal keep today's code paths | 00 §D D-10 (c) |
| D-AR-4 | Deleted: the public `Async.run`; nested roots (the root arm's `Some(cell)` branch with the inherited `pinned`, `open_root`'s parameters, `TaskCell.run`); the names `run_raw` / `run_result` (renamed `root_start` / `root_result`, `AsyncRootStart` / `AsyncRootResult`). Kept: `Take::Run`, `WaitToken`, `Waiter::Token`, `Woken::Token`, `Runnable::Token`, `Registry::wait`, `TaskState::WaitingCell`, `ScopeCell.opener` | 00 §D D-10 (d) |
| D-AR-5 | A REPL input is checked under `<IO, Panic>`; only if that reports E2015 and `Std.Async` is loaded, it is checked again under `<IO, Panic, Async>` and that result is the input's. An input accepted by the second check has `Async` in its row and gets the wrapper of D-AR-2 | 00 §D D-10 (f) |
| D-AR-6 | Every task starts at class `Task` with the whole depth. Already true in the code; the stage corrects stdlib.md 「タスクの上限」 and vm.md and states that an `Async` `main`'s own code runs at class `Task` inside its task | 00 §D D-10 (g) |
| D-AR-7 | The rewrite by the one-off script of 7.5 (not in the tree) and by hand for its manual class | 00 §D D-10 (h) |
| E2 / UJ-17 | The VM's scheduler is not reshaped: the entry `Task` waits for the root scope as it does for `Async.run` today. "No wait holds frames but a join" holds natively from `jit-tasks` (D-10 (b)) | 00 §A, §D D-10 (b) |

Conformance criteria (every unit's review checks those that apply):

| ID | Rule | Applies | Check |
|---|---|---|---|
| G-2 (D-10 (e)) | MUST NOT: a public `Async.run`, a nested root, the names `run_raw` / `run_result` | AR-6 on (code), AR-7 on (docs) | SC-2 |
| S-21 | One root scope per run; the run ends only with the root done and the scope empty | AR-2 on | the `async/` UI runs at N = 1, 2, 8 and two seeds; AR-2's tests |
| AR-G1 | MUST: a program whose entry has no `Async` compiles to the same Core IR and LIR text as at the base, with byte-identical `RunStats` | all | SC-3 |
| AR-G2 | MUST NOT: a new crate edge, feature or re-export; a change under `crates/eml_rt`; a new `unsafe` block; a change of `surface.rs`; a new `RuntimeEntry`; a new field of either IR's `Program`; a new `pub` function in `std/Async.em` | all | SC-4 |
| AR-G3 | MUST: a kind 1 or kind 2 test change found during execution that 7.4 does not list stops the unit; the orchestrator amends this plan with the user's approval (testing.md 「テストの変更の運用」) | all | review of each unit's diff against 7.4 |

### 5.2 Short-term decisions (this plan)

- S-AR-1 (diagnostic texts): E2004 prints the row of the labels the program can write: with `Std.Async` loaded "`main` must have type `Unit -> <IO, Panic, Async> Unit`, with each label optional" (the row as `types.display` prints it), otherwise today's text unchanged. E2015 reads "... but an input may perform only `IO`, `Panic` and `Async`" when `Std.Async` is loaded, otherwise today's text unchanged. No existing pin of either text loads `Std.Async` [V: proto: `invalid_main_type`, `main_with_other_effect`, `errors/diagnostics.repl`, `redefinition/effects.repl` and the unit tests pass unchanged].
- S-AR-2 (the name): `CodeBase::error` returns `RuntimeError::Fault { fault, function: <entry name without "entry$">, at: None }` when the instruction at the failing `pc` is the `Async` op of the row `AsyncRunResult` (`AsyncRootResult` after AR-6). The check reads the instruction, not scheduler state, so nothing stale can name a later error. Until AR-6 the rule also names failures that leave a user's `Async.run` in `main`, which is why 7.4's first scope moves in AR-2.
- S-AR-3 (placement in `std/Async.em`): AR-2 adds `root` after the last function definition, directly above the comment `-- ランタイムのインターフェース。` (L362), so no position pinned by a snapshot moves (`<std>/Async.em:13:3..19:3`, `:171:25`).
- S-AR-4 (how the second input check is known): `check_input` returns which row it used and `finish_input` writes that row into the input function's type (the prototype read `body_types.body_yield != RowYield::Never`, which is equivalent for an input; AR-3 picks the clearer of the two). Translate decides "root" for `main` and inputs by one rule: the entry's declared type has the label `Std.Async.Async` in its first arrow's row.
- S-AR-5 (the nested root in `pinned.rs`): `children_of_a_pinned_task_stay_on_the_main_worker` keeps its program as the script rewrites it (the inner run becomes `Async.scope`); `pinned_tasks` is 3, the nested root task being gone (kind 2, 7.4).

## 6. Implementation constraints and conventions

- One unit = one commit; each passes G (7.3.1) and the SC lines in force (7.3.2).
- TDD for AR-2 and AR-3: the listed tests are written first and seen to fail.
- From AR-2 to AR-5 both forms work: the public `Async.run` stays until AR-6, after its last user is rewritten. Every commit is green.
- `docs/spec/` text is written in the commit that implements the rule (the "Docs" line of each unit); AR-7 rewrites what remains. Code comments cite `docs/spec/` and `docs/implementation/` only; docs name no stage.
- Japanese text: draft in English, PLaMo with `form: plain`, identifiers and section names in `glossary`, earlier parts of the same document as `history`; review; no space between Japanese and Latin characters or inline code, also removed from any paragraph edited.
- Snapshots are accepted with `cargo insta review` after reading each diff against 7.4; never `cargo insta accept` over the whole tree.
- Quote every `--include='*.x'` and glob (zsh). Run ad-hoc `.em` programs under `timeout 20`. Stop any Bash call moved to the background before finishing a unit.

## 7. Implementation tasks

### 7.1 Task table

| ID | Commit title | After | Decisions / criteria | Model |
|---|---|---|---|---|
| AR-1 | `Start async-root` | - | this plan, 00 §D D-10, the later plans' lines; baseline | orchestrator |
| AR-2 | `Run a main with Async as the root task` | AR-1 | D-AR-1, D-AR-2, D-AR-3, S-AR-1..3; S-21, AR-G1, AR-G2 | Opus |
| AR-3 | `Run Async inputs of the REPL as root tasks` | AR-2 | D-AR-5, S-AR-1, S-AR-4 | Opus |
| AR-4 | `Rewrite the .em programs and transcripts without Async.run` | AR-3 | D-AR-7 | Sonnet |
| AR-4b | `Do not suggest a second Async in the row of main` | AR-4 | E2002's help (user, 2026-11-19) | Opus |
| AR-5 | `Rewrite the Rust-embedded programs without Async.run` | AR-4 | D-AR-7, S-AR-5 | Sonnet |
| AR-6 | `Remove Async.run and nested roots` | AR-5 | D-AR-4, D-AR-6; G-2 (code) | Opus |
| CP-A | push, CI of AR-6, checkpoint extras, the gate of 10.1 | AR-6 | S-5 | orchestrator |
| AR-7 | `Rewrite the docs without Async.run` | CP-A | G-2 (docs) | Sonnet |
| AR-8 | `Close async-root` | AR-7 | roadmap 「進め方」 | Sonnet |
| CP-Z | CI of AR-8, then merge | AR-8 | S-7 | orchestrator |

### 7.2 Units

Every unit's completion includes **G** (7.3.1) and the **SC** lines in force (7.3.2).

### AR-1: `Start async-root`

- Work:
  1. `git rev-parse --short HEAD` → `3f007940`, or a later `main` whose `git diff --stat 3f007940 -- crates std tests bench` is empty (else stop: the lists of 7.4 are re-derived). `git switch -c plan/async-root`.
  2. Save the script of 7.5 outside the tree (`$S`, e.g. the session's scratch directory) and dry-run it: `python3 "$S"/rewrite.py --dry-run $(grep -rlE 'Async\.run\b' tests/ui bench crates/*/tests --include='*.em' --include='*.rs' --include='*.repl')` → first line `changed 217 of 223`, and the `MANUAL` lines equal 7.5's recorded list. A difference stops the unit.
  3. Baseline: section 9's table on the clean tree; fill "Recorded".
  4. Commit; message: D-10's restatements and the baseline.
- Completion: `git diff --name-only HEAD~1 HEAD` → the six plan files of section 4's last row; section 9 has no empty "Recorded" cell.

### AR-2: `Run a main with Async as the root task`

- Tests first (seen to fail):
  - `crates/eml_types/tests/check.rs`: `main_may_perform_async` (with `import Async (Async)`: the rows `<Async>`, `<IO, Async>`, `<Panic, Async>`, `<IO, Panic, Async>` give no diagnostic) and `main_with_async_and_another_effect_is_rejected` (E2004 with the text of S-AR-1).
  - `crates/eml_core_ir/tests/async.rs`: `an_async_main_is_wrapped_in_root`: the Core IR text of `entry$main` for `main : Unit -> <IO, Async> Unit` calls `Std.Async.root@[Unit]` with `&main` (inline snapshot), and a program without `Async` has no function named `Std.Async.root@..`.
  - `crates/eml_vm/tests/tasks.rs`: `main_is_the_root_task` (each of the four rows and the arity-0 form `main = fn () -> ..` runs; a task spawned and not awaited still prints before the run ends; `run_both`, i.e. checked and fast) and `a_root_failure_names_main` (a panic in `main`, and a panic in a spawned child, both give `panicked: boom in \`main\``).
  - UI: `tests/ui/run/async/main_is_the_root.em` (starts and awaits a task, leaves a sleeping child that prints last) and `tests/ui/run-fail/async/main_failure_names_main.em` (stdout up to the failure; the error line `... in \`main\``).
- Work:
  1. `eml_hir`: `Program::async_effect() -> Option<EffectId>`: the effect `Async` of the std module `Std.Async`, `None` when the module is not loaded (same search as `function_by_canonical_name`).
  2. `check_main`: the allowed labels are `io`, `panic` and `async_effect()` when it is `Some`; the text per S-AR-1.
  3. `std/Async.em` (S-AR-3): `root : (Unit -> <IO, Panic, Async> a) -> <IO, Panic> a` / `root f = run_result (run_raw (bottom f))`, private, with a comment that the entry wrapper is its only caller (docs/spec/stdlib.md の「`Std.Async`」).
  4. `translate/instances.rs` `collect`: after the entry instance, when `hir.async_effect()` is `Some(e)` and the entry's declared type (`typed.decls[&ValueItem::Function(entry)].ty`) is a function type whose row has `e`: `collector.add(root, vec![ret], vec![])`, `root` found by `function_by_canonical_name("Std.Async.root")` (`expect`: std defines it), `ret` the type after the first arrow; carried as `Instances.root: Option<InstanceId>` through `order`.
  5. `ProgramBuilder::entry` takes `root: Option<FnIdx>`: with a root, the call is `Call::Direct(root, [f])`, where `f` is `Atom::Fn(target)` when the target has parameters and the value returned by `Call::Direct(target, [])` when its arity is 0; the wrapper's `yields` is `false` (`root`'s row has no yielding label). The `after` part is untouched.
  6. `CodeBase::error`: S-AR-2.
- Docs: types.md (the entry rule, L178), modules.md 「モジュール」 (L15), effects.md 「組み込みの `IO`」 (L97), diagnostics.md (E2004), core-ir.md 「変換の規則」 (the entry function: the root wrapper and the instance of `root`), stdlib.md 「`Std.Async`」 (a `main` with `Async` is the root task; `root`; the failure's name) and 「タスクの上限」 (an `Async` `main`'s code runs at class `Task` and counts toward its task: a depth overflow in it is a catchable `ResourceLimit`), vm.md 「誤り、止めること、終わり」 (the name rule) and 「`Async` のタスク」.
- Kind 2 in this commit: 7.4 scopes K2-a and K2-g.
- Completion: G; `cargo nextest run -p eml_types -p eml_core_ir -p eml_vm -p eml_cli` passes; `timeout 20 cargo run -q -p eml_cli -- run --debug-heap tests/ui/run/async/main_is_the_root.em` exits 0; `timeout 20 cargo run -q -p eml_cli -- run --native tests/ui/run/async/main_is_the_root.em` exits 1 with E0004.

### AR-3: `Run Async inputs of the REPL as root tasks`

- Tests first: the transcript `crates/eml_cli/tests/repl/async/root_input.repl` (after `import Async`: `1 + 1`; `let x = Async.await (Async.start (fn () -> [1, 2, 3]))` then `x`; `Async.spawn (fn () -> println "child")`; `Async.await (Async.start (fn () -> panic "boom"))` → `... in \`input$N\``; `10 / 0` keeps `at <input N>:1:4`; an effect `Ask` performed by an input → E2015 with the new text; `println "next"`); `crates/eml_types/src/check/input.rs` tests `an_input_may_perform_async` and `an_input_without_async_keeps_its_row` (the input function's type is `Unit -> <IO, Panic> T` unless the second check ran).
- Work: `check_input` runs the body check under `[io, panic]`; if a diagnostic has the code `INPUT_EFFECT` and `program.async_effect()` is `Some`, it runs it again under `[io, panic, async]` and returns that result (S-AR-4); `finish_input` writes the row used; the E2015 text per S-AR-1. Nothing in `crates/eml_cli/src/repl.rs` or `eml_vm` changes.
- Docs: types.md 「REPL の入力」 (L526-539), repl.md (L100), modules.md 「REPL のセッション」, core-ir.md 「REPL の入力」, diagnostics.md (E2015).
- Completion: G; `cargo nextest run -p eml_types -p eml_cli --test integration -E 'test(repl::)'` passes; the new transcript's inputs run in checked mode: `sed -n 's/^> //p' crates/eml_cli/tests/repl/async/root_input.repl | timeout 30 cargo run -q -p eml_cli -- repl --debug-heap` prints no `memory leak` and no `internal error` line.

### AR-4: `Rewrite the .em programs and transcripts without Async.run`

- Work:
  1. Delete the kind 1 files of 7.4 (four `.em` files and their snapshots); `git mv` `handle_outlives_run.em` and its snapshot to `handle_outlives_scope` (7.4).
  2. `python3 "$S"/rewrite.py $(grep -rlE 'Async\.run\b' tests/ui bench crates/eml_cli/tests/repl --include='*.em' --include='*.repl')` → `changed 192 of 194` (the base's `196 of 198` [V: dry run] less the four deleted files) and three `MANUAL` comment lines (`tests/ui/run/std/std_atomic.em:1`, `std_chan.em:1`, `crates/eml_cli/tests/repl/async/run_on_main.repl:1`).
  3. By hand: those three comments; every comment of a rewritten file that still describes `run` (`grep -rn '`run`' <the changed files>`; e.g. `tests/ui/run-fail/async/cancelled_run.em:1`), reworded for "the root" or "the scope" as the code now reads.
  4. Snapshots: scopes K2-b, K2-c, K2-d, K2-g of 7.4.
- Completion: G; `grep -rn 'Async\.run\b' tests bench crates/eml_cli/tests/repl` → empty; `cargo nextest run -p eml_cli --test integration -E 'test(ui::) | test(repl::)'` passes; by hand `timeout 20 cargo run -q -p eml_cli -- run --gui stub tests/ui/run/gui/counter.em` and one `bench/net/` program through `bench/net.sh --only base` (or its `eml run` line) run as before.

### AR-4b: `Do not suggest a second Async in the row of main`

- Purpose (added with the user, 2026-11-19): E2002's fix-it help "add `X` to the row of the signature of `main`" must not be given when the label is `Std.Async.Async` and `main`'s row already has it, since a repeated `Async` in `main`'s row is E2004 (D-AR-1, AR-2's review fix). The error and its labels stay; only the help goes.
- Tests first: a check-fail file `tests/ui/check-fail/types/main_with_async_performs_a_second_async.em` (or a unit test in `crates/eml_types/tests/check.rs`) pinning that E2002 has no such help for the shape of `compute_performs_async`; the help for a `main` without `Async` that performs `Async` stays (an existing pin, or a new one).
- Work: in the E2002 reporter (`crates/eml_types/src/check/report.rs`, the `fixable` / help path), suppress the help for the label `Async` when the signature is `main`'s and its row already has `Async`. Name the rule in a comment citing docs/spec/types.md (the entry rule).
- Kind 2: `types__compute_performs_async` loses its help line.
- Completion: G; SC-1, SC-3, SC-4; `cargo nextest run -p eml_types -p eml_cli --test integration -E 'test(ui::check_fail)'` passes.

### AR-5: `Rewrite the Rust-embedded programs without Async.run`

- Work:
  1. Delete the kind 1 tests of 7.4 (five tests).
  2. `python3 "$S"/rewrite.py $(grep -rlE 'Async\.run\b' crates/*/tests --include='*.rs')` → `changed 21 of 25` at the base [V: dry run] and the `MANUAL` lines of 7.5 that name `.rs` files, less the deleted tests.
  3. By hand, the manual class: `crates/eml_cli/tests/repl.rs:465` (the input loses its wrapper), `crates/eml_core_ir/tests/async.rs:7` and `perceus.rs:435` (`format!` templates: `main`'s row gains `Async.Async` or `Async`, the outer run is unwrapped, the inner one becomes `Async.scope (fn _ -> ..)`), `crates/eml_native/tests/unsupported.rs:37, 50` (`\n\`-continued strings: same rule), and the comment lines listed in 7.5.
  4. `crates/eml_vm/tests/miri/blocking.lir`: regenerate from the pipeline's output as `miri::a_blocking_call_runs_on_the_test_driver` instructs (its assertion message names the file).
  5. Expected values: scope K2-e of 7.4.
- Completion: G; `grep -rn 'Async\.run\b' crates --include='*.rs' --include='*.lir' | grep -v '/src/'` → empty; `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::` passes.

### AR-6: `Remove Async.run and nested roots`

- Work:
  1. `std/Async.em`: delete `pub run` (L74-75) and the placement comment (L77-79); rename `run_raw` → `root_start`, `run_result` → `root_result` in `root`, the externs and the runtime-interface comment (which also loses `run` from its list of signatures and gains `root`).
  2. `eml_extern` (`crates/eml_extern/src/lib.rs:1314-1324, 1415-1425, 2288, 2297`): `AsyncRunRaw` → `AsyncRootStart` (`"Std.Async.root_start"`), `AsyncRunResult` → `AsyncRootResult` (`"Std.Async.root_result"`); `crates/eml_rows/src/rows.rs:579, 588`; `crates/eml_core_ir/tests/externs.rs:649, 656, 739, 746`; `CodeBase::error`'s rule; `blocking.lir` regenerated.
  3. `vm.rs` root arm: a root starts only from the entry function, so `task.cell.is_some()` is `RunFault::Internal("a root starts inside a task")`; the `Some(cell)` branch, the parent scope and the inherited `pinned` go (the root task is started with `pinned = false`).
  4. `cells.rs`: `open_root()` takes no parameter (`new_scope(None, None, true)`); `TaskCell.run` and its three uses go (:1954, :2546-2548, the `run` link of the cancel mark at :2777-2781); the module docs at :2, :20, :22 follow. The 50 callers: `grep -rl 'open_root(None, None)' crates | xargs perl -pi -e 's/open_root\(None, None\)/open_root()/g'` (kind 3; `perl` because the dev shell's `sed` is GNU and macOS's is BSD).
  5. Comments in `crates/*/src` that name `Async.run`, a nested run or the old row names (`vm.rs`, `sched.rs:301-303`, `workers.rs`, `cells.rs`, `eml_nrt/src/run.rs:207`).
- Tests: none new. The arm's `Internal` error cannot be reached from eml source (`root` is private and translate calls it only from the entry wrapper), so it is reviewed by reading; the existing root tests of `cells.rs` call `open_root()`.
- Docs: stdlib.md 「`Std.Async`」 (the API list without `run`; L37, L1086, L1102, L1123-1125 for the rows), 「タスクの上限」 (L1156: the nested-run clause goes; D-AR-6), 「メインのworker」 (L1273: no pinned nested root); vm.md 「`Async` のタスク」 (L738, L837-839, L873-875, L892-896, L983-986: the rows renamed, `root_start` only from the entry function, no parent, no `TaskCell.run`), 「待ちの2種類」 (L822: the token wait is the root wrapper's), 「タスクの上限」 (L1028: the nested-run item goes); core-ir.md (L215, L220), declarations.md (the held-argument list).
- Kind 2 in this commit: scope K2-f.
- Completion: G; SC-2's code line empty; `grep -c '^pub ' std/Async.em` → `37`; `cargo nextest run -p eml_sched --lib` passes.

### CP-A

Push `plan/async-root`; CI green; the checkpoint extras (7.3.3); the gate of 10.1 on AR-6's commit against the base. A row over +0.5 % stops the stage; the orchestrator brings the table to the user.

### AR-7: `Rewrite the docs without Async.run`

- Work: the remaining hits of SC-2's docs line (at the base: spec 51 lines, implementation 5, future 22, less what AR-2, AR-3 and AR-6 rewrote): stdlib.md (the examples at L858, L966, L999-1000, L1413-1929 follow the rule of 7.5: `main`'s row gains `Async`, a whole-body run is unwrapped), vm.md, expressions.md, core-ir.md, declarations.md, testing.md, status.md, benchmarks.md (L762), async.md (decision 2), multicore.md (「`Async.run` と入れ子」 deleted, the other mentions rewritten), native.md, ffi.md, roadmap.md (items other than `async-root`'s own, which AR-8 deletes). Also `grep -rn '入れ子の `run`\|入れ子の`run`\|入れ子のrun\|入れ子の`Async' docs/spec docs/implementation docs/future crates std` → empty.
- Completion: G; SC-2 empty on both lines; `cargo nextest run -p eml_cli --test integration -E 'test(/^citations::/)'` passes.

### AR-8: `Close async-root`

Roadmap 「進め方」, one commit:
1. spec: no stage name and no 「今は〜、後で〜」 in the sections this stage wrote.
2. Measure (`crates/eml_vm`, `crates/eml_core_ir`, `std/` and `bench/` changed): `bench/scripts.sh` first (a row out of range → `Resize bench rows`); then `bench/run.sh`, `bench/rt.sh`, `bench/scripts.sh` on the clean tree of AR-6's commit → the three tables of benchmarks.md 「今の数値」. Without macOS: `Measure async-root` right after.
3. status.md: `main` and REPL inputs with `Async`; no `Async.run`.
4. architecture.md: none expected (no crate or pass changed).
5. roadmap.md: delete the `async-root` item; `task-limits`' 「前提」 becomes `par-total`, `sched-core`'s and `jit-tasks`' drop `async-root`; the `jit-tasks` item gains one sentence: native code splits the root wrapper at its wait into a start function the engine calls first and the entry, and the VM follows (the hand-off of 00 §D D-10 (b), stated without naming a plan).
6. overview.md's table and docs/README.md's 「内容」, only where changed.
7. CLAUDE.md: none expected (no rule or pointer names `Async.run`; the Topics row `Std.Async` tasks only if a section it names was renamed).
8. Delete `docs/plans/2026-11-17-async-root.md` (the appendix directory stays).
9. Message: test changes by kind with reasons, the gate table of CP-A, the measurements, D-10's date (2026-11-19).
- Completion: G; `citations::` passes; `ls docs/plans | grep -c async-root` → `0`; `sed -n '/^## 段の列/,/^## その後の項目/p' docs/future/roadmap.md | grep -c '^- `'` → `11`.

### 7.3 Gates and standing checks

#### 7.3.1 G (every unit, repository root, after `git add -A`)

1. `cargo nextest run` → exit 0; `N tests run: N passed[ (k slow)], 23 skipped`, N = 5302 plus the tests the units add and minus the five AR-5 deletes (each unit's message states its delta; a UI file or transcript is not a test of its own).
2. `cargo clippy --all-targets -- -D warnings` → exit 0.
3. `cargo fmt --check` → exit 0.
4. `bash nix/crate-graph.sh --check` → exit 0.
5. Snapshot and expectation changes are only those of 7.4 for the unit (`git diff --stat HEAD~1 -- 'crates/*/tests/snapshots' tests/ui crates/eml_cli/tests/repl` and the inline expectations, reviewed against 7.4).

#### 7.3.2 SC (standing checks)

```sh
# SC-1 (always): no docs/plans citation in code or docs
grep -rn 'docs/plans/20' crates docs --include='*.rs' --include='*.md' | grep -v '^docs/plans/'      # empty
# SC-2 (AR-6 on: first line; AR-7 on: both; G-2)
grep -rn 'Async\.run\b\|run_raw\|run_result\|AsyncRunRaw\|AsyncRunResult' crates std tests bench     # empty
grep -rn 'Async\.run\b\|run_raw\|run_result\|AsyncRunRaw\|AsyncRunResult' docs/spec docs/implementation docs/future   # empty
# SC-3 (always; AR-G1): programs without Async are untouched
git diff --stat 3f007940 -- crates/eml_vm/tests/bench.rs                                               # empty
cargo nextest run -p eml_core_ir -E 'test(every_program_lowers_to_the_same_text)'                      # passes
# SC-4 (always; AR-G2)
git diff --stat 3f007940 -- crates/eml_rt nix/crate-graph.sh crates/eml_cli/tests/surface.rs           # empty
git diff 3f007940 -- crates std | grep -c '^+.*\bunsafe\b'                                             # 0 (AR-6 may move lines inside the root arm: each hit is a line that existed)
git diff 3f007940 -- crates/eml_extern/src/lib.rs | grep -c 'RuntimeEntry'                             # 0
```

#### 7.3.3 Checkpoint extras (before CP-A and CP-Z)

```sh
nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run -- -D warnings   # exit 0
nix develop .#cross -c cargo clippy --target x86_64-unknown-linux-gnu --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run -- -D warnings  # exit 0
nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=thread TSAN_OPTIONS=halt_on_error=1 CARGO_TARGET_DIR=target/tsan cargo test -Zbuild-std --target $(rustc -vV | sed -n 's/^host: //p') -p eml_vm --test integration -- workers:: signal:: io:: chan:: ffi:: ffi_abandon:: seat:: pinned::   # pass
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::   # pass
```

### 7.4 Test changes by kind

Kind 1 (pass / fail), one by one:
- Deleted in AR-4 (their subject is the nested root; each has a `run` inside a task or a non-`main` function): `tests/ui/run/async/nested_run.em`, `nested_run_cancelled.em`, `cancel_reaches_nested_run.em`, `limit_depth_nested_run.em`, with `crates/eml_cli/tests/snapshots/integration__ui__run@async__<name>.em.snap`. What survives of their facts is checked by the scope files: cancellation reaching a scope's tasks (`failed_child_cancels_siblings.em` and the `cancel_*` files), the depth limit inside a scope (`limit_depth_fails_scope.em`).
- Deleted in AR-5:
  - `crates/eml_vm/tests/tasks.rs` `a_run_inside_a_task_waits_for_its_own_scope` (a nested root).
  - `crates/eml_vm/tests/memory.rs` `a_task_in_a_nested_run_is_flagged` (a nested root) and `a_task_that_grows_without_switching_after_a_crossing_without_a_candidate_is_flagged` (its subject is a `main` outside every task crossing the soft limit before a task exists; an `Async` `main` is itself a task and is the candidate [V: proto: the rewritten program prints nothing]).
  - `crates/eml_vm/tests/workers.rs` `a_start_after_a_nested_run_places_on_another_worker` and `a_nested_run_inside_a_task_on_worker_1_ends` (nested roots; rewritten they pass but state what the placement tests of the same file state).
- Added: AR-2's six tests and two UI files; AR-3's transcript and two unit tests.

Kind 2 (expected values), as scopes:
- K2-a (AR-2; the name of the root's failure, S-AR-2): the 16 `run-fail` snapshots that print `at <std>/Async.em:75:9` (`async__{await_rethrows_panic, blocking_fixture_null_handle, blocking_fixture_range_panic, cancelled_run, caught_await_still_fails_run, compute_out_of_range, exec_needs_the_real_driver, exec_pipe_stderr_on_run, exec_write_after_exit, limit_depth_uncaught, limit_memory_uncaught, run_on_main_panics, spawned_failure_fails_run, stream_missing_file}`, `gui__wait_without_window`, `tls__cancelled_setup`) and `repl__transcripts@async__stale_scope.repl.snap`: the two lines `<failure>` / `  at <std>/Async.em:75:9` become one, `<failure> in \`main\`` (`in \`input$4\`` in the transcript); `crates/eml_vm/tests/tasks.rs:129-135` (`await_raises_the_panic_of_the_task_again`) asserts `panicked: boom in \`main\``.
- K2-b (AR-4; the position of a failure raised in `main` after its run, now inside the root task): the 10 `run-fail` snapshots `http__{downgrade, too_many_redirects, truncated}`, `tls__{bad_ca, line_too_long, no_roots, server_closes_before_handshake, truncated, unknown_ca, write_after_shutdown}`: `<failure>` / `  at run-fail/<file>:l:c` becomes `<failure> in \`main\``. This is D-AR-3's rule for user code: an uncaught failure anywhere in an `Async` `main` crosses the task's bottom as a value and has no position.
- K2-c (AR-4; positions on rewritten lines): the check-fail snapshots `linearity__task_awaited_twice`, `names__async_op_handled`, `types__{async_in_par_child, compute_in_par_child, compute_performs_async, compute_performs_io, task_captures_var}` (line and column of the user's file only; codes and wording unchanged), except `types__compute_performs_async` (amended with the user, 2026-11-19): with `compute` called directly in an `Async` `main`, its E2002 names "the signature of `main`" instead of `run`'s lambda, at 7:3, and its E2020 moves to 7:3. AR-4 accepts that snapshot as it comes out (including a help line "add `Async` to the row of the signature of `main`"); AR-4b then removes that help line (its own kind 2).
- K2-d (AR-4): the nine transcript snapshots echo the rewritten inputs (no output line changes [V: proto]); `integration__ui__run@async__handle_outlives_run.em.snap` moves to `..handle_outlives_scope.em.snap` with its file (a move without a pass/fail change; the comment says a handle outlives the scope that started its task).
- K2-e (AR-5; counts asserted by value, each +1 for the `Async.scope` that replaced a run in a position the script cannot unwrap): `crates/eml_vm/tests/io.rs:61` `(4, 5, 0)` → `(4, 6, 0)` and `:76` `(5, 5)` → `(5, 6)` (`parks`, through the helper `connected_stats`); `crates/eml_vm/tests/tasks.rs` the second element of every tuple in `timeout_runs_the_body_in_the_calling_task` (`(2, 3, 1, 1)` → `(2, 4, 1, 1)`, ...), `race_decides_the_winner_in_one_claim` (`(3, 5, 2, 1, 0)` → `(3, 6, 2, 1, 0)`, ...) and `drain_within_waits_until_the_scope_has_no_task` (`(4, 7, 2, 1)` → `(4, 8, 2, 1)`); `crates/eml_vm/tests/pinned.rs:116` `pinned_tasks` 4 → 3 (S-AR-5) with its doc comment; `crates/eml_vm/tests/miri/blocking.lir` (regenerated; also in AR-6); inline Core IR / LIR snapshots of the two hand-rewritten templates in `crates/eml_core_ir/tests/{async,perceus}.rs`, if they print the entry (`c_functions.rs` and `lowering.rs` pass as the script rewrites them [V: proto]).
- K2-g (AR-2, AR-4; the native tallies follow the files added and deleted, each still refused for a task row): `integration__ui__run_native.snap` `rejected 151 → 152`, `async row 129 → 130` in AR-2 (`main_is_the_root.em`) and in AR-4 (the four deleted files; `limit_depth_nested_run.em` was `-- native: skip`, corrected with the user 2026-11-19) `skipped 6 → 5`, `rejected 152 → 149`, `async row 130 → 127`; `integration__ui__run_fail_native.snap` `rejected 34 → 35`, `async row 33 → 34` in AR-2 (`main_failure_names_main.em`).
- K2-f (AR-6): `<std>/Async.em:171:25` in `types__compute_performs_{async,io}` moves up by the lines deleted above `compute`; `blocking.lir`, and any IR text or snapshot that prints `Std.Async.run_raw`, `Std.Async.run_result` or `Std.Async.root@..`'s body, by the two row names.

Kind 3: every other rewritten file (expectations byte-identical: all 136 `run` files, the 25 `run-fail` files not in K2-a/K2-b, `chan_invariant`, the 10 `bench/` programs, the Rust tests not named above [V: proto]); the 50 `open_root()` call sites; comments.

### 7.5 The rewrite script (not in the tree; tested on a scratch worktree at 3f007940)

Rule (D-10 (h)): in a function `main` with the signature `main : Unit -> <IO> Unit` or `<IO, Panic>`, or in a transcript input:
- a first statement `use Async.run` is deleted;
- a body that is exactly `Async.run IDENT` becomes `IDENT ()`; a body that is exactly `Async.run (fn () -> ..)` (the lambda's closing `)` is the body's last character) becomes the lambda's body, re-indented as `main`'s block;
- every other use: `Async.run IDENT` → `Async.scope (fn _ -> IDENT ())`, `Async.run (fn () ->` → `Async.scope (fn _ ->`. `scope` waits for the tasks its body starts and lets the first failure win, as `run` did (stdlib.md 「スコープ」, std/Async.em:95-106);
- `main`'s row gains `Async` when the text before `main` has `import Async (.., Async, ..)`, else `Async.Async` (a program that wrote `Async.run` has the module name in scope);
- printed as `MANUAL path line reason` and left alone: a use outside `main` (`outside main`), a comment line that names `Async.run` (`comment`), a use the patterns do not cover (`unknown shape`; none at the base).

```python
#!/usr/bin/env python3
# One-off: rewrite the uses of Async.run (async-root, 7.5). Usage: rewrite.py [--dry-run] FILE...
# .em: the file is one program. .repl: every `> ` line is an input. .rs: every program whose lines sit at column 0
# of the Rust file (a "\-continued string) and every escaped one-line program
# ("..\nmain : Unit -> <IO, Panic> Unit\nmain () = ..").
import re, sys

RUN = re.compile(r'\bAsync\.run\b')
SIG = re.compile(r'^main : Unit -> <IO(, Panic)?> Unit\s*$')
HEAD = re.compile(r'^main (\(\) )?=')
IDENT = r"[a-z_][A-Za-z0-9_']*"
RUN_IDENT = re.compile(r'\bAsync\.run (' + IDENT + r')(?![A-Za-z0-9_\'.]| *\()')
RUN_LAMBDA = re.compile(r'\bAsync\.run \(fn \(\) ->')
ESCAPED = re.compile(r'\\nmain : Unit -> <IO(, Panic)?> Unit\\nmain \(\) = ')


def close_of(text, at, rust):
    """Index of the `)` that closes the `(` at `at`, skipping eml strings and `--` comments; None if unbalanced."""
    quote = '\\"' if rust else '"'
    depth, i, n = 0, at, len(text)
    while i < n:
        if text.startswith(quote, i):
            i += len(quote)
            while i < n and not text.startswith(quote, i):
                if rust and text.startswith('\\\\', i):
                    i += 4 if text.startswith('\\"', i + 2) else 3
                elif not rust and text[i] == '\\':
                    i += 2
                else:
                    i += 1
            i += len(quote)
            continue
        if text.startswith('--', i):
            j = text.find('\n', i)
            i = n if j < 0 else j
            continue
        if text[i] == '(':
            depth += 1
        elif text[i] == ')':
            depth -= 1
            if depth == 0:
                return i
        i += 1
    return None


def async_name(program):
    """How the row names the effect: `Async` where the text before `main` imports the name, else `Async.Async`
    (a program that writes `Async.run` has the module name in scope)."""
    for m in re.finditer(r'(?:^|\\n|\n|")import Async \(([^)]*)\)', program):
        if 'Async' in [item.strip() for item in m[1].split(',')]:
            return 'Async'
    return 'Async.Async'


def rewrite_body(body, rust):
    """`body` is the text after `main () = ` / `main = `. Returns the new text and the notes for manual work."""
    notes = []
    eol = '\\n' if rust == 'escaped' else '\n'
    # a first statement `use Async.run`: the rest of `main` was the run's body
    body = re.sub(r'^((?:[ \t]*\n)*)[ \t]*use Async\.run[ \t]*\n', r'\1', body, count=1)
    stripped = body.rstrip() if rust != 'escaped' else body
    tail = body[len(stripped):]
    lead = stripped[:len(stripped) - len(stripped.lstrip())]
    core = stripped.lstrip()
    # the whole body is the run: unwrap it
    if m := re.fullmatch(r'Async\.run (' + IDENT + r')', core):
        body = lead + m[1] + ' ()' + tail
    elif core.startswith('Async.run (fn () ->') and rust != 'escaped':
        if close_of(core, len('Async.run '), bool(rust)) == len(core) - 1:
            inner = core[len('Async.run (fn () ->'):-1]
            inner = inner[1:] if inner.startswith(' ') else inner
            if inner.startswith('\n'):
                # the lambda's body takes the place of the run: at the run's indentation, or as the block of `main`
                rows = inner[1:].split('\n')
                depth = min(len(row) - len(row.lstrip(' ')) for row in rows if row.strip())
                width = len(lead) - len(lead.rstrip(' ')) if '\n' in lead else 2
                rows = [' ' * width + row[depth:] if row.strip() else row for row in rows]
                body = '\n' + '\n'.join(rows) + tail
            else:
                body = lead + inner + tail
    # every other use waits for its tasks as a scope does
    out = []
    for line in body.split(eol):
        if line.lstrip().startswith('--'):
            notes += ['comment'] if RUN.search(line) else []
        else:
            line = RUN_LAMBDA.sub('Async.scope (fn _ ->', RUN_IDENT.sub(r'Async.scope (fn _ -> \1 ())', line))
            notes += ['unknown shape'] if RUN.search(line) else []
        out.append(line)
    return eol.join(out), notes


def rewrite_inputs(lines, path, manual):
    """A transcript: each `> ` line is an input, and an input is the root of its own run."""
    changed = False
    for i, line in enumerate(lines):
        m = re.match(r'> (let [^=]*= )?', line)
        if not RUN.search(line):
            continue
        if not m:
            manual.append((path, i + 1, 'comment' if line.startswith('#') else 'outside main'))
            continue
        new, notes = rewrite_body(line[m.end():], '')
        manual += [(path, i + 1, note) for note in notes]
        lines[i] = line[:m.end()] + new
        changed = True
    return changed


def rewrite_lines(lines, path, manual):
    if path.endswith('.repl'):
        return rewrite_inputs(lines, path, manual)
    rust = path.endswith('.rs')
    changed = False
    mains = set()
    i = 0
    while i < len(lines):
        if rust and (m := ESCAPED.search(lines[i])):
            # an escaped one-line program: from the marker to the end of the physical line
            head, body = lines[i][:m.end()], lines[i][m.end():]
            mains.add(i)
            if RUN.search(body):
                new, notes = rewrite_body(body, 'escaped')
                name = async_name(lines[i][:m.start()])
                head = head.replace('> Unit\\nmain', ', ' + name + '> Unit\\nmain')
                manual += [(path, i + 1, note) for note in notes]
                if new != body:
                    lines[i] = head + new
                    changed = True
            i += 1
            continue
        if not SIG.match(lines[i]) or i + 1 >= len(lines) or not HEAD.match(lines[i + 1]):
            i += 1
            continue
        sig, head = i, i + 1
        end = head + 1
        while end < len(lines) and (not lines[end].strip() or lines[end][0] in ' \t'):
            end += 1
        while not lines[end - 1].strip():
            end -= 1
        mains.update(range(sig, end))
        text = ''.join(lines[head:end])
        if RUN.search(text):
            start = head
            while start > 0 and not (rust and lines[start - 1].rstrip().endswith('"\\')):
                start -= 1
            cut = HEAD.match(text).end()
            new, notes = rewrite_body(text[cut:], 'lines' if rust else '')
            name = async_name(''.join(lines[start:sig]))
            manual += [(path, head + 1, note) for note in notes]
            if new != text[cut:]:
                new = text[:cut].rstrip(' ') + ('' if new.startswith('\n') else ' ') + new.lstrip(' ')
                lines[sig] = lines[sig].replace('> Unit', ', ' + name + '> Unit')
                lines[head:end] = new.splitlines(keepends=True)
                end = head + len(new.splitlines())
                mains.update(range(sig, end))
                changed = True
        i = end
    for i, line in enumerate(lines):
        if RUN.search(line) and i not in mains:
            comment = line.lstrip().startswith(('--', '//', '#'))
            manual.append((path, i + 1, 'comment' if comment else 'outside main'))
    return changed


if __name__ == '__main__':
    dry = '--dry-run' in sys.argv[1:]
    files = [a for a in sys.argv[1:] if a != '--dry-run']
    manual, count = [], 0
    for path in files:
        lines = open(path).read().splitlines(keepends=True)
        if rewrite_lines(lines, path, manual):
            count += 1
            if not dry:
                open(path, 'w').write(''.join(lines))
    print('changed', count, 'of', len(files))
    for entry in sorted(set(manual)):
        print('MANUAL', *entry)
```

Recorded at 3f007940 over the 223 files (189 `.em`, 9 `.repl`, 25 `.rs`): `changed 217 of 223`; a second run prints `changed 0`; the line numbers are those after the rewrite. Every rewritten `run` / `run-fail` `.em` file type-checks on the prototype (`eml check`, 175 files, no diagnostic).

```
MANUAL crates/eml_cli/tests/repl.rs 465 outside main                    # a REPL input in a Rust string
MANUAL crates/eml_cli/tests/repl/async/run_on_main.repl 1 comment
MANUAL crates/eml_cli/tests/ui.rs 1299 comment
MANUAL crates/eml_core_ir/tests/async.rs 7 outside main                 # format! template
MANUAL crates/eml_core_ir/tests/perceus.rs 435 outside main             # format! template
MANUAL crates/eml_native/tests/unsupported.rs 37 outside main           # "\n\"-continued string
MANUAL crates/eml_native/tests/unsupported.rs 50 outside main
MANUAL crates/eml_vm/tests/io.rs 1340 comment
MANUAL crates/eml_vm/tests/memory.rs 153 comment                        # deleted with its test (kind 1)
MANUAL crates/eml_vm/tests/memory.rs 162 outside main                   # deleted with its test (kind 1)
MANUAL crates/eml_vm/tests/miri.rs 632 comment
MANUAL crates/eml_vm/tests/pinned.rs 91 comment
MANUAL crates/eml_vm/tests/tasks.rs 1 comment
MANUAL crates/eml_vm/tests/tasks.rs 28 comment                          # deleted with its test (kind 1)
MANUAL crates/eml_vm/tests/tasks.rs 41 comment
MANUAL crates/eml_vm/tests/workers.rs 651 comment                       # deleted with its test (kind 1)
MANUAL crates/eml_vm/tests/workers.rs 846 comment                       # deleted with its test (kind 1)
MANUAL tests/ui/run/async/cancel_reaches_nested_run.em 16 outside main  # deleted (kind 1)
MANUAL tests/ui/run/async/limit_depth_nested_run.em 20 outside main     # deleted (kind 1)
MANUAL tests/ui/run/async/nested_run.em 11 outside main                 # deleted (kind 1)
MANUAL tests/ui/run/async/nested_run_cancelled.em 16 outside main       # deleted (kind 1)
MANUAL tests/ui/run/std/std_atomic.em 1 comment
MANUAL tests/ui/run/std/std_chan.em 1 comment
```

A run inside a task that sits inside `main` (`crates/eml_vm/tests/pinned.rs:101`, `api.rs:309`, `crates/eml_rows/tests/gui.rs:580-581`) is rewritten to `Async.scope` like any other use; those tests pass as rewritten [V: proto] and only their comments are reworded.

## 8. Execution order

```
AR-1 → AR-2 → AR-3 → AR-4 → AR-4b → AR-5 → AR-6 → [CP-A: CI + extras + gate] → AR-7 → AR-8 → [CP-Z] → merge
```

Order rationale: the root exists end to end for `eml run` first (AR-2), then for the REPL (AR-3), so the rewrites have something to be rewritten to; the rewrites while `Async.run` still works, so every commit is green and each rewritten file is checked against its old expectation; the deletion after the last user; the gate on the last code commit, the only one that edits the scheduler's files; the docs last, since AR-6 settles the names.

## 9. Verification plan

Baseline (AR-1, clean tree, after `cargo build --all-targets`; fill "Recorded"):

| Item | Command | Expected | Recorded |
|---|---|---|---|
| full run | `cargo nextest run` | `5302 tests run: 5302 passed, 23 skipped` [V: list; I: run] | `5302 tests run: 5302 passed, 23 skipped` (55.7 s, no slow) at 3f007940, macOS aarch64 |
| native snapshots | `cat crates/eml_cli/tests/snapshots/integration__ui__run_native.snap crates/eml_cli/tests/snapshots/integration__ui__run_fail_native.snap` | run: `accepted 492`, `skipped 6`, `rejected 151` (`async row 129`, `par 22`), `unlowered 27`; run-fail: `accepted 45`, `checked only 0`, `skipped 2`, `rejected 34` (`async row 33`, `par 1`) [V] | as expected, both files |
| script dry run | AR-1 step 2 | `changed 217 of 223` and 7.5's list [V: proto] | `changed 217 of 223`; the `MANUAL` lines equal 7.5's list |
| G-2 grep | SC-2, both lines, with `-l` and `\| wc -l` | code and tests 259 files less docs [V: census]; docs 78 lines | code and tests 243 files (this pattern has `AsyncRunResult`, not `WaitToken`); docs 77 lines |
| `pub` items | `grep -c '^pub ' std/Async.em` | `38` [V] | `38` |
| `bench/run.sh` base | 10.1, three runs at 3f007940 | record the medians | measured at CP-A in alternating pairs with AR-6 (10.1) |
| crate graph | `bash nix/crate-graph.sh --check` | exit 0 | exit 0 |
| citations | `cargo nextest run -p eml_cli --test integration -E 'test(/^citations::/)'` | pass | `4 tests run: 4 passed` |

Overall (on AR-8's commit): `cargo nextest run` passes with 7.4 applied; every SC line; the `async/` UI files at N = 1, 2, 8 and both seeds (part of `ui::run` / `ui::run_fail`); the gate of 10.1; CI `success` on every job (S-7).

## 10. Risks and unresolved items

### 10.1 The performance gate (01 §19 "Gates")

`bench/run.sh` prints one table of medians and the build directory changes counts (benchmarks.md 「命令の数の測り方」), so base and head are built and measured alternately in one worktree: `git switch --detach 3f007940`, `bench/run.sh`, `git switch --detach <AR-6>`, `bench/run.sh`, three pairs, the median per row. The compared column is 実行の分 (instructions minus `empty`'s). Every row of `bench/run.sh` is compared (none runs an `Async` program); `bench/async/`, `bench/net/` and the `https-get` row are not gated (recorded at Close). The commit measured is AR-6, the last code commit and the only one that edits `vm.rs`'s `async_op` and `cells.rs`; AR-2 touches only the cold `CodeBase::error`. A row over +0.5 % stops the stage; the orchestrator brings the table to the user.

### 10.2 Risks

| # | Item | Default |
|---|---|---|
| K-1 | K2-a moves in AR-2 by inference: the prototype applied the name rule and the rewrite together | if a snapshot of K2-a does not move in AR-2, it moves in AR-4 with its file; same scope, noted in the commit message |
| K-2 | A rewritten file changes its output on CI's other targets or under a seed (a task now outlives `main`'s body, or `Async.scope` orders differently) | none seen on macOS at the default run [V: proto]; a change is kind 2 only if 7.4 covers it, otherwise stop (AR-G3) |
| K-3 | An uncaught failure in an `Async` `main` has no position (K2-b): a regression against a synchronous `main` that D-AR-3 accepts | the roadmap item 「実行時エラーの backtrace」 is where a position through a task's bottom would come from; AR-7 adds one sentence there |
| K-4 | The entry's `yields` mark or the function value of the target is wrong for a shape the prototype did not run (a `main` with a constraint is already refused; an input whose value is a struct) | the debug-build verifiers fail loudly; AR-3's transcript includes a tuple-valued `Async` input |
| K-5 | `open_root()`'s cleanup changes the cancel mark (`cells.rs:2777`) | the `run` link is `None` for every task once no nested root exists, so the deleted branch is dead; `cargo nextest run -p eml_sched --lib` and the `async/` UI runs cover the rest |
| K-6 | A soft-limit test other than the deleted one depends on `main` running outside every task | none failed on the prototype; a new failure is kind 1 or 2 and stops the unit (AR-G3) |
| K-7 | JT-5's split (D-10 (b)) is a default, not a verified design | recorded in the `jit-tasks` plan and the roadmap item; JT-1 confirms it |
| K-8 | Pushes | the orchestrator pushes `plan/async-root` for the checkpoints; merging is asked at the end |
