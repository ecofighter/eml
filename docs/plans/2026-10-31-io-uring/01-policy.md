> 設計の段階の付録 (英語)。優先の順は、本文 (`../2026-10-31-io-uring.md`) > `05-rulings.md` > この付録である。`scratch/…` や `scratch-s*/…` の名前は設計のときの作業用のファイルで、リポジトリにはない。本文に要るものは本文か `06-harness.patch` に写してある。

# F1 (r2): the long-term policy for eml's Linux I/O backends (io_uring vs epoll)

Tags: [V] verified (how is given), [I] inferred. Changes from r1 are driven by the review table at the end. The main ones: ring tests no longer skip on machine settings (C-4); G-4 gets a scope and a revisit condition and the rustix fact is corrected (C-1); G-5 no longer freezes the trait (A-1, C-2); G-8 is labelled a Close-time check (B-2); the two policy preferences that remove a choice S3 gave the user are listed under Unresolved (C-3, B-2); every "How to check" was run or is marked for when its file exists (B-1, B-3, B-4, B-5, A-3).

Sources read: X-integration-r1, S1-r2, S2-r2, S3-r2, both dossiers, F1-review-r1, roadmap.md (「進め方」, 「段の列」, 「その後の項目」), async.md (I/O の driver, ファイルとパイプ, 未決の論点), multicore.md (スレッドと worker, スケジューラ), vm.md 「時計と driver」, architecture.md 「`eml_io` の内部」「外部 crate」, testing.md 「`eml_io` の検証」「OS ごとのテスト」「nix build の中で流さないテスト」「CI」, and the files under "Additional checks".

## 1. Conclusion

**Q1. Role of io_uring: an optional Linux backend admitted only on measured merit, with epoll as the reference backend.** Framing (a), sharpened:

