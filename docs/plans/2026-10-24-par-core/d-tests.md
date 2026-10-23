> 付録。par-core の設計の担当が書いた最終版で、英語のまま残す。本文 (docs/plans/2026-10-24-par-core.md) と食い違う所は本文が優先する。違いは本文の 5.3 にまとめた。文中の `par-core/probe/` は `docs/plans/2026-10-24-par-core/probe/` を指す。

# S4 response r2: test plan of `par-core`

Base ffc04d6b. Scratch clone `<work dir>/S4-repo`: I built `eml_cli` and the `eml_cli` integration tests there to verify the CLI and REPL claims and the timing baseline. The repository was not edited, and nothing is left running. [V] = verified, by reading the file or by running it in the clone. [I] = inferred.

Settled inputs:
- S1-response-r3.md, which fixes the contract, the names and the unit order: S1-1 → S2 ops → S3-1 → S3-2 → S1-2 → S1-3 → S3-3 → S1-4.
- `fork$`/`reperform$` exist only in programs that contain `Call::Par`.
- The uncaught child Panic names `reperform$Prelude.Panic.panicked`.
- The fork evv is a mirrored list with depth.
- The user decision that children may return `Lin` values; S3-3 carries the eml_types change.
- S3 r1's probes.

"Seeds" means `Sched::Seeded(1)` and `Sched::Seeded(2026)`, everywhere: the UI harness, `par.rs` and `bench.rs`.

Changes against r1:
- Preemption is enforced per file (C-1).
- A general rule for splitting the script arguments (B-1).
- Permanent guards for marking and transfer (C-3).
- Seeded runs skip VM-bug results (B-2).
- Source-level child-error tests (C-5).
- A `miri::` test of `par` (C-6).
- One list of excluded `RunStats` fields (A-1/A-2).
- The `par/` list now includes S3's tests and the `Lin` result tests (B-5).
- A measured per-file timing criterion (C-4).

## 1. Conclusion

### Q1 Seeded harness (crates/eml_cli/tests/ui.rs)
- **Where.** The seeded runs go inline in `ui::run` and `ui::run_fail`. No sixth file-collecting test is added, so `.config/nextest.toml` and testing.md's "five tests" stay as they are. A file is selected by `fn seeded(suffix) -> bool { suffix.starts_with("par/") }`; the suffix comes from `classify` and is the path below the top directory. `check-fail` is never seeded.
- **Order per selected file:**
  1. the checked sequential run, then the snapshot;
  2. the existing fast-mode comparison;
  3. then, **only if the checked result is `Ok` or an error with `fast_mode_reports(error)`**, one checked run per seed. Each seeded run gets a fresh `RunConfig`: `default().with_debug_heap(true).with_control(Arc::new(Control::new(default_limits()))).with_sched(Sched::Seeded(seed))`.
- **Why seeded runs skip VM bugs (B-2).** A `Leak`, `Internal` or heap error from the sequential run is a VM bug, already recorded in the snapshot. Its text can carry worker IDs (`WrongOwner { owner, worker }`, `TransferLeak { worker, .. }`), and those IDs depend on the schedule: tasks call `fork_worker` in interleaved order, and `retire_worker` recycles IDs in the order children finish. Every other error text, and all program output, contains no IDs.
- **Compared** against the checked sequential run, byte for byte:
  - stdout, stderr, written files, and the result (`Ok(exit)`, or `Err` compared as `to_string()`);
  - for `run/par/` also `schedule_independent(stats)` (A-1 below).
- **Preemption is enforced (C-1).** Under each seed, every `run/par/` file must report `finished.preemptions > 0`. Otherwise the file fails with ``{suffix}: never preempted under `--sched seeded:{seed}`; give one par a left child of 16 or more safepoints (testing.md 「UI テスト」)``.
  - This needs one refinement of S1's S4-facing counter: `Finished.switches` becomes `Finished.preemptions`, counting only the switches that `Policy::tick` initiates at a safepoint. Forced switches (a blocked join, a finished or failed right child) are not counted, so `Sequential` always gives 0.
  - A count of every switch cannot serve. Every `par` already forces two switches under `Sequential`, and under a seed a preemption can replace the forced block, so the total stays 2 [I from S1 Q2].
  - Not a test-only field: it records what the policy did, and the later `PREEMPT` and monitor work reads the same number.
  - **Needs consolidation approval** (S1 U5 kept "`switches`").
- **Why 16 safepoints suffice.** S1 Q2/Q6: `Seeded::tick` fires when its budget (1–16) is spent and reloads only after a switch happens; while the ready queue is empty it keeps firing. So the first safepoint after a fork whose left child has used up the remaining budget preempts. A left child with ≥ 16 safepoints is preempted under every seed [I].
  - Safepoints are calls (tail calls, `apply` and `clause` included), returns and self-tail jumps [V vm.md:145].
  - optimize may inline small non-recursive functions, so tests use a recursive helper (`spin : Int -> <Panic> Int`, n levels) in left children.
  - `run-fail/par/` has no `Finished`. Its single file has the same structure as `run/par/leftmost_abort_wins.em`, where preemption is enforced.
