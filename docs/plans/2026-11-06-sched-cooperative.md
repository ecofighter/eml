# sched-cooperative: eager workers, a placement ring, no rescue, no VM preemption

- Created: 2026-11-06
- Base: main @ f693bf2d
- Status: plan finalized (no unresolved item needs the user)

## 1. Goal and success criteria

- Goal: make the scheduler cooperative and thread-per-core, in one shape for the VM and the future native code. Workers 1..N-1 start at the beginning of a run. `start` places `Async` tasks on workers 1..N-1 in rotation, and worker 0 runs `main`. A task runs on the worker it was placed on until it ends, and switches only at `Async` operations. Rescue and VM preemption are removed. This stage is the base of `main-pinning` (docs/plans/2026-11-06-main-pinning.md).
- Success criteria:
  - The whole test suite passes with only the test changes listed in section 7, one by one (9 "overall verification").
  - No identifier of rescue, preemption or lazy spawn remains in `crates` (grep in S1-T2 and S1-T3).
  - `cargo verus verify -p eml_rt` reports 0 errors after the `PREEMPT` removal.
  - docs/ describe the new contract, and the grep sweep of S1-T4 leaves only the listed residuals.

## 2. Scope

- In scope: eager spawn of workers 1..N-1; the placement ring (worker 0 leaves the `start` rotation when N ≥ 2); removal of rescue (monitor stuck/overload detection, steal, reservation, futile marks, `RescueAlways`, entry counting); removal of VM preemption (`Control::PREEMPT`, the monitor's tick, `Policy::PREEMPTS`); the spec, implementation and future docs, roadmap and CLAUDE.md for these; the benchmark refresh at the close.
- Out of scope: the main worker and pinned tasks (`main-pinning`); `par-steal`; pool-backed `Stdin`/`FileIn` reads (roadmap item); a Verus proof of cooperative placement (roadmap item); making CLI tests kill their `eml` child on timeout (roadmap item).

## 3. Background context

- Workers 1..N-1 start lazily today: the first `par` fork, `start` or `fork_job` returns `Exit::Spawn`, `run_scheduled` spawns them and re-executes the instruction (`crates/eml_vm/src/lib.rs:1003-1032`; checks at `vm.rs:1542`, `:2533`, `:2643`, `:2719`; `task_failed` case `:2340`). The condition for wanting workers is in `begin_run` (`sched.rs:791-809`).
- Placement: `Run::next_owner` rotates over every live worker, worker 0 included (`workers.rs:424`); `par` children and `compute` jobs go to an idle worker through `pop_idle` (`vm.rs:1635`, `:2640`).
- Rescue: the monitor's `rescue_tick` (`monitor.rs:151`), `steal` (`sched.rs:101`), `steal_start/resume/wake` (`cells.rs:2089-2150`), `rescuable` (`workers.rs:81`, a stub returning true), `RescueAlways` (`ask_rescue`, `sched.rs:1087`), `TaskCell.entries`, `Registry::leave`, slot fields `ready_entries`, `inbox_work`, `settled`, `futile*`, `asked`; `Finished.rescues`/`rescued`; re-exports `Rescue`, `RescueAlways` at `lib.rs:49-50`.
- Preemption: `Control::PREEMPT`/`preempt`/`take_preempt` and `Stop::Preempt` in `crates/eml_rt/src/control.rs` (Verus-verified, gate in `crates/eml_rt/CLAUDE.md`); `Slot.switches`, `Ticker`, `Policy::PREEMPTS`, the monitor's preempt tick; tests in `crates/eml_vm/tests/preempt.rs`.
- In-place blocking externs (`Stdin`, `FileIn`, `Proc`, `Fs`) hold their worker until they return (docs/implementation/status.md:148). Without rescue, the tasks placed on that worker wait.
- The test suite at the base: `cargo nextest run --workspace` → 4696 passed, 23 skipped (measured in the stage's worktree at f693bf2d).
- A design prototype of this stage was run on a clone. It hoisted the spawn before `vm::run`, used the ring without worker 0, disabled the monitor's rescue tick, and ran `async/` UI files at `(2, Sequential)` and `(8, Sequential)`. Results:
  - `-p eml_vm --test integration`: 518/525. The seven failures are all count assertions, listed in S1-T1.
  - UI + REPL: 50/50 unchanged.
  - With the monitor's preemption also off: one more failure (`io::a_stream_closed_by_its_busy_host_reaches_its_peer_on_worker_1`, a task spinning without yielding), and `ui::`, `repl::` and `cli::` 103/103 unchanged.
  - Spawn cost (`eml run hello.em`, 16 workers, dev build): +0.4 ms per run and +0.7 MiB peak RSS.
- nextest stops a hung test after 60 s (`.config/nextest.toml`: `slow-timeout = { period = "2s", terminate-after = 30 }`), but an `eml` child that a CLI test spawned is left running. During design a prototype that hung left 43 such processes.

## 4. Prerequisites

| Needed | For | How to check | If missing |
|---|---|---|---|
| Nix devShell (direnv) with cargo-nextest, Verus, verusfmt | all; S1-T3 gate | `verus --version`, `verusfmt --version` → print versions | stop and ask |
| Clean tree at f693bf2d or later with no conflicting change in `crates/eml_vm` and `crates/eml_rt` | all | `git status --short` → empty | rebase the plan's file:line references first |
| Linux CI (Miri, ASan, TSan) | CI-only lines | the PR's CI | none locally; the lines are marked CI-only |

## 5. Decisions

### 5.1 Long-term guidelines (changing these requires re-examination)

#### D-1: Cooperative, thread-per-core scheduling (no task migration, no preemption)
- Decision:
  - A task runs on the worker it was placed on for its whole life, and switches only at `Async` operations. `Seeded`'s forced yields remain as a VM test instrument.
  - Cross-worker transfers are only these: placement at `start`; `par`/`compute` placement at fork (later, stealing of unstarted children in `par-steal`); channel items; pool jobs; results at join or `take`.
  - There is no rescue and no preemption, on the VM or natively.
- Rationale:
  - Native code (the center of practice) has no preemption and no rescue entry rule. One semantics for both backends.
  - Rescue saved only frameless work (never I/O waiters or started tasks), ran on wall-clock heuristics, and spread invariants over the registry core, the slots, the monitor and the native entry rule.
  - Removing it also makes "pinned tasks never move" (main-pinning) true by construction.
- Rejected alternatives:
  - Keep rescue and exclude pinned tasks from it one by one: it keeps every invariant, adds per-entry pinned bits, and still differs from native code.
  - Tokio-style stealing of started tasks: it makes every task's working set shared and loses reuse (multicore.md 「却下した案」).
- Scope of impact: eml_vm scheduler and monitor, eml_rt `Control`, docs/spec/vm.md, docs/future/multicore.md, native.md, async.md.
- Revisit when: a measured server workload leaves workers idle while another worker's ready queue grows for more than a tick. The remedy then is stealing *unstarted* `Async` tasks through the `par-steal` deques, not rescue of parked continuations (roadmap item, S1-T4).
- Conformance checks:
  - MUST NOT: any code path changes a task cell's owner after `start`.
  - MUST NOT: any `PREEMPT` bit, `Stop::Preempt`, `steal*`, `rescuable`, `RescueAlways`, `Notice::Rescue` exist (greps in S1-T2/S1-T3).
  - MUST: switches happen only in `Async` waits, join waits, child or job ends, `Async.run` roots and `Seeded`'s `tick`.
- Kept in the repository: docs/spec/vm.md 「タスクとスケジュール」 and 「`Async` のタスク」 (S1-T1..S1-T3), docs/future/multicore.md 「スケジューラ」 and 「却下した案」 (S1-T4).

#### D-2: Eager workers
- Decision: in a run that may use workers (N ≥ 2, the monitor runs, no session roots, no root primitives in the loaded code, no driver injected with `Machine::with_driver`), worker 0 spawns workers 1..N-1 at the start of the run, before the first instruction of `main`, and joins them at its end. Workers start parked and idle. No instruction is re-executed.
- Rationale: it deletes `Workers::Wanted`, `Exit`, `SPAWN_WORKERS` and instruction re-execution, and native code needs no "spawn on first fork" path. It costs +0.4 ms and +0.7 MiB per run at 16 workers (measured).
- Rejected alternatives: lazy spawn (today). It needs re-execution of the instruction, a VM-only path.
- Scope of impact: `begin_run`, `run_scheduled`, `spawn_workers`, the VM arms of `par`, `start_raw`, `start_in_raw`, `fork_job`.
- Revisit when: spawn cost shows in a measured short-run workload.
- Conformance checks: `git grep -n -e SPAWN_WORKERS -e 'Workers::Wanted' -- crates` → no output; `Finished.workers` = workers spawned at the start.
- Kept in the repository: vm.md 「タスクとスケジュール」 (S1-T1), multicore.md 「スレッドと worker」 (S1-T4).

#### D-3: Placement ring
- Decision: with N ≥ 2, `start_raw` and `start_in_raw` add one to the run's counter `next_place` and place on worker `1 + (value mod (live − 1))`. The workers rotate 1, 2, …, N−1, 1, …, and worker 0 receives no task by `start`. With N = 1 the caller is the owner. `Async.run` roots are placed on the caller. `par` children and `compute` jobs go to an idle worker (worker 0 included), else to the caller's queue.
- Rationale: worker 0 runs `main`, the code users write synchronous reads in. Without rescue, a `main` blocked in `Stdin` would otherwise starve the tasks placed on worker 0.
- Rejected alternatives: keep worker 0 in the rotation and document the starvation. The user chose the ring; at N = 2 it leaves one worker for tasks, and `--workers` sizes the pool.
- Scope of impact: `Run::next_owner`; tests that count placements.
- Revisit when: `par-steal` lands (it can let worker 0 take unstarted work).
- Conformance checks: unit test of `next_owner` (never 0 when live ≥ 2); `tasks_on_other_workers_run_while_main_blocks_in_an_extern`.
- Kept in the repository: vm.md 「`Async` のタスク」 (S1-T1), status.md (S1-T2), multicore.md (S1-T4).

### 5.2 Short-to-mid-term design

#### D-10: The monitor keeps three duties
- Signal relay (`Control::claim_signals` → mail → `NOTICE` on every word), waking (`Run::wake_all` when it relayed or a stop bit is set), and the memory soft limit (`pressure_tick` under `MEMORY = Watched`).
- `begin_run` starts the monitor when `kind != Virtual || workers >= 2` (the current condition).

#### D-11: Policies
- `Sched = { Normal, Sequential (default), Seeded(seed) }`.
- `Normal` and `Sequential` differ only in wake order (`push_back` vs `push_front`) and `MEMORY` (`Watched` vs `Gathered`). Both are cooperative, and `Normal::tick` is the constant false. `eml run` and `eml repl` keep `Normal`.
- `Policy` keeps `tick`, `reload`, `pick`, `wake`, `for_worker` and `MEMORY`, and loses `RESCUE`, `PREEMPTS`, `set_rescuable`, `workers_started` and `reserves`.
- `Finished.preemptions` counts the switches `Seeded`'s `tick` started; it is 0 under `Normal` and `Sequential`.

#### D-12: What a busy worker costs, and the remedies
- A CPU-bound task, or an in-place read inside a task, delays the other tasks of its worker until it waits or returns, on the VM as natively.
- On a worker running a task that does not wait, the completions of its driver are collected at that task's next switch. There is no time bound.
- Remedies: `compute`, `blocking`, `Std.Stream`, `Std.Exec`, `--workers`.
- A program whose in-place read can only complete through a task of the same worker hangs; the processor does not detect it. status.md states this limit (S1-T2).

## 6. Implementation constraints and conventions

- Each task is one commit that passes `cargo nextest run`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` on its own (CLAUDE.md).
- The docs/spec sections a task changes go into that task's commit (CLAUDE.md). The docs/future files and roadmap 「その後の項目」 are changed at the close (S1-T4). This plan's roadmap stage entry goes in the first commit, together with this plan.
- Code comments and docs are written in Japanese through PLaMo (CLAUDE.md). The English texts in this plan are the content to translate. Remove spaces between Japanese and Latin text in every paragraph you edit.
- Test hygiene for every local run:
  - outer `timeout 600` or less, and targeted `-E` filters while iterating;
  - after a run that hung or timed out, kill the leftover processes: `ps -axo pid,command | grep 'target/debug/eml ' | grep -v grep | awk '{print $1}' | xargs kill -9`. zsh does not word-split `$VAR`; use `xargs`.
- The test changes of each task are exactly the ones listed (kind 1 one by one, kind 2 as a scope, kind 3 mechanical). Any other diff in a test, snapshot or `RunStats` stops the stage for a plan amendment (docs/implementation/testing.md 「テストの変更の運用」).
- The plan's IDs (D-x, S1-Tx) never appear in code or docs.

## 7. Implementation tasks

| ID | Task | Based on | Depends on | Parallelizable |
|---|---|---|---|---|
| S1-T0 | Commit this plan and the roadmap stage entry | — | — | no |
| S1-T1 | Eager workers and the placement ring | D-2, D-3 | S1-T0 | no |
| S1-T2 | Remove rescue | D-1, D-10, D-12 | S1-T1 | no |
| S1-T3 | Remove VM preemption (eml_rt gate) | D-1, D-11 | S1-T2 | no |
| S1-T4 | Close: docs/future, status, roadmap, benchmarks | D-1..D-3 | S1-T3 | no |

### S1-T0: the plan and the roadmap entry
- Purpose: the stage's first commit (docs/future/roadmap.md 「進め方」).
- Work:
  - Add this plan as `docs/plans/2026-11-06-sched-cooperative.md`.
  - Add to docs/future/roadmap.md 「段の列」, before `main-pinning`: "`sched-cooperative`: workers 1..N-1 start at the beginning of a run; `start` places only on workers 1..N-1; rescue and VM preemption are removed, so a task runs on its worker until it ends and switches only at `Async` operations. Prereq: none. Design: [マルチコア対応の設計](multicore.md) の「スケジューラ」".
  - Change the `main-pinning` entry's 「前提」 to `sched-cooperative`, and drop its 救済 clause (the tasks of the main worker "do not move to other workers").
- Completion criteria: `cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` → passes.

### S1-T1: eager workers and the placement ring
- Targets: `crates/eml_vm/src/{lib.rs, vm.rs, workers.rs, sched.rs}`, `crates/eml_vm/tests/{workers.rs, chan.rs}`, `docs/spec/vm.md`, `CLAUDE.md`.
- Work:
  - In the entry worker's run (today's `run_scheduled` body), after `Scheduler::new` and `begin_run(roots_free)`: if workers are wanted (the current condition), call `spawn_workers` at once, then `set_live`, set `Workers::Started(n)` or `Denied`, and arm `Shutdown`. Then run `vm::run` once.
  - Delete `Workers::Wanted`, `Exit`, `SPAWN_WORKERS`, its four checks (`vm.rs:1542`, `:2533`, `:2643`, `:2719`), the `task_failed` case (`:2340`) and the re-execution loop (`lib.rs:1003-1032`). The prototype showed that hoisting today's `Exit::Spawn` arm before the loop is enough.
  - `Run::next_owner` = `if live <= 1 { 0 } else { 1 + (next_place.fetch_add(1) % (live - 1)) }`.
- Test changes:
  - Kind 2. Scope: "runs with N ≥ 2 report the workers spawned at the start, and `start` from worker 0 always counts as a placement". The values are from the prototype:
    - tests/workers.rs:120 `a_run_without_par_starts_no_worker` → rename to `a_run_with_workers_starts_them_at_the_start` (N = 4: `workers == 3`).
    - :123-131 `the_first_fork_starts_the_workers` → rename to `the_workers_exist_before_the_first_fork` (values unchanged; name and doc only).
    - :505-510 `exit_abandons_a_job_on_another_worker`. `placed_jobs` becomes schedule-dependent, because the task now runs on worker 1 and worker 0 may not be idle yet. Make `main` sleep before forking so that worker 0 is idle, and keep `placed_jobs == 1` and the exit code.
    - :600-605 `a_timer_on_worker_0_and_a_child_on_worker_1_agree` → `a_timer_task_and_a_child_on_other_workers_agree` (N = 3, `placed_tasks == 2`; the timer task lands on worker 2, so the name names no worker).
    - chan.rs:417 `producers_on_both_workers_feed_the_root_task`: `(1, 1)` → `(1, 2)`.
    - chan.rs:442 `parked_receivers_on_both_workers_are_woken_by_the_root`: 1 → 2.
    - chan.rs:491 `lin_items_cross_workers_and_come_back`: 1 → 2.
    - chan.rs:520 `receivers_cancelled_on_both_workers_leave_the_queue`: 1 → 2.
  - New unit test in workers.rs: `the_ring_never_places_on_worker_0` (live 3: 1, 2, 1, 2; live 1: 0).
  - Kind 2 (decided after review): tests whose names claim two workers but which the ring now puts on worker 1 alone run at N = 3, so that they keep checking two workers: ffi_abandon.rs `two_hung_workers_share_one_budget`, chan.rs `parked_receivers_on_both_workers_are_woken_by_the_root` and `receivers_cancelled_on_both_workers_leave_the_queue` (and the `run_at` doc), with their counts and docs updated.
  - Kind 1 (decided during execution): delete the unit test `placement_rotates_over_every_worker` (src/workers.rs); it asserts the old rotation through worker 0, and `the_ring_never_places_on_worker_0` replaces it.
  - Moved here from S1-T2 (decided during execution, kind 2): crates/eml_cli/tests/ui.rs `WORKER_RUNS` (:298-304) and the second table in `workers_runs_cover_par_and_async` (:755-763) → `async/` at `[(2, Sequential), (8, Sequential)]`, with the `RescueAlways`-only assertions they guard; testing.md :458-460 accordingly. With eager workers, `RescueAlways` can move an `Async.run` root to worker 1 before it starts, so the `placed_tasks > 0` check of those runs no longer holds.
  - Unchanged (checked): workers.rs:148-160, :171, :208, :274, :345, :626, :643, :647, :667, :686; chan.rs:563, :598; signal.rs:387; io.rs:1479, :1531.
- Spec and docs (same commit):
  - vm.md 「タスクとスケジュール」 :423-424, replace with: "Every `Scheduler` holds the state `Workers` of workers 1..N-1: `Started(n)`, `Denied` or `Peer`. Worker 0 decides it at the start of the run (`begin_run`); workers 1..N-1 are always `Peer`. Only `Started` and `Peer` schedulers place right children, `Async` tasks and `compute` jobs on other workers. Workers are wanted when `Limits::workers ≥ 2`, the monitor runs, the session has no roots, the loaded code has no root primitives and no driver was injected with `Machine::with_driver`. Worker 0 then spawns workers 1..N-1 at once (`eml_io::spawn_scoped_masked`, named `eml-worker-k`, 8 MiB), before the first instruction of `main`, and joins them at the end of the run. Workers start parked and idle. No instruction is re-executed: a `par`, `start` or `fork_job` never waits for workers."
  - vm.md 「`compute` の仕事」 :642: delete step 1 and renumber.
  - vm.md 「`par` の fork と join」: delete the `SPAWN_WORKERS` step.
  - vm.md 「`Async` のタスク」 :673, replace the first sentences with: "`Std.Async` tasks are placed on a worker at `start` and run there until they end. In a run with workers 1..N-1 (`Workers` is `Started` or `Peer`), `start_raw` and `start_in_raw` add one to the run's counter `next_place` and place on worker `1 + (value mod (live − 1))`: the workers rotate 1, 2, …, N−1, 1, …. Worker 0, which runs `main`, receives no task by `start`, so a synchronous read in `main` never stalls them. In a run without workers the caller is the owner. `Async.run` roots are placed on the caller."
  - CLAUDE.md:97 (eml_vm row) and :122: "workers 1..N-1 start at run start", "`Async` tasks are placed round-robin over workers 1..N-1 at `start` and never move".
- Left to the implementer: the shape of the hoisted spawn code, keeping today's error mapping of a failed spawn.
- Completion criteria:
  - `cargo nextest run` → 4696 passed (4696 + 1 new − 1 deleted), 0 failed.
  - `git grep -n -e SPAWN_WORKERS -e 'Workers::Wanted' -e 'Exit::Spawn' -- crates` → no output.
  - `git diff --stat HEAD~ -- 'crates/*/tests' '*.snap'` → only `crates/eml_vm/tests/workers.rs`, `crates/eml_vm/tests/chan.rs` and `crates/eml_cli/tests/ui.rs`.
  - `cargo clippy --all-targets -- -D warnings` → exit 0; `cargo fmt --check` → exit 0.
- Pitfalls: a test that relied on worker 0 receiving the first task now sees worker 1. Only the listed tests change; any other failure is a design defect to report.

### S1-T2: remove rescue
- Targets: `crates/eml_vm/src/{sched.rs, workers.rs, monitor.rs, cells.rs, vm.rs, lib.rs}`, `crates/eml_vm/tests/{workers.rs, io.rs, memory.rs, ffi.rs, ffi_abandon.rs}`, `crates/eml_cli/tests/ui.rs`, `docs/spec/{vm.md, stdlib.md, object-model.md}`, `docs/implementation/{testing.md, status.md, architecture.md}`, `CLAUDE.md`.
- Work: delete rescue entirely:
  - `steal`, `steal_start`, `steal_resume`, `steal_wake`, reservation, futile marks;
  - `Entry.rescued`, `Ready::counted`;
  - `Slot.{ready_entries, inbox_work, settled, futile*, asked}`;
  - `Notice::Rescue`;
  - `TaskCell.entries`, `Registry::leave`, `Scheduler.left`;
  - `RescueAlways`, `ask_rescue`, `rescuable`, `set_rescuable`;
  - `Policy::{RESCUE, workers_started, reserves}`;
  - the monitor's `rescue_tick`;
  - `Finished.{rescues, rescued}`;
  - the re-exports `Rescue` and `RescueAlways` at `lib.rs:49-50`.

  The monitor keeps relay, waking and memory pressure (D-10).
  - Decided during execution: `Registry::leave` also adds each entry's `added` bytes to `task.bytes` (the task-limit count). Keep that part as a byte-only batch: `Scheduler.left` → `Scheduler.added`, `Registry::leave` → `Registry::add_bytes`, flushed in the same registry section of `gather`, so lock counts and `RunStats` stay the same. Also delete `Ticker::stuck`, `Ticker.still`, `STUCK_TICKS` and `OVERLOAD`, which only rescue used.
  - Decided during execution: the paths that follow an owner change (`placed_owner` re-routing in `take_async`, `MailItem::Cancel { owner }` forwarding, `limit_forwarded`/`route_limit`, `take_op` not checking the record owner) stay in this unit; only their comments that cite 救済 change. Their removal is R-4.
- Test changes:
  - Kind 1 (deleted), one by one:
    - tests/workers.rs: `rescue_always_rescues_every_started_task`, `a_rescued_task_holding_a_lin_resource_passes_the_checks`, `sequential_never_rescues`, `rescue_always_at_one_worker_is_sequential`, `rescued_tasks_sleep_and_take_from_a_channel_on_their_new_worker`, `a_cancel_reaches_a_rescued_waiting_task`.
    - tests/io.rs: `a_rescued_task_does_io_through_its_host`.
    - tests/memory.rs: `a_task_moved_by_rescue_is_flagged`.
    - src/sched.rs unit tests: `steal_takes_a_place_and_the_new_owner_starts_it`, `steal_takes_a_wake_only_when_no_entry_of_its_task_is_live`, `steal_takes_start_and_resume_entries_in_queue_order_within_the_budget`, `steal_takes_a_yielded_resume`, `steal_leaves_resume_io_rescued_entries_tokens_and_par_tasks`, `steal_decrements_inbox_work_and_ready_entries`, `steal_reports_truncation_past_the_scan_cap`, `a_rescued_task_runs_under_its_own_id`, `a_flush_adds_settled_cells_to_the_slot`, `rescue_budget_takes_half_rounded_up_and_at_least_one`, `a_rescue_that_finds_nothing_marks_the_victim_futile`, `a_truncated_rescue_never_marks_the_victim_futile`, `a_rescue_request_after_the_end_is_dropped`, `rescue_always_asks_its_successor_once`, `rescue_always_pick_skips_reserved_entries`, `rescue_always_reserves_nothing_on_an_unrescuable_worker`, `a_rescue_always_request_takes_every_reserved_entry`, `a_rescue_always_victim_asks_again_after_each_rescue`, and the helpers `rescue_always_pair` and `rescue_always_worker`.
    - src/workers.rs unit: `ready_entries_follows_push_pick_and_retain`.
    - src/monitor.rs unit: `a_stuck_worker_without_a_hint_is_left_alone`, `an_overloaded_worker_is_rescued_while_it_switches`, `a_rescue_goes_to_an_idle_worker_other_than_the_victim`, `a_futile_victim_is_skipped_until_its_counts_change`, `a_futile_victim_is_retried_after_its_entries_settle`, `a_blocked_worker_is_rescued_within_three_ticks`.
    - src/cells.rs unit: `two_live_entries_keep_a_parked_task_unstealable`, and the helper `entries_of`.
    - Added during execution: src/sched.rs `a_stale_cancel_in_the_victim_mail_reaches_the_new_owner`; src/cells.rs `steal_helpers_refuse_a_cell_owned_by_a_third_worker`, `an_entry_count_follows_start_park_resume_and_finish`; src/workers.rs `inbox_work_counts_places_and_plain_wakes`; src/monitor.rs `without_an_idle_worker_nothing_is_sent`, `a_worker_is_stuck_after_two_ticks_without_a_switch`. Each tests a deleted mechanism.
  - Kind 2 (added during execution): src/cells.rs `leave_skips_a_released_cell` is rewritten against `add_bytes`, without the entry-count assertions; `leave_adds_the_bytes_and_resumed_returns_them` → `add_bytes_adds_the_bytes_and_resumed_returns_them`, without the return-value assertions.
  - Kind 3 (added during execution): src/vm.rs `a_self_yield_offers_top_and_a_real_wait_clears_it` reads `sched.added` in place of `sched.left`; the doc of `a_forwarded_limit_follows_a_new_owner` stops citing rescue.
  - Kind 2. Scope: "tests that used `RescueAlways` to put a task on another worker place it there at `start` under `Sequential`; the ignored monitor-rescue test becomes a placement test".
    - tests/workers.rs `a_nested_run_closed_before_its_caller_waits_goes_on`: N = 2, `Sequential`, no `rescued` assertion.
    - tests/workers.rs `the_monitor_rescues_a_worker_blocked_in_an_extern` (`#[ignore]`) → `tasks_on_other_workers_run_while_main_blocks_in_an_extern`, not ignored. N = 2 with `BlockingHost`: `main`'s `println "a"` blocks until a task prints `"b"`; the task is on worker 1 by D-3; output `b\na\n`; deterministic and under 1 s.
    - tests/io.rs `a_wait_from_a_task_placed_on_worker_2_ends_under_rescue_always` → `a_wait_from_a_task_placed_on_worker_2_ends` (N = 4, `Sequential`).
    - crates/eml_cli/tests/ui.rs:
      - (Done in S1-T1: `WORKER_RUNS` and the second table in `workers_runs_cover_par_and_async`.) Delete what remains of the `RescueAlways` assertions, e.g. the `rescued > 0` assertion (:374-375), if S1-T1 left any.
  - Kind 3:
    - ui.rs:527-528 and :574-575: the `Finished` literals lose `rescues`/`rescued`.
    - tests/ffi.rs:768 and tests/ffi_abandon.rs:109: delete `assert_eq!(finished.rescues, 0, ..)`.
- Spec and docs (same commit):
  - vm.md:
    - Delete 「救済」 (:988-997).
    - 「タスクとスケジュール」:
      - :409: "Policies are `Normal`, `Sequential` (default) and `Seeded(seed)`, chosen with `RunConfig::with_sched`. `eml run` and `eml repl` always use `Normal`; `Sequential` and `Seeded` are for deterministic tests. The library default is `Sequential` so that `RunStats` and the bench numbers do not depend on time."
      - :411-413: delete the `RESCUE`, `workers_started` and `RescueAlways` paragraphs.
      - :417: delete the slot fields `ready_entries`, `inbox_work`, `settled`, `futile*`, `asked`.
      - :420-422: delete the `Rescue { victim }` bullet; in the `Limit(cell)` bullet, drop "救済で受け箱から外さない" and "`Rescue` と同じく".
      - :430: "The monitor has three duties: relaying signals (`Control::claim_signals` → mail → `NOTICE` on every word), waking (`Run::wake_all` when it relayed or when a stop bit is set), and the memory soft limit (`pressure_tick`) under `MEMORY = Watched`." Delete the rescue duty; the preemption duty goes in S1-T3.
      - :436: the queue items are `Ready::Task` (VM tasks with frames: `par` right children, tasks switched out by `Seeded`, join waiters, `Async.run` waiters) and `Ready::Entry` (start or resume work with a cell, a logical ID, a kind and one word). Delete `rescued`, `ready_entries` and 盗む.
      - :440: delete the clauses about steal clearing `top`.
    - 「`Async` のタスク」:
      - :686: the owner is "written by `start` only"; delete `entries`.
      - :742: "tasks waiting on an operation never change worker, so…".
      - :776: delete the sentence about a root task moved by rescue.
      - :944: delete "(「救済」)" and the rescue cause of the two-`Task` note (keep the fact).
      - :968: delete the rescue clause.
      - :982: "owner-only fields are read and written only by the owner".
    - 「操作の記録」 :736: delete "(「救済」)".
    - 「C の関数の呼び出し」 :1077: "`submit` and `io_wait` stay adjacent so that a task holding an unfinished record is always in that wait".
    - 「`RunStats`」 :1108 and :1153: `Finished` without `rescues`/`rescued`; `workers` = workers spawned at the start.
    - 「並列化への備え」 :1172: five paths, without rescue; :1176: drop "救済も付けない".
  - stdlib.md:985: "Tasks are placed on a worker at `start` and run there until they end ([VM](vm.md) の「`Async` のタスク」)."
  - object-model.md:775: delete the bullet about rescue keeping the ID.
  - status.md:
    - :148: "…block their worker until they return; the tasks placed on that worker wait. `start` places no task on worker 0 when N ≥ 2, so a synchronous read in `main` stalls no task; inside a task, use `Std.Stream`, `Std.Exec`, `compute` or `blocking`. A program whose in-place read can only complete through a task of the same worker hangs; the processor does not detect it."
    - :166: "A task runs on the worker it was placed on until it ends, and switches only at `Async` operations. Nothing moves work between workers after placement: a CPU-bound task or an in-place read delays the other tasks of its worker until it waits or returns. `compute` moves CPU work to an idle worker at fork; if no worker is idle, the job runs on the caller's worker."
  - testing.md:
    - :56: "a task placed on worker 2 waits for a child under `Sequential` with four workers".
    - :282: drop the `RescueAlways` sentences; add "that tasks placed on other workers run while `main` blocks in an extern".
    - :426: "`async/`: … the sequential policy at N = 2 and 8 besides the two seeds".
    - :458-460: `WORKER_RUNS` "`async/`: N = 2 and 8 `Sched::Sequential`"; delete the `RescueAlways` paragraph and the `rescued > 0` sentence.
    - :614: delete the monitor-rescue timing example.
    - :749: delete the `the_monitor_rescues_a_worker_blocked_in_an_extern` command.
  - architecture.md:322-323: drop rescue from the eml_vm description ("`Async` tasks are placed on workers 1..N-1 in rotation and never move").
  - CLAUDE.md:139: "`par/` and `async/` run at 2 and 8 workers under `Sequential`".
- Completion criteria:
  - `cargo nextest run` → 4653 passed, 0 failed (amended: the 6 added kind-1 deletions). Originally: 4659 = 4696 − 38 deleted tests (8 integration, 18 sched unit, 1 workers unit, 6 monitor unit, 1 cells unit) + 1 for the formerly ignored test that now runs. If a deleted test was itself ignored, the passed count is one higher per such test; record the exact passed and skipped numbers in the commit message.
  - `git grep -n -e RescueAlways -e steal_start -e steal_resume -e steal_wake -e ready_entries -e inbox_work -e futile -e 'Notice::Rescue' -e rescuable -- crates` → no output.
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::) | test(repl::)'` → passes with no `.em` file or snapshot changed.
  - `git diff --stat HEAD~ -- crates/eml_rt` → empty.
  - clippy and fmt → exit 0.
- Pitfalls: the kind-1 list is complete as far as the design read the code. If compilation reveals another user of a deleted item, treat it as kind 3 when it only removes a deleted field or assertion. Otherwise stop and amend the plan.

### S1-T3: remove VM preemption
- Targets:
  - `crates/eml_rt/src/control.rs` (eml_rt gate);
  - `crates/eml_vm/src/{vm.rs, sched.rs, workers.rs, monitor.rs}`;
  - `crates/eml_vm/tests/{main.rs, preempt.rs, par.rs, io.rs}`;
  - `.github/workflows/ci.yml`;
  - `docs/spec/{vm.md, object-model.md}`;
  - `docs/implementation/{status.md, testing.md}`;
  - `CLAUDE.md`.
- Work:
  - Delete `Control::{PREEMPT, preempt, take_preempt}` and `Stop::Preempt` from eml_rt. `begin_run` lowers `NOTICE | END`, and `stop_reason` loses its branch.
  - Delete `Slot.switches`, the switch counting in `Scheduler::set_current`, `Ticker`, `bell_timeout`'s `ticks` argument (keep it for pressure only), `Monitor::start`'s `preempt` flag, `Policy::PREEMPTS`, the monitor starts in `add_task` that exist for preemption, and the safe point's `PREEMPT` branch. `Ctx::poll`'s cold path keeps `NOTICE` and the limits.
  - Keep `Policy::tick` and `reload` (`Seeded`); `Normal::tick` stays the constant false.
  - Decided during execution: the monitor starts that were gated on `P::PREEMPTS` also started the memory soft-limit watch (`Normal` is the only `Watched` policy). Keep one start in vm.rs `schedule`, before the `Ready::Entry` push, gated on `P::MEMORY == Memory::Watched`; every `Async` task enters there, so a run with no task starts no monitor thread. Also delete the `take_preempt` call in `Run::prepare` and its `control` parameter, `Polled::Switch`, and `limit_safepoint`'s `switch` argument; `bell_timeout`'s parameter becomes `pressure`.
- Test changes:
  - Kind 1 (deleted), one by one:
    - tests/preempt.rs (whole file deleted; remove `mod preempt;` from tests/main.rs): `a_preempt_without_a_ready_task_only_lowers_the_bit`, `a_stop_raised_with_preempt_wins`, `a_preempt_request_switches_at_the_next_safepoint`.
    - tests/par.rs:952 `the_monitor_preempts_a_long_par_child` (`#[ignore]`d today, so it lowers the skipped count, not the passed count).
    - src/monitor.rs unit tests about `PREEMPT`, listed by `git grep -n -B2 PREEMPT crates/eml_vm/src/monitor.rs | grep 'fn '`. Record their names in the commit message.
  - Kind 2. Scope: "a test whose busy task relied on the monitor's tick to reach a collect point makes the task yield". Only `tests/io.rs` `a_stream_closed_by_its_busy_host_reaches_its_peer_on_worker_1`: `spin` calls `Async.yield_now` each round; the doc comment says the close happens at the busy task's next switch.
  - Revised during execution (kind 2): since the ring, `busy` in `a_stream_closed_by_its_busy_host_reaches_its_peer_on_worker_1` runs on worker 1 and the stream's host (the root on worker 0) is not busy. Restructure the test so the root, which hosts the stream, closes it and then spins calling `Async.yield_now`, and `server` on worker 1 sets the flag; the doc says the close reaches the peer at the host's next switch.
  - Added during execution:
    - Kind 1: src/monitor.rs `a_tick_without_a_switch_asks_for_a_switch`, `a_tick_after_a_switch_does_not`, `the_monitor_sleeps_while_every_worker_is_parked`; eml_rt src/heap/tests/control.rs `preempt_reaches_only_the_worker_word`.
    - Kind 2: eml_rt control.rs tests `stop_reason_puts_preempt_after_the_stops` → `stop_reason_without_a_bit_is_the_depth_limit`, `preempt_and_notice_reach_only_their_word` → `a_notice_reaches_only_its_word`, `begin_run_reopens_the_end_and_lowers_preempt` → `begin_run_reopens_the_end`, `stop_reason_puts_notice_between_the_stops_and_preempt` → `stop_reason_puts_notice_after_the_stops`; trusted.rs `a_preempt_from_another_thread_is_seen` → `a_notice_from_another_thread_is_seen` (run Miri `trusted::`); vm.rs `a_notice_collects_and_a_notice_with_a_preempt_switches` → `a_notice_collects_without_switching`.
    - Kind 3: removed `preempt` calls, `PREEMPT` bits, the `limit_safepoint` argument, the `prepare` argument and the `Monitor::start` argument in tests.
  - Unchanged: `Seeded`-based `preemptions > 0` assertions (par.rs:883-886, :962-965, miri.rs:515, ui.rs:469) and the `== 0` ones for `Sequential` (api.rs:141, par.rs:879).
- Spec and docs (same commit):
  - vm.md 「タスクとスケジュール」:
    - :410: "`Policy` has the methods `tick`, `reload`, `pick`, `wake`, `for_worker` and the constant `MEMORY`. `tick` is called at every safe point and returns true to switch tasks (`Seeded` only); `reload` refills the budget after a switch; `pick` takes the next item from the ready queue; `wake` inserts a woken task; `for_worker(k)` gives worker `k`'s policy (`Seeded::new(splitmix64(seed ^ k))`; the others copy themselves)."
    - :416: "`Normal` never switches on its own (`tick` is the constant false); `pick` takes the head and `wake` appends. A task runs until it waits at an `Async` operation or ends."
    - :417: delete `switches`.
    - :430: delete the preemption duty.
    - :432: delete the tick-based preemption.
    - :433-434: delete the clause "ほかの者が `Control::preempt` …".
    - :437: switches happen at `Seeded`'s forced yields, join waits, child end, job end, `Async.run` roots and `Async` waits; delete "安全点での横取り".
    - :442-447, replace with: "At a safe point, after `Ctx::poll`, the VM calls `tick`; if it returns true and a runnable task exists, the current task is parked at the back of the queue (`Ready::Task`) and the next item runs. `NOTICE` is handled at the collect point. The fast path is `poll` returning `Ok(())`; the rest is `#[cold]`. `Finished.preemptions` counts the switches `tick` started (`Seeded`); it is 0 under `Normal` and `Sequential`."
  - vm.md 「時計と driver」:
    - :548: replace the 20 ms bound with "On a worker running a task that does not wait, the completions of its driver are collected at that task's next switch (an `Async` operation); there is no time bound. Keep CPU-bound work in `compute` or off I/O-hosting workers."
    - :556: "…or until the next switch (an `Async` wait or a `Seeded` yield)".
  - vm.md 「`RunStats`」: `preemptions` = switches started by `Seeded`'s `tick`.
  - object-model.md 「実行の制御」: delete `Control::PREEMPT (1 << 31)` from the bit list.
  - status.md:37: "`eml run` and `eml repl` use `Normal`, which never switches tasks by itself; `Sequential` and `Seeded` are library-only test policies."
  - status.md ~:166 (added after the S1-T2 review): replace 「CPUを使うタスクは同じworkerのほかのタスクと時間を分け合い」 with "a task switches only at `Async` operations; a CPU-bound task or an in-place read delays the other tasks of its worker until it waits or returns".
  - Line numbers in this task predate S1-T2, which removed vm.md lines; locate text by quotation.
  - testing.md 「ThreadSanitizer」, CLAUDE.md (the TSan command) and ci.yml: drop `preempt::` from the TSan lists.
- Completion criteria:
  - `cargo verus verify -p eml_rt` → both summary lines `0 errors`.
  - `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` → exit 0.
  - The rest of the eml_rt gate in `crates/eml_rt/CLAUDE.md`, as that file lists it.
  - `cargo nextest run` → (the S1-T2 count) − 3 − (number of deleted monitor unit tests) passed, skipped one lower, 0 failed; record the exact numbers in the commit message.
  - `git grep -n -e PREEMPT -e PREEMPTS -e take_preempt -e 'Stop::Preempt' -- crates` → no output.
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::) | test(repl::) | test(cli::)'` → passes, with no `.em` file or snapshot changed.
  - `nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- .github/workflows/ci.yml` → exit 0.
  - clippy and fmt → exit 0.
  - CI-only: the TSan job passes.

