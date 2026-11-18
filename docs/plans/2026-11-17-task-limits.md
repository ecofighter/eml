# Stage task-limits: memory has one limit; the limit classes are `Stops`, `Task`, `Aborting`

- Created: 2026-11-17 (design-level plan).
- Base: the `Close par-total` commit. Facts and line numbers in 01 describe main @ 4a2954c1; the refresh re-checks them.
- Status: design-level. `Start task-limits` replaces this file with an executable plan in the model of `2026-11-17-engine-trust.md`.
- Appendices: `docs/plans/2026-11-17-runtime/`; 00 wins (00 §D D-5 restates G-4's check); the design is 01 §7 (D-TL-1..D-TL-4), with 01 §3 SC-8, §14.2 (the soft threshold's glue), §18 G-4.

## 1. Goal and completion conditions

- Goal: `max_bytes` is the only memory limit and ends the run; the soft memory limit, the monitor's pressure role, the per-task byte counts and the classes `Pending` and `Aborting { pending }` are deleted; the depth limit of a task stays a catchable `ResourceLimit`.
- Completion conditions:
  - G-4, as restated by 00 §D D-5: a grep scoped to the soft-limit items is empty. Default text (TL-1 confirms it against the tree): `grep -rnE 'pressure_tick|claim_pressure|soft_bytes|mark_limited|limit_task|clear_limit|limit_forwarded|add_bytes|Limit::Pending|Aborting \{ *pending|Notice::Limit|MailItem::Limit|soft_of|flagged|const MEMORY' crates/*/src` → empty, and `grep -rn 'fn pressure' crates/eml_rt/src` → empty (at 4a2954c1 `Post::limit` matches nothing and is not in the list; `flagged` has 12 hits in `eml_vm` and `eml_rows` and `const MEMORY` is a limit class D-TL-2 deletes [V: plan verification]; TL-1 checks that each remaining pattern hits only soft-limit code).
  - The VM's `limit_hit` keeps its steps without the flag paths (D-TL-1): step 6's message choice and step 8's `clear_limit` are gone; step 9 is "no `IO` evidence: end the run".
  - `limit::every_probe`'s table is restated over `class x yielding x row kind x beyond headroom x over real` (kind 2).
  - `eml_rt` passes its gate on TL-3; `bench/run.sh` rows within +0.5 %.

## 2. Scope

- In: D-TL-1..D-TL-3 in the VM, `cells.rs`, `eml_cli` and `eml_rt/src/control.rs`; the docs of TL-4.
- Out: the native safepoint's limit decision (`sched-core` writes `eml_steps::limit::hit`; `jit-tasks` calls it); the monitor's other roles (kept, D-TL-3).

## 3. Received from earlier stages

- `async-root`: no nested root and no shared depth budget (D-AR-6).
- `par-total`: children run at `Stops` (D-PT-5); its sentence on soft-limit flags seen during a child is deleted here.

## 4. Decisions (01 §7)

- D-TL-1: `Aborting` has no field; `limit_hit`'s steps as above.
- D-TL-2: deleted (01 §7 lists every item): the monitor's pressure role (`pressure_tick`, `choose`, `claim_pressure`, the `pressure` argument of `Monitor::start`, the second trigger of `start_monitor`); `Notice::Limit`, `MailItem::Limit`, `Post::limit`; in `cells.rs` the cell fields `limit` and the byte count, `clear_limit`, `limit_task`, `limit_forwarded`, `add_bytes`, `TaskMarks.limit` / `.bytes`; in `eml_rt/src/control.rs` `Limits.soft_bytes`, the threshold `next`, its two readers, `claim_pressure`, and `Control::taken` if no reader remains; in `eml_vm` `Policy::MEMORY`, `Memory`, `Task.counted` / `.added`, `sched.mark`, `publish`, `clear_top`, `end_slice`, the classes `Pending` and `Aborting { pending }`, `flagged`, `mark_limited`, and the slot words `current`, `top`, `bytes`, `mark` with their version counter if no reader remains; `eml_cli`'s `soft_of`; `eml_rows/src/rows.rs` L922.
- D-TL-3: kept: the monitor thread (signal relay, `wake_all` on stop bits), `max_bytes`, the depth deferral.
- D-TL-4: gate: correctness; `bench/run.sh` rows within +0.5 %; the `eml_rt` gate for TL-3.
- Criteria: G-4 (restated), S-14's VM half (no unwinding action while yielding or aborting stays true in `limit_hit`).

