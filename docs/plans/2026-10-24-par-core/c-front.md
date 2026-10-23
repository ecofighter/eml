> 付録。par-core の設計の担当が書いた最終版で、英語のまま残す。本文 (docs/plans/2026-10-24-par-core.md) と食い違う所は本文が優先する。違いは本文の 5.3 にまとめた。文中の `par-core/probe/` は `docs/plans/2026-10-24-par-core/probe/` を指す。

# S3: front and middle of `par-core` (r2)

Base: ffc04d6b. This revision aligns with S1-response-r3 (§1 Q1, Q3, Q4, §5 "Interface for S3") and with the user decision that `par` children may return `Lin` values.

- [V] means verified, either by reading the source or by running it in the scratch clone `par-core/S3-repo`. That clone has `Extern::Par`, `std/Par.em`, the `STD` entry and the eml_types change of S3-3b. Probe programs are in `par-core/probe/`.
- [I] means inferred.
- Settled (do not reopen): F1 (mask on the call), F2 (mirrored fork `evv` with depth `d`), `child_abort(ev, f, t)`, conditional generation, names, result bound at the tuple type, `Call::Par` is never a tail call, and the unit order.

## 1. Conclusion

### Q1 `std/Par.em` and registration

```
-- <module header: what Std.Par provides; par waits for both children and re-raises the leftmost child's abort in the parent; both is the sequential version for any row; cite docs/spec/stdlib.md 「`Std.Par`」>
-- <par: the children's row is bounded by never, so IO, var effects and once/multi operations are rejected (E2020); a saturated call becomes the par instruction, not an extern instruction (docs/spec/core-ir.md 「`par` の呼び出し」)>
pub extern par : (Unit -> <never e> a) -> (Unit -> <never e> b) -> <e> (a, b)

-- <both: calls f, then g, in the caller's context; any row>
pub both : (Unit -> <e> a) -> (Unit -> <e> b) -> <e> (a, b)
both f g =
  let a = f ()
  let b = g ()
  (a, b)
```

- `<never e>` on an extern parses, lowers, type-checks and gives E2020 end to end [V]. The probes cover:
  - a child that performs IO (`io.em`) or calls `exit` (`exit.em`);
  - a child that uses a `var` (`varchild.em`);
  - a `once` operation, and an effect that mixes `never` and `once` operations even when the child performs only the `never` one (`once.em`);
  - a value use of `par` (`value.em`).
- Registration:
  - add `("Par.em", include_str!(…))` at the end of `eml_hir::STD`;
  - add `Par.em` to the list at modules.md:69.
- `par` is not exported from Prelude. Users write `import Std.Par` or `import Par (par, both)`.
- `Lin` values:
  - Children may capture and consume `Lin` values, because the parameter arrows are Kind variables [V `lin_cap2.em`].
  - With S3-3b, children may also return them: `lin_ret.em` and `lin_cap.em` type-check, and with `both` they print `10` and `11` [V].

### Q2 Extern row and tie tests

- The row is `Extern::Par => kinded("Std.Par.par", &[TObj, TObj], &[Unr, Unr], Obj, Unr, Purity::Pure)`, appended to the enum and to `ALL`.
- The VM's exhaustive matches (`compile/emit.rs`, `vm.rs::run_extern`) get an unreachable arm. A verified program never contains `extern Std.Par.par`.
- With the row, the module and the S3-3b change, exactly 2 tests fail [V: full `cargo nextest run`, 3548 passed, 2 failed]. Both become kind 2 changes:
  1. `eml_hir` `externs::every_extern_function_is_declared_once_in_std`. New rule:
     - For a `Pure` row that is not method-bound, the last outer arrow may be an open row with no labels, provided its variable is also the tail of some parameter's arrow row.
     - An inner arrow may be an open row with no labels only when its variable is the tail of the last outer arrow.
  2. `eml_core_ir` `externs::faulting_rows_carry_panic`. `tail == None` becomes: `None`, or the tail of a function-typed parameter's row.
- The comment on `kind_of` ("type variables of extern parameters are `Unr`") gets a matching edit. It changes no value, so it is kind 3.
- Scope statement for the plan (B-6): *"The extern tie tests accept a higher-order extern: open, label-less rows whose variable is shared by a callback parameter and the result, on a `Pure` row. Reason: `Std.Par.par` is the first extern with a `<never e>` row."*

