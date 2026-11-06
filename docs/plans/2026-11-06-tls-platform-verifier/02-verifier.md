# S2: verifier, tests, CI for `tls-platform-verifier` (r3)

Tags: [V] verified (how is given), [I] inferred.

The prototype is patches/02-verifier-prototype.patch (applies to the base commit). It hard-codes the fixtures trust in `client_raw`, where this design dispatches on the driver kind; `crates/eml_tls/examples/probe.rs` in it is a measurement probe and is not committed.

Part 1 is the revised deliverable. Part 2 lists what changed from r2. Unit A implements S1-r2 §5.1 (03-tls-on-pool.md) as written; this document repeats only what the implementer needs beyond it.

---

# Part 1: revised deliverable

## 1. Conclusion

1. **The macOS fact.** macOS rejects today's 100-year fixtures even when the CA is passed as an extra root. The error is `-67901 … not standards compliant` [V: rpv 0.7.1; 15 of 25 `eml_tls` tests fail].
   - With a custom anchor, the leaf validity limit is 825 days: 825 passes, 826 is rejected [V: probe].
   - A 100-year CA is fine [V].
   - A verification time pinned through `ClientConfig`'s time provider reaches the OS [V: probe, with leaves from 2020 and 2030].
   - Our new leaf lasts 31 days, so it stays below every limit Apple applies, including the public-root limits of 398 days and shorter [I].
2. **Fixtures pinned in time.** `gen.sh` creates a CA and a leaf valid 2026-01-01T00:00:00Z..2026-02-01T00:00:00Z. It also creates `other-ca.pem` (`CN=eml other CA`; its key is discarded).
   - `eml_tls::fixtures::NOW` = 2026-01-15T00:00:00Z (`1_768_435_200`).
   - Tests that expect a handshake to succeed verify at `NOW`. The fixtures never need regenerating, and at the real time they are expired.
   - The leaf has no AIA and no CDP [V: `openssl x509 -text`].
3. **`eml_tls` API.**
   - `Name::parse` checks the server name.
   - `Trust` is `Clone + Send + Sync + 'static`. It has three constructors:
     - `platform`: the OS verifier.
     - `platform_at`: the OS verifier with a pinned clock.
     - `fixtures`: rustls WebPKI with only the given CA, at `NOW`.
   - `Conn::client(&Name, &Trust)` does no I/O.
   - An adapter turns every verifier error that is not `InvalidCertificate` into a rejection.
   - New `Error::NoRoots` = "no root certificates are available".
   - No process-wide cache.
4. **Which verifier runs.** No new setting; the run's driver decides:
   - **`DriverKind::Virtual`:** the `Machine` built the test driver itself, so the network is fake. The run uses `Trust::fixtures`.
   - **`Real` and `Injected`:** `Trust::platform`.
   - The VM side is S1 §5.1's `Roots` (`Roots::of(DriverKind)`, `Roots::trust(pem)`) and `client_conn(roots, host, pem)`, built in unit A.
   - An embedder therefore cannot pick the weak verifier for a run that can reach a real peer.
5. **Tests that reach the real OS verifier.** These run on all 4 CI OSes:
   - `eml_tls/tests/platform.rs` (3 tests, in memory, at `NOW`).
   - `eml_cli/tests/tls.rs` (real socket and real driver, the full VM path, `other-ca.pem` at the real time, expects the fixed rejection; this does not depend on the date).
   - Two Linux-only tests in `platform.rs` run a child process with `SSL_CERT_FILE=/dev/null`, which is the nix-sandbox condition. One checks the rpv → `NoRoots` mapping; the other checks that a handshake with an extra root and no system roots succeeds.
6. **Windows.** Evidence that the tests stay offline and fast:
   - rpv's own mock tests use certs without URLs, like ours [V: `ca.go` has no OCSPServer/CRL/AIA].
   - They take both paths (`OnlyExtra`, and `ExtraAndPlatform`, which is our `new_with_extra_roots`, system chain first). 27 tests pass in 0.17 s on windows-latest, image `windows-2025-vs2026` [V: `gh run view` job 127430821725, 2026-11-05].
   - `IGNORE_ALL_REV_UNKNOWN` sits in `verify_chain_policy`, which runs on whichever chain is final [V: windows.rs:151-190].
   - The remaining unknown is Windows' automatic root update on the first chain built against the system store [I].
   - The PR's CI run is the probe before merging. The fallback is in §6.
7. **Linux and nix.** No test relies on OS roots. The sandbox condition is covered in CI by the Linux child test. A manual `nix build` result on aarch64-darwin is in §7.
8. **Dependencies.**
   - Added: `rustls-platform-verifier` 0.7.1. macOS adds core-foundation(-sys) and security-framework(-sys); Linux adds rustls-native-certs and openssl-probe; Windows reuses windows-sys.
   - Removed: `webpki-roots`, and `CDLA-Permissive-2.0` from `about.toml`.
   - [V: `nix/third-party-licenses.sh --check` exits 0.]