- **Mismatch message**: ``{suffix}: `eml run --debug-heap --sched seeded:{seed}` printed something else`` (likewise "wrote another standard error", "wrote other files", "ended otherwise", "did other work"). Rerunning the test reproduces it: the schedule is a pure function of the seed, the program and the inputs (S1 Q6).
- **Checked mode only.** The scheduler code is shared by both modes (`vm::run<M, P>`). A scheduler bug in the fast mode would be UB in the shared process, which the harness avoids today on purpose [V ui.rs:194-199].
- **Harness refactor (kind 3; snapshot strings and existing messages stay identical):**
  - `Ran` gains `finished: Option<Finished>`;
  - `execute_in(.., debug_heap, sched: Sched)`;
  - `assert_fast_mode_agrees` → `assert_agrees(suffix, label, other, checked, compare_stats)`;
  - `const SEEDS: [u64; 2] = [1, 2026]`.
- **Self-tests in ui.rs:**
  - `seeded_runs_cover_only_par`: `seeded("par/x.em")`, `seeded("par/dir")`, `!seeded("panic/x.em")`, `!seeded("parse/x.em")`;
  - `a_seeded_mismatch_names_the_seed`: `#[should_panic(expected = "--sched seeded:7")]` on two `Ran`s with different stdout.
- **Windows: no marks.** The tests start no processes, and the policy (splitmix64) reads no addresses and no hash order.

### Q2 `tests/ui/{run,run-fail}/par/`
Notes for every program in the list:
- `import Par (par, both)`, or `import Std.Par` with `Par.par` as in S3's probes. Single files.
- Each file defines `spin`, and at least one `par` has a left child that runs `spin 40` (C-1).
- `MutArray` uses the std API as the probes do: `MutArray.make`, `write m i v` (returns `m`), `read m i` (returns `(x, m)`), `freeze`.

| # | File | Checks |
|---|---|---|
| 1 | `run/par/pair_of_values.em` | Values with several Reprs: `(Float, Int)` (S3), `Char`, `String`, a record and a list, through the `tobj` pair and the struct unbox |
| 2 | `run/par/shared_capture.em` | The parent builds a list `xs` and a closure `f` that captures a record. Both children `map f xs` and sum. The parent uses `xs` after the join. **Permanent guard for fork marking** (C-3) |
| 3 | `run/par/fresh_result.em` | (a) The right child returns a fresh tree; the parent rebuilds it with a reusing `incr`. (b) The right child returns a captured, shared list unchanged; the parent then rebuilds it. **Permanent guard for transfer back** (C-3). `reuses` is compared under the seeds |
| 4 | `run/par/abort_reraised.em` | Under the parent's `Fail.to_result`: left `fail` with a fresh right value, which is dropped; right `fail` with a fresh left value, which is dropped; `Fail Int` and `Fail String` |
| 5 | `run/par/leftmost_abort_wins.em` | (a) Left: `spin 40` then `fail "left"`; right: `fail "right"` at once → `Err "left"`. Every seed runs the right child first. (b) Left `fail`, right `1 / 0`, with `try_panic` outside `to_result` → the `Fail` wins. (c) Left `panic "l"`, right `fail` → `try_panic` gets `"l"` |
| 6 | `run/par/panic_in_child.em` | (a) A child's own `try_panic` catches `1 / 0`. (b) The parent's `try_panic` catches a left `1 / 0`. (c) It catches a right `panic "boom"` |
| 7 | `run/par/never_ops_with_arguments.em` | A user effect with `never` operations of 1 (`Int`), 1 (`Unit`), 2 (`Int`, `String`) and 3 (with a `Float`) parameters, each handled by a `ctl` clause in the parent. Covers the tuple packing of `fork$`/`reperform$` from source |
| 8 | `run/par/handlers_inside_children.em` | Each child installs its own handlers: (a) `Fail.to_result` inside the child; (b) a resumable non-tail handler (`Std.State` or a `Yield` generator) inside each child, so yields occur while the seeded budget asks for a switch |
| 9 | `run/par/mask_on_the_call.em` | S3's `probe/mask.em` with `spin 40` added to the left child: the mask on the `par` call itself (F1). Prints `Err 7` [V S3 r1 for the probe] |
| 10 | `run/par/mask_inside_the_child.em` | S3's `probe/deepmask_par.em` with `spin 40` added to the left child: a mask inside a child skips a mirrored evidence, and the re-raise uses depth 1 (F2). Prints `Err 7` |
| 11 | `run/par/nested.em` | A divide-and-conquer sum over a tree with `par` down to depth 4. Inside one right child, an inner `par` whose left child aborts is caught there by `to_result`. Joins resolve inner first; IDs are recycled |
| 12 | `run/par/lin_through_children.em` | (a) S3's `probe/lin_cap2.em`: the child captures a `MutArray`, writes it and consumes it. (b) `probe/lin_ret.em`: the child creates and returns a `MutArray` (a `Lin` result, user decision). (c) A `MutArray` captured by the right child is written there and returned; the parent writes it again and prints. Checked: no `WrongOwner` and no `LinShared` |
| 13 | `run/par/poly_lin_pair.em` | `pair : a -> b -> (a, b)` builds `(arr, 7)` with `arr : MutArray Int`, a container without the lin bit. The right child captures the pair, and inside it a nested `par`'s right child captures it again (the rc −1 resend, S2's own-stack path). The inner child writes `arr` and returns it, and so does the outer one. The parent writes and prints. This is multicore.md:538's "`poly` で作った `Lin` の物体が rc == -1 で届くこと (検査モード)" from source |
| 14 | `run/par/both.em` | `both` runs left then right and allows any row: its children `println` in order; a left `fail` stops before the right child's `println`; `both` inside a `par` child |
| 15 | `run/par/par_as_value.em` | The pure part of S3's `probe/value.em`: `let p = Par.par` then `p f g`; `apply2 Par.par f` with pure long children. Covers the `$externN` wrapper whose body is `Call::Par` |
| 16 | `run-fail/par/uncaught_child_panic.em` | Prints `before`. Left: `spin 40` then `1 / 0`; right: `panic "right"` at once. Expected: ``panicked: division by zero in `reperform$Prelude.Panic.panicked` `` |

