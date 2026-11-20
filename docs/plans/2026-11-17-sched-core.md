# Stage sched-core: the new scheduler in `eml_sched`, complete and tested without an engine

- Created: 2026-11-17 (design-level plan).
- Base: the `Close native-checked` commit. Facts and line numbers in 01 describe main @ 4a2954c1; the refresh re-checks them.
- Status: design-level. `Start sched-core` replaces this file with an executable plan in the model of `2026-11-17-engine-trust.md` (deleted at its Close; read it with `git show 97cd579c:docs/plans/2026-11-17-engine-trust.md`). The `cells.rs` list of D-SC-3 is part of that plan and is approved with it (01 §22 U-1).
- Appendices: `docs/plans/2026-11-17-runtime/`; 00 wins; the design is 01 §10 (D-SC-1..D-SC-10), with 01 §2 (the model, types, loop, publishing and the verdict, `fork_join`), §13.3 (the crate change), §14.1 P-1..P-4a, P-6, §14.3 ST-1..ST-3, §15.2 H-2, H-6, §16 S-1..S-8, S-12, S-23, S-25, §18 G-8, G-9, G-11, G-12, G-22 (H-2, H-6), G-23, G-24 (D-SC-3a). 00 §D D-6 moves `Finished.claims` / `join_waits` to `jit-tasks`. 00 §D D-15 fixes the shape of `counters.rs` and the budget list's atomic rows.

## 1. Goal and completion conditions

- Goal: `eml_sched` holds the scheduler native code will run on, written new against 01 §2 and tested through the closure-driven `Exec` `Script`; its decision kernels live in the new crate `eml_steps`, all inside `verus!`, proved where 01 §14.1 says, with an exhaustive explorer. The VM is not touched beyond the adapter the `cells.rs` list names.
- Completion conditions:
  - G-8: `cargo verus verify -p eml_steps` passes; `crates/eml_steps/src/trust.rs` pins "no `external_body`, no `assume_specification`, no `unsafe`"; placement tests: `Open`'s operations, `plan::next`, `idle::step`, `verdict::step`, `limit::hit` each have one caller module in `eml_sched`.
  - G-9: an unstarted job exists only in an `Open`: no `Notice` variant carries a `Job`, and `grep -n 'in_flight' crates/eml_sched/src/run.rs crates/eml_sched/src/job.rs crates/eml_sched/src/fork.rs crates/eml_sched/src/drive.rs` → empty (`Registry::in_flight(k)` in `cells.rs` is not this). The refresh fixes the exact grep.
  - G-11: `grep -rn 'fn drive' crates/eml_sched crates/eml_nrt` → one hit (`crates/eml_sched/src/drive.rs`).
  - G-12 / D-SC-10: `git diff --stat <stage base> -- crates/eml_vm` lists only the adapter files the `cells.rs` list names; no UI output or snapshot changes (`git diff --stat <stage base> -- tests/ui 'crates/*/tests/snapshots' crates/eml_cli/tests/repl` → empty).
  - G-22 (this stage's hooks): H-2's test reads `Slot.current` from another thread; H-6 has its test as 01 §15.2 states.
  - G-24: `cargo nextest run -p eml_cli --test integration -E 'test(/^surface::/)'` passes at every commit; new public items follow docs/implementation/architecture.md 「公開の面と`unsafe`」 (D-SC-3a): `fork`, `join`, `drive` are `unsafe fn`, `Exec`'s methods `unsafe`, `Marked` unforgeable, `Script` safe; a new safe function or `pub` field that names a `surface.rs` token (whole identifiers, including the driver's `Op`) (`take_own`, `wait_join`, `WorkerCore::schedule`, ...) gets its `SAFE` entry in the unit that adds it; the `cells.rs` items the new files call are the `unsafe fn` of `engine-trust`'s ET-R1, so D-SC-3's "additions only" holds.
  - G-23: a placement test pins the readers of `listed` and that a scan reads a list only under its slot lock.
  - D-SC-6's tests pass; Miri `drive::` in both borrow models; TSan `claims::`, `par::`, `workers::`, `counters::`; the explorer (D-SC-7) passes as an `#[ignore = "exhaustive"]` test with its CI line in release.
  - D-15: `grep -n 'unsafe' crates/eml_sched/src/counters.rs` → empty; `surface::` needs no entry for the module.
  - `nix/crate-graph.sh --check` passes with the new crate and the edge `eml_sched -> eml_steps` in its tables.

## 2. Scope

- In: D-SC-1..D-SC-10; the crate `eml_steps` and its CI lines (Verus, verusfmt, tests, clippy); the new files of `eml_sched` (D-SC-2); the claim and join-wait counts inside `WorkerCore`, read by this stage's tests (the `Finished` fields come at `jit-tasks` JT-5, 00 §D D-6); docs/spec/runtime.md (new) and object-model.md's isolation invariant.
- Out: any engine using the new scheduler (`jit-tasks`); P-4b for every N (`sched-proofs`); moving or reshaping VM code (UJ-17).

