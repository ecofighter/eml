# Stage ir-origins: every statement carries where it came from

- Created: 2026-11-22 (design-level plan).
- Base: the `Close native-checked` commit. Facts and line numbers in the design describe main @ 0f04d6a2; the refresh re-checks them.
- Status: design-level. `Start ir-origins` replaces this file with an executable plan in the model of the executable plans of earlier stages (for example `git show 6177715b:docs/plans/2026-11-21-remote-reclaim.md`).
- Appendix: `docs/plans/2026-11-22-profiler/` (`design.md`, `prototype.patch`), shared with `native-prof` and deleted at the Close of `native-prof`. The design is `design.md` sections 3 to 5, with sections 8.1, 8.2, 9, 10 and Appendix A. Decisions: `docs/plans/2026-11-17-runtime/00-decisions.md` §D D-20, which replaces D-MT-1 of 01 §15.1.

## 1. Goal and completion conditions

- Goal: Core IR and LIR statements carry an origin (a source position and an inline scope), every pass keeps it by a stated rule, the verifiers check it, and native code can produce, on request, a per-function table from code offsets to points (origin, role, callee). No behaviour of any program changes and no generated code changes. The profiler (`native-prof`), runtime-error backtraces and AOT debug information read these tables later.
- Completion conditions:
  - Every existing IR text snapshot and every CLIF pin is byte-identical (the default printers print no origin; the CLIF dump clears source locations).
  - O-1, O-2, O-3 and O-6 (design 5.2) hold after every pass, checked in debug builds over the whole UI suite; O-4 holds after translate, as its own check.
  - O-5: lowering with source locations set gives byte-identical machine code to lowering without, as a test in the ordinary run over the `bench/` programs and the programs of `crates/eml_native/tests`.
  - Through the test API `NativeMachine::points`: a user lambda inlined into a std copy resolves to the lambda's source and line under the std function's scope; a clause expanded by handler specialisation resolves to a `Clause` source under the performing line; a continuation function (`f$kN`, `f{H}$hN`) and an `$owned` copy resolve to the original source; a `dup` has role `Rc`.
  - A placement test: only `lower/origin.rs` calls `set_srcloc`.
  - Encoded tables are at most 0.6 x the code bytes on the `bench/` programs; the `bench/native.sh` load rows with tables are within +3 % instructions of the baseline, and without tables within noise.

## 2. Scope

- In: the types `At`, `Scope`, `Source`, `Stmt { at, kind }`, `term_at`, `scopes`, `sources` in `eml_lir` and `eml_core_ir` (design 5.1); the second printer mode and `parse`; the verifier rules; the rule of every pass (design 5.3: translate, every sub-pass of optimize, evidence passing, `to_lir`, the six LIR passes with the copy forms of `$uniform` and `$owned`); `lower/origin.rs`, roles, the encoded tables in `Image`, the machine option `origins`, `NativeMachine::points` (design 5.4); the mechanical adaptation of `eml_vm`, `eml_native`, `eml_cli`, `eml_test_support` and the tests.
- Out: any sampler or CLI flag (`native-prof`); backtraces in runtime errors and merging `Extern.at` into the statement's origin (`native-backtrace`, after `vm-retire`); anything for the VM beyond the adaptation; DWARF.

## 3. Received from earlier stages

- `native-checked`: the entry descriptor's `class` as a `#[repr(u8)]` enum with fixed discriminants and a `name()`; `lower/audit.rs` emitting through the `FnLower`'s builder, so the audit calls fall under the statement's point; the twin CLIF pins, which stay byte-identical through this stage.
- The invariant "a function's name fixes its code and ABI" and the name-keyed decisions of optimize, on which "a function's origins are fixed by its name up to the numbering of files and sources" rests.

## 4. Decisions (design sections 4 and 5)

- D-IO-1: an origin is `At { loc, scope }`, a `Copy` field of a wrapper struct `Stmt { at, kind }` in both IRs, and `term_at` on a block. Not marker statements, not a side table, no role in the IR.
- D-IO-2: `scopes` per function is the inline tree (`Scope { source, parent: Option<At> }`, scope 0 the root); `sources` per program lists what the programmer wrote (`Source { name, kind, file, start, end, std }`). Copies, whole-body variants and continuation functions clone the table and keep the root source.
- D-IO-3: `Rhs::Extern.at` and `Call::Par.at` stay; they are behaviour. O-6 keeps the statement's position equal to them.
- D-IO-4: file and source numbers are per program; nothing compares them across programs without re-keying (files by path, sources by path, range, kind and name).
- D-IO-5: the four general rules R-keep, R-ins, R-term and R-inline, and the per-pass table of design 5.3. Clause expansion at a `perform` is an inlining.
- D-IO-6: the default text form prints no origin; a second printer mode prints a scopes header and a trailing origin, and `parse` reads both.
- D-IO-7: roles are decided in the lowering (design 4.2). `lower/origin.rs` owns `set_srcloc`; one point per statement part and one per call instruction with its callee.
- D-IO-8: tables are built only when the machine is created with `origins: true`. Rows and points are varint-encoded; every index of a load's tables refers to that load.
- D-IO-9: the VM adapts to the new statement type and ignores origins.

