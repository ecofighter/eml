# Stage jit-effects: effect operations run natively; evidence roles and fork evidence live in eml_rt

- Created: 2026-11-11; refreshed into this executable plan: 2026-11-14
- Base: main @ be1b1de5 (jit-externs closed). Every `file:line` below describes that commit. Line numbers are hints; the named item decides.
- Status: plan finalized. Every item of section 10 has a default. This file is in English by the user's decision (00 G.1, U-H); `docs/`, code comments and CLAUDE.md stay as CLAUDE.md requires (Japanese via PLaMo, no space between Japanese and Latin characters or inline code).
- Appendices (English, `docs/plans/2026-11-11-jit/`): `00-decisions.md` wins over all others (its section G records this refresh: G.1 the user-approved amendments, G.2 this plan's resolutions); `08-effects.md` is this stage's design (E-D1..E-D10, criteria G-1..G-18; it supersedes 05 D-1..D-6 and D-18 for this stage); `01-contract.md` is the long-term contract. This plan is self-contained for implementation; 08 holds the full design text (each unit cites its section). Where 08 and this plan differ, 00 G.2 and this plan win.

## 1. Goal and success criteria

- Goal: native code runs every effect operation: the effect prims (`yielding` / `yield_status` inline, the rest as `eml_rt_<prim>` entries), clause calls, abort landings at `prompt`, the yield slow path, and handler calls for extern failures (`try_panic`, `try_io`, also around C-row panics). The fields of evidence and evv nodes are named once in eml_rt, and fork evidence is built only by eml_rt operations, which the VM already uses.
- Success criteria (checks in section 9):
  - S-1: the native snapshots equal 7.5's bodies: `run` accepted 489 / rejected 151 (async row 129, par 22) / unlowered 27; `run-fail` accepted 45 / rejected 34 (async row 33, par 1). No file of `run/{effects,unwind,panic}` or `run-fail/panic` is rejected or marked `native: skip`.
  - S-2: `Form` is exactly `Par, AsyncRow, BlockingC, ChildAbort` (G-16).
  - S-3: every VM expected value is byte-identical (UI outputs, REPL transcripts, `RunStats` snapshots), except the V1 contingency of 7.4.
  - S-4: eml_rt passes Verus (`N verified, 0 errors`, N > 1250 after R3), verusfmt, Miri `trusted::` (two settings) and `bench/rt-gate.sh` on each commit touching it (G-15); no `external_body` / `assume` added (G-18).
  - S-5: every standing check of section 7.3.2 holds from the unit named.
  - S-6: `bench/native.sh` has numbers for `ep_float` and `nbody`; only `par_tree` stays `rejected`; native and VM bench tables replaced at Close.
  - S-7: CI is `success` on every job for the Close commit (CP-Z).

## 2. Scope

- In scope: the units of section 7; the docs each unit names; 00 section G.
- Out of scope (owner):
  - `par`, `child_abort` natively, `compute`, async rows, `blocking` C, limit classes, re-raise requests (`Propagate`, `AbortStarted`, `RAISE_DONE`), moving `Forks` / `ForkTable` / `fork_evv` to eml_sched (`fork_plan` / `ForkTables`), the restore of the limit class in `eml_nrt_landed`: jit-tasks (08 §5 "Left for jit-tasks / jit-repl").
  - The native REPL and its use of the root prims: jit-repl.
  - Optimisations forgone by 08 (fusing `ev_lookup` + `ev_clause`, a per-site `perform` cache, inline `evv_get` / `evv_set`, folding EP's `yielding` check into the unwind check; 08 §1 "Optimisations forgone"). Do not add them.
  - Any new `Native` field, thread-local, crate edge, feature or re-export (G-14).

## 3. Background context

All facts at be1b1de5 [V: fact dossiers of the refresh].
- Native today: `run` accepted 336 / skipped 6 / rejected 304 (async row 129, par 22, prim 153) / unlowered 31; `run-fail` accepted 43 / skipped 2 / rejected 36 (async row 33, par 1, prim 2) (`crates/eml_cli/tests/snapshots/integration__ui__run_native.snap`, `..._run_fail_native.snap`). The histogram key is the first `Form` of `eml_native::unsupported` (`crates/eml_native/src/unsupported.rs:11-30`; order `Par, AsyncRow, BlockingC, Prim, Clause, Prompt`); only `Prim::EmptyMutArray` is lowered.
- `crates/eml_native/src/lower/unwind.rs`: `After::{Call, Faults, Arith, Never}`; `After::Call` loads `yielding.target`, compares with `UNIT` and branches to a cold cleanup; `fill_cleanup` asserts `point.landing.is_none()`. `define_apply`'s `n < m` arm (`lower/apply.rs:372-386`) hand-copies that check.
- `eml$raise(ctx, function, site, purity)` only calls `eml_nrt_failed`, which always records and returns `Err(Fatal)` and ignores `purity` (`crates/eml_nrt/src/services.rs:51`). `Worker = {image, by_code, by_pap, exit, panic, error}`.
- `apply::arity` asserts `function.clause.is_none()` (`lower/apply.rs:65-89`); every closure entry is `uniform(n)`.
- Data symbols `s:`, `i:`, `x:`, `v:`, `a:mut`: 8-byte zero-init slots, `DATUM_*` kinds 0..4 (`crates/eml_nrt/src/image.rs:98-130`), filled at load by `fill_data` (`crates/eml_nrt/src/run.rs:135-176`).
- eml_rt: evidence `[table, state, outer]` and node `[ev, next]` fields are literal indices in 42 functions of `crates/eml_rt/src/heap/ctx.rs` and `heap/ctx/proof.rs` (`ev_ok`, `node_ok`, `path` at ctx.rs:330-362). `YieldState { target, clause, cont, abort: bool }` is `#[repr(C)]`; `CTX_*` consts: `CTX_YIELD_TARGET, CTX_FAULT, CTX_STOP, CTX_STACK_LIMIT, CTX_JMP` (ctx.rs:3816-3826, pinned at `heap/tests/ctx.rs:938`), no `CTX_YIELD_ABORT` (the `abort` byte is at 24). `evv_mask` takes `&[MaskItem { effect: u16, keep: u32, skip: u32 }]` and needs no order.
- VM: `fork_evv` (`crates/eml_vm/src/vm.rs:1912-1970`) and `child_abort` (:2490-2527) build and read fork evidence with literal indices; `compile/forks.rs` builds the fork tables and the frozen `Panic` tail. `fork$` / `reperform$` are generated in `crates/eml_core_ir/src/evidence/forks.rs`. The VM gives an engine panic (the `panic` row, a C-row panic) to `abort_to(Panic)` with no purity filter (`vm.rs:4212-4221`, `ffi.rs:410-424`).
- Verus at be1b1de5: `1243 verified, 0 errors` (47 s wall, macOS M-series).

## 4. Prerequisites

| Needed | For | How to check | If missing |
|---|---|---|---|
| The dev shell (Rust 1.99.1, nextest, insta, clippy) | every unit | `cargo --version` → `cargo 1.99.1`; `cargo nextest --version` exit 0 | stop |
| Verus and verusfmt (default devShell, macOS or x86_64 Linux) | R1, R2, R3, R4, N1 (EG) | `cargo verus --version` exit 0; `verusfmt --version` exit 0 | stop: eml_rt units cannot be gated |
| The `.#miri` shell | EG, V1, N2..N6 | `nix develop .#miri -c cargo miri --version` exit 0 | stop |
| macOS (for `bench/rt-gate.sh`, `bench/*.sh`) | EG, Z | `uname` → `Darwin` | rt-gate: stop; Z: `Measure jit-effects` commit later (K-9) |
| The `.#cross` shell | checkpoint extras | `nix develop .#cross -c rustc --print target-list \| grep -c x86_64-pc-windows-msvc` → 1 | stop before CP-B |
| `python3` | R1 (the rename script) | `python3 --version` exit 0 | stop |
| `plamo-translate` (MCP tool or CLI) | Japanese docs and comments | `plamo-translate --help` exit 0 or the MCP tool listed | use the `natural-japanese` skill (CLAUDE.md) |
| `gh` authenticated, push permission | checkpoints | `gh auth status` exit 0 | ask the user |
| The three plan files of S0 present, uncommitted, at the base | S0 | `git status --porcelain docs/plans` lists `08-effects.md`, `00-decisions.md`, `2026-11-11-jit-effects.md` | stop: the plan is incomplete |

## 5. Decisions

### 5.1 Long-term guidelines (changing these requires re-examination)

The design is 08 (approved by the user 2026-11-14 with its section 9, recorded in 00 G.1). If implementation conflicts with any of it, stop and report; do not redesign. A soundness or memory-safety problem goes to a strong advisor before the fix (CLAUDE.md).

Decisions (full text: 08 §1, one subsection per ID; cost model and forgone optimisations: 08 §1 end; rationale and rejected alternatives: 08 §2, §3):

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

Conformance criteria (verbatim from 08 §8; "unit 8 / unit 9 / section 5" there mean this plan's N5 / N6 / section 7.3; every unit's review checks the criteria whose "Applies to" covers it):

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
| G-17 | MUST: a check that keeps a `call_indirect` signature or a raw-memory size right runs in release builds (the Prelude shape at `load`, the clause-entry signature assert, the non-empty mask assert, `alloc_fork_table`'s immortality check). MUST NOT: rely on `debug_assert!` or on the debug-only `eml_lir::verify` for them | an unsafe boundary a std or compiler edit can reach | jit-effects on | `grep -n 'LoadError::Prelude' crates/eml_native/src/jit.rs` is outside any `cfg!(debug_assertions)` block (review of `load`); unit 9's refusal tests run in `--release` too (`cargo nextest run -p eml_native --release -E 'test(/::prelude_/) | test(release_check)'`) |
| G-18 | MUST: the new eml_rt operations are verified inside `verus!` with the contract of E-D2. MUST NOT: `external_body`, `assume`, or a move out of `verus!` to pass the gate | the shape of evidence objects is what verified operations assume | jit-effects | `git diff <stage base> -- crates/eml_rt | grep -cE '^\+.*(external_body|assume\()'` -> `0`; `src/trust.rs` unchanged |

Kept in the repository: each rule is written into `docs/spec/` (native.md, object-model.md, vm.md, lir.md, core-ir.md) by the unit that implements it (section 7 names the sections); G-13's and G-10's jit-tasks halves stay in 08 until jit-repl's Close deletes the appendix directory, by which point jit-tasks has written them into spec.

### 5.2 Short-to-mid-term design

00 section G.2 (the resolutions of this plan, consistent with the approved amendments) settles: `CTX_YIELD_ABORT` arrives in N1 with the call unwind check; N-9's wording includes the `n < m` guard's `target` load; the clause-signature assert runs in `lower::program`'s closure loop, not in `ClosureEntries::id`; the heap constructors' `requires`; the one helper `lower::effects::entry(prim)`; `Prim::KINDS` as an associated const; the T8 Miri calls instead of new `trusted` tests; R2 / R3 as two commits; `eml_nrt_landed` now; the rename script also fixes `proof.rs:524`. Read G.2 before R1.

## 6. Implementation constraints and conventions

- One unit = one commit; each passes G (7.3.1) and the SC lines in force (7.3.2). Commits touching `crates/eml_rt` also pass EG (7.3.3) and carry its table in the message (G-15).
- TDD: write each unit's new tests first and watch them fail where the unit adds behaviour.
- `docs/spec/` sections are written in the commit that implements the rule; code comments cite `docs/spec/` / `docs/implementation/` only, never `docs/plans/` or a plan ID; docs name no stage.
- Japanese text (docs, comments): draft in English, translate with PLaMo (`form: plain`, identifiers and section names in `glossary`, earlier parts of the same document as `history`), review; no space between Japanese and Latin characters or inline code; remove such spaces from any paragraph you edit.
- Test changes are only those of section 7.4 (kinds 1-3, testing.md 「テストの変更の運用」); another change of an expected value stops the unit.
- No `native: skip` mark is added (G-16). A disagreement found in N7 becomes a `Fix native <topic>` commit with a regression test (K-4).
- Release-mode checks for `call_indirect` signatures and raw-memory sizes (G-17); no `debug_assert!` for them.
- Run ad-hoc `.em` programs under `timeout 20`. Stop any Bash call moved to the background before finishing a unit.

## 7. Implementation tasks

"08" = `docs/plans/2026-11-11-jit/08-effects.md`; "00" = `docs/plans/2026-11-11-jit/00-decisions.md`. Tags: [V] verified at the refresh (how), [I] inferred.

### 7.1 Task table

| ID | Commit title | After | Decisions / criteria | Model |
|---|---|---|---|---|
| S0 | `Start jit-effects` | - | 00 G, 08 | Sonnet |
| R1 | `Name the evidence roles in eml_rt` | S0 | E-D1; G-1, G-15, G-18 | Sonnet (scripted, see R1) |
| R2 | `Build fork tables and the Panic tail in eml_rt` | R1 | E-D2 (heap half); G-13, G-15, G-17, G-18 | Opus |
| R3 | `Build and read fork evidence in eml_rt` | R2 | E-D2 (ctx half); G-13, G-15, G-18 | Opus |
| R4 | `Refuse a counted immortal that crosses workers in checked mode` | R3 | 00 G.3 U-K; G-15, G-17, G-18 | Opus |
| V1 | `Build the VM's fork evidence with eml_rt` | R4 | E-D2 (reroute); G-1, G-2, G-13 | Sonnet |
| CP-A | CI of V1 | V1 | | orchestrator |
| N1 | `Test the abort byte after a call` | CP-A | E-D6 (no landing yet), 00 E-2 (`CTX_YIELD_ABORT` with its first reader); G-3, G-4, G-14, G-15 | Opus |
| N2 | `Add the effect entries` | N1 | E-D3 (entries), E-D8; G-8 | Sonnet |
| N3 | `Lower the effect prims` | N2 | E-D3 (lowering), E-D4 (`e:`, `m:`, machine numbers), E-D8; G-6, G-7, G-17 | Opus |
| CP-B | CI of N3 | N3 | | orchestrator |
| N4 | `Call clauses natively` | CP-B | E-D5; G-9, G-17 | Opus |
| N5 | `Land aborts at prompt` | N4 | E-D6 (landing), 08 U-2 = now; G-4, G-5 | Opus |
| N6 | `Catch extern failures natively` | N5 | E-D7, E-D4 (`Image` effects); G-7, G-10, G-11, G-12, G-17 | Opus |
| CP-C | CI of N6 | N6 | | orchestrator |
| N7 | `Accept effects natively` | CP-C | E-D9; G-16 | Opus (triage of disagreements) |
| CP-D | CI of N7 | N7 | | orchestrator |
| Z | `Close jit-effects` | CP-D | E-D10 | Sonnet |
| CP-Z | CI of Z, then merge | Z | | orchestrator |

Fix commits (`Fix native <topic>`, each with a regression test in `crates/eml_native/tests`) may be inserted before N7 when N7's step 1 finds a disagreement; they are reviewed like units.

### 7.2 Units

Every unit's completion includes **G** (7.3.1) and the **SC** lines in force (7.3.2). "Kind" refers to docs/implementation/testing.md 「テストの変更の運用」. Japanese text (docs, code comments) is drafted in English and translated with PLaMo (`form: plain`, identifiers and section names in `glossary`, earlier parts as `history`), with no space between Japanese and Latin characters or inline code. Code cites `docs/spec/` / `docs/implementation/` sections only.

eml_rt unit tests live in `crates/eml_rt/src/heap/tests/ctx.rs` and are listed in its `both_modes!` call (:896); `both_modes!` makes a `fast::` and a `checked::` copy of each (`heap/tests/mod.rs:10-19`), so each test counts twice. A test for checked-mode errors begins with `if !M::CHECKED { return; }`, as `a_push_past_the_evv_limit_is_bad_evidence` does (`ctx.rs:480-483`). New eml_rt tests are named with the prefixes `fork_` and `evv_step_`, which no existing test of `heap::tests::ctx` has [V: `cargo nextest list` at be1b1de5; the only eml_rt tests containing `fork` are `heap::tests::share::fork_marking_…` and `heap::tests::workers::fork_skips_…`].

### S0: `Start jit-effects`

- Purpose: the stage's first commit (roadmap 「進め方」): this plan replaces the design-level plan, appendix 08 is added, 00 gains section G; the baseline is recorded in section 9 of this plan.
- Files (exactly three; written by the refresh and present uncommitted in the working tree at the base): `docs/plans/2026-11-11-jit-effects.md` (this file), `docs/plans/2026-11-11-jit/08-effects.md` (new), `docs/plans/2026-11-11-jit/00-decisions.md` (the reading guide names 08; new section G). Appendices 01 and 05 are not edited: G overrides them, as F did.
- Work:
  1. `git rev-parse --short HEAD` → `be1b1de5` and `git status --porcelain` lists only the three files (else stop: the line numbers need a refresh). `git switch -c plan/jit-effects`.
  2. Run the baseline of section 9 (macOS, clean tree apart from the three files, after `cargo build --all-targets`) and fill its "Recorded" column in this file. A different number is recorded with the difference stated; a different native snapshot count stops the stage.
  3. Commit `Start jit-effects`; message: the user decision U-H (2026-11-14), one paragraph on the reviews of the refresh (appendix B of this plan), the baseline.
- Completion:
  - `git diff --name-only HEAD~1 HEAD | sort` → `docs/plans/2026-11-11-jit-effects.md`, `docs/plans/2026-11-11-jit/00-decisions.md`, `docs/plans/2026-11-11-jit/08-effects.md`.
  - `cargo nextest run -p eml_cli --test integration -E 'test(/^citations::/)'` → no `failed`.
- Left to the implementer: none.

### R1: `Name the evidence roles in eml_rt`

- Purpose: E-D1; no proof edit; the seven consts crate-private.
- Files: `crates/eml_rt/src/heap/ctx.rs` (seven consts inside `verus!`, inserted before the doc comment of `spec fn ev_ok`, :327-330), `crates/eml_rt/src/heap/ctx/proof.rs`.
- Work (Sonnet; no hand edit of proof text):
  1. Save the script below as `$TMPDIR/roles.py` (not committed) and run `python3 -I $TMPDIR/roles.py crates/eml_rt/src/heap/ctx.rs crates/eml_rt/src/heap/ctx/proof.rs` → prints `{'index': 239, 'size': 17}`.
  2. `verusfmt --verus-only crates/eml_rt/src/heap/ctx.rs crates/eml_rt/src/heap/ctx/proof.rs`; `cargo fmt`.
  3. Add one doc comment per const (Japanese, PLaMo): what the index or size is (`EV_TABLE`: the entry table, `EV_STATE`: the state, `EV_OUTER`: the outer `evv`, `EV_WORDS`: an evidence's scanned words; `NODE_EV`, `NODE_NEXT`, `NODE_WORDS` likewise), citing object-model.md 「evidence の3つの物体」.
  4. docs/spec/object-model.md 「evidence の3つの物体」: the roles are named once, as crate-private consts of `heap/ctx.rs`, not exported, not in `layout`; the list item at L94 (「evidenceの物体の欄の役割」) points at that.
  5. Commit; then EG (7.3.3).

  ```python
  # usage: python3 -I roles.py crates/eml_rt/src/heap/ctx.rs crates/eml_rt/src/heap/ctx/proof.rs
  import re, sys
  EV = {'e', 'ev'}
  NODE = {'w', 'node', 'n', 'cp', 'last', 'cs[k]', 'cs1[k]'}
  EVN = ['EV_TABLE', 'EV_STATE', 'EV_OUTER']
  NN = ['NODE_EV', 'NODE_NEXT']
  OBJ = r'(\w+(?:\[\w+\])?)'
  counts = {'index': 0, 'size': 0}

  def role(obj, i):
      i = int(i)
      if obj in EV:
          return EVN[i]
      if obj in NODE and i < 2:
          return NN[i]
      return None

  def sub_index(pattern, spec, s):
      # pattern groups: (prefix up to and including the object), object, index, closing
      def f(m):
          c = role(m.group(2), m.group(3))
          if not c:
              return m.group(0)
          counts['index'] += 1
          return f"{m.group(1)}{c}{' as int' if spec else ''}{m.group(4)}"
      return re.sub(pattern, f, s)

  EXEC = r'(\b(?:heap\.set_field|heap\.field|h\.field)\(\s*' + OBJ + r',\s*)([0-2])(\s*[,)])'
  SPEC = r'(\b(?:scanned_word|word_at|scanned_at|set_passes|lemma_readable_scanned)\(\s*' + OBJ + r',\s*)([0-2])(\s*[,)])'
  LEM = r'(\b(?:lemma_set_unit_moved|lemma_set_moved|lemma_kept_set)\(\s*\w+,\s*\w+,\s*' + OBJ + r',\s*)([0-2])(\s*,)'
  EVW = r'(\blemma_ev_field_written\(\s*\w+,\s*\w+,\s*\w+,\s*' + OBJ + r',\s*)([0-2])(\s*,)'

  def fix_groups(pattern):
      # LEM/EVW: group 2 must be the object; they are written so that it is
      return pattern

  SIZES = [
      (r'\bdata_obj\(h, e, 3\)', 'data_obj(h, e, EV_WORDS as u8)'),
      (r'\bdata_obj\(h, n, 2\)', 'data_obj(h, n, NODE_WORDS as u8)'),
      (r'\balloc_data\(([^,()]+(?:\(\))?(?: \+ 1)?), 3, 0, false\)', r'alloc_data(\1, EV_WORDS, 0, false)'),
      (r'\balloc_data\(([^,()]+(?:\(\))?), 2, 0, false\)', r'alloc_data(\1, NODE_WORDS, 0, false)'),
      (r'\bMeta::new\(tag, 2, Kind::Data, false\)', 'Meta::new(tag, NODE_WORDS as u8, Kind::Data, false)'),
      (r'\bbits_scan\(wanted\) == 2\b', 'bits_scan(wanted) == NODE_WORDS'),
      (r'\b(scan_of|count_of)\((ev|e)\) == 3\b', r'\1(\2) == EV_WORDS'),
      (r'\bscan_of\((n|node)\) == 2\b', r'scan_of(\1) == NODE_WORDS'),
  ]

  CONSTS = ''.join(f'pub(crate) const {n}: usize = {v};\n\n' for n, v in
                   [('EV_TABLE', 0), ('EV_STATE', 1), ('EV_OUTER', 2), ('EV_WORDS', 3),
                    ('NODE_EV', 0), ('NODE_NEXT', 1), ('NODE_WORDS', 2)])

  for path in sys.argv[1:]:
      s = open(path).read()
      if path.endswith('heap/ctx.rs'):
          anchor = s.index('/// evidence の浅い形')
          s = s[:anchor] + CONSTS + s[anchor:]
      s = sub_index(EXEC, False, s)
      s = sub_index(SPEC, True, s)
      s = sub_index(LEM, True, s)
      s = sub_index(EVW, True, s)
      for pat, rep in SIZES:
          s, k = re.subn(pat, rep, s)
          counts['size'] += k
      open(path, 'w').write(s)
  print(counts)
  ```
  [V: on a clean clone at be1b1de5, steps 1-2 give `{'index': 239, 'size': 17}`, a diff of 272 insertions / 237 deletions in the two files, `verusfmt --verus-only --check` exit 0, `cargo fmt --check -p eml_rt` exit 0, `cargo verus verify -p eml_rt` → `1250 verified, 0 errors`, and the role-literal grep below prints nothing. The result equals S1's measured tree 8ff26d91 except for the one literal 8ff26d91 left at `proof.rs:524`.]
- Tests: none new; kind 3.
- Completion:
  - `cargo verus verify -p eml_rt` → `verification results:: 1250 verified, 0 errors`, no warning [V].
  - `grep -nE '(scanned_word|word_at|scanned_at|set_passes|field|set_field|lemma_readable_scanned)\((e|ev|node|cp|last|cs\[k\]|cs1\[k\]), [0-2][,)]' crates/eml_rt/src/heap/ctx.rs crates/eml_rt/src/heap/ctx/proof.rs` → no output [V].
  - `grep -cE '^pub\(crate\) const (EV_TABLE|EV_STATE|EV_OUTER|EV_WORDS|NODE_EV|NODE_NEXT|NODE_WORDS): usize' crates/eml_rt/src/heap/ctx.rs` → `7`.
  - `git diff --name-only HEAD~1 HEAD -- crates | sort` → `crates/eml_rt/src/heap/ctx.rs`, `crates/eml_rt/src/heap/ctx/proof.rs`.
  - EG.
- If Verus does not report `1250 verified, 0 errors` on the scripted tree: stop and report (the scripted tree is the measured one).
- Left to the implementer: the Japanese wording.

### R2: `Build fork tables and the Panic tail in eml_rt`

- Purpose: E-D2's two `Heap` constructors, verified.
- Files: `crates/eml_rt/src/heap/mod.rs` (next to `alloc_immortal`, :1188; `is_immortal` is `heap/walk.rs:298`, `unsafe`, requires a live object of this heap), `crates/eml_rt/src/heap/ctx/proof.rs` or `heap/proof.rs` for lemmas, `crates/eml_rt/src/heap/ctx.rs` and `heap/walk.rs` (`lemma_grown`).
- Items (inside `verus!`; contract per 00 G.2 "08 E-D2 preconditions"):
  - `pub unsafe fn alloc_fork_table(&mut self, entries: &[Word]) -> (r: Result<Word, HeapError>)`: `requires` as in G.2; in both modes `Err(NotImmortal)` before allocating if a pointer entry has `rc != 0`; else an immortal tag-0 `Data` with one scanned word per entry. Ensures `wf`; `r is Ok` when every entry is immediate or immortal (both modes). Also requires that a pointer entry has no `lin` bit; ensures `fork_table(t)` (a `pub closed spec fn`: `table_ok`, immortal, entries immortal). Lives in `heap/ctx.rs`.
  - `pub unsafe fn alloc_fork_tail(&mut self, panic: u16, table: Word) -> (r: Result<Word, HeapError>)`: in both modes `Err(BadEvidence)` for `panic == u16::MAX`, `Err(NotImmortal)` for a mortal `table`; builds `[table, imm 0, ()]` (tag `panic + 1`, `EV_WORDS`) and `[ev, ()]` (`NODE_WORDS`), both immortal, returns the node. Field indices are R1's consts. Requires `table` is a pointer to a live object of this heap that is a `fork_table` if immortal; ensures `fork_tail(n, panic, table)`.
- Tests (`heap/tests/ctx.rs`, added to `both_modes!`; both modes assert the same):
  - `fork_table_holds_its_immortal_entries` (two immediates and one immortal string; `tag_of` 0; `field(t, i)` returns each; `is_immortal(t)`).
  - `fork_table_refuses_a_mortal_entry` (→ `NotImmortal`; the live-object count is unchanged).
  - `fork_tail_is_an_immortal_node_of_a_fork_evidence` (node tag = ev tag = `panic + 1`; ev fields `[table, imm 0, ()]`; `Heap::freeze` succeeds).
  - `fork_tail_refuses_the_last_effect_number` (`u16::MAX` → `BadEvidence`; a mortal table → `NotImmortal`).
  - Miri: one call of each constructor in T8 `every_core_operation_once` (`heap/tests/trusted.rs:337`).
- docs: object-model.md 「evidence の3つの物体」 (fork tables and the `Panic` tail are built only by these) and 「基本操作の契約」 (two rows).
- Completion: EG; Verus `N verified, 0 errors` with N > 1250; `cargo nextest run -p eml_rt -E 'test(/^heap::tests::ctx::(fast|checked)::fork_/)'` → `8 tests run: 8 passed`; `grep -nE 'debug_assert|cfg!\(debug_assertions\)' crates/eml_rt/src/heap/mod.rs crates/eml_rt/src/heap/ctx.rs | grep -c fork` → `0` (G-17: the immortality check is not debug-only).
- Stop rule (08 E-D2): no `external_body`, `assume` or move out of `verus!`; if the proof does not close, or needs a `requires` beyond G.2's as amended, stop and report.
- Left to the implementer: the proofs.

### R3: `Build and read fork evidence in eml_rt`

- Purpose: E-D2's `Ctx` operations, verified. The stage's largest proof task.
- Files: `crates/eml_rt/src/heap/ctx.rs` (next to `ev_new`, :1997), `crates/eml_rt/src/heap/ctx/proof.rs` (`lemma_fork_ev_ok` next to `lemma_ev_new_ok`, :238). Prototype state (08 E-D2): cloned from `ev_new` (one `alloc_data`, two `set_field`), every call precondition discharges with `lemma_alloc_moved`, `lemma_arg_counted`, `lemma_points_in`, `lemma_arg_moved`, `lemma_set_passes_kept`, `lemma_imm_round_trip`; the open obligation is the final `wf`, i.e. a `lemma_fork_ev_ok` shaped like `lemma_ev_new_ok` without the table half.
- Items (`pub unsafe fn`, each with `# Safety`, inside `verus!`, contracts of 08 E-D2): `Ctx::fork_ev(&mut self, effect: u16, table: Word, depth: u32) -> Result<Word, HeapError>` (fast-mode precondition `effect < u16::MAX` and `fork_table(table)`; checked mode `BadEvidence`), `Ctx::fork_of(&self, ev: Word) -> Result<(u16, u32), HeapError>`, `Ctx::evv_step(&self, w: Word) -> Result<Option<(u16, Word)>, HeapError>`. (The heap constructors check in both modes and `fork_ev` only in checked mode: 08 E-D2 states it so, because `fork_ev` runs per fork and the constructors at load.) `fork_table` holds for an attached frozen table (it is defined over `table_ok`, not `of_this_heap`); carry it across `alloc_data` with `Heap::lemma_fork_table_grew` (added in R2).
- Tests (`heap/tests/ctx.rs`, `both_modes!`):
  - `fork_ev_is_pushed_and_its_clause_read` (table from `alloc_fork_table` of two immediates; `fork_ev(3, table, 2)`; `evv_push(UNIT, ev)` is `Ok`; `ev_clause(ev, 1)` is the second entry; `fork_of(ev) == (3, 2)`).
  - `fork_ev_refuses_a_bad_effect_or_table` (begins `if !M::CHECKED { return; }`; `u16::MAX` → `BadEvidence`; a mortal tag-0 `Data` as table → `BadEvidence`).
  - `fork_of_refuses_an_object_that_is_not_a_fork_evidence` (begins `if !M::CHECKED { return; }`; a cons cell, and an `ev_new` evidence whose state is a heap object → `BadEvidence`).
  - `evv_step_walks_a_list_head_first` (push e0 then e1: `(1, n0)`, then `(0, UNIT)`; `evv_step(UNIT)` → `None`).
  - Miri: `fork_ev`, `fork_of`, `evv_step` once each in T8 `every_primitive_once` (`trusted.rs:405`).
- docs: object-model.md 「evidence の3つの物体」 (a fork evidence is made by `fork_ev`; the readers) and 「基本操作の契約」 (three rows).
- Completion: EG; Verus N > R2's value, 0 errors; `cargo nextest run -p eml_rt -E 'test(/^heap::tests::ctx::(fast|checked)::(fork_|evv_step_)/)'` → `16 tests run: 16 passed` (8 from R2 + 8).
- Stop rule: as R2; also stop if `fork_table(table)` cannot be discharged for a table in an attached frozen region.

### R4: `Refuse a counted immortal that crosses workers in checked mode`

- **Purpose:** make the R3 spec rule ("a counted immortal object is reachable only from its own heap's worker") a checked precondition. Checked mode only; fast mode is unchanged.
- **Files:**
  - `crates/eml_rt/src/error.rs` (enum :15, `Display` :49-112)
  - `crates/eml_rt/src/heap/check.rs` (`checked_dup` rc-0 branch :370-372, `checked_decref` rc-0 branch :409-414, `immortal_counted` :240)
  - `crates/eml_rt/src/heap/mod.rs` (`acquire_immortal`, the `count_immortal` call at :3846)
  - `crates/eml_rt/src/heap/core.rs` (`Core::foreign_block` :330, reused unchanged)
  - `crates/eml_rt/src/heap/graph.rs` (trait `Graph`), `crates/eml_rt/src/heap/share.rs` (`ShareError`, `share_walk` :1651, the stop branches :1868-1930, the mapping near :2402)
  - `crates/eml_rt/src/heap/tests/share.rs`, `tests/transfer.rs`, `tests/trusted.rs`
  - `docs/spec/object-model.md`
- **Items (inside `verus!`):**
  - New variant `HeapError::CountedImmortal`, with a `Display` arm. No other crate matches `HeapError` exhaustively, so nothing else changes.
    - A new variant, not `WrongOwner { owner, worker }`: that variant's `owner` would carry a count, and `chain_err` already admits `WrongOwner` from release chains, so reuse would blur that contract.
    - Named for the object, not `ForeignImmortal`, because `share` refuses one of its own heap.
  - New helper in `check.rs`:
    ```
    pub(super) fn foreign_counted(&self, h: *mut Header) -> (r: bool)
        requires self.core.sound(), self.core.checked(), self.core.has(h@.addr), h == self.core.block(h@.addr),
        ensures !r,
    { self.core.foreign_block(h) && self.core.prefix_first_get(h) != UNCOUNTED }
    ```
    - `foreign_block` compares the page head's `sink` with this heap's and with null, and reads no block memory. It runs first, so own and region blocks never reach the count read here.
    - `ensures !r` holds because the model has only own and region blocks; no `requires` or `ensures` of a public operation changes for these three sites.
  - `if self.foreign_counted(h) { return Err(HeapError::CountedImmortal); }` at three sites, each already under `M::CHECKED`:
    - the `rc == 0` branch of `checked_dup`, before `immortal_counted` (covers `dup`, `dup_check`, and so `fork_ev`);
    - the `rc == 0` branch of `checked_decref`, before the count read (covers `decref` and every release path through it);
    - `acquire_immortal`, before `immortal_counted`.
  - `share`: new trait method `Graph::counted(&self, node) -> bool` with no `ensures`; `Objects::counted` returns `false` when `!M::CHECKED`, else `prefix_first != UNCOUNTED`.
    - New `ShareError::Counted`, mapped to `HeapError::CountedImmortal`.
    - In `share_walk`, a new branch before the `LinShared` branch: an rc-0 node with `graph.counted(node)` returns `Err(ShareError::Counted)`.
    - Gate it so that every walk that makes a value reachable from another worker refuses (`share` and the `par` fork marking), and state in the commit message which `check_lin` values those are. The prototype gated on `check_lin`; confirm that covers the fork marking, or drop the gate.
    - Contract changes: `share_walk`'s "`==> r is Ok`" becomes "`==> r is Ok || r == Err(Counted)`"; `share`'s "`share_all_ok(w) ==> r is Ok && consistent()`" becomes "`==> (r is Ok || r == Err(CountedImmortal)) && consistent()`".
    - Marks already made stay, as with `share`'s other errors.
  - `count_immortal` (check.rs:264) is unchanged: every caller is now behind one of the three checks.
- **Fast-mode cost:** zero. Every new exec line is under `M::CHECKED` or returns `false` for `!M::CHECKED` in a monomorphized `Graph` impl. The bench gate is the proof.
- **Tests:**
  - `heap/tests/share.rs`, `both_modes!`: `share_refuses_a_counted_immortal` (a cons cell holding an `alloc_string_immortal` of the same heap; checked gives `CountedImmortal`, fast gives `Ok`).
  - `heap/tests/trusted.rs`, both modes: `a_counted_immortal_of_another_heap_is_refused`, modelled on `a_remote_small_free_is_reclaimed_and_reused` (:1024, `two_heaps` :996).
    - Thread A makes a counted immortal and sends the word to thread B; A then only waits for the join.
    - B's `dup`, `decref` and `acquire_immortal` return `CountedImmortal` in checked mode and `Ok` in fast mode.
    - B's `dup` and `decref` of A's dummy pass in both modes.
    - After the join, A's `decref` returns `ImmortalOverRelease` in checked mode, so B wrote nothing.
  - This `trusted::` test is the Miri and ThreadSanitizer test; A is quiescent while B runs, so both must report nothing.
- **Test changes (kind 2, listed one by one; reason: they share a value that reaches a counted immortal of the same heap, which the new rule forbids):**
  - `heap::tests::share::checked::share_stops_at_immortal_objects`
  - `heap::tests::share::share_leaves_an_immortal_lin_object_alone`
  - `heap::tests::transfer::checked::shared_counts_above_one_and_immortal_objects_stop`
  - Each keeps its claim (the walk stops at an immortal and leaves it alone) with an uncounted immortal: one built in a second heap, frozen and attached, or the dummy. No assertion is dropped.
  - If one of them cannot be rebuilt on an uncounted immortal (the `lin` one is the candidate), stop: that is a kind 1 change for the user.
- **docs (`docs/spec/object-model.md`):**
  - 「核の操作」 (:397): after the R3 rule (a counted immortal is reachable only from its own heap's worker) add "Checked mode refuses it with `CountedImmortal`: `share` when the walk reaches a counted immortal, and `dup`, `decref` and `acquire_immortal` when the object is on another heap's page. Fast mode does not check."
  - 「不死の物体の数え方」 (:754): one bullet for the two refusals, and one for the limit: "the `dup`/`decref`/`acquire_immortal` refusal reads the other heap's count without an atomic, so it is a diagnosis of a broken precondition, not a guarantee; `share`'s refusal runs on the owner's thread".
  - 「報告する誤り」 (:767): a `CountedImmortal` bullet naming the four operations.
  - 「基本操作の契約」 rows of `dup`, `decref`/`release`, `acquire_immortal`, `share` and `fork_ev`: add `CountedImmortal` in the checked-error parentheses.
  - T14 (:1459): after condition (iv), "checked mode refuses a violation of (iv) as above".
- **Completion:**
  - EG (plan 7.3.3), with Verus `N verified, 0 errors` and N greater than R3's amended value.
  - `cargo nextest run -p eml_rt -E 'test(share_refuses_a_counted_immortal) | test(a_counted_immortal_of_another_heap_is_refused) | test(a_counted_immortal_under_a_cell_of_the_child_is_refused)'` → `6 tests run: 6 passed`.
  - Inbound refusal (00 G.3 (f), U-L): `transfer_walk` (`heap/transfer.rs`) returns `TransferError::Counted` → `HeapError::CountedImmortal` in checked mode when it reaches a counted immortal; `transfer_back`'s last `ensures` gains `|| r == Err(CountedImmortal)`; a failed `transfer_back` keeps the flips already made, as for `WrongOwner`. Spec: no thread but the owner's writes a count; a refusal that reads a count on another heap's page (`transfer_back`, `share` of a value already holding one, `dup`, `decref`, `acquire_immortal`) is a diagnosis, not a guarantee.
  - `cargo test --workspace -q` → no failure.
  - `nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=thread TSAN_OPTIONS=halt_on_error=1 CARGO_TARGET_DIR=target/tsan cargo test -Zbuild-std --target $(rustc -vV | sed -n 's/^host: //p') -p eml_rt trusted::` → pass, no report.
  - `git diff 4bce211a -- crates/eml_rt | grep -cE '^\+.*(external_body|assume\()'` → `0`; `src/trust.rs` unchanged.
  - `git diff 4bce211a --stat -- crates/eml_vm/tests/bench.rs tests/ui crates/eml_cli/tests` → empty.
- **RunStats and UI:** no change expected. Both engines freeze every immortal, so a run heap holds no counted immortal; the prototype's workspace run changed only the three eml_rt tests above.
- **Stop rule:** as R2 (no `external_body`, `assume` or move out of `verus!`). Also stop if:
  - any test other than the three listed changes outcome;
  - a `requires` of `dup`, `decref`, `acquire_immortal` or `share` must change;
  - a new trust-list row is needed;
  - the bench gate fails twice.
- **Left to the implementer:** the Japanese wording; the proofs in `share.rs` (the prototype needed the two contract edits above and no new lemma).

### V1: `Build the VM's fork evidence with eml_rt`

- Purpose: E-D2 reroute, kind 3.
- Files: `crates/eml_vm/src/compile/forks.rs` (`table` :80 → `heap.alloc_fork_table(&words)` with `Word::imm(f)` entries, keeping the assert at :84-87 before the call (it discharges the `# Safety` length clause, G-17); the calls are `unsafe`, and the comment at the `fork_ev` call states "the table was built by `alloc_fork_table` and frozen unchanged"; `panic_tail` :102 → `heap.alloc_fork_tail(panic, table)`; drop unused `layout` imports), `crates/eml_vm/src/vm.rs` (`fork_evv` :1912-1970: walk with `ctx.evv_step`, build with `ctx.fork_ev` + `ctx.evv_push(next, ev)`; `child_abort` :2490-2527: `ctx.fork_of(ev)`).
- docs: vm.md 「`par` の fork と join」: 「子の `evv`」 names the eml_rt operation that builds each object; 「子の中断」 reads the effect and depth with `fork_of`.
- Tests: none new; kind 3. Contingency (08 A-7): a changed `crates/eml_vm/tests/bench.rs` snapshot is allowed only for `bench/` programs that use `par` (kind 2), with the delta in the commit message; any other change stops the unit.
- Completion:
  - `grep -cE 'set_field|alloc_immortal|data_first' crates/eml_vm/src/compile/forks.rs` → `0` (10 at base [V]).
  - `awk '/^fn fork_evv/,/^}/' crates/eml_vm/src/vm.rs | grep -cE 'set_field|scanned\(|alloc_data|data_first|\.meta\('` → `0` (10 at base); same with `/^fn child_abort/` → `0` (2 at base). The ranges end at each function's closing brace [V at base: 59 and 38 lines, last lines `Ok(next)` / `child_done::<R>(…)` then `}`]; check `awk '/^fn fork_evv/,/^}/' crates/eml_vm/src/vm.rs | tail -2` shows `Ok(next)` and `}` after the edit.
  - `cargo nextest run -p eml_vm --test integration -E 'test(bench::)'` → no failed; `git status --porcelain | grep -c snap` → `0`.
  - Miri `eml_vm --test integration miri::`, both settings → pass, `user` ≤ 300 s / 400 s.
  - TSan: `nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=thread TSAN_OPTIONS=halt_on_error=1 CARGO_TARGET_DIR=target/tsan cargo test -Zbuild-std --target $(rustc -vV | sed -n 's/^host: //p') -p eml_vm --test integration -- workers::` → pass.
- Risk 08 A-2: if checked-mode `evv_push` refuses a fork evidence or the frozen tail, stop and report (08's fallback `Ctx::fork_node` is an eml_rt change; the orchestrator decides).

### CP-A

Push `plan/jit-effects` (with the user's permission unless given for this stage's checkpoints); `gh run watch` → workflow `CI` `success` on every job (test ×4 OS, fmt, verus, miri, asan, tsan, licenses, gui-native).

### N1: `Test the abort byte after a call`

- Purpose: E-D6 without landings; `CTX_YIELD_ABORT` with its first reader (G.2, E-2 line); I-land checked.
- Files and items:
  - eml_rt: `crates/eml_rt/src/heap/ctx.rs` :3816-3826 add `pub const CTX_YIELD_ABORT: usize = offset_of!(Ctx<Fast>, native) + offset_of!(Native, yielding) + offset_of!(YieldState, abort)`; include it in the const asserts (:3828-3850; 24 is ≤ 120 and a multiple of 8); re-export in `lib.rs:44` and `heap/mod.rs:65`.
  - `crates/eml_native/src/mem.rs`: import `CTX_YIELD_ABORT` (:10); `pub fn yield_abort(b: &mut FunctionBuilder, ctx: Value) -> Value` = trusted `load.i8` at the offset.
  - `crates/eml_native/src/lower/unwind.rs`: `pub(super) fn if_aborting(b: &mut FunctionBuilder, ctx: Value) -> (Block, Block)`: `a = mem::yield_abort`; creates `cold` (`b.set_cold_block`) and `hot`; `brif a, cold, hot`; returns `(cold, hot)` with no block selected. `FnLower::cold_block` (:186) only creates and marks a block, so `after_unwind_point` calls `if_aborting(&mut self.b, self.ctx)` for `After::Call` (:122-131) and fills `cold` with `fill_cleanup`. Until N5, unwind.rs loads no `target`.
  - `crates/eml_native/src/lower/apply.rs` `define_apply` `n < m` arm (:372-386): `if_aborting(&mut b, ctx)`; cold keeps `decref last; return UNIT`; hot: `t = mem::yield_target`, `t != UNIT` → a new cold block with `trap(TrapCode::unwrap_user(1))` (the arm's existing trap code), else the existing `return_call eml$apply$1`. The comment cites native.md 「関数の値」.
  - CLIF kept for helpers when asked: `crates/eml_native/src/lower/mod.rs` passes `keep_clif` to `define` for the applies (:228) and `eml$raise` (:192); `jit.rs` (:258-270) stores them in `clif` under `eml$apply$<m>` and `eml$raise`.
  - eml_lir `crates/eml_lir/src/verify.rs` (`check_block`, :693): after `Rhs::Call(Call::Apply { prompt: Some(v), .. })`, the next `Rhs::Call`, `LetMany` call or tail call of the block is `Call::Direct` of the function named `EP_PROMPT` with first argument `v`, and no `Rhs::Call`, `Rhs::Extern`, `prim yield_to` or `prim yield_abort` comes between the marked `apply` and that call; else the error ``the apply with the prompt marker `<v>` is not followed by `prompt$` `` (`<v>` as `call_text` prints vars).
- New tests:
  - `crates/eml_lir/tests/verify.rs`: `a_prompted_apply_followed_by_prompt_is_accepted` (the `try_panic` LIR of `crates/eml_vm/src/vm.rs:4655-4700`, `CAUGHT_PANIC`), `a_call_between_a_prompted_apply_and_prompt_is_refused` (inserts `let z.9: unit = call g()` between; asserts the message), `an_extern_row_between_a_prompted_apply_and_prompt_is_refused` (inserts an extern row between; asserts the same message).
  - `crates/eml_core_ir/src/evidence/tests.rs`: `prompt_calls_nothing_before_yield_finish` (status-1 and status-2 arms of `prompt$` built by `helpers::prompt`).
  - `crates/eml_native/tests/abort.rs`: `exit_inside_an_over_application_releases_the_last_argument` (LIR: `f` takes 1 argument and calls `extern Prelude.exit(3)`; `entry$main` applies `&f` as a value to 2 boxed arguments; `agree(text)` and `native(text)` → `Ok(Some(3))`; the census is empty, so the `n < m` arm's cold block released the last argument).
  - `crates/eml_native/tests/clif.rs`: `an_over_application` (pin of `clif(text, "eml$apply$2")`): one `load.i8 notrap aligned v0+24`, a cold block calling `%eml_rt_decref`, one `load.i64 notrap aligned v0`, one `trap user1`.
- Kind 2: `clif::a_call_and_its_unwind_check`, `clif::an_apply_site` (the only pins with `load.i64 notrap aligned v0` at base [V]); eml_rt `the_native_offsets_are_pinned` (`heap/tests/ctx.rs:938`) becomes `(0, 24, 32, 40, 48, 56)` over `CTX_YIELD_TARGET, CTX_YIELD_ABORT, CTX_FAULT, CTX_STOP, CTX_STACK_LIMIT, CTX_JMP`.
- docs: native.md 「巻き戻しの地点」 (After::Call row: abort byte; a yield returns on the hot path; I-land as the invariant the test needs), 「関数の値」 (the `n < m` arm uses the shared emitter, then the guard; delete the sentence that the arm's copy must follow the call-point rule); object-model.md 「欄」 (`CTX_YIELD_ABORT`, one byte at 24, readers); lir.md 「verifier」 and 「着地の印」 (the `prompt$` rule); core-ir.md 「handle と `prompt$`」 (I-land). Both state I-land as a rule whose violation is undefined in a release build, not as "a reported wrong run"; native.md 「巻き戻しの地点」 likewise.
- Completion:
  - EG (eml_rt touched; Verus count unchanged from R3: the const is outside `verus!`, `ctx.rs:3808` [V]).
  - `cargo nextest run -p eml_native -E 'test(clif::) | test(abort::)'` → `16 tests run: 16 passed` (clif 10 + 1, abort 4 + 1 [V: base counts]).
  - `cargo nextest run -p eml_lir -E 'test(prompt)'`, `cargo nextest run -p eml_core_ir -E 'test(prompt)'` → each includes the new tests, 0 failed.
  - `awk '/^fn a_call_and_its_unwind_check/,/^}/' crates/eml_native/tests/clif.rs | grep -c 'load.i8 notrap aligned v0+24'` → `1`; same range `grep -c 'load.i64 notrap aligned v0$'` → `0`.
  - `cargo nextest run` (the debug-build verifier checks I-land on every UI program) → no failed. A failure is 08 A-3 false: stop and report.
- Left to the implementer: CLIF block order; the Core IR walker of the `prompt$` test.

### N2: `Add the effect entries`

- Purpose: E-D3's 21 `shim` entries `eml_rt_<Prim::name()>`, table `EFFECTS`.
- Files: new `crates/eml_nrt/src/effects.rs` (`crate::entries! { pub(crate) const EFFECTS = "eml_rt_"; pub(crate) shim fn evv_get(ctx) -> tobj { .. } … }`; parameters exactly 08 E-D3's table; conversions with `try_from`, failure `Err(BadEvidence)`, `Err(RootMisuse)` for a root index); `crates/eml_nrt/src/lib.rs` (`mod effects;`, `TABLES` :38 gains `effects::EFFECTS`); `crates/eml_nrt/src/trust.rs` `FILES` and `UNSAFE_FILES` gain `effects.rs`; `crates/eml_lir/src/prim.rs` `kinds!` (:76-80) emits `impl Prim { pub const KINDS: [Prim; N] }` and `by_name` uses `Self::KINDS` (G.2). `evv_mask`'s `# Safety` names `items` / `n`.
- Entries (21): `evv_get, evv_set, evv_push, evv_mask, ev_new, ev_set_clause, ev_renew, ev_lookup, ev_clause, ev_evv, ev_take_evv, ev_take_state, ev_set_state, yield_to, yield_abort, yield_cont_take, yield_cont_set, yield_finish, dummy, root_get, root_set`. None for `yielding`, `yield_status`, `empty_mut_array`, `child_abort`.
- New tests:
  - new `crates/eml_native/tests/effects.rs` (`mod effects;` in `tests/main.rs`): `every_prim_entry_is_named_after_its_prim` (G-8): for each of `Prim::KINDS` other than the four, `eml_nrt::entries()` has `eml_rt_<name>` with `params.len()` = immediates (`evv_mask` 2, `ev_new` 2, `ev_set_clause` / `ev_lookup` / `ev_clause` / `root_get` / `root_set` 1, else 0) + `row().params.len()` and `result.is_none()` iff the row's result Repr is `Unit`; none for the four.
  - `crates/eml_nrt/src/entries.rs` tests module (the Miri filter `entries::` runs it): `the_effect_entries_install_and_find_a_handler` (direct calls `ev_new`, `ev_set_clause`, `evv_get`, `evv_push`, `evv_set`, `ev_lookup`, `ev_clause`, then releases; `live` empty, no fault parked).
- docs: native.md new 「エフェクト」 after 「行の下ろし方」 (entries: prefix, symbol from `Prim::name()`, parameters, conversions); architecture.md `eml_nrt` (`effects.rs`, `EFFECTS`); CLAUDE.md Architecture `eml_nrt` line: tables `ENTRIES`, `SERVICES`, `HEAP`, `ROWS`, `EFFECTS`, `CLOSURES`, `C`.
- Completion: G; `cargo nextest run -p eml_native -E 'test(effects::)'` → `1 passed`; `cargo nextest run -p eml_nrt` → no failed (incl. `the_table_has_only_named_prefixes`, which also checks uniqueness); Miri eml_nrt both settings → pass; `grep -c 'fn eml_rt_' crates/eml_nrt/src/effects.rs` → `0`.

### N3: `Lower the effect prims`

- Purpose: E-D3 lowering, E-D4 data and machine numbers, E-D8.
- Files and items:
  - new `crates/eml_native/src/lower/effects.rs` (eml_native `trust.rs` `FILES` gains it): `pub(super) fn entry(prim: &Prim) -> String` (G.2); inline `yielding` (`mem::yield_target` ≠ UNIT) and `yield_status` (`select`s, no branch); every other prim `call_entry(&entry(prim), args)`; `yield_abort` then `after_unwind_point(point, After::Never { raise: None })`; a `unit` result binds `UNIT`.
  - `crates/eml_native/src/lower/function.rs` `stmt()` (:201-317): `Rhs::Prim` other than `EmptyMutArray` / `ChildAbort` → effects.rs; `ChildAbort` keeps the panic (rejected form).
  - `crates/eml_native/src/lower/names.rs`: `effect(text) = "e:" + text`, `mask(text) = "m:" + text` (texts from `eml_lir::syntax::effect_text` / `mask_text`, `syntax.rs:23`, `:30`).
  - `crates/eml_native/src/lower/consts.rs` (:19-120): `Datum::Effect(u32)`, `Datum::Mask(Vec<eml_lir::MaskItem>)`, `Datum::bytes()`; `define` sizes each object by `bytes()`; `assert!(!items.is_empty())` at the mask use (every build; G-17).
  - `crates/eml_native/src/jit.rs`: `NativeMachine.effects: HashMap<String, u16>` assigned at `load` in `program.effects` order; past 65534 panic with the VM's text (`crates/eml_vm/src/compile/emit.rs:1048-1053`); `Tables::new` (:108-163) writes `DATUM_EFFECT` rows (`bits` = machine number) and `DATUM_MASK` rows (`items` → `MaskRow`s with machine numbers).
  - `crates/eml_nrt/src/image.rs` (:98-130): `DATUM_EFFECT = 5`, `DATUM_MASK = 6`, `#[repr(C)] MaskRow { effect: u16, keep: u32, skip: u32 }`, `DatumRow.items: Slice<MaskRow>`, `pub fn mask_bytes(n: usize) -> usize`; `crates/eml_nrt/src/run.rs` `fill_data` (:135-176): kinds 5 and 6 written directly (raw `u64`; typed `MaskItem` writes), kinds 0..4 through `constants::build`; `fill_data`'s and `run`'s `# Safety` and the `Sync` comment of `Image` cover the mask slot (`mask_bytes(items.len)` bytes, not one word); the pairing of rows with built words skips the raw rows; `const _: () = assert!(align_of::<MaskItem>() <= 8)` in image.rs.
- Kind 3: every `DatumRow { … }` literal gains `items` (empty `Slice`): `crates/eml_nrt/tests/run.rs` :203, :210, :217, `crates/eml_native/src/jit.rs` :138.
- New tests (`crates/eml_native/tests/effects.rs`; LIR text; `agree(text)` where the text is a whole run):
  - `yield_status_reads_as_the_runtime_does` (`yield_to` then `yield_status` of the same / another evidence → 1 / 3; `yield_finish` → 0; after `yield_abort` → 2).
  - `yielding_is_one_while_a_yield_is_in_flight`.
  - `a_handler_is_installed_and_found` (`evv_get`, `ev_new`, `ev_set_clause`, `evv_push`, `evv_set`, `ev_lookup`, `ev_clause`, `ev_evv`, `ev_take_state`, `ev_set_state`, `ev_renew`).
  - `a_mask_skips_the_named_evidence` (`evv_mask[E*1]`) and `a_mask_datum_holds_its_items_in_lir_order` (`machine.image()` rows of kind `DATUM_MASK`).
  - `effect_numbers_come_from_the_machine_table` (G-6): two programs whose `effect` lines list `A`, `B` and `B`, `A`; the same function returns the same result on each machine; the `DATUM_EFFECT` rows' `bits` differ accordingly.
  - `root_set_then_root_get_round_trips`, `root_get_of_an_empty_root_ends_with_the_vm_error`, `root_set_of_a_set_root_ends_with_the_vm_error` (stand-in thunk via `common::native::visit` that gives the `Ctx` roots, `crates/eml_rt/src/heap/ctx/roots.rs:65-179`; the error text equals the VM's for the same LIR; `root_get` ends in `RootMisuse` for an empty root or one outside the table, `root_set` only for a root already set).
  - `crates/eml_native/tests/clif.rs` `an_effect_lookup` (pin: `symbol_value` of `e:` + one load, then `call %eml_rt_ev_lookup`; no `iconst` holding an effect number).
- docs: native.md 「エフェクト」 (inline prims, `e:` / `m:`, machine numbers), 「Image」 (`DATUM_EFFECT`, `DATUM_MASK`, `MaskRow`, `items`), 「値と定数」 (the two symbols, data sizes); architecture.md `eml_native` (`lower/effects.rs`); CLAUDE.md Architecture `eml_native` line (`lower/` holds the effect prims).
- Completion: G; `cargo nextest run -p eml_native -E 'test(effects::) | test(clif::)'` → `22 tests run: 22 passed` (effects 1 + 9, clif 12); Miri eml_nrt both settings → pass; SC lines of N3.
- Left to the implementer: the `visit` thunk's root setup.

### CP-B

As CP-A, on N3's commit. Before it: the Windows and Linux cross clippy lines (7.3.4).

### N4: `Call clauses natively`

- Purpose: E-D5.
- Files and items:
  - `crates/eml_native/src/abi.rs`: `closure_entry_signature(function: &LirFn, captures: usize) -> Signature` (tail `(ctx, clo, kinds(params[captures..])) -> kinds(rets)`) and `clause_signature(program, effect, op) -> Signature` (`(ctx, clo, ev, op params) -> op ret`; a `never` op returns one `I64`).
  - `crates/eml_native/src/lower/apply.rs`: `arity` (:65-89) drops the clause assert and returns 0 for a clause function (no uniform assert for it); `ClosureEntries::id` (:123-147) declares with `closure_entry_signature` and names no clause; one helper `call_closure_code(b, ctx, callee, sig, args, tail)` (meta, `closure_code`, `call_indirect` / `return_call_indirect`) used by the exact arm of `apply`, `Call::Clause`, and N6's `eml$raise`; `FnLower::clause` (non-tail, then `After::Call`) and the tail form.
  - `crates/eml_native/src/lower/mod.rs` closure loop (:197-215): signature from `closure_entry_signature`; for a function with `clause = Some((e, op))`, `assert_eq!(signature, abi::clause_signature(program, e, op), …)` before the entry is defined (every build; G.2).
  - `crates/eml_native/src/lower/function.rs` / `call.rs`: `Call::Clause` in `Rhs::Call` and `Term::TailCall` goes to `FnLower::clause`.
  - `crates/eml_nrt/src/run.rs:146-153`: `DATUM_FN` accepts `bits == 0`; `crates/eml_nrt/src/constants.rs` `static_closure`: `data_meta(0, 1, false)` for arity 0.
- New tests (`effects.rs`; `agree` on LIR text that installs the handler with prims and has no `prompt` marker): `an_in_place_clause_returns_its_value`, `an_under_clause_runs_with_its_handlers_evv`, `a_tail_clause_call`, `a_clause_with_a_float_operand`, `a_static_clause_closure_has_tag_0` (`DatumRow` `(DATUM_FN, 0)` for `&g$entry`). Release check (G-17): `release_check_a_clause_entry_signature_mismatch_panics` (an `eml_lir::parse`d program, not verified, whose clause function's parameters differ from its operation's; `NativeMachine::load` under `catch_unwind` panics; the message is the assert's in a release build and the verifier's (`invalid LIR at the native load`) in a debug build (`cfg!(debug_assertions)` picks the expected text)).
- Test changes: none (no test pins arity 0 as `Internal` [V: `grep -rn arity crates/eml_nrt/tests crates/eml_native/tests` → only `apply.rs:58` `(DATUM_FN, 1)`, unaffected]).
- docs: native.md 「関数の値」 (signature rule, tag 0, `Call::Clause`, ownership of `clo`, where the clause-signature assert runs).
- Completion: G; `cargo nextest run -p eml_native -E 'test(effects::) | test(apply::) | test(placement::)'` → no failed; `awk '/fn id</,/^    }$/' crates/eml_native/src/lower/apply.rs | grep -c clause` → `0` (G-9: no clause branch in `ClosureEntries::id`); `grep -c 'clause_signature' crates/eml_native/src/lower/mod.rs` → `1`; Miri eml_nrt both settings → pass.

### N5: `Land aborts at prompt`

- Purpose: E-D6 landing; `eml_nrt_landed`.
- Files and items: `crates/eml_nrt/src/services.rs`: `pub shim fn eml_nrt_landed(ctx) -> obj` (the heap's dummy), in `SERVICES`; `crates/eml_native/src/lower/unwind.rs` `fill_cleanup` (:211) loses the assert; for a point with landing `v` the cold block is `t = mem::yield_target; brif t == v → land, other`; `land`: `call eml_nrt_landed`, bind the call's result var, `jump hot`; `other`: `call_entry(&effects::entry(&Prim::EvTakeState), [v])`, `eml_rt_decref` of the result, then the cleanup; `crates/eml_native/src/lower/function.rs` (:233-243, :342-349) accepts `Call::Apply { prompt: Some(_) }`.
- New tests (`effects.rs`; LIR text or `agree_source`, since a whole `handle` lowers from here on):
  - `an_abort_lands_and_the_clause_returns` (G-4's named test).
  - `an_abort_unwinds_three_frames_and_lands` (owned values and a token in each frame; census empty).
  - `a_resumable_yield_passes_three_frames`, `a_multi_shot_continuation`, `a_masked_perform_skips_the_inner_handler`.
  - `an_abort_through_a_handler_frame_releases_its_state` (stand-in thunk + `Heap::live_objects` before the run ends; G-5).
  - `crates/eml_native/tests/clif.rs` `a_yield_point_call` (one `load.i8 … v0+24` before the `brif`; `target` loaded only in EP's `yielding`) and `a_landing_point` (G-5: for `let x = yields apply f(()) prompt ev` the cold block loads `target`, compares with `ev`; the `land` block calls `%eml_nrt_landed` then jumps to the hot block; the `other` block calls `%eml_rt_ev_take_state`, `%eml_rt_decref`, then the frame's cleanup, then `return`).
- docs: native.md 「巻き戻しの地点」 (landing block, order, `eml_nrt_landed`; delete 「着地の印を持つ地点は下ろさない…」), 「エントリ」 service list.
- Completion: G; `cargo nextest run -p eml_native -E 'test(=effects::an_abort_lands_and_the_clause_returns)'` → 1 passed; `cargo nextest run -p eml_native` → no failed; `grep -c 'landing.is_none' crates/eml_native/src/lower/unwind.rs` → `0`; SC lines of N5.

### N6: `Catch extern failures natively`

- Purpose: E-D7; `Image.panic_effect` / `io_effect`; `LoadError::Prelude`.
- Files and items:
  - `crates/eml_nrt/src/services.rs` `eml_nrt_failed` (:51): `(ctx, function, site, purity, out: ptr) -> i64`, steps 1-5 of 08 E-D7; its `# Safety` names `out` (4 writable words; the `ptr` kind is `*const Word`, so the service casts to `*mut`); `crates/eml_nrt/src/image.rs`: `RAISE_PANIC = 0`, `RAISE_IO = 1`, `NO_EFFECT = u32::MAX`, `Image.panic_effect: u32`, `Image.io_effect: u32`.
  - `crates/eml_native/src/mem.rs`: a 4-word stack slot helper and a load from it.
  - `crates/eml_native/src/lower/unwind.rs` `define_raise` (:63-74): slot, call, two arms calling `apply::call_closure_code` with `uniform(2)` / `uniform(3)`, else nothing, `return`; after either clause call, `eml$raise` tests the abort byte with `unwind::if_aborting` and traps (`TrapCode::unwrap_user(1)`) when it is clear (08 E-D7).
  - `crates/eml_native/src/jit.rs`: `LoadError::Prelude { effect: String }` (:37-57 and `what()`), checked in `load` right after the `Loaded` check (:216-218), outside `cfg!(debug_assertions)` (the check reads each operation's `params` and `ret`: `[Obj]` / `[Enum, Obj]` and `ret == OpRet::Never`); `Tables::new` fills the two `Image` fields from the machine table (`ExternEffect::{Panic, Io}.row().name`).
- Kind 3: `crates/eml_nrt/tests/services.rs` :119, :164, :197 pass a 4-word buffer (from `as_mut_ptr()`) as `out`; every `Image { … }` literal gains `panic_effect: NO_EFFECT, io_effect: NO_EFFECT`: `crates/eml_nrt/tests/run.rs` :27, :225, :352, :406, `crates/eml_nrt/tests/services.rs` :37, `crates/eml_nrt/src/rows.rs` :188, `crates/eml_nrt/src/entries.rs` :532 (plus `jit.rs` :147, the producer). Assertions unchanged.
- New tests (new `crates/eml_native/tests/raise.rs`, in `tests/main.rs`): `prelude_with_two_panic_operations_is_refused`, `prelude_whose_panicked_takes_two_objects_is_refused`, `prelude_whose_panicked_returns_a_value_is_refused`, `try_io_sees_the_vm_kind_for_not_found_and_permission_denied`, `an_overflow_without_prelude_panic_ends_with_the_vm_text`, `try_panic_catches_inline_overflow`, `try_panic_catches_an_index_row`, `try_panic_catches_the_panic_row`, `try_panic_catches_a_c_row_narrowing_panic`, `try_io_catches_a_failing_open`, `a_stop_is_never_caught`, `a_pure_rows_fault_is_not_caught`, `no_handler_gives_the_vm_text`, `cleanups_run_after_raise_returns` (each `agree` / `agree_source`, census empty: G-11, G-12); `release_check_an_empty_mask_panics` (a parsed, unverified program whose `evv_mask` list is emptied after parsing; `load` panics; the message is the assert's in a release build and the verifier's (`invalid LIR at the native load`) in a debug build, `cfg!(debug_assertions)` picks the expected text); `crates/eml_native/tests/clif.rs` `the_raise_helper` (pin of `clif(text, "eml$raise")`: one `call %eml_nrt_failed`, two `call_indirect`, two `load.i8 ... v0+24`, one `trap user1`, no `iconst` but the two class values; G-10).
- docs: native.md 「externの失敗」 (classes, order, `out`, ownership; delete 「失敗を捕まえるハンドラはないので…」), 「Image」 (two fields), 「関数の形」 (`LoadError::Prelude`).
- Completion: G; `cargo nextest run -p eml_native -E 'test(raise::) | test(clif::)'` → no failed; `cargo nextest run -p eml_native --release -E 'test(/::prelude_/) | test(release_check)'` → `5 tests run: 5 passed` (G-17: the three `prelude_` tests of N6, N4's `release_check_a_clause_entry_signature_mismatch_panics`, N6's `release_check_an_empty_mask_panics`); `grep -rn 'debug_assert\|cfg!(debug_assertions)' crates/eml_native/src/jit.rs | grep -ci prelude` → `0`; SC lines of N6; Miri eml_nrt both settings → pass; ASan (CLAUDE.md command) `-p eml_native -- --skip fatal::` and `-p eml_nrt -- --skip fatal::` → pass.

### CP-C

As CP-A, on N6's commit; before it the cross clippy lines (7.3.4).

### N7: `Accept effects natively`

- Purpose: E-D9; the native UI runs every `effects/`, `unwind/`, `panic/` file.
- Work:
  1. Apply the `Form` change locally and run `cargo nextest run -p eml_cli --test integration -E 'test(=ui::run_native) | test(=ui::run_fail_native)'`. A disagreement or native panic stops N7: fix it in its own commit `Fix native <topic>` with a regression test in `crates/eml_native/tests` (reviewed), then resume. No `native: skip` mark (G-16).
  2. `crates/eml_native/src/unsupported.rs`: `Form` = `Par, AsyncRow, BlockingC, ChildAbort` (label `child abort`, trigger `Rhs::Prim(Prim::ChildAbort, _)`); delete `Clause` / `Prompt` and their detection; `crates/eml_cli/src/lib.rs` `rejected_what` (:288-300): `ChildAbort` → e.g. "the abort of a `par` child".
  3. `cargo insta accept`.
- Test changes: kind 1 (7.4); kind 2: the two snapshots.
- docs: native.md 「断る形」 (four forms); status.md L46 (native code runs effects and handlers; rejects `par`, task rows, `blocking` C) and L63 (drop エフェクト); testing.md 「ネイティブで流す」 L536 (residual rows occur only in std functions that hold a task row).
- Completion: G; the snapshots equal 7.5 (their counts can differ only through a fix commit that changes a file's forms, which its review states); `cargo nextest run -p eml_native -E 'test(unsupported::)'` → `8 tests run: 8 passed` (10 − 3 + 1); `grep -cE 'Form::(Prim|Clause|Prompt)' crates/eml_native/src/unsupported.rs crates/eml_cli/src/lib.rs` → `0` each; SC lines of N7; `cargo nextest run --profile timing -j 1 -p eml_cli --test integration -E 'test(=ui::run_native) | test(=ui::run_fail_native)'` → both PASS, no `SLOW`.

### CP-D

As CP-A, on N7's commit (the native UI on the four OS).

### Z: `Close jit-effects`

Roadmap 「進め方」, one commit:
1. spec: `grep -rnE 'jit-(effects|tasks|repl)' docs/spec docs/implementation` → empty; no 「今は〜、後で〜」 in the sections the stage wrote. Each prints ≥ 1: `sed -n '/^## エフェクト/,/^## /p' docs/spec/native.md | grep -c 'e:'`; same with `'m:'`; `sed -n '/^## externの失敗/,/^## /p' docs/spec/native.md | grep -c RAISE_PANIC`; `sed -n '/^### 欄/,/^### /p' docs/spec/object-model.md | grep -c CTX_YIELD_ABORT`; `grep -c fork_ev docs/spec/object-model.md`; `grep -c fork_ev docs/spec/vm.md`.
2. Measure (eml_rt, eml_vm, eml_lir changed): `bench/scripts.sh` first (a row out of range → `Resize bench rows` commit); then `bench/run.sh`, `bench/rt.sh`, `bench/scripts.sh` on the clean tree of the last code commit → replace the three tables of benchmarks.md 「今の数値」; `bench/native.sh` → replace 「ネイティブの数値」 (the `nrt_bench` rows and the native table); name `state`, `state_loop`, `state_loop_by_hand`, `state_nested`, `fold_state`, `ep_float`, `nbody`, `iter_generate`, `iter_pipeline`, `iter_pipeline_by_hand` in the prose. Without macOS: `Measure jit-effects` right after Z.
3. status.md: check N7's text.
4. architecture.md / CLAUDE.md check: `grep -c EFFECTS CLAUDE.md docs/implementation/architecture.md` ≥ 1 each; `grep -c 'effects.rs' docs/implementation/architecture.md` ≥ 1.
5. roadmap.md: delete the `jit-effects` item (L32); jit-tasks' 「前提: `jit-effects`」 → 「ネイティブのコードのエフェクト ([ネイティブの実行系](../spec/native.md)の「エフェクト」)」; docs/future/native.md 「中断と実行時エラー」 (L49-75) keeps only what remains future (task limits, `par` children) and points at spec/native.md.
6. overview.md (the line "native code runs everything but effects, tasks and blocking C functions") and README.md map: only if stale.
7. Confirm no decision lives only in the plan; delete `docs/plans/2026-11-11-jit-effects.md`; the appendix directory stays (jit-tasks, jit-repl).
8. Message: kinds 1-3 with reasons, the measurements, the U-H date (2026-11-14).
- Completion: G; every SC line; `cargo nextest run -p eml_cli --test integration -E 'test(/^citations::/)'` → no failed; `ls docs/plans | grep -c jit-effects` → `0`; ``grep -c '^- `jit-effects`' docs/future/roadmap.md`` → `0`; `grep -rn 'jit-effects' docs/spec docs/implementation docs/future docs/README.md docs/overview.md CLAUDE.md` → empty; native table `grep -c '| rejected | rejected |'` → `1` (`par_tree`), `grep -c differs` → `0`, `ep_float` and `nbody` have numbers.

### 7.3 Shared commands and standing checks

#### 7.3.1 G (every unit, repository root, after `git add -A`)
1. `cargo nextest run` → exit 0; summary `N tests run: N passed[ (k slow)], 23 skipped` with N ≥ 5143 [V: base 5143 / 23].
2. `cargo clippy --all-targets -- -D warnings` → exit 0.
3. `cargo fmt --check` → exit 0.
4. `bash nix/crate-graph.sh --check` → exit 0, and `git diff --exit-code be1b1de5 -- nix/crate-graph.sh` → exit 0 (G-14).

#### 7.3.2 SC (standing greps; from the unit named; all "always" lines hold at be1b1de5 [V])

```sh
# always
grep -rn 'thread_local' crates/eml_nrt/src crates/eml_native/src                                   # empty (G-14)
grep -c 'size_of::<Native>() == 72' crates/eml_rt/src/heap/ctx.rs                                  # 1 (G-14)
grep -rlnw 'eml_rt' crates/eml_native/src                                                          # crates/eml_native/src/mem.rs
grep -rln 'call_indirect' crates/eml_native/src | sort                                             # crates/eml_native/src/lower/apply.rs, crates/eml_native/src/thunk.rs (G-9)
grep -rnE 'ins\(\)\.(load|store|uload|sload|stack_load|stack_store)' crates/eml_native/src | grep -v -e src/mem.rs -e src/thunk.rs   # empty
grep -rnE 'CTX_[A-Z_]+\s*(:|=)' crates/eml_native/src crates/eml_nrt/src                           # empty
grep -rn 'Prelude\.' crates/eml_nrt/src                                                            # empty (G-7)
grep -rn 'native: skip' tests/ui | wc -l                                                           # 8
grep -rn 'docs/plans' crates | grep -v crates/eml_cli/tests/citations.rs                          # empty (citations.rs names the directory it skips)
git diff --exit-code be1b1de5 -- crates/eml_rt/src/trust.rs                                        # exit 0 (G-18)
git diff be1b1de5 -- crates/eml_rt | grep -cE '^\+.*(external_body|assume\()'                      # 0 (G-18)
# R1 on (G-1)
grep -rlE '\b(EV_TABLE|EV_STATE|EV_OUTER|EV_WORDS|NODE_EV|NODE_NEXT|NODE_WORDS)\b' crates | grep -v '^crates/eml_rt/'   # empty
grep -nE '\b(EV|NODE)_[A-Z]+\b' crates/eml_rt/src/lib.rs crates/eml_rt/src/heap/mod.rs crates/eml_rt/src/layout.rs     # empty
# V1 on (G-2, G-13)
grep -cE 'set_field|alloc_immortal|data_first' crates/eml_vm/src/compile/forks.rs                  # 0
awk '/^fn fork_evv/,/^}/' crates/eml_vm/src/vm.rs | grep -cE 'set_field|scanned\(|alloc_data|data_first|\.meta\('   # 0
awk '/^fn child_abort/,/^}/' crates/eml_vm/src/vm.rs | grep -cE 'set_field|scanned\(|alloc_data|data_first|\.meta\('  # 0
# G-3: the abort byte
grep -l 'mem::yield_abort(' crates/eml_native/src/lower/*.rs | sort    # N1, N2: unwind.rs; N3 on: effects.rs, unwind.rs
# target loads
grep -rln 'mem::yield_target(' crates/eml_native/src | sort           # base..N2: lower/apply.rs (base: apply.rs, unwind.rs); N3, N4: apply.rs, effects.rs; N5 on: apply.rs, effects.rs, unwind.rs
# N3 on (G-8: no lowering-side entry names)
grep -rnE '"eml_rt_(evv|ev|yield|root|dummy)' crates/eml_native/src                                 # empty
# N6 on (G-10, G-11)
grep -rn 'eml_nrt_raise_arg\|RAISE_DONE' crates docs/spec                                          # empty
# N7 on (G-16)
grep -cE 'Form::(Prim|Clause|Prompt)' crates/eml_native/src/unsupported.rs                         # 0
grep -rln 'native: skip' tests/ui/run/effects tests/ui/run/unwind tests/ui/run/panic tests/ui/run-fail/panic   # empty
```
`docs/future` is not in the "always" list: 9 citations of `docs/future/multicore.md` exist in eml_rt / eml_vm today [V]; new code adds none (review).

#### 7.3.3 EG (commits touching `crates/eml_rt`: R1, R2, R3, R4, N1; G-15)

```sh
cargo verus verify -p eml_rt                                         # verification results:: N verified, 0 errors; no warning
verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')   # exit 0
/usr/bin/time -p nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::                       # pass; user <= 280 s
/usr/bin/time -p nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows" cargo miri test -p eml_rt trusted::   # pass; user <= 365 s
bench/rt-gate.sh <parent commit>                                     # exit 0, clean tree, 5 pairs (macOS)
```
Procedure: commit, run the gate on the clean tree, amend the message with the gate's table (before any push). A failing gate is rerun once; a second failure stops the unit (no `--allow` without the orchestrator). Measured for R1's tree: 3 pairs exit 0, gated medians −0.029 % .. +0.051 % [V: 08 §7 item 12].

#### 7.3.4 Checkpoint extras
Before CP-B, CP-C and CP-D:
```sh
nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run -- -D warnings   # exit 0
nix develop .#cross -c cargo clippy --target x86_64-unknown-linux-gnu --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run -- -D warnings # exit 0
```
ASan before CP-C (N6's completion).

### 7.4 Test changes by kind

**Kind 1 (pass / fail), one by one (N7):**

| Test | Change | Reason |
|---|---|---|
| `eml_native` `unsupported::prim_is_rejected` | deleted | effect prims are lowered (E-D3, E-D9) |
| `unsupported::clause_is_rejected` | deleted | clause calls are lowered (E-D5) |
| `unsupported::prompt_is_rejected` | deleted | landings are lowered (E-D6) |
| `unsupported::forms_are_sorted_and_distinct` | rewritten: same source; asserts the result is exactly `[Form::Par, Form::ChildAbort]` | the forms left [V: probe prints `[Par, ChildAbort]`] |
| `unsupported::child_abort_is_rejected` | added: the same source; `Form::ChildAbort`, function `fork$Prelude.Panic.panicked` [V: probe] | E-D9 |

No `native: skip` mark is added.

**Kind 2 (expected values), scope:**
- R4 (user-approved, 00 G.3 U-K): `heap::tests::share::checked::share_stops_at_immortal_objects`, `heap::tests::share::share_leaves_an_immortal_lin_object_alone`, `heap::tests::transfer::checked::shared_counts_above_one_and_immortal_objects_stop` keep their claims on an uncounted immortal (frozen and attached, or the dummy), because sharing a value that reaches a counted immortal is now refused; if one cannot, stop (kind 1 for the user). The `fast::` variants of the two `both_modes!` tests change too (`mortal()` 1 → 0, the immortal now lives in a frozen region). In `share_leaves_an_immortal_lin_object_alone` the sub-claim "releasing a shared parent gives back a counted child's reference" is dropped: that state is now illegal in checked mode (reviewed as within kind 2).
- N1: `clif::a_call_and_its_unwind_check`, `clif::an_apply_site`; eml_rt `the_native_offsets_are_pinned`.
- N7: `integration__ui__run_native.snap`, `integration__ui__run_fail_native.snap`.
- Contingency V1: `crates/eml_vm/tests/bench.rs` `RunStats` of `bench/` programs that use `par` only (08 A-7).
- Nothing else; another change stops the unit.

**Kind 3 (byte-identical expected values):** R1 (rename); V1 (reroute); N3 `DatumRow` literals (`crates/eml_nrt/tests/run.rs` :203, :210, :217); N6 `eml_nrt_failed` callers (`crates/eml_nrt/tests/services.rs` :119, :164, :197) and `Image` literals (`crates/eml_nrt/tests/run.rs` :27, :225, :352, :406; `crates/eml_nrt/tests/services.rs` :37; `crates/eml_nrt/src/rows.rs` :188; `crates/eml_nrt/src/entries.rs` :532) [V: grep of `DatumRow {` / `Image {`].

**New tests:** R2 4 (×2 modes), R3 4 (×2), N1 6, N2 2, N3 10, N4 6, N5 8, N6 16, N7 1 (and 3 deleted): 62 more in the run [I], so ≥ 5205 after the stage.

### 7.5 Expected values (S-1)

Measured at be1b1de5 by replacing `unsupported`'s forms with 08 E-D9's (`Form = Par, AsyncRow, BlockingC, ChildAbort`; `ChildAbort` = `Rhs::Prim(Prim::ChildAbort, _)`) and skipping the native and fast runs in `native_entries` (the VM checked run of run-fail files kept, so `checked only` is real) [V]. These are the counts of the classifier; they equal the N7 snapshots when every newly accepted file agrees with the VM (otherwise a fix commit lands first; no skip mark is allowed).

`integration__ui__run_native.snap` after N7 (body):
```
accepted 489
skipped 6
rejected 151
  async row 129
  par 22
unlowered 27
  AsyncJobCell
  ChanToCell
  TlsPushTls
  TlsPushEof
  TlsPullTls
  TlsWritePlain
  TlsReadPlain
  TlsStatus
  TlsErrorMessage
  TlsSendCloseNotify
  TlsLineReady
  TlsTakeLine
  GuiCloseUi
  GuiBeginWindowRaw
  GuiBeginGridRaw
  GuiEndRaw
  GuiNextCellRaw
  GuiCreateRaw
  GuiAddItemRaw
  GuiSetTextRaw
  GuiSetIntRaw
  GuiCloseWindowRaw
  GuiOpenWindowsRaw
  GuiFlushRaw
  GuiPollRaw
  GuiEventIntRaw
  GuiEventTextRaw
```
`integration__ui__run_fail_native.snap` after N7:
```
accepted 45
checked only 0
skipped 2
rejected 34
  async row 33
  par 1
```
- No `child abort` or `blocking c` key [V]. `ShowBytesLength`, `ShowBytesGet`, `ProcStageCount`, `ProcWaitStage` leave `unlowered` [V].
- Newly accepted: 155 files, all previously `prim` (153 run + 2 run-fail) [V]. By directory: run/effects 33, run/unwind 15, run/panic 15, run/io 14, run/linearity 10, run/std 10, run/var 9, run/bytes 7, run/files 7, run/kinds 5, run/scripts 5, run/unboxed 5, run/arrays 4, run/ffi 3, run/borrow 2, run/byte 2, run/chars 2, run-fail/io 2, and 1 each in run/float, run/functions, run/records, run/reuse, run/strings. Full list: appendix A.
- All 111 entry files of `run/{effects,unwind,panic}` and `run-fail/panic` are accepted [V].
- `bench/native.sh` after the stage: 42 rows, 41 with numbers, `rejected` only for `par_tree` [V: the probe binary reaches lowering for `ep_float` and `nbody` and rejects `par_tree` with E0004 `par`]; `differs` 0 [I].
- Tests: ≥ 5205 run, 23 skipped [I]. Verus: R1 1250 [V]; R2, R3 strictly larger [I]; N1 equal to R3.
- Native UI timing: base 1.14 s / 0.95 s [V]; after N7 within the 10 s timing override [I].

## 8. Execution order

```
S0 → R1 → R2 → R3 → R4 → V1 → [CP-A]
   → N1 → N2 → N3 → [CP-B]
   → N4 → N5 → N6 → [CP-C]
   → (fix commits, if any) → N7 → [CP-D]
   → Z → [CP-Z] → merge
```

| Phase | Units | Native UI counts (run / run-fail accepted) |
|---|---|---|
| 0 | S0 | 336 / 43 |
| eml_rt + VM | R1..V1 | 336 / 43 (snapshots byte-identical) |
| native lowering | N1..N6 | 336 / 43 (byte-identical; `unsupported` unchanged, new code reached only by codegen tests) |
| accept | N7 | 489 / 45 (7.5; upper bound until the fix commits, if any, land) |
| close | Z | unchanged |

Order rationale: eml_rt first (08 U-5 default), so the gate runs on R1..R4 and N1 only. R2 (heap constructors) precedes R3 because R3's tests and `fork_ev`'s fast-mode precondition need a real fork table. N1 precedes N3 so the first effect-prim lowering is already behind the abort-byte check. CP-B splits the native lowering so a Win64 / Linux-only failure is localised to at most three commits. N5 needs N4 (the clause of a landing frame must be callable); N6 needs N4 (the clause call helper) and N5 (`try_*` lands).

## 9. Verification plan

- Baseline (S0, before the first code change; macOS M-series, clean tree, after `cargo build --all-targets`; fill "Recorded"):

| Item | Command | Expected [how known] | Recorded |
|---|---|---|---|
| full run | `cargo nextest run` | `5143 tests run: 5143 passed, 23 skipped` [V: run at be1b1de5 in a clean clone, 98 s] | `5143 tests run: 5143 passed, 23 skipped` (57 s); equal |
| eml_native integration | `cargo nextest list -p eml_native --test integration --message-format oneline \| wc -l` | 137 [V: list] | 137; equal |
| eml_rt tests | `cargo nextest list -p eml_rt --message-format oneline \| wc -l` | 911 [V: list] | 911; equal |
| eml_rt `heap::tests::ctx` | `cargo nextest list -p eml_rt -E 'test(/^heap::tests::ctx::/)' --message-format oneline \| wc -l` | 73 (72 in `fast::` / `checked::` + `the_native_offsets_are_pinned`) [V: list] | 73; equal |
| native snapshots | `cat crates/eml_cli/tests/snapshots/integration__ui__run_native.snap crates/eml_cli/tests/snapshots/integration__ui__run_fail_native.snap` | run: accepted 336 / skipped 6 / rejected 304 (async row 129, par 22, prim 153) / unlowered 31; run-fail: accepted 43 / checked only 0 / skipped 2 / rejected 36 (async row 33, par 1, prim 2) [V: file] | run: accepted 336 / skipped 6 / rejected 304 (async row 129, par 22, prim 153) / unlowered 31; run-fail: accepted 43 / checked only 0 / skipped 2 / rejected 36 (async row 33, par 1, prim 2); equal |
| native UI timing | `cargo nextest run --profile timing -j 1 -p eml_cli --test integration -E 'test(=ui::run_native) \| test(=ui::run_fail_native)'` | both PASS; record the times (M-series Mac: 1.140 s and 0.953 s [V]) | both PASS; 1.190 s (`run_native`) and 0.429 s (`run_fail_native`) on the second run (M5 Mac); the first run, cold, took 2.435 s and 1.158 s |
| Verus | `cargo verus verify -p eml_rt` | `verification results:: 1243 verified, 0 errors` [V: 08 §7 item 1] | `1243 verified, 0 errors`; equal |
| verusfmt | `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` | exit 0 [V] | exit 0; equal |
| Miri eml_rt | `/usr/bin/time -p nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::`, and with `-Zmiri-tree-borrows` added | record `user` (budgets 280 s / 365 s) | `user` 52.17 s (strict-provenance) / 47.29 s (+ tree-borrows); 175 passed each; within budgets |
| Miri eml_vm | same two settings, `cargo miri test -p eml_vm --test integration miri::` | record `user` (budgets 300 s / 400 s) | `user` 140.40 s / 123.21 s; 16 passed each; within budgets |
| Miri eml_nrt | same two settings, `cargo miri test -p eml_nrt --lib -- entries:: census:: constants::` | record `user` (no budget) | `user` 5.46 s / 5.41 s; 39 passed each |
| bench/native.sh | `bench/native.sh > $TMPDIR/native-base.md` | 42 program rows, 39 with numbers, `grep -c '\| rejected \| rejected \|'` → 3 (`ep_float`, `nbody`, `par_tree`), `grep -c differs` → 0 [V: benchmarks.md L445, L456, L459] | 42 program rows, 39 with numbers, `rejected` x3 (`ep_float`, `nbody`, `par_tree`), `differs` 0; equal |
| skip marks | `grep -rn 'native: skip' tests/ui \| wc -l` | 8 [V] | 8; equal |
| crate graph | `bash nix/crate-graph.sh --check` | exit 0 [I] | exit 0; equal |

- Per unit: the unit's completion criteria, G (7.3.1), the SC lines in force (7.3.2), EG for eml_rt commits (7.3.3), the checkpoint extras before CP-B / CP-C / CP-D (7.3.4).
- Overall (after Z, on the Close commit):
  - `cargo nextest run` → `N tests run: N passed, 23 skipped`, N ≥ 5205 [I: 5143 + 62 new].
  - `cargo nextest run -p eml_cli --test integration -E 'test(=ui::run_native) | test(=ui::run_fail_native)'` → pass with the bodies of 7.5 (S-1).
  - `cargo verus verify -p eml_rt` → `N verified, 0 errors`, N > 1250 (S-4).
  - every SC line of 7.3.2 → its stated value (S-5).
  - `bench/native.sh` → `grep -c '| rejected | rejected |'` → 1 (`par_tree`), `grep -c differs` → 0 (S-6).
  - CI workflow `CI` on the Close commit → `success` on every job (S-7).
- Manual checks by the user: none required. Optional: `timeout 20 cargo run -p eml_cli -- run --native tests/ui/run/effects/multi_choice.em` prints the same as without `--native`.

## 10. Risks and unresolved items

| # | Risk / item | Default (who / when) |
|---|---|---|
| K-1 | R2's or R3's proof does not close, or needs a `requires` beyond 00 G.2 | 08 E-D2 stop rule: stop; the orchestrator takes 08's alternative to the user |
| K-2 | 08 A-2: checked-mode `evv_push` refuses a fork evidence or the frozen tail | stop in V1; the orchestrator decides on 08's `Ctx::fork_node` (an eml_rt commit with EG) |
| K-3 | 08 A-3: the I-land verifier rule fails on a UI program | stop in N1; back to design |
| K-4 | a newly accepted file disagrees with the VM | `Fix native <topic>` commit with a regression test before N7 (Opus); no skip mark |
| K-5 | rt-gate noise on R2 / R3 / N1 | rerun once; then stop (orchestrator decides on `--allow`) |
| K-6 | V1 changes a `bench.rs` pin | kind-2 contingency for `par` programs only; else stop |
| K-7 | Miri budget exceeded by the T8 additions | measure in each EG run; above half the budget, follow testing.md 「Miri」 |
| K-8 | 08 E-D6's yield-side rule (shared with the VM's fast mode) is violated by some program | shows as a disagreement in N7 (K-4); a memory-safety finding goes to a strong advisor first (CLAUDE.md) |
| K-9 | macOS unavailable at Z | `Measure jit-effects` right after Z |
| K-10 | pushes | settled: the user allowed pushes of `plan/jit-effects` for the CI checkpoints (2026-11-14); merging or opening a PR is asked at the end |

## Appendix A: files newly accepted by native code after N7 [V]

```
run-fail/io: fail_io_uncaught.em try_io_does_not_catch_arith.em
run/arrays: basics.em extended.em read_out_of_range_caught.em shared_set_copies.em
run/borrow: demoted_tail_call_of_pair.em reader_after_each_resume.em
run/byte: overflow_try_panic.em std_byte.em
run/bytes: append_slice.em decode.em decode_try_panic.em get_set.em push_loop.em slice.em truncate.em
run/chars: ascii_classes.em of_int_try_panic.em
run/effects: continuation_in_inner_handle.em continuation_multi_passed.em continuation_partial.em continuation_passed.em effect_abort.em effect_parameters.em in_place_clause_catches_its_own_failure.em in_place_clause_failure_reaches_outer_handler.em in_place_clause_io_failure_reaches_outer_try_io.em io_twice.em mask_behind.em mask_behind_continuation.em mask_behind_handles.em mask_behind_multi.em mask_behind_state.em mask_callback.em mask_inside_handler.em mask_multi.em mask_never.em mask_twice_multi.em multi_captured.em multi_choice.em never_in_function_value.em operation_variable_in_a_nested_tuple.em over_application_yields.em pure_callback_under_state.em resume_reaches_handlers_around_the_resumption.em resume_under_a_handler_of_the_same_effect.em specialized_never_operation.em state_passing_panic.em state_passing_under_generator.em struct_yield_dummy.em transform_handler.em
run/ffi: fixture_dot.em libm_abs_narrow.em libm_memcpy.em
run/files: file_in_return_clause_on_abort.em file_out_released_on_abort.em file_released_on_abort.em file_state_across_an_outer_once_operation.em files_released_by_abort_across_frames.em read_bytes.em state_released_on_abort.em
run/float: std_float.em
run/functions: local_function_effects.em
run/io: exit_releases_files.em fail_io.em stdin_handles.em try_io_callback.em try_io_clause_outside.em try_io_cmd_exit.em try_io_kinds.em try_io_missing_file.em try_io_missing_program.em try_io_nested.em try_io_redirect_missing.em try_io_releases_files.em try_io_releases_handles.em try_panic_catches_in_use.em
run/kinds: multi_continuation_in_once_position.em never_row_bound.em row_parameters.em scheduler.em unr_closure_across_multi_into_once_position.em
run/linearity: captured_file_in_a_clause.em consumed_after_the_call.em diverging_argument_beside_a_file.em diverging_list_element.em diverging_match_arm.em fail_without_drop_k.em local_function_capturing_a_file.em matched_data_on_another_branch.em matched_tuple_on_another_branch.em two_diverging_calls.em
run/panic: catches_division_by_zero.em catches_overflow.em catches_slice_out_of_range.em catches_user_panic.em clause_panic_is_caught_outside.em exit_passes_try_panic.em file_released_on_panic.em nested_inner_catches.em ok_without_panic.em panic_inside_iter.em panic_through_list_map.em panic_through_state.em struct_return_panics.em try_io_ignores_panic.em try_panic_ignores_io_failure.em
run/records: structs_in_uniform_positions.em
run/reuse: multi_resume_rebuilds.em
run/scripts: csv2jsonl deploy grep logstat wc
run/std: bytes_with_capacity.em iter_file proc_capture_bytes.em proc_env.em proc_pipeline.em proc_signal_exit.em proc_spawn.em std_fail.em std_iter.em string_chars.em
run/strings: borrow_fails.em
run/unboxed: basics.em conversions.em generic.em int_elements.em shared_fill_and_thaw.em
run/unwind: abort_in_fn_clause.em abort_through_variant_and_apply_rest.em apply_rest.em borrowed_field_across_extern.em fail_deep_with_files.em from_return_clause.em inner_state_file.em io_extern_as_value.em io_failure_deep_with_files.em io_failure_in_io_only_function.em mask.em mask_in_non_yielding_function.em multi_resumed_twice.em reuse_token_io_failure.em state_holding_inner_continuation_fail.em
run/var: abort_across_ep_scope.em array_var_index.em index_panic_reaches_use_site.em index_panic_recovers.em last_expression_reads.em polymorphic_reentry.em same_name_sites.em taken_then_closure_panics.em three_arrays.em
```

## Appendix B: main changes from review (refresh 2026-11-14)

Only what keeps the implementer from relitigating.
- Design (08, Fable, one adversarial review round):
  - 05 D-2's `target` test plus a non-cold `split` → the `abort` byte (E-D6); a yield takes no branch. Verified basis: `YieldState::wf` gives `target == () ==> !abort`; only `yield_abort` / `exit_abort` set `abort`.
  - 05 D-6's spec/const role pairs → plain crate-private consts; measured: the rename verifies with no proof edit (1250 verified).
  - 05 D-6's `fork_plan` / `ForkTables` / shared `fork_evv` → deferred to jit-tasks (no second caller yet); eml_rt constructors now.
  - 05 D-4's `eml_nrt_raise_arg` and 2-word `out` → a 4-word `out`; `eml$raise` is program-independent with fixed signatures, guarded in every build by `LoadError::Prelude` and the clause-signature assert (review C-1: a debug-only guard was unsound in release).
  - A clause closure has tag 0, so a generic `apply` of one traps (review B-2: the LIR verifier is debug-only).
  - Engine panics are caught with no purity filter: the VM does the same (`vm.rs:4212-4221`, `ffi.rs:410-424`) (review C-3).
  - Review C-2's "double free" framing rejected: a frame that wrongly takes its cold block releases its references once and returns; the outcome is a reported leak, not memory unsafety. I-land is nevertheless checked by a verifier rule and a test (N1).
- Units (Opus, one adversarial review round):
  - Test filters use the prefixes `fork_` / `evv_step_` (two existing eml_rt tests contain `fork`).
  - The baseline 5143 is a real `cargo nextest run` count (98 s at be1b1de5), not a list count.
  - The rename script is in R1 (no dependence on a scratch commit); it also renames the leftover literal at `proof.rs:524`.
  - The landing block gets its own CLIF pin (`a_landing_point`); N1 gets a run-time test of the `n < m` arm's cold path.
  - An extra checkpoint after N3 shortens the bisect window.
- Execution amendments (2026-11-14): the fork constructors and `Ctx` fork operations are `pub unsafe fn` (U-I), N6's Prelude check also covers `ret == OpRet::Never` and `eml$raise` traps when the abort byte is clear (U-J), N1's verifier rule is widened (U-J), plus mechanical corrections to tests and counts. Full list with old -> new lines: 00 G.3.
