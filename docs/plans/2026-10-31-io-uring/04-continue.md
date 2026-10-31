> 設計の段階の付録 (英語)。優先の順は、本文 (`../2026-10-31-io-uring.md`) > `05-rulings.md` > この付録である。`scratch/…` や `scratch-s*/…` の名前は設計のときの作業用のファイルで、リポジトリにはない。本文に要るものは本文か `06-harness.patch` に写してある。

# S3 (r4): the continue branch of `io-uring`: shipping the io_uring backend

Tags: [V] = verified, with how; [I] = inferred. "A" is the arm the gate passes: `io_uring` (try-first) or `io_uring_direct` (ready-gated). After U1, A's code answers to the value `io_uring`.

**Policy and rulings.**
- Policy comes from F1 r2. G-5 and G-11 are replaced by the amended texts in X-ruling-r1 §3 and §4, which the user approved.
- Binding as well: X-ruling-r1 (V-1, V-2), X-resolutions-r1 (Cf-1 … Cf-11) and user-answers-2.
- The table at the end maps each finding to its change. §8 is the Challenge section.

**Ownership.**
- S1 owns P2's shape: `IoBackend`, the constructor set, `Linux::pick`, `Uring`, the enter at the end of `Uring::new`, and the `ring()` test helper and its message.
- S3 owns the continue edits to that shape (U1, U2), the tests outside `sys/uring.rs`, CI, the bench targets, and docs.
- U2 is stated against S1-response-r4 (reconciled): `IoBackend::parse` is `pub(crate)` inside `impl IoBackend`, with its test in `sys/linux.rs`; the root constructor calls `sys::os(b)`; `sys::os` on Linux is `linux::Linux::pick(b, uring::Uring::new)`, and on other OSes it holds the `Unsupported` text; the real.rs module doc names no OS backend; the construction enter is in P2.

Scratch evidence: `scratch-s3/` (`nt/` nextest, `mi/` Miri and ASan, `k.rs` errno → `ErrorKind`).

**Switch point** (Cf-4). Run these at U1's start:
```sh
rg -n 'pub enum IoBackend|pub fn from_env|pub fn with_backend\b|pub fn with_backend_and_lookup' crates/eml_io/src/real.rs   # expect 4 lines
rg -n 'pub\(crate\) fn pick' crates/eml_io/src/sys/linux.rs                                                              # expect 1 line
rg -n 'pub\(crate\) fn os' crates/eml_io/src/sys/mod.rs                                                                 # expect 1 line per OS cfg (Linux, Apple, Windows)
```
Read the results like this:
- **Expected counts (4, 1 and the `os` lines).** P2 has the shape. U1 and U2 are written against it.
- **No lines at all.** P2 predates the shape. U2 first introduces S1 r3's shape with the ruling's constructor set, then applies its edits.
- **Anything else.** P2 is half-done. Stop and ask S1's owner.

## 1. Conclusion

### 1.1 Selection (Q1): the final shape after U2

```rust
// crates/eml_io/src/real.rs (re-exported from lib.rs)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum IoBackend { #[default] Auto, Epoll, IoUring }       // U1 removes IoUringDirect
impl IoBackend {
    /// Linux だけが EML_IO_BACKEND を読む。ほかの OS は読まずに Auto (docs/spec/vm.md の「時計と driver」)
    pub fn from_env() -> io::Result<IoBackend>;              // Linux: IoBackend::parse(env::var_os("EML_IO_BACKEND").as_deref()); else Ok(Auto)
    pub(crate) fn parse(v: Option<&OsStr>) -> io::Result<IoBackend>;   // pure; tested from sys/linux.rs
}
impl RealDriver {
    pub fn with_backend_and_lookup(b: IoBackend, lookup: fn(&str, u16) -> io::Result<Vec<SocketAddr>>) -> io::Result<RealDriver>; // root: Ok(RealDriver(Driver::new(sys::os(b)?, lookup))); reads no environment
    pub fn new() -> io::Result<RealDriver>;                  // with_backend_and_lookup(IoBackend::from_env()?, sys::lookup)
    pub fn with_backend(b: IoBackend) -> io::Result<RealDriver>;   // with_backend_and_lookup(b, sys::lookup)
    pub fn with_lookup(lookup: ...) -> io::Result<RealDriver>;     // with_backend_and_lookup(IoBackend::from_env()?, lookup)
}
// crates/eml_io/src/sys/mod.rs
pub(crate) fn os(b: IoBackend) -> io::Result<Os>;   // Linux: linux::Linux::pick(b, uring::Uring::new); Apple/Windows: Auto => Os::new(), else Err(Unsupported, "this OS has one I/O backend")
// crates/eml_io/src/sys/linux.rs (S1's associated fn; U1 drops the Arm parameter of the closure)
impl Linux { pub(crate) fn pick(b: IoBackend, ring: impl FnOnce() -> io::Result<Uring>) -> io::Result<Linux> }
  Epoll   => Linux::Epoll(Unix::new()?)                                  // the ring is never tried
  IoUring => ring().map(Linux::Uring).map_err(pinned_error)
  Auto    => ring() → Ok: Uring; Err(e) if e.kind() ∈ {PermissionDenied, Unsupported}: Epoll; Err(e): Err(e)
```

**Constructors** (V-2). `with_backend_and_lookup` is the root. Every other constructor is a one-line delegate, and only the delegates call `from_env`.
- The root calls `sys::os(b)`. On Linux that is `Linux::pick(b, Uring::new)`, its only call site.
- On other OSes, `sys::os` builds `Os::new()` for `Auto`, and gives `Err(Unsupported, "this OS has one I/O backend")` for `Epoll` or `IoUring`.
- The existing callers of `with_lookup` (`eml_vm/tests/api.rs:935,958`) do not change.
- eml_vm and eml_cli do not change.

**`parse` values.**
- unset → `Auto`;
- `"epoll"` → `Epoll`;
- `"io_uring"` → `IoUring`;
- anything else → `InvalidInput`, message `EML_IO_BACKEND must be epoll or io_uring, not "bogus"`. This covers `""`, case variants, non-UTF-8 and `io_uring_direct`. The value is printed with `{:?}` of `to_string_lossy()`.

**Pinned error.** `pinned_error(e)` keeps `e.kind()`, with the text `EML_IO_BACKEND is io_uring, but io_uring cannot be used here: {e}`.