### Q3 Translate

- `ProgramBuilder::extern_call` serves both saturated calls and the `$externN` wrapper. On `Extern::Par` it emits:
  - `let t: <type of (a, b)> = par (,) #0(f, g)`;
  - that is, `Call::Par { left, right, pair: Ctor { layout: tuple_layout(2), tag: TUPLE }, at }`, carried in `Rhs::Call { call, mask }`.
- The result is bound directly at the tuple type, `obj` or `struct(S)`.
  - Box insertion treats `par`'s result position as `obj`.
  - When the binding is a struct, the existing `rebind` adds the struct `unbox`, because `Conversion::between(Obj, Struct)` is `Unbox` (boxing.rs:111-124 [V]).
  - Tuple layouts always have `tobj` fields (core-ir.md:206 [V]). So a struct's box has the same shape as the VM's pair.
- The mask (F1) is the one the type checker recorded on the call's last arrow: `self.mask(id, args.len() - 1)`.
  - For any other extern, the debug assert at translate/expr.rs:395 stays.
  - Over-application is impossible because the result is a tuple (A-2).
  - A partial application `Par.par f` becomes a `$externN` closure whose body has no mask. Any masks sit on the later `apply`. `let p = Par.par; p f g` type-checks [V], and the `both` version prints `(0.25, 7)` [V `float_int.em`].
  - The translate snapshot in S3-3a pins the arrow index.
- optimize treats `Call::Par` as an ordinary statement:
  - it is not a yield point;
  - it does not touch `H` (`touch.rs`);
  - its atoms are visited by `reach`.
- The handlers of a never-only effect stay in place, so the parent always holds the evidence that a re-raise needs (C-3):
  - The only optimization that removes a `handle` is handler specialization, and it refuses effects whose operations are all `never` (optimize/handlers/mod.rs:637).
  - `inline` refuses functions that contain a `handle` (fuse/inline.rs:32).
  - `Rhs::Call` is never pure, so contraction never drops one (tail.rs `pure`).
  - The grep `Call::Handle` in `optimize/` finds no other site that removes a handle [V].
  - S3-3a adds a test that pins this.

### Q4 `Call::Par` and `Prim::ChildAbort` through the passes

`Call::Par { left: Atom, right: Atom, pair: Ctor, at: Option<Loc> }` exists in both `eml_core_ir::Call` and `eml_lir::Call`. `for_each_atom` visits `left`, then `right`. `prompt()` returns `None`.

| File | Change |
|---|---|
| `lib.rs` (both) | Add the variant and `for_each_atom(_mut)`. Both operands are consumed (the default `for_each_consumed`) |
| `text.rs` / `pretty.rs` (both) | Text form `par (,) #0(f.1, g.2)`, written like `con`. Optional `mask [..]` prefix (Core IR) and position suffix. Must round-trip |
| core_ir `lowering.rs` | Copy 1:1 |
| core_ir `tail.rs`, lir `boxing.rs::tail_position_call`, lir `contract.rs::tail_call` | `par` is never a tail-position call |
| core_ir and lir `verify.rs` | `par` is allowed at every stage, with a mask at the translated stage. Rules below |
| lir `boxing.rs::call` | Convert both operands to `TObj`. Result `Some(Repr::Obj)` |
| lir `flatten.rs` | Operands go through `single`. `never_returns` is false |
| lir `perceus.rs` | Generic handling: both operands consumed, result owned |
| lir `reuse.rs::may_yield` | false |
| lir `variants.rs` | Unreachable in tail position (the verifier rejects it) |
| lir `unwind.rs::call_abort` | `Abort::Always` |
| core_ir `optimize/*` | Pass-through arms |
| core_ir `evidence/handlers.rs` | A masked `par` goes to `masked_call`; an unmasked one is kept. Inside a clause body it forces the under form |
| core_ir `evidence/split.rs` | Not a yield point |
| vm `compile/emit.rs` | S3-1 leaves an unreachable arm; S1-2 replaces it with `Op::Par` |