- **Owned by S3:**
  - E2020 tests: `check-fail/par/child_performs_io.em` and the `eml_types` `row_bounds.rs` tests (`exit`, a once-op, value use);
  - `ok.em`-style front-end checks.
- **Child runtime errors are not UI tests (C-5).** They cannot be reached at UI limits:
  - every value fault in a child becomes a `Panic` abort through the `Panic` fork evidence;
  - heap faults, `Interrupted` and `MemoryLimit` end the run at once (S1 Q5);
  - `DepthLimit` at `default_limits()` costs 1 GiB [V testing.md 「CLI のテスト」].

  This deviates from the briefing's list of `par/` UI tests, and the plan must say so. The rules are instead tested from **source**, in `crates/eml_vm/tests/par.rs`, with a small `max_depth` and the same seeds (unit S4-1; see §5). That follows `scaling.rs`, which tests limits from source [V scaling.rs:1451-1460].

### Q3 RunStats
- **One list of excluded fields (A-1/A-2).** `eml_test_support::schedule_independent(stats: RunStats) -> RunStats` zeroes exactly the fields that depend on the schedule. It carries one line per field with the reason, and the UI harness, `par.rs` and `bench.rs` all use it.
  - Today it zeroes **only `peak_objects`**: how many objects are live at once depends on the interleaving (multicore.md:264).
  - `peak_stack` stays in. Under S1 Q2 it is the maximum over tasks of `depth_offset + stack.len()`. A right child's offset is fixed at its fork, and each task's stack trajectory is the same in every schedule [I].
  - If a field ever differs, the fix is decided per field: either change the field's definition so it no longer depends on the schedule, or add it to `schedule_independent` with its reason, in the same commit, recorded in testing.md 「UI テスト」 and the plan. There is no blanket fallback.
- **Pinned under sequential**: `bench/par_tree.em` (B-4).
  - Program: a complete tree of `Int` nodes, a `par` `incr` down to depth 4 (15 forks) with sequential `incr` below, then a sequential `incr` pass, then one printed line. Sized so that `bench::par_tree` with all three runs takes < 1 s alone in checked dev (default 2^12 − 1 nodes).
  - It pins: right children allocate (their input is shared) while left children reuse; flipped and fresh results are reused after the join; the evidence and unwind work of joins.
  - `fn par_tree()` snapshots the `Sequential` stats and also asserts `schedule_independent` equality under both seeds, so the bench program's invariance is checked directly rather than inferred from similar UI tests.
  - Consumers that must change in **the same commit**:
    - `bench.rs`: `PROGRAMS` (`[&str; 41]`), `fn par_tree`, and the `unwind_points` line `par_tree: N`. `every_program_in_bench_has_a_test` compares the `bench/` listing with `PROGRAMS` [V bench.rs:11, :1123-1137].
    - benchmarks.md 「基準のプログラム」 gets its row.
  - Consumers that change without edits: `crates/eml_core_ir/tests/common/programs.rs` walks `bench/`, so `lowering::every_program_lowers_to_the_same_text` and the ignored `census` pick it up [V programs.rs:49]; `bench/run.sh` globs `bench/*.em` [V run.sh:65].
  - `bench/scripts.sh` lists its programs by name, so `par_tree` is not added there [V scripts.sh:117-123].
  - benchmarks.md 「今の数値」 is refreshed at stage close (S1-4).
- **Invariance in three places, each checking a different fact** (testing.md 「重複させない」 gets the trio):
  - S1-2's hand-written-LIR test: the VM's switch and join code;
  - the UI harness: generated code on every `run/par/` program;
  - `bench::par_tree`: the pinned bench shape.