**Fallback classes** (`Auto` only). [V] `k.rs`:
- `PermissionDenied`: EPERM 1, EACCES 13.
- `Unsupported`: ENOSYS 38, and S1's own `Unsupported` (kernel below 6.1, missing feature).
- Everything else fails the driver: EINVAL 22 (`InvalidInput`), ENOMEM 12 (`OutOfMemory`), EMFILE 24 (`TooManyOpenFiles`) and the rest.

**Construction enter** (Cf-5). S1's P2 now ends `Uring::new` with the non-blocking enter. The gate, R0 and Close therefore all measure the same startup path. U2 adds no code for it and keeps a test of it (U2 item 3).

**Behaviour.**
- The choice is made each time a driver is built. Nothing is cached, and no separate probe ring is built.
- At run time a failure shows as ``runtime error: the I/O driver failed: <text> in `entry$main` `` with exit 1 [V `ulimit -n 4`]. The REPL then stops (`is_broken`).
- A seccomp filter in kill mode kills eml in `Auto`. The escape is `EML_IO_BACKEND=epoll`, and the spec states it.

**Spec** (Cf-7). U2 writes it, because U2 is the commit where `Auto` becomes io_uring. It is a new bullet in vm.md 「時計と driver」 after the structure bullet (vm.md:469). Points:
1. Linux has io_uring and epoll. epoll is the reference backend.
2. The backend is chosen each time a real driver is built. `IoBackend::from_env` reads `EML_IO_BACKEND`. `RealDriver::with_backend` and `with_backend_and_lookup` take the value directly.
3. Unset means io_uring. If the system refuses io_uring (permission, an unsupported call or feature), epoll is used. Any other error fails the driver.
4. `epoll` or `io_uring` fixes the backend, and a fixed backend that cannot be built fails with the reason. Any other value fails, the empty value included. Other OSes do not read the variable.
5. io_uring needs Linux 6.1+, S1's flags and features, `REGISTER_RING_FDS`, and one non-blocking enter.
6. Under a sandbox that kills on these calls, use `EML_IO_BACKEND=epoll`.
7. A failure to build ends the run with `VmFault::Driver`.
8. Programs cannot see which backend was chosen.

U2 also adds a pointer bullet to stdlib.md 「起動」.

### 1.2 Tests on both backends (Q2)

- **One backend per test process,** chosen by the product's variable (G-9). [V] nextest passes the variable through, and so does `nix develop -c`.
- **Profile.** In `.config/nextest.toml`:
  ```toml
  # 本物の driver を作るか `eml` を起こすテスト。Linux の2つの backend で流し直す (docs/implementation/testing.md の「Linux の2つの backend」)
  [profile.real-driver]
  default-filter = 'package(eml_io) | (package(eml_vm) & (test(/^api::/) | test(/^signal::/))) | (package(eml_cli) & (test(/^cli::/) | test(/^terminal::/) | test(/^tls::/)))'
  ```
  [V] `nt/`: `default-filter` works, `slow-timeout` is inherited, `-E` narrows the set, and `--message-format oneline` prints one line per test.
- **Coverage test.** `crates/eml_cli/tests/profiles.rs`, declared in `tests/main.rs`, test `the_real_driver_profile_covers_every_test_file_that_uses_the_real_driver`.
  - It scans `crates/*/tests/**/*.rs`. Every file containing one of the markers must be covered: either by `package(<crate>) |`, or by `package(<crate>) &` together with `test(/^<module>::/)`.
  - The markers are built at run time so that `profiles.rs` does not match itself (Cf-9): `const MARKERS: [[&str; 2]; 3] = [["Clock::", "Real"], ["Real", "Driver"], ["CARGO_BIN_EXE_", "eml"]];`, joined with `concat()`.
  - The one exemption is `crates/eml_cli/tests/ui.rs`. Its `proc_*` children get their environment from `CaptureHost` without the variable, so they always run `Auto`.
  - Completion of U3 checks that `profiles.rs` itself is not reported.
- **Ring tests never skip** (G-9). S1's `ring()` helper:
  - With the variable unset or set to `io_uring`, a ring that cannot be built panics with `io_uring cannot be used here ({e}); set EML_IO_BACKEND=epoll to run the tests on epoll`.
  - With `EML_IO_BACKEND=epoll`, it prints `EML_IO_BACKEND=epoll: ring tests do not run` and the test returns.
  - The Linux ordinary run therefore needs io_uring. testing.md gets no skip exception.
- **Abort test** (V-1). U1 adds `sys::uring::tests::dropping_the_ring_with_a_recv_in_flight_aborts`. It calls
  `crate::driver::tests::abort::in_child(concat!(module_path!(), "::dropping_the_ring_with_a_recv_in_flight_aborts"), URING_HELD_ABORT, || { … })`.
  In the child it gets the ring through `ring()`, starts a pending `Recv` on a loopback pair, and drops the `Uring`. The test adds no environment read.
  - `URING_HELD_ABORT` is the message `Uring::drop` writes before aborting.
  - `in_child` today asserts the core's `HELD_ABORT` (abort.rs:31). So `in_child` gains a parameter: `pub(crate) fn in_child(path: &str, message: &str, body: impl FnOnce())`. The existing callers pass `HELD_ABORT`, a kind-3 change with expected values byte-identical.
  - Visibility: `driver/mod.rs:10` becomes `#[cfg(test)] pub(crate) mod tests;` and `driver/tests/mod.rs:4` becomes `pub(crate) mod abort;`. These are test-only visibility edits and the only change to `driver/`.
  - `trust::abort_is_named_only_where_memory_is_at_stake` still holds. It counts `abort` identifiers, and `driver/tests/mod.rs` keeps one. `uring.rs` is in `COMPLETION_BACKENDS`. [V trust.rs:379-392]
  - The test is ignored under Miri. With `EML_IO_BACKEND=epoll` it returns, like the other ring tests.
  - `EML_IO_ABORT_CHILD` stays read only in `driver/tests/abort.rs`.
- **Selection tests.** These, together with the ring tests, are the only tests that name a backend (G-9): `parse_maps_each_value`, the `pick` tests and `new_follows_the_variable` (§5 U2).
- **1 s rule.** U1 and U2 time the real-driver set under each pinned value. CI's timing step times the ordinary run, which needs the ring.
- **Test changes.**
  - Kind 1: U1 deletes the losing arm's tests.
  - Kind 3: the `in_child` signature, and the renames when A = direct.
  - New tests are listed in §5.

### 1.3 CI (Q3)

