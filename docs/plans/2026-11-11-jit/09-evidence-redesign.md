# RD design, revision 3 (final): evidence passing that is sound by construction

Tags: [V] = I ran it or read the cited lines; [I] = inferred. Paths are relative to the repository root unless absolute. The prototype is a feasibility patch at 87c2faf4: `09-rd-prototype.diff` (the "prototype diff"). Measurement files of the design session are cited as "(measured in the design session)". Nothing in the worktree was edited by the design session.

Adopted by the user on 2026-11-15 (U-Q): the redesign before N6, both loads refuse, `ev_retire` in `eml_rt` under the full gate. Three points of this revision go beyond that adoption and need the user (section 8).

Unit numbers changed from revision 2 so that no interim state overclaims:

| r3 | Commit title | r2 |
|---|---|---|
| T0 | `Verify the whole program at every native load` | T0 |
| T1 | `Refuse an invalid program at the VM's load` | T5 |
| T2 | `Perform an operation through the context` | T1 |
| T3 | `Retire an evidence and enter it into the evidence list in eml_rt` | T2 |
| T4 | `Give evidences their roles and one owner` | T3 |
| T5 | `Build an evidence whole` | T4 |

## 1. Root cause

- **R-1.** `obj` / `tobj` describe a machine shape; no Repr says "evidence", "`evv` list" or "closure of a `clause (E, op)` function", so the primitive table accepts any `obj` where fast-mode `eml_rt` requires `ev_shape`.
- **R-2.** `ev_new` + n x `ev_set_clause`, and `ev_lookup` + `ev_clause` + `clause`, each state one fact over several statements joined by untyped variables; hence the tracing of L1 / L2.
- **R-3.** One evidence object is used in four roles with different rights (owner frame, clause entry, captured template, fork evidence), all `obj`, `unr`, freely duplicated. A looked-up evidence has no static role.
- **R-4.** Program-level checks are debug-only at the native load; the VM discards every verifier error.

## 2. Design

### 2.1 Reprs

`eml_extern::Repr` gains `Ev`, `Cev`, `Fev`, `Rev`, `Evv` (text `ev`, `cev`, `fev`, `rev`, `evv`): the context Reprs.

| Repr | Value | Mark | Produced only by | Used only by |
|---|---|---|---|---|
| `ev` | the owner's reference to a running evidence | always `lin` | `evidence` (`ev_new` until T5), `ev_renew`; a parameter | `evv_enter` (b), `ev_take_evv` (b), `yield_status` (b), `ev_last_state` (b), `ev_set_clause` (b, until T5), the `prompt` marker, a direct-call argument, `ev_retire`, `drop` |
| `cev` | the evidence a handler entry is called with | `unr` | the evidence parameter of a handler entry | `ev_take_state` (b), `ev_set_state` (b), `ev_evv` (b), `yield_to`, `yield_abort` |
| `fev` | a fork evidence | `unr` | the evidence parameter of a fork entry | `child_abort` (b) |
| `rev` | a retired evidence (table and tag; state and outer `()`) | `unr` | `ev_retire` | `ev_renew` (b); a closure capture |
| `evv` | `()` or a list of nodes | `unr` | `evv_get`, `evv_enter`, `evv_mask`, `ev_evv`, `ev_take_evv` | `evv_set`, `evv_enter`, `evv_mask`, `evidence` / `ev_new` (outer) |

- `is_rc()` true for the five; new `is_value_ref()` = `obj | tobj`, used by `compatible`, so a context Repr is compatible only with itself; `needs_box()` / `is_uniform()` false. [V]
- Machine shape: `evv` as `tobj`, the others as `obj` (`I64`; placeholder `UNIT`).
- `()` does not fit `evv`. Hand-written IR takes a list from `evv_get()`.

### 2.2 One owner

- C-7: an `ev` variable has the mark `lin`. V1 refuses its `dup` / `decref`; the ownership level requires exactly one consumption per path. Consuming uses: a direct-call argument, `ev_retire`, `drop`.
- `evv_enter(w, ev)` borrows `ev`; `Ctx::evv_enter` adds the node's reference itself (2.4). No engine adds a `dup`.
- Handle site and `resume$`: make the evidence, `evv_enter` (b), `evv_set`, `apply .. prompt ev`, `call prompt$(ev, ret, x)`.
- `prompt$(ev: ev lin, ret, x)`: `w = ev_take_evv(ev)`, `evv_set(w)`, `yield_status(ev)`, then
  - 0: `st = ev_last_state(ev)`, `drop ev`, `tail apply ret(x, st)`
  - 1: `drop x`, κ, `c = yield_finish()`, `st = ev_last_state(ev)`, `h = ev_retire(ev)`, `k = closure resume$(h, ret, κ)`, `tail apply c(k, st)`
  - 2: `drop ret`, `drop x`, `c = yield_finish()`, `st = ev_last_state(ev)`, `drop ev`, `tail apply c(st)`
  - 3: `drop x`, `st = ev_last_state(ev)`, κ, `h = ev_retire(ev)`, `closure rewind$(h, ret, st, κ)`, set κ, return the dummy.
- `resume$(h: rev, ret, κ, v, s)`: `ev2 = ev_renew(h, s)`, then as the handle site. `rewind$(h: rev, ret, s, κ, v)`: `tail resume$`.
- [V] prototype: every compiled program passes with this shape; `RunStats` unchanged.

### 2.3 Instructions and primitives

**`clause`** (T2): `Call::Clause { effect, op, args }`; text `clause State.get(())`, `tail clause State.get(())`. Meaning: find the first `E` evidence of `evv`, read entry `op`, call it with (captures, the evidence, `args`). Unwind point, yield point, `never` rules, contract's rule: unchanged.

**`evidence`** (T5): `Rhs::Evidence { effect, state, outer, entries: Vec<(FnIdx, Vec<Atom>)> }`; text

```
let ev.3: ev lin = evidence State(s.1, w.2) [closure get$entry(a.0), &put$entry]
```

Meaning: make the entry closures, the table and the evidence; consume `state`, `outer` and every capture. Not a call, not an unwind point, not pure.

**Primitive table after T5** ((b) = borrowed; parameter mark `lin` at every `ev` position):

| Primitive | Parameters | Result |
|---|---|---|
| `evv_get` | | `evv` |
| `evv_set` | `evv` | `unit` |
| `evv_enter` | `evv`, `ev` (b) | `evv` |
| `evv_mask[items]` | `evv` (b) | `evv` |
| `ev_take_evv` | `ev` (b) | `evv` |
| `yield_status` | `ev` (b) | `int` |
| `ev_last_state` | `ev` (b) | `tobj poly` |
| `ev_retire` | `ev` | `rev` |
| `ev_renew` | `rev` (b), `tobj` | `ev lin` |
| `ev_evv` | `cev` (b) | `evv` |
| `ev_take_state` | `cev` (b) | `tobj poly` |
| `ev_set_state` | `cev` (b), `tobj` | `unit` |
| `yield_to` | `cev`, `tobj`, `tobj` | `unit` |
| `yield_abort` | `cev`, `tobj` | `unit` |
| `child_abort` | `fev` (b), `tobj` | `unit` |
| `yielding`, `yield_cont_take`, `yield_cont_set`, `yield_finish`, `dummy`, `root_get`, `root_set`, `empty_mut_array` | unchanged | unchanged |

