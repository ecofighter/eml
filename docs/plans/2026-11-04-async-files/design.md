# async-files: long-term design (guideline)

Tags: [V] verified (how is given), [I] inferred. Paths are under the repository root. This is the guideline of the plan `../2026-11-04-async-files.md` (its D-1..D-12 summarize it). `commits.md` is the commit-by-commit implementation design built on it; `ruling.md` holds the approved amendment of G-7 for files and the G-12 ruling. Where this file and `commits.md` §5b differ on a How-to-check command, §5b wins.

## 1. Conclusion

### A1 Coexistence
- Both stay. The blocking `<IO>` modules `FileIn`, `FileOut`, `Fs`, `Stdin`, `Stdout`, `Stderr`, `Proc` keep their names, handle types, Host path (`Reader`, `BufWriter`, `Child` in eml_rt host.rs) and tests. `Stdin` gets no async form (Windows console stdin cannot be overlapped; not one of the three goals).
- Two new modules, driver-backed, `<IO, Async>` only, shaped like `Std.Net`:
  - `Std.Stream`: handle types `Stream.Reader` (Lin) and `Stream.Writer` (Lin) shared by regular files and child pipes; file opening; read/write/close.
  - `Std.Exec`: async child processes: `Exec.Child` (Lin), `spawn`/`run`/`output`/`capture`, takers that split the child's pipes into `Stream.Reader`/`Stream.Writer`, `wait`, `wait_all`.
- Handle types are NOT shared between the sync and async worlds. `Cmd` (Prelude) is shared: pipelines (`.|`), stdin from text/file, stdout to file are `Cmd` settings honoured by `Proc` and `Exec` alike. Only "read stderr separately" and non-blocking I/O need `Exec`.
- Out of scope of this stage (stay sync or absent, so the next subtask does not guess): `Fs` stays as it is (sync, `read_text`/`write_text` only; no exists/list/remove/rename/metadata); no seek/offset, no read+write mode, no async `Stdin`/`Stdout`/`Stderr` and no wrapping of them in `Stream` handles; no `_within` variants on `Stream`; no buffered async writer. Each is a roadmap item with the 見直す条件 "a script needs it".
- Scripts, `bench/scripts.sh`, the REPL and `cli.rs` keep using `Proc`/`FileIn` unchanged ([V] grep of tests/ui/run/scripts shows only sync modules).

### A2 Pipelines
- Library, not grammar. A pipeline is a `Cmd` whose first stage's stdin is another `Cmd`. [V] probe (scratch dir): a mutually recursive `pub data` pair plus `pub infixl 1 .|` compiles and runs with the existing `eml` binary. [V] lexical.md:46-47: `.` and `|` are operator characters; only single `.` and `|` are reserved, so `.|` is one token.
- Prelude types (replace the current `Cmd` record; users never write `Cmd { … }`, stdlib.md 「`Cmd`」):

```haskell
pub data StdinMode =
  | DefaultStdin            -- the launch form decides: run -> inherit, output/capture -> empty, spawn -> pipe
  | NullStdin
  | StdinText String        -- A3
  | StdinFile String        -- `< in`, path relative to file_root like FileIn.open
  | StdinFrom Cmd           -- the stage before this one
  deriving (Eq, Show)

pub data StdoutMode =
  | DefaultStdout           -- run -> inherit, output/capture/spawn -> pipe
  | NullStdout
  | StdoutFile String       -- `> out` (create or truncate)
  | StdoutAppend String     -- `>> out`
  deriving (Eq, Show)

pub data StderrMode =
  | InheritStderr
  | MergeStderr
  | DiscardStderr
  | PipeStderr              -- honoured by Exec.spawn only (A4)
  deriving (Eq, Show)

pub data Cmd =
  | Cmd { program : String, args : List String, cwd : Option String, env : List (String, String),
          stdin : StdinMode, stdout : StdoutMode, stderr : StderrMode }
  deriving (Eq)
```
  `cmd_new program args` (the literal desugar, Prelude.em:274, expressions.md 「コマンドリテラル」 "既定の値" bullet) sets `DefaultStdin`, `DefaultStdout`, `InheritStderr`. The hand-written `instance Show Cmd` (Prelude.em:264-266) prints the two new fields in field order; `show` of a pipeline is nested (`stdin = StdinFrom Cmd { … }`), `display` prints the stages joined by ` | ` (head first).

- In `Std.Proc`:

```haskell
pub infixl 2 .|
pub (.|) : Cmd -> Cmd -> Cmd           -- head-targeting: sets the HEAD stage of `right` to `StdinFrom left`
pub stdin_text : String -> Cmd -> Cmd   -- head-targeting: head stage's stdin = StdinText s
pub stdin_file : String -> Cmd -> Cmd   -- head-targeting
pub no_stdin : Cmd -> Cmd               -- head-targeting: NullStdin
pub stdout_file : String -> Cmd -> Cmd  -- last stage (the value itself)
pub stdout_append : String -> Cmd -> Cmd
pub wait_all : Child -> <IO> List Int   -- exit codes of every stage, head first
```
  Setter rule (fixes the order trap): a pipeline value is its last stage. Setters that concern input (`.|`, `stdin_text`, `stdin_file`, `no_stdin`) walk the `StdinFrom` chain to the head stage and set that stage's `stdin`, replacing whatever it was; so `stdin_text s (a .| b)` and `a .| b` applied after `stdin_text s a` both give `a` the text, and `x .| (a .| b)` connects `x` to `a`. Every other setter (`cwd`, `env`, `arg`, `args`, `merge_stderr`, `discard_stderr`, `stdout_file`, `stdout_append`, `Exec.pipe_stderr`) applies to the stage it is applied to, i.e. the last stage of a pipeline; to configure an earlier stage, configure it before `.|`. stdlib.md states both sentences. `infixl 2` binds tighter than `|>` (`infixl 1`, [V] Prelude.em:7).
- Semantics (`Proc` and `Exec`):
  - One extern call spawns every stage, head first; stage k's stdout and stage k+1's stdin are the ends of one `std::io::pipe()` (blocking, never registered with a driver). After the call the parent holds no end of any inter-stage pipe. Stage k failing to spawn kills and waits stages < k and reports `Io { op: Run, name: <stage k program> }`.
  - Launch errors (kind `Other`, message ``cannot run `<program>`: <reason>``) at spawn time: a non-last stage whose `stdout` is not `DefaultStdout` (reason `the standard output of a stage that is not last is redirected`); `PipeStderr` on a non-last stage or on any `Proc` launch, or on `Exec.run`/`output`/`capture` (reason ``the standard error pipe needs `Exec.spawn` on the last stage``). Nothing is silently dropped.
  - `stderr` of each stage follows its own `Cmd`: `InheritStderr` default, `MergeStderr` merges into the pipe to the next stage (`2>&1 |`), `DiscardStderr`.
  - `wait`, `run`, `output`, `capture` return the exit code of the LAST stage. `wait_all` returns all codes head first. `Child`/`Exec.Child` hold every stage; `wait`/`wait_all` reap every stage in order; drop kills every unreaped stage then waits each. No `pipefail`; a stage killed by SIGPIPE shows 141 in `wait_all` only.
  - Children get std's `SIG_DFL` SIGPIPE reset ([V] R4 §6); eml keeps Rust's `SIG_IGN`.
