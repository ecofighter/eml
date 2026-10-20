# S14d Numeric Benchmarks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Five numeric programs (nbody, spectral-norm, mandelbrot, fannkuch-redux, the sieve) in `bench/` run against plain Python versions in `bench/scripts.sh`. Every row of `bench/scripts.sh` is sized to about 300 ms of eml wall time. The new programs' `RunStats` are pinned as S14e's baseline, and the results are recorded in `docs/implementation/benchmarks.md`.

**Architecture:**
- Each program reads an optional size from `Env.args ()`:
  - with no argument it runs at a small default size, which is what `crates/eml_vm/tests/bench.rs` and `bench/run.sh` use;
  - `bench/scripts.sh` passes a large size.
- `compare.sh` gains a path-taking form, which both tables use to check that eml and Python print the same output.
- `gen.py` generates one input file per kind and size, so each S12d script row gets its own input size.
- No compiler changes, except a fix for a bug that keeps a program from running correctly. Such a fix comes with a test written first.

**Tech Stack:** eml programs (`bench/*.em`), Python 3.14 standard library, bash (macOS `/usr/bin/time -l`), insta (`crates/eml_vm/tests/bench.rs`).

**Spec:** `docs/superpowers/specs/2026-10-21-s14d-numeric-bench-design.md`, the whole document. The S14 design `docs/superpowers/specs/2026-10-19-s14-numeric-design.md` gives the S14d and S14e completion conditions.

## Global Constraints

- **Language:** code comments and docs are in Japanese, written with the `yomiyasu:yomiyasu` skill. Comments explain why and cite `docs/` paths. `CLAUDE.md` stays in English.
- **Matching Python:** every float operation in an eml program and its Python version happens in the same order with the same association. The formulas below are normative for both sides.
  - Print floats with `Float.to_fixed 9 x` in eml and `f"{x:.9f}"` in Python.
  - Write unary minus on floats as `0.0 - x` on both sides.
- **Size argument (every eml program in this plan):**
  - no arguments → the default size;
  - exactly one argument that `String.to_int` reads as an integer ≥ 1 → that size;
  - anything else → `panic "usage: <program> [SIZE]"`, where `<program>` is the file's stem, for example `nbody`.
  - Each program has its own `size : Int -> <IO, Panic> Int`, taking the default. Programs cannot share a module.
- **Size argument (Python versions):** they take the size as their only argument, `int(sys.argv[1])`.
- **Default sizes:** each program finishes within 1 s in a debug build with `debug_heap`, which is what `bench.rs` runs. The existing defaults of `sieve_var` (20000) and `fannkuch_var` (7) stay unchanged.
- **Sizes in `bench/scripts.sh`:** each row's size is chosen so that the release `eml run`'s median wall time over 3 runs is about 300 ms, between 250 and 350 ms when chosen.
- **Test changes:**
  - **Kind 1:** none.
  - **Kind 2:** only the `RunStats` snapshots of `sieve_var` and `fannkuch_var`, and `RESIDUAL` if it moves. Any other changed snapshot or UI output is a bug.
- **Gate per task:** the task's commands, then `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check`. Use `git diff --no-ext-diff`.
- **Commits:** messages end with the attribution lines the controller gives.

## Review Focus

- **Bad size arguments:** `eml run bench/nbody.em abc`, `... 0`, `... 1 2` and the Python versions with `0` must all exit non-zero, never print a result or hang. Tests: Task 2 step 6 and Tasks 3–5, step 5.
- **Small sizes at the edges:**
  - `sieve` at 1, 2 and 3;
  - `fannkuch` at 1, 2 and 3;
  - `mandelbrot` at 1;
  - `spectral_norm` at 1;
  - `nbody` at 1.

  Each must match Python. Off-by-one loop bounds show up here first. Tests: the `compare.sh --paths` loops in Tasks 2–5.
- **Float drift at large sizes:** an operation order that differs from Python passes at the default size and diverges after millions of steps. Test: `bench/scripts.sh` compares at the large size before measuring (Task 6). Task 3 step 5 also compares nbody at 100000 steps.
- **Stale cached inputs:** an input generated at another size, or by the old `gen.py`, must never be measured. Test: inputs are named by kind and size (Task 6 step 3), so every size has its own file.
- **Old usages of `compare.sh`:** the `NAME` form and `--fixture` must still print `same`. Test: Task 1 step 1.

---

### Task 1: `compare.sh --paths`

**Files:**
- Modify: `bench/scripts/compare.sh`