Removed: `ev_lookup`, `ev_clause` (T2); `evv_push` (T4, replaced by `evv_enter`); `ev_new`, `ev_set_clause` (T5). `ev_last_state` is the owner's row of `Ctx::ev_take_state`.

**`eml_lir::Step`** (T2; in `prim.rs`): `EvLookup`, `EvClause`, and from T5 `EvNew`, `EvSetClause`: the `Ctx` operations the two instructions run. `Step::ALL`, `Step::name()` (`"ev_lookup"`, ..), `Step::operands() -> usize` (machine operands after `ctx`: 1, 2, 4, 3), `Step::returns() -> bool` (true, true, true, false). The native lowering names an entry as `eml_rt_` + `Step::name()`; the G-8 test iterates `Step::ALL` too.

### 2.4 `eml_rt` (T3)

- `Ctx::ev_retire(ev) -> Result<Word, HeapError>` (`unsafe fn`, inside `verus!`):
  - requires (fast mode): `ev_shape(ev)`; the caller holds one reference of `ev` and gives it up.
  - **both modes**, before any change: `Err(BadEvidence)` unless the state word and the outer word are `()`. (EP takes both before it retires; nothing is released here, so no `Lin` object can be finalised by this operation.)
  - if `ev` is unique: returns `ev`; no count changes.
  - otherwise: allocates an evidence with `ev`'s tag, `ev`'s table (one more reference of the table), state `()`, outer `()`; releases the caller's reference of `ev`; returns the new object.
  - ensures: the result satisfies `ev_shape`, is local, has state and outer `()`, and has `ev`'s table and tag.
  - checked mode additionally returns `BadEvidence` for a non-evidence.
- `Ctx::evv_enter(l, e) -> Result<Word, HeapError>` (`unsafe fn`, inside `verus!`): `Ctx::evv_push`'s requires and ensures, except that `e` is borrowed: the node holds a new reference of `e` and the caller keeps its own. Body: `dup(e)` then the push. `Ctx::evv_push` (consuming) stays for the run time's `fork_evv`.
- No other change; no `external_body` / `assume` (G-18).

### 2.5 Verifier rules and messages

All in `crates/eml_lir/src/check.rs` (shared by both verifiers) unless noted. "in `f`" is appended as usual.

| Rule | Check | Message |
|---|---|---|
| C-1 (T4) | a variable of a context Repr is a function or block parameter, or bound by `evidence` or by a primitive whose row returns that Repr; a multi-value `let` binds none | `` `e.3` is ev, but it is not bound by a primitive that returns ev `` |
| C-2 (T4) | a `never` binding does not pass to a context Repr position (`Context::passes`) | the existing position messages |
| C-3 (T4, `verify.rs`, program level) | no layout field, struct leaf, operation parameter or result, extern row, or function result has a context Repr | ``the layout `L` has a field of ev``; ``the struct shape of `L` has a leaf of ev``; ``the operation `E.op` takes ev`` / ``returns ev``; ``the extern `X` takes ev`` / ``returns ev``; ``the function returns ev`` |
| C-4 (T4, `verify.rs`) | the program entry and the functions of `RuntimeEntry::ALL` have no context-Repr parameter | `` `main` is entered by the run time, but its parameter 0 is ev `` |
| C-6 (T4) | a `closure` capture (and from T5 an `evidence` capture) is not `ev`, `cev`, `fev`, `evv` | ``a closure of `g` captures `e.1` (cev), but only a retired evidence can be captured`` |
| C-7 (T4) | an `ev` variable has the mark `lin` | `` `ev.2` is ev, but its mark is unr, not lin `` |
| marker (T4) | the `prompt` marker's Repr is `ev` (replaces `prompt_marker`) | `` `ev.1` is the prompt marker of an apply, but it is cev, not ev `` |
| clause function (T4) | the evidence parameter is `cev` or `fev` (`clause_function`) | the existing `` `g` is a clause of `E.op`, but its parameter 1 is obj `` |
| F (T4, `verify.rs` `forks`) | a function with a fork name has a `clause` mark, the name equals `fork_name` of that mark, it has `1 + n` parameters, and its evidence parameter is `fev`; a `clause` function taking `fev` has a fork name | the existing ``the fork entry is not a clause of `Fail.fail` ``; ``the fork entry has 3 parameters, but it takes the evidence and 1 arguments of the operation``; ``the fork entry takes its evidence as cev, not fev``; `` `g` takes a fork evidence, but its name is not `fork$Fail.fail` `` |
| clause call (T2) | the operation is in the table; `args.len()` equals its parameters; boundary Reprs and marks | ``a clause of `Counter.get` is given 2 arguments, but the operation takes 1``; the existing argument messages with `argument 0` now the first operation argument |
| evidence (T5) | 1 entries = operations; 2 entry i is a `clause (E, i)` function taking `cev`; 3 exact capture count; 4 boundary Reprs and marks (captures pass to `unr`; `state` to `tobj`; `outer` is `evv`; the binding is `ev`) | ``an evidence of `State` has 1 entries, but the effect has 2 operations``; ``entry 0 of an evidence of `State` is `g`, which is not a clause``; ``entry 0 of an evidence of `State` is `g`, a clause of `Counter.get` ``; ``entry 0 of an evidence of `State` is `g`, which takes a fork evidence``; ``entry 0 of an evidence of `State` gives `g` 2 captures, but the clause captures 1``; ``capture 0 of entry 0 of an evidence of `State` is `n.1` (int), but the function takes tobj`` |
| clause functions as values (T5) | `&g`, `closure g(..)`, `call g` of a `clause` function are refused where written | the existing `` `g` is a clause of `E.op`, but it is used as a function value `` / ``.. but it is called directly`` |

Existing rules that do the rest with no new code [V: read]: `value_references` (`verify.rs:281`) refuses `&prompt$`; `apply` / `par` / uniform positions compare with `tobj`; a tag `switch` needs the layout's Repr (`check.rs:1170`); `unpack` needs `obj` or a struct (`check.rs:1008`); `borrowed_params` and `borrow.rs` use `is_rc()` + `unr` and are not changed.

### 2.6 Why no compiled program is refused by C-6, and why a retire finds a unique object

To be written into core-ir.md 「EP が作る補助の関数」 (T4), as the invariant EP keeps:

