> 設計の段階の付録 (英語)。優先の順は、本文 (`../2026-10-31-io-uring.md`) > `05-rulings.md` > この付録である。`scratch/…` や `scratch-s*/…` の名前は設計のときの作業用のファイルで、リポジトリにはない。本文に要るものは本文か `06-harness.patch` に写してある。

# S1 (r4): prototype io_uring `Backend` for eml_io (Linux)

Tags: [V] = verified, with how; [I] = inferred.

Sources:
- **Probes:** in `scratch-s1/` (`probe.c`, `probe2.c`, `probe3.c`, `probe4.c`, `sz.c`), run on this kernel (6.18.44).
- **Release build of current main:** `scratch/eml/target/release/eml`. It produced the strace counts below.

r4 applies the binding conformance ruling (X-ruling-r1 §1–§3, with the user-approved G-5 amendment) and the orchestrator resolutions (Cf-2, Cf-4, Cf-5). The changes from r3:
- **Constructors (V-2):** `RealDriver` has one value-taking root, `with_backend_and_lookup`, which reads no environment. `new`, `with_backend` and `with_lookup` are one-line delegates, and `from_env()` is called only in `new` and `with_lookup`. All of this is introduced in P2.
- **Construction enter (Cf-5):** `Uring::new` ends with one non-blocking enter that submits the waker's `POLL_ADD`. Construction fails with that enter's error. This is in P2, so the gate, R0 and Close all measure the same startup path.
- **`pick` (Cf-4):** `Linux::pick`'s exact signature is stated as the governing one.
- **Amended G-5 (ruling §3):**
  - Selection code outside `sys/` is only `enum IoBackend` and `impl IoBackend`.
  - `parse` becomes `IoBackend::parse`, and its test moves to `sys/linux.rs`.
  - The `real.rs` module doc stops naming backends.
  - The amended check is in P2.
- **The "ring tests do not run" check (Cf-2)** uses `--success-output immediate`.
- **Abort child test (V-1):** S1 plans none. The rule for whoever adds one is in the test bar.

The changes r3 made over r2:
- **Selection:** P2 introduces F1's final public shape: `IoBackend`, `IoBackend::from_env`, `RealDriver::with_backend`, a pure `parse`, and `sys::linux::pick`. Two things are prototype-only:
  - the variant `IoBackend::IoUringDirect` (value `io_uring_direct`);
  - `Auto` resolving to epoll.
- **The ring test helper:** it is now `ring()`. It fails, naming `EML_IO_BACKEND=epoll`, when no ring can be built; if `EML_IO_BACKEND=epoll` is pinned it prints one line and the test returns. It never skips on a machine setting.
- **G-3 to G-7 and G-9:** the checks are added to the P2 completion criteria.
- **echo-self-fresh:** it joins the gate cases on S2's side. Nothing changes in S1, but the direct arm is the one that can move it.
- **No plan IDs in docs or code:** the plan's identifiers (unit names, criterion IDs, review IDs, test numbers) never appear in `docs/` or in code comments. The IDs in this deliverable are for the plan only.

The response table for the earlier review and the F1 conformance table are at the end.

## 1. Conclusion

1. **Execution model: mixed** (F1 Q2: the long-term model).
   - The completion ops:
     - `RECV` into the lent buffer, `SEND` from the lent buffer;
     - `ACCEPT` into op-table memory, `CONNECT` from op-table memory;
     - a one-shot `POLL_ADD` for `Peek` and for the waker.
   - **Submission:** SQEs are queued. The next `wait`'s single `io_uring_enter` submits, waits and reaps them in one call. The only earlier submit is a non-blocking one when the SQ is full.
   - **Two measured arms:**
     - `io_uring` (try-first): makes exactly epoll's sync attempts.
     - `io_uring_direct` (ready-gated): skips the sync `recv`/`accept4` when the row's `ready` hint is false.
   - The arm that measures better ships as `IoBackend::IoUring`, and the `IoUringDirect` variant is then removed (continue branch).
2. **Minimum kernel 6.1.** The backend is usable only if all of these hold:
   - `io_uring_setup(256, {CQSIZE|SUBMIT_ALL|SINGLE_ISSUER|DEFER_TASKRUN, cq_entries: 1024})` succeeds;
   - `features ⊇ SINGLE_MMAP|NODROP|FAST_POLL|EXT_ARG`;
   - `IORING_REGISTER_RING_FDS` returns 1.

   If setup fails with `EINVAL`, or a feature is missing → `Err(Unsupported)`. `EPERM`/`EACCES` → `PermissionDenied`, `ENOSYS` → `Unsupported` (the OS error as-is). These are exactly F1's fallback classes. In the prototype nothing falls back; the continue branch's `pick` does.

   Why 6.1:
   - `DEFER_TASKRUN` charges all task work to our task, inside our `GETEVENTS` enter, with no IPIs;
   - 6.1 also guarantees `SUBMIT_ALL`, `CQE_F_SOCK_NONEMPTY` (which the `ready` hint needs) and registered ring fds;
   - one ring configuration;
   - an LTS kernel; the CI runner has 6.17 [dossier].
3. **Bindings: hand-written over `libc::syscall`** (G-4). About 120 lines of `#[repr(C)]` definitions with size asserts. No crate.
4. **Ring:**
   - SQ 256, CQ 1024.
   - The ring fd is registered, and every enter goes through `ENTER_REGISTERED_RING`.
   - A full SQ gets a submit-only enter.
   - `EBUSY`/`EBADR` from enter: reap, `enter(0, 0, GETEVENTS)`, reap.
5. **Waking:** the existing eventfd, watched by a one-shot `POLL_ADD` under `WAKE` and re-armed after each wake. Timeouts go through `EXT_ARG` with the exact `Duration`. `EINTR` and `ETIME` return `Ok(())`.
6. **Cancel:** queue `ASYNC_CANCEL` with `user_data = CANCEL` and return `Cancel::Pending`. The target's CQE becomes `Cancelled` or carries the op's real result.
7. **`user_data`:** `generation << 32 | op_index`; the generation goes up each time an entry is reused. This was chosen on merit (the op table also owns the sockaddr memory); the roadmap no longer prescribes it.
8. **Teardown:** the core is unchanged.
   - `Driver::drop` cancels the memory-holding ops, reads completions for up to 1 s, aborts if they do not finish, then closes the handles.
   - `Uring::drop` aborts if a memory-holding op is still in flight, then unmaps the rings and closes the ring fd. Op memory is freed after that (`ring` is the first field).
9. **Selection** (F1 Q4 / §5.4 shape, introduced in P2):
   - `pub enum IoBackend { Auto, Epoll, IoUring, IoUringDirect }` in `real.rs`, re-exported from `lib.rs`. `IoUringDirect` is prototype-only.
   - `IoBackend::from_env()` is the only reader of `EML_IO_BACKEND`.
   - `RealDriver::with_backend_and_lookup(b, lookup)` is the root and reads no environment. `new`, `with_backend` and `with_lookup` are one-line delegates (§2 Files).
   - `sys/linux.rs` holds `enum Linux { Epoll(Unix<Epoll>), Uring(Uring) }` and `pick`.
   - **Prototype `pick`:** `Auto | Epoll` → epoll; `IoUring` → `Uring(TryFirst)`; `IoUringDirect` → `Uring(Direct)`. Pinned io_uring values fail hard.
   - **What the continue branch changes:** only `pick`'s `Auto` arm (io_uring, falling back to epoll on `PermissionDenied`/`Unsupported`), and it removes the losing variant.
