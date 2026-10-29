# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

eml is an experimental functional language exploring "linear types × algebraic effects" (Rust, Cargo workspace, edition 2024). Source files use the `.em` extension.
The documentation lives under `docs/`; start from `docs/README.md`. `docs/spec/` is normative (what to implement), `docs/implementation/` covers architecture, testing, diagnostic presentation and the current status, and `docs/future/` holds future designs (native code, FFI, async, multicore, roadmap, stdlib notes).

## Rules

- The documents under `docs/` are the source of truth.
- eml is unreleased and under development, so ignore backward compatibility and develop aggressively: change syntax, APIs, internal representations, and diagnostics freely, without deprecation paths or compatibility shims.
- Put a concise, refined implementation first. Accept a reasonable cost in revising tests, in further design discussion, and in implementation effort to reach it.
- When a test fails and there is a sound reason that the test itself is what should change, put the change in the stage's plan under `docs/plans/` with its kind (see Testing); approving the plan is the agreement. Mechanical follow-ups need no agreement.
- The documents under `docs/` describe the current state. They do not name stages, dates, or what came before; that history lives in git (docs/README.md の「文書の扱い」). The only exception is `docs/reports/`.
- A stage's design and plan live in `docs/plans/`, are committed as the stage's first commit, and are deleted when the stage closes (docs/future/roadmap.md の「進め方」). Code and `docs/` never cite `docs/plans/`; write the `docs/spec/` section in the same commit that implements the rule, and cite that.
- Code comments and docs are written in Japanese. Code comments explain why, not what: do not restate the code, and cite `docs/` paths when pointing at a rule.
- Whenever you write Japanese text (docs under `docs/`, code comments, or anything else), always follow the `natural-japanese:natural-japanese` skill: invoke it before writing and apply its rules to everything you write.
- The CLAUDE.md files (this one and `crates/eml_rt/CLAUDE.md`) hold working rules and pointers into `docs/`. When a stage changes the design, write it in `docs/` and touch a CLAUDE.md only when a rule or a pointer changes, so the two never drift apart.
- Run ad-hoc `.em` programs (`eml run`, `cargo run -p eml_cli -- run`, probes in a scratchpad) under `timeout` (e.g. `timeout 20`). An eml program can legitimately loop forever: a tail-recursive loop that does not allocate hits neither `max_depth` nor `max_bytes` (docs/spec/vm.md の「上限と割り込み」).
- When a Bash call exceeds its timeout, Claude Code moves it to a background task instead of killing it. Before finishing (above all as a subagent), stop every such task with TaskStop or wait for it to complete; a task left running keeps a finished subagent listed as running and leaves its process behind.

## Commands

The dev environment is a Nix flake (`direnv` with `use flake`) on nixos-unstable. Every Rust toolchain comes from fenix: stable is pinned in `rust-toolchain.toml` to the version Verus requires (1.99.1); the flake reads its channel and rustup reads the same file outside nix, and it serves the devShell, `nix build` and Verus; the `.#miri` shell has a nightly with miri and rust-src, and the `.#cross` shell adds the Windows msvc and Linux `rust-std` to the stable toolchain for cross-target clippy (docs/implementation/testing.md の「OS ごとのテスト」). The devShell provides clippy, rustfmt, rust-analyzer, cargo-insta, cargo-nextest, and `verus` / `cargo-verus` and `verusfmt`. Every shell sets `CARGO_HOME` to `${XDG_CACHE_HOME:-$HOME/.cache}/eml-cargo-home` so rustup's `~/.cargo/bin` proxies do not shadow fenix's `cargo-clippy` / `cargo-miri`.

