# 付録 A: eml_tls、eml_vm、std/Tls.em、spec

> 付録。本文 (`../2026-10-30-net-tls.md`) の決定が優先する。食い違いは本文の「付録との差分」に挙げた。ここに出てくる [V] は設計の段階で試作して確かめたこと、[I] は推定である。


## 1. Conclusion

### 1.1 `eml_tls` (fills the crate whose skeleton S3's T1 creates)

Dependencies are S3's workspace lines: `rustls = { version = "0.23.46", default-features = false, features = ["aws_lc_rs","std","tls12"] }` and `webpki-roots = "1.0.11"`. PEM is parsed through `rustls::pki_types::pem::PemObject`.

Public API:

```rust
pub const HANDSHAKING: u32 = 1;   // rustls is_handshaking()
pub const HAS_OUTGOING: u32 = 2;  // rustls wants_write()
pub const PEER_CLOSED: u32 = 4;   // close_notify received (IoState::peer_has_closed)
pub const TRUNCATED: u32 = 8;     // push_eof seen without close_notify (also mid-handshake)
pub const FAILED: u32 = 16;       // a failure was recorded

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error { ServerName, CaPem, CertPem, KeyPem, Config }   // setup only; Display = fixed texts below

pub mod fixtures;  // S2: pub const CA_PEM, CERT_PEM, KEY_PEM: &[u8]; always compiled (decision B-1)

pub struct Conn {
    tls: rustls::Connection,
    plain: Vec<u8>, start: usize,     // unread plaintext = plain[start..]
    failure: Option<Failure>,         // private enum Failure { Rejected, Tls(rustls::Error), Io(io::Error) }
    eof: bool, peer_closed: bool,
}
impl Conn {
    pub fn client(server_name: &str, extra_ca_pem: &[u8]) -> Result<Conn, Error>;
    pub fn server(cert_chain_pem: &[u8], key_pem: &[u8]) -> Result<Conn, Error>; // test driver terminator and tests only
    pub fn push_tls(&mut self, bytes: &[u8]);
    pub fn push_eof(&mut self);
    pub fn pull_tls(&mut self, out: &mut Vec<u8>);      // every queued record, including the alert after FAILED
    pub fn write_plain(&mut self, bytes: &[u8]);        // ignored after a failure
    pub fn plaintext(&self) -> &[u8];                   // the VM appends and finds lines in place
    pub fn consume(&mut self, n: usize);                // frees the Vec when it empties
    pub fn read_plain(&mut self, out: &mut Vec<u8>) -> usize; // extend + consume(all); for S2's terminator
    pub fn status(&self) -> u32;
    pub fn error_message(&self) -> String;              // "" when not failed
    pub fn send_close_notify(&mut self);                // idempotent; no-op after a fatal alert
}
```

Internals:
- **`client`.**
  - Name: `ServerName::try_from(name.to_owned())`; an error gives `ServerName`.
  - Roots: `RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() }`, plus `roots.add(der)` for each `CertificateDer::pem_slice_iter(extra_ca_pem)` item. A PEM error, an `add` error, or a non-empty input with 0 certificates gives `CaPem`.
  - Config: `ClientConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider())).with_safe_default_protocol_versions()?.with_root_certificates(roots).with_no_client_auth()`, so TLS 1.2 and 1.3. An error here, or from `ClientConnection::new`, gives `Config`.
  - `install_default` is never called. A new config is built per connection.
- **`server`.** The chain comes from `pem_slice_iter`; an error or an empty chain gives `CertPem`. The key comes from `PrivateKeyDer::from_pem_slice`; an error gives `KeyPem`. The builder is the same, then `.with_no_client_auth().with_single_cert(..)`; an error there gives `KeyPem`.
- **Both constructors** call `tls.set_buffer_limit(None)`, so `write_plain` never writes short.
- **`push_tls`** [V]:
  ```rust
  while !bytes.is_empty() && self.failure.is_none() && !self.peer_closed {
      match self.tls.read_tls(&mut bytes) { Ok(0) => break, Ok(_) => {}, Err(e) => { self.failure = Some(Failure::Io(e)); break } }
      match self.tls.process_new_packets() {
          Ok(state) => { self.drain(); if state.peer_has_closed() { self.peer_closed = true } }
          Err(e) => { self.drain(); self.failure = Some(match e { rustls::Error::InvalidCertificate(_) => Failure::Rejected, e => Failure::Tls(e) }) }
      }
  }
  // drain: compact plain (drain(..start), start = 0), then reader().fill_buf()/consume until empty, appending to plain.
  ```
  Compaction keeps the bytes of `plaintext()` unchanged, so an offset into `plaintext()` (`line_from`, 1.2) stays valid across pushes.
- **`push_eof`** sets `eof = true` and nothing else. `TRUNCATED` is `eof && !peer_closed`.
- **`pull_tls`** is `while tls.wants_write() { let _ = tls.write_tls(out); }`.
- **After `send_close_notify`**, `write_plain` still makes records (rustls does not check) [V test 20]. Writing after `shutdown_write` therefore reaches the socket and fails there (1.3).
- **Setup texts:**

  | variant | text |
  |---|---|
  | `ServerName` | "the server name is not valid" |
  | `CaPem` | "the CA certificates are not valid PEM" |
  | `CertPem` | "the certificate chain is not valid PEM" |
  | `KeyPem` | "the private key is not valid PEM" |
  | `Config` | "the TLS configuration is not valid" |