10. **Prototype bar:**
    - The whole workspace passes with `EML_IO_BACKEND` unset, `=io_uring` and `=io_uring_direct`, on a host where io_uring can be built.
    - With `EML_IO_BACKEND=epoll` the ring unit tests print one line and return, and the rest passes.
    - Plus the release smoke runs, the strace identity check and the G-check greps (P2).
    - Two commits.

## 2. Rationale

### Execution model (Q1)

Syscalls on current main, from `strace -f -c` of the release build [V]:
- **`many.em 1000`:** per connection, 4 `recvfrom` (1 EAGAIN), 2 `close`, 2.002 `epoll_ctl`, 2 `sendto`, 1.5 `accept4` (0.5 EAGAIN), 1.19 `epoll_pwait`, 1.001 `socket`, 1 `connect` (EINPROGRESS), 1 `getpeername`, 1 `getsockopt`. Also exactly 1 `epoll_create1` in the whole run.
- **`bulk.em 16 1 fresh fresh`:** 16384 `sendto` for 16384 1 KiB writes, 1523 `recvfrom` (381 EAGAIN), 383 `epoll_pwait`.
- **`echo.em 2000 16 fresh`:** per round trip 4 `recvfrom` (2 EAGAIN), 2 `epoll_pwait`, 2 `sendto`.
- **`bench/fib.em`:** 1 `epoll_create1`, 1 `epoll_ctl`, 1 `eventfd2`, all from the driver.

What each arm can save [I]:
- **Both arms:**
  - per connection: 2 `epoll_ctl`, `getsockopt`, `getpeername`, the sync `connect`;
  - the retry syscall after each wait (the CQE carries the data or the fd);
  - `epoll_pwait` becomes about as many `io_uring_enter` (through the registered ring).
- **`io_uring_direct` also removes:** the EAGAIN `recvfrom` (1 per connection in many-self, 2 per round trip in echo-self) and the EAGAIN `accept4` (0.5 per connection). The cost: a task park when the hint is a false negative. That is extra work but never an extra syscall, because the SQE completes inline at the next enter.
- **Not saveable by either arm:** bulk-1k's writes and echo-py's immediate round trips (1 sync syscall per op). Pure completion would turn each of these into a park and a resume with no syscall saved.

Expected outcome [I]: the gate is more likely to fail than pass. Per connection in many-self, about 191 µs is mostly VM and TCP-setup work. The direct arm is there so that a discard does not leave the strongest lever unmeasured.

### Features above the 6.1 baseline (the gate cases decide)

| Feature (kernel) | Decision | Reason in terms of the gate and the contract |
|---|---|---|
| `DEFER_TASKRUN` + `SINGLE_ISSUER` (6.1/6.0) | Required | Cheapest completion path, all CPU charged to our task; one ring configuration |
| `SUBMIT_ALL` (5.18) | Required | A bad SQE never strands the rest; removes the stop-on-error path |
| `CQE_F_SOCK_NONEMPTY` (5.19) | Relied on | The `ready` hint after a `RECV` CQE is always informative, which the direct arm uses |
| Registered ring fd (5.18) | Used | One registration; every enter skips an fd lookup; every `wait` is an enter |
| Registered files | Not used | An fd table to keep in step with the row table; no gate case shows fd-lookup cost |
| Multishot `POLL_ADD` (5.13) for the waker | Not used | Wakes are rare in the gate cases (resolver and monitor threads only); a one-shot re-arm is one line |
| Multishot `ACCEPT` (5.19) | Not used | Accepts connections nobody asked for. That breaks the one-event-per-step contract (backend.rs:57) and the accept errors rule (vm.md: `EMFILE` must be returned, not looped on). The gain is 0.5 `accept4` per connection in many-self, which the direct arm already removes |
| Multishot `RECV` + provided buffer rings (6.0/5.19) | Not used | Data would land in kernel-chosen buffers and need a copy into the VM's lent buffer, adding user CPU to every read. The VM lends one buffer per op (vm.md 「時計と driver」). The per-read re-arm it saves is already free inside the batched enter |
| `SEND_ZC` (6.0) | Not used | Slower than copying for 16 B and 1 KiB writes; adds a second notification CQE per send |
| `MSG_RING` (5.18) | Not used | The waker needs no ring of its own, and the eventfd poll already works ([V] probe 8b) |
| `ASYNC_CANCEL_FD`/`_ALL` (5.19) | Not used | The core cancels per op and per key; nothing in teardown cancels per fd; no gate case cancels |
| `CQE_SKIP_SUCCESS` (5.17) on cancel SQEs | Not used | It would save one ignored CQE per cancel, and no gate case cancels |
| `SQPOLL` | Not used | A spinning kernel thread charged to user+sys |
| `NO_SQARRAY` (6.6) | Not used | It would raise the floor to 6.6 to save filling an identity array once |

### Step mapping and the `ready` hint (Q2, C-2)

**`ready` (per row, read direction) is only a hint:**
- A false positive costs one EAGAIN sync syscall, after which `ready = false` and the SQE is queued.
- A false negative costs one `Pending` round. The queued `POLL_ADD`/`RECV`/`ACCEPT` checks state at submit ([V] probe 3: `POLL_ADD` on a readable socket completes at submit; probe 5: `RECV` with data completes inline), so it ends at the next `wait`.

No rule here can hang a step or spin.

**`ready` transitions:**

| Event | `io_uring` (try-first) | `io_uring_direct` |
|---|---|---|
| Row adopted (listen, socket, accept) | `false` | `false` |
| Sync `recv`/`MSG_PEEK`/`accept4` gets EAGAIN | `false` | `false` |
| Sync `recv` returns `n` | unchanged (as epoll) | `n < cap` → `false`; `n == cap` → unchanged |
| Sync `MSG_PEEK` returns data, EOF or an error; any accept result | unchanged | unchanged |
| `POLL_ADD` CQE, `res > 0` | `true` | `true` |
| `RECV` CQE, `res >= 0` | `flags & CQE_F_SOCK_NONEMPTY != 0` | same |

There is no `fresh` flag. In epoll, the first `Peek` after an edge is answered from `fresh`. Here the `POLL_ADD` CQE answers that `Peek` itself (`Done(Ok(Size(0)))`). A later `Peek` with `ready == true` does a `MSG_PEEK`, as epoll does once `fresh` is consumed.

**After an RST** ([V] probe3 H: RST mask 0x2019):
1. The `POLL_ADD` CQE has `POLLERR` → `ready = true`, `Retry`.
2. The re-started `Peek`'s `MSG_PEEK` returns `ECONNRESET`, consuming the error → `Done(Err(ConnectionReset))`.
3. The next `Peek` (`ready` still true) gets 0 from `MSG_PEEK` → `Done(Size(0))`.

This is the same sequence as epoll (unix.rs `peek`), and is pinned by `peeks_after_a_reset_report_the_error_once_then_eof`.

**Steps:**