- A context variable is bound only at five places, and at none of them is it live at a yield point:
  1. the handle site (`w0`, `ev`, `w1`): the marked `apply` is not a yield point; `call prompt$` consumes `ev`; the lists are dead before it;
  2. a `mask` wrapper (`w0`, `w1`): the continuation's section starts after `evv_set(w0)`, so `w0` is not live in it (「`mask` の包み」);
  3. an under-form entry (`w`, `o`, `e`): its body has no yield point by the definition of an immediate clause (「`perform` と節の3つの形」);
  4. a resuming or `never` entry (`e`): `yield_to` / `yield_abort` consumes it;
  5. the helpers `prompt$`, `resume$`, `rewind$`, `mask$`, `fork$`: fixed bodies; `mask$` reads the list again after the resumption and does not hold it across the `apply` in a closure.
- A closure captures only variables live at a yield point (continuations), the fixed captures of the helpers (`κ`, `c`, `ret`, `h: rev`, a state), or source values. So no closure captures `ev`, `cev`, `fev` or `evv`.
- At `ev_retire` in `prompt$` the owner's reference is the only one: the list was replaced by `evv_set(w)` at the top of `prompt$`; frames between the yield and `prompt$` have returned; `yield_finish` released the target; and no closure holds a list or a running evidence (above).
- What keeps it true: a new EP form that binds a context variable must not leave it live at a yield point. The EP-level verifier refuses a violation (C-6) on every test program; `crates/eml_core_ir/src/evidence/tests.rs` gets the unit test `no_context_variable_is_live_at_a_yield_point` over EP's own outputs (T4).
- [V] corpus: with the rules on, no UI test, REPL transcript or bench program is refused, and the prototype's `ev_retire` (which asserts uniqueness) never trips.

### 2.7 Fork tables and the REPL

Engines keep resolving fork entries by name over every function the machine has (`crates/eml_vm/src/compile/forks.rs:15-70`, unchanged). Rule F is checked where the function is defined, and every load verifies its program, so a function the machine has under a fork name was checked with that name. "Same name, same code" across loads is P2 of docs/future/engine-trust.md, unchanged.

### 2.8 The loads and the trust boundary (decision on review C-1)