- **Failure texts (`error_message`):** `Rejected` (any `InvalidCertificate`) gives "the server certificate was rejected", with no rustls detail. `Tls(e)` and `Io(e)` give `e.to_string()`.
- **Measured** (dev profile, opt-level 1) [V]:
  - `Conn::client` takes 27 µs, and 25 µs with the extra CA.
  - A full in-memory handshake takes 0.70 ms.
  - `size_of::<Conn>() == 1272`, `align_of == 8`, so `alloc_foreign`'s const assert (alignment ≤ 8) holds and the value is kept inline.

### 1.2 eml_vm side

- **Features (S3):** `crates/eml_vm/Cargo.toml` gets `eml_tls = { workspace = true, optional = true }` and `[features] tls = ["dep:eml_tls", "eml_io/tls"]`. There is no dev-dependency on `eml_tls`.
- **`crates/eml_vm/src/tls.rs`**, declared `mod tls;` unconditionally:
  - `#[cfg(feature = "tls")] use eml_tls::{Conn, PEER_CLOSED};` and `const SETUP_KIND: i64 = externs::IO_OTHER;`.
  - `#[cfg(not(feature = "tls"))] mod off`, imported the same way:
    - `pub(crate) enum Conn {}`, uninhabited, with the same method signatures, each with the body `match *self {}`;
    - `fn client(_: &str, _: &[u8]) -> Result<Conn, NotAvailable>` that always returns `Err(NotAvailable)`, where `NotAvailable`'s Display is `not available`;
    - `const PEER_CLOSED: u32 = 4`;
    - `const SETUP_KIND: i64 = externs::IO_NOT_AVAILABLE`.
  - `vm.rs` has no cfg. Clippy with `-D warnings` is clean both ways [V].
  - The payload of the `Foreign` (lin bit set):
    ```rust
    pub(crate) struct TlsConn { conn: Conn, line_from: usize }
    impl TlsConn { fn consume(&mut self, n: usize) { self.conn.consume(n); self.line_from = 0; } }
    ```
    **Invariant:** `plaintext()[..line_from]` contains no `\n`. Every consumption goes through `TlsConn::consume` (called from `read_plain` and `take_line` only), which resets `line_from` to 0. `push_tls` only appends to the view, so it keeps the invariant. A `\r` at `line_from - 1` is still seen, because `find_line` checks `pending[end - 1]` on the whole slice [V test `a_crlf_split_across_pushes_is_one_end`].
  - Line search, compiled in both configurations [V]:
    ```rust
    #[derive(Debug, PartialEq, Eq)]
    pub(crate) enum Line { More, Ready(usize), TooLong, NotUtf8 }
    pub(crate) fn line_state(pending: &[u8], from: usize, eof: bool) -> (Line, usize) {
        match cells::find_line(pending, from, eof) {
            Some(Found::Line(n)) => (if std::str::from_utf8(&pending[..n]).is_ok() { Line::Ready(n) } else { Line::NotUtf8 }, from),
            Some(Found::TooLong) => (Line::TooLong, from),
            None => (if cells::open_line_len(pending) > LINE_LIMIT { Line::TooLong } else { Line::More }, pending.len()),
        }
    }
    ```
- **`cells.rs` (T2):** make `find_line(pending: &[u8], from: usize, eof: bool) -> Option<Found>` and `open_line_len(pending: &[u8]) -> usize` free `pub(crate)` functions, and have the `NetStream` methods delegate to them. Make `Found`, `LINE_LIMIT`, `READ_ROOM` and `#[cfg(test)] test_ctx` `pub(crate)`.
- **`crates/eml_extern/src/lib.rs`:**
  - Type row `TlsConn => type_row("Std.Tls.Conn", Lin, Repr::Obj)` goes after `NetTcpStream`.
  - The function rows go after `NetSetNodelay`. None of them are in `is_async()`, so they run in `run_extern`'s generic match, with arms placed before `Extern::FailIo`.

| row | std type | table expression |
|---|---|---|
| `TlsClientRaw` | `String -> String -> <IO> Conn` | `kinded("Std.Tls.client_raw", &[Obj, Obj], &[Unr, Unr], Obj, Lin, Purity::Effectful)` |
| `TlsPushTls` | `Bytes -> Conn -> <IO> Conn` | `writing("Std.Tls.push_tls")` |
| `TlsPushEof` | `Conn -> <IO> Conn` | `kinded("Std.Tls.push_eof", &[Obj], &[Lin], Obj, Lin, Purity::Effectful)` |
| `TlsPullTls` | `Conn -> <IO> (Bytes, Conn)` | `borrowing(&[true], FunctionRow { with_arg: Some(0), ..kinded("Std.Tls.pull_tls", &[Obj], &[Lin], Obj, Unr, Purity::Effectful) })` |
| `TlsWritePlain` | `Bytes -> Conn -> <IO> Conn` | `writing("Std.Tls.write_plain")` |
| `TlsReadPlain` | `Bytes -> Conn -> <IO> (Bytes, Conn)` | `borrowing(&[false, true], FunctionRow { with_arg: Some(1), ..kinded("Std.Tls.read_plain", &[Obj, Obj], &[Unr, Lin], Obj, Unr, Purity::Effectful) })` |
| `TlsStatus` | `Conn -> <IO> (Int, Conn)` | `borrowing(&[true], FunctionRow { with_arg: Some(0), ..kinded("Std.Tls.status", &[Obj], &[Lin], Int, Unr, Purity::Effectful) })` |
| `TlsErrorMessage` | `Conn -> <IO> (String, Conn)` | as `TlsStatus`, name `Std.Tls.error_message`, ret `Obj` |
| `TlsSendCloseNotify` | `Conn -> <IO> Conn` | as `TlsPushEof`, name `Std.Tls.send_close_notify` |
| `TlsLineReady` | `Conn -> <IO> (Int, Conn)` | as `TlsStatus`, name `Std.Tls.line_ready` |
| `TlsTakeLine` | `Conn -> <IO> (String, Conn)` | as `TlsErrorMessage`, name `Std.Tls.take_line` |

