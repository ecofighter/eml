# S1 (r2): TLS on the blocking pool

Part A is the revised deliverable in full. Part B answers the r1 review.

---

# Part A: revised deliverable

Tags: [V] means verified, and says how. [I] means inferred.

The prototype is at patches/03-tls-on-pool-prototype.patch (see its `git diff`).
- It contains F1's eml_rt `foreign_take` patch.
- It contains S1's rows, std code, VM arms and tests.
- It contains a stub `Roots`: `Roots::of(sched.driver_kind)` is real; `client_conn` ignores `roots` and calls today's `Conn::client(host, pem)`.
- F1 units 2-3 (`jobs.rs`) do not exist yet. In their place, `proto_submit`/`proto_take` in `vm.rs` carry the value through today's `OpKind::Ffi` `u64`, with debug `eprintln!`s. That part is not for copying.
- What the prototype does not exercise is marked [I]:
  - `jobs::submit`/`jobs::take` themselves.
  - S2's `Name`/`Trust`.
  - The G-1/2/3/6 shapes checked against a real `jobs.rs`.

The design follows S2 r2 and the coordination premise: S2-A creates the dispatch, and S1 moves the call into the job.

## 1. Conclusion

1. **Pool work.**
   - Building the `Conn` (name, trust, `Conn::client`) runs on the pool.
   - Every push made while `HANDSHAKING` is set runs on the pool.
   - `fill` chooses between the two pushes by the `status` its caller already read. Pushes with `HANDSHAKING` clear stay synchronous through `push_tls`.
   - `push_eof`, `pull_tls`, `write_plain`, `read_plain`, `status`, `error_message`, `send_close_notify`, `line_ready` and `take_line` stay synchronous.
   - The `push_tls` arm returns `VmFault::Internal("Std.Tls.push_tls runs during the handshake")` when `HANDSHAKING` is set. Only a std bug can reach this: the network cannot (§2, C-3).
2. **Rows.** `client_raw` goes, and three rows come in. There are 13 Tls rows in all.
   - `Std.Tls.submit_client : String -> String -> <IO> Async.OpId`
   - `Std.Tls.submit_push : Bytes -> Conn -> <IO> Async.OpId`
   - `Std.Tls.take_conn : Async.OpId -> Async.Wake -> <IO> Conn`

   Both submits use the one take, with `JobSource::Std(Extern::TlsTakeConn)`; the payload is `Result<TlsConn, String>`.
3. **Trust dispatch.** S2-A owns it in `crates/eml_vm/src/tls.rs`, with the interface in §5.1. S1 only calls `client_conn` from the client job.
4. **Setup errors.** `client_conn` returns `Err("cannot set up TLS for {host}: {e}")`. `take_conn` raises it with kind `SETUP_KIND`. The messages are unchanged.
5. **Cancel and deadlines.**
   - A running job runs to its end.
   - The cancelled task then drops the `Conn` in `jobs::take`. The unwinding drops the socket without close_notify. The task raises `Cancelled` / `task was cancelled`.
   - `Async.timeout` returns only after a running verification ends.
   - "A deadline loses the `Stream`" still holds.
   - Setup becomes a cancellation point.
6. **Two commits.**
   - S1a: setup on the pool, `take_conn`.
   - S1b: handshake pushes on the pool, the assertion.

   Both come after F1-3 and S2-A, and before S2-B.

## 2. Rationale

- **Handshake only.**
  - Pooling every push would add one job per ≤64 KiB of ciphertext after the handshake. For the 600 MiB `https-get` row that is about 9 600 jobs [I: arithmetic; not measured].
  - Handshake-only pooling costs one setup job plus one push job per connection [V: prototype trace, one `submit_push` per handshake in `echo.em`].
- **Why the network cannot reach the assertion.** The evidence comes from reading rustls 0.23.46 and eml_tls.
  - `HANDSHAKING` is `is_handshaking()`, which is `!(may_send_application_data && may_receive_application_data)` [V: eml_tls/src/lib.rs:226, rustls common_state.rs:114].
  - Both flags are assigned only `true` after construction [V: grep, common_state.rs:480,490; the only `false` is the constructor at 77-78]. So once clear, `HANDSHAKING` never comes back.
  - TLS 1.2: a `HelloRequest` after the handshake gets a `no_renegotiation` warning and is otherwise ignored [V: common_state.rs:203-215].
  - TLS 1.2 `ExpectTraffic` takes only application data [V: client/tls12.rs:1325-1346].
  - TLS 1.3 `ExpectTraffic` takes application data, `NewSessionTicket` and `KeyUpdate`, and rejects every other message [V: client/tls13.rs:1598-1628].
  - `NewSessionTicket` stores the verifier `Arc` in the session value and does not call `verify_server_cert` [V: tls13.rs:1515-1535].
  - The only calls are tls12.rs:885 and tls13.rs:1192, both handshake states [V: grep].
  - `fill` decides by the `status` read right before it, on the same `Conn`; only the socket read comes between. Even if a future rustls set `HANDSHAKING` again, the push would go to the pool and not to the internal error.