**Verifier rules for `Call::Par`:**
- `pair` must name a layout constructor with exactly two `tobj` fields. Message: ``the pair of a par is `L` #t, which does not have two tobj fields``.
- At the boundary stages, each operand must pass to `tobj`. Message: `argument 0 of a par is …`.
- The result is `(Obj, Poly)`.
- With `Receiver::Tail`, the message is `a par is a tail call, but the join needs the parent's frame`.
- From S3-3a on, `Rhs::Extern { ext: Par }` is rejected with ``` `Std.Par.par` is not an extern instruction```.

**Mark rules (B-4).**
- Operands are checked against `Poly`, as for `apply`. Any mark passes, including `lin` closures that capture a `Lin` value. No mark check is added.
- Because both operands are consumed:
  - V1 rejects a `lin` operand that is also used after the call (`` `c.1` (lin) is used again after it is consumed `` or the existing V1 wording);
  - Perceus `dup`s an `unr` or `poly` operand that is used later and leaves a `lin` operand alone.
- The result mark is `Poly`, so it passes to the binding's mark, which may be `unr`, `poly` or `lin` (a `Lin` result).
- The struct `unbox` that box insertion adds gets its fresh variable's mark from `converted_mark`, which copies the binding's mark (boxing.rs:136).
- `param_kinds [Unr, Unr]` is read only by the tie test. `Call::Par` is not an extern instruction, so the LIR never compares its operands against the row.

**`Prim::ChildAbort`** (as settled in S1 r3):
- Text form `prim child_abort(ev, f, t)`.
- Params `[Obj, TObj, TObj]`, borrowed `[true, false, false]`, ret `Unit`.
- Marks `[Unr, Unr, Poly] -> Unr`.
- Not pure; `Abort::Always`; produced only by EP.

**Evidence passing.** The parent passes no evidence. Only the mask on the call matters (F1).

### Q5 `fork$E.op` and `reperform$E.op`

- **Where.** A new file `evidence/forks.rs`, run at the end of `evidence()`. The functions are built in final form.
- **When (B-2, rule widened).** Forks are generated when the program *may run `par`*: it contains a `Call::Par`, or a `prim root_get`.
  - The second clause covers REPL sessions. A REPL binding has a fixed type (E2017 [V]), so it can hold a closure from an earlier input whose body runs `par`. That body is in no function of the current program; the program reaches it only through `root_get`.
  - Without the clause, an input that installs a handler for a newly defined never-only effect around a call of such a closure would get no fork table for that effect. S1 r3's mirror walk would then meet a parent evidence that has no table.
  - Code from earlier inputs that is called by name is lowered again (the whole session is lowered at each input, vm.md:56 [V]). Closures inside data in roots also go through `root_get`. So the rule is complete.
  - `eml run` has no roots, so every existing dump and snapshot stays unchanged. REPL transcripts pin no IR.
- **Which effects.** Every effect `E` in the table with at least one operation and `EffectInfo::never_only()` (C-2).
  - For an extern effect, the multiplicity comes from its `eml_extern` row, looked up by canonical name. This is the same name link that HIR uses for extern effects, and the table is where types.md:24 fixes extern multiplicities.
  - Any other effect qualifies when all of its operations are non-`resumable`.
  - Result: `Prelude.Panic` qualifies. `Prelude.IO` does not, since it is `Once` even though `io_failed` is `never`.
  - Leaving IO in would mirror every `try_io` into each child for an effect children can never perform.
- **Who checks (B-3).**
  - `eml_lir::Program::may_run_par()` (`Call::Par` or `Prim::RootGet` present) is the LIR form of the predicate. `forks.rs` has the Core IR twin. `to_lir` copies 1:1, so the two agree.
  - The LIR verifier (debug builds, every LIR stage) adds a program-level rule. If `may_run_par()` holds, every qualifying effect must have a `fork$E.op` with `clause (E, op)` and a `reperform$E.op` for each operation. Message: `` `Fail.Fail` has no fork entry, but the program may run par ``.
  - S1-2 already makes the mirror walk return `Internal` when a never-only evidence has no table. S3-1/S3-2 hand S1 one more line for the VM load check: whenever `may_run_par()` holds, a missing `fork$` name is a load-time internal error (`CompileError` → `internal error: …`), not a silent skip.