Arms. Arguments are read with `task.get(arg(i))`. Word results are stored with `task.set(dst, w)`, Int results with `task.set_raw(dst, v)`. Each arm calls a `pub(crate) unsafe fn` in `tls.rs`.

| row | what the arm does |
|---|---|
| `client_raw` | `copy_string` the host and the PEM, then `Conn::client`. On `Ok`, `alloc_foreign(TlsConn { conn, line_from: 0 }, true)`. On `Err(e)`, return `Err(VmFault::Extern(Fault::Raised(Box::new(Raised { kind: SETUP_KIND, message: format!("cannot set up TLS for {host}: {e}").into() }))))`. This is the only row that raises. |
| `push_tls`, `write_plain` | `&*ctx.heap().bytes(bytes)?` and `foreign_unique::<TlsConn>(conn)?`, then the call. Return the conn word. |
| `push_eof`, `send_close_notify` | Call, then return conn. |
| `pull_tls` | Pull into a fresh `Vec`, then `alloc_bytes`. |
| `read_plain` | `n = min(plaintext().len(), READ_ROOM)`. If `n == 0`, return `buf` unchanged. Otherwise `bytes_extend(buf, &plaintext()[..n])?`, then `TlsConn::consume(n)`. |
| `status` | `i64::from(status())`. |
| `error_message` | `fresh_string(&error_message())`. |
| `line_ready` | `line_state(plaintext(), line_from, status & PEER_CLOSED != 0)`. Store the returned `from` into `line_from`, then map More / Ready / TooLong / NotUtf8 to 0 / 1 / 2 / 3. |
| `take_line` | `line_state` must give `Ready(n)`; otherwise `VmFault::Internal("Std.Tls.take_line runs without a ready line")`. Then `fresh_string(&plaintext()[..n])` (line end included; `""` at EOF with nothing left), then `TlsConn::consume(n)`. |

**Memory that `max_bytes` does not count.** These are Rust allocations outside the heap pages: `Conn`'s plaintext, rustls's buffers, and the `Vec` that `pull_tls` fills. The 1272-byte box is counted. The uncounted part has these bounds:

- **Plaintext.** `fill` pushes one `Net.read_chunk` (≤ 65 536 bytes of ciphertext) and runs only when the eml code needs more:
  - `read_chunk`: when no plaintext is buffered, so at most 65 536 bytes afterwards;
  - `read_line`: when there is no complete line, and an open line over `LINE_LIMIT` raises, so at most 1 048 576 + 65 536 bytes;
  - handshake: the plaintext that arrives with the server flight, at most 65 536 bytes.

  So unread plaintext is never more than 1 048 576 + 2 × 65 536 bytes per `Conn`.
- **Write side.** During one `write_bytes b` call, the records held in rustls are about `|b|` plus the record overhead. The pulled `Bytes` and `b` are counted heap objects. Nothing stays after the call.
- **Receive side inside rustls.** At most one partial record, ≤ 16 KiB plus the overhead.

### 1.3 `std/Tls.em` (full text; type-checked and run [V])

