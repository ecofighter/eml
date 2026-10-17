# S13b-3 std effects, `Iter` and `Fs.read_line` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the std modules `Std.Yield`, `Std.Fail`, `Std.State`, `Std.Iter`, the extern-backed `Fs.read_line`, rewrite the S12d scripts grep / logstat / csv2jsonl onto them with byte-identical output, and re-measure `bench/scripts.sh`.

**Architecture:** The four std modules are plain eml files under `std/`, embedded through `eml_hir::STD`, written with S13b-2's `fn` / `ctl` clauses. `Fs.read_line` follows `Stdin.read_line`: a std-private extern `read_line_raw : File -> <IO> (File, String)` (an `eml_rt` `Ctx` method next to `fs_read_all`, a Host-boundary read over the `File`'s `BufRead`), wrapped in eml with the shared `String.trim_line_end`.

**Tech Stack:** Rust workspace, eml std (`std/*.em`), Verus (`cargo verus verify -p eml_rt`), Miri (`trusted::`), insta, the UI test harness, `bench/scripts/compare.sh`, `bench/scripts.sh` (macOS).

**Spec:** `docs/superpowers/specs/2026-10-17-s13b3-std-effects-iter-design.md`

## Global Constraints

- Code comments and docs are Japanese, written with the `yomiyasu:yomiyasu` skill; comments explain why and cite `docs/` paths. CLAUDE.md stays English.
- Signatures exactly as the spec's 「モジュールと関数」 section (names, argument order, rows). The `Yield` → `Iter` constructor is `Iter.generate` (not `from`, a keyword).
- New std handler clauses use `fn` where they resume immediately, `ctl` otherwise; `return` clauses take neither.
- Script rewrites are kind-3: stdout, stderr, exit code and written files stay byte-identical; `bench/scripts/compare.sh --fixture <name>` prints `same` for grep, logstat, csv2jsonl.
- Existing tests that declare their own `Yield` / `State` / `Fail` are not rewritten.
- A change to `eml_rt` must pass `cargo verus verify -p eml_rt`, `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')`, and the Miri run of `trusted::` (`nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::`) besides the usual gate.
- Gate per task: `cargo test --workspace`, `cargo clippy --all-targets`, `cargo fmt --check`. Use `git diff --no-ext-diff`.
- Do not reorder the existing entries of `eml_hir::STD` (tests index `STD[1]`); append new ones.
- Commit messages end with the attribution lines the controller gives.

## Review Focus

- A std module that both declares an effect and is imported only through a qualifier (`Yield.for_each` without `import Yield`): loading by qualifier must make the effect and its operation usable (Task 1 test with a qualifier-only use).
- An `Iter` dropped before its first `next`, or a `zip` where the first `Iter` ends: the other / unstarted producer's `File` must be closed, no leak in checked mode (Tasks 2-3 tests).
- `Fs.read_line` on a line longer than 64 KiB and on a file without a final newline (Task 3 Miri and UI tests).
- `Fs.read_line` after the file has been read to the end by `Fs.read_all` (returns `None`, keeps the `File`) (Task 3 UI test).
- A user module named `Yield.em` / `Iter.em` next to `main.em`: the user module wins as for other std names (Task 1 check: run the existing `modules/` tests; add one user `Iter.em` shadow case).

---

### Task 1: `Std.Yield`, `Std.Fail`, `Std.State`

**Files:**
- Create: `std/Yield.em`, `std/Fail.em`, `std/State.em`
- Modify: `crates/eml_hir/src/lib.rs` (`STD`: append the three entries)
- Create: `tests/ui/run/std/yield.em`, `tests/ui/run/std/fail.em`, `tests/ui/run/std/state.em`, `tests/ui/check-fail/std/to_option_linear_error.em`, `tests/ui/run/modules/std_name_shadowed_by_user/main.em` + `Iter.em` (a user `Iter` module that wins over std; may be added in Task 2 instead if `Std.Iter` must exist first) (+ snapshots)

**Interfaces:**
- Produces: the spec's `Std.Yield` (`effect Yield a`, `for_each`, `collect`, `fold`), `Std.Fail` (`effect Fail x` with `never fail`, `to_result`, `to_option`), `Std.State` (`effect State s`, `run`, `eval`, `exec`) — every effect and function `pub`.

