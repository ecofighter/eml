# Appendix S3: tests, harness, CI and Linux measurement

S3 r2 Part 1. W/s3-probe/* is copied to probes/ next to this file. Unit names S3-x map to A-units in s4-integration.md §1.3; s4-integration.md §1.2 (D-5: no WORKERS_PENDING, all async/ files at A10) and §5 (K2-d) override this appendix where they differ.


Part 1 is the complete revised deliverable. Part 2 answers S3-review-r1 finding by finding.

Paths are relative to /home/user/eml, at commit 2c8de78. W = /tmp/claude-0/-home-user-eml/ff77671a-e89e-5f5c-8ef2-c41b31f88c75/scratchpad/at.

- [V] marks a claim I verified, with how.
- [I] marks an inference.
- (r2) marks what changed in this round.

The Japanese drafts are rough. The implementer rewrites them using the natural-japanese skill. Plan-unit names (U1–U9, S3-x) appear only in this deliverable. They never go into code comments or `docs/` (finding B-2).

# Part 1: deliverable

## 1. Conclusion

### 1. UI harness

**How `async/` runs at N = 2 and 8.** Exactly like `par/`:
- checked mode, `Sched::Sequential`, the virtual clock, and the file's signal marks;
- only `Limits.workers` changes;
- the runs happen after the seed runs, inside `ui::run` and `ui::run_fail`.

**What is compared at N ≥ 2.** For `async/`: stdout, stderr, written files and how the run ended. `RunStats` is never compared for `async/` (`Compare::Nothing`, G-A18). `par/` keeps its stats comparison.

**Extra check per `run/async/` run at N ≥ 2.** Every run that returns `Finished` must satisfy `finished.workers ≥ 1 && finished.placed_tasks > 0`.
- (r2) The meaning of `placed_tasks` is pinned in §5.1.
- This check depends on `next_place` starting at 1, which the orchestrator adopts as an S1 amendment.

**Opt-out mark: `-- workers: skip (<reason>)`.**
- It sits in `leading_marks` next to `windows:` and `signal:`, and the reason is required.
- It is allowed only on `run/` and `run-fail/` entry files under `par/` or `async/`. Anywhere else, or when malformed, it panics.
- (r2) A file gets the mark only after a diff has been observed at N = 2/8 on the real code, and only if no byte-identical rewrite removes the dependence (§5.2).

**Enabling per unit.** A temporary constant `WORKERS_PENDING` in ui.rs lists the file prefixes that do not run at N ≥ 2 yet.
- (r2) Its value is `["async/chan_", "async/counter", "async/net_", "async/signal_"]`. `counter.em` now goes with the channel files at U7 (S2).
- U5 enables the remaining 39 `run/` files and 5 `run-fail/` files.
- U6a removes `net_`, U7 removes `chan_` and `counter`, and U8 removes `signal_` together with the constant.
- `workers_runs_cover_par_only` is replaced by `workers_runs_cover_par_and_async`.

**Classification (81 files read).**
- 78 are independent by analysis.
- `shield_boundary.em` is order-dependent (P1). It gets a one-line rewrite in U5, listed as kind 2 (r2, C-3).
- `drain_within_cancelled_at_deadline.em` and `drain_within_deadline_coincides.em` are predicted order-dependent (P3). (r2) They are stress-run unmarked at U5 and marked only if they diverge.
- Five more predictions are confirmed at their units: the 13 `net_*` files at U6a, and the `Chan.recv` stop site at U8.

**Categories that stay at N = 1.** Everything outside `par/` and `async/` (`tls/`, `http/`, `std/`) stays at N = 1. This is deferred in the roadmap.

### 2. Fast mode at N ≥ 2 (r2, A-1)

The UI harness still never runs fast mode at N ≥ 2. One process runs all files, so a scheduler bug in fast mode could corrupt other files.

New targeted tests cover the configuration users actually get:
- `the_fast_mode_runs_tasks_on_four_workers` in eml_vm `workers.rs`: fast mode, `Sched::Normal`, Virtual clock, N = 4. It also runs under TSan.
- `eml_run_spreads_tasks_over_four_workers` in eml_cli `cli.rs`: the real binary with its default config plus `--workers 4`, Real clock.

(r2) A finding of my own: three `cli.rs` signal tests use `yield_now` to hand over, then run the binary with the default worker count (CPU count). After the stage they would race. They get `--workers 1`, which is kind 3.

`tls.rs` uses `default_limits()`, which means `workers = 1`, so it stays N = 1 [V].

### 3. Timing

Measured in a scratch harness (harness cost only; tasks still stay on worker 0):

| Measurement | Before | After |
|---|---|---|
| `ui::run`, `async/` files, serial sum | 3360 ms | 3728 ms |
| `ui::run_fail`, `async/` files, serial sum | 420 ms | 435 ms |
| Largest single file | 109 ms | 162 ms |

`par/` files that start real worker threads take at most 50 ms each.

The procedure for each enabling unit is in §5.5:
- step 1 is the 5 s whole-test check;
- step 2 is the 1 s per-file check;
- the wall time of `ui::run` before and after goes into the commit message (r2, B-4).

### 4. Rust tests and CI

New tests go into the existing topic files (§5.4).

CI changes (§5.7):
- U4: TSan step `-p eml_vm --lib counters::`, and `counters::` in the Miri lib step (S2's tests).
- U6a: TSan and ASan add `io::`.
- U7: TSan adds `chan::` (S2: new N = 2/4/8 tests go in `chan.rs`).

No other Miri target is added.

### 5. Linux speedup measurement (r2, A-2)

**Compute scaling** uses a new script `bench/async/scale.sh`. In one session, interleaved, it runs:
- `spread.em`: 8 tasks, each computing `fib 30`;
- its `par` twin `spread_par.em`;
- a hardware control: N concurrent `--workers 1` processes, each doing 8/N of the work;
- S2's `chan_pingpong.em`.

Each runs at N = 1, 2 and 4.

**Basis and close criterion.**
- Speedup basis: median raw process wall time. S(N) = t(1) / t(N).
- Close criterion: S_spread(N) ≥ 0.80 × S_hw(N) for N = 2 and N = 4. The user confirms this.

**Measured today on the 4-vCPU cloud VM (release build):**

| | t(1) | t(2) | t(4) | S(2) | S(4) |
|---|---|---|---|---|---|
| Hardware control | 926 ms | 487 ms | 285 ms | 1.90 | 3.25 |
| `par` twin | 853 ms | 512 ms | 490 ms | 1.67 | 1.74 |

The `par` twin plateaus at N = 4. That is why the control is hardware, not `par`.

**Net cases** use `bench/net.sh --workers 1,2,4`: `serve-self-32`, `conn-self-32`, `busy-self` and `echo-self-fresh`. They feed the U6b gate and are recorded only.

**Where results go.** A new heading 「worker の数と実時間」 in benchmarks.md.

**One-task startup cost, a proxy for `next_place` = 1 (r2, B-5).** Measured today. Spawning the extra worker threads costs:
- +0.4 ms at N = 4;
- +15.7 ms at N = 64 (about 0.25 ms per thread).

A program with one task pays this at default `eml run` after the stage. It is measured again at U5.

### 6. RunStats

N = 1 snapshots stay byte-identical: `bench.rs`, `tasks.rs`, `chan.rs`, and the UI seed comparisons against sequential. `cargo insta test --check` is a criterion in every unit (r2, C-4). Nothing is compared at N ≥ 2 for `async/`.

### 7. Documentation

Drafts are in §5.7:
- testing.md;
- roadmap.md. (r2, B-1) Every `async-threads` mention is listed, including line 324.
- benchmarks.md;
- multicore.md 「テスト」;
- CLAUDE.md.

### 8. Units

| Part | Lands in |
|---|---|
| S3-0 | plan commit |
| S3-4 | U4 |
| S3-5, S3-6a, S3-7, S3-8 | U5, U6a, U7, U8 |
| S3-9a | right before U9 |
| S3-9b | U9 |
| S3-6b | if U6b lands |
| S3-close | close commit |

Each unit's criteria are in §5.8.

## 2. Rationale

**Why N ≥ 2 copies the `par/` settings.** Checked mode catches ownership and leak errors where they cross workers. Sequential keeps each worker's own order fixed. That leaves real parallelism as the only source of nondeterminism.

**Why fast mode at N ≥ 2 goes into dedicated tests, not the UI harness.** nextest runs each test in its own process, so undefined behaviour stays inside the failing test. `ui::run` runs about 585 files in one process.

**Why `RunStats` is not compared for `async/` at N ≥ 2.** Who owns a task depends on the race between starts. `peak_stack` and similar fields follow from that. G-A18 fixes this decision.

**Why `placed_tasks` and `workers` replace stats.** They guard against a silent N = 1-equivalent run. Tasks never move (G-A7). Only the owner materializes and runs a task (G-A3). So a task counted as placed did execute on another worker. `workers ≥ 1` shows the threads existed.

**Why the mark is a first-line mark.** It follows the `windows:` precedent: grep counts the marks, and the reason sits next to the file. Adding the line keeps snapshots byte-identical [V].

**(r2, C-1/C-2) The rewrite/mark rule is about output only.** A file is touched only if its output can differ at N ≥ 2.
- `race.em` and `timeout_cancelled.em` use the same `bit_first` helper. Their output cannot differ, because the child's later timer only advances after `main` cancels. They stay unchanged. At N ≥ 2 they may not reproduce the "bit first" interleaving, and the N = 1 and seed runs keep covering it. testing.md states this.
- `shield_boundary.em` can print "unreachable", because no wait synchronizes the child's end with `main`'s cancel. It is rewritten.
- The two drain files depend on S1's partition-expiry behaviour. They are observed before any mark.

**(r2, A-2) Why the control is hardware rather than `par`.** Today's `par` twin stops scaling between N = 2 and N = 4 (S 1.67 → 1.74). On the same VM, four independent processes run in parallel (S_hw(4) = 3.25) [V]. A `par`-relative criterion would therefore accept a weak async result. The hardware control measures what the machine can give, under the same per-task code, in the same session. The `par` row stays as information, and its plateau is a finding for the existing roadmap item 「`par` の右の子を置く粒度」.

**Why `serve-self-32` decides U6b.** Before U6b, accepted streams are bound to the listener's host, so a server with many connections is the shape U6b changes. A single-connection echo cannot scale.

**Why `bench.py` defaults to `--workers 1`.** For the same reason `bench/run.sh` pins `EML_WORKERS=1` (benchmarks.md:101 [V]).

## 3. Alternatives rejected

- **A temporary per-file mark for not-yet-enabled files:** it churns 30 files. The prefix list does the same with one line per unit.
- **Comparing a subset of stats at N ≥ 2:** it would need a new `Compare` variant, and it conflicts with G-A18.
- **Seeded runs or fast mode at N ≥ 2 in the UI harness:** see §2.
- **A UI "smoke" mark for fast-mode N ≥ 2 (r2):** it adds a second harness mode with the same undefined-behaviour risk. The two dedicated tests cover the shipped configuration.
- **Extending to `tls/`, `http/` and `std/`:** deferred. Those categories are not seeded, and `net_*` plus `io.rs` cover the cross-worker paths.
- **A new `threads.rs` test file:** testing.md asks for one file per topic, and S1's criteria name `io::`.
- **Miri on a two-worker run:** excluded for cost, the same as for `par` (roadmap.md).
- **A `par`-relative close criterion (r2):** rejected on today's data (see §2). `par` stays a recorded row.
- **Marking the drain files in advance (r1):** replaced by observe-then-mark (C-2).
- **Absolute thresholds (r1: w4/w1 ≤ 0.45):** today's `par` twin would fail them on this VM. Replaced by the hardware-relative criterion.

## 4. Assumptions

- **A1 (r2, now a requirement on S1):** `Finished.placed_tasks` is defined as in §5.1.
- **A2:** `next_place` starts at 1. The orchestrator adopts this as an S1 amendment, and S1's test expectations and G-A7 change with it.
- **A3:** a stop on a worker k ≥ 1 is quiet, and worker 0 reports the stop at its own location.
  - This is true today (workers.rs:507-520 [V]).
  - The `sleep_ms` stop texts were confirmed by the reviewer [V by review].
  - The `Chan.recv` pair is checked at U8.
- **A4 [I]:** forwarding a Virtual op costs under 100 µs, and each `net_*` file issues fewer than 100 ops.
- **A5 [V]:** the cloud VM has 4 vCPUs (`lscpu`: 1 thread per core, 4 cores, Xeon @ 2.80 GHz). Four concurrent processes give S_hw(4) = 3.25.

## 5. Implications for implementation

### 5.1 Harness code shape and the `placed_tasks` definition

**`Finished.placed_tasks` — text to pin in S1:**

> "the number of `Async` tasks of this run whose `owner`, set once by `Registry::start`, differs from the physical worker that executed the `start`. Counted under the registry lock in both modes; summed over the run; not part of `RunStats`."

Why "owner ≠ starting worker" and not "owner ≠ 0":
- It counts placement decisions, which is what the check guards. That is the same notion as `Finished.placed` for `par` right children ("ほかの worker に置いた").
- "owner ≠ 0" would also count a task started on worker 3 and owned by worker 3 (no crossing). It would also tie the meaning to worker 0 being special, which `main-pinning` changes.

The evidence that a placed task actually ran elsewhere:
- G-A3/G-A7 say tasks never move and only the owner materializes and runs them.
- `Denied` places on self, so it is not counted.
- The harness also asserts `finished.workers ≥ 1`, the existing field that counts created worker threads.
- No new observable is needed.

**common/mod.rs:**
```rust
pub struct Marks { pub windows_skip: bool, pub workers_skip: bool, pub signals: Vec<VirtualSignal> }
// leading_marks: also recognises `<leader> workers:`; any order with windows:/signal:
pub fn workers_skip(line: &str, leader: &str) -> Result<bool, String>
//   shares a private parser with windows_skip (windows_skip's message unchanged);
//   error: "the workers mark is `{leader} workers: skip (<reason>)`, not `{line}`"
pub fn marks_of(shown, text, leader, run_entry: bool) -> Marks   // rename `signals` -> `run_entry`
//   !run_entry && workers_skip -> panic "{shown}: workers marks are not supported here"
pub fn for_each_file(root, extension, run_entry: bool, test: impl Fn(&Path, &Marks) + Sync)
//   callers: ui.rs run/run_fail/check_fail, repl.rs:346 (mechanical)
```

**ui.rs:**
```rust
const WORKER_COUNTS: [usize; 2] = [2, 8];
/// worker をまたぐ道がまだない機能の `async/` のファイル。N >= 2 では流さない
const WORKERS_PENDING: [&str; 4] = ["async/chan_", "async/counter", "async/net_", "async/signal_"];

/// 複数の worker で流すのは `par/` と `async/` で、印 `-- workers: skip (<理由>)` のないファイルである
fn workers_run(suffix: &str, skip: bool) -> bool {
    let category = suffix.starts_with("par/") || suffix.starts_with("async/");
    assert!(category || !skip, "{suffix}: the workers mark is only for par/ and async/");
    category && !skip && !WORKERS_PENDING.iter().any(|p| suffix.starts_with(p))
}

fn assert_workers_agree(suffix, program, path, root, signals, checked) {
    for workers in WORKER_COUNTS {
        let ran = execute_in(program, path, root, signals, true, Sched::Sequential, workers);
        // タスクを置く worker はスケジュールで決まるので、`async/` の `RunStats` は比べない
        let compare = if suffix.starts_with("async/") { Compare::Nothing } else { compares_stats(checked, &ran) };
        assert_agrees(suffix, &workers_label(workers, signals), &ran, checked, compare);
        if suffix.starts_with("async/") && let Some(finished) = ran.finished {
            assert!(finished.workers >= 1 && finished.placed_tasks > 0,
                "{suffix}: placed no task on another worker under `--workers {workers}`; start a task (docs/implementation/testing.md の「seed で流す」)");
        }
    }
}
// run()/run_fail(): closure `|path, marks|`, use marks.signals; gate with workers_run(&suffix, marks.workers_skip)
// rewrite the doc comments of WORKER_COUNTS/workers_run (the "async/ を N >= 2 で流しても確かめることがない" sentence is false)
```

**Self-tests.** These are the final forms, at U8. Before that, add `assert!(!workers_run("<pending prefix>x.em", false))` for each prefix still pending.
```rust
#[test] fn workers_runs_cover_par_and_async() {
    assert!(workers_run("par/x.em", false)); assert!(workers_run("par/dir", false));
    assert!(workers_run("async/x.em", false)); assert!(workers_run("async/dir", false));
    assert!(workers_run("async/net_x.em", false)); assert!(workers_run("async/chan_x.em", false));
    assert!(workers_run("async/counter.em", false)); assert!(workers_run("async/signal_x.em", false));
    assert!(!workers_run("par/x.em", true)); assert!(!workers_run("async/x.em", true));
    assert!(!workers_run("panic/x.em", false)); assert!(!workers_run("parse/x.em", false)); assert!(!workers_run("tls/x.em", false));
    assert_eq!(WORKER_COUNTS, [2, 8]);
}
#[test] #[should_panic(expected = "the workers mark is only for par/ and async/")]
fn a_workers_mark_outside_par_and_async_fails() { workers_run("io/x.em", true); }
#[test] #[should_panic(expected = "workers marks are not supported here")]
fn a_workers_mark_in_a_transcript_fails() { marks_of("x.repl", "# workers: skip (x)\n> 1\n", "#", false); }
#[test] fn the_workers_mark_needs_a_reason() { /* Ok(true) "-- workers: skip (x)"; Err for "-- workers: skip", "-- workers: skip ()", "-- workers:skip (x)"; Ok(false) "-- 説明"; leading_marks sets all three kinds in a mixed header */ }
/// `Async.run` だけのプログラムはタスクを置かず、ハーネスが落とす
#[test] #[should_panic(expected = "placed no task on another worker")]
fn an_async_file_that_places_no_task_fails() {
    // "import Async\n\nmain : Unit -> <IO, Panic> Unit\nmain () = println (show (Async.run (fn () -> 1)))\n"
    // load as run/async/short.em, checked = execute_in(.., 1), assert_workers_agree("async/short.em", ..)
}
```

The existing `Finished { .. }` literals in ui.rs gain `placed_tasks: 0`. This is kind 3.

### 5.2 Classification rule and procedure (goes into testing.md 「seed で流す」)

**What counts as order-dependent.** An `async/` file is order-dependent at N ≥ 2 if its *output* (stdout, stderr, written files, how the run ended) can differ under any of these:

- **P1, no hand-over.** The file relies on `yield_now` or a park handing the worker to a specific task before the yielder continues. The typical case is a cancel that must arrive before its target finishes, while nothing on the target's path waits for the canceller to go idle (no timer, no `await` on the canceller).
- **P2, FIFO across tasks.** The file relies on a task started earlier running earlier.
- **P3, same-deadline timers.** Timers of different tasks are due at the same virtual instant, and the woken tasks act on each other.
- **P4, concurrent printers or a varying stop site.**
  - Two prints that nothing orders: no `await`/`run` return, no channel hand-off, and no timer that needs the other printer idle.
  - Or a `run-fail/` text whose function name could change with worker 0's last waiting task.

**Not order-dependent.**
- A cancel or failure whose every arrival point prints the same thing. `start` is not a cancellation point (stdlib.md:1063 [V]).
- A file whose output is fixed even though N ≥ 2 may not reproduce the exact interleaving its comment describes. The N = 1 and seed runs cover that interleaving. Example: the `bit_first` helper in `race.em` and `timeout_cancelled.em`.

**Procedure for a predicted or observed dependence.**
1. Run the file unmarked at N = 2 and N = 8 on the code of the unit that enables it, 200 times each, using the stress loop in §5.5 step 3.
2. If no diff is seen, leave the file enabled and record the prediction in the plan.
3. If a diff is seen, rewrite the file if a rewrite keeps the snapshot byte-identical and keeps the scenario it describes at N = 1. List the rewrite in the plan as kind 2, with the reason.
4. Otherwise add `-- workers: skip (<reason naming the observed diff>)`. This is kind 3: the program and snapshot are unchanged.
5. A file that later fails only sometimes in CI goes through the same steps. First debug it as a VM bug. CI never retries.

**Exception: `shield_boundary.em`.** It is rewritten in U5 without waiting for an observed diff.
- Its P1 race has no synchronisation at all.
- The rewrite makes its own comment (「子が shield の中で待つ間に s を取り消す」) true at every N.
- The rewrite is listed as kind 2 (C-3).

### 5.3 File classification (81 files read [V]; analysis [I])

Rows are order-independent unless the "Class / action" column says otherwise.

| Unit | Files | Class / action |
|---|---|---|
| U5 run/ (39) | await_cancelled, cancel, cancel_persists, cancel_reaches_nested_run, cancel_reaches_open_scopes, cancel_requested, cancelled_file, cancelled_kind, check_cancelled, closed_scope, drain_within, drain_within_cancelled, drop_cancels, drop_handle, exit_in_task, failed_child_cancels_siblings, handle_outlives_run, lin_result, nested_run, nested_run_cancelled, race, race_failure, scope_cancel, scope_cancel_reaches_body, scope_child_failure_wins, scope_stays_open, shield, shield_marking, spawn_grandchild, start_await, start_in_flagged, timeout, timeout_cancelled, timeout_nested, try_all_isolates, yield_now | independent (36). `race` and `timeout_cancelled` may not reproduce the "bit first" interleaving at N ≥ 2; output unaffected. |
| U5 run/ | shield_boundary | P1: the `bit_first` child can finish before `main` cancels and print "unreachable". **Rewrite**: line 55, the `Async.yield_now ()` inside the `shield` of the `bit_first` call → `Async.sleep_ms 10`. Snapshot unchanged, seeds pass [V]. Kind 2. |
| U5 run/ | drain_within_cancelled_at_deadline | predicted P3 (the `o` task's 100 ms timer and the drainer's 100 ms deadline): "m: Cancelled" instead of "m: done". **Observe at U5** (§5.2). If it diverges, mark `-- workers: skip (同じ期限の2つのタイマーで起きたタスクの順に頼る)`. |
| U5 run/ | drain_within_deadline_coincides | predicted P3: "a: Cancelled" instead of "a: done". **Observe at U5**. If it diverges, mark `-- workers: skip (同じ期限のタイマーがすべて満ちてからタスクが動くことに頼る)`. |
| U5 run-fail/ (5) | await_rethrows_panic, cancelled_run, caught_await_still_fails_run, drain_own_scope, spawned_failure_fails_run | independent |
| U6a run/ (13) | net_accept_within, net_cancel_read, net_connect_errors, net_echo, net_failed_child_cancels_accept, net_listen_connect, net_read_before_write, net_read_chunk_within, net_read_line, net_read_line_within, net_server_try_all, net_shutdown_write, net_write_after_close | independent [I]: one connect per run, and every time-dependent step waits for global idleness. Confirm by the U6a stress step (completion instants vs `_within` deadlines). |
| U6a run-fail/ (2) | net_accept_deadlock, net_read_deadlock | independent |
| U7 run/ (12) | 11 `chan_*` files and `counter` | independent (S2, final) |
| U7 run-fail/ (1) | chan_deadlock | independent |
| U8 run/ (5) | signal_cancelled_wait, signal_many_waiters, signal_race, signal_repeat, signal_scheduled_keeps_the_run_alive | independent: distinct virtual instants |
| U8 run-fail/ (4) | signal_instead_of_deadlock, signal_interrupt_unheard, signal_terminate_unheard, signal_wait_deadlock | independent under A3. Confirm at U8 that the `Chan.recv` texts (`signal_instead_of_deadlock`) stay identical at N = 2/8. |

Running totals:
- U5: 39 run + 5 run-fail files (of which up to 2 may be marked).
- End of U8: 69 run + 12 run-fail, minus the marks.

`Async` users outside `async/` stay at N = 1 and are not seeded: `run/kinds/scheduler.em`, `run/io/try_io_lin_pair_after_yield.em`, `run/std/{std_chan,std_atomic,proc_child_released_on_abort}.em`, `tls/*` and `http/*`. The REPL transcripts are not affected either.

### 5.4 Rust tests (placement follows testing.md 「テストの置き場所」)

| Unit | Test | Where | Asserts |
|---|---|---|---|
| U2 | `a_cancel_and_a_submit_are_ordered_by_the_lock` (S1) | `crates/eml_vm/src/cells.rs` `#[cfg(test)]` | per S1 |
| U3 | `a_handle_dropped_under_the_registry_lock_does_not_deadlock`, `mail_keeps_arrival_order` (S1) | cells.rs `#[cfg(test)]` | per S1 |
| U3 | `a_mail_item_pending_at_the_deadlock_check_defers_the_verdict` (S1) | `crates/eml_vm/src/workers.rs` `mod tests` | per S1 |
| U4 | `counters::` tests (S2) | eml_vm lib | per S2. Run under TSan and Miri (§5.7) |
| U5 | `placement_rotates_over_every_worker` (S1; owners 1,2,0,1,2,0 with start 1) | `#[cfg(test)]` in the module that picks the owner | per S1 as amended |
| U5 | `the_first_start_starts_the_workers`, `a_start_after_a_nested_run_starts_the_workers`, `exit_in_a_task_on_another_worker`, `a_timer_on_worker_1_advances_the_virtual_clock` (S1) | `crates/eml_vm/tests/workers.rs` | per S1 |
| U5 | **`handles_dropped_on_every_worker_are_freed_once`** (Mail-drain race) | `crates/eml_vm/tests/workers.rs` | see below |
| U5 | **`the_fast_mode_runs_tasks_on_four_workers`** (r2, A-1) | `crates/eml_vm/tests/workers.rs` | see below |
| U6a | `an_echo_between_two_workers_completes`, `a_forwarded_submit_is_settled_by_its_host`, `an_op_record_left_on_worker_1_is_swept` (S1) | `crates/eml_vm/tests/io.rs` (extend its header doc comment) | per S1. The real echo checks lower bounds only, with a 3 s watchdog. |
| U6a | `a_notice_after_the_last_collect_is_counted_as_left` (S1) | workers.rs `mod tests` | per S1 |
| U6a | **`eml_run_spreads_tasks_over_four_workers`** (r2, A-1) | `crates/eml_cli/tests/cli.rs` | see below |
| U7 | new N = 2/4/8 `chan::` tests (S2) | `crates/eml_vm/tests/chan.rs` | per S2. The 11 existing tests stay at N = 1. |
| U8 | `a_pending_signal_reaches_a_waiter_on_worker_1` (S1) | `crates/eml_vm/tests/signal.rs` | per S1. Fix the doc comment of the existing `a_pending_signal_reaches_a_waiter_on_two_workers` (kind 3). |

**`handles_dropped_on_every_worker_are_freed_once`.**
- Setup: N = 4, checked mode, Sequential, Virtual.
- `main` starts 16 parents. Each parent starts 2 children. Each child runs `spin 200`, then `yield_now`, then returns a `MutArray`.
- Each parent drops one handle at once, calls `yield_now`, then drops the other.
- `main` awaits the 16 parents and prints "done".
- Expect: `Ok`, stdout `done\n`, `placed_tasks ≥ 1`, no leak.

**`the_fast_mode_runs_tasks_on_four_workers`.**
- Three source programs run in fast mode, `Sched::Normal`, Virtual, N = 4:
  - 16 tasks each return the `fib` of a different argument, and the results are awaited in a fixed order;
  - the `cancel.em` program;
  - the `net_echo.em` program.
- Each program's stdout and result must equal its checked N = 1 sequential run.
- Each fast run must report `placed_tasks ≥ 1`.
- The test runs in its own nextest process, so undefined behaviour cannot spread.

**`eml_run_spreads_tasks_over_four_workers`.**
- Runs `eml run --workers 4 main.em` with the default sched and Real clock.
- The program starts 16 compute tasks, plus a loopback listener task with one client that does one write/read round trip. It prints the sum and the echoed line.
- Expect exit 0 and the exact stdout.
- Watchdog: the existing `DEADLINE`.

**Pinning three existing `cli.rs` tests to N = 1** (r2, my finding; kind 3: the arguments change, the expected values do not).

`a_terminate_reaches_a_waiting_task`, `an_interrupt_after_a_delivered_one_ends_the_program_with_130` and `a_second_signal_ends_a_program_whose_waiter_cannot_be_reached` (used by two `#[test]`s) all:
- rely on `Async.yield_now ()` letting the waiter reach `Signal.wait` before `ready` is written (P1);
- run the binary with the default worker count;
- pass `--sched sequential` (cli.rs:1175, 1214, 1271 [V]).

At N ≥ 2 the signal can arrive before the waiter waits, so they would race on multi-core runners. The fix:
- Add `"--workers", "1"` to their `args`.
- Their doc comments gain 「worker を1つに固め、`yield_now` で待ち手に順を渡す」.
- The multi-worker signal paths are covered by `signal.rs` (`a_pending_signal_reaches_a_waiter_on_worker_1`, and the existing two-worker tests).

**Other `cli.rs` tests checked [V]:**
- `an_interrupt_stops_a_sleeping_program_with_130` and `a_terminate_ends_an_idle_program_with_143` run only `main`, so they place nothing.
- `an_interrupt_stops_a_par_program_with_130` is unaffected.

**Q5.** No N ≥ 2 runs are added to the stats in `bench.rs`, `tasks.rs` or `chan.rs`.

### 5.5 Timing procedure (at the end of S3-5, S3-6a, S3-7 and S3-8)

1. **Whole-test check (the 5 s rule; CI uses 10 s).** Run:
   ```sh
   cargo nextest run --profile timing -j 1 -p eml_cli --test integration -E 'test(=ui::run) | test(=ui::run_fail)'
   ```
   Expected: 2 tests pass. Also record `ui::run`'s wall time before and after the unit in the commit message, from `cargo nextest run -p eml_cli --test integration -E 'test(=ui::run)'`.

2. **Per-file check (the 1 s rule).**
   ```sh
   cargo test -q -p eml_cli --test integration --no-run
   for f in tests/ui/run/basics/hello.em tests/ui/run/async/*.em tests/ui/run-fail/async/*.em; do
     s=$(date +%s%N); EML_UI_FILTER="$f" RUST_TEST_THREADS=1 cargo test -q -p eml_cli --test integration -- --exact ui::run ui::run_fail >/dev/null 2>&1 || echo "FAIL $f"
     echo "$(( ($(date +%s%N) - s) / 1000000 )) $f"; done | sort -n | tail -5
   ```
   Expected: no `FAIL` line, and every file's time minus `hello.em`'s time is under 1000 ms. Record the largest value.

3. **Stress check (used by §5.2, and for the files the unit enables).**
   ```sh
   for i in $(seq 200); do EML_UI_FILTER=<file> cargo test -q -p eml_cli --test integration -- --exact ui::run ui::run_fail >/dev/null 2>&1 || echo "diff at $i"; done
   ```
   - The harness runs N = 2 and N = 8 on every iteration.
   - At U5, run it for the two drain files and for `shield_boundary`.
   - At U6a, run it for the 13 `net_*` files (50 iterations each).
   - At U8, run it for `signal_instead_of_deadlock` (50 iterations).
   - Expected: no "diff" line, except where §5.2 then applies.

4. **A file over 1 s.**
   1. Compare its N = 1 and N = 2 runs, with `WORKER_COUNTS` set to `[2]` locally.
   2. If N = 2 is more than 3× N = 1, or the file hangs, debug it as a VM bug.
   3. If the cost is inherent per-op forwarding, shrink the program's work without changing its output, and list the change in the plan.
   4. Last resort: `-- workers: skip (N = 2 以上で 1 秒を超える)`, and keep the coverage in `io::`.

### 5.6 Linux speedup measurement

**Programs.** S2 already creates `bench/async/`. `bench.rs` lists only top-level `.em` files, because the directory has no `.em` extension (bench.rs:108-118 [V]).

- **`bench/async/spread.em`** (probe: probes/spread.em).
  - Arguments: `k m`.
  - Starts `k` tasks that each compute `fib m`, awaits them, and prints `spread done <sum>`.
- **`bench/async/spread_par.em`** (probe: probes/spread_par.em).
  - Arguments: `k m`.
  - Does the same work by splitting `k` in halves with `Par.par` down to single `fib m` leaves.
  - The output is identical to `spread.em`.
- **`bench/net/serve.em`** (probe: probes/serve.em).
  - Arguments: `n size conns`.
  - A server scope accepts `conns` connections and spawns one `serve` task per connection.
  - `conns` client tasks each do `n / conns` round trips.
  - Prints `serve done <each*conns>`.

The probe outputs on today's binaries [V]:

| Command | Output |
|---|---|
| `spread.em 8 22` | `spread done 141688` |
| `spread_par.em 8 25` | `spread done 600200` |
| `serve.em 2048 16 32` | `serve done 2048` |

**`bench/async/scale.sh` (new; Linux and macOS; release build).** It takes `EML` and `ROUNDS`, defaulting to `target/release/eml` and 11. In each round, in rotating order, it runs:
- (a) `$EML run --workers N bench/async/spread.em 8 30` for N in 1, 2, 4;
- (b) `$EML run --workers N bench/async/spread_par.em 8 30` for N in 1, 2, 4;
- (c) for N in 1, 2, 4: N concurrent `$EML run --workers 1 bench/async/spread.em $((8/N)) 30`, timing until the last one exits;
- (d) `$EML run --workers N bench/async/chan_pingpong.em` for N in 1, 2, 4 (S2's program; it checks the `800040000` output).

It then prints one Markdown table: `| 物差し | t(1) ms | t(2) | t(4) | S(2) | S(4) |`. The values are medians of raw process wall time, and S(N) = t(1) / t(N). It ends with a `lscpu` (or `sysctl -n machdep.cpu.brand_string hw.ncpu`) line.

Every run's stdout is checked against the expected line, and the script exits non-zero on any mismatch.

**`bench/net` tooling.**
- `bench.py` gets an option `--workers LIST`, default `1`.
  - `--workers N` is always passed to the program.
  - When the option is given, the labels become `LABEL:wN`; otherwise they stay `LABEL`.
  - Labels are interleaved as today.
  - Older binaries accept `--workers` [V R2 §7].
- `report.py`, for labels of the form `X:wN`, prints `## worker の数: X (実時間の中央値、床を引いて単位あたり µs)` with the columns `| 件 | 単位 | w1 | w2 | w4 | w2÷w1 | w4÷w1 | 判定 w2 | 判定 w4 |`.
  - The ratio uses the floor-subtracted medians.
  - 判定 applies the existing rule (ratio ≤ 0.90 or ≥ 1.10, and the 25–75% ranges of raw wall time do not overlap) to raw wall time.
  - Each basis is named in the heading.
- New `cases.tsv` row:
  ```
  serve-self-32	serve.em	20480 16 32	2048 16 32	-	-	20480	2048	往復
  ```

**Measurement (S3-9b; Linux; idle; release build).**
```sh
cargo build --release -p eml_cli
bench/async/scale.sh > /tmp/at-scale.md
bench/net.sh A=target/release/eml --workers 1,2,4 --no-strace \
  --only '^(base|serve-self-32|conn-self-32|busy-self|echo-self-fresh)$' > /tmp/at-net.jsonl
python3 -I bench/net/report.py /tmp/at-net.jsonl > /tmp/at-net.md
```

**Recording.** New heading `### worker の数と実時間` under benchmarks.md 「ネットワークの計測」, containing:
- the machine (`lscpu` line), the commit and the commands;
- the `scale.sh` table and the "worker の数" table, copied verbatim;
- one sentence per row;
- the close-criterion check with numbers;
- the U6b decision with numbers;
- after U6b, if it lands: a second `serve-self-32` / `echo-self-fresh` table.

S2's U7 `chan_pingpong` numbers go in this heading, as the `scale.sh` row.

**Close criterion (the user confirms it).** S_spread(N) ≥ 0.80 × S_hw(N) for N = 2 and N = 4, from the same `scale.sh` run.

- **Validity condition:** the control itself must show S_hw(2) ≥ 1.6 and S_hw(4) ≥ 2.4. If it does not, the machine is too noisy or effectively has too few cores (SMT, CPU quota). Re-run on another machine, or record the numbers and ask the user. Do not judge the criterion from that run.
- **Today's control on the 4-vCPU VM:**

  | | t(1) | t(2) | t(4) | S(2) | S(4) |
  |---|---|---|---|---|---|
  | Hardware control | 926 ms | 487 ms | 285 ms | 1.90 | 3.25 |
  | `par` twin | 853 ms | 512 ms | 490 ms | 1.67 | 1.74 |

  Medians of 5 runs [V]. So spread would need S(2) ≥ 1.52 and S(4) ≥ 2.60.
- **If missed:** the stage does not close. Check `placed_tasks` and `workers` with `--stats`, the time the registry lock is held, monitor preemption, and the imbalance across workers. Fix, and re-measure.
- Tests never assert speedup.

**Expected net results [I] (recorded only):**

| Case | Expected |
|---|---|
| `conn-self-32` | w4÷w1 ≈ 0.6–0.9 |
| `serve-self-32` before U6b | ≈ 0.8–1.2 (server I/O funnels to the listener's host) |
| `busy-self` | w4 < w1; w2 ≈ w1 (at N = 2 the busy task lands on worker 0, next to `main`) |
| `echo-self-fresh` | w2 ≥ w1 (forwarding) |

**U6b gate.** U6b goes into the stage if either holds:
- (a) `conn-self-32` w4÷w1 ≤ 0.90 and 差, while `serve-self-32` w4÷w1 > 0.90 or 差なし;
- (b) `echo-self-fresh` w2÷w1 ≥ 1.10 and 差.

Otherwise U6b is deferred to the roadmap (item 4 in §5.7). After U6b, re-run with `--only '^(base|serve-self-32|echo-self-fresh)$'`.

**Startup cost of `next_place` = 1 (B-5; recorded in the S1 amendment).** After the stage, a program that starts even one task starts N − 1 worker threads, at default `eml run` and in `bench/scripts.sh`.

Proxy measured today: release build, median of 21 runs, W/s3-probe [V].

| Program | w1 | w4 | w64 |
|---|---|---|---|
| `hello.em` | 14.2 ms | 13.0 ms | 15.3 ms |
| one trivial `Par.par` (worker threads are spawned) | 17.0 ms | 17.4 ms | 32.7 ms |
| one `Async.start` (today: no spawn) | 22.7 ms | 21.1 ms | 21.1 ms |

So the expected cost is about 0.4 ms at 4 workers and about 16 ms at 64 workers (≈ 0.25 ms per thread).

At U5, re-measure the one-task program (probes/onetask.em, to be copied into the plan) at `--workers 1`, `4` and `64`:
- Expected: w4 − w1 ≤ 2 ms, and w64 − w1 ≤ the `par` proxy's delta + 5 ms.
- If it is larger, it is an S1 performance issue, recorded in the plan.

### 5.7 Doc and CI changes (rough Japanese; the implementer rewrites)

**testing.md**

- 「UI テスト」:
  - New bullet after the signal-mark bullet: 「`run/` と `run-fail/` の `par/` と `async/` のエントリの `.em` は、先頭の印の行に `-- workers: skip (<理由>)` を書いて、worker の数 N = 2 以上の run から外せる (下の「seed で流す」)」.
  - The `async/` category bullet: 「逐次のほかに2つの seed でも流す」 → 「逐次のほかに2つの seed と、worker の数 N = 2 と 8 でも流す」.

- 「seed で流す」: replace the bullet that ends 「…`workers_runs_cover_par_only` が確かめる」 with:
  > `par/` と `async/` の分類 (`run/` と `run-fail/`) は、seed の run の後で worker の数 N = 2 と 8 でも流す。`Limits.workers` を変えるだけで、検査モード、逐次の方針、仮想の時計は N = 1 の run と同じである。比べるのは標準出力、標準エラー、書いたファイル、終わり方である。`run/par/` では、スケジュールによらない `RunStats` も比べ、worker の和と最大が N = 1 の値と一致しなければならない ([VM](../spec/vm.md) の「`RunStats`」)。`async/` では `RunStats` を比べない。タスクを置く worker がスケジュールで決まるためである。代わりに `run/async/` の run は、worker を作り (`Finished.workers`)、少なくとも1つのタスクを始めた worker と違う worker に置いた (`Finished.placed_tasks`) ことを確かめる。置かなければ worker 0 の上だけで流したことになる。逐次の run が VM の誤りで終われば流さない。N = 1 のスナップショットは変えず、N ごとのスナップショットは作らない。先頭の印の行に `-- workers: skip (<理由>)` を書いたファイルは N = 2 以上で流さない。印は `run/` と `run-fail/` の `par/` と `async/` のエントリにだけ書け、ほかの場所の印と理由のない印は panic する。一覧は `git grep -n '^-- workers: skip (' -- tests/ui` で出す。対象を決める規則は `workers_runs_cover_par_and_async` が確かめる

  Add an interim sentence, present only while it is true and written without unit names:
  - at U5: 「`async/` のうち名前が `chan_`、`counter`、`net_`、`signal_` で始まるファイルは、worker をまたぐ道がまだないので N = 2 以上で流さない」;
  - shrink it at U6a (drop `net_`) and at U7 (drop `chan_` and `counter`);
  - delete it at U8.

- 「seed で流す」: new bullet after 「`async/` のファイルは、出力の順がスケジュールで変わらないように書く…」:
  > `async/` のファイルは、N = 2 以上でも出力が変わらないように書く。複数の worker では、置いたタスクが始めたタスクと同時に動き、`yield_now` はほかの worker のタスクに順を渡さず、実行待ちの列の順は worker をまたいで保たれない。同じ期限のタイマーは worker ごとに満ちるので、先に起きたタスクが、同じ期限でまだ起きていないタスクを取り消せる。取り消しは、届ける側が空きになるまで取り消される側が進めない形 (時間の待ちか、届ける側を待つ `await`) で書くか、どこで届いても同じ出力になるように書く。聞く者のいないシグナルで止まる位置は worker 0 の最後のタスクが待つ関数なので、`run-fail/` のファイルはそれがどの置き方でも同じになるように書く。N = 2 以上の run は、ファイルが説明する順の組み合わせを再現するとは限らない。その組み合わせは N = 1 の run と seed の run が確かめる。N = 2 以上でだけ出力が食い違うときは、まず VM の不具合を疑う。ファイルが規則に反していると分かったら、スナップショットを変えずに書き直せれば書き直し、書き直せなければ、見た食い違いを理由に書いて印を付ける

- 「Windows で流さないテスト」:
  - 「`<leader> windows:` か `<leader> signal:` で始まる行の並びで、2種類を」 → 「`<leader> windows:`、`<leader> signal:`、`<leader> workers:` で始まる行の並びで、3種類を」.
  - Add 「`workers_skip` が worker の印を読む」.

- 「eml_vm のテスト」:
  - S3-5, new bullet: 「`workers.rs` は、worker を作る時と止める手順、`par` の子と `Async` のタスクをほかの worker に置くこと、ほかの worker のタスクの `exit`、仮想の時計、複数の worker が mail を処理しても落としたハンドルのタスクを1回だけ放すこと、速いモードの普通の方針で4つの worker にタスクを置いても出力が検査モードの N = 1 と同じことを確かめる」.
  - S3-6a, io.rs bullet: + 「ほかの worker の driver に縛ったハンドルの操作を転送して host が決着させることと、本物の driver で2つの worker にまたがる echo」.
  - S3-8, signal.rs bullet: + 「worker 1 に置いた待つタスクに保留のシグナルが届くこと」.

- 「CLI のテスト」 (S3-6a):
  - + 「`eml run --workers 4` が、計算と本物の接続のタスクを複数の worker に置いても同じ出力で終わること」.
  - + 「`yield_now` で待ち手に順を渡すシグナルのテストは `--workers 1` で流す」.

- 「AddressSanitizer」 (S3-6a): 「`api::` と `signal::`」 → 「`api::`、`signal::`、`io::` (`io::` はほかの worker の driver に記憶を貸す道を含む)」.

- 「ThreadSanitizer」:
  - S3-4: + 「`eml_vm` の lib の `counters::`」 as its own step.
  - S3-6a: + `io::`.
  - S3-7: + `chan::`.
  - The parenthesis becomes 「(`eml_rt` の remote 解放の経路と、`eml_vm` の起こしの語、run の表、run の登録表と mail、ほかの worker への操作の転送、`Counter` の atomic、worker をまたぐ channel)」.

- 「Miri」 / 「CI」 (S3-4): the `miri` line gains `counters::`. The `asan`/`tsan` lines change as above.

- 「よく使うコマンド」: the comments on the sanitizer lines change the same way. The `bench/net.sh` line gains `[--workers 1,2,4]`. Add `bench/async/scale.sh   # Async のタスクと par と別プロセスの床を worker の数 1、2、4 で比べた実時間 (release)` (S3-9a).

**benchmarks.md**
- 「ネットワークの計測」 intro (S3-9a): `--workers` sentence: 「`--workers 1,2,4` は、どのラベルも各 worker の数で流し、ラベルに `:wN` を付ける。省くと `--workers 1` で流す。固めないと `Async` のタスクを置く worker の数が機械の CPU の数で変わるためである」.
- 「件」: add the `serve-self-32` row: 「32 本の接続の echo。サーバーは接続ごとにタスクを置き、32 個のクライアントのタスクが 16 バイトの往復を繰り返す」.
- New paragraph 「`bench/async/scale.sh`」 describing (a)–(d) and the S(N) basis.
- S3-9b: the heading `### worker の数と実時間`.

**roadmap.md**
- **S3-0 (plan commit):**
  - In the `async-threads` entry, remove 「、`compute`」 and 「、[非同期の設計](async.md) の「`compute`」」.
  - Insert after it: 「- `async-compute`: タスクから CPU を使う仕事を子が1つの `par` の仕事として出し、終わるまでそのタスクだけを待たせる `Async.compute` を入れる。待つ間、worker はほかのタスクを動かす。前提: `async-threads`。設計: [非同期の設計](async.md) の「`compute`」」.
- **S3-close (r2, B-1).** Every mention of `async-threads` in roadmap.md, [V] grep at 2c8de78 plus the S3-0 line:

  | Where | Action |
  |---|---|
  | :31, the entry | remove |
  | :32 `async-rescue` 前提 | → 「前提: なし」 |
  | :33 `task-limits` 前提 | → 「前提: なし」 |
  | :49 `main-pinning` 前提 | → 「前提: なし」 |
  | the `async-compute` entry 前提 (added in S3-0) | → 「前提: なし」 |
  | :324 見直す条件 | 「`async-threads` の後、channel を多く使うプログラムのプロファイルに取り込みの歩きが見えたとき。」 → 「channel を多く使うプログラムのプロファイルに取り込みの歩きが見えたとき。」 (the condition "after Async runs on several workers" is now always met) |

  No other file under `docs/` mentions `async-threads` [V grep].
- **New deferred items (under 「マルチコア対応」):**
  1. 「仮想の時計で、同じ期限のタイマーを worker をまたいで1度に満たすこと」. 後に回した理由: 区画の項を替えるのは持ち主だけという規則を保つ。同じ期限の順は本物の時計でも決まらない。見直す条件: 同じ期限の順に頼るテストを N = 2 以上で流したいとき。
     - Write this item only if a drain file was marked or a P3 dependence was observed. Otherwise drop it.
  2. 「`async/` のほかの `Async` を使う分類 (`tls/`、`http/`、`std/` の一部) を N = 2 と 8 で流すこと」. 後に回した理由: これらは seed でも流さない分類で、出力の順がスケジュールによらないことはレビューだけが確かめている。worker をまたぐ道は `async/` の `net_*`、`eml_vm` の `io.rs`、`eml_cli` の `cli.rs` の複数の worker の run が覆う。見直す条件: `Std.Tls` か `Std.Http` が worker に依る状態を持つとき。または N = 2 以上でだけ出る不具合をこれらの分類で見つけたとき。
  3. Under 「miri.rs のテストの1つあたりの Miri の費用」: + 「`Async` のタスクをほかの worker に置く run も `miri.rs` にない。理由は同じである」.
  4. Only if U6b is deferred: 「受けたストリームを、最初に使うタスクの worker の driver に縛ること」. 後に回した理由: 計測 ([計測の基準](../implementation/benchmarks.md) の「worker の数と実時間」) で、受けを待ち受けの worker に縛ることが律速にならなかった。見直す条件: 多くの接続を受けるサーバーの実時間が、worker を増やしても縮まないと測ったとき。
  5. In the existing item 「タスクを置く worker を…いちばん短いもの」, rewrite 後に回した理由 from the measured `serve-self-32` result and cite the heading.
  6. The `par` plateau seen in `scale.sh` (S_par(4) ≈ S_par(2) today): add a sentence to 見直す条件 of the existing item 「`par` の右の子を置く粒度」 only if S3-9b confirms it on the measuring machine.

**multicore.md 「テスト」** (hand these to S1's U9 amendments):
- 「`async/` は、`Async` のタスクを複数の worker で動かすときに、順によらないものだけを N が 2 と 8 で流す」 → 「`async/` も N が 2 と 8 で流し、順に頼るファイルは印で外す ([テスト戦略](../implementation/testing.md) の「seed で流す」)」.
- The TSan sub-bullet gains `io::`, `chan::` and `counters::`.

**CLAUDE.md**
- Line 40 (ASan comment): 「(also eml_vm --test integration -- api:: signal:: io::; …)」. S3-6a.
- Line 41 (TSan comment): 「(also -p eml_vm --test integration -- workers:: signal:: preempt:: io:: chan:: and -p eml_vm --lib counters::; …)」. Add pieces as they land: S3-4, S3-6a, S3-7.
- The Miri `--lib` line: `-- cells::a_read cells::a_write counters::`. S3-4.
- Line 53: the `bench/net.sh` line gains `[--workers 1,2,4]`. Add a `bench/async/scale.sh` line. S3-9a.
- Line 153, the Testing bullet: 「the `par/` ones at 2 and 8 workers」 → 「the `par/` and `async/` ones at 2 and 8 workers except files marked `-- workers: skip (<reason>)`」. S3-8.

**.github/workflows/ci.yml**

S3-4 (U4):
```diff
@@ miri
-      - name: "cargo miri test -p eml_vm --lib -- cells::a_read cells::a_write"
+      - name: "cargo miri test -p eml_vm --lib -- cells::a_read cells::a_write counters::"
 ...
-          nix develop .#miri -c cargo miri test -p eml_vm --lib -- cells::a_read cells::a_write
+          nix develop .#miri -c cargo miri test -p eml_vm --lib -- cells::a_read cells::a_write counters::
@@ tsan (new step after the integration step)
+      - name: "cargo test -p eml_vm --lib counters::"
+        if: ${{ !cancelled() && steps.rt.outcome != 'skipped' }}
+        run: |
+          nix develop .#miri -c cargo test -Zbuild-std --target x86_64-unknown-linux-gnu -p eml_vm --lib counters::
```

S3-6a (U6a), plus ` chan::` appended to the tsan lines at S3-7 (U7):
```diff
@@ asan
-      - name: "cargo test -p eml_vm --test integration -- api:: signal::"
+      - name: "cargo test -p eml_vm --test integration -- api:: signal:: io::"
-          nix develop .#miri -c cargo test -Zbuild-std --target ${{ matrix.target }} -p eml_vm --test integration -- api:: signal::
+          nix develop .#miri -c cargo test -Zbuild-std --target ${{ matrix.target }} -p eml_vm --test integration -- api:: signal:: io::
@@ tsan
-  # worker のスレッドの間で共有する記憶 (eml_rt の remote 解放、VM の起こしの語と run の表) を ThreadSanitizer の下で流す。
+  # worker のスレッドの間で共有する記憶 (eml_rt の remote 解放、VM の起こしの語、run の表と登録表、mail、操作の転送、Counter、channel) を ThreadSanitizer の下で流す。
-      - name: "cargo test -p eml_vm --test integration -- workers:: signal:: preempt::"
+      - name: "cargo test -p eml_vm --test integration -- workers:: signal:: preempt:: io::"
-          nix develop .#miri -c cargo test -Zbuild-std --target x86_64-unknown-linux-gnu -p eml_vm --test integration -- workers:: signal:: preempt::
+          nix develop .#miri -c cargo test -Zbuild-std --target x86_64-unknown-linux-gnu -p eml_vm --test integration -- workers:: signal:: preempt:: io::
```

The fast-mode test `the_fast_mode_runs_tasks_on_four_workers` lives in `workers::`, so TSan also covers fast mode at N = 4. The existing `workers::` tests stay as they are.

### 5.8 Units and completion criteria

**Criteria common to every unit**, in addition to S1's and S2's criteria for the same unit:
- (a) `cargo nextest run` passes.
- (b) `cargo insta test --check` reports no pending or changed snapshot (G-A5, C-4).
- (c) `git diff 2c8de78 --stat -- crates/eml_cli/tests/snapshots crates/eml_vm/tests/bench.rs` is empty.
- (d) `cargo clippy --all-targets -- -D warnings && cargo fmt --check` is clean.

**S3-0 (plan commit)**
- Content: the roadmap edits of S3-0.
- Criteria:
  - `cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` → 4 pass.
  - `grep -c 'async-compute' docs/future/roadmap.md` → 1.

**S3-4 (in U4)**
- Content: the ci.yml miri and tsan `counters::` diff; CLAUDE.md lines 41 and Miri; testing.md 「ThreadSanitizer」, 「Miri」, 「CI」 and 「よく使うコマンド」.
- Criteria:
  - `nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- .github/workflows/ci.yml` → no output.
  - S2's U4 TSan and Miri commands → pass.

**S3-5 (in U5)**
- Content: §5.1; the `shield_boundary.em` rewrite; `handles_dropped_on_every_worker_are_freed_once`; `the_fast_mode_runs_tasks_on_four_workers`; the testing.md bullets for 「UI テスト」, 「seed で流す」 (with the interim sentence), 「Windows で流さないテスト」 and the `workers.rs` bullet; the marks, if any (step 4 below).
- Criteria:
  1. `cargo nextest run -p eml_cli --test integration -E 'test(ui::)'` → passes, including the five new self-tests. `ui::workers_runs_cover_par_only` no longer exists.
  2. `git diff 2c8de78 -- tests/ui/run/async/shield_boundary.em` → exactly one changed line: `Async.yield_now ()` → `Async.sleep_ms 10`.
  3. §5.5 step 3 for `shield_boundary` → no diff in 200 iterations.
  4. §5.5 step 3 for each of the two drain files, run unmarked first:
     - for each file that showed a diff, apply §5.2 and record the observed diff in the plan;
     - afterwards, `git grep -n '^-- workers: skip (' -- tests/ui` lists exactly the files that showed a diff, at line 1;
     - re-running step 3 on every enabled file now gives no diff.
  5. `cargo nextest run -p eml_vm --test integration -E 'test(workers::)'` → passes, including the two new tests.
  6. §5.5 steps 1 and 2 pass.
  7. The startup re-measurement in §5.6 is recorded in the plan.

**S3-6a (in U6a)**
- Content:
  - Remove `"async/net_"`, and flip its self-test assertion.
  - Add `eml_run_spreads_tasks_over_four_workers`, and the `--workers 1` pin on the three `cli.rs` signal tests (kind 3).
  - The ci.yml asan/tsan `io::` diff; CLAUDE.md lines 40 and 41; testing.md 「AddressSanitizer」, 「ThreadSanitizer」, 「CI」, 「CLI のテスト」, 「よく使うコマンド」 and the io.rs bullet; the interim sentence without `net_`.
- Criteria:
  1. `EML_UI_FILTER=/async/net_ cargo nextest run -p eml_cli --test integration -E 'test(=ui::run) | test(=ui::run_fail)'` → pass.
  2. §5.5 step 3 on the 13 `net_*` files (50 each) → no diff, or §5.2 is applied.
  3. `cargo nextest run -p eml_cli --test integration -E 'test(cli::)'` → pass.
  4. TSan: `nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=thread TSAN_OPTIONS=halt_on_error=1 CARGO_TARGET_DIR=target/tsan cargo test -Zbuild-std --target x86_64-unknown-linux-gnu -p eml_vm --test integration -- workers:: signal:: preempt:: io::` → exit 0.
  5. ASan: the same with `-Zsanitizer=address` and `CARGO_TARGET_DIR=target/asan`, filters `-- api:: signal:: io::` → exit 0.
  6. actionlint → no output.
  7. §5.5 steps 1 and 2 pass.

**S3-7 (in U7)**
- Content: remove `"async/chan_"` and `"async/counter"` and update the self-test; ` chan::` in the tsan step, CLAUDE.md and testing.md; the interim sentence reduced to `signal_`.
- Criteria:
  1. `EML_UI_FILTER=/async/chan_ cargo nextest run -p eml_cli --test integration -E 'test(=ui::run) | test(=ui::run_fail)'` → pass.
  2. The same filtered by `/async/counter` → pass.
  3. The TSan command with `chan::` → exit 0.
  4. §5.5 steps 1 and 2 pass.

**S3-8 (in U8)**
- Content: remove `"async/signal_"`, `WORKERS_PENDING` and its comment; the final self-test; delete the interim sentence; the signal.rs bullet; CLAUDE.md line 153.
- Criteria:
  1. `grep -n 'WORKERS_PENDING' crates/eml_cli/tests/ui.rs` → no output.
  2. `EML_UI_FILTER=/async/ cargo nextest run -p eml_cli --test integration -E 'test(=ui::run) | test(=ui::run_fail)'` → pass.
  3. §5.5 step 3 on `signal_instead_of_deadlock` (50 iterations) → no diff (A3 for `Chan.recv`).
  4. `cargo nextest run -p eml_vm --test integration -E 'test(signal::)'` → pass.
  5. §5.5 steps 1 and 2 pass.

**S3-9a (a separate commit, right before U9)**
- Content: `bench/async/spread.em`, `spread_par.em` and `scale.sh`; `bench/net/serve.em`; the cases.tsv row; the `--workers` option in `bench.py`; the table in `report.py`; benchmarks.md (intro, 件, `scale.sh` paragraph); the CLAUDE.md and testing.md command lines.
- Criteria:
  1. `cargo build --release -p eml_cli && target/release/eml run --workers 4 bench/async/spread.em 8 15` → `spread done 4880`.
  2. `target/release/eml run --workers 4 bench/async/spread_par.em 8 15` → `spread done 4880`.
  3. `target/release/eml run --workers 4 bench/net/serve.em 2048 16 32` → `serve done 2048`.
  4. `ROUNDS=1 bench/async/scale.sh` → exit 0 and a table with the rows `spread`, `spread_par`, `床 (別プロセス)` and `chan_pingpong`.
  5. `bench/net.sh A=target/release/eml --workers 1,2 --runs 1 --no-strace --only '^(base|serve-self-32)$' > /tmp/w.jsonl` → exit 0, and `grep -c '"label": "A:w2"' /tmp/w.jsonl` → 2.
  6. `python3 -I bench/net/report.py /tmp/w.jsonl | grep -c '^## worker の数: A'` → 1.
  7. `bench/net.sh A=target/release/eml --runs 1 --no-strace --only '^base$' | grep -c '"label": "A"'` → 1.
  8. The citations test passes.
  9. `cargo nextest run -p eml_vm --test integration -E 'test(bench::every_program_in_bench_has_a_test)'` → pass.

**S3-9b (in U9)**
- Content: the §5.6 measurement; the benchmarks.md heading; the roadmap deferred items; the multicore.md bullets handed to S1.
- Criteria:
  1. `grep -n '^### worker の数と実時間' docs/implementation/benchmarks.md` → 1 line.
  2. The heading contains both tables and the `lscpu` line.
  3. The control validity condition holds, and S_spread(N) ≥ 0.80 × S_hw(N) for N = 2 and 4. Otherwise the stage does not close (§5.6).
  4. The U6b decision is recorded.
  5. The citations test passes.

**S3-6b (only if U6b lands)**
- Content: re-measure, and add the second table.
- Criterion: the table exists, with `serve-self-32` w4÷w1 recorded. The expected value is ≤ 0.80 [I], but this is not a gate.

**S3-close (close commit)**
- Content: the roadmap edits of §5.7 for S3-close.
- Criteria:
  - `grep -c 'async-threads' docs/future/roadmap.md` → 0.
  - `grep -rn 'async-threads' docs CLAUDE.md --include=*.md | grep -v '^docs/plans/'` → no output.
  - The citations test passes.

**Test-change kinds (for the plan)**
- **Kind 1:** delete the unit test `ui::workers_runs_cover_par_only`. The rule it pins (`async/` never runs at N ≥ 2) is reversed. It is replaced by `ui::workers_runs_cover_par_and_async`.
- **Kind 2 (r2):** the scope "`tests/ui/run/async/shield_boundary.em`: in the `bit_first` call, the child's wait inside `shield` becomes `sleep_ms 10` instead of `yield_now`, so that the child waits until the parent has cancelled, for every worker count. Snapshot unchanged." The program and the N = 1 path change, so it is listed even though the expected text is identical.
- **Kind 3:**
  - the `Finished` literals gain `placed_tasks: 0`;
  - the `for_each_file` closure signature and the `marks_of` parameter rename;
  - first-line marks added after an observed diff (program and snapshot unchanged);
  - `--workers 1` added to three `cli.rs` signal tests, with doc comments;
  - doc-comment fixes in existing two-worker signal tests.

## 6. Unresolved items with defaults

1. **`next_place` = 1.** The orchestrator adopts it as an S1 amendment. The S1 text should carry the cost note and measurement from §5.6.
2. **Close criterion.** Default: hardware-relative, 0.80 × S_hw(N), with the validity condition. The user confirms it.
3. **U6b gate.** Default: §5.6 (a)/(b).
4. **`async-compute` position.** Default: directly after `async-threads`.
5. **N ≥ 2 for `tls/`, `http/` and `std/`.** Default: deferred (roadmap item 2).
6. **`bench/scripts.sh`** (macOS, the user's `Measure async-threads` commit). Default: leave it unpinned. The `https-get` row then reflects default `eml run`, including worker start-up. Mention this in that commit.
7. **The `par` plateau at N = 4.** Default: record it in S3-9b. Touch the 「粒度」 roadmap item only if the measuring machine confirms it.

## 7. Additional checks

- [V] All 81 `async/` UI files and the `run-fail/async` snapshots. The harness at ui.rs:176-626, common/mod.rs:1-260, repl.rs:346, sched.rs:455-474 and workers.rs:470-520.
- [V] r1 scratch harness timings: base vs N = 2/8 for `async/`, `par/`, `tls/` and `http/`.
- [V] (r2) `cli.rs`:
  - The three signal tests at :1159, :1197 and :1253 use `yield_now` before `ready` and pass `--sched sequential`, but no `--workers`.
  - `eml()` and `eml_in()` inherit the environment, so workers = `EML_WORKERS` or the CPU count.
  - `tls.rs:76-81` uses `default_limits()`, which is `Limits::default()` (`workers = 1`).
- [V] (r2) Release build in W/target-s3. Probes in W/s3-probe:
  - `scale.sh` precursor: `spread_par.em 8 30`, 5 runs at w1/2/4 → 853, 512, 490 ms.
  - Hardware control (N processes × 8/N `fib 30`) → 926, 487, 285 ms.
  - Four concurrent single-worker processes run 1.35× slower than one, so the VM really has about 4 cores.
  - `lscpu`: 4 cores, 1 thread per core.
  - Startup medians of 21 runs for `hello.em`, `onefork.em` and `onetask.em` at w1/w4/w64 (§5.6).
- [V] (r2) roadmap.md `async-threads` mentions at :31, :32, :33, :49 and :324. No other `docs/` file mentions it.
- [V] (r2) bench.rs `every_program_in_bench_has_a_test` filters entries by the `.em` extension, so `bench/async/` is ignored.
- [V] (r2) S2-response-r2.md §5.6–5.7: the `chan_pingpong` output `800040000`, TSan `--lib counters::`, Miri `counters::`, and the 13 channel/counter UI files at U7.
- No background task is left running.

## 8. Conformance with S1

- **G-A5:** every unit runs `cargo insta test --check` and diffs snapshots and bench.rs against 2c8de78.
- **G-A7:** every `run/async` N ≥ 2 run asserts `workers ≥ 1 && placed_tasks > 0`. `placed_tasks` is pinned as "owner ≠ starting worker" (§5.1). The expected owners in `placement_rotates_over_every_worker` become 1,2,0,… (S1 amendment).
- **G-A10:** `io::` runs under TSan and ASan from U6a. `a_forwarded_submit_is_settled_by_its_host` is in io.rs.
- **G-A11:** the U6b gate is numeric on `serve-self-32` and `echo-self-fresh`. The echo test is in io.rs.
- **G-A12:** deadlock UI files run at N = 2/8: `drain_own_scope` (U5), the `net_*` deadlocks (U6a), `chan_deadlock` (U7), `signal_wait_deadlock` (U8).
- **G-A13:** `signal_*` UI at N ≥ 2 (U8). `signal::` is under TSan and ASan. The `cli.rs` 130/143 tests are pinned to N = 1, and the N ≥ 2 signal paths are covered in `signal.rs`.
- **G-A14:** `exit_in_task.em` at N ≥ 2 from U5. With start = 1, its task runs on worker 1.
- **G-A15:** the two end-of-run tests are placed in io.rs and workers.rs.
- **G-A17 (S2):** `chan::` and `counters::` run under TSan, `counters::` under Miri, and the channel UI files at U7.
- **G-A18:** TSan covers `workers::` (Mail-drain race, fast mode N = 4), `signal::`, `io::`, `chan::` and `counters::`. `async/` runs at N = 2/8 per `WORKERS_PENDING`, minus marks made on observed diffs. `RunStats` for `async/` is compared only at N = 1. The timing procedure runs at every enabling unit. The rule is pinned by `workers_runs_cover_par_and_async`.
- **G-A20:** at close, the roadmap prerequisites become なし. The amendment citations are S1's.