**Interfaces:**
- Produces: `bench/scripts/compare.sh --paths EML_FILE PY_FILE [ARG...]`, which reads stdin. It runs `"$EML" run EML_FILE ARG...` and `python3 -I PY_FILE ARG...`, compares stdout, `@OUT` contents and exit code as today, prints `same`, or prints the diff and exits 1.
  - `compare.sh NAME [ARG...]` behaves as `--paths tests/ui/run/scripts/NAME/main.em bench/scripts/NAME.py ARG...`.
  - `--fixture NAME` is unchanged in behaviour.

- [ ] **Step 1: Record the current behaviour.** Build first with `cargo build -p eml_cli`. Then run `bench/scripts/compare.sh --fixture wc` and `printf 'a b\nc\n' | bench/scripts/compare.sh wc -l`. Both print `same`.
- [ ] **Step 2: Add `--paths`.** Make the existing `NAME` form a thin wrapper that resolves the two paths, and update the header comment's usage lines.
- [ ] **Step 3: Verify.** The two commands from step 1 still print `same`. `printf 'x\n' | bench/scripts/compare.sh --paths tests/ui/run/scripts/wc/main.em bench/scripts/wc.py -l` prints `same`. `bench/scripts/compare.sh --paths bench/fib.em bench/scripts/wc.py` prints a diff and exits 1.
- [ ] **Step 4: Commit.** Message: `Let compare.sh take the eml and Python paths`.

### Task 2: Size arguments for the sieve and fannkuch-redux, with their Python versions

**Files:**
- Modify: `bench/sieve_var.em`, `bench/fannkuch_var.em`
- Create: `bench/numeric/sieve.py`, `bench/numeric/fannkuch.py`
- Modify: `crates/eml_vm/tests/bench.rs`, for the kind 2 snapshots only

**Interfaces:**
- Consumes: Task 1's `compare.sh --paths`.
- Produces:
  - `bench/sieve_var.em [N]`: the number of primes below N, default 20000.
  - `bench/fannkuch_var.em [N]`: default 7.
  - `bench/numeric/sieve.py N` and `bench/numeric/fannkuch.py N`, with the same outputs.

- [ ] **Step 1: Write `bench/numeric/sieve.py`.** It is a plain list-of-bool sieve over `range(2, n)` that crosses out from `i * i` and prints the count.
- [ ] **Step 2: Write `bench/numeric/fannkuch.py`.** It follows `bench/fannkuch_var.em`'s loops one for one: the same `perm1`, `perm`, `count`, `r`, flip and rotation steps, the same checksum sign rule (`parity % 2 == 0`), and the same two output lines.
- [ ] **Step 3: Add `size` to both eml programs.** Call it from `main` (`count (sieve (size 20000))`, `fannkuch (size 7)`).
- [ ] **Step 4: Run `cargo test -p eml_vm --test integration bench::`.** `sieve_var` and `fannkuch_var` fail only on their `RunStats` snapshots. Accept the new values with `cargo insta review`. Check that `var_matches_hand_written_work` still passes with `RESIDUAL` unchanged. If `RESIDUAL` moves, update it and note it for the Task 7 record (it is kind 2).
- [ ] **Step 5: Check the sizes against Python.** Run:

```sh
cargo build -p eml_cli
for n in 1 2 3 10 100 20000; do bench/scripts/compare.sh --paths bench/sieve_var.em bench/numeric/sieve.py $n < /dev/null; done
for n in 1 2 3 5 7 8; do bench/scripts/compare.sh --paths bench/fannkuch_var.em bench/numeric/fannkuch.py $n < /dev/null; done
```

Expected: every line prints `same`. If eml fails or differs at an edge size, find out why first. If `fannkuch_var.em`'s loops simply cannot handle that `n`, which the S14c-3 program never had to, fix the program and keep the default output. A compiler bug follows the constraint in the Architecture section.
- [ ] **Step 6: Check bad arguments.** `target/debug/eml run bench/sieve_var.em abc`, `... 0` and `... 1 2` each exit non-zero with the usage message.
- [ ] **Step 7: Commit.** Message: `Give sieve_var and fannkuch_var a size argument and Python versions`. The body names the kind 2 snapshot change.

### Task 3: nbody

**Files:**
- Create: `bench/nbody.em`, `bench/numeric/nbody.py`
- Modify: `crates/eml_vm/tests/bench.rs` (`PROGRAMS` 29 → 30, plus `fn nbody`)

**Interfaces:**
- Produces: `bench/nbody.em [STEPS]` and `bench/numeric/nbody.py STEPS`. Both print two lines: the energy before the steps and the energy after, each with 9 decimals.

