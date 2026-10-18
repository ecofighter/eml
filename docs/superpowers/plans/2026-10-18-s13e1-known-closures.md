# S13e-1 Known-Closure Specialization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the Core IR pass `optimize` between translate and EP, and teach it known-closure specialization: an `apply` of a known closure becomes a direct `call`, translate's one-instruction wrappers collapse back to their instruction, and a function or closure that receives a known closure is copied with the closure's captures opened as parameters.

**Architecture:** A new module `crates/eml_core_ir/src/optimize/` runs a per-function worklist over the program. It keeps a frozen copy of translate's output, so every copy is built from the callee's translate code and the result does not depend on processing order. Copies are named only from their key, are never `internal`, and are deduplicated by key. After the worklist finishes, `yields` is recomputed as a least fixpoint, and unreachable functions are dropped. `eml_vm::RunStats` gains `allocations`, which surfaces `eml_rt`'s existing checked-mode counter, so the bench tests can compare automatic and hand specialization.

**Tech Stack:** Rust (`crates/eml_core_ir`, `crates/eml_vm`), insta, `eml_test_support`, the UI and REPL harnesses, `bench/` with `crates/eml_vm/tests/bench.rs`, `bench/run.sh` (macOS).

**Spec:** `docs/superpowers/specs/2026-10-18-s13e-specialization-design.md` (sections 「最適化パスの共通の形」 and 「S13e-1 既知の閉包への特殊化」; completion bar in 「作業の分け方と完了の条件」)

## Global Constraints

- Code comments and docs are Japanese, written with the `yomiyasu:yomiyasu` skill. Comments explain why and cite `docs/` paths. CLAUDE.md stays English.
- Pass order is translate, `optimize`, EP. `eml_core_ir::Pass` is `{Translate, Optimize, Evidence}`, and `Pass::Optimize.name()` is `"optimize"`. In debug builds `optimize` is followed by `verify_translated`, reported as `internal error: invalid Core IR after optimize: …`. `optimize` emits no `tail`, `box`, `unbox` or RC instruction.
- Determinism (spec 「鍵と名前」):
  - A copy is built from the callee's **translate output**, then rewritten by the same rules.
  - Every decision (specialize, cut off) depends only on the key and that translate code. No whole-program budget.
  - Cutoff is by key nesting depth: `MAX_KEY_DEPTH = 4`. The implementer may tune it and must record why in `benchmarks.md`.
