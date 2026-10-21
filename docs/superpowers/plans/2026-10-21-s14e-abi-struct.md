# S14e Type-Directed Calling Convention and Struct Representation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every function and closure gets its ABI from its instantiated type, with `apply` converting at run time where the two sides disagree (S14e-1). Small single-constructor types and tuples then become struct values: one Core IR variable, flattened into leaf variables in LIR and returned as multiple values (S14e-2). The S14e allocation targets are met and recorded, which closes S14.

**Architecture:**
- **S14e-1.** The order keeps the tree green at every commit.
  - First, the IR carries the new facts (a role per function, a site per `apply`), and the VM learns typed `apply` with run-time conversion and conversion frames. Both are inert while boxing still makes every value-used function uniform.
  - Then boxing switches over: role-based uniform positions, typed `apply` sites, tail calls with incompatible results. T3, the internal-uniform rule and `f$boxed` go.
  - Last, translate makes the wrappers per-type instances.
- **S14e-2.**
  - A struct `Repr` and a shape table come first.
  - LIR gains the flatten pass and multi-value statements, then the VM gains multi-value calls and returns. Both are tested with hand-written LIR before any translate change.
  - Finally translate and optimize produce structs.

**Tech Stack:** Rust (`eml_extern`, `eml_core_ir`, `eml_lir`, `eml_vm`; `eml_rt` is not changed), insta, `eml_test_support`, the UI and REPL harnesses, `bench/` with `crates/eml_vm/tests/bench.rs`, `bench/run.sh` and `bench/scripts.sh` (macOS), Miri.

**Spec:** `docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md` is the authority for every rule below. Each task names the spec sections it implements. Rules being replaced are in `docs/spec/lir.md`:
- 「一様な関数」
- 「位置の規則」
- 「box の挿入」
- 「T3」
- 「末尾の位置の呼び出し」
- 「ダミーの値」

## Global Constraints

- Code comments and docs are Japanese, written with the `yomiyasu:yomiyasu` skill. Comments explain why and cite `docs/` paths. CLAUDE.md stays English.
- `eml_rt` is not modified. If a task finds it must be, stop and ask: it brings the `crates/eml_rt/CLAUDE.md` gate.
- Names decided here (the spec left them to the plan):
  - **Roles** (`eml_lir::Role`, re-used by Core IR):
    - `Typed`: no uniform positions.
    - `Body`: handle and `var` scope bodies. The last parameter (`()`) and the result are uniform.
    - `Clause`: operation clause functions that `prompt$` applies. The last two parameters (`k` and the state) and the result are uniform.
    - `Return`: return clauses. The last two parameters (the value and the state) and the result are uniform.
    - `Continuation`: `f$kN`. The last parameter (the resumed value) and the result are uniform. `split.rs` gives `f$kN` the live variables, then the resumed value, and no state parameter.
    - Methods: `Role::uniform_params(self) -> usize` (0, 1, 2, 2, 1) and `Role::uniform_ret(self) -> bool`.
    - Field `role: Role` on `CoreFn` and `LirFn`. `internal: bool` stays and no longer decides ABI.
    - Text: `role body fn`, `role clause fn`, `role return fn`, `role cont fn`. `Typed` writes nothing.
  - **Apply sites:** `pub enum Site { Typed(Vec<Repr>), Uniform }` on Core IR and LIR `Call::Apply { .., site }`.
    - `Typed` holds one `Repr` per argument. A variable argument's entry equals the variable's `Repr` (the verifier checks this). A constant argument's entry is what translate decides from the arrow type.
    - The result `Repr` is the binding's `Repr`. In tail position it is the function's final `ret` (spec 「`apply` の ABI」).
    - Text: a typed site writes `: <repr>` after each constant argument only. The parser fills variable entries from the variables. A uniform site is `uniform apply f(x)`.
  - **Struct reprs:** `eml_extern::StructId(pub u32)` and `Repr::Struct(StructId)`. `Repr` stays `Copy`.
    - `eml_lir::StructShape { layout: LayoutId, fields: Vec<(Repr, Mark)> }` lives in `eml_lir::Structs`, a table unique by content. It is the `structs` field of both `Program`s.
    - Text: `Vec3{float, float, float}`, `(,){int, obj lin}`.
  - **LIR multi-value forms:**
    - `LirFn::rets: Vec<(Repr, Mark)>` replaces `ret` / `ret_mark`, with length ≥ 1. Core IR keeps `ret` / `ret_mark`.
    - `Stmt::LetMany { vars: Vec<VarId>, call: Call }`: `let (a.1: float, b.2: float) = call f(x.0)`.
    - `Term::ReturnMany(Vec<Atom>)`: `return (a.1, b.2)`.
    - `Stmt::UnboxStruct { value: VarId, ctor: Ctor, fields: Vec<Option<VarId>> }`: `let (x.4: float, _) = unbox w.3 Vec3`.
    - `Stmt::Release { .., unboxed: bool }`: `release unboxed w.3 Vec3 #0(..)`.
    - `Pass::Flatten`, file `crates/eml_lir/src/flatten.rs`.
  - **VM:**
    - `SigId(u32)` and `Signature { params: Vec<Repr>, ret: Vec<Repr> }` in a new `crates/eml_vm/src/sig.rs`, interned by content in `CodeBase::signatures`. The `Repr::Struct` ids inside are re-interned in `CodeBase::structs`.
    - `Class { Word, Int, Float, Enum, Struct(u32) }`, where `Struct` carries the skeleton id. `Word` covers `obj`, `tobj` and `unit`.
    - `FnEntry::sig: SigId` replaces `ret_rc`.
    - New ops: `CallN { at }`, `RetN { at }`, `ApplyN { at }`, `Receive { at }`, `UnboxStruct { src, at }`, `ReleaseUnboxed { src, at }`.
    - `RunStats::conversions`.
    - Constant `CONVERSION_FRAME_SIZE` in `op.rs`.