```haskell
-- TLS のクライアント (docs/spec/stdlib.md の「`Std.Tls`」)。ソケットの I/O はすべて `Std.Net` が行い、ここでは
-- `Conn` と `TcpStream` の間でバイトを汲む。待ち、取り消し、期限は `Net` のもので、ここでは何も足さない
import Async (Async)
import Bytes
import Net
import String

-- rustls の接続と、復号してまだ返していない平文の列を持つ。I/O を持たない。Kind は `Lin` で、落とすとメモリだけを放す
pub extern data Conn

-- TLS の状態とソケットの組。落とすとソケットを閉じ、close_notify は送らない
pub data Stream =
  | Stream { conn : Conn, sock : Net.TcpStream }

-- `host` の `port` へ接続し、`host` を SNI と証明書の名前に使って handshake まで行う。根は既定の根だけである
pub connect : String -> Int -> <IO, Async> Stream
connect host port = connect_with_ca "" host port

-- `connect` に、根に足す CA の証明書 (PEM) を渡す。空の文字列なら既定の根だけを使う
pub connect_with_ca : String -> String -> Int -> <IO, Async> Stream
connect_with_ca ca host port =
  -- 名前と PEM の誤りは、接続を開く前に投げる
  let conn = client_raw host ca
  let sock = Net.connect host port
  handshake host conn sock

-- 既にある `TcpStream` の上で handshake する。`host` は SNI と証明書の名前で、根は既定の根だけである
pub wrap : String -> Net.TcpStream -> <IO, Async> Stream
wrap host sock =
  let conn = client_raw host ""
  handshake host conn sock

-- handshake の間、送るものを書き出しては読んで `Conn` に入れる。最後の書き出しがクライアントの Finished を送る
handshake : String -> Conn -> Net.TcpStream -> <IO, Async> Stream
handshake host conn sock =
  let prefix = "cannot complete the TLS handshake with \{host}"
  let (conn, sock) = flush conn sock
  let (st, conn) = status conn
  if st &&& handshaking == 0 then Stream { conn, sock }
  else if st &&& (peer_closed ||| truncated) != 0 then
    drop conn
    drop sock
    fail_io ConnectionReset "\{prefix}: the peer closed the connection"
  else
    let (conn, sock) = fill prefix conn sock
    handshake host conn sock

-- 平文を `b` の後ろに 1 バイト以上 65 536 バイト以下足して返す。相手が close_notify で閉じた後は何も足さない。
-- close_notify のない EOF は `ConnectionReset` の IO の失敗である。取り消しの地点である
pub read_chunk : Bytes -> Stream -> <IO, Async> (Bytes, Stream)
read_chunk b s =
  if Async.cancel_requested () then
    drop s
    raise_cancelled ()
  else
    let Stream { conn, sock } = s
    read_loop b conn sock

read_loop : Bytes -> Conn -> Net.TcpStream -> <IO, Async> (Bytes, Stream)
read_loop b conn sock =
  let before = Bytes.length b
  let (b, conn) = read_plain b conn
  if Bytes.length b > before then (b, Stream { conn, sock })
  else
    let (st, conn) = status conn
    if st &&& peer_closed != 0 then (b, Stream { conn, sock })
    else if st &&& truncated != 0 then truncated_eof conn sock
    else
      let (conn, sock) = fill read_prefix conn sock
      read_loop b conn sock

-- 1行を読み、終わりの改行を除いて返す。意味、上限、文言は `Net.read_line` と同じである。読みすぎた平文は `Conn` に
-- 残り、次の読みが先に返す。取り消しの地点である
pub read_line : Stream -> <IO, Panic, Async> (Option String, Stream)
read_line s =
  if Async.cancel_requested () then
    drop s
    raise_cancelled ()
  else
    let Stream { conn, sock } = s
    line_loop conn sock

line_loop : Conn -> Net.TcpStream -> <IO, Panic, Async> (Option String, Stream)
line_loop conn sock =
  let (code, conn) = line_ready conn
  if code == 0 then
    let (st, conn) = status conn
    if st &&& truncated != 0 then truncated_eof conn sock
    else
      let (conn, sock) = fill read_prefix conn sock
      line_loop conn sock
  else line_result code conn sock

-- `line_ready` の 0 でない番号を行か IO の失敗にする。1 は行 (残りのない EOF なら空)、2 は上限を超えた行、3 は UTF-8
-- でない行である
line_result : Int -> Conn -> Net.TcpStream -> <IO, Panic> (Option String, Stream)
line_result code conn sock =
  if code == 1 then
    let (line, conn) = take_line conn
    (line_of line, Stream { conn, sock })
  else
    drop conn
    drop sock
    if code == 2 then fail_io Other "\{read_prefix}: a line is longer than 1048576 bytes"
    else fail_io NotUtf8 "\{read_prefix}: the line is not valid UTF-8"

line_of : String -> <Panic> Option String
line_of line = if String.byte_length line == 0 then None else Some (String.trim_line_end line)

-- 平文を暗号にして、同じ呼び出しの中で書き出す
pub write_bytes : Bytes -> Stream -> <IO, Async> Stream
write_bytes b s =
  let Stream { conn, sock } = s
  let conn = write_plain b conn
  let (conn, sock) = flush conn sock
  Stream { conn, sock }

pub write : String -> Stream -> <IO, Async> Stream
write str s = write_bytes (Bytes.of_string str) s

pub write_line : String -> Stream -> <IO, Async> Stream
write_line str s = write_bytes (Bytes.of_string "\{str}\n") s

-- close_notify を書き出し、送る側を閉じる
pub shutdown_write : Stream -> <IO, Async> Stream
shutdown_write s =
  let Stream { conn, sock } = s
  let conn = send_close_notify conn
  let (conn, sock) = flush conn sock
  let sock = Net.shutdown_write sock
  Stream { conn, sock }

-- `shutdown_write` してから落とす。相手がもう閉じていれば close_notify は書けないので、書きの失敗は捨てる。取り消し
-- だけは取り消しのまま投げる
pub close : Stream -> <IO, Async> Unit
close s =
  match try_io (fn () -> drop (shutdown_write s)) with
    | Ok () -> ()
    | Err (IOError { kind = k, message = m }) -> if k == Prelude.Cancelled then fail_io k m else ()

-- 送るものがあれば、すべて書き出す
flush : Conn -> Net.TcpStream -> <IO, Async> (Conn, Net.TcpStream)
flush conn sock =
  let (st, conn) = status conn
  if st &&& has_outgoing == 0 then (conn, sock)
  else
    let (out, conn) = pull_tls conn
    let (_, sock) = Net.write_bytes out sock
    (conn, sock)

-- ソケットを1回読んで `Conn` に入れる。空の読みは EOF である
fill : String -> Conn -> Net.TcpStream -> <IO, Async> (Conn, Net.TcpStream)
fill prefix conn sock =
  let (b, sock) = Net.read_chunk (Bytes.empty ()) sock
  let conn = if Bytes.length b == 0 then push_eof conn else push_tls b conn
  checked prefix conn sock

-- TLS の誤りなら alert を書き出してから IO の失敗にし、同じ押し込みで復号した平文も返さない。誤りでなければ送るもの
-- (TLS 1.3 の KeyUpdate への応答など) を書き出す
checked : String -> Conn -> Net.TcpStream -> <IO, Async> (Conn, Net.TcpStream)
checked prefix conn sock =
  let (st, conn) = status conn
  if st &&& failed == 0 then flush conn sock
  else
    let (reason, conn) = error_message conn
    -- alert を書けなくても TLS の誤りのほうを報告する。取り消しだけは取り消しのまま投げる
    match try_io (fn () -> drop (flush conn sock)) with
      | Err (IOError { kind = k, message = m }) ->
        if k == Prelude.Cancelled then fail_io k m else fail_io Other "\{prefix}: \{reason}"
      | Ok () -> fail_io Other "\{prefix}: \{reason}"

truncated_eof : Conn -> Net.TcpStream -> <IO> a
truncated_eof conn sock =
  drop conn
  drop sock
  fail_io ConnectionReset "\{read_prefix}: the peer closed the connection without close_notify"

-- `Std.Net` と同じ取り消しの失敗 (docs/spec/stdlib.md の「取り消し」)
raise_cancelled : Unit -> <IO> a
raise_cancelled () = fail_io Prelude.Cancelled "task was cancelled"

read_prefix : String
read_prefix = "cannot read the stream"

-- `status` の印 (docs/spec/stdlib.md の「`Std.Tls`」)
handshaking : Int
handshaking = 1

has_outgoing : Int
has_outgoing = 2

peer_closed : Int
peer_closed = 4

truncated : Int
truncated = 8

failed : Int
failed = 16

-- ランタイムのインターフェース。`client_raw` のほかは投げず、TLS の誤りは `status` の 16 と `error_message` に残る
extern client_raw        : String -> String -> <IO> Conn
extern push_tls          : Bytes -> Conn -> <IO> Conn
extern push_eof          : Conn -> <IO> Conn
extern pull_tls          : Conn -> <IO> (Bytes, Conn)
extern write_plain       : Bytes -> Conn -> <IO> Conn
extern read_plain        : Bytes -> Conn -> <IO> (Bytes, Conn)
extern status            : Conn -> <IO> (Int, Conn)
extern error_message     : Conn -> <IO> (String, Conn)
extern send_close_notify : Conn -> <IO> Conn
extern line_ready        : Conn -> <IO> (Int, Conn)
extern take_line         : Conn -> <IO> (String, Conn)
```