```sh
cargo build                          # all crates
cargo nextest run                    # all tests (`cargo test` runs the same tests; `cargo test -p` is faster for a short loop on one crate)
cargo nextest run -p eml_syntax --test integration parser::empty_file   # a single test
cargo nextest run -p eml_cli --test integration -E 'test(ui::)'   # UI tests
cargo insta review                   # accept snapshots
cargo clippy --all-targets -- -D warnings && cargo fmt
cargo clippy -p eml_cli --no-default-features --features types -- -D warnings   # non-default features (none, types, core; eml_test_support also hir)
cargo run -p eml_cli -- check <file.em>    # or: run [--debug-heap] [--sched normal|sequential|seeded:N] [--clock real|virtual] <file.em> [args...] (the VM; --debug-heap is checked mode; --sched picks how `par` children and `Async` tasks switch, normal (a timer-driven monitor thread) by default; --clock picks the timer clock, real by default for the CLI; `eml <file.em> [args...]` is the same), repl [--debug-heap] [--sched ...] [--clock ...] [file.em] (the terminal REPL)
nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- .github/workflows/ci.yml   # lint the CI workflow
nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings   # type-check the Windows-only code (Linux: x86_64-unknown-linux-gnu)
nix build                            # build the eml binary (eml_cli)
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::   # Miri, trusted tests only; run once more with -Zmiri-tree-borrows added
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::   # Miri on the VM's small runs (same two settings)
cargo verus verify -p eml_rt         # Verus proofs of eml_rt
verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')   # format inside verus! (rustfmt does the rest)
bench/run.sh                         # instruction counts of the bench/ programs (macOS, release)
bench/rt.sh                          # eml_rt fast-mode instruction counts (macOS)
bench/scripts.sh                     # the scripts under tests/ui/run/scripts/ and the numeric bench/ programs against their Python versions, each row sized to ~300 ms of eml time: wall time and peak RSS (macOS)
bench/scripts/compare.sh --fixture wc   # a script's eml vs Python stdout and exit code on its UI-test inputs (or: compare.sh NAME ARGS... < input, compare.sh --paths EML PY ARGS... < input)
```

In Claude Code cloud sessions, the proxy refuses GitHub archive downloads, so the environment's setup script must be the one below: it installs Nix, runs `nix/prefetch-inputs.sh` (fetches the flake's `github:` inputs over git) and builds the devShell into the environment cache, and installs the natural-japanese and ecoskills plugins, which cloud sessions do not take from `.claude/settings.json`. If `flake.lock` changed since the cache was built, run `nix/prefetch-inputs.sh` before `nix develop`. The reference copy and the reasons are in docs/implementation/testing.md の「クラウドのセッション」; keep this copy and the one in `README.md` identical to it.

```sh
#!/bin/bash
set -euo pipefail

# コンテナには systemd がないので、daemon を使わず root だけが使う Nix を入れる
if [ ! -e /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh ]; then
  curl --proto '=https' --tlsv1.2 -sSfL https://artifacts.nixos.org/nix-installer |
    sh -s -- install linux --init none --enable-flakes --no-confirm
fi
. /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh

# main の flake の input を git で取り、devShell を組んで環境のキャッシュに入れる
eml=$(mktemp -d)
git clone --depth 1 https://github.com/ecofighter/eml "$eml"
"$eml/nix/prefetch-inputs.sh" "$eml"
nix develop "path:$eml" -c true
rm -rf "$eml"

# クラウドのセッションはリポジトリの enabledPlugins を読まないので、プラグインを user の scope に入れる
claude plugin marketplace add coji/natural-japanese
claude plugin marketplace add ecofighter/ecoskills
claude plugin install natural-japanese@natural-japanese --scope user
claude plugin install ecoskills@ecoskills --scope user
```

The dev profile (and so `cargo nextest run` and `cargo test`) is opt-level 1 with debug assertions on; `--release` is only for `bench/*.sh` and the `#[ignore]` timing tests. For a debugger, build once with `CARGO_PROFILE_DEV_OPT_LEVEL=0 cargo test ...`.

The full list, with the scaling tests, is in docs/implementation/testing.md の「よく使うコマンド」.

## Architecture

A batch pipeline whose dependencies flow strictly top to bottom (docs/implementation/architecture.md の「crate の依存関係」):

```
eml_cli          check / run / repl; the only pipeline driver (`Session`); stage features types < core < run
eml_vm           LIR -> register bytecode (`CodeBase`) run on `eml_rt`'s `Ctx`; `par`, `Async` tasks and channels on one worker; `Machine` appends programs by name (the REPL)
eml_rt           runtime core shared by the VM and native code; depends only on vstd, verified with Verus
eml_core_ir      typed HIR -> Core IR (translate -> optimize -> evidence), then `to_lir` and the LIR passes
eml_lir          LIR (leaf crate on `eml_extern` only): types both IRs share, boxing -> flatten -> contract -> Perceus -> reuse, verifier
eml_types        Kind / type / row inference, class constraints; linearity, multiplicity, and exhaustiveness checks
eml_hir          module loading, CST -> HIR, name resolution
eml_syntax       logos lexer, layout stage, event-based parser, rowan CST, typed AST wrappers
eml_diagnostics  Diagnostic, FileId / SourceFiles, ariadne rendering
eml_extern       dependency-free extern table (types, effects, functions, value `Repr`s)
```

These invariants hold across crates; keep them when you change anything:

