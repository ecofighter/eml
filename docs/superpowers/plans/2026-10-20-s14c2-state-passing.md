# S14c-2 State-Passing Handler Specialization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Specialize a handler whose operations all resume and whose clauses all resume immediately (`Std.State`, `Yield.fold`, `Yield.for_each`, user handlers of that shape) even when its path reaches a `perform` through non-tail calls, by threading the state through blocks and returning it from the copies. A non-tail `put` loop then runs with `evidence_steps: 0` and the same work as a hand-written state-passing loop. Nested handlers of this kind are specialized inside out.

**Architecture:**
- `optimize/handlers/` gets a second form beside S13e-2's tail-path form. A handler's form is fixed by its clauses:
  - **Passing**: every operation is resumable and every clause is `ClauseForm::Resuming`.
  - **Tail**: anything else. It keeps today's rules and code unchanged.
- In the Passing form, path functions are reached through `yields` direct calls, tail or not. The copy `f{H}` threads the state as a variable: an extra parameter on every `jump` target, and a pair or state-only result. A `perform` becomes the clause body inlined in place, whose tail `resume k(v, s)` is a `jump` to a new block holding the rest of the original block. No `$hN` / `$hjJ` functions are made. The `return` clause runs at the handle site, not inside the copies.
- The handlers phase repeats in rounds until no handle changes, so an outer handler can see through the copies its inner handlers made (`f{H=r1}{H=r2}`).

**Tech Stack:** Rust (`crates/eml_core_ir`), insta, `eml_test_support`, the UI and REPL harnesses, `bench/` with `crates/eml_vm/tests/bench.rs`, `bench/run.sh` (macOS).

**Spec:** `docs/superpowers/specs/2026-10-20-s14c-arrays-var-design.md`: 「S14c-2 状態を渡す形の特殊化」, the S14c-2 row of 「作業の分け方と完了の条件」, and 「テストの変更」. The current rules being extended are in `docs/spec/core-ir.md` 「handler への特殊化」, 「handler の節の形」 and 「handler の複製」.

## Global Constraints

- Code comments and docs are Japanese, written with the `yomiyasu:yomiyasu` skill. Comments explain why and cite `docs/` paths. CLAUDE.md stays English.
- Pass order inside `optimize` stays `specialize`, recompute, `handlers`, recompute, `fuse`, recompute, `retain_reachable`. `handlers` emits no `tail`, `box`, `unbox` or RC instruction, and its output passes `verify_translated`.
- Names (decided here, as the spec left them to the plan):
  - A Passing copy has the same name as a Tail copy, `<f>{H=<ret>}`, hashed by `specialize::limited` beyond 64 characters. The `return` clause function fixes the handle, and the handle fixes the form, so one name always means one form and one ABI.
  - A copy of a copy made in a later round nests the suffix: `<f>{H=<r1>}{H=<r2>}`, with `limited` taking the inner copy's name as the base.
  - The Passing form adds no functions besides the copies, and no new IR text syntax.
  - Passing copies are not `internal`.
- Results of a Passing copy of f, as `enum Returned` decided by `fn returned(function: &CoreFn, stateful: bool) -> Returned`:
  - `Plain`, for a stateless handler: f's `ret` and `ret_mark`.
  - `StateOnly`, for a stateful handler when f's `ret` is `Repr::Unit`: the state, with the Repr and mark of the `return` clause's state parameter.
  - `Pair`, otherwise: `obj`, built as `con (,) #0(result, state)`, with mark `f.ret_mark.join(state mark)`.
  - The `(,)` layout is looked up by name in `program.layouts`. If it is missing, it is appended as `Layout { name: "(,)", constructors: [LayoutCtor { name: "(,)", fields: [TObj, TObj] }] }`, the same shape as `translate`'s `tuple_layout(2)`.
- Determinism:
  - Copies are built from the code at the start of their round.
  - The copy tables (`(f, ret) -> copy` and `clause -> <g>{k}`) persist across rounds, so a name is created once. A function that is a path function has no `handle` and is never rewritten later, so a copy made in an earlier round is still valid.
