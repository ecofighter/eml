# Stage native-checked: native code gets the checked mode and `--stats`

- Created: 2026-11-17 (design-level plan).
- Base: the `Close std-par` commit. Facts and line numbers in 01 describe main @ 4a2954c1; the refresh re-checks them.
- Status: design-level. `Start native-checked` replaces this file with an executable plan in the model of `2026-11-17-engine-trust.md` (deleted at its Close; read it with `git show 97cd579c:docs/plans/2026-11-17-engine-trust.md`).
- Appendices: `docs/plans/2026-11-17-runtime/`; 00 wins; the design is 01 §9 (D-NC-1..D-NC-10), with 01 §14.1 P-5, §16 S-24, §18 G-6, G-7, G-22 (H-4), §15.2 H-4.

## 1. Goal and completion conditions

- Goal: on the programs native code accepts, `eml run --native --debug-heap` and `--native --stats` behave as the VM's checked mode: every inline memory access is preceded by a verified audit, every direct heap entry call is the full checked operation, and the counters equal the VM's.
- Completion conditions:
  - G-6: every module of `lower/` that names one of the nine `mem::` access helpers (`load_rc`, `store_rc`, `load_field`, `store_field`, `load_meta`, `load_tag`, `closure_code`, `load_box`, `is_big_int`) emits the audit through `lower/audit.rs` (placement test; the module list is `rc`, `data`, `apply`, `unwind`); checked lowering emits the same instructions after the audit (twin CLIF pins); `eml_rt_dup_slow` and `eml_rt_decref_slow` are named only by `Step::slow()` (placement test).
  - P-5 / S-24: for each of the seven audited operations `audit_x` ensures `r is Ok ==> x_passes(w)`; `x_body` keeps `wf()` under `x_passes` (checked) or `x_ok` (fast); `x = audit_x; x_body` keeps today's contract; Verus passes with no `external_body` or `assume` added.
  - G-7 (D-NC-10): the UI harness runs every accepted `run` / `run-fail` file in checked native mode too and compares with the checked VM: the error class of every failing run and every field of `RunStats::fields()` but `peak_stack` on every passing run; a difference is a failure.
  - G-22 (this stage's hook): a test over the entry descriptor rows checks each row's `class` (H-4).
  - The fast CLIF pins are byte-identical except the cold call's callee name (`eml_rt_decref` -> `eml_rt_decref_slow`).
  - `crates/eml_vm/tests/evidence.rs`, `reuse.rs` and the hand-written LIR programs of `crates/eml_lir/tests` run through the new machine helper of `eml_test_support` on both engines, checked.

## 2. Scope

- In: D-NC-1..D-NC-10; the two `eml_rt` units (NC-2, NC-3) with its gate; `eml_nrt`'s `Mode`-generic entries and two tables with the descriptor `class` (H-4); `lower/audit.rs`; the end-of-run checks and census in checked mode; `--stats`; the harness; the machine helper in `eml_test_support`; CI lines.
- Out: tasks and `par` natively (`jit-tasks`); a `checked` cargo feature (D-NC-6: none; fallback recorded); the strip test (`native-aot`).

## 3. Received from earlier stages

- `engine-trust` (not a prerequisite: the roadmap gives `native-checked` 前提: なし; the group order closes it first): `Trusted` at the native entries; the promise list in lir.md 「エンジンの約束」 (checked mode is not a sandbox: that sentence stays).
- `par-total`: no `child_abort` form; `JobEnd` is a value.

## 4. Decisions (01 §9)

- D-NC-1: audit before inline: the seven audits `eml_rt_audit_{dup, decref, field, store, unique, header, box}` with what each checks and counts (01 §9's table); a failing audit takes the fatal path; for a `tobj` / `evv` word the audit sits after the immediate test (NC-1 confirms against the VM's counts); the cold call of the inline decrement is the new `eml_rt_decref_slow`; in the checked table only `eml_rt_dup_slow` and `eml_rt_decref_slow` are bodies without the top-level audit and count; `eml_rt_decref` stays the full checked operation for its three direct callers (`unwind.rs` L302, L317; `apply.rs` L398).
- D-NC-2: tokens, allocation and the free on rc 1 are calls in checked lowering.
- D-NC-3: the switch sites go through one helper in `lower/audit.rs`; `mem.rs` stays the only module that knows offsets; `Ctx<Checked>` and `Ctx<Fast>` share the `Native` prefix.
- D-NC-4: NC-1 writes the table "LIR statement or helper -> the VM's checked operations and counters -> the native checked sequence", plus the direct heap entry calls as "full checked operation, no audit"; NC-5..NC-7 implement exactly that table.
- D-NC-5: the audits in `eml_rt` under Verus (`audit_x`, `x_body`, `x_passes`; `x_ok` and `word_in(w)` stay premises).
- D-NC-6: `entries!` emits every entry generic over `M: Mode` and two tables; the descriptor row gains `class`; no `checked` feature.
- D-NC-7: `RunStats` natively: from `Ctx<Checked>`, `Heap` and the audits; `boxes` / `unboxes` by one count call `eml_rt_audit_count(ctx, which)` at the head of each `box` / `unbox` statement; `unwound_frames` inside `FnLower::cleanup`; two placements are NC-1's stated decision (defaults: `eml$apply$m`'s cleanup counts only when arguments remain; thunk frames do not count); a field that cannot equal the VM's by construction is listed and deleted at `vm-retire`; `peak_stack` is bytes natively and not compared.
- D-NC-8: `--native --debug-heap` and `--native --stats`: owner checks, leak and invariant checks at the end, `TransferLeak`, census.
- D-NC-9: rule for later stages: every inline access added to generated code is preceded by its audit through `lower/audit.rs`, with a twin CLIF pin. A later stage may instead lower an operation in checked mode as a call that is the full checked operation (D-NC-2's form); then checked code has no inline access and the fast pin has no twin (00 §D D-15 does this for `Std.Atomic`).
- D-NC-10: parity while the VM exists (G-7).
- Criteria: G-6, G-7, P-5, S-24, H-4 (the descriptor `class`: `Alloc`, `Free`, `RcSlow`, `Share`, `Reuse`, `Audit`, `Row`, `Task`, `Service`).

## 5. Units (in order)

1. NC-1 `Start native-checked`: plan; D-NC-4's table; the counter probes (U-2); the two `unwound_frames` placements; where the audit sits relative to the immediate test; the UI timing measurement of section 9's first risk (with `cargo nextest run --profile timing -j 1` on x86_64 Linux, as CI measures; not macOS); baseline (Verus count, CLIF pins).
2. NC-2: `eml_rt`: `audit_dup` / `audit_decref` and the bodies (gate).
3. NC-3: `eml_rt`: the field, store, unique, header and box audits (gate).
4. NC-4: `Mode`-generic entries, two tables, `class`, `eml_rt_decref_slow`, the checked table's two body entries, the placement test on `Step::slow()`.
5. NC-5: `lower/audit.rs`, `lower/rc.rs`, twin pins.
6. NC-6: `lower/data.rs` (fields, stores, boxes, tokens as calls), twin pins.
7. NC-7: `lower/apply.rs`, `lower/unwind.rs`, the count calls for `boxes` / `unboxes` / `unwound_frames`.
8. NC-8: end-of-run checks, census, `--debug-heap`.
9. NC-9: `--stats`.
10. NC-10: the harness (D-NC-10); the negative tests `shared_capture`, `fresh_result`; the machine helper in `eml_test_support`; `evidence.rs`, `reuse.rs` and the `crates/eml_lir/tests` programs on both engines, checked.
11. NC-11: CI lines; docs.
12. NC-12 `Close native-checked` (`bench/rt.sh` and `bench/native.sh` re-measured).

## 6. Test changes by kind (scope)

- Kind 1: the CLI tests that expect `--native --debug-heap` or `--native --stats` to be a usage error (NC-1 lists them; they become run tests).
- Kind 2: the callee name of the inline decrement's cold call in the fast CLIF pins (`eml_rt_decref` -> `eml_rt_decref_slow`), otherwise byte-identical; the native UI snapshots if the harness adds a checked column.
- Kind 3: the `Mode`-generic entries (every existing entry test keeps its expectation).
- New: the H-4 `class` test; the audit tests in `eml_rt` (both modes), twin CLIF pins, the placement tests, the negative tests, the parity comparison, the machine helper's tests.
- A kind 1 or kind 2 change found during execution that the executable plan does not list stops the unit for a plan amendment approved by the user.

## 7. docs/spec sections written

native.md 「検査モード」 (new); object-model.md 「検査モード」「核の操作」 ("audit, then body"; P-5's statement); native.md 「エントリ」 (two tables, `class`); testing.md 「ネイティブで流す」 (the checked column, the parity rule); benchmarks.md 「ネイティブの数値」; CLAUDE.md: the Commands sentence on `--native` ("not with --debug-heap, --stats" goes), the Testing bullet on `ui::run_native` / `ui::run_fail_native` (the checked comparison), and, if the timing risk below fires, the "seven tests" sentence of the Testing section. docs/spec/native.md 「VMとの違い」 item 2 (its 「`--debug-heap`も`--stats`も」 clause); docs/future/native.md 「テスト」: the first bullet's skip mark for tests that need checked mode, the bullet that calls the end-of-run census the substitute for checked mode, and the bullet 「`RunStats` のスナップショットは VM だけのものである」 are rewritten to the checked native runs.

## 8. Acceptance checks

- CLAUDE.md's gates; the `eml_rt` gate on NC-2 and NC-3 (Verus count strictly larger; no `external_body` / `assume` added: `git diff <stage base> -- crates/eml_rt | grep -cE '^\+.*(external_body|assume\()'` → `0`).
- G-6's two placement tests and the twin pins pass.
- G-7: the harness's checked native column passes on every accepted file.
- `bench/native.sh` and the native table replaced at Close; fast native rows within the noise of the record (fast code changes only the cold callee's symbol).
- Roadmap: the `native-checked` item is deleted.

## 9. Risks with defaults

| Item | Default |
|---|---|
| The checked native run of every accepted UI file breaks the 1 s per file / 5 s per directory rule of `ui::run_native` (about 490 files accepted at the base [V: snapshot]) | NC-1 measures with `cargo nextest run --profile timing -j 1` on x86_64 Linux (CI's timing check); at the base on the macOS dev machine `ui::run_native` takes 1.43 s with `-j 1` [V: plan verification]. If over, the checked native comparison runs as two directory tests of their own, `ui::run_native_checked` and `ui::run_fail_native_checked`, each with the same 1 s per file / 5 s budget; the "seven tests" sentences of testing.md 「性能のテスト」 and of CLAUDE.md's Testing section become nine and name them |
| U-2: a `RunStats` field cannot equal the VM's by construction | listed at NC-1 with its reason; excluded from G-7's comparison; deleted at `vm-retire` (kind 2 there) |
| U-8: build time of the runtime library with both tables | acceptable; fallback: the feature of D-NC-6, declared by `eml_nrt` |
| `bench/rt-gate.sh` fails on NC-2 / NC-3 because `x = audit_x; x_body` changes inlining of the fast path | stop; no `--allow` / `--allow-bound` without the user's approval; the orchestrator brings the table to the user |
| A proof of NC-2 / NC-3 does not close | stop; the orchestrator takes it to a strong advisor (CLAUDE.md) |
| The VM counts immediates at an audit site (D-NC-1's open placement) | the audit moves before the immediate test, accepting an immediate; twin pins follow |

## 10. Refresh at the stage's first commit

Confirm 01 §27 item 2 (D-NC-4's table, the counter probes, the two `unwound_frames` placements, the audit relative to the immediate test) and the three direct `eml_rt_decref` callers' line numbers; record the baseline (Verus count, fast CLIF pins, native snapshots, `bench/native.sh`).
