# Stage task-limits: memory has one limit; the limit classes are `Stops`, `Task`, `Aborting`

- Created: 2026-11-17 (design-level plan); refreshed into this executable plan 2026-11-20 (r2, after the review r1; the response table is Appendix A).
- Base: main @ afcde66f (the merge of bb9d27a1, no other change) plus the ruling commit 94d34252 (`Rule on the pending items before task-limits`). bb9d27a1 (`Fix what the review of the runtime docs found`; its commits d19db81f, 833ec835, bb9d27a1 sit on main @ 3aba15e9 and change only docs/future/{multicore,native,stdlib,verification,async,roadmap}.md and docs/plans) is already merged. Code facts are those of 3aba15e9 (no code changed) [V: `git diff --stat 3aba15e9 bb9d27a1 -- crates std tests bench` empty]. Line numbers are hints; docs edits are addressed by quoted anchors (section 6), code edits by item names.
- Status: executable plan. User decisions of 2026-11-20 it implements: O-1 (a), the `--allow` of 10.2 for TL-3b; U-1 (`par-direct` after `sched-proofs`; 00 §D D-12), U-2 (docs/future runtime design rewritten in the base; this plan owns every soft-limit sentence), U-3 (the VM gate, 10.1); X2 ruling of 2026-11-20 (00 §D D-14). Approving this plan is the agreement for the test changes of 7.4 and for 00 §D D-13 (TL-1). The design is 01 §7 (D-TL-1..D-TL-4) with 01 §3 SC-8, §14.2, §18 G-4, read through 00 §D D-5, D-10, D-11, D-12.
- Design documents (`docs/plans/2026-11-17-runtime/`): `00-decisions.md` wins over everything; `01-design.md`. This file stands alone.
- Language: this plan is English. `docs/`, code comments and CLAUDE.md are Japanese: every text marked "EN" is an English draft the implementer translates with PLaMo (`form: plain`, identifiers and section names in `glossary`, earlier parts of the same document as `history`), reviews, and writes with no space between Japanese and Latin characters or inline code.
- Tags: [V: proto] checked on a prototype of TL-2 and TL-3 in a scratch worktree at 3aba15e9 (clippy, fmt and `cargo nextest run` after each unit); [V: read] read in the source; [V: grep] the command is shown; [V: measured] measured on macOS arm64, release; [I] inferred. Prototype patches, kept in the appendix directory `docs/plans/2026-11-17-task-limits/` (committed by TL-1 with this file, deleted at TL-5; each applies to 3aba15e9 with `git apply --check`): `tl-proto.patch` (TL-2 + TL-3 against 3aba15e9, plain `git diff --no-ext-diff`, 32 files, no docs), `tl-3a-rt.patch` (TL-3a's `control.rs`), `tl-3b-work-first.patch` (the `Heap` field order of S-TL-12).

## 1. Goal and success criteria

- Goal: `max_bytes` is the only memory limit and ends the run. The soft memory limit, the monitor's pressure role, the per-task byte counts, the per-worker byte counts and their slot words, and the limit classes `Pending` and `Aborting { pending }` leave `eml_vm`, `eml_sched`, `eml_rt` and `eml_cli`. The depth limit of a task stays a catchable `ResourceLimit`; its behaviour is unchanged.
- Success criteria:
  - S-1 (G-4 as restated by 00 §D D-5 and D-13; code): SC-2's three commands give their expected results on TL-3b and later. Every hit of `P` at the base is a soft-limit item (section 9, C-5).
  - S-2 (D-TL-1): `sed -n '/^pub(crate) enum Limit {/,/^}/p' crates/eml_vm/src/vm.rs | grep -cE '^    (Stops|Task|Aborting),$'` → `3` and the enum has no other variant; `limit_hit` has the eight steps of D-TL-1; S-14's VM half holds (steps 1 and 2 come first; `vm::tests::a_depth_hit_while_aborting_ends_the_run` passes).
  - S-3 (SC-8): `tests/ui/run-fail/async/limit_memory_ends_the_run.em` passes in checked and fast mode, under both seeds and at 2 and 8 workers (all part of `ui::run_fail`); its runtime error is ``the memory limit is exceeded in `build` `` [V: proto, `ui::run_fail` green].
  - S-4: programs without `Async` and `par` keep their LIR and `RunStats`: SC-6 [V: proto].
  - S-5 (the `eml_rt` gate, 10.2): `bench/rt-gate.sh <TL-2>` exits 0 on TL-3a; on TL-3b, `bench/rt-gate.sh <TL-3a>` exits 0, or exits 0 with the `--allow` rows and bound the user approved in O-1 (the Close message quotes the approval); `cargo verus verify -p eml_rt` → `1265 verified, 0 errors` on TL-3b (1270 at the base) [V: proto and on the O-1 candidate]; `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` exits 0.
  - S-6 (the VM gate, U-3): no row stops the stage under 10.1 on TL-3b, or the user decided O-2 for each row that does (the Close message carries the tables and the decision).
  - S-7 (C-4): SC-4 after TL-5 hits exactly one line, `docs/implementation/testing.md` 「ハーネスは上限の印を2つとも」 (both marks still exist).
  - S-8: CI `success` on every job for the Close commit.

## 2. Scope

- In: D-TL-1..D-TL-3 in `eml_vm` (`vm.rs`, `sched.rs`, `workers.rs`, `monitor.rs`, `lib.rs`), `eml_sched/src/cells.rs`, `eml_cli` (`lib.rs` and the UI harness), `eml_rt` (`control.rs`, `alloc.rs`, `heap/mod.rs`, `heap/ctx.rs`), one test literal in `eml_rows/src/rows.rs`; the UI files of 7.4; the docs of section 8.
- Out (owner): the native limit decision `eml_steps::limit::hit` (`sched-core`, `jit-tasks`); the monitor's other roles and the depth deferral (kept, D-TL-3); the native treatment of the remaining `-- max-bytes:` file (it uses task rows, which native code rejects until `jit-tasks`).
- No new crate edge, feature, cross-crate re-export, `RuntimeEntry`, `unsafe` block or `unsafe fn`. `crates/eml_cli/tests/surface.rs` does not change [V: proto, `surface::` green]. `bash nix/crate-graph.sh --check` exits 0 [V: proto].

## 3. Background context (code at 3aba15e9 = base)

Today [V: read]:
- `eml_rt/src/control.rs`: `Limits.soft_bytes` (default `usize::MAX`), `Control.taken: Vec<AtomicUsize>` (one counter per stop word, `byte_counters`) and `Control.next` (the soft threshold); `charge(slot, bytes)` adds to `bytes` and `taken[slot]`, raises `MEMORY` over `max_bytes`, else raises `NOTICE` on the charging worker's word when the total crosses `next`; `taken(k)`, `rearm`, `MIN_GAP`, `pressure`, `claim_pressure`; `clear` re-arms `next`. `Control::MEMORY` is the hard-limit stop bit and stays.
- `eml_rt/src/alloc.rs`: `Pages.slot`, set by `Pages::new(prefix, control, slot)`, read by `Pages::slot()` and by `charge_pages(&self.control, self.slot, bytes)` (T1 `external_body`). `Heap::for_worker(control, slot)` (`heap/mod.rs`) has three callers: `Ctx::for_worker` (`heap/ctx.rs`), the heap copy in `heap/mod.rs` and `empty_core` [V: grep `for_worker(`]. `Ctx::for_worker(control, host, k)` keeps `k`: it selects the stop word.
- Readers of `Control::taken`: `eml_vm/src/monitor.rs` (`choose`, `pressure_tick`), `eml_vm/src/vm.rs` (`sched.mark`), `eml_vm/src/sched.rs` (`end_slice`, `published_count`); tests in `eml_rt` (`heap/tests/control.rs`, `heap/tests/trusted.rs`), `eml_native/tests/machine.rs`, the VM unit tests. No reader outside the soft limit [V: grep `\.taken\(`].
- Readers of the slot words `Slot.{seq, current, bytes, mark, top, top_bytes}` (`workers.rs`): only `monitor::choose` and tests; writers `Scheduler::write_published`, `Run::{offer_top, clear_top, drop_top}`. `Scheduler::current` (a `TaskId`) is not a slot word and stays [V: grep].
- The VM: `enum Limit { Stops, Task, Pending, Aborting { pending: bool } }`, `Limit::flagged`, `mark_pending`, `mark_limited`, `landed`, `limit_hit` (step 6 chooses the soft message, step 8 calls `clear_limit` and resets the counts, step 9 returns to `Pending`), `abort_to`'s `Pending` bracket, `Task.counted` / `.added`, `Scheduler.{limited, added, mark, yielded, published}` and their functions; `Policy::MEMORY` with `enum Memory { Watched, Gathered }` (re-exported as `eml_vm::Memory`, no user outside `eml_vm` [V: grep]); `gather` calls `monitor::pressure_tick` for `Gathered` and `Registry::add_bytes`; `schedule` starts the monitor at the first `Async` task when `MEMORY == Watched`; `Notice::Limit`, `Mailer::limit`.
- The monitor (`monitor.rs`): `watch_loop` relays signals and calls `wake_all` when a stop bit is up, after every wake. With a doorbell it waits on it with `bell_timeout(pressure, running, until_next)`, which is `Some(tick)` only when `pressure` (policy `Watched`, i.e. `Normal`) and `running > 0`; otherwise it waits until rung. So at the base the monitor with a doorbell already never ticks under `Seeded` and `Sequential`, nor under `Normal` while no worker runs; the tick exists only for `pressure_tick`. `Run::unpark_locked` rings the doorbell when `running` goes from 0 to 1 only to restart that tick. Without a doorbell the monitor ticks every 10 ms whatever the policy [V: read `monitor.rs:174-215`].
- Who raises stop bits, and who wakes parked workers for them [V: read]: `MEMORY` is raised by `Control::charge`, which runs on the worker taking a page; that worker is running and reads its stop word at its next safepoint, ends the run, and every worker exit path calls `Run::wake_all` (`workers.rs`, the exit paths after `set_fatal` / `end_run`, and `Shutdown::drop`). `INTERRUPT` / `TERMINATE` come from the signal handlers and the GUI close request (`eml_cli/src/main.rs` `gui_park`), which ring the doorbell; `END` is raised by those exit paths, which call `wake_all` themselves.
- `eml_sched/src/cells.rs`: `TaskCell.limit` / `.bytes`, `TaskMarks`, `Registry::{started, resumed}` return marks, `clear_limit`, `limit_task`, `limit_forwarded`, `add_bytes`, `MailItem::Limit`, `Post::limit` (written `fn limit(&self` in the trait), `receive_mail` and the `pub unsafe fn drain` take `limited`; testing helpers `limit_task` and `limit_flag`.
- `eml_cli/src/lib.rs`: `soft_of` sets `soft_bytes` in `default_limits` and `repl_limits`. `eml_rows/src/rows.rs` `rows_that_do_not_fault_return_ok_on_an_exhausted_budget` writes `max_bytes: 0, soft_bytes: 0`.
- UI: five files carry `-- max-bytes: 268435456 soft 4194304`, all soft-limit tests: `run/async/limit_memory_after_catch.em`, `run/async/limit_memory_caught.em`, `run/async/limit_memory_fails_scope.em`, `run-fail/async/limit_memory_uncaught.em`, `run/limits/soft_limit_without_a_task.em` (the only file of `limits/`); their snapshots are `crates/eml_cli/tests/snapshots/integration__ui__run@async__limit_memory_{after_catch,caught,fails_scope}.em.snap`, `integration__ui__run@limits__soft_limit_without_a_task.em.snap`, `integration__ui__run_fail@async__limit_memory_uncaught.em.snap` [V: ls]. The harness reads the mark in `crates/eml_cli/tests/common/mod.rs` (`max_bytes_mark`) and applies it in `ui.rs` (`execute_in`).
- `limit::every_probe` does not exist (`grep -rn every_probe crates` empty) [V: grep]; D-12 records it.

What the prototype showed:
- The whole deletion compiles and the suite stays green with the test changes of 7.4: `cargo nextest run` 5272 → 5237 (TL-2) → 5226 (TL-3), 23 skipped throughout; clippy, `cargo fmt --check`, `nix/crate-graph.sh --check`, the feature clippies of `eml_sched` (no `testing`), `eml_io eml_rows eml_vm eml_nrt --no-default-features --all-targets` and `eml_cli --no-default-features --features types` pass [V: proto].
- No UI snapshot, `bench.rs` `RunStats`, Core IR / LIR dump or REPL transcript changes, except `integration__ui__run_native.snap` (`accepted 492 → 491`, `rejected 145 → 142`, `async row 126 → 123`) [V: proto]. `run_fail_native.snap` is unchanged.
- The depth limit's observable behaviour is unchanged: every `run*/async/limit_depth_*` snapshot passes untouched [V: proto].
- A hard-limit UI file whose allocation grows in a started task reports the function where worker 0 sees the stop bit, which differs at 2 workers (`Std.Async.start_task$handle0$await_cell` vs `build`); with the growth in the root task the report is `build` at every mode, seed and worker count [V: proto].
- After the deletion, `git grep -nwiE soft -- crates std tests bench ':!crates/eml_gui/src/winui/bindings.rs' | grep -vi microsoft` has one hit, the malformed-mark literal `"-- max-bytes: 100 soft 50"` in `crates/eml_cli/tests/ui.rs` [V: proto].
- `eml_rt` gate measurements (`bench/rt-gate.sh`, 5 pairs) and the cause of the TL-3b regression: section 10.2.
- The VM gate on the prototype (TL-2 + TL-3, without S-TL-12, against 3aba15e9): two alternating pairs of `bench/run.sh` put 10 rows over +0.5 % of 実行の分; 15 alternating single-row rounds (10.1 step 3) of `state`, `par_tree`, `fib`, `fold_state`, `sieve_by_hand`, `state_loop`, `loop` gave +0.043, +0.285, -0.020, +0.279, +0.051, -0.081, +0.036 % [V: measured]. The pair tables move with the `empty` row (head 0.2-0.4 M instructions lower: the monitor thread no longer ticks, and `/usr/bin/time -l` counts every thread), which 実行の分 subtracts once per table; the rounds measure `empty` next to the row.

Baseline at 3aba15e9: `cargo nextest run` → `5272 tests run: 5272 passed, 23 skipped` [V].

## 4. Prerequisites

| Needed | For | How to check | If missing |
|---|---|---|---|
| The dev shell (Rust 1.99.1, nextest, clippy, insta, Verus, verusfmt) | every unit | `cargo --version` → `cargo 1.99.1`; `cargo nextest --version`, `cargo verus --version`, `verusfmt --version` exit 0 | stop |
| macOS | `bench/rt-gate.sh` (TL-3a, TL-3b), the gate of 10.1, the Close measurements | `uname` → `Darwin` | gates: stop for the user; Close: `Measure task-limits` right after |
| The `.#cross` and `.#miri` shells | checkpoint extras | `nix develop .#cross -c true`, `nix develop .#miri -c true` exit 0 | stop before CP-A |
| `plamo-translate` (MCP tool or CLI) | Japanese text | tool listed or `plamo-translate --help` exit 0 | the `natural-japanese` skill (CLAUDE.md) |
| `gh` authenticated with push | checkpoints | `gh auth status` exit 0 | ask the user |
| The base merged | TL-1 | `git merge-base --is-ancestor bb9d27a1 HEAD` exit 0; `git diff --stat 3aba15e9 HEAD -- crates std tests bench` empty; `grep -c '^- D-12 ' docs/plans/2026-11-17-runtime/00-decisions.md` → `1`; `grep -c '^- D-14 ' docs/plans/2026-11-17-runtime/00-decisions.md` → `1`; `grep -c '^- D-13 ' docs/plans/2026-11-17-runtime/00-decisions.md` → `0` | stop |

A unit that finds it must add an `unsafe` block or `unsafe fn`, change `surface.rs`, or change `crates/eml_rt` beyond TL-3a / TL-3b stops for the user.

## 5. Decisions

### 5.1 Long-term guidelines

| ID | Decision | Source |
|---|---|---|
| D-TL-1 | `Aborting` has no field. `limit_hit`'s steps: 1 `Stops` or `Aborting` → stop the run; 2 yielding → continue; 3 not an entry row → continue; 4 over `real + LIMIT_HEADROOM` → stop; 5 `Defers` → continue; 6 class `Aborting`, limit `real + LIMIT_HEADROOM`; 7 `abort_to` `io_failed` with `ResourceLimit` and `the call depth limit is exceeded`; 8 no `IO` evidence: class `Task`, limit `real`, stop the run | 01 §7, confirmed [V: proto] |
| D-TL-2 | Deleted: 01 §7's list, read with S-TL-1..S-TL-6 | 01 §7; D-13 |
| D-TL-3 | Kept: the monitor thread (signal relay, `wake_all` on stop bits), `max_bytes`, `Control::MEMORY`, the depth deferral, `LIMIT_HEADROOM` | 01 §7, confirmed |
| D-TL-4 | Gates: correctness; the VM gate of 10.1 (U-3) on TL-3b; the `eml_rt` gate on TL-3a and TL-3b (10.2) | 01 §7; U-3 |

Conformance criteria (every unit's review checks those that apply):

| ID | Rule | Why | Applies | How to check |
|---|---|---|---|---|
| G-4 | MUST: the limit classes are `Stops`, `Task`, `Aborting`; the monitor has no memory role; no soft-limit item exists, in code, test names or docs | SC-8; D-5 | TL-3b on (code), TL-5 (docs) | SC-2, SC-4; S-2 |
| S-14 (VM half) | MUST: `limit_hit` starts no unwinding action while yielding or `Aborting` | 01 §16 | TL-2 on | step order of D-TL-1 (read); `vm::tests::a_depth_hit_while_aborting_ends_the_run` |
| C-4 | MUST: the docs units list every document that states the soft limit or the `Pending` class, from a grep over `docs/` | X1 C-4 | TL-1, TL-2, TL-3a, TL-3b, TL-4, TL-5 | SC-4 counts (section 9) |
| C-5 | MUST: completion greps cover `crates/*/tests` and `tests/`, and each pattern hits only soft-limit items at the base | X1 C-5 | TL-1 | section 9's G-4 row |
| C-6 | MUST: every test this plan names exists at the base | X1 C-6 | TL-1 | `cargo nextest list` contains every name 7.4 deletes or renames |
| C-7 | MUST: the roadmap items whose 見直す条件 depends on the soft limit or this stage are listed | X1 C-7 | TL-1, TL-5 | 7.2 TL-5 step 5 |
| C-9 | MUST: the VM gate follows U-3; no `--allow`, exemption or threshold change without the user | X1 C-9 | CP-A, TL-3b | 10.1, 10.2; the Close message carries the tables |
| TL-G1 | MUST NOT: a new `unsafe` block, `unsafe fn`, crate edge, feature or re-export; a change of `surface.rs` or `nix/crate-graph.sh` | S-26 | all | SC-5 |
| TL-G2 | MUST: a program without `Async` and `par` keeps its LIR and its `RunStats` | the stage removes only task and limit paths | all | SC-6 |
| TL-G3 | MUST: a kind 1 or kind 2 test change found during execution that 7.4 does not list stops the unit; the orchestrator amends this plan with the user's approval | testing.md 「テストの変更の運用」 | all | review of each unit's diff against 7.4 |

### 5.2 Short-term decisions (this plan)

- S-TL-1 (`Control::taken` and the slot plumbing): `taken`, `byte_counters`, `charge`'s `slot` parameter and its `NOTICE`, `Pages.slot`, `Pages::slot()` and `Heap::for_worker` go; `charge(bytes)` adds to `bytes` and raises `MEMORY` over `max_bytes`; `Ctx::for_worker` and the two internal callers use `Heap::with_control`, and `with_slot` merges into `with_control`. Doc comment of `charge` (EN): "Adds the bytes of a page taken to the total; raises `MEMORY` when the total exceeds the limit (docs/spec/object-model.md 「実行の制御」)." TL-3a removes the threshold, TL-3b the counts and the slot (10.2 explains the split).
- S-TL-2 (slot words): `Slot.{seq, current, bytes, mark, top, top_bytes}`, `NO_CELL`, `cell_word`, `Run::{offer_top, clear_top, drop_top}`, `Scheduler.{limited, added, mark, yielded, published}`, `Scheduler::{end_slice, publish, unpublish, write_published, keep_running, published_count, offer_top, clear_top, left_entry, publish_idle}` go, with their calls in `resume`, `switch_away`, `enter`, `entry_finish`, `schedule`, the `par` wait and the root wait.
- S-TL-3 (the monitor): `choose`, `pressure_tick`, `bell_timeout`, the `pressure` argument of `Monitor::start` and the tick bookkeeping go. The monitor with a doorbell waits until rung, as it does at the base under `Seeded` and `Sequential` and under `Normal` while no worker runs (section 3); without a doorbell it still ticks every 10 ms. No stop bit needs the tick: `MEMORY` is raised on a running worker, which reaches its safepoint, ends the run and wakes all workers on exit; `INTERRUPT` / `TERMINATE` sources ring the doorbell; `END` is raised by paths that call `wake_all` (section 3). A worker inside a long extern or a `blocking` C call sees a bit when it returns, as at the base under `Seeded` and `Sequential` [I]. `watch_loop` is (code):
  ```rust
  fn watch_loop(relay: &Relay, watch: &Watch) {
      loop {
          // EN: without a self-pipe, look at the signals and the stop bits every tick: only the monitor can wake a sleeping worker
          match relay.run.bell.as_deref() {
              Some(doorbell) => {
                  doorbell.wait(None);
              }
              None => thread::park_timeout(TICK),
          }
          if watch.stop.load(Ordering::Relaxed) {
              return;
          }
          relay.relay();
      }
  }
  ```
  `Run::unpark_locked` no longer rings the doorbell when `running` goes from 0 to 1; `Run.bell` stays; its doc (EN): "The monitor's self-pipe. The monitor sleeps on it; the signal handlers, the GUI close request and the side that stops the monitor ring it." `start_monitor` is called only by `begin_run`, so a virtual-clock run with one worker never starts the monitor, whatever its policy [V: proto, suite green].
- S-TL-4 (`limit_hit`): loses its `Scheduler` parameter (`limit_hit::<R>(code, ctx, task)`); `landed` turns `Aborting` into `Task` with limit `real`; `abort_to` no longer changes the class; `apply_limit` has no `Pending` arm.
- S-TL-5 (cells API): `Registry::started` → `Result<u32, CellFault>`, `Registry::resumed` → `Result<i64, CellFault>`; `TaskMarks`, `clear_limit`, `limit_task`, `limit_forwarded`, `add_bytes`, `MailItem::Limit`, `Post::limit` and the testing helpers `limit_task`, `limit_flag` go; `Registry::receive_mail` and `pub unsafe fn drain` lose the `limited` parameter (`drain`'s `# Safety` is unchanged). `MailItem::for_worker`'s arm becomes `MailItem::HandleDropped(_) => true`.
- S-TL-6: `eml_vm`'s `pub use sched::Memory` goes with `Memory`.
- S-TL-7 (the UI mark): `-- max-bytes: <bytes>`, `bytes` a positive decimal number, one space after the colon, nothing after it; a line starting with `-- max-bytes:` in any other form panics with ``the max-bytes mark is `-- max-bytes: <bytes>`, not `<line>` ``. `Marks.max_bytes: Option<usize>`; `execute_in` sets `max_bytes: marks.max_bytes.unwrap_or(defaults.max_bytes)`. The parser (code):
  ```rust
  /// EN: Reads the mark `<leader> max-bytes: <bytes>`. `bytes` is only positive decimal digits, after one space.
  /// EN: A line starting with `<leader> max-bytes:` that is not of this form is an error, so a misspelled mark does not fall back to the default limit silently.
  pub fn max_bytes_mark(line: &str, leader: &str) -> Result<usize, String> {
      line.strip_prefix(leader)
          .and_then(|rest| rest.strip_prefix(" max-bytes: "))
          .filter(|bytes| bytes.bytes().all(|b| b.is_ascii_digit()))
          .and_then(|bytes| bytes.parse().ok())
          .filter(|&bytes| bytes > 0)
          .ok_or_else(|| format!("the max-bytes mark is `{leader} max-bytes: <bytes>`, not `{line}`"))
  }
  ```
- S-TL-8 (the five soft-limit UI files): deleted by name with their five snapshots (section 3); the directory `tests/ui/run/limits/` goes with its only file. One new file pins SC-8 in `async/` (so it runs at both seeds and at 2 and 8 workers; native code rejects it by its task rows). `tests/ui/run-fail/async/limit_memory_ends_the_run.em` (comment line EN: "The memory limit ends the run even inside a task; `try_io` does not catch it (docs/spec/vm.md 「上限と割り込み」)."):
  ```haskell
  -- max-bytes: 16777216
  -- EN: (the comment above)
  import Async (Async)

  build : Int -> List Int -> <Panic> List Int
  build n acc = if n == 0 then acc else build (n - 1) (n :: acc)

  grow : List Int -> <Panic> Int
  grow acc = grow (build 1000 acc)

  main : Unit -> <IO, Panic, Async> Unit
  main () =
    println "before"
    match try_io (fn () -> grow []) with
      | Ok n -> println "unreachable \{show n}"
      | Err e -> println "unreachable: \{e}"
  ```
  Snapshot (accept after reading): stdout `before`, runtime error ``the memory limit is exceeded in `build` `` [V: proto]. The growth stays in the root task (section 3).
- S-TL-9 (`eml_cli`): `soft_of` goes; `default_limits` sets `max_depth` and `max_bytes`; `repl_limits` is `Limits { max_bytes: physical_memory().map_or(usize::MAX, |bytes| bytes / 5 * 2), ..default_limits() }`; the doc sentence 「柔らかい上限は確保量の上限の 7/8 である」 goes. Between TL-2 and TL-3a `Limits.soft_bytes` still exists and is `usize::MAX` through `..Limits::default()`.
- S-TL-10 (roadmap item on the `-- max-bytes:` files): deleted at `Close task-limits`, not at `vm-retire`. 00 §A and §B are append-only, so D-13 records that it supersedes A-21 and the `vm-retire` plan on this item; neither text is edited (`vm-retire` rewrites its plan at its own Start, roadmap 「進め方」).
- S-TL-11 (00 §D D-13, appended by TL-1; EN):
  > D-13 (2026-11-20; the refresh of `task-limits`): 01 §7 is confirmed on a prototype of the whole stage at main @ 3aba15e9. Beyond D-TL-2's list the stage deletes, because their only readers were the soft limit: `Control::taken` with the per-worker counters, `charge`'s `slot` and its `NOTICE`, `Pages.slot` and `Heap::for_worker`; the slot words `seq`, `current`, `bytes`, `mark`, `top`, `top_bytes` with `NO_CELL` and `cell_word`; `Scheduler.{limited, added, mark, yielded, published}`; the monitor's tick with a self-pipe and the doorbell ring when `running` goes from 0 to 1; the marks returned by `Registry::{started, resumed}`; the `limited` parameter of `Registry::receive_mail` and `cells::drain`; `limit_hit`'s `Scheduler` parameter; `eml_vm::Memory`. `Heap`'s field `work` moves first, with a compile-time check of its offset on aarch64 (a layout fix the `eml_rt` gate needs; the plan's S-TL-12). The UI mark is `-- max-bytes: <bytes>`; the five soft-limit UI files and the category `limits/` are deleted, and `run-fail/async/limit_memory_ends_the_run` pins that the hard limit ends the run inside a task under `try_io`. G-4 (D-5) is the plan's S-1 with SC-2. Amends §B A-21 and the `vm-retire` plan's §8: the roadmap item on the `-- max-bytes:` files is deleted at `Close task-limits`, not at `vm-retire`. The `eml_rt` part is two commits: the soft threshold, then the per-worker counts. The VM gate is U-3 of 2026-11-20 with the path rule of the plan's 10.1.
- S-TL-12 (`Heap` field order; TL-3b): the field `work: Vec<Word>` (with its doc) moves to be the first field of `pub struct Heap<M: Mode>` (`docs/plans/2026-11-17-task-limits/tl-3b-work-first.patch`). Comment (EN): "`work` comes first: `release_children` swaps it with 16-byte loads, and at an offset that is not a multiple of 16 the arm64 load cannot take the offset, which costs a register and a saved register pair per call (docs/implementation/benchmarks.md 「`eml_rt` のインライン化」)." benchmarks.md 「`eml_rt` のインライン化」 gains a bullet under 主張 (EN): "`Heap`'s field `work` is placed first. `release_children` swaps it with a pair of 16-byte loads and stores; when the field's offset is not a multiple of 16, arm64 materialises the address in a register, and `release_children` saves one more register pair on every call" and under 数値 (EN): "Removing an 8-byte field before `work` moved it from offset 0x3c0 to 0x3b8: `map` +1.1 %, `map-reuse` +1.5 % (two and one extra instructions per element). With `work` first, the same tree measured `map` -1.1 %, `map-reuse` -3.0 % against the tree before the removal. M-series Mac, 2026-11-20." Measured basis: 10.2. After the last `} // verus!` of `crates/eml_rt/src/heap/mod.rs` add

```rust
// EN: Rust does not fix the field order of `Heap`, so the place of `work` that its comment relies on is checked here (docs/implementation/benchmarks.md 「`eml_rt` のインライン化」)
#[cfg(target_arch = "aarch64")]
const _: () = assert!(std::mem::offset_of!(Heap<crate::Fast>, work).is_multiple_of(16));
```

(`std::mem`, not `core::mem`: `heap` has a module named `core`.) If the assertion fails on a toolchain or target, the unit stops.

## 6. Implementation constraints and conventions

- One unit = one commit; each passes G (7.3.1) and the SC lines in force (7.3.2). `cargo fmt` before every commit; `verusfmt --verus-only` on the `eml_rt` files a unit changes.
- Deletions are compile-driven: delete the named item, then fix every error and every `-D warnings` lint by deleting the dead arm, import, field, helper or test helper. A compile error whose fix is not a deletion and is not listed in the unit stops the unit. "Delete test `x`" means the `#[test]` function with its doc comment (for `both_modes!` tests also the list entry).
- `docs/spec/` text is written in the commit that implements the rule (each unit's "Docs" part); TL-4 rewrites docs/implementation/status.md, docs/overview.md and docs/future. Docs edits are addressed by anchors: an anchor is a quoted substring that `git grep -cF '<anchor>' bb9d27a1 -- <file>` counts once [V: every anchor of section 8 checked on bb9d27a1]. The edit applies to the sentence, bullet or table row that contains the anchor. A hit of SC-4 that no item covers stops the unit. Code comments cite `docs/spec/` and `docs/implementation/` only; docs name no stage. After every unit `cargo nextest run -p eml_cli --test integration -E 'test(/^citations::/)'` passes (part of G).
- Comments that state the removed mechanism are rewritten in the unit that removes it. The prototype left these for the implementer [V: proto grep]: in `vm.rs` the docs of `limit_hit`, `limit_safepoint`, `landed`, `collect` (「受けた上限の旗を VM の `Task` に付ける」), `enter` (「旗が立っていれば、保留にする」), the comment after `idle_wait`, the doc of `Limit::Task`; `lib.rs` the comment before `main_ctx` (EN for its last sentence: "so that the safepoint at which a deterministic run reaches the memory limit does not depend on the speed of threads"); `eml_rt/src/control.rs` the comment before `impl Control` (EN: "The stop words carry only the bits that say stop, and the byte count carries only its value. ..."). The word 保留 stays only where it means the depth deferral (`code.rs` `Defers`) or pending signals; SC-2's third command checks the rest.
- Snapshots are accepted with `cargo insta review` after reading each diff against 7.4; never `cargo insta accept` over the whole tree.
- Quote every glob (zsh). Run ad-hoc `.em` programs under `timeout 20`; never run the new UI file with `eml run` (without the mark its limit is 80 % of the machine's memory). Stop any Bash call moved to the background before finishing a unit.

## 7. Implementation tasks

### 7.1 Task table

| ID | Commit title | After | Decisions / criteria | Executor |
|---|---|---|---|---|
| TL-1 | `Start task-limits` | - | this plan, 00 §D D-13, baseline | orchestrator |
| TL-2 | `Remove the soft memory limit from the VM and the task cells` | TL-1 | D-TL-1, D-TL-2, S-TL-2..S-TL-9; G-4 (VM half), S-14 | Sonnet from `docs/plans/2026-11-17-task-limits/tl-proto.patch` and the lists below; Opus reviews the diff against 7.5 before the commit |
| TL-3a | `Remove the soft threshold from eml_rt` | TL-2 | S-TL-1 (threshold); 10.2 | Sonnet from `docs/plans/2026-11-17-task-limits/tl-3a-rt.patch`; gate passes [V: measured] |
| TL-3b | `Remove the per-worker byte counts from eml_rt` | TL-3a | S-TL-1 (counts), S-TL-12; O-1; 10.2 | Sonnet; the gate decides whether it lands (10.2) |
| CP-A | push, CI of TL-3b, checkpoint extras, the gate of 10.1 | TL-3b | S-5, S-6 | orchestrator |
| TL-4 | `Rewrite the docs without the soft memory limit` | CP-A | SC-4 | Sonnet: prose rewrite from drafts and anchors |
| TL-5 | `Close task-limits` | TL-4 | roadmap 「進め方」 | Sonnet: steps listed |
| CP-Z | CI of TL-5, then merge | TL-5 | S-8 | orchestrator |

No soundness question is expected (no `unsafe` is added or moved; 7.5); one that appears goes to Fable (CLAUDE.md).

### 7.2 Units

"N tests" is the expected `cargo nextest run` total, all passing, 23 skipped.

#### TL-1: `Start task-limits`

1. On main with the base merged: `git switch -c plan/task-limits`.
2. Replace `docs/plans/2026-11-17-task-limits.md` with sections 1-10 of this plan (Appendix A stays out), and add its appendix directory `docs/plans/2026-11-17-task-limits/` (the three prototype patches).
3. Append S-TL-11's D-13 to `docs/plans/2026-11-17-runtime/00-decisions.md` §D, after D-14.
4. Baseline: section 9's table on the clean tree; fill "Recorded". A count that differs from section 9 is corrected in the plan before the commit if the difference is explained by a docs commit merged after bb9d27a1; otherwise stop.
5. Commit; message: D-13 and the baseline.
- Completion: `git diff --name-only HEAD~1 HEAD` → the two plan files and the three patches; section 9 has no empty "Recorded" cell; N = 5272.

#### TL-2: `Remove the soft memory limit from the VM and the task cells`

Work (the prototype patch's hunks for these files are the reference; take them, then fix the comments of section 6):
- `crates/eml_vm/src/vm.rs`: `Task.counted` / `.added`; `enum Limit` becomes
  ```rust
  pub(crate) enum Limit {
      /// EN: outside a task, a `compute` job, every `par` child. The limit is the real one; going over it ends the run.
      Stops,
      /// EN: inside an `Async` task. The limit is the real one.
      Task,
      /// EN: from the start of an abort until it lands. The limit is the real one plus `LIMIT_HEADROOM`; going over it ends the run.
      Aborting,
  }
  ```
  `Limit::flagged`, `mark_pending`, `mark_limited` go; `limit_hit` as D-TL-1 and S-TL-4 (after step 5: `task.limit = Limit::Aborting; ctx.set_depth_limit(real.saturating_add(LIMIT_HEADROOM));`, `abort_to` with `ctx.fresh_string("the call depth limit is exceeded")`, then `if !caught { task.limit = Limit::Task; ctx.set_depth_limit(real); return Err(RunFault::Stopped(Stop::DepthLimit)); } Ok(())`); `apply_limit` without `Pending`; `landed` (`if task.limit == Limit::Aborting { task.limit = Limit::Task; ctx.set_depth_limit(real_limit(ctx, task.depth_offset)); }`); `abort_to` without the `Pending` bracket; `collect` without `mark_limited`; `enter` without marks (`let spawner = sched.cells.lock().started(cell)?;`, `let wake = ...resumed(...)?;`) and without the count, publish and `clear_top` lines; `entry_finish`, `resume`, `switch_away`, `schedule`, the `par` wait and the root wait without the slot-word calls of S-TL-2; `schedule` without the second monitor trigger; the end-of-run `drain` call without `&mut Vec::new()`; `Limit::Aborting { .. }` → `Limit::Aborting`; the unit tests of 7.4.
- `crates/eml_vm/src/sched.rs`: `Memory`, `Policy::MEMORY` and its three impls; `take_async` without `limited` and the `Notice::Limit` arm; the `Scheduler` fields and functions of S-TL-2; `gather` without `pressure_tick`, `add_bytes` and `limited`; `start_monitor` without the `pressure` argument and with the doc (EN): "Starts the monitor once. Called by `begin_run`; a run it does not start the monitor for creates no thread (docs/spec/vm.md 「タスクとスケジュール」)."; the test `only_normal_watches_memory`; imports.
- `crates/eml_vm/src/workers.rs`: `Notice::Limit`, `Mailer::limit`, the `Notice::Limit` arm of `left_notices`, the slot words and their initialisers, `NO_CELL`, `cell_word`, `offer_top`, `clear_top`, `drop_top`, the ring in `unpark_locked`, `Run.bell`'s doc (S-TL-3), the `debug_assert!` on `sched.added` in the worker loop; the test `a_notice_to_the_parked_main_worker_rings_its_park_waker` sends `Notice::Cancel(CellRef::decode(0))` instead of `Notice::Limit(..)` (kind 3).
- `crates/eml_vm/src/monitor.rs`: S-TL-3; the module doc loses its memory sentence; the tests of 7.4; imports.
- `crates/eml_vm/src/lib.rs`: `pub use sched::{Normal, Policy, Scheduler, Seeded, Sequential};`; the comment before `main_ctx` (section 6).
- `crates/eml_sched/src/cells.rs`: S-TL-5; `TaskCell.limit` / `.bytes` and their initialisers; the doc of `receive_mail` (EN: "Handles one mail item. Any worker may handle a dropped handle or a signal; only the owner takes a cancel (`Mail::take`). Pushes the wakes of tasks a cancel unparked to `out`, the driver cancels to `work`, the values and IDs of released cells to `discards`, and returns the signal bits to lower from pending. Handling can push new items to the mail (docs/spec/vm.md 「タスクとスケジュール」).") and of `drain` (drops the sentences on the VM `Task` flags and the limit item); the tests of 7.4; `.resumed(..).unwrap().0` → `.unwrap()` and the `drain` test calls without `&mut Vec::new()` (kind 3); `Posted` loses `limits` and its `limit`; the other test `Post` impls lose `limit`.
- `crates/eml_cli/src/lib.rs`: S-TL-9.
- `crates/eml_cli/tests/common/mod.rs`, `crates/eml_cli/tests/ui.rs`: S-TL-7; the harness tests of 7.4 (exact new texts in the prototype patch).
- `crates/eml_cli/tests/api.rs`: delete `default_limits_put_the_soft_limit_at_seven_eighths`.
- `crates/eml_vm/tests/memory.rs`: delete the file and `mod memory;` in `crates/eml_vm/tests/main.rs`.
- UI: S-TL-8; accept `integration__ui__run_native.snap`.
- Docs (section 8): docs/spec/vm.md V-1..V-9; docs/spec/stdlib.md L-1..L-3; docs/spec/object-model.md O-8; docs/implementation/testing.md T-1..T-3.
- Completion:
  - G with N = 5237 [V: proto].
  - `git grep -nE "$P" -- crates/eml_vm crates/eml_sched crates/eml_cli tests` → empty, with `P` of SC-2 (the `eml_rt`, `eml_rows/src/rows.rs` and `eml_native` hits stay until TL-3a / TL-3b); SC-2's third command → empty; SC-3.
  - `cargo nextest run -p eml_cli --test integration -E 'test(=ui::run_fail) | test(=ui::run_native) | test(=ui::run_fail_native) | test(/max_bytes/)'` passes.
  - The feature clippies of CLAUDE.md for `eml_sched` (no `testing`) and `eml_io eml_rows eml_vm eml_nrt --no-default-features --all-targets` exit 0.
  - `git grep -nE "$D" -- docs/spec/vm.md docs/spec/stdlib.md` → empty, with `D` of SC-4.
- Left to the implementer: the Japanese wording of comments; whether to keep the test helper `limit_fixture_with` (it stays used by the renamed preemption test).

#### TL-3a: `Remove the soft threshold from eml_rt`

Work (`docs/plans/2026-11-17-task-limits/tl-3a-rt.patch` is the `control.rs` part):
- `crates/eml_rt/src/control.rs`: `Limits.soft_bytes` (field, doc, `Default`, `Control::unbounded`'s literal), `Control.next` and its initialiser, `MIN_GAP`, `rearm`, `pressure`, `claim_pressure`, the re-arm line of `clear`, and in `charge` the `next` comparison and `notice`; `charge` keeps `slot` and `taken` here; its doc (EN): "Adds the bytes of a page taken to worker `slot`'s count and to the total; raises `MEMORY` when the total exceeds the limit (docs/spec/object-model.md 「実行の制御」)." The comment block before `impl Control` loses 「閾値 `next`」.
- `crates/eml_rt/src/heap/tests/control.rs`: the tests of 7.4 for TL-3a; `soft_control` and `KIB` if unused.
- `crates/eml_rows/src/rows.rs`: delete `soft_bytes: 0,` in `rows_that_do_not_fault_return_ok_on_an_exhausted_budget`.
- `crates/eml_native/tests/machine.rs`: delete `a_notice_does_not_stop_the_run` and the imports only it used (`std::sync::Arc`, `eml_rt::{Control, Limits}`).
- Docs: docs/spec/object-model.md O-3, O-4, O-5, O-6 (`pressure`, `claim_pressure`); docs/spec/native.md N-1; docs/implementation/testing.md T-4, T-5.
- Completion: G with N = 5229 (5237 minus the 8 tests of 7.4 for TL-3a); `cargo verus verify -p eml_rt` → `0 errors`, record the count; `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` exits 0; `bench/rt-gate.sh <TL-2's commit>` on a clean tree exits 0 [V: measured, 10.2].
- Left to the implementer: comments.

#### TL-3b: `Remove the per-worker byte counts from eml_rt`

Work:
- `crates/eml_rt/src/control.rs`: `Control.taken`, `byte_counters`, `taken`, `charge`'s `slot` (S-TL-1 doc); `crates/eml_rt/src/alloc.rs`: `Pages.slot`, `Pages::new`'s `slot`, `Pages::slot()`, `charge_pages(control, bytes)` and its call, the test helper `new_pages`; `crates/eml_rt/src/heap/mod.rs`: `for_worker` and `with_slot` go (the body moves into `with_control`), `empty_core(control)`, the two internal callers use `with_control`, and S-TL-12 and the layout check of S-TL-12; `crates/eml_rt/src/heap/ctx.rs`: `Ctx::for_worker` calls `Heap::with_control(control.clone())`.
- Tests: 7.4 for TL-3b; every `control.charge(k, n)` → `control.charge(n)` (kind 3).
- Docs: docs/spec/object-model.md O-1, O-2, O-6 (`taken`), O-7, O-8; docs/implementation/benchmarks.md the two bullets of S-TL-12.
- Order (the gate needs a commit and a clean tree): commit TL-3b on a side branch `tl-3b` cut from `plan/task-limits`; run `bench/rt-gate.sh <TL-3a's commit> --allow 'find-bytes-1mib' --allow-bound 0.3` there (O-1, approved by the user on 2026-11-20); on exit 0, fast-forward `plan/task-limits` to it; on failure, leave `plan/task-limits` at TL-3a and go to O-1.
- Completion: G with N = 5226 [V: proto]; `grep -c 'offset_of!(Heap<crate::Fast>, work)' crates/eml_rt/src/heap/mod.rs` → `1`; `cargo verus verify -p eml_rt` → `1265 verified, 0 errors` [V: on the O-1 candidate]; `verusfmt` check exits 0; Miri `trusted::` under both models; the gate as above.
- Left to the implementer: nothing beyond comments.

#### CP-A

Push `plan/task-limits`; CI green on its tip; the checkpoint extras (7.3.3); the gate of 10.1 with base TL-1's commit and head the branch tip. A stop under 10.1 or an open O-1 goes to the user with the tables.

#### TL-4: `Rewrite the docs without the soft memory limit`

Work: section 8's items S-1, OV-1, F-1..F-3 (docs/implementation/status.md, docs/overview.md, docs/future/async.md, multicore.md, native.md). Run SC-4 first; its hits outside docs/future/roadmap.md and the kept testing.md line are the worklist.
- Completion: G (N = 5226, or 5229 if TL-3b did not land); SC-4 hits only docs/future/roadmap.md and the kept testing.md line; `citations::` passes.

#### TL-5: `Close task-limits`

Roadmap 「進め方」, one commit:
1. spec: `grep -rn 'task-limits' docs/spec docs/implementation` → empty; no 「今は〜、後で〜」 in the sections this stage wrote.
2. Measure (`crates/eml_vm`, `crates/eml_rt` changed): `bench/scripts.sh` first (a row out of range → `Resize bench rows`); then `bench/run.sh`, `bench/rt.sh`, `bench/scripts.sh` on the clean tree of TL-4 → the three tables of benchmarks.md 「今の数値」. 「ネイティブの数値」 is not touched. Without macOS: `Measure task-limits` right after.
3. status.md: done in TL-4; `git grep -nE "$D" -- docs/implementation/status.md` → empty.
4. architecture.md: no crate or path changed; `grep -n 'memory\.rs\|limits/' docs/implementation/architecture.md` → empty [V: grep at base].
5. roadmap.md (C-7; every anchor counts 1 with `git grep -cF` on bb9d27a1 [V]):
   - delete the `task-limits` item of 「段の列」 (anchor ``- `task-limits`: メモリの上限を、``); in the `sched-core` item, 「前提: `task-limits`」 becomes 「前提: なし」.
   - delete, with their sub-bullets, the six items of 「その後の項目」 whose premise is the soft limit or the per-task count, anchored: 「待ちのタスクが持つメモリを数えること」, 「本当の待ちから起きてまだ動いていないタスクを、メモリの柔らかい上限の候補にすること」, 「実行待ちのタスクを worker ごとに2つ以上公開すること」, 「`par` の右の子と `compute` の仕事の確保をタスクに数えること」, 「右の子がほかの worker で動く間の、親のタスクへの旗」, 「同じセルのエントリの `Task` が重なるときのタスクの数」.
   - delete 「`-- max-bytes:`の印を持つ5つのUIテストのネイティブでの扱い」 with its sub-bullets (S-TL-10).
   - read and keep (their conditions do not depend on the soft limit): 「書くファイルの buffer を確保量の上限に数えること」, 「監視のスレッドを `Machine` が run をまたいで持つこと」, 「タスクの中の`par`の子の深さの超過を、親のタスクの`ResourceLimit`にすること」, 「上限の近くで待ちに入ったタスクの再開の深さ」.
   - `grep -nE '柔らかい|max-bytes|task-limits|上限の旗|圧力の役目' docs/future/roadmap.md` → empty.
6. overview.md: done in TL-4; docs/README.md 「内容」: no change expected (`grep -n '柔らかい' docs/README.md` → empty at base).
7. CLAUDE.md: none (`grep -nE 'soft|柔らかい|max-bytes' CLAUDE.md crates/eml_rt/CLAUDE.md` → empty at base [V]).
8. Delete `docs/plans/2026-11-17-task-limits.md` and its appendix directory `docs/plans/2026-11-17-task-limits/` (the directory `docs/plans/2026-11-17-runtime/` stays). No other plan file is edited (S-TL-10).
9. Message: the test changes by kind with reasons (7.4), the gate tables of 10.1 and 10.2 with O-1's and O-2's outcomes and any user approval quoted, the measurements, D-13's date (2026-11-20), the deleted roadmap items.
- Completion: G; `citations::` passes; `ls docs/plans | grep -c task-limits` → `0`; ``sed -n '/^## 段の列/,/^## その後の項目/p' docs/future/roadmap.md | grep -c '^- `'`` → `10` (11 at bb9d27a1 [V]); S-7.

### 7.3 Gates and standing checks

#### 7.3.1 G (every unit, repository root, after `git add -A`)

1. `cargo nextest run` → exit 0; `N tests run: N passed[ (k slow)], 23 skipped` with N = 5272 (TL-1), 5237 (TL-2), 5229 (TL-3a), 5226 (TL-3b on) [V: proto for 5237, 5226; 5229 by the lists of 7.4]. If N differs, list with `cargo nextest list` and compare with 7.4's names for the unit: a listed change not made is the unit's own omission and is fixed; any other difference stops the unit.
2. `cargo clippy --all-targets -- -D warnings` → exit 0.
3. `cargo fmt --check` → exit 0.
4. `bash nix/crate-graph.sh --check` → exit 0.
5. Snapshot and expectation changes are only those of 7.4 for the unit (`git diff --stat HEAD~1 -- 'crates/*/tests/snapshots' tests/ui crates/eml_cli/tests/repl` and the inline expectations, read against 7.4).
6. `cargo nextest run -p eml_cli --test integration -E 'test(/^citations::/)'` passes.

#### 7.3.2 SC (standing checks)

```sh
# SC-1 (always): no docs/plans citation in code or docs
grep -rn 'docs/plans/20' crates docs --include='*.rs' --include='*.md' | grep -v '^docs/plans/'      # empty
# SC-2 (G-4; TL-2: first and third empty outside eml_rt, eml_rows/src/rows.rs, eml_native; TL-3b on: all three as stated)
P='soft_bytes|soft_of|pressure_tick|claim_pressure|fn pressure|bell_timeout|mark_limited|mark_pending|limit_task|limit_forwarded|clear_limit|add_bytes|limit_flag|TaskMarks|Notice::Limit|MailItem::Limit|post\.limit\(|fn limit\(&self|Limit::Pending|Aborting \{ *(pending|\.\.)|\bflagged\b|Memory::|MEMORY: Memory|P::MEMORY|\.end_slice\(|fn end_slice|published_count|offer_top|clear_top|drop_top|publish_idle|keep_running|write_published|top_bytes|NO_CELL|cell_word|\.taken\([a-z0-9]|fn taken\(&self, k|byte_counters|MIN_GAP|fn rearm|memory soft limit|fn a_pending_task|fn a_hit_[a-z_]*stays_pending|上限の旗|旗が立っていれば、保留|保留へ戻'
git grep -nE "$P" -- crates                                                                              # empty
git grep -nE 'max-bytes: [0-9]+ soft|memory soft limit|soft limit' -- tests std bench                  # empty
git grep -nwiE soft -- crates std tests bench ':!crates/eml_gui/src/winui/bindings.rs' | grep -vi microsoft   # exactly one line: crates/eml_cli/tests/ui.rs, "-- max-bytes: 100 soft 50"
# SC-3 (TL-2 on): one limit file, one mark
git grep -l 'max-bytes:' -- tests                                                                        # tests/ui/run-fail/async/limit_memory_ends_the_run.em
# SC-4 (docs; TL-2: none in docs/spec/vm.md, stdlib.md; TL-3b: none in docs/spec; TL-4: only roadmap.md and the kept testing.md line; TL-5: only the kept line)
D='柔らかい上限|soft_bytes|soft limit|上限の旗|上限の印|max-bytes: <bytes> soft|max-bytes: [0-9]+ soft|Limit::Pending|Limit::flagged|`Task`、`Pending`|`Task` と `Pending`|`Task`と`Pending`|\| `Pending` \||`Pending`の毒|`Pending` は 0|`Pending` では上限|`Pending`のまま|保留へ戻|Aborting \{|claim_pressure|pressure_tick|`pressure`|Control::pressure|圧力の役目|limit_task|limit_forwarded|clear_limit|add_bytes|TaskMarks|mark_pending|mark_limited|Control::taken|taken\(|top_bytes|NO_CELL|`top`|`counted`|`added`|`end_slice`|offer_top|clear_top|`yielded`|`choose`|Memory::|`Watched`|`Gathered`|定数`MEMORY`|知らせは9種類|`Limit\(cell\)`|`limits/`|charge\(slot|メモリの見張り|確保の多いタスク|確保の数がいちばん大きい|タスクの数`bytes`|memory\.rs|保留のタスク|0 は保留|上限を0にした|保留か、猶予|保留のまま'
git grep -cE "$D" -- docs README.md CLAUDE.md crates/eml_rt/CLAUDE.md ':!docs/plans' ':!docs/reports'
# SC-5 (always; TL-G1)
git diff HEAD~1 -- 'crates/*/src' | grep '^+.*unsafe'                                                    # empty in every unit
git diff <TL-1> -- crates/eml_cli/tests/surface.rs nix/crate-graph.sh                                    # empty
# SC-6 (always; TL-G2)
git diff <TL-1> -- crates/eml_vm/tests/bench.rs                                                          # empty
cargo nextest run -p eml_core_ir -E 'test(every_program_lowers_to_the_same_text)'                        # passes
```

`P`'s last six alternatives are the renamed and replaced tests (7.4) and the comment phrases of section 6; at the base they hit only soft-limit items (`上限の旗`, `旗が立っていれば、保留`, `保留へ戻` in comments of `vm.rs`, `sched.rs`, `monitor.rs`, `workers.rs`, `cells.rs`; the six `vm.rs` test names). Unqualified `stays_pending` and `旗が立っていれば` would also hit `eml_io` tests and the scope flag in `cells.rs`, so the pattern qualifies them [V: grep at 3aba15e9]. `上限を0にした` in `D` targets testing.md's 「確保量の2つの上限を0にした」 (T-5) [V].

#### 7.3.3 Checkpoint extras (before CP-A and CP-Z)

```sh
nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run -- -D warnings   # exit 0
nix develop .#cross -c cargo clippy --target x86_64-unknown-linux-gnu --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run -- -D warnings  # exit 0
nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=thread TSAN_OPTIONS=halt_on_error=1 CARGO_TARGET_DIR=target/tsan cargo test -Zbuild-std --target $(rustc -vV | sed -n 's/^host: //p') -p eml_vm --test integration -- workers:: signal:: io:: chan:: ffi:: ffi_abandon:: seat:: pinned::   # pass (the monitor and the slots changed)
nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=thread TSAN_OPTIONS=halt_on_error=1 CARGO_TARGET_DIR=target/tsan cargo test -Zbuild-std --target $(rustc -vV | sed -n 's/^host: //p') -p eml_rt trusted::   # pass
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::        # pass; once more with -Zmiri-tree-borrows
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::   # pass; once more with -Zmiri-tree-borrows
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_sched --lib -- cells::a_read cells::a_write   # pass; once more with -Zmiri-tree-borrows
```

### 7.4 Test changes by kind

Kind 1 (pass / fail), one by one. Net: 49 test names leave, 3 arrive, 4 are renamed; 5 UI files leave, 1 arrives [V: proto, the name lists of the base run and of each unit's run]. Every name below exists at 3aba15e9 (C-6) [V: base run's name list; re-checked by the review].

- TL-2, deleted UI files with their snapshots (their subject is the soft limit): `run/async/limit_memory_after_catch.em`, `run/async/limit_memory_caught.em`, `run/async/limit_memory_fails_scope.em`, `run-fail/async/limit_memory_uncaught.em`, `run/limits/soft_limit_without_a_task.em`. Added: `run-fail/async/limit_memory_ends_the_run.em` (S-TL-8).
- TL-2, deleted tests:
  - `eml_cli api::default_limits_put_the_soft_limit_at_seven_eighths`.
  - `eml_sched cells::{a_late_limit_notice_after_the_raise_is_dropped, a_limit_goes_to_the_owner_of_a_running_task, a_limit_in_the_mail_is_taken_by_any_worker, a_limit_on_a_waiting_task_waits_for_its_entry, add_bytes_adds_the_bytes_and_resumed_returns_them, add_bytes_skips_a_released_cell, clear_limit_zeroes_the_bytes}`.
  - `eml_vm monitor::tests::{a_gathered_tick_without_a_candidate_moves_the_threshold, a_monitor_with_a_doorbell_ticks_only_while_a_worker_runs, choose_picks_the_largest_count, pressure_tick_flags_once_per_threshold, pressure_tick_without_a_candidate_keeps_the_threshold, the_monitor_thread_flags_the_published_task}`.
  - `eml_vm sched::tests::only_normal_watches_memory`.
  - `eml_vm vm::tests::{a_flag_during_a_child_reaches_the_class_the_fork_saved, a_flag_for_a_waiting_task_inside_a_child_reaches_the_saved_class, a_flag_in_a_nested_child_reaches_the_outermost_saved_class, a_gathered_policy_flags_in_gather, a_parent_waiting_on_its_join_is_not_chosen_in_the_gather_of_its_switch, a_pending_task_returns_to_pending_after_a_panic_lands, a_preempted_task_is_offered_as_top_and_cleared_when_resumed, a_self_yield_offers_top_and_a_real_wait_clears_it, a_task_that_keeps_running_after_a_sleep_is_published_again, an_entry_that_waits_is_not_chosen_in_the_gather_of_its_switch, collect_marks_every_task_of_the_cell, collect_marks_the_task_of_a_forwarded_limit_only_while_flagged, collect_marks_the_task_of_a_limited_cell, end_slice_adds_the_slice_to_counted_and_added, mark_pending_keeps_the_abort_bracket}`, with the helpers `soft_fixture`, `take_pages` and the constant `CAUGHT_PANIC`.
  - `eml_vm memory::{a_compute_job_counts_to_no_task, a_crossing_in_a_right_child_while_its_parent_waits_flags_no_task, a_flag_delivered_to_a_parked_task_is_caught_by_its_own_try_io, a_left_child_allocation_counts_to_its_task, a_right_child_allocation_counts_to_no_task}` (the file).
- TL-2, replaced by name (the old test's subject was the flag or the `Pending` class; the new one pins what the class rules leave):
  - `ui::the_max_bytes_mark_sets_both_limits` → `ui::the_max_bytes_mark_sets_the_limit` (mark `-- max-bytes: 268435456` → `Some(268435456)`; the mixed line `-- max-bytes: 100` → `Some(100)`; the run under `Some(8 << 20)` fails and the free run passes).
  - `vm::tests::a_pending_task_without_an_io_handler_keeps_the_flag` → `vm::tests::a_task_entry_without_an_io_handler_ends_the_run` (`spin`'s entry, class `Task`, `depth_offset = DEPTH - 1`: `Err(Stopped(DepthLimit))`, class `Task`, limit `real`).
  - `vm::tests::a_raise_for_a_real_overrun_resets_the_count` → `vm::tests::a_raise_for_a_real_overrun_aborts_with_the_headroom` (`CAUGHT_IO` stepped to `leaf`, `depth_offset = DEPTH - 1`: `Ok`, class `Aborting`, limit `real + LIMIT_HEADROOM`).
- TL-2, renamed (the class `Pending` is gone; the bodies change as K2-c says): `vm::tests::a_hit_at_a_helper_entry_stays_pending` → `a_hit_at_a_helper_entry_is_deferred`; `vm::tests::a_hit_after_a_return_stays_pending` → `a_hit_after_a_return_is_deferred` (body unchanged); `vm::tests::a_pending_task_collects_a_notice_at_its_safepoint` → `a_task_over_its_limit_collects_a_notice_at_its_safepoint`; `vm::tests::a_pending_task_still_spends_the_preemption_budget` → `a_task_over_its_limit_still_spends_the_preemption_budget`. (The renames leave the totals unchanged.)
- TL-3a, deleted: `eml_native machine::a_notice_does_not_stop_the_run` (no `NOTICE` source is left in a native run; the safepoint's notice path stays pinned by `eml_nrt services::a_notice_is_taken_and_the_run_continues`); `eml_rt heap::tests::control::{claim_pressure_moves_by_a_quarter_of_the_headroom, clear_rearms_a_gap_above_the_current_bytes, crossing_the_soft_limit_notices_only_the_charging_worker_once, pressure_rearms_at_soft_when_bytes_fall_below_it, the_hard_limit_wins_over_the_soft_limit, the_soft_limit_never_stops_the_run, without_a_soft_limit_there_is_no_pressure}` (the hard limit stays pinned by `memory_limit_sets_the_stop_bit`, `reaching_the_memory_limit_exactly_sets_no_bit`, `the_memory_limit_reaches_the_worker_word`).
- TL-3b, deleted: `eml_rt heap::tests::control::taken_counts_each_worker_and_ignores_refunds`; `eml_rt heap::tests::trusted::{checked, fast}::a_worker_heap_charges_its_own_slot`.

Kind 2 (expected values), as scopes:
- K2-a (TL-2): `integration__ui__run_native.snap`: `accepted 492 → 491`, `rejected 145 → 142`, `async row 126 → 123`, the rest unchanged [V: proto]. `integration__ui__run_fail_native.snap` unchanged.
- K2-b (TL-2; the mark's form, S-TL-7): `ui::a_repeated_max_depth_or_max_bytes_mark_panics` (input `-- max-bytes: 8` / `-- max-bytes: 16`, message `` at `-- max-bytes: 16` ``), `ui::a_malformed_max_bytes_mark_panics` (malformed lines `-- max-bytes:`, `-- max-bytes: `, `-- max-bytes: 0`, `-- max-bytes: 1k`, `-- max-bytes: +5`, `-- max-bytes:  100`, `-- max-bytes: 100 `, `-- max-bytes:100`, `-- max-bytes: 100 soft 50`; message `` the max-bytes mark is `-- max-bytes: <bytes>`, not `<line>` ``), `ui::a_max_bytes_mark_in_a_transcript_panics` (input `# max-bytes: 100`).
- K2-c (TL-2; `vm.rs` unit tests lose their `Pending` and `Aborting { pending }` cases): `a_hit_at_a_helper_entry_is_deferred` loses its first loop (class `Pending` at `compose$` and `prompt$`) and keeps the real-overrun case; `a_helper_entry_beyond_the_headroom_ends_the_run` runs class `Task` only; the two `a_task_over_its_limit_*` tests set class `Task` instead of `Pending` with a depth over the real limit (the safepoint still takes the cold path at a non-entry pc) and the preemption test's last assertion reads `Limit::Task`; `a_depth_hit_while_aborting_ends_the_run` sets `Limit::Aborting`.
- K2-d (TL-3b): `heap::tests::control::control_keeps_a_word_for_the_main_worker` loses its three `taken` lines.

Kind 3: `control.charge(k, n)` → `control.charge(n)` (TL-3b); `.resumed(..).unwrap().0` → `.unwrap()` and the `drain` test calls without `&mut Vec::new()` (TL-2); `Notice::Limit` → `Notice::Cancel` as the vehicle of `a_notice_to_the_parked_main_worker_rings_its_park_waker` (same expectations); unused `mut sched` bindings in `vm.rs` tests; the `rows.rs` literal; the `new_pages` helper; comments.

Expected unchanged (a change stops the unit): every other UI snapshot, in particular `run/async/limit_depth_{after_cancel, caught, fails_scope, in_shield, repeated}` and `run-fail/async/{limit_depth_par_child, limit_depth_uncaught}`; every `bench.rs` snapshot; every Core IR / LIR dump; every REPL transcript; `run_fail_native.snap`; `eml_nrt services::a_notice_is_taken_and_the_run_continues`; `workers::`, `signal::` and `chan::` of `eml_vm`.

### 7.5 Soundness points and what holds each

| Point | Unit | Argument | Held by |
|---|---|---|---|
| `limit_hit` and the classes | TL-2 | No memory safety depends on the class: it chooses the `Ctx` depth limit and whether an abort starts. The abort path (`abort_to`, `unwind`, `landed`) is the base's for a real overrun; only the `Pending` branches go. S-14: steps 1-2 come first | K2-c tests and the two new ones; `run*/async/limit_depth_*` in checked mode at 1, 2, 8 workers and two seeds |
| Cells API | TL-2 | `drain` keeps its `# Safety`; deleting a parameter weakens nothing it relies on; `started` / `resumed` keep their state checks | `surface::`; `cells.rs` tests; TSan `workers::`, `chan::` |
| Monitor and slots | TL-2 | Removed: the seqlock writes and reads of the slot words (their only reader was `choose`) and one doorbell ring. Stop bits reach parked workers without the tick (S-TL-3): `MEMORY` through the raising worker's exit and `wake_all`, `INTERRUPT` / `TERMINATE` through the doorbell, `END` through the exit paths. With a doorbell this is the base's behaviour under `Seeded` and `Sequential` | TSan at CP-A (`workers::`, `signal::`); `signal.rs` on the real driver; `api::the_session_continues_after_the_memory_limit`; the new UI file at 2 and 8 workers |
| `charge` and `Pages` | TL-3a, TL-3b | `charge_pages` stays T1 `external_body` with no promise; the removed field and parameter carried no proof obligation; the `Heap` field order is not a proof fact; `trust.rs` is unchanged; `Heap` is `repr(Rust)` and nothing outside `eml_rt` reads inside it (native code uses only the `CTX_*` offsets inside `Native`); `offset_of!(Ctx, heap)` and `size_of::<Heap>` do not change; `work` drops before `core` and only frees its buffer | Verus; Miri `trusted::` in both models; `bench/rt-gate.sh` |

A gap found in any of these is fixed in the stage after consulting a strong advisor (CLAUDE.md), not noted for later.

## 8. Docs: the file-by-file list (C-4)

SC-4's hits per file on bb9d27a1 [V: `git grep -cE "$D" bb9d27a1 -- docs ...`]: docs/spec/vm.md 43, docs/spec/object-model.md 10, docs/spec/stdlib.md 5, docs/spec/native.md 1, docs/implementation/testing.md 6 (1 kept), docs/implementation/status.md 3, docs/overview.md 1, docs/future/async.md 4, docs/future/multicore.md 4, docs/future/native.md 6, docs/future/roadmap.md 9; 92 lines. README.md, CLAUDE.md, crates/eml_rt/CLAUDE.md, architecture.md, benchmarks.md: 0. Each item gives an anchor (counted once on bb9d27a1 [V]) and the change; "EN" texts are drafts.

docs/spec/vm.md (TL-2):
- V-1 「中断と実行時エラーの分担」, anchor `深さの上限の超過とタスクの上限の旗も中断で` → 「深さの上限の超過も中断で」.
- V-2 「上限と割り込み」: the table row anchored `` | `soft_bytes` | `max_bytes` の 7/8 `` goes; `(柔らかい上限もその 7/8)` goes; the bullet anchored `- メモリの柔らかい上限 `soft_bytes` は run を止めない` goes; in the depth bullet, the sentence anchored ``そうでなく旗だけなら`the memory soft limit is exceeded`である`` → EN "The message is `the call depth limit is exceeded`, without a position". Add one bullet after the `max_bytes` row's text (EN): "Going over `max_bytes` ends the run, also inside an `Async` task: `try_io` and `try_all` do not catch it."
- V-3 「タスクとスケジュール」: ``- `Policy`は5つのメソッドと定数`MEMORY`を持つ`` → 「5つのメソッドを持つ」; the bullet ``- `Policy`の定数`MEMORY`は`` goes; `- 知らせは9種類である`: 8 kinds, `Limit` leaves the list; the bullet ``- `Limit(cell)`は語を持たず`` goes; anchor `監視のスレッドは動く worker のない間は眠りうるので` → EN "the monitor with a self-pipe sleeps until it rings, so ..."; anchor ``ほかのrun (`Clock::Virtual`でworkerが1つのrun) では、`MEMORY`が`Watched`の方針に限り``: the sentences on the second trigger and on soft-limit candidates go (EN: "Other runs (a `Clock::Virtual` run with one worker) create no monitor thread"); `- 監視のスレッドの役目は3つある` → 2, the third role's sentences go; the bullet anchored `- 監視の刻みは10 msである` → EN "The monitor's tick is 10 ms and is used only without a self-pipe: such a monitor wakes every tick to look at pending signals and stop bits, whatever the policy, because a signal handler cannot wake anyone; a stop is therefore seen within one tick plus the OS delay. With a self-pipe the monitor sleeps until it rings: the signal handlers, the GUI close request and the side that stops the monitor ring it. A stop bit needs no tick: `MEMORY` is raised by a running worker, which reaches its safepoint and ends the run, and the ending worker wakes every worker. A program that only waits in `Signal.wait` therefore does not wake the CPU." (keep the thread name, stack size and failure sentences); the bullets anchored ``- タスクの数は、`Async`のタスクが動いているVMの`Task`だった間に``, `- 各workerは、動いているタスクと実行待ちの中で数のいちばん大きいタスクを`, ``- 欄の`top`と`top_bytes`は`` go.
- V-4 「メインのworker」, anchor `決定的な方針ではメモリの柔らかい上限の旗の地点が確保量で決まるためである` → EN "so that the safepoint at which a deterministic run reaches the memory limit depends on the allocations only"; anchor `N > 1では柔らかい上限の旗の地点はスケジュールだけでは決まらない` → EN "with N > 1 that safepoint also depends on the schedule".
- V-5 「深さ」, anchor `タスクが子の中で動いている間に届いた上限の旗は`: the sentence goes.
- V-6 「セルと登録表」: anchor ``タスクの上限の旗`limit`、タスクの数`bytes`を持つ`` → the two items leave the list; anchor `動いているタスクの欄を読み書きすること` leaves the list; anchor `監視のスレッドが立てるタスクの上限の旗の4つである` → 「…中継したシグナルの3つである」; the sentence anchored ``上限の旗の項では、`Registry::limit_task`でセルに旗を立てて届け先を決める`` goes.
- V-7 「タスクの上限」: the table becomes EN `| Stops | real | outside a task (including par children and compute jobs); going over it ends the run |`, `| Task | real | inside a task |`, `| Aborting | real + LIMIT_HEADROOM | from abort_to of a ResourceLimit until the landing |`; the `LIMIT_HEADROOM` bullet's ``Task` と `Pending` でも`` → 「`Task`でも」; the bullets anchored `- タスクの上限の旗はタスクのセルの欄 `limit` にあり`, `- 旗を届ける道:`, ``- `Notice::Limit`を受けた持ち主は``, ``- 集める地点は、返されたセルのVMの`Task`に旗を付ける``, ``- 旗を受けた後の状態は、どの所でも`Limit::flagged`で決める`` go; 切り替え: `(`Pending` は 0、`Aborting` は ...)` → 「(`Aborting`は`real + LIMIT_HEADROOM`)」 and 「保留と猶予が」 → 「猶予が」; `limit_hit`'s list becomes D-TL-1's eight steps (steps 2, 3, 5 say 「続ける」 where they said 「保留のまま続ける」; step 4 loses its `Pending` sentence; step 5 loses 「旗でも本当の深さの超過でも同じである」); 囲み → EN "`abort_to` does not change the class. Every place where `vm::unwind` returns `Ok` (the `Finish` of `exit`, a landing) calls `landed`, which turns `Aborting` into `Task` (limit `real`)."; エントリを待つ間 loses its `Pending` and 「旗」 sentences; IOのevidenceのない所 → EN "Outside the bottom `try_all` (before it in `start_task` and `bottom`, and after it returns) there is no evidence: a real overrun at an entry there ends the run (step 8)."; 節のエントリで起こしたとき: 「保留の旗がそこで起きると」 → 「深さの超過がそこで起きると」; 取り消しとの競合: 「旗の立ったタスクが」 → 「深さを超えたタスクが」 and its 保留 sentence goes; the bullets anchored `- メモリの柔らかい上限: タスクの数` and `- メモリの残る危険:` go; 残る危険 loses the flag sentence and the `top` / `add_bytes` sentence.
- V-8 「取り消しの届け方」, anchor `最初の失敗、タスクの上限の旗)` → 「最初の失敗)」.
- V-9 「タスクの上限」, the bullet anchored ``- 状態を上限の値で表すので、`Ctx::poll`、`stop_reason`、`vm::poll`の`Ok`の枝は``: 「本当の超過か、保留か、猶予の超過かは冷たい道が決める」 → 「本当の超過か猶予の超過かは冷たい道が決める」; 「予算で切り替える方針 (`Seeded`) が保留のタスクを切り替えなくなる」 → 「…が深さを超えたままのタスクを切り替えなくなる」; its last sentence 「ほかは保留のまま次の安全点に任せる」 → 「ほかは続けて次の安全点に任せる」.
- Done: `git grep -nE "$D" -- docs/spec/vm.md` → empty.

docs/spec/stdlib.md (TL-2):
- L-1 「`try_io`」, anchor `とメモリの柔らかい上限は` → 「は」.
- L-2 「タスクの上限」: the bullet anchored ``- 確保量が柔らかい上限`soft_bytes`の閾値を越えると`` → EN "Going over the memory limit `max_bytes` is a runtime error that ends the run, also inside a task; no `try_io` catches it ([VM](vm.md) 「上限と割り込み」)."; anchor `そのタスクの数に入る` → EN "runs inside the root task"; the bullet anchored ``- 上限の印を受けたときに `IO` の失敗を起こせない所`` goes.
- L-3 「実行時エラー」, anchor ``メモリの柔らかい上限も同じ種類のIOの失敗`the memory soft limit is exceeded`である``: the clause goes and the sentence is rejoined.
- Done: `git grep -nE "$D" -- docs/spec/stdlib.md` → empty.

docs/implementation/testing.md:
- T-1 (TL-2), anchor ``memory.rs`のメモリの柔らかい上限のテスト``: the clause with its parenthesis goes.
- T-2 (TL-2), anchor `` `-- max-bytes: <bytes> soft <bytes>` ``: the mark bullet → EN "An entry `.em` of `run/` and `run-fail/` may write `-- max-bytes: <bytes>` among its leading marks; the harness applies it as `Limits { workers, max_bytes, ..default_limits() }` to every run of the file (with `-- max-depth:` if present). The number is positive decimal digits after one space. A malformed or repeated `-- max-bytes:` line, and the mark in a `check-fail/` file or a transcript, panic."
- T-3 (TL-2), anchor ``- `limits/`: ``: the bullet goes; the `async/` bullet gains EN "the depth limit (`limit_depth_*.em`) and the memory limit (`limit_memory_*.em`)".
- T-4 (TL-3a), anchor `柔らかい上限1バイトで立った知らせを安全点が受け取り、runが続くこと`: the clause goes.
- T-5 (TL-3a), anchor `確保量の2つの上限を0にした` → 「確保量の上限を0にした」.
- Kept: the line with `ハーネスは上限の印を2つとも` (both marks still exist).

docs/spec/object-model.md:
- O-1 (TL-3b) 「確保と解放」, anchor ``- 確保器は `Pages` に worker の番号 (`slot`) を持ち`` → EN "The allocator passes the bytes of each page it takes to `Control::charge(bytes)`." (the `taken` and `slot` sentences go).
- O-2 (TL-3b) 「実行の制御」, the table row anchored `| workerごとの取った量 |` goes.
- O-3 (TL-3a) the row anchored `| 柔らかい上限の閾値 |` goes; the `Limits` row loses ``、`soft_bytes` (…)``.
- O-4 (TL-3a) the bullet anchored `- 柔らかい上限は run を止めない` goes; the remaining `charge` sentence says it raises only `MEMORY`.
- O-5 (TL-3a) `Control::clear()`'s bullet, anchor `閾値`next`を今の確保量からの`rearm`に置き直す`: the clause goes.
- O-6 (TL-3a `pressure`, `claim_pressure`; TL-3b `taken`) the list of safe operations outside `verus!` loses the three names.
- O-7 (TL-3b) 「`unsafe` の核」 T1, anchor ``charge_pages`は課金したworkerの取った量にも足し、そのworkerの止める語に`NOTICE`を立てうる。``: the sentence goes.
- O-8 (TL-2; the `Pending` class): the table row anchored ``| `depth_limit` | 今のタスクの深さの上限。``: 「保留のタスクでは0、猶予の間は」 → 「猶予の間は」; 「タスクの切り替え」, anchor `VM はこれで、タスクの深さの超過を知らせる保留と、巻き戻しの間の猶予を表す。0 は保留で、次の安全点の `poll` が必ず `DepthLimit` で失敗する。`: → EN "The VM uses it for the headroom during an unwinding." and the next sentence (on the depth passed being at least 1, which only argued that 0 always fails) goes; 「実行の制御」, anchor `VMが上限を0にした保留のタスクの上限である` → EN "`DepthLimit` means: no stop bit, and the depth exceeds the current limit, which is the real limit or the real limit plus the headroom; the VM decides which". O-8 is in TL-2 because TL-2 removes the class.
- Done after TL-3b: `git grep -nE "$D" -- docs/spec/object-model.md` → empty.

docs/spec/native.md (TL-3a): N-1, anchor ``- `max_bytes`と`soft_bytes`は`` → 「`max_bytes`は」.

TL-4 (anchors counted on bb9d27a1):
- S-1 docs/implementation/status.md: anchor `確保量がメモリの柔らかい上限 (既定は`max_bytes`の7/8) の閾値を越えたときも` → the sentence goes, and the bullet says the memory limit `max_bytes` ends the run also inside a task; anchor `子の中で受けたタスクの上限の印は、join の後の親で起こす` → the sentence goes; the bullet anchored `- メモリの柔らかい上限が選ぶタスクは、原因のタスクとは限らない` goes.
- OV-1 docs/overview.md, anchor `メモリの柔らかい上限 (既定は硬い上限の 7/8) で選ばれると` → the row 「タスクごとの上限」 says EN "Exceeding the depth limit inside an `Async` task raises the IO failure `ResourceLimit` in that task without ending the run; `try_io` and `try_all` catch it. The depth limit outside a task and the memory limit end the run" (keep the row's remaining clauses).
- F-1 docs/future/async.md: decision 7, anchor `メモリの柔らかい上限で選ばれると、そのタスクの IO の失敗` → EN "Exceeding the depth or stack limit inside a task is that task's IO failure `ResourceLimit`. The memory limit ends the whole run"; anchor `タスクの中で深さの上限を超えるか、メモリの柔らかい上限で選ばれたタスクは` → 「タスクの中で深さの上限を超えたタスクは」; the bullets anchored `- メモリの数は、worker のヒープが取ったページを、切り替えの地点で` and `- 残る危険: 待ちのタスクが持ち続けるメモリと` go.
- F-2 docs/future/multicore.md: anchor `ほかのrunでは、`MEMORY`が`Watched`の方針に限り、最初の`Async`のタスクを実行待ちに積むときに始まる。` → EN "In other runs there is no monitor thread."; in the same bullet, anchor `どれかのworkerが動いている間だけ監視の刻み (既定10 ms) ごとに起き、ほかの間は自己パイプで眠る。眠っていたworkerが動き始めると自己パイプが鳴る。` → EN "sleeps on the self-pipe until it rings."; the sub-bullet anchored `  - 確保量がメモリの柔らかい上限の閾値を越えていれば` goes; anchor `またはメモリの柔らかい上限で選ばれたときは` (the 「タスクの上限」 paragraph) → the clause goes, and from `スケジューラの側の仕事は2つある。` to the sentence ending `残りの余裕の 1/4 だけ先に進む。` → EN "The memory limit `max_bytes` is not per task: it ends the run."; the item anchored `- **待ちのタスクが持つメモリを数えること**` goes.
- F-3 docs/future/native.md: anchor `またはメモリの柔らかい上限で選ばれたときは` → the clause goes; anchor ``値は`Stops`、`Task`、`Pending`、`Aborting`で`` → 「値は`Stops`、`Task`、`Aborting`で」 and the sentence anchored ``Pending`では`stack_limit`に毒`` goes; anchors ``Task`と`Pending`なら、戻り値の道で`` and ``Task`と`Pending`では`ResourceLimit`を起こさず`` → 「`Task`なら」 / 「`Task`では」; the bullet anchored `- メモリの数は `eml_rt` の確保器が worker ごとに足すので` goes; anchor `(`Pending`の毒を含む)` → removed.
- Done: SC-4 hits only roadmap.md and the kept testing.md line.

## 9. Verification plan

Baseline (TL-1, clean tree, after `cargo build --all-targets`; fill "Recorded"):

| Item | Command | Expected | Recorded |
|---|---|---|---|
| full run | `cargo nextest run` | `5272 tests run: 5272 passed, 23 skipped` [V] || `5272 tests run: 5272 passed (12 slow), 23 skipped` |
| Verus | `cargo verus verify -p eml_rt` | `1270 verified, 0 errors` [V] || `1270 verified, 0 errors` |
| native snapshots | `sed -n 5,9p crates/eml_cli/tests/snapshots/integration__ui__run_native.snap` | `accepted 492`, `skipped 5`, `rejected 145`, `async row 126`, `par 19` [V] || as expected |
| G-4 grep (C-5) | `git grep -cE "$P" -- crates` (SC-2) | the files of 3aba15e9 [V]: eml_cli/src/lib.rs 3, eml_cli/tests/api.rs 2, eml_cli/tests/ui.rs 3, the four snapshots of the deleted async files 1 each, eml_native/tests/machine.rs 2, eml_rows/src/rows.rs 1, eml_rt/src/control.rs 15, eml_rt/src/heap/tests/control.rs 12, eml_rt/src/heap/tests/trusted.rs 3, eml_sched/src/cells.rs 57, eml_vm/src/monitor.rs 39, eml_vm/src/sched.rs 36, eml_vm/src/vm.rs 123, eml_vm/src/workers.rs 23, eml_vm/tests/memory.rs 3, plus the hits of the six alternatives added in r2 (test names and comment phrases in vm.rs and cells.rs; record the counts). Every hit is a soft-limit item [V: read; the pattern avoids `bytes_end_slice`, `reap_flagged`, `Control::MEMORY`, `Heap::taken`, `remote.rs`'s `taken`, `chan_cancel_points.em`'s `flagged`] || as expected, with the r2 alternatives: eml_sched/src/cells.rs 67, eml_vm/src/monitor.rs 42, eml_vm/src/sched.rs 38, eml_vm/src/vm.rs 139, eml_vm/src/workers.rs 24; other files as listed |
| UI grep | `git grep -cE 'max-bytes: [0-9]+ soft|memory soft limit|soft limit' -- tests std bench` | the five UI files, 1 each [V] || the five files, 1 each |
| docs grep (C-4) | SC-4 | section 8's counts, 92 lines [V on bb9d27a1] || 92 lines: async.md 4, multicore.md 4, future/native.md 6, roadmap.md 9, status.md 3, testing.md 6, overview.md 1, spec/native.md 1, object-model.md 10, stdlib.md 5, vm.md 43 |
| crate graph | `bash nix/crate-graph.sh --check` | exit 0 || exit 0 |
| citations | `cargo nextest run -p eml_cli --test integration -E 'test(/^citations::/)'` | pass || pass (4) |

Overall (on TL-5's commit): `cargo nextest run` with 5226 tests (5229 if TL-3b did not land); every SC line; Verus 1265 (1270 minus the TL-3a count difference if TL-3b did not land; record); the gates of 10.1 and 10.2; CI `success` on every job.

Manual checks (CP-A):
- A program that only waits on `Std.Signal` with the real driver does not wake the CPU while idle (the monitor sleeps on the doorbell) and still ends on Ctrl-C: `timeout 20 cargo run -q --release -p eml_cli -- run <a file whose main does Signal.wait interrupt>` sits at 0 % CPU in `top`, then Ctrl-C ends it with code 130.
- The new UI file is not run by hand (the default limit is 80 % of memory); the hard limit is covered by the UI file, `api::the_session_continues_after_the_memory_limit` and `calls::a_growing_heap_hits_the_memory_limit`.

## 10. Risks and unresolved items

### 10.1 The VM gate (U-3, user decision of 2026-11-20)

Rule (C-9, U-3): a `bench/run.sh` row whose median over three alternating pairs of 実行の分 deltas is over +0.5 % is re-measured alone over 15 alternating rounds; it stops the stage only if its 15-round median stays over +0.5 % and the stage changed code on that row's path; otherwise the Close message records it as layout. There is no `bench/run-gate.sh` [V: `ls bench`]; the procedure below is the gate.

The row's path (the reading of "the unit's diff names a function on it"): the functions the row runs per unit of its work, i.e. whose number of calls grows with the row's iteration count. One-time code is not on any row's path. The stage's changed functions fall into these classes; a class is on row R's path as stated:

| Class | Changed functions (from the diff B..H) | On R's path when |
|---|---|---|
| One-time setup | `Scheduler::begin_run`, `start_monitor`, `Monitor::start`, `watch_loop` (another thread, asleep), `Control::new`, `Heap::with_control`, `Ctx::for_worker`, `Pages::new`, `eml_cli` limits | never |
| Cold limit paths | `limit_hit`, `landed`, `abort_to`, `apply_limit`, `collect`'s limit arm | never (no `bench/` row reaches the depth limit) |
| Task switch, fork and join | `enter`, `resume`, `switch_away`, `entry_finish`, `schedule`, `gather`, `take_async`, `Registry::{started, resumed, receive_mail}`, `cells::drain`, `Run::unpark_locked`, the `par` and root waits | `grep -lE '\bpar\b|Async' bench/R.em` lists R |
| Allocation and release | `Control::charge`, `charge_pages`, `fresh_page`, and every `Heap` method whose code changed through S-TL-12 or the `Pages` layout | R's `allocations` in its `crates/eml_vm/tests/bench.rs` snapshot exceeds `empty`'s |
| Dispatch and safepoint fast path | the VM's instruction loop, `Ctx::poll` | always, if the diff changes them (the prototype does not [V: proto diff]) |

A row stops the stage when its 15-round median is over +0.5 % and a class on its path has a changed function. The orchestrator then brings the user the pair table, the 15-round medians, and `git diff -U0 "$B" "$H" -- crates/eml_vm/src crates/eml_rt/src crates/eml_sched/src | grep '^@@'` (O-2).

Procedure (macOS; every command from the repository root of the clean worktree the first step makes; both commits build in the same path because the build directory changes counts, benchmarks.md 「命令の数の測り方」):

```sh
# 0. Names. B = TL-1's commit, H = the tip of plan/task-limits after TL-3b; S = a scratch directory outside the repository.
B=$(git log -n1 --format=%H --grep='^Start task-limits$' plan/task-limits)
H=$(git rev-parse plan/task-limits)
S=$(mktemp -d)
git worktree add --detach "$S/tl-gate" "$B"
cd "$S/tl-gate"
# 1. Three alternating pairs.
for i in 1 2 3; do
  git switch -q --detach "$B"; bench/run.sh > "$S/base-$i.md"
  git switch -q --detach "$H"; bench/run.sh > "$S/head-$i.md"
done
cp target/release/eml "$S/eml-head"                       # the loop's last build is H
git switch -q --detach "$B"; cargo build -q --release -p eml_cli; cp target/release/eml "$S/eml-base"
# 2. Medians of the pairs (pairs.py below); rows marked OVER go to step 3.
python3 "$S/pairs.py" "$S"/base-1.md "$S"/head-1.md "$S"/base-2.md "$S"/head-2.md "$S"/base-3.md "$S"/head-3.md
# 3. 15 alternating rounds for every OVER row (rounds.sh below).
bash "$S/rounds.sh" "$S/eml-base" "$S/eml-head" 15 <OVER rows...>
# 4. Clean up.
cd - && git worktree remove --force "$S/tl-gate"
```

`$S/pairs.py` (median over the pairs of `(head 実行の分 / base 実行の分 - 1) × 100`; `empty` has no 実行の分 and is skipped):
```python
import re, statistics, sys
def parse(p):
    return {m.group(1): int(m.group(3)) for line in open(p)
            if (m := re.match(r"\| `([^`]+)` \| (\d+) \| ?(\d*) ?\|", line)) and m.group(3)}
f = sys.argv[1:]
pairs = [(parse(f[i]), parse(f[i + 1])) for i in range(0, len(f), 2)]
for n in sorted(pairs[0][0]):
    d = [(h[n] / b[n] - 1) * 100 for b, h in pairs]
    m = statistics.median(d)
    print(f"{n:24s} {m:+7.2f} %  [{' '.join(f'{x:+.2f}' for x in d)}]{' OVER' if m > 0.5 else ''}")
```

`$S/rounds.sh` (run from the worktree root; each round measures `empty` and the row with both binaries; prints the median of the per-round deltas of 実行の分):
```sh
#!/usr/bin/env bash
# usage: rounds.sh BASE_BIN HEAD_BIN ROUNDS ROW...
set -euo pipefail
base_bin=${1:?BASE_BIN} head_bin=${2:?HEAD_BIN} rounds=${3:?ROUNDS}
shift 3
export EML_WORKERS=1
count() { { /usr/bin/time -l "$1" run "$2" >/dev/null; } 2>&1 | awk '/instructions retired/ {print $1}'; }
for row in "$@"; do
  deltas=()
  for _ in $(seq "$rounds"); do
    be=$(count "$base_bin" bench/empty.em); br=$(count "$base_bin" "bench/$row.em")
    he=$(count "$head_bin" bench/empty.em); hr=$(count "$head_bin" "bench/$row.em")
    deltas+=("$(python3 -c "print(f'{(($hr - $he) / ($br - $be) - 1) * 100:.3f}')")")
  done
  med=$(printf '%s\n' "${deltas[@]}" | sort -g | sed -n "$(( (rounds + 1) / 2 ))p")
  echo "$row $med"
done
```

Expected [V: measured on the prototype without S-TL-12, against 3aba15e9]: steps 1-2 mark several rows OVER (two pairs gave 10, up to `par_tree` +1.91 %); step 3 gives at most +0.29 % for the seven rows tried. With S-TL-12 the allocation-heavy rows are expected to fall [I: from 10.2]. `bench/async/`, `bench/net/` and the `https-get` row are not gated.

### 10.2 The `eml_rt` gate (crates/eml_rt/CLAUDE.md, unchanged)

`bench/rt-gate.sh <the commit before the unit>` on a clean tree with the default 5 pairs, after TL-3a and after TL-3b; exit 0 is required. `--allow ROWS --allow-bound PCT` passes only with a negative gated sum and only with the user's approval; anything else is a waiver of the gate, which only the user can grant.

Measurements [V: measured, 5 pairs each, scratch commits: X = 3aba15e9 + TL-3a's `control.rs`; Y = X + TL-3b's `eml_rt` without S-TL-12; C = Y + S-TL-12]:
- TL-3a (X against 3aba15e9; `eml_rt` of TL-2 equals the base's): exit 0; gated sum -3.95 M; largest rows `find-bytes-1mib` +0.016, `show-int` +0.002, `map` +0.001 %.
- TL-3b without S-TL-12 (Y against X): exit 1. Over +0.1 %: `map` +1.115, `map-reuse` +1.487, `show-int` +0.483, `show-int-long` +0.192, `slice-short` +0.519, `find-bytes-1mib` +0.240; gated sum +57.8 M instructions, so `--allow` cannot pass it.
- Cause [V: objdump of `rt_bench`, X vs Y]: removing `Pages.slot` (8 bytes, inside `Heap.core`) moved `Heap.work` from offset 0x3c0 to 0x3b8. `release_children` swaps `work` with 16-byte `ldr q0` / `str q0`; 0x3b8 is not a multiple of 16, so the compiler keeps the address in `x23`, needs one more callee-saved register and saves and restores one more pair (`stp x26, x25` / `ldp x26, x25`) on every call. `map` calls `release_children` per element: two extra instructions per element (+20 M over 10 M elements), `map-reuse` one. This is a real per-call cost caused by a field offset, not instruction placement; the `show-int` and `slice-short` rows move by the same 1-instruction-per-iteration quantum [I: same cause; their loops are inlined into `main`, not compared].
- With S-TL-12 (C against X): exit 1 only by `find-bytes-1mib` +0.244 % (+1.0 M); every other row falls (`map` -1.111, `map-reuse` -2.964, `show-int` -1.439, `show-int-long` -0.574, `slice-short` -1.546, `concat-copy` -0.306); gated sum -200.7 M. `find_bytes` (`eml_rt::heap::ctx::positions::find_bytes`) is the same instruction sequence in X and C except branch addresses [V: objdump diff with addresses masked]; the row also moved +0.233 % when only `taken` was removed (3 pairs against 3aba15e9). Its extra million instructions over 100 scans of 1 MiB come from code the stage does not change [I: data placement of the haystack against the 8-byte SWAR head of `find_byte`]. Verus on C: `1265 verified, 0 errors`; `cargo test -p eml_rt`: 920 passed [V].

### 10.3 Open items with defaults

| # | Item | Default |
|---|---|---|
| O-1 | TL-3b (with S-TL-12) fails `bench/rt-gate.sh` by one row, `find-bytes-1mib` +0.24 % (measured; sum -200.7 M) | Decided by the user on 2026-11-20: (a). TL-3b runs `bench/rt-gate.sh <TL-3a's commit> --allow 'find-bytes-1mib' --allow-bound 0.3`; it lands when that exits 0 with a negative gated sum. Any other row over +0.1 %, `find-bytes-1mib` over +0.3 %, or a non-negative sum stops the unit and goes back to the user with the table. The Close message quotes the approval |
| O-1b | (not used: O-1 was decided (a)) Roadmap item if (b) (EN): "Remove the per-worker page counts of `Control` (`taken`) and the worker number of `Pages`: nothing reads them. Deferred because the removal moves a `Heap` field offset; revisit when `eml_rt`'s layout changes for another reason." | written only under (b) |
| O-2 | The VM gate (10.1) stops on a row | the user decides with the tables; expected not to happen [V: measured without S-TL-12] |
| O-3 | A virtual-clock one-worker run with tasks no longer starts a monitor thread, and the monitor with a self-pipe never ticks; something relied on it | confirmed by the X2 ruling: every writer of a stop bit or a deferred signal rings the doorbell (`RunConfig.doorbell`'s contract) or is a running worker that ends the run; a failure on CI's other targets stops TL-2 |
| O-4 | `limit_memory_ends_the_run` reports a different function on another target | the stop site is `build` on every target: only `build` allocates after `before`, and each of its allocations is followed by its own self tail call, whose safepoint reads the stop word. If any run reports another function, TL-2 stops and reports the target, mode, seed and worker count; the snapshot is not re-accepted per target |
| O-5 | A docs commit merged after bb9d27a1 touched an anchored line | TL-1 re-counts and re-anchors (TL-1 step 4) |
| O-6 | The docs are not prototyped: a `citations::` failure or an SC-4 hit no item names | the unit that deletes the cited text rewords the citation; TL-4 owns every remaining hit outside the roadmap, TL-5 the roadmap |
| O-7 | Pushes | the orchestrator pushes `plan/task-limits` for the checkpoints; merging is asked at the end |

## Additional checks

- Read: CLAUDE.md, crates/eml_rt/CLAUDE.md, the design-level plan, X1 (F-3..F-6, sections 4 and 5), 00 (§A A-21, §D), 01 §3, §7, §11.5, §14, §18, §27; the par-total executable plan for the shape; the sources of section 3; `monitor.rs` (`bell_timeout`, `watch_loop`, `relay`), the worker exit paths in `workers.rs`, `eml_cli/src/main.rs` `gui_park`; the docs hits of SC-4 on bb9d27a1; roadmap 「進め方」 and 「段の列」 on bb9d27a1.
- Ran: in scratch worktrees at 3aba15e9 (removed afterwards, with their branches): the baseline `cargo nextest run`; the prototype of TL-2 and TL-3 with clippy, fmt, crate-graph, three feature clippies and `cargo nextest run` after each; Verus on base, prototype and C; `bench/run.sh` pairs and 15-round single rows; `bench/rt-gate.sh` (5 pairs) on X, Y and C; `objdump` of `rt_bench` and `rt_extern_bench` for X, Y and C; `cargo test -p eml_rt` on C.
- Corrections to X1: `const MEMORY` in D-5's default pattern also matches `Control::MEMORY` (kept), so SC-2 uses `MEMORY: Memory` and `P::MEMORY`; `Post::limit` is written `fn limit(&self` / `post.limit(`; `Control::taken` has readers in `eml_vm/src/sched.rs` too (all soft-limit); deleting it carries `Pages.slot` and `Heap::for_worker` with it, which needs S-TL-12 for the `eml_rt` gate.
- Not done: Miri, TSan, ASan and the cross-target clippy on the prototype (CP-A runs them); the docs rewrite; the VM gate with S-TL-12; the cause of `find-bytes-1mib`.
