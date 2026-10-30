# 付録 B: テストの driver の終端器、fixtures、テスト

> 付録。本文 (`../2026-10-30-net-tls.md`) の決定が優先する。食い違いは本文の「付録との差分」に挙げた。ここに出てくる [V] は設計の段階で試作して確かめたこと、[I] は推定である。


Features and CI follow S3 r2:
- eml_io `tls = ["dep:eml_tls"]`
- eml_vm `tls = ["dep:eml_tls", "eml_io/tls"]`
- eml_cli `tls = ["run", "eml_vm/tls", "dep:eml_tls"]`, with `default = ["run", "tls"]`
- eml_test_support gets no `tls` feature.
- No crate other than eml_tls dev-depends on eml_tls or rustls.
- CI runs `cargo nextest run -p eml_io -p eml_vm -E 'test(tls_off::)'`.

## 1. Conclusion

### 1.1 Terminator (`eml_io`, feature `tls`)

**Placement**
- New file `crates/eml_io/src/test/tls.rs`, declared in `test.rs` as `#[cfg(feature = "tls")] mod tls;`.
- It is a child of `test`, so it sees net's `pub(super)` items (`Network`, `End.{peer,inbox,closed,shut}`, `Network::{close, shutdown_write}`) and the `pub(crate)` items `virtual_connect` and `Connection`. [V: the prototype compiled]
- `net.rs` gets exactly one new function: `pub(super) fn pair(&mut self) -> io::Result<(IoHandle, IoHandle)>`.
  - It calls `free_port()` and then `connected_ends(port)`.
  - It returns `(connecting end that holds the port, other end)`.
  - Its error is `io::Error::other("no free port")`.
  - `virtual_connect` is changed to call it; this is a mechanical change.
- `connected_ends`, `free_port` and `handle` stay private.

**TestDriver (`test.rs`)**
- Add the field `#[cfg(feature = "tls")] tls: tls::Terminators`, initialized with `Default`.
- `settle()` runs in this order: `empty_bin()`, then `#[cfg(feature = "tls")] self.tls.pump(&mut self.net);`, then the existing single pass over the ops, then the shuffle.
- The `Op::Connect` arm calls a new private method:
  ```rust
  fn connect(&mut self, host: &str, port: u16) -> io::Result<Connection> {
      #[cfg(feature = "tls")]
      if let Some(cut) = tls::tls_host(host) { return self.tls.connect(&mut self.net, port, cut); }
      check_virtual_host(host)?;
      self.net.virtual_connect(port)
  }
  ```
- With the feature off, both names hit `check_virtual_host`, which returns `NotFound "unknown host"`.
- The `Done::Stream` peer is still `localhost(port)`.

**`tls.rs` items**
- `tls_host(host) -> Option<bool>`: `"tls.localhost"` gives `Some(false)`, `"tls-cut.localhost"` gives `Some(true)`, anything else gives `None`.
- `const RECORD: usize = 16 * 1024`.
- `struct Terminator { conn: eml_tls::Conn, cipher: IoHandle, plain: IoHandle, cut: bool, started: bool, eof_pushed: bool, stopped: bool }`
  - `cipher` is the terminator's ciphertext end. Its peer, E_c, is the eml client's `TcpStream`.
  - `plain` is the connecting end of the plaintext pair. Its peer, S_p, is the backlog end that the eml server accepts.
  - `started` means client bytes have been pushed into `conn` at least once.
- `#[derive(Default)] pub(super) struct Terminators(Vec<Terminator>)`. The Vec keeps creation order, so the pump order is deterministic.
- `Terminators::connect(net, port, cut)`:
  1. Call `net.virtual_connect(port)?`. Return `Refused` and `Pending` as they are; a pending connect is retried at each settle.
  2. On `Connected { client_end: plain }`, call `net.pair()`. If it fails, call `net.close(plain)` and return the error.
  3. Create `Conn::server(fixtures::CERT_PEM, fixtures::KEY_PEM).expect("the test certificate")`.
  4. Push the terminator and return `Connected { client_end: E_c }`.
- Ports: `plain` takes the first port and E_c the second.
  - With a listener on 49152, the eml server's `accept`/`peer_addr` shows `127.0.0.1:49153`, and E_c holds 49154.
  - A refused or pending connect takes no port. [V]