**`test` job.** Two Linux steps after 「cargo nextest run without tls」:
```yaml
      # Linux の本物の driver は io_uring と epoll を持つ。本物の driver を使うテストを backend を固めて両方で流す。
      # io_uring に固めた実行は io_uring を作れなければ落ちる (docs/implementation/testing.md の「Linux の2つの backend」)
      - name: cargo nextest run --profile real-driver with io_uring
        if: ${{ runner.os == 'Linux' && !cancelled() && steps.test.outcome != 'skipped' }}
        run: |
          uname -r
          grep -H . /proc/sys/kernel/io_uring_disabled || true
          EML_IO_BACKEND=io_uring cargo nextest run --profile real-driver --failure-output immediate-final
      - name: cargo nextest run --profile real-driver with epoll
        if: ${{ runner.os == 'Linux' && !cancelled() && steps.test.outcome != 'skipped' }}
        run: EML_IO_BACKEND=epoll cargo nextest run --profile real-driver --failure-output immediate-final
```

**`asan` job.** Add a matrix field `backends`: `io_uring epoll` on both Linux legs, `unread` on macOS, with the comment 「macOS は EML_IO_BACKEND を読まないので、1回だけ流すための名前である」. Set job env `BACKENDS: ${{ matrix.backends }}`. Each step loops:
`for backend in $BACKENDS; do EML_IO_BACKEND=$backend nix develop .#miri -c cargo test -Zbuild-std --target ${{ matrix.target }} <args>; done`, where `<args>` is `-p eml_io` in the first step and `-p eml_vm --test integration -- api:: signal::` in the second.

**Proof that io_uring was used.** With the variable unset, the ordinary run fails on a runner that refuses io_uring (ring tests and `new_follows_the_variable`). The pinned step shows the same thing explicitly.

**A runner refuses io_uring → discard** (§1.8; G-8; user answer 2). Revisit condition (4) applies.

**Push.**
- First push right after the U3 commit: `git push -u origin ccr-f0300e7d-bir57l`.
- After every later commit: `git push origin ccr-f0300e7d-bir57l`.
- Check with U3 item 8.

Unchanged: Miri (`driver::tests::` only; the ring abort test lives in `sys::uring::tests` and does not run there), fmt, verus and licenses.

### 1.4 ASan and Miri for `uring.rs` (Q4)

- **ASan.** CI runs it on both Linux backends.
  - [V] `mi/`: under ASan the ring sets up and maps (`fd=5 features=0x3ffff head=0`).
  - ASan sees our own user-space accesses. It does not see the kernel's writes into lent buffers or into `Mem` [I].
  - That safety rests on the core's `Hold::Memory` rule (`driver::tests::abort`) and on U1's ring abort test (§1.2).
- **Miri.** Nothing in `uring.rs` runs under Miri. [V] `mi/`: `unsupported syscall number 425`.

### 1.5 Removing prototype-only code (Q5)

**U1** (Cf-7):
- Remove `Arm`, `IoBackend::IoUringDirect`, their branches and the arm loops.
- `Uring::new()` and `with_sizes(sq, cq)` lose their arm argument, and so does `ring()`. `Linux::pick`'s closure becomes `impl FnOnce() -> io::Result<Uring>`.
- `parse` drops `io_uring_direct` from its accepted values. Its message becomes `EML_IO_BACKEND must be epoll or io_uring, not "{v}"`.
- `Auto` still maps to epoll in U1.
- If A = try-first: delete tests 6 and 7 and the `n < cap` rule. If A = direct: rename tests 6 and 7 to `a_recv_on_a_row_not_known_readable_skips_the_sync_attempt` and `a_short_sync_recv_clears_ready`.

**U2.** Changes `pick`'s `Auto` arm and the pinned error (§1.1), adds the tests, and writes the spec.

**Comments.** Remove prototype and measurement wording. Comments cite vm.md 「時計と driver」.

**`trust.rs`** stays as P2 left it: `FILES` += `sys/linux.rs`, `sys/uring.rs`; `UNSAFE_FILES` += `sys/uring.rs`; `COMPLETION_BACKENDS = ["sys/iocp.rs", "sys/uring.rs"]`.

**bench.**
- `--env` and its sentence in benchmarks.md belong to M1 (Cf-3).
- The `io_uring_enter` column belongs to M2.
- U4 adds `check.py` and the targets.

### 1.6 Docs (Q6)

Write all Japanese with natural-japanese. No stage names or dates. No 「今回」 or 「試作」 outside the benchmarks records. Do not call epoll legacy or optional (G-1).