## 3. Received from earlier stages

- `async-root`: one root task per run; no nested root (the new `run.rs` has none). The root is started by the row `root_start`, called on the worker's own ID before `drive(Until::Root)`, and its result is taken by the row `root_result` after the root scope has closed (00 §D D-10 (b)); `cells.rs` keeps `Take::Run` for it.
- `par-total`: total children; a child's error ends the run; no join error record.
- `task-limits`: three limit classes; no soft limit; the limit table that `eml_steps::limit::hit` restates (P-3 keeps three example rows from the VM's table).

## 4. Decisions (01 §10, §2)

- D-SC-1: `eml_steps`: `open.rs`, `plan.rs`, `limit.rs`, `idle.rs`, `verdict.rs`, `sleepers.rs`; vstd only; no `unsafe`, no trust item.
- D-SC-2: `eml_sched` gains `job.rs`, `run.rs` (`Run`, `Slot`, `WorkerCore`, inbox, `Notice`; implements `cells::Post`), `fork.rs`, `drive.rs`, `exec.rs` (`Exec`, `Script`), `policy.rs`, `io.rs`, `clock.rs`, `signals.rs`, `limits.rs`, `monitor.rs`, `pool.rs`, `main_loop.rs`, `threads.rs`, `recover.rs`, `counters.rs`; the VM's files with the same logic are read, not moved. `counters.rs` is written to 00 §D D-15, not read from the VM's file: a private `Table` (`const fn new`, `push(&self, n) -> Result<&AtomicI64, RunFault>`, `find(&self, word) -> Option<&AtomicI64>`), one `static TABLE: Table`, and the public `Counter`, `new`, `find` over it. No engine uses the module before `jit-tasks` JT-6; until then its tests are its only callers.
- D-SC-3: the `cells.rs` list (additions only, `pub(crate)` where not public; `Discards::run`'s `depth_offset` is passed 0; an item that must change shape gets a thin adapter in `eml_vm`, listed).
- D-SC-4: across `enter`, `drive` holds no reference or pointer derived from `this` other than `this`; `Exec::with` makes a borrow unable to leave an item boundary; the rule for engines (a shim calls `eml_fatal` only after every guard it holds is dropped) is written in native.md.
- D-SC-5: `Policy::at(Point)` is the one hook mechanism; `eml_sched`'s feature `testing` gets no new item.
- D-SC-6: the tests listed in 01 §10 (`claims::`, `par::`, `jobs::`, `workers::`, `pinned::`, `io::`, `clock::`, `signals::`, `counters::`), Miri `drive::`, TSan lines. `counters::` is, on a `Table` local to each test (so no other test's `new` interferes): `push_hands_out_distinct_slots`; `find_accepts_only_handed_out_words` (accepts each pushed slot's word; rejects 0, -1, `i64::MAX`, a slot's word + 1 and + 4, the word of the slot at index `len` in an initialised bucket, the address of a local `AtomicI64`); `add_wraps_around`; `a_slot_keeps_its_value_while_buckets_grow` (at most 300 pushes); `concurrent_pushes_and_adds_agree` (4 scoped threads). On the static: `the_static_table_hands_out_counters` (two `new`, distinct words, each reads back its own value, `find(word)` returns it); it asserts nothing about other slots. Also `policy::seeded_picks_an_entry_other_than_the_head`, `policy::a_short_budget_yields_before_128_operations`, `policy::the_same_seed_gives_the_same_order` (through `Script`), and the count of reorders in `WorkerCore` beside the claim and join-wait counts (00 §D D-15 (d)).
- D-SC-7: the explorer over `idle::step`, `verdict::step` and the publish steps for 3 workers (worker 0 among them) and 3 publishes, and for "a worker stops for good while it has a listed job", with the three assertions of 01 §10.
- D-SC-8: end-of-run checks in every build: every `open` empty, no `Joined` in an inbox.
- D-SC-9: object-model.md gains the isolation invariant; runtime.md is created with 「workerのループ」「仕事の一覧」「眠りと起こし」「`par` の fork と join」「操作の予算」「信頼の一覧」; vm.md keeps describing the VM until `vm-retire`.
- D-SC-10: no change under `crates/eml_vm` but the listed adapter; no UI output or snapshot change.
- The model of 01 §2 is normative: the four rules, `running` and waking, the verdict's four steps as a loop, the lock order, `wait_join`'s predicate and wake sources, `Seeded`'s permutation and budgets. §2.3's budget list is as 00 §D D-15 (d) states: the rows that read or write a registry cell or a channel spend; no `Std.Atomic` row does.

## 5. Units (in order)

1. SC-1 `Start sched-core`: plan; the `cells.rs` list (U-1) and any adapter; the module skeleton; baseline.
2. SC-2: `eml_steps`: the crate, CI lines (Verus, verusfmt), `open.rs` (P-1), `plan.rs` (P-2); `nix/crate-graph.sh` tables.
3. SC-3: `eml_steps`: `limit.rs` (P-3), `sleepers.rs` (P-6), `idle.rs`, `verdict.rs`, the explorer (P-4a).
4. SC-4: `run.rs`, `job.rs`, `Marked`, the four rules, `running` and waking.
5. SC-5: `exec.rs`, `drive.rs`, `Script`, item open / close, `Slot.current` (H-2).
6. SC-6: `fork.rs`, `wait_join`, taken jobs, D-SC-8.
7. SC-7: `io.rs`.
8. SC-8: `clock.rs` and the verdict's shell.
9. SC-9: `signals.rs`.
10. SC-10: `limits.rs`.
11. SC-11: `monitor.rs`, `pool.rs`.
12. SC-12: `main_loop.rs`, `threads.rs`, `recover.rs`.
13. SC-13: `policy.rs` (`Seeded`, budget points), `counters.rs` (00 §D D-15) with its tests and its Miri and TSan lines; the claim and join-wait counts and the reorder count in `WorkerCore` with their tests (not in `Finished`: 00 §D D-6).
14. SC-14: Miri and TSan lines for the rest; runtime.md; object-model.md.
15. SC-15 `Close sched-core`.

## 6. Test changes by kind (scope)

- None to existing tests (01 §10). Every test is new: D-SC-6's list, the `eml_steps` unit tests and the explorer, placement tests (G-8, G-11, G-23), the H-2 test that reads `Slot.current` from another thread, H-6's test. No `Finished` literal changes (00 §D D-6).
- A kind 1 or kind 2 change found during execution that the executable plan does not list stops the unit for a plan amendment approved by the user.

## 7. docs/spec sections written

docs/spec/runtime.md (new; D-SC-9's sections and 「信頼の一覧」 with ST-1..ST-3); object-model.md (the isolation invariant of 01 §2.1); native.md (the shim rule of D-SC-4, if not already stated); docs/implementation/architecture.md (`eml_steps` in 「crate の依存関係」 and its section); testing.md (the explorer line, the Miri / TSan lines, `eml_steps`' gate); CLAUDE.md (the crate table row of `eml_steps`; the `eml_sched` row, whose "depends only on eml_io and eml_rt" gains `eml_steps` and whose list of contents gains the scheduler; the runtime-closure list of the strata invariant gains `eml_steps`; the Verus, verusfmt and explorer command lines; pointer and rule changes, so they are allowed); crates/eml_rt/CLAUDE.md (a pointer to `eml_steps`' rule, A-35's second half). runtime.md 「操作の予算」 follows 00 §D D-15 (d) and says that an atomic operation is not a switch point; runtime.md gets 「`Std.Atomic`の数の表」 (the process-wide table, buckets, the two parts of `find`).

## 8. Acceptance checks

- CLAUDE.md's gates; `cargo verus verify -p eml_steps`; `verusfmt --verus-only --check` over `crates/eml_steps/src`; the explorer in release (`cargo nextest run -p eml_steps --release --run-ignored only -E 'test(explore)'`) passes.
- G-8, G-9, G-11, G-12, G-23 as section 1 states.
- Miri `drive::` (two settings) and the TSan lines pass.
- `nix/crate-graph.sh --check` with the new tables; `expect_tree`: `eml_steps` reaches no workspace crate.
- Roadmap: the `sched-core` item and 「`eml_sched`のcrate」 are deleted.

## 9. Risks with defaults

| Item | Default |
|---|---|
| U-1: `cells.rs` needs a change of shape | a thin adapter in `eml_vm`, listed in the plan before approval (D-SC-3) |
| A proof of P-1..P-3 or P-6 does not close in Verus | stop; a strong advisor reviews before any weakening; never `external_body` (G-8) |
| The explorer exceeds CI time in release | keep the bound of D-SC-7; split its two cases into two tests, each `#[ignore]` with its CI line |
| A Miri or TSan line is slow | follow testing.md 「Miri」 / 「ThreadSanitizer」 budgets; narrow the filter, never drop the borrow model |

## 10. Refresh at the stage's first commit

Write the `cells.rs` list (01 §27 item 1) and any adapter; confirm the pinned vstd still specifies `VecDeque`'s `push_back`, `pop_back`, `pop_front`, `len`, `index`; re-read the VM's scheduler files named in D-SC-2 for behaviour the new files must match (I/O hosting, the clock, signals, the monitor, the pool, the main loop, recovery); record the baseline.
