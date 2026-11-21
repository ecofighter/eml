# Stage remote-reclaim: a worker takes in remote frees before it marks a value for another worker

- Created: 2026-11-21 (design-level plan, commit 6177715b); refreshed into this executable plan 2026-11-21; r2 of 2026-11-22 after the review r1 (the response table is Appendix A).
- Base: main @ 4e413670 (`Merge branch 'plan/row-bound-retire'`). Code facts are those of 4e413670. Line numbers are hints; docs edits are addressed by quoted anchors (section 8), code edits by item names.
- Status: executable plan. User decisions of 2026-11-21 it implements: U-1 reclaim before sharing, the doorbell is a later item; U-2 the `eml_rt` part (RR-2) is in the stage, with the fallback to RR-3 alone if RR-2 fails its gate; U-3 the small-page sweep is deferred and its roadmap item rewritten; U-4 the stage is `remote-reclaim` and runs now, first in 「段の列」; U-5 the sleeping-owner retention is accepted and stated in the spec. Approving this plan is the agreement for the test changes of 7.4 and for 00 §D D-18 (S-RR-7).
- Design documents (`docs/plans/2026-11-17-runtime/`): `00-decisions.md` wins over everything; `01-design.md`. This file stands alone.
- Language: this plan is English. `docs/`, code comments and CLAUDE.md are Japanese. Every text marked "EN" here, and every comment line in the patches that starts with `EN:`, is an English draft: translate it with PLaMo (`form: plain`, identifiers and section names in `glossary`, earlier parts of the same document as `history`), take the PLaMo output as the starting point and change it only where a term, an identifier or a section name is wrong, and write it with no space between Japanese and Latin characters or inline code. Terms: worker, 取り込む (take in), 印を付ける (mark), 物体, 級 (size class), 大きい物体の列, 受けた数 (received count), 空きリスト, 止める語, 安全点.
- Tags: [V: proto] checked on a prototype of RR-2 and RR-3 in a scratch worktree at 4e413670 (removed); [V: read] read in the source; [V: grep] the command is shown; [V: measured] measured on macOS arm64; [I] inferred.
- Appendix directory `docs/plans/2026-11-21-remote-reclaim/` (on main since 6177715b; deleted at RR-5). RR-1 deletes its `prototype.patch` (made against 6ff1f664, third call in another place, no tests) and adds two patches, plain `git diff --no-ext-diff` with `a/` `b/` prefixes, which apply in this order to 4e413670 (`git apply --check` then `git apply`, each on the result of the one before) and rebuild the prototype tree byte for byte [V: proto, applied in order on a clean checkout and compared with `git diff`]: `rr-2-rt-reclaim.patch` (6 files, 249 lines), `rr-3-share-points.patch` (3 files, 147 lines). No docs are in the patches. RR-1 also adds the four gate scripts of 10.1: `gate-pairs.py`, `gate-rounds.sh`, `gate-rounds-file.sh`, `gate-workers.sh`. The probes (`p_*.em`, `f_*.em`, `t_ack_*.em`, `run.sh`, `watch.sh`) stay. The orchestrator copies the patches from `/private/tmp/claude-501/-Users-arakaki-Projects-eml/7165a97b-ee85-4236-86ab-b62e82dd973b/scratchpad/rr-appendix/`.

## 1. Goal and success criteria

- Goal: on the owner's thread, an engine calls `Ctx::reclaim_if_due` before it marks a value for another worker, so a task that never switches no longer keeps every block other workers free: it keeps what was alive at its last marking point and fewer than `REMOTE_RECLAIM` small blocks. `Heap::reclaim_if_due` takes the large pages whenever some wait and walks the pages only when 256 small blocks wait; the received count counts small blocks only.
- Success criteria:
  - S-1 (the leak is closed): `workers::a_try_send_loop_takes_in_the_frees_of_its_receiver` and `workers::a_spawn_loop_takes_in_the_frees_of_its_tasks` pass: after 20,000 hand-offs worker 0's pages are below 4 MiB. Measured on the prototype: 786,432 and 458,752 bytes with the calls; 18,481,152 (`try_send`) and 18,874,368 (`spawn`) without them, where both tests fail [V: proto]. Each also runs its loop with 600 items in checked mode at two workers and requires a clean end (S-RR-4). The whole `workers::` module (34 tests) runs in 0.27 s.
  - S-2 (the count rule): `remote::{checked,fast}::large_frees_do_not_count_toward_the_walk` and `trusted::{checked,fast}::a_due_reclaim_leaves_few_small_blocks` pass; without RR-2's source change all four fail by an assertion (`0 passed; 4 failed`) [V: proto].
  - S-3 (proofs): `cargo verus verify -p eml_rt` after `cargo clean -p eml_rt` → `1265 verified, 0 errors` (1265 at the base); no `external_body` or `assume(` added [V: proto].
  - S-4 (share sites): SC-2 prints exactly its five lines and SC-3 its counts (7.3.2).
  - S-5 (gates): `bench/rt-gate.sh <RR-1>` exits 0 on RR-2; the VM gate of 10.1 passes on RR-3.
  - S-6 (no existing test changes): 7.4's kind 1 and kind 2 lists are empty and stay empty [V: proto, the full workspace run].
  - S-7: after RR-5, `git grep -n 'remote-reclaim' -- docs ':!docs/plans'` is empty and `ls docs/plans | grep -c remote-reclaim` → `0`.
  - S-8: CI `success` on every job for the Close commit.

## 2. Scope

- In: `crates/eml_rt/src/remote.rs`, `heap/reclaim.rs`, `heap/ctx.rs`, `heap/ctx/recover.rs` with two tests (RR-2); `crates/eml_vm/src/vm.rs` (two calls), `crates/eml_sched/src/cells.rs` (`Depot::publish`), two tests in `crates/eml_vm/tests/workers.rs` (RR-3); the docs of section 8; 00 §D D-18; the roadmap.
- Out (owner): the `NOTICE` doorbell and the small-page sweep (later items, texts in S-RR-9); native code (it runs on one worker until `jit-tasks`; D-18 carries the rule there); any change of `Std.Chan`, `Std.Async`, the operation budget or a schedule.
- No new crate edge, feature, re-export, `pub` item, `unsafe` block or `unsafe fn`. `crates/eml_cli/tests/surface.rs` and `nix/crate-graph.sh` do not change [V: proto, the workspace run and clippy are green].

## 3. Background context (code at 4e413670 = base)

The defect [V: read; V: measured at 6ff1f664, the code of these paths is unchanged since]:
- `Ctx::reclaim_if_due` is called at four places of `crates/eml_vm/src/vm.rs`: `switch_away`, before sleeping in the idle loop, `complete_join`, after `take_job` / `drop_job`. `eml run` uses `Sched::Normal`, which never preempts, so a task that does not wait reaches none of them.
- `Chan.try_send` spends the operation budget but never yields; `Async.start` / `spawn` / `start_in` / `spawn_in` spend none (docs/spec/stdlib.md 「操作の予算」). A task that only uses them hands values to other workers forever without a reclaim point. `Chan.send` yields every 128 fast paths.
- A large object freed by another thread is refunded at the push and stays resident on the owner's list until the owner reclaims; a small block is not refunded and is not reusable until then, so the owner carves fresh pages.
- Measured at two workers, fast mode (probes in the appendix): a `try_send` loop of 1 MiB arrays with at most 5 alive reached 1.26 GB, linear in the run; the same through `spawn` 1.26 GB; through `send` 151 MB flat. With the prototype of the earlier round: 19 MB and 18.6 MB.
- It is not a memory-safety issue: the remote lists are linked through the freed blocks, and the push/take protocol (T14) does not depend on when the owner takes.