| File | Unit | Change |
|---|---|---|
| vm.md 「時計と driver」 | U1 | The structure bullet (469) gets io_uring as a second Linux backend. The io_uring description:<ul><li>a completion backend, with A's sync attempts (try-first: as epoll; direct: `recv`/`accept4` only when the row's mark says readable, and a short read clears the mark);</li><li>`Readable` is a `MSG_PEEK` when the row is marked, otherwise a one-shot poll;</li><li>`Read`/`Write`/`Accept`/`Connect` go to the ring with the lent buffer, or with the op table's address memory;</li><li>the waker's eventfd is watched by a one-shot poll, re-armed after each wake;</li><li>the timeout is exact (`EXT_ARG`);</li><li>cancel goes through `ASYNC_CANCEL`;</li><li>`user_data` is the table index plus a generation advanced on each reuse;</li><li>no multishot, no provided buffers, no zero-copy send.</li></ul>The drop bullet (473): io_uring makes memory-holding ops, and closing the ring cancels the remaining polls. No "default" wording yet. |
| vm.md 「上限と割り込み」 | U1 | An interrupted `io_uring_enter` returns like the epoll wait. |
| testing.md 「`unsafe` の一覧」 | U1 | `COMPLETION_BACKENDS` is `sys/iocp.rs` and `sys/uring.rs`. Reword the three tests' descriptions to match. |
| vm.md (selection, §1.1 points; the structure bullet becomes 「Linux は io_uring (使えれば既定) か epoll (基準の backend)」), stdlib.md 「起動」 | U2 | §1.1. |
| testing.md, new `### Linux の2つの backend` under 「`eml_io` の検証」 | U3 | Include:<ul><li>a Linux run without the variable needs io_uring, and a ring test that cannot build its ring fails with a message that names `EML_IO_BACKEND=epoll`;</li><li>with `epoll` pinned, the ring tests do not run;</li><li>the profile: what it covers and why;</li><li>the coverage test and its ui.rs exemption;</li><li>CI's steps, and that the ordinary run already proves the default;</li><li>io_uring does not stay the default without a Linux CI leg that builds it;</li><li>no exception in 「Windows で流さないテスト」.</li></ul> |
| testing.md, other sections | U3 | 「AddressSanitizer」: each Linux backend; kernel writes are invisible to ASan. 「核の Miri」: the backends are not run under Miri. 「CI」: the new steps. 「よく使うコマンド」: the two profile commands, and the `bench/net.sh` line (679) gets `[--env LABEL KEY=VALUE]`. |
| testing.md 「nix build の中で流さないテスト」 | U3, only if `nix build` fails on ring tests | The flake check phase sets `EML_IO_BACKEND=epoll`, with the reason (§6.1). |
| CLAUDE.md | U3 | Command lines: `EML_IO_BACKEND=epoll cargo nextest run --profile real-driver   # the real-driver tests on one Linux backend (epoll or io_uring; unset requires io_uring where it is not refused)`; the ASan line and the `bench/net.sh` line get the variable. Architecture `eml_io`: "io_uring (default where buildable) or epoll, kqueue, IOCP". Testing bullet: "Linux tests need io_uring unless `EML_IO_BACKEND=epoll`; CI also runs the real-driver tests pinned to each Linux backend". |
| benchmarks.md 「driver の syscall の目標」 | U4 (structure), Close (「今」) | An epoll table (the reference, listed first) and an io_uring table, each measured with its backend pinned. |
| benchmarks.md | Close | `### io_uring と epoll` in S2's layout, from the Close numbers. Its rule sentence names the five gate cases, and its last bullet cites vm.md. Older recorded `bench.py` commands get `--env <label> EML_IO_BACKEND=epoll` on every non-`main`/BASE label: lines 432, 505, 523, 524 and any other hit of the G-11 grep. This happens in the continue branch only (Cf-3, amended G-11); older binaries ignore the variable [I]. The intro's `--env` sentence is M1's and is not touched here. Add S2's regime bullet to 「比べる規則」. |
| async.md | Close | Row 10 (28): 「Linux は io_uring (使えなければ epoll)」. Line 243: drop 「io_uring は使わない」; say io_uring is written by hand over `libc::syscall` without a crate, with the reason, and add: 変数を読むのは `eml_io` の `IoBackend::from_env` だけで、`RealDriver::with_backend` と `with_backend_and_lookup` が値を直接受ける. 「ファイルとパイプ」: F1 §5.3's bullet. Line 392: `SINGLE_ISSUER\|DEFER_TASKRUN`, plus a pointer to roadmap (d). |
| architecture.md 「`eml_io` の内部」 | Close | The four constructors and the root. `IoBackend` sits in `real.rs` as the public surface (amended G-5). `sys/linux.rs` holds `enum Linux` and `pick`. `sys/uring.rs`: the ring, op table and `user_data`, `Mem`, the teardown order, `ring` as the first field. The `unix.rs` and `epoll.rs` shared helpers and `EventFd`. The `unsafe` list gains `sys/uring.rs`. In 「外部 crate」, the `libc` row says the io_uring syscalls go through `libc::syscall`, with the ABI defined in `uring.rs`. |
| architecture.md 「CLI と lib API」 | Close, only if eml_io's public items are listed there | `IoBackend` and the constructors. |
| status.md | Close | 150: Linux uses io_uring, or epoll below 6.1 or where io_uring is refused. 152: one sentence on io_uring. |
| docs/README.md | Close | Add 「Linux の2つの backend」 to the testing.md row. |
| README.md | Close | Line 127: `(io_uring と epoll、kqueue、IOCP)`. |
| roadmap.md | Close | Delete the `io-uring` entry; BASE left line 31 as it was (Cf-6). Add the `async-files` 論点 (F1 §5.2). Under 「その後の項目」 → 「### 非同期」 add (a) multishot ops and provided buffers, (b) CI on 6.1–6.16 kernels, (c) one item per kept regression, (d) cross-thread submission on io_uring. Each item gets 後に回した理由 and 見直す条件, as in F1 §5.2 and r3. |

### 1.7 Regressions (Q7)

- **One default for all of Linux.** It never depends on the workload (G-6).
- **What counts as a regression,** read from 「epoll → uring」 with `base` included:
  1. CPU: 判定 「差」 and 比 ≥ 1.100.
  2. Startup: the `base` row has 判定 「差」 and 生の CPU の比 ≥ 1.05.
  3. Memory: uring's median max RSS is at least max(1 MiB, 5%) above epoll's.

  Wall time is recorded only.
- **Handling.**
  - **Fixed:** in U5, then that case is rerun.
  - **Kept:** only with a measured mechanism from the same session (a syscall count or `RunStats` counter per unit that differs between the two backends) and only if the regression is not blocking. A kept regression gets a bullet in benchmarks.md and a roadmap item.
  - **Blocking:** CPU 比 ≥ 1.250, or startup 比 ≥ 1.10, or more kept regressions than passing gate cases. A blocking regression is fixed or the stage is discarded. There is no epoll-default option.
- **When the rule runs.**
  - **R0:** on M3's full session, before the decision commit (Cf-8). M3's binary already includes the construction enter (Cf-5).
  - **Close:** on the final binary. A Close failure means fix and retry, then discard (Cf-1).

### 1.8 Discard after M3

**Triggers.** Any one of these:
- R0 finds a blocking row with no fix;
- the Close gate fails and fixing has not made it pass;
- a blocking regression cannot be fixed;
- the CI pinned `io_uring` step, or the Linux ordinary run, fails because the runner refuses io_uring.

**Steps.**
1. Make no Close commit.
2. Revert, newest first, with `git revert --no-edit`: U4, U3, U2, every U5, U1.
3. Then apply S2's D1 (M2, P2, P1, M1) and D2. The `Record the io-uring decision` commit stays; it is plan-only, and the plan is deleted at D2.
4. D2 records:
   - the prototype gate, and the final gate or the CI failure where those happened;
   - the roadmap item, with F1 §5.2's discard text and 見直す条件 (1)–(4);
   - in benchmarks.md, the `--env` text and pins only inside `### io_uring と epoll` (amended G-11, discard check).

**Completion** (Cf-10: one path list, shared with S2 D1).
1. `git diff $BASE -- crates bench .config .github flake.nix Cargo.toml Cargo.lock THIRD-PARTY-LICENSES.md docs/spec README.md CLAUDE.md docs/implementation/testing.md` is empty.
2. Amended G-11 discard check:
   - `rg -l -- '--env|EML_IO_BACKEND' bench docs` prints exactly `docs/implementation/benchmarks.md` and `docs/future/roadmap.md`;
   - in benchmarks.md, every hit is inside `### io_uring と epoll`;
   - `rg -n -- '--env' bench` prints nothing.
