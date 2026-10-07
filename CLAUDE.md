# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

eml is an experimental functional language exploring "linear types × algebraic effects" (Rust, Cargo workspace, edition 2024). Source files use the `.em` extension.
The documentation lives under `docs/`; start from `docs/README.md`. `docs/spec/` is normative (what to implement), `docs/implementation/` covers architecture, testing and the current status, and `docs/future/` holds future designs (multicore, evidence passing, roadmap, stdlib notes).

## Rules

- The documents under `docs/` are the source of truth.
- eml is unreleased and under development, so ignore backward compatibility and develop aggressively: change syntax, APIs, internal representations, and diagnostics freely, without deprecation paths or compatibility shims.
- Put a concise, refined implementation first. Accept a reasonable cost in revising tests, in further design discussion, and in implementation effort to reach it.
- When a test fails and there is a sound reason that the test itself is what should change, put the change in the work's spec with its kind (see Testing); approving the spec is the agreement. Mechanical follow-ups need no agreement.
- Code comments and docs are written in Japanese. Code comments explain why, not what: do not restate the code, and cite `docs/` paths when pointing at a rule.
- Whenever you write Japanese text (docs under `docs/`, code comments, or anything else), always follow the `yomiyasu:yomiyasu` skill: invoke it before writing and apply its rules to everything you write.

## Commands

The dev environment is a Nix flake (`direnv` with `use flake`). The devShell provides clippy, rustfmt, and cargo-insta.

```sh
cargo build                          # all crates
cargo test                           # all tests
cargo test -p eml_syntax --test integration parser::empty_file   # a single test
cargo test -p eml_cli --test integration ui::   # UI tests
cargo insta review                   # accept snapshots
cargo clippy --all-targets && cargo fmt
cargo run -p eml_cli -- check <file.em>    # or: run [--debug-heap] <file.em>
nix build                            # build the eml binary (eml_cli)
```

## Architecture

A batch pipeline. Dependencies flow strictly top to bottom; every crate that reports diagnostics uses `eml_diagnostics` (currently all but `eml_core_ir`, `eml_runtime`, and `eml_interp`).

```
eml_cli          check / run; only wires the stages together (lib API is called from tests)
eml_interp       runs Core IR on a CEK machine
eml_runtime      object model, heap, RC, debug_heap checks, OutputSink
eml_core_ir      typed HIR -> Core IR (ANF, dup/decref insertion)
eml_types        Kind/type/row inference; linearity, multiplicity, and exhaustiveness checks
eml_hir          module loading, CST -> HIR, name resolution
eml_syntax       logos lexer, layout stage, event-based parser, rowan CST, typed AST wrappers
eml_diagnostics  Diagnostic, FileId/SourceFiles, ariadne rendering (does not depend on rowan)
```

- Each stage is a pure function `fn stage(&In) -> (Out, Vec<Diagnostic>)` with no global mutable state, so it can later move onto queries (salsa).
- Never stop on errors: `eml_cli::Session::check` / `compile` run every checking stage and collect all diagnostics. The parser recovers with `ERROR` nodes; type checking onward inserts an `Error` type and suppresses cascading diagnostics.
- The parser follows rust-analyzer (event stream -> rowan tree built in `sink.rs`). A layout stage (`layout.rs`) between the lexer and the parser inserts virtual `LAYOUT_OPEN` / `LAYOUT_SEP` / `LAYOUT_CLOSE` tokens (`docs/spec/layout.md`); the parser emits no events for them, so the CST is always lossless. Grammar rules are confined to `eml_syntax/src/grammar/`.
- From HIR onward, nodes are referenced by IDs and analysis results (types, ...) live in side tables. HIR is a `Program` of modules (module 0 is the Prelude, module 1 the entry file, then the imported modules in the order the loader found them; the loader reads files through a `ModuleSource`); items use program-wide `ItemId`s (module + local index, side tables are `ItemMap`), while body nodes use per-body arena IDs (`ExprId`, etc.). HIR nodes carry a `TextRange` (not a `SyntaxNodePtr`), because some expressions, such as reassociated operator subexpressions, have no syntax node.
- Diagnostic codes are defined in a per-stage `codes` module (e.g. `eml_syntax::codes`, E0xxx). E0004 (not yet supported) is used by every stage, so it lives in `eml_diagnostics` (`NOT_YET_SUPPORTED`, `Diagnostic::not_yet_supported`).
- `OutputSink` is `Send + Sync` in preparation for multicore. `RunConfig` is `#[non_exhaustive]`; build it from `Default`.
- `eml_syntax` implements the grammar of milestones M1 and M2 (modules: `import`, `pub`, qualified names; `docs/implementation/status.md`). Constructs of later milestones (records, lists and string interpolation in M3; command literals in M9) and the reserved float and char literals (M4) are lexed and parsed far enough to report E0004; where each one is reported is listed in `docs/spec/grammar.md`. The milestones are listed in `docs/future/roadmap.md`.