**The `_within` functions.** Not in this stage (decided by the user). The surface is the 11 public functions above.

### 1.4 Behaviour and messages (normative; goes into stdlib.md 「`Std.Tls`」)

| case | kind | message |
|---|---|---|
| server name not usable | Other | `cannot set up TLS for {host}: the server name is not valid` |
| bad extra CA PEM | Other | `cannot set up TLS for {host}: the CA certificates are not valid PEM` |
| built without TLS | NotAvailable | `cannot set up TLS for {host}: not available` |
| certificate rejected | Other | `cannot complete the TLS handshake with {host}: the server certificate was rejected` |
| other TLS error in the handshake | Other | `cannot complete the TLS handshake with {host}: {rustls text}` |
| peer closed during the handshake (EOF or close_notify) | ConnectionReset | `cannot complete the TLS handshake with {host}: the peer closed the connection` |
| TLS error while reading | Other | `cannot read the stream: {rustls text}` |
| EOF without close_notify | ConnectionReset | `cannot read the stream: the peer closed the connection without close_notify` |
| line too long / not UTF-8 | Other / NotUtf8 | the same as `Net.read_line` |
| write after `shutdown_write` | as `Net` (ConnectionReset) | `cannot write to the stream: connection reset` [V real driver] |
| cancellation | Cancelled | `task was cancelled` |
| socket errors | as `Std.Net` | `Std.Net`'s messages |

- **Reads deliver buffered plaintext before reporting a truncation.** A TLS failure discards the plaintext decoded in the same push and raises (A-1 decision).
- **`read_chunk` and `read_line` are always cancellation points**, checked at entry as in `Net` (C-2). `write_*` is a cancellation point when it writes.
- **`close` never fails except with `Cancelled`.**
- **Dropping a `Stream`** (including drops during unwinding) sends no close_notify.

## 2. Rationale

- **The design works end to end** [V, prototype with the real driver against `openssl s_server` and Python `ssl`]:
  - working paths: handshake, 81 lines, 1 MB written, 3 MiB read, a clean EOF with a last line that has no newline;
  - failure paths with the messages above: rejection, bad PEM, bad name, truncation, a 1 MiB line, a write after shutdown;
  - `--debug-heap` is clean, and the build without TLS gives NotAvailable [V `tls_off` test].
- **The uninhabited stub type** keeps `vm.rs` free of cfg, and the build without TLS cannot construct a `TlsConn`.
- **The `read_line` leftover lives in `Conn`'s plaintext buffer.** `find_line`/`open_line_len`/`LINE_LIMIT` are shared with `Net`, so the semantics are identical by construction.
  - The design's 「`Tls.Stream` は専用の読み残しの `Bytes` を持つ」 contradicts its own two-field `Stream`.
  - A leftover in eml would cost an extra copy and a per-line `Bytes.slice`.
  - Wording for docs/future at close: 「`Tls.Stream` の読み残しは `Conn` の平文の列にあり、行は `Net.read_line` と同じ関数で切り出す」.
- **The alert is written inside `try_io`.** The TLS reason survives a failed alert write, and `Cancelled` is still re-raised.
- **The cancel check at entry** costs one call. It restores `Net`'s guarantee when plaintext is already buffered.

## 3. Alternatives rejected

- **Leftover `Bytes` and line logic in eml:** an extra copy and duplicated limit rules.
- **One row for lines, with sentinels or raising:** this overloads `""` or breaks "only `client_raw` raises".
- **cfg on each arm, or a second arm set:** spreads cfg, or doubles the code.
- **`Box<Conn>`:** not needed at 1272 bytes with alignment 8.
- **A static cached `ClientConfig`:** 27 µs per connect does not justify global state.
- **Delivering plaintext decoded before a failure:** this would need an extra failed-but-buffered state in every read loop. The bytes come from a connection that has just been shown to be bad.
- **`_within` with a per-read deadline as the default:** a misleading bound (C-1).

## 4. Assumptions (impact if false)