Normative computation, the same on both sides. `Vec3 x y z`, `sub a b = Vec3 (a.x - b.x) (a.y - b.y) (a.z - b.z)`, `add` likewise, `scale v k = Vec3 (v.x * k) (v.y * k) (v.z * k)`, `dot a b = a.x * b.x + a.y * b.y + a.z * b.z` (left to right).
- **Bodies:** in the Benchmarks Game order: sun, Jupiter, Saturn, Uranus, Neptune. Positions and velocities use the Game's literals. Each velocity literal is multiplied by `days_per_year = 365.24`, component by component. Each mass is `literal * solar_mass`, with `solar_mass = 39.47841760435743` written as a literal because a top-level value cannot do arithmetic. The sun's mass is `solar_mass` itself.
- **Offset:** `p` starts at `Vec3 0.0 0.0 0.0`. For `i` from 0 to 4: `p = add p (scale vel[i] m[i])`. Then `vel[0] = Vec3 (0.0 - p.x / solar_mass) (0.0 - p.y / solar_mass) (0.0 - p.z / solar_mass)`.
- **Energy:** `e = 0.0`. For each `i`: `e = e + 0.5 * m[i] * dot vel[i] vel[i]`. Then for each `j > i`: `d = sub pos[i] pos[j]`; `e = e - m[i] * m[j] / Float.sqrt (dot d d)`.
- **Step (`dt = 0.01`):**
  - for each `i < j`: `d = sub pos[i] pos[j]`; `d2 = dot d d`; `mag = dt / (d2 * Float.sqrt d2)`; `vel[i] = sub vel[i] (scale d (m[j] * mag))`; `vel[j] = add vel[j] (scale d (m[i] * mag))`;
  - then for each `i`: `pos[i] = add pos[i] (scale vel[i] dt)`.
- **State:** `pos` and `vel` are array variables (`var pos = ...`, `MutArray Vec3`), and the masses are an `Array Float`. Because of E3008, `freeze` happens only in a block's last expression, so split the functions accordingly. The energy function takes the frozen `Array Vec3`s.

- [ ] **Step 1: Write `bench/numeric/nbody.py`** with the computation above, using lists of `[x, y, z]`. Run `python3 -I bench/numeric/nbody.py 1000` and keep its two lines; they are the expected output at the default size.
- [ ] **Step 2: Write the failing test.** Add `"nbody"` to `PROGRAMS`, keeping it sorted, with the count 30. Add:

```rust
#[test]
fn nbody() {
    insta::assert_debug_snapshot!(stats("nbody", "<line 1>\n<line 2>\n"), @"");
}
```

Use the two lines from step 1. Run `cargo test -p eml_vm --test integration bench::nbody`. Expected: FAIL, because the file is missing. `every_program_in_bench_has_a_test` should now fail the other way, too.
- [ ] **Step 3: Write `bench/nbody.em`.** Use the default of 1000 steps, or less if the debug test takes more than 1 s.
- [ ] **Step 4: Run the test** and accept the snapshot with `cargo insta review`. Look at `allocations`: it is the S14e baseline, so make sure it grows with the step count, which shows that each vector operation allocates.
- [ ] **Step 5: Compare at more sizes.** Run `cargo build -p eml_cli`. Then `bench/scripts/compare.sh --paths bench/nbody.em bench/numeric/nbody.py $n < /dev/null` for n = 1, 10, 1000, and 100000 (with `EML=target/release/eml` for the last). Each prints `same`. Also check that `abc`, `0` and `1 2` exit non-zero.
- [ ] **Step 6: Commit.** Message: `Add nbody with Vec3 and its Python version`.

### Task 4: spectral-norm

**Files:**
- Create: `bench/spectral_norm.em`, `bench/numeric/spectral_norm.py`
- Modify: `crates/eml_vm/tests/bench.rs` (`PROGRAMS` → 31, plus `fn spectral_norm`)

**Interfaces:**
- Produces: `bench/spectral_norm.em [N]` and `bench/numeric/spectral_norm.py N`. Both print one line: the norm with 9 decimals.

Normative computation:
- `a i j = 1.0 / Float.of_int ((i + j) * (i + j + 1) / 2 + i + 1)`.
- `mul_av v` gives a vector whose element `i` is the sum over `j` from 0 to n−1, ascending, starting at 0.0, of `a i j * v[j]`.
- `mul_atv v` is the same with `a j i`.
- `mul_atav v = mul_atv (mul_av v)`.
- `u` starts as n copies of `1.0`. Repeat 10 times: `v = mul_atav u`, then `u = mul_atav v`.
- `vbv` is the ascending sum of `u[i] * v[i]`, and `vv` the ascending sum of `v[i] * v[i]`. Print `Float.sqrt (vbv / vv)`.
- **Representation:** vectors are `Array Float`, built with `Array.tabulate`. Each sum is a `var` accumulator with `for`.

