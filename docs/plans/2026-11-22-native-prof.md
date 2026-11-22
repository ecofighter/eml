# Stage native-prof: a time profiler for native runs on Linux and macOS

- Created: 2026-11-22 (design-level plan).
- Base: the `Close jit-tasks` commit. Facts and line numbers in the design describe main @ 0f04d6a2; the refresh re-checks them.
- Status: design-level. `Start native-prof` replaces this file with an executable plan.
- Appendix: `docs/plans/2026-11-22-profiler/` (`design.md`, `prototype.patch`), shared with `ir-origins`; this stage is the last planned with it, and its Close deletes the directory. The design is `design.md` sections 2, 4 and 6, with sections 8.2, 8.3, 9, 10 and Appendix B (the soundness review of the sampler). Decisions: `docs/plans/2026-11-17-runtime/00-decisions.md` §D D-20.

## 1. Goal and completion conditions

- Goal: `eml run --prof FILE` records a time profile of a native fast-mode run on Linux and macOS without changing generated code, and `eml prof report`, `eml prof folded` and `eml prof export` attribute it to source functions, lines, inlined frames and roles.
- Completion conditions:
  - The tests of design 6.10 pass: the walk over the slice reader (also under Miri), the registry protocol, the synchronous sample through a user `extern "C"` function, the deterministic `EINTR` tests, two workers, two runs in one process, the refusal when SIGPROF is owned, a truncated file.
  - On macOS arm64 and x86_64 Linux a profile of each `bench/` program and of a deep-recursion probe has at least 98 % rooted and trusted samples.
  - Overhead at 1 kHz is at most 2 % wall on `bench/fib.em` and `bench/rbtree.em` sized to 2 s, and at most 7 % on the deep probe (`#[ignore]` measurements, recorded in benchmarks.md).
  - The fast CLIF pins are unchanged; no runtime entry gains an instruction.
  - `grep -rn 'thread_local' crates/eml_sched/src/prof*` is empty; `unsafe` appears under `crates/eml_sched/src/prof/` only in `sys.rs`, pinned by a test.
  - `--prof` is a usage error with the VM, on Windows, with `--debug-heap` or `--stats`, and is refused when the process already has a SIGPROF handler.
  - A file left by a second Ctrl-C, `exit`, a fatal error or a kill is read up to its last complete record.

## 2. Scope