Run-time `Heap::share` sites [V: grep, SC-2's command at the base]: `eml_vm::vm::fork_child` (two calls; `par` and `compute`), `eml_sched::cells::Registry::start` (two calls, under the registry lock; its one caller is `eml_vm::vm::start_task`), `eml_sched::cells::Depot::publish` (one call; callers are the `AsyncFinishOk` and `ChanPushRaw` arms). The sixth hit, `crates/eml_nrt/src/entries.rs`, is inside that file's test module. A value leaves its worker only through these.

The reclaim today [V: read]: `Heap::reclaim_if_due_held` is due when `sink_large_pending` or `sink_count >= REMOTE_RECLAIM` (256), and then `reclaim_held` resets the count and reads the remote word of every page of the heap, also for one large page. `remote_push` and `remote_large_push` both add 1 to `RemoteSink.count`.

What the prototype showed (worktree at 4e413670, both patches applied, EN lines left as they are) [V: proto]:
- `cargo test --workspace -q --no-fail-fast` (on the r1 patches): exit 0; 31 test binaries; 5213 passed, 0 failed, 23 ignored. Six of the 5213 are new (7.4), so the base should have 5207 [I: the base itself was not run; RR-1 records the real number]. No `.snap.new` file and no changed snapshot. After the r2 changes (three assertions in one `trusted` test, one comment in `reclaim.rs`, the position of the call in `fork_child`, a checked run in the two `workers::` tests) the affected sets were run again: `cargo test -p eml_rt` and `cargo test -p eml_vm --test integration -- workers::` (34 passed); the whole workspace was not.
- `cargo test -p eml_rt`: 928 passed, 0 failed, 1 ignored (924 at the base).
- `cargo clean -p eml_rt` then `cargo verus verify -p eml_rt` (run in `crates/eml_rt`): `2102 verified, 0 errors` for vstd and `1265 verified, 0 errors` for `eml_rt`. The only edit inside a proof is the loop invariant `n <= self.core.pages.page_count()`. Run again on the r2 patches with the same two results.
- `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` exit 0 and `cargo fmt --check` exit 0, with each EN draft on one line. Translated comments wrap differently, so both are run again after the translation.
- `cargo clippy --all-targets -- -D warnings`: finished without a warning.
- Miri, `-Zmiri-strict-provenance`, and once more with `-Zmiri-tree-borrows`, on `trusted::{checked,fast}::` `a_due_reclaim_leaves_few_small_blocks`, `the_owner_carves_while_another_thread_reads_and_frees`, `two_remote_large_frees_are_both_reclaimed`, `a_remote_large_free_refunds_at_once`: `8 passed` in both settings (about 7 s of test time each). The r2 form of the new test ran again alone: `2 passed` in both settings.
- Red steps: 7.2 RR-2 step 1 and RR-3 step 1 give the failures stated there.
- Hot-path cost on this prototype [V: measured; release builds of 4e413670 and of 4e413670 with both patches, `EML_WORKERS=1`, instructions of the run part (the row minus `bench/empty.em`), five alternating rounds, each run under the watchdog]:

  | Row | Command | base | head | median delta | range of the 5 | per event |
  |---|---|---|---|---|---|---|
  | `pfib30` | `bench/pfib.em big` (about 1.35 M forks) | 19,756,257,749 | 19,764,998,238 | +0.038 % | +0.011..+0.045 | about 6.5 instructions per fork |
  | `par_tree` | `bench/par_tree.em` (15 forks) | 86,031,956 | 85,983,193 | -0.057 % | -0.803..+1.240 | noise |
  | `spread` | `bench/async/spread.em 100000 1` (100,000 task starts and ends) | 3,054,386,245 | 3,058,817,437 | +0.174 % | +0.086..+0.295 | about 44 per task |
  | `pingpong` | `bench/async/chan_pingpong.em` (160,000 pushes) | 3,351,849,094 | 3,357,549,161 | +0.180 % | +0.136..+0.216 | about 36 per push |

  The call is not "two loads" where nothing waits: `Ctx::reclaim_if_due` runs `settle` first and is reached through `Depot::publish` in another crate. All four are under the +0.5 % of 10.1.
- Wall time and peak RSS at 2 and 4 workers, median of 3, base against head [V: measured; a record, `gate-workers.sh` repeats it]: `pfib30` 1.11 s / 676 MB against 1.11 s / 676 MB (N = 2) and 1.11 s / 656 MB against 1.09 s / 658 MB (N = 4); `par_tree` 0.28 s / 15-16 MB in all four; `spread 8 30` 0.57 s against 0.57 s (N = 2), 0.40 s against 0.39 s (N = 4), 16-17 MB; `spread 100000 1` 0.42 s / 58 MB against 0.42 s / 58 MB (N = 2), 0.49 s / 55 MB against 0.50 s / 53 MB (N = 4); `chan_pingpong` 0.42 s against 0.42 s (N = 2), 0.83 s against 0.84 s (N = 4), 16 MB. No row moved.

Not run on the prototype: `bench/rt-gate.sh`, the full procedure of 10.1 (`bench/run.sh` pairs), `bench/async/scale.sh`, the whole `trusted::` under Miri, Miri on `eml_vm` and `eml_sched`, the sanitizers, the cross-target clippies, `cargo nextest run`, the full workspace on the r2 patches.

## 4. Prerequisites

| Needed | For | How to check | If missing |
|---|---|---|---|
| The dev shell (Rust 1.99.1, nextest, clippy, insta, Verus, verusfmt) | every unit | `cargo --version` → `cargo 1.99.1`; `cargo nextest --version`, `cargo verus --version`, `verusfmt --version` exit 0 | stop |
| macOS | `bench/rt-gate.sh` (RR-2), the gate of 10.1, the Close measurements | `uname` → `Darwin` | gates: stop for the user; Close: `Measure remote-reclaim` right after |
| The `.#cross` and `.#miri` shells | RR-2 step 3, checkpoint extras | `nix develop .#cross -c true`, `nix develop .#miri -c true` exit 0 | stop before RR-2 |
| `plamo-translate` (MCP tool or CLI) | Japanese text | tool listed or `plamo-translate --help` exit 0 | the `natural-japanese` skill (CLAUDE.md) |
| `gh` authenticated with push | checkpoints | `gh auth status` exit 0 | ask the user |
| The base | RR-1 | `git merge-base --is-ancestor 4e413670 HEAD` exit 0; `git diff --no-ext-diff --stat 4e413670 HEAD -- crates std tests bench` empty; `grep -c '^- D-17 ' docs/plans/2026-11-17-runtime/00-decisions.md` → `1`; `grep -c '^- D-18 ' ...` → `0` | stop |

Memory rule: a new or changed `.em` program, and every probe of the appendix, is run alone under `docs/plans/2026-11-21-remote-reclaim/watch.sh 2097152 CMD...` (timeout 20, kill above 2 GB of RSS); never two test binaries, or a test binary and an ad-hoc run, at once; a stuck test binary is killed before anything else. The probes `p_try.em`, `p_spawn.em` and `p_small_try.em` grow to 1.3 GB at the base by design.

A unit that finds it must change `crates/eml_rt` beyond RR-2's six files, `crates/eml_vm/src` beyond `vm.rs`, `crates/eml_sched/src` beyond `cells.rs`, `surface.rs` or `nix/crate-graph.sh` stops for the user.

## 5. Decisions

### 5.1 Long-term guidelines

| ID | Decision | Source |
|---|---|---|
| D-RR-1 | Rule for every engine: on the owner's thread, before a run marks a value for another worker with `Heap::share`, it calls `Ctx::reclaim_if_due` on the same `Ctx`. The call is outside the registry lock | U-1 |
| D-RR-2 | `RemoteSink.count` counts small blocks only. `reclaim_if_due` takes the list of large objects whenever it is not empty and walks the pages only when the count is `REMOTE_RECLAIM` (256) or more; it returns true when it did either. `Heap::reclaim`, `Ctx::reclaim` and recovery always walk and reset the count | U-2; confirmed [V: proto] |
| D-RR-3 | What a heap holds for blocks other threads freed is at most the objects alive at its last marking point and fewer than `REMOTE_RECLAIM` small blocks received since its last walk; with the pages of 「確保と解放」 that is the bound the docs state. Every channel push is a marking point, so `send` and `try_send` have the same bound. A sleeping worker takes its frees in when it next runs | U-5 |
| D-RR-4 | The doorbell and the small-page sweep are later items | U-1, U-3 |

Why a reclaim at these points frees nothing the task still uses: a reclaim empties only the set `remote_take` / `remote_large_take` promise (`Core::freed_set`), blocks whose last reference another thread dropped. A word in a VM register, on a task's stack, in the channel `Hold` or in the root table is a reference, so its target is not in the set. `held_set` names only the `Ctx`'s own words; the same argument already covers every other task's stack at `switch_away`. The value being published is alive when `publish` runs.

Conformance criteria (every unit's review checks those that apply):

| ID | Rule | Why | Applies | How to check |
|---|---|---|---|---|
| RR-G1 | MUST: every run-time `Heap::share` call is one of SC-2's five; `fork_child` and `Depot::publish` call `reclaim_if_due` before their first `share`; every caller of `Registry::start` calls it before it takes the registry lock | D-RR-1 | RR-3 on | SC-2, SC-3, reading `start_task` |
| RR-G2 | MUST NOT: a reclaim inside the allocator, `over_budget` or the registry lock | object-model.md 「別のスレッドからの解放」; vm.md 「セルと登録表」 | RR-2, RR-3 | the diff touches no file but 2's list; SC-5 |
| RR-G3 | MUST NOT: `external_body`, `assume(` or `unsafe` added in `crates/eml_rt/src` outside tests | the `eml_rt` gate | RR-2 | SC-6 |
| RR-G4 | MUST: every Japanese text of the stage comes from an EN draft through PLaMo; no `EN:` marker is left | CLAUDE.md | RR-2..RR-5 | SC-4 |
| RR-G5 | MUST: a kind 1 or kind 2 test change found during execution stops the unit; the orchestrator amends this plan with the user's approval | testing.md 「テストの変更の運用」 | all | review of each unit's diff against 7.4 |
| RR-G6 | MUST: the docs units cover every document section 8's grep finds | docs are the source of truth | RR-2..RR-5 | section 8's grep and table |
| RR-G7 | MUST: code and `docs/` outside `docs/plans/` cite no `docs/plans/` path and name no stage | CLAUDE.md | all | SC-1 |

### 5.2 Short-term decisions (this plan)

- S-RR-1 (`eml_rt`, code in `rr-2-rt-reclaim.patch`):
  - `remote_large_push`: the line `sink.count.fetch_add(1, Ordering::Relaxed);` is deleted. `remote_push` keeps its own.
  - `Heap::reclaim_if_due_held`: `let pages = sink_count(sink) >= REMOTE_RECLAIM; if pages || sink_large_pending(sink) { self.reclaim_held(Ghost(held), pages); true } else { ...false }`.
  - `Heap::reclaim_held(&mut self, Ghost(held), pages: bool)`: `if pages { sink_count_take(sink); }`; `let n = if pages { self.core.pages.pages_len() } else { 0 };`; the first loop's invariant `n == self.core.pages.page_count()` becomes `n <= ...`. Nothing else in the function changes.
  - Callers that pass `true`: `Heap::reclaim`, `Ctx::reclaim_when`'s `else` arm (`heap/ctx.rs`), recovery (`heap/ctx/recover.rs`).
- S-RR-2 (the three sites, code in `rr-3-share-points.patch`): `ctx.reclaim_if_due();` as
  - the first statement of `eml_vm::vm::fork_child`, with its comment above the existing comment on marking, which stays next to `fork_worker` and the `share` calls it describes;
  - the first statement of `eml_vm::vm::start_task`, before `unshared_roots` and before `sched.cells.lock()`;
  - the first statement of `eml_sched::cells::Depot::publish`.
- S-RR-3 (why `Depot::publish` and not the `ChanPushRaw` arm): the shared layer then carries the call for both engines, and `AsyncFinishOk` gets it too, which costs one call per task end.
- S-RR-4 (tests, 7.4): the two `eml_rt` tests pin D-RR-2; the two `workers::` tests pin D-RR-1 through `Control::bytes()` after the run, which is worker 0's high-water mark of pages (the other heaps are dropped after a successful run and pages of small objects are never returned). Each item is acknowledged through a `Std.Atomic.Counter` before the next hand-off, so every free has arrived and the tests do not depend on timing. Each test then runs the same loop with 600 items in checked mode at two workers (`run(&src.replace("20000", "600"), 2)`) and requires `Ok` and the output `600`: the checks at the end of a checked run (invariants, leaks, `check_owned`) then see heaps that reclaimed at the hand-offs. The page bytes are asserted in fast mode only, because checked mode quarantines and never reuses. No UI test: a UI file cannot read pages, and the rule is checked in one place.
- S-RR-5 (no test of the sleeping owner): the retention is an accepted limit, not a rule; no public reading shows a parked worker's waiting frees, and after the run everything is taken in.
- S-RR-6 (fallback, U-2): if RR-2 fails `bench/rt-gate.sh`, Verus, or Miri and the cause is not found in the unit, the user decides; the default is to take RR-2's commit back (`git reset --hard <RR-1>` on the unpushed branch), run RR-3 alone, and add the later item of S-RR-9 (c). RR-3 does not depend on RR-2's code: its tests use small objects, and its patch applies to the tree without RR-2 [V: the two patches touch disjoint files].
- S-RR-7 (00 §D D-18, appended by RR-1 after D-17; English, as that file is):
  > D-18 (2026-11-21; user decisions of 2026-11-21 on the case the final review of `std-par` left untraced): the stage `remote-reclaim` is outside the group, first in 「段の列」 before `native-checked`, 前提: なし. (a) A task that never switches (`Chan.try_send` or `Async.start` in a loop) kept every block other workers freed: at two workers a loop that sends 1 MiB arrays held 1.26 GB with five arrays alive. Rule for every engine: on the owner's thread, before a run marks a value for another worker with `Heap::share`, it calls `Ctx::reclaim_if_due` on the same `Ctx`, outside the registry lock. In the VM the sites are `fork_child`, `start_task` and `Depot::publish`. (b) `RemoteSink`'s received count counts small blocks only; `reclaim_if_due` takes the list of large objects whenever it is not empty and walks the pages only at `REMOTE_RECLAIM`. (c) Amends 01 without editing it: §2.3 `drive` calls `reclaim_if_due` at item close and before it parks (step 6); §2.5 step 1 and §11.4 `eml_nrt_fork` call it before `fork_worker`; §11.4 `eml_nrt_join` calls it after `wait_join`; every native row that calls `Registry::start` calls it before the registry lock; `Depot::publish` carries its own. §18 gains the criterion: every run-time call of `Heap::share` in `eml_sched` and `eml_nrt` is preceded by `reclaim_if_due` on the same `Ctx`, and no other run-time `share` call exists. (d) Restates D-17: `native-checked`'s base is `Close remote-reclaim`. (e) Later items: a `NOTICE` doorbell from the freeing thread, and returning or reusing pages of small objects.
- S-RR-8 (the 「段の列」 entry, added by RR-1 as the first entry, before the line anchored 「- `native-checked`: ネイティブのコードが受け入れるプログラムに」; EN):
  > `remote-reclaim`: A worker takes in the frees it received from other threads before it marks a value for another worker. The points are the fork of `par` and `compute`, the start of a task, and `Depot::publish`, which deposits a value. A task that never switches because it only repeats `Chan.try_send` or `Async.spawn` then no longer keeps the blocks other workers freed: what a heap holds for such blocks is at most the objects that were alive at its last such point and fewer than `REMOTE_RECLAIM` small blocks. `Heap::reclaim_if_due` takes the list of large objects out whenever it is not empty and walks the pages only when the number of small blocks received reaches `REMOTE_RECLAIM`; the received count counts small blocks only. 前提: なし. 設計: [オブジェクトモデル](../spec/object-model.md)の「別のスレッドからの解放」
  The Japanese of the earlier round (PLaMo output put into the repository's terms; use it as `history`; its third sentence predates the words "and fewer than `REMOTE_RECLAIM` small blocks", which the translation adds):
  > - `remote-reclaim`: workerは、値にほかのworkerの印を付ける前に、ほかのスレッドから受けた解放を取り込む。その地点は、`par`と`compute`のfork、タスクを始める行、値を預ける`Depot::publish`である。`Chan.try_send`や`Async.spawn`だけを繰り返して切り替えないタスクも、ほかのworkerが空けたブロックを持ち続けなくなる。ヒープがそういうブロックのために持つ量は、最後のその地点で生きていた物体までになる。`Heap::reclaim_if_due`は、大きい物体の列が空でなければそれを取り出し、ページを歩くのは受けた小さいブロックの数が`REMOTE_RECLAIM`に届いたときだけにする。受けた数は小さいブロックだけを数える。前提: なし。設計: [オブジェクトモデル](../spec/object-model.md)の「別のスレッドからの解放」
- S-RR-9 (the later items, written by RR-5 into 「その後の項目」の「マルチコア対応」; EN, each with its two nested bullets 後に回した理由 / 見直す条件):
  - (a) "Telling the owner about frees from another thread: the freeing thread raises `NOTICE` on the owner's stop word, and the owner takes the frees in at its next safepoint." Reason: "Because a worker takes them in before it marks a value, what a heap holds for blocks other threads freed is at most the objects alive at its last such point and fewer than `REMOTE_RECLAIM` small blocks. A notice needs the heap to know its worker's stop word, and a sleeping worker would also have to be woken." Condition: "When the peak RSS (`/usr/bin/time -l`, `bench/scripts.sh`) of a program that marks values rarely is, in a run with two or more workers, more than twice its peak RSS in a run with one worker."
  - (b) replaces the item anchored 「- ページを OS に返すこと」 with its two nested lines: "Returning pages of small objects and reusing them across size classes: a freed block goes back to the free list of its class, and while the heap lives its page is neither used by another class nor returned. The pages of a heap are therefore at most the sum, over the size classes, of the most pages each class needed at one time. A program whose sizes move from class to class keeps the pages of every earlier class (a measurement that used 12 classes in turn held 8 times the pages of the same work in one class)." Reason: "The amount is bounded, and no long-running program limited by it has been measured. The default design is a sweep at a point the engine chooses: it counts the free blocks of each page, unlinks the blocks of the pages that are entirely free from the free list, and returns those pages or carves them for another class. It leaves the allocation and free paths and the page layout unchanged." Condition: "When the peak RSS of a long-running program (a server, the REPL, a GUI application) keeps growing with the length of the run although its live data does not grow, or when the memory limit stops such a program. `RunStats` has no byte count, so this item first adds a way to read the bytes of pages and the pages of each class in fast mode. Decide it before a stage expands the allocation of small objects inline in native code."
  - (c) only under S-RR-6's fallback: "Walking the pages only for small blocks: `reclaim_if_due` walks every page whenever a large page waits." Reason: "<the gate row or proof that failed>." Condition: "When a program that hands off large objects from a large heap spends a measurable share of its time in `reclaim_held`."

## 6. Implementation constraints and conventions

- One unit = one commit; each passes G (7.3.1) and the SC lines in force (7.3.2). `cargo fmt` before every commit; in RR-2 also `verusfmt --verus-only crates/eml_rt/src/remote.rs crates/eml_rt/src/heap/reclaim.rs` after the comments are written (the tool re-wraps long comment lines, and `--check` fails otherwise).
- Take each unit's patch, then replace every `EN:` line by the PLaMo translation of its draft (language rule in the header). Japanese comments wrap at about 110 columns like their neighbours; the patches hold each draft on one line. A translated `///` or `//!` block keeps its place in the doc comment.
- `docs/spec/` text is written in the commit that implements the rule (each unit's "Docs" part). An anchor is a quoted substring that `git grep -cF -e '<anchor>' 4e413670 -- <file>` counts once [V: every anchor of 7.2 and section 8 counted 1; the `-e` is needed for anchors that begin with `-`]. The edit applies to the sentence, bullet or table row that contains the anchor. Code comments cite `docs/spec/` and `docs/implementation/` only; docs name no stage.
- In every paragraph, bullet or table row of `docs/` that a unit touches, remove the spaces between Japanese and Latin characters or inline code in the whole paragraph, not only in the sentence replaced (CLAUDE.md). "The rest of the bullet stays" below means its wording stays; its spacing is normalised. SC-8 checks the added lines.
- Quote every glob (zsh). `git diff` in checks takes `--no-ext-diff`. The bare directory form `'crates/*/src'` matches nothing as a `git diff` pathspec; checks here write `':(glob)crates/*/src/**'` or name directories.
- No snapshot is expected to change. A `.snap.new` file is a finding under RR-G5, not something to accept.
- Stop any Bash call moved to the background before finishing a unit.

## 7. Implementation tasks

### 7.1 Task table

| ID | Commit title | After | Decisions / criteria | Executor |
|---|---|---|---|---|
| RR-1 | `Start remote-reclaim` | - | this plan, 00 §D D-18, the roadmap entry, the recorded baseline (N0 and the skipped count) | orchestrator |
| RR-2 | `Count only small blocks toward the walk of remote frees` | RR-1 | D-RR-2, S-RR-1; S-2, S-3; 10.2 | Sonnet from `rr-2-rt-reclaim.patch`; Fable reviews the diff and 7.5 before the commit (CLAUDE.md, the trusted core) |
| RR-3 | `Take in remote frees before marking a value for another worker` | RR-2 | D-RR-1, D-RR-3, S-RR-2..S-RR-5; S-1, S-4 | Sonnet from `rr-3-share-points.patch`; Opus reviews against 7.5 |
| CP-A | push, CI of RR-3, checkpoint extras, the gate of 10.1 | RR-3 | S-5 | orchestrator |
| RR-4 | `Rewrite the future docs around the reclaim points` | CP-A | section 8 | Sonnet: prose from drafts and anchors |
| RR-5 | `Close remote-reclaim` | RR-4 | roadmap 「進め方」 | Sonnet: steps listed |
| CP-Z | CI of RR-5, then merge | RR-5 | S-8 | orchestrator |

### 7.2 Units

"N" is the `cargo nextest run` total, all passing. N0 is the number RR-1 records at the base.

#### RR-1: `Start remote-reclaim`

1. On main at the base: `git switch -c plan/remote-reclaim`.
2. Replace `docs/plans/2026-11-21-remote-reclaim.md` with sections 1-10 of this plan. In `docs/plans/2026-11-21-remote-reclaim/`: `git rm prototype.patch`; add `rr-2-rt-reclaim.patch`, `rr-3-share-points.patch` and the four `gate-*` scripts. The old file cited the path of the deleted plan of the previous stage; this plan cites no other stage's plan path. Check: `git grep -nE 'plans/2026-11-21-row-bound[-]retire' -- .` → empty.
3. Append S-RR-7's D-18 to `docs/plans/2026-11-17-runtime/00-decisions.md` §D, after D-17.
4. docs/future/roadmap.md 「段の列」: add S-RR-8's entry as the first entry (translated from its EN draft).
5. Baseline: section 9's table on the clean tree; fill "Recorded". `cargo nextest run` gives the real N0 and the real skipped count; write both into 7.3.1 and section 9 in place of 5207 and 23 before the commit (they are inferred from `cargo test`'s `passed` and `ignored`). Any other count that differs is corrected in the plan before the commit if a commit merged after 4e413670 explains it; otherwise stop.
6. Commit; message: D-18, the defect in one paragraph with the measured numbers, the user decisions, the baseline.
- Completion: `git diff --no-ext-diff --name-status HEAD~1 HEAD` → `M` the plan, `M` `00-decisions.md`, `M` `docs/future/roadmap.md`, `D` `prototype.patch`, `A` the two patches and the four gate scripts; N = N0; ``sed -n '/^## 段の列/,/^## その後の項目/p' docs/future/roadmap.md | grep -c '^- `'`` → `10`; SC-8; `cargo nextest run -p eml_cli --test integration -E 'test(/^citations::/)'` passes.

#### RR-2: `Count only small blocks toward the walk of remote frees`

Work (`rr-2-rt-reclaim.patch`, six files; its nine `EN:` lines are drafts to replace: `remote.rs` 4, `reclaim.rs` 3, the two tests 1 each). The only edit inside a Verus proof is the invariant of `reclaim_held`'s first loop, `n == self.core.pages.page_count()` → `n <= self.core.pages.page_count()`; no lemma, `requires` or `ensures` changes. `verusfmt --verus-only` is run on `remote.rs` and `heap/reclaim.rs` after the comments are translated, because it re-wraps them:
- `crates/eml_rt/src/remote.rs`: S-RR-1's deletion in `remote_large_push`. Drafts: the doc of `RemoteSink.count` ("The number of small blocks pushed since the owner last walked its pages. Large pages are not counted; the owner sees them by `large` being non-null. It is a hint for when to walk, and it is exact at a stop."); the line added to `remote_large_push`'s doc ("The received count is not raised: the owner takes this list out whenever it is not empty, without walking its pages."); the doc of `sink_count` ("The number of small blocks received since the last walk of the pages (T14). A hint; other threads may add to it at the same time."); the doc of `sink_count_take` ("Resets the received count to 0 before a walk of the pages (T14).").
- `crates/eml_rt/src/heap/reclaim.rs`: S-RR-1's `reclaim_if_due_held` and `reclaim_held`. Drafts: the doc of `REMOTE_RECLAIM` ("When the number of small blocks received reaches this, `reclaim_if_due` walks the pages (docs/spec/object-model.md 「別のスレッドからの解放」). Large pages are not counted."); the doc of `Heap::reclaim_if_due` ("Takes the list of large objects out when it is not empty, and walks the pages only when the received count of small blocks has reached `REMOTE_RECLAIM`. Returns true when it did either; with fewer small blocks waiting it leaves them where they are (docs/spec/object-model.md 「別のスレッドからの解放」)."); the line added to `reclaim_held`'s doc ("With `pages` false it neither resets the received count nor walks the pages, and takes only the list of large objects.").
- `crates/eml_rt/src/heap/ctx.rs`, `heap/ctx/recover.rs`: the argument `true`.
- Not in the patch, same file `heap/ctx.rs`: the doc of `Ctx::reclaim_if_due` lists the VM's points (anchor 「VM は待つ前、タスクを切り替えるとき、」). Replace the sentence from that anchor to the end of the comment by (EN) "The engine calls it at the points docs/spec/object-model.md 「別のスレッドからの解放」 lists." so that RR-3 need not touch `eml_rt`.
- `crates/eml_rt/src/heap/tests/remote.rs`: `large_frees_do_not_count_toward_the_walk<M: Mode>`, listed in `both_modes!` after `reclaim_if_due_waits_for_enough_frees`. Draft: "A large page freed by another heap does not count toward the walk of the pages: with 255 small blocks waiting, a large free makes `reclaim_if_due` take the large page only, and the 256th small block makes it walk."
- `crates/eml_rt/src/heap/tests/trusted.rs`: `a_due_reclaim_leaves_few_small_blocks<M: Mode>` before `two_threads_free_into_one_page`, listed in `both_modes!` after `two_remote_large_frees_are_both_reclaimed`. It asserts `remote_pending()` after the first and the second `reclaim_if_due` and `!remote_pending()` after `reclaim`, which holds in both modes only with the new rule. Draft: "Another thread frees a short shared list and one large object. The owner's `reclaim_if_due` takes the large page back at once and leaves the small blocks, whose count is below `REMOTE_RECLAIM`, on the remote lists, so frees are still pending; a `reclaim` takes them and nothing is pending."

Docs:
- docs/spec/object-model.md 「別のスレッドからの解放」:
  - anchor 「積んだ後で、`RemoteSink` の受けた数に 1 を足す」: the first two sentences of the bullet (up to 「止まった時点では正確になる。」) become (EN) "After pushing a small block, the thread adds 1 to the received count of the `RemoteSink`. A large page is not counted: the owner sees it by the list of large objects being non-empty. The count is the number of small blocks pushed since the owner last walked its pages; it is a hint for when to walk, and it is exact at a stop." The rest of the bullet stays.
  - anchor 「`Heap::reclaim_if_due` は、大きい物体の列が空でないか」: the first sentence (up to 「真を返す。」) becomes (EN) "`Heap::reclaim_if_due` takes the list of large objects out when it is not empty, and walks the pages only when the received count is `REMOTE_RECLAIM` (256) or more; it returns true when it did either. It leaves the small blocks on the remote lists while fewer than `REMOTE_RECLAIM` of them have been received since the last walk." The rest of the bullet stays in this unit.
  - read and keep: the bullet anchored 「- 持ち主は`Heap::reclaim`で取り込む」 (`reclaim` still resets the count and walks every page); the T14 row of 「`unsafe` の核」 (anchor 「`sink_count`、`sink_count_take`、`sink_large_pending`」: it promises nothing about the values).
- docs/implementation/testing.md:
  - anchor 「受けた数が `REMOTE_RECLAIM` に届くまで `reclaim_if_due` が取り込まないこと」: after that clause add (EN) "that a large page freed by another heap does not count toward that number".
  - after the bullet anchored 「返ってきた大きい物体のページを見て、次の `reclaim_if_due` が取り込み」 add a bullet (EN) "Another thread frees a short list and a large object: the next `reclaim_if_due` takes the large page back and leaves the small blocks, and a `reclaim` takes them."

Order:
1. Red step: apply only the two test files of the patch; `cargo test -p eml_rt --lib -- a_due_reclaim_leaves large_frees_do_not` → `0 passed; 4 failed`, each by an assertion: `remote::fast` at `assert!(a.reclaim_if_due())`, `remote::checked` at its first `assert_eq!`, and both `trusted` variants at the first `assert!(a.remote_pending())` [V: proto].
2. Apply the rest; replace the drafts; the `ctx.rs` comment; `verusfmt --verus-only` on the two files; `cargo fmt`.
3. Before the commit: `cargo test -p eml_rt --lib` → `928 passed; 0 failed; 1 ignored`; `cargo clean -p eml_rt`, then in `crates/eml_rt` `cargo verus verify -p eml_rt` → `1265 verified, 0 errors` (without the clean, cargo verus skips an unchanged crate and prints nothing); `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` exit 0; Miri on the new and the neighbouring tests, twice:
   `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt --lib -- trusted::checked::a_due_reclaim_leaves trusted::fast::a_due_reclaim_leaves trusted::checked::the_owner_carves trusted::fast::the_owner_carves trusted::checked::two_remote_large trusted::fast::two_remote_large trusted::checked::a_remote_large_free trusted::fast::a_remote_large_free` → `8 passed`, and once more with `-Zmiri-tree-borrows` added → `8 passed` [V: proto, both, on the r1 form; the r2 form of the new test alone: `2 passed` in both]. A Miri error stops the unit for Fable. Then G and Fable's review of the diff.
4. Commit.
5. After the commit, on the clean tree: `bench/rt-gate.sh <RR-1's commit>` (10.2). Exit 0: the unit is done. Any other exit: nothing is pushed and RR-3 does not start; S-RR-6.
- Completion: G (N = N0 + 4); steps 1, 3 and 5 as stated; SC-8; `git diff --no-ext-diff --name-only HEAD~1 HEAD` → the six files of the patch, `docs/spec/object-model.md`, `docs/implementation/testing.md`; SC-4 empty; SC-6.
- Left to the implementer: the wording of the Japanese texts.

#### RR-3: `Take in remote frees before marking a value for another worker`

Work (`rr-3-share-points.patch`, three files; its eight `EN:` lines are drafts to replace: `vm.rs` 2, `cells.rs` 1, `workers.rs` 5):
- `crates/eml_vm/src/vm.rs`: S-RR-2's two calls. Drafts: in `fork_child` ("Before marking a value for another worker, take in the frees received from other threads (docs/spec/object-model.md 「別のスレッドからの解放」)"); in `start_task` ("`Registry::start` marks the closure under the registry lock, so the frees received from other threads are taken in here, before the lock (docs/spec/object-model.md 「別のスレッドからの解放」)").
- `crates/eml_sched/src/cells.rs`: the call in `Depot::publish`, with the first draft again.
- `crates/eml_vm/tests/workers.rs`, before `the_fast_mode_runs_the_data_parallel_functions_on_four_workers`: the helper `pages_of_worker_0(src, stdout) -> usize` and the two tests. Drafts: the helper ("Runs `src` in fast mode on two workers and returns the bytes of the pages worker 0's heap holds after the run. The other workers' heaps are dropped after a successful run and pages of small objects are never returned, so the value is the most pages worker 0 held (docs/spec/object-model.md 「確保と解放」)."); `a_try_send_loop_takes_in_the_frees_of_its_receiver` ("A task that hands off items with `Chan.try_send` never switches. The receiver on the other worker drops each item and then counts it, and the sender waits for the count before the next item, so every free has arrived at the next hand-off. The sender takes them in before it marks the next item, so its pages do not grow with the number of items (docs/spec/object-model.md 「別のスレッドからの解放」)."); `a_spawn_loop_takes_in_the_frees_of_its_tasks` ("The same with `Async.spawn`: each task drops the array its closure holds and then counts it."); and, in both tests, the comment above the checked run ("The same loop, shorter, ends cleanly in checked mode: the audits at the end of the run see the heaps after the reclaims at the hand-offs.").

Docs:
- docs/spec/object-model.md 「別のスレッドからの解放」, the bullet anchored 「`Heap::reclaim_if_due` は、大きい物体の列が空でないか」:
  - its sentences 「取り込む地点は VM が決める。待つ前、タスクの切り替え、`par` の join の後、run の終わりの検査の前である (…)。」 become (EN) "The engine decides the points: before waiting, at a task switch, after the join of `par`, before it marks a value for another worker (the fork of `par` and `compute`, the start of a task, and `Depot::publish`), and before the checks at the end of a run ([VM](vm.md) 「タスクとスケジュール」「`par` の fork と join」「子の終わりと join」「`Async` のタスク」「時計と driver」「誤り、止めること、終わり」)." The bullet's last sentence stays.
  - after the bullet add two bullets (EN): "A value leaves its worker only where the engine marks it. So the blocks other threads freed that a heap has not yet taken in were alive at that heap's last marking point: besides the pages its size classes needed (「確保と解放」 above), a heap holds at most the objects that were alive then and fewer than `REMOTE_RECLAIM` small blocks received since its last walk. Every push to a channel is a marking point, for `send` and for `try_send`." and "A worker that sleeps does not take in the frees it receives while asleep; it takes them in when it next runs. The amount is at most what it had allocated before it slept."
  - anchor 「決めていないこと: 小さい物体のページを OS に返すこと」 (in 「確保と解放」): before that paragraph add a bullet (EN) "A page serves one size class as long as the heap lives. The pages of small objects of a heap are therefore at most the sum, over the size classes, of the most pages each class needed at one time."
- docs/spec/vm.md:
  - after the bullet anchored 「- ほかのタスクに切り替えるたびに、ほかのスレッドから受けた解放が溜まっていれば」 add (EN) "The VM also calls `Ctx::reclaim_if_due` before it marks a value for another worker: at the head of the fork of `par` and `compute`, before `start` takes the registry lock, and in `Depot::publish`. A task that only starts tasks or calls `try_send` never switches, and a value leaves its worker only at these places ([オブジェクトモデル](object-model.md) 「別のスレッドからの解放」)."
  - anchor 「2. `heap.fork_worker()` で右の子の ID を出す」: the step becomes (EN) "Calls `Ctx::reclaim_if_due`, then makes the right child's ID with `heap.fork_worker()`".
  - anchor 「- `start_raw g`: 今の `Task` のセルの今のスコープに」: after the bullet's first sentence add (EN) "Before the registry lock is taken, the VM calls `Ctx::reclaim_if_due`."
  - anchor 「- 受け手の決まらない値の預かり先の操作は `Depot` にまとめる」: 「`publish` (`share`)」 becomes 「`publish` (`Ctx::reclaim_if_due`の後に`share`)」.
  - read and keep: 「2. forkの手順2と3と同じく、仕事のIDを出し」 (it points at the step above); 「| `push_raw` | `publish` (lock の外)」; the bullet on what runs under the registry lock.
- docs/implementation/testing.md, anchor 「- `workers.rs`は、workerを作る時と止める手順」: add to the list (EN) "that a task which only hands values off with `Chan.try_send` or `Async.spawn` does not grow its pages with the number of items".

Order:
1. Red step: apply only `crates/eml_vm/tests/workers.rs`; `cargo test -p eml_vm --test integration -- workers::a_try_send_loop workers::a_spawn_loop` → `0 passed; 2 failed`, the panic messages `18481152` and `18874368` (the page bytes) [V: proto].
2. Apply the rest; replace the drafts; `cargo fmt`.
3. `cargo test -p eml_vm --test integration -- workers::a_try_send_loop workers::a_spawn_loop` → `2 passed` (about 0.2 s; fast-mode values 786,432 and 458,752 on the prototype); repeat it five times. Run the probes of the appendix, each alone, under the memory rule: `watch.sh 2097152 target/release/eml run --workers 2 <probe>` for `p_try.em`, `p_spawn.em`, `p_small_try.em` after `cargo build --release -p eml_cli`: none is killed and each ends with `exit=0` (at the base `p_try.em` and `p_spawn.em` reach 1.26 GB).
4. G; Opus's review; commit.
- Completion: G (N = N0 + 6); steps 1 and 3; `git diff --no-ext-diff --name-only HEAD~1 HEAD` → `crates/eml_vm/src/vm.rs`, `crates/eml_sched/src/cells.rs`, `crates/eml_vm/tests/workers.rs`, `docs/spec/object-model.md`, `docs/spec/vm.md`, `docs/implementation/testing.md`; SC-2, SC-3, SC-4, SC-8.

#### CP-A

Push `plan/remote-reclaim`; CI `success` on RR-3; checkpoint extras (7.3.3); the gate of 10.1.

#### RR-4: `Rewrite the future docs around the reclaim points`

docs/future/multicore.md (section 8's rows):
- anchor 「`RemoteSink` は受けた数を atomic に数える」 → (EN) "`RemoteSink` counts the small blocks it received atomically".
- the bullet anchored 「- 持ち主のヒープが取り込むのは、VM が決める地点」: its first two sentences become (EN) "The owner's heap takes them in with `reclaim_if_due` at the points the engine decides (before waiting, at a task switch, after a join, before it marks a value for another worker, before the checks at the end of a run), and before an operation that walks the pages. `reclaim_if_due` takes the list of large objects out whenever it is not empty and walks the pages when the received count is 256 or more." The rest stays.
- the bullet anchored 「- 小さい物体のページは run の間 OS に返さない」 becomes (EN) "Pages of small objects are not returned to the OS during a run and are not used by another size class. The pages of a heap are therefore at most the sum, over the size classes, of the most pages each class needed at one time, plus what it holds for blocks other threads freed, which is at most the objects alive at its last marking point and fewer than `REMOTE_RECLAIM` small blocks ([オブジェクトモデル](../spec/object-model.md) 「別のスレッドからの解放」)."
- the bullet anchored 「- **ページを OS に返すこと**」 becomes (EN) "**Returning pages of small objects and reusing them across classes**: the default does neither, and the bound is the one of 「別のスレッドからの解放」 above. The roadmap item states when to revisit."
- Completion: G (N = N0 + 6); `citations::` passes; SC-8; section 8's grep hits only lines its table lists.

#### RR-5: `Close remote-reclaim`

Roadmap 「進め方」, one commit:
1. spec: `grep -rn 'remote-reclaim' docs/spec docs/implementation` → empty; no 「今は〜、後で〜」 in the sentences this stage wrote.
2. Measure (`crates/eml_vm`, `crates/eml_rt` changed): `bench/scripts.sh` first (a row out of range → `Resize bench rows`); then `bench/run.sh`, `bench/rt.sh`, `bench/scripts.sh` on the clean tree of RR-4 → the three tables of benchmarks.md 「今の数値」. 「ネイティブの数値」 is not touched (not a native stage).
3. status.md, architecture.md, overview.md, docs/README.md, CLAUDE.md, `crates/eml_rt/CLAUDE.md`: no change (section 8).
4. roadmap.md: delete the entry anchored 「- `remote-reclaim`: 」 from 「段の列」; in 「その後の項目」の「マルチコア対応」 replace the item anchored 「- ページを OS に返すこと」 (three lines) by S-RR-9 (b) and add S-RR-9 (a) after it; under the fallback also (c). `git grep -n 'remote-reclaim' -- docs/future/roadmap.md` → empty.
5. Delete `docs/plans/2026-11-21-remote-reclaim.md` and `docs/plans/2026-11-21-remote-reclaim/`. No other plan file is edited: `native-checked`'s "Base:" line is rewritten at its own Start, from D-18 (d).
6. Message: the defect and the measured numbers before and after, D-18's date and the user decisions, the gate tables of 10.1 and 10.2, the measurements, the two later items, that no existing test changed (or the amendment made under RR-G5).
- Completion: G; `citations::` passes; SC-8; S-7; ``sed -n '/^## 段の列/,/^## その後の項目/p' docs/future/roadmap.md | grep -c '^- `'`` → `9`.

### 7.3 Gates and standing checks

#### 7.3.1 G (every unit, repository root, after `git add -A`)

1. `cargo nextest run` → exit 0; `N tests run: N passed[ (k slow)], K skipped` with K the skipped count RR-1 recorded (23 expected) and N = N0 (RR-1), N0 + 4 (RR-2), N0 + 6 (RR-3 on). If N differs, list with `cargo nextest list` and compare with 7.4's names; a difference 7.4 does not explain stops the unit.
2. `cargo clippy --all-targets -- -D warnings` → exit 0.
3. `cargo fmt --check` → exit 0.
4. `bash nix/crate-graph.sh --check` → exit 0.
5. No snapshot or expectation changes: `git status --porcelain | grep -c '\.snap'` → `0`, and `git diff --no-ext-diff --stat HEAD -- crates tests` lists only the unit's files of 7.2.
6. `cargo nextest run -p eml_cli --test integration -E 'test(/^citations::/)'` passes.

#### 7.3.2 SC (standing checks; `<RR-1>` is that unit's commit)

```sh
# SC-1 (always; RR-G7): no docs/plans citation in code or docs
grep -rn 'docs/plans/20' crates docs std --include='*.rs' --include='*.md' --include='*.em' | grep -v '^docs/plans/'   # empty
# SC-2 (always; RR-G1): the run-time share sites, by file and enclosing item
git grep -n '\.share(' -- crates/eml_vm/src crates/eml_sched/src crates/eml_nrt/src | grep -v 'crates/eml_nrt/src/entries.rs' | cut -d: -f1 | sort | uniq -c
#   3 crates/eml_sched/src/cells.rs      (two in Registry::start, one in Depot::publish)
#   2 crates/eml_vm/src/vm.rs            (both in fork_child)
git grep -c '\.share(' -- crates/eml_nrt/src                                # crates/eml_nrt/src/entries.rs:1 (in its test module)
# SC-3 (always; RR-G1): the reclaim calls
git grep -c 'reclaim_if_due()' -- crates/eml_vm/src/vm.rs crates/eml_sched/src/cells.rs
#   before RR-3: crates/eml_vm/src/vm.rs:4            from RR-3: crates/eml_sched/src/cells.rs:1 and crates/eml_vm/src/vm.rs:6
git grep -c 'sink.count.fetch_add' -- crates/eml_rt/src/remote.rs           # crates/eml_rt/src/remote.rs:2 before RR-2, :1 from RR-2 (remote_push only)
# SC-4 (RR-2 on; RR-G4): no draft marker left (the patches bring 9 and 8)
git grep -nE '(//|--|#) ?EN: ' -- std crates tests bench                    # empty
# SC-5 (always; RR-G2): the files the stage may change
git diff --no-ext-diff --name-only <RR-1> HEAD -- ':(glob)crates/**'
#   RR-2: crates/eml_rt/src/{remote.rs,heap/reclaim.rs,heap/ctx.rs,heap/ctx/recover.rs,heap/tests/remote.rs,heap/tests/trusted.rs}
#   RR-3 on: also crates/eml_vm/src/vm.rs, crates/eml_sched/src/cells.rs, crates/eml_vm/tests/workers.rs; nothing else
git diff --no-ext-diff --stat <RR-1> HEAD -- crates/eml_cli/tests/surface.rs nix/crate-graph.sh   # empty
# SC-6 (RR-2 on; RR-G3)
git diff --no-ext-diff <RR-1> HEAD -- ':(glob)crates/eml_rt/src/**' ':(exclude,glob)crates/eml_rt/src/heap/tests/**' | grep -cE '^\+.*(external_body|assume\(|unsafe)'   # 0
git grep -cE 'external_body|assume\(' -- crates/eml_rt/src/remote.rs        # 14, as at the base
# SC-7 (always): eml_nrt's rule is kept
grep -rn 'rows::call\|#\[used\]' crates/eml_nrt/src                          # empty
# SC-8 (every unit that edits docs/, on its own commit before committing: replace HEAD~1 HEAD by --cached): no space between Japanese and Latin or inline code in an added line
git diff --no-ext-diff -U0 HEAD~1 HEAD -- docs ':!docs/plans' | grep '^+' | grep -v '^+++' | perl -CSD -ne 'print if /[\p{Han}\p{Hiragana}\p{Katakana}] [A-Za-z0-9`]|[A-Za-z0-9`] [\p{Han}\p{Hiragana}\p{Katakana}]/'   # empty
```

At the base [V: grep at 4e413670]: SC-1 is empty; SC-2 prints the two counted lines and `entries.rs:1`; SC-3 prints `crates/eml_vm/src/vm.rs:4` and `crates/eml_rt/src/remote.rs:2`; SC-4 is empty; `git ls-files ':(glob)crates/*/src/**' | wc -l` → `347`, `git ls-files ':(glob)crates/**' | wc -l` → `1677`, and SC-6's pathspec with its exclude lists 51 files, so the pathspecs of SC-5 and SC-6 are not vacuous; SC-6's second line prints `14`; SC-7 is empty. SC-8's filter is not vacuous: on the docs diff of `b1bbcb92` (the previous stage's Close) it prints 2 lines [V].

#### 7.3.3 Checkpoint extras (before CP-A and CP-Z)

```sh
nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run -- -D warnings   # exit 0
nix develop .#cross -c cargo clippy --target x86_64-unknown-linux-gnu --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run -- -D warnings  # exit 0
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::                        # pass; and once more with -Zmiri-tree-borrows
nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=thread TSAN_OPTIONS=halt_on_error=1 CARGO_TARGET_DIR=target/tsan cargo test -Zbuild-std --target $(rustc -vV | sed -n 's/^host: //p') -p eml_rt trusted::   # pass
nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=thread TSAN_OPTIONS=halt_on_error=1 CARGO_TARGET_DIR=target/tsan cargo test -Zbuild-std --target $(rustc -vV | sed -n 's/^host: //p') -p eml_vm --test integration -- workers:: signal:: io:: chan:: ffi:: ffi_abandon:: seat:: pinned::   # pass (CI's set; the two new tests cross threads through the changed paths)
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::       # pass; and once more with -Zmiri-tree-borrows (RR-3 changes vm.rs)
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_sched --lib -- cells::a_read cells::a_write   # pass; and once more with -Zmiri-tree-borrows (RR-3 changes cells.rs)
nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=address CARGO_TARGET_DIR=target/asan cargo test -Zbuild-std --target $(rustc -vV | sed -n 's/^host: //p') -p eml_vm --test integration -- api:: signal:: io:: workers:: ffi:: ffi_abandon:: seat:: pinned::   # pass
```

The Miri budgets (`eml_rt` `trusted::`: 280 s Stacked Borrows, 365 s Tree Borrows of `user` time, and one test may add 5 s under Tree Borrows; `eml_vm` `miri::`: 300 s and 400 s; docs/implementation/testing.md 「Miri」) are read from `/usr/bin/time -p` on the built tree; over budget stops CP-A. The new `trusted` test took under 2 s of test time for both modes under each setting on the prototype. Miri on `eml_vm` and `eml_sched` and both sanitizers were not run on the prototype; a report is a finding for Fable, not a test to trim.

### 7.4 Test changes by kind

Kind 1 (pass/fail moves, deletions): none.

Kind 2 (expected values): none. The full workspace run of the prototype passed with no existing test or snapshot edited [V: proto]. The tests whose stated subject is the changed rule were read and pass unchanged: `remote::{checked,fast}::reclaim_if_due_waits_for_enough_frees` (255 small frees, then the 256th), `trusted::{checked,fast}::the_owner_carves_while_another_thread_reads_and_frees` (after the partial take its new list carves from the current page of its class, so the page bytes do not move), `trusted::{checked,fast}::a_remote_large_free_refunds_at_once`, `trusted::{checked,fast}::two_remote_large_frees_are_both_reclaimed`, `workers::the_fast_mode_runs_the_data_parallel_functions_on_four_workers`. `RunStats.peak_objects` at two or more workers is already zeroed by `eml_test_support::schedule_independent` wherever such a run is pinned.

Kind 3: none.

New tests (N):
- RR-2 (+4): `heap::tests::remote::{checked,fast}::large_frees_do_not_count_toward_the_walk`; `heap::tests::trusted::{checked,fast}::a_due_reclaim_leaves_few_small_blocks`.
- RR-3 (+2): `workers::a_try_send_loop_takes_in_the_frees_of_its_receiver`; `workers::a_spawn_loop_takes_in_the_frees_of_its_tasks`. Each has a fast-mode part (20,000 items, the page bytes) and a checked-mode part (600 items, a clean end).

Existing tests this plan names, all present at the base [V: read]: the five of kind 2's paragraph; `citations::`; `ui::run`, `ui::run_fail`.

### 7.5 Soundness points and what holds each

| Point | Holds because |
|---|---|
| `reclaim_held` with `pages == false` | It runs `remote_large_take`, whose `taken` promise stands alone, and then the unchanged release loop; `acc` is empty before it. Verus verifies the function with the one invariant changed; nothing is `external_body` or assumed anew. Pinned by the new `trusted` test in both modes under Miri with both borrow models |
| The count is no longer raised for a large page | `count` is read only by `sink_count`, `sink_count_take` and `sink_pending`. `sink_pending` also tests `large`, so a waiting large page is still seen; `check_owned`, `check_invariants` and `freeze` run after a full `reclaim`, which always walks and resets |
| Small blocks left after a due reclaim | They stay `Live` in the owner's model, as before any reclaim; the next walk or any full `reclaim` takes them. They are left while fewer than `REMOTE_RECLAIM` have been received since the last walk; the count is a hint other threads raise concurrently, so it is not an exact bound at an instant |
| Checked mode at the three sites | The two `workers::` tests run their loops in checked mode at two workers and end with the audits of the end of a run; the `par/` and `async/` UI files run at 2 and 8 workers in checked mode through the same sites |
| A reclaim before `share` (three sites) | It frees only blocks in `Core::freed_set`: the last reference was dropped by another thread. Registers, task stacks, the channel `Hold`, the root table and the value being published hold references, so none of their targets is in the set (5.1). `Ctx::reclaim_if_due` is a safe function the owner's thread may call at any time |
| The registry lock | `start_task` calls before `sched.cells.lock()`; `Depot::publish` is called outside the lock (vm.md 「channel」: 預かり先の操作は lock の外); `fork_child` holds no lock |
| A reservation is held at `ChanPushRaw` | A reclaim is neither a safepoint nor a switch; `Hold::take_send` has already taken the reservation out, and `switch_away`'s assertion on `sched.hold` is not reached |

A soundness question beyond these goes to Fable (CLAUDE.md).

## 8. Docs: the file-by-file list (RR-G6)

Grep (the stage's subject), at 4e413670:

```sh
D='受けた数|REMOTE_RECLAIM|reclaim_if_due|受けた解放|ページを ?OS ?に返|ヒープごとの最大の和|remote-reclaim'
git grep -cE "$D" -- docs README.md CLAUDE.md crates/eml_rt/CLAUDE.md ':!docs/plans' ':!docs/reports'
```

```
docs/future/multicore.md:5  docs/future/roadmap.md:2  docs/implementation/testing.md:3  docs/spec/native.md:1  docs/spec/object-model.md:12  docs/spec/vm.md:6
```

These six files are all the grep finds [V: grep at 4e413670].

| File | Item (anchor) | Unit | Edit |
|---|---|---|---|
| docs/spec/object-model.md | 「積んだ後で、`RemoteSink` の受けた数に 1 を足す」; the first sentence of 「`Heap::reclaim_if_due` は、大きい物体の列が空でないか」 | RR-2 | 7.2 RR-2 |
| docs/spec/object-model.md | the points and the two new bullets after 「`Heap::reclaim_if_due` は、…」; the bullet before 「決めていないこと: 小さい物体のページを OS に返すこと」 | RR-3 | 7.2 RR-3 |
| docs/spec/object-model.md | 「- 持ち主は`Heap::reclaim`で取り込む」, 「- 回復は、写す前に取り込む」, the T14 row, 「回復」 (lines 910-927) | - | kept: true |
| docs/spec/vm.md | 「- ほかのタスクに切り替えるたびに…」, 「2. `heap.fork_worker()` で右の子の ID を出す」, 「- `start_raw g`: …」, 「- 受け手の決まらない値の預かり先の操作は `Depot` にまとめる」 | RR-3 | 7.2 RR-3 |
| docs/spec/vm.md | the idle loop (「空きのループは走れるタスクができるまで」), join step 4, `take_job` step 2, the end of a run | - | kept: these points do not change |
| docs/spec/native.md | the census sentence (`reclaim`) | - | kept |
| docs/implementation/testing.md | the `remote.rs` bullet; the new bullet in the Miri list | RR-2 | 7.2 RR-2 |
| docs/implementation/testing.md | the `workers.rs` bullet | RR-3 | 7.2 RR-3 |
| docs/implementation/testing.md | 「受けた数が少ないうちは `reclaim_if_due` が取り込まないこと」 | - | kept: true for small blocks |
| docs/future/multicore.md | the four anchors of 7.2 RR-4 | RR-4 | 7.2 RR-4 |
| docs/future/multicore.md | 「すべてのヒープで、ほかのスレッドから受けた解放を取り込む」 | - | kept |
| docs/future/roadmap.md | the entry (RR-1, deleted at RR-5); the later items | RR-1, RR-5 | S-RR-8, S-RR-9 |
| docs/implementation/benchmarks.md | 「今の数値」 | RR-5 | the three tables |
| docs/implementation/status.md, architecture.md, docs/overview.md, docs/README.md, README.md, CLAUDE.md, crates/eml_rt/CLAUDE.md | - | - | no change: none states the count rule, the reclaim points or the bound |
| docs/plans/2026-11-17-runtime/00-decisions.md | D-18 | RR-1 | S-RR-7 |

The docs are not prototyped. A `citations::` failure is fixed in the unit that wrote the citation.

## 9. Verification plan

Baseline (RR-1 fills "Recorded"; expected values at 4e413670):

| Check | Expected | Recorded |
|---|---|---|
| `cargo nextest run` | `N0 tests run: N0 passed, K skipped`; expected N0 = 5207 and K = 23 [I: 5213 passed and 23 ignored on the prototype with `cargo test --workspace`, less the six new tests; nextest's skipped count can differ from cargo's ignored count]. RR-1 writes the real N0 and K into the plan || `5207 tests run: 5207 passed (3363 slow), 23 skipped` (N0 = 5207, K = 23) |
| `cargo test -p eml_rt --lib` | `924 passed; 0 failed; 1 ignored` [V: 928 on the prototype less four] || `924 passed; 0 failed; 1 ignored` |
| `cargo clean -p eml_rt`, then `cargo verus verify -p eml_rt` in `crates/eml_rt` | `1265 verified, 0 errors` || `1265 verified, 0 errors` |
| SC-1, SC-4, SC-7 | empty || empty |
| SC-2 | `3 crates/eml_sched/src/cells.rs`, `2 crates/eml_vm/src/vm.rs`; `crates/eml_nrt/src/entries.rs:1` || as expected |
| SC-3 | `crates/eml_vm/src/vm.rs:4` (no line for `cells.rs`); `crates/eml_rt/src/remote.rs:2` || as expected |
| `git grep -cE 'external_body\|assume\(' -- crates/eml_rt/src/remote.rs` | `14` || `14` |
| `git ls-files ':(glob)crates/*/src/**' \| wc -l`; `git ls-files ':(glob)crates/**' \| wc -l` | `347`; `1677` || `347`; `1677` |
| every anchor of 7.2 and section 8: `git grep -cF -e '<anchor>' -- <file>` | `1` each || not counted one by one at Start; each unit stops on an anchor that does not count 1 |
| section 8's `git grep -cE "$D"` | the six counts shown there || not recounted at Start |
| ``sed -n '/^## 段の列/,/^## その後の項目/p' docs/future/roadmap.md \| grep -c '^- `'`` | `9` before step 4, `10` after || `10` after step 4 |
| `grep -n '^- D-1[78] ' docs/plans/2026-11-17-runtime/00-decisions.md` | one line (D-17) before step 3, two after || two lines after step 3 |
| `git grep -nE 'plans/2026-11-21-row-bound[-]retire' -- .` | two hits before step 2 (lines 132 and 298 of the design-level plan), empty after || empty after step 2 |
| `watch.sh 2097152 target/release/eml run --workers 2 docs/plans/2026-11-21-remote-reclaim/p_try.em` (after `cargo build --release -p eml_cli`) | `exit=0 killed=0`; the run holds about 1.26 GB while it lasts (the defect is at the base; measured at 6ff1f664), so do not run it beside anything else || peak 1.21 GB over 10 samples, growing linearly, exit 0, not killed |

Per unit: G, the SC lines in force and the unit's completion commands.

## 10. Risks and unresolved items

### 10.1 The VM gate (`bench/run.sh`, +0.5 %; the rule of U-3 of 2026-11-20, carried over, with three rows added)

B = RR-1's commit, H = the tip after RR-3. The scripts are files of the appendix directory (`docs/plans/2026-11-21-remote-reclaim/gate-pairs.py`, `gate-rounds.sh`, `gate-rounds-file.sh`, `gate-workers.sh`); each states its usage in its head. Run on macOS, with nothing else running.

What the stage changed and which row sees it:

| Changed code (B..H) | Rows |
|---|---|
| `fork_child`: one `reclaim_if_due` per fork | the `bench/run.sh` rows that fork: `pfib`, `par_tree`, `par_map`, `par_map_unboxed`, `par_reduce`, `par_tabulate_float`, `par_tabulate_int` (`grep -lE '^import (Std\.)?Par' bench/*.em`); the added row `pfib30` |
| `start_task`: one call per task start; `Depot::publish`: one per task end and per channel push | no `bench/run.sh` row (none imports `Async` or `Chan`); the added rows `spread` and `pingpong` |
| `eml_rt`: `reclaim_held`, `remote_large_push` | no row at one worker (no remote free); the record of part 3 |
| Everything else | no row; an OVER row here is layout |

Part 1, the `bench/run.sh` rows (the carried-over rule): a row whose median over three alternating pairs of 実行の分 deltas is over +0.5 % is re-measured alone over 15 alternating rounds; it stops the stage only if its 15-round median stays over +0.5 % and it is one of the forking rows above; otherwise the Close message records it as layout.

Part 2, the three added rows, always measured over 15 alternating rounds with `gate-rounds-file.sh` (instructions of the run part at one worker):

| Row | Command | Expected median (section 3) | Stop for the user when the 15-round median is over |
|---|---|---|---|
| `pfib30` | `bench/pfib.em big` | +0.04 % | +0.12 % |
| `spread` | `bench/async/spread.em 100000 1` | +0.17 % | +0.5 % |
| `pingpong` | `bench/async/chan_pingpong.em` | +0.18 % | +0.5 % |

The limit is the smaller of +0.5 % and three times the expected median. A row over its limit means the call costs more per event than on the prototype (about 6.5 instructions per fork, 44 per task, 36 per push): the tables go to the user, who decides between accepting the number, the lighter form of O-2, and stopping; the orchestrator does not waive it.

Part 3, a record and not a gate: `gate-workers.sh` prints wall time and peak RSS at 2 and 4 workers for `pfib30`, `par_tree`, `spread 8 30`, `spread 100000 1` and `chan_pingpong` on both binaries, and `bench/async/scale.sh` is run once per binary (`EML=<binary> bench/async/scale.sh`). Both outputs go into the Close message. Section 3 has the prototype's values, where no row moved. If a wall time of H is more than 1.2 times B's on any row at any worker count, or a peak RSS more than 1.2 times, the orchestrator shows the user both tables before CP-A closes; instruction counts at one worker cannot see contention on the `RemoteSink` lines or a walk of the pages.

Procedure (from a clean worktree at B made for the gate; both commits build in the same path):

```sh
A=$PWD/docs/plans/2026-11-21-remote-reclaim      # from the repository root, on plan/remote-reclaim
B=$(git log -n1 --format=%H --grep='^Start remote-reclaim$' plan/remote-reclaim); H=$(git rev-parse plan/remote-reclaim); S=$(mktemp -d)
cp "$A"/gate-pairs.py "$A"/gate-rounds.sh "$A"/gate-rounds-file.sh "$A"/gate-workers.sh "$S"/
git worktree add --detach "$S/rr-gate" "$B"; cd "$S/rr-gate"
for i in 1 2 3; do
  git switch -q --detach "$B"; bench/run.sh > "$S/base-$i.md"
  git switch -q --detach "$H"; bench/run.sh > "$S/head-$i.md"
done
cp target/release/eml "$S/eml-head"
git switch -q --detach "$B"; cargo build -q --release -p eml_cli; cp target/release/eml "$S/eml-base"
# part 1
python3 "$S/gate-pairs.py" "$S"/base-1.md "$S"/head-1.md "$S"/base-2.md "$S"/head-2.md "$S"/base-3.md "$S"/head-3.md
bash "$S/gate-rounds.sh" "$S/eml-base" "$S/eml-head" 15 <OVER rows...>
# part 2 (prints: label, median delta %, median run part of base, of head)
bash "$S/gate-rounds-file.sh" "$S/eml-base" "$S/eml-head" 15 pfib30   bench/pfib.em big
bash "$S/gate-rounds-file.sh" "$S/eml-base" "$S/eml-head" 15 spread   bench/async/spread.em 100000 1
bash "$S/gate-rounds-file.sh" "$S/eml-base" "$S/eml-head" 15 pingpong bench/async/chan_pingpong.em
# part 3
bash "$S/gate-workers.sh" "$S/eml-base" "$S/eml-head" 3
EML="$S/eml-base" bench/async/scale.sh > "$S/scale-base.md"; EML="$S/eml-head" bench/async/scale.sh > "$S/scale-head.md"
cd - && git worktree remove --force "$S/rr-gate"
```

The worktree stays at B for parts 2 and 3; the bench programs are the same at B and H (the stage changes none). `spread 100000 1` holds about 60 MB and `pfib30` about 680 MB at two workers; every program here ends in a few seconds.

The scripts were written for this plan and are not what measured section 3: those numbers came from the same commands run one at a time under the watchdog, five rounds. `gate-pairs.py` and `gate-rounds.sh` are the listings of the `std-par` plan. `gate-rounds.sh`, `gate-rounds-file.sh` and `gate-workers.sh` were each run once with one binary given as both BASE and HEAD and one round, and print the stated lines [V]; `gate-pairs.py` was not run. With identical binaries a single round read +0.87 % on `spread` and -0.93 % on `par_tree`, so one round decides nothing and the 15-round median is what counts. A script error is fixed in the appendix, not worked around. `gate-workers.sh` with 3 rounds takes about two minutes.

### 10.2 The `eml_rt` gate (crates/eml_rt/CLAUDE.md, unchanged)

After RR-2's commit, on the clean tree (the script refuses a dirty tree and compares HEAD with the base): `bench/rt-gate.sh <RR-1's commit>` with the default 5 pairs; exit 0 is required before anything is pushed or RR-3 starts. Options come before the commit (`bench/rt-gate.sh [--allow 'ROW ROW...' --allow-bound PCT] BASE [PAIRS]`); the stage uses none. Expected: every row within +0.1 % [I: no `bench/rt.sh` row reclaims or frees across heaps; the change adds no instruction to an allocation or free path. Not run on the prototype].

Also on RR-2: Verus, verusfmt and Miri as its step 3 says.

### 10.3 Open items with defaults

| # | Item | Default |
|---|---|---|
| O-1 | RR-2 fails `bench/rt-gate.sh`, Verus or Miri | the user decides with the output; default S-RR-6 (RR-3 alone, later item (c)) |
| O-2 | The VM gate (10.1) stops on a forking row or on `pfib30`, `spread` or `pingpong` | the user decides with the tables. A known lighter form, not built: make the due test the first thing `Ctx::reclaim_if_due` does, before `settle` (an `eml_rt` change under its gate, with its proof), so the call costs the two loads when nothing waits |
| O-3 | N0 is not 5207, or the two `ls-files` counts differ | RR-1 corrects the plan if a commit after 4e413670 or the doc-test assumption explains it; otherwise stop |
| O-4 | A snapshot or an existing test moves in RR-2 or RR-3 | RR-G5: stop, amend 7.4 by name with the user's approval |
| O-5 | The two `workers::` tests exceed 1 s alone in the timing profile on CI's Linux (`cargo nextest run --profile timing -j 1`) | lower the item count and the threshold together (the ratio without the calls is over 20 to 1), in RR-3 before its commit if it is seen locally with `cargo nextest run -j 1` |
| O-6 | ThreadSanitizer or the whole `trusted::` under Miri reports at CP-A, or the Miri budget is exceeded | CP-A stops; a finding for Fable |
| O-7 | A docs commit merged after 4e413670 touched an anchored line | RR-1 re-counts and re-anchors |
| O-8 | Pushes | the orchestrator pushes `plan/remote-reclaim` for the checkpoints; merging is asked at the end |

## Additional checks

- The prototype worktree was made at 4e413670 with `git worktree add --detach` in the session scratchpad and removed with `git worktree remove --force` and `git worktree prune`; the main tree stayed clean at 4e413670.
- The full workspace run took about 20 minutes in the fresh worktree; `eml_cli`'s integration binary printed "still running after 10s" for UI files that start child processes (first launches of new binaries on macOS) and passed (264 passed, 2 ignored, 90.6 s). It ran alone as far as this session's own processes go; another agent's test run on the same machine cannot be ruled out.
- The red steps were run by reversing the source parts of each patch in the worktree and re-applying them.
- `trusted::fast::a_due_reclaim_leaves_few_small_blocks` passes with and without RR-2's source change: in fast mode a walk gives the small blocks back to the free lists and returns no page, so the page bytes are the same. The `checked` variant and the two `remote::` variants tell the difference.
- Which of the two `workers::` tests read 786,432 and which 458,752 bytes was not recorded; both values are far below the 4 MiB threshold and the failing values are 18.5 MB and 18.9 MB.
- The hot-path numbers of section 3 are from the earlier prototype (base 6ff1f664, third call in the `ChanPushRaw` arm). `crates/eml_rt`, `crates/eml_vm/src` and `crates/eml_sched/src` did not change between 6ff1f664 and 4e413670 [V: `git diff --no-ext-diff --stat 6ff1f664 4e413670 -- crates/eml_rt crates/eml_vm/src crates/eml_sched/src` is empty]. No release build of the 4e413670 prototype was made.