- Copies are not `internal`. Originals keep their marks.
- Translate's shared wrappers (`op$…`, `con$…`, `cont$`, `cont$state`) are never called directly and never copied. `crates/eml_core_ir/tests/boxing.rs` must stay green unchanged.
- An `apply` with a non-empty `mask` is never turned into a `perform`.
- When a function containing a `handle` is copied, its clause and `return` function values are copied under names derived from the copy, so each clause function belongs to exactly one `handle` (EP's `debug_assert!` in `crates/eml_core_ir/src/evidence/handlers.rs` `entries`).
- Test changes:
  - Kind 1 (pass/fail): none expected. Any one found goes into the spec's 「種類1」 with its reason, and gets the user's confirmation before commit.
  - Kind 2 (expected values): within the spec's 「種類2」 scope. That covers Core IR/LIR snapshots after `Pass::Evidence` and later, `RunStats` snapshots, and internal-error texts with function names. Never bend the design to keep a test.
- Gate per task: the task's test commands, then `cargo test` (whole workspace), `cargo clippy --all-targets`, and `cargo fmt --check`. Use `git diff --no-ext-diff`.
- Commit messages end with the attribution lines the controller gives.

## Review Focus

- A `once` closure that captures a `File`, passed to a higher-order function that calls it exactly once. The copy must drop or close the `File` exactly once, with no leak in checked mode. Test: Task 4 UI test `tests/ui/run/functions/specialized_once_closure.em`.
- A closure passed to a function that only stores it in a constructor and returns it. It must not be specialized (the parameter is not applied, passed on or captured), and the program's output is unchanged. Test: Task 4 `a_closure_only_stored_in_data_is_not_specialized`.
- A CPS-style loop whose closure grows each step (`let go k n = if n == 0 then k 0 else go (fn x -> k (x + 1)) (n - 1)`). Compilation must terminate at `MAX_KEY_DEPTH` and the output must be correct. Test: Task 4 UI test `tests/ui/run/functions/cps_closure_chain.em`.
- A known closure applied inside a uniform (`_`) instance created by polymorphic recursion. Repr mismatches between `tobj` arguments and the closure's native parameters must be fixed by box insertion, with correct output. Test: Task 4 UI test `tests/ui/run/functions/specialized_uniform_instance.em`.
- In the REPL, a later input passes its own lambda to an earlier input's higher-order function, twice in different inputs. Both copies must append without a debug `Mismatch`. Test: Task 5 transcript `crates/eml_cli/tests/repl/generics/specialized_across_inputs.repl`.

---

### Task 1: `RunStats::allocations` and the S13e bench programs

**Files:**
- Modify: `crates/eml_vm/src/lib.rs` (`RunStats`, `HeapWork`, `HeapWork::of`, `heap_work`, the `RunStats` literal in `run_entry`)
- Create in `bench/`, each printing one result line:
  - `orders.em` / `orders_by_hand.em`: 100,000 generated `Order` records through `List.filter` / `List.map` / `List.fold` with lambdas. The hand version is the spike's `orders_spec`.
  - `push_fold.em` / `push_fold_by_hand.em`: `Yield.fold (fn acc x -> acc + x) 0 (nums 100000)`. The hand version uses a dedicated `sum` handler with `acc + x` inline.
  - `iter_pipeline.em` / `iter_pipeline_by_hand.em`: `Iter.generate (nums 100000)` through `Iter.filter` / `Iter.map` / `Iter.fold`. The hand version uses dedicated `evens` / `squares` / `sum`.
  - `iter_generate.em`: `Iter.fold (fn acc x -> acc + x) 0 (Iter.generate (nums 100000))`.
  - `sum_loop.em`: the same sum of 1..100000 as a direct tail-recursive loop. It is the baseline the spec compares `iter_generate` and `push_fold` against.
  - `tree_by_hand.em`: `bench/tree.em` with `fold (fn acc x -> acc + x)` replaced by a dedicated `fold_sum`.
  - `upsert.em` / `upsert_by_hand.em`: the `logstat` AA-tree `upsert key increment` over 20,000 generated string keys, then `sort_by (fn (_, a) (_, b) -> a > b)` of the entries. The hand version has `increment` and the comparison inlined.
- Modify: `crates/eml_vm/tests/bench.rs` (`PROGRAMS`, one snapshot test per new program, the `allocations` field in every existing snapshot), `crates/eml_vm/tests/scaling.rs` and any other `RunStats` snapshot (field added; kind 2)
- Modify docs: `docs/spec/vm.md` (the `RunStats` section: the new count, checked mode only, copied from `eml_rt`'s `Stats::allocations`), `docs/implementation/benchmarks.md` (the program table and the 「回数のテスト」 field list), `CLAUDE.md` (the `RunStats` description: "ten work counts", naming `allocations`)

**Interfaces:**
- Produces: `pub allocations: u64` in `eml_vm::RunStats`, placed after `unboxes`. It counts heap allocations in checked mode and is 0 in fast mode. Recovery work is excluded, the same way as `rc_increments`.

- [ ] **Step 1: Write the failing test.** In `bench.rs`, add the new names to `PROGRAMS` and add one `stats("<name>", "<expected output>")` snapshot test per new program. The expected output strings are the programs' printed results; compute them with Python before writing the snapshot. In `crates/eml_vm/tests`, add `allocations_are_counted_in_checked_mode`: run `bench/list.em` through `eml_test_support::run_stats` and assert `stats.allocations >= 200_000` (100,000 `Cons` cells each for `range` and `map`).
- [ ] **Step 2: Run** `cargo test -p eml_vm --test integration bench` — it fails to compile (no field `allocations`).
- [ ] **Step 3: Implement.** Thread `Stats::allocations` through `HeapWork`, using the same before/after subtraction as `rc_increments`.
- [ ] **Step 4: Run** `cargo test -p eml_vm` and `cargo insta review`. Accept only diffs that add `allocations` or are new snapshots. Then run `cargo test` for the whole workspace.
- [ ] **Step 5: Commit** "Count allocations in RunStats and add the S13e bench programs".

### Task 2: The `optimize` pass skeleton

**Files:**
- Create: `crates/eml_core_ir/src/optimize/mod.rs`
- Modify:
  - `crates/eml_core_ir/src/lib.rs` (`mod optimize; pub use optimize::optimize;`)
  - `crates/eml_core_ir/src/pipeline.rs` (`Pass::Optimize`, `name`, `core_until` calls `optimize` then `check(&program, Pass::Optimize)`, which uses `verify_translated`)
  - `crates/eml_core_ir/tests/common/mod.rs` (`read_back`: `Pass::Optimize` verifies with `verify_translated`; mechanical)
- Test: `crates/eml_core_ir/tests/optimize.rs` (new; declare it in `tests/main.rs`)

**Interfaces:**
- Produces:
  - `pub fn optimize(program: &mut Program)`.
  - `pub(crate) fn retain_reachable(program: &mut Program)` in `optimize/reach.rs`. It keeps the functions reachable from `program.entry` through `Call::Direct`, `Rhs::MakeClosure`, `Atom::Fn` and the `Call::Handle` atoms, then renumbers every `FnIdx` in the program, in the original relative order.

- [ ] **Step 1: Write the failing tests** in `optimize.rs`:
  - `optimize_keeps_a_first_order_program`: the snapshot of `core_text("main : Unit -> <IO> Unit\nmain () = println (show (1 + 2))", Pass::Optimize)` equals `core_text(…, Pass::Translate)` (assert equality, not an insta snapshot).
  - `unreachable_functions_are_dropped`: hand-written Core IR text, read with `eml_core_ir::parse`. It holds an entry, a function the entry calls, and an unreferenced function. After `retain_reachable`, the printed program lacks the unreferenced function, and the call's `FnIdx` still names the right function.
- [ ] **Step 2: Run** `cargo test -p eml_core_ir --test integration optimize` — it fails to compile.
- [ ] **Step 3: Implement.** `optimize` only calls `retain_reachable` for now.
- [ ] **Step 4: Run** `cargo test -p eml_core_ir` and then `cargo test`. Translate never makes unreachable functions, so no snapshot should change. Investigate any change.
- [ ] **Step 5: Commit** "Add the optimize pass between translate and EP".

### Task 3: Known `apply` to `call`, wrapper collapse, and `yields`

**Files:**
- Create: `crates/eml_core_ir/src/optimize/known.rs` (the per-function rewrite), `crates/eml_core_ir/src/optimize/yields.rs`
- Modify: `crates/eml_core_ir/src/optimize/mod.rs`
- Test: `crates/eml_core_ir/tests/optimize.rs`

**Interfaces:**
- Produces (crate-private):
  - `struct KnownClosure { function: FnIdx, captures: Vec<Atom> }`.
  - `fn known_closures(function: &CoreFn) -> HashMap<VarId, KnownClosure>`: variables bound by `MakeClosure`. A use of `Atom::Fn(f)` counts as `KnownClosure { function: f, captures: vec![] }`.
  - `enum Wrapper { Perform { effect: u32, op: u32, resumable: bool }, Con(Ctor), Resume { stateful: bool } }`.
  - `fn wrapper(function: &CoreFn) -> Option<Wrapper>`. It recognizes **by shape**: one block whose statements are exactly one `let r = <perform | con | resume>` over the parameters in order (`cont$` resumes `(k, v, ())`, `cont$state` resumes `(k, v, s)`), ending in `return r`.
  - `fn recompute_yields(program: &mut Program, keep: &HashSet<FnIdx>)`.

Rules (spec 「書き換え」):
- An `apply` of a known closure whose argument count makes it exactly saturated becomes `Call::Direct(f, captures ++ args)`, keeping the `mask`.
- An under-saturated `apply` becomes `MakeClosure(f, captures ++ args)`.
- An over-saturated `apply` with an empty mask becomes `Direct` followed by `apply` of the rest. This is translate's known-callee split, reusing translate's helper if one exists. With a non-empty mask, it stays.
- A saturated `apply`, or a `Direct` call, of a `Wrapper` function becomes that instruction. `Perform` is used only when the mask is empty, and `Resume` takes the wrapper's `k` capture as its continuation.
- `yields` is the least fixpoint where a function yields iff its translate `yields` is set and its body has a resumable `perform`, a `yields` `apply`, a `Direct` call to a yielding function, a `resume`, or a `handle`.
  - `keep` holds the functions whose mark is never recomputed: functions referenced by a `handle`'s body, clause or `return` atom, and the wrappers. They keep their translate mark (spec 「`yields` の印」 rules for handle bodies and clauses).

- [ ] **Step 1: Write the failing tests** (insta snapshots of `core_text(…, Pass::Optimize)` unless noted):
  - `a_known_lambda_is_called_directly`: `let f = fn x -> x + 1` then `f 2` in `main`. The snapshot shows `call main$lambda0(2)` and no `apply`.
  - `a_partial_application_is_called_directly`: `add : Int -> Int -> Int`, `let g = add 1` then `g 2`, giving `call add(1, 2)`.
  - `an_under_saturated_known_apply_builds_a_closure`: `add3 : Int -> Int -> Int -> Int`, `let g = add3 1` then `let h = g 2` then `h 3`.
  - `yield_as_a_value_becomes_a_perform`: inside a function of row `<Yield Int>`, `let y = yield` then `y 1`. The output has `perform Std.Yield.Yield.yield(1)`, and no function named `op$…` is called.
  - `a_masked_apply_of_an_operation_value_stays`: an operation value applied under a `mask` (use the S13b-1 positional-mask fixture from `crates/eml_core_ir/tests/translate.rs`). The `apply` remains.
  - `a_constructor_value_becomes_a_con`: `let p = Pair 1` then `p 2`, giving `con Pair`.
  - `a_caller_without_yield_points_stops_yielding` (assert, not snapshot): `f : Int -> <State Int> Int` with `f n = let g = fn x -> x + n in g 1`, called from `main` under `State.run`. `f` is `yields` after `Pass::Translate` (its row may yield) and not `yields` after `Pass::Optimize` (no yield point is left in its body).
- [ ] **Step 2: Run** `cargo test -p eml_core_ir --test integration optimize` — the new tests fail.
- [ ] **Step 3: Implement** the rules in `known.rs` and `yields.rs`. `optimize` runs `known` over every function, then `recompute_yields`, then `retain_reachable`.
- [ ] **Step 4: Run** `cargo test`. Review every changed snapshot with `cargo insta review`. Accept only kind-2 changes of the spec's scope (`apply` becoming `call`, `yields` marks, dropped functions, `RunStats`). `boxing.rs` must not change.
- [ ] **Step 5: Commit** "Call known closures directly in optimize".

### Task 4: Copies keyed on known closure arguments and captures

**Files:**
- Create: `crates/eml_core_ir/src/optimize/specialize.rs` (keys, names, the worklist, copying with substitution)
- Modify: `crates/eml_core_ir/src/optimize/mod.rs`, `known.rs`
- Test:
  - `crates/eml_core_ir/tests/optimize.rs`
  - UI tests: `tests/ui/run/functions/specialized_once_closure.em`, `cps_closure_chain.em`, `specialized_uniform_instance.em`, and `tests/ui/run/std/specialized_handlers.em`

**Interfaces:**
- Produces (crate-private):
  - `struct ClosureShape { function: FnIdx, captures: Vec<(Repr, Mark)> }`.
  - `struct Key { callee: FnIdx, args: Vec<(usize, ClosureShape)>, owner: Option<String> }` (positions ascending) for both `Direct` calls and `MakeClosure` captures. `owner` is `None` except for a clause or `return` function of a `handle` inside a copy, where it is the name of the copy that contains the `handle`.
  - `fn key_depth(key: &Key, program: &Program) -> usize`: 1 plus the largest depth among the key's closure functions that are themselves copies.
  - `fn copy_name(key: &Key, program: &Program) -> String`. The form is `<callee>{<i>=<closure fn>, …}`, with `<callee>@{…}` and the owner as `via <owner>` for handle-owned copies. Names over 64 characters become `<callee base>@#<16 hex>`, using the same FNV-1a as `crates/eml_core_ir/src/translate/names.rs`; move `Fnv` to a shared place if needed. The text form must quote these names and read them back (`core_text`'s `read_back` checks this).

Rules (spec 「書き換え」/「鍵」/「鍵と名前」):
- Specialize argument position `i` of a `Direct` call or capture position `i` of a `MakeClosure` only when the argument is a known closure and, in the callee's translate body, parameter `i` is applied, passed to a `Direct` call, captured by a `MakeClosure`, or used as a `handle` atom.
- The copy is the callee's translate body with parameter `i` replaced by fresh parameters, one per capture. Their `VarInfo` takes the capture atom's Repr and mark (a constant capture takes the callee parameter's Repr and `unr`). Inside the copy, those parameters form a `KnownClosure` for parameter `i`.
- Wherever the copy needs the closure as a value, it rebuilds it in place with `MakeClosure(f, params)` (or `Atom::Fn(f)` if there are no captures). Such places are a `Term::Return`, a `Con` argument, an `apply` argument, an unspecialized call, or a capture that is not specialized.
- Identical keys share one copy. Recursion such as `map f rest` hits the same key.
- If `key_depth > MAX_KEY_DEPTH`, do not specialize: leave the call or closure as it was.
- When copying a function containing a `handle`, copy its clause and `return` function values with `owner = Some(copy name)`, applying the same capture opening.
- New copies go on the worklist and are rewritten by Task 3's rules and these rules.

- [ ] **Step 1: Write the failing tests:**
  - `map_is_copied_for_a_known_lambda` (snapshot of the function list's names plus the copy's body): `List.map (fn x -> x * 2) xs` gives a copy named `Std.List.map@[Int, Int]{0=main$lambda0}` (or whatever `copy_name` yields for that key). It holds `call main$lambda0(x)` and no `apply`. Its local `go` is reached through the copy.
  - `a_recursive_call_reuses_the_copy` (assert): `bench/tree.em`'s `fold` has exactly one copy for its lambda.
  - `captures_become_parameters` (snapshot): `let k = 3` then `List.map (fn x -> x * k) xs`. The copy takes `k` as a parameter and builds no closure.
  - `a_closure_captured_by_a_closure_is_specialized` (snapshot): `Iter.map (fn x -> x + 1) it` gives `closure "Std.Iter.map@[Int, Int]$lambda0{…}"(it)`.
  - `generate_reaches_the_handle_body` (assert on names): `Iter.generate (nums 3)` yields a copy of `Std.Iter.generate@[Int]$handle0` whose body calls `nums` directly.
  - `a_closure_only_stored_in_data_is_not_specialized` (snapshot): `wrap : (Int -> Int) -> Option (Int -> Int)` with `wrap f = Some f`. There is no copy of `wrap`.
  - `a_copy_does_not_depend_on_the_rest_of_the_program` (assert): program A has `main` calling `List.map (fn x -> x + 1) xs`. Program B is A plus an earlier top-level `other : List Int -> List Int` with `other ys = List.map (fn y -> y * 2) ys`, called from `main` after the first `map`. The printed function named for A's copy is byte-identical in A and B.
  - `two_handler_copies_keep_their_own_clauses` (assert): `Yield.for_each (nums 3) (fn x -> println "\{x}")` and `Yield.for_each (other 3) (fn x -> println "\{x}")` in one `main` give two clause functions, each referenced by exactly one `handle` after `Pass::Optimize`. `core_until(…, Pass::Evidence)` does not panic.
  - The four UI tests:
    - `specialized_once_closure.em`: open a file and pass `fn () -> Fs.close f` as a `once` closure to `call_once : (once Unit -> <IO> Unit) -> <IO> Unit`.
    - `cps_closure_chain.em`: the CPS loop from Review Focus with `n = 50`, printing `50`.
    - `specialized_uniform_instance.em`: a polymorphic-recursion `data Nested a = | Flat a | Nest (Nested (List a))` with a signature-annotated `map_nested : (a -> b) -> Nested a -> Nested b`, applied to `fn x -> x + 1`.
    - `specialized_handlers.em`: the two-`for_each` program above.
- [ ] **Step 2: Run** `cargo test -p eml_core_ir --test integration optimize` and `cargo test -p eml_cli --test integration ui::` — the new tests fail (the UI tests may pass already; they guard regressions).
- [ ] **Step 3: Implement** `specialize.rs` and wire it into `optimize`'s worklist before `recompute_yields` and `retain_reachable`.
- [ ] **Step 4: Run** `cargo test` and `cargo insta review`. Accept kind-2 changes within the spec's scope only. Every UI test's stdout/stderr must be unchanged. A change in a UI snapshot means a bug, not a snapshot update.
- [ ] **Step 5: Commit** "Specialize functions and closures on known closures".

### Task 5: The S13e-1 bar, REPL determinism, and the stage docs

**Files:**
- Modify: `crates/eml_vm/tests/bench.rs` (comparison test; snapshots now reflect `optimize`)
- Create: `crates/eml_cli/tests/repl/generics/specialized_across_inputs.repl`
- Modify docs:
  - `docs/spec/core-ir.md`: the 「パス」 table gains `optimize`, plus a new section 「最適化」 with the S13e-1 rules: keys, names, determinism, non-`internal` copies, wrappers, handle-owned clause copies, and `yields`. The 「REPL の入力」 wrapper bullet changes to say `optimize` never calls the wrappers directly.
  - `docs/implementation/architecture.md` and `CLAUDE.md`: the Core IR pass list and `Pass::{Translate, Optimize, Evidence}`.
  - `docs/implementation/status.md`: closures flowing through data or block parameters are not specialized, and S13e-1 does not inline.
  - `docs/implementation/benchmarks.md`: a new 「S13e-1」 section. It holds the spec's 「計測 (S13e の前)」 table as the before values and the `bench/run.sh` table after, with the environment lines.
  - `docs/future/roadmap.md`: mark S13e-1 済み and say where its decisions went.

**Interfaces:**
- Consumes: `RunStats::allocations` (Task 1), `Pass::Optimize` (Task 2).

- [ ] **Step 1: Write the failing tests:**
  - `specialization_matches_hand_written_work` in `bench.rs`: for each pair `("orders", "orders_by_hand")`, `("push_fold", "push_fold_by_hand")`, `("iter_pipeline", "iter_pipeline_by_hand")`, `("tree", "tree_by_hand")`, `("upsert", "upsert_by_hand")`, assert `allocations`, `boxes` and `unboxes` are equal (spec: RC and instruction counts are S13e-3's bar).
  - The transcript `specialized_across_inputs.repl`:
    - define `twice f x = f (f x)`;
    - evaluate `twice (fn x -> x + 1) 3`, expecting `5`;
    - evaluate `twice (fn x -> x * 2) 3`, expecting `12`;
    - define `let k = 10`;
    - evaluate `twice (fn x -> x + k) 1`, expecting `21`.
- [ ] **Step 2: Run** `cargo test -p eml_vm --test integration bench::specialization_matches_hand_written_work` and `cargo test -p eml_cli --test integration repl::`. If a pair differs, find which rule is missing before touching the bench program. A hand version may only be fixed where it does more or less work than its automatic twin for a reason unrelated to closures, and the reason goes in the commit.
- [ ] **Step 3: Fix** whatever the comparison exposes in `optimize`.
- [ ] **Step 4: Run** `cargo test`, then `bench/run.sh`, and record the output in `benchmarks.md` 「S13e-1」. Then update the docs listed above (run the yomiyasu linter on each changed Japanese doc).
- [ ] **Step 5: Commit** "Meet the S13e-1 bar and document the optimize pass".