### Q4 Negative tests (C-3)
- **Permanent guards that fail if `Op::Par` forgets to mark** (checked sequential run, so no seed is needed):
  - **UI `run/par/shared_capture.em`**: the right child `dup`s the captured `xs` (owner 0) under worker 1 → `WrongOwner { owner: 0, worker: 1 }`. Every `run/par/` test whose right closure captures anything fails the same way, because the child also frees the closure itself (`take_captures` under the child's ID, S1 Q2).
  - **VM `par.rs::a_transferred_mut_array_is_written_by_both`**: S1-2's "transferred `MutArray` written by the child and by the parent" test. An unmoved `MutArray` written by worker 1 → `WrongOwner`.
  - Omitting the root share is guarded by S1-3's `roots.rs` "a right child reads a root" test.
  - Omitting the mirrored `fork_evv` share is guarded by `run/par/abort_reraised.em`: the child `dup`s its parent-owned evidence.
- **Permanent guards that fail if the join forgets `transfer_back`**:
  - **UI `run/par/fresh_result.em`**: `retire_worker(1)` finds child-owned live objects → `TransferLeak`. Without the flip, the shared result is also never reused.
  - **VM `par.rs::a_lin_result_moves_back_to_the_parent`**: S1-2's "a right child returning a lin object, then the parent frees it". The parent frees a child-owned object → `WrongOwner`.
  - **`bench::par_tree`**: the `reuses` and `allocations` snapshot changes.
- **Permanent mechanism tests below the VM:**
  - eml_rt (S2 U3) `share.rs::fork_marking_lets_the_child_touch_what_the_closure_reaches`. Setup: a `Kind::Closure` object holding a 3-cell list plus a second reference (rc 2), `child = fork_worker()`.
    - Unmarked: `set_worker(child)`, then `dup` → `WrongOwner { owner: 0, worker: child }`.
    - Marked: `share(closure, child)`, then a `dup`/`decref` walk → `Ok`, then cleanup, then `mortal == 0`.
  - eml_rt (S2 U4) the `TransferLeak` test listed by S2.
  - VM (S1-2) `par.rs::a_child_that_leaks_fails_at_its_join`, run with `run_unverified`, checked only:
    - Program: `main` runs `par` on closures `&left` and `&right`, both without captures. `right` allocates a 2-field `con` and returns `()` without `decref`ing it. The verifier rejects that, hence `run_unverified`.
    - Expected: `RuntimeError::Fault { fault: VmFault::Extern(Fault::Heap(HeapError::TransferLeak { worker: 1, live: 1 })), .. }`. Checked mode hands out ID 1 first (S2 Q3).
    - It pins that the VM calls `retire_worker` at each join. Without `par`, the same leak would surface only at the end, as `RuntimeError::Leak`.
- **Mutation runs M1–M3, a one-off confirmation in S4-1's commit message.** Each is applied, run with `EML_UI_FILTER=/par/ cargo nextest run -p eml_cli --test integration -E 'test(=ui::run) | test(=ui::run_fail)'`, and reverted.
  - M1, delete `heap.share(g, child)`: `shared_capture.em` fails with `WrongOwner`.
  - M2, delete the `transfer_back` loop: `fresh_result.em` fails with `TransferLeak`.
  - M3, re-raise the right abort first: `leftmost_abort_wins.em` and `uncaught_child_panic.em` fail **already in the sequential checked run (snapshot mismatch)**. Children are never cancelled, so after a left abort the right child still runs to its own abort, and the join always holds both aborts in every schedule.

  The guards above are permanent, so later stages rerun them simply by running the suite.
- **testing.md wording.** 「`eml_vm` のテスト」's `run_unverified` sentence becomes "VM 自身の見張り (内部の誤りと、検査モードのヒープの誤り)".

### Q5 REPL and CLI
- **`crates/eml_cli/tests/repl/par/root_in_child.repl`** (moved from S1-4 to S4-2). Inputs [V syntax and the sequential results with `eml repl --debug-heap` on the base]:
  ```
  > import Par (par)
  > upto : Int -> List Int -> <Panic> List Int
  . upto n acc = if n == 0 then acc else upto (n - 1) (n :: acc)
  .
  > let xs = upto 100 []
  > total : List Int -> <Panic> Int
  . total ys = match ys with
  .   | [] -> 0
  .   | y :: rest -> y + total rest
  .
  > par (fn () -> total xs) (fn () -> total xs * 2)
  > let p = par (fn () -> upto 3 []) (fn () -> total xs)
  > p
  > total xs
  ```
  Expected:
  - `(5050, 10100)`, then `([1, 2, 3], 5050)`, then `5050`;
  - the snapshot ends `no leaks`;
  - the same values that the sequential forms (`(total xs, total xs * 2)`, …) printed on the base.
- **`crates/eml_cli/tests/repl/par/recover.repl`**: the same setup plus `import Fail (Fail(..))`, then:
  1. `par (fn () -> total xs) (fn () -> 1 / 0)` → ``runtime error: panicked: division by zero in `reperform$Prelude.Panic.panicked` ``. Recovery copies the shared roots (S2 U1).
  2. `par (fn () -> total xs) (fn () -> total xs + 1)` → `(5050, 5051)`. The roots are shared again at this fork.
  3. `Fail.to_result (fn () -> par (fn () -> Fail.fail "l") (fn () -> total xs))` → `Err "l"`.
  4. `effect Stop where` / `.   never stop : Int -> a` / `.` → nothing printed.
  5. `handle par (fn () -> stop 1) (fn () -> 2) with` / `.   | ctl stop n -> (n, n)` / `.` → `(1, 1)`.

  The input in step 5 contains `par`, and the REPL lowers the whole session program at each input, so `fork$Stop.stop` (with the session version) is generated there (S3 Q5). On the base I checked [V] the declaration form, the clause without `return` and the printed `(1, 1)` with `handle (stop 1) with | ctl stop n -> (n, n)`. The snapshot ends `no leaks`.
- **Transcripts run sequentially only** (user decision); the harness keeps `RunConfig::default()` [V repl.rs:302-304].
- **terminal.rs `repl_runs_par_under_a_seeded_schedule`**: `repl_with(&["--sched", "seeded:3"], b"import Par (par)\npar (fn () -> 1 + 1) (fn () -> 2 * 3)\n")` → stdout `(2, 6)\n`. A piped REPL prints values only: tuples print as `(1, 2)` [V on the base].
- **Splitting the script arguments (B-1), unit S1-1.** General rule:
  - After `run`, scan the tokens in order:
    - a token equal to the long name of a `run` option that takes a value (`--sched` today) consumes itself and the next token, if there is one;
    - any other token starting with `-` consumes itself (`--debug-heap`, `--sched=seeded:7`);
    - the first remaining token is the file, and everything after it goes to the script unchanged.
  - The set of value-taking options is read from clap's own definition of `run` (`<Cli as clap::CommandFactory>::command()`, the `run` subcommand's arguments with a long name whose action takes values [I API names, clap 4.6.7 per Cargo.lock]). No hand list can drift from the parser, and a later flag such as `rescue-always` needs no edit. Fallback: one `const RUN_VALUE_FLAGS` next to the clap struct.
  - The rule changes nothing for today's arguments. Doc: architecture.md:357 and stdlib.md 「起動」 (:15-16) say "ファイルは、値を取るフラグの値でない、`-` で始まらない最初の引数".
  - cli.rs `sched_flag_is_accepted_and_rejected` pins each case:

