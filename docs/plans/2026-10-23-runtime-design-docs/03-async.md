# S2: std `Async`, structured concurrency, cancellation, I/O backends, channels (r2, after review r1)

Foundation: S1 r3 (dispatch contract `start_task` / `resume_task`, `rt_wait` / `rt_ready`, `run_scheduler` as a VM built-in that parks the caller, `Arc<Cell>` task handles with an `owner` field, one mailbox per worker, rescue stealing of parked or ready tasks, per-worker drivers, `Msg::Completion`, transfer walk with the flip rule, `Panic` reclassified `Never`, writable row bound `<never e>`, worker 0 spawned and worker M = host thread) and S3 r1 (`blocking` → `rt_offload`, `Gui.next_event` as an Async op, plugins as Rust crates). Where S1 r3's *cancellation* contract (hard cancel = drop `k`) does not support scopes and cleanup, this document chooses the model it needs (§Q3) and states it as named requirements R-S1-n (§5); the coordinator reconciles. Tags: [V] verified (how), [I] inferred. Paths relative to /Users/arakaki/Projects/eml. Probes p1-p5 are in the scratchpad `S2-scratch/` (§7); the reviewer's q1-q4 are in `S2-review-scratch/`.

Facts the design rests on:

1. A closed row rejects `Panic`, so a task body typed `Unit -> <IO, Async> Unit` (multicore.md:221) cannot even add two `Int`s [V, probe p1: `[E2002] `+` performs `Panic`, which this lambda does not allow`]. Task rows are therefore `<IO, Panic, Async>` (`Panic` being `Never` in r3 does not remove the need for the label).
2. A `ctl` clause can store a `once` `k` in a value and return; resuming later re-installs the handler; a `never` op aborts correctly after a resume; a `data T : Lin` value survives the suspension [V, probe p2].
3. A suspension inside `try_io`'s body followed by a resume keeps `try_io` catching [V, probe p3]: an IO failure raised by a wrapper after a wake unwinds through the task's own `try_io` frames.
4. Dropping a stored `k` that captures an open `FileIn` closes the file with no leak in checked mode [V, probe p4]. The unwind of a task (by abort or by drop) is a complete cleanup of its `Lin` resources.
5. `List.map` over a `Lin` element type type-checks [V, reviewer's probe q2]; a `try_all` combinator over `try_io`+`try_panic`, the constructor names of Q7, and `Either` with a `Lin` component compile and run [V, probe p5].

## 1. Conclusion

### Q1 `Async` API (module `std/Async.em`)

The effect is an ordinary eml effect (not `extern effect`: extern effects are classified as non-yielding, docs/spec/types.md:212 [V, read], and Async ops must yield). Its raw operations are the ABI between eml and the runtime; users call the wrapper functions. Every wrapper that can park is a **checkpoint**: it is where cancellation is delivered (Q3).

```haskell
pub effect Async where
  await_cell : Cell -> Wake                -- raw: park until a task/scope cell is done (k → rt_wait)
  await_io   : OpId -> Wake                -- raw: park until a driver op completes
  await_chan : ChanWait -> Wake            -- raw: park on a channel side
  await_time : Int -> Wake                 -- raw: park until a deadline (ms)
  reschedule : Unit -> Wake                -- raw: k → rt_ready (yield_now and the op budget)

data Wake = | Woke | Cancelled             -- how the runtime resumes a parked task (R-S1-C1)

-- scopes and tasks
pub run      : (Unit -> <IO, Panic, Async> a) -> <IO, Panic> a                  -- root scope; the VM built-in
pub scope    : (Scope -> <IO, Panic, Async> a) -> <IO, Panic, Async> a          -- nursery; `use s <- Async.scope`
pub extern data Scope                                                           -- Unr handle of an open scope
pub spawn    : (Unit -> <IO, Panic, Async> Unit) -> <Async> Unit                -- into the innermost scope, no handle
pub start    : (Unit -> <IO, Panic, Async> a) -> <Async> Task a                 -- into the innermost scope, with handle
pub spawn_in : Scope -> (Unit -> <IO, Panic, Async> Unit) -> <IO, Panic, Async> Unit   -- Panic if the scope has closed
pub start_in : Scope -> (Unit -> <IO, Panic, Async> a) -> <IO, Panic, Async> Task a
pub cancel_scope : Scope -> <IO> Unit                                           -- cancels every task of it, incl. the body
pub await    : Task a -> <IO, Panic, Async> a                                   -- consumes; re-raises the task's failure
pub cancel   : Task a -> <IO> Unit                                              -- = drop; request only, the scope waits
pub extern data Task a                                                          -- Lin for every a; drop = cancel

-- failures and cancellation
pub data Failure = | IOFailed IOError | Panicked String
pub try_all  : (Unit -> <IO, Panic | e> a) -> <IO | e> Result Failure a         -- try_io + try_panic (probe p5)
pub check_cancelled : Unit -> <IO, Async> Unit                                  -- raises Cancelled if requested
pub shield   : (Unit -> <IO, Panic, Async> a) -> <IO, Panic, Async> a           -- checkpoints inside ignore cancellation

-- control
pub yield_now : Unit -> <IO, Async> Unit
pub sleep_ms  : Int -> <IO, Async> Unit
pub timeout   : Int -> (Unit -> <IO, Panic, Async> a) -> <IO, Panic, Async> Option a   -- None: body cancelled
pub race      : (Unit -> <IO, Panic, Async> a) -> (Unit -> <IO, Panic, Async> b)
              -> <IO, Panic, Async> Either a b                                  -- loser cancelled; Either new in Prelude
pub compute   : (Unit -> <never e> a) -> <IO, Async | e> a                      -- par pool, task parks (Q8)
```
`Prelude` gains `pub data Either a b = | Left a | Right b` (Kind = join of `a`, `b` by the usual rule; `Either FileIn Int` is `Lin` [V, p5]).

- `Task a` is `Lin` regardless of `a` (extern type; a `Foreign` wrapping S1's `Arc<Cell>`). `await` consumes it; `drop`/`cancel` sends `Msg::Cancel` to `cell.owner` (re-checked under the destination's lock, r3 item 2) and releases the handle; for a finished cell the message is `Release` and the stored result is dropped by descriptor. A `Task a` may travel through a `Chan` like any `Lin` value.
- The raw ops carry runtime tokens (`Cell`, `OpId`, `ChanWait`: `Foreign` tokens, std-internal). The handler in `start_task` has one clause per raw op; each clause stores `k` with `rt_wait`/`rt_ready` and returns (S1 dispatch contract). Every raw op returns `Wake`; the wrappers turn `Cancelled` into the `Cancelled` IO failure (Q3).
- **Op budget, in the runtime's fast-path externs** (review B-5): the non-parking fast paths (`try_send`/`try_recv`, immediate completions, `cancel_requested`) increment `ctx.budget`; after 128 (Tokio's constant [V, R4 §1]) the fast path reports "must park" so the wrapper goes through `reschedule`, which resets the counter. No per-op Rust call in the handler, no interaction with the VM's safe-point preemption (the two cover VM code and native code respectively).