- In: `eml_sched::prof` (the registry and `prof::registered`, the handler and its installation, the walk over `Words`, `Live` with one assembly load, the rings, the governor, the monitor's tick, state samples, the file); the `Io` phase guard around the rows that block synchronously; `--prof` and `--prof-hz`; the `Load` records written by `eml_cli`; `eml prof report` (function, line, role, worker, `--user`, `--split`), `eml prof folded`, `eml prof export` (Firefox Profiler JSON); the frame-pointer flag on x86_64 Linux; the trust rows SM-1, SM-3, SM-4; the user's guide.
- Out: Windows (`prof-windows`, its own design and soundness review); the count profile (`prof-counts`); task events, waits, per-task views and the sampling of pool threads (`prof-tasks`); the heap profile (`prof-heap`); built apps (`prof-aot`); backtraces (`native-backtrace`); the exit anchor at C calls (not taken without the user, section 9); `eml_rt` (no change).

## 3. Received from earlier stages

- `ir-origins`: origins, roles and the encoded tables; `NativeMachine::points`; the `origins` option.
- `sched-core`: `Slot.current` (H-2), `Slot.phase` (H-8: `Run`, `Idle`, `Io`, `Join`, `Park`), `Policy::at` (H-6), the monitor thread, docs/spec/runtime.md 「信頼の一覧」 with a group for memory-safety assumptions.
- `jit-tasks`: every program runs natively at any worker count; the worker thread bodies; the frame chain test (H-1); the stack headroom of a worker; roles set by every lowering it added.
- `native-checked`: the entry descriptor's `class` with fixed discriminants and names.

## 4. Decisions (design sections 2, 4 and 6)

- D-NP-1: recording is a flag of `run`; reporting is the subcommand `eml prof`. Time is wall-clock time of a worker in phase `Run`.
- D-NP-2: the mechanism is a directed `SIGPROF` sent by the monitor thread under the registry mutex to each registered worker in phase `Run`; the handler walks its own thread's frame chain. No timer signal, no thread suspension, no thread-local.
- D-NP-3: a registry entry is keyed by the registering thread's `pthread_t`, stored last and cleared first; the handler matches its own key and then requires `lo <= sp < hi`. Registration is the scoped function `prof::registered`; the entry holds the handle the monitor signals.
- D-NP-4: the walk is safe code over a reader; the shipped reader checks bounds and alignment and loads one word by inline assembly; its contract is "mapped and readable". The walk continues to the root (at most 4,000,000 records), keeps 48 leaf and 16 root slots with repeat counts, and a governor bounds walking at 5 % of a worker's time.
- D-NP-5: signal state: `SA_SIGINFO | SA_RESTART`, an empty mask, installed once over `SIG_DFL` or `SIG_IGN` only and never removed; SIGPROF unblocked at registration; the handler's libc functions called once at installation.
- D-NP-6: the file is self-delimiting records; each load's tables are written when the code is loaded; the rings are drained every 100 ms.
- D-NP-7: the tool decides `rooted` and `trusted`; the report prints the broken and untrusted shares.
- D-NP-8: `unsafe` lives in `prof/sys.rs` only; the trust rows are SM-1, SM-3 and SM-4 in runtime.md.
- D-NP-9: `--prof-hz` is clamped to 1,000; the export format is Firefox Profiler's processed-profile JSON with a pinned version.

## 5. Units (in order)

1. NP-1 `Start native-prof`: the executable plan; the refresh of section 10; the toolchain check of design F-16; `prof/`'s file list and its `trust` test.
2. NP-2: the walk, `Words`, `Live`, the ring, the governor; the Miri line.
3. NP-3: the registry and `prof::registered` in the worker thread bodies, the handler and its installation, the monitor's tick, state samples, the `Io` phase guard around the synchronously blocking rows (listed by this unit).
4. NP-4: the usage errors and the deterministic `EINTR` tests.
5. NP-5: the file, the `Load` records and `--prof`.
6. NP-6: `eml prof report`.
7. NP-7: `eml prof folded` and `eml prof export`.
8. NP-8: the frame-pointer flag on x86_64 Linux and the measurement of broken plus untrusted samples.
9. NP-9: CI lines (the synchronous tests on Linux x86_64 and aarch64 and on macOS; ASan with the handler tests; TSan with the registry, ring and monitor protocol only); the overhead measurements.
10. NP-10 `Close native-prof`.

## 6. Test changes by kind (scope)

- Kind 1 and kind 2: none to existing tests. The CLI's usage text gains the flags and the subcommand (kind 2 for the snapshot of the usage text, if one exists at the base).
- New: the tests of design 6.10, the report and export tests on recorded files checked into the test directory, the `trust` test, the `#[ignore]` measurements.

## 7. docs/spec sections written

docs/spec/runtime.md 「プロファイルの記録」 (new: the registry, the handler, the walk, the file, the phases) and 「信頼の一覧」 (SM-1, SM-3, SM-4); docs/implementation/profiling.md (new: the user's guide, with the limits of design 4.4); docs/implementation/architecture.md (`eml_sched`'s section, 「CLI と lib API」); docs/implementation/testing.md (the sampler's tests, the sanitizer lines); docs/implementation/benchmarks.md (the overhead record); CLAUDE.md (the Commands line of `run`, the signal handler rule, the topic table).

## 8. Acceptance checks

- CLAUDE.md's gates on Linux and macOS; the Windows build still passes with `--prof` refused.
- The completion conditions of section 1.
- The soundness review of the sampler (design Appendix B) is re-read against the executable plan before NP-3; a change of mechanism goes back to a strong advisor (CLAUDE.md's rule on `unsafe` boundaries).
- Roadmap: the `native-prof` entry is deleted; the later items it left are in 「その後の項目」; `docs/plans/2026-11-22-profiler/` is deleted.

## 9. Risks with defaults

| Item | Default |
|---|---|
| Broken plus untrusted samples on x86_64 Linux exceed 2 % | stop; the measurement goes to the user with the exit anchor (a new `Native` field in `eml_rt` and a store before direct C calls) as the proposal. No change to `eml_rt` or fast code without the user's approval |
| The pinned std has no frame pointers on x86_64 Linux | found at NP-1; the same stop, with the anchor extended to runtime entries of class `Row` and `Task` as the proposal |
| A libc function the handler calls is not async-signal-safe on a supported target | SM-4; the stage stops and takes the question to a strong advisor |
| The executable plan changes the registry protocol, the reader or the signal state | the changed part is reviewed for soundness again before its unit |
| A profiled run changes a program's behaviour through `EINTR` | the deterministic tests of NP-4 and an `#[ignore]` run of the `io::`, `signal::` and FFI suites under a 1 kHz sampler; a failure stops the unit |
| The Firefox Profiler UI does not upgrade the pinned format version | the export writes the Gecko format instead |
| Table memory or load time with `origins` on is over the budget of design 2.3 | the measurement is recorded and the tables are delta-encoded further; `--prof` still works |

## 10. Refresh at the stage's first commit

Re-read against the base: the monitor thread and `Slot` of `sched-core` (H-2, H-8), the worker thread bodies and stack headroom of `jit-tasks`, the signal handlers of `eml_cli` (or of `eml_io`, if the interrupt handling has moved), the doorbell's wait, the rows that block synchronously, and `eml_sched`'s dependencies. Confirm on x86_64 Linux what the prototype showed on macOS arm64 only (design F-5, F-6, F-18, F-19).