- `Terminators::pump(net)`: `retain_mut(|t| { while t.step(net) {} !net.ends[&t.cipher].closed })`.
  - Invariant: every plaintext peer is an end the eml program holds. Listeners are only created by `Op::Listen`, so one terminator never feeds another, and a fixpoint per terminator is the global fixpoint.
  - The invariant goes into a code comment and into the vm.md paragraph (C-3).

**`Terminator::step(net) -> bool`** (`true` = something moved)
- Notation: `c = ends[cipher].peer` (E_c) and `s = ends[plain].peer` (S_p).
- `deliver(net, to, bytes)` extends `to.inbox` unless `to.closed`. It never fails.

0. Drain `ends[cipher].inbox` into a local Vec. Do this even when `stopped`; in that case the bytes are discarded.
1. **Early close.** If `!stopped && !started && s.closed && ends[plain].inbox` is empty: call `stop()` and return `true`. The eml server closed before the terminator got any client byte, so the client sees EOF in mid-handshake (→ `ConnectionReset` in `Std.Tls`).

Steps 2–12 run only when `!stopped`:

2. If the drained bytes are not empty: `conn.push_tls(&bytes)` and set `started = true`.
3. If `c.closed || c.shut` and `!eof_pushed`: `conn.push_eof()` and set `eof_pushed`.
4. Read `st = conn.status()`.
5. If FAILED: `pull_tls` and deliver to c (the alert), then `stop()`, and return `true`.
6. `read_plain` and deliver to s.
7. If (PEER_CLOSED, TRUNCATED or `eof_pushed`) and `plain` is neither shut nor closed: `net.shutdown_write(plain)`.
8. If not HANDSHAKING: move at most `RECORD` bytes from `ends[plain].inbox` to `conn.write_plain`.
9. If not HANDSHAKING, `s.closed || s.shut`, `ends[plain].inbox` is empty, and `cipher` is neither shut nor closed:
   - send `send_close_notify()` unless `cut`;
   - `pull_tls` and deliver to c;
   - `net.shutdown_write(cipher)`.
   - A server close during the handshake waits here until the handshake completes, and then gets close_notify.
10. `pull_tls` and deliver to c.
11. If `s.closed` and `plain` is not closed: `net.close(plain)`. This frees its port.
12. This step always runs. If `c.closed` and `cipher` is not closed: `net.close(plain)` and `net.close(cipher)`, then return `false`. `pump` then drops the terminator.

**`stop()`**
- Sets `stopped`.
- Calls `net.shutdown_write(cipher)`: the client reads EOF and can still write.
- Calls `net.close(plain)`: the server reads what is left, then EOF, and its writes get `BrokenPipe`.

**Divergences from the design text (go into vm.md)**
- **D1. Shut instead of close.** On a server close, on FAILED and on an early close, the ciphertext end is shut, not closed. It is closed only when the client closes E_c.
  - Removal means "removed at the first settle after the client's end is closed". By then both terminator ends are closed.
  - Reason: if the end were closed, the client's close_notify written after it reads EOF fails with `BrokenPipe`. [V: the first prototype run]
  - That would break `Tls.close` after EOF.
  - The client still reads EOF in both cases. [V test.rs:195-200]
  - A client that drops its `Stream` without `Tls.close` closes E_c through the handle bin, and the terminator is removed at the next settle.
- **D2. Early-close rule (step 1).** The design has no rule for "the server closed before the handshake". Its run-fail test needs one, so this rule is added.
  - It depends only on whether any client byte has been pumped. The other behaviors are covered below and in the tests.

**Determinism**
- The pump runs after `empty_bin` and before ops are tried in the same settle. It is a deterministic function of the `Network` state.
- The seed only shuffles `finished`.
- `wait` advances the clock only when nothing finished after the pump.
- Ops that finish during a settle (Read, Readable, Accept, Connect retried after a full backlog, Job) never add bytes to a terminator inbox. Such bytes come only from `submit`.
- [I from test.rs:76-95, 232-245, 247-276; exercised by the prototype]
- **The early-close outcome:**
  - If an Accept is already pending when the client connects, it completes inside a settle, after that settle's pump has pushed the ClientHello. Then step 1 cannot fire. The server's close gives a clean close_notify once the handshake is done, under any scheduler and any task order. [I from test.rs:76-95; the prototype has tests for both sides of the rule]
  - Step 1 fires only if the server accepts and closes, with Accept completing at submit, before the first settle that follows the client's ClientHello write.
  - **Rule for test authors (vm.md and testing.md):** a test whose server closes without writing must either start the acceptor before the client connects (as `async/net_*` do with `spin 40`), or run in a single task like `server_closes_before_handshake.em`. Otherwise the outcome depends on the scheduler.