- **A1. Fixtures (S2).** S2's commit lands before T1 with `eml_tls::fixtures::{CA_PEM, CERT_PEM, KEY_PEM}: &[u8]`, always compiled, leaf SAN `tls.localhost` / `localhost` / `127.0.0.1`. **Decision B-1:** keep it an unconditional `pub mod`. The test driver in `eml_io` needs it in normal builds, and the keys are public test keys of a few hundred bytes. The `eml_vm` tests reach it through the optional normal dependency, under `#[cfg(all(test, feature = "tls"))]`, so they follow S3's dev-dependency rule. If it were a feature `fixtures` instead, both `eml_io/tls` and `eml_vm/tls` would have to add `"eml_tls/fixtures"`.
- **A2. S3's T1 order.** S3's T1 (skeleton, link test, gates G1 and G2) comes before my T1. My T1 adds `mod conn;` to the `tests/main.rs` that S3 creates. With the `ring` fallback, only the `default_provider()` path changes.
- **A3. Terminator API (S2).** The terminator uses only API listed in 1.1, and `pull_tls` yields the alert after FAILED [V test 9].
- **A4. Feature wiring (S3).** eml_cli `tls = ["run", "eml_vm/tls"]`, default `["run", "tls"]`. The workspace run has `tls` on, so the `eml_vm` tests under `cfg(all(test, feature = "tls"))` run there. `-p eml_vm` alone has it off, so `tls_off::` runs there through S3's CI step.

## 5. Implementation tasks