- [ ] **Step 1: Write `bench/numeric/spectral_norm.py`** and record its output at the default size, 100.
- [ ] **Step 2: Write the failing test** `fn spectral_norm` in the same shape as Task 3's, with the expected line. Add `"spectral_norm"` to `PROGRAMS`. Run it; expected: FAIL.
- [ ] **Step 3: Write `bench/spectral_norm.em`.** Use the default 100, or less to fit the 1 s debug bound.
- [ ] **Step 4: Run the test and accept the snapshot.**
- [ ] **Step 5: Compare** at n = 1, 2, 10 and 100 with `compare.sh --paths`, and check the bad arguments as in Task 3.
- [ ] **Step 6: Commit.** Message: `Add spectral-norm and its Python version`.

### Task 5: mandelbrot

**Files:**
- Create: `bench/mandelbrot.em`, `bench/numeric/mandelbrot.py`
- Modify: `crates/eml_vm/tests/bench.rs` (`PROGRAMS` → 32, plus `fn mandelbrot`)

**Interfaces:**
- Produces: `bench/mandelbrot.em [N]` and `bench/numeric/mandelbrot.py N`. Both print one line: the number of grid points that never escape.

Normative computation:
- `Complex re im`, with `add`, `mul (Complex a b) (Complex c d) = Complex (a * c - b * d) (a * d + b * c)`, and `norm2 (Complex a b) = a * a + b * b`.
- For `y` from 0 to n−1 and `x` from 0 to n−1: `c = Complex (2.0 * fx / fs - 1.5) (2.0 * fy / fs - 1.0)`, where `fx`, `fy` and `fs` are `Float.of_int` of `x`, `y` and `n` (`fn` is a keyword), and `2.0 * fx / fs` means `(2.0 * fx) / fs`.
- `z` starts at `Complex 0.0 0.0`. Up to 50 times: `z = add (mul z z) c`; if `norm2 z > 4.0`, the point has escaped and the loop stops.
- Points that finish all 50 iterations without escaping are counted.
- **Python:** write `mul` and `add` out the same way: `re = (zr * zr - zi * zi) + cr`, `im = (zr * zi + zi * zr) + ci`. Do not use `complex`, whose multiplication order is not pinned.

- [ ] **Step 1: Write `bench/numeric/mandelbrot.py`.** Pick the default size: the largest power of two up to 128 whose debug `bench.rs` run stays within 1 s. Record the output at that size.
- [ ] **Step 2: Write the failing test** `fn mandelbrot`, add the name to `PROGRAMS`, and run it; expected: FAIL.
- [ ] **Step 3: Write `bench/mandelbrot.em`.**
- [ ] **Step 4: Run the test and accept the snapshot.**
- [ ] **Step 5: Compare** at n = 1, 2, 16 and the default size, and check the bad arguments.
- [ ] **Step 6: Commit.** Message: `Add mandelbrot with Complex and its Python version`.

### Task 6: Per-row sizes and the numeric table in `bench/scripts.sh`

**Files:**
- Modify: `bench/scripts/gen.py`, `bench/scripts.sh`

**Interfaces:**
- Consumes: Task 1's `compare.sh --paths`, and the programs and Python versions from Tasks 2–5.
- Produces:
  - `python3 -I bench/scripts/gen.py {log|csv} KIB OUTPUT` writes about `KIB` KiB of that kind to `OUTPUT`. It uses a fresh `random.Random(12)` per file, so a given kind and size always produce the same bytes.
  - `bench/scripts.sh` prints the environment lines, then two tables, each starting with a 大きさ column: the scripts table and the 「数値計算」 table.

