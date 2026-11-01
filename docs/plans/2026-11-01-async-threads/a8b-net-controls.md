# A9 advice: driver calls made on the owner's thread, and what else must exist before placement

## Verdict
- The plan can't be followed as written. A9's soundness rule (6章 / s4 §1.1) needs a change the plan doesn't list: `Std.Net.shutdown_write` and `Std.Net.set_nodelay` have to become waits (`<IO, Async>`). That changes a std signature (stdlib.md 「`Std.Net`」), the extern table, and eml_rt's `Op` / `IoDriver`, so the eml_rt gate applies. The plan needs an amendment, approved before the work, adding a new unit between A8 and A9.
- I also found a second gap that the plan doesn't cover. A handle dropped on a worker that isn't its host goes into the host's `HandleBin`, but nothing wakes the host. On the real clock this can leave a socket open indefinitely, and the peer then hangs waiting for EOF. It must be fixed before placement turns on.

## 1. Every direct driver touch reachable from a task's instruction

| Where | What | Status once a task is on k ≥ 1 |
|---|---|---|
| crates/eml_vm/src/vm.rs:2592-2620 (`NetShutdownWrite`, `NetSetNodelay`) | Synchronous `driver_mut(&mut sched.driver)?.shutdown_write(handle)` / `.set_nodelay(handle, on)` on the owner's thread, on the handle read from the `NetStream` (`cells::stream_handle`, cells.rs:3502) | **Broken.** k ≥ 1 has no driver, so this raises the internal error `NO_DRIVER` (sched.rs:286-295). After A11 it would reach the wrong driver whenever the handle's host isn't the caller. |
| vm.rs:1942, 2353, 2374, 2401, 2437, 2452, 2468, 2479, 2505, 2544 (`sched.driver.as_deref_mut()` passed to `cells::submit_*`, `park_io*`, `cancel_on`) | `Option` driver. With `None`, `submit_op` (cells.rs:2409-2429) posts `Submit` to the host. | OK (A8) |
| vm.rs:2082, 2391; sched.rs:566 (`run.now_ms_ceil(sched.driver.as_deref())`) | Reads the time. Only `RunClock::Injected` asks the driver (driver.rs:69-75). | OK. `Injected` only comes from `Machine::with_driver`, whose runs are `Workers::Denied`, so k ≥ 1 never exists there. |
| cells.rs:3486-3505 (`local_port`, `peer_addr`, `stream_handle`) | Read fields cached in the heap object | OK, no driver |
| cells.rs:770-781 (`NetStream`/`NetListener` `Drop` calls `bin_push`) | Pushes onto the host's `Arc<HandleBin>` under its leaf mutex | Thread-safe, but **nothing wakes the host** (§3.1) |
| crates/eml_vm/src/tls.rs | No driver access, only `cells::` line helpers | OK |
| sched.rs:671, 690, 1324; workers.rs:760, 770 | Host-side paths that run on the driver's owner | OK |

## 2. `shutdown_write` / `set_nodelay`

Facts that decide the design:
- Both are plain `<IO>` externs, not `Async` waits (std/Net.em:258, 261; eml_extern lib.rs:1483-1497, both `Purity::Effectful`).
- In eml a task suspends only through the `Async` effect: `park_io` takes the captured continuation `k` (std/Async.em:58, 353). A plain `<IO>` extern can't suspend its task, and the VM has no resume-at-pc path.
- The test driver keeps the localhost socket state in memory, on worker 0, and isn't `Sync` (eml_io/src/test/net.rs:171 `end.shut = true`). The real driver keeps the handle → sock table inside the driver (eml_io/src/driver/mod.rs:796-808).

The options weighed:
- **(b) Call the socket directly from the owner's thread: rejected.** There is no OS socket under the virtual clock, and those are the clocks the `async/` UI tests run at N = 2/8 from A10 (net_shutdown_write.em). On the real driver it would mean sharing the handle table outside the driver thread, which breaks "a driver is used only on its own thread" (eml_rt driver.rs:100-103).
- **(c) A synchronous request to the host, with the owner thread blocking: rejected.**
  - The host is worker 0, which also runs tasks and `par` children, so a whole worker would block for an unbounded time.
  - It can deadlock against the end-of-run join and quiesce, where the host waits for the other workers.
  - It breaks determinism under `--sched sequential/seeded`.
- **A fire-and-forget notice with the error dropped: rejected.** It loses the error `shutdown_write` raises today (for example ENOTCONN), and the meaning would then depend on placement.
- **(a) Forwarded ops that end at submit: recommended.** The host only moves an `Op` and its raw result, and the owner keeps the words. That fits S1 Q5 unchanged.