- Spawn lock (closes the inheritance window, C-1): eml_rt host.rs gets `pub static SPAWN_LOCK: Mutex<()>`. Every `Host::spawn` implementation holds it from before creating any inter-stage/here-file object until `Command::spawn` returns for the last stage; `IoDriver::pipe` holds it around pipe creation and `FD_CLOEXEC`/handle setup on every OS. On Windows the child end is created NON-inheritable and std's `Command` duplicates it inheritable under its own `CREATE_PROCESS_LOCK` ([V] R4 §3), so no foreign child can inherit it; on Apple (`pipe` + `FD_CLOEXEC`, no `pipe2`) the lock serialises the window against eml's own spawns, which are the only forks in the process. Threads outside eml that fork are not covered (document).

### A3 Stdin given as text
- `Proc.stdin_text`/`stdin_file`; no `Proc.output_with`.
- Here-file: the Host writes the bytes to an anonymous temporary file and passes it as the child's stdin. Unix: `File::create` of `<temp_dir>/eml-stdin-<pid>-<counter>`, `unlink` at once, write, `seek(0)`. Windows: `OpenOptions::new().read(true).write(true).create_new(true).access_mode(GENERIC_READ | GENERIC_WRITE | DELETE).share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).custom_flags(FILE_FLAG_DELETE_ON_CLOSE)` (DELETE access is required for delete-on-close), write, seek, pass the handle (std duplicates it inheritable). Std only in eml_rt.
- The child sees a seekable regular file, not a pipe (as a shell here-string, which bash also backs by a temp file [I]); document. Failure to create or write the file is a launch error ``cannot run `prog`: <reason word>`` (one of the four fixed reason words of `Proc`'s launch errors, as `commits.md` D9 settles).
- `write*`/`close_stdin` on a child whose stdin is not a pipe fail `NotAvailable`.

### A4 Stderr read separately; `Cmd` growth
- `StderrMode` gains `PipeStderr`; setter `Exec.pipe_stderr : Cmd -> Cmd`.

```haskell
pub extern data Child                               -- Lin; every stage's process and the pipe ends not yet taken
pub spawn      : Cmd -> <IO, Async> Child           -- cancellation point; stdin/stdout default to pipes
pub run        : Cmd -> <IO, Async> Int             -- defaults inherit; PipeStderr is a launch error
pub output     : Cmd -> <IO, Async> (String, Int)   -- stdin default empty; reads stdout to EOF, then waits
pub output_bytes : Cmd -> <IO, Async> (Bytes, Int)
pub capture    : Cmd -> <IO, Async> String          -- nonzero last code -> IO failure Other, as Proc.capture
pub capture_bytes : Cmd -> <IO, Async> Bytes
pub stdin      : Child -> <IO> (Stream.Writer, Child)   -- takes the pipe out; twice or not a pipe -> NotAvailable
pub stdout     : Child -> <IO> (Stream.Reader, Child)
pub stderr     : Child -> <IO> (Stream.Reader, Child)   -- only after pipe_stderr
pub wait       : Child -> <IO, Async> Int           -- closes an untaken stdin, waits every stage, code of the last
pub wait_all   : Child -> <IO, Async> List Int
pub pipe_stderr : Cmd -> Cmd
```
- Redirection `< in`/`> out` is subsumed by `StdinFile`/`StdoutFile`/`StdoutAppend`: the Host opens the file under `file_root` (`FileIn.open` rules, `IsADirectory` normalisation) and passes it as `process::Stdio::from(File)`; failure is ``cannot run `prog`: cannot open `<path>`: <reason>`` with the open's kind.

### A5 Async child lifecycle
- `wait` = per stage `Op::Wait { process }` (exit notification, never reaps) followed on the owner worker by the Host's `Process::wait` (immediate, reaps; `CaptureHost` joins its drain threads there). Cancellation point.
- Task cancelled while waiting: driver cancels the notification; wrapper raises `Cancelled`; unwinding drops `Child`: push untaken pipe handles to the bin (unbound ones close at once), kill every unreaped stage, blocking `wait` each, discard errors. Bound: direct children die from SIGKILL/`TerminateProcess`, so the blocking wait is immediate for `StdHost`; `CaptureHost::wait` also joins drain threads that read inherited stdout/stderr until EOF, which a grandchild holding the pipe can delay (test Host only, same as today's `Proc.Child`; `Given` ends are never drained by the Host). Same for drop in a cancelled scope, `exit`, uncaught runtime error, `Async.timeout` around `wait`.
- Taken `Stream.Reader`/`Writer` are independent: dropping the `Child` kills the process but a taken reader keeps reading until EOF; EOF arrives when every holder of the write end has closed it, so a grandchild that inherited the pipe can delay it; the only deadline form is `Async.timeout`, which drops the reader. Document in stdlib.md.
- Task cancelled while reading/writing a pipe: readiness backends cancel at once; IOCP `CancelIoEx` and wait for the packet (existing rule); wrapper drops the stream, raises `Cancelled`.
- Task waiting on a file pool job: cannot be interrupted; the task stays parked until the job ends, then unwinds (A6).
- Ctrl-C / `Terminate`: no task thread blocks in the OS for pipes, waits or file jobs (file jobs block pool threads only), so the stop word is seen at the worker's next safe point on every OS. The wake path is the existing one: the handler rings the `Doorbell`, the monitor thread relays and wakes sleeping workers through the driver's waker ([V] vm.md 「上限と割り込み」 steps 2-4; iocp.rs `PortWaker::wake` posts to the port from any thread, iocp.rs:155-181). So the status.md:185 limit "pipe reads do not return on Ctrl-C" stays for the sync rows and does not apply to `Stream`/`Exec`. A run stopped while a file job runs ends after `quiesce` waits for that job; the second Ctrl-C `_exit(130)`s as today.
- Hosts whose `Process::raw()` is `None` (`MemoryHost`, embeddings): `Exec.wait` runs the Host's blocking `wait` in place; documented.

### A6 Async regular files
```haskell
-- Std.Stream
pub extern data Reader                              -- Lin; driver handle (file or child pipe) + name for messages + line buffer
pub extern data Writer                              -- Lin; no write buffer
pub open   : String -> <IO, Async> Reader           -- regular file; cancellation point
pub create : String -> <IO, Async> Writer           -- create or truncate
pub append : String -> <IO, Async> Writer           -- create or append (O_APPEND)
pub read_chunk      : Bytes -> Reader -> <IO, Async> (Bytes, Reader)       -- contract of Net.read_chunk
pub read_line       : Reader -> <IO, Panic, Async> (Option String, Reader) -- contract of Net.read_line (1 MiB)
pub read_line_bytes : Reader -> <IO, Panic, Async> (Option Bytes, Reader)
pub read_all        : Reader -> <IO, Async> (String, Reader)
pub read_all_bytes  : Reader -> <IO, Async> (Bytes, Reader)
pub write       : String -> Writer -> <IO, Async> Writer
pub write_bytes : Bytes -> Writer -> <IO, Async> (Bytes, Writer)           -- same object handed back
pub write_line  : String -> Writer -> <IO, Async> Writer
pub close        : Reader -> <IO> Unit              -- drop
pub close_writer : Writer -> <IO> Unit              -- drop; nothing to flush
```
- Regular files only (C-2): `Host::open_raw(path, mode)` opens with `O_CLOEXEC | O_NONBLOCK | O_NOCTTY` (unix; `O_NONBLOCK` so a FIFO open cannot block; it has no effect on regular files and is left set) and then checks `fstat` `S_IFREG` (Windows: `File::metadata()?.file_type().is_file()`); anything else is closed and fails ``cannot open `<path>`: not a regular file`` kind `NotAvailable`. FIFOs, ttys, `/dev/stdin`, sockets and directories never reach a pool thread. A hung network file system can still block a job (as it blocks the sync rows today); `quiesce` then waits; document.
- Buffering: `Reader` has only the `read_line` carry-over `Vec<u8>` (as `NetStream.rest`). `Writer` has no buffer; each `write*` is one op writing all bytes. No `flush`.
- The handle is bound at open by `Op::Adopt { res: RawResource::File }` on the opener's I/O host (`Run::io_worker(k)`: its own driver on the real clock, worker 0 on the virtual clock), answered at submit with `Done::Handle(IoHandle)`, as `connect`'s result is; a file is never `Bind::Unbound` inside a `Foreign` (B11). Reads/writes are pool jobs (B8/B9). `close` is the driver's `close(handle)` on its own thread.
- Opener cancelled before settling (the `connect` path, no new mechanism): `OpKind::Open` settles like `Connect`: the owner builds the stream `Foreign` from `Done::Handle` at settle whether or not the record is `cancelling`; the wrapper's `Cancelled` branch calls `finish op`, which drops the record's `made`, so `Bind::close` pushes the handle to its host's bin and the host closes it at its next collection; a run that ends with a settled-but-untaken record is swept by `sweep_all`, which discards `made`. Tests: `cells::an_open_cancelled_after_adoption_bins_its_handle` (fake driver, `cancelled: true` completion on an `Open` record, then the handle is in the host's bin) and `run/async/stream_open_cancelled.em` (cancel a task parked at `open`'s `io_wait`; the run ends without `a task is left at the end` and the test driver's file table is empty at `quiesce`).
- Failures: outcome codes of `Std.Net` unchanged (0 ok, 1 late (unused), 2 reset, 3 refused, 4 addr in use, 5 timed out, 6 other). `BrokenPipe` on a pipe write is code 2 -> `ConnectionReset` ([V] Prelude.em:205-217 has no BrokenPipe). Code 6 is thrown by `finish`, which maps the OS kind as `Net`'s `finish` does (`NotFound`, `PermissionDenied`, `Unsupported` -> `NotAvailable`, else `Other`) and keeps the OS reason in the message; so disk full, EIO and a read-only target are `Other` with the OS text, and a permission error on write is `PermissionDenied` (today's `FileOut.write` reports `Io { op: Write }` with the OS kind: same kinds, deliberately). Prefixes: files ``cannot read `<path>` ``/``cannot write to `<path>` ``; child pipes ``cannot read the output of `prog` ``, ``cannot read the errors of `prog` ``, ``cannot write to `prog` ``. Opens fail as `FileIn.open`/`FileOut.create` do, plus the `not a regular file` case above. EOF never fails; string reads fail `NotUtf8` as `Net`.
- Cancellation of an in-flight pool job: impossible; the record stays until the job completes; the completion commits the bytes (read), then the wrapper drops the stream and raises `Cancelled`. `Async.timeout` around a file op returns only after the job ends. Both the real and the test driver behave so (B12).

