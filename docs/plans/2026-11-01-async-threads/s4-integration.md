# Appendix S4: integration into units A0..A14 and C


[V] = verified (how is stated); [I] = inferred. Paths are relative to /home/user/eml at base 2c8de78. Appendix references: "S1 §x" is S1-response-r3 Part 1, "S2 §x" is S2-response-r2 Part 1, "S3 §x" is S3-response-r2 Part 1. AM-1/AM-2 and the X-ruling texts are applied as in §1.3. The unit IDs below (A0..A14, C) are new; the appendices' IDs (U1..U9, U6a/U6b, S3-x) are mapped in the table. (r2) marks what changed from r1. Part 2 answers S4-review-r1.

# Part 1: deliverable

## 1 Units table (summary)

### 1.1 Soundness rule and the order it forces (r2, B-1)

**Rule.** Every commit must be sound for any eml program at any worker count, not only for the enabled tests. No intermediate commit may let a word cross threads outside a path that exists for it.

**Why A2–A8 are sound.**
- From A2 to A8, every `Async` task is owned by the worker that ran `start`, which is always worker 0. A `par` child cannot call any `Async`, `Chan`, `Std.Atomic`, `Std.Net` or `Std.Signal` extern, because its row is `<never e>` and those externs are `<IO>` [V: std/Par.em:4-6, std/Atomic.em:10-14; S1 Q4 for `Async`/`Chan`].
- So no task, channel item, counter or driver op is touched off worker 0 until placement turns on in A9. Code added for the cross-worker paths in A3–A8 is reachable only on worker 0 or from lib unit tests.

**What A9 needs in place before placement turns on.** Every path a placed task can reach must already be cross-worker-correct:
- the run registry and lock (A3);
- notices, mail and signals (A4);
- the lock-free `Counter` (A5);
- channel reservations and the hold (A6);
- `RawBuf: Send` (A7);
- `Op` forwarding to a host plus the end-of-run order (A8).

From A9 on, I/O of every task is hosted by worker 0 for both clocks. A11 then gives every worker its own driver on the real clock. That changes speed, not soundness.

`Std.Tls` and `Std.Http` keep no per-worker state [I: crates/eml_vm/src/tls.rs uses only `cells::` line helpers, no `Scheduler` or worker access; grep].

### 1.2 Other order decisions

- **D-2.** `RawBuf: Send` (eml_rt) is its own unit A7, with the eml_rt gate.
- **D-3 (r2, B-2).** All of S1 Q7 (signals) lands in A4, together with two lib unit tests that run worker 1's collect against the mail (no threads). The reason: from A9 on, the existing `signal.rs` tests at N = 2 have their waiter on worker 1 [V: signal.rs:340-401].
- **D-4.** The `--workers 1` pin on the three `cli.rs` signal tests lands in A9, where placement makes them race [V: cli.rs:1155-1290 run without `--workers`].
- **D-5 (r2, B-4).** The UI harness unit A10 enables **all** `async/` files at once. Every cross-worker path exists at A9, so there is no `WORKERS_PENDING` and no interim testing.md sentence.
- **D-6.** The roadmap split is its own commit A1 [V: precedents a2b8094, and 3ec0c1c followed by e75b2a9].
- **D-7 (r2, C-1).** Every sentence in `docs/` and CLAUDE.md is changed in the commit that makes it false (§2.0 table).
  - The design-decision amendments 1-9 and 11 (multicore.md, async.md) stay in A13 (Cf-3). They record decisions for later stages, not current facts.
  - The status lines of multicore.md that state current facts (:72, :488, :523) move to A9.
  - status.md, architecture.md, overview, README and the roadmap stay in C, per roadmap 「進め方」 steps 3-6. Exceptions: A1, and A5's roadmap.md:344.
- **D-8.** `relay` lives in `Registry`, written once by worker 0 in `begin_run` [V: sched.rs:459-507: it depends on the monitor that `begin_run` starts].
- **D-9.** `chan_pingpong` is measured only by `scale.sh` (A13). S2 U7 step 13 is dropped. S2's decision rule is applied in §3.2.

### 1.3 Table