| Backend call | Sync attempt | If it would block / skipped | `Start` | CQE → `Arrival` |
|---|---|---|---|---|
| `Peek` | `!ready` → none. `ready` → `recv(1 byte, MSG_PEEK\|MSG_DONTWAIT)`: data/EOF → `Done(Size(0))`; error → `Done(Err)`; EAGAIN → `ready = false` | `POLL_ADD(fd, POLLIN\|POLLRDHUP)` | `Pending(Hold::None)` | See the poll-result rule below |
| `Recv` | try-first: always `recv(buf, MSG_DONTWAIT)`. direct: only if `ready` | `RECV(addr = buf.ptr, len = min(buf.cap, i32::MAX as usize) as u32, msg_flags = 0)` | `Pending(Hold::Memory)` | `res >= 0` → `Done(Ok(Size(res as usize)))`, and set `ready` as in the table. Cancelled op with `-ECANCELED`/`-EINTR` → `Cancelled`. Else `Done(Err(from_raw_os_error(-res)))` |
| `Send` | always `send(buf, MSG_NOSIGNAL\|MSG_DONTWAIT)` (both arms; there is no write hint) | `SEND(addr = buf.ptr, len = min(cap, i32::MAX) as u32, msg_flags = MSG_NOSIGNAL)` | `Pending(Hold::Memory)` | `res >= 0` → `Done(Ok(Size(res)))`; the core re-issues short sends and maps 0 to `WriteZero`. Errors as for `Recv` |
| `Accept` | try-first: always `accept4(NONBLOCK\|CLOEXEC)`. direct: only if the listener's `ready`. `transient` → `ConnectionAborted` (unix.rs) | `ACCEPT(addr = &mem.addr, off = &mem.len as u64, accept_flags = SOCK_NONBLOCK\|SOCK_CLOEXEC)`, with `mem.len = size_of::<sockaddr_storage>()` set before the push | `Pending(Hold::Memory)` | `res >= 0`: `OwnedFd::from_raw_fd(res)`; peer = `from_storage(&mem.addr, mem.len)` (`None` → close the fd, `Done(Err(other("the peer has no IP address")))`); adopt a row; `Done(Ok(Accepted))`. `res < 0`: cancelled with `ECANCELED`/`EINTR` → `Cancelled`; otherwise apply `transient` → `ConnectionAborted`, then `Done(Err)` |
| `Connect` | none | write `RawAddr::new(peer)` into `mem`; `CONNECT(addr = &mem.addr, off = len)` | `Pending(Hold::Memory)` | `0` or `-EISCONN` → `Done(Ok(Connected))`. `-EISCONN` is defensive, for a possible re-issue on 6.1-era kernels [I]; it cannot be a real error on a fresh socket. Cancelled with `ECANCELED`/`EINTR` → `Cancelled`. Else `Done(Err)` |
| `listen` / `socket` / `shutdown_write` / `set_nodelay` | same syscalls as `Unix`, without registration | | | |
| `close` | release the row (generation + 1), drop the `OwnedFd`. `debug_assert` that no memory-holding op names the row | | | |

**Poll-result rule:**
1. `res < 0`: cancelled with `-ECANCELED`/`-EINTR` → `Cancelled`; otherwise `Done(Err)`.
2. `res & POLLERR != 0` → `ready = true`, `Retry`.
3. Any other `res > 0` → `ready = true`, `Done(Ok(Size(0)))`. This covers `POLLIN|POLLRDHUP` (EOF, [V] 0x2001) and `POLLHUP` alone (both directions shut; `recv` gives 0 [I]).

`POLLPRI` is not requested. `POLLNVAL` cannot occur, because the request holds the file [I].

**Hold rule:** an op whose SQE carries a pointer to our memory is `Hold::Memory`; only `POLL_ADD` is `Hold::None`.
- `Send` is included: a freed and reused lent buffer would put unrelated bytes on the network.
- `Connect` is included by the same rule, although its address is copied at submit ([V] probe2 C).

**Length clamp** (C-7): `len = min(cap, i32::MAX as usize) as u32`. The result is a short read or write, which the core already handles.

### Ring (Q3, C-6, C-8)

- **Setup:**
  1. `io_uring_setup(256, Params{flags: SETUP_CQSIZE|SETUP_SUBMIT_ALL|SETUP_SINGLE_ISSUER|SETUP_DEFER_TASKRUN, cq_entries: 1024})`.
  2. Feature check.
  3. Two mmaps (the ring with `SINGLE_MMAP`, then the SQEs).
  4. Identity SQ array.
  5. `io_uring_register(fd, REGISTER_RING_FDS = 20, &RsrcUpdate{offset: u32::MAX, resv: 0, data: fd as u64}, 1)` must return 1; keep `offset` as `ring_index`.
  6. `Uring::new` queues the waker's `POLL_ADD`.
  7. `Uring::new` ends with one non-blocking enter: `ring.enter(ring.unsubmitted(), 0, ENTER_GETEVENTS, None)`, i.e. `io_uring_enter(ring_index, 1, 0, IORING_ENTER_GETEVENTS | IORING_ENTER_REGISTERED_RING, NULL, 0)`. It submits the waker's poll and must return 1. `EINTR` retries the same call. Any other error makes construction fail with that `io::Error` (`from_raw_os_error`, so `EPERM` is `PermissionDenied` and `ENOSYS` is `Unsupported`). This lets a sandbox that allows setup but refuses enter be caught at construction, inside the continue branch's fallback classes. A return of 0 → `Err(other("io_uring did not take the waker's poll"))`. `with_sizes` (tests) takes the same path. Cost: one `io_uring_enter` per driver, inside `base` for every io_uring label.

  Any failure → `Err`. `EINVAL` from setup → `ErrorKind::Unsupported` "io_uring needs Linux 6.1"; a missing feature → `Unsupported` naming the feature. [V] probe4 shows steps 1 and 5 succeed here with sq 256 and cq 1024.
- **Enter:** `syscall(SYS_io_uring_enter, ring_index, to_submit, min, flags | ENTER_REGISTERED_RING, arg, argsz)`. The registered index is per task. Since the driver is not `Send` (C-3), it is always used on the registering thread. `to_submit` is always `sq_tail - load_acquire(kernel sq_head)`.
- **Full SQ:** `push` calls `enter(unsubmitted, 0, 0)`. If that errors or consumes nothing, the step ends with `Start::Done(Err)`, or `cancel` returns `Err` and the core breaks the driver. These are the only enters whose flags argument is exactly `ENTER_REGISTERED_RING` (0x10, no `GETEVENTS`). S2 counts them with `strace -f -e trace=io_uring_enter` and `grep -c ', 0, IORING_ENTER_REGISTERED_RING'`. No code counter is needed (C-8).
- **CQ overflow:** if enter returns `EBUSY` or `EBADR`: reap, call `enter(0, 0, GETEVENTS)` (this moves the overflow list into the CQ), reap again, return `Ok(())`. On `EAGAIN` (no memory for requests): reap and return `Ok(())`; the SQEs stay queued. `an_overflowing_completion_queue_loses_nothing` pins "no empty wait while overflowed completions remain".

### `wait`, waking, signals (Q4)