Order:
1. S3 T1 (skeleton and gates);
2. S2 fixtures;
3. **T1**;
4. S2 terminator (`eml_io` `tls`, with eml_io's `tls_off` and the S3 CI step);
5. **T2** (any time before T3);
6. **T3**.

### T1. `eml_tls::Conn`

**Files.**
- `crates/eml_tls/src/lib.rs`: the API in 1.1. Comments cite docs/implementation/architecture.md の「`eml_tls` の内部」.
- `crates/eml_tls/tests/conn.rs`, plus `mod conn;` in `tests/main.rs`.
- docs/implementation/architecture.md: new 「`eml_tls` の内部」, covering:
  - the sans-IO wrapper and `builder_with_provider`;
  - the roots and the extra PEM;
  - `set_buffer_limit(None)`;
  - draining after every `process_new_packets`;
  - the five bits and their exact conditions;
  - the failure capture, and that the alert can still be pulled;
  - the fixed texts;
  - records after close_notify;
  - that `server` and `fixtures` exist only for the test driver and tests.

**Tests first** (in-memory client↔server pumping). 21 tests passed in the prototype in 0.07 s in total [V], with openssl-generated certificates standing in for S2's fixtures.

| # | test | checks |
|---|---|---|
| 1 | `handshake_then_bytes_both_ways` | status 0 on both ends; "hello"/"world" both ways |
| 2 | `an_ip_name_checks_the_ip_san` | `client("127.0.0.1", CA)` reaches status 0 |
| 3 | `one_push_of_many_records_moves_all_plaintext` | 200 KiB in one push arrives whole |
| 4 | `close_notify_is_peer_closed` | status == `PEER_CLOSED`, data intact |
| 5 | `eof_without_close_notify_is_truncated` | status == `TRUNCATED`, data intact |
| 6 | `eof_in_the_handshake_is_truncated` | `HANDSHAKING \| TRUNCATED` |
| 7 | `an_unknown_ca_is_rejected_with_a_fixed_message` | the client is FAILED with "the server certificate was rejected"; the server receives the alert and is FAILED |
| 8 | `a_name_not_in_the_certificate_is_rejected` | `client("example.com", CA)` gives the same message |
| 9 | `the_alert_waits_until_pulled` | `HANDSHAKING \| HAS_OUTGOING \| FAILED`; after the pull, non-empty bytes and `HANDSHAKING \| FAILED` |
| 10 | `setup_errors` | `""` and `"a b"` give `ServerName`; `b"garbage"` and a CERTIFICATE block with bad DER give `CaPem`; plus `CertPem`, `KeyPem`, and the Display texts |
| 11 | `non_tls_bytes_fail` | FAILED, with a non-empty message |
| 12 | `plaintext_written_before_the_handshake_is_sent_after_it` | 100 000 bytes arrive |
| 13 | `a_byte_at_a_time` | the handshake completes with 1-byte pushes |
| 14 | `nothing_is_read_after_close_notify` | records encrypted after close_notify are dropped; status == `PEER_CLOSED` |
| 15 | `close_notify_after_a_failure_sends_nothing` | — |
| 16 | `write_after_close_notify_still_makes_records` | `HAS_OUTGOING` after `write_plain` |
| 17 | `data_then_garbage_in_one_push_fails_and_keeps_the_data` | FAILED, and `plaintext() == b"good"` |
| 18 | `a_tls12_only_server_handshakes` | a raw `rustls::ServerConnection` restricted to `&rustls::version::TLS12` gives `protocol_version() == Some(TLSv1_2)`, then a round trip |
| 19 | `read_plain_moves_everything` | — |
| 20 | `the_box_fits_alloc_foreign` | `align_of::<Conn>() <= 8` |

The prototype's 21st test is a timing print, not a test for the repository.

**Completion.**
- `cargo nextest run -p eml_tls` passes, with each test under 1 s.
- `cargo clippy -p eml_tls --all-targets -- -D warnings` is clean.
- `cargo fmt --check` is clean.

### T2. Shared line search (mechanical, kind 3)

- `crates/eml_vm/src/cells.rs`, as in 1.2.
- Completion: `cargo nextest run -p eml_vm` gives the same pass count as before; `cargo clippy -p eml_vm --all-targets -- -D warnings` is clean.

### T3. `Std.Tls`

**Files.**
- `crates/eml_extern/src/lib.rs`: 1 type row and 11 function rows.
- `std/Tls.em`, and the entry `("Tls.em", include_str!("../../../std/Tls.em"))` after Signal in `STD` in `crates/eml_hir/src/lib.rs`.
- `crates/eml_vm/{Cargo.toml, src/lib.rs, src/tls.rs, src/vm.rs, tests/main.rs, tests/tls_off.rs}`.
- `crates/eml_core_ir/tests/externs.rs`.
- The docs below.

**Tests first.**
- **`crates/eml_core_ir/tests/externs.rs`** [V passed]:
  - add to `PROBES`: `probe_tls_conn : Tls.Conn -> Tls.Conn` / `probe_tls_conn c = c`;
  - add to `probe()`: `ExternType::TlsConn => "probe_tls_conn"`;
  - in `only_the_listed_rows_borrow_or_return_a_pair`:
    - `"Std.Tls.pull_tls" | "Std.Tls.status" | "Std.Tls.error_message" | "Std.Tls.line_ready" | "Std.Tls.take_line" => (&[true], Some(0))`
    - `"Std.Tls.read_plain" => (&[false, true], Some(1))`
    - `"Std.Tls.push_tls" | "Std.Tls.write_plain" => (&[true, false], None)`
  - in `only_the_listed_rows_carry_a_lin_or_poly_mark`:
    - `client_raw => (&[Unr, Unr], Lin)`
    - `push_tls | write_plain => (&[Unr, Lin], Lin)`
    - `push_eof | send_close_notify => (&[Lin], Lin)`
    - the five pair rows `=> (&[Lin], Unr)`
    - `read_plain => (&[Unr, Lin], Unr)`
- **`crates/eml_vm/src/tls.rs` `#[cfg(test)] mod tests`** (both configurations). The first six passed [V]:
  - `lines_end_with_lf_or_crlf`: `b"ab\ncd"` gives `(Ready(3), 0)`, and `b"ab\r\ncd"` gives `(Ready(4), 0)`;
  - `no_newline_is_more_and_moves_from`: `b"abc"` gives `(More, 3)`;
  - `eof_takes_the_rest`: `(b"abc", 3, true)` gives `(Ready(3), 3)`, and `(b"", 0, true)` gives `(Ready(0), 0)`;
  - `the_limit_counts_content`:
    - `LINE_LIMIT` `a`s followed by `\r\n` give `Ready(LINE_LIMIT + 2)`;
    - `LINE_LIMIT` `a`s followed by `\r`, at `from` 0, give `More`;
    - `LINE_LIMIT + 1` `a`s give `TooLong`;
    - `LINE_LIMIT + 1` `a`s followed by `\n` give `TooLong`;
  - `invalid_utf8_is_reported`: `b"\xff\n"` gives `NotUtf8`;
  - `a_crlf_split_across_pushes_is_one_end`: `b"ab\r"` gives `(More, 3)`, then `(b"ab\r\nx", 3)` gives `(Ready(4), 3)`;
  - `an_incremental_from_matches_from_zero` [I]: for every split point of a fixed multi-line input, feeding the prefix and then the whole input gives the same `Line` as `from = 0`.
- **`#[cfg(all(test, feature = "tls"))] mod on_tests`** (`cells::test_ctx()`, a client `TlsConn` pumped in memory against `eml_tls::Conn::server(fixtures)`):
  - `read_plain_and_lines_interleave` [V passed]:
    1. The server sends `ab`. `line_ready` gives 0, and `read_plain` gives `ab`.
    2. The server sends `cd\nef`. `line_ready` gives 1, and `take_line` gives `"cd\n"`.
    3. The server sends 100 000 `x`s. `read_plain` gives exactly 65 536 bytes, starting with `ef`.
    4. After the `decref`s, `ctx.heap().leaks()` is empty.
  - `pull_tls_on_an_idle_conn_is_empty` [I].
  - `client_errors_are_other_with_fixed_messages` [I]: `"a b"` and bad PEM give kind 4 and the messages in 1.4, and nothing is allocated.
- **`crates/eml_vm/tests/tls_off.rs`**, declared in `tests/main.rs` as `#[cfg(not(feature = "tls"))] mod tls_off;`, not OS-gated (S3's CI step `test(tls_off::)`). [V passed]
  ```rust
  const SRC: &str = "import Async
  import Tls

  main : Unit -> <IO, Panic> Unit
  main () = Async.run (fn () ->
    match try_io (fn () -> Tls.connect \"tls.localhost\" 1) with
      | Ok s -> Tls.close s
      | Err e -> println (show e))
  ";
  #[test]
  fn std_tls_is_not_available() {
      let (out, result) = eml_test_support::run(SRC);
      assert!(result.is_ok(), "{result:?}");
      assert_eq!(out, "IOError { kind = NotAvailable, message = \"cannot set up TLS for tls.localhost: not available\" }\n");
  }
  ```
  If S2's eml_vm `tls_off` module lands first, this test joins that module.
- **UI tests**, written first in this commit. Content is S2's; the expected messages are in 1.4.
  - `tests/ui/run/tls/`: `echo`, `large_body`, `close_notify_eof`, `read_line`, `shutdown_write` (uses `Tls.wrap`).
  - `tests/ui/run-fail/tls/`: `unknown_ca`, `truncated`, `server_closes_before_handshake`.
  - Proposed for the consolidated list:
    - `run/tls/close_after_peer_closed`: `Tls.close` after EOF returns normally; this guards `close`'s `try_io`.
    - `run/tls/read_line_mixed`: CRLF, a last line without a newline at close_notify EOF, and `read_line` alternated with `read_chunk`, so no byte is lost or reordered.
    - `run-fail/tls/write_after_shutdown`: `Tls.write` after `Tls.shutdown_write` gives kind ConnectionReset, `cannot write to the stream: connection reset`. This assumes S2's terminator makes the write to a self-shut end fail with BrokenPipe, as R1 says the test driver does.
    - `run-fail/tls/line_too_long`: a line over 1 MiB gives the `Net` limit message; it costs about 1 MiB through rustls, which is well under 1 s.
    - only if the `_within` functions are kept: `run/tls/read_chunk_within`.

**Docs in the same commit.** `citations::every_cited_heading_exists` fails without them [V].
- **docs/spec/stdlib.md**: new `## \`Std.Tls\`` between 「`Std.Net`」 and 「`Std.Signal`」:
  1. Intro: client only; `Conn` is a Lin `Foreign` without I/O; `Stream` is eml data; all socket I/O, waits, cancellation and deadlines belong to `Std.Net`; aws-lc-rs, webpki-roots, the extra CA as PEM, no OS store, TLS 1.2 and 1.3.
  2. Example: `connect_with_ca (Fs.read_text "ca.pem")`, `write_line`, `read_line`, `close`.
  3. Table of name, type and implementation.
  4. Bullets for:
     - connect, connect_with_ca and wrap: SNI and the name check; setup errors come before the connection; no deadline, so use `Async.timeout`;
     - read_chunk: 1..65 536 bytes; EOF only after close_notify; buffered plaintext before a truncation is delivered first; a failure discards the plaintext of the same push;
     - the cancellation point at entry;
     - mixing `read_line` and `read_chunk`;
     - `read_line` is the same as `Net.read_line`;
     - `write_*` encrypts and writes in one call; a write after `shutdown_write` fails like `Net`'s;
     - shutdown_write, and `close`: it never fails, except with Cancelled;
     - drop and cancellation send no close_notify;
     - memory outside `max_bytes`, with the bounds in 1.2;
     - the build without TLS;
     - deadlines with `Async.timeout`.
  5. The table in 1.4.
  6. Wrapper shape:
     - the status bits table (1/2/4/8/16 and their exact conditions);
     - the `line_ready` codes 0–3;
     - the failure path (flush the alert inside `try_io`, then `fail_io`);
     - the extern table (行 / 型 / 借りる引数) of 11 generic rows, of which only `client_raw` raises.
- **docs/spec/vm.md 「上限と割り込み」**: after the stack bullet, add a bullet on the uncounted `Foreign` memory (the `NetStream` buffer, and the `Conn` plaintext and rustls buffers), with the bounds in 1.2.
- **docs/spec/object-model.md**, the table at :141-145: add the row `Std.Tls.Conn` | `Foreign`、`lin` ビットを立てる | 破棄処理の表と、`eml_vm` の Rust の値 `TlsConn` (「`Std.Tls`」).
- **docs/spec/modules.md:69 and docs/implementation/status.md:28**: add `Tls.em`. status.md also gets a TLS bullet after TCP (:38).
- **docs/implementation/architecture.md** 「`eml_core_ir`、`eml_lir`、`eml_vm` の内部」: `tls.rs` holds `TlsConn` and the arms, and without the feature it builds against an uninhabited `Conn`.
- **Unchanged:** 「extern の境界」 (it already covers Foreign and pairs; the Tls extern table states the borrows) and 「実行時エラー」 (the `Net` messages also live in their own section).
- **docs/future and the roadmap at stage close:** where the leftover lives, the two added rows, `plaintext`/`consume`, and the `_within` decision.

**Test-change kind.** No existing expected values change. The core_ir lists only gain entries for new rows.

**Completion.**
- `cargo nextest run` passes, including `ui::`, `citations::`, core_ir `externs::`, and eml_hir's `the_embedded_std_matches_the_std_directory` and `every_extern_*`.
- `cargo nextest run -p eml_io -p eml_vm -E 'test(tls_off::)'` passes, and `std_tls_is_not_available` is among the passing tests.
- `cargo clippy --all-targets -- -D warnings`, `cargo clippy -p eml_vm -- -D warnings` and `cargo clippy -p eml_vm --features tls -- -D warnings` are clean.
- S3's `cargo tree` guard lines give 0.

## 6. Unresolved items (defaults)

1. **`_within`:** dropped unless the user keeps them (1.3).
2. **Ciphertext read buffer:** `fill` passes `Bytes.empty ()`, so each socket read grows a fresh 64 KiB `Bytes`. Measure it in std-http's `https-get`. If it costs, keep a scratch buffer in `Stream`.
3. **Session resumption** is off, because the config is built per connection (27 µs).

## 7. Additional checks

- [V] 21 eml_tls tests pass in 0.07 s.
- [V] Connect cost: 27 µs; handshake: 0.70 ms.
- [V] eml_vm `tls::` tests: 6 pass without the feature, 7 with it; `tls_off::std_tls_is_not_available` passes without the feature.
- [V] The real-driver scenarios from 2 pass on the revised `Tls.em`, including `--debug-heap`.
- [V] The full workspace tests passed on the r1 prototype, apart from citations.
- [V] The rustls source has no close_notify check in `send_plain`, and `send_close_notify` is a no-op after a fatal alert (common_state.rs:652-660).
- [I] On the test driver, the client's close_notify write after the server's close succeeds, because S2's terminator only shuts the ciphertext end (S2 §1); `close`'s `try_io` only matters on real networks.
- No background tasks or servers are left running.