- Test changes:
  - Kind 1: none expected. If one appears, add it to the spec's 「種類1」 with the reason and get the user's confirmation before committing.
  - Kind 2: within the spec's scope, 「すぐに再開する節だけの handler を特殊化するテストのすべて」 (bench `RunStats`, specialization IR snapshots, and assertions of tests whose handler now specializes).
  - Standing ruling from S13e-2: a test whose purpose was EP's handle / perform / resume lowering, and whose program now specializes, gets its handle body passed through data (`hide x = Box x`). Its assertions stay, and the commit message lists it.
  - A changed UI stdout, stderr or outcome is a bug, never a snapshot update.
- Gate per task: the task's test commands, then `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`. Use `git diff --no-ext-diff`. `eml_rt` is untouched, so Verus and Miri are not part of the gate.
- Commit messages end with the attribution lines the controller gives.

## Review Focus

- A `Lin` handler state (a `MutArray`) threaded through a join and through non-tail calls. Checked mode must report no leak and no `LinDup`. Test: Task 1 UI test `state_passing_lin_state.em`.
- A `Panic` raised deep inside a specialized path, caught by a `try_panic` outside the handler, while the state holds a heap list. The `Err` text must be printed and the run must not leak. Test: Task 1 UI test `state_passing_panic.em`.
- A path function that also performs an outer effect which really yields through EP (`Iter.generate`, which stays unspecialized). The state must survive inside the EP continuation. Test: Task 1 UI test `state_passing_under_generator.em`.
- A `perform` in one arm of an `if`, followed by more work after the join. The state must reach the join from both arms. Test: Task 1 UI test `state_passing_join.em` and the IR snapshot `a_conditional_perform_passes_the_state_to_the_join`.
- REPL inputs that reuse an earlier input's `State` loop, under one handler and then under two nested handlers. The debug "same name, same code" check must pass. Test: Task 3 transcript `crates/eml_cli/tests/repl/std/state_passing.repl`.

---

### Task 1: The Passing form

**Files:**
- Create: `crates/eml_core_ir/src/optimize/handlers/passing.rs`, which builds a Passing copy.
- Modify: `crates/eml_core_ir/src/optimize/handlers/mod.rs`:
  - `Shape` gets `form`;
  - `eligible` decides the form and checks the Passing conditions;
  - `path` takes the form;
  - `Copies::build` dispatches on the form;
  - the site rewrite inserts several statements;
  - update the module comment.
- Modify: `crates/eml_core_ir/src/optimize/handlers/piece.rs`: split the block-copying core out of `inline_clause` (below), keeping `inline_clause`'s behaviour.
- Test: `crates/eml_core_ir/tests/optimize.rs`, `crates/eml_vm/tests/bench.rs`, and `crates/eml_core_ir/tests/evidence.rs` (standing ruling).
- Test: new UI tests in `crates/eml_cli/tests/ui/run/effects/`: `state_passing_lin_state.em`, `state_passing_panic.em`, `state_passing_under_generator.em`, `state_passing_join.em`.
- Docs: in `docs/spec/core-ir.md`:
  - 「handler への特殊化」: the two forms, and the Passing path and conditions.
  - 「handler の複製」: the Passing copy, the `Returned` shapes, the `(,)` layout, and the names above.
  - 「handler の特殊化で意味が変わらない理由」: the state is updated only by the inlined clauses, in the order of the `perform`s, as in EP. An outer yield captures the state as an ordinary live variable. An abort drops the state with the frames, as EP's unwinding drops the handler's state.