| argv after `eml` | Expected |
|---|---|
| `run --sched sequential main.em` | 0 |
| `run --sched seeded:7 main.em a` | 0, script gets `["a"]` |
| `run --sched=seeded:7 main.em` | 0 |
| `run --debug-heap --sched seeded:7 main.em` / `run --sched seeded:7 --debug-heap main.em` | 0 |
| `run main.em --sched bogus` | **0**, script gets `["--sched", "bogus"]` |
| `main.em --sched seeded:7` (implicit run) | 0, script gets `["--sched", "seeded:7"]` |
| `run main.em --sched` (trailing, after the file) | 0, script gets `["--sched"]` |
| `run --sched seeded:1 --sched seeded:2 main.em` (repeated) | 2: clap rejects a repeated option; `run --debug-heap --debug-heap` does so today [V] |
| `run --sched` (trailing, no value, no file) | 2 |
| `--sched seeded:7 main.em` (implicit run with a leading flag) | 2: flags need `run`, as `eml --debug-heap f.em` today [V exit 2] |
| `run --sched bogus main.em`, `run --sched seeded: main.em`, `run --sched seeded:-1 main.em`, `run --sched rescue-always main.em`, `check --sched sequential main.em`, `repl --sched bogus` | 2 |

- **`eml_vm` api.rs** (S1-1, as S1 lists): `the_default_sched_is_sequential`, `a_sched_parses_and_rejects`, and the type asserts.

### Q6 Scope of multicore.md 「テスト」 (C-6)
Items of multicore.md:538 that are out of scope:
- `try_map`'s leftmost `Err`, float `fold` bits across granularities, and `for_mut` marking all belong to **std-par**: `Std.Par` has only `par` and `both` here.
- N = 2 and N = 8, TSan and the stress tests belong to **worker-threads**.

