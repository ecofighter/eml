> 設計の段階の付録 (英語)。本文 (`../2026-10-31-io-uring.md`) に次いで優先する。01〜04 の付録と食い違うときは、この文書が勝つ。

# Rulings and resolutions that override appendices 01-04

## A. User answers (settled)

1. Binary rule confirmed: io_uring lives in the repo only as the Linux default with epoll fallback; a blocking regression means fix or discard (no "epoll default + io_uring opt-in").
2. If GitHub Linux runners cannot run io_uring (the pinned CI step fails), discard by default.
3. Push permission granted: after the continue decision, push the stage branch (ccr-f0300e7d-bir57l) to origin when CI changes land, and check CI.
4. macOS 「今の数値」 re-measure is waived; close on Linux only; the waiver and reason go in the Close commit message.
Earlier: min kernel may be raised (S1 picked 6.1); bindings decided by F1 (hand-written, no io-uring/rustix/polling crate); no POLL_ADD-readiness arm.

## B. Fable ruling on conformance (G-5 and G-11 amendments approved by the user)


Read-only checks on /home/user/eml. [V] verified, [I] inferred.

## 1. V-1 (abort child test in `sys/uring.rs` reads `EML_IO_ABORT_CHILD`) — violates as planned; criterion unchanged

- Intent of the G-6/G-7 grep: the only environment input to backend *selection* is `EML_IO_BACKEND`, read in `real.rs`. A test's child marker is not a selection input, so the criterion is right; the planned test puts the read in the wrong place.
- Not the remedy: moving the test under a `tests/` directory. `sys/` keeps its unit tests inline (`sys/mod.rs:48`, `sys/unix.rs:708`, `sys/iocp.rs:935` [V]); bending layout to satisfy a grep is what CLAUDE.md forbids. Not the remedy: narrowing the grep to `EML_IO_BACKEND`; the ring test helper's panic text names that variable in `sys/uring.rs`, so that grep would also print two lines.
- Rule the designers apply: **`EML_IO_ABORT_CHILD` is read in exactly one place, `driver/tests/abort.rs::in_child` (today line 19 [V]).** Make `in_child` `pub(crate)` (and `driver/tests` `pub(crate) mod tests` under `#[cfg(test)]`); the ring abort test calls `crate::driver::tests::abort::in_child("eml_io::sys::uring::tests::dropping_the_ring_with_a_recv_in_flight_aborts", || ...)`. No new `var_os`/`env::var` call appears anywhere else in `crates/eml_io/src`. The helper's `--exact` path logic already handles any module path [V abort.rs:24-27].
- Completion: the G-6 grep prints exactly one line, in `real.rs`, with the ring tests present. `rg -n 'EML_IO_ABORT_CHILD' crates/eml_io/src` prints only `driver/tests/abort.rs` lines.

## 2. V-2 (`with_lookup` builds via `from_env` with no value-taking sibling) — violates; required now, not deferred

- G-7 is a MUST checked at the stage's end; "add it only if a test needs both" leaves the check failing by its literal reading and leaves the decision to the implementer, which the criterion exists to prevent. The intent (an embedder reaching any configuration without the environment) is not met for the custom-lookup configuration; `eml_vm/tests/api.rs:935,958` use it today [V].
- Signature rule: **one value-taking root constructor, every other constructor a one-line delegate to it, and `from_env()` called only in delegates.**

```rust
pub fn with_backend_and_lookup(b: IoBackend, lookup: fn(&str, u16) -> io::Result<Vec<SocketAddr>>) -> io::Result<RealDriver>  // the root; reads no environment
pub fn new() -> io::Result<RealDriver>                      // with_backend_and_lookup(IoBackend::from_env()?, sys::lookup)
pub fn with_backend(b: IoBackend) -> io::Result<RealDriver> // with_backend_and_lookup(b, sys::lookup)
pub fn with_lookup(lookup: ...) -> io::Result<RealDriver>   // with_backend_and_lookup(IoBackend::from_env()?, lookup); callers unchanged
```

- Completion: G-7's grep becomes `rg -n 'pub fn from_env|pub fn with_backend|pub fn with_backend_and_lookup' crates/eml_io/src/real.rs` printing three lines; `rg -n 'from_env' crates/eml_io/src/real.rs` prints its definition plus the two delegates and nothing in `with_backend_and_lookup`.

## 3. I-1 (`IoBackend { Auto, Epoll, IoUring }` in `real.rs`) — the guideline needs amending (scope only)