**Interfaces:**
- Produces (crate-private):
  - `enum Form { Tail, Passing }`, a field of `Shape`.
  - `fn path(program: &Program, translated: &[bool], body: FnIdx, effect: u32, form: Form) -> Option<Vec<FnIdx>>`. `Form::Tail` keeps today's behaviour exactly.
  - `enum Returned { Plain, StateOnly, Pair }` and `fn returned(function: &CoreFn, stateful: bool) -> Returned`, in `passing.rs`.
  - `pub(super) fn build(copies: &mut Copies, index: FnIdx, function: FnIdx, shape: &Shape)`, in `passing.rs`.
  - `fn pair_layout(program: &mut Program) -> LayoutId`, in `passing.rs`, called once by `handlers()` before `Copies` borrows the program, and only when a Passing stateful handler exists.
  - `pub(crate) fn clause_blocks(vars: &mut Vec<VarInfo>, clause: &CoreFn, args: &[Atom], state: Atom, base: u32, resume: impl FnMut(&mut Vec<VarInfo>, Atom, Atom) -> (Vec<Stmt>, Term)) -> Vec<Block>`, in `piece.rs`:
    - it returns the clause's blocks with fresh variables and substituted parameters;
    - block 0 is the entry, whose statements the caller appends to the host block;
    - clause block i > 0 becomes `BlockId(base + i - 1)`;
    - each tail `resume k(v, s)` becomes `resume(...)`;
    - constant `switch`es fold as today.
    `inline_clause` becomes a wrapper over it.

Passing eligibility, in addition to today's conditions other than the tail-only one:
- `path(program, translated, body, effect, Form::Passing)` follows every `Direct` call to a `yields` function, tail or not. It still refuses a `mask`, `Handle`, `Resume`, `Clause` or `yields` `apply` anywhere on the path.
- For every targeted `perform` on the path, and for every tail `resume k(v, s)` of the operation's clause:
  - v's Repr (the clause's var info, or `Atom::fits` for a constant) is `Repr::compatible` with the `perform`'s result variable, and their marks are `Mark::compatible`;
  - in a stateful handler, s is compatible in the same way with the `return` clause's state parameter.
  The resume becomes a `jump`, and `jump`s get no conversion from boxing (`docs/spec/lir.md` 「位置の規則」).

The Passing copy `f{H}`, built forward over f's blocks in order:
- Parameters: f's, then the clause and `return` captures, then the state `s0` when stateful.
- Before emitting, compute where each original block starts in the new block list. Each original block grows by `clause.blocks.len()` per targeted `perform` in it: the clause blocks after the entry, plus the continuation block C. Original targets map to their new start.
- State: the entry starts with `s0`. Every original `jump` target gets a fresh state parameter appended, and every `jump` passes the current state. A `switch` target starts with the state current at its `switch` (R3).
- A targeted `perform` `let r = perform E.op(a…)`:
  - append the clause entry's statements to the current block;
  - emit the other clause blocks;
  - open C with parameters `[r, s']` (stateless: `[r]`);
  - the tail `resume k(v, s)` becomes `jump C(v, s)`;
  - continue the original block's remaining statements and terminator in C, with state `s'`.
- A non-tail `let r = call g(args)` with g a path function becomes a call of `g{H}(args…, captures…, state)`, received by g's `Returned`:
  - `Plain`: as is.
  - `StateOnly`: `let s' = call …`, and every later use of r becomes `()`.
  - `Pair`: `let p = call …; unpack p (,) #0(r, s')`.
- A tail path call (by `tail_position_call`) whose callee's `Returned` equals f's: truncate the block there, `let t = call g{H}(…)`, `return t`. If the shapes differ, treat the call as non-tail and fall through to `return`.
- `return a`, by f's `Returned`:
  - `Plain`: `return a`.
  - `StateOnly`: `return s`.
  - `Pair`: `let p = con (,) #0(a, s); return p`.
- Finish with `blocks::prune`.

At the handle site (`let x = handle …`), apply the rewrites per owner in descending `(block, stmt)` order, so earlier sites' indices stay valid. A Passing site becomes:
- `Plain`: `let r = call body{H}(body captures…, (), captures…)`, then `let x = call ret(ret captures…, r, ())`.
- `StateOnly`: `let s = call body{H}(…, init)`, then `let x = call ret(ret captures…, (), s)`.
- `Pair`: `let p = call body{H}(…, init); unpack p (,) #0(r, s)`, then `let x = call ret(ret captures…, r, s)`.
- Fresh variables take the info of body's `ret` and of the state parameter.