- [ ] **Step 1: Write the failing UI tests.**
  - `std/yield.em`: `for_each` called directly and through `use x <- Yield.for_each gen`; `collect` keeps order; `fold` sums; one use through the qualifier only (`Yield.yield` without an import list) and one through `import Yield (Yield(..))`.
  - `std/fail.em`: `to_result` success and failure, `to_option` success and failure, a `fail` from a nested function, nested `to_result` (inner catches), a `Fail (Int, String)` error.
  - `std/state.em`: `run` returns `(result, final)`, `eval`, `exec`; a counter loop with `get` / `put`.
  - `check-fail/std/to_option_linear_error.em`: `Fail.to_option` over a computation whose error type is `File` → the existing E3001.
- [ ] **Step 2: Run** `cargo test -p eml_cli --test integration ui::` — the new tests fail (unknown module).
- [ ] **Step 3: Write the three modules** with a short header comment like the other std files, `fn` clauses for `for_each` / `fold` / `get` / `put`, `ctl fail` for `Fail`; `collect` may be built on `fold` (reverse at the end); `eval` / `exec` may be projections of `run`. Append to `STD`.
- [ ] **Step 4: Run** `cargo test --workspace` — pass, including `the_embedded_std_matches_the_std_directory` and `std_program()` in `crates/eml_hir/tests/externs.rs` (every std module must check cleanly on its own).
- [ ] **Step 5: Commit** `Add the std effect modules Yield, Fail and State`.

### Task 2: `Std.Iter`

**Files:**
- Create: `std/Iter.em`; Modify: `crates/eml_hir/src/lib.rs` (`STD`)
- Create: `tests/ui/run/std/iter.em` (+ snapshot)

**Interfaces:**
- Consumes: Task 1 `Std.Yield`.
- Produces: the spec's `Std.Iter` (`pub data Iter <e> a = | Iter (once Unit -> <e> Option (a, Iter <e> a))` and the 13 functions).

- [ ] **Step 1: Write the failing UI test** `std/iter.em`: every function; laziness (a producer that `println`s before each `yield`: building `generate` / `map` / `take` prints nothing, the first `next` prints); `take 0` and `take` past the end; an empty producer; `zip` with different lengths (both directions); `enumerate 1`; `from_list` → `to_list` round trip; `yield_all` back into `Yield.collect`; dropping an `Iter` before any `next`.
- [ ] **Step 2: Run** it — fails.
- [ ] **Step 3: Write `std/Iter.em`** per the spec (`take` drops the rest at the `next` after the n-th element; `zip` drops the other on end). Append to `STD`.
- [ ] **Step 4: Run** `cargo test --workspace` — pass.
- [ ] **Step 5: Commit** `Add the pull generator Std.Iter`.

### Task 3: `Fs.read_line` and `String.trim_line_end`

