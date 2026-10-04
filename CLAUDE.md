# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

eml is an experimental functional language exploring "linear types × algebraic effects" (Rust, Cargo workspace, edition 2024). Source files use the `.em` extension.
The documents under `docs/` are the source of truth; start from `docs/README.md`. `docs/spec/` is normative (what to implement), `docs/implementation/` covers architecture, testing and the current status, and `docs/future/` holds future designs (multicore, evidence passing, roadmap, stdlib notes). Code comments and docs are written in Japanese. Code comments explain why, not what: do not restate the code, and cite `docs/` paths when pointing at a rule.
Whenever you write Japanese text (docs under `docs/`, code comments, or anything else), always follow the `yomiyasu:yomiyasu` skill: invoke it before writing and apply its rules to everything you write.

## Commands

The dev environment is a Nix flake (`direnv` with `use flake`). The devShell provides clippy, rustfmt, and cargo-insta.

```sh
cargo build                          # all crates
cargo test                           # all tests
cargo test -p eml_syntax --test parser empty_file   # a single test
cargo test -p eml_cli --test ui      # UI tests
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
eml_hir          CST -> HIR, name resolution
eml_syntax       logos lexer, layout stage, event-based parser, rowan CST, typed AST wrappers
eml_diagnostics  Diagnostic, FileId/SourceFiles, ariadne rendering (does not depend on rowan)
```

- Each stage is a pure function `fn stage(&In) -> (Out, Vec<Diagnostic>)` with no global mutable state, so it can later move onto queries (salsa).
- Never stop on errors: `eml_cli::check` / `compile` run every checking stage and collect all diagnostics. The parser recovers with `ERROR` nodes; type checking onward inserts an `Error` type and suppresses cascading diagnostics.
- The parser follows rust-analyzer (event stream -> rowan tree built in `sink.rs`). A layout stage (`layout.rs`) between the lexer and the parser inserts virtual `LAYOUT_OPEN` / `LAYOUT_SEP` / `LAYOUT_CLOSE` tokens (`docs/spec/layout.md`); the parser emits no events for them, so the CST is always lossless. Grammar rules are confined to `eml_syntax/src/grammar/`.
- From HIR onward, nodes are referenced by arena IDs (`ExprId`, etc.) and analysis results (types, ...) live in side tables. HIR nodes carry a `TextRange` (not a `SyntaxNodePtr`), because some expressions, such as reassociated operator subexpressions, have no syntax node.
- Diagnostic codes are defined in a per-stage `codes` module (e.g. `eml_syntax::codes`, E0xxx). E0004 (not yet supported) is used by every stage, so it lives in `eml_diagnostics` (`NOT_YET_SUPPORTED`, `Diagnostic::not_yet_supported`).
- `OutputSink` is `Send + Sync` in preparation for multicore. `RunConfig` is `#[non_exhaustive]`; build it from `Default`.
- `eml_syntax` implements stage S1 of the final syntax (`docs/implementation/status.md`); S2/S3 constructs (records, modules, interpolation, command literals, ...) are lexed and parsed far enough to report E0004. The later stages implement the vertical-slice steps that `docs/implementation/status.md` marks as done; HIR reports constructs of later steps as E0004.

## Testing

- Work test-first (TDD). Snapshots use `insta`, mostly inline (`@"..."`).
- UI tests (`crates/eml_cli/tests/ui.rs`): `tests/ui/run/*.em` must run to completion and `tests/ui/run-fail/*.em` must compile cleanly and end in a runtime error, and `tests/ui/check-fail/*.em` must produce at least one error; output is snapshotted. Pass/fail expectation is decided by directory. Run tests always enable `debug_heap`.
- `crates/eml_cli/tests/cli.rs` checks the binary's exit codes (0 success / 1 diagnostic or runtime error / 2 usage error).
- Test changes come in three kinds (`docs/implementation/testing.md`): (1) behavior changes (UI output, diagnostic codes or wording, pass/fail, deleting or moving tests) need agreement beforehand; (2) changes to internal-representation snapshots (e.g. Core IR dumps) are listed in the work's spec, and approving the spec is the agreement; (3) mechanical follow-ups that keep every expected value byte-identical are allowed when the plan says so. Kinds 1 and 2 are recorded in `docs/implementation/test-changes.md`.
- Never bend the design to keep existing tests unchanged (no parallel enum variants, flags, test-only fields, or spec exceptions for that purpose). Propose the test change instead, stating its kind.
- `eml_test_support` (dev-only) builds the pipeline for integration tests (`parse` / `lower` / `check` / `core` / `run` / `execute`) and formats diagnostics (`short` / `full`). Use it only from `tests/`, never from `#[cfg(test)]` in `src/`: the crate under test would be linked twice. Stages are features (`hir` < `types` < `core` < `run`); each crate enables only up to its own stage, so its tests still build while downstream crates are broken mid-refactor.

## Syntax

The final syntax is defined in `docs/spec/` (`lexical.md`, `layout.md`, `grammar.md`, `declarations.md`, `expressions.md`, `records.md`, `modules.md`; program examples in `examples.md`). When a syntax choice is undecided, lean toward Haskell conventions.