- Test changes:
  - **Kind 1:** exactly the spec's 「種類1」 list. Task 5 deletes the six tests, Task 1 adds the `records_wide` row, Task 11 deletes the `records` row. If another pass/fail change appears, add it to the spec's 「種類1」 with the reason and ask the user before committing.
    - Task 3 deletes or rewrites the four `crates/eml_lir/tests/verify.rs` tests of the removed `non_uniform` rule, because S14e-1 drops the requirement that value-used functions be uniform (`apply` converts at run time). It deletes `a_function_value_passed_to_a_block_is_uniform` and `a_function_value_that_is_dropped_or_switched_on_is_uniform`. It rewrites `a_function_used_as_a_value_is_uniform` as `a_function_value_need_not_be_uniform` and `a_closure_of_a_function_that_is_not_uniform_is_rejected_before_its_arguments` as `a_closure_checks_its_argument_scopes`: their rejections become `Ok(())`, and their other assertions stay.
    - Task 5 changes four more tests that check T3, `f$boxed` and the Repr compatibility of tail calls, which S14e-1 removes. It deletes `crates/eml_core_ir/tests/translate.rs` `a_boxed_wrapper_is_named_after_its_instance`. It rewrites `crates/eml_lir/tests/contract.rs` `a_call_whose_result_is_not_compatible_with_the_ret_is_not_a_tail_call` as `an_incompatible_tail_position_call_becomes_tail` and `crates/eml_lir/tests/verify.rs` `the_result_of_a_tail_call_is_compatible_with_the_function` as `an_incompatible_tail_call_result_is_accepted`: they now assert the new behavior. It drops the incompatible tail `clause` case from `the_boundaries_of_prims_and_clauses_are_compatible`.
    - Task 11 rewrites two tests that turn to failures because their values become structs, so that they check the same thing again (the spec's 「種類1」 handles `records` the same way). `crates/eml_vm/tests/reuse.rs` `the_unwinding_ui_tests_reuse_boxes` asserts `reuses > 0` on `tests/ui/run/unwind/reuse_token_exit.em` and `reuse_token_io_failure.em`, whose `Stats { count, total }` becomes a struct with no box to reuse. Both programs give `Stats` five more unused `Int` fields (7 words), so it stays boxed and the unwinding still meets a live token; their stdout and outcome do not change. `crates/eml_vm/tests/scaling.rs` `an_int_in_a_type_variable_field_is_boxed_and_unboxed` asserts a `box` and an `unbox`, but `Wrap Int` becomes the struct `Wrap{int}`. `Wrap` gets a second constructor `Gone` (and `unwrap` a `panic` arm), so its field stays `tobj` in the layout.
  - **Kind 2:** within the spec's 「種類2」 scope, as each task states. A changed stdout, stderr or outcome of an existing UI test is a bug, never a snapshot update.
- Gate per task:
  - the task's test commands, then `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`;
  - `cargo clippy -p eml_cli --no-default-features --features types -- -D warnings` (and `core`);
  - use `git diff --no-ext-diff`.
  - Tasks 3, 4 and 10 change how the VM reads and writes heap words, so they also run both Miri commands from CLAUDE.md (`eml_rt trusted::` and `eml_vm miri::`, each with and without `-Zmiri-tree-borrows`).
- Commits go on `main`. Messages end with the attribution lines the controller gives. A message that carries a kind-1 or kind-2 change lists it with the reason.

## Review Focus

- **REPL appends across inputs:**
  - A lambda defined in one REPL input is applied in a later input after more signatures were interned. A struct type first seen in a later input gets its conversion code then.
  - Ids must be stable, and the debug `Mismatch` check must pass.
  - Tests: Task 3 transcript `crates/eml_cli/tests/repl/typed_closures.txt` and Task 10 transcript `crates/eml_cli/tests/repl/structs.txt`.
- **Runtime errors and aborts through conversion frames and `ApplyRest` frames holding raw words:**
  - The error names the function that failed, not a conversion.
  - Unwinding never decrefs a raw word, and checked mode reports no leak.
  - Tests: Task 4 `tests/ui/run-fail/panic/panic_through_conversion.em` and `tests/ui/run/unwind/abort_through_conversion.em`.
- **`Float` bits through every new path:** `-0.0` and `NaN` passed through a conversion `box` / `unbox`, a raw closure capture, a struct leaf and a `RetN` print the same as when passed directly. Test: Task 12 `tests/ui/run/float/float_bits_through_structs.em`.
- **`Lin` values through run-time boxes:** a struct with a `File` leaf, passed through a `_` position and back, is closed exactly once with no leak in checked mode, although the run-time box has no `lin` bit. Test: Task 12 `tests/ui/run/linearity/lin_struct_through_uniform.em`.
- **Deep non-tail recursion through mismatched applies:** each level pushes a conversion frame. The depth limit must stop it with the usual error rather than a crash, and `peak_stack` must count the conversion frames. Test: Task 4 `crates/eml_vm/tests/scaling.rs` `non_tail_conversions_count_in_the_stack`.

---

## S14e-1 Type-directed calling convention

### Task 1: Baseline bench programs

These are added before any change, so that S14e-1's "does not increase" criteria compare against pinned numbers.

**Files:**
- Create:
  - `bench/apply_int.em` and `bench/apply_float.em`: choose an `Int -> Int` (resp. `Float -> Float`) function value at run time from `Env.args`, and `apply` it 100000 times in a loop.
  - `bench/ep_float.em`: an operation with a `Float` result and an `Int` state, performed in a loop under a handler that is not specialized (the path contains a `mask`).
  - `bench/records_wide.em`: `records.em` with a 7-`Int` record, so that it never becomes a struct.
- Modify: `crates/eml_vm/tests/bench.rs`: the program list, one snapshot test per new program, and add `("records_wide", …)` to `rebuilding_programs_reuse_boxes`. The `records` row stays until Task 11.

- [ ] **Step 1: Write the programs and their tests.** Each comment says what the program measures and which spec criterion it serves (spec 「足す計測のプログラム」).
- [ ] **Step 2: Run** `cargo test -p eml_vm --test integration bench::`, then accept the new inline snapshots (`cargo insta review`). Check: `records_wide` has `reuses > 0`, and `ep_float` has `evidence_steps > 0` (it is not specialized).
- [ ] **Step 3: Gate, then commit** (`Add the S14e-1 bench programs`).

### Task 2: Roles and apply sites in the IR

This task is data only. Boxing still reads `internal` and treats every `apply` as uniform, so the run behavior is unchanged.

**Files:**
- Modify:
  - `crates/eml_lir/src/lib.rs`: `Role`, `Site`, `Call::Apply::site`, `LirFn::role`.
  - `crates/eml_lir/src/{text.rs,pretty.rs,syntax.rs}`.
  - `crates/eml_core_ir/src/lib.rs`: `CoreFn::role`, `Call::Apply::site`.
  - `crates/eml_core_ir/src/{text.rs,pretty.rs,builder.rs,lowering.rs}`.
- Modify (who assigns roles and sites):
  - `crates/eml_core_ir/src/translate/{mod.rs,expr.rs,program.rs}`: lambdas and local functions are `Typed`; handle and var bodies are `Body`; clause functions are `Clause`; return clauses are `Return`. `apply` gets `Site::Typed` from the arrow type at the site: variables give their own `Repr`, and constants get the type's `Repr`.
  - `crates/eml_core_ir/src/evidence/{split.rs,helpers.rs,handlers.rs}`: `f$kN` is `Continuation`, and `f$jJ` and the helpers are `Typed` (their declared types are already uniform). Every `apply` EP creates is `Site::Uniform`: the helper bodies, the resume made from `Call::Resume`, and the resume inside `cont$`.
  - `crates/eml_core_ir/src/optimize/{fuse/copies.rs,handlers/mod.rs,handlers/piece.rs,handlers/passing.rs}`:
    - copies take the source's role;
    - `f{H}`, `f{H}$hN`, `f{H}$hjJ` and `<g>{k}` are `Typed`;
    - the `apply k` inside `<g>{k}` becomes `Site::Typed`;
    - an `apply` that optimize builds copies the replaced variable's `Repr` into its site.
  - `crates/eml_core_ir/src/verify.rs` and `crates/eml_lir/src/verify.rs`:
    - a typed site has one entry per argument;
    - a variable entry equals the variable's `Repr` at the translated and lowered stages;
    - a role's uniform-parameter count does not exceed the parameter count.
- Test:
  - `crates/eml_core_ir/tests/translate.rs`;
  - `crates/eml_core_ir/tests/evidence.rs`;
  - `crates/eml_core_ir/tests/text.rs`;
  - `crates/eml_lir/tests/text.rs`;
  - `crates/eml_core_ir/tests/verify.rs`.

**Interfaces:**
- Produces: `Role`, `Site`, the `role` field and the `site` field as in Global Constraints. Every later task reads them.

- [ ] **Step 1: Write the failing tests:**
  - `text.rs` (both crates): `roles_and_sites_round_trip`. A function of each role, a typed `apply` with a constant `#1: tobj` and a variable argument, and a `uniform apply` print and parse back to the same program.
  - `translate.rs` `lambda_and_handler_roles` (assert): in a program with a lambda, a stateful handle with one clause and its return clause, the roles are `Typed`, `Body`, `Clause`, `Return`. The lambda's `apply` site lists `[int]` for an `Int` variable and `[tobj]` for a constant `#0` at a type-variable position.
  - `evidence.rs` `ep_applies_are_uniform_sites` (assert): every `apply` inside `prompt$`, `resume$` and `cont$`, and the resume rewritten from `Call::Resume`, is `Site::Uniform`. `f$k0` is `Continuation`.
  - `optimize.rs` `handler_specialization_functions_are_typed` (assert): in the `state_loop` program, `f{H=…}` and `<g>{k}` are `Typed`, and the `apply k` in `<g>{k}` is typed.
  - `verify.rs` `a_site_entry_must_match_its_variable`: the error text is ``the apply site gives `x.0` int but the variable is tobj``.
- [ ] **Step 2: Run them to verify they fail.** Run: `cargo test -p eml_core_ir --test integration` and `cargo test -p eml_lir --test integration text::`. Expected: FAIL to compile, since the fields do not exist yet.
- [ ] **Step 3: Implement** the fields, the text form, the assignment and the verifier rules.
- [ ] **Step 4: Run** `cargo test`.
  - Kind 2 is limited to Core IR and LIR snapshots that show a role keyword, `uniform apply` or a constant's `: repr`.
  - `bench.rs` must not change.
- [ ] **Step 5: Gate, then commit** (`Record each function's EP role and each apply's site`).

### Task 3: VM signatures, typed apply and raw captures

Spec: 「S14e-1」「VM」, bullets 1–7 and 「`apply` の手順」 steps 1–4, apart from result conversion, which is Task 4.

**Files:**
- Create: `crates/eml_vm/src/sig.rs`: `SigId`, `Signature`, `Class`, `fn class(repr: Repr, structs: &Structs) -> Class`, `CodeBase::intern_sig`.
- Modify:
  - `crates/eml_vm/src/code.rs`: `FnEntry::sig`, `CodeBase::signatures`, `Index::signatures`. The debug `Mismatch` check also compares signatures by content.
  - `crates/eml_vm/src/apply.rs`: compare classes; take the fast path, which copies captures straight from the closure and drops the temporary `Vec`; convert mismatched arguments by consuming unbox, or box. `make_closure` lays out scanned words, then raw words, from the target's `FnEntry`.
  - `crates/eml_vm/src/op.rs`: the `Apply` / `TailApply` / `Closure` operands gain the site signature id.
  - `crates/eml_vm/src/compile/emit.rs`: emit the site signature from `Site`, with a uniform site giving all `tobj`.
  - `crates/eml_vm/src/vm.rs`: `ApplyRest` frame layout `[result, n, sig, offset, args…]`, and `unwind` frees only the reference arguments of that frame.
  - Every reader of the `&f` immediate masks off `k << 32`: `apply`, `clause`, the function-number check, `disasm.rs`.
  - `crates/eml_lir/src/check.rs` / `verify.rs`: delete the `non_uniform` rule (value-used functions need not be uniform any more).
  - `crates/eml_vm/src/lib.rs`: `RunStats::conversions`.
  - `crates/eml_vm/src/disasm.rs`: print signatures by content.
- Test:
  - new `crates/eml_vm/tests/conversions.rs` (declared in `tests/main.rs`);
  - `crates/eml_vm/tests/calls.rs`;
  - `crates/eml_vm/tests/miri.rs`;
  - `crates/eml_vm/tests/codegen.rs`;
  - REPL transcript `crates/eml_cli/tests/repl/typed_closures.txt`.

**Interfaces:**
- Consumes: `Site` (Task 2).
- Produces (crate-private to `eml_vm`):
  - `fn class(repr: Repr, structs: &Structs) -> Class`;
  - `CodeBase::intern_sig(&mut self, sig: Signature) -> SigId`;
  - `FnEntry { name, start, end, arity, frame_size, sig }`, where `arity` counts source arguments;
  - closure first word `Word::imm(f | (k << 32))`;
  - `RunCounts::conversions`.

- [ ] **Step 1: Write the failing tests** in `conversions.rs`. These are hand-written LIR programs whose functions are not uniform, so they are run through `run_lir` with the verifier rule deleted:
  - `a_typed_apply_takes_the_fast_path`: `&inc` with `(x: int) -> int` applied at a typed site `(int) -> int`. Output `11`; `conversions == 0`, `boxes == 0`.
  - `a_uniform_site_boxes_into_a_typed_function`: a `uniform apply &inc(b)` with a boxed `10`. Output `11`; `conversions == 1`, and `unboxes` counts the argument.
  - `a_closure_keeps_a_float_capture_raw`: `closure scale(2.5)` applied to `4.0`. Output `10.0`; `boxes == 0`.
  - `a_partial_application_by_the_vm_lays_out_raw_words`: `apply &add3(1)`, then the closure applied to `2`, then to `3`. Output `6`; `boxes == 0`.
  - `apply_rest_keeps_raw_words`: `make` returns a closure, and one `apply` passes `int` arguments to both stages. Then the same thing inside a `try_panic` with a `panic` in the second stage. No leak, no fault.
  - `the_capture_count_is_packed_with_the_function` (checked mode): a closure whose immediate claims the wrong count fails with `Internal`.
  - `a_mismatch_between_different_types_is_internal` (checked mode): an `int` site applying a `float` function fails with `Internal`, since a mismatch is only ever between `Repr`s of one type.
  - `miri.rs`: `typed_apply_with_raw_captures`, the same as `a_closure_keeps_a_float_capture_raw`.
  - REPL `typed_closures.txt`: input 1 defines `scale : Float -> Float -> Float`, and input 2 maps `scale 2.0` over a list and prints. The transcript's output is fixed.
- [ ] **Step 2: Run them to verify they fail.** Run: `cargo test -p eml_vm --test integration conversions::`. Expected: FAIL (the verifier rejects non-uniform values, or there is no `conversions` field).
- [ ] **Step 3: Implement.**
  - Comparison is per argument by `Class`. The fast path must not add instructions over today's `apply` in `bench/run.sh`'s `apply_int` row; packing classes into a word is allowed (spec 「作る人に任せること」).
  - Count `conversions` once per mismatched `apply`. A conversion's `box` / `unbox` also counts in `boxes` / `unboxes`.
- [ ] **Step 4: Run** `cargo test`, then `bench/run.sh` and compare its `apply_int` row with the last recorded one. `bench.rs` snapshots must not change, except `conversions: 0` appearing (kind 2: the new field).
- [ ] **Step 5: Gate (with Miri), then commit** (`Give FnEntry a signature and let apply convert mismatched arguments`).

### Task 4: Conversion frames and their composition

Spec: 「VM」, the bullets from 変換のフレーム through 安全点.

**Files:**
- Modify:
  - `crates/eml_vm/src/op.rs`: `CONVERSION_FRAME_SIZE`. The scalar conversion sequences (box and unbox for `int`, `float`, `enum`) are built-ins after `APPLY_REST`, and `BUILTINS` grows.
  - `crates/eml_vm/src/code.rs`:
    - `CodeBase::conversion(from: Class, to: Class) -> Option<Pc>`;
    - `CodeBase::is_conversion(pc: Pc) -> bool`;
    - `function_at` checks `pc < entry.end`, so that conversion code appended later (Task 10) is never taken for a function.
  - `crates/eml_vm/src/apply.rs`: on a result mismatch, push a conversion frame, then enter.
  - `crates/eml_vm/src/vm.rs`:
    - composition in `TailCall`, `TailSelf`, `TailApply`, `TailClause` and the second stage of `ApplyRest`: replace c1 by c1∘c2, pop when the composition is the identity, push c2 when the caller frame is not a conversion;
    - `unwind` treats a conversion frame as owning nothing, counts it in `unwound_frames`, and lands through it;
    - the yielding check: a conversion frame writes the target `Repr`'s dummy without boxing;
    - `last_ret` / `function_at` skip conversion pcs.
  - `crates/eml_vm/src/compile/emit.rs`: a direct tail call whose callee `ret` differs from the caller's `ret` gets a static conversion operand (`NONE` when none).
  - `crates/eml_vm/src/disasm.rs`.
- Test:
  - `crates/eml_vm/tests/conversions.rs`;
  - `crates/eml_vm/tests/scaling.rs`;
  - `crates/eml_vm/tests/miri.rs`;
  - UI `tests/ui/run-fail/panic/panic_through_conversion.em`;
  - UI `tests/ui/run/unwind/abort_through_conversion.em`.

**Interfaces:**
- Consumes: `Class`, `SigId` (Task 3).
- Produces:
  - `CodeBase::conversion(from: Class, to: Class) -> Option<Pc>`;
  - `CodeBase::is_conversion(pc) -> bool`.
  - The composition rule: compose((X→Y), (W→X)) is (W→Y), or the identity when W and Y are the same class.

- [ ] **Step 1: Write the failing tests:**
  - `conversions.rs`:
    - `a_result_mismatch_pushes_one_conversion_frame`: a typed site `-> int` applies a `tobj`-returning function. Output is right; `conversions == 1`.
    - `tail_applies_compose_conversions`: a loop of `n` tail `apply`s alternating between an `int`-returning and a `tobj`-returning function value, called once from a typed site. `peak_stack` is the same at `n = 1000` and `n = 2000`.
    - `a_composed_identity_pops_the_frame`: `f` returns `tobj` and is entered from an `int` site; it tail-applies an `int`-returning `g`. `unwound_frames` stays 0, and the disassembly shows no conversion in the tail path.
    - `yielding_through_a_conversion_frame_writes_a_dummy`: a `uniform` site enters a typed function that yields. `boxes` does not grow by the yield, and the program completes.
    - `an_abort_unwinds_a_conversion_frame`: a `never` clause aborts from below a conversion frame. `unwound_frames` counts the conversion frame, and there is no leak.
  - `scaling.rs`:
    - `incompatible_tail_calls_keep_the_stack_flat`: a function ending in `State Int`'s `get` (direct tail), and a ring of function values. Both run at n = 1000 and 2000 with equal `peak_stack`.
    - `an_unspecialized_clause_resuming_in_tail_position_keeps_the_stack_flat`.
    - `non_tail_conversions_count_in_the_stack` (Review Focus): non-tail recursion through a mismatched `apply` grows `peak_stack` linearly. With a small depth limit it stops with the depth error.
  - UI `panic_through_conversion.em`: a `1 / 0` inside a function reached through a conversion frame. The expected stderr names that function.
  - UI `abort_through_conversion.em`: `try_panic` catches a panic raised under a conversion frame and an `ApplyRest` frame with raw `Int` and `Float` arguments, then prints.
  - `miri.rs`: `a_conversion_frame_round_trip`.
- [ ] **Step 2: Run them to verify they fail.** Run: `cargo test -p eml_vm --test integration conversions:: scaling::` and `cargo test -p eml_cli --test integration ui::`.
- [ ] **Step 3: Implement.**
  - The receiving slot sits in a `Receive` op just before each conversion sequence (spec: the instruction just before), so Task 10's `RetN` writes through it unchanged.
  - Size `CONVERSION_FRAME_SIZE` from the longest generated sequence, and assert it when sequences are generated.
  - `conversions` counts only `apply` mismatches (Task 3). The static conversions of direct tail calls and `clause` are by design and are not counted. A `never` tail `clause` gets no conversion and no composition.
- [ ] **Step 4: Run** `cargo test`. `bench.rs` snapshots must not change: no program reaches a mismatch yet.
- [ ] **Step 5: Gate (with Miri), then commit** (`Convert apply results in conversion frames and compose them on tail calls`).

### Task 5: Boxing by type and role

Spec: 「関数の ABI」, 「`apply` の ABI」, 「box の挿入」. This is the switch-over.

**Files:**
- Modify:
  - `crates/eml_lir/src/boxing.rs`:
    - delete the internal-uniform loop, `raise_tail_returns`, `uniformize`, `boxed` and `values`;
    - `conform_role(function)` makes the role's uniform positions `uniform_repr`;
    - `conform_clause` is kept;
    - `Converter::call` converts `apply` arguments to the site's entries and rebinds the result from the binding's `Repr` (a uniform site gives `tobj` everywhere);
    - the binding of a tail-position call (`tail_position_call`) and its `return` get no conversion, and the binding takes the callee's result `Repr`.
  - `crates/eml_lir/src/contract.rs`: `tail_call` makes every tail-position call `tail` regardless of `Repr` compatibility. Marks are still compared.
  - `crates/eml_lir/src/verify.rs`: the range stage accepts an incompatible `return` only in the tail-call shape.
  - `crates/eml_vm/src/compile/emit.rs`: a tail-position `apply`'s result `Repr` is the function's final `ret`.
- Test:
  - `crates/eml_lir/tests/boxing.rs`;
  - `crates/eml_core_ir/tests/boxing.rs`;
  - `crates/eml_lir/tests/contract.rs`;
  - `crates/eml_lir/tests/verify.rs`;
  - `crates/eml_vm/tests/bench.rs`;
  - new UI tests in `tests/ui/run/functions/`.
- Kind 1: delete the six tests of the spec's 「種類1」, first part.
- Kind 2:
  - Core IR and LIR snapshots with function values, lambdas, captures, `f$boxed` or T3 `ret`s;
  - `crates/eml_lir/tests/boxing.rs` apply tests (e.g. `an_apply_in_ir_text_boxes_its_argument_and_rebinds_its_result`);
  - VM disassembly;
  - `bench.rs` `RunStats`.

**Interfaces:**
- Consumes: `Role::uniform_params`, `Role::uniform_ret`, `Site`, and Tasks 3–4's VM.
- Produces: after boxing, a function's params and `ret` are its typed `Repr`s, except the role's uniform positions. Nothing named `$boxed` exists.

- [ ] **Step 1: Write the failing tests:**
  - `crates/eml_lir/tests/boxing.rs` (hand-written LIR):
    - `a_direct_call_to_a_lambda_converts_nothing` (no `box` / `unbox` around `call $lambda0(x: float)`);
    - `a_uniform_site_converts_its_arguments_and_result`;
    - `a_role_boxes_only_its_uniform_positions`, using a `role clause fn` with captures `(n: int)`, an op argument `int`, `k`, and the state;
    - `closure_arguments_convert_to_the_callee_params` (`closure depth@[_](5)` boxes `5`);
    - `an_incompatible_tail_call_becomes_tail` (after `Pass::Contract`, `tail call g(..)`, where `g` returns `tobj` into an `int` function).
  - `crates/eml_core_ir/tests/boxing.rs`: `no_boxed_wrapper_is_made` (a top-level function used only as a value keeps its name, and no `$boxed` exists).
  - `crates/eml_lir/tests/verify.rs`: `an_incompatible_return_is_accepted_only_after_a_tail_position_call`. The other shape is rejected with the existing message.
  - UI, `tests/ui/run/functions/`:
    - `polymorphic_recursion_with_function_values.em`: a function value passed through a `_` polymorphic recursion;
    - `function_value_made_inside_a_uniform_instance.em`: a function value made inside `_` and called outside;
    - `function_values_inside_data_called_generically.em`: function values stored in data and called inside `_`;
    - `float_closure_partial_application.em`: a closure capturing a `Float`, partially applied, with extra arguments.
  - UI trade-off (spec 「引き換えにすること」): `tests/ui/run/functions/unspecialized_map_passes_floats.em` with its `RunStats` pinned in `crates/eml_vm/tests/bench.rs` `trade_offs`. This needs a `stats`-style helper for a UI path, if `bench.rs` has none.
- [ ] **Step 2: Run them to verify they fail.** Run: `cargo test -p eml_lir --test integration` and `cargo test -p eml_core_ir --test integration boxing::`.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test`, then `cargo insta review` within the kind-2 scope.
  - Compare every `bench.rs` row with Task 1's numbers and the pre-task snapshots.
  - `conversions`, and any increase in `allocations`, `boxes` or `unboxes`, are allowed only where a wrapper is still scheme-typed (`op$`, `con$`, `$externN`, `cont$`). List those rows with the cause in the commit message. Task 6 removes them, and the hard criterion is checked there.
  - An increase with any other cause is a bug to fix in this task.
- [ ] **Step 5: Gate, then commit** (`Decide every ABI by type and role; drop T3 and f$boxed`). The message lists the kind-1 deletions and the kind-2 scope.

### Task 6: Per-type wrapper instances and S14e-1 completion

Spec: 「関数の ABI」 bullets 2–3, 「完了の条件」 S14e-1.

**Files:**
- Modify:
  - `crates/eml_core_ir/src/translate/program.rs`: `op$`, `con$`, `cont$` and `cont$state` become instances named with type arguments (`con$Some@[Int]`), typed from the instantiated type.
  - `crates/eml_core_ir/src/translate/expr.rs`: `$externN`, same rule.
  - `crates/eml_core_ir/src/translate/instances.rs`: naming, through the same instance-name function as other instances.
  - `crates/eml_core_ir/src/optimize/known.rs` and `optimize/handlers/{mod.rs,piece.rs}`: recognize the wrappers by base name, not by exact name.
  - `docs/implementation/benchmarks.md`: section 「S14e-1」.
- Test:
  - `crates/eml_core_ir/tests/translate.rs`;
  - `crates/eml_core_ir/tests/instances.rs`;
  - `crates/eml_vm/tests/bench.rs`.
- Kind 2: snapshots showing wrapper names and `bench.rs` `RunStats`.

- [ ] **Step 1: Write the failing tests:**
  - `instances.rs` `wrappers_are_per_type_instances`: `Some` used as a function value at `Int` and at `String`. The program has `con$Some@[Int]` with param `int` and `con$Some@[String]` with param `obj`.
  - Same pattern for an operation used as a value (`op$…@[Float]`), an extern used as a value (`$extern0`), and `k` used as a value (`cont$@[…]`).
  - `bench.rs` `typed_benches_do_not_convert`: for every `bench/` program except `ep_float`, `conversions == 0`. `ep_float` keeps its pinned count.
- [ ] **Step 2: Run them to verify they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Verify S14e-1's completion criteria:**
  - **No increase:** against the pre-S14e snapshots (git `9e4fe41f`), `allocations`, `boxes` and `unboxes` do not increase for any `bench/` program. A row that increases is a bug to fix, not a snapshot to accept.
  - **Conversions:** `conversions` is 0 except `ep_float`.
  - **New programs:** in `apply_int` and `apply_float`, `boxes` and `unboxes` drop to the loop-independent constant.
  - **Stack:** Task 4's scaling tests still pass.
  - Run `bench/run.sh`.
- [ ] **Step 5: Record** 「S14e-1」 in `docs/implementation/benchmarks.md`: the `RunStats` changes per program, `bench/run.sh`'s counts, and `ep_float`'s conversion count with the reason (spec 「完了の条件」).
- [ ] **Step 6: Gate, then commit** (`Make wrappers per-type instances and record S14e-1`).

## S14e-2 Struct representation

### Task 7: The struct Repr and the shape table

**Files:**
- Modify:
  - `crates/eml_extern/src/lib.rs`:
    - `StructId` and `Repr::Struct`;
    - delete `BOXED_SCALARS`, so that `needs_box()` is the predicate, true for `int`, `float`, `enum` and structs;
    - `is_rc()` is false for structs;
    - `compatible()` treats two structs as compatible only when the ids are equal, and its doc comment says so;
    - `name()` returns `"struct"`.
  - `crates/eml_lir/src/lib.rs`:
    - `StructShape`, `Structs` and `Program::structs`;
    - `Atom::fits_compatible` takes `&Structs`.
  - `crates/eml_core_ir/src/lib.rs`: `Program::structs`, and `lowering.rs` copies it.
  - The text form and pretty printer of both IRs.
  - `crates/eml_lir/src/check.rs`, `verify.rs`, `crates/eml_core_ir/src/verify.rs`:
    - every `Repr` comparison that can see structs calls `Structs::compatible`;
    - the stage rules (spec 「verifier」 under 「LIR」 and 「Core IR」) apply, except the ones about flatten, which are Task 8.
  - Every exhaustive `match` on `Repr` in `eml_vm`: structs are `unreachable!` until Task 10, because Task 8 removes them before codegen.
- Test:
  - `crates/eml_lir/tests/text.rs`;
  - `crates/eml_lir/tests/verify.rs`;
  - `crates/eml_core_ir/tests/verify.rs`;
  - a unit test module in `crates/eml_lir/src/lib.rs` for `Structs`.

**Interfaces:**
- Produces:
  - `Structs::intern(&mut self, shape: StructShape) -> StructId`;
  - `Structs::shape(&self, id) -> &StructShape`;
  - `Structs::leaves(&self, id) -> Vec<(Repr, Mark)>`: depth-first, `unit` leaves included and marked by position;
  - `Structs::words(&self, id) -> usize`: `unit` counts 0;
  - `Structs::compatible(&self, a: Repr, b: Repr) -> bool`: same skeleton, marks ignored;
  - `Structs::has_non_unr_rc_leaf(&self, id) -> bool` (some RC leaf whose mark is not `unr`; the 「RC の対象」 drop rules use it).

- [ ] **Step 1: Write the failing tests:**
  - `structs_intern_by_content`: two equal shapes get one id.
  - `compatibility_ignores_marks`: `(,){int, obj lin}` and `(,){int, obj poly}` are compatible; `(,){int, obj}` and `(,){float, obj}` are not.
  - `nested_leaves_are_depth_first`.
  - `struct_text_round_trips`: Core IR and LIR text with `Vec3{float, float, float}` variables.
  - verify: `a_struct_is_never_a_switch_scrutinee`, `a_struct_con_is_checked_against_its_shape` (Core IR translated stage).
- [ ] **Step 2: Run them to verify they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test`. Nothing produces structs yet, so all snapshots are unchanged.
- [ ] **Step 5: Gate, then commit** (`Add the struct Repr and the shape table`).

### Task 8: Boxing structs and the flatten pass

Spec: 「箱に入れる位置」, 「LIR」 (box の挿入, 開くパス, verifier).

**Files:**
- Create: `crates/eml_lir/src/flatten.rs`.
- Modify:
  - `crates/eml_lir/src/lib.rs`: `LirFn::rets`, `Stmt::LetMany`, `Term::ReturnMany`, `Stmt::UnboxStruct`, `Release::unboxed`, plus `defs` / `for_each_atom` / `for_each_consumed` for them. The `ret` → `rets` rename is mechanical (kind 3) in `eml_lir`, `eml_core_ir/src/lowering.rs` and `eml_vm`.
  - `crates/eml_lir/src/boxing.rs`:
    - structs take the scalar paths with `box s : obj`;
    - `unbox w` accepts `obj` and `tobj`;
    - fresh variables copy the original mark for structs;
    - the peephole applies to struct boxes;
    - `impossible` lists struct ↔ other struct and struct ↔ scalar;
    - a `clause` entry's struct parameters are taken back at the entry.
  - `crates/eml_lir/src/pipeline.rs`: `Pass::Flatten` between `Boxing` and `Contract`.
  - `crates/eml_lir/src/verify.rs`: the after-flatten stage rejects `struct` anywhere, and multi-value `let` / `return` and `UnboxStruct` are allowed only there.
  - `crates/eml_lir/src/text.rs`, `pretty.rs`, `syntax.rs`.
- Test: new `crates/eml_lir/tests/flatten.rs` (declared in `tests/main.rs`), `crates/eml_lir/tests/boxing.rs`, `crates/eml_lir/tests/verify.rs`.

**Interfaces:**
- Consumes: `Structs` (Task 7).
- Produces:
  - `pub fn flatten(program: &mut Program)`;
  - leaf variables named `<name>.<i>` with leaf `Repr` and mark `min(shape leaf mark, variable mark)`;
  - after flatten, `rets.len() > 1` exactly for struct-returning functions, which return through `Term::ReturnMany`.

- [ ] **Step 1: Write the failing tests** in `flatten.rs`, hand-written struct LIR through `passes_until(.., Pass::Flatten)`, as pretty snapshots:
  - `con_and_unpack_of_a_struct_vanish`;
  - `box_builds_the_layout_con` (`Ball`-like layout with a `tobj` field holding a `Vec3`, so the nested struct is boxed recursively, and `(Int, Float)` boxes each leaf);
  - `a_leaf_from_an_unbox_is_not_reboxed` (swap the elements of a boxed `(Float, Float)` and box again: no `box` of a `float`);
  - `unbox_becomes_a_multi_def_with_unused_leaves_skipped`;
  - `unit_leaves_make_no_variables`;
  - `calls_applies_and_returns_carry_leaves`;
  - `a_call_that_never_returns_binds_a_struct_to_a_dummy_obj`;
  - `a_struct_drop_drops_each_rc_leaf`;
  - `a_lin_box_without_rc_leaves_is_dropped` (spec: the `drop w` rule).
- `boxing.rs`:
  - `a_struct_passed_to_a_uniform_position_is_boxed`;
  - `a_struct_parameter_in_a_role_position_is_unboxed_at_entry`.
- `verify.rs`:
  - `a_struct_after_flatten_is_rejected`;
  - `a_return_list_of_the_wrong_length_is_rejected`.
- [ ] **Step 2: Run them to verify they fail.** Run: `cargo test -p eml_lir --test integration flatten:: boxing:: verify::`.
- [ ] **Step 3: Implement** `flatten` as loops over blocks and statements. The only recursion is over shape depth.
- [ ] **Step 4: Run** `cargo test`. Only the mechanical `rets` rename touches other crates. Expected values are byte-identical.
- [ ] **Step 5: Gate, then commit** (`Box structs at uniform positions and flatten them into leaves`).

### Task 9: Multi-value statements in the later LIR passes

Spec: 「後のパス」, 「ダミーの約束を広げる」 (the LIR side).

**Files:**
- Modify:
  - `crates/eml_lir/src/liveness.rs`;
  - `contract.rs`: `LetMany` + `ReturnMany` in the same order becomes `tail`; `UnboxStruct` counts as a pure `let`, so that it is removed when unused;
  - `perceus.rs`: `UnboxStruct` borrows like `unpack`, the constructor is known, and releasing `w` emits `release unboxed w L #0(..)`;
  - `reuse.rs`: a `release unboxed` is a reuse source;
  - `unwind.rs`: `owned` excludes every result leaf;
  - `verify.rs`: the V1/V2 mark checks per leaf.
- Test: `crates/eml_lir/tests/{contract.rs,perceus.rs,reuse.rs,unwind.rs}`, `crates/eml_lir/tests/flatten.rs`.

- [ ] **Step 1: Write the failing tests** (hand-written struct LIR through `passes_until`):
  - `contract.rs`:
    - `a_multi_value_call_returned_in_order_becomes_tail`;
    - `a_reordered_return_is_not_tail`;
    - `an_unused_struct_unbox_is_removed_and_the_call_becomes_tail`.
  - `perceus.rs` `an_unboxed_box_is_released_with_its_named_leaves`.
  - `reuse.rs` `swapping_tuple_elements_still_reuses_the_box` (the `release unboxed` + `con … reuse` pair survives).
  - `unwind.rs` `leaves_of_a_multi_value_call_are_not_owned`.
- [ ] **Step 2: Run them to verify they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test`. No expected value outside the new tests changes.
- [ ] **Step 5: Gate, then commit** (`Teach the later LIR passes multi-value statements`).

### Task 10: VM multi-value calls and returns, struct conversions

Spec: 「S14e-2」「VM」, 「ダミーの約束を広げる」 (the VM side).

**Files:**
- Modify:
  - `crates/eml_vm/src/op.rs`: `CallN`, `RetN`, `ApplyN`, `UnboxStruct`, `ReleaseUnboxed`.
  - `crates/eml_vm/src/compile/{emit.rs,slots.rs,mod.rs}`: emit them; slot interference covers multi-defs; `load` rejects an entry with `rets.len() > 1`.
  - `crates/eml_vm/src/vm.rs`: `RetN` reads the destination slots from the op before the return pc (`CallN`, `ApplyN` or `Receive`). In checked mode, any other op or a count mismatch is `Internal`.
  - `crates/eml_vm/src/data.rs`:
    - `UnboxStruct` writes leaf dummies on a dummy;
    - `ReleaseUnboxed` is a no-op on a dummy and gives an empty token for the token form;
    - fast mode detects the dummy by kind (`BigInt` against `Data`), and checked mode also compares with `ctx.heap().dummy()` and rejects any other `BigInt`.
  - `crates/eml_vm/src/sig.rs` / `code.rs`:
    - re-intern struct shapes by content into `CodeBase::structs`;
    - for each new shape, append box and unbox conversion sequences, which follow the layout's physical shape (`LayoutCtor::shape`);
    - `Class::Struct` compares by skeleton.
  - `crates/eml_vm/src/apply.rs`: a multi-value result conversion and struct argument conversion.
  - `crates/eml_vm/src/disasm.rs`.
- Test:
  - new `crates/eml_vm/tests/structs.rs`;
  - `crates/eml_vm/tests/miri.rs`;
  - REPL transcript `crates/eml_cli/tests/repl/structs.txt`.

**Interfaces:**
- Consumes: Task 8's LIR forms and Task 4's conversion machinery.
- Produces: `CallN { at }` with operands `[f, nargs, args…, nrets, dsts…]`, and `RetN { at }` with `[n, srcs…]`.

- [ ] **Step 1: Write the failing tests** in `structs.rs` (hand-written struct LIR run through all passes):
  - `a_struct_comes_back_in_registers`: `add : Vec3 -> Vec3 -> Vec3`. Output is right; `allocations == 0`.
  - `a_tail_call_returns_multiple_values_through_the_callers_frame`.
  - `apply_rest_returns_multiple_values_to_the_original_apply`.
  - `a_struct_result_crosses_a_conversion_frame`: a uniform site applies a struct-returning function, and the result is boxed once.
  - `a_struct_unbox_on_a_dummy_gives_leaf_dummies`: yield through a struct-returning call under an unspecialized handler.
  - `release_unboxed_on_a_dummy_does_nothing`: checked mode; another `BigInt` there is `Internal`.
  - `a_multi_value_entry_is_rejected_at_load`.
  - `miri.rs` `multi_value_returns`.
  - REPL `structs.txt`: input 1 declares `data P = P Float Float` and `swap`, and input 2 maps `swap` over a list of `P`. The output is fixed.
- [ ] **Step 2: Run them to verify they fail.**
- [ ] **Step 3: Implement.** Single-value `Call` / `Ret` must keep their speed: `bench/run.sh`'s rows are unchanged within noise.
- [ ] **Step 4: Run** `cargo test` (no expected value changes yet) and `bench/run.sh`.
- [ ] **Step 5: Gate (with Miri), then commit** (`Add multi-value calls and returns and struct conversions to the VM`).

### Task 11: Translate produces structs

Spec: 「構造体にする型」, 「Kind の印と RC」, 「Core IR」 (translate, EP, verifier).

**Files:**
- Modify:
  - `crates/eml_core_ir/src/translate/types.rs`:
    - `repr` decides struct types: single-constructor `data` outside a recursive group, or a non-empty tuple; ≤ 6 non-`unit` leaf words, with at least one; no `_` leaf; scheme variables count as `tobj` leaves; `Lin` fields and row arguments are allowed;
    - `mark` stops using `scalar_or` for structs;
    - the shape is interned in `Program::structs`.
  - `crates/eml_core_ir/src/translate/program.rs`: layout fields of a struct type read as `tobj`.
  - `crates/eml_core_ir/src/translate/{expr.rs,pattern.rs}`:
    - a struct `con` binds a struct variable, with its arguments checked against the shape;
    - destructuring is always `unpack`;
    - the `with_arg` tuples become structs;
    - `entry$<name>` returns `obj` for a struct.
  - The recursive-group test needs the HIR declaration graph. Compute the SCCs of type declarations once per program in `translate/program.rs`.
  - `crates/eml_core_ir/src/evidence/{mod.rs,split.rs}`: a struct `ret` yields `prim dummy()` bound to a struct variable with mark `poly`. The "RC target" drops include structs with non-`unr` RC leaves.
  - `crates/eml_core_ir/src/verify.rs`.
- Test: `crates/eml_core_ir/tests/translate.rs`, `crates/eml_core_ir/tests/verify.rs`, `crates/eml_vm/tests/bench.rs`.
- Kind 1: delete the `("records", …)` row of `rebuilding_programs_reuse_boxes` (the spec's 「種類1」 second part).
- Kind 2:
  - Core IR and LIR snapshots with tuples or struct types;
  - VM disassembly;
  - `bench.rs` `RunStats`.

- [ ] **Step 1: Write the failing tests** in `translate.rs` (assert on the `Repr` of a binding):
  - `a_small_record_is_a_struct`;
  - `a_recursive_group_is_not` (`Iter`);
  - `seven_words_are_not`;
  - `a_uniform_leaf_is_not`;
  - `a_lin_field_is_allowed`;
  - `a_row_argument_is_allowed`;
  - `only_unit_leaves_is_not`;
  - `a_scheme_variable_is_a_tobj_leaf`;
  - `a_struct_typed_field_is_tobj_in_its_layout` (`Ball` is `(tobj, float)`);
  - `a_struct_takes_its_mark_from_its_kind` (a `(Int, MutArray Int)` variable is `lin`; its shape is `(,){int, obj lin}`).
- [ ] **Step 2: Run them to verify they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test`, then `cargo insta review` within the kind-2 scope. No UI test output changes. In `bench.rs`, `allocations` must not increase for any program.
- [ ] **Step 5: Gate, then commit** (`Represent small single-constructor types and tuples as structs`).

### Task 12: Optimize with structs, UI coverage and S14e-2 completion

Spec: 「Core IR」 (optimize), 「完了の条件」 S14e-2, 「足すテスト」 S14e-2 (UI).

**Files:**
- Modify:
  - `crates/eml_core_ir/src/optimize/fuse/copies.rs`: value-shape copies pass struct `con` fields with the shape `Repr`.
  - `crates/eml_core_ir/src/optimize/handlers/passing.rs`: `Returned::info` builds a tuple struct shape from the values' `Repr`s and marks when it fits in 6 words, and keeps `obj` otherwise. A struct `var` state stays a struct.
  - `crates/eml_vm/tests/bench.rs`:
    - `RESIDUAL` becomes `[("sieve_var", 0), ("fannkuch_var", 1)]` (kind 2);
    - add `nbody_allocates_25_per_step`, running nbody with arguments 1, 1000 and 2000 (add an args-taking `stats` helper);
    - add `mandelbrot_does_not_allocate_complex`: `reuses == 0`, and `allocations` independent of the grid point count apart from the constant;
    - add `spectral_norm_allocates_14_fewer`, against the S14e-1 snapshot.
  - `docs/implementation/benchmarks.md`: 「S14e-2」, and the S14d nbody breakdown text corrected (27 = 10 `Vec3` + 17 `(MutArray, MutArray)` tuples).
- Create (UI, `tests/ui/run/`):
  - **`records/`:**
    - `structs_in_uniform_positions.em` (`List (Int, Float)`, `Array Vec3`, `Option Vec3`);
    - `nested_structs.em`;
    - `struct_typed_field.em`;
    - `swap_map_specialized.em`, with `RunStats` pinned in `bench.rs` `trade_offs`, so that `allocations` and `reuses` equal the pre-S14e-2 numbers;
    - `swap_map_unspecialized.em`, a trade-off pinned the same way;
    - `three_rc_leaves_through_a_call.em`, with `rc_increments` pinned.
  - **`functions/struct_capture.em`.**
  - **`effects/struct_yield_dummy.em`:** an unspecialized handler, yielding while a struct-returning call is pending.
  - **`linearity/`:**
    - `lin_struct.em` (a `Lin` field);
    - `lin_without_rc_leaves.em`;
    - `lin_struct_through_uniform.em` (Review Focus).
  - **`panic/struct_return_panics.em`:** `panic` and `fail` inside a struct-returning function.
  - **`float/float_bits_through_structs.em`** (Review Focus).
- Test: the files above, plus `crates/eml_core_ir/tests/optimize.rs` `passing_returns_a_tuple_struct` (snapshot) and `a_struct_var_state_stays_a_struct`.

- [ ] **Step 1: Write the failing tests** above. Use the spec's exact values:
  - **nbody:** the per-step allocation difference is 25 (steps 1, 1000, 2000);
  - **mandelbrot:** `reuses == 0`;
  - **`sieve_var`:** allocations equal `sieve_by_hand`'s;
  - **`fannkuch_var`:** allocations are `fannkuch_by_hand`'s + 1;
  - **spectral-norm:** allocations are 14 fewer than at S14e-1.
- [ ] **Step 2: Run them to verify they fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Verify** that no `bench/` program's `allocations` exceeds its S14e-1 value. Run `bench/run.sh` and `bench/scripts.sh`. If a `scripts.sh` row moves far from 300 ms, resize it by the S14d rule (docs/implementation/benchmarks.md 「S14d」).
- [ ] **Step 5: Record** 「S14e-2」 in `docs/implementation/benchmarks.md`: the `RunStats` changes, `bench/run.sh`'s counts, and the `bench/scripts.sh` numeric table.
- [ ] **Step 6: Gate, then commit** (`Return state-passing tuples as structs and record S14e-2`). The message lists the `RESIDUAL` change.

### Task 13: Documents, closing S14

Spec: 「文書の更新」, every bullet.

**Files:**
- Modify:
  - `docs/spec/lir.md`, `core-ir.md`, `vm.md`, `object-model.md`: each section the spec lists.
  - `docs/future/roadmap.md`:
    - delete the S14 section and its row;
    - move the remaining S14 issues to 「UTF-8 と標準ライブラリ」, 「マルチコア対応」 and 「言語機能と構文」;
    - edit 「処理系」: inline struct fields; the native entries and the reuse key replacing `f$boxed`; the `f$owned` wrapper for borrowing; delete the T3 dummy item.
  - `docs/superpowers/specs/2026-10-19-s14-numeric-design.md`: note that S14e included the type-directed calling convention.
  - `docs/implementation/testing.md` 「性能のテスト」: remove the T3 time test, and add the composition loop test.
  - `docs/implementation/status.md` and `architecture.md`, once.
  - `CLAUDE.md` only if a pointer changed (e.g. the LIR pass list in the architecture block).

- [ ] **Step 1: Write the documents** with the `yomiyasu:yomiyasu` skill. Check the result with the skill's lint and check scripts (`yomiyasu_lint.py`, `yomiyasu_check.py`).
- [ ] **Step 2: Verify** that no document still says that internal functions are always uniform, or mentions T3 or `f$boxed` as current. Run `grep -rn 'f\$boxed\|T3' docs/spec docs/implementation docs/future`. Expected: only historical records in `benchmarks.md` and the roadmap's native-code notes.
- [ ] **Step 3: Gate, then commit** (`Document S14e and close S14`).