### A7 Determinism and tests
- Files on the test driver: `adopt(RawResource::File)` supported; `Read`/`Write` on file handles are Job-class ops: executed at settle and in `quiesce` with real `std::fs::File` reads/writes on the fd, never in `submit`, never cancellable (`cancel` is a no-op on them, like `Op::Job`), results delivered even after a cancel request. `CaptureHost::open_raw` keeps files out of the repository: `Read` opens the real file under the test dir; `Create`/`Append` open a per-run temp file under `std::env::temp_dir()` and record path -> temp path; `CaptureHost::written()` merges their contents into the `--- written: <path> ---` snapshot; a later `open_raw(path, Read)` or `FileIn.open` of a path written in the run reads the temp file; temp files are removed when the host drops. `run/async/stream_*.em` run on the virtual clock under seeds 1 and 2026 and at N = 2/8 like `net_*.em`, with `RunStats` compared under `driver_independent`.
- Processes on the test driver are unsupported (`pipe`, `adopt(Pipe*)`, `Op::Wait` -> `Unsupported`). `Exec.spawn_raw` raises `NotAvailable` ``cannot run `<prog>`: the test driver has no processes`` when the run's clock is `Virtual`, before touching any driver, on every worker (worker k >= 1 of a virtual-clock run has no driver, so without the guard it would hit the internal fault `a worker without a driver`). Test: `tests/ui/run-fail/async/exec_needs_the_real_driver.em` (no `-- clock: real` mark, so it runs under seeds and at N = 2/8) whose task calls `Exec.spawn` and expects that failure on every worker. Async process UI tests carry the leading mark `-- clock: real (<reason>)`: `ui.rs::execute_in` adds `.with_clock(Clock::Real)` to the `RunConfig` (the Host is `CaptureHost` as for every UI test; the clock does not touch the Host), runs checked then fast mode only; no seed runs, no worker runs, no `RunStats`/`placed_tasks`/`preemptions` assertions. Rules for marked files: at most 3 child launches per file, every child is `eml` itself, no `sleep_ms`/`timeout`/`_within`/`drain_within` (they would be real time), no dependence on which child finishes first. [V] one `eml run child.em child` costs ~10 ms wall in the dev profile on this macOS; Windows CI is slower, so the 3-child cap keeps a file under 1 s with margin (3 children x 2 modes x ~100 ms). Cross-worker pipe adoption, `Wait` cancel and wake on the real driver are covered by `crates/eml_vm/tests/api.rs` (N = 1 and 4, `Clock::Real`, ASan + TSan).

### B8 Driver ops
```rust
pub enum Op {
    Readable { handle: IoHandle },                      // sockets only; never issued on pipe or file handles (B8 rule below)
    Read { handle: IoHandle, buf: RawBuf },             // socket, pipe, file (dispatched on the handle's class)
    Write { handle: IoHandle, buf: RawBuf },            // same; writes all bytes
    Listen { .. }, Accept { .. }, Connect { .. }, ShutdownWrite { .. }, SetNodelay { .. },   // unchanged
    Job(Box<dyn FnOnce() -> io::Result<usize> + Send + 'static>),                           // unchanged
    Wait { process: RawProcess },                       // Done::Size(0) once the process has exited; never reaps
    Adopt { res: RawResource },                         // answered at submit with Done::Handle by the core's own adopt(res)
}
pub enum Done { Size(usize), Listener { .. }, Stream { .. }, Accepted { .. }, Handle(IoHandle) }
pub type OsHandle = OwnedFd /* unix */ | OwnedHandle /* windows */;              // one alias in eml_rt
pub struct RawPipe(OsHandle);     // Send; bound to no driver; parent end prepared for the backend
pub struct RawFile(OsHandle);     // synchronous regular file from Host::open_raw
pub struct RawProcess(OsProcess); // unix: pid i32; windows: OwnedHandle duplicated by the Host
pub enum RawResource { Socket(RawSocket), PipeRead(RawPipe), PipeWrite(RawPipe), File(RawFile) }
pub struct PipePair { pub parent: RawPipe, pub child: OsHandle }   // child end: plain, blocking, NOT inheritable
pub trait IoDriver {
    // unchanged: now, wait, submit, cancel, completions, pending, waker, bin, close, quiesce
    fn adopt(&mut self, res: RawResource) -> io::Result<IoHandle>;
    fn pipe(&mut self, parent_reads: bool) -> io::Result<PipePair>;
}
```
- Replacement for the `Readable`/`Probe` fallback (B-1): in cells.rs `submit_read` and `submit_read_line` (and the line chain continuation) the handle class decides. Socket: unchanged (unique buffer with >= 8 spare -> lend; else `Op::Readable` + `Probe`). Pipe or file: never `Probe`; if the buffer is unique with >= 8 bytes spare, lend that spare up to 65 536 bytes; otherwise call `grow_read_room` (for `read_line`: `lend_line_room`) at submit time, which makes the buffer unique with capacity `len + max(len, 65 536)` (the growth rule the socket path applies after its probe), and lend the new spare; the record goes straight to `Transfer`. Records keep the one shape (`OpKind::Read`/`Line`/`Write`, phase `Transfer`/`Done`).
- Core/backend boundary: `Step` gains `Wait { process: &RawProcess }` with `Outcome::Size(0)`; `Kind` gains `FileRead { handle, buf }`/`FileWrite { handle, buf, written }` (pool job via ticket, `owner = (handle, dir)` so `close` defers until the job ends) and `Wait { process }` (no owner). `Job` keeps returning `io::Result<usize>`.
- Hold: a job holding a `RawBuf` is `Memory`-held: driver `Drop` waits `HELD_LIMIT` (1 s) then aborts (as for IOCP boxes); `quiesce` waits without limit (as for every job). With regular files only (A6), a job ends when the disk answers.
- No `Op::Open` (the Host opens); `Op::Adopt { res }` and `Done::Handle(IoHandle)` are the driver's adoption answered at submit (the core maps `Op::Adopt` to its own `adopt(res)`, so backends gain no obligation beyond `adopt`).