- [ ] **Step 1: Write the failing tests** in `crates/eml_core_ir/tests/optimize.rs`:
  - `a_non_tail_put_is_specialized` (assert): the program of `bench/state_loop.em` (Task 3 creates the file; inline its text here). After `Pass::Optimize` there is no `handle ` and no `perform Std.State`. A copy whose name starts with `bump{H=` exists, and the copy of `loop` calls it non-tail and then tail-calls itself.
  - `a_stateful_handler_threads_its_state` (kind 2: rewrite its snapshot). It must now show `count{H=…}` with no `$h0` / `$h1` functions, a block `b…(t.…: int, st.…: int)` after each inlined clause, and `return st…` (`StateOnly`, since `count` returns `Unit`).
  - `for_each_is_specialized` (kind 2: rewrite its snapshot). One function, no `$h0`.
  - `a_conditional_perform_passes_the_state_to_the_join` (snapshot): `tally` from the Review Focus join test. The join block carries the state parameter, and both arms jump with their own state.
  - `a_pair_result_is_built_at_return` (snapshot): `count : Int -> <State Int, Panic> Int` returning `0` at the end (the shape of `bench/state.em`). Its copy returns `con (,) #0(…)`.
  - `a_non_tail_yielding_frame_keeps_the_handle` (kind 2): keep the `emit2` producer, but consume it with `Iter.to_list (Iter.generate nums)`, a Tail-form handler, so the test still pins the Tail form's refusal.
  - `a_non_tail_yielding_frame_is_specialized_when_clauses_resume` (assert): the old fixture with `Yield.for_each`. No `handle ` is left.
  - `a_resume_value_of_another_repr_keeps_the_handle` (Core IR text, parsed by `eml_core_ir::parse`): an effect `Ask { ask(unit) -> tobj }` handled by a known body and an immediately resuming clause that resumes with a `tobj` value, while the `perform`'s result is `int`. The handle remains after `optimize`.
  - `a_tail_call_across_result_shapes_is_adapted` (Core IR text): under a stateful Passing handler, f returns `unit` and tail-calls a path function g returning `tobj`. After `optimize`, `verify_translated` passes, and f's copy calls g's copy non-tail, unpacks the pair and returns the state.
- [ ] **Step 2: Write the four UI tests** (their snapshots are new):
  - `state_passing_lin_state.em`: an effect `Marks` (`mark : Int -> Unit`, `marked : Int -> Bool`) handled from `MutArray.make n False`:
    - clauses `fn mark i arr -> ((), MutArray.write arr i True)` and `fn marked i arr -> MutArray.read arr i`;
    - `return _ arr -> MutArray.freeze arr`;
    - a sieve up to 50 that calls `marked` in an `if` and a crossing helper non-tail.
    It prints `[2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47]`. If the type checker rejects the `Lin` state, stop and report: S14c-3 depends on it.
  - `state_passing_panic.em`: `try_panic (fn () -> State.exec [] (fn () -> push_until 0))`, where `push_until i` does `put (100 / (3 - i) :: get ())` and recurses. It prints `division by zero`.
  - `state_passing_under_generator.em`: the producer `running n ()` runs `State.exec 0 (fn () -> sums 1 n)`, where `sums` does `put (get () + i)` and then `yield (get ())`, and finally yields the total. It prints `Iter.to_list (Iter.generate (running 4))` as `[1, 3, 6, 10, 10]`.
  - `state_passing_join.em`: `tally xs` adds the even elements with `if x % 2 == 0 then put (get () + x) else ()`, then recurses. It prints `State.exec 0 (fn () -> tally [1, 2, 3, 4, 5, 6])` as `12`.