## 5. Units (in order)

1. TL-1 `Start task-limits`: plan; the kind-1 list; `Control::taken`'s readers and the VM's `current` / `top` readers (01 §27 item 4); G-4's scoped grep written; baseline.
2. TL-2: the VM, `cells.rs`, `eml_cli` (D-TL-1, D-TL-2 outside `eml_rt`).
3. TL-3: `eml_rt` `control.rs` (its gate; the trust list's sentence on the soft threshold shortened).
4. TL-4: docs (section 7).
5. TL-5 `Close task-limits` (`bench/rt.sh` re-measured with the other tables).

## 6. Test changes by kind (scope)

- Kind 1 (TL-1 lists one by one): `run/async/limit_memory_after_catch`, `limit_memory_caught`, `limit_memory_fails_scope`, `run-fail/async/limit_memory_uncaught`; the unit tests of the soft limit and of the monitor's pressure role (including `eml_rt`'s `claim_pressure_moves_by_a_quarter_of_the_headroom`, `crates/eml_rt/src/heap/tests/control.rs:149` at 4a2954c1).
- Kind 2: `limit::every_probe`'s table; `RunStats` snapshots that count the removed slot-word publications, if any (TL-1 finds them).
- Kind 3: the rest.
- A kind 1 or kind 2 change found during execution that the executable plan does not list stops the unit for a plan amendment approved by the user.

## 7. docs/spec sections written

vm.md 「上限と割り込み」「タスクの上限」; stdlib.md 「タスクの上限」; object-model.md 「実行の制御」 and the trust list's sentence on the soft threshold.

## 8. Acceptance checks

- CLAUDE.md's gates; the `eml_rt` gate on TL-3.
- G-4's scoped grep empty.
- `bench/run.sh` within +0.5 %; the three tables replaced at Close.
- Roadmap: the soft-limit items of 「その後の項目」 (「待ちのタスクが持つメモリを数えること」, 「本当の待ちから起きてまだ動いていないタスクを、メモリの柔らかい上限の候補にすること」, 「実行待ちのタスクを worker ごとに2つ以上公開すること」, 「`par` の右の子と `compute` の仕事の確保をタスクに数えること」, 「右の子がほかの worker で動く間の、親のタスクへの旗」, 「同じセルのエントリの `Task` が重なるときのタスクの数」) are deleted (their premise is gone); the `task-limits` item is deleted.

## 9. Risks with defaults

| Item | Default |
|---|---|
| `Control::taken` keeps a reader outside the soft limit | it stays |
| The slot words `current` / `top` have a reader other than the soft limit | they stay; G-9's pattern is unaffected (it names only the new files of `eml_sched`) |
| A UI file depends on the soft limit beyond the four of section 6 | kind 1, listed at TL-1 |

## 10. Refresh at the stage's first commit

Confirm 01 §27 item 4, the line numbers of `eml_rt/src/control.rs` and `eml_rows/src/rows.rs` L922, and the scoped G-4 grep against the tree; list kind 1; record the baseline (full run, `bench/run.sh`, Verus count).

Performance gate procedure (01 §19 "Gates"; the executable plan writes it out): `bench/run.sh` prints one table of medians and the build directory changes counts (benchmarks.md L162), so base and head are built and measured alternately in one worktree (`git switch --detach <stage base>`, `bench/run.sh`, `git switch <head>`, `bench/run.sh`, three pairs, the median per row). The compared column is 実行の分 (instructions minus `empty`'s). Rows compared: every row of `bench/run.sh`. A row over +0.5 % stops the stage; the orchestrator brings the table to the user.
