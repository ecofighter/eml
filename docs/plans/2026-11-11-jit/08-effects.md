# Appendix 08: jit-effects (the stage's design)

Approved by the user 2026-11-14 together with its section 9; 00 section G records the approval and wins over this file. Base: main @ be1b1de5. Tags: [V] verified (how is inline or in section 7), [I] inferred. "05" = appendix 05, "01" = appendix 01, "00" = appendix 00. Paths are relative to the repository root. Measurements cite a session-local scratch clone that is not kept; their results are quoted inline. The executable plan `docs/plans/2026-11-11-jit-effects.md` carries the units, the rename script and every expected value; where it and this file differ, 00 section G.2 and the plan win.

## 1. Conclusion

| ID | Decision | Status |
|---|---|---|
| E-D1 | Evidence roles are seven crate-private plain consts in eml_rt; the 42 functions move by a mechanical rename with no proof edit | revised from 05 D-6 (no spec/const pairs; measured) |
| E-D2 | Fork evidence: eml_rt gets the constructors and readers; eml_vm is rerouted in place; `fork_plan`, `ForkTables` and a shared `fork_evv` are not built in this stage | revised from 05 D-6 |
| E-D3 | Prim surface: `yielding`, `yield_status` inline; every other prim one `eml_rt_<Prim::name()>` shim entry whose parameters are the `Ctx` method's | kept from 05 D-1, with a derivation rule added |
| E-D4 | Effect numbers and masks: `e:<effect>` (8-byte slot) and `m:<mask>` (`mask_bytes(n)` bytes) data symbols; the `Image` carries machine numbers; `Image.panic_effect` / `io_effect` | revised from 05 D-1 (settles 00 E-3) |
| E-D5 | Clause calls: one signature rule for every closure entry; a clause closure has tag 0; `Call::Clause` lowered in `lower/apply.rs`; the placement rule is unchanged | revised from 05 D-3 (settles 00 E-6 / C-14) |
| E-D6 | Unwind point after a call tests the `abort` byte, not `target`; a yield never branches; the landing test is in the cold block; one emitter shared with `eml$apply$m` | revised from 05 D-2 |
| E-D7 | Extern failure: `eml_nrt_failed(ctx, function, site, purity, out)` writes all clause operands to a 4-word `out`; no `eml_nrt_raise_arg`; `eml$raise` does not depend on the program; `load` refuses a program whose `Prelude.Panic` / `Prelude.IO` operations differ from the fixed shape, in every build | revised from 05 D-4 |
| E-D8 | Root prims are two ordinary prim entries | kept from 05 D-5 |
| E-D9 | `Form` becomes `Par, AsyncRow, BlockingC, ChildAbort` | new |
| E-D10 | Close measurement | kept from 05 D-18 |

### E-D1 Roles (question a)

- In `crates/eml_rt/src/heap/ctx.rs`, inside `verus!`, next to `ev_ok`: `pub(crate) const EV_TABLE: usize = 0; EV_STATE = 1; EV_OUTER = 2; EV_WORDS = 3; NODE_EV = 0; NODE_NEXT = 1; NODE_WORDS = 2;`. Not re-exported from `lib.rs` or `heap/mod.rs`; not in `layout.rs` (that module is `pub` and eml_native imports it).
- Use: exec code writes the const (`set_field(ev, EV_STATE, s)`); spec and proof code writes `EV_STATE as int`. One const serves both modes. No `spec fn` twin, no `when_used_as_spec`.
- Scope of the rename: every literal role index in `heap/ctx.rs` and `heap/ctx/proof.rs` (arguments of `scanned_word`, `word_at`, `scanned_at`, `set_passes`, `field`, `set_field`, `lemma_readable_scanned`, `lemma_set_unit_moved`, `lemma_set_moved`, `lemma_kept_set`, `lemma_ev_field_written`), plus the object sizes 3 and 2 in `data_obj`, `alloc_data`, `Meta::new` of evidence and node objects.
- Spelling at the size sites: `data_obj(h, e, EV_WORDS as u8)`, `Meta::new(tag, NODE_WORDS as u8, ..)`, `alloc_data(tag, EV_WORDS, 0, false)`, `h.scan_of(n) == NODE_WORDS`, `h.count_of(ev) == EV_WORDS`, `bits_scan(wanted) == NODE_WORDS`.
- Verus cost: measured. The scripted rename of the indices (220 replacements) and of the sizes (15 sites), with no proof text edited, verifies: `1250 verified, 0 errors`, 28 s wall, against the baseline `1243 verified, 0 errors` [V: section 7, items 1-2, 10]. `verusfmt --verus-only --check` fails on the raw script output (49 diff lines, line wrapping) and passes after one `verusfmt --verus-only` run on the two files [V: item 10]; the unit runs verusfmt once.
- Bench gate on the rename: `bench/rt-gate.sh be1b1de5 3` on the committed rename exits 0; the 31 gated rows have medians from -0.029 % (`yield`) to +0.051 % (`dup`), all `ok` [V: item 12]. The stage's own run uses 5 pairs.
- No fallback is needed.

### E-D2 Fork evidence (question g)

- New `pub` items of eml_rt, all inside `verus!`, all `pub unsafe fn` with a `# Safety` section (00 G.3):
  - `unsafe Heap::alloc_fork_table(&mut self, entries: &[Word]) -> Result<Word, HeapError>`: immortal `Data` (tag 0), one scanned word per entry. Every entry must be an immediate or point at an immortal object; the function checks this in both modes before it allocates (`Heap::is_immortal`) and returns `Err(HeapError::NotImmortal)` otherwise. It runs at load only, so the check is not on a hot path.
  - `unsafe Heap::alloc_fork_tail(&mut self, panic: u16, table: Word) -> Result<Word, HeapError>`: the immortal evidence `[table, imm 0, ()]` (tag `panic + 1`) and its immortal node `[ev, ()]`; returns the node. `table` gets the same immortality check; `panic == u16::MAX` is `Err(BadEvidence)`.
  - `unsafe Ctx::fork_ev(&mut self, effect: u16, table: Word, depth: u32) -> Result<Word, HeapError>`: a local evidence `[table, imm depth, ()]`, tag `effect + 1`.
  - `unsafe Ctx::fork_of(&self, ev: Word) -> Result<(u16, u32), HeapError>`: `(tag - 1, depth)`; checked mode returns `BadEvidence` for a non-evidence or a non-immediate state.
  - `unsafe Ctx::evv_step(&self, w: Word) -> Result<Option<(u16, Word)>, HeapError>`: `None` for `()`, else `(effect of the node, next node)`; borrows. Replaces 05's `evv_effects -> Vec<u16>` (no allocation, one step, a small contract).