- F1 itself puts `IoBackend` in `real.rs` (Q4, §5.4) while G-5's "Applies to" lists `real.rs`; the two cannot both be literal. The selection value must name the backends somewhere public (its values are the variable's values), and `real.rs` is `eml_io`'s public surface, already the one file that names `sys::Os` [V real.rs:16]. The invariant G-5 protects is the trait and the core (`the_core_names_no_os`, trust.rs:256 [V]); the deliverables' reading (b) is the intended one.
- Amended G-5 text (replace "Applies to" and add one sentence to the code check):
  - Applies to: `crates/eml_io/src/driver/` (the `Backend` trait and the core). `real.rs` is the public surface: it may name OS backends only as `IoBackend`'s variants and in the docs of `from_env` / `with_backend` / `with_backend_and_lookup`; the type's definition and `from_env` are the only selection code outside `sys/`. Not covered: `cfg_attr(expect(dead_code))` lists; adding OS-neutral methods in later stages.
  - Code check: unchanged (`trust::` passes; the `driver/` grep prints only comment lines), plus `rg -n 'epoll|io_uring|kqueue|iocp' crates/eml_io/src/real.rs -i` prints only lines inside `enum IoBackend`, `impl IoBackend`, or `///` doc lines.

## 4. G-11 in the discard branch — the guideline needs amending: continue-branch criterion, with a discard check

- S2's challenge holds: after D1, `bench.py` has no `--env` and the tree has no `EML_IO_BACKEND` [I from S2 D1 scope; today `rg -- '--env' bench` prints nothing [V]]. Pinning the five recorded commands (benchmarks.md 385, 432, 505, 523, 524 [V]) would document an option that does not exist, against "docs describe the current state". G-11's own wording ("post-prototype binary", "the default may change") presupposes a shipped second backend.
- Amended G-11 text (append after "Not covered"):
  - Branch: continue branch only. In the discard branch no pin exists; the discard check is: `rg -l -- '--env|EML_IO_BACKEND' bench docs` prints exactly `docs/implementation/benchmarks.md` and `docs/future/roadmap.md`; in benchmarks.md every hit is inside `### io_uring と epoll`, where the recorded gate command carries the pins as the record of the measured prototype binary; `rg -n -- '--env' bench` prints nothing.
- The continue-branch check stays as written.

## Additional checks

- `rg 'var_os|env::var' crates/eml_io/src`: one hit, `driver/tests/abort.rs:19` [V].
- `sys/` test modules are inline `#[cfg(test)] mod tests`; only `driver/` uses a `tests/` directory [V find + grep].
- `RealDriver` today: `new()` delegates to `with_lookup`; two external callers of `with_lookup` in `eml_vm/tests/api.rs` [V].
- `the_core_names_no_os` word list does not include `epoll`/`io_uring` [V trust.rs:261-270].

## C. Orchestrator resolutions, conformance round 1