## 2. Rationale

- **Pinned time, not regenerated short-lived certs.** Regenerated certs would make results depend on the calendar. Pinning works on every OS:
  - macOS [V].
  - Windows: `now` is passed as `pTime` (windows.rs:440-457) [V source].
  - Linux: WebPKI takes `now` [V source].
- **Test-world verifier chosen by the driver kind, not by a `RunConfig` field.** CLAUDE.md forbids test-only fields. A selectable verifier that ignores the OS roots, pins the time and checks no revocation must not be selectable for a run with real sockets.
  - `DriverKind::Virtual` already exists and means "the `Machine` built the test driver". Under it the only peers are the test driver's (`tls.localhost`, which presents the fixture leaf).
  - The fixture keys and the test driver already ship in the binary for that world. The weak verifier adds no reach beyond it.
  - `Injected` (an embedder's own driver) gets `Platform`.
- **Rustls WebPKI rather than the OS verifier in the test world.** The test world checks eml's TLS pumping. WebPKI with only the passed roots is the same on all four OSes and in the nix sandbox, and takes about 0.1 ms [V: probe], against 1.3-5 ms for SecTrust [V]. On Linux the OS verifier is that same WebPKI plus the system roots.
- **The real-driver test becomes a rejection test.** On the real driver the verifier is the OS one at the real time. No fixed certificate can pass there forever, and a pinned-time option for real runs is the weak path just ruled out.
  - Rejection with `other-ca.pem` (`UnknownIssuer`) holds at any date. It covers the production chain end to end on every OS: real driver, VM `client_raw`, rpv, `Opaque`, the fixed message.
  - Lost: a successful data path over the real driver. That path is the same `std/Tls.em` code over `Std.Net`. The real-driver `Std.Net` tests cover the socket side, and the UI tests cover the TLS side.
- **No cache.** On Linux a construction costs 0.7-1.6 ms for a 333 KB bundle with 128 roots [V: same parse + `WebPkiServerVerifier` build on macOS `/etc/ssl/cert.pem`, debug and release]. That is the order of a handshake. Without a cache the design has no global state, sees changes to root files and `SSL_CERT_*`, and S1 may put construction on the pool anyway.
- **Construction split from the connection.** Construction is the only step that can block before the handshake (Linux reads files). Checking the name first keeps the order of setup errors (ServerName before CaPem/NoRoots) [V: the eml_vm test fails when the trust is built first].
- **The adapter (`Opaque`).** rpv can return `rustls::Error::General(os text)` from `verify_server_cert` [V: apple.rs `create_with_certificates(..).map_err(General(e.to_string()))`; windows.rs store/engine errors]. `push_tls` would display it.
  - `InvalidCertificate` passes through unchanged, so rustls still sends a specific alert.
  - Cost: an OS-side failure (for example a Windows engine error) is reported as a certificate rejection. It is documented and not logged, because eml installs no `log` logger.
- **Extra roots checked first** (`RootCertStore::add` on a scratch store). `CaPem` then does not depend on the OS and comes before any file read [V: `setup_errors` passes on macOS].
- **Sending `Conn` to the pool** (coordination fact).
  - `Trust` holds `Arc<dyn ServerCertVerifier>`, and that trait requires `Send + Sync`, so this is checked at compile time.
  - rpv's Windows `CertEngine` declares `Send + Sync` and states that its `Drop` is safe on any thread [V: windows.rs:312-324].
  - macOS `SecCertificate` releases are CF refcount drops [I: thread-safe].
  - `Conn` stays `Send + 'static`, and a compile-time assertion guards it (§5.5).

## 3. Alternatives considered and rejected

- **Short-lived fixtures regenerated periodically:** results depend on the calendar (rpv #117).
- **A `RunConfig.tls_verify` field (r1):** a test-only, selectable weak verifier in a production API.
- **A dev-only cargo feature that switches it on:** with feature unification, test builds would no longer exercise the production code paths. It adds a feature combination to keep green.
- **OS verifier at a pinned time for the whole test world:** brings Windows chain building into about 20 tests. The outcome of `Tls.connect` with no CA would depend on the OS: unknown issuer on macOS and Windows, `NoRoots` on Linux without root files.
- **rpv `dbg` / `verifier_for_dbg`:** tests rpv's fake-root path, not the `new_with_extra_roots` path production uses.
- **Cert generation at run time (rcgen):** C-crypto dev-dependency outside `eml_tls` breaks the `aws-lc|ring v` greps.
- **A process-wide cache (r1):** see §2.
- **`BuilderVerifierExt` / `ConfigVerifierExt`:** no extra roots, no time hook, and the second one uses the process-default provider. `new_with_extra_roots` with an empty list equals `new` on the three desktop OSes [V: source].
- **On Windows, `#[ignore]` plus a separate CI step (r1 fallback):** testing.md 「CI」 does not run `#[ignore]` tests in CI.

## 4. Assumptions made

- The run's `DriverKind` is read on the worker as `sched.driver_kind` in `async_op`, and is passed to `client_conn` as a `Roots` value (S1 §5.1). This is why unit A moves `TlsClientRaw` into `is_async`: `run_extern` has no scheduler.
- rustls does not call the verifier after the handshake: no renegotiation; post-handshake auth is client-cert only [I]. A resumed session would skip verification, but a fresh `ClientConfig` per connection carries a fresh resumption store, so no connection resumes another [V: `ClientConfig.resumption` is per config; we build one per connection].

## 5. Implications for implementation

### 5.1 `crates/eml_tls`

```rust
pub use rustls::pki_types::UnixTime;

pub enum Error { ServerName, CaPem, CertPem, KeyPem, Config, NoRoots }
// NoRoots => "no root certificates are available"; the other texts are unchanged

#[derive(Clone, Debug)] pub struct Name(ServerName<'static>);
impl Name { pub fn parse(name: &str) -> Result<Name, Error>; }                 // ServerName

#[derive(Clone, Debug)]
pub struct Trust { verifier: Arc<dyn ServerCertVerifier>, now: Option<UnixTime> }
impl Trust {
    /// The OS verifier; `extra_ca_pem` gives extra roots. Linux reads the root files on every call
    pub fn platform(extra_ca_pem: &[u8]) -> Result<Trust, Error>;
    /// The same with a pinned clock; for the tests of the OS verifier
    pub fn platform_at(extra_ca_pem: &[u8], now: UnixTime) -> Result<Trust, Error>;
    /// The test world: only the CAs in `ca_pem`, rustls `WebPkiServerVerifier`, at `fixtures::NOW`. Empty => NoRoots
    pub fn fixtures(ca_pem: &[u8]) -> Result<Trust, Error>;
}
impl Conn {
    pub fn client(name: &Name, trust: &Trust) -> Result<Conn, Error>;  // Config only; no I/O
}
// fixtures.rs: CA_PEM, CERT_PEM, KEY_PEM, OTHER_CA_PEM, pub const NOW: UnixTime = since_unix_epoch(1_768_435_200 s)
```

Rules:
- **Extra roots:**
  - non-empty input with no PEM block, an `Err` from `pem_slice_iter`, or a failed `RootCertStore::add` on a scratch store → `CaPem`.
- **Errors from rpv construction:**
  - an empty extra-root list → `NoRoots`;
  - `InvalidCertificate(_)` → `CaPem`;
  - otherwise → `Config`.
- **`Opaque(Arc<dyn ServerCertVerifier>)`:** wraps both verifiers.
  - In `verify_server_cert`, `Err(InvalidCertificate(_))` passes through and any other `Err` becomes `InvalidCertificate(ApplicationVerificationFailure)`.
  - It forwards `verify_tls12_signature`, `verify_tls13_signature`, `supported_verify_schemes`, `requires_raw_public_keys` and `root_hint_subjects`.
- **`ClientConfig`:** built per connection, with `builder_with_details(provider(), Arc::new(Pinned(now)))` when `now` is set and `builder_with_provider` otherwise, then `.dangerous().with_custom_certificate_verifier(..)`.
- **`push_tls`'s mapping:** unchanged.

### 5.2 What blocks (report to S1)

| Call | Linux | macOS | Windows |
|---|---|---|---|
| `Name::parse`, `Conn::client`, `pull_tls`, `write_plain`, `read_plain`, `status`, `push_eof`, `send_close_notify`, `Trust::fixtures` | CPU, µs [V code; probe ≈10 µs] | same | same |
| `Trust::platform(_)`, `platform_at` | reads `SSL_CERT_FILE`/`SSL_CERT_DIR` or the openssl-probe paths and parses them, eagerly [V source]: ≈1 ms for a 333 KB bundle [V by proxy], local file I/O | `SecCertificate::from_der` per root, 10-50 µs [V] | `new`: none; extra roots: memory store plus `CertCreateCertificateChainEngine` [I: sub-ms] |
| `push_tls` while `status & HANDSHAKING`: the push that completes Certificate + CertificateVerify runs `verify_server_cert` inline [V: rustls tls13.rs:1192, tls12.rs:885] | WebPKI, CPU only [V] | `SecTrustEvaluateWithError`: trustd IPC; it may fetch OCSP/CRL/AIA by OS default, with no limit set by rpv [V source]. 1.3-5 ms for the fixture chain [V] | `CertGetCertificateChain` up to twice (system, then the extra-root engine on PARTIAL_CHAIN/UNTRUSTED_ROOT), each with a 10 s accumulative URL timeout, so about 20 s at worst [V flags; I duration], plus automatic root update [I] |
| `push_tls` after the handshake | does not verify [I] | same | same |

For S1:
- Only a `push_tls` made while `HANDSHAKING` is set can block in the verifier.
- `Trust` and `Conn` can be built on the VM thread or the pool, and can be dropped on either.

### 5.3 VM and bench

- **VM:** unit A implements S1 §5.1 as written. It checks against the `eml_tls` API of §5.1: `Name::parse(host)` → `Result<Name, Error>`, `Trust::{fixtures, platform}(&[u8])` → `Result<Trust, Error>`, `Conn::client(&Name, &Trust)` → `Result<Conn, Error>`, and `Error: Display` gives the texts. The error order of §5.1 (ServerName, then `CaPem`, then `NoRoots`, then `Config`) is the order these functions produce [V: prototype; `CaPem` is checked inside `Trust::*` before any verifier is built]. Nothing in §5.1 needs changing. In detail, unit A:
  - Replaces `unsafe fn client_raw` in `crates/eml_vm/src/tls.rs` with `Roots` and `client_conn`. It removes the `Extern::TlsClientRaw` arm of `run_extern` (vm.rs:4246 today) and the `off::Conn::client` stub with its `NotAvailable` type. The build without `tls` gets its text from `client_conn` (`cannot set up TLS for {host}: not available`, as today).
  - Makes `SETUP_KIND` `pub(crate)`.
  - Adds `Extern::TlsClientRaw` to `Extern::is_async` (`crates/eml_extern/src/lib.rs`; add `Std.Tls`'s `client_raw` to its doc comment) and adds the `async_op` arm of S1 §5.1. The arm stays synchronous: it copies `host` and `pem`, calls `client_conn(Roots::of(sched.driver_kind), ..)`, and either allocates the `Foreign` or calls `raise_again` with `Raise::Io { kind: SETUP_KIND, .. }`.
  - Leaves the `on_tests` helpers in S1 §5.1's final form: `new_client`, `pump` (`HANDSHAKING` dispatch) and `client_errors_are_other_with_fixed_messages` (on `client_conn`).
- **Bench:**
  - `crates/eml_tls/examples/https_server.rs` takes `CERT_PEM_PATH KEY_PEM_PATH MIB`. Its only user is `bench/scripts.sh` [V: grep].
  - The `https-get` row in `bench/scripts.sh` makes a fresh PKI in a temp dir with `/usr/bin/openssl` (LibreSSL 3.3.6):
    - CA with `-days 2`, `basicConstraints=critical,CA:TRUE`, `keyUsage=critical,keyCertSign,cRLSign` and `subjectKeyIdentifier=hash`.
    - Leaf with `-days 2`, `basicConstraints=critical,CA:FALSE`, `keyUsage=critical,digitalSignature`, `extendedKeyUsage=serverAuth`, `subjectAltName=IP:127.0.0.1`, `subjectKeyIdentifier=hash` and `authorityKeyIdentifier=keyid`.
    - Key through `pkcs8 -topk8 -nocrypt`.
  - The AKI matters [V]: without it Python 3.14's default (strict) context rejects the chain with `Missing Authority Key Identifier`. With it, both Python and the macOS OS verifier accept it.

### 5.4 Units of work (one commit each; green after each)

**Order across S1 and S2:** S2-A, S2-C, S1a, S1b, S2-B, S2-D.
- S2-C lands right after A: A's fixtures are expired at the real time, so the bench row `https-get` fails until C gives it a fresh PKI. C does not depend on rpv.
- S2-B must not land before S1b: once S1b is in, the handshake pushes that can block in the OS verifier run on the pool.

**A. Pinned fixtures, `Trust`, and the VM's verifier dispatch** (lands before S1a).

Contents:
- `gen.sh`: the Jan 2026 window, `other-ca.pem` and its copy to `tests/ui/run-fail/tls/`. Regenerate.
- `fixtures.rs`: `NOW`, `OTHER_CA_PEM`.
- `eml_tls`:
  - `Name`, `Trust { fixtures, platform }`, `Conn::client(&Name, &Trust)`, `Error::NoRoots`.
  - In this commit `Trust::platform` is today's code moved as it is: webpki-roots plus the extra PEM, at the real time. No new semantics, so nothing has to be thrown away later.
- `eml_vm` and `eml_extern`: the whole of §5.3 "VM".
- Every test change of A in §5.5, including `bad_ca.em`, `no_roots.em` and the real-driver test.
- Docs:
  - `docs/spec/vm.md` 「時計と driver」: the test world's verification.
  - `docs/spec/stdlib.md` 「`Std.Tls`」: the `NoRoots` row, worded for the test-driver condition. The extern paragraph changes from "どれも待たないので、VM は普通の extern の行として実行する" to: `client_raw` runs as an `Async` instruction because it reads the run's driver kind, and it still does not wait; the other rows stay ordinary externs.
  - `docs/spec/vm.md`'s list of `Async`-instruction rows, if it enumerates them.

Completion:
- `cargo nextest run` exits 0. `cargo clippy --all-targets -- -D warnings` exits 0. `cargo fmt --check` exits 0.
- `cargo clippy -p eml_vm --all-targets --no-default-features -- -D warnings` exits 0. `cargo nextest run -p eml_io -p eml_vm -E 'test(tls_off::)'` gives 2 passed.
- `cargo nextest run -p eml_vm --features tls --lib -E 'test(tls::)'` gives 10 passed (7 `tests`, 3 `on_tests`). This is the N in S1's completion criteria.
- `cargo nextest run -p eml_core_ir --test integration -E 'test(externs::)'` gives 14 passed.
- `git status --porcelain crates/eml_cli/tests/snapshots` shows exactly these four lines:
  - ` M crates/eml_cli/tests/snapshots/integration__ui__run_fail@tls__unknown_ca.em.snap`
  - ` M crates/eml_cli/tests/snapshots/integration__ui__run_fail@tls__server_closes_before_handshake.em.snap`
  - `?? crates/eml_cli/tests/snapshots/integration__ui__run_fail@tls__no_roots.em.snap`
  - `?? crates/eml_cli/tests/snapshots/integration__ui__run_fail@tls__bad_ca.em.snap`

**S1a, S1b** (S1-r2 §5.7).

**B. Switch to rustls-platform-verifier** (lands after S1b).

Contents:
- `Trust::platform*` = `rustls_platform_verifier::Verifier::new_with_extra_roots(roots, provider())` in `Opaque`, plus `platform_at`.
- Remove `webpki-roots` from the workspace and from `eml_tls`.
- Add `crates/eml_tls/tests/platform.rs` and `mod platform;`.
- `about.toml`: remove `"CDLA-Permissive-2.0"`. Regenerate `THIRD-PARTY-LICENSES.md`.
- `docs/spec/stdlib.md` 「`Std.Tls`」 wording (§5.6), and the `NoRoots` row with the Linux condition added.

Completion:
- `grep -c 'name = "webpki-roots"' Cargo.lock` → `0`.
- `cargo nextest run` exits 0, and `cargo nextest run -p eml_tls` passes.
- `nix develop -c nix/third-party-licenses.sh --check` exits 0.
- The diff of the crate list adds the rpv family and removes webpki-roots. The exact versions are whatever `Cargo.lock` resolves.
- The 4 `feature resolution of tls` commands in ci.yml still hold.
- `git status --porcelain crates/eml_cli/tests/snapshots` is empty.
- The PR's CI is green on all 4 OSes. On windows-2025 the nextest lines for `eml_tls::integration platform::*` and `eml_cli::integration tls::*` each show < 1.000 s.

Manual checks:
- aarch64-darwin: `nix build` succeeds. This runs the check phase, including the `platform::` tests on macOS (§7).
- x86_64-linux, by whoever has a Linux machine: `nix build` succeeds [I]. CI's Linux child tests cover the same condition.
- `.#cross` clippy is unaffected because it excludes `eml_tls` [V: the `--exclude eml_tls` tree has no rpv]. Not run.

**C. Bench** (§5.3), right after A.

Completion: `bench/scripts.sh` on macOS prints the `https-get` row, with no `rejected` and no `CERTIFICATE_VERIFY_FAILED` on stderr.

**D. Docs** (§5.6), CLAUDE.md and README crate lines, after B.

Completion: `grep -rn 'webpki-roots\|webpki-root' docs CLAUDE.md README.md crates std bench about.toml` → no hits.

### 5.5 Test changes (testing.md 「テストの変更の運用」)

The wording follows the table there:
- kind 1 = a move between `run`/`run-fail`/`check-fail`, or a deletion;
- kind 2 = a change to UI output or to any expected value;
- kind 3 = only how the test is put together changes, and every snapshot string and `assert` value stays byte-identical.

#### Unit A

**Kind 1:** none.

**Kind 2:**
- **Fixture data:** `crates/eml_tls/fixtures/{ca,cert,key}.pem` and the 4 `ca.pem` copies are regenerated with the Jan 2026 window.
- **Test world, as a scope:**
  - Every TLS run on the test driver now verifies with `Trust::fixtures`: all `tests/ui/{run,run-fail}/{tls,http}` UI tests, `crates/eml_io/tests/tls.rs`, and `eml_vm` `tls::on_tests`.
  - There, `Tls.connect` or `wrap` without a CA fails at setup with `NoRoots` instead of reaching the handshake.
  - Outputs change only as listed in this unit.
- **`tests/ui/run-fail/tls/unknown_ca.em`** and its snapshot:
  - The header comment is rewritten (still 2 lines):
    ```
    -- 渡したCAがテストのdriverの証明書に署名していなければ、handshakeは証明書の拒否で失敗する。
    -- 文言にrustlsとOSの詳細は付かない (docs/spec/stdlib.mdの「`Std.Tls`」)
    ```
  - `import Fs` is added, and the call becomes `Tls.connect_with_ca (Fs.read_text "other-ca.pem") "tls.localhost" port`.
  - The snapshot's location moves `19:7` → `20:7`; the message and the kind stay the same [V].
- **`tests/ui/run-fail/tls/server_closes_before_handshake.em`** and its snapshot: `import Fs` is added, and the call becomes `Tls.wrap_with_ca (Fs.read_text "ca.pem") "tls.localhost" c`. The location moves `21:7` → `22:7` [V].
- **`crates/eml_tls/tests/conn.rs::an_unknown_ca_is_rejected_with_a_fixed_message`** and **`crates/eml_io/tests/tls.rs::a_rejected_certificate_closes_the_plaintext_end`:** the input goes from the default roots (`b""`) to `Trust::fixtures(OTHER_CA_PEM)`. The assertions are unchanged.
- **`conn.rs::setup_errors`:**
  - the `CaPem` cases go through `Trust::fixtures` and `Trust::platform`;
  - `Trust::fixtures(b"")` → `NoRoots` is added, and the `NoRoots` text joins the list of fixed texts.
- **`crates/eml_tls/tests/provider.rs`:**
  - it now goes through `Conn::client(&Name::parse("tls.localhost")?, &Trust::fixtures(CA_PEM)?)` and asserts `HANDSHAKING|HAS_OUTGOING`, then a first byte `0x16`, then `HANDSHAKING`;
  - its point stays the same: aws-lc is linked and used.
- **`eml_vm` `tls::on_tests::client_errors_are_other_with_fixed_messages`:**
  - It now calls `client_conn(Roots::Fixtures, host, pem.as_bytes())` and asserts the `Err` strings.
  - Its `raised.kind == IO_OTHER` assertion and its leak check are removed: `client_conn` touches no `Ctx`. The kind is now checked by the new UI test `bad_ca.em`.
  - A case is added: `("tls.localhost", "", "cannot set up TLS for tls.localhost: no root certificates are available")`.
  - The two existing message strings are unchanged.

**Kind 3** (only the assembly changes; every assert value identical):
- `conn.rs` `client()` helper → `Conn::client(&Name::parse(n).unwrap(), &Trust::fixtures(CA_PEM).unwrap())`; `conn.rs` `ServerName` cases → `Name::parse`.
- `eml_io/tests/tls.rs` `client()` helper.
- `eml_vm` `on_tests` `new_client` → `alloc_foreign(client_conn(Roots::Fixtures, "tls.localhost", CA_PEM).ok().unwrap(), true)`.
- `eml_vm` `on_tests` `pump`: while `HANDSHAKING` is set it pushes through `(*tls_conn(ctx, client)?).conn.push_tls(..)`, and otherwise through the `push_tls` arm (S1 §5.1).
- The changed verifier behind these helpers is covered by the "test world" scope above.

**New tests:**
- **`tests/ui/run-fail/tls/bad_ca.em`** with its snapshot. This takes S1 §5.1's shape, with a 2-line header comment [V: snapshot produced in the scratch clone on the synchronous path].
  ```
  -- `Tls.connect_with_ca`に渡したCAがPEMとして読めないと、バイトを送る前に固定の文言でTLSの設定が失敗する
  -- (docs/spec/stdlib.mdの「`Std.Tls`」)
  import Async
  import Net
  import Tls

  main : Unit -> <IO, Panic> Unit
  main () =
    let r = Async.run (fn () ->
      let l = Net.listen "127.0.0.1" 0
      let (port, l) = Net.local_port l
      let r = try_io (fn () -> Tls.close (Tls.connect_with_ca "garbage" "tls.localhost" port))
      Net.close_listener l
      r)
    match r with
      | Ok () -> println "connected"
      | Err e ->
        println (show e.kind)
        fail_io e.kind e.message
  ```
  The snapshot body:
  ```
  --- stdout ---
  Other
  --- stderr ---
  --- runtime error ---
  cannot set up TLS for tls.localhost: the CA certificates are not valid PEM
    at run-fail/tls/bad_ca.em:19:7
  ```
- **`tests/ui/run-fail/tls/no_roots.em`** with its snapshot. Same shape as `bad_ca.em`. The header comment:
  ```
  -- テストのdriverでは根はプログラムが渡すCAだけなので、CAを渡さない`Tls.connect`は接続の設定で失敗する
  -- (docs/spec/vm.mdの「時計と driver」)
  ```
  The call is `Tls.close (Tls.connect "tls.localhost" port)`. The snapshot has stdout `Other` and the runtime error `cannot set up TLS for tls.localhost: no root certificates are available` at `run-fail/tls/no_roots.em:19:7` [V].
- **`crates/eml_tls/tests/fixtures.rs`:**
  - parse `OTHER_CA_PEM`;
  - every `other-ca.pem` under `tests/ui` equals `OTHER_CA_PEM`, and exactly one exists (`run-fail/tls`).
- **`conn.rs`:** a compile-time assertion that `Trust: Send + Sync + 'static` and `Conn: Send + 'static`, next to `the_box_fits_alloc_foreign`.

#### Real-driver test (unit A)

**Kind 1:** delete `crates/eml_cli/tests/tls.rs::a_client_talks_to_a_local_tls_server`.
- Reason: on the real driver no checked-in certificate stays valid at the real time (the new fixtures expire on 2026-02-01, and from unit B on the OS verifier checks the real time). A pinned-time option for real runs would put a weak verifier on a production path.
- It is replaced by `a_real_socket_handshake_is_rejected_by_the_os_verifier`.

This must land in **unit A**: the regenerated fixtures are expired at the real time, and the webpki verifier in A checks the real time, so the old test would fail as soon as A lands. In A the rejection comes from webpki-roots (unknown issuer); from B on it comes from rpv. The expected stdout is the same in both:
- The new test runs the real driver (`Clock::Real`) against a loopback server thread with `Conn::server(CERT_PEM, KEY_PEM)`.
- The program runs `Tls.connect_with_ca (Fs.read_text "other-ca.pem") "127.0.0.1" {port}` inside `try_io` and prints the failure.
- Expected stdout: `Other\ncannot complete the TLS handshake with 127.0.0.1: the server certificate was rejected\n`.
- The server thread checks that the client's alert arrived (`FAILED` set after `push_tls`, or EOF), with a 5 s read timeout.
- The test needs `other-ca.pem` under its host root `crates/eml_tls/fixtures`, which `gen.sh` writes there.

#### Unit B

**Kind 1, 2, 3:** none.

**New tests:** `crates/eml_tls/tests/platform.rs`.
- On all OSes, at `NOW` [V on macOS: about 42 ms each]:
  - `an_extra_root_is_trusted`: `tls.localhost` and `127.0.0.1`, status 0.
  - `an_unknown_ca_is_rejected`: `OTHER_CA_PEM`, fixed message.
  - `a_name_not_in_the_certificate_is_rejected`: `example.com`.
- Linux only, under `#[cfg(all(unix, not(target_vendor = "apple")))]`:
  - `no_system_roots_and_no_ca_is_no_roots`: `Trust::platform(b"")` → `Err(NoRoots)`.
  - `no_system_roots_with_a_ca_handshakes`: `platform_at(CA_PEM, NOW)`, status 0.
  - Each runs its body in a child copy of the test binary: `--exact <name> --nocapture --test-threads=1`, with `SSL_CERT_FILE=/dev/null`, `SSL_CERT_DIR` removed, and a marker env var. This is the pattern of `crates/eml_io/src/driver/tests/abort.rs`. [V source: rnc loads only `SSL_CERT_FILE` when it is set; an empty store makes rpv return its `General` error.]


### 5.6 Docs (intent; Japanese via PLaMo; no space between Japanese and Latin text)

- **`docs/spec/stdlib.md` 「`Std.Tls`」:**
  - The OS verifier checks certificates: Security.framework on macOS, CryptoAPI on Windows, and rustls over the OS root files on Linux (`SSL_CERT_FILE`/`SSL_CERT_DIR` override them).
  - The CA from `_with_ca` is trusted in addition to the OS roots.
  - Depending on the OS, verification may contact the network. Waits stay `Std.Net`'s; where the work runs is S1's text.
  - New row: 根の証明書がない | `Other` | `cannot set up TLS for {host}: no root certificates are available`. Condition: Linux without root files and no CA passed, or a run on the test driver with no CA passed. Advice: use `connect_with_ca`.
  - "理由によらず同じ文言で、rustls と OS の詳細を付けない". An OS-side failure inside verification is also reported as a rejection.
  - `Std.Http` `ca` (line 1746): "空なら OS の根だけ".
- **`docs/spec/vm.md` 「時計と driver」:** a run whose `Machine` built the test driver verifies TLS with only the CA it is passed, using rustls at 2026-01-15T00:00:00Z. A run on the real driver or an injected driver uses the OS verifier.
- **`docs/implementation/architecture.md`:**
  - Lines 50, 81 and 96: rustls-platform-verifier.
  - 「`eml_tls` の内部」:
    - `Name` / `Trust` / `Conn::client`;
    - no process-wide state, and Linux re-reads the root files on every construction;
    - the per-connection `ClientConfig`, and no resumption between connections;
    - the check of the extra roots and the error mapping;
    - `Opaque` and its diagnostic cost;
    - the window of the fixtures, `NOW`, `other-ca.pem`, and that the fixtures are expired at the real time;
    - the `https_server` arguments.
  - Crate table: replace `webpki-roots` with `rustls-platform-verifier | 0.7 | eml_tls の証明書の検証 (macOS は security-framework、Linux は rustls-native-certs と openssl-probe、Windows は windows-sys の CryptoAPI)`.
  - Licence paragraph: drop webpki-roots and CDLA.
- **`docs/implementation/status.md`** (lines 42, 150) and **`docs/overview.md`** (line 56): roots = the OS roots plus the passed CA.
- **`docs/future/roadmap.md`:**
  - Remove the item when the stage closes.
  - Line 415 (musl): a static distribution needs root files or `SSL_CERT_FILE`, or programs that pass CAs.
- **`docs/implementation/testing.md`:**
  - 427-428: the test world uses the fixtures trust; the `other-ca.pem` copy.
  - 550: the real-driver test is a rejection through the OS verifier.
  - New paragraph:
    - which tests reach the OS verifier: `platform::`, `eml_cli` `tls::`, and the Linux child tests standing in for the nix sandbox;
    - the fixtures have no AIA/CDP, and Windows builds the system chain first;
    - no test relies on the OS roots.
- **`docs/implementation/benchmarks.md`** (162-172): `https-get` makes its PKI on each run.
- **CLAUDE.md and README crate line:** `(aws-lc-rs provider, rustls-platform-verifier)`.

## 6. Unresolved items (with defaults)

- **Windows automatic root update** [I]. Default: accept, based on rpv's windows-2025 evidence (§1.6).
  - If the PR's CI shows a `platform::` or `eml_cli` `tls::` test ≥ 1 s or failing on windows-2025 with no code cause, the plan adds `#[cfg(not(windows))]` to those tests, with an entry in testing.md 「OS ごとのテスト」.
  - That is a kind-1 change (tests removed on one OS) and needs the plan's approval before it lands. No `#[ignore]`.
- **Names** (`Trust`, `Name`, `NoRoots` text): default as given. S1 may rename `Trust` if it carries the trust across the pool.
- **Ed25519 server certs on macOS/Windows** (rpv #195): no action; they show as a rejection.

## 7. Additional checks

- [V] macOS rpv behaviour: the 100-year fixtures are rejected (`-67901`); 825 days passes, 826 is rejected; a 100-year CA is fine; a pinned time works; an unknown CA gives `UnknownIssuer`; a name mismatch gives `NotValidForName`. Method: `examples/probe.rs` against `(the probe's own certs; not kept)/gen.sh` outputs.
- [V] rpv's CI on 2026-11-05:
  - windows-latest = image `windows-2025-vs2026`: 27 mock tests (OnlyExtra + ExtraAndPlatform, certs without URLs) in 0.17 s.
  - macos-latest = `macos-26-arm64`: in 0.33 s. These are rpv's own ~731-day leaves; ours are 31 days.
  - Method: `gh run view -R rustls/rustls-platform-verifier --job …`.
- [V] Cost of building a Linux-style verifier (pinned bundle parse + `WebPkiServerVerifier`): 0.7-1.6 ms for 128 roots and 333 KB.
- [V] Bench PKI made with LibreSSL: Python 3.14 rejects it without AKI and accepts it with SKI/AKI; macOS rpv accepts it.
- [V] Prototype: `eml_tls` 29/29 pass. On the TLS/UI/fixtures subset of `eml_vm`, `eml_io` and `eml_cli`, every test passes except the two snapshot location changes.
- [V] Licences: `--check` exits 0 without webpki-roots or CDLA; schannel 0.1.30 (MIT) is included.
- [V] `cargo tree -e normal` per target: Linux adds openssl-probe, rustls-native-certs, rpv; macOS adds core-foundation(-sys), security-framework(-sys), bitflags, libc, rpv; Windows adds rpv and windows-sys/windows-link, with no schannel.
- [V] aarch64-darwin `nix build "git+file://<scratch>" -L` of the r1 prototype exits 0. It links security-framework, and its check phase runs the workspace tests, including `eml_tls` `platform::` on the macOS verifier (pass). This machine runs Nix with `sandbox = false`.
- [V] `bad_ca.em`, `no_roots.em` and the rewritten `unknown_ca.em`: their snapshots were produced in the scratch clone with `INSTA_UPDATE=always … -E 'test(=ui::run_fail)'` (pass), with the trust built synchronously in the old extern. The `async_op` arm of S1 §5.1 is S1's prototype [V there]; it is not run here.
- [V] `eml_vm` `tls.rs` today has 7 `tests` and 3 `on_tests` (grep), so A's `tls::` lib count is 10.
- [I, not run] Linux nix sandbox build; Windows timings for eml's own tests.

---