### Code shape
**eml_rt (crates/eml_rt/src/driver.rs):**
- Add `Op::ShutdownWrite { handle }` and `Op::SetNodelay { handle, on: bool }`, both documented as "always ends at submit with `Submit::Done(Ok(Done::Size(0)))` or `Submit::Done(Err(_))`". A broken driver returns `Done(Err)`, as it already does for every op.
- Remove the trait methods `IoDriver::shutdown_write` and `set_nodelay`, so there is one entry point.
- driver.rs sits outside `verus!` and has no `external_body`, so no trust row (T0-T15) is needed. The eml_rt gate still applies:
  - `cargo verus verify -p eml_rt`
  - `verusfmt --verus-only --check`
  - tests, clippy, fmt.
- The fallback if the user would rather not touch eml_rt is a VM-side `enum HostOp { Driver(Op), ShutdownWrite(IoHandle), SetNodelay(IoHandle, bool) }` in `OpRecord.pending_op`. It works, but `run_io` then gets a second submit path, so I don't recommend it.

**eml_io:**
- `driver/mod.rs` `submit`: match the two variants, look up the handle as the current methods do (`handles.get_mut`, `!closed`, else `no_handle()`), call `backend.shutdown_write` / `set_nodelay`, and return `Submit::Done`. The backend traits (backend.rs:113-114) stay as they are.
- `test.rs` `submit`: the same, through `net.shutdown_write` / `is_stream`.
- Add one `driver::tests::` case over the fake backend, so the Miri `driver::tests::` command covers it.

**eml_extern + std:**
- Replace the rows `NetShutdownWrite` / `NetSetNodelay` with `Std.Net.submit_shutdown_write : TcpStream -> <IO> OpId` and `submit_set_nodelay : Bool -> TcpStream -> <IO> OpId`. The table row and the std declaration go together, and crates/eml_core_ir/tests/externs.rs must stay green.
- `shutdown_write` / `set_nodelay` become std wrappers with signature `TcpStream -> <IO, Async> TcpStream`, in the same shape as `write_bytes` (Net.em:201-221): `Async.io_wait (submit_… s)`, then `op_stream`, then `finish op`.
- Error text and kinds must stay byte-identical to today's `io_kind(e)` + `"{what}: {e}"`.
  - Add `OpKind::ShutdownWrite` / `OpKind::SetNodelay` with `prefix()` returning `"cannot shut down the stream"` / `"cannot set the no-delay option"`.
  - Make sure their `outcome()` puts every `Err` under `OUTCOME_FAILED`. Otherwise `RecordTaken::failure()` (cells.rs:417-424) returns `None` for an error mapped to 2..5, and a reset would pass silently. Let `finish` raise the error rather than `op_outcome`/`fail_code`, which would change the kinds.
- Cancellation, a decision for the user:
  - My recommendation: no `cancel_requested` check up front. Handle `Async.Cancelled` from `io_wait` as `write_bytes` does (`finish op; drop s; raise_cancelled ()`). That branch can only fire when the task really parked, which means a remote host.
  - The trade-off: N = 1 and owner == host behave exactly as today (`op_ready` returns 0, no park), but whether the call is a cancellation point then depends on placement.
  - The uniform alternative is to add the check up front, like `write_bytes`. That changes N = 1 behaviour for a cancelled task that calls these. Tls.close (Tls.em:139-143) already re-raises `Cancelled`, so Tls is fine either way. Grep the UI tests before choosing it.

**eml_vm:**
- Delete the vm.rs:2592-2620 arm and its entries in the list at vm.rs:3617-3619. `driver_mut` is then no longer imported in vm.rs.
- Add `cells::submit_control` (or two thin functions) that builds `net_record(OpKind::…, worker, Some(stream))` and calls `submit(shared, here, ctx, driver, depth_offset, record, Some(op))`. No `RawBuf` is involved.
- The fake drivers in crates/eml_vm/tests/{io.rs:326-333, 537-543, machine.rs:638-645, 827-833, api.rs:441-447, 726-732} drop their two methods and, if they match `Op`, handle the new variants. These are kind 3.

**Spec sentences, in Japanese with the natural-japanese skill:**
- stdlib.md 「`Std.Net`」: the new signatures. `shutdown_write` and `set_nodelay` are driver operations that end at once. With the recommended choice, they are cancellation points only when they wait.
- vm.md 「操作の記録」 / 「時計と driver」:
  - Every driver call goes through `submit`.
  - `ShutdownWrite` and `SetNodelay` end at submit.
  - When the owner isn't the host, the call travels to the host as `Submit` like any other op.
- Grep docs/ for `shutdown_write : TcpStream -> <IO>` and for any sentence that says the extern acts "on the spot".

**Tests:**
- Unit:
  - eml_io `driver::tests::` (fake backend, also under Miri).
  - test.rs: shutdown then the peer reads EOF, through `submit`.
- VM:
  - io.rs `a_shutdown_and_nodelay_are_ops_that_end_at_submit` (N = 1; the injected driver sees the `Op`).
  - In A9, once placement is on: io.rs `a_shutdown_on_worker_1_is_done_by_its_host` (virtual clock, N = 2, the task on worker 1 calls `set_nodelay True`, then `write`, then `shutdown_write`; the peer on worker 0 reads the bytes and then EOF; assert the output and `placed_tasks >= 1`).