**Files:**
- Modify: `std/Fs.em`, `std/String.em`, `std/Stdin.em`
- Modify: `crates/eml_extern/src/lib.rs` (variant, `ALL`, row `Std.Fs.read_line_raw`, kinded `&[Lin]` → `Lin`, `Obj` → `Obj` tuple), `crates/eml_vm/src/vm.rs` (dispatch arm like `ReadAll`), `crates/eml_vm/src/compile/emit.rs` (extern classification list), `crates/eml_core_ir/tests/externs.rs` (linearity match arm)
- Modify: `crates/eml_rt/src/heap/ctx/io.rs` (`Ctx::fs_read_line` next to `fs_read_all`, same proof shape), `crates/eml_rt/src/host.rs` (a line read over `File`'s `BufRead` through `StopOnInterrupt::read_bytes(_, true)`), `crates/eml_rt/src/trust.rs` (a T11 row if a new `external_body` function is added), `crates/eml_rt/src/heap/tests/trusted.rs`, `crates/eml_rt/src/heap/tests/externs.rs`, `crates/eml_rt/src/heap/tests/expected.txt`
- Create: `tests/ui/run/std/read_line.em`, `tests/ui/run/std/iter_file/main.em` + `lines.txt` (+ snapshots)

**Interfaces:**
- Produces: `pub read_line : File -> <IO> (File, Option String)` in `Std.Fs`; `pub trim_line_end : String -> String` in `Std.String` (removes one trailing `\n` or `\r\n`, otherwise returns the string unchanged); `Ctx::fs_read_line(&mut self, file: Word) -> Result<Word, Fault>` (same contract as `fs_read_all`: consumes a unique `File`, returns the pair tuple, releases the file on every fault).

- [ ] **Step 1: Write failing tests.**
  - `trusted.rs` (`files_go_through_the_host`): line reads — a line crossing the 64 KiB chunk, a final line without `\n`, `\r\n` kept by the raw read, EOF returns the empty string, a failure mid-line (`fail_after`), non-UTF-8, the stop word; `mortal(ctx.heap()) == 0` after each.
  - `externs.rs`: `read_line_raw` refuses a non-file and a shared file (mirror the `read_all` test); the expected-value table gains a `Std.Fs.read_line_raw` row (regenerate with the `#[ignore]` writer test and review the diff).
  - UI `std/read_line.em`: write a file with `Fs.write_text` containing `\n`, `\r\n`, an empty line and no final newline, then read it line by line; an empty file; `read_line` after `read_all` returns `None`.
  - UI `std/iter_file/main.em` + `lines.txt`: the spec's `lines` producer under `Iter.generate`, `Iter.take 2`, the rest dropped (checked mode reports no leak); the consuming loop wrapped in `try_io`; a missing file caught by `try_io`.
  - `std/stdin.em` must keep its output after `Stdin` uses `String.trim_line_end`.
- [ ] **Step 2: Run** `cargo test -p eml_rt` and the UI tests — fail.
- [ ] **Step 3: Implement** the extern row, the `Ctx` method and Host read (reuse `read_bytes` in line mode on the `File`'s reader; set the file's `size` to `None` like the first `read_all` does), the VM arm, the std wrappers, and move `Stdin`'s `without_end` to `String.trim_line_end`. Update the trust list rows and the `TRUSTED` table test if a new `external_body` function exists.
- [ ] **Step 4: Run** `cargo test --workspace`, `cargo verus verify -p eml_rt`, `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')`, and the Miri `trusted::` run — all pass.
- [ ] **Step 5: Commit** `Read a File line by line with Fs.read_line`.

### Task 4: Rewrite grep, logstat and csv2jsonl on the std effects

**Files:**
- Modify: `tests/ui/run/scripts/grep/main.em`, `tests/ui/run/scripts/logstat/main.em`, `tests/ui/run/scripts/csv2jsonl/main.em`

**Interfaces:**
- Consumes: Tasks 1 (`Yield`, `Fail`), S13b-2 `let … and …`.

- [ ] **Step 1: Rewrite** per the spec's 「スクリプトの書き換え」: drop the scripts' own `Yield` / `Fail` declarations; grep's counting handler via `Yield.fold` or a `fn`-clause handler, still reading whole files; logstat keeps `Record`, its `Yield` → `Record` handler as a `fn` clause or `Yield.fold`; csv2jsonl uses `Fail (Int, String)` with `fail (1, "…")`, `Fail.to_result` for `catching`, and one `let … and …` group for the CSV states (in a field, in quotes, after a closing quote, at a row start).
- [ ] **Step 2: Run** `cargo test -p eml_cli --test integration ui::` — the three script snapshots are unchanged (no `cargo insta accept`).
- [ ] **Step 3: Run** `cargo build && bench/scripts/compare.sh --fixture grep`, `… logstat`, `… csv2jsonl` — each prints `same`.
- [ ] **Step 4: Commit** `Use the std effects in grep, logstat and csv2jsonl` (kind 3 in the body).

### Task 5: Measure and record S13b-3

**Files:**
- Modify: `docs/spec/stdlib.md`, `docs/spec/object-model.md`, `docs/implementation/benchmarks.md`, `docs/implementation/status.md`, `docs/future/roadmap.md`, `docs/future/stdlib.md`, `CLAUDE.md`

- [ ] **Step 1: Measure** `bench/scripts.sh` (macOS) and record its table in `benchmarks.md` as S13b next to the S12d table, with one line per changed row explaining why it moved (or that it did not).
- [ ] **Step 2: Docs** per the spec's 「段の終わりの文書」: `stdlib.md` sections for the four modules (tables like the other modules), `Fs.read_line`, `String.trim_line_end`, the three `Iter` notes (speed, where effects arrive, `multi` → E3006); `object-model.md` extern list and the T7 / T11 rows; `status.md`; `roadmap.md` (mark S13b 済み in the 段の列 table and in the S13b table, delete the S13b section's remaining 「決めたこと」 subsection, keep the section's pointer to the docs like S13b-1/2); `future/stdlib.md`; `CLAUDE.md` (std file list, extern count 38 → 39).
- [ ] **Step 3: Run** `cargo test --workspace` (docs `citations` test) — pass.
- [ ] **Step 4: Commit** `Record S13b-3 in the specs, the benchmarks and the roadmap`.