- **Names.** `fork$<effect>.<op>` and `reperform$<effect>.<op>`, built from the effect table names, for example `fork$Fail.Fail.fail` and `reperform$Prelude.Panic.panicked`.
  - Session effects carry their version (`fork$E#2.op`).
  - The code depends only on the operation's table row and the tuple layout's name. So the same name always gives the same code.
  - Effect type parameters do not matter: operation params are `tobj` at type variables.
- **`fork$E.op`.**
  - Flags: `clause (E, op)`; not `yields`, not `internal`, no `split`.
  - Params `(ev: obj unr, a_i: P_i M_i)`; ret `obj poly` (the clause-entry ABI, check.rs:32-95 [V]).
  - The packed argument `t` is `()` for k = 0 and `a_1` for k = 1. For k ≥ 2 it is a `con` with `tuple_layout(k)`, which EP appends by name if absent; the `con`'s mark is the join of the `M_i`.
  - Body: `prim child_abort(ev, &reperform$E.op, t)`, then `return prim dummy()`.
  - B-7, ownership of `ev`: the clause entry owns `ev`, because every `clause` call consumes its operands (`for_each_consumed`). `child_abort` only borrows it.
  - Perceus therefore puts `decref ev` after the prim, where control never arrives. The `child_abort` unwind point's owned set is `[ev]`, so the VM's unwind through the child frames releases it under the child's ID.
  - A Perceus snapshot and an unwind snapshot in S3-2 pin this.
- **`reperform$E.op`.**
  - Flags: not `internal`, not `yields`, no `split`.
  - One param `t`: `unit` for k = 0, uniform(`P_1`) for k = 1, `obj` for k ≥ 2. Mark `poly`; ret `tobj poly`.
  - Body: an `unpack` (k ≥ 2), then `ev_lookup[E]`, `ev_clause[E.op]`, `clause E.op c(ev, x..)`, `return r`.
  - For `Panic`: `extern Prelude.panic(t)`, no position.
- **Contract with the VM (B-5).** The runtime calls `reperform$E.op` in the parent's context, with `evv` already masked by `(E, 0, d)` so that its first `E` evidence is the handler the child reached (S1 r3 Q3 step 4).
  - The function performs at the first `E` evidence of the current `evv`, as every `perform` does.
  - S3-2 writes this into core-ir.md's new paragraph. vm.md (S1) states the masking step.
  - Passing the depth as an argument was rejected: `evv_mask` takes static items, so it would need a dynamic-mask prim.

```
clause Fail.Fail.fail fn fork$Fail.Fail.fail(ev.0: obj, a.1: tobj poly) -> obj poly {
b0():
  let u.2: unit = prim child_abort(ev.0, &reperform$Fail.Fail.fail, a.1)
  let d.3: obj = prim dummy()
  return d.3
}
fn reperform$Fail.Fail.fail(t.0: tobj poly) -> tobj poly {
b0():
  let ev.1: obj = prim ev_lookup[Fail.Fail]()
  let c.2: tobj = prim ev_clause[Fail.Fail.fail](ev.1)
  let r.3: tobj = clause Fail.Fail.fail c.2(ev.1, t.0)
  return r.3
}
```

### Q6 Hazards

- F1 (a `mask` on the call) and F2 (a `mask` inside the child that crosses the boundary) are settled in S1 r3.
- The mirrored-node ownership (B-1) is S1's: see S1 r3 Q1, "per fork", and Q3.
- Panic needs only a single fork evidence at `d = 0`, because extern effects are never masked (types.md:174). The `both` version of `probe/panic_mask.em` (an inner and an outer `try_panic` around a `<Panic | e>` callback) prints `Ok (Err "x")` [V] (C-5).
- Handlers inside a child are pushed in front of the mirrored list. A continuation that escapes the child re-attaches to the current `evv` (`resume$`/`ev_renew`, core-ir.md:901 [V]).
- Mixed `never`/`once` effects, `var`, `IO` and `exit` are rejected by E2020 [V].

### Q7 E2020 tests (C-6)