## Testing

- Work test-first (TDD). Snapshots use `insta`, mostly inline (`@"..."`).
- Each crate's integration tests build into one binary, `integration` (`autotests = false`; `tests/main.rs` declares every file as a module). A file not declared there does not run. Libs set `doctest = false`, and libs without unit tests also set `test = false` (`docs/implementation/testing.md`).
- UI tests (`crates/eml_cli/tests/ui.rs`): `tests/ui/run/**/*.em` must run to completion and `tests/ui/run-fail/**/*.em` must compile cleanly and end in a runtime error, and `tests/ui/check-fail/**/*.em` must produce at least one error; output is snapshotted. Pass/fail expectation is decided by the top-level directory, and every test sits in a category subdirectory below it (`docs/implementation/testing.md`). A test is one file `<top>/<category>/<name>.em` or one directory `<top>/<category>/<name>/main.em` whose other `.em` files are the modules it imports; the harness fails on any other layout. Run tests always enable `debug_heap`.
- `crates/eml_cli/tests/cli.rs` checks the binary's exit codes (0 success / 1 diagnostic or runtime error / 2 usage error).
- Test changes come in three kinds (`docs/implementation/testing.md`): (1) pass/fail changes (moving a test between `run`, `run-fail` and `check-fail`, deleting a UI test) are listed one by one in the work's spec; (2) expected-value changes (UI output, diagnostic codes or wording, stage-dump snapshots such as Core IR, moving a UI test without changing pass/fail) are described as a scope in the work's spec (e.g. "rewrite every UI test that uses E2007"); approving the spec is the agreement for kinds 1 and 2, and the reason goes in the spec and the commit message; (3) mechanical follow-ups that keep every expected value byte-identical need no agreement, and the commit message is the record.
- Never bend the design to keep existing tests unchanged (no parallel enum variants, flags, test-only fields, or spec exceptions for that purpose). Put the test change in the work's spec instead, stating its kind.
- `eml_test_support` (dev-only) builds the pipeline for integration tests (`parse` / `lower` / `def_map` / `check` / `core` / `core_until` / `run` / `execute`, multi-file `*_files` variants that take root-relative `(path, text)` modules read through `MemorySource`, and `parse_clean` / `lower_clean` that assert a clean source; `lower_clean` needs `hir`) and formats diagnostics (`short` / `short_text` / `full`, fixes with `fixes`, joined to a stage dump with `with_diagnostics`). Core IR tests are written as IR text read by `eml_core_ir::parse`. Use it only from `tests/`, never from `#[cfg(test)]` in `src/`: the crate under test would be linked twice. Stages are features (`hir` < `types` < `core` < `run`); each crate enables only up to its own stage, so its tests still build while downstream crates are broken mid-refactor.

## Syntax

The final syntax is defined in `docs/spec/` (`lexical.md`, `layout.md`, `grammar.md`, `declarations.md`, `expressions.md`, `records.md`, `modules.md`; program examples in `examples.md`). When a syntax choice is undecided, lean toward Haskell conventions.