- Each stage is a pure function `fn stage(&In) -> (Out, Vec<Diagnostic>)` with no global mutable state, so it can later move onto queries (salsa). The exception is `eml_core_ir`, which takes only an error-free program and returns a `Program` (docs/implementation/architecture.md の「各段階の規律」).
- Never stop on errors. `eml_cli::Session` is the only pipeline driver (the CLI, UI tests and `eml_test_support` all go through it); each stage method runs every stage up to its own and returns all their diagnostics, sorted, and `Session` keeps no intermediate results. The parser recovers with `ERROR` nodes; type checking onward inserts an `Error` type and suppresses cascading diagnostics. `Session` is `Send + Sync` (asserted in `crates/eml_cli/tests/api.rs`).
- The parser follows rust-analyzer (event stream -> rowan tree). The layout stage inserts virtual layout tokens the parser emits no events for, so the CST is always lossless; grammar rules are confined to `eml_syntax/src/grammar/`.
- From HIR onward, nodes are referenced by IDs and analysis results live in side tables. HIR keeps only the positions of written names and nodes; derived layout is computed from the source text where needed.
- Built-ins are `extern` declarations backed by one table in `eml_extern`, and only standard-library modules (`std/`, embedded as `eml_hir::STD`) may write `extern`. Tests tie the table to `std/` (`eml_hir`) and to the Reprs of the std signatures (`crates/eml_core_ir/tests/externs.rs`), so a new extern needs a table row and its std declaration together.
- Later stages add types only through `TypeStore::substitute`; `TypeStore::intern` is crate-private.
- Two IR layers with separate types: Core IR is the semantic layer, LIR is Core IR after evidence passing with RC, boxing and tail calls explicit. Each crate's `pipeline.rs` knows only its own pass order. Verifiers and linearity checks run in debug builds after the passes, so a pass that breaks an IR rule fails loudly there.
- A function's name fixes its code and ABI: instance names depend only on type arguments, optimize's copies are keyed and named by what they specialize, `internal` marks the ABI by name, and the REPL appends code by name under "same name, same code". Do not make a function's code depend on the rest of the program.
- The VM is single-threaded but keeps the constraints that let it run in parallel later (docs/future/multicore.md).
- Diagnostic codes are defined in a per-stage `codes` module (e.g. `eml_syntax::codes`, E0xxx). E0004 (not yet supported) means the same at every stage, so it lives in `eml_diagnostics` (`NOT_YET_SUPPORTED`, `Diagnostic::not_yet_supported`).

The crate-to-spec reading map is in `docs/README.md` (「読む順」). Topics that span crates:

| Topic | Where |
|---|---|
| Crate internals, CLI and lib API | docs/implementation/architecture.md の「CLI と lib API」 and the per-crate sections |
| Module loading, the std modules, REPL sessions | docs/spec/modules.md の「標準ライブラリ」, docs/spec/modules.md の「REPL のセッション」 |
| Externs, the std signatures, runtime errors | docs/spec/stdlib.md の「extern の境界」, docs/spec/stdlib.md の「実行時エラー」 |
| `IO` and `Panic` | docs/spec/effects.md の「組み込みの `IO`」, docs/spec/effects.md の「組み込みの `Panic`」, docs/spec/vm.md の「`IO` と `Panic` への中断」 |
| Inference, inclusions, Kind export | docs/spec/types.md の「推論」, docs/spec/types.md の「呼び出しの包含を解く」, docs/spec/types.md の「Kind の書き出し」 |
| Linearity, implicit drops, array `var`s | docs/spec/linearity.md の「暗黙の後始末」, docs/spec/linearity.md の「配列の変数」 |
| `var` from syntax to Core IR | docs/spec/expressions.md の「`var`」, docs/spec/types.md の「`var` の変数」, docs/spec/core-ir.md の「`var` の変換」 |
| Translate, instance names, optimize | docs/spec/core-ir.md の「変換の規則」, docs/spec/core-ir.md の「最適化」 |
| Evidence passing | docs/spec/core-ir.md の「evidence passing」 |
| Reprs, marks, the LIR passes, verifiers | docs/spec/lir.md の「値の表現」, docs/spec/lir.md の「Kind の印」, docs/spec/lir.md の「パスの順番」, docs/spec/lir.md の「verifier」 |
| `Std.Async` tasks | docs/spec/stdlib.md の「`Std.Async`」, docs/spec/stdlib.md の「取り消し」, docs/spec/stdlib.md の「時間」, docs/spec/vm.md の「`Async` のタスク」, docs/spec/vm.md の「取り消しの届け方」, docs/spec/vm.md の「時計と driver」, docs/spec/declarations.md の「`extern`」 (held arguments) |
| `Std.Chan` and `Std.Atomic` | docs/spec/stdlib.md の「`Std.Chan`」, docs/spec/stdlib.md の「`Std.Atomic`」, docs/spec/vm.md の「channel」, docs/spec/object-model.md の「論理的な worker ID」 |
| VM frames, unwinding, limits | docs/spec/vm.md の「命令とフレーム」, docs/spec/vm.md の「巻き戻し」, docs/spec/vm.md の「上限と割り込み」 |
| Runtime objects, `Ctx`, externs in the word ABI | docs/spec/object-model.md, and `crates/eml_rt/CLAUDE.md` |
| The implemented language and its limits | docs/implementation/status.md; what comes next and why is in docs/future/roadmap.md |