| ID | Title | Depends on | From | Main files |
|---|---|---|---|---|
| A0 | Plan async-threads | — | — | docs/plans only |
| A1 | Split `async-compute` from `async-threads` in the roadmap | A0 | S3-0 | docs/future/roadmap.md |
| A2 | Give each run one time base | A1 | S1 U1 | eml_io, eml_vm |
| A3 | Share one registry per run under one lock | A2 | S1 U2 | eml_vm cells.rs, sched.rs, lib.rs |
| A4 | Notices through the slots, the run mail, collects on every worker, signals | A3 | S1 U3 + Q7 | eml_vm cells.rs, workers.rs, sched.rs, monitor.rs |
| A5 | Make the `Counter` table lock-free | A4 | S2 U4, S3-4 | eml_vm counters.rs (new), ci.yml, CLAUDE.md, testing.md |
| A6 | Reserve channel room and items for the commit (one worker) | A5 | S2 U7 (code, unit tests) | eml_vm cells.rs, vm.rs, sched.rs, std/Chan.em (comments) |
| A7 | Make `RawBuf` `Send` | A6 | S1 U6a (part) | eml_rt driver.rs, lib.rs |
| A8 | Forward driver operations to a host; end runs in the cross-worker order | A7 | S1 U6a (forwarding, end order) | eml_vm cells.rs, workers.rs, sched.rs, lib.rs |
| A9 | Place `Async` tasks on every worker | A8 | S1 U5, S2 U7 (N ≥ 2 tests), S3-5 (Rust tests), S3-6a/S3-8 (cli), I-3 | eml_vm, eml_cli tests/cli.rs, ci.yml, CLAUDE.md, testing.md |
| A10 | Run the `async/` UI tests at 2 and 8 workers | A9 | S3-5 (harness), S3-6a/7/8 (enabling) | eml_cli tests, tests/ui/run/async/shield_boundary.em, testing.md, CLAUDE.md |
| A11 | Give every worker its own driver on the real clock | A10 | S1 U6a (rest), S3-6a (rest) | eml_vm, eml_cli tests/cli.rs, testing.md, CLAUDE.md |
| A12 | Add the worker-count measurement tools | A11 | S3-9a | bench/async/*, bench/net/*, benchmarks.md, CLAUDE.md, testing.md |
| A13 | Measure worker scaling on Linux; amend the design documents | A12 | S1 U9, S3-9b | benchmarks.md, multicore.md, async.md |
| A14 | Bind accepted streams to the first task that uses them (gated) | A13 | S1 U6b, S3-6b | eml_rt, eml_io, eml_vm, docs |
| C | Close async-threads | A13 (A14 if landed) | roadmap 「進め方」 1-9 | docs, CLAUDE.md, docs/plans removed |

After C the user makes the commit `Measure async-threads` (macOS tables, §3.3 step 2).

### 1.4 Consolidated conformance texts (override the S1 appendix where they differ)

- **G-A6 (X-ruling):** "… no `Ctx`/`Heap` call under the registry lock except what today's `start` (`fork_worker`, `share`) and the owner's `advance`/`settle_op` (allocation of `made` objects under `with_worker`) already do; Rust collection growth (`Vec`, `VecDeque`, `BTreeMap`) under the lock is allowed and is not a heap allocation in this sense."
- **G-A7 (AM-1):** `next_place` starts at 1. `placement_rotates_over_every_worker` expects owners 1,2,0,1,2,0 at N = 3.
- **`Finished.placed_tasks` (AM-2; S3 §5.1 text):** the number of `Async` tasks of this run whose `owner` (set once by `Registry::start`) differs from the physical worker that executed the `start`. It is counted under the registry lock in both modes and is not part of `RunStats`.
- **G-A9, interim and final:**
  - A8–A10: `k ≥ 1 ⇒ driver.is_none()` for every clock, so the host is always worker 0.
  - A11 on: the S1 form, Virtual only.
- **G-A13 check (I-3):** `signal.rs` asserts `terminated()`/`interrupted()` at N = 2, not exit codes [V: signal.rs:340-430]. eml_cli maps them to 143/130 [V: crates/eml_cli/src/main.rs:142-146]. A9 adds two real-binary tests at `--workers 2` with the task on worker 1. These assert 143 and 130 and use no waiter, so the result is deterministic.
- **G-A15 (X-ruling):**
  - Add `debug_assert!(sched.hold.is_none())` to each worker's exit sequence, where `Retired` is built (workers.rs:434-443).
  - Add `hold.is_none()` on worker 0 before `sweep_all`.
  - These assertions land in A6, where the hold is born. A8 places them in the new exit order.
- **G-A16:** verbatim from X-ruling-r1 I-1. S1 Q9 gains the channel-items clause.
- **G-A18:** TSan covers `workers::`, `signal::`, `io::`, `chan::` (all from A9), and `--lib counters::` (from A5). `async/` runs at N = 2/8 from A10, minus marks (§5 scope K2-d).
- **G-A20 (Cf-3):** amendments 1-9 and 11 are written in A13. Amendment 10 is spec text in object-model.md: its first sentence goes in A4, its second in A6. Amendment 6 includes the X-ruling hold sentence. Amendment 11 is S3 §5.7 「multicore.md 「テスト」」. C makes the roadmap `async-rescue` entry cite amendments 1-3.
- **S2's placement assumption A2 is replaced.** With AM-1, a consecutive pair of `start`s from the root at N = 2, with no earlier `start`, gets owners (1, 0). So `placed_tasks == 1` [I].
- **busy-self:** `busy.em` starts `server` first and `busy` second [V: busy.em:54-58]. So `busy` is owned by worker 0 at N = 2 and by worker 2 at N = 4, and S3 §5.6 holds as written.

### 1.5 Fields and signatures and where each comes from (r2, C-5)

The final constructor is `Run::new(workers: usize, bell: Option<Arc<Doorbell>>, clock: RunClock, counters: Arc<Counters>) -> Run`.

| Item | Kind | Introduced in | Notes |
|---|---|---|---|
| `RunClock { Real { epoch }, Virtual(Arc<AtomicU64>), Injected }`, `Run.clock` | type, `Run::new` argument | A2 | callers at lib.rs:881, sched.rs:622, vm.rs:3610, monitor.rs:214/261/297/322, workers.rs:669/736 [V grep]; tests pass a Virtual clock |
| `Shared { cells: Mutex<Registry>, mail: Mail }`, `Part`, `TaskCell.owner`, `OpRecord.{host, submitted, pending_op, arrival}`, `Woken`, `Route`, `Discards`, `IoWork` | types and fields | A3 (`Mail` is fed from A4) | `owner` is always the starting worker until A9 |
| `Registry.relay: bool` | field (not in `Run`) | A4 | written once in `begin_run` |
| `Notice::{Wake, Cancel}`, `MailItem` | variants | A4 | |
| `Run.counters: Arc<Counters>` | `Run::new` argument | A5 | `Machine.counters` → `Start` → `Run` |
| `ChanCell.{sends, recvs}`, `Scheduler.{hold, woken}`, `Hold` | fields | A6 | |
| `Notice::{Place, Submit, CancelIo, Completion}`, `Retired.left`, `Run::io_worker` (= 0) | variants and fields | A8 | `start` still owns on self |
| `Run.next_place: AtomicUsize` | field, initialised to 1 inside `Run::new`, **not** an argument | A9 | |
| `Finished.placed_tasks: u64`, `Registry.placed_tasks` counter | field | A9 | |
| `Retired.kept_pending` per worker, `Bind`, `Run::io_worker(k) = k` on the real clock | fields | A11 | |
| `RawSocket`, `Done::Accepted`, `IoDriver::adopt`, `Bind::Unbound` | eml_rt and eml_io types | A14 | |

### 1.6 Rules for every unit

- **G-0.** No plan-unit IDs, `docs/plans/` paths or appendix names in code, comments or `docs/`. Check: `git diff HEAD~1 -U0 -- . ':!docs/plans' | grep -nE '^\+.*(docs/plans|\b(U[0-9]b?|S3-[0-9]+[ab]?|A1[0-4]|A[0-9])\b)'` gives no output, or only hits the reviewer accepts as identifiers.
- Japanese text goes through the natural-japanese skill. Comments say why and cite `docs/` sections.
- **Common completion checks K1–K6** (§2 lists only the checks a unit adds):
  - **K1 (r2, A-1):** `cargo insta test --test-runner nextest --check` → exit 0, 0 failed, no pending snapshot.
    - This is testing.md:689's form with `--check` instead of `--accept`. It runs the workspace under nextest's default profile, including the `threads-required` and slow-timeout overrides in `.config/nextest.toml` [V].
    - It replaces a separate `cargo nextest run`.
  - **K2:** `git diff $BASE --stat -- crates/eml_cli/tests/snapshots crates/eml_vm/tests/bench.rs` → empty, where `$BASE` is the parent of A0.
  - **K3:** `cargo clippy --all-targets -- -D warnings && cargo fmt --check` → clean.
  - **K4** (units touching eml_vm or eml_io):
    - `cargo clippy -p eml_io -p eml_vm --all-targets --no-default-features -- -D warnings` → clean;
    - `cargo nextest run -p eml_io -p eml_vm -E 'test(tls_off::)'` → pass.
  - **K5:** the G-0 grep.
  - **K6 (r2, C-1):** for every §2.0 row of the unit, `git grep -n '<old phrase>' -- docs CLAUDE.md` → no output, and the new text is present.
- **Recording.**
  - Numbers and observations (timings, stress results, startup cost) go in the unit's commit message.
  - A file that falls under kind-2 scope K2-d is named, with its observed diff, in the commit message. It is also listed in the plan by a `Record … in the async-threads plan` commit right after the unit. That commit is a record, not the approval: the approval is the K2-d scope, approved in A0.
  - The U6b decision gets a `Record …` commit too [V: precedent 274ed86].

## 2 Unit details

### 2.0 Sentence ownership: every sentence that becomes false, and the unit that changes it (r2, C-1)

Line numbers are at 2c8de78 [V: `sed -n`]. Locate each by its phrase. "Old phrase" is the K6 grep string.

| Doc:line | Old phrase (grep) | False from | Unit | New content (rough; S1 §5, S2 §5.8 and S3 §5.7 texts where cited) |
|---|---|---|---|---|
| vm.md 「時計と driver」 | (addition) | — | A2 | the run has one time base; the real clock counts from the run's start; the virtual clock is one counter shared by the run's drivers |
| vm.md:599 | `登録表は \`Scheduler\` が持ち、\`vm::run\` ごとに作る` | A3 | A3 | one registry per run under the run lock, shared by every `Scheduler`; timer wheel and op count per worker partition |
| vm.md:~610 (セルが持つ語) | `登録表を \`Scheduler\` と一緒に` | A3 | A3 | error/`exit` runs drop the run registry after the join without touching the heaps |
| vm.md:801 | `スコープの lock の下で行う` | A3 | A3 | 「run の登録表の lock の下で行う」 |
| vm.md:876 (first clause) | `\`Registry\` は worker 0 が持ち` | A3 | A3 | the registry is one per run under the run lock, and the channel cells are in it. The future-tense rest is rewritten in A6 |
| vm.md:400 | `知らせは2種類で` | A4 | A4 (Wake, Cancel), A8 (Place, Submit, CancelIo, Completion) | S1 §5 row |
| vm.md:409 | `受け箱に知らせを積んでから` (monitor) | A4 | A4 | the monitor pushes `Signal` into the mail and raises `NOTICE` on every word |
| vm.md:504 | `受け箱の知らせだけを処理し、OS も時計も見ない` | A4 | A4 | every worker collects the same way; worker 0 alone advances virtual time and declares deadlock after the slot-lock re-check |
| vm.md:~607 (`Task a` value) | `登録表の受け箱を持つ` | A4 | A4 | the handle holds the run mail |
| vm.md:609 | `受け箱 \`Inbox\` は` | A4 | A4 | mail plus slot notices; ready queue owner-only, unchanged |
| vm.md 「上限と割り込み」, stdlib.md:1560 | (signal delivery wording; check) | A4 | A4 | any collecting worker delivers; an unheard signal raises every worker's word |
| object-model.md 「論理的な worker ID」 | (addition) | A4 / A6 | A4 / A6 | X-ruling amendment-10 sentence 1 (A4), sentence 2 (A6) |
| stdlib.md:1258 | `複数の worker で動くときは` | A5 | A5 | S2 §5.8 |
| vm.md:606, :698 | `\`counters\` も持つ`, `\`Machine\` の表 \`counters\`` | A5 | A5 | S2 §5.8 |
| roadmap.md:344 | `表を \`SeqCst\` の atomic にする` | A5 | A5 | 「見直す条件: `Counter` を大量に作るプログラムが出たとき。」 |
| vm.md:604, :690-694 rows, 「channel」 :733-739 | `items.len() < cap + receivers` | A6 | A6 | S2 §5.8 (reservations, hold, S-a, S-b) |
| vm.md:876 (rest) | `動かすときは、登録表を run の lock の下に置き` | A6 | A6 | present tense: reservation counts in `ChanCell`, hold per worker, `Woken` to the owner |
| vm.md:870 item 4 (channel clause) | `\`push_raw\` の \`share\`` | A6 | A6 | `publish`/`receive` in the arms outside the lock (S2) |
| stdlib.md 「`Std.Chan`」 :1198-1199 | `0 を返した` | A6 | A6 | S2 §5.8; S-a/S-b bullets |
| vm.md:483 | `\`Send\` ではなく、driver を持つスレッドから出さない` | A7 | A7 | S1 §5 text |
| object-model.md:1303 (T10) | `別のスレッドからは読ませない` | A7 | A7 | the driver's thread may use the lent memory under the same lifetime rule |
| vm.md:492 | `driver を持つのは worker 0 の \`Scheduler\` だけ` | A8 | A8 (interim), A11 (final) | A8: the driver is on worker 0; an operation of a task owned elsewhere is submitted by worker 0 and settled by its owner. A11: per-worker drivers on the real clock. The text must not contain `worker 0 の上だけ` (B-5) |
| vm.md 「誤り、止めること、終わり」 | (end-of-run order) | A8 | A8 | G-A15 order; A11 adds the per-worker quiesce |
| vm.md:870 item 4 (lead) | `経路は、\`par\` の右の子を置くことだけである` | A8 | A8 | adds `Place` (closure), mail discards after the owner's last use, `Op`s carry no words |
| object-model.md:815-818 | (recovery) | A8 | A8 | one sentence: the registry of an error run is dropped untouched before recovery |
| vm.md:590 | `worker 0 の上だけで動く` | A9 | A9 | placement round-robin from worker 1; a task never moves |
| vm.md:402-403 | `最初の \`par\` の fork で` | A9 | A9 | also the first `start` that places |
| vm.md:577 (exit on k) | (check) | A9 | A9 | a worker k ≥ 1 that ends the run by `exit` calls `end_run` before waking |
| vm.md 「`RunStats`」 or where `Finished.placed` is described | (addition) | A9 | A9 | `placed_tasks` |
| vm.md:859 | `\`Async\` のタスクを複数の worker で動かすことと` | A9 | A9 | present tense |
| stdlib.md:21 | `\`par\` の右の子を走らせる worker` | A9 | A9 | also `Async` tasks |
| stdlib.md:910 | `1つの worker の上でタスクを動かす` | A9 | A9 | 「複数の worker の上で」 |
| stdlib.md 「`Std.Chan`」 | (addition) | A9 | A9 | 「送り手と受け手は別の worker で動いてよい」 |
| testing.md:417 (last two sentences) | `\`Async\` のタスクは worker 0 の上だけで動き` | A9 | A9 → A10 | A9 drops the reason clause (`async/` is still not run at N ≥ 2); A10 replaces the bullet (S3 §5.7) |
| testing.md 「CLI のテスト」, 「eml_vm のテスト」, 「AddressSanitizer」, 「ThreadSanitizer」, 「CI」 | (additions) | A9 | A9 | S3 §5.7 texts (cli pin; workers.rs, io.rs, signal.rs, chan.rs bullets; ASan `io:: workers::`; TSan `io:: chan::`) |
| CLAUDE.md:94 (eml_vm row) | `\`Async\` tasks, channels and the I/O driver on worker 0` | A9 | A9, A11 | A9: tasks placed round-robin over the workers (workers start at the first fork or placing start); their I/O is hosted by worker 0's driver. A11: a driver per worker on the real clock, worker 0's only on the virtual clock |
| CLAUDE.md:117 | `Only \`Async\` tasks, channels and the I/O driver are pinned to worker 0` | A9 | A9, A11 | same two steps |
| CLAUDE.md ASan/TSan/Miri lines | (filters) | A5, A9 | A5, A9 | S3 §5.7 |
| multicore.md:72, :488, :523 (status sentences) | `最初の \`par\` の fork で`, `worker 0 だけで動く`, (523: the "Async on one worker" clause) | A9 | A9 | present facts (D-7) |
| testing.md 「UI テスト」, 「seed で流す」, 「Windows で流さないテスト」; CLAUDE.md:153 | `\`workers_runs_cover_par_only\``, `the \`par/\` ones at 2 and 8 workers` | A10 | A10 | S3 §5.7 (no interim sentence, D-5) |
| testing.md io.rs and CLI bullets | (additions) | A11 | A11 | echo across workers; `--workers 4` run |
| benchmarks.md, CLAUDE.md:53, testing.md 「よく使うコマンド」 | (tools) | A12 | A12 | S3 §5.7 |
| multicore.md amendments 1-9, 11; async.md:244, :249-250 | — | — | A13 | S1 §5 and X-ruling texts |
| status.md:36, :159; architecture.md:316-317; roadmap; overview/README | — | A9 | C | 「進め方」 steps 3-6 allow the close |

### A0 Plan async-threads

- Files: `docs/plans/2026-11-01-async-threads.md`, the plan in Japanese, written by the orchestrator. It holds §1.4, §1.5, the unit list, the kind-1/2 list with the K2-d scope (§5), the risks and the "user to confirm" close criterion.
- Directory `docs/plans/2026-11-01-async-threads/`:
  - `s1-concurrency.md`: S1 r3 Part 1, with AM-1, AM-2 and the X-ruling edits applied;
  - `s2-chan-counter.md`;
  - `s3-tests-measure.md`;
  - `s4-integration.md` (this file);
  - `probes/` with copies of probes/{spread.em, spread_par.em, serve.em, onetask.em, onefork.em, hello.em}.
- Completion:
  - `git show --stat HEAD` → only paths under `docs/plans/`.
  - `cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` → 4 passed. `docs/plans` is skipped [V by review: citations.rs:38].

### A1 Split `async-compute` from `async-threads` in the roadmap

- S3 §5.7 「S3-0」: remove `compute` from the :31 entry and insert the `async-compute` entry directly after it, with 「前提: `async-threads`」. Its own commit (D-6). The commit message gives the reason: `compute` needs a `par` job issued from a task, which does not depend on the cross-worker registry.
- Completion:
  - `grep -c 'async-compute' docs/future/roadmap.md` → 1.
  - `sed -n '/^- `async-threads`/p' docs/future/roadmap.md | grep -c 'compute'` → 0.
  - citations → 4 passed.

### A2 Give each run one time base (S1 U1)

- What it does: S1 §5 and §1.5. `RealDriver::new_at`, `TestDriver::new_shared`, `TestDriver::clock`. `RunClock` and `Run.clock`. `Run::now_ms_*` becomes the only source of VM time.
- Tests: eml_io unit tests `a_driver_made_at_an_earlier_epoch_counts_from_it` (`new_at(now - 10 s).now() >= 10 s`) and `test_drivers_on_one_clock_see_each_others_waits` (`a.wait(Some(7))`, then `b.now() == 7 ms`). `Run::new` call shapes are kind 3.
- Completion:
  - K1–K6.
  - `cargo nextest run -p eml_io -E 'test(a_driver_made_at_an_earlier_epoch_counts_from_it) | test(test_drivers_on_one_clock_see_each_others_waits)'` → 2 passed.
  - `grep -rn '\.now()' crates/eml_vm/src` → only inside `Run::now_ms_*` and the driver constructors.
- G-A to check: G-A8, G-A5.

### A3 Share one registry per run under one lock (S1 U2)

- What it does: S1 §5 types. `Arc<Shared>` is created in `run_scheduled` and cloned into every `Scheduler`, with partitions, `owner`, `Woken`/`Route`, `Discards`/`IoWork` and the record states. Everything still runs on worker 0.
- Spec: §2.0 rows for A3.
- Tests: cells.rs `a_cancel_and_a_submit_are_ordered_by_the_lock` (S1 U2, table-driven, no threads).
- Completion:
  - K1–K6.
  - `cargo nextest run -p eml_vm --lib -E 'test(cells::a_cancel_and_a_submit_are_ordered_by_the_lock)'` → 1 passed.
  - `cargo nextest run -p eml_vm --test integration -E 'test(tasks::the_task_cells_import_only_std_and_the_runtime)'` → 1 passed.
  - `grep -nE 'Mutex|Atomic' crates/eml_vm/src/cells.rs` → only `Shared.cells`, `Mail`, `HandleBin` (G-A1).
  - Miri `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --lib -- cells::a_read cells::a_write`, then the same with `-Zmiri-tree-borrows` added → pass.
- G-A to check: G-A1, G-A3, G-A5, G-A6, G-A10 (section rule).

### A4 Notices through the slots, the run mail, collects on every worker, signals (S1 U3 + Q7)

- What it does: S1 Q3, Q6 (a)-(d), Q7 and §1.5. `Notice::{Wake, Cancel}`. `Mail` with `MailItem`. `mark` routing. The S1/S2/S3 collect sections on every worker. The deadlock re-check, including `mail.pending` and `NOTICE`.
- Q7 in full:
  - `Signal` mail items;
  - `NOTICE` on all words and `wake_all`;
  - first-collector delivery;
  - `unheard` → all words;
  - `set_watched` under the lock with `Registry.relay`.
- Mail discards run under `with_worker(X)` after unlock.
- Spec: §2.0 rows for A4.
- Tests:
  - cells.rs `a_handle_dropped_under_the_registry_lock_does_not_deadlock` and `mail_keeps_arrival_order`.
  - workers.rs `mod tests` `a_mail_item_pending_at_the_deadlock_check_defers_the_verdict`.
  - (r2, B-2) Two `mod tests` cases in the module that holds the collect (sched.rs), driven by hand with one `Run` of 2 slots, one `Shared`, and the collect of a `Scheduler` with index 1 (no threads):
    - `a_signal_drained_by_worker_1_wakes_a_waiter_owned_by_worker_0`: a waiter cell owned by 0 waits on `Terminate`; push `MailItem::Signal(TERMINATE)`; worker 1 collects → slot 0 holds `Notice::Wake` for that cell, `unheard == 0`, and `set_watched` was called with the waiter bits cleared.
    - `an_unheard_signal_drained_by_worker_1_raises_every_word`: no waiter; worker 1 collects → `control.worker_bits(k) & TERMINATE != 0` for k = 0 and 1.
  - Kind 3: monitor tests' call shapes.
- Completion:
  - K1–K6.
  - `cargo nextest run -p eml_vm --lib -E 'test(cells::a_handle_dropped_under_the_registry_lock_does_not_deadlock) | test(cells::mail_keeps_arrival_order) | test(workers::tests::a_mail_item_pending_at_the_deadlock_check_defers_the_verdict) | test(sched::tests::a_signal_drained_by_worker_1_wakes_a_waiter_owned_by_worker_0) | test(sched::tests::an_unheard_signal_drained_by_worker_1_raises_every_word)'` → 5 passed.
  - `cargo nextest run -p eml_vm --test integration -E 'test(signal::)'` → 10 passed.
  - `grep -n 'cells' crates/eml_vm/src/monitor.rs` → no `Shared.cells` access.
  - TSan `nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=thread TSAN_OPTIONS=halt_on_error=1 CARGO_TARGET_DIR=target/tsan cargo test -Zbuild-std --target x86_64-unknown-linux-gnu -p eml_vm --test integration -- workers:: signal:: preempt::` → exit 0.
- G-A to check: G-A2, G-A3, G-A4, G-A5, G-A6, G-A12, G-A13, G-A16 (mail).

### A5 Make the `Counter` table lock-free (S2 U4 + S3-4)

- What it does: S2 §5.1; `Run::new` takes its final form (§1.5).
- CI/doc (S3-4):
  - the ci.yml Miri `counters::` step and the TSan `--lib counters::` step;
  - CLAUDE.md TSan and Miri lines;
  - testing.md 「ThreadSanitizer」, 「Miri」, 「CI」, 「よく使うコマンド」.
- Spec and comments: §2.0 rows for A5; eml_extern lib.rs:253 and std/Atomic.em:4 comments.
- Tests: 5 `counters::` unit tests.
- Completion:
  - K1–K6.
  - `cargo nextest run -p eml_vm --lib -E 'test(counters::)'` → 5 passed.
  - `grep -n "counters\|fn counter" crates/eml_vm/src/cells.rs` → no output.
  - TSan `… -p eml_vm --lib counters::` → exit 0.
  - Miri `… cargo miri test -p eml_vm --lib -- counters::` under both settings → pass.
  - `nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- .github/workflows/ci.yml` → no output.
- G-A to check: G-A17 (Counter), G-A1, G-A5.

### A6 Reserve channel room and items for the commit (r2, B-1; S2 U7 code)

- What it does: S2 §5.2-5.4 in full:
  - `sends`/`recvs`;
  - `Hold`;
  - one registry section per channel instruction;
  - `publish`/`receive`/`fork_worker`/`retire_worker` outside the lock;
  - `Woken` routing through `route`, with `Run::send` when owner ≠ self (unreachable until A9);
  - the hold assertions at `switch_away`, `preempt`, the worker exit and before `sweep_all`.
- Behaviour at N = 1 is identical (S2 §2).
- Spec: §2.0 rows for A6; std/Chan.em:54-55 and :144-146 comments.
- Tests: the 11 cells.rs unit tests of S2 §5.6. Tasks are started with explicit owners, so the owner-1 cases run without threads. The 11 existing `chan::` tests are unchanged.
- Completion:
  - K1–K6.
  - `cargo nextest run -p eml_vm --lib -E 'test(cells::)'` → all pass, including the 11 new tests.
  - `cargo nextest run -p eml_vm --test integration -E 'test(chan::)'` → 11 passed.
  - `git diff HEAD~1 -- crates/eml_vm/tests/chan.rs` → empty.
  - S2 U7 criteria 10 and 11 (the channel-section greps) → as S2 states.
- G-A to check: G-A6, G-A15 (hold), G-A16, G-A17, G-A5.

### A7 Make `RawBuf` `Send` (eml_rt)

- What it does:
  - `unsafe impl Send for RawBuf {}` in eml_rt driver.rs, and the doc comment's rule is rewritten (S1 Q5).
  - A static `Send` assert next to the existing type asserts in eml_rt lib.rs.
  - `unsafe impl Send for` is on the allow-list [V: trust.rs:242-247]. Scratch check [V]: with the line added, `cargo test -p eml_rt --lib trust::` → 6 passed; reverted.
- Spec: §2.0 rows for A7.
- Completion:
  - K1–K6.
  - eml_rt gate: `cargo verus verify -p eml_rt` → verified, 0 errors.
  - `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` → exit 0.
  - `cargo nextest run -p eml_rt -E 'test(trust::)'` → 6 passed.
- G-A to check: G-A19 (the type).

### A8 Forward driver operations to a host; end runs in the cross-worker order (r2, B-3; placement still off)

- What it does:
  - (a) `Notice::{Place, Submit, CancelIo, Completion}` and their handling. `start` keeps `owner = self`, with the `owner ≠ self → Run::send(owner, Place)` branch present in the code.
  - (b) S1 Q5 steps 1-5, with `Run::io_worker(k) = 0` for every clock.
  - (c) S1 Q8 steps 1-3:
    - the registry-state check in `main`'s `Finish`;
    - `END` handled only at a safepoint or the idle wait;
    - the final collect that counts `left`;
    - `Retired.left`;
    - the hold assertion in the exit sequence;
    - worker 0 quiesces after the join and records arrivals.
  - (d) Error and `exit` runs drop the registry untouched.
  - (e) G-A9 interim assertion.
- Spec: §2.0 rows for A8.
- Tests (lib, no threads):
  - workers.rs `mod tests` `a_notice_after_the_last_collect_is_counted_as_left` (S1).
  - cells.rs or sched.rs `a_forwarded_read_is_submitted_by_the_host_and_settled_by_the_owner`: a `Registry`, a `Run` of 2 slots and a `TestDriver` on host 0. A task cell owned by worker 1 issues a read on a `localhost` stream bound to host 0. Owner 1 `queue_op` → slot 0 gets `Notice::Submit`. Host 0 `take_queued` → `driver.submit` → completion → `arrive` returns `Some(1)` → slot 1 gets `Notice::Completion`. Owner 1 `settle_op` → `Done`, and `take_op` returns the bytes.
  - `a_cancel_before_the_host_submits_is_seen_by_the_host`: owner `cancel_io` before `take_queued` → `take_queued` returns `cancel_now == true`, and the arrival is `cancelled`.
- Completion:
  - K1–K6.
  - The three lib tests → 3 passed.
  - `cargo nextest run -p eml_vm --test integration -E 'test(workers::) | test(io::) | test(signal::) | test(machine::)'` → pass (par runs at N = 2/8 unchanged).
  - TSan as A4 → exit 0.
  - `nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run -- -D warnings` → clean.
- G-A to check: G-A4, G-A6, G-A9 (interim), G-A10, G-A15, G-A16, G-A19 (`Op` built by the owner).

### A9 Place `Async` tasks on every worker

- What it does:
  - `Run.next_place = 1` (AM-1);
  - `owner = next_place.fetch_add(1) % N` when `places()`;
  - the sentinel comes first in both start arms;
  - `placed_tasks` (AM-2);
  - `exit` on k ≥ 1 calls `end_run` before `wake_all` (G-A14).
- Spec, docs, CLAUDE.md: §2.0 rows for A9.
- CI: TSan `-- workers:: signal:: preempt:: io:: chan::`; ASan `-- api:: signal:: io:: workers::` (Cf-4). Plus the CLAUDE.md comments and testing.md.
- Tests added:
  - workers.rs `mod tests`: `placement_rotates_over_every_worker`.
  - tests/workers.rs: `the_first_start_starts_the_workers`, `a_start_after_a_nested_run_starts_the_workers`, `exit_in_a_task_on_another_worker`, `a_timer_on_worker_1_advances_the_virtual_clock`, `handles_dropped_on_every_worker_are_freed_once`, and `the_fast_mode_runs_tasks_on_four_workers` (fib, `cancel.em`, `net_echo.em`; Cf-1 resolved).
  - tests/io.rs: `a_forwarded_submit_is_settled_by_its_host` and `an_op_record_left_on_worker_1_is_swept`.
  - tests/chan.rs, section 「worker をまたぐ channel」: S2's 6 tests.
  - tests/signal.rs: `a_pending_signal_reaches_a_waiter_on_worker_1`.
  - cli.rs (I-3), `#[cfg(unix)]`, `--workers 2`: `an_unheard_terminate_stops_a_program_with_a_task_on_worker_1_with_143` and `an_unheard_interrupt_stops_a_program_with_a_task_on_worker_1_with_130`. Program: `main () = Async.run (fn () -> let t = Async.start (fn () -> eprintln "ready"; Async.sleep_ms 600000); Async.await t)`. The test reads `ready`, sends the signal, and expects the code within `DEADLINE`.
- Tests changed:
  - Kind 2: `workers::exit_abandons_a_child_on_another_worker` and `workers::a_timer_on_worker_0_and_a_child_on_worker_1_agree` (§5).
  - Kind 3: `--workers 1` on `cli::a_terminate_reaches_a_waiting_task`, `cli::an_interrupt_after_a_delivered_one_ends_the_program_with_130` and the helper `a_second_signal_ends_a_program_whose_waiter_cannot_be_reached` (two `#[test]`s), each with the doc sentence 「worker を1つに固め、`yield_now` で待ち手に順を渡す」.
  - Kind 3: the doc comment of `signal::a_pending_signal_reaches_a_waiter_on_two_workers` (the waiter is now on worker 1).
  - Kind 3: `Finished` literals gain `placed_tasks: 0`.
- Completion:
  - K1–K6.
  - `cargo nextest run -p eml_vm --test integration -E 'test(workers::)'` → 22 passed (16 [V] + 6).
  - `… -E 'test(io::)'` → 24 passed (22 [V] + 2).
  - `… -E 'test(chan::)'` → 17 passed.
  - `… -E 'test(signal::)'` → 11 passed.
  - `cargo nextest run -p eml_vm --lib -E 'test(workers::tests::placement_rotates_over_every_worker)'` → 1 passed.
  - `cargo nextest run -p eml_cli --test integration -E 'test(cli::)'` → pass; the two new tests are 2 passed on unix.
  - `for i in $(seq 20); do cargo nextest run -p eml_vm --test integration -E 'test(workers::) | test(io::) | test(signal::) | test(chan::)' || exit 1; done` → 20 passing runs.
  - TSan (final filter) → exit 0.
  - ASan `nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=address CARGO_TARGET_DIR=target/asan cargo test -Zbuild-std --target x86_64-unknown-linux-gnu -p eml_vm --test integration -- api:: signal:: io:: workers::` → exit 0, and `… -p eml_io` → exit 0.
  - Windows cross clippy → clean.
  - actionlint → no output.
  - `cargo nextest run --profile timing -j 1 -p eml_vm --test integration -E 'test(workers::) | test(io::) | test(chan::) | test(signal::)'` → no SLOW, no failure.
  - Startup re-measurement (S3 §5.6; `onetask.em` vs `onefork.em`; release; median of 21 at `--workers 1/4/64`) in the commit message. Expected w4 − w1 ≤ 2 ms; a miss is recorded, not blocking.
  - The A13-style greps for the §2.0 A9 rows → no output.
- G-A to check: G-A3, G-A5, G-A7, G-A12, G-A13, G-A14, G-A16, G-A17, G-A18 (TSan set).
- Left to the implementer: where `placed_tasks` is counted.
- If the diff is too large to review, split it into (i) the placement code plus spec, CLAUDE.md and cli pins plus the workers.rs, io.rs and kind-2 tests; and (ii) the chan.rs, signal.rs and cli 130/143 tests plus CI. (i) must pass K1 by itself: it contains every code change, and (ii) adds tests only.

### A10 Run the `async/` UI tests at 2 and 8 workers (S3-5 harness; all files; D-5)

- What it does:
  - S3 §5.1 in its final form: no `WORKERS_PENDING`, and `workers_runs_cover_par_and_async` final.
  - The `workers:` mark; `Marks`; `marks_of(…, run_entry)`; the `for_each_file` signature (kind 3); `Compare::Nothing` for `async/`; the `workers ≥ 1 && placed_tasks > 0` check.
  - The `WORKER_COUNTS`/`workers_run` doc comments are rewritten.
  - `shield_boundary.em` line 55 → `Async.sleep_ms 10` (kind 2).
  - Enabled: all 69 run and 12 run-fail `async/` files [V: ls], minus any K2-d marks.
- Doc: testing.md 「UI テスト」 (2 bullets), 「seed で流す」 (testing.md:417 replaced by S3's text; the new rules bullet), 「Windows で流さないテスト」; CLAUDE.md:153.
- Tests:
  - Kind 1: delete `ui::workers_runs_cover_par_only`.
  - Added: `workers_runs_cover_par_and_async`, `a_workers_mark_outside_par_and_async_fails`, `a_workers_mark_in_a_transcript_fails`, `the_workers_mark_needs_a_reason`, `an_async_file_that_places_no_task_fails`.
- Completion:
  - K1–K6.
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::)'` → pass.
  - `cargo nextest list -p eml_cli --test integration | grep -c workers_runs_cover_par_only` → 0.
  - `git diff $BASE -- tests/ui/run/async/shield_boundary.em` → exactly one changed line.
  - Stress (S3 §5.5 step 3): 200 iterations each for `shield_boundary` and the two drain files; 50 each for every `net_*`, `chan_*`, `signal_*` file and `counter`. Expected: no diff, or K2-d applied.
  - `git grep -n '^-- workers: skip (' -- tests/ui` → exactly the K2-d marked files, at line 1.
  - S3 §5.5 steps 1-2: `cargo nextest run --profile timing -j 1 -p eml_cli --test integration -E 'test(=ui::run) | test(=ui::run_fail)'` → 2 passed; every file − `hello.em` < 1000 ms (otherwise S3 §5.5 step 4 under K2-d). `ui::run` wall time before and after in the commit message.
- G-A to check: G-A5, G-A12 (deadlock files at N = 2/8), G-A13 (`signal_*` UI), G-A18.

### A11 Give every worker its own driver on the real clock (S1 U6a rest + S3-6a rest)

- What it does:
  - `Run::io_worker(k) = if Virtual { 0 } else { k }`;
  - lazy `RealDriver::new_at(epoch)` on k, with `Slot.waker` set once;
  - `Bind::Bound{host, …}`: `listen`/`connect` bound to the owner's worker, accepted streams to the accepting host;
  - waits in the driver on k;
  - per-worker quiesce, arrivals, drop, `kept_pending` → poisoned;
  - G-A9 in its final form.
- Spec, docs, CLAUDE.md: §2.0 rows for A11 (vm.md:492 final, end-of-run quiesce; CLAUDE.md:94/117 driver clause; testing.md io.rs and CLI bullets).
- Tests: io.rs `an_echo_between_two_workers_completes` (Real, N = 2, lower bounds, 3 s watchdog); cli.rs `eml_run_spreads_tasks_over_four_workers` (S3 §5.4).
- Completion:
  - K1–K6.
  - `cargo nextest run -p eml_vm --test integration -E 'test(io::)'` → 25 passed.
  - `cargo nextest run -p eml_cli --test integration -E 'test(cli::eml_run_spreads_tasks_over_four_workers)'` → 1 passed.
  - 20-loop as A9 → 20 passing runs.
  - TSan and ASan as A9 → exit 0.
  - Windows cross clippy → clean.
  - `timeout 20 cargo run -p eml_cli -- run --workers 4 tests/ui/run/async/net_echo.em` → its snapshot's stdout, exit 0.
  - `grep -n 'worker 0 の上だけ' docs/spec/vm.md` → no output (B-5).
- G-A to check: G-A8, G-A9, G-A10, G-A11 (pre-A14), G-A15.

### A12 Add the worker-count measurement tools (S3-9a)

- Files:
  - `bench/async/spread.em`, `spread_par.em` (from probes);
  - `bench/async/chan_pingpong.em` (S2 §5.6; prints `800040000`);
  - `bench/async/scale.sh`;
  - `bench/net/serve.em`, the `serve-self-32` cases.tsv row;
  - `bench.py --workers`, the `report.py` table.
- Doc: benchmarks.md intro, 件 and `scale.sh` paragraph; CLAUDE.md commands; testing.md 「よく使うコマンド」.
- Completion:
  - K1–K6.
  - S3-9a criteria 1-9 with S3's expected outputs.
  - `target/release/eml run --workers 4 bench/async/chan_pingpong.em` → `800040000`.
  - `bench::every_program_in_bench_has_a_test` → pass [V: bench.rs:108-118, top-level `.em` only].

### A13 Measure worker scaling on Linux; amend the design documents (S1 U9 + S3-9b)

- Measurement: S3 §5.6 commands verbatim (`scale.sh`; `bench/net.sh … --workers 1,2,4 --no-strace --only '^(base|serve-self-32|conn-self-32|busy-self|echo-self-fresh)$'`; `report.py`).
- benchmarks.md heading `### worker の数と実時間`: machine, commit, commands, both tables verbatim, one sentence per row, the close-criterion check, the U6b decision, and the `chan_pingpong` decision (§3.2).
- Design docs (G-A20):
  - S1 amendments 1-9: multicore.md:124, :126+:160, :157, :128-129, :115, :336 (with the X-ruling hold sentence), :439; async.md:244, :249-250.
  - Amendment 11: multicore.md 「テスト」, S3 §5.7.
- Completion:
  - K1–K6.
  - `grep -n '^### worker の数と実時間' docs/implementation/benchmarks.md` → 1 line.
  - Validity: S_hw(2) ≥ 1.6 and S_hw(4) ≥ 2.4 in the recorded run. Otherwise re-run on another machine, or record and ask the user.
  - **Close criterion (user to confirm): S_spread(N) ≥ 0.80 × S_hw(N) for N = 2 and 4.** If it is missed, the stage does not close (S3 §5.6 diagnosis).
  - The U6b decision goes in a `Record …` commit.
  - `grep -n 'worker 0 だけで動く\|worker 0 の上だけ' docs/future/multicore.md docs/future/async.md` → no output.

### A14 Bind accepted streams to the first task that uses them (gated; S1 U6b + S3-6b)

- Gate (S3 §5.6). Land A14 if either holds:
  - (a) `conn-self-32` w4÷w1 ≤ 0.90 with 差, while `serve-self-32` w4÷w1 > 0.90 or 差なし;
  - (b) `echo-self-fresh` w2÷w1 ≥ 1.10 with 差.

  Otherwise skip A14, and C writes S3 §5.7 roadmap item 4.
- What it does:
  - `RawSocket` (`Send`, asserted), `Done::Accepted`, `IoDriver::adopt` (eml_rt);
  - the backends and the test driver (eml_io);
  - `Bind::Unbound` until the first `submit_*`, then bound to that owner's worker;
  - under IOCP, accepted streams stay bound to the accepting host (S1 A4 fallback, `cfg(windows)`).
- Spec: vm.md, accepted-stream binding and the Windows sentence if the fallback is used. (r2, B-6) A second edit of async.md:244/:249 (amendments 8/9) if A13's text names the accepting host.
- Tests: io.rs, an accepted stream used first on another worker (Real, N = 2); eml_io `adopt` tests per backend.
- Completion:
  - K1–K6.
  - eml_rt gate as A7.
  - Windows cross clippy → clean.
  - TSan/ASan as A9 → exit 0.
  - Re-run `bench/net.sh … --only '^(base|serve-self-32|echo-self-fresh)$'` and add the second table.
  - (r2, B-6) Re-run `bench/async/scale.sh`, and re-check the validity condition and the close criterion on the A14 tree. Record both verdicts in the heading.
- G-A to check: G-A11, G-A19.

## 3 Plan commit and close unit

### 3.1 A0 and A1

See §2. The plan commit holds `docs/plans/` only; the roadmap split is a separate commit [V: precedents].

### 3.2 Decisions from A13 that feed C

- **U6b:** the gate in §2 A14.
- **`chan_pingpong`:** if t(4) > t(1) in `scale.sh`, roadmap.md:285 (「ロックフリーの受け箱と channel」) 見直す条件 becomes 「N が 8 以上で競合を測ったとき。`bench/async/chan_pingpong.em` は worker 4 つで 1 つより遅い ([計測の基準](../implementation/benchmarks.md) の「worker の数と実時間」)。同じインターフェースの後ろで替える。」. Otherwise it is unchanged.
- **`par` plateau:** only if S_par(4) ≈ S_par(2) on the measuring machine (S3 §5.7 item 6).
- **K2-d marks for P3 dependences:** if there are any, roadmap item 1 (S3 §5.7).

### 3.3 C: `Close async-threads` (one commit; roadmap 「進め方」 steps 1-9; r2, A-2)

1. **Spec.**
   - `grep -rn 'async-threads' docs/spec` → none.
   - `grep -nE 'worker 0 の上だけ|1つの worker の上でタスク|複数の worker で動かすときは|driver を持つのは worker 0 の' docs/spec` → none.
   - Every §2.0 row is in its new form. No 「今は〜、後で〜」 is left about placement.
2. **Measurement.**
   - The in-stage Linux numbers are in benchmarks.md (A13).
   - The 「今の数値」 tables (`bench/run.sh`, `bench/rt.sh`, `bench/scripts.sh`) are macOS-only. The cloud session cannot run them, so the user commits `Measure async-threads` directly after C. In that commit the user runs `bench/scripts.sh` first, and makes a separate `Resize bench rows` commit if any row leaves its range.
   - `bench/scripts.sh` stays unpinned (S3 §6 item 6).
   - Findings that rest on "Async tasks on worker 0 only" are re-measured or removed: `grep -n 'worker 0' docs/implementation/benchmarks.md`, then review.
3. **status.md:** :36 (tasks on every worker; cross-worker channels and counters); :159 (`race` winner: one run registry); the deferred items' user-visible limits.
4. **architecture.md:** `counters.rs`, `Shared`/`Mail`, the run registry, placement, per-worker drivers (:316-317).
5. **roadmap.md:**
   - Remove :31.
   - :32 `async-rescue` → 「前提: なし」, and its 設計 cites the multicore.md sections rewritten by amendments 1-3 (G-A20).
   - :33, :49 and the `async-compute` entry → 「前提: なし」.
   - :324 → 「  - 見直す条件: channel を多く使うプログラムのプロファイルに取り込みの歩きが見えたとき。」. This is the only edit to :324: S2's item is :283-285 [V].
   - :285 per §3.2. :344 is already final from A5.
   - Deferred items: S3 §5.7 items 2, 3 and 5 always; item 1 per §3.2; item 4 only if A14 was skipped; item 6 per §3.2.
   - Criteria: `grep -c 'async-threads' docs/future/roadmap.md` → 0, and `grep -rn 'async-threads' docs CLAUDE.md --include=*.md | grep -v '^docs/plans/'` → none.
6. **overview.md / README 「文書の地図」:** change only rows that name worker 0 for `Async` (`grep -n 'worker 0\|Async' docs/overview.md docs/README.md`).
7. **CLAUDE.md:** already current from A5/A9/A10/A11/A12. Check it once more against the final tree.
8. **Delete the plan inside this commit:** `docs/plans/2026-11-01-async-threads.md` and its directory, as roadmap 「進め方」 says. Before deleting, check that every decision is in `docs/` (G-A texts → spec/design docs; kinds → this message; deferred items → roadmap).
9. **Commit message (r2, C-6):**
   - what moved where (spec per unit, amendments 1-11, roadmap items);
   - test changes by kind with reasons (§5, plus every K2-d file and its observed diff);
   - measured highlights: S(2), S(4), S_hw, the close verdict (and its re-check after A14), `chan_pingpong`, the U6b decision, the startup cost;
   - rows resized (none in-stage; the `Resize bench rows` commit, if any, comes with `Measure async-threads`);
   - the machine change: the in-stage numbers were measured on a Linux cloud VM (`lscpu` line), and the 「今の数値」 tables follow on macOS in `Measure async-threads`;
   - the decision dates of the deferred items;
   - the stage name and date.
- Completion: K1–K6, the greps above, `ls docs/plans` → no async-threads entry.

## 4 Base, prerequisites, baseline, overall verification, manual checks

### 4.1 Base

`$BASE` = 2c8de78 [V: git log]. If main has moved, rebase and re-locate the cited lines by phrase.

### 4.2 Prerequisites

| Tool | Check | If missing |
|---|---|---|
| devShell (cargo 1.99.1, nextest, cargo-insta, clippy, rustfmt, verus, cargo-verus, verusfmt) | `nix develop -c sh -c 'cargo nextest --version && cargo insta --version && which verus verusfmt'` [V: here cargo-insta 1.49.0 with `--check` and `--test-runner`] | `nix/prefetch-inputs.sh`, then `nix develop` |
| `.#miri` (Miri, ASan, TSan) | `nix develop .#miri -c cargo miri --version` | stop and report; A3, A5 Miri and A4–A14 sanitizer checks need it |
| `.#cross` | `nix develop .#cross -c cargo clippy --version` | stop and report (A8, A9, A11, A14) |
| actionlint | `nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- --version` | needs nixpkgs |
| python3, `lscpu`, release build, ≥ 4 idle cores (A12–A14) | `python3 --version; lscpu` | A13 cannot judge; record and ask the user |

### 4.3 Baseline (record in the A1 commit message)

- `cargo insta test --test-runner nextest --check` → exit 0. Record the nextest summary count ("N tests run").
- [V grep] tests/workers.rs 16, io.rs 22, signal.rs 10, chan.rs 11, citations.rs 4.
- [V ls] `run/async` 69, `run-fail/async` 12.
- TSan/ASan/Miri commands of CLAUDE.md, `cargo verus verify -p eml_rt` → pass on base.
- `ui::run` wall time; the `onetask.em`/`onefork.em` startup medians.

### 4.4 Overall verification after A13/A14, before C

- K1–K6 on the tip.
- `cargo nextest run --profile timing -j 1` → no SLOW, no failure.
- Every sanitizer and Miri command of CLAUDE.md in its final form.
- verus and verusfmt.
- Windows and Linux cross clippy.
- The non-default feature clippy lines.
- actionlint.
- K2 against `$BASE`.
- A 50-iteration stress loop of `EML_UI_FILTER=/async/ cargo test -q -p eml_cli --test integration -- --exact ui::run ui::run_fail` → no failure.

### 4.5 Manual checks for the user

- Confirm the close criterion.
- Approve §5, including the K2-d scope and its cap.
- Review the A13 (and A14) numbers and the U6b decision.
- macOS: the three bench scripts for `Measure async-threads`; `bench/net.sh --workers 1,2,4`.
- Windows CI green at A8, A9, A11 and A14.
- On a desktop, run a net program at the default worker count and Ctrl-C it.

## 5 Test change list (kinds 1 and 2), whole stage (r2, C-2, C-3)

Rule applied: a test whose purpose stays and whose input or expected value changes is kind 2; a test that is removed or replaced by one with another purpose is kind 1 (testing.md 「テストの変更の運用」).

| ID | Unit | Kind | Test | Change | Reason |
|---|---|---|---|---|---|
| K1-a | A10 | 1 | `ui::workers_runs_cover_par_only` (ui.rs:620 [V]) | deleted; replaced by `ui::workers_runs_cover_par_and_async` | the rule it pins (`async/` never runs at N ≥ 2) is reversed |
| K2-a | A10 | 2 | `tests/ui/run/async/shield_boundary.em` | line 55 `Async.yield_now ()` → `Async.sleep_ms 10` inside `shield` in the `bit_first` call; snapshot byte-identical [V by S3] | P1 race at N ≥ 2 can print "unreachable" (S3 §5.2) |
| K2-b | A9 | 2 | `workers::exit_abandons_a_child_on_another_worker` | `assert_eq!((finished.workers, finished.placed), (1, 1))` → `assert_eq!((finished.workers, finished.placed_tasks), (1, 1))`; doc comment: the spawned task (on worker 1) and whatever it forked are abandoned | Purpose unchanged: `exit` abandons work running on another worker, worker 1 reads `END`, both heaps recover, exit 3, empty stdout. With AM-1 the spawned task is on worker 1, and `main`'s `yield_now` no longer hands it the worker, so whether its `par` fork happens before `exit` is a race [I: workers.rs:316-335 [V]]. The deterministic count of work placed on worker 1 is now `placed_tasks`. Expected-value change → kind 2 |
| K2-c | A9 | 2 | `workers::a_timer_on_worker_0_and_a_child_on_worker_1_agree` | prefix the body with `let d = Async.start (fn () -> ())`, `Async.await d`, `Async.sleep_ms 1`. The dummy takes worker 1 (first start). The sleep returns only when every worker is idle, so worker 1 is parked and idle at the fork. `t` (second start) stays on worker 0, and the right child goes to worker 1. Assertions unchanged (`out == alone`, `placed == 1`), plus `placed_tasks == 1` | Purpose unchanged: a timer on worker 0 does not fire while a child runs on worker 1. Without the prefix, AM-1 puts `t` on worker 1 and the child's worker becomes a race. Input change with the same purpose → kind 2 [I: idleness argument from S1 Q6 and G-A12] |
| K2-d | A10 (and any later unit, CI) | 2 (scope) | `run/async/*` and `run-fail/async/*` UI files | **Scope.** A file qualifies if it either shows an output diff at N = 2 or 8 in the S3 §5.5 step 3 stress run (200 iterations for the files S3 §5.3 predicts, 50 for the rest), or fails at N ≥ 2 in CI and is shown, after debugging, not to be a VM bug. Such a file may get one of: (1) a rewrite that keeps its snapshot byte-identical and keeps its described scenario at N = 1; (2) a reduction of its work that keeps its output byte-identical (S3 §5.5 step 4, over the 1 s budget); (3) the first-line mark `-- workers: skip (<the observed diff, or "N = 2 以上で 1 秒を超える">)`. **Cap: 5 files in the whole stage**; more needs a plan amendment approved by the user. Each file is named, with its observed diff and the chosen action, in the unit's commit message and in a `Record …` commit | Marks remove a file from the N ≥ 2 guarantee (G-A18), and rewrites change test programs. Both need prior agreement, so they are kind 2 by scope (r2, C-2). S3 predicts at most the two drain files |

Kind 3 (no agreement needed; commit message records them): `--workers 1` on three `cli.rs` signal tests and their docs (A9); `Finished` literals; `Run::new` and `Scheduler`/monitor call shapes; `for_each_file`/`marks_of`; doc comments of existing tests. Additions are not listed. No `RunStats` snapshot and no N = 1 UI snapshot changes in any unit (K2).

## 6 Risks

| # | Where | Risk | Fallback / design answer |
|---|---|---|---|
| R1 | A9 | a large test diff | the split in §2 A9: code first, tests-only second, both passing |
| R2 | A4 | Q7 wrong but only exercised later | A4's two worker-1 collect unit tests (B-2); A9's 20-loop `signal::` |
| R3 | A8/A9 | lost wake or hang in the deadlock re-check, `left`, `END` | S1 Q6 (d) and Q8; the unit tests; TSan; watchdogs. Debug as a VM bug; never mark first |
| R4 | A9 | a `RawBuf` used after its record moved or was freed | I1–I4 (S1 Q5); ASan `io:: workers::` from A9; `take_op` requires `Done` |
| R5 | A10 | Virtual `net_*` over 1 s; drain files diverge | K2-d with its cap |
| R6 | A10+ | a CI failure that happens only sometimes (CI never retries) | S3 §5.2 step 5, then K2-d |
| R7 | A11 | per-worker drivers on IOCP or kqueue | cross clippy; ASan on macOS in CI; fix before A12 |
| R8 | A6 | a std change breaks reserve→commit adjacency | hold assertions |
| R9 | A13/A14 | close criterion missed | the stage does not close; diagnose (S3 §5.6), fix, re-measure. The user may revise the criterion |
| R10 | A13 | S_hw validity fails | another machine, or ask the user |
| R11 | A14 | S1 A4 fails on IOCP | the Windows fallback with a spec sentence |
| R12 | A14 | gate not met | skip A14; roadmap item 4 |
| R13 | A9 | startup cost of `next_place` = 1 | record it; follow-up |
| R14 | A7/A14 | eml_rt gate | [V] `trust::` passes with the `Send` impl; `verus verify` unaffected [I] |
| R15 | K1 | `cargo insta test --test-runner nextest --check` duration | accepted; it is the single suite run per unit |
| R16 | A8 | forwarding code reachable only from lib tests until A9 | the two lib tests drive both sides by hand. A9 adds the integration tests before placement can reach users |

## 7 Unresolved items with defaults

1. Close criterion S_spread ≥ 0.80 × S_hw at N = 2 and 4, valid only if S_hw(2) ≥ 1.6 and S_hw(4) ≥ 2.4. **User to confirm.**
2. The K2-d cap of 5 files.
3. `Registry.relay` (D-8).
4. The order of §1.1 (channel and forwarding before placement). The alternative is S1's order with a guard; it was rejected because a guard would also need removal and review.
5. Skip the waker when the slot was not parked: yes (S1).
6. `bench/scripts.sh` unpinned; `async-compute` directly after `async-threads` (S3 §6).

## 8 Additional checks

- [V] `git log --stat`: a2b8094 (plan only), e75b2a9 after 3ec0c1c (roadmap), 7303081 (separate deletion; not followed, A-2).
- [V] std/Par.em:4-6 (`<never e>`, `IO` refused), std/Atomic.em:10-14 (`<IO>`): `par` children cannot reach task, channel or counter code before A9.
- [V] signal.rs:91-430; main.rs:142-146; cli.rs:1150-1300.
- [V] workers.rs:295-470; tests/preempt.rs at default `Limits` (N = 1).
- [V] crates/eml_vm/src/tls.rs: no `Scheduler` or worker state (grep).
- [V] vm.md:400-609, 801, 855-876 text; stdlib.md:21, 910, 1258, 1560; CLAUDE.md:94, 117; testing.md:417, 689; multicore.md:72, 115-160, 336, 439, 488, 523; async.md:244-250 (sed/grep).
- [V] `.config/nextest.toml` overrides; `cargo insta test --help` lists `--check` and `--test-runner`.
- [V] trust.rs allow-list; scratch `unsafe impl Send for RawBuf {}` → `trust::` 6 passed; reverted.
- /home/user/eml was not modified. No background task is left running.

