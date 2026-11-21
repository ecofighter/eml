# Stage remote-reclaim: remote frees a long task never takes in; small pages and fragmentation (design-level)

Repository: /Users/arakaki/Projects/eml, main @ 6ff1f664, read-only. Tags: [V: read] read in source or docs, [V: measured] measured here, [V: prototype] built and run in a scratch copy, [I] inferred.

Evidence files, in the appendix directory `docs/plans/2026-11-21-remote-reclaim/` (committed with this plan; the stage's Close deletes both): the probes `p_*.em`, `f_*.em`, `t_ack_*.em` and the sampler `run.sh` with the watchdog `watch.sh` it calls; `prototype.patch`, the whole change of this stage (118 diff lines) against main @ 6ff1f664, as built and measured. The logs stayed in the session scratchpad.

Every `.em` run went alone under `watch.sh 2097152`. "base" is `target/release/eml` of the main tree (newer than the last `crates/*/src` and `std/` commit 5eb29eb1 [V: git log, mtime]); "head" is the release build of the copy with `prototype.patch`.

User decisions of 2026-11-21 that this revision implements: reclaim before sharing, the doorbell is a later item; Part 2 is in the stage with the fallback to Part 1; the small-page sweep is deferred and its roadmap item rewritten; the stage is `remote-reclaim`, after `row-bound-retire`; the sleeping-owner retention is accepted and stated in the spec.

## 0. Summary of rulings

| Topic | Verdict | Ruling |
|---|---|---|
| A. Remote frees of a task that never switches | Real, unbounded, wider than reported. Not a memory-safety issue. | Stage `remote-reclaim`, after `row-bound-retire` and before `native-checked`. |
| B. Returning pages of small objects | Never returned while the heap lives; by design, documented. | Keep. The roadmap item is rewritten with the true bound, a default design and a 見直す条件 the tools can read. |
| C. Fragmentation | Bounded per heap by the sum over size classes of each class's peak; 8x measured on a class-shift probe. | Same later item as B; no stage. The stage of A corrects the stated bound. |

---

## A. Remote frees a long task never takes in

### A.1 Findings

1. Reclaim points today [V: read crates/eml_vm/src/vm.rs]: `Ctx::reclaim_if_due` at four places: `switch_away` (:1975), before sleeping in the idle loop (:1998), `complete_join` (:2201), after `take_job` / `drop_job` (:2516). `Ctx::reclaim` at the end of the run (crates/eml_vm/src/lib.rs `reclaim_all`). docs/spec/object-model.md 「別のスレッドからの解放」 says the allocator and `over_budget` never reclaim, because the allocation proofs promise "only adds an object".
2. `eml run` uses `Sched::Normal`, whose `tick` is constant false: a task runs until it waits or ends in an `Async` operation (docs/spec/vm.md 「タスクとスケジュール」; `safepoint`, vm.rs:512). A safepoint that sees `NOTICE` collects notices and does not reclaim [V: read].
3. Operations that hand an object to another worker [V: read docs/spec/stdlib.md 「操作の予算」, std/Chan.em, std/Async.em]:
   - `Chan.try_send`: spends the budget, never yields (no `Async` in its row).
   - `Async.start` / `spawn` / `start_in` / `spawn_in`: spend no budget.
   - `Chan.send`: yields every 128 fast paths and parks when full, so it reaches `switch_away`. Its bound is by item count, not bytes: up to one budget of items stays held (measured 151 MB for 1 MiB items).
4. What accumulates [V: read crates/eml_rt/src/remote.rs, heap/reclaim.rs]:
   - Large objects, fast mode: the freeing thread refunds the bytes at the push and leaves the page on the owner's `RemoteSink.large`. Until the owner reclaims, the page is resident and unaccounted; `max_bytes` never sees it.
   - Small objects, fast mode: the block goes to its page's remote list, not refunded. The owner's class free list does not get it back, so the owner carves fresh pages and `Control` bytes grow truthfully. A false `the memory limit is exceeded` at 80% of RAM would follow [I: not run to the limit; only the growth is measured].
   - Checked mode: the block stays `Live` in the owner's stats until reclaim. Checked mode never reuses or returns, so its RSS is the total allocation either way (docs/spec/vm.md 「上限と割り込み」).
5. Measured, 2 workers, fast mode; the producer is `main` on worker 0, the consumer on worker 1 [V: measured]:

   | Probe | What | base peak RSS | head peak RSS |
   |---|---|---|---|
   | `p_try.em` | 1200 x 1 MiB array by `try_send`, a spin loop between sends; at most 5 alive | 1,258,688 KB, linear in the run | 19,136 KB, flat |
   | `p_spawn.em` | the same arrays captured by `Async.spawn` closures | 1,261,312 KB, linear | 18,560 KB, flat |
   | `p_send.em` | the same by `Chan.send` | 150,800 KB, flat | not rerun |
   | `p_small_try.em` | 1,000,000 x `Array.make 100` by `try_send` | >= 607,072 KB (2 samples, still growing) | 17.9 MB (`time -l`) |
   | `p_small_send.em` | the same by `send` | 17,120 KB | not rerun |
   | `t_ack_try.em` | 200,000 x `Array.make 100` by `try_send`, each acknowledged through a `Std.Atomic.Counter` before the next | 200.6 MB (`time -l`) | 17.9 MB |
   | `t_ack_spawn.em` | the same by `Async.spawn` | 209.1 MB | 17.2 MB |

6. Memory safety [V: read]: none. The remote lists are linked through the freed blocks; nothing overflows; the push/take protocol (T14) does not depend on when the owner takes. It is a resource issue.
7. Other paths that free across workers [V: read]:
   - `Async` results (`finish_ok` -> `Depot::publish`): the producer ends, which is a switch.
   - `compute` and `par` (`fork_child`): the parent reclaims after the join / `take_job`; bounded by what the closure captured. Confirms "not reachable through `Std.Par`".
   - `Std.Atomic`: `Counter` holds an `Int` only (std/Atomic.em). No path.
   - Sleeping owner: a parked worker reclaims before it sleeps, not while asleep. What is freed to it while asleep stays until it next runs, at most what it had allocated before sleeping.
8. Run-time `Heap::share` sites [V: read; the reviewer confirmed]: `eml_vm::vm::fork_child` (vm.rs:1791, 1794; `par` and `compute`), `eml_sched::cells::Registry::start` (cells.rs:1941, 1943; under the registry lock; one caller, `start_task`, vm.rs:3500), `eml_sched::cells::Depot::publish` (cells.rs:5913; callers `AsyncFinishOk` vm.rs:2415 and `ChanPushRaw` vm.rs:2711). The `share` in crates/eml_nrt/src/entries.rs:1363 is test code. An object leaves its worker only through these.
9. A due reclaim walks every page of the heap, also for one large page [V: read reclaim.rs `reclaim_held`], and both push functions count into `RemoteSink.count` (remote.rs:111, :148).
10. The runtime plans (docs/plans/2026-11-17-runtime/01-design.md, 2026-11-17-sched-core.md, 2026-11-17-jit-tasks.md) name no reclaim point for `drive` or the native rows [V: grep `reclaim`, `取り込` empty].

### A.2 Verdict

Real. A task that only uses `Chan.try_send` or `Async.spawn` and never waits makes its heap keep every block another worker frees, without bound in the run length. Large objects are invisible to `max_bytes` (1.26 GB resident for 5 live MiB). For small objects the accounted bytes grow with the run; that this ends in a false memory-limit stop is inferred, not measured. No undefined behaviour. The documented bound "RSS は worker のヒープごとの最大の和" (docs/future/multicore.md:320, :539, docs/future/roadmap.md:313) does not hold today.

### A.3 Design

Rule (engine obligation): on the owner's thread, before an engine marks a value for another worker (`Heap::share` during a run), it calls `Ctx::reclaim_if_due` on the same `Ctx`.

The bound it gives: blocks other threads freed that a heap has not yet taken in were alive at that heap's last share point. So, per heap:

  pages held <= (the sum over size classes of each class's peak, section C) + (the objects alive at the last share point).

The second term can be as large as the first (a whole graph shared at one point, freed later while the owner shares nothing). The unaccounted large bytes are within the second term. This is not "twice the peak of live bytes".

Why a reclaim there cannot free a word the task still uses: `reclaim` empties only the set `remote_take` / `remote_large_take` promise (`Core::freed_set`): blocks whose last reference another thread dropped. A word in a VM register, on a task's stack, in the channel `Hold` or in the root table is a reference, so its target is not in that set. `held_set` only names the `Ctx`'s own words; the same argument already covers every other task's stack at `switch_away`. The sent word of `ChanPushRaw` is alive when `publish` runs, so a reclaim before the `share` leaves it alone [V: read; V: prototype runs].

#### Part 1: three sites

| Site | Position | Covers |
|---|---|---|
| `eml_vm::vm::fork_child` | first statement, before `fork_worker()` | `par`, `compute` |
| `eml_vm::vm::start_task` | first statement, before `unshared_roots` and before `sched.cells.lock()` | `start`, `spawn`, `start_in`, `spawn_in`, the main-worker variants; no reclaim under the registry lock |
| `eml_sched::cells::Depot::publish` | first statement, before `share` | `ChanPushRaw`, `AsyncFinishOk`; the native rows inherit it |

The prototype put the third call in the `ChanPushRaw` arm instead of `Depot::publish`; `Depot::publish` adds the same call to `AsyncFinishOk` (once per task end) [I: cost not re-measured for that one extra call].

Cost when nothing waits [V: measured, `EML_WORKERS=1`, `/usr/bin/time -l` instructions, median of 3, base vs head]:

| Program | base | head | delta | per hand-off |
|---|---|---|---|---|
| `bench/pfib.em` size 30 (about 1.35 M forks) | 19,803,854,099 | 19,812,092,472 | +0.04% | about 6 instructions |
| `bench/async/chan_pingpong.em` (160,000 sends) | 3,430,840,408 | 3,432,981,038 | +0.06% | about 13 instructions |
| `bench/pfib.em` size 20 (the `bench/run.sh` row) | 318,535,680 | 319,596,270 | +0.33%, inside the run-to-run spread of 0.25% | - |
| `bench/par_tree.em` | 165,346,733 | 165,339,840 | 0.00% | - |

`Ctx::reclaim_if_due` runs `settle` and the due test (two relaxed loads) before anything else; the measured cost is 6 to 13 instructions, so the due test is not moved in front of `settle`. The VM gate (`bench/run.sh`, +0.5%; 00-decisions A-5) has room.

#### Part 2: a due reclaim walks the pages only for small blocks

Options compared:

| Option | `count` means | A due reclaim | Existing tests | Ruling |
|---|---|---|---|---|
| r1: partial arm, count untouched | all frees, large ones stay counted after they are taken | walks every 256 frees of any kind | pass | Rejected: `count` no longer means "waiting"; a producer of large objects still pays a walk per 256 sends; the spec sentence on `sink_pending` becomes false. |
| `fetch_sub` of the taken large pages in the partial arm | small blocks waiting plus large pages not yet taken | walks when count >= 256 | pass | Rejected: the same effect as the chosen option with one more atomic write and a number to carry out of `remote_large_take`. |
| Small-pending flag: keep "large pending => full reclaim", skip the walk when no small block came | small blocks waiting | walks whenever a large page and any small block wait | pass | Rejected: one small block beside each large one (a closure, a record around an array) brings the walk back at every hand-off. |
| Chosen: large pushes do not count | small blocks pushed and not yet taken by a walk | large list: whenever non-empty. Pages: when count >= 256 | pass, unchanged | Chosen. |

The chosen change, exactly as in `prototype.patch`:
- `remote_large_push`: delete `sink.count.fetch_add(1, Ordering::Relaxed)`.
- `Heap::reclaim_if_due_held`: `let pages = sink_count(sink) >= REMOTE_RECLAIM; if pages || sink_large_pending(sink) { self.reclaim_held(Ghost(held), pages); true } else { false }`.
- `Heap::reclaim_held(held, pages: bool)`: `if pages { sink_count_take(sink) }`; the page loop runs to `n = if pages { pages_len() } else { 0 }` with the invariant `n <= page_count()`. `Heap::reclaim`, `Ctx::reclaim` (`reclaim_when(false)`) and recovery pass `true`.

Meaning of the fields afterwards: `count` is a hint of the small blocks pushed since the last walk; a walk resets it to 0; `large` is the list of large pages; after a full `reclaim` at a quiescent point all three fields are 0, so `sink_pending`, `check_owned`, `check_invariants` and `freeze` are unchanged. `reclaim_if_due` returns true when it took the large list or walked; with fewer than 256 small blocks waiting it leaves them (at most 255 blocks, under 255 KiB).

Proof [V: prototype]: `cargo verus verify -p eml_rt` on the copy: "1265 verified, 0 errors". The argument: `remote_large_take` carries its own `taken` promise; with the loop skipped `acc` is empty before it, and every later invariant is stated over `out` and `acc`, not over the pages visited. The only edit inside the proof is the loop invariant `n <= page_count()`.

Tests [V: prototype]: `cargo test -p eml_rt` on the copy: 924 passed, 0 failed, with no test edited. That includes `the_owner_carves_while_another_thread_reads_and_frees` (its new `list` carves from the cons class's current page, which has room, so `control.bytes()` does not move), `reclaim_if_due_waits_for_enough_frees` and the test at trusted.rs:1183. Not run: Miri, verusfmt, clippy, `bench/rt-gate.sh`, the workspace tests.

#### Alternatives to the rule

| Alternative | Ruling | Reason |
|---|---|---|
| Reclaim in the allocator (large path or over-budget poll) | Rejected | The spec forbids it for the allocation proofs; the budget poll misses large objects, already refunded. |
| Reclaim in the rows that allocate large objects | Rejected | Many sites inside `eml_rt` and `eml_rows`; misses small objects. |
| Doorbell: the freeing thread raises `NOTICE`; the owner reclaims in `collect_notice` | Later item (user decision) | Needs the heap-to-worker link `task-limits` deleted (D-13), T14 code, a ring policy, ThreadSanitizer cases; does not wake a sleeping owner. |
| Make `try_send` and `start` spend and yield | Rejected | `try_send` has no `Async` in its row; changes schedules and `RunStats.parks`. |
| The freeing thread returns or decommits the large page | Rejected | The owner's page list and permissions name the page; `eml_rt` holds no `extern "C"`. |
| Part 1 alone | Fallback (user decision) | Correct; a due reclaim then walks the pages at every hand-off with a large page waiting. |

### A.4 Placement

Stage `remote-reclaim`, second in 「段の列」: after `row-bound-retire`, before `native-checked`.

- Not folded into `sched-core`: it leaves the VM's scheduler unchanged, and the VM is the default engine until `jit-repl`.
- Decision number: `row-bound-retire` takes D-17 (docs/plans/2026-11-21-row-bound-retire.md:107). This stage takes D-18. If `row-bound-retire` has not landed when this stage starts, the number is still D-18 and D-17 stays reserved for it.
- D-18 also restates D-17's "`native-checked`'s base is `Close row-bound-retire`" as "`Close remote-reclaim`".
- D-18 writes the rule into the runtime design (01-design.md 2.3, 2.5, 11.4, 18): `drive` calls `reclaim_if_due` at item close and before it parks; `eml_nrt_join` after `wait_join`; `eml_nrt_fork` and the native start rows before their `share`; `Depot::publish` already carries it.
- Conformance criterion for the design (01-design 18) and for this stage: `grep -rn '\.share(' crates/eml_vm/src crates/eml_sched/src crates/eml_nrt/src`, test modules aside, lists exactly the calls in `fork_child`, `Registry::start` and `Depot::publish` (plus, from `jit-tasks` on, the ones D-18 names); `fork_child` and `Depot::publish` begin with `reclaim_if_due`; every caller of `Registry::start` calls it before taking the registry lock.

### A.5 Open questions

None left for A. The six questions of r1 are decided (see the head of this file).

---

## B. Pages of small objects

### B.1 Findings

1. A page of small objects is never given back while its `Heap` lives: not to the OS, not to a pool, not to another class [V: read docs/spec/object-model.md 「確保と解放」; crates/eml_rt/src/alloc.rs: `free` pushes the block on `classes[class].free`; `impl Drop for Pages` returns every page].
2. A page whose blocks are all free, locally or through `remote_take` + `Pages::free`, keeps them on its class's LIFO free list. There is one free list per class across pages, no per-page count and no per-page list, so the allocator cannot tell that a page is empty [V: read `struct Class`, `struct Pages`].
3. RSS shrinks only through large objects: a local free calls `dealloc` at once (`release_large`); a remote free is refunded at the push and returned at the owner's reclaim; a pinned page is parked and returned when its `PagePin` drops [V: read]. Whether `dealloc` lowers RSS is the system allocator's choice.
4. Checked mode quarantines every freed block and returns no page, large ones included, until the heap drops [V: read docs/spec/vm.md 「上限と割り込み」].
5. A heap drops at the end of a run (workers 1..N-1, the main worker), at `Machine` drop, and at recovery, which copies the live objects into a new heap [V: read]. A REPL session or a server keeps worker 0's heap for its life.
6. docs/spec/object-model.md:236 lists the return of small pages and per-page free lists as undecided; docs/future/roadmap.md:312-314 and docs/future/multicore.md:320, :539 defer it. docs/future/native.md and verification.md hold no note on it [V: read, grep].
7. Native code allocates through entries; no generated code reads the allocator's fields [V: read: no allocator offset among the `CTX_*` consts]. No planned stage freezes the page layout, unless one inlines small allocation.

### B.2 Verdict

By design and documented; the documented bound is imprecise (C). No stage now (user decision).

### B.3 Design and alternatives for the later item

| Option | Ruling | Reason |
|---|---|---|
| Sweep (`Pages::trim`) at a point the engine chooses: count the free blocks of each page from the class free lists, unlink the blocks of pages that are entirely free and fully carved, then return those pages or keep them on an empty-page stack that `next_page` takes before `alloc_page` (this is the size-class rebalancing) | Default design | Leaves `alloc`, `free`, the page layout and the remote protocol unchanged; the proof is local to alloc.rs; O(free blocks) per sweep. Candidate points: before reporting the memory limit, between REPL inputs, before a long park. |
| Per-page free lists and counts (mimalloc) | Rejected until measured | Changes the hot path and rewrites the allocator's proofs (alloc.rs, 3655 lines). |
| Decommit empty pages with `madvise` | Rejected | FFI in `eml_rt`, a Miri hole, three OS variants, on `std::alloc` memory; needs the sweep's emptiness test first. |
| A pool shared by the workers of a `Control` | Later, on top of the sweep | Needs a lock or lock-free stack and ThreadSanitizer cases; only when one worker's empty pages starve another. |
| Keep as is with a 見直す条件 | Chosen (user decision) | Bounded (C.2); no measured long-running workload is limited by it. |

Sparse survivors defeat every option but compaction, which the object model excludes. They add nothing to today's bound, because free blocks of a class are reusable across its pages.

### B.4 Placement

Not a stage. The roadmap item is rewritten at `Close remote-reclaim` (D.6). Decide it before any stage inlines small allocation in native code.

---

## C. Fragmentation

### C.1 Findings

1. 27 size classes: 2 to 16 words by one, then 20, 24, 28, 32, 40, 48, 56, 64, 80, 96, 112, 128 words; 64 KiB pages with a 48-byte head [V: read alloc.rs]. Internal waste: rounding to the class (worst 17 -> 20 words) and the page tail.
2. A page belongs to one class for the heap's life (B.1.2).
3. Class shift, 1 worker, fast mode [V: measured]:

   | Probe | What | RSS during the final idle loop (live data 0) |
   |---|---|---|
   | `f_shift.em` | 12 phases, each about 48 MB of small arrays of one class, dropped before the next; 12 classes | 610,064 KB |
   | `f_same.em` | the same 12 phases in one class | 75,488 KB |

   8.1x. Peak live at any moment is one phase in both.
4. Sparse survivors of one class: its free blocks are reused by the same class; stranded only for other classes [V: read].
5. Producer and consumer across workers: remote-freed small blocks return to the owner's class lists at reclaim; before reclaim they are topic A.
6. `RunStats` has no byte count; `peak_objects` exists only in checked mode, whose RSS is not comparable [V: read crates/eml_sched/src/config.rs]. The live figures come from the programs' shape.

### C.2 Verdict

Bounded, not by live bytes. Per heap: small pages <= the sum over the 27 classes of each class's peak (at worst 27 times the peak of live bytes; 8x in the probe), plus, after `remote-reclaim`, the objects alive at the last share point. Before `remote-reclaim` there is no bound across workers. That `max_bytes` can stop a class-shifting program whose live data fits is inferred.

### C.3 Ruling

Keep and record: the sweep of B.3 addresses it; B and C are one later item. The stage of A corrects the stated bound in docs/future/multicore.md, docs/future/roadmap.md and docs/spec/object-model.md.

---

## D. Plan of stage `remote-reclaim`

### D.1 Goal

A worker takes in the frees it received from other threads before it marks a value for another worker; a due reclaim takes the large pages at once and walks the pages only when 256 small blocks wait.

### D.2 Scope

In: `eml_rt` (remote.rs, heap/reclaim.rs, the two callers), `eml_vm` (two calls), `eml_sched` (`Depot::publish`), spec and future docs, D-18, tests.
Out: the doorbell; returning small pages; any change of `Chan`, `Async` or the budget; native code.

### D.3 Decisions

- D-RR-1: the rule and the three sites of A.3 Part 1.
- D-RR-2: A.3 Part 2 as in `prototype.patch`. `REMOTE_RECLAIM` stays 256.
- D-RR-3: fallback. If RR-2 fails its gate (Verus, Miri budget or `bench/rt-gate.sh`) and the cause is not fixed inside the unit, RR-2 is dropped, Part 1 ships alone, and Part 2 becomes a later item.
- D-RR-4: the doorbell and the small-page sweep are later items (D.6).
- D-RR-5: the sleeping-owner retention is accepted and stated in the spec; no test pins it (D.4).
- D-RR-6: D-18 as in A.4.

Units (each leaves CI green):

| Unit | Content | Done when |
|---|---|---|
| RR-1 | The plan, the roadmap entry, D-18 | committed as the stage's first commit |
| RR-2 (`eml_rt`) | D-RR-2, its test, the spec sentences on `count` and `reclaim_if_due` | `cargo verus verify -p eml_rt` (1265 verified, 0 errors), verusfmt, `cargo test -p eml_rt`, both Miri settings of `trusted::` within budget, clippy, `bench/rt-gate.sh <commit before>` exit 0 |
| RR-3 (`eml_vm`, `eml_sched`) | D-RR-1, its tests, the spec sentences | the two new tests fail before the calls and pass after; `cargo nextest run`; `bench/run.sh` before and after: the run part of `pfib`, `par_tree`, `par_map`, `par_map_unboxed`, `par_reduce`, `par_tabulate_int`, `par_tabulate_float` within +0.5%; `/usr/bin/time -l` instructions of `bench/async/chan_pingpong.em` at `--workers 1`, median of 3, within +0.5% (measured here +0.06%); the grep criterion of A.4 |
| RR-4 | Close: benchmarks tables, status, roadmap, later items | the Close checklist of docs/future/roadmap.md |

### D.4 Test changes by kind

- Kind 1 (pass/fail): none.
- Kind 2 (expected values): none found. `cargo test -p eml_rt` passes unedited with Part 2 [V: prototype]. Not run: the workspace tests. Reasoning for the VM side: a snapshot moves only if a worker takes in remote frees earlier in checked mode, which needs 2 or more workers and a `par`, start or channel push after a remote free; `RunStats.peak_objects` is then already schedule-dependent. `crates/eml_vm/tests/bench.rs` runs on one worker, where no remote free exists. If RR-3's `cargo nextest run` shows a snapshot that moved, the plan is amended with that file by name before the unit is committed; it is not rewritten silently.
- Kind 3: none.
- New tests:
  - RR-2, `crates/eml_rt/src/heap/tests/trusted.rs`, both modes, `a_due_reclaim_leaves_few_small_blocks`: another thread frees a 16-cell shared list and one large object. `reclaim_if_due()` is true; in checked mode `stats().reclaimed == 1` (the large one only); a second `reclaim_if_due()` is false; `reclaim()` then makes `reclaimed == 17`; `mortal == 0`.
  - RR-2, `crates/eml_rt/src/heap/tests/remote.rs`, both modes, `large_frees_do_not_count_toward_the_walk`: 255 small remote frees, then one large remote free: `reclaim_if_due()` is true and (checked) `reclaimed` grows by 1; one more small free: `reclaim_if_due()` is true and `reclaimed` grows by 256.
  - RR-3, `crates/eml_vm/tests/workers.rs`, fast mode, 2 workers, programs `t_ack_try.em` and `t_ack_spawn.em` at 20,000 items: after the run, `control.bytes()` (worker 0's pages; the other heaps are dropped after a successful run, and small pages are never returned, so it is worker 0's high-water mark) is below 4 MiB. Measured shape at 200,000 items: base 200.6 MB and 209.1 MB of RSS, head 17.9 MB and 17.2 MB, so about 1 KiB per item is kept without the calls; the expected failing value at 20,000 items is about 20 MiB [I: scaled]. The implementer records the real failing value in the commit message when the test is written first. Run time at 200,000 items is 0.6 s and 2.1 s in release; confirm each test stays under 1 s alone at 20,000 in the dev profile, else lower the count and the threshold together.
  - No test for the sleeping owner: the retention is an accepted limit, not a rule; after the run everything is taken in, and no public reading shows a parked worker's waiting frees. A test would pin the limit.

### D.5 Docs list

- docs/spec/object-model.md 「別のスレッドからの解放」, rewritten sentences (English drafts; translate with PLaMo, `form: plain`):
  - the push bullets: "After pushing a small block the thread adds 1 to the received count of the `RemoteSink`. A large page is not counted; the owner sees it by the list being non-empty. The count is the number of small blocks pushed since the owner last walked its pages; it is a hint for when to walk, and it is exact at a stop."
  - `Heap::reclaim`: unchanged ("受けた数を0に戻し…").
  - `Heap::reclaim_if_due`: "takes out the list of large objects when it is not empty, and walks the pages only when the received count is `REMOTE_RECLAIM` (256) or more; it returns true when it did either. With fewer small blocks waiting it leaves them where they are."
  - the reclaim points gain "before the engine marks a value for another worker (the fork of `par` and `compute`, the start of a task, `Depot::publish`)".
  - the bound of A.3 and the sleeping-owner sentence (D.6 holds its Japanese).
  - line 236 「決めていないこと」 points at the rewritten later item.
- docs/spec/vm.md: 「タスクとスケジュール」 (the reclaim bullet), 「`par` の fork と join」 fork, 「`Async` のタスク」 命令 (the start rows), 「channel」 (the `Depot` table: `publish` reclaims first).
- docs/future/multicore.md :319-320 and :539, docs/future/roadmap.md: the corrected bound; the stage entry; the two later items.
- docs/implementation/status.md, docs/implementation/benchmarks.md 「今の数値」 at Close.
- docs/plans/2026-11-17-runtime/00-decisions.md (D-18), 01-design.md (2.3, 2.5, 11.4, 18).
- Doc comments: crates/eml_rt/src/remote.rs (`RemoteSink.count`, `remote_large_push`, `sink_count`), heap/reclaim.rs, heap/ctx.rs (`reclaim_if_due`'s list of points), crates/eml_sched/src/cells.rs (`Depot::publish`).

### D.6 Roadmap and spec texts (Japanese; drafted in English, translated with the `plamo-translate` CLI, then put into the repository's terms)

「段の列」, after `row-bound-retire`, before `native-checked`:

> - `remote-reclaim`: workerは、値にほかのworkerの印を付ける前に、ほかのスレッドから受けた解放を取り込む。その地点は、`par`と`compute`のfork、タスクを始める行、値を預ける`Depot::publish`である。`Chan.try_send`や`Async.spawn`だけを繰り返して切り替えないタスクも、ほかのworkerが空けたブロックを持ち続けなくなる。ヒープがそういうブロックのために持つ量は、最後のその地点で生きていた物体までになる。`Heap::reclaim_if_due`は、大きい物体の列が空でなければそれを取り出し、ページを歩くのは受けた小さいブロックの数が`REMOTE_RECLAIM`に届いたときだけにする。受けた数は小さいブロックだけを数える。前提: `row-bound-retire`。設計: [オブジェクトモデル](../spec/object-model.md)の「別のスレッドからの解放」

(The 前提 is the order only; if the stages are reordered it is 「なし」.)

docs/spec/object-model.md 「別のスレッドからの解放」, the sleeping owner:

> - 眠っているworkerは、眠っている間に受けた解放を取り込まず、次に動くときに取り込む。その量は、眠る前にそのworkerが確保していた量までである。

「その後の項目」の「マルチコア対応」, new:

> - 別のスレッドからの解放を持ち主に知らせること: 空けたスレッドが持ち主の止める語に`NOTICE`を立て、持ち主が次の安全点で取り込む
>   - 後に回した理由: 印を付ける前に取り込むので、ヒープがほかのスレッドの空けたブロックのために持つ量は、最後のその地点で生きていた物体までである。知らせるにはヒープが自分のworkerの止める語を知る必要があり、眠っているworkerは起こす必要もある。
>   - 見直す条件: 値に印を付けることの少ないプログラムの最大RSS (`/usr/bin/time -l`、`bench/scripts.sh`) が、workerが2つ以上のrunで、workerが1つのrunの2倍を超えたとき。

「その後の項目」の「マルチコア対応」, replaces 「ページを OS に返すこと」:

> - 小さい物体のページを返すことと、級をまたいで使い回すこと: 空けたブロックはその級の空きリストに戻り、ページはヒープが生きている間、ほかの級に使われず、返されもしない。そのため、ヒープのページは、級ごとの同時に要った最大のページ数の和までである。大きさが級から級へ移るプログラムは、前の級のページをすべて持ち続ける (12の級を順に使う計測では、1つの級だけを使う場合の8倍だった)
>   - 後に回した理由: 量には上限があり、これで困る長く走るプログラムを測っていない。既定の設計は、エンジンが選ぶ地点での掃き出しである。ページごとの空いたブロックを数え、すべて空いたページのブロックを空きリストから外し、そのページを返すかほかの級のために切り出す。確保と解放の道とページの形は変えない。
>   - 見直す条件: 長く走るプログラム (サーバ、REPL、GUIのアプリ) の最大RSSが、生きているデータが増えないのにrunの長さとともに増え続けるとき。または、確保量の上限がそのようなプログラムを止めたとき。`RunStats`はバイト数を持たないので、この項目は最初に、速いモードでページのバイト数と級ごとのページ数を読む手段を足す。小さい物体の確保をネイティブのコードに展開する段より前に決める。

### D.7 Risks

- RR-2's Miri budget and `bench/rt-gate.sh` were not run on the prototype. `bench/rt.sh` rows do not reclaim, so the gate should read 0 [I]. If either fails, D-RR-3.
- A walk happens once per 256 small remote frees, now also from `try_send` and `spawn` loops. Same amortisation as at a switch.
- The `workers::` tests rely on the consumer dropping the item before `Atomic.add`; the probes write `drop a` before the add, and the head runs stayed flat, which they would not if the order were wrong [V: measured]. The thresholds at 20,000 items are scaled, not measured.
- `Depot::publish` was measured at the `ChanPushRaw` arm only; the extra call at `AsyncFinishOk` is once per task end [I].
- The rule reaches the native engine through D-18 and the grep criterion; `jit-tasks` must keep both.
- D-18 must be consistent with D-17's text on `native-checked`'s base, whichever lands first.

## E. Additional checks

- Prototype in a scratch copy (rsync of the tree without `target/` and `.git`, own target directory): `cargo verus verify -p eml_rt` 1265 verified, 0 errors; `cargo test -p eml_rt` 924 passed, 0 failed, 1 ignored; `cargo build --release -p eml_cli` built. The copy is deleted; `prototype.patch` remains.
- `ps` is slow on this machine, so `run.sh` samples and `watch.sh` checks about once per 0.5 to 1 s. Probes were sized so the worst case stays under 1.3 GB. The `time -l` runs put `eml` one level below what `watch.sh` watches; those programs are bounded at about 210 MB by construction.
- Another agent's `cargo test --workspace` ran on the machine during part of the measurements; instruction counts are not affected, wall times may be.
- docs/plans/2026-11-21-row-bound-retire.md now exists in the working tree and claims D-17 [V: read].
- No worktree was made; no process of mine is left (`pgrep -x eml` empty).

---