## Testing

The rules are in docs/implementation/testing.md; the facts below are the ones every change runs into.

- Work test-first (TDD). Snapshots use `insta`, mostly inline (`@"..."`).
- Test changes come in three kinds (docs/implementation/testing.md の「テストの変更の運用」): (1) pass/fail changes (moving a test between `run`, `run-fail` and `check-fail`, deleting a test, UI or unit) are listed one by one in the stage's plan; (2) expected-value changes (UI output, diagnostic codes or wording, stage-dump snapshots such as Core IR, `RunStats` snapshots, moving a UI test without changing pass/fail) are described as a scope in the stage's plan (e.g. "rewrite every UI test that uses E3005"); approving the plan is the agreement for kinds 1 and 2, and the reason goes in the plan and the commit message; (3) mechanical follow-ups that keep every expected value byte-identical need no agreement, and the commit message is the record.
- Never bend the design to keep existing tests unchanged (no parallel enum variants, flags, test-only fields, or spec exceptions for that purpose). Put the test change in the stage's plan instead, stating its kind.
- Every test in the ordinary run (`cargo nextest run`, no flags) finishes within 1 s when run alone (`cargo nextest run -j 1`), and its outcome does not depend on machine speed; nextest marks a test `SLOW` after 2 s. The five tests that run a directory of files in parallel get 1 s per file and 5 s in all; anything else gets `#[ignore = "<reason>"]` (docs/implementation/testing.md の「性能のテスト」).
- Pick where a new test goes from docs/implementation/testing.md の「テストの置き場所」, and check each fact in one place only.
- Each crate's integration tests build into one binary, `integration` (`autotests = false`; `tests/main.rs` declares every file as a module). A file not declared there does not run.
- UI tests live in `tests/ui/{run,run-fail,check-fail}/<category>/`, as `<name>.em` or `<name>/main.em` with the modules it imports; the top-level directory decides the expected outcome, and `run` / `run-fail` tests run in checked and then fast mode (docs/implementation/testing.md の「UI テスト」), and the `par/` and `async/` ones also in checked mode under two seeds (docs/implementation/testing.md の「seed で流す」); a UI test or transcript that cannot run on Windows carries the first-line mark (docs/implementation/testing.md の「OS ごとのテスト」). REPL transcripts live in `crates/eml_cli/tests/repl/` (docs/implementation/testing.md の「REPL のトランスクリプト」).
- `eml_test_support` (dev-only) wraps `eml_cli::Session` for integration tests. Use it only from `tests/`, never from `#[cfg(test)]` in `src/`: the crate under test would be linked twice. Stages are features (`hir` < `types` < `core` < `run`); each crate enables only up to its own stage, so its tests still build while downstream crates are broken mid-refactor.
- Core IR and LIR tests are written as IR text read by `eml_core_ir::parse` / `eml_lir::parse`; hand-written LIR tests live in `crates/eml_lir/tests` (docs/implementation/testing.md の「LIR のテキストの形」).
- The `bench/` programs have their `RunStats` pinned by inline snapshots in `crates/eml_vm/tests/bench.rs`, so a change in work counts is an expected-value change. When a stage that touched `eml_vm`, `eml_rt`, `eml_lir`, `eml_core_ir`, `std/` or `bench/` closes, replace the three tables in docs/implementation/benchmarks.md の「今の数値」 with fresh `bench/run.sh`, `bench/rt.sh` and `bench/scripts.sh` output measured on a clean tree; the stage-by-stage history is the file's git log.
- CI (`.github/workflows/ci.yml`) runs the tests, clippy and the feature-combo clippy on macOS, Linux and Windows, `cargo fmt --check` on Linux, and Verus, verusfmt and Miri on Linux through nix; it checks pass/fail only, never timing, and never retries (docs/implementation/testing.md の「CI」).
- A change to `eml_rt` has a gate of its own: see `crates/eml_rt/CLAUDE.md`.

## Syntax

The final syntax is defined in `docs/spec/` (`lexical.md`, `layout.md`, `grammar.md`, `declarations.md`, `expressions.md`, `records.md`, `modules.md`; program examples in `examples.md`). When a syntax choice is undecided, lean toward Haskell conventions.