3. S2 D1's and D2's completion criteria hold.
4. `git push origin ccr-f0300e7d-bir57l`, and CI is green.

## 2. Rationale

- **The root constructor and delegates** (V-2). Every configuration, a custom lookup included, can be reached without the environment, and the env read stays in one public function.
- **Only "refused" falls back.** Our own setup bugs and resource errors stay loud. The enter in P2 makes the probe exercise all three syscalls, on the binary the gate measures.
- **Ring tests fail rather than skip.** Pass/fail stays independent of the machine's settings (testing.md:504), and the opt-out is one variable.
- **`in_child` is reused instead of a second env read.** One reader of `EML_IO_ABORT_CHILD`, and the same parent/child logic for both abort tests.
- **R0 runs before the decision commit,** and the default flips in U2. So the decision, the spec sentence and the flip are each made once, with the evidence already in hand.

## 3. Alternatives considered and rejected

- A per-process cache, or a separate probe ring.
- Letting an unknown or pinned value fall back.
- Falling back on any error.
- Treating an empty value as unset.
- Pinning on every OS.
- `RunConfig` or `Clock` carrying the backend.
- Iterating backends inside one process.
- Skipping ring tests on refusal.
- Observability through stats or stderr.
- An old-kernel CI leg (moved to roadmap (b)).
- A heap-backed fake ring under Miri.
- Running the whole suite twice.
- "epoll default with opt-in io_uring".
- Moving the ring abort test under a `tests/` directory: the ruling rejects bending the layout to satisfy a grep.
- Narrowing the env grep to `EML_IO_BACKEND`: the ring helper's message names the variable.

## 4. Assumptions made

- M3 records the gate as passed for arm A, and R0 finds nothing blocking (otherwise §1.8).
- P2 follows S1 r4: the ruling's constructor set, `sys::os`, `Linux::pick` with a ring constructor, and the construction enter (Cf-4, Cf-5).
- Kernels 6.1–6.16 are untested [I].
- GitHub's Ubuntu runners have no seccomp filter on steps [I]. If they refuse io_uring, §1.8 applies.
- `.#miri`'s shell passes the environment through [I].
- check.py does not depend on label names. It classifies by `env`, and labels appear only in its printed lines [V by reading `scratch-s2/check.py`; S2 verified the label handling, Cf-11].

## 5. Implications for implementation

**Constraints.**
- No change to eml_vm or eml_cli `src/`.
- The only change in `crates/eml_io/src/driver/` is the test-only visibility of `tests` and `abort` plus `in_child`'s parameter.
- Comments are Japanese, explain why, and cite vm.md.
- Every command runs inside `nix develop` from `/home/user/eml`.
- `REAL` is the profile's filter text (§1.2). It is used before U3 adds the profile.

**Std checks** (all exit 0):
1. `cargo clippy --all-targets -- -D warnings`
2. `cargo clippy -p eml_io -p eml_vm --all-targets --no-default-features -- -D warnings`
3. `cargo fmt --check`
4. `nix develop .#cross -c cargo clippy --target T --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run -- -D warnings`, for T in `x86_64-pc-windows-msvc` and `aarch64-unknown-linux-gnu`
5. `cargo nextest run -p eml_io --lib -E 'test(trust::)'`
6. `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_io --lib -- driver::tests::`

### R0, then `Record the io-uring decision` (plan-only commit; Cf-8)

1. M3 has committed 「門: 通った <腕>」. If it recorded 「門: 通らない」, M3 also recorded 捨てる and went to D1, and nothing here applies.
2. Apply §1.7 to M3's `full.md` 「epoll → <A's label>」 and its per-label RSS table.
3. In the plan's 「計測の結果」, record each regression row as blocking, to fix, or to keep (with its mechanism).
4. Write 「判断: 続ける (<腕>)」, or 「判断: 捨てる (<理由>)」 when a blocking row has no plausible fix.
5. Commit only `docs/plans/` as `Record the io-uring decision`.
6. If the decision is 捨てる, go to S2's D1 and D2.

Completion: `git show --stat HEAD` lists only files under `docs/plans/`, and the plan contains one 「判断:」 line.

### U1 `Keep the <A> io_uring backend`

**Change.** §1.5 U1, the ring abort test and the `in_child` change (§1.2), vm.md (no default wording), testing.md 「`unsafe` の一覧」.

**Plan entries.**
- Kind 1: the deleted tests.
- Kind 3: the renames (A = direct) and the `in_child(path, message, body)` callers.

**Completion.**
1. `grep -rnE 'io_uring_direct|IoUringDirect|\bArm\b|TryFirst|Arm::Direct' crates/eml_io/src` prints nothing.
2. `EML_IO_BACKEND=io_uring cargo nextest run` → exit 0, `0 failed`, no `SLOW`.
3. `EML_IO_BACKEND=epoll cargo nextest run -E "$REAL"` → exit 0.
4. For V in `io_uring` and `epoll`: `EML_IO_BACKEND=$V cargo nextest run --profile timing -j 1 -E "$REAL"` → exit 0.
5. `cargo nextest run -p eml_io --lib -E 'test(=sys::uring::tests::dropping_the_ring_with_a_recv_in_flight_aborts) | test(/^driver::tests::abort::/)'` → all pass, with the variable unset.
6. Environment reads:
   - `rg -n 'EML_IO_ABORT_CHILD' crates/eml_io/src` prints only `driver/tests/abort.rs` lines;
   - `rg -n 'var_os|env::var' crates/eml_io/src -g '!**/tests/**'` prints exactly one line, in `real.rs`.
7. `cargo build --release -p eml_cli`, then `EML_IO_BACKEND=io_uring_direct timeout 20 target/release/eml run bench/net/base.em` → exit ≠ 0, and stderr contains `must be epoll or io_uring`.
8. `EML_IO_BACKEND=io_uring timeout 60 strace -f -c -o /tmp/u.txt target/release/eml run bench/net/many.em 1000` → stdout `many done 1000`. `/tmp/u.txt` shows `io_uring_setup` 1 and `io_uring_register` 1, and has no `epoll_create1` row.
9. G-3: `rg -n 'MULTISHOT|MULTI\b|BUFFER_SELECT|SEND_ZC|PROVIDE_BUFFERS|PBUF_RING|RECVSEND_BUNDLE' crates/eml_io/src` prints nothing.
10. Std checks.

### U5… (only for R0's "to fix" rows) `Fix <case> on io_uring`

One commit per cause.