**Miri is in scope**: one `miri::` test in `crates/eml_vm/tests/miri.rs` (unit S1-2), `par_switches_between_tasks`.
- A hand-written LIR program with a nested `par` whose children allocate, abort once and return values. It runs `Sequential` and `Seeded(1)` in both modes (with `miri.rs`'s `run_program`, extended by a `sched` argument).
- Reason: the stage adds VM code that holds raw heap words in `Task`/`JoinNode` across switches and calls `evv_swap`, `set_worker`, `share` and `transfer_back` from unverified code. That is exactly what `miri.rs` exists for [V miri.rs:1-6].
- Run with `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::`, then again with `-Zmiri-tree-borrows`. Both pass.

### Q7 Units: see §5.

## 2. Rationale
- **Preemption counter.** It is the only per-file observable that the seeded runs exercised what they claim to. Counting only policy switches makes `Sequential = 0` an invariant, which `par.rs` can also assert.
- **Clap-derived value flags.** The tail split is hand-rolled before clap. Deriving it from clap's definition keeps a single source of truth.
- **Skipping seeded runs after a VM bug.** Heap-error texts carry schedule-dependent worker IDs, and the result is already a failure.
- **Source-level child-error tests in `par.rs`.** They check the user-visible rule (the user's decision) through generated `fork$` code. Unit order allows it: S4-1 comes after S3-3.

## 3. Alternatives rejected
- A sixth test `ui::par_schedules`.
- Seeded fast-mode runs.
- Seeding every category: the user decided against it, and a program without `par` never switches.
- `switches > 0` on S1's all-switch count: always true for any `par`.
- `switches(seed) > switches(Sequential)`: not guaranteed, since a preemption can replace a forced switch.
- A waiver marker for short files: a test-only exception.
- Comparing heap-error text across seeds.
- A hand list of value flags.
- A blanket "zero the differing field" fallback (A-2).
- UI tests for `DepthLimit`.

## 4. Assumptions
- A1 S1 accepts the `preemptions` refinement. Otherwise C-1 has no enforced check, only the prose rule.
- A2 S3-3 lands the eml_types `Lin`-result change together with the front end (user decision). Before S4-1 starts, check that `timeout 20 cargo run -p eml_cli -- run --debug-heap probe/lin_ret.em` prints `3` and `probe/lin_cap2.em` prints `3`. That is the gate for tests 12 and 13.
- A3 `fork_worker` hands out 1 first in checked mode (S2 Q3).
- A4 The fork tables live in the constants' work heap, so `RunStats` of programs without `par` do not change [V constants.rs work heap; lib.rs:315-335 reads only the run heap].
- A5 clap 4's `CommandFactory` exposes each argument's long name and whether its action takes values [I].

## 5. Implications for implementation

### Units
Order: S1-1 → S2 ops → S3-1 → S3-2 → S1-2 → S1-3 → S3-3 → **S4-1 → S4-2** → S1-4.

| Unit | S4 items | Completion (exact) |
|---|---|---|
| S1-1 | `split_script_args` rule and the cli.rs table test; eml_vm api.rs tests; `Finished.preemptions` (0 for now) | `cargo nextest run -p eml_cli --test integration -E 'test(cli::)'` and `cargo nextest run -p eml_vm --test integration -E 'test(api::) \| test(bench::)'` pass; bench snapshots unchanged |
| S2 U3 / U4 | `fork_marking_lets_the_child_touch_what_the_closure_reaches`; the S2 `TransferLeak` test | S2 gate G |
| S1-2 | Additions to S1's `par.rs` list: (1) a helper `run_par_lir(text)`: checked under `Sequential` and both seeds, which must agree on output and error text, then fast `Sequential` when `Ok`; (2) `a_left_abort_does_not_rescue_a_right_error` and `the_left_error_wins_when_both_children_fail` (LIR, small `max_depth`); (3) `a_seed_reruns_the_same_schedule` (equal `preemptions`); (4) `sequential_never_preempts` (0) and `seeds_preempt_long_children` (> 0); (5) `a_child_that_leaks_fails_at_its_join`; (6) the RunStats test uses `schedule_independent` and seeds 1 and 2026; (7) `miri::par_switches_between_tasks`. Hand-written LIR that declares a never-only effect also defines its `fork$`/`reperform$`. No existing hand-written LIR does: only `Prelude.IO` is declared `never`, in `tests/ir/mask.lir` and `multi_resume.lir` [V] | `cargo nextest run -p eml_vm --test integration -E 'test(par::) \| test(miri::)'`; both Miri commands in Q6 |
| S1-3 | (S1's `roots.rs` tests: guard for the root share) | as S1 |
| **S4-1** | The harness (Q1) with its self-tests; `eml_test_support::schedule_independent`; files 1–16; `bench/par_tree.em` with all its same-commit consumers; source-level child-error tests in `par.rs` (below); mutation runs M1–M3 | See "S4-1 completion" below |
| **S4-2** | The two transcripts; terminal.rs `repl_runs_par_under_a_seeded_schedule` | `cargo nextest run -p eml_cli --test integration -E 'test(repl::) \| test(terminal::)'` passes; both snapshots end `no leaks` |
| S1-4 | benchmarks.md 「今の数値」 (includes `par_tree`), status.md, vm.md | as S1 |

Source-level child-error tests (S4-1, in `crates/eml_vm/tests/par.rs`, through `eml_test_support::execute` with `max_depth: 1000`, under `Sequential` and both seeds checked, plus fast `Sequential`):
- `a_right_depth_error_is_reported_after_the_left_finishes`: the left prints nothing and returns; the right recurses without limit. Error: `the call depth limit is exceeded in `<fn>``, identical under every schedule.
- `a_left_abort_does_not_rescue_a_right_error`: the left does `Fail.fail` under a parent `to_result`; the right recurses. Error: the right one.
- `the_left_error_wins`: both recurse in differently named functions. The text names the left function.

The LIR versions in S1-2 check the VM's sequencing before the front end exists. These check the same rules through generated code and source text, and the pair goes into 「重複させない」.

S4-1 completion:
1. `cargo nextest run -p eml_cli --test integration -E 'test(ui::)'` passes; `cargo insta test -p eml_cli --test integration --check` shows nothing pending.
2. Per-file time (C-4): `cd tests/ui && for f in run/par/*.em run-fail/par/*.em; do EML_UI_FILTER="$f" cargo nextest run -p eml_cli --test integration -E 'test(=ui::run) | test(=ui::run_fail)' 2>&1 | grep -E 'PASS|FAIL'; done`. Every reported duration is < 1.000 s.
   - The baseline on this machine is 0.036–0.038 s for one file, compile included [V `run/basics/hello.em`, `run/panic/nested_inner_catches.em`, `run/effects/mask_never.em`].
   - The whole of `ui::run` takes 4.25 s single-threaded and 0.78 s in parallel [V].
   - In the ordinary parallel run, `ui::run` and `ui::run_fail` each stay < 5 s.
3. `cargo nextest run -j 1 -p eml_vm --test integration -E 'test(=bench::par_tree) | test(par::)'`: every test < 1 s; `test(bench::)` passes, with the `unwind_points` line added.
4. `cargo nextest run -p eml_core_ir --test integration -E 'test(lowering::)'` passes.
5. M1–M3 fail as stated in Q4, are reverted, and are recorded in the commit message.
6. Workspace: `cargo nextest run`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

### Docs (S4-1 / S4-2 / S1-1)
- testing.md 「UI テスト」: the seeded runs (`par/` only, checked mode, the seeds, what is compared, `schedule_independent` and its field list with reasons, the skip after VM-bug results, preemption enforced, the 16-safepoint rule with vm.md's safepoint list, how to reproduce).
- testing.md 「2つのモードで流す」: seeded runs are checked only.
- testing.md 「`eml_vm` のテスト」: `par.rs` (LIR and source parts), the `run_unverified` wording, the `miri.rs` path list.
- testing.md 「重複させない」: the invariance trio and the child-error pair.
- testing.md 「REPL のトランスクリプト」: transcripts run sequentially.
- testing.md 「CLI のテスト」: `--sched` and the split rule (S1-1).
- testing.md 「よく使うコマンド」: `EML_UI_FILTER=/par/` and `eml run --debug-heap --sched seeded:N`.
- benchmarks.md 「基準のプログラム」: the `par_tree` row.
- architecture.md:357 and stdlib.md 「起動」: the split rule (S1-1).
- vm.md 「`RunStats`」: `Finished.preemptions`.
- multicore.md 「テスト」 and roadmap.md:30: the seeded scope is `par/` (user decision); the negative tests are the permanent guards; std-par and worker-threads items as in Q6.
- status.md 「既知の制限」: the uncaught child Panic names `reperform$Prelude.Panic.panicked` instead of a source location.

### Test changes by kind (stage-wide)
**Kind 1:**
- K1-1 (S2 U1): `crates/eml_rt/src/heap/tests/recover.rs::recovery_refuses_what_it_cannot_copy` loses its 4th case (a shared cons) [V :166-190]. `recovery_copies_shared_objects_as_local` replaces it.
- No UI test moves between top directories; no existing UI test uses `par` [V].

**Kind 2:**
- K2-1 (S2 U3): `share.rs::a_lin_object_moves_to_the_receiver_and_its_children_are_shared` [V :77-94]: renamed; the child's expected rc goes from −1 to 1, owner 7. The other lin `share` tests do not change [V :96-113, :259-270, :328-344].
- K2-2 (S2 U2): existing eml_rt tests that hit `OwnedMiscount`. Expected empty [I].
- K2-3 (S3-3): exactly two tie tests [V S3 r1: 3548 passed, 2 failed]: `eml_hir externs::every_extern_function_is_declared_once_in_std` and `eml_core_ir externs::faulting_rows_carry_panic`. Both are generalized, not carved out for `Par`.
- K2-4 (S4-1): `bench.rs::unwind_points` gains `par_tree: N`.
- The old conditional K2-5 is gone: forks exist only with `Call::Par` (S1 r3), so existing Core IR, LIR, `instances.rs` and `unwind_points` lines stay byte-identical.
- The `RunStats` of the existing 40 bench programs do not change; S1-1 and S2's gate check that.

**Kind 3:**
- ui.rs harness refactor (`Ran.finished`, `execute_in(.., sched)`, `assert_agrees`).
- bench.rs `PROGRAMS` length 41.
- miri.rs `run_program` gains a `sched` argument.
- `RunConfig` construction sites (non-exhaustive, built with builders), `Finished` (read only by tests) and `Task` (tests only assert `Send`) need no edits [V grep].

## 6. Unresolved items (defaults)
- U-a: the `preemptions` refinement of S1's `Finished.switches` (A1). Default: adopt.
- U-b: the clap-derived value-flag set. Default: derive; fallback `const RUN_VALUE_FLAGS`.
- U-c: `par_tree` sizes. Default: 2^12 − 1 nodes and depth 4, shrunk until three runs take < 1 s.
- U-d: S2's `TransferLeak` Display reads "1 objects". Default: as S2 states; the VM test matches the variant and the text.

## 7. Additional checks
- [V, built `eml`] Today's CLI: `run --debug-heap --debug-heap f` → exit 2 ("cannot be used multiple times"); `eml --debug-heap f` → exit 2; `run f --debug-heap` → exit 0, the flag going to the script; `run --debug-heap` without a file → exit 2.
- [V, `eml repl --debug-heap` on the base] The `upto`/`total` definitions, `(total xs, total xs * 2)` → `(5050, 10100)`, `let p = …; p` → `([1, 2, 3], 5050)`, `Fail.to_result (fn () -> Fail.fail "l")` → `Err "l"`, an `effect Stop where never stop : Int -> a` input, and `handle (stop 1) with | ctl stop n -> (n, n)` → `(1, 1)`.
- [V] Per-file UI cost and the totals quoted in §5. Safepoint sites (vm.md:145).
- [V] Consumers of `bench/`: bench.rs, programs.rs (lowering, census), run.sh. scripts.sh lists its programs by name.
- [V] `miri.rs` runs hand-written LIR in both modes through `run_program` (miri.rs:10-26).
- `cargo build` and `cargo nextest` ran only inside the scratch clone. Nothing is running in the background.

---

## Response table to S4-review-r1

| ID | Verdict | Reason | Where it changed |
|---|---|---|---|
| C-1 | Accepted, with a different check | Counting every switch makes `switches > 0` always true for any `par` (each join forces switches). The check is therefore `preemptions > 0` per `run/par/` file per seed, with `Finished.switches` refined to count policy-initiated switches only. The "safepoint" definition is given (vm.md:145), and the 16-safepoint bound is derived from S1's budget | Q1 (preemption bullet), A1, U-a, S1-1/S1-2 rows |
| B-1 | Accepted | General rule (value-taking options consume their value; the set is read from clap's `run` definition). Every case asked for is in the table, including the repeated flag (exit 2, as clap does today [V]), trailing flags, the implicit form and `run main.em --sched bogus` → 0 | Q5 table, Docs (architecture.md, stdlib.md), S1-1 |
| C-2 | Resolved by settled facts | S1 r3: forks only with `Call::Par`. The REPL `Stop` step works because the session program is lowered whole at each input, and that input contains `par` | Q5 recover.repl, K2 list (K2-5 removed) |
| C-3 | Accepted | Permanent guards named for marking (UI `shared_capture.em`, VM `a_transferred_mut_array_is_written_by_both`, root and fork-evv guards) and for transfer (UI `fresh_result.em`, VM `a_lin_result_moves_back_to_the_parent`, `bench::par_tree`). M1–M3 remain only as a one-off confirmation | Q4 |
| B-2 | Accepted | Worker IDs depend on the schedule (interleaved `fork_worker` calls, recycling). Seeded runs are skipped when the sequential result is a VM bug (`!fast_mode_reports`). No other compared text contains IDs | Q1 order and B-2 bullet |
| B-3 | Rejected (premise wrong); M3 text corrected | The reviewer reasons from runtime errors, but test 14's children *abort*. Children are not cancelled, so under `Sequential` the right child still runs after the left aborts, and the join holds both aborts. M3 therefore changes the sequential snapshot too. The r1 wording "in every schedule" was imprecise; it now says where the failure shows | Q4 M3 |
| B-4 | Accepted | Same-commit consumer list (bench.rs ×3, benchmarks.md row), automatic consumers (programs.rs → lowering and census, run.sh), and the one not affected (scripts.sh). `par_tree` now asserts its own invariance under the seeds | Q3 |
| B-5 | Accepted, settled by the user decision and S3 facts | Capture-and-consume [V S3] and `Lin` results (user decision, S3-3) make tests 12–13 sound. Test 13 is the source-level rc −1 test. The gate before S4-1 is that `probe/lin_ret.em` and `lin_cap2.em` print `3` under `--debug-heap`. `par_as_value` follows S3's `$externN` fact | Q2 #12, #13, #15; A2 |
| C-4 | Accepted | Measured baseline (0.036 s per file; 4.25 s single-threaded / 0.78 s parallel for the whole of `ui::run`) and a per-file loop command with a < 1.000 s criterion | §5 S4-1 completion item 2 |
| C-5 | Accepted (deviation made explicit; coverage added) | Not reachable in UI tests: value faults become aborts in children; heap faults and stops end the run; `DepthLimit` needs 1 GiB. Added source-level child-error tests in `par.rs` with small `max_depth` and the UI seeds; S1-2's helper also uses seeds 1 and 2026 | Q2 last bullet, §5 source-level tests |
| C-6 | Accepted | `try_map`, float `fold` and `for_mut` are std-par; N ≥ 2, TSan and stress are worker-threads. Miri is in scope: `miri::par_switches_between_tasks` in S1-2, both Miri settings | Q6, S1-2 row |
| A-1 | Accepted (REPL steps verified) | Syntax and results checked on the base with `eml repl` (effect declaration, `ctl` without `return`, tuple display). Only the `par` itself is unverifiable before the stage; its expected values equal the sequential forms | Q5 |
| A-2 | Accepted | One `schedule_independent` with per-field reasons, shared by the harness, `par.rs` and `bench.rs`. Today it excludes `peak_objects` only. A field that differs later is either redefined or excluded in that commit with its reason; there is no blanket fallback | Q3 first bullet |