- `fork_ev` requires in fast mode `effect < u16::MAX` and that `table` is a fork table (`Heap::fork_table`); checked mode returns `BadEvidence` for `effect == u16::MAX` or a `table` that is not an immortal tag-0 `Data`.
- Why a fork evidence is an evidence (`ev_ok`, `ctx.rs:343-349`): it is a `Data` with `EV_WORDS` scanned words; its tag `effect + 1` is not 0; `EV_TABLE` holds a `table_ok` object (the constructor's result: tag 0, no `lin` bit, every entry dupable because it is immediate or immortal); `EV_STATE` holds an immediate, which is `droppable`; `EV_OUTER` holds `()`, which is `dupable`. `evv_push`'s fast-mode precondition `ev_shape(e)` therefore holds for `fork_ev`'s result.
- Contract required of each new operation: `wf` kept; `evv_spec` and `yield_spec` unchanged; `!M::CHECKED ==> r is Ok` under the stated fast-mode precondition; the checked-mode errors above. `fork_ev` also states the functional result as `ev_new` does (`ev_shape(e)`, `ev_tag(e) == effect + 1`, `ev_table(e) == table`, `ev_state(e) == imm(depth)`, `ev_outer(e) == ()`, the `evolved_by` delta), because `evv_push`'s precondition is discharged from it.
- Verus cost of `fork_ev`: partly measured. A prototype cloned from `ev_new` (one `alloc_data`, two `set_field`) discharges every call precondition after four iterations with existing lemmas (`lemma_alloc_moved`, `lemma_arg_counted`, `lemma_points_in`, `lemma_arg_moved`, `lemma_set_passes_kept`, `lemma_imm_round_trip`); the one open obligation is `final(self).wf()` at the end, which in `ev_new` is `lemma_ev_new_ok` (about 110 lines in `ctx/proof.rs:238-350`) [V: item 11, `fork_ev-prototype.rs`]. The work is one lemma `lemma_fork_ev_ok` of that size without the table half. `fork_of` and `evv_step` are single field reads like `ev_evv` and one step of `checked_room`'s loop [I]. The two `Heap` constructors (`alloc_immortal` + `set_field`) are not measured [I].
- Stop rule: if unit 3 does not verify, the stage does not move the constructors out of `verus!` or mark them `external_body`. It goes to the user with the alternative "per-fork evidence from the existing operations (`ev_new(effect, n_ops, imm depth, ())` + `ev_set_clause` per operation; one more allocation per mirrored evidence), frozen tail left in eml_vm", which keeps G-13 only in part.
- Nodes of a child `evv` are built with the existing `Ctx::evv_push(next, ev)`.
- eml_vm rerouted in place: `compile/forks.rs` (`table`, `panic_tail`) calls the two `Heap` constructors; `vm.rs` `fork_evv` uses `evv_step`, `fork_ev`, `evv_push`; `child_abort` uses `fork_of`. `Forks`, `ForkTable` and `fork_evv` stay in eml_vm.
- Not in this stage: `eml_lir::fork_plan` (the existing `fork_effects` + `fork_name` are what it would wrap [V: `crates/eml_lir/src/lib.rs:1012-1023`]), eml_sched `ForkTables` / `fork_evv`. jit-tasks moves `Forks`, `ForkTable`, `fork_evv` to eml_sched when eml_nrt becomes the second caller.

### E-D3 Prim surface (question b)

- Prim lowering lives in new `crates/eml_native/src/lower/effects.rs`. Inline (readers in `mem.rs`):
  - `yielding()`: `L target; icmp ne UNIT` -> 0 / 1.
  - `yield_status(ev)`: `t = L target; a = L8 abort;` result `t == UNIT ? 0 : (t != ev ? 3 : (a ? 2 : 1))` with `select`s, no branch. Exactly `Ctx::yield_status` [V: `ctx.rs:1402-1424`].
- Entries: table `EFFECTS` (prefix `eml_rt_`) in new `crates/eml_nrt/src/effects.rs`, added to `TABLES`. All `shim` (with `catch_unwind`). The symbol is `"eml_rt_" + Prim::name()`; lowering derives it, it keeps no table. The body is one call of the same-named `Ctx` method (`dummy`: `ctx.heap().dummy()`). Parameters are the `Ctx` method's parameters in order; a slice is `(ptr, len)`; an effect or op number is `i64`. A shim converts an effect with `u16::try_from` and an op, count or root index with `usize::try_from` / `u32::try_from`; a failed conversion is `Err(HeapError::BadEvidence)` (`RootMisuse` for a root index), which ends in the shim's fatal path. A correct `Image` never produces one.

  | Prim | Entry after `ctx` | Immediate source |
  |---|---|---|
  | `evv_get` | `() -> tobj` | |
  | `evv_set` | `(l: tobj)` | |
  | `evv_push` | `(l: tobj, e: obj) -> tobj` | |
  | `evv_mask[items]` | `(items: ptr, n: i64, w: tobj) -> tobj` | `symbol_value(m:<mask>)`, `n` immediate |
  | `ev_new[E]` | `(effect: i64, n_ops: i64, state: tobj, outer: tobj) -> obj` | `L e:<E>`; `n_ops` immediate |
  | `ev_set_clause[E.op]` | `(ev: obj, op: i64, c: tobj)` | `op` immediate |
  | `ev_renew` | `(ev: obj, state: tobj) -> obj` | |
  | `ev_lookup[E]` | `(effect: i64) -> obj` | `L e:<E>` |
  | `ev_clause[E.op]` | `(ev: obj, op: i64) -> tobj` | `op` immediate |
  | `ev_evv`, `ev_take_evv`, `ev_take_state` | `(ev: obj) -> tobj` | |
  | `ev_set_state` | `(ev: obj, s: tobj)` | |
  | `yield_to` | `(ev: obj, clause: tobj, k: tobj)` | |
  | `yield_abort` | `(ev: obj, clause: tobj)`, then `After::Never { raise: None }` | |
  | `yield_cont_take`, `yield_finish` | `() -> tobj` | |
  | `yield_cont_set` | `(k: tobj)` | |
  | `dummy` | `() -> obj` | |
  | `root_get[n]` | `(index: i64) -> tobj` | `n` immediate |
  | `root_set[n]` | `(index: i64, w: tobj)` | `n` immediate |

  A prim with a `unit` result binds the constant `UNIT`.
- `perform` is not a lowering unit: `ev_lookup`, `ev_clause` and `clause` are lowered one by one. Handler install likewise (`evv_get`, `ev_new`, `ev_set_clause` x ops, `evv_push`, `evv_set`).
- `CTX_YIELD_ABORT`: `offset_of!(Ctx<Fast>, native) + offset_of!(Native, yielding) + offset_of!(YieldState, abort)` (24), added with the other `CTX_*` consts, const asserts and the pin test (`heap/tests/ctx.rs:938`). Readers: the unwind check (E-D6) and inline `yield_status`. It is read as one byte (`mem::yield_abort`, an 8-bit load); the 7 bytes after it are padding.
- `prim child_abort` stays rejected (E-D9); `empty_mut_array` is unchanged.

### E-D4 Effect numbers and masks (question c)

- Machine numbers: `NativeMachine` keeps `effects: name -> u16`, assigned at `load` for every entry of `program.effects` in table order, as `eml_vm` `Index.effects` does [V: `crates/eml_vm/src/compile/mod.rs:241-250`]. Same load sequence gives the VM's numbers. A number is below 65535 (`u16::MAX` is not an effect, object-model.md 「evidence の3つの物体」); `load` panics past that with the VM's text, as `eml_vm::compile::emit::effect_number` does [V: `emit.rs:1048-1053`].
- `Datum::Effect(u32)`: symbol `e:` + `eml_lir::syntax::effect_text`; 8 bytes; `DatumRow { kind: DATUM_EFFECT = 5, bits: machine number (< 65535) }`. `fill_data` writes `bits` to the slot as a raw `u64`. A use is `symbol_value` + 1 load, passed as `i64`.
- `Datum::Mask(Vec<MaskItem>)`: symbol `m:` + `eml_lir::syntax::mask_text` [V: `crates/eml_lir/src/syntax.rs:30`]; size `eml_nrt::image::mask_bytes(n)` = `n * size_of::<eml_rt::MaskItem>()`, align 8; `DatumRow { kind: DATUM_MASK = 6, items: Slice<MaskRow> }`, new field `items`, `#[repr(C)] MaskRow { effect: u16 /* machine number */, keep: u32, skip: u32 }` (the field types of `MaskItem`, so no narrowing at fill). `n >= 1`: the LIR verifier requires a non-empty list (`Prim::EvvMask` doc, `prim.rs:14`), and the lowering asserts it, so no zero-sized data object exists. `fill_data` writes `MaskItem`s through a typed pointer, in LIR order. No sort: `Ctx::evv_mask` does not require an order [V: doc comment at `ctx.rs:3191-3193`, "ここには前提として求めない"]. A use is `symbol_value` only (0 loads).
- `Data::define` sizes each object by its datum (`Datum::bytes()`): 8 for the word kinds, `mask_bytes(n)` for a mask. eml_native never names `MaskItem`.
- `fill_data` separates heap-word kinds (0..4, through `constants::build`) from raw kinds (5, 6, written directly).
- `Image` gains `panic_effect: u32` and `io_effect: u32` (machine number or `NO_EFFECT = u32::MAX`), set by eml_native from the machine's table by `ExternEffect::{Panic, Io}.row().name`. `Worker` reads them through `image`. `eml_nrt_failed` converts with `u16::try_from`; `NO_EFFECT` fails the conversion and takes the "no handler" outcome without calling `ev_find`. This is 01 §1.3's text as written.
- `n_ops`, op indices and root numbers stay immediates: the effect's declaration and the function's LIR fix them.

### E-D5 Clause calls (question d)

- One rule for every closure entry `c<k>:<g>`: its signature is `tail (ctx, clo, kinds(params[k..])) -> kinds(rets)` of `g` itself. For a non-clause function this equals `uniform(n)` (the existing assert in `apply::arity` guarantees it). `ClosureEntries::id` and `define_closure_entry` use the rule; there is no clause branch in them.
- `ClosureEntries::id` asserts, in every build, that a clause function's entry signature equals the signature built from its operation (`LirFn.clause = Some((effect, op))`); a mismatch is a compiler bug and panics in lowering.
- Tag: the tag of a function value is the number of arguments `apply` must still supply; a clause closure has tag 0, "not an `apply` target". `apply::arity` drops the `clause.is_none()` assert and returns 0 for a clause function without the uniform assert. `fill_data` accepts `DATUM_FN` with `bits == 0` (today an `Internal` error), and `static_closure` builds `data_meta(0, 1, false)`. An `apply` of such a value with `m` arguments never reaches the typed entry: `0 != m` sends it to `eml$apply$m`, whose `n < m` arms end in `eml$apply$1`'s trap for `n < 1` [V: native.md 「関数の値」]. Readers of the tag are the apply site, `eml$apply$m` and `eml_nrt_pap` (reached only when `n > m`) [V: native.md 「関数の値」 last bullet]; `Call::Clause` does not read it.
- Ownership: a closure entry consumes `clo` (it releases the box after loading the captures) and passes the other arguments on; `Call::Clause` consumes `c` and every argument, as the LIR says.
- `Call::Clause { effect, op, callee: c, args }`: `meta = mem::load_meta(c); code = mem::closure_code(b, c, meta); r = call_indirect sig code(ctx, c, args...)`, `sig` built in `abi.rs` from `program.effects[effect].operations[op]` (`(ctx, clo, ev, op params) -> op ret`; a clause function's parameters are captures, then `ev`, then the operation's parameters), then `After::Call`. `TailCall(Clause)` is `return_call_indirect` for an operation that returns, with `assert_eq!` in every build that the clause signature's results equal the caller's results (Cranelift's verifier rejects a tail call whose results differ from the caller's, and its flag is not relied on: G-17). A `never` operation's clause is called with `call_indirect` in both positions (a `never` clause's binding keeps the Repr translate chose, and contraction makes a tail-position one a tail call whatever the Repr, docs/spec/lir.md; the entry returns one `I64`), the result is discarded, then the abort test (`unwind::if_aborting`: the cold side is the point's cleanup, in tail position returning placeholders of the caller's results; the hot side traps with `TrapCode::unwrap_user(1)`, as `eml$raise` does), and execution continues in a block with no predecessor whose statement variables are bound to placeholders of their own Reprs, as `After::Never` does for rows. No tag test.
- Placement: the clause lowering and the one helper that emits "load the code word of a closure and call it indirectly" live in `lower/apply.rs`; the exact arm of `apply`, `Call::Clause` and `eml$raise` (E-D7) call that helper. `placement::indirect_calls_are_only_in_apply_and_thunk` and N-37 (00 F.2 text) stay as they are.
- `fork$E.op` functions are reached by jit-tasks through `fn_value` (unchanged from 05 D-3).

