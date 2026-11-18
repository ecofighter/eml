# Stage sched-proofs: the wake-up protocol and the verdict for every number of workers

- Created: 2026-11-17 (design-level plan).
- Base: the `Close vm-retire` commit (the stage needs only `sched-core`; it runs last in the listed order). Facts in 01 describe main @ 4a2954c1; the refresh re-checks them.
- Status: design-level. `Start sched-proofs` replaces this file with an executable plan in the model of `2026-11-17-engine-trust.md`.
- Appendices: `docs/plans/2026-11-17-runtime/`; 00 wins; the design is 01 §12.3 and §14.1 P-4b, with 01 §10 D-SC-7 (the three assertions), §14.3, §22 U-4. This is the group's last stage: its Close deletes `docs/plans/2026-11-17-runtime/` (00 §D D-2; roadmap 「進め方」 step 8).

## 1. Goal and completion conditions

- Goal: a Verus proof, in `crates/eml_steps/src/proto.rs`, that a spec state machine whose `next` is the specs of `idle::step`, `verdict::step`, `Sleepers` and `Open` has an inductive invariant implying the safety assertions of D-SC-7 for any number of workers and publishes.
- Completion conditions:
  - `init ==> inv`, `inv && next ==> inv'`, `inv ==> safe` verified (`cargo verus verify -p eml_steps`), with no `external_body`, `assume` or `assume_specification` (`trust.rs` unchanged).
  - The Close states which of D-SC-7's three assertions the invariant covers ((1) no listed job with every taker asleep and no wake in flight; (2) worker 0 never reaches the verdict's end while work remains; (3) every state with no enabled step is the verdict's end).
  - If the invariant is not found within the stage's four units (U-4), P-4a (the explorer) stays the standing check, the item returns to the roadmap's 「その後の項目」 with what was learned, and the Close says so.
  - The roadmap item 「協調的な配置のVerusの証明」's wake-up half is closed (the item kept only its placement half at `Start engine-trust`).

## 2. Scope

- In: `proto.rs`; the paper invariant; the proof.
- Out: a refinement proof of the scheduler's shell (01 §14.4 "Not proposed"); `rt-concurrent-proofs` (deferred).

## 3. Received from earlier stages

- `sched-core`: `eml_steps` with `idle::step`, `verdict::step`, `Sleepers`, `Open` and their specs; the explorer.
- `vm-retire`: one engine; no change to the kernels is expected.

## 4. Decisions

- 01 §14.1 P-4b; the proof is tied to the code by using the shipped functions' specs as `next` (01 §14.1 "How the proofs are tied to the code"); ST-1..ST-3 stay trust items of runtime.md 「信頼の一覧」.

## 5. Units (in order)

1. PR-1 `Start sched-proofs`: plan and the invariant on paper.
2. PR-2: the model over `idle::step`, `verdict::step` and the publish steps.
3. PR-3: the inductive proof.
4. PR-4 `Close sched-proofs` (the roadmap item's wake-up half closed; the appendix directory deleted).

## 6. Test changes by kind (scope)

- None. The explorer stays.
- A kind 1 or kind 2 change found during execution that the executable plan does not list stops the unit for a plan amendment approved by the user.

## 7. docs/spec sections written

runtime.md 「信頼の一覧」 (what P-4b proves and what stays trusted); docs/future/verification.md (the scheduler's kernels as the precedent of a proved leaf crate, if not already written by `sched-core`).

## 8. Acceptance checks

- `cargo verus verify -p eml_steps`; `verusfmt --verus-only --check` over `crates/eml_steps/src`; CLAUDE.md's gates.
- `ls docs/plans` → no `2026-11-17-*` entry after the Close (the group is finished); before deleting the directory, check that no decision of 00 or 01 lives only there (roadmap 「進め方」 step 8): every rule is in `docs/spec`, `docs/implementation` or the roadmap.

## 9. Risks with defaults

| Item | Default |
|---|---|
| U-4: no invariant within 4 units | P-4a stands; the roadmap item returns with its condition |
| The spec functions are too weak for the invariant | strengthen their `ensures` in `eml_steps` (a proof change of `sched-core`'s code, with its tests), never an `assume` |

## 10. Refresh at the stage's first commit

Re-read `sched-core`'s kernels and specs as they are after `vm-retire`; record the Verus count.