- Existing UI tests net_shutdown_write.em, tls/shutdown_write.em and run-fail/tls/write_after_shutdown.em must stay byte-identical (K2). Check whether any program calls `Net.shutdown_write` / `set_nodelay` outside `Async`; that would be a pass/fail change (kind 1) and must be listed. bench/net/busy.em and echo.em already call them inside `Async`. bench.rs pins no net program, so no `RunStats` snapshot should move; confirm with K2.

**Which unit:** a new unit between A8 and A9 (for example "A8b: Make shutdown_write and set_nodelay driver ops"). It touches eml_rt, so the gate applies; it touches eml_io, so Miri on `driver::tests::`, ASan on `-p eml_io` and the Windows cross clippy apply; and it changes std and the extern table. Folding it into A9 would make A9's already large diff larger and would mix an eml_rt-gated change into the placement commit. Add the plan amendment (unit row, spec rows in §2.0, the test list, the cancellation decision) to docs/plans before starting.

## 3. Other things that must be in place before placement turns on

### 3.1 A cross-worker handle drop doesn't wake the host (must fix; not in the plan)
- **The drop path.** A `NetStream`/`NetListener` dropped on worker k (`Net.close`, or a `drop`) runs `bin_push` (cells.rs:770-781). That only pushes onto the host's `HandleBin`.
- **When the host closes it.** The host closes binned handles only inside `wait`, `completions` and `quiesce` (eml_io driver/mod.rs:230-241 `empty_bin`).
- **Real clock, host blocked on the driver.** With pending ops, worker 0 blocks in `driver.wait` (workers.rs:768-771), and only notices or a deadline wake it.
- **Host parked on its condition variable.** With no pending op and no deadline, worker 0 waits on the slot's condition variable (workers.rs:762-766).
- **Result:** a socket closed on worker 1 can stay open indefinitely. Two concrete hangs:
  - A reader hosted on worker 0 waits for EOF from an in-process peer that closed on worker 1, while main is waiting on something else.
  - An external client waits for the server's close.
- **Why N = 1 doesn't show it.** The drop happens on the host itself before its next `wait`.
- **The virtual clock** is probably safe: worker 0 waits on the driver only when every worker is idle, and that `wait` empties the bin. The real clock is broken from A9, and the bug stays after A11 for accepted streams and for any handle dropped away from its host.
- **Constraints on the fix:**
  - The `Drop` impls may take only leaf locks (plan 4章 lock order: "`TaskHandle`、`NetStream`、`NetListener` の `Drop` は葉の lock だけを取る"), so `Drop` can't take a slot lock or call `Run::send`.
  - N = 1 must not change: no extra waker syscall when the dropping thread is the host.
- **Suggested shape:**
  - Next to `bin`, keep a shared `Arc` link per host holding an `AtomicBool` meaning "the bin has handles from another worker". `Drop` sets it lock-free.
  - Each worker checks the flags of the driver-owning workers at its loop points (between task slices, before parking, at preempt switches). If a flag is set and the host isn't itself, it wakes the host the way a notice does, outside every lock. One host at A9; per worker after A11.
  - The host clears the flag when it empties the bin, and its wake path reaches `completions`, which empties the bin.
  - After the run ends the wake must be a no-op (`Weak`), because a `Lin` stream can outlive `Async.run` and the heap sweep drops leftovers.
- **Spec:** a sentence in vm.md 「時計と driver」 / 「操作の記録」: a handle dropped on a worker other than its host is closed by the host, and the dropping worker wakes the host.
- **Test, in A9:** workers.rs or io.rs `a_stream_closed_on_worker_1_reaches_its_peer` on the **real clock**, N = 2:
  - A client task forced onto worker 1 connects, writes, then `Net.close`.
  - The server task on worker 0 reads to EOF while main waits on an unrelated task.
  - Assert that the run ends under the watchdog.
- **Where:** the new pre-A9 unit or the A9 code commit, but before placement turns on.

### 3.2 Already covered, confirmed
- Time reads need no driver except under `Injected`, which never has k ≥ 1.
- `local_port`, `peer_addr` and the Tls helpers don't touch a driver.
- Every `submit_*` / `park_io*` / `cancel_on` call handles a missing driver by forwarding (A8).
- A9's own list (`next_place`, the sentinel first, `placed_tasks`, `exit` on k ≥ 1 calling `end_run` before `wake_all`) is unchanged by this advice.

### 3.3 Additions to A9's completion criteria
- `git grep -n "driver_mut" crates/eml_vm/src/vm.rs` should print nothing, as evidence that no owner-side driver call is left.
- The two new tests (§2 placed shutdown, §3.1 cross-worker close) go into the 20-run loop and the TSan/ASan `io:: workers::` filters.