- [ ] **Step 3: Run** `cargo test -p eml_core_ir --test integration optimize` (the new and rewritten tests fail) and `cargo test -p eml_cli --test integration ui::` (the UI tests may pass already on EP; they guard the change).
- [ ] **Step 4: Implement** `passing.rs`, the `piece.rs` split, and the `mod.rs` changes per the rules above.
- [ ] **Step 5: Run** `cargo test`, then `cargo insta review`:
  - accept only kind-2 changes in `bench.rs` (`state`, `push_fold`, `push_fold_by_hand`, `fold_state` may move) and in the specialization snapshots;
  - apply the standing ruling to EP-purpose tests in `evidence.rs`, `perceus.rs` and `lowering.rs`;
  - every UI stdout, stderr and outcome stays the same;
  - `specialization_matches_hand_written_work` and `_rc` must still pass.
- [ ] **Step 6: Write the `docs/spec/core-ir.md` changes** listed under Files, then run `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`.
- [ ] **Step 7: Commit** "Specialize immediately resuming handlers by passing the state".

### Task 2: Nested handlers, inside out

**Files:**
- Modify: `crates/eml_core_ir/src/optimize/handlers/mod.rs`:
  - `handlers` loops over rounds;
  - the copy tables move into a `Table` struct owned by `handlers` and lent to each round's `Copies`;
  - `Copies` records each created function's source in `table.sources`.
- Modify: `crates/eml_core_ir/src/optimize/mod.rs`: pass `&mut translated`.
- Test: `crates/eml_core_ir/tests/optimize.rs`, and the UI test `crates/eml_cli/tests/ui/run/effects/state_passing_nested.em`.
- Docs: in `docs/spec/core-ir.md` 「handler への特殊化」, the rounds:
  - inner handles go first, because a path still refuses any `handle`;
  - each round builds from its start code;
  - the tables persist across rounds;
  - `yields` is recomputed between rounds;
  - the loop stops when a round changes no handle;
  - it terminates because new sites come only from clause copies, whose nesting depth strictly decreases.

**Interfaces:**
- Consumes: Task 1's `Form`, `passing::build`.
- Produces: `pub(crate) fn handlers(program: &mut Program, translated: &mut Vec<bool>)`. After each round:
  - `translated` is extended so that a created function inherits its source's entry (`f{H}`, `$hN` and `$hjJ` from f; `<g>{k}` from g);
  - `recompute_yields(program, &kept_marks(program))` runs before the next round.

- [ ] **Step 1: Write the failing tests:**
  - `nested_handlers_are_specialized_inside_out` (assert): the program of `bench/state_nested.em` (Task 3; inline its text here). After `Pass::Optimize` there is no `handle `, and a function name contains `}{H=`.
  - `a_nested_handle_keeps_the_handle` (kind 2: rewrite the fixture). The inner handle becomes `let _ = try_panic (fn () -> yield 2)`, which is never specialized, so the outer `Yield.for_each` owner still holds `handle `.
  - `nested_handler_copies_do_not_depend_on_the_rest_of_the_program` (assert): build `state_nested`'s functions alone and next to an unrelated second `State.run` loop. Every name containing `}{H=` must have the same code in both, compared with the existing determinism test's helper.
  - UI `state_passing_nested.em`: the `state_nested` program with 10 iterations. It prints `55 10`.
  - The spec asks for a test with two `Std.State` handlers stacked. Two `State` handlers can both receive operations only through a `mask`, and a `mask` keeps the handle. So the nested tests stack `Std.State` on a user stateful effect (`Tick`), which exercises the same inside-out rounds.
- [ ] **Step 2: Run** `cargo test -p eml_core_ir --test integration optimize` — the new tests fail.
- [ ] **Step 3: Implement** the rounds and the persistent `Table`.
- [ ] **Step 4: Run** `cargo test` and `cargo insta review`, under the same rules as Task 1 (`fold_state` may now lose its outer `State.run` handle: kind 2). Then `cargo clippy --all-targets`, `cargo fmt --check`.
- [ ] **Step 5: Commit** "Specialize nested handlers inside out".

### Task 3: The S14c-2 bar, REPL determinism, and the docs