### S1-T4: close the stage
- Purpose: the `Close sched-cooperative` commit (docs/future/roadmap.md 「進め方」), in that procedure's order.
- Work:
  - docs/future/multicore.md:
    - :5: drop the rescue sentence of the introduction.
    - 確定した設計判断: row 6 "parallelism spreads in this order: worker threads and `par` placement, `Async` across workers, `par` stealing"; row 10 "a parked task's continuation never leaves its worker and is not marked"; row 14 "an `Async` task runs on the worker it was placed on until it ends"; row 15 "cooperative: a task switches only at `Async` operations, on the VM as natively; `Seeded` is a test policy that forces yields at safe points"; row 16 "an in-place extern or C call holds its worker; long calls go to the pool (`blocking`) or to `compute`".
    - 「スレッドと worker」 bullet 3: "Workers 1..N-1 are spawned by worker 0 at the start of a run that may use them (N ≥ 2, no session roots, no root primitives, no injected driver) and joined at its end; runs that cannot use them run on worker 0 alone. Heaps live for the run." Delete the monitor's preempt and rescue bullets.
    - 「worker のループ」: delete the rescue step.
    - 「`Async` のタスク」:
      - bullet 1: "placed on workers 1..N-1 in rotation; worker 0 receives none when N ≥ 2; a started task runs on its worker until it ends";
      - bullet 2: drop the stealable-form sentences;
      - bullet 3: the notices list without `Rescue`;
      - bullet 5: "the owner is written once, at `start`".
    - 「公平」: "FIFO per worker; a task runs until it waits at an `Async` operation; CPU-bound work goes to `compute`; the driver is looked at once per loop iteration and before waiting; new tasks rotate over workers 1..N-1; `par` children are bounded by the grain."
    - 「横取り」 → replace with "### 切り替え: Tasks switch only at `Async` operations (and at `Seeded`'s forced yields in tests). There is no preemption, on the VM or natively; `Ctx::poll` reads the stop word for stop bits and `NOTICE` only."
    - 「救済」 → replace with "### タスクは worker から動かない: A task runs on the worker it was placed on until it ends. The only cross-worker transfers are placement at `start`, `par`/`compute` placement at fork (and stealing of unstarted children in `par-steal`), channel items, pool jobs and results at join or `take`. The remedies for a busy worker are `compute`, `blocking`, `Std.Stream`, `Std.Exec` and `--workers`; `start` never places on worker 0 when N ≥ 2."
    - 「不変条件」: drop the rescue clause; "a parked continuation is touched by its worker only".
    - 「VM の並列化の備え」: drop "救済が動かすのは…".
    - 「1つの worker での実行」: three policies; `Normal` "switches only at `Async` operations"; delete `RescueAlways`.
    - 「テスト」: the `Async` bullet "N = 2 and 8 under `Sequential`"; delete the 救済 sub-bullet; drop `preempt::` from the TSan list; add "a Verus proof of cooperative placement is a roadmap item".
    - 「未決」: "監視と救済の値" → "監視の値"; drop the rescue reference of the EINTR item.
    - 「スレッドをまたぐ RC」: :200, :222, :227, :276, :299, :305, :314, :321 delete the rescue clauses and rows; :420 drop 救済 from the primitive list.
    - 「却下した案」, add two entries:
      - "Rescue (moving frameless work from a stuck or overloaded worker to an idle one): removed. It saved only unstarted tasks and wake-resumable continuations, never I/O waiters or started tasks; its triggers were wall-clock heuristics; its invariants touched the registry core, the slots, the monitor and the native entry rule."
      - "VM preemption by the monitor's tick: removed; native code has none, and the reference implementation should not differ."
  - docs/future/native.md:
    - 「安全点とスタック」: drop "`PREEMPT` は、ネイティブの確かめでは定数の AND で隠す…"; VM and native read the same bits.
    - 「並列の実行」 :126: "A native task switches only at `Async` operations; a task without them holds its worker until it ends, as on the VM. The other tasks of that worker wait; CPU-bound work goes to `compute`."
  - docs/future/async.md:
    - :11: drop 救済 from both lists.
    - :119: "…a native task that neither waits nor uses `compute` holds its worker until it ends; the other tasks of that worker wait."
    - :123: drop the rescue sentence.
    - :248: drop "救済でタスクが別の worker に動いた後が".
    - :249-250: delete.
    - :255: "…blocks its worker; the other tasks of that worker wait until it returns; `start` places no task on worker 0 when N ≥ 2".
    - :276: delete the row.
    - :369: "N = 2 and 8 under `Sequential`".
  - docs/future/ffi.md:158: drop "救済せず".
  - docs/future/roadmap.md:
    - Delete this stage's entry from 「段の列」.
    - 「マルチコア対応」: delete the three rescue items (:341-348) and add these deferred items, each with 「後に回した理由」 and 「見直す条件」:
      - work migration (stealing *unstarted* `Async` tasks through the `par-steal` deques): revisit when a measured server workload leaves workers idle while another worker's ready queue grows for more than a tick;
      - pool-backed `Stdin` and `FileIn` reads (`Async` rows like `Std.Stream`'s): revisit when a script needs stdin and tasks on one worker;
      - a Verus proof of cooperative placement (an owner set once at `start`, notices routed to the owner, no lost wakeups in `park_idle`/`wake`): revisit with multicore.md 「テスト」.
    - Under 「処理系」, add the deferred item "CLI tests kill their `eml` child on timeout (a process group on unix or a job object on Windows, or kill-on-drop), so that nextest's slow-timeout does not orphan a hung `eml`": revisit when an orphaned `eml` shows in CI.
  - docs/overview.md:93: "…`Async` tasks are placed on workers 1..N-1 in rotation at `start` and run there until they end".
  - docs/implementation/benchmarks.md: delete 「救済の判定の値」 (:919-926); :708 drop the clause naming it; :913 "one queue per worker under the slot's mutex (the shape `par-steal` revisits)"; also (added after the S1-T3 review) the 「横取り2回分」 explanation (~:522) and the `busy-self` note (~:672; at N = 2 the compute task now runs on worker 1) must describe the new numbers without preemption; refresh the three tables in 「今の数値」 from `bench/run.sh`, `bench/rt.sh` and `bench/scripts.sh` on a clean tree (U-2: refresh at this close).
  - docs/README.md: change the map entries only where the content changed.
  - Delete `docs/plans/2026-11-06-sched-cooperative.md`.
- Completion criteria:
  - `git grep -n 救済 -- docs CLAUDE.md ':!docs/plans' ':!docs/reports'` → exactly the roadmap 「マルチコア対応」 work-migration item and the multicore.md 「却下した案」 entry.
  - `git grep -n -e RescueAlways -e rescuable -e ready_entries -- docs CLAUDE.md ':!docs/plans' ':!docs/reports'` → only multicore.md 「却下した案」.
  - `git grep -n 横取り -- docs ':!docs/plans' ':!docs/reports'` → only sentences about `Seeded`'s forced switches and 「却下した案」.
  - `git grep -n -e Wanted -e SPAWN_WORKERS -- docs/spec/vm.md docs/future/multicore.md docs/implementation CLAUDE.md` → no output.
  - `cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` → passes.
  - `git status --short docs/plans` → the plan file deleted.

## 8. Execution order

S1-T0 → S1-T1 → S1-T2 → S1-T3 → S1-T4, strictly in sequence; each commit is green on its own. Then `main-pinning` starts from its own plan.

## 9. Verification plan

- Baseline (before S1-T1): `timeout 600 cargo nextest run` → 4696 passed, 23 skipped, 0 failed.
- Overall (after S1-T3):
  - `cargo nextest run` → the S1-T3 count passed, 0 failed, and no `TIMEOUT` line.
  - `ps -axo command | grep 'target/debug/eml ' | grep -v grep` → nothing.
  - `cargo clippy --all-targets -- -D warnings` and the feature-combo clippy lines of CLAUDE.md → exit 0.
  - `nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run -- -D warnings` → exit 0.
  - `cargo verus verify -p eml_rt` → 0 errors.
  - CI: all jobs green (Miri, ASan and TSan included).
- Manual checks by the user: none.

## 10. Risks and unresolved items

| ID | Item | Impact | How / when | Default |
|---|---|---|---|---|
| R-1 | The design ran a prototype that disabled rescue and preemption rather than deleting them. Deleting the code may reveal compile-level users not listed here. | S1-T2, S1-T3 | the implementer, at compile time | removing a field or assertion of a deleted item is kind 3; anything else stops for an amendment |
| R-4 | After S1-T2 a task's owner never changes, so the owner-forwarding paths (`placed_owner` re-routing, `MailItem::Cancel { owner }` (no direct test since S1-T2), `limit_forwarded`/`route_limit`, `take_op`'s missing owner check) and `Scheduler.current_woken` with its branches (vm.rs, sched.rs; the kind-2 test `a_nested_run_closed_before_its_caller_waits_goes_on` no longer reaches it) are unreachable code; vm.md still describes them (「持ち主が変わっても起こしを取り残さない」, 「持ち主がまた変わっていればそこへ回し」, the `current_woken` sentence in 「`Async` のタスク」) | after S1-T3 | the orchestrator, at the final review | default: list it as a follow-up for the user; do not remove it inside S1-T2/S1-T3 |
| R-2 | A `tests/ui/run/async/` file depends on rescue | S1-T2 | the UI run in S1-T2 | the prototype ran them 50/50; if one fails, stop and amend |
| R-3 | Windows has no prototype run | all | CI | fix in the task's commit if CI fails |
| R-4 | A hung test leaves orphaned `eml` processes | local runs | the hygiene rule in section 6 | kill after each run; the roadmap item fixes the cause |

## Appendix: main points settled in review

- Removing rescue can hang a `main` blocked in an in-place read with tasks placed on worker 0 → the placement ring (D-3), the status.md limit, and the pool-backed reads roadmap item.
- The design first kept VM preemption → removed, so that the VM and native code share one semantics. `Seeded` stays as a test instrument.
- Grep checks over docs would always match this plan → they exclude `docs/plans` and `docs/reports`.