- **UI.** `tests/ui/check-fail/par/child_performs_io.em` gives exactly one E2020 [V]. The new snapshot is `integration__ui__check_fail@par__child_performs_io.em.snap`.
  - Category `par/`: the roadmap names this test for the `par` feature, and S4's seeded runs use the `par/` category under `run`/`run-fail`.
  - `check-fail/types/row_bound_exceeded.em` stays the generic E2020 rendering.
- **The other two cases** (`exit`, once-op) go to `crates/eml_types/tests/row_bounds.rs`, following testing.md 「重複させない」. They check facts about the bound, not rendering, which is pinned once already:
  - `an_extern_signature_carries_its_never_bound` (a child that calls `exit`);
  - `an_effect_with_a_once_operation_exceeds_a_never_bound_even_through_its_never_operation`;
  - `a_value_use_of_par_keeps_the_bound`.

### Q8 `Lin` results (user decision, unit S3-3b)

- **Rule.** In `declaration_schemes` (eml_types check/mod.rs:719-726), `param_type_vars` stops collecting type variables from the final result of a function-typed parameter. It still collects the parameters of that function type at every arrow of its spine, and every non-function parameter.
  - A variable that also appears in a collected position is still fixed to `Unr`.
  - Scratch implementation: 16 lines replace the `visit_type_vars` call. It is a loop over the parameter's arrow spine, with the old visit kept for non-function parameters [V].
- **Why it is sound.** A callback's result is a value the extern receives from eml code. `par` puts it into the result pair exactly once, or drops it when the other child aborts. Dropping on abort is the same as unwinding a frame that holds a `Lin` value (linearity.md 「暗黙の後始末」).
  - The runtime side is free (S1 r3 U10): `transfer_back` covers a moved region, and the generic `decref` frees a discarded value.
  - No other extern has a function-typed parameter. The workspace stays green except for the two tie tests [V].
- **Spec**:
  - declarations.md:122, with the obligation: an extern that takes a callback uses each callback result at most once;
  - core-ir.md:166 and :198, which say the parameter type variables are `Unr`, plus the note that `par`'s `ret_kind` compares as `Unr` through `Poly`;
  - stdlib.md 「`Std.Par`」, which states that `both` and `par` accept the same types.
- **Tests (new only).**
  - `crates/eml_types/tests/` (the file that tests extern Kind schemes, or `row_bounds.rs` if none fits): `a_par_child_may_return_a_lin_value` (`lin_ret.em` source, no diagnostics) and `an_extern_still_fixes_its_plain_parameter_variables` (a `MutArray`-element `Array.make` call stays E3001).
  - S4's `run/par/`: `lin_ret.em` (prints `10`) and `lin_cap.em` (prints `11`).
  - multicore.md:538's checked-mode test that "a `Lin` object made by `poly` code arrives with rc == -1" is S4's. Its source is `lin_ret.em`: `MutArray.make` runs in `poly` code inside the child and is returned.

## 2. Rationale

- **Fork condition `Call::Par` ∨ `root_get`.** This is the smallest rule that is complete for REPL closures and still leaves `eml run` dumps unchanged.
- **The verifier owns the fork contract.** Any IR that may run `par` but lacks forks then fails in debug builds at the stage that produced it. That matches how every other IR rule is guarded.
- **`reperform$` reads the current `evv`.** This keeps it an ordinary `perform`. The masking stays with the VM, which already knows `d` and `parent_evv`.
- **The `Lin` rule follows where values flow.** Only values that the extern receives are exempt. Values that it passes on (callback parameters, plain parameters) stay `Unr`.

## 3. Alternatives rejected

| Alternative | Why rejected |
|---|---|
| Unconditional fork generation | Every post-EP dump changes (kind 2, wide) |
| A per-session "ever loaded" flag | A pipeline stage would need to remember the session; stages are pure |
| A depth argument to `reperform$` | Needs a dynamic `evv_mask` prim |
| Removing IO from `never_only` by name (`!= "Prelude.IO"`) | A name special case; the extern table already holds IO's multiplicity |
| Writing `once` on `par`'s parameter arrows | Not needed (`lin_cap2.em` [V]) |
| An unpack plus `con` in translate | An extra form, and an allocation or free per pair |
| `Rhs::Par` | Loses the mask machinery |
| Generating forks in translate | Prims and the `clause` mark are rejected before EP |