- Cf-1 Close gate failure: S3 §1.8 governs (fix-and-retry, then discard), consistent with the user's binary-rule answer. S2 aligns its wording.
- Cf-2 "ring tests do not run" checks: completion commands use `cargo nextest run ... --success-output immediate` (or `--no-capture`); owner of each command fixes it (S1 P2, S3 U2/U3).
- Cf-3 benchmarks.md `--env` sentence: M1 (S2) adds the sentence describing `--env` next to the bench.py usage, because M1 adds the option; rewriting older recorded commands with an epoll pin is continue-only, at Close (S3). Discard reverts M1, so the sentence disappears with it.
- Cf-4 `pick` shape: S1's P2 shape governs (it lands first). S3 U2 states its edits against S1 r3's exact signature; S3's switch point also checks sys/linux.rs.
- Cf-5 non-blocking enter at the end of `Uring::new`: moves into S1's P2, so the gate, R0 and Close measure the same startup path. S3 drops it from U2 (keeps the test).
- Cf-6 roadmap line 31: no unit rewrites it in place. The BASE plan commit leaves it; D2 (discard) deletes it and writes the 「その後の項目」 item; Close (continue) deletes it and writes the async-files 論点 and items (a)-(d). Completion greps check deletion and new text, not a rewritten line 31.
- Cf-7 spec text on the default: written in U2, the commit that flips `Auto` (G: spec in the same commit as the rule). U1 only removes the losing arm and its value from error text.
- Cf-8 M3 records the gate result and the arm ("門: 通った <腕>" or "門: 通らない"); the 続ける/捨てる decision is recorded after R0 (S3) in the plan, in a commit `Record the io-uring decision` (plan-only). If the gate fails, M3 also records 捨てる and goes to D1.
- Cf-9 profiles.rs self-match: S3 fixes (exempt profiles.rs itself or build scan strings so they don't self-match).
- Cf-10 discard completion diff path set: one list for both D1 and §1.8: `crates bench .config .github flake.nix Cargo.toml Cargo.lock THIRD-PARTY-LICENSES.md docs/spec README.md CLAUDE.md docs/implementation/testing.md`.
- Cf-11 check.py source: S3 U4 cites S2 r3's check.py block; label handling verified by S2.

## D. Orchestrator resolutions, conformance round 2

- R2-1 `Uring::drop`'s abort message: P2 defines it in `sys/uring.rs` as `pub(super) const URING_HELD_ABORT: &str`, written to stderr by `Uring::drop` before `std::process::abort()` when a memory-holding op is still in flight. The text is English, in the style of the core's `HELD_ABORT` (`driver/mod.rs:38`); it names io_uring and the in-flight operation. U1's ring abort test passes it to `in_child(path, message, body)`.
- R2-2 Tests changed by the continue branch, listed with their kinds (CLAUDE.md):
  - U1, kind 1: delete the losing arm's tests (try-first wins: `a_direct_recv_skips_the_sync_attempt`, `a_short_sync_recv_clears_ready_in_direct`; direct wins: none deleted) and the `io_uring_direct` / `IoUringDirect` cases inside `parse_maps_each_value` and `pick_builds_the_named_backend`.
  - U1, kind 3: renames when direct wins (`a_recv_on_a_row_not_known_readable_skips_the_sync_attempt`, `a_short_sync_recv_clears_ready`); `in_child` gains `message`, existing callers pass `HELD_ABORT`; ring tests lose the arm loop.
  - U2, kind 1: delete `pick_builds_the_named_backend` (its `Auto → Epoll` expectation contradicts the new default); its other cases move into the new `pick` tests and `new_follows_the_variable`.
- R2-3 M3's script copies `check.py` before use: after `S=$(mktemp -d)`, write the plan's `check.py` text (§4 of the plan body) to `$S/check.py`.
- R2-4 Discard revert order: revert every commit after BASE except plan-only commits, strictly newest first (`git log --format=%H BASE..HEAD -- crates bench .config .github flake.nix Cargo.toml Cargo.lock THIRD-PARTY-LICENSES.md docs/spec README.md CLAUDE.md docs/implementation docs/future docs/README.md` gives the list; revert in that order). This replaces "U4, U3, U2, every U5, U1" and "in S3's order".
- R2-5 JSONL line counts are informative only. The binding checks are: `check.py` exit status as specified, `report.py` prints one `## 比べ:` table per later label, and every gate case appears in each table. Gate sessions with A ring arms have 12·(A+1) lines [V: 36 for 3 labels].
- R2-6 Citations of "S1 r3" / "S2 r3 §5 M3" in 04-continue.md mean 02-prototype.md (S1 r4) and 03-measurement.md §5 M3 (S2 r4).
- R2-7 `IoBackend::parse` carries `#[cfg(target_os = "linux")]`, like `from_env`'s reading branch, so macOS and Windows builds have no unused function.
- R2-8 Close (continue branch only) rewrites roadmap.md's 「macOS の kqueue の書きの起こし」 item, whose 後に回した理由 says 「syscall の目標は epoll だけに置き」 (line 391 at BASE): after Close the targets sit on both Linux backends, so it becomes 「syscall の目標は Linux の backend (epoll と io_uring) にだけ置き」. The discard branch leaves it unchanged.
- R2-9 G-11 continue-branch check, made exact: labelled `bench.py` commands get `--env <label> EML_IO_BACKEND=epoll` per label (benchmarks.md 523, 524 at BASE: `u512`/`u256`/`u1024`); commands without labels (432, 505 at BASE) get the phrase 「epoll の backend で」 in their sentence. Check: `rg -n 'bench\.py[^|]*[A-Za-z0-9_]+=' docs | rg -v -- '--env' | rg -v 'main='` prints nothing, and `rg -n 'bench\.py' docs/implementation/benchmarks.md` hits without a label each sit in a sentence containing `epoll`.

## E. Decisions during execution (T-3)

- E-1 Connect submits at once. After queuing a `CONNECT`, `Uring` issues one non-blocking `io_uring_enter(unsubmitted, 0, REGISTERED_RING)` (no `GETEVENTS`), because vm.md 「時計と driver」 says `submit` puts the step's one attempt before the OS and the peer sees nothing until the SYN leaves. Other steps keep submitting at the next `wait`. Cost: +1 enter per connect. If the enter fails, the SQE is retracted and the step ends `Start::Done(Err)`.
- E-2 `Ring::drop` calls `IORING_UNREGISTER_RING_FDS` before unmapping and closing; without it the ring is not torn down when the fd closes. Every criterion that says `io_uring_register` 1 now means 2: 02-prototype.md §5 P2 item 12, 04-continue.md §5 U1 item 8, and `URING_TARGETS`' structural row (`io_uring_setup` = 1 and `io_uring_register` = 2). The connect-check row of `URING_TARGETS` counts no `connect` syscall; the extra enter is not a connect check.
- E-3 Registered ring fds are limited to 16 per thread by the kernel; a 17th live `Uring` on one thread fails `REGISTER_RING_FDS` with EBUSY (`ResourceBusy`). Current callers hold one driver per thread. T-9 (U2) decides, with a test, one of: (a) on EBUSY run that ring unregistered (enter with the real fd), or (b) treat EBUSY as a fallback class for `Auto` only. Default (a): it keeps one backend per process (G-6) and costs only the fd lookup on that ring's enters. Record the choice in T-9's commit message.
- E-4 Construction errors: EINVAL from the construction enter maps to `Unsupported` like setup and register.