- Both safe loads run the final-level verifier in every build and refuse: `NativeMachine::load` panics (`internal error: invalid LIR at the native load: ..`); the VM's load returns `CompileError::Invalid(VerifyError)` (display `invalid LIR: {error}`).
- No new `unsafe` item in the shipped API. The tests that need a function the verifier refuses use `eml_vm::compile::load_unverified` / `Machine::load_unverified`: `pub unsafe fn`, compiled only under a new dev-only feature `testing` of `eml_vm` (enabled by `eml_vm`'s own dev-dependency on itself; `eml_cli` does not enable it; `eml_sched` has the same pattern). Its body is today's load (a refused function gets an empty unwind table). `# Safety`: the program satisfies the execution premises, including every rule the verifier checks, except for the faults the calling test triggers on purpose, and then the test accepts the VM's guards and checked mode as its only protection.
- This is not `Trusted`. docs/future/engine-trust.md keeps its shape (the `unsafe` is on the program value, `Trusted::new`; every engine entry takes `&Trusted`) and is built by its own stage. What changes there (T1 edits the file; English source):
  - 「決めたこと」, replacing "`Trusted::new`はverifierを通さない。…debugのビルドに残す": "`Trusted::new` does not run the verifier. Both loads run the final-level verifier in every build and refuse a program it refuses ([LIR](../spec/lir.md)の「実行の前提」). So promise P1 covers only what the verifier cannot check: the premises listed there."
  - same section, replacing the guard-test sentence: "A test that needs IR the verifier refuses does not make a `Trusted`. It uses the unverified load of `eml_vm`, an `unsafe fn` behind the dev-only feature `testing`."
  - 「今の穴」 P1's "破れる理由": "... the verifier at the load refuses what it can check; the listed premises remain".
- Why not fold into `Trusted` now: `Trusted` changes every engine entry and every caller (`Session`, the REPL, about all engine tests); it is the roadmap's own item and does not depend on this stage. Why not a second public `unsafe` loader: it would mark "verified / unverified", not "sound / unsound" (review C-1).

### 2.9 Engines

- **VM.** New instructions, no temporaries (review A-1):
  - T2: the clause call instructions (`emit.rs:657`, `:802`, `:863`) lose the callee and evidence operands and gain the effect and operation numbers; execution does `ctx.ev_lookup(effect)`, `ctx.ev_clause(ev, op)`, then today's `apply_clause` with the evidence as the first argument. `Op::EvLookup`, `Op::EvClause` are deleted.
  - T4: `Op::EvRetire { dst, ev }` (`Ctx::ev_retire`); `Op::EvvPush` becomes `Op::EvvEnter` (`Ctx::evv_enter`); `ev_last_state` emits `Op::EvTakeState`.
  - T5: `Op::Evidence { dst, at }`, operands [effect, operation count, state slot, outer slot, then per entry: function, capture count, capture slots]; execution builds each closure with the existing closure routine, then `ctx.ev_new`, then `ctx.ev_set_clause` per entry. `Op::EvNew`, `Op::EvSetClause` are deleted.
  - `RunStats`: no instruction is counted, and the `Ctx` calls are the same, so every counter but `peak_stack` is unchanged. `peak_stack` can only shrink (the evidence, callee and entry-closure variables no longer take slots). This is an expected-value change of T2 and T5 (section 8, U-S).
- **Native.** `FnLower::clause` (`apply.rs:533`) calls `eml_rt_ev_lookup`, `eml_rt_ev_clause` (names from `Step`), then continues as today. `evidence` (T5, `effects.rs`): the closure lowering, `eml_rt_ev_new`, `eml_rt_ev_set_clause`. New entries in `eml_nrt` (T4): `eml_rt_ev_retire`, `eml_rt_evv_enter`, `eml_rt_ev_last_state`; `eml_rt_evv_push` is deleted with its primitive. No inline RC is added for evidences.
- **Reference walkers and `evidence` (T5).** `Rhs::Evidence` names functions outside `Atom::Fn` / `MakeClosure`. Add `Rhs::for_each_fn(&self, f)` (closure target, `Atom::Fn` atoms, evidence entries) and use it wherever a pass or engine collects referenced functions; `for_each_atom` yields the captures, `state`, `outer`. Sites to read:
  - `crates/eml_lir/src/variants.rs:111,145` and `borrow.rs` `adapt` / `Program::lends`: no change; an entry is a `clause` function, which is `pinned()` (`lib.rs:149`) and never gets a `$uniform` / `$owned` copy or a lent parameter. Add a `debug_assert!` there that an evidence entry is pinned.
  - `verify.rs` `value_references`: entries are not value references (they use the operation ABI); skip them, as `clause` functions are skipped today.
  - `crates/eml_core_ir/src/tail.rs:47`, `lowering.rs`: map the entry `FnIdx` like a closure target.
  - `crates/eml_vm/src/compile/emit.rs`: map entries through `links.functions`; `crates/eml_native/src/lower/apply.rs` `ClosureEntries`: an entry function gets its closure entry as a closure target does.
  - any reachability or renumbering walk found by `grep -rn 'MakeClosure' crates/eml_lir/src crates/eml_core_ir/src crates/eml_vm/src crates/eml_native/src`: each such site handles `Evidence` too; the commit message lists them.

### 2.10 L1-L4 afterwards; 「実行の前提」 after the stage

| Rule | After |
|---|---|
| L2 C (callee provenance) | gone (T2) |
| L3 (marker) | Repr `ev` + `lin` + the existing "owned after the call" (T4) |
| L4 (not built) | `ev_renew` takes `rev`; helper arguments by the ordinary argument check; `&prompt$` by `value_references` (T4) |
| L1 (capture count; traceable closure and evidence), `clause_values` | checks 2, 3 of `evidence`; "a clause function is never named as a value" (T5) |
| L2 F (filled once) | gone by construction (T5) |
| I-land | stays, keyed on `EP_PROMPT` |
| `forks()` | stays, plus F |

docs/spec/lir.md 「実行の前提」 after T5 (English source of the list; it names only what neither load checks):

- "A program reaches an engine only through a load, and a load refuses a program the final-level verifier refuses. The verifier checks ownership, Reprs, marks, the roles of evidences, the entries of every evidence and the fork entries. The fast mode and native code still assume the following, which the verifier cannot check:
  - every `obj` / `tobj` value is read and released with the layout it was made with; this includes the state of an evidence, which is read as what was last stored (a taken state is `()`);
  - a mark `poly` or `unr` on a value that is `Lin` at run time is not checked;
  - `evv` is restored at returns, and every `clause E.op` and `evv_mask` item finds its evidences;
  - the yield primitives run in EP's order, and `prompt$` runs no call, row, `yield_to` or `yield_abort` before `yield_finish`;
  - an extern row receives values of its std signature's types.
  These follow from the source program's types and the correctness of the passes."
- Removed items: "the last pass's LIR" and "the verifier's ownership rules hold" (both are checked by the load), "`evv` holds only evidences", the fork-table item. The paragraph "エンジンのエントリは…releaseのビルドではこの前提を確かめない" becomes "the loads check what the verifier checks, in every build; the caller promises the rest".

## 3. Alternatives

| Design | Verdict |
|---|---|
| A. This design | chosen |
| B. Typed but unfused (`ev[E]`, an entry Repr, typed `ev_lookup`) | rejected: leaves unfilled slots, needs a "found" Repr and directed subtyping |
| C. Run-time kinds or signature ids in fast mode | rejected: a compare on every `perform` (08 E-D5), fail-stop only at run time |
| D. Dispatch inside `eml_rt` | rejected: the operation ABI cannot pass through a Rust function unboxed |
| E. Keep L1-L3, add L4.. | rejected by the user |
| F. One instruction for the marked `apply` + `prompt$` instead of linearity | rejected: fixes the marker only |
| G. A borrowing `Ctx::evv_push` (changing the existing operation) | rejected for `evv_enter`: `fork_evv` pushes evidences it does not keep, so a borrowing push would add a `dup` + `decref` per fork evidence and move the `par` `RunStats` |

## 4. Units (plan-ready)

Order: T0, T1, T2, T3, T4, T5, then N6, CP-C, N7, CP-D, Z. One commit each. Every unit: **G** (plan 7.3.1) and the **SC** lines in force. Docs are written in the same commit; Japanese through PLaMo (`form: plain`, identifiers and section names in `glossary`), no space between Japanese and Latin characters. Code cites `docs/spec/` only. Test counts: the suite is at the post-L3 count N0 (5219 run in the prototype tree [V]).

Interim soundness of the docs: T0 and T1 make both loads refuse before any rule is added, so every later rule is checked on both engines from its own commit. T2 makes no typing claim. T4 brings the five Reprs at once while L1 / L2 F still guard `ev_new` / `ev_set_clause`. T5 replaces those.

### T0: `Verify the whole program at every native load`

- Model: Sonnet. Purpose: U-Q (D3). The native load refuses, in every build, what `eml_lir::verify` refuses; one walk per function.
- Files:
  - `crates/eml_lir/src/unwind.rs`, `verify.rs`: `pub fn verified(program: &Program) -> Result<Vec<Unwinds>, VerifyError>` = `verify_at(Level::Ownership)` with the recorder on for every function. `forks()` builds one `FxHashMap<&str, &LirFn>` and looks names up in it.
  - `crates/eml_native/src/jit.rs:247-251`: delete the `cfg!(debug_assertions)` block. `crates/eml_native/src/lower/mod.rs:161-172`: call `verified`; on `Err` `panic!("internal error: invalid LIR at the native load: {error}")`.
- Docs: lir.md 「verifier」: replace each "関数ごとの検査なので、`eml_lir::unwind::points`がどのビルドでも走らせ、ネイティブの読み込みはreleaseのビルドでも破ったプログラムを断る" (four places) by one bullet: "The native load runs the ownership level on the whole program in every build and refuses a program it refuses (`eml_lir::unwind::verified`)." native.md 「関数の形」: the same sentence. The VM sentences stay until T1.
- Tests: new (1) `crates/eml_native/tests/effects.rs` `release_check_a_program_level_rule_is_refused` (parsed, unverified: a `clause` function whose parameter Repr differs from the operation's; `load` under `catch_unwind`; message contains `invalid LIR at the native load`). Kind 2: the six `release_check_*` tests expect `invalid LIR at the native load` in both builds (their `cfg!(debug_assertions)` choice is deleted): `release_check_a_tail_clause_result_mismatch_panics`, `release_check_a_clause_entry_signature_mismatch_panics`, `release_check_an_unreadable_clause_entry_panics`, `release_check_an_unfilled_entry_panics`, `release_check_a_clause_callee_not_read_from_its_entry_panics`, `release_check_a_marked_apply_not_followed_by_prompt_panics`.
- Completion: G (N0 + 1); `cargo nextest run -p eml_native --release -E 'test(release_check)'` -> `7 tests run: 7 passed`; `grep -c 'debug_assertions' crates/eml_native/src/jit.rs` -> `0`; `bench/native.sh` on the parent and on this commit (clean trees), both tables in the message.
- Stop: a native wall-time row slower by more than 5 %; any test outside the list failing.

### T1: `Refuse an invalid program at the VM's load`

- Model: Opus. Purpose: U-Q (D4); 2.8.
- Files:
  - `crates/eml_vm/src/compile/mod.rs:108-120`: call `eml_lir::unwind::verified` semantics for the converted functions (program-level checks on the program; per-function walk for `wanted`); return `CompileError::Invalid(VerifyError)`. Keep the debug-build comparison of already-loaded functions.
  - `CompileError` (`Invalid`, display `invalid LIR: {error}`); `crates/eml_cli` where it matches on `CompileError` (an internal error, as other compile errors).
  - `crates/eml_vm/Cargo.toml`: `[features] testing = []`; `[dev-dependencies] eml_vm = { path = ".", features = ["testing"] }`. `#[cfg(feature = "testing")] pub unsafe fn load_unverified` on the loader and on `Machine` (2.8). `nix/crate-graph.sh` tables: the feature.
  - `eml_vm` test fixtures: a helper that calls the unverified load, used only by the tests listed in the commit.
- Work, first step: run `cargo nextest run -p eml_vm -p eml_cli` with the refusal on. Expected [V: measured in the design session, whole verifier at HEAD]: `eml_cli` all pass; exactly 25 `eml_vm` tests fail: `apply` 2, `calls` 3, `codegen` 1, `data` 8, `machine` 3, `par` 1, `run` 4, `structs` 1, `workers` 1, `vm::tests` 1. Classify each by its claim:
  - (s) the claim does not need the violation (e.g. `codegen::array_rows_use_dedicated_ops`: a `lin` result bound to an `unr` variable; `machine::effects_are_renumbered_by_name`: an evidence still owned at the end; `machine::a_continuable_error_recovers_the_heap_for_the_next_run`: an unfilled entry; the seven "still owned at the end" inputs): fix the IR so it verifies. Kind 2, input-only unless a dump is pinned.
  - (g) the claim is a guard of the VM or of checked mode on IR the verifier refuses (e.g. a `lin` value duplicated to see `a Lin object is shared`, a use after a move, a wrong `unpack`): call the unverified load. Kind 3.
- Docs: lir.md 「実行の前提」 last paragraph and 「verifier」 ("VMは拒まれた関数を表のない関数として読む" goes; "both loads refuse"); vm.md 「追記」, 「巻き戻しの表」 (a refused function no longer loads); testing.md 「`eml_vm` のテスト」 (the unverified load and when a test may use it); docs/future/engine-trust.md (2.8); architecture.md feature list if it lists features.
- Tests: new (1) `eml_vm` `machine::a_program_the_verifier_refuses_is_not_loaded` (asserts `CompileError::Invalid` and the message). Kind 2 / kind 3 as classified; the commit message lists the 25 with their class.
- Completion: G (N0 + 2); `cargo nextest run -p eml_vm --release -E 'test(a_program_the_verifier_refuses)'` -> 1 passed; `bash nix/crate-graph.sh --check` -> exit 0; `cargo clippy -p eml_vm --all-targets -- -D warnings` and `cargo clippy -p eml_cli -- -D warnings`; `grep -rn 'load_unverified' crates --include='*.rs' | grep -v 'crates/eml_vm/'` -> empty; `bench/run.sh` on the parent and this commit in the message.
- Stop: more or other tests fail than the 25; a class (g) test outside checked-mode or guard claims; a `bench/run.sh` row's instruction count up by more than 0.5 %.

### T2: `Perform an operation through the context`

- Model: Opus. Purpose: 2.3 `clause`; deletes L2 C.
- Files: `crates/eml_lir/src/` `lib.rs` (`Call::Clause`, `for_each_*`), `prim.rs` (`EvLookup`, `EvClause` removed; `Step`; the mark table test 25 -> 23), `syntax.rs`, `text.rs`, `pretty.rs`, `check.rs` (delete `found`, `entries`, `clause_callee`, and the `EvLookup` / `EvClause` arms of `note_stmt`), `verify.rs`, `boxing.rs:606-617`, `flatten.rs:431,456`, `contract.rs:83`, `reuse.rs:29`, `variants.rs:62`; `crates/eml_core_ir/src/` `lib.rs`, `text.rs:282`, `pretty.rs:197`, `lowering.rs:141`, `verify.rs:448`, `evidence/handlers.rs:662-681`, `evidence/forks.rs:112-128`, `evidence/split.rs:149`, pattern arms in `optimize/`; `crates/eml_vm/src/` `op.rs`, `disasm.rs`, `vm.rs`, `compile/emit.rs`; `crates/eml_native/src/lower/{apply,effects,function}.rs`, `unsupported.rs:99`; the G-8 test (`crates/eml_native/tests/effects.rs:15-30`) iterates `Step::ALL`.
- Docs (English source): lir.md 「節の呼び出し」: "`Call::Clause { effect, op, args }` performs an operation: it finds the first evidence of the effect in `evv`, reads the entry of the operation and calls it with the evidence and `args`. The text form is `clause State.get(())`." 「基本操作」: remove the two rows; 「verifier」: remove the callee bullet. core-ir.md 「`perform` と節の3つの形」: the three-line form becomes `let r = clause E.op(a_1, .., a_n)`; 「`par` の子の中断」 (`reperform$`). vm.md 「呼び出しと戻り」 (the clause call finds its evidence). native.md 「関数の値」 (the two entry calls). testing.md text forms.
- Tests: new (2) `eml_lir` `text::a_clause_names_its_operation_and_arguments`, `eml_native` `effects::a_clause_finds_its_evidence` (`agree`). Kind 1: delete `eml_lir` `verify::a_clause_call_calls_what_ev_clause_read`, `eml_native` `effects::release_check_a_clause_callee_not_read_from_its_entry_panics`; `eml_core_ir` `verify::the_evidence_level_checks_entries_and_clause_callees` loses its callee half and is renamed `the_evidence_level_checks_entries`. Kind 2, scope "hand-written IR and IR or disassembly snapshots with `ev_lookup`, `ev_clause` or a `clause` call": 21 files with `ev_lookup` (`eml_core_ir` 5, `eml_lir` 7, `eml_native` 2, `eml_vm` 7), one `eml_cli` snapshot [V: grep]; and `peak_stack` in `crates/eml_vm/tests/bench.rs` (smaller only; U-S). The commit message lists the tests.
- Completion: G (N0 + 2); `grep -rnE 'EvLookup|EvClause' crates/eml_lir/src crates/eml_core_ir/src | grep -v 'Step::'` -> empty; `grep -cE 'fn clause_callee|found:|entries:' crates/eml_lir/src/check.rs` -> `0`; `cargo nextest run -p eml_vm -E 'test(bench::)'` -> 57 passed, the snapshot diff touches only `peak_stack` lines and only downwards; `cargo nextest run -p eml_native --release -E 'test(release_check)'` -> `6 tests run: 6 passed`; Miri `eml_vm` `miri::` and `eml_nrt` (both settings) -> pass.
- Stop: a `RunStats` field other than `peak_stack` moves, or `peak_stack` grows; a UI output or REPL transcript changes.

### T3: `Retire an evidence and enter it into the evidence list in eml_rt`

- Model: Opus. Purpose: 2.4. Gate: **EG** (plan 7.3.3), table in the message.
- Files: `crates/eml_rt/src/heap/ctx.rs` (both operations inside `verus!`; `ctx/proof.rs` only if a lemma is needed), `crates/eml_rt/src/heap/tests/ctx.rs` (`both_modes!`). No caller yet.
- Docs: object-model.md 「基本操作の契約」: two rows with the contracts of 2.4; 「evidence の3つの物体」: "A retired evidence is made only by `ev_retire`, which refuses an evidence that still has a state or an outer list." `crates/eml_rt/CLAUDE.md` only if a rule or pointer changes.
- Tests (each x2 modes; new 8): `ev_retire_returns_a_unique_evidence_itself`, `ev_retire_copies_a_shared_evidence`, `ev_retire_refuses_a_state_or_an_outer_list` (both modes: `BadEvidence`, heap unchanged), `evv_enter_keeps_the_callers_reference`.
- Completion: G (N0 + 10); EG: `cargo verus verify -p eml_rt` -> `N verified, 0 errors` with N above the parent's; verusfmt; Miri `trusted::` both settings within budget; `bench/rt-gate.sh <parent>` exit 0; `git diff <parent> -- crates/eml_rt | grep -cE '^\+.*(external_body|assume\()'` -> `0`.
- Stop: the proof needs `external_body` / `assume`, or the gate fails twice. The fallback is the orchestrator's decision, not the implementer's.

### T4: `Give evidences their roles and one owner`

- Model: Opus. Purpose: 2.1, 2.2, 2.5 (C-1..C-7, marker, F), 2.6; deletes L3; `evv_push` -> `evv_enter`.
- Files: start from `09-rd-prototype.diff` (19 files; a feasibility patch at 87c2faf4: do not copy the probes `captures_probe`, `PROBE2`, `PROBE3`, `PROBE4`, the `repr` argument and override in EP's four `prim` helpers, the typed `ev_lookup` row, the VM's hand `dup` in `Op::EvvPush`, or the assert in `Op::EvRetire`). Final form:
  - `crates/eml_extern/src/lib.rs`: the five variants, `is_rc`, `is_value_ref`, `compatible`, `name`; unit test `no_extern_row_has_a_context_repr`.
  - `crates/eml_lir/src/prim.rs`: rows of 2.3 (with `ev_new`, `ev_set_clause` typed `(tobj, evv) -> ev lin` and `(ev b, tobj)`); `EvvPush` -> `EvvEnter`; new `EvRetire`, `EvLastState`; the mark table test.
  - `crates/eml_lir/src/check.rs`, `verify.rs`, `crates/eml_core_ir/src/verify.rs`: 2.5; delete `renewed`, `prompt_marker` and its two calls; `syntax.rs` `REPRS`; `flatten.rs:409`; `boxing.rs:186-200,614`.
  - `crates/eml_core_ir/src/evidence/`: `helpers.rs` (2.2; the `prim` helpers take the Repr and mark from `prim.row()`), `handlers.rs` (`e: cev` at :346, :520; the under-form `w`, `o`), `forks.rs:59` (`fev`), `split.rs`, `mod.rs:192`, `tests.rs` (`no_context_variable_is_live_at_a_yield_point`).
  - `crates/eml_vm/src/`: `op.rs`, `disasm.rs`, `vm.rs`, `compile/emit.rs` (2.9).
  - `crates/eml_nrt/src/entries.rs`: `eml_rt_ev_retire`, `eml_rt_evv_enter`, `eml_rt_ev_last_state` (body: `Ctx::ev_take_state`); delete `eml_rt_evv_push`. `crates/eml_native/src/lower/{mod,unwind}.rs` (two arms).
- Docs (English source of the new sentences):
  - lir.md 「値の表現」: "A context Repr (`ev`, `cev`, `fev`, `rev`, `evv`) names a role of an object the run time owns. It is compatible only with itself, is reference counted, and is never uniform." plus the table of 2.1. 「Kind の印」: "A variable of Repr `ev` always has the mark `lin`: a running evidence has one owner." 「基本操作」: both tables. 「着地の印」: "The marker is the frame's own `ev`." 「エントリの関数の印」: the evidence parameter is `cev`, or `fev` for a fork entry. 「verifier」: the rules of 2.5; the L3 bullet goes. 「実行の前提」: remove "`evv`はevidenceだけを持つ" and the fork-table item.
  - core-ir.md 「EP が作る補助の関数」 (parameter Reprs; the invariant of 2.6), 「EP の明示の `drop`」 (`drop ev` on paths 0 and 2), 「handle と `prompt$`」, 「再開と状態」 (`ev_retire`, `rev`), 「`par` の子の中断」 (`fev`), 「verifier」.
  - object-model.md 「evidence の3つの物体」: "That a value reaches only retired evidences is a rule of the verifier: no closure captures a running evidence or a list ([LIR](lir.md)の「verifier」)." 「基本操作の契約」: "The Reprs the verifier checks give `ev_shape` and `evv_wf` at every primitive."
  - vm.md (the two instructions), native.md 「巻き戻しの地点」, 「エントリ」, testing.md.
- Tests, new (9): `eml_lir` `verify::an_evidence_is_bound_only_by_what_makes_one`, `verify::a_never_binding_does_not_pass_as_an_evidence`, `verify::an_evidence_does_not_pass_as_a_value`, `verify::the_owner_of_an_evidence_is_linear` (a `dup` of `ev`; two `prompt$` calls; a `cev` as marker; a `cev` given to `prompt$`; an `ev` marked `unr`), `verify::a_closure_captures_only_a_retired_evidence`, `verify::a_fork_entry_is_checked_under_its_name`; `eml_core_ir` `evidence::tests::no_context_variable_is_live_at_a_yield_point`; `eml_vm` `evidence::retiring_a_shared_evidence_copies_it` (checked and fast; census empty); `eml_native` `effects::a_handler_leaves_no_object_behind` (`agree_source` on a program that handles, resumes and returns; census empty: a missing or extra reference of the evidence fails it).
- Test changes: kind 1: `eml_lir` `verify::a_prompt_marker_is_an_evidence_made_in_its_function` deleted (its claim is in `the_owner_of_an_evidence_is_linear`). Kind 2, scope "hand-written IR and IR or disassembly snapshots whose evidence or list is written `obj` / `tobj`, that contain `evv_push`, or that contain `prompt$` / `resume$` / `rewind$` / `fork$` bodies": at least the 152 tests measured in round 1 (`eml_core_ir` 31, `eml_lir` 54, `eml_native` 26, `eml_vm` 41; measured in the design session); the commit message lists them.
- Completion: G (N0 + 18); `cargo nextest run -p eml_cli` -> no failed, no snapshot change; `cargo nextest run -p eml_vm -E 'test(bench::)'` -> 57 passed, no snapshot change; `grep -c 'renewed\|fn prompt_marker' crates/eml_lir/src/check.rs` -> `0`; `grep -rn 'evv_push' crates/eml_lir crates/eml_core_ir crates/eml_native crates/eml_nrt` -> empty; Miri `eml_vm` `miri::`, `eml_nrt` (both settings); the TSan line of CLAUDE.md for `eml_vm`.
- Stop: a compiled program is refused by a new rule (2.6 says none is; report the program); any `RunStats` field moves.

### T5: `Build an evidence whole`

- Model: Opus. Purpose: 2.3 `evidence`; deletes L1, L2 F.
- Files: `crates/eml_lir/src/` `lib.rs` (`Rhs::Evidence`, `for_each_atom`, `for_each_fn`), `prim.rs` (`EvNew`, `EvSetClause` removed, added to `Step`; table test), `syntax.rs`, `text.rs`, `pretty.rs`, `check.rs`, `verify.rs`, `boxing.rs`, `flatten.rs`, `contract.rs`, `perceus.rs`, `liveness.rs`, `reuse.rs`, `variants.rs`, `borrow.rs` (2.9: asserts only); `crates/eml_core_ir/src/` `lib.rs`, `text.rs`, `pretty.rs`, `lowering.rs`, `tail.rs`, `verify.rs`, `liveness.rs`, `evidence/handlers.rs`; `crates/eml_vm/src/` `op.rs`, `disasm.rs`, `vm.rs`, `compile/emit.rs`; `crates/eml_native/src/lower/{effects,apply}.rs`.
- `check.rs` symbols to delete (and their calls in both `verify.rs`): `evidences`, `filling`, `unfilled`, `struct Filling`, `note_stmt`, `end_block`, `entries_filled`, `fill_entry`, `close_filling`, `violate`, `check_set_clause`, `clause_values`. **Keep** `closures`, `note_closure`, `Context::closure`: Core IR's `parameters_left` (`crates/eml_core_ir/src/verify.rs:645`) reads them for a `handle` before EP.
- EP (`handlers.rs` `lower`): collect the clause variables of every `handle` and their `closure` statements (the scan at :86-96); remove those statements; emit `evidence` with `(entries[g], captures)`; `&g` gives `(entries[g], [])`. A clause atom that is a variable without such a statement: `panic!("internal error: a clause of a handle is not a closure made in its function")`. [V] in every compiled program only other clause closures sit between a clause `closure` and its `handle`, so no call, yield point or unwind point is crossed and nothing else becomes live across one.
- Docs (English source): lir.md 「evidence」: "An evidence is made with all its entries by one instruction, `evidence E(state, outer) [entries]`; no instruction changes a table afterwards." 「基本操作」 (two rows go), 「エントリの関数の印」: "An entry function is named only by an `evidence` entry (or, for a fork entry, by its name)." 「verifier」: the evidence rules of 2.5; the L1 / L2 bullets and the two closing bullets go. core-ir.md 「handle と `prompt$`」 (the form), 「`perform` と節の3つの形」 (the last two bullets: entries are written in the instruction), 「verifier」. vm.md, native.md 「エフェクト」, testing.md.
- Tests, new (4): `eml_lir` `verify::an_evidence_names_one_clause_function_per_operation` (each message of 2.5), `verify::a_clause_function_is_named_only_by_an_evidence`, `text::an_evidence_round_trips`; `eml_core_ir` `evidence::a_handle_becomes_one_evidence_instruction` (snapshot, with a capturing clause). Kind 1, deleted (9): `eml_lir` `verify::a_clause_closure_captures_exactly_what_its_function_takes`, `verify::ev_set_clause_takes_a_function_it_can_read`, `verify::ev_set_clause_takes_an_evidence_made_in_the_function`, `verify::ev_set_clause_names_the_clause_of_its_function`, `verify::every_entry_is_filled_once_before_the_evidence_is_used`; `eml_core_ir` `verify::ev_set_clause_names_the_clause_of_its_function`, `verify::the_evidence_level_checks_entries`; `eml_native` `effects::release_check_an_unreadable_clause_entry_panics`, `effects::release_check_an_unfilled_entry_panics`. Kind 2, scope "hand-written IR and IR or disassembly snapshots with `ev_new` or `ev_set_clause`": 20 files (`eml_core_ir` 3, `eml_lir` 4, `eml_native` 2, `eml_vm` 11) [V: grep]; `peak_stack` (smaller only; U-S).
- Completion: G (N0 + 13); `grep -rnE 'EvNew|EvSetClause' crates/eml_lir/src crates/eml_core_ir/src | grep -v 'Step::'` -> empty; `grep -cE 'fn (note_stmt|end_block|entries_filled|fill_entry|close_filling|violate|check_set_clause|clause_values)|struct Filling' crates/eml_lir/src/check.rs` -> `0`; `cargo nextest run -p eml_vm -E 'test(bench::)'` -> 57 passed, the snapshot diff touches only `peak_stack`, only downwards; `cargo nextest run -p eml_cli` -> no failed; `cargo nextest run -p eml_native --release -E 'test(release_check)'` -> `4 tests run: 4 passed`; Miri `eml_vm` `miri::`, `eml_nrt` (both settings).
- Stop: `allocations`, `rc_*`, `evidence_steps` or `peak_objects` move; EP's panic fires on any test program.

## 5. Amendments to paste

### 5.1 `docs/plans/2026-11-11-jit/00-decisions.md`, G.3

- (j), (k), (l): append "Rule code superseded by the redesign of (m): L2's callee rule by T2, L3 by T4, L1 and L2's entry rule by T5. The commits stay (L3 is 87c2faf4)."
- New (m): "U-Q (user, 2026-11-15): the evidence fragment is redesigned at its root before N6 (units T0-T5 of the plan; design 09-evidence-redesign.md). Evidences get role Reprs (`ev` linear owner, `cev`, `fev`, `rev`, `evv`); an operation is performed by one `clause` instruction and an evidence is built by one `evidence` instruction; `Ctx::ev_retire` (and `Ctx::evv_enter`, U-R) join eml_rt under the full gate; both loads run the final-level verifier in every build and refuse. L4 is not built. D5-D8 of the design are taken as recommended: I-land stays keyed on `prompt$`; Core IR's `handle` is not changed; no end-of-list test in `ev_lookup`; three commits touch the hand-written IR."
- New (n), if approved (section 8): U-R (`Ctx::evv_enter`), U-S (`peak_stack`), U-T (feature `testing` of `eml_vm`; G-14 exception).

### 5.2 Plan `docs/plans/2026-11-11-jit-effects.md`

- **1, S-3**: "every VM expected value is byte-identical (UI outputs, REPL transcripts, `RunStats`), except the V1 contingency of 7.4, IR and disassembly snapshots in T2, T4, T5, and `peak_stack` in T2 and T5 (smaller only)".
- **1, S-4**: "... on each commit touching it (R1-R5, N1, T3)".
- **2, Out of scope, "Optimisations forgone"**: after the list add "The IR's `clause` instruction names the lookup and the entry read as one instruction; native code still calls the two entries and adds no cache."
- **5.1 table**: E-D3 "... every other prim one `eml_rt_<Prim::name()>` shim entry; the four operations the `clause` and `evidence` instructions run are `eml_lir::Step`s with entries `eml_rt_<Step::name()>`"; E-D5 "... `Call::Clause` lowered in `lower/apply.rs`, after the `ev_lookup` and `ev_clause` entry calls"; E-D6 add "the marker has Repr `ev`, which is linear".
- **G-8**: "MUST: a non-inline prim's entry is `eml_rt_` + `Prim::name()`, and a step's entry is `eml_rt_` + `Step::name()`; each is a `shim` whose body is one call of the `Ctx` method of that name (`ev_last_state` calls `Ctx::ev_take_state`: it is the owner's row of that operation). MUST NOT: a lowering-side table of entry names. Check: a test iterates `Prim::KINDS` and `Step::ALL` and checks symbol and descriptor against `Prim::row()` plus the immediates, and against `Step::operands()` / `Step::returns()`."
- **G-13**: add "MUST: an evidence is retired only by `Ctx::ev_retire` and entered into a list by an owner only through `Ctx::evv_enter`; no engine adds a `dup` for it."
- **G-14**: "MUST NOT: a new `Native` field, thread-local or eml_sched edge in this stage. The one new feature is `eml_vm`'s dev-only `testing` (U-T)." Check line unchanged plus "`nix/crate-graph.sh` differs from be1b1de5 only by that feature".
- **G-15**: applies to T3.
- **G-17**: "MUST: both loads run the final-level verifier in every build and refuse (`eml_lir::unwind::verified`). The lowering's own asserts (the Prelude shape at `load`, the clause-entry signature, the tail-clause result, the I-land shape, the non-empty mask, `alloc_fork_table`'s immortality check) stay as second guards and also run in release. MUST NOT: rely on `debug_assert!` for them."
- **7.1 task table**, after L3: T0 (Sonnet; U-Q, G-17), T1 (Opus; U-Q, U-T, G-17), T2 (Opus; U-Q, U-S, G-8, G-9), T3 (Opus; U-Q, U-R, G-13, G-15, G-18), T4 (Opus; U-Q, G-8, G-13), T5 (Opus; U-Q, U-S); then N6 "After: T5".
- **7.2**: paste section 4's six units after L3.
- **N6**: in "Completion", the release line becomes `8 tests run: 8 passed` with the list "the three `prelude_` tests, N4's two, N5's one, T0's `release_check_a_program_level_rule_is_refused`, N6's `release_check_an_empty_mask_panics`". In "New tests", `release_check_an_empty_mask_panics`: "`load` panics with `invalid LIR at the native load` in both builds (the verifier refuses an empty mask before the lowering assert)"; delete "`cfg!(debug_assertions)` picks the expected text". N6's hand-written LIR uses the new text (`clause E.op(..)`, `evidence`, the role Reprs). `eml$raise`: unchanged; it calls the slot the service returns.
- **N7**: no change but the base counts; `Form` detection of `Clause` reads `Call::Clause` as before.
- **Z**: 1a adds "`grep -n 'evvはevidenceだけ\|forkの表) のエントリ' docs/spec/lir.md` -> empty; lir.md 「実行の前提」 is the list of 2.10". 1b C1: "engine-trust.md already says both loads verify (T1); add nothing but the native side's panic text"; delete the carry-forward "natively built fork tables need their own every-build check" from the jit-tasks bullet group (rule F covers it). `exec/carry-forward.md`: drop the fork-table, "`evv` holds only evidences" and "retired evidence" items.
- **7.4**: kind 1 and kind 2 lists of T0-T5 as in section 4. New tests: T0 1, T1 1, T2 2 (2 deleted), T3 4 (x2), T4 9 (1 deleted), T5 4 (9 deleted): 1 + 1 + 0 + 8 + 8 - 5 = 13 more than the plan counted, so the stage floor of 7.4 becomes 5225 + 13 = 5238 (N6 and N7 included, as before).
- **Cumulative `--release` `release_check` counts**: after L3 6; T0 7; T2 6; T5 4; N6 5 (+ 3 `prelude_` = 8).
- **10 Risks**: add the items of section 6.

### 5.3 `docs/plans/2026-11-11-jit/08-effects.md`

- E-D3: add the `Step` sentence of 5.2.
- E-D5: "`Call::Clause` has no callee operand; the lowering calls the `ev_lookup` and `ev_clause` entries, then the entry with `clause_signature`."
- E-D6 and the soundness notes: "The marker is the frame's own `ev` (linear), so the landing's `ev_take_state` is given an evidence by the verifier's typing; I-land is unchanged."
- E-D7: "The `Panic` / `IO` slots hold clause closures by the `evidence` checks; the fixed signatures still rest on `LoadError::Prelude` and the clause-signature assert."
- G-8, G-13, G-14, G-17: as in 5.2.

## 6. Risks

- T3's Verus proofs are not written [I].
- T4 is large and cannot be split without a false interim typing claim.
- T1's classification of the 25 tests is the executor's judgment; the stop rule bounds it.
- `evidence`, the fused `clause`, the VM's two new instruction forms, `Step`, rule F and C-3 / C-4 / C-7 as checks are not prototyped.
- Three touches of the same hand-written tests (T2, T4, T5).
- Not closed (2.10): the data layer including the state field, handler presence, the yield order, `prompt$`'s body.

## 7. What I ran

- The prototype tree at 87c2faf4 (scripts and later edits); diff `09-rd-prototype.diff` (19 files, +184 / -46) plus the round-3 probe below.
  - Round 1 [V]: `ev` / `evv` only: workspace 5067 passed, 152 failed (hand-written IR and IR snapshots), 23 ignored (measured in the design session).
  - Round 2 [V]: five Reprs, linear owner, `ev_last_state`, `ev_retire` (asserting uniqueness), C-1, the capture probe: `eml_cli` 258 + 16 passed; `bench::` 57 passed, snapshots byte-identical; `lowering::` 8 passed.
  - Round 3 [V]: probe "only closures sit between a clause `closure` and its `handle`" (`PROBE4`): `eml_cli` 258 + 16 passed; `bench::` 57 passed.
- A second tree at 87c2faf4 [V]: the VM's load panics when `eml_lir::verify(program)` refuses (whole verifier): `eml_cli` 258 + 16 passed; `eml_vm` 25 failed, the same 25 as with the per-function walk only; reasons by count: "still owned at the end" 7, unfilled entry 3, `con` binding Repr 4, `lin` duplicated 2, one each of eight others (measured in the design session).

## 8. Decisions that still need the user

| ID | Decision | Recommendation |
|---|---|---|
| U-R (adopted by the user 2026-11-15) | A second new `eml_rt` operation, `Ctx::evv_enter` (borrowing push), in T3 beside the adopted `ev_retire` | yes. It makes the primitive's row and the `Ctx` contract agree, so no engine adds a `dup` by hand. Changing `Ctx::evv_push` itself would add a `dup` + `decref` per fork evidence and move the `par` `RunStats` |
| U-S (adopted by the user 2026-11-15) | `peak_stack` in the `bench::` `RunStats` snapshots may shrink in T2 and T5 (the VM's fused instructions need no slots for the evidence, the callee and the entry closures); S-3 is amended | yes. The alternative keeps the old instructions and invents slot temporaries that mirror the old variables |
| U-T (adopted by the user 2026-11-15) | `eml_vm` gets a dev-only feature `testing` that exposes an `unsafe fn` unverified load to its own tests; G-14 gets this one exception | yes. The shipped API gains no `unsafe` and no unverified path; `Trusted` stays the roadmap's item with its shape unchanged |