## 4. Assumptions

- **A1.** The checker's mask for `par` sits on arrow 1 [I]. The S3-3a translate snapshot pins it.
- **A2.** The VM compiles layouts to `[tag, scanned, raw]`, so layout ids do not cross REPL loads [I].

## 5. Implications for implementation

**Order:** S1-1 → S2 ops → **S3-1** → **S3-2** → S1-2 → S1-3 → **S3-3a** → **S3-3b** → S1-4.

Every unit must leave `cargo nextest run`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` green, with no pending snapshot (`cargo insta test --check`).

### S3-1 `Call::Par` and `Prim::ChildAbort` in both IRs

- **Content:** everything in the Q4 table, plus the mark rules. The VM gets a placeholder arm that is unreachable.
- **Tests**:
  - `eml_lir/tests/text.rs`: `par` and `child_abort` round-trip.
  - `eml_lir/tests/verify.rs`:
    - rejected: tail `par`; a bad pair constructor; a non-uniform operand; `child_abort` with the wrong arity or a non-`obj` `ev`; a `lin` closure operand used again after `par`;
    - accepted: a `lin` closure operand used once.
  - `eml_lir/tests/boxing.rs`:
    - a `(Float, Int)` struct binding of a `par` result gets a struct `unbox` from `obj`;
    - an `obj` binding gets no conversion.
  - `eml_lir/tests/contract.rs`: `let t = par …; return t` is not made a tail call.
  - `eml_lir/tests/unwind.rs`: both `par` and `child_abort` are unwind points; check their owned sets.
  - `eml_lir/tests/perceus.rs`: an `unr` closure operand used after `par` is `dup`ed; a `lin` operand gets no RC instruction.
  - `eml_core_ir/tests/evidence.rs`:
    - a masked `par` in a `yields` function and in a non-`yields` function gets the `evv_mask` wrapper;
    - `par` in a clause body forces the under form.
  - `eml_core_ir/tests/text.rs`: a masked `par` round-trips.
- **Criteria:** `cargo nextest run -p eml_lir -p eml_core_ir`; the whole workspace green; no existing snapshot changes.
- **Spec**:
  - lir.md: 「LIR の命令」, 「基本操作」 (the `child_abort` row), 「末尾の位置の呼び出し」, 「巻き戻しの地点」, 「verifier」, 「box の挿入」, 「Kind の印の検査」 (`par` operands are `poly` receivers);
  - core-ir.md: 「Core IR の命令」, 「verifier」;
  - testing.md: 「LIR のテキストの形」.
- **Test changes:** none of kind 1 or kind 2.

### S3-2 Fork generation in EP

- **Content:**
  - `evidence/forks.rs`;
  - in `eml_lir`: `EffectInfo::never_only`, `Program::may_run_par`, and the LIR verifier's fork rule;
  - tuple layouts added by name;
  - one line for S1-2's load check (missing names are an internal error when `may_run_par()` holds).
- **Tests (`eml_core_ir/tests/evidence.rs`, IR text)**:
  - (a) A program with neither `par` nor `root_get` comes out byte-identical.
  - (b) A program with `par` and effects `Prelude.IO`, `Prelude.Panic`, a k = 1 `tobj` effect, a k = 2 `(int, obj)` effect, a `once` effect and an effect with no operations:
    - forks are made for exactly Panic, k1 and k2;
    - their snapshots match;
    - the output passes `verify_evidence` and then `eml_lir::passes_until(Reuse)`;
    - the Perceus snapshot of `fork$` shows `decref ev` after `child_abort`, and its unwind point owns `ev` (B-7).
  - (c) A program with `root_get` and no `par` gets the forks.
- **Other tests:**
  - `eml_lir`: a unit test of `never_only` covering IO excluded, Panic included, a user never-only effect, a mixed effect, and an effect with no operations;
  - `eml_lir/tests/verify.rs`: a missing fork entry is rejected.
- **Criteria:** `cargo nextest run -p eml_core_ir --test integration -E 'test(evidence::)'`; `cargo nextest run -p eml_lir`; the workspace green.
- **Spec**:
  - core-ir.md, a new 「`par` の子の中断」 under evidence passing: the condition, the effect predicate, names, shapes, the ownership of `ev`, and the B-5 contract that `reperform$` performs at the first `E` evidence of the `evv` the runtime prepared (pointer to vm.md 「`par` の fork と join」);
  - core-ir.md 「EP が作る補助の関数」: one sentence;
  - lir.md 「verifier」: the fork rule.

### S3-3a Front end

- **Content:**
  - `std/Par.em`, the `Extern::Par` row and `STD`;
  - the translate branch with the mask;
  - the verifier rule against `extern Std.Par.par`;
  - the two tie-test generalizations. These are kind 2, with the scope statement from Q2.
- **Tests**:
  - the E2020 tests from Q7;
  - `eml_core_ir/tests/translate.rs`: snapshots of a masked `par` call (this pins arrow 1) and of a `$externN` for `par`;
  - `eml_core_ir/tests/optimize.rs`: a `Fail.to_result` whose only `fail` is inside a `par` child keeps its `handle` after optimize (C-3);
  - `eml_core_ir/tests/verify.rs`: `extern Std.Par.par` is rejected;
  - the REPL transcript `crates/eml_cli/tests/repl/par/root_closure_runs_par.repl` (B-2):
    1. `import Std.Par`, and `let f : (Unit -> <Panic> Int) -> <Panic> (Int, Int) = fn h -> Par.par h h`;
    2. a new effect `Stop where never stop : Unit -> a`;
    3. `handle f (fn () -> 1) with | ctl stop () -> (0, 0)`, which prints `(1, 1)`.

    Without the `root_get` clause, step 3 runs `par` under a parent `Stop` evidence that has no fork table.
- **Criteria**:
  - `cargo nextest run -p eml_hir -p eml_core_ir -p eml_types`;
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::) | test(repl::)'`;
  - `timeout 20 cargo run -p eml_cli -- run probe/<x>.em` prints, for each probe:

    | Probe | Expected output |
    |---|---|
    | `ok.em` | `6`, then `Err "no"` |
    | `mask.em` | `Err 7` |
    | `deepmask_par.em` | `Err 7` |
    | `float_int.em` | `3.0`, `42`, `(0.25, 7)` |
    | `panic_mask.em` | `Ok (Err "x")` |