Completion:
- `python3 -I bench/net/bench.py --only '^(base|<case>)$' --runs 11 --strace-runs 3 epoll=target/release/eml uring=target/release/eml --env epoll EML_IO_BACKEND=epoll --env uring EML_IO_BACKEND=io_uring > /tmp/f.jsonl && python3 -I bench/net/report.py /tmp/f.jsonl` shows the row no longer meets any §1.7 criterion.
- U1 items 2, 3 and 10 still hold.

### U2 `Make io_uring the Linux default`

**Edits.** These are against S1 r3's `real.rs` / `sys/linux.rs`, as amended by the ruling's constructors (§1.1).
- `Linux::pick`'s `Auto` arm: from `Linux::Epoll(Unix::new()?)` to "try `ring()`, fall back on the two classes".
- `IoUring` arm: wrap the error with `pinned_error`.
- `sys::os` off Linux: check that the `Unsupported` text is exactly `this OS has one I/O backend`, and fix it if not.
- vm.md and stdlib.md (§1.1 spec).

**Tests first** (all added; Linux unless marked):
- `sys/linux.rs` `parse_maps_each_value` (pure; calls `IoBackend::parse`; S1 r4 already moves it here, so extend it rather than add a second one):
  - `None` → `Auto`; `"epoll"` → `Epoll`; `"io_uring"` → `IoUring`;
  - `""`, `"EPOLL"`, `"io_uring_direct"`, `"kqueue"` and `OsStr::from_bytes(b"\xff")` → `InvalidInput`;
  - the exact text for `"bogus"`.
- `sys/linux.rs`:
  - `auto_falls_back_when_the_system_refuses_the_ring`: `Linux::pick(IoBackend::Auto, || Err(io::Error::from_raw_os_error(libc::EPERM)))` → `Linux::Epoll(_)`. The same for `ENOSYS`, and for `io::Error::new(Unsupported, "io_uring needs Linux 6.1")`.
  - `auto_reports_other_ring_errors`: `ENOMEM` → `Err` of kind `OutOfMemory`.
  - `a_pinned_epoll_never_tries_the_ring`: `Linux::pick(IoBackend::Epoll, || panic!())` → `Epoll`.
  - `a_pinned_ring_reports_why_it_cannot_be_built`: kind `Unsupported` and the exact `pinned_error` text.
  - `new_follows_the_variable`: `IoBackend::from_env()?` then `Linux::pick(b, Uring::new)`. Expected: `epoll` → `Epoll`; `io_uring` → `Uring`; unset → `Uring`. On failure, print the opt-out message.
- `sys/mod.rs` `a_second_backend_is_refused_off_linux` (`#[cfg(not(target_os = "linux"))]`): `os(IoBackend::Epoll)` and `os(IoBackend::IoUring)` → `Unsupported`. It lives in `sys/` so that `real.rs` names no backend outside `impl IoBackend` (amended G-5). It runs in CI on macOS and Windows.
- `crates/eml_cli/tests/cli.rs` `an_unknown_io_backend_fails_the_run` (`#[cfg(target_os = "linux")]`): `eml run tests/ui/run/basics/hello.em` with `.env("EML_IO_BACKEND", "bogus")`. Expect exit code 1, empty stdout, and stderr exactly ``runtime error: the I/O driver failed: EML_IO_BACKEND must be epoll or io_uring, not "bogus" in `entry$main` `` followed by a newline.

**Completion.**
1. `cargo nextest run` → exit 0, `0 failed`, no `SLOW`.
2. For V in `io_uring` and `epoll`:
   - `EML_IO_BACKEND=$V cargo nextest run -E "$REAL" --success-output immediate 2>&1 | tee /tmp/r-$V.txt` → exit 0 (Cf-2);
   - `grep -c 'ring tests do not run' /tmp/r-$V.txt` → ≥ 1 for `epoll`, `0` for `io_uring`;
   - `EML_IO_BACKEND=$V cargo nextest run --profile timing -j 1 -E "$REAL"` → exit 0.
3. Construction enter test (Cf-5; P2 owns the code). `cargo build --release -p eml_cli`, then `timeout 20 strace -f -c -o /tmp/a.txt target/release/eml run tests/ui/run/basics/hello.em` with the variable unset. Expect stdout `Hello, world!` and, in `/tmp/a.txt`, `io_uring_setup` 1, `io_uring_enter` ≥ 1 (hello.em submits no op [I]) and no `epoll_create1`. With `EML_IO_BACKEND=epoll`: `epoll_create1` 1 and no `io_uring_setup`.
4. `EML_IO_BACKEND=bogus timeout 20 target/release/eml run bench/net/base.em; echo $?` → the stderr line above, then `1`.
5. Selection greps (G-6, G-7 as ruled, G-9):
   - `rg -n 'var_os|env::var' crates/eml_io/src -g '!**/tests/**'` prints exactly one line, in `real.rs`.
   - `rg -n 'pub fn from_env|pub fn with_backend|pub fn with_backend_and_lookup' crates/eml_io/src/real.rs` prints three lines (`with_backend` also matches the root; the expected lines are the three definitions).
   - `rg -n 'from_env' crates/eml_io/src/real.rs` prints exactly three lines: the definition and the calls in `new` and `with_lookup`. None is inside `with_backend_and_lookup`.
   - `rg -n 'IoBackend::(Epoll|IoUring)' crates --glob '*tests*' --glob '!crates/eml_io/src/sys/**'` prints nothing.
   - `rg -n 'skipped' crates/eml_io/src/sys` prints nothing.
6. Amended G-5:
   - `rg -n 'io_uring|epoll|kqueue|iocp|IoBackend' crates/eml_io/src/driver | rg -v '^\S+:\d+:\s*//'` prints nothing.
   - `rg -n -i 'epoll|io_uring|kqueue|iocp' crates/eml_io/src/real.rs` prints only lines inside `enum IoBackend`, inside `impl IoBackend`, or `///` doc lines. Check the line numbers against those spans.
7. Std checks.
8. `grep -c EML_IO_BACKEND docs/spec/vm.md docs/spec/stdlib.md` reports ≥ 1 for each file.

### U3 `Run the real-driver tests on both Linux backends`

**Change.** The profile and `profiles.rs` (§1.2), `ci.yml` (§1.3), testing.md and CLAUDE.md (§1.6), and `flake.nix` plus the testing.md nix section only if needed (§6.1).