```
wait(timeout, events):
  n = sq_tail - load_acquire(kernel sq_head)
  rc = match timeout {
    Some(ZERO) => enter(n, 0, GETEVENTS, None)
    Some(d)    => enter(n, 1, GETEVENTS|EXT_ARG, Some(&GeteventsArg{sigmask: 0, sigmask_sz: 8, pad: 0, ts: &KernelTimespec(d)}))   // argsz 24
    None       => enter(n, 1, GETEVENTS, None)
  }
  Ok(_) | Err(EINTR | ETIME) -> reap
  Err(EBUSY | EBADR)         -> reap; enter(0, 0, GETEVENTS, None) (ignore EINTR; any other error returns Err only if the first reap queued nothing); reap
  Err(EAGAIN)                -> reap
  Err(e)                     -> return Err(e)     // nothing queued yet
  Ok(())
reap: load_acquire cq tail; read each Cqe by value (cqes.add(i & mask).read()); dispatch; store_release cq head once at the end
```

- **Timeouts** ([V] probes 7 and 4): `ETIME` after 20.1 ms for 20 ms; a zero `ts` returns at once. The exact `Duration` is never early (like kqueue's `timespec`, kqueue.rs:116). Seconds saturate at `i64::MAX`.
- **Signals** ([V] probe2 E): an interrupted enter returns the submitted count, or `-EINTR` when nothing was submitted. Both end as `Ok(())` (vm.md 「上限と割り込み」). `sigmask = 0` keeps the mask.
- **Waking:**
  - `epoll::EventFd` becomes `pub(super)` (`new`, `wake`, `drain`).
  - `Uring::new` queues `POLL_ADD(efd, POLLIN)` with `user_data = WAKE`. In reap, `WAKE` → `drain()`, then re-queue. The poll checks state at submit, so a wake between the drain and the re-arm is kept for the next `wait`.
  - A wake from another thread ends a blocking enter ([V] probe 8b).

### Cancel, teardown, threads (Q5, C-3, C-4)

- **Op table:** `ops: Vec<Entry>` plus a free list.
  - `Entry { generation: u32, op: Option<InFlight>, mem: Box<Mem> }`.
  - `InFlight { key, what: Peek|Recv|Send|Accept|Connect, row, row_generation, cancelled }`.
  - `Mem { addr: sockaddr_storage, len: socklen_t }` (boxed, so its address is stable).
  - Releasing an entry bumps its generation.
- **Rows:** `Row { generation, read: Option<u32>, write: Option<u32>, ready }` plus a free list, as in `unix::Rows`.
- **`cancel(key, step)`:**
  1. Find the row and direction from the step (as `Unix::cancel` does), then the entry.
  2. If there is none: `debug_assert!(false)`, return `Ok(Cancel::Done)`.
  3. `debug_assert_eq!(entry.key, key)`.
  4. Push `ASYNC_CANCEL{addr: token}`, set `cancelled = true`, return `Ok(Cancel::Pending)`. If the push fails, return `Err`.
- **Probed cancel behaviour** [V]:
  - A cancel behind its target's unsubmitted SQE works: the target gets `-ECANCELED` (probe3 F).
  - If the target finished inline, the cancel gets `-ENOENT` and the data CQE arrives (probe3 G).
  - The two CQEs come in either order (probe 6).
  - The `CANCEL` CQE yields no event; `res ∉ {0, -ENOENT, -EALREADY}` → `debug_assert!(false)`. A lost target is caught by the core's `CANCEL_LIMIT` and `HELD_LIMIT`.
- **CQEs that match no live op:**
  - Free entry or generation mismatch → `debug_assert!(false)` and drop it. For an `ACCEPT` with `res >= 0`, close the fd.
  - Live entry whose row generation changed: only a `POLL_ADD` whose row `Driver::drop` released. Skip the row update; the core drops the event, because the key is gone.
- **Threads** (C-3) [V]:
  - A scratch `cargo check` of `is_send::<eml_io::RealDriver>()` fails (E0277: "`*mut u8` cannot be sent between threads safely" within `eml_rt::driver::RawBuf`).
  - Every construction site in `crates/eml_io/tests/real.rs`, `crates/eml_vm/tests/api.rs` and `signal.rs` was read. Each driver lives on one thread. In `real.rs:1608` it is created inside the spawned thread. `real.rs:1639` moves only the waker `Arc`.
  - `Uring` holds raw ring pointers, so it is not `Send` either.
  - So `SINGLE_ISSUER` and the per-task registered ring index are safe by type. An `EEXIST` or `EBADF` from enter would be a `wait` error (driver broken).
- **Teardown** (C-4): `Driver::drop`:
  1. forgets `Hold::None` slots (their `POLL_ADD`s stay armed);
  2. cancels the `Hold::Memory` slots and `turn`s for up to 1 s, or aborts;
  3. then closes all handles (driver/mod.rs:925-931).

  Then `Uring::drop` runs: abort if any entry with `what != Peek` is in flight (defence in depth, as in `Iocp::drop`), then drop the fields in order: `ring` (munmap, close the ring fd, which cancels the armed polls and releases their files) before `ops` and `rows`.

  Epoll also closes the sockets only at the end of `Driver::drop`, so the extra delay before the peer sees FIN is only the few statements between the two drops [I]. `dropping_with_a_pending_peek_closes_the_socket` pins EOF at the peer within 1 s.
- **Direct `Uring` tests** (A-3): a test that leaves a `Recv`/`Send`/`Accept`/`Connect` pending must drive it to its CQE (cancel, then `wait`) before dropping the backend; otherwise `Uring::drop` aborts the test binary. The helper `settle(&mut Uring)` waits until nothing is in flight, with a 1 s watchdog. Only `dropping_with_a_pending_peek_closes_the_socket` drops with an op in flight, and that op is a `Peek`.

### Bindings (Q6)

- **Hand-written over `libc`.**
  - The ABI is stable.
  - libc 0.2.191 has `SYS_io_uring_setup` = 425, `SYS_io_uring_enter` = 426 and `SYS_io_uring_register` = 427 on x86_64 and aarch64 gnu, plus `EBADR`, `ETIME`, `MAP_POPULATE`, `POLLRDHUP` ([V] grep under `dl/libc_x`).
  - All `unsafe` stays in one file that `trust.rs` audits. This agrees with async.md:243 (backends written ourselves).
  - `Cargo.lock` and `THIRD-PARTY-LICENSES.md` stay unchanged.
- **Rejected: the `io-uring` crate.** It adds two crates to the eml_cli Linux graph and hides the ring's own `unsafe`. The unsafe pushes and the `user_data` table remain ours anyway.
- **Rejected: rustix.** Same dependency reason.
- **Definitions** (`#[repr(C)]`, each with a `const _: () = assert!(size_of::<T>() == N)`). Layouts were checked [V] by `sz.c` against /usr/include/linux/io_uring.h (SQE offsets: fd 4, off 8, addr 16, len 24, op_flags 28, user_data 32, buf_index 40, personality 42, file_index 44, addr3 48).

```rust
struct Sqe { opcode: u8, flags: u8, ioprio: u16, fd: i32, off: u64, addr: u64, len: u32,
             op_flags: u32 /* msg_flags | poll32_events | accept_flags | cancel_flags */,
             user_data: u64, buf_index: u16, personality: u16, file_index: u32, addr3: u64, pad: u64 } // 64
struct Cqe { user_data: u64, res: i32, flags: u32 }                                                 // 16
struct SqOffsets { head: u32, tail: u32, ring_mask: u32, ring_entries: u32, flags: u32, dropped: u32, array: u32, resv1: u32, user_addr: u64 } // 40
struct CqOffsets { head: u32, tail: u32, ring_mask: u32, ring_entries: u32, overflow: u32, cqes: u32, flags: u32, resv1: u32, user_addr: u64 } // 40
struct Params { sq_entries: u32, cq_entries: u32, flags: u32, sq_thread_cpu: u32, sq_thread_idle: u32,
                features: u32, wq_fd: u32, resv: [u32; 3], sq_off: SqOffsets, cq_off: CqOffsets }  // 120
struct GeteventsArg { sigmask: u64, sigmask_sz: u32, pad: u32, ts: u64 }                             // 24
struct KernelTimespec { tv_sec: i64, tv_nsec: i64 }                                                  // 16
struct RsrcUpdate { offset: u32, resv: u32, data: u64 }                                              // 16
const OP_POLL_ADD: u8 = 6; const OP_ACCEPT: u8 = 13; const OP_ASYNC_CANCEL: u8 = 14;
const OP_CONNECT: u8 = 16; const OP_SEND: u8 = 26; const OP_RECV: u8 = 27;
const SETUP_CQSIZE: u32 = 1 << 3; const SETUP_SUBMIT_ALL: u32 = 1 << 7;
const SETUP_SINGLE_ISSUER: u32 = 1 << 12; const SETUP_DEFER_TASKRUN: u32 = 1 << 13;
const FEAT_SINGLE_MMAP: u32 = 1 << 0; const FEAT_NODROP: u32 = 1 << 1; const FEAT_FAST_POLL: u32 = 1 << 5; const FEAT_EXT_ARG: u32 = 1 << 8;
const ENTER_GETEVENTS: u32 = 1 << 0; const ENTER_EXT_ARG: u32 = 1 << 3; const ENTER_REGISTERED_RING: u32 = 1 << 4;
const REGISTER_RING_FDS: u32 = 20;
const OFF_SQ_RING: i64 = 0; const OFF_SQES: i64 = 0x1000_0000;   // the CQ shares the SQ mapping under SINGLE_MMAP
const CQE_F_SOCK_NONEMPTY: u32 = 1 << 2;
const WAKE: u64 = u64::MAX; const CANCEL: u64 = u64::MAX - 1;
```

- **`Ring`:**
  - Fields: `fd: OwnedFd`, `ring_index: u32`, the ring mapping (ptr, len), the SQE mapping (ptr, len), `*const AtomicU32` for the SQ head and tail and the CQ head and tail, the masks, `*mut Sqe`, `*const Cqe`.
  - Methods: `new(sq, cq)`, `push(Sqe)`, `unsubmitted()`, `enter(to_submit, min, flags, arg)`, `reap(f)`, and `Drop` (munmap both mappings; the fd closes after that).
  - Mapping length = `max(sq_off.array + 4*sq_entries, cq_off.cqes + 16*cq_entries)`, with `PROT_READ|PROT_WRITE` and `MAP_SHARED|MAP_POPULATE`.
  - Head and tail are accessed through `AtomicU32::from_ptr`.
  - SQEs are written with `sqes.add(i).write(sqe)` and CQEs read with `cqes.add(i).read()`. No slices are made.
- **Size** [I]: definitions about 120 lines, `Ring` about 190, backend about 450, tests about 420. `linux.rs` about 130. Edits to `unix.rs` and `epoll.rs` about 30 lines.


### Files, selection, trust (Q7)

- **`real.rs`** (core file per `trust.rs` `is_core`: no `libc::`, `Instant` or the other banned names). Under the amended G-5, backend names may appear only inside `enum IoBackend`, `impl IoBackend`, or `///` doc lines:

  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
  pub enum IoBackend { #[default] Auto, Epoll, IoUring, IoUringDirect /* 試作だけの腕 */ }
  impl IoBackend {
      pub fn from_env() -> io::Result<IoBackend>
      //   Linux: IoBackend::parse(env::var_os("EML_IO_BACKEND").as_deref()); other OSes: Ok(Auto), no read
      pub(crate) fn parse(v: Option<&OsStr>) -> io::Result<IoBackend>
      //   None -> Auto; "epoll" -> Epoll; "io_uring" -> IoUring; "io_uring_direct" -> IoUringDirect;
      //   anything else -> Err(InvalidInput, "EML_IO_BACKEND must be epoll, io_uring or io_uring_direct")
  }
  impl RealDriver {
      /// 根の構築子。環境を読まない
      pub fn with_backend_and_lookup(b: IoBackend, lookup: fn(&str, u16) -> io::Result<Vec<SocketAddr>>) -> io::Result<RealDriver> {
          Ok(RealDriver(Driver::new(sys::os(b)?, lookup)))
      }
      pub fn new() -> io::Result<RealDriver> { RealDriver::with_backend_and_lookup(IoBackend::from_env()?, sys::lookup) }
      pub fn with_backend(b: IoBackend) -> io::Result<RealDriver> { RealDriver::with_backend_and_lookup(b, sys::lookup) }
      pub fn with_lookup(lookup: fn(&str, u16) -> io::Result<Vec<SocketAddr>>) -> io::Result<RealDriver> {
          RealDriver::with_backend_and_lookup(IoBackend::from_env()?, lookup)
      }
  }
  ```

  - `parse` is `pub(crate)` so that its test can live in `sys/linux.rs`. A test in `real.rs` would put backend strings outside `impl IoBackend`.
  - `from_env`'s Linux branch is `#[cfg(target_os = "linux")]`, so this is the only `var_os`/`env::var` line in `crates/eml_io/src` outside `tests/` (G-6/G-7).
  - Doc comments on the delegates do not repeat the word `from_env`, so `rg -n 'from_env' real.rs` prints exactly the definition and the two calls.
  - **The module doc (`//!`)** no longer names epoll, kqueue or IOCP: 「OS を呼ばない核を、OS ごとの backend (`sys/`) の上で動かす」. Under the amended G-5 check only `///` lines may name backends.
  - Existing callers keep compiling unchanged: `with_lookup` (eml_vm `api.rs:935,958`) and `new`.
  - The continue branch replaces the error text when it drops `IoUringDirect`.
- **`sys/mod.rs`** gains the one place that maps an `IoBackend` to an `Os`:
  - Linux: `pub(crate) fn os(b: IoBackend) -> io::Result<Os> { linux::Linux::pick(b, uring::Uring::new) }`.
  - Apple and Windows: `pub(crate) fn os(b: IoBackend) -> io::Result<Os> { match b { IoBackend::Auto => Os::new(), _ => Err(io::Error::new(io::ErrorKind::Unsupported, "this OS has one I/O backend")) } }`.
- **`sys/linux.rs`** (new, no `unsafe`):
  - `pub(crate) enum Linux { Epoll(Unix<Epoll>), Uring(Uring) }`, with `impl Backend for Linux` delegating `type Sock = unix::Sock` and the 11 methods through `match`.
  - **Governing signature (Cf-4)**, an associated function of `Linux`:

    ```rust
    impl Linux {
        pub(crate) fn pick(b: IoBackend, ring: impl FnOnce(Arm) -> io::Result<Uring>) -> io::Result<Linux>
    }
    ```

    It is called only as `Linux::pick(b, Uring::new)` (`Uring::new: fn(Arm) -> io::Result<Uring>`) from `sys::os`. When the continue branch removes `Arm`, the parameter becomes `impl FnOnce() -> io::Result<Uring>` and the call becomes `Linux::pick(b, Uring::new)` with `Uring::new: fn() -> io::Result<Uring>`. Its tests then pass closures (`|| Err(...)`) to inject failures. S3 states its edits against this form. `Auto | Epoll` → `Linux::Epoll(Unix::new()?)`; `IoUring` → `Linux::Uring(ring(Arm::TryFirst)?)`; `IoUringDirect` → `Linux::Uring(ring(Arm::Direct)?)`. The constructor parameter lets the continue branch test `Auto`'s fallback with an injected failure (G-6: inputs are the value and the ring constructor only).
  - It reads no environment. It names no completion value, `abort`, `into_raw`/`from_raw` or `libc::`.
- **`sys/uring.rs`** (new; first item `#![allow(unsafe_code)]`; `#[cfg(target_os = "linux")]` on the module):
  - `pub(crate) struct Uring` with `Backend<Sock = unix::Sock>`.
  - Fields in order: `ring`, `waker`, `rows`, `ops`, `free_ops`, `arm`, `epoch`. **`ring` stays first.**
  - `pub(crate) enum Arm { TryFirst, Direct }`; `Uring::new(arm)`; the test constructor is `Uring::with_sizes(sq, cq, arm)`.
  - It reuses unix.rs's `syscall`, `recv_raw`, `send_raw`, `setsockopt_int`, `name_of`, `stream_socket`, `accept_raw`, `transient`, `RawAddr`, `from_storage`, `would_block` and the constants `BACKLOG`, `RECV`, `PEEK`, `SEND`, all made `pub(super)`. The `Sock` fields become `pub(super)`.
- **`sys/mod.rs`:**
  - On Linux: `mod linux; mod uring; pub(crate) type Os = linux::Linux; type Readiness = unix::Unix<epoll::Epoll>;`, plus `os(b)` above.
  - On Apple: `type Readiness = unix::Unix<kqueue::Kqueue>; pub(crate) type Os = Readiness;`.
  - Windows is unchanged.
- **`sys/unix.rs` tests:** `type Os = super::Readiness;`.
- **`sys/epoll.rs`:** `pub(super) struct EventFd` with `pub(super) fn new() -> io::Result<Arc<EventFd>>` and `pub(super) fn drain`.
- **`driver/backend.rs`:**
  - The trait is unchanged (G-5).
  - The four `cfg_attr(not(any(windows, test)), expect(dead_code))` become `not(any(windows, target_os = "linux", test))`.
  - The comments become 「完了型の backend は IOCP と io_uring だけなので、Windows と Linux とテストのほかに…」. The `Hold::Memory` doc names io_uring's lent buffer and the op-table sockaddr. All of these are `//`/`///` lines, which G-5's grep allows.
- **`trust.rs`** (in the commit that creates the files, because `FILES` uses `include_str!`):
  - `FILES` += `sys/linux.rs`, `sys/uring.rs`.
  - `UNSAFE_FILES` += `sys/uring.rs`.
  - `COMPLETION_BACKENDS = &["sys/iocp.rs", "sys/uring.rs"]`, with its comment changed to 「`OVERLAPPED` の箱か io_uring の op の表を持つ」.
  - Constraints these tests put on `uring.rs`:
    - no `from_raw_parts`, `mem::forget`, `Box::leak`;
    - pointers come from `&raw mut *entry.mem` and `Vec::as_mut_ptr`;
    - `abort` only here;
    - no raw strings.
- **Code text constraints:**
  - `crates/eml_io/src` must contain none of `MULTISHOT`, `MULTI` as a word, `BUFFER_SELECT`, `SEND_ZC`, `PROVIDE_BUFFERS`, `PBUF_RING`, `RECVSEND_BUNDLE`, in comments too (G-3 grep). Where a comment explains why multishot is not used, write the word in lower case.
  - `skipped` appears nowhere under `crates/eml_io/src/sys` (G-9 grep).
  - Comments are Japanese (natural-japanese skill), explain why, and cite docs/spec/vm.md 「時計と driver」/「上限と割り込み」, never plan identifiers.
  - `sys/mod.rs` and `real.rs` module docs mention io_uring next to epoll. `docs/` is not changed by the prototype.

### Test bar (Q8)

- **Existing suites** must pass in three runs: `EML_IO_BACKEND` unset, `=io_uring`, `=io_uring_direct`. Unset means `Auto`, which is epoll in the prototype. The `RealDriver` sites:
  - `crates/eml_io/tests/real.rs`: 50 `#[test]`, 50 constructions [V];
  - `crates/eml_vm/tests/api.rs` and `signal.rs`;
  - `crates/eml_cli/tests/cli.rs` (spawns `eml`) and `tls.rs`.

  `eml_vm/tests/bench.rs` uses no real clock [V].
- **Triage rule when a test fails under an io_uring value:**
  1. Reproduce it alone.
  2. If the spec requires what the test checks: backend bug. Fix it; measurement waits.
  3. If only epoll does what the test checks: do not change the test in the prototype. Record it in the plan's prototype notes as a candidate kind-1/2 change. Measurement proceeds only if it touches nothing on the gate programs' paths (`Readable`, `Read`, `Write`, `Accept`, `Connect` by IP literal, `close`, `quiesce` with nothing pending, wake).
  4. Timing failures are bugs.
- **Ring test helper** (in `sys/uring.rs` tests, conforming to F1 §5.4):

  ```rust
  fn ring(arm: Arm) -> Option<Uring> {
      if IoBackend::from_env().expect("EML_IO_BACKEND") == IoBackend::Epoll {
          eprintln!("EML_IO_BACKEND=epoll: ring tests do not run");
          return None;
      }
      Some(Uring::with_sizes(256, 1024, arm).unwrap_or_else(|e| {
          panic!("io_uring cannot be used here ({e}); set EML_IO_BACKEND=epoll to run the tests on epoll")
      }))
  }
  ```

  Tests call `let Some(mut u) = ring(arm) else { return };`. Tests with small rings use a sibling `ring_sized(sq, cq, arm)` with the same two branches. There is no skip on machine settings: on a host that refuses io_uring, the ordinary run fails with that message unless `EML_IO_BACKEND=epoll` is pinned. S3 owns the CI and testing.md side.
- **New unit tests in `sys/uring.rs`**, each over both arms:
  - Loopback, 1 s watchdogs, lower-bound-only timing, each under 1 s.
  - A test that leaves a `Recv`/`Send`/`Accept`/`Connect` pending drains it (cancel, then `wait`, via `settle`) before dropping the backend. Otherwise `Uring::drop` aborts the test binary. Only the drop-with-`Peek` test drops with an op in flight.

  The tests:
  - `a_token_keeps_its_index_and_generation`: round trip for `(0,0) (1,0) (0,1) (u32::MAX-2, u32::MAX)`; `WAKE`/`CANCEL` never decode below `u32::MAX - 1`.
  - `a_stale_generation_names_no_entry`: the pure `live(user_data)` gives `None` after release and reuse.
  - `a_pending_peek_completes_when_data_arrives`.
  - `peeks_after_a_reset_report_the_error_once_then_eof`.
  - `a_blocked_recv_is_completed_into_the_lent_buffer`.
  - `a_direct_recv_skips_the_sync_attempt` (Direct).
  - `a_short_sync_recv_clears_ready_in_direct` (Direct).
  - `a_blocked_send_finishes_after_the_peer_drains`.
  - `a_cancelled_recv_ends_cancelled`.
  - `a_cancel_queued_before_its_op_was_submitted_still_cancels`.
  - `accept_and_connect_complete_through_the_ring`: the peer address matches; `O_NONBLOCK` and `FD_CLOEXEC` are set.
  - `an_overflowing_completion_queue_loses_nothing`: `(4, 8)`, 32 readable `Peek`s; no empty `wait(Some(ZERO))` before all 32 have arrived.
  - `a_full_submission_queue_is_flushed`: `(4, 16)`, 12 pending `Peek`s.
  - `a_refused_connect_reports_connection_refused`.
  - `dropping_with_a_pending_peek_closes_the_socket`: no abort; the peer reads EOF within 1 s.
  - `a_wake_is_kept_until_the_next_wait`, `a_wake_from_another_thread_ends_a_blocking_wait`.
  - `a_zero_wait_does_not_block`, `a_timed_wait_does_not_return_early` (≥ 20 ms for 20 ms).
  - `a_send_to_a_closed_peer_raises_no_sigpipe`.
- **`parse` test:** `parse_maps_each_value` lives in `sys/linux.rs`'s tests (pure, no `set_var`; it covers all four values, unset and a bogus value). It calls `IoBackend::parse`. That keeps backend strings out of `real.rs`, and `sys/` is excluded from G-9's glob.
- **Abort child test (V-1):** S1 plans none. If one is added under `sys/`, it calls `crate::driver::tests::abort::in_child(<full test path>, || ...)` (with `in_child` made `pub(crate)` and `driver::tests` `pub(crate)` under `#[cfg(test)]`). `EML_IO_ABORT_CHILD` is read only in `driver/tests/abort.rs`.
- **`sys/linux.rs` unit test:** `pick_builds_the_named_backend`. `Epoll` → `Linux::Epoll`. `IoUring`/`IoUringDirect` → `Linux::Uring` with the matching arm, built through `ring`'s rule: if `EML_IO_BACKEND=epoll`, only the epoll case runs; otherwise a failure to build fails with the opt-out message. `Auto` → `Linux::Epoll` in the prototype. The continue branch's `new_follows_the_variable` replaces this test, with unset → `Uring`.
- **Sanitizers:**
  - Miri is unaffected: CI runs only `driver::tests::` under it, on the fake backend.
  - The ASan job (`cargo test -p eml_io`, env unset) now runs the ring unit tests, so on Linux it needs io_uring, or `EML_IO_BACKEND=epoll` pinned. That is S3's CI decision.
  - Kernel writes into the mmap'd ring and the lent heap buffers are invisible to ASan, so false reports are not expected [I, unverified].

## 3. Alternatives considered and rejected

- **Pure completion** (every step an SQE even when `ready`): turns the immediate paths of bulk-1k (16384 immediate `sendto` [V]) and echo-py into park and resume with no syscall saved, and breaks vm.md's one-`recv` promise.
- **Readiness on io_uring** (`POLL_ADD` as a `Poller` under `Unix<P>`, even multishot): measures only `epoll_pwait` against `io_uring_enter`. Excluded by the user and F1 Q6.
- **Zero-length `RECV` or `RECV(MSG_PEEK)` for `Peek`:** zero length gives 0 for both data and EOF ([V] probe 1); a peek byte would make `Peek` `Hold::Memory`.
- **Submit in `start`:** one more syscall per pending op, and no batching.
- **`user_data = OpKey`, or row + direction:** both are reused without a generation, so a stale cancel could hit a new op.
- **`RealDriver(Box<dyn IoDriver>)`, or an enum of two `Driver`s:** a second dynamic dispatch, or a doubled core.
- **Prototype-only parsing (`choose` in `linux.rs`, r2) that S3 replaces:** rejected. Introducing F1's final `IoBackend`/`from_env`/`with_backend`/`pick` shape in the prototype leaves the continue branch two edits: `Auto`'s arm in `pick`, and dropping one variant.
- **The arm in a second environment variable:** a second env read, against G-7's single reader.
- **A 5.11 fallback ring tier:** dropped; kernels below 6.1 use epoll.
- **Newer features** (multishot, provided buffers, `SEND_ZC`, `MSG_RING`, `CANCEL_FD`/`ALL`, `CQE_SKIP_SUCCESS`, `SQPOLL`, `NO_SQARRAY`, registered files): see the feature table.
- **Cancelling armed `POLL_ADD`s in `Uring::drop`:** closing the ring already cancels them, and the core closes sockets at the same point as with epoll.
- **A debug counter for full-SQ flushes:** strace tells these enters apart by their flags argument.
- **The `io-uring`, rustix and `polling` crates:** G-4.

## 4. Assumptions made

- [I] The gate programs submit one op per task and wait for it ([V] read `bench/net/many.em`; echo and bulk from traces).
- [I] On 6.1–6.17 (only 6.18 was probed):
  - `ACCEPT`/`RECV`/`SEND`/`CONNECT` on `O_NONBLOCK` sockets arm poll;
  - `CONNECT` returns 0 or `-EISCONN`;
  - `SOCK_NONEMPTY` is set as on 6.18.

  S3 verifies these on the CI kernel.
- [I] Under `DEFER_TASKRUN`, task-work CPU is charged to the eml process; network ops that arm poll do not use io-wq.
- [I] "A short TCP `recv` drained the queue" holds well enough as a hint. A false negative costs a park, never a hang.
- `Auto` = epoll in the prototype (coordination premise); the continue branch changes it to io_uring with fallback.

## 5. Implications for implementation

### Constraints

- No change to `driver/mod.rs`, the `Backend` trait, eml_vm or eml_cli.
  - `real.rs` gains `IoBackend` (with `from_env` and `parse`), `with_backend_and_lookup` (the root) and `with_backend`; `new` and `with_lookup` become delegates. `sys/mod.rs` gains `os(b)`.
  - `backend.rs` changes only its `cfg_attr`s and comments.
- `ring` is `Uring`'s first field. Op memory lives in `Box<Mem>`; never take pointers into `Vec<Row>` or `Vec<Entry>` storage.
- Every `unsafe` block has a Japanese `// SAFETY:` comment. Code and `docs/` never cite plan identifiers.
- `docs/` is not touched by the prototype.
  - Discard: S2 reverts the prototype commits.
  - Continue: S3 writes the docs, changes `pick`'s `Auto` arm, and removes `IoUringDirect` and the losing `Arm`.
- Test changes: only the `type Os` line in unix.rs's tests (kind 3). Everything else is added.
- **For S2:**
  - Labels: `--env epoll EML_IO_BACKEND=epoll`, `--env uring EML_IO_BACKEND=io_uring`, `--env direct EML_IO_BACKEND=io_uring_direct`. One binary for all three. Each label subtracts its own `base`. The gate is applied per arm (G-11).
  - Identity under the io_uring values: `io_uring_setup` 1, `io_uring_register` 1, `epoll_create1` 0.
  - The gate cases now include echo-self-fresh (F1). The direct arm targets its 2 EAGAIN `recvfrom` per round trip [V main strace].

### Unit P1 (mechanical): share the unix helpers

Covers visibility, `Sock` fields, `EventFd::new`, the `Readiness` alias, and the unix tests on `Readiness`. Run inside `nix develop`:
- `cargo nextest run -p eml_io` → exit 0, `0 failed`
- `cargo clippy --all-targets -- -D warnings` → exit 0
- `cargo fmt --check` → exit 0
- `nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run -- -D warnings` → exit 0

### Unit P2: io_uring backend, arms, selection shape

Tests first. On this host (io_uring available):

1. `cargo nextest run` → exit 0, `0 failed`, no `SLOW`.
2. For `V` in `io_uring io_uring_direct`: `EML_IO_BACKEND=$V cargo nextest run` → exit 0, `0 failed`, no `SLOW`.
3. `EML_IO_BACKEND=epoll cargo nextest run -p eml_io --success-output immediate` → exit 0. The output contains `EML_IO_BACKEND=epoll: ring tests do not run` at least once. Without `--success-output immediate`, nextest swallows a passing test's stderr (Cf-2).
4. `EML_IO_BACKEND=io_uring_direct cargo nextest run -p eml_io -j 1 --profile timing` → exit 0.
5. `cargo nextest run -p eml_io --lib -E 'test(trust::)'` → exit 0 (G-5).
6. `cargo clippy --all-targets -- -D warnings` and `cargo clippy -p eml_io -p eml_vm --all-targets --no-default-features -- -D warnings` → exit 0.
7. Cross clippy → exit 0:
   - `nix develop .#cross -c cargo clippy --target aarch64-unknown-linux-gnu --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run -- -D warnings`
   - the same with `--target x86_64-pc-windows-msvc`. The Windows build covers the non-Linux arm of `from_env`/`with_backend`.
8. `cargo fmt --check` → exit 0.
9. `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_io --lib -- driver::tests::` → exit 0.
10. G-greps (all [V] empty on main today):
    - `rg -n 'MULTISHOT|MULTI\b|BUFFER_SELECT|SEND_ZC|PROVIDE_BUFFERS|PBUF_RING|RECVSEND_BUNDLE' crates/eml_io/src` → no output (G-3).
    - `rg -n '^(io-uring|rustix|polling|mio)' crates/eml_io/Cargo.toml` → no output, and `cargo tree -p eml_io -e normal --prefix none | sort -u` lists only `eml_io`, `eml_rt`, `libc`, `windows-*`, vstd's build graph (and `eml_tls`'s subtree with `--features tls`) (G-4).
    - Amended G-5:
      - `rg -n 'io_uring|epoll|kqueue|iocp|IoBackend' crates/eml_io/src/driver | rg -v '^[^:]+:[0-9]+:\s*//'` → no output.
      - `rg -n -i 'epoll|io_uring|kqueue|iocp' crates/eml_io/src/real.rs` → only lines inside `enum IoBackend` / `impl IoBackend`, or `///` lines. Checked by reading the hits: each printed line number falls inside those two items, or the line starts with `///` after whitespace.
    - `rg -n 'var_os|env::var' crates/eml_io/src -g '!**/tests/**'` → exactly one line, in `real.rs` (G-6/G-7).
    - `rg -n 'pub fn from_env|pub fn with_backend|pub fn with_backend_and_lookup' crates/eml_io/src/real.rs` → three lines.
    - `rg -n 'from_env' crates/eml_io/src/real.rs` → exactly three lines: the `pub fn from_env` definition, the call in `new` and the call in `with_lookup`. None is inside `with_backend_and_lookup` (G-7).
    - `rg -n 'EML_IO_ABORT_CHILD' crates/eml_io/src` → only `driver/tests/abort.rs` lines (V-1).
    - `rg -n 'IoBackend::(Epoll|IoUring)' crates --glob '*tests*' --glob '!crates/eml_io/src/sys/**'` → no output; `rg -n 'skipped' crates/eml_io/src/sys` → no output (G-9).
11. `cargo build --release -p eml_cli`. Then, from the repo root, for `V` in `epoll io_uring io_uring_direct`:
    - `EML_IO_BACKEND=$V timeout 60 target/release/eml run bench/net/many.em 1000` → stdout exactly `many done 1000`
    - `... bench/net/bulk.em 64 1 fresh fresh` → `bulk done 67108864`
    - `... bench/net/echo.em 2000 16 fresh` → `echo done 2000`
    - all exit 0.
12. strace identity (G-3). `EML_IO_BACKEND=$V timeout 60 strace -f -c -o /tmp/u-$V.txt target/release/eml run bench/net/many.em 1000`:
    - **`V=epoll`:** must match main [V]: `epoll_create1` 1, `epoll_ctl` 2002, `connect` 1000 (1000 errors), `getsockopt` 1000, `getpeername` 1000, `accept4` 1500 (500 errors), `socket` 1001, `recvfrom` 4000 (1000 errors), `sendto` 2000.
    - **`V=io_uring` and `io_uring_direct`:**
      - `io_uring_setup` 1, `io_uring_register` 1, `io_uring_enter` > 0 (including the one construction enter), `eventfd2` 1;
      - no `epoll_create1`, `epoll_ctl`, `epoll_pwait`, `connect`, `getsockopt` or `getpeername` rows;
      - `socket` 1001, `sendto` 2000 (= writes);
      - `recvfrom` errors ≤ 1000 (= connections), and fewer under `io_uring_direct` than under `io_uring`.
    - Record the numbers in the plan's prototype notes.
13. `EML_IO_BACKEND=bogus timeout 20 target/release/eml run bench/net/many.em 1` → non-zero exit; stderr contains `EML_IO_BACKEND`.

## 6. Unresolved items (with defaults)

- **Behaviour on 6.1–6.17:** default as designed; S3 checks on the CI kernel.
- **ASan job and hosts without io_uring:** now that the ring tests fail rather than skip, a CI or ASan leg that refuses io_uring fails. Default: S3/G-8 decides (pin `epoll` on that leg, or discard per the user's answer 2).
- **Duplicated `listen`/`socket`/`shutdown_write`/`set_nodelay` bodies between `Unix` and `Uring`:** default kept small over the shared helpers.

## 7. Additional checks

- **Probes** [V], 6.18.44 (details in r2 §7, unchanged):
  - setup with `CQSIZE|SUBMIT_ALL|SINGLE_ISSUER|DEFER_TASKRUN`, sq 256, cq 1024;
  - `REGISTER_RING_FDS` returns 1, and a timed enter through the index gives `ETIME` at 20.1 ms;
  - `RECV`/`POLL_ADD`/`ACCEPT`/`CONNECT`/cancel/EXT_ARG/`EINTR`/close behaviours.
- **Layouts** [V] by `sz.c`.
- **strace on main** [V]: the P2 item 12 epoll numbers; `bench/fib.em` gives only the driver's 1 `epoll_create1` + 1 `epoll_ctl` + 1 `eventfd2`.
- **`RealDriver: !Send`** [V] by compile check. All test construction sites were read; each driver lives on one thread.
- **G-greps on current main** [V] (`rg` as in P2 item 10): the G-3, G-5, G-6 and G-9 greps print nothing today.
- No background task is running.

## Challenge

None. Every F1 criterion that touches S1 can hold on this design. Two points of timing, not conflict:
- **G-2's code check** (`new_follows_the_variable` with unset → `Uring`) is true only after the continue branch changes `pick`'s `Auto` arm. In the prototype, `Auto` is epoll by the coordination premise, and `pick_builds_the_named_backend` asserts that instead.
- **F1 §5.4** writes the helper's type as `ring(arm) -> Uring` but describes a `None` return. S1 uses `-> Option<Uring>`, as the described behaviour needs.