- **Spec**:
  - stdlib.md: a new 「`Std.Par`」, and the exception in 「extern の境界」;
  - modules.md:69;
  - declarations.md:121, since a `par` call is not executed in place;
  - core-ir.md:221 (the exception) and a new 「`par` の呼び出し」 in 「変換の規則」;
  - types.md:57 (`par` is implemented, `compute` is not);
  - status.md (`par`, `both`).

### S3-3b `Lin` results (Q8)

- **Content:** the `param_type_vars` change.
- **Tests:** the eml_types tests from Q8.
- **Criteria**:
  - `cargo nextest run -p eml_types -p eml_core_ir --test integration -E 'test(externs::)'`, with the Kind tie test unchanged;
  - `timeout 20 cargo run -p eml_cli -- run probe/lin_ret.em` prints `10`, and the same for `lin_cap.em` prints `11`.
- **Spec:** declarations.md:122, core-ir.md:166 and :198, stdlib.md 「`Std.Par`」.
- **Test changes:** none of kind 1 or kind 2. Only the comment edit on `kind_of` (kind 3) if it was not already done in S3-3a.

### Facts for S4

| Fact | Detail |
|---|---|
| `par` accepts `Lin`-capturing closures | Yes, from S3-3a on [V `lin_cap2.em`] |
| `par` children return `Lin` values | Yes, from S3-3b on [V with `both`] |
| Unsaturated `par` | `Par.par f` is a closure of `<root>$externN`; applying it runs the same `Call::Par`; `let p = Par.par` works; over-application cannot type-check |
| Uncaught child Panic | The error text names `reperform$Prelude.Panic.panicked` |

Proposed `run/par/` tests. Each one's `both` version prints the same output [V].