1. epoll is the *reference* Linux backend: the syscall targets' epoll table, the test-driver parity, the fallback, and every deployment that refuses io_uring (Docker ≥ 25 seccomp, gVisor, `io_uring_disabled` [V dossier B5]) run on it. It is never described as legacy or optional.
2. io_uring exists in the tree in one form: **the Linux default when it can be built, falling back to epoll**. Not an opt-in-only backend, not the sole backend. (This removes the "ship opt-in" outcome S3 left to the user; listed in §6 for the user's confirmation.)
3. The stage keeps its prototype-and-gate shape. One change to the gate: **echo-self-fresh joins the gate cases** (many-self-1000, echo-self-fresh, echo-py-fresh, echo-py-reuse, bulk-1k). Threshold (比 ≤ 0.800 with 判定 「差」, 2 of up to 3 sessions, idle machine, regime check) unchanged. S3 §1.7's regression rule stays; its blocking outcome becomes "fix or discard" (§6, user confirmation).
4. Later stages do not depend on io_uring: `async-files` designs the pool path for regular files on every OS (kqueue needs it) and may add the ring's READ/WRITE as a Linux backend-internal path; `worker-threads` gets one ring per worker by building one driver per worker; `async-threads` keeps the inbox detour (the shipped ring is `SINGLE_ISSUER`).

Why not (b): everything that makes io_uring more than "fewer syscalls on the same model" (multishot recv, provided buffers, `SEND_ZC`) conflicts with the settled buffer contract (one lent, unique `Bytes` per op; the kernel writes only into it; vm.md 「時計と driver」, async.md decision 10). Under that contract the gain is bounded to the syscalls S1 counted (about 6 of 38 per connection in many-self, 2–4 of 8 driver syscalls per round trip in echo-self [V S1 strace on main]; [I] single-digit to low-teens percent of CPU today). The bound is a property of the design, so waiting for async-files or multicore would not move it.

**Q2. Execution model: the mixed model is the long-term model.** Sync attempt first (as IOCP does today), an SQE only for a wait. The ready-gated arm (S1's `io_uring_direct`) is the long-term shape if it measures at least as well; the prototype measures both and ships the winner (S1/S2 unchanged).

| Feature | Long-term direction |
|---|---|
| `DEFER_TASKRUN`+`SINGLE_ISSUER`, `SUBMIT_ALL`, `SOCK_NONEMPTY`, registered ring fd | In (floor 6.1) |
| One-shot `POLL_ADD` for `Peek` and the waker | In. Multishot `POLL_ADD` (`IORING_POLL_ADD_MULTI`) is out: the waker is re-armed once per wake, and `Peek` is one event per step |
| Multishot accept / recv, provided buffers, `SEND_ZC` | Out while the lent-buffer and one-event-per-step contracts stand; continue branch records them in 「その後の項目」 with 見直す条件 |
| Registered files | Out: an fd table to keep in step with the row table; no gate case shows fd-lookup cost |
| Regular-file READ/WRITE | Only if io_uring ships and `async-files` picks it as a Linux path inside the ring backend; never a prerequisite of `async-files` |
| `SQPOLL`, `MSG_RING`, `IOSQE_ASYNC` | Out (kernel threads charged to the process; a second ring; io-wq) |

`Backend` trait: **unchanged by this stage**, and its invariant is "no OS-specific names, no selection knowledge" (already pinned by `the_core_names_no_os` [V trust.rs:256]). Later stages may add methods (file steps, a per-worker constructor); the invariant, not the method count, is the rule.

**Q3. Bindings: hand-written over `libc::syscall`, no crate**, for this stage, with the revisit condition in G-4. ~120 lines of `#[repr(C)]` definitions with size asserts [V S1 sz.c layouts]. `libc` 0.2.191 has the three syscall numbers and nothing else [V dossier grep]. The `io-uring` crate would enter the eml_cli Linux graph and THIRD-PARTY-LICENSES (its `bitflags` and `cfg-if` are already there [V Cargo.lock]), and hides the ring's `unsafe` from `trust.rs` while the pushes and the op table stay ours. **rustix fact corrected:** rustix 1.1.6 is in `Cargo.lock` only through `insta → tempfile`, a dev-dependency [V grep Cargo.lock; `cargo tree -p eml_io -e normal` lists no rustix]; its `io_uring` feature gives raw syscall wrappers and constants, not a ring, so it would save the ~120 definition lines, add rustix + `errno` + `linux-raw-sys` to the shipped graph, and leave the ring `unsafe` where it is. Not worth it for 120 lines.

**`polling` crate (coordinator's addition): reject.** The `io-uring` crate is not adopted, so the package question does not arise. On its own merits: it would replace ~330 lines (`epoll.rs` 161 + `kqueue.rs` 165 [V dossier wc]) with rustix, linux-raw-sys, tracing, cfg-if and a Windows AFD backend to cfg-exclude; it defaults to oneshot and offers edge mode only on some backends, so `Unix<P>`'s edge once-registration, the backend-controlled waker drain and `Ready::fresh` would still need adapter code; and the `unsafe` leaves the `trust.rs` audit. No design gain for the saving.

**Q4. Selection.** `EML_IO_BACKEND` is read **inside `eml_io`, in one public named function**, and every constructor that uses it has a sibling that takes the value:

```rust
// eml_io, real.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum IoBackend { #[default] Auto, Epoll, IoUring }   // all OSes
impl IoBackend {
    /// Linux だけが `EML_IO_BACKEND` を読む。ほかの OS は読まずに `Auto`
    pub fn from_env() -> io::Result<IoBackend>;          // S3's `want`; texts unchanged
}
impl RealDriver {
    pub fn new() -> io::Result<RealDriver>               // = with_backend(IoBackend::from_env()?)
    pub fn with_backend(b: IoBackend) -> io::Result<RealDriver>   // S3's `pick`; non-Linux: Epoll | IoUring -> Err(Unsupported)
    pub fn with_lookup(..)                                // unchanged; uses from_env()
}
```

Not a `RunConfig` field, not a `Clock` payload; eml_vm and eml_cli do not change. Reasons: (1) the setting means the same for every driver of a process and never varies per run or per worker, so it is not run configuration; (2) `Clock` must stay `Copy + Send` and OS-neutral for `worker-threads` (multicore.md 「層の構成」); (3) the sibling gives tests and embedders an explicit path. `worker-threads`: each worker's `RealDriver::new()` reads the same variable; `Auto`'s fallback classes (permission, missing syscall, kernel below the floor, missing feature) are properties of the kernel and the sandbox, so every worker gets the same backend [I]; any other error (`ENOMEM`, `EMFILE`) fails that driver and the run, loudly.

Default: **one default for all Linux**: `Auto` = io_uring when buildable, else epoll (S3 §1.1). Fallback classes `PermissionDenied`, `Unsupported` only. Pinned values fail hard; unknown value fails (`InvalidInput`). Seccomp kill mode: documented escape `EML_IO_BACKEND=epoll`. Minimum kernel **6.1**, a fallback boundary that may rise whenever a feature needs it (premise).

**Q5. Testing and CI.**
- One backend per test process, chosen by the product's own variable (S3 §1.2): profile `real-driver`, run pinned to each backend on every Linux CI leg; the coverage test; ASan on both backends. In-process iteration over backends is rejected (doubles the real-driver tests' durations against the 1 s rule and buys nothing the pinned run does not).
- **Ring unit tests do not skip on machine settings** (changed from S1/S3 after C-4). With the variable unset or `io_uring`, a ring test that cannot build its ring **fails** with a message naming the cause and the opt-out: `io_uring cannot be used here ({e}); set EML_IO_BACKEND=epoll to run the tests on epoll`. With `EML_IO_BACKEND=epoll` the ring tests do not run (they `return` after printing one line; this is an explicit pin, like the Windows mark, not machine detection). So testing.md's 「機械の設定で飛ばすことはしない」 stays intact and needs no exception. The ordinary `cargo nextest run` therefore requires io_uring on Linux; a developer in a sandbox that refuses it runs with `EML_IO_BACKEND=epoll`. `new_follows_the_variable` with the variable unset simply asserts `Uring`.
- `nix build` runs the tests in nix's sandbox (testing.md 「nix build の中で流さないテスト」). Whether that sandbox allows `io_uring_setup` is unknown [I]. Default: if it refuses, the flake's check phase sets `EML_IO_BACKEND=epoll` and testing.md records it in that section (an environment fixed by nix, the section's existing precedent). §6.
- CI proof that the default was exercised: the Linux legs' ordinary run (variable unset) now fails if the runner refuses io_uring, and the pinned `io_uring` run is the same proof made explicit. A runner that refuses io_uring is thus visible at the first CI run of the stage branch.
- **Runner refuses io_uring:** a backend does not become the default without a Linux CI leg that builds it (G-8, Close-time). My default outcome is discard of the stage (後に回した理由: hosted runner が io_uring を作れない; 見直す条件: runner が許したとき). This removes an open question S3 left to the user; §6 lists it for confirmation.

**Q6. POLL_ADD-readiness arm: no.** It keeps every retry syscall and does strictly more SQ work than epoll per wakeup, so it cannot ship; S1's two arms already decompose the lever that matters (EAGAIN removal). It would add a third `Poller` (~150 lines) and lengthen every session for a number nobody acts on.

**Q7 and Q8:** §5.

## 2. Rationale

- **20% stays.** A second Linux backend costs ~900 lines of unsafe-adjacent code, a second CI and ASan run, a second syscall-target table and a permanent "which backend ran" question in every network measurement. S1's strace counts bound the saving (per connection 2 `epoll_ctl` + `getsockopt` + `getpeername` + 1 retry + up to 1.5 EAGAIN of 38 syscalls [V]); at ~0.5–1 µs per syscall [I, dossier B7] against 84 µs per connection [V benchmarks.md 「今」] that is single-digit percent. 20% is the size of win that pays for the cost.
- **echo-self-fresh joins** because both ends run in eml and the ring can remove 2 EAGAIN and 2 post-wait `recvfrom` of ~8 driver syscalls per round trip [V S1 strace] over 21 µs [V benchmarks.md]. It is the case where the whole lever applies and a legitimate eml shape (two tasks on one worker relaying). Without it the gate holds two cases the design cannot help (bulk-1k writes, echo-py's immediate round trips).
- **Binary rule (default or absent).** An opt-in backend the default never exercises is dead unsafe code with spec sentences to maintain. `EML_IO_BACKEND=epoll` is not a symmetric opt-in: it is the escape for sandboxes that kill on the syscalls, and the pinned CI run keeps it tested.
- **Env read inside `eml_io`.** What C-3 (S3 review) objected to was a hidden read with no explicit path. The fix is one named public function plus a value-taking sibling, not a `RunConfig` field that would put a Linux-only enum into an OS-neutral `Copy + Send` config every worker receives.
- **Ring tests fail rather than skip.** The reviewer is right that an unpinned skip on `PermissionDenied` makes pass/fail depend on machine settings, which testing.md:504 forbids for the stated reason; and `nix build`'s exception (testing.md:533) is about an environment nix fixes, not a precedent for machine detection. A loud failure with the opt-out named costs a developer one variable and keeps the principle.
- **Fallback does not make io_uring primary.** Docker ≥ 25's default seccomp returns `EPERM` for the three syscalls; gVisor leaves `io_uring_register` unimplemented [V dossier]. Many deployments will run epoll whatever the default says.
- **`async-files` does not wait.** The pool path for regular files is required on macOS (async.md 「ファイルとパイプ」), so the `RawBuf`-across-threads question (async.md:358) must be solved regardless; the ring path is an optimization weighed in that stage, inside the ring backend.
- **No cross-thread submission on io_uring.** `SINGLE_ISSUER` makes another thread's submit fail with `EEXIST` [V dossier io_uring_setup(2)]; the inbox detour is required for epoll anyway.
- **The roadmap stops prescribing `user_data`, teardown and the floor.** Roadmap 「進め方」: 「決めたこと」は方針で、細部はその段の設計で決める [V line 10]. S1 kept them on merit.

## 3. Alternatives considered and rejected

- **(b) io_uring as the primary backend later stages build on**: the buffer contract bounds its gain; many deployments cannot use it; two Linux designs in `async-files`/`async-threads`.
- **(c) drop the stage without prototyping**: the prototype is two commits and ~12 min of idle-machine measurement, and a discard leaves measured numbers plus a reachable commit, worth more than my estimate once native code shifts the CPU share.
- **Lower threshold**: the cost is permanent.
- **Pure completion** (S1): no syscall saved on immediate paths; breaks vm.md's one-`recv` promise.
- **`io-uring`, rustix, `polling` crates**: Q3.
- **`RunConfig.io_backend` / `Clock::Real(IoBackend)`**: §2.
- **Per-workload or per-deployment default**: the driver is built before the program runs.
- **"Ship with epoll default, io_uring opt-in" on a blocking regression**: binary rule (§6 for the user).
- **In-process backend iteration in tests**: Q5.
- **Unpinned skip of ring tests on `PermissionDenied`** (S1/S3): Q5, C-4.
- **POLL_ADD-readiness arm**: Q6.
- **Reordering `io-uring` after `async-files` or `worker-threads`**: the gate cases are sockets either way, and `async-files` wants to know whether a ring exists before designing its Linux path.

## 4. Assumptions made

- [I] 0.5–1 µs per syscall on this machine (dossier B7); not needed by the gate.
- [I] `Auto`'s fallback classes are identical on every worker thread of a process.
- [I] GitHub's `ubuntu-24.04` runners (kernel 6.17 [V dossier]) may or may not allow `io_uring_setup`; nix's build sandbox likewise. Q5 covers both.
- S1/S2/S3 r2 texts stand wherever marked keep below.

## 5. Implications for implementation

### 5.1 Keep / change / drop table

| Ref | Decision as it stands | Verdict | New state |
|---|---|---|---|
| S1 §1.1, §2 | Mixed model; arms `io_uring` / `io_uring_direct` | keep | Long-term model (Q2) |
| S1 §1.2 | Floor 6.1, flags, features | keep | |
| S1 §1.3 | Hand-written bindings over libc | keep | G-4 with scope and revisit condition |
| S1 §1.4–1.8 | Ring sizes, registered ring fd, one-shot waker poll, cancel, `user_data`, teardown | keep | On merit; the roadmap stops prescribing them |
| S1 §1.9, §2 Files | `enum Linux` delegation; `choose(EML_IO_BACKEND)` in `linux.rs` | change | `choose` → S3's `want`/`pick` → `IoBackend::from_env` / `RealDriver::with_backend` (5.4); `enum Linux` stays |
| S1 §2 feature table | Multishot, provided buffers, SEND_ZC, SQPOLL, MSG_RING, CANCEL_FD, NO_SQARRAY rejected | keep | Continue branch: one 「その後の項目」 entry |
| S1 §1.10, §5 P2 | Prototype bar (three env values), strace identity, smoke runs | keep | Under `EML_IO_BACKEND` unset the ring tests must now build their ring (below) |
| S1 §2 Test bar | `ring_or_skip` skips on PermissionDenied/Unsupported | change | `ring()` fails with the message in Q5 unless `EML_IO_BACKEND=epoll`, which makes ring tests return after one printed line |
| S1 §2 Test bar | Triage rule | keep | |
| S1 §5 trust.rs | `FILES`, `UNSAFE_FILES`, `COMPLETION_BACKENDS` += uring.rs | keep | |
| S2 §1 Commit order, harness | BASE → M1 → P1 → P2 → M2 → M3; `--env`; one binary | keep | |
| S2 §1 Gate cases | many-self-1000, bulk-1k, echo-py-fresh/reuse | change | Add echo-self-fresh (5.5) |
| S2 §1 Gate rule | 比 ≤ 0.800, 判定 差, 2 of 3, idle, A/A, regime | keep | |
| S2 §1 Discard | Revert M2, P2, P1, M1; benchmarks.md `### io_uring と epoll`; async.md:243 pointer | keep | |
| S2 §5 D2 roadmap item | Deferred item repeats 5.11 EXT_ARG, `user_data`, teardown, "CI both" | change | Carries policy and the pointer to `<P2>` only (5.2) |
| S2 §1 Continue | Close reruns the gate on the final binary; check.py | keep | |
| S3 §1.1 | `want`/`pick`, Linux-only read, fallback classes, pinned fails hard, enter in `Uring::new`, error texts | keep semantics, change shape | Public `IoBackend`, `from_env`, `with_backend`; `new()` = `with_backend(from_env()?)` |
| S3 §1.1 spec bullets | vm.md, stdlib.md 「起動」 | keep | architecture.md gets `with_backend` |
| S3 §1.1 [Fable] | Env read in `Linux::new()`; product option deferred | change | Decided (Q4); no `RunConfig` field |
| S3 §1.2 | Profile `real-driver`, coverage test, timing runs, `new_follows_the_variable` | keep | Unset case asserts `Uring` outright |
| S3 §1.2 | Ring tests skip when unpinned and unavailable | change | Fail with the opt-out message (Q5) |
| S3 §1.2 "Hosts without io_uring: ordinary run passes" | | change | Ordinary run on Linux requires io_uring; `EML_IO_BACKEND=epoll` is the opt-out |
| S3 §1.3 | Pinned CI steps both backends; ASan matrix | keep | |
| S3 §1.3 | Runner refuses io_uring → stop and report | change | G-8 (Close-time); default outcome discard; §6 user confirmation |
| S3 §1.4 | ASan both; Miri untouched; abort child test | keep | |
| S3 §1.5 | Remove losing arm and prototype wording | keep | |
| S3 §1.6 docs table | | keep + add | 5.3 |
| S3 §1.7 | Regression classes and thresholds; kept-with-mechanism; blocking thresholds | keep | |
| S3 §1.7 [Fable] | Blocking → user picks fix, epoll default, or discard | change | Fix or discard; §6 user confirmation |
| S3 §1.7 [Fable] | Default never per workload | keep | G-6 |
| S3 §1.8, §6.2, §6.3 | Close failure path; macOS waiver; push permission | keep | |
| X §19.3 | Items kept only from the old roadmap | resolved | Kept on merit; roadmap no longer prescribes them |
| X §19.4 (a)(b)(c) | io-uring crate; POLL_ADD arm; leave S1 | decided | (a) no; (b) no; (c) S1 stands except `choose` and the skip |

### 5.2 Roadmap revision (text points; the implementer writes the prose with natural-japanese)

**`io-uring` entry (replaces line 31):**
- 目的: Linux の本物の driver の既定の backend を io_uring にするか (作れなければ epoll) を、試作と計測で決める。epoll は Linux の基準の backend のままで、io_uring は既定になる形でだけ入れる。
- 試作: `Backend` の実装を1つ (同期の試みを先にし、塞がれた待ちだけを完了で読む形。腕は2つ)。`EML_IO_BACKEND` で選ぶ。
- 門: `bench/net` の many-self-1000、echo-self-fresh、echo-py-fresh、echo-py-reuse、bulk-1k のどれかで、床を引いた単位あたりの user+sys の CPU の比が 0.800 以下で判定が「差」であることが、2回 (割れたら3回目) で成り立つこと。ほかの件の 1.10 以上の後退は直す。直せなければ捨てる。
- 続けるとき: 既定は io_uring で、権限か非対応で作れないときだけ epoll。`EML_IO_BACKEND` (`epoll` | `io_uring`) で固め、固めた backend を作れなければ誤り。変数は `eml_io` の1つの関数が読み、値を直接受ける構築子を並べる。最低の kernel は 6.1 (それ未満は epoll)。束縛は `libc::syscall` の上に自前で書く。Linux のテストは変数を立てないと io_uring を要し、`epoll` に固めると ring のテストは流れない。CI は本物の driver のテストを両方の backend に固めて流す。io_uring を作れる Linux の runner がなければ既定にはしない。
- 捨てるとき: 数字を [計測の基準] に書き、「その後の項目」へ移す。
- 前提: なし。設計: [非同期の設計](async.md) の「`IoDriver` と `eml_io`」。
- Dropped: 5.11 / `EXT_ARG`, `user_data` の形, 落とすときの手順.

**Stage order:** unchanged; `io-uring` stays first.

**`async-files` entry (continue branch only):** add a 論点: 「Linux の通常のファイルの読み書きは、kqueue と同じくプールの仕事で作るのが既定で、io_uring の backend の中で `READ`/`WRITE` に出す形を足すかは、この段の設計で `RawBuf` の規則とともに決める。核は backend の違いを知らない」.

**「その後の項目」 → 「### 非同期」:**
- Discard branch, 「Linux の io_uring の backend」: body points to the prototype commit `<P2>` and to [計測の基準] の「io_uring と epoll」 (design details live in the commit). 後に回した理由: 試作では門の5件のどれでも CPU が2割縮まなかった (いちばん縮んだのは <腕> の <件> の <比>)。貸した buffer を1つだけカーネルに渡す契約の下では、io_uring の利点は待ちの syscall の削減に限られる。 見直す条件: (1) ネイティブ化の後に many-self-1000 と echo-self-fresh の CPU を測り直し、記録した syscall の削減分が CPU の2割に当たるとき。(2) buffer の契約を変えて、カーネルが選ぶ buffer (multishot の受け、provided buffer) を受け入れる設計をするとき。(3) `async-files` の Linux のファイルの道で、プールの仕事の費用が計測で目立ったとき。(4) hosted runner が io_uring を作れなかったために捨てたなら、runner が許したとき。
- Continue branch: S3 (a) 多重の操作と渡す buffer, (b) 6.1〜6.16 の kernel の CI, (c) kept regressions, plus (d) 「io_uring でのスレッドをまたぐ受け付け」: 後に回した理由: 出荷する ring は `SINGLE_ISSUER` で、別のスレッドからの submit は `EEXIST` になる。受け箱の道は epoll に要るので既定にする。 見直す条件: `async-threads` で受け箱の遅れが計測で目立ったとき。

**「### マルチコア対応」:** no change.

### 5.3 Other docs changes (on top of S3 §1.6 and S2 D2)

| File | Branch | Change |
|---|---|---|
| testing.md 「Linux の2つの backend」 (new, S3) | continue | State: 変数を立てない Linux の実行は io_uring を要し、ring のテストは作れなければ落ちる (文言に `EML_IO_BACKEND=epoll` を示す); `epoll` に固めると ring のテストは流れない; io_uring を作れる Linux の脚が CI にない間は既定にしない. No exception is added to 「Windows で流さないテスト」 |
| testing.md 「nix build の中で流さないテスト」 | continue, only if needed | If nix's sandbox refuses io_uring: the check phase sets `EML_IO_BACKEND=epoll`, with the reason |
| async.md 「`IoDriver` と `eml_io`」 :243 | continue | S3's text, plus: 変数を読むのは `eml_io` の `IoBackend::from_env` だけで、`RealDriver::with_backend` が値を直接受ける |
| async.md 「ファイルとパイプ」 | continue | One bullet: Linux で io_uring の backend があるときは、通常のファイルの読み書きを ring の `READ`/`WRITE` で出せ、`RawBuf` が driver のスレッドを出ない。プールの道は kqueue に要るので、足すかは `async-files` の設計で決め、足すなら backend の中で分け、核は知らない |
| async.md 「未決の論点と将来の拡張」 :392 | continue | S3's `SINGLE_ISSUER` note plus a pointer to roadmap item (d) |
| architecture.md 「`eml_io` の内部」 | continue | `RealDriver::new` は `IoBackend::from_env` で選ぶ; `with_backend` は値を直接受ける; `sys/linux.rs` holds `pick` only |
| architecture.md 「CLI と lib API」 | continue | Add `IoBackend` only if eml_io's public items are listed there (implementer checks) |
| vm.md 「時計と driver」 | continue | S3's bullets |
| roadmap.md | both | 5.2 |
| CLAUDE.md | continue | S3's command lines; Architecture line `eml_io`: "io_uring (default where buildable) or epoll, kqueue, IOCP"; Testing bullet: Linux tests need io_uring unless `EML_IO_BACKEND=epoll` |

### 5.4 Code shape of the selection (continue branch, U2)

- `IoBackend` in `real.rs`, re-exported from `lib.rs`. `from_env`: `#[cfg(target_os = "linux")]` reads `env::var_os("EML_IO_BACKEND")` through a pure `fn parse(Option<&OsStr>) -> io::Result<IoBackend>` (S3's `want` mapping and texts); other OSes return `Ok(Auto)` without reading.
- `with_backend(b)`: Linux → `sys::linux::pick(b, Uring::new)` (S3's `pick`, unchanged); other OSes: `Auto` builds `sys::Os`, `Epoll`/`IoUring` → `Err(Unsupported, "this OS has one I/O backend")`.
- `new()` = `with_backend(IoBackend::from_env()?)`; `with_lookup` keeps its signature. `with_backend_and_lookup` only if a test needs both.
- Ring test helper `ring(arm) -> Uring` (replaces `ring_or_skip`): if `IoBackend::from_env()` is `Epoll`, print `EML_IO_BACKEND=epoll: ring tests do not run` and return `None` (the test returns); otherwise `Uring::new(arm).unwrap_or_else(|e| panic!("io_uring cannot be used here ({e}); set EML_IO_BACKEND=epoll to run the tests on epoll"))`.
- Tests: `parse_maps_each_value` (pure); `pick` tests unchanged; `new_follows_the_variable`: `epoll` → `Epoll`, `io_uring` → `Uring`, unset → `Uring`; `cli.rs` bogus-value test unchanged.
- Completion addition to S3 U2: `rg -n 'var_os|env::var' crates/eml_io/src -g '!**/tests/**'` prints exactly one line, in `real.rs` (today it prints nothing [V]; the only other read is `driver/tests/abort.rs:19`, excluded by the glob [V]).

### 5.5 Gate command change (S2 M3, S3 Close)

`G='^(base|many-self-1000|echo-self-fresh|echo-py-fresh|echo-py-reuse|bulk-1k)$'`. A gate session's `wc -l` becomes 12·A (A arms); check.py's `regime` lines stay 2·A (echo-py only). The rule sentence in benchmarks.md `### io_uring と epoll` names the five cases.

## 6. Unresolved items (with defaults)

1. **[needs user confirmation] Binary rule.** "io_uring is either the Linux default (with epoll fallback) or not in the tree; a blocking regression leads to fix or discard, not to an opt-in io_uring." Question: 「io_uring を、既定にならない opt-in の backend としては残さない方針でよいですか (後退で既定にできなければ捨てます)。」 Default: yes.
2. **[needs user confirmation] Runner refuses io_uring → discard.** Question: 「CI の Linux の runner が io_uring を作れないときは、段を捨てて『runner が許したとき』を見直す条件にする方針でよいですか。」 Default: yes. Without push permission (item 3) this cannot be checked before Close.
3. **Push permission for CI** (S3 §6.3). Needs the user at R0.
4. **macOS 「今の数値」 waiver.** Default: waive (S3 §6.2). Needs the user at R0.
5. **nix build sandbox and io_uring.** Default: if `nix build` fails on ring tests, the flake's check phase sets `EML_IO_BACKEND=epoll` and testing.md 「nix build の中で流さないテスト」 records it. Implementer runs `nix build` in U3.
6. **Kernels 6.1–6.16 untested.** Default: roadmap item (S3).
7. **`with_backend_and_lookup`**: only if a test needs both.
8. **architecture.md 「CLI と lib API」**: implementer adds `IoBackend` only where `RealDriver` is listed.

## 7. Additional checks

- [V] `crates/eml_io/src/driver/backend.rs:95-130`: `trait Backend` has `type Sock` and 11 `fn` (now, waker, spawn, listen, socket, shutdown_write, set_nodelay, close, start, cancel, wait). r1's "13" was wrong.
- [V] `grep -rn "var_os\|env::var" crates/eml_io/src` prints only `driver/tests/abort.rs:19`.
- [V] `Cargo.lock`: rustix 1.1.6 depends on bitflags, errno, libc, linux-raw-sys, windows-sys; the only dependent is `tempfile`, whose dependent is `insta` (dev-dependency). `cargo tree -p eml_io -e normal` lists eml_rt, libc and vstd's build graph, no rustix.
- [V] `crates/eml_io/Cargo.toml`: dependencies are eml_rt, optional eml_tls, libc (unix), windows-sys (windows).
- [V] `flake.nix:112`: `checkFlags` skips one test; nothing sets environment for the check phase.
- [V] `.config/nextest.toml`: profiles `default` and `timing` only.
- [V] `crates/eml_vm/src/driver.rs`: `Clock { Real, Virtual }`, `Copy`, `Default = Virtual`; `RunConfig` fields at `lib.rs:145-160`; eml_cli sets `Clock::Real` at `main.rs:104`, `terminal.rs:40`.
- [V] `crates/eml_cli/Cargo.toml`: eml_cli depends on eml_vm, not eml_io.
- [V] `testing.md:504` the no-skip rule and its reason; `:533` the nix-sandbox case.
- [V] `bench/net/cases.tsv`: `echo-self-fresh` exists with unit 往復; `bench/net/` has no `check.py` today (S3 U4 adds it).
- [V] roadmap.md 「進め方」 line 10: 細部 belong to the stage design.
- No scratch copy was needed; no benchmarks were run; no background task is running.

## 8. Conformance criteria

These criteria live in the stage's plan. Docs and code cite spec sections, never these IDs or S1/S2/S3 unit names. "Design check" = what a reviewer reads in a design deliverable; "code check" = a command against the repository at the stage's end unless marked Close-time.

- **G-1** epoll MUST remain the reference Linux backend: the syscall targets' epoll table, the fallback target and the test-driver parity are defined against it; no doc may describe epoll as legacy or optional.
  - Why: deployments that refuse io_uring are common, so epoll is what many users run.
  - Applies to: docs/spec, docs/implementation, `crates/eml_io/src/sys/`, CI. Not covered: the io_uring target table (additional).
  - Design check: the fallback is epoll; benchmarks.md keeps an epoll target table. Code check: `EML_IO_BACKEND=epoll cargo nextest run --profile real-driver` exits 0 on Linux; `rg -n 'legacy|古い backend' docs/spec docs/implementation` prints nothing about epoll.

- **G-2** io_uring MUST exist in the tree only as the Linux default with epoll fallback; it MUST NOT be opt-in-only or the sole Linux backend. (§6 item 1.)
  - Why: an unexercised unsafe backend is debt; a sole io_uring backend strands refused deployments.
  - Applies to: selection code and every later change to it. Not covered: the pinned `epoll` escape.
  - Design check: `Auto` resolves to io_uring whenever it builds; a blocking regression ends in fix or discard. Code check: `new_follows_the_variable` passes with the variable unset (asserts `Uring`), and `cargo nextest run -p eml_io -E 'test(=sys::linux::tests::new_follows_the_variable)'` exits 0.

- **G-3** The mixed model MUST hold: a step makes the same sync attempt epoll makes, or skips it only on the `ready` hint, and submits an SQE only when the sync attempt would block. Multishot requests, provided buffers and zero-copy send MUST NOT be used while the lent-buffer and one-event-per-step contracts stand; the waker's `POLL_ADD` is one-shot.
  - Why: vm.md promises "データのある `Read` は受け付けの1回の `recv` で終わる", and the kernel may write only into the one lent object.
  - Applies to: every file under `crates/eml_io/src/sys/` that submits SQEs, now and in later stages. Not covered: `ASYNC_CANCEL`.
  - Design check: the step table has a sync column for `Recv`/`Send`/`Accept`. Code check (once `sys/uring.rs` exists; today the directory has no ring file): `rg -n 'MULTISHOT|MULTI\b|BUFFER_SELECT|SEND_ZC|PROVIDE_BUFFERS|PBUF_RING|RECVSEND_BUNDLE' crates/eml_io/src` prints nothing; the strace identity of the prototype bar holds (`io_uring_setup` 1, `sendto` = writes, `recvfrom` errors ≤ connections).

- **G-4** For this stage and until the revisit condition holds, `eml_io`'s OS bindings MUST be hand-written over `libc` / `windows-sys`; `io-uring`, rustix's `io_uring`, `polling` and `mio` are not added. Revisit when a second consumer of the io_uring ABI appears (the native runtime, file ops), or when the hand-written definitions would exceed about 300 lines, or when the kernel ABI used goes beyond what size asserts can pin.
  - Why: all `unsafe` stays in files `trust.rs` audits; the shipped graph and THIRD-PARTY-LICENSES stay unchanged; the ABI is ~120 lines.
  - Applies to: `crates/eml_io`. Not covered: `eml_tls` (a protocol, not an OS binding); dev-dependencies.
  - Code check: `cargo tree -p eml_io -e normal --prefix none | sort -u` lists only `eml_io`, `eml_rt`, `libc`, `windows-sys` (and its `windows-*` children), `eml_tls` and its subtree when `--features tls`, and vstd's build graph; `rg -n '^(io-uring|rustix|polling|mio)' crates/eml_io/Cargo.toml` prints nothing.

- **G-5** The `Backend` trait and the driver core MUST stay OS-neutral: no method, type or value names an OS mechanism, and the core never learns which backend runs; selection lives in `sys/`.
  - Why: the core is tested on the fake backend under Miri and must not branch on the backend.
  - Applies to: `driver/backend.rs`, `driver/mod.rs`, `real.rs`. Not covered: `cfg_attr(expect(dead_code))` lists; adding OS-neutral methods in later stages.
  - Code check: `cargo nextest run -p eml_io --lib -E 'test(trust::)'` exits 0 (`the_core_names_no_os` is in it); `rg -n 'io_uring|epoll|kqueue|iocp|IoBackend' crates/eml_io/src/driver` prints only comment lines (lines starting with `//` after whitespace).

- **G-6** Backend choice MUST be a pure function of the variable, the kernel and the sandbox, made when a driver is built, and identical for every driver of a process; it MUST NOT depend on the program, the workload or the run.
  - Why: the driver is built before the program runs; `worker-threads` builds one driver per worker and all must agree.
  - Applies to: `IoBackend::from_env`, `sys::linux::pick`, later per-worker construction. Not covered: `Machine::with_driver` injections.
  - Design check: no selection input other than the three. Code check: `pick`'s signature takes `IoBackend` and a ring constructor only; `rg -n 'var_os|env::var' crates/eml_io/src -g '!**/tests/**'` prints exactly one line, in `real.rs`.

- **G-7** `EML_IO_BACKEND` MUST be read in exactly one public function of `eml_io` named for it, and every `eml_io` constructor that uses it MUST have a sibling taking the value directly. The same shape is the default for any later process-wide setting of the I/O driver.
  - Why: tests and embedders reach a configuration without the environment; the read is auditable.
  - Applies to: `crates/eml_io`. Not covered: `Host` and extern environment reads (`eml_vm/src/hosts.rs`, `eml_rt`'s `Env` externs), CLI flags, and `EML_WORKERS` (decided in `worker-threads`; multicore.md pairs it with `--workers`).
  - Code check: `rg -n 'pub fn from_env|pub fn with_backend' crates/eml_io/src/real.rs` prints both; the G-6 grep.

- **G-8 (Close-time)** A Linux backend MUST NOT become the default unless a Linux CI leg has run the real-driver tests pinned to it and passed; "built" is proven by that pinned run, whose ring tests fail on `PermissionDenied`/`Unsupported`. Who runs it: the implementer at U3, after the user grants the push (§6 item 3); if the push is not granted, the user decides at Close with the local results only. (§6 item 2.)
  - Why: CI checks pass/fail without retries (testing.md 「CI」); an untested default contradicts it.
  - Applies to: `.github/workflows/ci.yml`, testing.md 「Linux の2つの backend」, the Close precondition. Not covered: kernels 6.1–6.16 (roadmap item).
  - Design check: the CI design has a pinned `io_uring` step on every Linux leg and says what happens when it fails. Code check (Close-time, needs the push): `gh run view <id> --json jobs --jq '.jobs[] | .name + " " + .conclusion'` shows `success` for `test (ubuntu-24.04)`; before the push, `EML_IO_BACKEND=io_uring cargo nextest run --profile real-driver` exits 0 locally.

- **G-9** A real-driver test process MUST run on one backend chosen by the product's variable; tests MUST NOT iterate backends in one process or carry test-only selection fields, and MUST NOT skip on machine settings: a ring test that cannot build its ring fails unless `EML_IO_BACKEND=epoll` is set.
  - Why: the 1 s rule; the product path is what gets tested; testing.md 「Windows で流さないテスト」.
  - Applies to: `crates/eml_io/tests`, `crates/eml_vm/tests/{api,signal}.rs`, `crates/eml_cli/tests/{cli,terminal,tls}.rs`, `sys/uring.rs` and `sys/linux.rs` unit tests. Not covered: the ring unit tests' direct `Uring::new`, which is one backend by nature.
  - Review rule: any test naming `IoBackend::Epoll` or `IoBackend::IoUring` is either a ring/selection unit test in `sys/` or the one probe in `new_follows_the_variable`. Code check: `rg -n 'IoBackend::(Epoll|IoUring)' crates --glob '*tests*' --glob '!crates/eml_io/src/sys/**'` prints nothing; `rg -n 'skipped' crates/eml_io/src/sys` prints nothing.

- **G-10** Later stages MUST NOT require io_uring: `async-files` designs the pool path for regular files as the baseline on every OS, and any ring path is an implementation inside the ring backend, invisible to the core; `async-threads` keeps the inbox detour as the default.
  - Why: macOS needs the pool regardless; the shipped ring is `SINGLE_ISSUER`.
  - Applies to: roadmap entries `async-files`, `async-threads`; async.md 「ファイルとパイプ」「未決の論点」. Not covered: a later stage that changes the buffer contract.
  - Design check: those designs name the pool and the inbox as the baseline, and no core code path branches on `IoBackend` (G-5's grep stays empty).

- **G-11** Linux backend measurements MUST pin every label of a post-prototype binary with `--env <label> EML_IO_BACKEND=…`, subtract each label's own `base`, and apply the gate per arm; recorded commands in docs MUST carry the pin.
  - Why: the default may change; a recorded command must mean the same later.
  - Applies to: benchmarks.md 「ネットワークの計測」, `bench/net` usage text, the Close rerun. Not covered: the BASE binary, which has only epoll.
  - Code check: `rg -n 'bench.py' docs | rg -v -- '--env' | rg -v 'main=|BASE|<BASE>'` prints nothing; once S3's U4 has added `bench/net/check.py`, `python3 -I bench/net/check.py <jsonl>` prints only `backend … ok` lines for the gate sessions (Close-time, implementer).