- [ ] **Step 1: Measure at the old sizes.** Before changing anything, run the current `bench/scripts.sh`. Save its output to `target/bench-scripts/s14d-30mb.md`. These are the 30 MB values the S14d record needs. If a compiler change lands after this step, rerun it from a worktree at this task's parent commit.
- [ ] **Step 2: Change `gen.py`'s interface** to `gen.py {log|csv} KIB OUTPUT`. Keep the `write_log` and `write_csv` bodies, and update the header comment.
- [ ] **Step 3: Rewrite `bench/scripts.sh`'s rows.**
  - **Script rows:** `script_row LABEL KIND KIB NAME [ARG...]`. The input is `$data/access-${KIB}k.log` or `$data/data-${KIB}k.csv`, generated with `gen.py` when missing. The 大きさ cell is `${KIB} KiB`. For `csv2jsonl`, the CSV path is an argument and stdin is `/dev/null`, as today.
  - **Numeric rows:** `numeric_row LABEL PROGRAM PY SIZE`. The program is `bench/PROGRAM.em`, the Python version is `bench/numeric/PY.py`, and stdin is `/dev/null`. The 大きさ cell is `SIZE`. Each row first runs `compare.sh --paths` at `SIZE`.
  - **Numeric table labels:** `nbody`, `spectral-norm`, `mandelbrot`, `fannkuch-redux`, `sieve`.
  - Update the header comment, and the input line among the environment lines, which now lists each generated input's size.
- [ ] **Step 4: Choose the sizes.** For each of the 10 rows, try candidate sizes with `/usr/bin/time -p target/release/eml run ...`. Pick the size whose median over 3 runs is about 300 ms (250–350 ms), and write the sizes into `bench/scripts.sh`.
- [ ] **Step 5: Run `bench/scripts.sh`.** Expected: both tables print, every row passes its comparison, and every eml time lies in 250–350 ms. Save the output to `target/bench-scripts/s14d.md` for Task 7.
- [ ] **Step 6: Commit.** Message: `Size every scripts.sh row to about 300 ms of eml time and add the numeric table`.

### Task 7: The S14d record and the documents

**Files:**
- Modify: `docs/implementation/benchmarks.md`, `docs/future/roadmap.md`, `docs/implementation/testing.md` (「よく使うコマンド」), `CLAUDE.md` (the Commands lines for `bench/scripts.sh` and `compare.sh`)

**Interfaces:**
- Consumes: `target/bench-scripts/s14d-30mb.md` and `s14d.md` from Task 6, the `bench.rs` snapshots, and any friction noted while writing Tasks 2–5.

- [ ] **Step 1: Run `bench/run.sh`** and keep its output.
- [ ] **Step 2: Count the allocations S14e will remove.**
  - For `nbody` and `mandelbrot` at the default size, use the optimized Core IR (`Pass::Optimize`) or a count in the program. Split `allocations` into two parts: `Vec3` / `Complex` construction (1 object each; the `Float` fields are raw), which S14e removes, and writes into arrays (boxed `Float`s, and after S14e boxed `Vec3`s), which S14e will not remove.
  - Do the same for `spectral_norm`, where every allocation should be an array element box or an array.
- [ ] **Step 3: Update `benchmarks.md`.**
  - 「基準のプログラム」: add the three new programs, and the size argument on `sieve_var` and `fannkuch_var`.
  - 「回数のテスト」: no change unless `RESIDUAL` moved.
  - 「スクリプトの計測」: the 300 ms rule from the spec's 「大きさ」 (target, the 150/600 ms resize threshold, recording both sizes on a resize, the size column, the `instructions retired` note), per-row inputs from `gen.py`, `bench/numeric/` and `compare.sh --paths`. Replace the 「約 30 MB」 text, and restate the goal as at most 1.5× Python's wall time.
  - 「記録」 gets a new section 「S14d」 containing:
    - the environment;
    - the scripts table at 30 MB, compared with S13e-4;
    - both tables at the new sizes;
    - the `bench/run.sh` table;
    - the five programs' `RunStats`;
    - the allocation split from step 2;
    - a 「書いてみて分かったこと」 list.
- [ ] **Step 4: Update `roadmap.md`.**
  - Mark S14d done in the S14 work table, with a short summary and pointers into `benchmarks.md`.
  - Add S14d to the 「済み」 list in the 段の列 completion column.
  - Move 「計測に選ぶプログラム」 from 論点 to the settled 「計測」 item.
  - Add each new piece of friction to 論点 or 「その後の項目」. The E3008 split is one. The top-level arithmetic limit is already known, so it only gets a mention in the record.
- [ ] **Step 5: Update `testing.md` and `CLAUDE.md`.** Their `bench/scripts.sh` lines now say "S12d scripts and numeric programs against Python, sized to ~300 ms of eml time". Their `compare.sh` lines gain the `--paths EML PY ARGS...` form.
- [ ] **Step 6: Run the yomiyasu check** on the changed Japanese docs, then `cargo test`, clippy and fmt.
- [ ] **Step 7: Commit.** Message: `Record S14d and document the numeric benchmarks`.