### B9 Per-OS mechanism
- Unix pipes: `pipe()` under `SPAWN_LOCK`: Linux `pipe2(O_CLOEXEC)`; Apple `pipe` then `FD_CLOEXEC` on both. `O_NONBLOCK` on the parent end only ([V] R4 §1 separate descriptions). `adopt(PipeRead/PipeWrite)` registers edge-triggered as sockets (`register_row`, write interest for the write end only). `Sock` gets a class tag selecting `read/write` instead of `recv/send`; `MSG_PEEK` never runs on pipes. EPIPE -> `BrokenPipe` (process ignores SIGPIPE).
- Child exit, Linux: at `Step::Wait` `pidfd_open(pid, 0)`, registered `EPOLLIN | EPOLLET` on a new row that holds the pidfd instead of a socket; readiness = zombie -> `Outcome::Size(0)`, close the pidfd. `ENOSYS` -> `Start::Done(Err(Unsupported))`. Cancel: close the pidfd, `Cancel::Done`.
- Child exit, macOS: `kevent(EVFILT_PROC, ident = pid, NOTE_EXIT, EV_ADD | EV_ONESHOT, udata = row token)`; then, after the registration, `waitid(P_PID, pid, WEXITED | WNOHANG | WNOWAIT)` ([V] `WNOWAIT` MacOSX.sdk sys/wait.h:174): if it reports the child exited, `EV_DELETE` and `Start::Done(Ok(Size(0)))`. Order closes the window: an exit before the registration is seen by the check (or gives `ESRCH` at registration, also `Done`); an exit after it fires the event. kqueue.rs `poll` gets an `EVFILT_PROC` arm delivering `Ready { read: true, .. }` for the token (today only `EVFILT_READ/WRITE` are matched, [V] kqueue.rs:140-160), and the row table gets rows without an fd. Cancel: `EV_DELETE`, `Cancel::Done`. The driver never reaps.
- Windows pipes: `pipe()` under `SPAWN_LOCK`: `CreateNamedPipeW("\\.\pipe\eml-<pid>-<counter>", PIPE_ACCESS_INBOUND|OUTBOUND per `parent_reads`, FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_TYPE_BYTE, 1 instance, 64 KiB buffers)` for the parent end; `CreateFileW` of the same name, synchronous, `SECURITY_ATTRIBUTES` NULL (not inheritable) for the child end. `adopt` associates the parent end with the port under `SOCKET_KEY`'s sibling key `PIPE_KEY`; `Read`/`Write` are `ReadFile`/`WriteFile` with an `Over` box (`What::PipeRead`/`PipeWrite`, `Hold::Memory`, lent memory to the kernel, cancel `CancelIoEx`) through the existing `boxes` table ([V] iocp.rs:103-152 verifies packet addresses in `boxes` before reading). No zero-byte probe.
- Child exit, Windows (no new box state, C-6): `Step::Wait` calls `RegisterWaitForSingleObject(&mut wait, process, callback, ctx, INFINITE, WT_EXECUTEONLYONCE)`, `ctx` = `Arc<Exited>` where `Exited { keys: Mutex<Vec<OpKey>>, waker: Arc<PortWaker> }` is owned by the backend and shared with the callback; the callback pushes the key and calls `waker.wake()` (the same cross-thread post the waker already does, [V] iocp.rs:155-181). `wait()` after `GetQueuedCompletionStatusEx` drains `keys` into `Event { key, Arrival::Done(Ok(Size(0))) }`. Cancel: `UnregisterWaitEx(wait, INVALID_HANDLE_VALUE)` (blocks only while the callback runs, which takes the `keys` mutex and posts; the driver thread holds neither, so no deadlock), then remove the key from `keys` if present, `Cancel::Done`. `RawProcess`'s duplicated handle lives in the slot until the registration is gone. The core sees only readiness semantics (`Pending(Hold::None)`, `Cancel::Done`), as on unix.
- Regular files on all three OSes: pool jobs calling `read(2)`/`write(2)` (unix) or `ReadFile`/`WriteFile` with a null `OVERLAPPED` on the synchronous handle (Windows) with the raw pointer inside `sys/` (no slice). Windows overlapped file I/O is not used.

### B10 Lent memory rule (T10) revision
- Rule: "`RawBuf` memory is used, during the op, by the thread that owns the driver, by the kernel, and by pool threads of that driver running the op's file job. The lender's duties are unchanged (alive, unmoved, unique for reads, unmodified for writes, until `Submit::Done` or the completion is taken). The job with the pointer crosses to the pool thread through the pool queue mutex and the result crosses back through the inbox mutex, `wait` and `completions`; both are happens-before edges, so the owner's `bytes_commit`/read never races the job." A job uses the pointer only inside the OS call, never through a slice, reads at most `cap` bytes into `ptr[0..cap]` and reports `n`.
- Driver drop: buffer-holding jobs are `Memory`-held (B8); `quiesce` waits without limit (existing), so abort is reachable only from a poisoned `Machine`.
- Required tests: (1) eml_rt `trusted::`: a spawned thread writes `n` bytes into `bytes_lend` space through the raw pointer; main joins, `bytes_commit`s, reads back; `LARGE_BODY + 3`; both modes; Miri `-Zmiri-strict-provenance` and `-Zmiri-tree-borrows`. (2) eml_io `driver::tests::` with the fake backend plus a real pool thread (pool limit 2 under Miri): a `FileRead` whose job fills a `Vec` spare through `RawBuf`, `quiesce`, then read the `Vec`; a `FileWrite` whose job reads the lent bytes; driver drop while a job holds a buffer waits (not aborts) when the job finishes in time. (3) ASan: `eml_vm` `api::` cases reading and writing a real file through `Std.Stream` on `Clock::Real` (api:: is in the ASan and TSan lists [V] testing.md:220-228). (4) TSan: add `-p eml_io --lib -- driver::tests::` to the TSan command (eml_io is absent today [V] testing.md:228); docs and ci.yml.
- object-model.md T10 row names the driver's pool threads as external writers; vm.md 「時計と driver」 `RawBuf` bullet carries the rule above.