### E-D6 Unwind point after a call (question e)

- `After::Call`: `a = L8 CTX_YIELD_ABORT; brif a, cold, hot`. Nothing else on the hot path; no `target` load, no compare with a constant.
- A resumable yield returns on the hot path with the callee's LIR dummy; the LIR's own `prim yielding()` takes the slow path. No `split` block exists.
- Cold block of a point without a landing: the cleanup as today.
- Cold block of a point with landing `v`: `t = L target; brif (t == v), land, other`.
  - `land`: `x = call eml_nrt_landed(ctx)` (new service in `services.rs`, result `obj`: the heap's dummy; jit-tasks restores the limit class in it); bind the call's result variable to `x`; `jump hot`.
  - `other`: `s = call eml_rt_ev_take_state(ctx, v); call eml_rt_decref(ctx, s)`; then the cleanup (owned by count, tokens, placeholders). This is the VM's order [V: `crates/eml_vm/src/vm.rs:1707-1740`].
- `fill_cleanup` loses its `landing.is_none()` assert.
- One emitter: `unwind::if_aborting(b, ctx) -> (cold, hot)` (load + `brif` + cold block). `after_unwind_point` and `define_apply`'s `n < m` arm both call it; the arm's cold block keeps its own content (`decref a_m`, return `UNIT`). The hand copy of the check is deleted.
- The arm relies on "only the last arrow of one `apply` can yield" [V: docs/spec/native.md 「関数の値」 `n < m` bullet, citing core-ir.md 「`apply` の印と分け方」]. It guards the rule: on the hot side of `if_aborting` it loads `target` and branches to a cold `trap` when it is not `UNIT` (a yield in a non-last part). The guard is in this helper arm only.
- Invariant the abort test needs (I-land): from a landing until `yield_finish` no call unwind point is passed. It holds at HEAD: EP emits the marked `apply` immediately followed by `let r = call prompt$(ev, ret, x)` [V: `crates/eml_core_ir/src/evidence/handlers.rs:644-657`], and `prompt$`'s status-2 arm is `drop ret; drop x; yield_finish; ev_take_state; apply c(st)` with only prims before `yield_finish` [V: `evidence/helpers.rs:60-118`]; `resume$` has the same shape (core-ir.md 「再開と状態」) [I]. The check after `call prompt$` runs when `prompt$` has returned, after `yield_finish`.
- I-land is checked: `eml_lir::verify` gains the rule "after an `apply` with a prompt marker `v`, the next `Rhs::Call` or tail call in that block is a direct call of `prompt$` whose first argument is `v`, and no `Rhs::Call`, `Rhs::Extern`, `prim yield_to` or `prim yield_abort` comes between them" (message `` the apply with the prompt marker `ev.2` is not followed by `prompt$` ``), and a unit test in `crates/eml_core_ir/src/evidence/tests.rs` asserts that no `Rhs::Call` precedes `prim yield_finish` in `prompt$`'s status-1 and status-2 arms. The first half (what follows each marked `apply`) varies per program and per LIR pass, so native lowering also checks it in every build: when a block lowers `Call::Apply { prompt: Some(v) }` it asserts that, until the direct call of `prompt$` whose first argument is `v`, the block lowers no `Rhs::Call`, `Rhs::Extern`, `prim yield_to` or `prim yield_abort`, and does not end with the marker pending (the verifier's message). The second half (the body of `prompt$`) is one fixed function and its unit test is exhaustive.
- The entry safepoint of `prompt$` runs with the abort still set. `eml_nrt_safepoint` in this stage returns `ACTION_CONTINUE` or a fatal `Err`, never an unwind [V: `services.rs:23`, 01a dossier §2], so it cannot start cleanups there. jit-tasks keeps this (05 D-14 step 4, "yielding: `Continue`").

### E-D7 Extern failure (question f)

- `eml$raise(ctx, function, site, purity)` (signature unchanged) is:
  ```
  out = 4-word stack slot (mem.rs helper)
  class = call eml_nrt_failed(ctx, function, site, purity, out)
  class == RAISE_PANIC (0): call clause out[0] with (ctx, out[0], out[1], out[2])           sig = uniform(2)
  class == RAISE_IO    (1): call clause out[0] with (ctx, out[0], out[1], out[2], out[3])   sig = uniform(3)
  otherwise: nothing
  return
  ```
  Both arms are always emitted; the two signatures are fixed and do not come from the program: `uniform(2)` = `tail (ctx, clo, ev, message) -> I64` and `uniform(3)` = `tail (ctx, clo, ev, kind, message) -> I64`, every leaf `I64`. They are the entry signatures of `panicked$entry(e: obj, m: obj) -> obj` and `io_failed$entry(e: obj, k: enum, m: obj) -> obj` [V: LIR text at `crates/eml_vm/src/vm.rs:4680`, `4804`; effect lines `never panicked(obj)` at `4657`]. `NativeMachine::load` checks in every build, before lowering, that an effect of `program.effects` named `ExternEffect::Panic.row().name` has exactly one operation with `params == [Obj]` and `ret == OpRet::Never`, and one named `ExternEffect::Io.row().name` exactly one with `params == [Enum, Obj]` and `ret == OpRet::Never`; otherwise it returns the new `LoadError::Prelude { effect: String }` and compiles nothing. With E-D5's signature assert, the fixed signatures then equal those of every clause the service can return. The clause is called through E-D5's helper. After either clause call, `eml$raise` tests the abort byte with `unwind::if_aborting` and traps (`TrapCode::unwrap_user(1)`) when it is clear. The site then jumps to its cleanup as today.