- **After a failed handshake.**
  - The flags stay false, so `FAILED` and `HANDSHAKING` are set together [V: same reading].
  - `checked` raises on `FAILED` right after the push, so `fill` never runs on that `Conn` again [V: std/Tls.em `checked`].
- **Construction on the pool.**
  - S2 r2 has no cache, so `Trust::platform` reads and parses the Linux root files on every call (about 1 ms, S2 [V by proxy]).
  - `Http` makes one `Conn` per hop.
  - The pool costs one job per connection.
- **One take row.**
  - Both jobs return a `TlsConn`, so there is one `Cancelled` branch.
  - A deterministic UI test covers that branch (`cancelled_setup.em`).
  - A second take row's cancel branch could not be reached deterministically (§7).
- **One `fill`.** Passing the caller's `st` removes the r1 duplicate `fill_handshake`, and it costs no extra `status` call: `handshake`, `read_loop` and `line_loop` already hold `st` [V prototype].

## 3. Alternatives considered and rejected

- **Every push on the pool.** It costs a job per read after the handshake, for no blocking work.
- **A static split** (r1's `fill_handshake` for the handshake loop, `fill` for reads). It duplicates code, and only call-site discipline keeps it correct.
- **`client_raw` stays synchronous.** Each connection would do about 1 ms of file I/O on the VM thread on Linux.
- **A lazy trust built in the first handshake job.** `NoRoots`/`CaPem` would then surface after the socket opens, under the handshake prefix.
- **Two take rows.** They duplicate the cancel branch, and only one of them is testable.
- **An internal fault replaced by a connection failure in `push_tls`.** Unneeded: with the status dispatch, only std code can reach the fault.

## 4. Assumptions made

- **A1.** F1 r2 §1, items 2-3 fix the API:
  - `jobs::submit::<R, T>(ctx, task, sched, dst, from, job)`;
  - `jobs::take::<R, T>(ctx, task, shared, token, wake, from) -> Result<Outcome<T>, VmFault>`.
- **A2.** The order is F1-1, F1-2, F1-3, S2-A, S1a, S1b, S2-B, S2-C, S2-D. S2-B (rpv) must not land before S1b, or the handshake push would block the VM thread.
- **A3.** `TlsConn: Send + 'static` [V: the prototype moves it through an `Op::Job` and `alloc_foreign`s it].
- **A4.** The test driver runs `Op::Job` inline at settle. It does not run a pending job whose record is already cancelled [V: crates/eml_io/src/test.rs `try_finish`].
- **A5.** S2's `Name::parse`, `Trust::fixtures`, `Trust::platform` and `Conn::client(&Name, &Trust)` behave as S2 r2 §5.1 states [I: not compiled here].

## 5. Implications for implementation

### 5.1 The interface S2-A implements verbatim (S1 does not change it)

`crates/eml_vm/src/tls.rs`. Comments are to be written in Japanese with PLaMo; the intents are given here.

```rust
use crate::driver::DriverKind;

/// The roots a run's connections trust. Chosen on the worker from the run's driver kind; S1 moves it into the job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Roots {
    /// The test driver that the `Machine` built itself: only the CA passed in, at `fixtures::NOW`.
    Fixtures,
    /// The OS verifier, plus the CA passed in.
    Platform,
}

impl Roots {
    pub(crate) fn of(kind: DriverKind) -> Roots {
        if kind == DriverKind::Virtual { Roots::Fixtures } else { Roots::Platform }
    }

    #[cfg(feature = "tls")]
    fn trust(self, pem: &[u8]) -> Result<eml_tls::Trust, eml_tls::Error> {
        match self {
            Roots::Fixtures => eml_tls::Trust::fixtures(pem),
            Roots::Platform => eml_tls::Trust::platform(pem),
        }
    }
}

/// Builds the `Foreign` content of a client. Touches no `Ctx`; S1 runs it on the pool.
pub(crate) fn client_conn(roots: Roots, host: &str, pem: &[u8]) -> Result<TlsConn, String>
```

- **With `tls`:**
  ```rust
  eml_tls::Name::parse(host)
      .and_then(|name| roots.trust(pem).and_then(|trust| Conn::client(&name, &trust)))
  ```
  - `Ok(conn)` gives `Ok(TlsConn { conn, line_from: 0 })`.
  - `Err(e)` gives `Err(format!("cannot set up TLS for {host}: {e}"))`.
- **Error order:** first ServerName, then the trust errors (`CaPem`, then `NoRoots`, inside `Trust::*`, S2), then `Config`.
- **Texts:**
  - `the server name is not valid`
  - `the CA certificates are not valid PEM`
  - `no root certificates are available`
  - S2's `Config` text
- **Without `tls`:** `Err(format!("cannot set up TLS for {host}: not available"))`, with `roots` unused (`let _ = roots;`).
- `SETUP_KIND` becomes `pub(crate)`.

**S2-A's `client_raw`.** It stays synchronous, but it must read `sched.driver_kind`, and `run_extern` has no scheduler [V: vm.rs:3928, signature `(code, ctx, task, counts, dst, at)`]. So S2-A does the following:
- Moves `TlsClientRaw` into `Extern::is_async` and into the `Async externs run through Op::Async` list.
- Adds this arm in `async_op`:
  ```rust
  ext @ Extern::TlsClientRaw => {
      let host = ctx.copy_string(task.get(arg(0)), usize::MAX)?;   // consumed, as today
      let pem = ctx.copy_string(task.get(arg(1)), usize::MAX)?;
      match tls::client_conn(tls::Roots::of(sched.driver_kind), &host, pem.as_bytes()) {
          Ok(c) => { let w = unsafe { ctx.heap_mut() }.alloc_foreign(c, true); task.set(dst, w); }
          Err(message) => return raise_again::<R>(code, ctx, task, dst, Raise::Io { kind: tls::SETUP_KIND, message }, ext),
      }
  }
  ```
  [V: `Roots::of(sched.driver_kind)` and the `raise_again`/`Raise::Io` shape compile and run in the prototype's `async_op`. That `client_raw` itself works in `async_op` is I.]

**`on_tests` helpers that S2-A leaves in final form.** S1 does not touch them.
- **`new_client`:** `alloc_foreign(client_conn(Roots::Fixtures, "tls.localhost", CA_PEM).ok().unwrap(), true)`.
- **`pump`:** it reads `status(ctx, client)`.
  - If `HANDSHAKING` is set, it pushes `(*tls_conn(ctx, client)?).conn.push_tls(&back)`, which is the job body's effect.
  - Otherwise it pushes through the `push_tls` arm.

  So the arm stays covered for pushes after the handshake [V: prototype, `read_plain_and_lines_interleave` passes this way].
- **`client_errors_are_other_with_fixed_messages`:** it asserts `client_conn(Roots::Fixtures, host, pem.as_bytes())`'s `Err` strings, including S2's `NoRoots` case. Its kind assertion moves to:
- **`tests/ui/run-fail/tls/bad_ca.em`** (+ snapshot), owned by S2-A. Its shape follows `unknown_ca.em`:
  ```haskell
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
  Imports: `Async`, `Net`, `Tls`.
  - The snapshot has stdout `Other` and the runtime error `cannot set up TLS for tls.localhost: the CA certificates are not valid PEM`, at `run-fail/tls/bad_ca.em:17:7` (with the three imports and no header comment; a header comment of k lines shifts the line by k) [V: pooled prototype].
  - The location is the user's `fail_io`, so the synchronous and the pooled setup give the same snapshot [I: the sync path was not run with this file].

### 5.2 S1a: setup on the pool

**Rows** (`crates/eml_extern/src/lib.rs`): remove `TlsClientRaw` and add:
```rust
TlsSubmitClient => borrowing(&[true, true], kinded("Std.Tls.submit_client", &[Obj, Obj], &[Unr, Unr], Int, Unr, Purity::Effectful)),
TlsTakeConn     => kinded("Std.Tls.take_conn", &[Int, Enum], &[Unr, Unr], Obj, Lin, Purity::Effectful),
```
In `is_async` and in the `Async externs run through Op::Async` list, `TlsClientRaw` is replaced by these two.

**Arms** (`vm.rs` `async_op`; remove S2-A's `TlsClientRaw` arm):
```rust
Extern::TlsSubmitClient => {
    let host = unsafe { &*ctx.heap().string(task.get(arg(0)))? }.to_owned();   // borrowed: copy, no release
    let pem = unsafe { &*ctx.heap().string(task.get(arg(1)))? }.to_owned();
    let roots = tls::Roots::of(sched.driver_kind);
    jobs::submit::<R, _>(ctx, task, sched, dst, JobSource::Std(Extern::TlsTakeConn),
        move || tls::client_conn(roots, &host, pem.as_bytes()))?;
}
ext @ Extern::TlsTakeConn => {
    let raise = match jobs::take::<R, Result<tls::TlsConn, String>>(ctx, task, &sched.cells,
            task.raw(arg(0)), task.raw(arg(1)), JobSource::Std(Extern::TlsTakeConn))? {
        Outcome::Woke(Ok(c)) => {
            let w = unsafe { ctx.heap_mut() }.alloc_foreign(c, true);
            task.set(dst, w);
            return Ok(());
        }
        // only the client job returns `Err`; the push job always returns `Ok` (§5.3)
        Outcome::Woke(Err(message)) => Raise::Io { kind: tls::SETUP_KIND, message },
        Outcome::Failed(e) => Raise::Io { kind: io_kind(&e), message: format!("cannot run TLS: {e}") },
        Outcome::Cancelled => Raise::Io { kind: cells::IO_CANCELLED, message: "task was cancelled".into() },
    };
    return raise_again::<R>(code, ctx, task, dst, raise, ext);
}
```
[V: this shape, with `proto_take`, compiles and runs. `jobs::*` is I.]

**`std/Tls.em`** (comment intents to be translated with PLaMo) [V: prototype]:
```haskell
connect_with_ca ca host port =
  -- name and PEM errors are raised before the connection opens
  let conn = client host ca
  let sock = Net.connect host port
  handshake host conn sock

wrap_with_ca ca host sock =
  let conn = client host ca
  handshake host conn sock

-- (intent) builds the `Conn` on the blocking pool, because building the roots may read OS files;
-- name and PEM errors are raised by the take (docs/spec/stdlib.md 「`Std.Tls`」)
client : String -> String -> <IO, Async> Conn
client host ca =
  let (w, op) = Async.io_wait (submit_client host ca)
  take_conn op w
```
Extern block: the `client_raw` line becomes `extern submit_client : String -> String -> <IO> Async.OpId` and `extern take_conn : Async.OpId -> Async.Wake -> <IO> Conn`. The header comment of the block becomes "`take_conn` のほかは投げず".

**Comment sites that name `client_raw`.** They are updated in S1a, after S2-A has touched the ones it changes:
- `crates/eml_extern/src/lib.rs:2012-2013`, the Tls rows comment.
- `crates/eml_vm/src/vm.rs:4244-4245`, the ordinary-arm comment "投げるのは `client_raw` だけ".
- `crates/eml_vm/src/tls.rs:1-5`, the module doc ("`client_raw` がいつも `NotAvailable`" becomes "`client_conn` がいつも `NotAvailable` を返す"), and `:18`, the `SETUP_KIND` doc.
- `std/Tls.em`, the extern-block comment.
- `docs/spec/stdlib.md:1697,1701,1713`.
- `docs/spec/core-ir.md:233`.
- `docs/implementation/architecture.md:322`.

### 5.3 S1b: handshake pushes on the pool

**Row:**
```rust
TlsSubmitPush => borrowing(&[true, false], kinded("Std.Tls.submit_push", &[Obj, Obj], &[Unr, Lin], Int, Unr, Purity::Effectful)),
```
It is added to `is_async` and to the `Op::Async` list.

**Arm:**
```rust
Extern::TlsSubmitPush => {
    let (bytes, conn) = (task.get(arg(0)), task.get(arg(1)));
    let c = unsafe { ctx.heap().foreign_take::<tls::TlsConn>(conn) }?;   // G-2 (a)
    unsafe { ctx.heap_mut().decref(conn) }?;                               // G-2 (b)
    let b = unsafe { &*ctx.heap().bytes(bytes)? }.to_vec();               // G-2 (c), borrowed
    jobs::submit::<R, _>(ctx, task, sched, dst, JobSource::Std(Extern::TlsTakeConn),
        move || Ok::<_, String>(tls::push_conn(c, b)))?;                   // G-2 (d)
}
```
- `tls.rs` gets `pub(crate) fn push_conn(mut c: TlsConn, b: Vec<u8>) -> TlsConn { c.conn.push_tls(&b); c }`.
- The push job's `Ok` wrapper exists only so that both jobs share `take_conn`'s payload type. `jobs::submit` wraps it once more for panics (F1). That is deliberate.
- The `push_tls` arm gets this check after `tls_conn`:
  ```rust
  if c.conn.status() & HANDSHAKING != 0 {
      return Err(VmFault::Internal("Std.Tls.push_tls runs during the handshake"));
  }
  ```
  `off` gets `pub(crate) const HANDSHAKING: u32 = 1;`, and the `use` lines import `HANDSHAKING`.

**`std/Tls.em`** [V: prototype]. Only the call sites change: `handshake`, `read_loop` and `line_loop` now call `fill prefix st conn sock`, where `st` is the `status` each one has just read.
```haskell
-- (intent) reads the socket once and pushes into `Conn`; an empty read is EOF. While handshaking the push may verify the
-- certificate and the OS verifier may block, so it runs on the pool; `st` is the status the caller just read
fill : String -> Int -> Conn -> Net.TcpStream -> <IO, Async> (Conn, Net.TcpStream)
fill prefix st conn sock =
  let (b, sock) = Net.read_chunk (Bytes.empty ()) sock
  let conn =
    if Bytes.length b == 0 then push_eof conn
    else if st &&& handshaking != 0 then push_pooled b conn
    else push_tls b conn
  checked prefix conn sock

push_pooled : Bytes -> Conn -> <IO, Async> Conn
push_pooled b conn =
  let (w, op) = Async.io_wait (submit_push b conn)
  take_conn op w
```
Extern block: add `extern submit_push : Bytes -> Conn -> <IO> Async.OpId`.

### 5.4 Cancellation, deadlines, interrupts (what the user sees)

| Moment | What happens |
|---|---|
| Cancel pending at `submit_client`/`submit_push` | The submit's cancellation point drops the closure and the `TlsConn`. `take_conn` raises `Cancelled`. Unwinding drops the socket without close_notify [V: `timeout_connect.em` with `ms = 0`, the job never ran] |
| Cancel or deadline while the job is queued or running | The job runs to its end, and the task waits for it. Then `jobs::take` drops the `TlsConn` and `take_conn` raises `Cancelled`; no close_notify is sent. `Async.timeout` returns `None` only after the job [V on the virtual clock: `cancelled_setup.em`, where the take woke `CANCELLED` after the job ran; for running jobs on the real driver this is I, following F1's lifecycle] |
| Virtual clock | Jobs run at settle, before the clock advances. A virtual deadline never lands during a job, and the whole handshake takes no virtual time [V: `timeout_connect.em` with `ms = 1` connects] |
| SIGINT / `Std.Signal` interrupt / REPL Ctrl-C during a handshake job | The run stops. Quiesce waits up to `job_budget` (1 s) for the job. A longer verification is abandoned and the `Machine` is poisoned, so the REPL ends the session after that input (vm.md:73, :532). Accepted: before this stage the verifier ran on the VM thread and the interrupt could not land until it returned (up to ~20 s on Windows) [I] |
| `exit` from a sibling while a connection is handshaking | The same: quiesce waits ≤ 1 s, then the job is abandoned and the `Machine` poisoned |
| The job panics | IO failure with kind `io_kind(e)` (`Other`) and the message `cannot run TLS: a job panicked: …` (F1 U4) |
| Slot pressure | TLS jobs use the driver's 24 `Op::Job` slots together with `blocking` C calls. A new connection can wait for a slot behind long C calls or slow OS verifications. A TLS job waits on nothing of the run, so it cannot be the cause of a deadlock |

close_notify behaviour is unchanged: dropping a `Conn` never sends it.

### 5.5 Tests

[V: workspace `cargo nextest run` on the full prototype (S1a + S1b + the stub + `bad_ca.em`): 4687 passed, 0 failed. The `eml_cli` integration tests, `tls_off::` (2) and `externs::` (14) pass. Every existing `tls/` and `http/` output and the real-driver test are unchanged. No `RunStats` snapshot changes.]

**Kind 2 (expected-value change; list in the plan):**
- `crates/eml_core_ir/tests/externs.rs`:
  - S1a:
    - `only_the_listed_rows_carry_a_lin_or_poly_mark`: `"Std.Tls.client_raw" => (&[Unr, Unr], Lin)` becomes `"Std.Tls.take_conn" => (&[Unr, Unr], Lin)`.
    - `only_the_listed_rows_borrow_or_return_a_pair`: add `"Std.Tls.submit_client" => (&[true, true], &[])`.
  - S1b:
    - Add `"Std.Tls.submit_push" => (&[Unr, Lin], Unr)` to the marks test.
    - Add `"Std.Tls.submit_push"` to the `(&[true, false], &[])` arm of the borrow test.
  - Doc comments: "`Std.Tls` の6つの行" becomes 7 in S1a and 8 in S1b; "`Std.Tls` の11行" stays 11 in S1a and becomes 12 in S1b.

**Kind 1:** none. **Kind 3:** none; S2-A already left the `on_tests` helpers final (§5.1).

**New tests** (UI files run inside the existing directory tests and add no nextest test):
- **S1a: `tests/ui/run-fail/tls/cancelled_setup.em`** (+ snapshot): stdout `Cancelled`, runtime error `task was cancelled` [V: checked and fast mode agree]. It covers G-6.
  ```haskell
  main () = Async.run (fn () ->
    let ca = Fs.read_text "ca.pem"
    let l = Net.listen "127.0.0.1" 0
    let (port, l) = Net.local_port l
    Net.close_listener l
    Async.scope (fn s ->
      Async.spawn_in s (fn () -> Async.cancel_scope s)
      match try_io (fn () -> Tls.close (Tls.connect_with_ca ca "tls.localhost" port)) with
        | Ok () -> println "connected"
        | Err e ->
          println (show e.kind)
          fail_io e.kind e.message))
  ```
  Imports: `Async (Async)`, `Async`, `Fs`, `Net`, `Tls`.
- **S1a: `tests/ui/run/tls/timeout_connect.em`** (+ snapshot): stdout `0: timed out` / `1: connected` [V]. It pins the setup cancellation point and "a virtual deadline never precedes a running job".
  ```haskell
  attempt : String -> Int -> Int -> <IO, Panic, Async> Unit
  attempt ca port ms =
    match Async.timeout ms (fn () -> Tls.connect_with_ca ca "tls.localhost" port) with
      | Some s ->
        println "\{show ms}: connected"
        Tls.close s
      | None -> println "\{show ms}: timed out"

  main () = Async.run (fn () ->
    let ca = Fs.read_text "ca.pem"
    let l = Net.listen "127.0.0.1" 0
    let (port, l) = Net.local_port l
    attempt ca port 0
    attempt ca port 1
    Net.close_listener l)
  ```
- **S1b: `crates/eml_vm/src/tls.rs` `on_tests::push_tls_during_the_handshake_is_an_internal_error`.** On a fresh `new_client`, `push_tls` with 1 byte gives `Err(VmFault::Internal("Std.Tls.push_tls runs during the handshake"))` [V].
- Header comments (PLaMo) cite stdlib.md 「`Std.Tls`」.

**Not changed:**
- The `crates/eml_vm/tests/bench.rs` `RunStats`: no bench program uses TLS.
- `RunStats.parks` grows by one per job wait. Only `--stats` shows this; the UI harness compares checked against fast mode only.
- `tls/` and `http/` have no seed or worker runs.

**Not covered** (U1): a cancel that lands while the push job specifically is in flight. The shared `take_conn` branch is covered by `cancelled_setup.em`; dropping the `T` is covered by F1's `jobs::take` unit tests.

### 5.6 Spec text (intent; write in Japanese with PLaMo, no space between Japanese and Latin text)

`docs/spec/stdlib.md` 「`Std.Tls`」.

**S1a:**
- **Intro.** Keep "ソケットの読み書き、待ち、取り消し、期限は `Std.Net` のもの". Add: building the `Conn` runs as a pool job (vm.md 「プールの仕事」), because building the roots may read the OS root files. While a job runs, the worker's other tasks keep running. The jobs use the driver's 24 `Op::Job` slots, shared with `blocking` calls (the 「`Std.Stream`」 bullet), so a connection can wait for a slot. On the virtual clock the jobs run inline on worker 0.
- **Deadline bullet.** Keep "期限が来ると巻き戻しが `Stream` を落とす". Add:
  - A running job cannot be cancelled. A deadline or cancel during a job waits for the job to end, then drops the `Conn` and the socket without close_notify and raises `Cancelled`.
  - `Async.timeout` returns only then.
  - The virtual clock's deadline never precedes a running job.
  - An interrupt during a job: the run waits for it up to the job budget, and a longer job poisons the `Machine` (vm.md 「時計と driver」).
- **Cancellation bullet.** `connect`, `connect_with_ca`, `wrap` and `wrap_with_ca` are cancellation points at setup.
- **Failure table.** New row: 「TLS の仕事を走らせられない (パニックなど)」 | kind of the failure | `cannot run TLS: {理由}`.
- **Extern rows.** Remove `client_raw`, and add these:
  - `submit_client | String -> String -> <IO> Async.OpId | 両方`
  - `take_conn | Async.OpId -> Async.Wake -> <IO> Conn | なし`
  - Rule: `submit_client` and `take_conn` are a pool-job pair run as `Async` instructions. Only `take_conn` raises. The rest never wait.
- **Description.** `submit_client host ca` copies both strings and builds the `Conn` in the job, with roots by the run's driver (vm.md 「時計と driver」, S2's text). `take_conn` returns it as a new object or raises the setup error.

**S1b:**
- **Intro sentence.** The pushes made while handshaking also run as pool jobs, because the push may verify the certificate and the OS verifier may block or go to the network. Pushes after the handshake run on the worker, because rustls verifies only during the handshake.
- **Extern rows.** Add `submit_push | Bytes -> Conn -> <IO> Async.OpId | バイト列`; `submit_push` moves the `Conn` into the job and `take_conn` returns it.
- **`push_tls`.** Calling it while handshaking is the internal error `Std.Tls.push_tls runs during the handshake`.
- **「包みの形」.** `fill` picks the push by the handshaking bit of the status its caller read.
- **The 13-row count.**

`docs/spec/vm.md`:
- Nothing beyond F1's 「プールの仕事」 and S2's 「時計と driver」 text.
- If F1's section lists std job rows, add the three Tls rows.

`docs/spec/core-ir.md:233` marks table:
- S1a: the `client_raw` row becomes `` `Std.Tls.take_conn` | `Unr`、`Unr` | `Lin` (`Conn`) ``.
- S1b: add `` `Std.Tls.submit_push` | `Unr`、`Lin` | `Unr` (`OpId`) ``.

`docs/implementation/architecture.md:322`: `client_conn`, the setup and handshake jobs through `jobs.rs`.

### 5.7 Units and completion criteria

**S1a** (after F1-3 and S2-A): §5.2, plus the S1a tests and docs.
1. `cargo nextest run` exits 0. `cargo clippy --all-targets -- -D warnings` exits 0. `cargo fmt --check` exits 0.
2. `cargo clippy -p eml_vm --all-targets --no-default-features -- -D warnings` exits 0. `cargo nextest run -p eml_io -p eml_vm -E 'test(tls_off::)'` gives 2 passed.
3. `cargo nextest run -p eml_core_ir --test integration -E 'test(externs::)'` gives 14 passed.
4. `cargo nextest run -p eml_vm --features tls --lib -E 'test(tls::)'` gives N passed, where N is S2-A's count. This unit adds no unit test. (Without `--features tls`, `on_tests` do not compile: `-p eml_vm` alone gives 7 [V].)
5. `cargo nextest run -p eml_cli --test integration -E 'test(ui::) | test(tls::)'` passes. `git status --porcelain crates/eml_cli/tests/snapshots` shows exactly `?? …run_fail@tls__cancelled_setup.em.snap` and `?? …run@tls__timeout_connect.em.snap`.
6. `grep -rn "client_raw\|TlsClientRaw" crates std docs/spec docs/implementation` prints nothing.

**S1b:** §5.3, plus the S1b test and docs.
1. Items 1 and 2 as for S1a.
2. `externs::` gives 14 passed.
3. `cargo nextest run -p eml_vm --features tls --lib -E 'test(tls::)'` gives N+1 passed, where N is S2-A's count. On today's tree plus S1 it is 11 = 7 `tests` + 3 `on_tests` + 1 [V: prototype].
4. The UI tests pass with no snapshot change (`git status --porcelain crates/eml_cli/tests` is empty).
5. `cargo nextest run -p eml_vm --test integration -E 'test(bench::)'` passes, and `git status --porcelain crates/eml_vm/tests` is empty.

**F1 checks as read for these rows** (run after S1b):
- **G-1:** `grep -n "jobs::submit" crates/eml_vm/src/vm.rs` prints 2 lines, in the `TlsSubmitClient` and `TlsSubmitPush` arms.
- **G-2:** in the `TlsSubmitPush` arm, `foreign_take::<`, `decref(` and `jobs::submit` appear in that order:
  ```
  awk '/Extern::TlsSubmitPush =>/,/^        }/' crates/eml_vm/src/vm.rs | grep -n "foreign_take::<\|decref(\|jobs::submit"
  ```
  This prints them with increasing line numbers.
- **G-4 (reading: the closure is the text from `move ||` to the end of the call; the `ctx, task, sched` arguments before it are not the closure):**
  ```
  perl -0ne 'while (/jobs::submit(?:::<[^>]*>)?\((.*?)\)\?;/sg) { my $c = $1; $c =~ s/^.*?move \|\|//s; print "$c\n" }' crates/eml_vm/src/vm.rs | grep -c 'ctx\|heap\|task\|sched\|shared\|Word'
  ```
  This prints `0`. [V: the same command with `proto_submit` prints `0` on the prototype.]
- **G-5 (reading: call shapes only; the `extern` declaration lines are excluded):**
  - `grep -n "submit_client\|submit_push" std/Tls.em | grep -v ":extern "` prints exactly two lines, each containing `Async.io_wait (submit_`, and the next line of each is `  take_conn op w` [V].
  - `grep -c "io_wait_until (submit_" std/Tls.em` prints `0`.
- **G-6:** `grep -n -A6 "Outcome::Cancelled" crates/eml_vm/src/vm.rs` shows `IO_CANCELLED` and `"task was cancelled"` in the `TlsTakeConn` arm. `cancelled_setup.em`'s snapshot ends with `task was cancelled`.
- **G-7:** `grep -n "^pub extern submit_\|^pub extern take_conn" std/Tls.em` prints nothing. The three `FunctionRow`s in §5.2-5.3 are read against the five values.
- **G-9 (reading: the job rows sit behind private wrappers, so the check is on the public entries that reach them, in `run/` files, which run in checked mode first):** `grep -rl "Tls.connect_with_ca\|Tls.wrap_with_ca" tests/ui/run | wc -l` prints ≥ 1 [V: 7 on main, 8 with `timeout_connect.em`].
- **G-12:** `grep -rn "docs/plans" crates docs/spec std` prints nothing.
- The UI directory tests stay under the timing budget: the prototype's `ui::run_fail` took 0.33 s and `ui::run` 1.7 s [V].

## 6. Unresolved items (with defaults)

- **U1. No deterministic test of a cancel during the push job.** Default: none (§5.5). Six relay schedules on the virtual clock all cancelled at the client's `Net` read (§7).
- **U2. The `Outcome::Failed` prefix.** Default: `cannot run TLS: {e}`, with no host, because the take does not know it.
- **U3. The interrupt and `exit` poisoning on long verifications** (§5.4). Default: accept it and state it in the spec.
- **U4. The G-4, G-5 and G-9 readings** (§5.7). The conformance reviewer should accept them.

## 7. Additional checks

- [V] **The prototype after the r2 changes** (stub `Roots`, single `fill`, the `pump` dispatch, the new unit test, `timeout_connect.em`): `cargo nextest run` gives 4687 passed. `-p eml_vm --features tls --lib -E 'test(tls::)'` gives 11. `externs::` gives 14. `tls_off::` gives 2.
- [V] **rustls 0.23.46 facts** (§2), read from rustls 0.23.46 (crates.io source) rustls-0.23.46/src: `common_state.rs` 40-41, 77-78, 114-116, 203-215, 476-492; `client/tls12.rs` 885, 1325-1346; `client/tls13.rs` 1192, 1515-1535, 1598-1628.
- [V] **`run_extern` has no scheduler** (vm.rs:3928). `sched.driver_kind` is readable in `async_op` (vm.rs:3286).
- [V] **Cancel experiments on the virtual clock:**
  - A cancel pending at the submit: the closure is dropped and the job never runs.
  - A cancel after the submit: the job runs, then the take wakes `CANCELLED`.
  - The relay variants (a task that forwards the ClientHello and the server flight, then set_nodelay / yield / `start` / `sleep_ms`) never cancelled the client on the push job. The client resumes from its read only at a collect, and the canceller keeps running through the fast paths.
- [V] **The poisoning and REPL behaviour after an abandoned job:** vm.md:73 and :532.

---