**Completion.**
1. For V in `io_uring` and `epoll`: `EML_IO_BACKEND=$V cargo nextest run --profile real-driver` → exit 0. The number of tests run equals `cargo nextest list --profile real-driver --message-format oneline | wc -l`.
2. `EML_IO_BACKEND=epoll cargo nextest run --profile real-driver --success-output immediate 2>&1 | grep -c 'ring tests do not run'` → ≥ 1 (Cf-2).
3. `cargo nextest list --profile real-driver --message-format oneline | grep -cE ' (ui::|repl::)'` → `0`.
4. `cargo nextest run -p eml_cli -E 'test(=profiles::the_real_driver_profile_covers_every_test_file_that_uses_the_real_driver)'` → 1 passed. This also shows `profiles.rs` is not reported (Cf-9). A scratch removal of `| test(/^tls::/)` makes the test fail.
5. `nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- .github/workflows/ci.yml` → exit 0, no output.
6. For V in `io_uring` and `epoll`:
   - `nix develop .#miri -c env EML_IO_BACKEND=$V RUSTFLAGS=-Zsanitizer=address CARGO_TARGET_DIR=target/asan cargo test -Zbuild-std --target x86_64-unknown-linux-gnu -p eml_io` → exit 0;
   - the same with `-p eml_vm --test integration -- api:: signal::` → exit 0.
7. `nix build` → exit 0. If it fails only on ring tests, add the check-phase variable (§6.1) and rerun.
8. CI:
   ```sh
   git push -u origin ccr-f0300e7d-bir57l
   id=$(gh run list --branch ccr-f0300e7d-bir57l --limit 1 --json databaseId --jq '.[0].databaseId')
   gh run watch "$id" --exit-status                                            # exit 0
   gh run view "$id" --json jobs --jq '.jobs[] | .name + " " + .conclusion'     # every line ends in success
   ```
   Every job must succeed, including `test (ubuntu-24.04)`, `test (ubuntu-24.04-arm)`, `asan (x86_64-unknown-linux-gnu)` and `asan (aarch64-unknown-linux-gnu)`.
   - If the runner refuses io_uring (the log shows the opt-out message, or `io_uring_disabled` is not `0`), go to §1.8.
   - For any other failure, fix it and push again.

### U4 `Give each Linux backend its checks and syscall targets in bench/net`

**`bench/net/check.py`** (Cf-11). Copy it verbatim from S2 r3 §5 M3, whose source file is `scratch-s2/check.py`. The plan carries the text, and S2 r4's copy wins if it differs. Prepend the Japanese header comment 「各ラベルが名前どおりの backend で動いたかと、echo-py の切り替えの形を確かめる (docs/implementation/benchmarks.md の「io_uring と epoll」)」.
- It classifies labels by `env`, not by name. A label without `--env` counts as epoll; only `main` (BASE) runs that way.
- From U4 on, M3-style sessions run `bench/net/check.py` instead of `$S/check.py`.

**`report.py`.**
- Add `"epoll_create1"`, `"io_uring_setup"` and `"io_uring_register"` to `SYS`.
- Choose the target table per label from `env["EML_IO_BACKEND"]`:
  - `epoll` → `TARGETS`;
  - `io_uring` → `URING_TARGETS`;
  - no value → print 「目標の表は backend を固めたラベルにだけ出す」.

**`URING_TARGETS`.** Unchanged from r3:

| Class | Rows |
|---|---|
| Structural | readiness calls = 0; `io_uring_setup` = 1 and `io_uring_register` = 1; connect checks ≤ 0.005 per connection |
| Same as epoll | write 1 ≤ 1.05; one round trip ≤ 4; `accept4` ≤ 1.05 per connection |
| Per arm [I] | direct: E ÷ round trip ≤ 0.05; try-first: ≤ 2.05 |

Only the [I] rows may be reset from measurement, and the reset is recorded in the plan.

**benchmarks.md.** Two tables, the epoll table first. 「今」 is `—` until Close.

**Completion.**
```sh
C='^(base|bulk-send-py|bulk-recv-py|echo-py-reuse|echo-py-fresh|echo-self-fresh|conn-self-32|many-self-250|many-self-2000)$'
python3 -I bench/net/bench.py --only "$C" --runs 3 --strace-runs 3 epoll=target/release/eml uring=target/release/eml --env epoll EML_IO_BACKEND=epoll --env uring EML_IO_BACKEND=io_uring > /tmp/t.jsonl
python3 -I bench/net/report.py /tmp/t.jsonl > /tmp/t.md; python3 -I bench/net/check.py /tmp/t.jsonl; echo $?
```
1. `## 目標: epoll` shows 13 rows and `## 目標: uring` shows A's 7 rows. All pass.
2. check prints only `backend … ok` lines. It exits 0, or non-zero only from `regime … uring … NG` lines.
3. A label with no `--env` prints the no-target line.
4. Push, and check CI as in U3 item 8.

### Close `Close io-uring`

**Preconditions.**
- U1–U4 (and every U5) are committed and pushed.
- CI is green on HEAD.
- `git status --short` is empty.

**1. Gate rerun on the final binary.** Use the same machine (`uname -r` = `6.18.44-fc-v185`). Run the idle checks before each session:
- `pgrep -af 'cargo|rustc|nextest|bench.py'` shows only this procedure;
- the first field of `/proc/loadavg` is below 0.5.

```sh
S=$(mktemp -d); BASE=<the plan commit>
mkdir "$S/base" && git archive "$BASE" | tar -x -C "$S/base"
cargo build --release -p eml_cli --manifest-path "$S/base/Cargo.toml" && cp "$S/base/target/release/eml" "$S/eml-main"
cargo build --release -p eml_cli && cp target/release/eml "$S/eml-final"
G='^(base|many-self-1000|echo-self-fresh|echo-py-fresh|echo-py-reuse|bulk-1k)$'
ARMS="epoll=$S/eml-final uring=$S/eml-final --env epoll EML_IO_BACKEND=epoll --env uring EML_IO_BACKEND=io_uring"
python3 -I bench/net/bench.py --only "$G" --runs 11 --strace-runs 3 main=$S/eml-main epoll=$S/eml-final --env epoll EML_IO_BACKEND=epoll > $S/aa.jsonl
python3 -I bench/net/bench.py --only "$G" --runs 11 --strace-runs 3 $ARMS > $S/gate1.jsonl
python3 -I bench/net/bench.py --only '^(?!busy-self$)' --runs 11 --stats $ARMS main=$S/eml-main > $S/full.jsonl
python3 -I bench/net/bench.py --only "$G" --runs 11 --strace-runs 3 $ARMS > $S/gate2.jsonl
for f in aa gate1 full gate2; do python3 -I bench/net/report.py $S/$f.jsonl > $S/$f.md; python3 -I bench/net/check.py $S/$f.jsonl > $S/$f.check; echo "$f $?"; done
```