- `eml_nrt_failed(ctx, function: i64, site: i64, purity: i64, out: ptr) -> i64`, order:
  1. `f = ctx.take_fault()`. `None`: record `Internal`, `Err`.
  2. `Fault::Engine`: take `worker.panic` (a message word). It is class `Panic` with no purity filter. This equals the VM: the only producers of an engine panic are `eml_x_Panic` and the `eml_x_c_*` helpers [V: `grep park_panic crates/eml_nrt/src`: `rows.rs:96`, `c.rs:39`], and the VM handles both by calling `abort_to(.., ExternEffect::Panic, ..)` directly, not through `failed` [V: `vm.rs:4212-4221`, `ffi.rs:410-424`]. (jit-tasks inserts "a re-raise request, if any, first" here and nowhere else.)
  3. Any other `f`: class `Panic` if `f.panics() && purity != Pure`; class `IO` if `f.io_error_kind()` is `Some` and `purity == Effectful`; otherwise uncatchable. This is VM `failed` [V: 01b dossier §5, `vm.rs:4109`].
  4. Uncatchable, the class's `Image` effect is `NO_EFFECT` (no `ev_find` call), or `ctx.ev_find(effect)` is `None`: `worker::record(ctx, fault, Some(At { function, site }))` and return its `Fatal` (for an engine panic the recorded fault is `Panicked(copy of the message)`, as today).
  5. Found `ev`: `clause = ctx.ev_clause(ev, 0)`; write `out = [clause, ev, message]` (Panic; the message is the parked word, or `fresh_string(f.to_string())`) or `[clause, ev, kind, message]` (IO); return `RAISE_PANIC` / `RAISE_IO`. `kind` is the raw integer of `Fault::io_error_kind()` (`IO_NOT_FOUND`, ...), the value of an `enum` leaf, not a tagged immediate; the VM passes the same bits as `Word::from_bits(kind as usize)` [V: `vm.rs:4126-4131`]. Ownership: `clause` (from `ev_clause`, already `dup`ed), `ev` (from `ev_find`, already `dup`ed) and `message` each carry one reference that passes to the clause entry, which consumes `clo`, `ev` and the operands.
- `RAISE_PANIC = 0`, `RAISE_IO = 1` are defined in `image.rs`. `RAISE_DONE` is not defined in this stage; the "otherwise" arm of `eml$raise` already serves it.
- No `eml_nrt_raise_arg`. Nothing is left in the worker struct between two entries.
- Catching a C-row panic needs nothing else: `eml_x_c_*` already park `worker.panic` + `Fault::Engine` [V: 06 §5.6 "Panics of the helpers"].

### E-D8 Root prims (question h)
As in E-D3's table. `root_set`'s `Err(RootMisuse)` ends in the shim's fatal path; it is already in spec/native.md's fast-mode `Err` list [V: 01a dossier §9, native.md line 44]. Tested by LIR-text codegen tests; no `eml run --native` program contains them.

### E-D9 `Form` (question i)
`Form::Prim` is renamed `Form::ChildAbort` (label `child abort`, trigger `Rhs::Prim(Prim::ChildAbort, _)`) and moved last; `Form::Clause` and `Form::Prompt` are deleted. Declaration order: `Par, AsyncRow, BlockingC, ChildAbort`. No file's first form becomes `child abort` in the UI runs, since `fork$` functions exist only with `Par`, `fork_job` (an async row) or `root_get` [V: core-ir.md 「`par` の子の中断」].

### E-D10 Close
`bench/native.sh` replaces 「ネイティブの数値」; rows `state`, `state_loop`, `state_loop_by_hand`, `state_nested`, `fold_state`, `ep_float`, `iter_generate`, `iter_pipeline`, `iter_pipeline_by_hand` are named in the text. `ep_float` must have a number (it is `rejected` today [V: benchmarks.md line 445]). The VM tables are replaced too (eml_rt, eml_vm changed).

### Native fast-path cost model (question 2)

`L` load, `L8` byte load, `B` conditional branch, `C` entry call (about 8.3 instructions of call overhead per shim entry, benchmarks.md 「ネイティブの数値」 [V]), `I` indirect call.

| Path | Fast path | Floor / what lowers it |
|---|---|---|
| Unwind check after a non-tail call | 1 L8 + 1 B (today: 1 L + materialise `UNIT` + compare + 1 B) | Floor for return-value propagation. One compare fewer than today |
| Call that is a yield point | unwind check (1 L8 + 1 B), then EP's `yielding` (1 L + compare + `switch`) | Two fields, two checks; folding forgone |
| A yield passing a frame | 1 L8 + B (not taken), EP's check (taken), then the LIR slow path (κ closure, `yield_cont_take`, `yield_cont_set`) | 05 D-2 paid 2 L + 2 B here; now nothing beyond the hot path |
| An abort passing a frame | cold: `decref` per owned reference, `drop_token`, return | Per frame O(owned); a landing frame adds 1 L + 1 B + 1 C or 2 C |
| `yielding` / `yield_status` | 1 L + compare / 1 L + 1 L8 + 3 `select` | Inline |
| `perform E.op` | 1 L (`e:`) + C `ev_lookup` (walk: 1 L + compare per node, 1 `dup`) + C `ev_clause` (2 L, 1 `dup`) + 2 L (meta, code word) + 1 I + unwind check | Evidence stays out of generated code (N-24). Fusing and a per-site cache forgone |
| Handler install | 4 C + 1 C per op + 1 L (`e:`), then the body's `apply` and `call prompt$` | Allocation-bound (evidence, table, node) |
| `mask` wrapper | `symbol_value` + 3 C (`evv_get`, `evv_mask`, `evv_set`) on entry, `evv_set` on return | The list is static data, built once at load |
| Under-form clause | 2 C on entry (`evv_get`, `ev_evv`) + `evv_set`, `ev_take_state`; `ev_set_state`, `evv_set` on return | As the LIR states it |
| Over-application (`eml$apply$m`, `n < m`), per nested part | as today, with the abort test (1 L8 + 1 B) in place of the `target` test, plus the guard 1 L + compare + 1 B | The guard is the price of trapping a broken EP rule; not on the exact-apply path |
| Extern failure caught by `try_*` | cold: `eml$raise`, 1 C, 1 I | Not a hot path |
| Root prims | 1 C | REPL only |

### Optimisations forgone (question 2)

| Optimisation | Gain | Why forgone |
|---|---|---|
| Fold EP's `yielding` / `switch` into the unwind check | 1 L + 1 B per yield-point call | Needs recognising EP's statement shape |
| One entry for `ev_lookup` + `ev_clause` | about 8 instructions and 1 `dup` / `decref` pair per `perform` | A peephole over adjacent prims; a second body for the same work |
| Inline `ev_lookup` walk in generated code | about 2 C per `perform` | Roles in eml_native (N-24); a second, unverified lookup |
| Per-site `perform` cache | most of the walk | Invalidation by `evv` identity |
| `evv_get` / `evv_set` inline | 1 C each | `evv` would move into `Native`, and `evv_set` releases a list |
| One "install handler" entry | about 3 C per `handle` | Not a LIR prim; variable arity |
| No `catch_unwind` on the verified prim entries | none measured (`entry-plain` = `entry-shim`) | An exception list to re-argue per prim |
| Mask as static data with relocations to `e:` slots (no load-time fill) | one `DatumRow` kind fewer | The entry would rebuild a `[MaskItem]` on every call, or `evv_mask`'s verified signature would change |
| `prim dummy` as a load-time datum | 1 C on slow paths | The dummy is per heap |
| Fork evidence from existing operations (`ev_new` + `ev_set_clause` per fork, no shared fork table) | no `fork_ev` proof | One more allocation and `n_ops` writes per mirrored evidence on every fork; the frozen tail would still be built with role literals in eml_vm. It is the alternative of E-D2's stop rule only |
| `fork_evv` memo per worker | allocations per `par` | Unchanged from 05 §3.1 |