**Example 1: echo server, one task per connection, failures isolated per connection.**
```haskell
import Async
import Net
import Bytes

serve : TcpStream -> <IO, Panic, Async> Unit
serve conn =
  let (chunk, conn) = Net.read_chunk (Bytes.empty ()) conn
  if Bytes.length chunk == 0 then Net.close conn
  else serve (Net.write_bytes chunk conn)

-- a connection's failure (peer reset, a Panic in the handler, a timeout) is logged, not propagated
connection : TcpStream -> <IO, Panic, Async> Unit
connection conn =
  match Async.try_all (fn () -> serve conn) with
    | Ok () -> ()
    | Err f -> eprintln (show f)

accept_loop : TcpListener -> <IO, Panic, Async> Unit
accept_loop l =
  let (conn, l) = Net.accept l                -- ECONNABORTED/EINTR are retried inside accept; EMFILE is an IO failure
  Async.spawn (fn () -> connection conn)      -- conn (Lin) moves into the child
  accept_loop l

main : Unit -> <IO, Panic> Unit
main () =
  use Async.run
  accept_loop (Net.listen "0.0.0.0" 8080)
```
`connection` is the shape users copy: `try_all` catches both IO failures and `Panic` (probe p5), so one connection's divide-by-zero does not end the server. Without it, `spawn`'s Trio semantics apply: the failure is the scope's failure and the server stops (Q2, Q4). A max-connections limit is a `Chan Unit` used as a semaphore: `Chan.new n` pre-filled with `n` units; `accept_loop` does `Chan.recv sem` before `accept` and the child does `Chan.send () sem` in `connection` after the match (channel ops are `<IO, Async>`; it runs even after a cancellation because `send` on a channel with room never parks — fast path).

**Example 2: fan out N probes, gather, partial failure tolerated.**
```haskell
probe : String -> <IO, Panic, Async> Result Failure Int        -- bytes of the first chunk of a greeting
probe host =
  Async.try_all (fn () ->
    let conn = Net.connect host 7
    let (chunk, conn) = Net.read_chunk (Bytes.empty ()) (Net.write_line "hello" conn)
    Net.close conn
    Bytes.length chunk)

probe_all : List String -> <IO, Panic> List (Result Failure Int)
probe_all hosts =
  use Async.run
  let tasks = List.map (fn h -> Async.start (fn () -> Async.timeout 5000 (fn () -> probe h))) hosts
  List.map (fn t -> match Async.await t with | Some r -> r | None -> Err (IOFailed (timed_out h))) tasks
```
(`timed_out` builds an `IOError { kind = TimedOut, … }`; a real program would carry `h` with the task.) Rule (review C-3): **a task's uncaught failure is the scope's failure at the moment it happens**, whether or not anyone awaits it; `await` re-raises it to the awaiter as its first notification, and the scope re-raises it at exit regardless. Isolation therefore always happens *inside* the task (`try_all` returning a `Result`), never around `await`. The wrapping of `timeout` is outside `try_all` here on purpose: a timed-out probe is `None`, a failed one is `Err`.

**Example 3: a read with a deadline.**
```haskell
-- (a) idle timeout: cancel the whole connection
read_or_close : TcpStream -> <IO, Panic, Async> Option (Bytes, TcpStream)
read_or_close conn = Async.timeout 10000 (fn () -> Net.read_chunk (Bytes.empty ()) conn)

-- (b) request timeout: keep the stream to answer 408
read_request : TcpStream -> <IO, Panic, Async> TcpStream
read_request conn =
  match Net.read_chunk_within 10000 (Bytes.empty ()) conn with
    | (Some req, conn) -> handle req conn
    | (None, conn) -> Net.close (Net.write_line "408 Request Timeout" conn)
```
(a) cancels the body task; the unwind drops the captured `Lin` `conn`, which closes it. (b) is the deadline form (review C-7): the op is cancelled at the deadline, the buffer and the handle come back, nothing is closed.

### Q2 Structured concurrency