### B11 Handle placement
- Pipes are created unbound (`Bind::Unbound(RawResource::Pipe*)`): `Exec.spawn` calls `pipe()` on the spawning worker's own driver (`sched.own_driver()`); the first worker that submits an op on a pipe end adopts it (`bind_stream` generalised to `adopt(RawResource)`); the handle then stays with that driver and other workers' ops are routed through `Submit`/`Completion` mail, unchanged.
- Files: `Stream.open`/`create`/`append` get the file from the Host and submit `Op::Adopt { res: RawResource::File }` as an `Open` record; it is answered at submit with `Done::Handle` by the opener's I/O host (`Run::io_worker(k)`), and the `Foreign` is `Bind::Bound { host }` from the start, as `connect`'s result is. Statement for vm.md 「時計と driver」: the file's host is the worker whose driver adopted it, fixed at open; rescue moves the owner task, never the host; ops from a non-host owner go by `Submit`/`Completion` mail as for a `connect`ed stream. `Op::Adopt` is answered at submit by the host's driver; the opener parks only when its I/O host is another worker (a `Submit`/`Completion` mail round trip), is never rescued while parked, and then settles like `connect`. On the real clock a non-local reader costs only the mail round trip (reads are pool jobs on the host driver); on the virtual clock worker 0 hosts every worker's I/O, so the opener's host and any first submitter's host are the same driver.
- `Op::Wait` has no handle: record `owner = host = the waiting task's worker`, submitted on that worker's own driver (virtual clock: unsupported). A task waiting on an op is never rescued ([V] vm.md 「救済」), so the owner cannot move while the `Wait` is pending; the completion is collected by that worker as for any own-driver op. api.rs tests this at N = 4 by waiting on a child from a task placed on worker 2 under `RescueAlways`.

### B12 Test driver
- Must model: `adopt(File)` (real fd in the handle table; it answers `Op::Adopt { res: File }` at submit with `Done::Handle`), Job-class file `Read`/`Write` (settled at `wait` and in `quiesce`, uncancellable, results committed), `close` of file handles, `HandleBin`. Must refuse: `pipe`, `adopt(PipeRead/PipeWrite)`, `Op::Wait` with `Unsupported`. Virtual network, TLS terminator, clock unchanged.

## 2. Rationale
- Coexistence: scripts and the REPL must not need `Async.run` to read a file; `<IO>` rows cannot wait (`op_ready` needs a task cell [V] vm.rs `current_cell`); the sync Host path keeps `CaptureHost`'s in-memory `FileOut` and the Windows console stdin.
- Pipeline as a `Cmd` field: every launch form accepts pipelines with unchanged types; OS-fd joining needs no task; the deferred redirection item wanted `Cmd` stdin/stdout fields ([V] roadmap.md:167). Head-targeting input setters make setter order irrelevant.
- Here-file: no thread, no task, no deadlock, works for sync and async and pipelines; std-only in eml_rt.
- Exit notification instead of a pool job per `wait`: pool exhaustion and non-cancellability otherwise; the REPL's `quiesce` needs cancel.
- Windows exit via the shared `Exited` list + waker: no new core states, no box freed from a foreign thread; reuses the one cross-thread primitive the backend already has.
- Host opens files / driver adopts: `file_root`, `IsADirectory`, "no files in the repo" and now the regular-file check all live in the Host; the driver stays OS-only.
- Regular files only: the T10 safety story (jobs end) and the REPL's unbounded `quiesce` need jobs that end; special files are what blocks forever.
- Pool threads use lent memory: one op-record shape, no copy; safety = external writer + mutex happens-before, as for IOCP boxes.
- `Readable` socket-only with grow-at-submit: the probe only saves memory for idle sockets ([V] cells.rs:3296-3345); pipes/files have no such multiplicity.
- `-- clock: real`: the test driver cannot model real children without threads; child output of `eml` is deterministic; no real timers keeps it machine-independent.
- `wait` = last stage's code with `wait_all`: shell `$?` and an unchanged `Child -> Int`.