**Files:**
- Create: `bench/state_loop.em`, `bench/state_loop_by_hand.em`, `bench/state_nested.em`.
- Modify: `crates/eml_vm/tests/bench.rs`:
  - add the three programs to `PROGRAMS` (25) with a snapshot test each;
  - add `("state_loop", "state_loop_by_hand", "5000050000\n")` to `PAIRS` (6), updating the comments that say 「5つの組」;
  - extend `handler_specialization_removes_yields_and_evidence`;
  - accept the new `unwind_points` lines (kind 2).
- Create: `crates/eml_cli/tests/repl/std/state_passing.repl`.
- Docs:
  - `docs/implementation/benchmarks.md` 「S14c-2」: `bench/run.sh` output at this plan's base commit and after, the `RunStats` that moved and why, and `state_loop` against `state_loop_by_hand`.
  - `docs/future/roadmap.md`: the S14c row says S14c-2 is done and where its decisions went; the S14 row's 完了 list adds S14c-2.
  - `CLAUDE.md`: the `optimize` handlers sentence (two forms, non-tail paths for the Passing form, rounds, the `}{H=` names).

The programs:
- `state_loop.em`:
  - `bump i = put (get () + i)` : `Int -> <State Int, Panic> Unit`;
  - `loop i n` does `if i > n then () else` `bump i` then `loop (i + 1) n`;
  - `main` prints `State.exec 0 (fn () -> loop 1 100000)`, giving `5000050000`.
- `state_loop_by_hand.em`, the same loop with the state threaded by hand, mirroring `Std.State`'s wrappers:
  - `bump i st = st + i`;
  - `loop i n st = if i > n then st else loop (i + 1) n (bump i st)`;
  - `run_by_hand : s -> (s -> <e> s) -> <e> (Unit, s)` returns `((), action init)`;
  - `exec_by_hand` takes the second component of `run_by_hand`;
  - `main` prints `exec_by_hand 0 (fn st -> loop 1 100000 st)`.
- `state_nested.em`:
  - `effect Tick where tick : Unit -> Unit`;
  - `step i` does `tick ()` then `put (get () + i)`;
  - `loop` calls `step i` non-tail;
  - `count : (Unit -> <Tick | e> a) -> <Panic | e> (a, Int)` handles `Tick` from `0` with `fn tick () c -> ((), c + 1)` and `return x c -> (x, c)`;
  - `counted () = let (_, ticks) = count (fn () -> loop 1 100000) in ticks`;
  - `main` prints `State.run 0 counted` as `"\{total} \{ticks}"`, giving `5000050000 100000`.

- [ ] **Step 1: Write the failing tests:**
  - In `handler_specialization_removes_yields_and_evidence`, assert `evidence_steps == 0` for `state_loop` and `state_nested`.
  - `specialization_matches_hand_written_work` and `_rc` now cover the `state_loop` pair.
  - The transcript `state_passing.repl`:
    - input 1 is `import Std.State (State(..))`;
    - input 2 defines `bump` and `loop` as in `state_loop.em`;
    - input 3 evaluates `State.exec 0 (fn () -> loop 1 10)` (`55`);
    - input 4 defines `Tick` and `count` as in `state_nested.em`;
    - input 5 evaluates `State.run 0 (fn () -> let (_, t) = count (fn () -> loop 1 4) in t)` (`(4, 10)`).
    Put in input 2's comment that the names and code depend only on the handle and the path functions.
- [ ] **Step 2: Run** `cargo test -p eml_vm --test integration bench::` and `cargo test -p eml_cli --test integration repl::`. If the pair differs:
  - a difference that comes from the specialization is a bug, fixed in `handlers/`;
  - a difference that comes from the hand version not mirroring `State.run` / `State.exec` is fixed in the hand version, and the report says how.
- [ ] **Step 3: Run** `bench/run.sh` at the base commit (in a worktree) and at HEAD. Record both in `benchmarks.md`, then write the docs listed above, applying yomiyasu to every changed Japanese doc.
- [ ] **Step 4: Run** `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`.
- [ ] **Step 5: Commit** "Meet the S14c-2 bar and document state-passing specialization".