- **Scopes, Trio/Eio shape; no free `spawn`.** `spawn`/`start` attach the child to the innermost dynamically enclosing scope (the task cell's `scope` field); `spawn_in`/`start_in` attach to a named open scope (an `Unr` `Scope` handle, Eio `Switch`/Trio nursery object; using it after the scope closed is a `Panic`). `scope f` opens a child scope cell, runs `f s` in the *current* task with that scope as current, then waits until the scope's live counter is 0. A task never outlives its scope. `run` is the root scope (the VM built-in that parks the caller, r3 item 1; native nests the loop).
- **Scope exit protocol**, all in eml in `std/Async`, so it runs on every path (review B-1):
  ```haskell
  scope f =
    let s = open_scope ()                                   -- extern: child cell of the current scope
    let r = try_all (fn () -> f s)                          -- the body's abort (IO failure, Panic, Cancelled) lands here
    match r with | Err _ -> cancel_scope s | Ok _ -> ()     -- body failed: cancel every child
    wait_closed s                                           -- shielded await_cell on the scope cell (see below)
    match (first_failure s, r) with                         -- first recorded failure wins; Cancelled never beats a real one
      | (Some (IOFailed e), _) -> fail_io e.kind e.message
      | (Some (Panicked m), _) -> panic m
      | (None, Err f) -> rethrow f
      | (None, Ok x) -> x
  ```
  - **When siblings are cancelled** (review A-1): **at the moment a task of the scope fails** (its bottom records the failure in the scope cell and the runtime cancels every task of the scope, including the body's own task, R-S1-C3). The body receives it at its next checkpoint, *including a park in a driver op* (`accept`): the runtime cancels the op, and on its completion resumes the body with `Cancelled` (R-S1-C2). So an idle server whose connection task fails stops promptly; with `connection` of Example 1 it does not stop at all.
  - `wait_closed` is a shielded park: if the body's own task is cancelled while waiting for its children (nested scopes under `timeout`/`race`), the children have already been cancelled by R-S1-C3 and the wait continues until the counter is 0; the task then re-raises `Cancelled` (or the first failure) to its parent. No orphan children, no hanging counter.
  - Outcome priority: first recorded failure of the scope (body or child, in completion order; a `Cancelled` is recorded only if nothing else is), else the body's value. First failure wins, the others are dropped silently and counted in `RunStats` (no MultiError).
- **`main` becomes async by writing `use Async.run`**; the entry rule (`Unit -> <IO, Panic> Unit`, docs/spec/modules.md:15 [V, read]) is unchanged. `run` inside a task parks the calling task until the inner root scope is empty (r3 item 1); semantically it is `scope`. Documented, not recommended.
- **Graceful shutdown** (review C-1) is written with a named connection scope and a signal source:
  ```haskell
  main () =
    use Async.run
    use conns <- Async.scope                                -- in-flight connections live here
    let l = Net.listen "0.0.0.0" 8080
    Async.race (fn () -> accept_loop conns l) (fn () -> Signal.wait Terminate)   -- SIGTERM cancels the acceptor
    Async.spawn_in conns (fn () -> Async.sleep_ms 30000; Async.cancel_scope conns)   -- drain deadline
    -- the connection scope's exit waits for the drain
  ```
  with `accept_loop conns l` doing `Async.spawn_in conns (fn () -> connection conn)`. `std/Signal.em`: `data Signal = | Interrupt | Terminate` and `wait : Signal -> <IO, Async> Unit` (submit/await/take over the driver: `signalfd`/`EVFILT_SIGNAL`/Windows console handler → completion). While a task is parked in `Signal.wait Interrupt`, Ctrl-C is delivered to it instead of being the fatal interrupt; a second Ctrl-C is `_exit(130)` as today (docs/spec/vm.md 「上限と割り込み」); with no waiter the current behaviour stands. `Terminate` is `SIGTERM` (Windows: `CTRL_CLOSE_EVENT`/`CTRL_SHUTDOWN_EVENT`).
- Handles may escape their scope (no scope type parameter: that would need a rank-2 special form like `run_heap`; rejected). An escaped `Task a` is finished, failed or cancelled; `await` on it returns or re-raises.

### Q3 Cancellation — the model (owner: S2)

- **Delivered as a value at checkpoints; the task unwinds itself.** A cancellation request (`cancel`/`drop` of the handle, `cancel_scope`, a sibling's failure) sets the cell's cancel bit. When the task next performs a raw Async op, or if it is parked in one, the runtime resumes `k` with the immediate word `Wake.Cancelled` (R-S1-C1); the std wrapper raises the IO failure of kind `Cancelled` (new `IOErrorKind` variant, message `task was cancelled`) through the std-internal extern `fail_io : IOErrorKind -> String -> <IO> a` (A-3 below). The failure unwinds the task's frames, releasing `Lin` values (fact 4), **running the task's own `try_io`/`try_all` bodies and the scope exit protocol on the way** (fact 3), until it reaches `start_task`, which records `Cancelled` and wakes awaiters and the scope. This is the model the coordinator sketched; S1 r3's hard drop remains the last resort (R-S1-C4): at `exit`, at a fatal error, and for a task still parked when the run ends.
- **Sticky**: after the first delivery, every later checkpoint of that task raises again, *except inside `shield`*. So cleanup that needs no checkpoint (counters, `Chan.close`, `try_send`, `eprintln`, closing handles) runs in the task's `try_all` handlers unchanged (review B-2); cleanup that must park (telling a registry actor through a full channel, a goodbye write) is wrapped in `Async.shield`, whose checkpoints ignore the cancel bit (Trio's shielded scope); a shielded region is the user's bounded promise, and the scope waits for it. No `finally`, no user-defined drop glue: the `Lin` rule plus `try_all` plus `shield` cover the server cases (connection counter decrement, channel close, registry update), and all three exist today or are one function each.
- **Soft check**: `check_cancelled ()` raises if a request is pending; `cancel_requested ()` returns the bit. For loops without checkpoints.
- **Cooperative, so CPU-bound code is not interrupted** (review C-2): a task in a pure loop is cancelled at its next checkpoint; `timeout` around `compute` returns when the `par` job finishes (a1; a2 adds `par` cancellation). The rule for servers: put CPU-heavy work in `compute`, which parks the task so its worker keeps serving its other connections, and the job runs on the `par` pool where helping and stealing apply. A native task that neither parks nor computes holds its worker until it ends; its sibling tasks are rescued by S1's monitor (stuck after 2 ticks, run queue > 64) — that is S1's bound, repeated here so the implementer knows the mitigation and its latency (~20 ms).
- **In-flight I/O on completion backends**: a cancelled task parked on a driver op is not resumed until the op has completed: the runtime asks the driver to cancel it (`CancelIoEx`, `io_uring` `ASYNC_CANCEL`, deregister + drop on epoll/kqueue), the completion arrives as `Msg::Completion` (status cancelled, or the data if it raced), and only then is the task resumed with `Cancelled` (R-S1-C2); `take_*` returns the buffer and the handle, which the unwind then drops. Buffers are never freed while the kernel may write to them (the hazard monoio/tokio-uring document [V, R4 §3]). The hard-drop last resort (R-S1-C4) drops the `Lin` op token, whose drop glue orphans the op: the driver frees the buffer at completion.
- **Disarming other waits** (review B-7): resuming a parked task with `Cancelled` first removes its registration — the channel waiter entry (an item is never handed to a removed waiter; a rendezvous `send`'s value stays in the sender's frame and is dropped by the unwind, i.e. finalized, not leaked), the timer entry, the cell's awaiter entry (R-S1-C2). The "never lost" claim of Q7 is about the `Rejected` return on a closed channel, not about cancellation.
- Rejected: hard cancel as the primary mechanism (r3 item 4): it skips the scope exit protocol (review B-1) and every non-`Lin` cleanup (B-2); a `never` op in `Async` for cancellation (works, probe p2, but is a second catch mechanism users cannot handle); user-defined drop glue (contradicts docs/spec/linearity.md 「破棄処理」: drops run no eml code).

### Q4 `IO`, `try_io`, `exit`, `Panic`, user effects, `Heap`, `var` inside tasks

- **Each task is an abort boundary.** An IO failure or `Panic` no handler inside the task catches reaches `start_task`'s bottom (`try_all` there), which records it in the cell (`Done(value) | Failed(Failure) | Cancelled`), marks the scope and triggers the scope's cancellation (Q2).
- **Where it goes**: to the awaiter (`await` re-raises: IO failures via `fail_io kind message`, `Panic` via `panic message`, faithful because `try_panic` only sees the message, docs/spec/stdlib.md:91 [V, read]) and to the scope (re-raised at exit). A parent's `try_io` catches a child's failure exactly when it wraps the `await` or the `scope`; isolation is done inside the task with `try_all` (Example 1, 2).
- **`fail_io`** (review A-3): a new extern row `Prelude.fail_io : IOErrorKind -> String -> <IO> a` (`Repr::Enum` parameter, `OpRet::Never`), implemented as a new `Fault::Raised { kind: u8, message }` that the VM's `failed` routes to the `io_failed` clause entry exactly like `Fault::Io` today (docs/spec/vm.md 「`IO` と `Panic` への中断」), with the kind tag passed through instead of mapped from `IoReason`. `Proc.exited` becomes a use of it. New `IOErrorKind` variants: `Cancelled`, `TimedOut`, `ConnectionReset`, `AddrInUse` (kind-2 test change for `show` snapshots).
- **`exit n` in any task ends the program** (review C-8: the rationale is simply that `exit` means process exit everywhere; a library handler must not call it, and `try_all` cannot stop it): the abort reaches the task bottom as `Exit(n)`; the runtime raises the session stop with the code; workers stop at their next safe point; in-flight driver ops are abandoned (the process exits; the driver's teardown cancels them so no completion writes into freed memory before `_exit`); the root `run` returns the exit to `main`'s bottom; stdout flushed; exit `n & 255`. Heaps are dropped, so `FileOut` buffers are written and children are killed by drop glue, as today.
- **Runtime errors** (heap errors, limits, interrupt without a `Signal.wait Interrupt` task) stay whole-run fatal, reported once.
- **User-defined effects** must be handled inside the task closure (closed row; the child's `evv` starts empty); a user handler inside a task that stores `k` works as today. **`Heap h`** is excluded by the closed row. A **`var`** of the parent captured by a task closure is a type error (docs/spec/types.md 「範囲の外へ出る閉包」 [V, read]); a `var` inside the task body is fine.

### Q5 I/O backend

- **The std API is completion-shaped with owned buffers**, on every OS. An async op is three steps, all in std: `submit_*` (extern: moves the buffer and the handle's registration into the driver, returns a `Lin` `OpId` token), the raw `await_io token` (parks), `take_*` (extern: returns `(Bytes, Handle)` or raises the op's IO failure; after `Cancelled`, returns them with a cancelled status). Externs cannot yield (docs/spec/effects.md:62-63 [V, read]), so the park is an eml-level op; the triple is the uniform shape for sockets, timers, signals, `Gui.next_event` (S3 Q8) and `compute`.
  ```haskell
  pub read_chunk : Bytes -> TcpStream -> <IO, Async> (Bytes, TcpStream)
  read_chunk buf s =
    let op = submit_read buf s
    match await_io op with
      | Woke -> take_read op s
      | Cancelled -> let (buf, s) = take_read op s in drop buf; drop s; cancelled ()
  ```
  `read_chunk_within ms buf s` = `submit_read`, then `await_io_until op deadline` (a raw-op variant that arms a timer on the park; the runtime cancels the op at the deadline and resumes with `Woke`, R-S1-C2), then `take_read` with status `TimedOut` → `(None, s)`. Same for `read_line_within` and `accept_within`.
- **Buffer ownership and size** (review B-6): the buffer is a `Bytes` passed by value. `submit_read` takes the word; if the block is not uniquely owned (rc ≠ 1, which includes shared-marked rc < 0 blocks: `is_unique` is rc == 1, OM:321 [V, S1]) it copies into a fresh unique block; it then ensures capacity `length + 64 KiB` (reallocating the unique block if needed) and hands `(payload + length, 64 KiB)` to the kernel. The word sits in the driver's pending table, a registered root of the worker (S1 invariant 2), and comes back with its length advanced. `write_bytes` moves the buffer the same way (copy if not unique) and frees it after completion; it does not return it. Perceus reuse is not relied on; the per-iteration allocation of Example 1 is one block of 64 KiB (`Bytes.empty ()` has capacity 0; the extern grows it), which is the cost to beat later with a returned write buffer if profiles show it. S1 req 2's "`Lin`" is read as "uniquely owned word" (R-S1-6).
- **Driver**: a `Driver` trait in `eml_rt` next to `Host` (word-level, `external_body`, trust class T11), implemented in a new crate `eml_io`: `RealDriver` (per worker, supplies `Park`/`Unpark`) and `TestDriver` (virtual clock, in-memory streams, seeded completion order) behind the same trait; `eml_test_support` selects. Default implementation of the real driver: **`compio-driver` + `compio-buf`** (IOCP native, io_uring, `polling` for kqueue/epoll, owned buffers) [I: R4 §1 confirms the backends and the owned-buffer API; standalone use, per-instance ownership and an interruptible wait are unverified offline]. **A spike (M-A2s) decides this before M-A2** with go/no-go criteria (§5); fallback: own driver over `polling` + hand-written IOCP (`windows-sys`). Rejected: mio (AFD readiness emulation on Windows over undocumented APIs [V, R4 §3]).
- **Handle/worker binding**: user-visible rule: none. The runtime binds a handle to the driver of the worker that runs its *first* `submit_*`; an accepted socket is created unregistered and binds wherever its child task first reads, so rescue stealing between `spawn` and the first op costs nothing (review B-8). A later move (rescue of a parked/ready task, r3 item 5) makes the next `submit_*` go through the binding worker's mailbox (`Msg::Submit`) with the completion delivered to `cell.owner` (R-S1-1; required for correctness, not an optimization). A single acceptor per listener is M-A's shape; `SO_REUSEPORT` multi-acceptor is later (B-8: accept is cheap relative to a connection's life; the M-A4 scaling criterion is on the echo workload, where it is not the bottleneck at N ≤ 4 [I]).
- **Blocking std rows inside tasks** (review C-4; corrects r1): an extern cannot park, and a row without `Async` in its type cannot yield, so **the existing synchronous rows (`FileIn`, `Stdin`, `Proc`, DNS) run inline in a task and block its worker**; S1's rescue stealing moves that worker's other tasks after ≤ 2 ticks. This is acceptable for occasional file reads in a server and wrong for pipe pumping, so the three `Proc` roadmap items are *not* discharged by offload; they move to **M-A3: async files and pipes in the driver**, i.e. `Async`-rowed variants (`Fs.read_text_async`? no: a module `AFile`/`Proc` ops with `<IO, Async>` rows over the triple, child stdio as driver handles: pollable pipes on Unix, overlapped named pipes on Windows). S3's `blocking`/`rt_offload` therefore applies to **user `extern "C"` rows that carry `<IO, Async>`** and are lowered to the triple (submit = post the thunk to the pool, await, take = result); a `blocking` row without `Async` is inline. R-S3-1 restated accordingly. DNS inside `connect` is a pool job through the same triple (M-A2).

### Q6 Network and OS surface; first milestone

`std/Net.em` (M-A2):
```haskell
pub extern data TcpListener                   -- Lin
pub extern data TcpStream                     -- Lin
pub listen         : String -> Int -> <IO> TcpListener               -- host, port (0 = ephemeral)
pub local_port     : TcpListener -> (Int, TcpListener)
pub accept         : TcpListener -> <IO, Async> (TcpStream, TcpListener)          -- retries ECONNABORTED/EINTR
pub accept_within  : Int -> TcpListener -> <IO, Async> (Option TcpStream, TcpListener)
pub close_listener : TcpListener -> <IO> Unit
pub connect        : String -> Int -> <IO, Async> TcpStream         -- DNS on the pool, then async connect
pub read_chunk     : Bytes -> TcpStream -> <IO, Async> (Bytes, TcpStream)        -- appends ≤ 64 KiB; nothing = EOF
pub read_chunk_within : Int -> Bytes -> TcpStream -> <IO, Async> (Option Bytes, TcpStream)
pub read_line      : TcpStream -> <IO, Panic, Async> (Option String, TcpStream)  -- eml over read_chunk + per-stream buffer
pub read_line_within : Int -> TcpStream -> <IO, Panic, Async> (Option (Option String), TcpStream)
pub write          : String -> TcpStream -> <IO, Async> TcpStream
pub write_bytes    : Bytes -> TcpStream -> <IO, Async> TcpStream     -- whole buffer; loops internally
pub write_line     : String -> TcpStream -> <IO, Async> TcpStream
pub shutdown_write : TcpStream -> <IO> TcpStream                      -- FIN; reads still allowed
pub close          : TcpStream -> <IO> Unit                           -- = drop
pub peer_addr      : TcpStream -> (String, TcpStream)
pub set_nodelay    : Bool -> TcpStream -> <IO> TcpStream
```
Names follow docs/spec/stdlib.md 「ハンドル」 (handle last, result first). Errors are IO failures: existing kinds plus `Cancelled`, `TimedOut`, `ConnectionReset`, `AddrInUse`; `Other` stays the catch-all.

| Item | M-A | Later |
|---|---|---|
| TCP listener/stream, deadlines (`_within`) | yes | `SO_REUSEPORT` multi-acceptor, UDP |
| Timers | `sleep_ms`, `timeout`, `race`, deadlines (driver timer wheel; virtual clock in tests) | `Time` module |
| Signals | `std/Signal.em` `wait : Signal -> <IO, Async> Unit` (Q2) | more signals |
| DNS | `getaddrinfo` on the blocking pool via the triple, inside `connect`; `resolve` | async resolver |
| TLS | no; later a Rust plugin (`eml_tls` over rustls, S3 Q8 pattern) | plugin |
| Files, pipes, `Proc` in tasks | inline (block the worker; rescue covers) | M-A3 async files/pipes in the driver; the three `Proc` roadmap items land there |
| HTTP | no (`Http` is a library on `Net` + `Chan`) | later |

### Q7 Inter-task communication (`std/Chan.em`), shared state, lock-free structures

```haskell
pub extern data Chan a                                  -- Unr handle; one type, MPMC, Go-shaped
pub new      : Int -> <IO> Chan a                       -- capacity; 0 = rendezvous
pub send     : a -> Chan a -> <IO, Async> SendResult a
pub recv     : Chan a -> <IO, Async> Option a           -- None: closed and drained
pub try_send : a -> Chan a -> <IO> TrySendResult a      -- never parks
pub try_recv : Chan a -> <IO> TryRecvResult a           -- never parks
pub close    : Chan a -> <IO> Unit                      -- idempotent; wakes every parked side
pub merge    : List (Chan a) -> <Async> Chan a          -- one forwarder task per source, in the innermost scope
pub data SendResult a     = | Sent | Rejected a         -- Rejected: closed; the value comes back
pub data TrySendResult a  = | Accepted | Full a | Refused a
pub data TryRecvResult a  = | Got a | Empty | Drained
```
Constructor names are distinct per module (E1003 forbids reuse across `data` in one module; the reviewer's probe q1 and probe p5 [V]).
- Payloads of any Kind: `Unr` values are marked at send and transfer-walked at receive (S1 req 4; the walk descends only through rc == -1 objects, r3 item 6: a payload the sender keeps stays shared and unreusable); `Lin` values move. When the last handle of a channel is dropped, queued items are dropped by descriptor (finalized like a dropped `Task` result; review C-6 (1): this is a finalization, not a skipped cleanup, and the same thing happens to a `Lin` value in any dropped continuation).
- The channel object is a runtime object (Rust MPMC queue with a mutex first; lock-free later as an internal change), its words registered as roots; senders push directly, parked sides are woken through their worker's mailbox (S1 req 4). Bounded → back-pressure; "unbounded" = a large capacity. A semaphore is a `Chan Unit` (Example 1).
- `send`/`recv` are checkpoints; fast paths (`try_*`, and the inlined fast path of `send`/`recv`) are externs that also count the op budget (Q1).
- **No separate oneshot** (`Task a` + `await`; `Chan.new 1`), no broadcast/watch, **no multi-channel `select`** in M-A: racing is `race`/`timeout`, fan-in is `merge`.
- **Shared mutable state**: the idiom is an owning task behind a `Chan` (actor). `std/Atomic.em` provides `Counter` only (`new : Int -> <IO> Counter`, `add : Int -> Counter -> <IO> Int`, `load`, `store`; `Unr`; no CAS). No atomic `Cell`, no mutex.
- **Lock-free queues and atomics stay runtime internals**: users have no memory model or atomics API (Koka refuses too [V, R4 §1]); multicore.md decision 2; Saturn is an opam library, not OCaml's Stdlib [V, R4 §1]. A lock-free MPMC can replace the mutex queue behind `Chan` later (an `eml_rt` trusted row with Miri/TSan).

### Q8 `Async.compute`

`compute : (Unit -> <never e> a) -> <IO, Async | e> a` (r3's writable bound; one spelling, review A-4). The result row carries `e` so the child's `never` operations — including `Panic`, now a `Never` label, and `Fail x` — are re-performed in the calling task after the join, exactly as `par` a1 does on the parent (S1 r3 B-6 `fork$`/`reperform$`). Body: a `par` node with one child, `await_cell` on the join node (owner mailbox wake, S1 req 8), transfer walk of the result, re-perform if the child aborted. Cancellation takes effect after the join (a1). `Par.map` etc. inside a task remain plain helping.

### Q9 Testing and determinism

- **Seeded deterministic scheduling at N = 1** (`--sched seeded:<n>`; the UI runner's default for `tests/ui/run/async/`): safe-point counter preemption (S1), seeded choice among a worker's ready items, and `TestDriver` with seeded completion order and a **virtual clock** (`sleep_ms`/deadlines advance virtual time only when every task is parked), so timeout and cancellation tests run in microseconds and never depend on machine speed (the 1 s rule). `TestDriver` also serves in-memory `TcpStream` pairs (`listen`/`connect` on a virtual host) and virtual signals (`Signal.wait` woken by a test hook).
- **Real sockets**: a small smoke set in `tests/ui/run/net/` over loopback with port 0, one connection at a time or with order-independent output, N = 1; the multi-worker job (N ∈ {2, 8}) runs only order-independent ones; the Windows CI job runs the same set on IOCP (M-A2w).
- Negative/semantic tests: a cancelled task's `FileIn`/`TcpStream` is closed (checked-mode leak check is the oracle, probe p4); a failing child cancels siblings *and* an acceptor parked in `accept` (virtual stream) promptly; `try_all` isolation keeps the server up after a `Panic` in a connection; scope exit under `timeout` with a nested scope leaves no orphan (counter reaches 0); `shield` lets a goodbye `send` complete under cancellation; a sticky cancel fails a second checkpoint; in-flight read cancelled on the test driver returns the buffer before raising; `read_chunk_within` returns `(None, s)` with the stream usable; rendezvous send cancelled: value finalized, receiver not woken; op budget forces a switch after 128 fast-path ops; `exit` in a child ends the run with its code; `Signal.wait Interrupt` turns a simulated Ctrl-C into a cancellation.
- `RunStats` gains `tasks`, `parks`, `cancels`, `channel_ops`, `completions`; pinned under `sequential`.

## 2. Rationale

- Closed task rows with `Panic` (fact 1): `<IO, Panic, Async>` is "main's row plus Async".
- Cancellation as a delivered value + IO failure: the task unwinds through its own frames, so `try_all` cleanup and the eml scope protocol run on every path (facts 3, 4); hard drop cannot run the scope protocol (review B-1) or any non-`Lin` cleanup (B-2). Sticky delivery plus `shield` gives bounded cleanup without a `finally`.
- Nurseries with a `Scope` handle: a function's children and their handles are finished when it returns (Trio [V, R4 §2]; Eio `Switch` [V, R4 §1]); the handle makes drain/shutdown and "spawn into the outer scope" expressible without changing `spawn`.
- Failure = scope failure immediately: the one rule that makes test expectations derivable (A-1, C-3); isolation inside the task is one combinator (`try_all`).
- Submit/await/take triple: externs cannot yield; one shape for sockets, timers, signals, GUI events, pool jobs and `compute`.
- Owned buffers with copy-if-not-unique: the only shape correct on IOCP/io_uring (R4 §3); `is_unique` is already the runtime's test (OM:321).
- Go-style `Chan` with explicit `close` and value return on `Rejected`: `Unr` handles cannot signal "all senders dropped"; `Lin` payloads are never silently consumed by the API.
- Inline blocking rows: the honest consequence of "externs cannot park" (C-4); rescue stealing is S1's safety net; the real fix is driver-level files/pipes (M-A3).

## 3. Alternatives considered and rejected

- `Async` as `extern effect` with runtime ops: extern effects are non-yielding (types.md:212).
- Async-capable extern rows (an extern that parks): every extern call a yield point; the triple costs two calls and no compiler change.
- Hard cancel (drop `k`) as the primary mechanism: see Q3.
- Cancellation as a `never` op of `Async`: a second catch mechanism users cannot handle.
- User-defined drop glue / `finally`: contradicts the no-eml-code-in-drops rule; `try_all` + `shield` suffice.
- Scope-typed handles (rank-2 `scope`): a second `run_heap`-style special form; escaped handles are harmless.
- Free `spawn` (Tokio): leaks `Lin` resources at program end and loses failures.
- Awaiter-only failure routing (a `start` variant whose failure does not fail the scope): two semantics for one operation; `try_all` inside the task is the one idiom.
- Multi-channel `select`: needs language support or a cross-channel registration protocol; `race`/`timeout` + `merge`.
- Exposing atomics/lock-free queues: no memory model for users.
- Separate sender/receiver channel types: drop-based close that `Unr` handles cannot provide.
- mio for the driver: AFD readiness on Windows, not native IOCP.
- Offloading std's sync rows from inside the extern: impossible without parking inside an extern (C-4).
- Deadlines as per-stream state (`set_read_timeout`): hidden state on the handle; the explicit `_within` variants keep the row and the result visible.

## 4. Assumptions made

- A1: `compio-driver` fits (standalone, per-instance, interruptible wait); decided by the M-A2s spike. If not: own driver, no API change.
- A2: `List.map` over `Lin` elements type-checks — confirmed by probe q2; retired as an assumption.
- A3: adding `IOErrorKind` variants and `Either` to Prelude are kind-2 test changes (`show` snapshots, Kind export).
- A4: `Fault::Raised` with an arbitrary kind tag fits the VM's `failed` routing with a small change (one new `Fault` variant and a table row).
- A5: the `Cancelled` immediate word injected by the runtime is well-typed for every raw op because every raw op returns `Wake` (R-S1-C1 fixes this as a std/ABI invariant checked by a test).
- A6: recovery (REPL) with in-flight driver ops cancels them and drains completions before the copy.
- A7: a single acceptor is not the bottleneck of the echo workload at N ≤ 4.

## 5. Implications for implementation

Constraints:
- `eml_rt` gets `Driver` (trait) and the channel object; `eml_io` implements `RealDriver`/`TestDriver`; std modules `Async`, `Chan`, `Net`, `Signal`, `Atomic`; Prelude gets `Either`, `fail_io`, the new `IOErrorKind` variants; extern table rows for the tokens (`Cell`, `OpId`, `ChanWait` as `Foreign`), `Task`/`Scope`/`Chan`/`Counter`/`TcpListener`/`TcpStream` as `Foreign` with drop glue, submit/take rows, scope rows, timer rows, with the tie tests of `crates/eml_core_ir/tests/externs.rs`. No new syntax; `<never e>` written only in std.
- Every parking path is a raw Async op handled in `start_task`; no extern parks; every raw op returns `Wake`.

Named requirements on S1 (cancellation model; replace r3 item 4 and r1's R-S1-2/R-S1-7):
- **R-S1-C1**: the runtime resumes a parked or arriving-at-checkpoint cancelled task by calling `resume_task(k, Wake.Cancelled)` (an immediate word std declares); it never drops `k` as the primary cancellation. All raw Async ops return `Wake` (std invariant, tested).
- **R-S1-C2**: before resuming with `Cancelled`, the runtime disarms the wait: driver op → cancel it and wait for its completion (resume only after `Msg::Completion`); timer → remove the entry; channel wait → remove the waiter so no item is delivered to it; cell await → remove the awaiter. `await_io_until` arms a timer that cancels the op at the deadline and resumes with `Woke`.
- **R-S1-C3**: a task's bottom recording a failure in its scope cell cancels every task of that scope (including the body's task) and every nested scope's tasks; `cancel_scope` does the same on request. Scope cells record the first failure and expose a per-task "scope failed" flag (`rt_scope_failed`, one atomic load).
- **R-S1-C4**: hard drop of `k` (r3 item 4) only at `exit`, at a fatal error, and for tasks still parked at run end; the `Lin` op token's drop glue orphans a driver op.
- **R-S1-1**: a `submit` on a handle bound to another worker's driver is forwarded through that worker's mailbox (`Msg::Submit { op, cell }`); its completion is delivered to `cell.owner`.
- **R-S1-4**: `ctx.budget` counter reset at dispatch (op budget, Q1).
- **R-S1-5**: `exit` inside any task propagates through the parked `run_scheduler` to `main`'s bottom and the CLI's exit code; other workers stop at their next safe point; the driver is torn down before `_exit`.
- **R-S1-6 (clarification)**: req 2's "`Lin` buffers" = uniquely owned words (lin bit, or rc == 1 after copy-if-shared).
- **R-S1-7**: signals: `SIGTERM` and a Ctrl-C with a registered `Signal.wait` waiter are delivered to the driver as completions instead of setting the fatal interrupt; the second Ctrl-C keeps `_exit(130)`.
- **R-S3-1 (restated)**: `blocking` applies to user `extern "C"` rows whose type carries `<IO, Async>`; such a row is lowered to the submit/await/take triple with the thunk posted to the pool; a `blocking` row without `Async` runs inline. `rt_offload` is therefore "submit a pool job + completion", not a call that parks.
- **R-S3-2**: plugins (GUI, later TLS) use the triple with `OpId` tokens delivered as `Msg::Completion`; `Gui.next_event` parks with `await_io`.
- **R-eml_io**: every driver wait returns on `Unpark` and on the monitor's wake (r3 item 10).

Milestones (S1 M1 = single-worker scheduler is the gate):
- **M-A0 `std/Async` core** (on S1 M1, N = 1): effect and raw ops, `Wake`, `run`, `scope`/`Scope`, `spawn`/`start`/`spawn_in`/`start_in`, `await`, `cancel`, `cancel_scope`, `Failure`/`try_all`, `check_cancelled`/`cancel_requested`, `shield`, `yield_now`, `sleep_ms`, `timeout`, `race`, `Either`, `fail_io` + `Fault::Raised`, new `IOErrorKind` variants, scope exit protocol, op budget, `RunStats` fields, `TestDriver` with the virtual clock (timers only), raw-op visibility decision (§6). Done when: `scheduler.em`-style programs on std give the same output; the semantic tests of Q9 pass; every `tests/ui/run/async/` test is byte-identical for two seeds.
- **M-A1 `std/Chan` + `Atomic.Counter`**: channel object, send/recv/try/close/merge. Done when: ping-pong, pipeline and fan-in tests pass seeded; a `Lin` payload crosses a channel and is finalized by the receiver's drop; close wakes parked sides; cancelled rendezvous leaves no item.
- **M-A2s driver spike** (before M-A2; one to two weeks): `Driver` trait draft in `eml_rt`; a Rust-only prototype of `RealDriver` over `compio-driver` on macOS (kqueue) and on a Windows runner (IOCP): per-instance driver, owned-buffer read/write/accept, cancel of an in-flight read returning the buffer, wait interruptible by a cross-thread wake, timers. Go/no-go decides A1; the fallback is own code.
- **M-A2 `std/Net` + `std/Signal` + `eml_io`** (on S1 M1 for N = 1; macOS/Linux): `Driver` trait final, `RealDriver`, `TestDriver` with in-memory streams and virtual signals, TCP surface incl. `_within`, DNS pool job, `Signal.wait`. Done when: Example 1 serves 1000 sequential connections in a test and 100 concurrent ones in an `#[ignore]` timing test; a failing connection without `try_all` stops an idle server promptly (virtual streams); `read_chunk_within` keeps the stream; graceful shutdown drains under a virtual `SIGTERM`.
- **M-A2w Windows**: the same UI set on IOCP in CI. Done when it passes.
- **M-A3 async files and pipes in the driver**: `<IO, Async>` file and child-stdio operations (pollable pipes / overlapped named pipes), `Proc.output_with` (stdin from a string), `Child` stderr reads, eml-pumped `.|`. Done when: the three roadmap items have UI tests on the test driver and loopback pipes; a file read in one task does not stall a ping-pong pair (test clock).
- **M-A4 multi-worker** (with S1 M2): placement, forwarding (R-S1-1), order-independent net tests at N ∈ {2, 8}, `compute` over the `par` pool (with S1 M0/M2b), user `blocking` rows via the triple (with S3 N6). Done when: the echo server scales monotonically at N ∈ {1, 2, 4} (`#[ignore]`); checked-mode leak/owner checks pass with rescue stealing forced (`--sched rescue-always`).
- Later: UDP, `SO_REUSEPORT`, TLS plugin, broadcast/watch channels, `Time`, `Http`.

## 6. Unresolved items (default if unresolved)

- Visibility of the raw ops of `pub effect Async` (default: visible, documented internal; a module-private-op rule is a small declarations.md change, consistent with how `io_failed` is hidden).
- Time unit and type (default: `Int` milliseconds).
- Multi-channel `select` (default: none; `race`/`timeout` + `merge`).
- `Atomic` beyond `Counter` (default: none).
- Nested `Async.run` inside a task (default: allowed, S1's park).
- Placement API (`spawn_here`) (default: none; `Main.run` for thread-bound work).
- Driver crate (default: `compio-driver`, decided by M-A2s).
- Channel queue implementation (default: mutex; lock-free later, internal).
- Op budget size (default 128).
- Returning the buffer from `write_bytes` for reuse (default: no; revisit with M-A2 profiles).
- MultiError-style reporting of secondary failures (default: first failure wins, others counted).

## 7. Additional checks

- Probes under `timeout 60` with `cargo run -q -p eml_cli -- run [--debug-heap]` (`S2-scratch/`): p1 closed-row `<IO>` lambda calling `+` → E2002 [V]; p2 `once` + `never` ops, `k` stored in `data Step`, `data Handle : Lin` across the suspension, `never` op after resume → `job: before / resuming / job: after / handle 1 / end` [V]; p3 `try_io` body suspended, resumed, then a failing `FileIn.open` → caught [V]; p4 stored `k` holding an open `FileIn` dropped → no leak in checked mode [V]; p5 (new) `SendResult`/`TrySendResult`/`TryRecvResult`/`Either` declarations compile in one module, `Either FileIn.FileIn Int` with a `Lin` component is usable, and `try_all` over `try_io`+`try_panic` catches `1 / 0` ("division by zero") and a failing open [V].
- Reviewer's probes read: q1 (E1003 on reused constructors), q2 (`List.map` over `Lin` elements passes), q3/q4.
- Read: docs/spec/effects.md, linearity.md, stdlib.md (`try_io`, `exit`, handles, `Child`), types.md (Kind, var escape, line 212), modules.md:15, multicore.md, future/stdlib.md, std/Prelude.em, std/Proc.em, std/FileIn.em, std/Bytes.em, std/List.em, `crates/eml_rt/src/host.rs` function list; tests/ui/run/kinds/scheduler.em.
- Not run: Verus, Miri, benchmarks; nothing on Windows; `compio-driver` source not available offline.

## 8. Response to review r1

| Finding | Verdict | Resolution |
|---|---|---|
| A-1 when siblings are cancelled; idle accept loop | accepted | Rule fixed: a task's failure cancels every task of its scope at the moment it is recorded (R-S1-C3), including a body parked in a driver op (the op is cancelled, R-S1-C2). Example 1's claim is now true for an idle server; the headline example isolates with `try_all` so it does not stop. |
| A-2 `Either` missing | accepted | Added to Prelude (`Left`/`Right`); Kind by the join rule; `Either FileIn Int` is `Lin` (probe p5). |
| A-3 `rethrow_io` mechanism | accepted | `Prelude.fail_io : IOErrorKind -> String -> <IO> a` as a table row (`Repr::Enum`, `OpRet::Never`) over a new `Fault::Raised { kind, message }` routed like `Fault::Io`; `Proc.exited` becomes a use of it; new kinds listed. |
| A-4 `compute` spelling, `Panic` | accepted | One signature `(Unit -> <never e> a) -> <IO, Async | e> a`; `Panic` is a `Never` label and travels in `e`. |
| B-1 hard cancel skips scope exit | accepted | Model changed (Q3): cancellation is delivered as `Wake.Cancelled` and raised as an IO failure, so the task unwinds through `try_all` and the eml scope protocol; `wait_closed` is shielded; nested scopes' children are cancelled by R-S1-C3. Hard drop only as last resort (R-S1-C4). |
| B-2 non-`Lin` cleanup on cancel | accepted | `try_all` handlers run on cancellation; sticky delivery with `shield` for cleanup that must park; counters, `Chan.close`, `try_send`, logging need no checkpoint. User drop glue rejected (spec rule). |
| B-3 per-connection isolation incl. `Panic` | accepted | `Async.try_all` (`Failure = IOFailed | Panicked`, probe p5) is the idiom and the headline example uses it; `accept` retries transient errors internally; EMFILE is an IO failure the acceptor may catch and back off. |
| B-4 constructor reuse | accepted | Renamed: `Sent/Rejected`, `Accepted/Full/Refused`, `Got/Empty/Drained`; types declared; compiled in one module (probe p5). |
| B-5 op budget rows/cost | accepted | Counter moved into the fast-path externs (`ctx.budget`, reset at dispatch); no Rust call per handler op; relation to VM preemption stated. |
| B-6 buffer size, reuse, uniqueness | accepted | Read size 64 KiB with capacity growth by the extern; uniqueness = `is_unique` (rc == 1; shared-marked blocks copied); reuse claim withdrawn; `write_bytes` consumes; buffer return left unresolved with default. |
| B-7 disarming early wakes | accepted | R-S1-C2: registrations removed before the resume; channel items never delivered to removed waiters; rendezvous value finalized by the unwind; `Interrupted` wake removed. |
| B-8 single acceptor, placement | partially accepted | Binding happens at the first op on whichever worker runs it, so spawn-time moves cost nothing; later moves use forwarding (R-S1-1, now required). Multi-acceptor stays later with the reason stated (A7). |
| B-9 milestones | accepted | M-A0 lists `Either`, `fail_io`, kinds, visibility; new M-A2s spike and M-A2w Windows; `eml_rt`/table/token/drop-glue items listed under constraints. |
| B-10 `Http.status` in an example | accepted | Example 2 rewritten on `Net` only. |
| C-1 graceful shutdown | accepted | `Scope` handle + `spawn_in`/`cancel_scope` + `std/Signal.em` `wait` in M-A2; drain pattern shown; Ctrl-C delivered to a waiter instead of being fatal (R-S1-7). |
| C-2 CPU-bound tasks | partially accepted | Stated: cooperative cancellation cannot interrupt pure code; `compute` is the rule for CPU-heavy sections (parks the task, worker keeps serving); native non-parking tasks are bounded only by S1's rescue (trigger and ~20 ms latency repeated here). a2 `par` cancel will make `timeout` around `compute` prompt. |
| C-3 fan-out partial failure | accepted | Rule: failure = scope failure immediately; isolation inside the task with `try_all`; Example 2 rewritten; the unresolved item removed. |
| C-4 blocking rows cannot park | accepted | r1's offload story withdrawn: sync std rows run inline in tasks (rescue covers); async files/pipes become M-A3 in the driver; S3's `blocking` applies to `<IO, Async>` rows lowered to the triple (R-S3-1 restated). |
| C-5 compio unverified | accepted | M-A2s spike with go/no-go before M-A2; `TestDriver`/`RealDriver` share the `Driver` trait. |
| C-6 `Chan` Lin items, limits | partially accepted | Dropped queued `Lin` items are finalized like any dropped continuation capture (stated); semaphore pattern (`Chan Unit`) for connection limits added to Example 1; `select` still out. |
| C-7 deadline reads close the stream | accepted | `read_chunk_within`/`read_line_within`/`accept_within` in M-A2 via `await_io_until`; Example 3 shows both forms. |
| C-8 `exit` rationale | accepted | Rationale replaced; driver teardown before `_exit` stated. |
| C-9 escaped handles | accepted | `Release` to `cell.owner` with the re-check; `Task` through `Chan` allowed as a `Lin` payload. |