## 3. Alternatives considered and rejected
- Replace the sync rows by async ones: every script and the REPL would need `Async.run`; Windows console stdin cannot be async; ~150 kind-1 UI rewrites.
- `.|` as grammar: the operator already lexes; a library function suffices.
- Pipeline as `List Cmd` or a `Pipeline` type: every launch function would need a second overload.
- Stdin text by a writer task/thread: needs a task (sync `Proc` excluded) or a Host thread; here-file needs neither.
- Process wait as a pool job: uncancellable, a pool thread per child, blocks `quiesce`, moves the `Process` out of the Foreign.
- SIGCHLD fallback on Linux: signal plumbing in `eml_cli` for kernels CI never runs.
- Windows pipes as pool jobs (tokio's `Blocking`): uncancellable blocking `ReadFile`; contradicts the async.md decision.
- Windows exit via a posted box address with a new key: needs a box table entry freed from a system thread and new core states (`Cancel::Pending` on a readiness op); the shared list avoids both.
- Windows overlapped regular-file I/O: file-pointer and skip-on-success caveats for one OS.
- Copy-based file jobs: a copy and a Rust allocation per chunk on a pool thread, a second record shape.
- Allowing special files in `Stream.open`: blocks pool threads and the driver drop forever; sync rows exist for ttys/FIFOs.
- Virtual filesystem in `TestDriver`: a second written-files store; the Host temp file keeps one.
- Test driver pumping real children with threads: threads per pipe and process plus a frozen virtual clock; much test-only code.
- Buffered async `Writer`: cannot flush in `Drop`.
- `pipefail`: 141 for `… .| head`; `wait_all` exposes codes instead.
- Silently dropping redirections on middle stages: a launch error instead (B-7).
- Nested module `Std.Async.File` / module `Std.Cmd`: std is flat ([V] ls std/); `Cmd` is a Prelude type.

## 4. Assumptions made
- kqueue reports `NOTE_EXIT` for a process that exits after a successful `EV_ADD`; registration on an unreaped zombie either succeeds or returns `ESRCH`. Impact if a zombie registration succeeds and never fires: covered by the post-registration `waitid` check (B9). A unix.rs test (spawn `true`, sleep until exited, `Step::Wait`, expect `Done` within 1 s) pins it.
- Rust std on Apple spawns by `posix_spawn` or `fork` without `POSIX_SPAWN_CLOEXEC_DEFAULT` [I]; the `SPAWN_LOCK` makes the design independent of it.
- `RegisterWaitForSingleObject` callbacks may call `PostQueuedCompletionStatus` ([V] tokio does this with a oneshot, R4 §2; `PortWaker::wake` already posts from pool/resolver threads). Impact if false: a dedicated `WaitForSingleObject` thread per wait; API unchanged.
- `CreateNamedPipeW` + `CreateFileW` pipes inherit and overlap like std's `child_pipe` ([V] R4 §3). Impact if false: use `Stdio::piped()` handles (overlapped on the parent side) for non-merged pipes.
- std impls `From<OwnedFd>`/`From<OwnedHandle>`/`From<File> for process::Stdio`; `OpenOptionsExt::{access_mode, share_mode, custom_flags}` exist on Windows [I, io-safety and fs_ext APIs]. Impact if false: eml_rt would need `windows-sys` for the here-file.
- "last stage" is the POSIX shell's `$?` convention [I].
- SIGPIPE: eml keeps `SIG_IGN`, children get `SIG_DFL` ([V] R4 §6); an embedding must keep SIGPIPE ignored.
- Windows CI process creation stays under ~100 ms per `eml` child [I]; the 3-child cap gives margin.

## 5. Implications for implementation
Host/driver spawn interface (B-2), concrete:
```rust
// eml_rt host.rs
pub enum StdinSpec<'a> { Inherit, Null, Pipe, Given(OsHandle), Text(&'a [u8]), File(&'a str) }
pub enum StdoutSpec<'a> { Inherit, Null, Pipe, Given(OsHandle), File { path: &'a str, mode: WriteMode } }
pub enum StderrSpec { Inherit, ToStdout, Null, Given(OsHandle) }
pub struct Stage<'a> { pub program: &'a str, pub args: &'a [&'a str], pub env: &'a [(&'a str, &'a str)], pub cwd: Option<&'a str>, pub stderr: StderrSpec }
pub struct Spawn<'a> { pub stages: &'a mut [Stage<'a>], pub stdin: StdinSpec<'a>, pub stdout: StdoutSpec<'a> }
pub struct Spawned { pub stdin: Option<Box<dyn Write + Send>>, pub stdout: Option<Box<dyn Read + Send>>, pub processes: Vec<Box<dyn Process>> }
pub trait Process: Send { fn wait(&mut self) -> io::Result<i32>; fn kill(&mut self) -> io::Result<()>; fn raw(&self) -> Option<RawProcess> { None } }
pub static SPAWN_LOCK: Mutex<()>;
pub trait Host { …; fn open_raw(&self, path: &str, mode: OpenMode) -> io::Result<RawFile>; }   // default Err(Unsupported)
```
  `Pipe` is the sync path (std `piped()`, boxes come back in `Spawned`). `Given` is the async path: the VM's `Exec.spawn_raw` arm calls `driver.pipe(parent_reads)` per needed pipe (stdin, stdout, stderr; for `MergeStderr` with a piped stdout one pipe and a `try_clone` of the child end made by the Host), keeps each `parent` as `Bind::Unbound`, passes each `child` as `Given`; `Spawned.stdin/stdout` are `None` for `Given`. `StdHost` and `CaptureHost` both implement `Given` by `process::Stdio::from(OsHandle)`, `Text` by the here-file, `File` by `Stdio::from(File)` under `file_root`; `CaptureHost` drains only `Inherit` stdout/stderr with threads, never `Given`. `OsProcess::raw` returns `Child::id()` (unix) / `as_raw_handle()` duplicated with `try_clone_to_owned` (windows); `CapturedChild` (hosts.rs:358) delegates `raw` to its inner `OsProcess` so real-clock UI tests exercise `Op::Wait`; `MemoryHost` keeps the default `None`.
  `Std.Proc.spawn_raw`'s eml signature becomes `spawn_raw : String -> Int -> String -> Int -> String -> <IO> Child` (stages string, stdin code, stdin payload, stdout code, stdout path); stages string = the existing length-prefixed encoding with 5 fields per stage (program, args list, env list, cwd, stderr code 0-3); stdin code 0 inherit/1 null/2 pipe/3 text/4 file, stdout code 0 inherit/1 null/2 pipe/3 file/4 append. `Std.Exec.spawn_raw` has the same arguments and is an `Async`-instruction row.

Constraints
- eml_rt changes go through its gate (Verus, verusfmt, T10/T11 rows, Miri `trusted::`).
- eml_io core stays OS-free; OS code in `sys/`; `unsafe` only in `trust.rs`-listed files; completion-type values only in core and `sys/iocp.rs`.
- Extern rows for `Std.Stream`/`Std.Exec` follow the `Std.Net` pattern; `<IO>` Host rows (`open_raw`, takers, reap) go in the generic extern arm.
- `RunStats`: no new field.

Units of work, rough order (spec section in the same commit)
1. Prelude `Cmd`/modes, `cmd_new`, `Show Cmd`; `Proc` setters, `.|`, `wait_all`; host.rs `Spawn`/`Stage`/specs, `SPAWN_LOCK`, here-file, pipeline spawn, `Process::raw`; expressions.md 「コマンドリテラル」 defaults bullet; stdlib.md `Proc`; sync UI tests `run/std/proc_pipeline.em`, `proc_stdin_text.em`, `proc_redirect.em`, `run-fail/std/proc_stage_redirect.em`.
2. eml_rt driver.rs types (`OsHandle`, `RawPipe`, `RawFile`, `RawProcess`, `RawResource`, `PipePair`, `Op::Wait`, `adopt`/`pipe`), `Host::open_raw` with the regular-file check; vm.md and T10 text; `trusted::` Miri test.
3. eml_io core: handle classes, `FileRead`/`FileWrite` jobs (`Memory` hold), `Step::Wait`; fake-backend tests incl. Miri; TSan command.
4. eml_io sys: unix pipes, `read/write`, pidfd, kevent `EVFILT_PROC` arm + fd-less rows; iocp named pipes, `PipeRead/PipeWrite` boxes, `Exited` list; `tests/real.rs` cases.
5. eml_io test driver: `adopt(File)`, Job-class file ops, refusals.
6. eml_vm cells.rs: `Bind::Unbound(RawResource)`, `Stream` Foreigns, grow-at-submit for pipe/file reads, `OpKind::Wait`, `Exec.Child` Foreign; `CaptureHost::open_raw` temp files + `written()`; api.rs cases.
7. std/Stream.em, std/Exec.em, extern rows, `is_async()`; stdlib.md 「`Std.Stream`」「`Std.Exec`」; status.md; roadmap.
8. UI harness mark `-- clock: real`; tests.
9. benchmarks.md tables at stage close.

Test-change scope (kinds, per testing.md 「テストの変更の運用」)
- Kind 2 (expected values): every snapshot showing a `Cmd` value: `git grep -l "InheritStderr" -- crates/eml_cli/tests/snapshots tests/ui` (today [V] `integration__ui__run@basics__command_literal.em.snap`, `integration__repl__transcripts@std__command_literal.repl.snap`); `crates/eml_core_ir/tests/externs.rs` row for `Std.Proc.spawn_raw`; `crates/eml_hir/tests/{lower,strings}.rs` and `crates/eml_hir/src/{def_map,hir,lower/expr}.rs` where `cmd_new`'s arity/defaults are named ([V] git grep list); `bench/` `RunStats` snapshots if `std/` changes shift counts.
- Kind 1: none planned (no test changes pass/fail or moves).
- Kind 3: the sync `Proc`/`FileIn` UI tests stay byte-identical.

Test strategy
- Virtual driver (UI, seeds, N = 2/8): `stream_read_file.em` (read_all/read_line/read_chunk of a file in the test dir), `stream_write_read_back.em`, `stream_cancelled_reader.em` (leak check), `stream_timeout_after_job.em`, `stream_open_special.em` (a directory path everywhere -> `not a regular file`; the FIFO case is the real-clock `api::a_stream_open_of_a_fifo_fails_at_once`, see `commits.md`).
- Real clock (UI with the mark, checked+fast, <= 3 children): `exec_pipeline.em` (3-stage `eml` pipeline, `capture`), `exec_stdin_text.em` (`cat`-like child, > 64 KiB both ways), `exec_stderr_split.em` (two tasks), `exec_child_released_on_abort.em`, `exec_wait_all.em`, `run-fail/async/exec_pipe_stderr_on_run.em`.
- Rust only: `crates/eml_io/tests/real.rs` (pipe EOF/EPIPE/cancel, `Wait` on exited/running/killed child, pipe adoption by another thread's driver, driver drop with a running file job), `src/sys/unix.rs` and `src/sys/iocp.rs` `mod tests` (zombie registration, `UnregisterWaitEx` after the callback ran, non-inheritable child end), `crates/eml_vm/tests/api.rs` (real driver, N = 1 and 4, ASan+TSan, `Wait` from a rescued-placement task), `crates/eml_cli/tests/cli.rs` (SIGINT 130 while `Exec.wait` waits; the Windows Ctrl-C case is deferred to the roadmap because cli.rs has no console-event harness, see `commits.md` commit 13).
- Windows: all UI tests run; `#[cfg(unix)]`/`-- windows: skip` only for signal exits, FIFOs and `sh`.

Candidate completion criteria (clean tree)
- `cargo nextest run`; `cargo clippy --all-targets -- -D warnings && cargo fmt --check`; the feature-combo clippy lines of CLAUDE.md.
- `cargo verus verify -p eml_rt`; `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')`.
- `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::` and with `-Zmiri-tree-borrows`, within the budgets of crates/eml_rt/CLAUDE.md; the same two for `cargo miri test -p eml_io --lib -- driver::tests::`.
- ASan command of CLAUDE.md for `eml_io` and `eml_vm --test integration -- api:: signal:: io:: workers::`; TSan commands including `-p eml_io --lib -- driver::tests::`: no reports.
- `timeout 20 cargo run -p eml_cli -- run tests/ui/run/async/exec_pipeline.em` prints the snapshot's stdout and exits 0 on macOS, Linux and Windows CI.
- `cargo nextest run --profile timing -j 1` on CI: no test over 2 s, `ui::run` under 10 s.
- `git grep -n '^-- clock: real (' -- tests/ui` lists only `tests/ui/run/async/exec_*.em` and `tests/ui/run-fail/async/exec_*.em`.
- `cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` passes.

## 6. Unresolved items
- Per-stage stderr readers for middle stages: default launch error; roadmap item.
- `Exec.output_err : Cmd -> <IO, Panic, Async> (String, String, Int)`: default not in this stage.
- `_within` on `Stream`, buffered `Writer`, async `Fs` helpers: default none; roadmap items.
- Linux < 5.3: default `Exec.wait` fails `NotAvailable`.
- Whether `env` should apply to every stage of a pipeline: default last stage only (document); 見直す条件: a script sets the same variable on every stage.

## 7. Additional checks
- r1: probe program (recursive `pub data`, record update, `pub infixl 1 .|`) runs on the existing `eml` binary [V]; macOS `man kqueue` lists `EVFILT_PROC` `NOTE_EXIT`/`NOTE_EXITSTATUS` [V]; `WNOWAIT` in MacOSX.sdk sys/wait.h:174 [V]; `eml run child_stderr.em child` ~0.01 s wall (3 runs) [V].
- r2: iocp.rs:100-181 (`Over`/`Lent`/`boxes`/`waiters`, `PortWaker::wake` posting `WAKE` from any thread), iocp.rs:871-910 (`wait` matches `(lpCompletionKey, lpOverlapped.is_null())`, `WAKE` and `SOCKET_KEY` arms, box address verified before use) [V]; kqueue.rs:40-165 (`register` by fd ident, `poll` matches only `EVFILT_READ`/`EVFILT_WRITE`) [V]; vm.md 「上限と割り込み」 (handler rings `Doorbell`, monitor relays and wakes workers; Windows console handler; sync-read limits) [V]; host.rs has no spawn lock, `write_stopping` at 1127 [V]; hosts.rs:358 `impl Process for CapturedChild` [V]; `git grep -ln "InheritStderr\|stderr = \|cmd_new\|Cmd {"` lists the kind-2 scope of section 5 [V]; expressions.md 「コマンドリテラル」 defaults bullet names `None`, `Nil`, `InheritStderr` [V].

## 8. Conformance criteria

- **G-1** The blocking modules `FileIn`, `FileOut`, `Fs`, `Stdin`, `Stdout`, `Stderr`, `Proc` MUST keep their names, handle types and `<IO>` rows; async I/O MUST live in `Std.Stream` and `Std.Exec` with `<IO, Async>` waiting rows and distinct handle types.
  - Why: scripts, the REPL and native code need file/process I/O without `Async.run`; `<IO>` rows cannot wait on the driver.
  - Applies to: std modules and their extern rows. Not covered: sync `Cmd` features added to `Proc`.
  - How to check: `git grep -n "Async" -- std/FileIn.em std/FileOut.em std/Fs.em std/Proc.em std/Stdin.em std/Stdout.em std/Stderr.em` prints nothing; `git grep -n "<IO>" -- std/Stream.em std/Exec.em` prints only `close`, `close_writer`, `stdin`, `stdout`, `stderr` and non-`pub` runtime rows.
- **G-2** A pipeline MUST be a `Cmd` with `StdinFrom` on its head, spawned by one `Host::spawn` call under `SPAWN_LOCK`, and the parent MUST hold no end of an inter-stage pipe after the call returns.
  - Why: unchanged launch types; an early-exiting reader must deliver EPIPE/SIGPIPE to the writer; no foreign child may inherit an end.
  - Applies to: Prelude `Cmd`, `Proc.(.|)`, `host_spawn`, `StdHost::spawn`, `CaptureHost::spawn`.
  - How to check: `git grep -n "SPAWN_LOCK" -- crates/eml_rt/src/host.rs crates/eml_vm/src/hosts.rs crates/eml_io/src/sys` shows a lock acquisition in each `spawn` and in each `pipe`; `cargo nextest run -p eml_cli --test integration -E 'test(ui::run)'` with `EML_UI_FILTER=proc_pipeline` passes (its last stage exits early and the run terminates).
- **G-3** Stdin text MUST be delivered through a here-file created by the Host with std only; eml MUST NOT write the text through a pipe or thread.
  - Why: no read-while-write deadlock at any size; same for sync and async.
  - Applies to: `StdinSpec::Text` in host.rs and hosts.rs.
  - How to check: `git grep -n "windows_sys\|libc::" -- crates/eml_rt/src/host.rs` prints nothing; `git grep -n "thread::spawn\|spawn_masked" -- crates/eml_rt/src/host.rs` prints nothing; `EML_UI_FILTER=proc_stdin_text cargo nextest run -p eml_cli --test integration -E 'test(ui::run)'` passes with a > 1 MiB text.
- **G-4** `Exec.wait` MUST use `Op::Wait` (pidfd / `EVFILT_PROC` / `RegisterWaitForSingleObject` + `Exited` list) and MUST NOT run a wait on a pool thread; the driver MUST NOT reap.
  - Why: pool exhaustion and non-cancellability otherwise; `CaptureHost` must reap in `Process::wait`.
  - Applies to: eml_io backends, cells.rs `Exec` records, `Process` trait.
  - How to check: `git grep -n "waitpid\|waitid\|WaitForSingleObject" -- crates/eml_io/src` shows only `waitid(... WNOWAIT ...)` in sys/unix.rs and test code; `git grep -n "Op::Job" -- crates/eml_vm/src/cells.rs` prints nothing; `cargo nextest run -p eml_io --test integration -E 'test(real::a_wait_on_a_running_child_is_cancelled_at_once)'` passes.
- **G-5** Regular-file reads and writes MUST be pool jobs on every OS using the lent `RawBuf` inside the OS call with no slice; such jobs MUST be `Memory`-held for driver drop; `Readable` MUST NOT be issued on pipe or file handles (grow-at-submit instead).
  - Why: one record shape; no copy; driver drop must not leave a pool thread writing freed memory.
  - Applies to: eml_io core and `sys/`, cells.rs submit paths. Not covered: socket IOCP paths.
  - How to check: `grep -rn "from_raw_parts" crates/eml_io/src | grep -v "src/test.rs"` prints nothing; `cargo nextest run -p eml_io --lib -E 'test(driver::tests::a_dropped_driver_waits_for_a_file_job)'` passes; `git grep -n "Op::Readable" -- crates/eml_vm/src/cells.rs` shows every site guarded by a socket-class match.
- **G-6** `Stream.open` MUST accept regular files only: open with `O_NONBLOCK | O_NOCTTY | O_CLOEXEC` then `fstat`, and refuse anything else with kind `NotAvailable` before any pool job.
  - Why: a FIFO, tty or `/dev/stdin` would block a pool thread forever and make driver drop abort.
  - Applies to: `Host::open_raw` in `StdHost` and `CaptureHost`. Not covered: hung network file systems.
  - How to check: `git grep -n "O_NONBLOCK" -- crates/eml_rt/src/host.rs` shows the open flags; `EML_UI_FILTER=stream_open_special cargo nextest run -p eml_cli --test integration -E 'test(ui::run)'` passes (directory -> `not a regular file`).
- **G-7** Pipe ends MUST be created unbound (`Bind::Unbound`) and adopted by the driver of the first worker that submits on them. Files MUST be opened by the Host (`Host::open_raw`) and bound at open by `Op::Adopt { res: RawResource::File }`, answered at submit with `Done::Handle` by the core's `adopt`, hosted by the opener's I/O host (`Run::io_worker(k)`: its own driver on the real clock, worker 0 on the virtual clock), as `connect`'s result is; a file is never `Bind::Unbound` inside a `Foreign`. Unix parent ends MUST have `O_NONBLOCK`, child ends MUST NOT; Windows parent ends MUST be overlapped named-pipe handles and child ends synchronous and NOT inheritable at creation; inter-stage pipes MUST be plain `std::io::pipe()` ends.
  - Why: locality of the reading task for pipes (the acceptor-style hand-off); for files the opener is the reader, as for `connect`, and the virtual clock hosts every worker's I/O on worker 0, so lazy adoption cannot run there (cells.rs `bind_stream` faults when `here.host != here.me`, sched.rs `own_driver` is a no-op on worker k >= 1 of a virtual-clock run); nonblocking stdio breaks children; only overlapped handles reach the IOCP; a pre-made inheritable handle leaks into concurrent spawns.
  - Applies to: `IoDriver::pipe` in sys/unix.rs and sys/iocp.rs; `bind_stream`; `host_spawn`.
  - How to check: `cargo nextest run -p eml_io --lib -E 'test(sys::unix::tests::a_pipe_parent_end_is_nonblocking_and_the_child_end_is_not)'` passes (reads `F_GETFL` of both); on Windows `cargo nextest run -p eml_io --lib -E 'test(sys::iocp::tests::a_pipe_child_end_is_not_inheritable)'` passes (`GetHandleInformation` `HANDLE_FLAG_INHERIT` clear); `cargo nextest run -p eml_vm --test integration -E 'test(api::a_child_pipe_is_adopted_by_another_worker)'` passes; `cargo nextest run -p eml_vm --lib -E 'test(cells::an_open_record_binds_the_file_to_the_openers_host)'` passes (N = 2 on the in-file fake driver: a record opened by worker 1 has `Bind::Bound { host: 0 }` on the virtual clock, `host: 1` on the real clock).
- **G-8** The test driver MUST support file handles as Job-class ops (settled at `wait` and `quiesce`, uncancellable) and MUST refuse `pipe`, pipe adoption and `Op::Wait` with `Unsupported`; async process UI tests MUST carry `-- clock: real (<reason>)`, use at most 3 child launches, no real-time waits, and run only in checked and fast mode.
  - Why: deterministic file tests under seeds and N = 2/8; real children cannot be virtual; CI never retries.
  - Applies to: crates/eml_io/src/test.rs, crates/eml_cli/tests/{ui.rs,common/mod.rs}, tests/ui/{run,run-fail}/async/.
  - How to check: `cargo nextest run -p eml_io --test integration -E 'test(test_driver::a_cancelled_file_read_still_delivers_its_bytes) | test(test_driver::the_test_driver_has_no_pipes)'` passes; `git grep -n '^-- clock: real (' -- tests/ui` lists only `async/exec_*.em`; `git grep -ln "sleep_ms\|timeout\|_within" -- $(git grep -l '^-- clock: real (' -- tests/ui)` prints nothing.
- **G-9** Written files of UI tests MUST stay out of the repository: `CaptureHost::open_raw` for `Create`/`Append` MUST use per-run temp files merged into the `--- written:` snapshot and deleted on drop.
  - Why: testing.md 「UI テスト」 rule; one store for written files.
  - Applies to: crates/eml_vm/src/hosts.rs. Not covered: `StdHost`.
  - How to check: `cargo nextest run -p eml_cli --test integration -E 'test(ui::)' && git status --porcelain -- tests/` prints nothing after the run; `grep -n "written: " crates/eml_cli/tests/snapshots/integration__ui__run@async__stream_write_read_back.em.snap` shows the written text.
- **G-10** The T10 row and the vm.md `RawBuf` bullet MUST be rewritten in the same commit as the first file job, naming pool threads as writers and the mutex happens-before edges, together with the Miri tests of B10.
  - Why: the rule is enforced only by documentation and Miri.
  - Applies to: docs/spec/object-model.md T10 row, docs/spec/vm.md 「時計と driver」, crates/eml_rt/src/heap/tests/trusted.rs, crates/eml_io/src/driver/tests/.
  - How to check: `git log --format=%H -S"FileRead" -- crates/eml_io/src/driver/mod.rs | tail -1 | xargs git show --stat` lists both docs and both test files; the two Miri commands of section 5 pass.
- **G-11** The exit code of a pipeline MUST be the last stage's in `wait`/`run`/`output`/`capture`; `wait`/`wait_all`/drop MUST reap every stage; drop MUST kill every unreaped stage; a redirection on a middle stage and `PipeStderr` outside `Exec.spawn`'s last stage MUST be launch errors.
  - Why: shell `$?` with unchanged types; no zombies; no silent drops.
  - Applies to: `Proc.Child`, `Exec.Child`, `host_spawn` validation. Not covered: grandchildren of `sh -c`.
  - How to check: `EML_UI_FILTER=proc_pipeline cargo nextest run -p eml_cli --test integration -E 'test(ui::run)'` snapshot shows the last code and `wait_all` with a nonzero head; `EML_UI_FILTER=proc_stage_redirect cargo nextest run -p eml_cli --test integration -E 'test(ui::run_fail)'` snapshot shows ``cannot run `…`: the standard output of a stage that is not last is redirected``.
- **G-12** Every waiting function of `Std.Stream` and `Std.Exec` MUST follow the `Std.Net` wrapper shape (cancellation check, `submit_*` -> `io_wait` -> `op_*` -> `take_*`/`finish`, outcome codes 0-6) and MUST NOT expose `OpId`; `Op::Wait` records MUST have `owner == host`.
  - Why: one record protocol; the rescue invariant (a task with an unfinished record is in its op wait) holds only if submit and wait are adjacent and own-driver ops stay with their owner.
  - Applies to: std/Stream.em, std/Exec.em, cells.rs `submit_wait`. Not covered: takers and `close`.
  - How to check: `git grep -n "Async.OpId" -- std/Stream.em std/Exec.em` shows only non-`pub` `extern` rows; `cargo nextest run -p eml_vm --lib -E 'test(cells::a_wait_record_is_hosted_by_its_owner)'` passes; `git grep -n "op_failed" -- std/` prints nothing; `git grep -n "op_prefix" -- std/Stream.em std/Exec.em` shows one non-`pub` `extern` row per module (`op_prefix : Async.OpId -> <IO> (String, Async.OpId)`, the record's message prefix read before `finish` in the `Woke` branch, with a module-local `fail_code : Int -> String -> <IO> a` as in Net.em:248-251; code 2 is `ConnectionReset` with `\{prefix}: broken pipe`); `git grep -n "OUTCOME_FAILED" -- crates/eml_vm/src/cells.rs` shows no handle-class branch in `outcome()`.