Trades against the VM (N-38 (c)): the VM's `fork_evv` goes through `fork_ev` + `evv_push`; in checked mode each push walks the list being built. Chosen for one constructor of evidence objects; the VM's fast mode allocates the same objects.

## 2. Rationale

- **E-D1.** The cost that made roles "the stage's largest risk" does not exist: plain consts verify without touching a proof [V]. Spec/const pairs would double the names for no gain. Crate-private placement makes N-24 hold by visibility, not by review.
- **E-D2.** What must be shared is the definition of an evidence object's fields, and that is eml_rt's. The walk-and-rank loop over `Forks` is engine orchestration; with one caller it has nothing to be shared with, and the table's entry words differ per engine (VM function numbers, native static closures). Building `ForkTables` in eml_sched now would design an interface against one user; jit-tasks can move a role-free 40-line function unchanged. 01 §5.3's jit-effects row asks only for "roles onto the `Ctx` surface with eml_vm and the eml_rt lemmas rerouted" [V].
- **E-D6.** `YieldState::wf` (verified) gives `target == () ==> !abort`, and `yield_abort` / `exit_abort` are the only writers of `abort = true` [V: `ctx.rs:86-93`, `1513-1596`]; so "aborting" is exactly the `abort` byte. Testing it makes the check cheaper than today's, removes 05 D-2's non-cold `split` block from every call site, and makes the `n < m` arm's check exact instead of "any target is treated as an abort".
- **E-D5.** A closure entry is an adapter from "closure + arguments" to the function; its signature is a function of the callee alone. Stating that once removes the clause special case from three places and keeps every `call_indirect` in the file that already owns "call a function value".
- **E-D7.** The service decides; the module helper only performs the call Rust cannot make (a `tail` callee). Putting all operands in `out` removes an entry and the state a second call would have to find. Fixed signatures make `eml$raise` the same code in every module, which "same name, same code" needs once loads repeat.
- **E-D4.** The `Image` is per-load data, so machine numbers may live there; code reads them through by-name symbols (00 R-6). eml_nrt needs no name table and no ordering rule. A mask as sized module data has no owner to manage and costs no load at the use.

## 3. Alternatives considered and rejected

- **Spec/const pairs or role accessor spec fns** (05 D-6): more names and trigger risk; plain consts already verify.
- **Roles in `layout.rs`**: public to eml_native.
- **`fork_evv` as a verified eml_rt operation**: a list-building loop with `path` invariants, comparable to `evv_mask`'s proof, for code whose callers are unverified anyway.
- **`fork_plan` / `ForkTables` in this stage** (05 D-6): no second user; see E-D2.
- **05 D-2 as written** (`target` test, then `abort` in a non-cold `split`): 2 L + 2 B on every frame a yield passes, one more compare on every call.
- **Abort flag folded into `target`'s low bits**: changes `YieldState::wf` and every lemma over `target`.
- **`eml_nrt_failed` calling the clause through a nested `eml_enter`**: a re-entering service (N-12 list) for a call generated code can make directly.
- **`out` of 2 words + `eml_nrt_raise_arg`** (05 D-4): an entry and a hand-over through the worker struct for values that fit the slot.
- **`eml$raise` arms emitted only for effects the program has** (05 D-4): the helper's code would depend on the program.
- **Clause lowering in its own file with the placement test widened** (the design-level plan §7): a second home for `call_indirect`.
- **Tag = parameter count for clause closures** (r1): an `apply` of one with the matching count would call a typed entry with the uniform signature; only the LIR verifier's `clause_values` [V: `crates/eml_lir/src/verify.rs:660-673`] excludes it, and `load` runs the verifier in debug builds only.
- **`eml$raise` signatures guarded by a debug assertion** (r1): a std edit would reach a mismatched `call_indirect` in release.
- **Fork constructors as `external_body` or outside `verus!`**: grows the trust list for objects whose shape verified operations rely on.
- **The `n < m` arm testing `target` instead of `abort`**: a yield in a non-last part would then be unwound as an abort and silently drop the remaining arguments.
- **`m:` as an 8-byte slot holding a pointer to a runtime-owned list**: an owner and a lifetime to manage, one more load.
- **Effect numbers assigned by eml_nrt at fill**: a name table in the runtime that must persist across loads.
- **Folding `child_abort` into `Form::Par`**: `Par` sorts first, so files counted under `async row` (via `fork_job`) would move.

## 4. Assumptions made

| # | Assumption | If false |
|---|---|---|
| A-2 [I] | `evv_push` accepts a fork evidence and a frozen tail in both modes (checked `checked_ev` passes fork evidence today through `ev_clause` in `par` UI tests) | `Ctx::fork_node(effect, table, depth, next)` replaces `fork_ev` + `evv_push` |
| A-3 [V for handle and `prompt$`, I for `resume$`] | I-land of E-D6 | the new verifier rule fails on the first program; the stage stops and goes back to design |
| A-8 [I] | No later LIR pass puts a call between the marked `apply` and `call prompt$` (the passes insert RC, `box` / `unbox` and tail marks, not calls) | the verifier rule, which runs after the passes in debug builds, reports it |
| A-9 [I] | The new `Heap` constructors and the two readers verify at the cost of their closest siblings | the stop rule of E-D2 |
| A-4 | The role rename changes no eml_rt machine code: the gate passes at 3 pairs, largest gated median +0.051 % [V: item 12]. `CTX_YIELD_ABORT` adds a const only [I] | N-29 decides |
| A-5 [V] | `panicked` has parameters `[Obj]` and `io_failed` `[Enum, Obj]`, and both are `never`; their entries are `(e, m) -> obj` and `(e, k, m) -> obj` (`vm.rs:4657-4680`, `4804`) | `load` returns `LoadError::Prelude` (E-D7) |
| A-6 [I] | Every `effects/`, `unwind/`, `panic/` UI file runs natively within 1 s and equals the VM | listed per file in the accept unit |
| A-7 [I] | `bench.rs` `RunStats` pins are unchanged by the VM reroute (`evv_push` counts no `evidence_steps` [V: `ctx.rs` counts only at 2963, 3144, 3259]) | kind 2, scope "`par` programs' `RunStats`" |

## 5. Implications for implementation

### Constraints
- Lowering units land before the accept unit; until then `unsupported` is unchanged and the new lowering is exercised by LIR-text codegen tests (`NativeMachine::load` does not call `unsupported` [V: 01a dossier "Other incidental facts"]). The UI snapshots change once.
- Every commit touching `crates/eml_rt` carries the N-29 gate table.
- Service or entry signature changes update docs/spec/native.md and all callers in the same commit (01 §1.4 procedure).

### Units (order; dependencies in brackets)
1. Plan: replace `docs/plans/2026-11-11-jit-effects.md`; add this design as an appendix and section G of 00; baseline counts.
2. eml_rt roles (E-D1) + `CTX_YIELD_ABORT`. Gates.
3. eml_rt fork constructors and readers (E-D2) with `trusted::` tests. Gates. [2] This is the stage's largest proof task: `lemma_fork_ev_ok` (about the size of `lemma_ev_new_ok`), plus the proofs of two immortal constructors. S2 may split it into `fork_ev` + readers and the `Heap` constructors.
4. eml_vm rerouted onto them (kind 3). [3]
5. Unwind check on `abort`, `if_aborting`, `define_apply` arm with its guard; the verifier rule and the `prompt$` test of I-land; CLIF pins re-pinned (kind 2) and a new pin `clif::an_over_application` of `eml$apply$2`. [2]
6. `e:` / `m:` data, `Image` fields, `EFFECTS` entries, inline `yielding` / `yield_status`, prim lowering (all but `child_abort`), root prims. Codegen tests, including: `yield_status` returns 0, 1, 2, 3 as `Ctx::yield_status` does; `root_set` then `root_get` round-trips through roots a stand-in thunk makes; `root_get` of an empty root and `root_set` of a set root end the run with the error the VM gives; two machines, each loading a program whose effect table lists the same effects in a different order, run the same function with the same result. [2]
7. Clause calls, the entry-signature rule and tag 0 (E-D5). Tests: in-place clause, under form, tail clause, a clause with a `float` operand; a static clause closure (`&g$entry`) has tag 0. [6]
8. Landings and `eml_nrt_landed`; `fill_cleanup` with a landing. Tests: `never` abort through 3 frames with owned values and a token landing at `prompt$` (census); resumable yield through 3 frames; multi-shot; mask; an abort passing a handler frame releases its state before the run ends (stand-in thunk + `Heap::live_objects`); CLIF pin `a_yield_point_call`. [5, 7]
9. `eml$raise` / `eml_nrt_failed` (E-D7), `LoadError::Prelude`. Tests: a program whose `Prelude.Panic` has two operations, whose `panicked` takes `(obj, obj)`, or whose `panicked` returns a value, is refused by `load`; `try_io` sees the same `IOErrorKind` as the VM for `NotFound` and `PermissionDenied`; a program without `Prelude.Panic` in its effect table that overflows ends with the VM's text (the `NO_EFFECT` path); `try_panic` around inline overflow, an index row, the `panic` row, a C-row narrowing panic; `try_io` around a failing open; `Stopped` never caught; purity filter (a `Pure` row's fault is not caught); no handler gives the VM's text; `Faults` and `Never` cleanups run after `eml$raise` returns. [7, 8]
10. Accept: `Form` (E-D9), `unsupported` tests, native UI snapshots, CLI text, status.md. [6-9]
11. Close: benches (E-D10), docs check, plan file deleted.