## 5. Units (in order)

1. IO-1 `Start ir-origins`: the executable plan; the refresh of section 10; the baseline.
2. IO-2: the types, both printers, `parse`, O-1 and O-2, and the mechanical adaptation of every crate with `At::NONE` everywhere (one commit; every snapshot byte-identical).
3. IO-3: translate sets origins, sources and root scopes; O-4 as its own check after translate; O-6.
4. IO-4: optimize: `specialize`, `known`, `fuse` (`inline`, `known_con`, `copies`, `blocks`); O-3 on.
5. IO-5: optimize: `handlers` (path copies, clause expansion, continuation and join-block functions, clause copies, the `return` clause, state threading).
6. IO-6: evidence passing (`split.rs`, `handlers.rs`, `helpers.rs`).
7. IO-7: the LIR passes (boxing and `variants.rs`, flatten, contract, borrow with `$owned`, Perceus, reuse).
8. IO-8: `lower/origin.rs`, roles, the encoded tables, `origins`, `NativeMachine::points`, O-5, the placement test, the size and load measurements.
9. IO-9 `Close ir-origins`.

Units 4 to 7 each start by listing the statement-creating sites of their files against the rule table; a site without a rule stops the unit for a plan amendment.

## 6. Test changes by kind (scope)

- Kind 1: none.
- Kind 2: none expected. If a snapshot of an IR text or a CLIF pin changes, the unit stops.
- Kind 3: the adaptation of tests that construct or match statements (`Stmt::Let { .. }` becomes `StmtKind::Let { .. }` under a `Stmt`), with every expected value byte-identical.
- New: the origin tests through the second printer mode (one per rule family), the verifier tests of O-1..O-4 and O-6, O-5, the `points` tests of section 1, the placement test.

## 7. docs/spec sections written

docs/spec/lir.md 「出どころ」 (new: the types, O-1..O-6, the general rules, the LIR pass rules); docs/spec/core-ir.md 「位置」 (rewritten: statement origins beside the extern position; the Core IR pass rules); docs/spec/native.md 「位置の表」 (new: roles, points, the encoding, `origins`); docs/implementation/architecture.md (the per-crate sections that name the statement types); docs/implementation/testing.md 「LIR のテキストの形」 (the second printer mode); CLAUDE.md's invariant on names gains "and, up to the numbering of files and sources, its origins".

## 8. Acceptance checks

- CLAUDE.md's gates (tests, clippy with the feature combinations, fmt, the crate graph check).
- The completion conditions of section 1.
- `bench/run.sh`, `bench/rt.sh` and `bench/scripts.sh` re-measured at Close (`eml_lir` and `eml_core_ir` change); the VM's rows within noise, because no code changes.
- Roadmap: the `ir-origins` entry is deleted.

## 9. Risks with defaults

| Item | Default |
|---|---|
| A statement-creating site has no rule in design 5.3 | the unit stops for a plan amendment; O-1 and O-3 catch a cloned statement whose scope table was not cloned |
| A pass must build one statement from parts of two scopes | it takes the consumer's origin (R-ins); if a position must be synthesised it is a position-less origin in the right scope, which O-4 allows after translate |
| O-5 fails on some program (the prototype covers ten) | stop; the tables are then built from a second compile only when asked, and the plan is amended with the measured cost |
| Encoded tables over 0.6 x | drop the per-call points for eml calls; the callee is then read from the statement's point |
| The mechanical diff of IO-2 collides with other work | the stage runs alone; IO-2 is one commit |

## 10. Refresh at the stage's first commit

Re-read `optimize/mod.rs`'s pass order and the file headers named in design F-2, F-3 and F-20 against the base; re-count the `Stmt::` and `Term::` sites; confirm that `native-checked` delivered the two items of section 3; record the baseline (IR and CLIF snapshot hashes, `bench/native.sh` load rows).