**`submit` and its immediate `try_finish`**
- A Read on E_c submitted right after a Write is `Pending`. It finishes at the next `wait` (pump, then op). This costs one extra wait round compared with a plain pair; the `immediate` counts are not compared for `tls/`.
- No false deadlock: `wait(None)` settles (including the pump) before it returns `false`. [V test.rs:247-276; the prototype's `read()` helper takes this path]

### 1.2 `eml_tls::Conn` surface used (S1 owns it)
- **Terminator:**
  - `Conn::server(&[u8], &[u8]) -> Result<Conn, _>`;
  - `push_tls(&[u8])`, `push_eof()`, `pull_tls(&mut Vec<u8>)`, `write_plain(&[u8])`, `read_plain(&mut Vec<u8>)`, `send_close_notify()`;
  - the `status()` bits HANDSHAKING=1, PEER_CLOSED=4, TRUNCATED=8, FAILED=16.
- **Tests:** `Conn::client(&str, &[u8])`.
- Status reads go through 3 private predicates (`handshaking`, `failed`, `peer_done`), so a change in how the status is represented stays local.
- If `write_plain` returns an accepted count, step 8 pushes the rest back to the front of the inbox.
  - The 16 KiB chunk followed by `pull_tls` stays within rustls's default 64 KiB send limit.
  - [V: `write_all` of 200 KiB under the default limit fails with WriteZero]
- **Required:**
  - `pull_tls` after FAILED yields the alert;
  - `push_eof` on a server Conn must not panic.
- **Optional:** `send_tls13_tickets = 0` in `Conn::server`.

### 1.3 Fixtures

**Files**
- `crates/eml_tls/fixtures/ca.pem`: self-signed CA, ECDSA P-256.
- `crates/eml_tls/fixtures/cert.pem`: the leaf only.
- `crates/eml_tls/fixtures/key.pem`: the leaf key, PKCS#8.
- `crates/eml_tls/fixtures/gen.sh`.
- The CA key is not kept.

**Module `crates/eml_tls/src/fixtures.rs`**
- Exposed as `pub mod fixtures`, always compiled. It is not `cfg(test)`, because the non-test `TestDriver` uses it.
- Contents: `pub const CA_PEM: &[u8] = include_bytes!("../fixtures/ca.pem");`, `CERT_PEM`, `KEY_PEM`.
- **Decision (B-4).** The key and the certificate are public constants and are linked into `eml`. `Machine` can build a `TestDriver` (`Clock::Virtual`), so they are reachable. [V eml_vm/src/driver.rs:25]
  - This is accepted: the key authenticates nothing. Its CA is trusted only by a program that passes `ca.pem` explicitly to `Tls.connect_with_ca`.
  - The module's doc comment states this: テスト専用、何も信頼しない.

**Certificate contents** [V: `openssl x509 -text`; `openssl verify` OK; a rustls handshake succeeds for all 3 names, and `example.com` fails with NotValidForName]
- CA: `CN=eml test CA`; `basicConstraints=critical,CA:TRUE`; `keyUsage=critical,keyCertSign,cRLSign`.
- Leaf: `CN=tls.localhost`; `basicConstraints=critical,CA:FALSE`; `keyUsage=critical,digitalSignature`; `extendedKeyUsage=serverAuth`; `subjectAltName=DNS:tls.localhost,DNS:localhost,IP:127.0.0.1`.
- Both: ecdsa-with-SHA256; notBefore 2026-01-01T00:00:00Z; notAfter 2126-01-01T00:00:00Z.

**gen.sh**
- Needs OpenSSL 3.4 or later for `-not_before`/`-not_after`. The host has 3.0.13 [V]; nixpkgs has 3.5.9 [V].
- Run: `nix shell nixpkgs#openssl -c crates/eml_tls/fixtures/gen.sh`.
- Every run makes new keys; commit all outputs together.
- `std-http` adds its two `http` dirs to the `for` list.
- Exact body (comments in Japanese, why-only, to be added; `EXT` must start at column 0):

```sh
#!/bin/sh
set -eu
dir=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$dir/../../.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$tmp/ca.key"
openssl req -x509 -new -key "$tmp/ca.key" -sha256 -subj "/CN=eml test CA" \
  -not_before 20260101000000Z -not_after 21260101000000Z \
  -addext "basicConstraints=critical,CA:TRUE" \
  -addext "keyUsage=critical,keyCertSign,cRLSign" \
  -out "$dir/ca.pem"
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$dir/key.pem"
openssl req -new -key "$dir/key.pem" -subj "/CN=tls.localhost" -out "$tmp/leaf.csr"
cat > "$tmp/leaf.ext" <<EXT
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature
extendedKeyUsage=serverAuth
subjectAltName=DNS:tls.localhost,DNS:localhost,IP:127.0.0.1
EXT
openssl x509 -req -in "$tmp/leaf.csr" -CA "$dir/ca.pem" -CAkey "$tmp/ca.key" -sha256 \
  -not_before 20260101000000Z -not_after 21260101000000Z \
  -extfile "$tmp/leaf.ext" -out "$dir/cert.pem"
for copy in "$root/tests/ui/run/tls" "$root/tests/ui/run-fail/tls"; do
  mkdir -p "$copy"
  cp "$dir/ca.pem" "$copy/ca.pem"
done
```

[V: this exact script ran with exit 0 under `nix shell nixpkgs#openssl`; it prints `Certificate request self-signature ok` / `subject=CN=tls.localhost` on stderr]

**CA copies for the UI tests**
- One copy per category: `tests/ui/run/tls/ca.pem` and `tests/ui/run-fail/tls/ca.pem`.
- The UI host's file root is the directory that holds the test file. [V ui.rs:111-124, 144-158; hosts.rs:105]
- Category-level input files have precedent: `tests/ui/run/kinds/lines.txt`.
- `for_each_file` collects only `.em`. [V common/mod.rs:65-72]
- Programs read the file with `Fs.read_text "ca.pem"`. The design's `FileIn.read_all "ca.pem"` does not type-check (`FileIn.read_all : FileIn -> <IO> (String, FileIn)`). [V std/FileIn.em:10, std/Fs.em:8]

**Copy-identity test** (in `eml_tls`)
- Walk `<CARGO_MANIFEST_DIR>/../../tests/ui` recursively.
- Every file named `ca.pem` must be byte-equal to `fixtures::CA_PEM`.
- `run/tls/ca.pem` and `run-fail/tls/ca.pem` must exist.
- `.gitattributes` is `* -text`.

### 1.4 Tests

**(a) `eml_io` Rust tests**
- File: `crates/eml_io/tests/tls.rs`, declared in `tests/main.rs` as `#[cfg(feature = "tls")] mod tls;`.
- Local helpers: `raw`, `next_key`, `run_op`, `settle`, `read` (submit; if Pending, settle once and assert exactly that key finished), `write`, `flush` (`pull_tls` then Op::Write), `handshake` (flush; then, while HANDSHAKING and not FAILED: read → `push_tls` → flush; an empty read does `push_eof` and stops).
- [V: all 9 pass, 0.04 s in total; the slowest single test takes 0.06 s alone, mostly the one-time aws-lc init]

1. `a_tls_connect_reaches_the_plaintext_listener`:
   - the connect peer is `127.0.0.1:<listener port>`;
   - the accept peer is `127.0.0.1:49153`;
   - after the handshake, the server reads `hello`.
2. `a_large_body_and_close_notify_reach_the_client`:
   - the server writes 200 KiB and closes;
   - the client gets 204800 bytes and the status bits are exactly PEER_CLOSED;
   - the client then writes close_notify successfully;
   - after the client closes and a settle, `listen(49153)` and `listen(49154)` succeed.
3. `a_half_close_lets_the_server_reply`:
   - the client sends `req`, close_notify and `shutdown_write`;
   - the server reads `req`, then EOF, then writes `done` and closes;
   - the client gets `done` and PEER_CLOSED.
4. `the_cut_host_closes_without_close_notify`: the client gets `partial` with TRUNCATED and without PEER_CLOSED.
5. `a_rejected_certificate_closes_the_plaintext_end`: with `Conn::client(.., b"")` the client hits FAILED; after the alert is flushed and a settle, both the server and the client read EOF.
6. `a_server_close_before_any_client_byte_gives_eof`: accept, close the server end, then flush the ClientHello. The client reads 0 bytes and is still HANDSHAKING.
7. `a_server_close_during_the_handshake_waits_for_it` (A-1):
   - the ClientHello is pumped and the flight is read;
   - then accept and close the server end;
   - the client finishes the handshake and reads to EOF with PEER_CLOSED.
8. `a_client_close_leaves_the_server_its_bytes_then_eof` (C-1, C-2):
   - the client writes `req` and closes without close_notify;
   - the server reads `req`, then EOF;
   - a server write gets `BrokenPipe`;
   - `listen(49153)` and `listen(49154)` succeed, so the terminator released both ports.
9. `a_tls_name_without_a_listener_is_refused`: `ConnectionRefused`.

**Feature-off test (B-2, following S3's `tls_off` rule)**
- File: `crates/eml_io/tests/tls_off.rs`, declared in `tests/main.rs` as `#[cfg(not(feature = "tls"))] mod tls_off;`. It has no OS gate.
- It has its own `run_op`. Test `the_tls_names_are_not_found`: listen on port 0, then Connect to `tls.localhost` and to `tls-cut.localhost`; each is `ErrorKind::NotFound`. [V: passes under `cargo test -p eml_io`; clippy is clean with tls off and on]
- CI runs it through S3's step `cargo nextest run -p eml_io -p eml_vm -E 'test(tls_off::)'`. That step lands in the same commit as the first `tls_off` module, whichever of S2-2 (eml_io) or S1's eml_vm task comes first.
- The workspace `cargo nextest run` compiles the module out, because eml_cli's default turns on `eml_io/tls`. [V: unification experiment in exp-S2/unify]

**(b) Real-socket test in `eml_cli`**
- File: `crates/eml_cli/tests/tls.rs`, declared as `#[cfg(all(unix, feature = "tls"))] mod tls;`.
- Why unix: the Windows real driver has no sockets.
- It uses `eml_tls` through eml_cli's optional dependency (`dep:eml_tls` in `tls`, per S3 A3). There is no dev-dependency.
- **Server thread** (`eml_tls::Conn::server` over a blocking `std::net::TcpStream`):
  1. Bind `127.0.0.1:0` and accept.
  2. Set `set_read_timeout(5 s)`, so a stuck client fails the test instead of hanging.
  3. Loop until the plaintext contains `\n`: `pull_tls` → `write_all`, `read` → `push_tls` → `read_plain`, and assert not FAILED.
  4. `write_plain(b"pong\n")`, `send_close_notify`, `pull_tls` → `write_all`.
  5. Read to EOF and return the line.
- **Client** (adjust to S1's API at consolidation):
  - Compile with `Session::load("tls.em", &src, &MemorySource(&[])).compile()`.
  - Run with `eml_cli::execute(&program, &RunConfig::default().with_clock(Clock::Real).with_control(Arc::new(Control::new(eml_cli::default_limits()))), host)`, where `host = capture_host(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../eml_tls/fixtures"))`.
  ```haskell
  import Async
  import Bytes
  import Fs
  import Tls
  main : Unit -> <IO, Panic> Unit
  main () = Async.run (fn () ->
    let s = Tls.connect_with_ca (Fs.read_text "ca.pem") "localhost" {port}
    let s = Tls.write_line "ping" s
    let (line, s) = Tls.read_line s
    match line with
      | Some text -> println "Some \{text}"
      | None -> println "None"
    let (rest, s) = Tls.read_chunk (Bytes.empty ()) s
    println (show (Bytes.length rest))
    Tls.close s)
  ```
- Expected: stdout `Some pong\n0\n`, the server returns `ping\n`, and the run is `Ok`.
- **Nix sandbox:**
  - Loopback works and `localhost` resolves to 127.0.0.1 only. [V: runCommand probe]
  - The real driver falls through the resolved addresses. [V driver/mod.rs:375-405]
  - `src = ./.`, so the build tree contains `crates/eml_tls/fixtures` and `tests/ui`. [V flake.nix:102]
  - The relative path from `CARGO_MANIFEST_DIR` holds because buildRustPackage builds in the unpacked tree. [I]
  - No `checkFlags` entry.
  - If the S2-4 `nix build` check fails only on the fixture path: write `eml_tls::fixtures::CA_PEM` into a `std::env::temp_dir()` subdirectory unique to the test, use that as the file root, and remove it at the end.

**(c) UI tests** (single file each, `Async.run`, no Windows mark)
- Not seeded: `seeded()` covers only `par/` and `async/`. [V ui.rs:275-277]
- Each file runs in checked mode (Sequential), then fast mode (Normal), and they compare no RunStats. [V ui.rs:258-268]
- Prints must be causally ordered, or come from one task.
- Every server that closes without writing has its acceptor already pending: start the acceptor task, then `spin 40` (the `async/net_*` helper), then connect. The one exception is the single-task run-fail file.
- Options are printed by explicit `match`, as in `Some \{line}`, so the expected output does not depend on `show`.

**Placeholders in the expected output (B-3)**
- `<S1:verb>`: S1's helper or function name, where it differs from the design's surface (`connect_with_ca`, `wrap`, `read_chunk`, `read_line`, `write_line`, `shutdown_write`, `close`).
- `<msg:cert>`, `<msg:truncated>` and `<msg:handshake-eof>`: S1's final messages for a rejected certificate, EOF without close_notify, and EOF during the handshake.
- Everything else below is final.

`tests/ui/run/tls/` (plus `ca.pem`); expected stdout, `--- stderr ---` empty:

| file | program | stdout |
|---|---|---|
| `echo.em` | Server task: accept, close the listener, print `server: peer \{addr}` from `Net.peer_addr`, `Net.read_line`, then `Net.write_line` of the line back, then `Net.close`. Client: `Tls.connect_with_ca ca "tls.localhost" port`, `Tls.write_line "hello"`, `Tls.read_line`, print, `Tls.close`, await the server. | `server: peer 127.0.0.1:49153` / `client: Some hello` |
| `large_body.em` | Server: `Net.write_bytes (Bytes.make 204800 120u8)`, then `Net.close`. Client: sum the lengths from `Tls.read_chunk` until an empty read; print the sum. | `204800` |
| `close_notify_eof.em` | Server: `Net.write "bye"`, then `Net.close`. Client: read and print `String.decode`; read an empty chunk and print `eof`; read again and print `eof`; `Tls.close`; print `closed`. | `bye` / `eof` / `eof` / `closed` |
| `read_line.em` | Server: `Net.write "one\r\ntwo\nthree"`, then `Net.close`. Client: call `Tls.read_line` until `None`, printing `Some \{l}` or `None`. | `Some one` / `Some two` / `Some three` / `None` |
| `shutdown_write.em` | Client: `Net.connect "tls.localhost" port`, `Tls.wrap "tls.localhost"`, `Tls.write "req"`, `Tls.shutdown_write`. Server: read until EOF, print `server: \{text}`, `Net.write "done"`, `Net.close`. Client: read until EOF, print `client: done`, `Tls.close`. | `server: req` / `client: done` |

`tests/ui/run-fail/tls/` (plus `ca.pem`):
- Each file wraps the failing call in `try_io`, prints `show e.kind`, then calls `fail_io e.kind e.message`.
- Snapshot shape: `--- stdout ---\n<stdout>--- stderr ---\n--- runtime error ---\n<msg>\n  at run-fail/tls/<file>.em:<line of fail_io>:<col>\n`.

| file | program | stdout | `<msg>` |
|---|---|---|---|
| `unknown_ca.em` | Listen and never accept; `Tls.connect "tls.localhost" port`. | `Other` | `<msg:cert>` (the design forbids rustls details; the roots are webpki-roots, fixed by the design) |
| `truncated.em` | Acceptor pending (spin 40). Server: `Net.write "partial"`, `Net.close`. Client on `tls-cut.localhost`: read and print `partial`; the next read fails. | `partial` / `ConnectionReset` | `<msg:truncated>` |
| `server_closes_before_handshake.em` | A single task: `Net.listen`, `Net.connect "tls.localhost" port` (finishes at submit), `Net.accept l` (finishes at submit), `Net.close s`, `Net.close_listener l`, `Tls.wrap "tls.localhost" c`. | `ConnectionReset` | `<msg:handshake-eof>` |

**Timing**
- At opt-level 1, the first handshake in a process takes 42–63 ms (one-time init). Later ones take 0.41–0.50 ms with fresh configs. 200 KiB moves in 0.15–0.5 ms. [V]
- `Net.read_chunk` reads up to 64 KiB, so 200 KiB is 4 reads. [V]
- The eml-side cost was not measured. [I]
- **Fallback (C-4):** if `large_body.em` takes more than 1 s alone in checked + fast mode, lower the body to `70000`. That still spans two 64 KiB reads and five records. Change the program and the expected stdout in the same commit.

**Windows:** no marks. S3's rule switches the provider (or stops the stage) on any OS's build failure, so there is no `eml` without tls on Windows.

## 2. Rationale
- **Child module of `test`:** gives the terminator access to the network with one added function.
- **Pump after `empty_bin` and before ops:** the existing determinism argument holds unchanged.
- **Shut, not close (D1):** keeps `Tls.close` after EOF working.
- **Early-close keyed on "no client byte yet" (D2):** gives one answer for every task order in which the acceptor is pending. The alternative, "still handshaking", flips with whether the client's Finished has already been pumped (A-1).
- **Public fixtures:** they are inert test material, and `TestDriver` is non-test code.

## 3. Alternatives rejected
- **Terminator outside `test/`:** would need `End`/`ends` made `pub(crate)`.
- **Pumping in `submit`:** a second place where the network moves; it changes which ops finish at submit.
- **A single pump pass:** the handshake and close need several rounds per settle.
- **Abort while still handshaking (r1 step 5):** order-dependent (A-1).
- **rcgen as generator:** needs a bin or a dev-dependency.
- **A `ca.pem` per test dir:** more copies.
- **rustls/eml_tls dev-dependency in eml_cli:** breaks the cross clippy (S3 rule).
- **A feature-gated fixtures module:** another feature for inert data.
- **A ui.rs `needs_tls` gate:** no supported run builds the UI runner with `run` but without `tls` (S3: unsupported, stated in testing.md).

## 4. Assumptions
- **A1.** S1 keeps the sketched `Conn` methods and status bits. If not, adapt the predicates and calls in `tls.rs` only.
- **A2.** S1's `Tls` maps EOF without close_notify, including mid-handshake, to `ConnectionReset`, and certificate rejection to `Other`, with messages that carry no rustls detail (design). If not, the run-fail stdout and messages change at consolidation.
- **A3.** S3 r2's wiring: eml_cli `tls` includes `dep:eml_tls` (S3 A3).
- **A4.** S2-3 is part of S1's `std/Tls.em` commit: the tests are written first, and the expected values are taken from S1's final messages.

## 5. Tasks (commit-sized)

Order: S3-T1 → **S2-1** → S1 `Conn` → S3-T2 (features; R2: before any `cfg(feature = "tls")`) → **S2-2** (+ S3-T3 if it is the first `tls_off`) → S1 VM + `std/Tls.em` (contains **S2-3**) → **S2-4**.

No existing test's expected value or pass/fail changes; only tests are added.

### S2-1 Fixtures and gen.sh
- **Files:**
  - `crates/eml_tls/fixtures/{gen.sh,ca.pem,cert.pem,key.pem}`;
  - `crates/eml_tls/src/fixtures.rs` + `pub mod fixtures;`;
  - `tests/ui/{run,run-fail}/tls/ca.pem`;
  - `crates/eml_tls/tests/fixtures.rs` (in eml_tls's integration binary).
- **Tests first:**
  - `the_fixtures_parse`: via `rustls_pki_types::pem::PemObject`, CA and CERT hold one certificate each, and KEY is PKCS#8;
  - `every_ca_copy_is_the_fixture`.
- **Completion:**
  - `nix shell nixpkgs#openssl -c crates/eml_tls/fixtures/gen.sh` exits 0;
  - `nix shell nixpkgs#openssl -c openssl verify -CAfile crates/eml_tls/fixtures/ca.pem crates/eml_tls/fixtures/cert.pem` prints `crates/eml_tls/fixtures/cert.pem: OK`;
  - `nix shell nixpkgs#openssl -c openssl x509 -in crates/eml_tls/fixtures/cert.pem -noout -enddate -ext subjectAltName` prints `notAfter=Jan  1 00:00:00 2126 GMT` and `DNS:tls.localhost, DNS:localhost, IP Address:127.0.0.1`;
  - `cargo nextest run -p eml_tls` passes.

### S2-2 Terminator
- **Files:**
  - `crates/eml_io/src/test.rs`, `src/test/net.rs`, new `src/test/tls.rs`;
  - `crates/eml_io/tests/main.rs`, new `tests/tls.rs` (9 tests), new `tests/tls_off.rs`;
  - `crates/eml_tls/tests/fixtures.rs`: `the_leaf_is_valid_for_each_name` (3 names succeed, `example.com` fails), unless S1 already has it.
- **Docs:**
  - **`docs/spec/vm.md` 「時計と driver」**, one paragraph covering:
    - the two names exist only with `tls` (otherwise NotFound);
    - the plaintext connection to `localhost:port` with Refused/pending as for `connect`;
    - the two ports and the one shown by `peer_addr`;
    - the pump at the start of each settle after the handle bin, to a fixpoint;
    - the invariant that the plaintext peer is always held by the program;
    - client EOF/close → server EOF, then close;
    - server shut/close → close_notify (none for tls-cut), then the ciphertext end is shut;
    - a server close during the handshake waits for its completion;
    - early close (no client byte yet) → EOF without TLS bytes;
    - FAILED → alert, then shut/close;
    - removal at the settle after the client's end closes;
    - the test certificate in `eml_tls`'s `fixtures` (inert), with its CA copied by `gen.sh`.
  - **`docs/implementation/architecture.md` 「`eml_io` の内部」:** `test/tls.rs` and the test files.
- **Completion:**
  - `cargo nextest run -p eml_io --features tls -E 'test(tls::)'` passes 9;
  - `cargo nextest run -p eml_io -E 'test(tls_off::)'` passes 1;
  - `cargo nextest run -j 1 -p eml_io --features tls` reports no SLOW;
  - `cargo clippy -p eml_io --all-targets -- -D warnings` and `cargo clippy -p eml_io --features tls --all-targets -- -D warnings` are clean;
  - the workspace `cargo nextest run` passes.

### S2-3 UI tests (inside S1's `std/Tls.em` commit, written before `Tls.em`)
- **Files:**
  - the 5 + 3 `.em` in §1.4 (c);
  - snapshots `crates/eml_cli/tests/snapshots/integration__ui__run@tls__<name>.em.snap` / `integration__ui__run_fail@tls__<name>.em.snap`;
  - `docs/implementation/testing.md` 「UI テスト」: category `tls/` (the test driver's `tls.localhost`, the category's `ca.pem` written by `gen.sh`, not seeded and why, and the acceptor-pending rule for servers that close without writing). Coordinate with S3.
- **Completion:**
  - each snapshot's stdout equals the §1.4 (c) table, with placeholders filled from S1;
  - `EML_UI_FILTER=/tls/ cargo nextest run -p eml_cli --test integration -E 'test(ui::)'` passes;
  - `EML_UI_FILTER=tls/large_body cargo nextest run -j 1 -p eml_cli --test integration -E 'test(ui::run)'` reports the test under 1 s; otherwise apply the C-4 fallback.

### S2-4 Real-socket test
- **Files:** `crates/eml_cli/tests/tls.rs` and `tests/main.rs`.
- **Completion:**
  - `cargo nextest run -p eml_cli --test integration -E 'test(tls::)'` passes in under 1 s;
  - `cargo clippy -p eml_cli --no-default-features --features run --all-targets -- -D warnings` is clean;
  - `nix build` succeeds (it runs the test in the sandbox; the fallback is in §1.4 (b)).

## 6. Unresolved items (with defaults)
- **U1.** Generator: openssl ≥ 3.4 from nixpkgs. Alternative: `-days 36525`.
- **U2.** Decided: no ui.rs gate (B-1).
- **U3.** eml_tls test layout follows S1/S3's convention.
- **(r1 U4 withdrawn.)** It was wrong. The workspace dependency is `eml_cli = { path = "crates/eml_cli", default-features = false }` [V Cargo.toml:21], and eml_test_support enables only `run`, so the ASan/Miri `-p eml_vm` runs do not reach eml_tls (S3 verified this with `cargo tree`).

## 7. Additional checks
- **Read:** test.rs, net.rs, ui.rs, common/mod.rs, hosts.rs, Fs/FileIn/Net.em, the Cargo manifests, flake.nix:99-116, the workspace Cargo.toml:21-22, and the `net_read_line` snapshot (print style). [V]
- **Prototype (exp-S2/repo):**
  - 9 tls tests pass;
  - the `tls_off` test passes with the feature off;
  - clippy is clean with the feature on and off;
  - the revised early-close rule (step 1) and the deferred close during the handshake are both exercised. [V]
- **Feature unification** (workspace on, `-p` off). [V]
- **Fixtures:** verified with openssl 3.5.9 and in rustls handshakes. [V]
- **Nix sandbox:** loopback and localhost. [V]
- No background tasks.