Units 3-4 are independent of 5-9 and may run after them.

### Candidate completion criteria
- `nix develop -c cargo verus verify -p eml_rt` prints `verification results:: N verified, 0 errors` with N >= 1250, no warning.
- `grep -nE '(scanned_word|word_at|scanned_at|set_passes|field|set_field|lemma_readable_scanned)\((e|ev|node|cp|last|cs\[k\]|cs1\[k\]), [0-2][,)]' crates/eml_rt/src/heap/ctx.rs crates/eml_rt/src/heap/ctx/proof.rs` -> no output.
- `grep -rlE '\b(EV_TABLE|EV_STATE|EV_OUTER|EV_WORDS|NODE_EV|NODE_NEXT|NODE_WORDS)\b' crates | grep -v '^crates/eml_rt/'` -> no output.
- `grep -cE 'set_field|alloc_immortal|data_first' crates/eml_vm/src/compile/forks.rs` -> `0`.
- `awk '/^fn fork_evv/,/^}/' crates/eml_vm/src/vm.rs | grep -cE 'set_field|scanned\(|alloc_data|data_first|\.meta\('` -> `0`; the same for `fn child_abort`.
- `grep -rl 'call_indirect' crates/eml_native/src | sort` -> `crates/eml_native/src/lower/apply.rs`, `crates/eml_native/src/thunk.rs`.
- `grep -l 'mem::yield_abort(' crates/eml_native/src/lower/*.rs | sort` -> `crates/eml_native/src/lower/effects.rs`, `crates/eml_native/src/lower/unwind.rs`.
- `cargo nextest run -p eml_native -E 'test(clif::a_call_and_its_unwind_check)'` passes with a snapshot that has no load at offset 0 of `ctx` (`CTX_YIELD_TARGET`) and one `i8` load at offset 24.
- `cargo nextest run -p eml_lir -E 'test(prompt)'` and `cargo nextest run -p eml_core_ir -E 'test(prompt)'` pass and include the two I-land tests.
- `grep -rn 'debug_assert\|cfg!(debug_assertions)' crates/eml_native/src/jit.rs | grep -ci 'prelude'` -> `0` (the Prelude check is unconditional).
- `grep -rn 'eml_nrt_raise_arg\|RAISE_DONE' crates docs/spec` -> no output.
- `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` -> exit 0.
- `grep -cE 'Form::(Prim|Clause|Prompt)' crates/eml_native/src/unsupported.rs` -> `0`.
- `cargo nextest run -p eml_cli --test integration -E 'test(ui::run_native) | test(ui::run_fail_native)'` passes with snapshots whose `rejected` keys are a subset of `async row`, `blocking c`, `par`.
- For each entry file under `tests/ui/run/{effects,unwind,panic}` and `tests/ui/run-fail/panic`: `timeout 20 eml run --native <file>` prints no line containing `native code does not support` (0 of 111 files; today 63 are rejected).
- `bench/native.sh` output has a number, not `rejected`, in the `ep_float` row.
- `cargo nextest run -p eml_native` passes, including `clif::a_yield_point_call` whose snapshot shows one `load.i8` at offset 24 before the `brif`, the `target` load only in EP's check and the cold landing block.
- Miri `trusted::` under both settings within the budgets of `crates/eml_rt/CLAUDE.md` (+5 s per added test under Tree Borrows).
- `bash nix/crate-graph.sh --check` passes with no table edit (no new edge, feature or re-export).

### Test-change kinds
- Kind 1: `unsupported::prim_is_rejected`, `clause_is_rejected`, `prompt_is_rejected` deleted; `forms_are_sorted_and_distinct` rewritten for the four forms; `child_abort_is_rejected` added.
- Kind 2: the `eml_nrt` / `eml_native` tests that pin "a function value of arity 0 is an `Internal` error" (`fill_data`) change to accept 0; CLIF pins showing the call check (`a_call_and_its_unwind_check`, `an_apply_site`, any other containing the `target` compare); the two native UI snapshots; `eml_nrt` entry / descriptor counts; the `CTX_*` pin test gains one value; VM bench tables at Close.
- Kind 3: the eml_vm reroute; the eml_rt rename.
- Not changed: `placement::indirect_calls_are_only_in_apply_and_thunk`.

### docs/spec sections (same commit as the rule)
- spec/native.md: new 「エフェクト」 (prim table, `e:` / `m:`, machine numbers, `Image` fields); 「関数の値」 (entry-signature rule, tag 0, `Call::Clause`, the `n < m` guard); 「巻き戻しの地点」 (abort test, landing, `eml_nrt_landed`, the invariant A-3); 「externの失敗」 (classes, order, `out`); 「Image」 (`DATUM_EFFECT`, `DATUM_MASK`, `MaskRow`, the two effect fields); 「断る形」; 「値と定数」 (data sizes).
- spec/object-model.md: 「evidence の3つの物体」 (role names, fork constructors), 「基本操作の契約」 (new rows), 「欄」 (`CTX_YIELD_ABORT`).
- spec/vm.md 「`par` の fork と join」 (which eml_rt operation builds each object).
- spec/lir.md 「verifier」 and 「着地の印」 (the `prompt$` rule); spec/core-ir.md 「handle と `prompt$`」 (I-land as an invariant EP keeps).
- implementation: architecture.md (`eml_nrt` `effects.rs`), testing.md, status.md, benchmarks.md; docs/future/native.md 「中断と実行時エラー」 shrunk to what remains.

### Left for jit-tasks / jit-repl
- jit-tasks: (a) move `Forks`, `ForkTable`, `fork_evv` to eml_sched and call them from `eml_nrt_par`; build native fork tables with `Heap::alloc_fork_table` over `fn_value` words inside `constants::build`; (b) `eml_nrt_failed` step 2 gains the request branch, `RAISE_DONE` and the `Pending -> Aborting` transition, with no signature or `eml$raise` change; (c) `eml_nrt_landed` gains the limit-class restore; (d) `child_abort` lowers to `eml_nrt_child_abort` using `Ctx::fork_of`; `Form::ChildAbort` goes with `Unsupported`; (e) the `n < m` arm's safepoint issue of 05 §5.2 is untouched here.
- jit-repl: root prim entries exist; `NativeMachine.effects` already assigns by name across loads; `eml$raise` is program-independent.

## 6. Unresolved items

| # | Item | Default |
|---|---|---|
| U-1 | Exact accepted / rejected counts after the stage | measured in the accept unit; the plan states only "keys are a subset of `async row`, `blocking c`, `par`" and 0 rejected in the three categories |
| U-2 | `eml_nrt_landed` now, or `eml_rt_dummy` at the landing until jit-tasks | now (jit-tasks then changes a body, not the lowering or a CLIF pin) |
| U-4 | Number of Miri `trusted::` tests added | one per new constructor / reader (5), within +25 s under Tree Borrows |
| U-6 | Whether unit 3 is one commit or two | S2 decides; each commit passes the gates on its own |
| U-5 | Whether units 3-4 precede the native units | eml_rt units first (2, 3), so later commits do not re-run the gate |

## 7. Additional checks

