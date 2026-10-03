# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

eml is an experimental functional language exploring "linear types × algebraic effects" (Rust, Cargo workspace, edition 2024). Source files use the `.em` extension.
The specs in `docs/superpowers/specs/` are the source of truth for the design; work plans live in `docs/superpowers/plans/`. Code comments and docs are written in Japanese.

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

A batch pipeline. Dependencies flow strictly top to bottom; every crate uses `eml_diagnostics`.

```
eml_cli          check / run; only wires the stages together (lib API is called from tests)
eml_interp       runs Core IR on a CEK machine
eml_runtime      object model, heap, RC, debug_heap checks, OutputSink
eml_core_ir      typed HIR -> Core IR (ANF, dup/decref insertion)
eml_types        Kind/type/row inference; linearity, multiplicity, and exhaustiveness checks
eml_hir          CST -> HIR, name resolution
eml_syntax       logos lexer, event-based parser, rowan CST, typed AST wrappers
eml_diagnostics  Diagnostic, FileId/SourceFiles, ariadne rendering (does not depend on rowan)
```

- Each stage is a pure function `fn stage(&In) -> (Out, Vec<Diagnostic>)` with no global mutable state, so it can later move onto queries (salsa).
- Never stop on errors: `eml_cli::analyze` runs every stage and collects all diagnostics. The parser recovers with `ERROR` nodes; type checking onward inserts an `Error` type and suppresses cascading diagnostics.
- The parser follows rust-analyzer (event stream -> rowan tree built in `sink.rs`); the CST is always lossless. Grammar rules are confined to `eml_syntax/src/grammar/` so swapping in the final syntax only touches `grammar/`, `SyntaxKind`, the lexer, and the AST wrappers.
- From HIR onward, nodes are referenced by arena IDs (`ExprId`, etc.) and analysis results (types, ...) live in side tables. HIR nodes carry a `SyntaxNodePtr`.
- Diagnostic codes are defined in a per-stage `codes` module (e.g. `eml_syntax::codes`, E0xxx).
- `OutputSink` is `Send + Sync` in preparation for multicore. `RunConfig` is `#[non_exhaustive]`; build it from `Default`.
- The project is at the milestone-1 skeleton stage: apart from the `eml_syntax` foundation, the stages (hir / types / core_ir / interp) are stubs.

## Testing

- Work test-first (TDD). Snapshots use `insta`, mostly inline (`@"..."`).
- UI tests (`crates/eml_cli/tests/ui.rs`): `tests/ui/run/*.em` must run to completion and `tests/ui/check-fail/*.em` must produce at least one error; output is snapshotted. Pass/fail expectation is decided by directory. Run tests always enable `debug_heap`.
- `crates/eml_cli/tests/cli.rs` checks the binary's exit codes (0 success / 1 diagnostic or runtime error / 2 usage error).
- Agreed exception: test sources written in the provisional syntax may be mechanically rewritten when switching to the final syntax (expected results must not change).

## Syntax

Milestone 1 uses a provisional syntax (language-design spec §7). When a provisional-syntax choice is undecided, lean toward Haskell conventions.