| File | Source | Output |
|---|---|---|
| `mask_on_the_call.em` | `mask.em` | `Err 7` |
| `mask_inside_the_child.em` | `deepmask_par.em` | `Err 7` |
| `panic_reaches_nearest_try_panic.em` | `panic_mask.em` | `Ok (Err "x")` |
| `float_int_results.em` | `float_int.em` | `3.0`, `42`, `(0.25, 7)` |
| `lin_result.em` | `lin_ret.em` | `10` |
| `lin_capture.em` | `lin_cap.em` | `11` |
| `values_and_fail.em` | `ok.em` | `6`, then `Err "no"` |

## 6. Unresolved items (defaults)

- **U1.** The VM load check for missing forks lives in S1-2 code. If S1 prefers not to add it, the LIR verifier rule alone covers debug builds.
- **U2.** Where the S3-3b eml_types tests go: the file that tests extern Kind schemes, if one exists; otherwise `row_bounds.rs`.

## 7. Additional checks (r2)

- [V] The S3-3b prototype in the scratch clone gives a full run of 3548 passed and 2 failed, the two failures being the tie tests. `lin_ret.em` and `lin_cap.em` type-check, and their `both` versions print `10` and `11`.
- [V] A REPL `let` needs a decided type (E2017).
- [V] The only optimize sites that remove a handle are handler specialization (guarded at :637); `inline` refuses functions with a `handle`.
- [V] The `both` versions print: `float_int` → `3.0`, `42`, `(0.25, 7)`; `panic_mask` → `Ok (Err "x")`.
- Nothing is running in the background. The main repository was not modified.

## Response to S3-review-r1

| ID | Verdict | Reason | Where it changed |
|---|---|---|---|
| A-1 | Accepted | The user decided to include `Lin` results in par-core | Q8, S3-3b, Q1 |
| A-2 | Accepted | The result is a tuple, so over-application is impossible. A partial application goes through the wrapper, with masks on the later `apply`. A snapshot pins the arrow index | Q3, A1, S3-3a tests |
| B-1 | Resolved by S1 r3 | Ownership of the mirrored nodes is S1's (r3 Q1 "per fork", Q3) | Q6 pointer |
| B-2 | Accepted; rule changed | A REPL root can hold a closure that runs `par` from an earlier input, so the current program has no `Call::Par`. The fork condition is now `Call::Par` ∨ `root_get`, with a REPL transcript | Q5 "When", S3-3a test |
| B-3 | Accepted | `may_run_par()` in eml_lir; the LIR verifier rejects missing forks; the VM load check reports an internal error, not a silent skip | Q5 "Who checks", S3-2 |
| B-4 | Accepted | Operands are `poly` receivers; V1 and Perceus behaviour for `lin` and `unr` operands is stated; the struct unbox mark is copied from the binding | Q4 mark rules, S3-1 tests |
| B-5 | Accepted (documented) | A depth argument would need a dynamic-mask prim. core-ir.md states that `reperform$` performs at the first `E` evidence of the `evv` the runtime prepared | Q5 "Contract with the VM", S3-2 spec |
| B-6 | Accepted | The scope statement and reason are given for the plan | Q2 |
| B-7 | Accepted | The clause entry owns `ev` (every `clause` call consumes its operands); `child_abort` borrows it. `decref ev` is unreachable and the unwind point owns `ev`. Snapshots pin both | Q5, S3-2 tests |
| C-1 | Resolved by S1 r3 | The contract and unit order are settled | — |
| C-2 | Partially accepted | IO stays excluded, but through its extern-table multiplicity, which is the source of truth (types.md:24). Name lookup is how HIR links extern effects too. Leaving IO in would mirror every `try_io` into each child. The unit test covers all cases | Q5 "Which effects", S3-2 tests |
| C-3 | Partially accepted | Verified: the only handle-removing optimization is guarded at :637; `inline` refuses functions with a `handle`; contraction never drops a call. No dead-handler pass exists. A test is added anyway | Q3, S3-3a test |
| C-4 | Partially accepted | Tuple layouts are always `tobj` (core-ir.md:206), so a non-`tobj` struct box cannot occur. Added a boxing test with `(Float, Int)` and a run test | S3-1, S4 facts |
| C-5 | Accepted | `panic_mask.em` is now a proposed `run/par/` test | Q6, S4 facts |
| C-6 | Accepted | The placement reason (one fact, one place) and the `par/` category reason are stated | Q7 |