1. [V] Baseline Verus at be1b1de5 in the scratch clone: `cargo verus verify -p eml_rt` -> `verification results:: 1243 verified, 0 errors`, real 46.77 s, user 194.89 s (includes building vstd).
2. [V] Role rename experiment: `python3 -I roles.py ctx.rs ctx/proof.rs` (220 replacements + `scanned_at` / `set_passes`), seven consts added before `ev_ok`; same command -> `1250 verified, 0 errors`, real 28.16 s. No proof text edited. Diff: `s1-scratch/roles.patch` (1781 lines). verusfmt and the bench gate: items 10 and 12. Miri was not run.
3. [V: read] `ctx.rs`: `YieldState` and `wf` (73-93), `yielding` / `yield_status` (1385-1424), `start_yield`, `exit_abort` (1571-1596), `ev_ok` / `node_ok` / `path` (330-362), `ev_clause`, `ev_renew` (outer = current `evv`, so it cannot build a fork evidence), `ev_new`, `evv_push`, `evv_mask` (no order precondition), `evidence_steps` writers.
4. [V: read] `crates/eml_vm/src/vm.rs:1690-1745` (unwind order), `1905-1965` (`fork_evv`); `compile/emit.rs:703-730` (mask sort is the VM's own canonical order); `compile/mod.rs:236-250`.
5. [V: read] `crates/eml_native/src/lower/unwind.rs` (whole), `mem.rs` function list (`spill` exists for the `out` slot), `crates/eml_nrt/src/run.rs` `fill_data`.
6. [V: read] `crates/eml_native/src/lower/data.rs:137-261`: `unbox` and `release unboxed` already handle a dummy, so a yield's LIR dummy on the hot path is covered.
7. [V: grep] `crates/eml_lir/src/verify.rs` has `clause_values` and the per-function clause ABI check; `syntax.rs` has `effect_text`, `mask_text`.
8. [V: read] 01 §1.3, §1.4, §5.3 rows, N-4..N-43 as listed in the briefing; 00 A-F; 05 §1.0-§7; core-ir.md 「evidence passing」; lir.md 「ダミーの値」, 「`evv` の不変条件」; vm.md 「`par` の fork と join」.
9. Not run: any native prototype; Miri; the histogram after the stage.
10. [V] Size rename: 15 sites (`data_obj`, `alloc_data`, `Meta::new`, `scan_of`, `count_of`, `bits_scan`) on top of item 2 -> `1250 verified, 0 errors`, real 28.01 s. `verusfmt --verus-only --check` on the scripted output: 49 diff lines; after `verusfmt --verus-only` on the two files: `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` exit 0. Committed in the scratch clone as 8ff26d91.
11. [V] `fork_ev` prototype (`s1-scratch/fork_ev-prototype.rs`, weak contract: `wf`, frame, fast-mode `Ok`): iteration 1 failed `lemma_arg_moved`'s `held_word` and `set_field`'s `word_in` of the immediate; iterations 2-4 fixed both with `lemma_arg_counted` + `lemma_points_in` and `lemma_imm_round_trip`; the last run reports one error, `postcondition not satisfied: final(self).wf()` at the end of the body. Not carried further.
12. [V] `bench/rt-gate.sh be1b1de5 3` in the scratch clone at 8ff26d91 (the index and size rename, verusfmt applied): exit 0; 31 gated rows `ok`, medians -0.029 % .. +0.051 %; sum of gated deltas +185621 instructions; log `s1-scratch/rt-gate.log`. Three pairs, not the gate's five.
13. [V: read] `crates/eml_vm/src/vm.rs:4109-4175` (`failed`, `abort_to`), `4205-4221` (the `panic` row), `crates/eml_vm/src/ffi.rs:408-424` (C `raise`), `vm.rs:4655-4680`, `4797-4804` (effect lines and clause entries), `crates/eml_vm/src/compile/emit.rs:1048-1053`, `crates/eml_rt/src/externs.rs:231-245` (`io_error_kind -> Option<i64>`), `crates/eml_core_ir/src/evidence/handlers.rs:636-657`, `helpers.rs:60-136`, `crates/eml_native/src/jit.rs:37-45`, `215-221` (`LoadError`, the debug-only verify).

## 8. Conformance criteria

| ID | Rule | Why | Applies to | How to check |
|---|---|---|---|---|
| G-1 | MUST: evidence and node field indices and sizes are written only as the seven consts in `eml_rt/src/heap/ctx.rs`. MUST NOT: export them or name them outside eml_rt | one definition of the roles | jit-effects on | the two greps of section 5 |
| G-2 | MUST NOT: eml_native, eml_nrt, eml_sched or eml_vm read or write a field of an evidence, node, entry table or fork table; they call `Ctx` / `Heap` operations | N-24 by construction | all stages | `mem.rs` exports no evidence accessor (review of one file); the eml_vm greps of section 5 |
| G-3 | MUST: a call unwind point's hot path is one byte load of `CTX_YIELD_ABORT` and one branch, emitted only by `unwind::if_aborting`. MUST NOT: load `target` on that path or copy the check | cost model; one rule | jit-effects, jit-tasks (`par` site) | CLIF pins; `grep -l 'mem::yield_abort(' crates/eml_native/src/lower/*.rs` lists `effects.rs` and `unwind.rs` only |
| G-4 | MUST: any EP or LIR-pass change keeps I-land (E-D6) and "a yield returns only through a LIR yield point, a tail call or a `prompt` apply". MUST: the safepoint service never returns an unwinding action while a yield or abort is in flight | G-3 is only correct under them | eml_core_ir, eml_lir changes in any stage; jit-tasks' safepoint | the verifier rule and the `prompt$` test of E-D6; `cargo nextest run -p eml_native -E 'test(effects::an_abort_lands_and_the_clause_returns)'` |
| G-5 | MUST: a landing frame's cold block is: landing test, then (`eml_nrt_landed`, continue) or (`ev_take_state`, `decref`, owned, tokens, return), in the VM's order | dtor order equals the VM (N-43) | jit-effects, jit-tasks | CLIF pin; the state-release test of unit 8 |
| G-6 | MUST: generated code gets an effect number only by loading `e:<effect>` and a mask only as the address of `m:<mask>`. MUST NOT: a machine or program effect number as an immediate | 00 R-6 | jit-effects, jit-repl, native-aot | a codegen test loads two programs whose effect tables differ in order and runs the same function; CLIF pin shows no effect immediate |
| G-7 | MUST: machine effect numbers are assigned by name in `NativeMachine` and reach eml_nrt only through `Image` (`DatumRow.bits`, `MaskRow.effect`, `panic_effect`, `io_effect`) | eml_nrt learns a program only from `Image` | jit-effects, jit-repl, native-aot | review; `grep -rn 'Prelude\.' crates/eml_nrt/src` -> no output (empty today [V]) |
| G-8 | MUST: a non-inline prim's entry is `eml_rt_` + `Prim::name()`, a `shim` whose body is one call of that `Ctx` method. MUST NOT: a lowering-side table of entry names | one definition per prim | jit-effects, jit-tasks (`child_abort` is a service, the stated exception) | a test iterates `Prim::KINDS` and checks symbol and descriptor against `Prim::row()` plus the immediates |
| G-9 | MUST: every closure entry's signature is derived from its function by the one rule; a clause call's signature from the operation table, and their equality is asserted in every build; a clause closure has tag 0; `call_indirect` only in `lower/apply.rs` and `thunk.rs` | N-37 unchanged | jit-effects on | the placement test, unchanged; `grep -c 'clause' crates/eml_native/src/lower/apply.rs` shows no branch in `ClosureEntries::id` (review) |
| G-10 | MUST: `eml$raise`'s code is the same for every program; all classification is in `eml_nrt_failed`; a class other than `RAISE_PANIC` / `RAISE_IO` makes no call. MUST NOT: change `eml$raise` or the service's signature to add re-raise requests | "decisions in shared routines"; jit-tasks readiness | jit-effects, jit-tasks | CLIF pin of `eml$raise`; jit-tasks' diff touches `services.rs` only for this |
| G-11 | MUST: on `RAISE_PANIC` / `RAISE_IO` the service has written exactly the operands of that class to `out` and owns none of them afterwards; on any other outcome it has written nothing. MUST NOT: leave raise operands in the worker struct | no hand-over state across entries; no leak or double release | jit-effects, jit-tasks | census in unit 9's tests (caught and uncaught) |
| G-12 | MUST: caught and uncaught outcomes of an extern failure equal the VM's (`failed`, `raise_again`) for every purity | N-43 | jit-effects | unit 9's `agree` tests |
| G-13 | MUST: fork tables, the `Panic` tail and fork evidences are built only by eml_rt's constructors; the `evv` walk uses `evv_step`. MUST (jit-tasks): `fork_evv` moves to eml_sched as is and both engines call it | N-24 "one implementation" | jit-effects, jit-tasks | section 5 greps; jit-tasks: `grep -rn 'fn fork_evv' crates` -> one hit, in eml_sched |
| G-14 | MUST NOT: a new `Native` field, thread-local or eml_sched edge in this stage | N-5; crate graph | jit-effects | `nix/crate-graph.sh --check`; `size_of::<Native>() == 72` assert unchanged |
| G-15 | MUST: each commit touching `crates/eml_rt` passes Verus, verusfmt, Miri `trusted::` (two settings), `bench/rt-gate.sh` and carries the table | N-29 | jit-effects | commit messages |
| G-16 | MUST: after the stage `Form` is exactly `Par, AsyncRow, BlockingC, ChildAbort`, and no file of `effects/`, `unwind/`, `panic/` is rejected or marked `native: skip` | completion condition | jit-effects | snapshots; `grep -rln 'native: skip' tests/ui/run/effects tests/ui/run/unwind tests/ui/run/panic tests/ui/run-fail/panic` -> no output |
| G-17 | MUST: a check that keeps a `call_indirect` signature or a raw-memory size right runs in release builds (the Prelude shape at `load`, the clause-entry signature assert, the tail-clause result assert, the non-empty mask assert, the I-land shape after a marked `apply`, `alloc_fork_table`'s immortality check). MUST NOT: rely on `debug_assert!` or on the debug-only `eml_lir::verify` for them | an unsafe boundary a std or compiler edit can reach | jit-effects on | `grep -n 'LoadError::Prelude' crates/eml_native/src/jit.rs` is outside any `cfg!(debug_assertions)` block (review of `load`); unit 9's refusal tests run in `--release` too (`cargo nextest run -p eml_native --release -E 'test(/::prelude_/) | test(release_check)'`) |
| G-18 | MUST: the new eml_rt operations are verified inside `verus!` with the contract of E-D2. MUST NOT: `external_body`, `assume`, or a move out of `verus!` to pass the gate | the shape of evidence objects is what verified operations assume | jit-effects | `git diff <stage base> -- crates/eml_rt | grep -cE '^\+.*(external_body|assume\()'` -> `0`; `src/trust.rs` unchanged |

## 9. Amendments (approved 2026-11-14; recorded in 00 section G.1)

Appendix 01:
- §1.4 table, row "Unwind point that is a call", later-stage cell: "behind the same test, the `abort` load; ... That side is not marked cold" -> "jit-effects replaces the test: one byte load of `yielding.abort` and a cold branch; a yield does not branch and returns on the hot path; the cold block tests the `landing` against `yielding.target`".
- §1.4 table, row "Unwind point that is a call", jit-core cell: "load `yielding.target`, compare, cold branch" -> unchanged for jit-core; superseded from jit-effects by the line above.
- §1.4 table, row "Faulting row", later-stage cell: "when `eml_nrt_failed` returns a clause, call it with the clause ABI and return" -> "when `eml_nrt_failed` returns a class, call the clause it wrote to `out` with that class's fixed signature and return".
- §1.4 "Evidence roles": "the fork-evidence builders (`fork_evv`, the `Panic` tail, the re-raise mask) become one implementation used by eml_vm and eml_nrt" -> "jit-effects moves the construction and reading of fork-evidence objects into eml_rt; the `fork_evv` loop and the re-raise mask move to eml_sched in jit-tasks, with their second caller".
- N-24 check: "rt-layout G-10's grep for role literals in eml_vm is empty after jit-effects" -> the same, plus "the role consts are crate-private to eml_rt; `fork_evv` has one definition from jit-tasks on".
- N-9 (as 05 D-2 applied it: "exactly one `target` load per call point; `abort` only after it") -> "exactly one `abort` byte load per call point; `target` is loaded only by inline `yielding` / `yield_status` and in a landing's cold block".
- §1.3 "Function values": "tag = the number of argument leaves still missing" -> the same, plus "a clause closure has tag 0 and is never an `apply` target; its entry has the operation's signature".

Appendix 00:
- C-2: "05's extra `out` parameter for jit-effects is added on top of it" -> the same, with "`out` is a 4-word slot `[clause, ev, a0, a1]`; there is no `eml_nrt_raise_arg`".
- C-6: "jit-effects adds `m:<mask>`" -> "jit-effects adds `e:<effect>` (8 bytes, machine number) and `m:<mask>` (`eml_nrt::image::mask_bytes(n)` bytes, `MaskItem`s); a data object's size is its datum's, no longer always 8 bytes".
- C-14: "jit-effects lifts the assert and types the entries of clause functions with the op's signature" -> "jit-effects lifts the assert; every closure entry's signature is derived from its function (equal to `uniform(n)` for non-clause functions); `apply::arity` gives a clause function 0".
- C-13: "calls `eml$apply$(m-1)` and tail-calls `eml$apply$1`" -> the same, with "between them it tests the `abort` byte (cleanup) and then traps if `yielding.target` is set".
- E-2: "the const arrives with its first reader in generated code" -> the same, with "the first reader is the call unwind check".
- E-3: settled: `m:` shape as in C-6 above; the two prelude effect numbers are `Image.panic_effect` / `Image.io_effect` (01 §1.3 as written).
- E-6: settled by the C-14 line above.
- F.2 N-37: no change ("`call_indirect` in eml_native only in `lower/apply.rs` and `thunk.rs`"); the design-level plan's kind-2 change to the placement test is withdrawn.
- New section G records E-D1..E-D10 as overriding 05 D-1..D-6; in particular 05 D-6's `fork_plan`, `ForkTables`, `fork_evv` in eml_sched and `evv_effects` are withdrawn from jit-effects.

Soundness notes (no advisor question is open; each item states what holds in release builds):
- `call_indirect` signatures. `eml$raise` uses two fixed signatures; `load` refuses, in every build, a program whose `Prelude.Panic` / `Prelude.IO` operations differ (`LoadError::Prelude`), and `ClosureEntries::id` asserts in every build that a clause entry has its operation's signature. A clause closure has tag 0, so a generic `apply` of one traps instead of calling the typed entry.
- `out` (E-D7) and `m:` data (E-D4) are raw memory shared between generated code and eml_nrt, reachable only from generated code and `load`, like the `argv` of `eml_nrt_pap` and the datum slots. `m:` objects have `n >= 1` items (asserted in lowering), are written before the first entry of their load and are read-only afterwards.
- E-D6, abort side. It rests on the verified `YieldState::wf` and on I-land. If I-land were broken, a frame would take its cold block once, release what its unwind point lists and return; no frame runs after its cleanup, so nothing is released twice. The result of the call that returned is leaked, and the abort reaches the bottom of the task as `Internal("an abort reached the bottom of the task")`. I-land is a rule whose violation is undefined in a release build: a call between the landing and `prompt$` runs with the abort set, and a `yield_to` there clears it, so the landing frame would go on with the dummy as a value. The first half is checked by the verifier rule (debug builds, widened to externs and the yield prims) and by an assert in the lowering in every build; the second half (the fixed body of `prompt$`) by a unit test. Only the `prompt$` half remains undefined in a release build if the body were edited.
- E-D6, yield side. "A yield returns only through a LIR yield point, a tail call or a `prompt` apply" is EP's existing contract; code that ran past a yield with a dummy would read a `BigInt` as another object. The VM's fast mode and 05 D-2 depend on it in the same way, and E-D6 adds no new dependence. The `n < m` arm and `eml$raise` (E-D7), the two places where native code itself relies on a part of it, trap on a violation.
- Fork evidence. `alloc_fork_table` refuses a non-immortal entry in both modes, so a frozen table cannot hold a reference that dies; the constructors stay verified (G-18).
- The debug-only `eml_lir::verify` in `NativeMachine::load` is unchanged: release builds trust the LIR the pipeline produced, as for every other rule.