**Third session.** Only when a case passed in exactly one of the two gate sessions: `python3 -I bench/net/bench.py --only '^(base|<case>)$' --runs 11 --strace-runs 3 $ARMS > $S/gate3.jsonl`.

**Expected results** (S2's rules):
- `wc -l`: aa 24, gate1 24, gate2 24, full 183 [I].
- check:
  - a `backend` NG voids the session: fix, then rerun;
  - an epoll `regime` NG voids the session: rerun at most twice, then report;
  - a uring `regime` NG voids only uring's echo-py rows, and the non-zero exit it causes is expected.
- aa: all 5 gate rows show 差なし.
- Gate: a single case has 比 ≤ 0.800 and 判定 「差」 in 「epoll → uring」 in both gate1 and gate2. gate3 decides a split. echo-py rows count only where uring's regime is `ok`.
- full:
  - the §1.7 rule holds;
  - `epoll → main` has no row with 判定 「差」 and 比 ≤ 0.909;
  - `## 目標: epoll` and `## 目標: uring` pass every row.

If any of these fails: fix in U5 commits and rerun this step. If it cannot be fixed, go to §1.8 (Cf-1).

**2. Docs.** Make the Close rows of §1.6, using these sessions' numbers. The prototype numbers go only into the commit message.

**3. 「今の数値」.** The macOS measurement is waived (user answer 4). The commit message records the waiver and the reasons:
- `bench/run.sh`, `rt.sh` and `scripts.sh` never reference `bench/net` [V grep];
- macOS behaviour is unchanged: the variable is Linux-only, P1 is visibility-only, and eml_vm, eml_rt, eml_lir, eml_core_ir and std/ are untouched.

**4.** Delete the stage's `docs/plans/` files.

**Completion.**
1. `git diff HEAD~1 --stat -- crates bench .config .github flake.nix` is empty.
2. `cargo nextest run` → exit 0 (includes `citations::` and `profiles::`).
3. `grep -rn 'io_uring_direct' crates bench docs CLAUDE.md README.md` prints nothing.
4. `grep -n 'io-uring' docs/future/roadmap.md` prints nothing. `grep -c '多重の操作\|スレッドをまたぐ受け付け' docs/future/roadmap.md` → ≥ 2, and the `async-files` entry contains `READ`.
5. `grep -n 'io_uring は使わない' docs/future/async.md` prints nothing.
6. `for f in docs/spec/vm.md docs/spec/stdlib.md docs/implementation/testing.md docs/implementation/benchmarks.md CLAUDE.md; do grep -q EML_IO_BACKEND $f || echo MISSING $f; done; grep -l EML_IO_BACKEND docs/future/roadmap.md` prints nothing.
7. `ls docs/plans` lists none of this stage's files.
8. `git diff HEAD~1 -U0 -- docs/spec docs/implementation/architecture.md docs/implementation/testing.md docs/implementation/status.md docs/future | grep -E '^\+.*(試作|今回)'` prints nothing.
9. G-1: `rg -n 'legacy|古い backend' docs/spec docs/implementation` prints nothing about epoll.
10. G-11 (continue): `rg -n 'bench.py' docs | rg -v -- '--env' | rg -v 'main=|BASE|<BASE>'` prints nothing.
11. G-4:
    - `cargo tree -p eml_io -e normal --prefix none | sort -u` lists only F1 G-4's allow-list;
    - `rg -n '^(io-uring|rustix|polling|mio)' crates/eml_io/Cargo.toml` prints nothing.
12. G-2: with the variable unset, `cargo nextest run -p eml_io -E 'test(=sys::linux::tests::new_follows_the_variable)'` → 1 passed.
13. The amended G-5 and the G-6/G-7 greps (U2 items 5 and 6) still hold.
14. `git push origin ccr-f0300e7d-bir57l`, and CI is green.
15. The commit message lists:
    - what moved where;
    - the test-change kinds;
    - the final and the prototype gate rows;
    - the regressions and how each was handled;
    - the machine;
    - the decision dates;
    - the macOS waiver.

## 6. Unresolved items (with defaults)

1. **The nix build sandbox.** [V] A sandboxed `nix build` here ran `io_uring_setup` successfully (`sandbox = true`, `filter-syscalls = true`; caveat `sandbox-fallback = true` [I]).
   - Default: no flake change; U3 item 7 confirms it.
   - Otherwise: set `preCheck = "export EML_IO_BACKEND=epoll";` next to `checkFlags` (flake.nix:112), and record it in testing.md 「nix build の中で流さないテスト」.
2. **Kernels 6.1–6.16.** Roadmap item (b).
3. **architecture.md 「CLI と lib API」.** Add `IoBackend` only where `RealDriver` is listed.
4. **S1 r4.** Reconciled (`IoBackend::parse`, `sys::os`, the module doc, the enter in P2). If S1 changes again, re-check U2's edit list only.

## 7. Additional checks

- [V] `driver/tests/abort.rs`: `in_child(path, body)` is private, reads `EML_IO_ABORT_CHILD` at line 19, and asserts `HELD_ABORT` at line 31. `driver/mod.rs:10` has `mod tests;`, and `driver/tests/mod.rs:4` has `mod abort;`.
- [V] `trust.rs:379-392`: `abort_is_named_only_where_memory_is_at_stake` counts identifiers. `driver/tests/mod.rs` may have at most 1, and `COMPLETION_BACKENDS` files are free.
- [V] `scratch-s2/check.py` exists. Its `uring` predicate reads `env`, and labels are used only for grouping and printing.
- [V] Carried over from r3: the nextest probes, the env passthrough, Miri rejecting syscall 425, ASan setting up a ring, errno → `ErrorKind`, the driver error line, the grep of real-driver files, the bench scripts not referencing `bench/net`, the nix sandbox probe.
- No background tasks were started.

## 8. Challenge

- **The ring abort test needs its own message.** S1 r4 plans no ring abort test, so U1's test (§1.2) is the only one. The ruling's `in_child` call cannot work unchanged. `in_child` asserts that the child's stderr contains the core's `HELD_ABORT` (abort.rs:31), but `Uring::drop` aborts with its own message. Hence the added `message` parameter. It is a kind-3 change to the existing callers and adds no env read. If S1 r4 makes `Uring::drop` write `HELD_ABORT` verbatim instead, drop the parameter.
- **Older recorded commands (G-11, continue).** Pinning them edits commands that recorded older binaries. This is harmless (those binaries do not read the variable [I]) and conformed.

