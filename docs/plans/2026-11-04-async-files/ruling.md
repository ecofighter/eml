# async-files: ruling on G-7 (files) and G-12 (outcome codes)

The user approved the Q1 amendment on 2026-11-04; `design.md` carries it. Q2 is applied in `commits.md` D5.

## Q1: G-7 for files (S2 D3, `Op::Adopt` at open)

Ruling: **the guideline needs amending**. D3 is the conforming mechanism once G-7 (and the B8/B11/A6/B12 sentences that repeat it) say files are bound at open on the opener's I/O host, as `connect`'s result is today. The review's alternatives do not work.

Reason:
- G-7's literal rule (an unbound file until the first read/write) cannot run at N = 2/8 on the virtual clock, which G-8 requires: `bind_stream` adopts on the owner's thread with the owner's driver and faults when `here.host != here.me` (verified cells.rs:4165-4190), and `own_driver()` does nothing on worker k >= 1 of a virtual-clock run (verified sched.rs:829-832). So the two criteria conflict as written.
- Review C-3's alternatives: "route the adoption to worker 0 by `Submit` mail" needs the host to hand the handle back to the owner, but a record's result is one `io::Result<Done>` and the host never touches the owner's words (verified cells.rs:3976-3980 "host は記録の語に触れない"); so it is a second record phase plus a new result shape, which is S2's rejected alternative 3, now with evidence. "`bind_stream` routes an unbound file to worker 0's driver" has the owner's thread touch another worker's driver, which vm.md:516 forbids ("driver は... 持つ worker だけが触る"). Rejected.
- G-7's "Why" (locality of the reading task) is kept for pipes and is not lost for files: `connect`'s result already binds to the accepting worker (verified vm.md:516, cells.rs:3435-3452) and `open` is the same pattern (the opener is the reader; only `accept` hands its result to a task elsewhere). On the virtual clock worker 0 hosts every worker's I/O, so "the opener's host" and "the first submitter's host" are the same driver there (verified vm.md:516). On the real clock a file read is a pool job on the host driver; a non-local reader costs only the existing `Submit`/`Completion` mail round trip, not a blocked thread (inferred from B8/B9). The precedent is a verified fact, not a reading; so the amendment is to the text, not the intent.
- Review C-3's "backends must answer `Adopt`" is wrong: the core maps `Op::Adopt` to its own `adopt(res)` (S2 commit 3), which every backend must implement under B8 anyway; no backend obligation is added (inferred from S2's text; not yet code).

Amended criterion text (replaces G-7's first clause):

> **G-7** Pipe ends MUST be created unbound (`Bind::Unbound`) and adopted by the driver of the first worker that submits on them. Files MUST be opened by the Host (`Host::open_raw`) and bound at open by `Op::Adopt { res: RawResource::File }`, answered at submit with `Done::Handle` by the core's `adopt`, hosted by the opener's I/O host (`Run::io_worker(k)`: its own driver on the real clock, worker 0 on the virtual clock), as `connect`'s result is; a file is never `Bind::Unbound` inside a `Foreign`. Unix parent ends MUST have `O_NONBLOCK`, ... (rest unchanged).
> - Why: locality of the reading task for pipes (the acceptor-style hand-off); for files the opener is the reader, as for `connect`, and the virtual clock hosts every worker's I/O on worker 0, so lazy adoption cannot run there (cells.rs `bind_stream`, sched.rs `own_driver`).
> - How to check: (unchanged commands) plus `cargo nextest run -p eml_vm --lib -E 'test(cells::an_open_record_binds_the_file_to_the_openers_host)'` passes (N = 2 on the in-file fake driver: a record opened by worker 1 has `Bind::Bound { host: 0 }` on the virtual clock, `host: 1` on the real clock).

Collateral amendments (same meaning, so the implementer does not meet contradictions): A6 bullet "The handle is `Bind::Unbound(RawResource::File)` until the first op adopts it" -> "The handle is bound at open by `Op::Adopt` on the opener's I/O host"; B8's "No `Op::Open`, no `Done::File`" -> "No `Op::Open` (the Host opens); `Op::Adopt { res }` and `Done::Handle(IoHandle)` are the driver's adoption answered at submit"; B11 first bullet: pipes only are unbound, files as above; B12 "`adopt(File)`" stays (the test driver answers `Op::Adopt` through it).

Rescue/ownership statement the implementer records in vm.md 「時計と driver」: the file's host is the worker whose driver adopted it (`Bind::Bound { host }`), fixed at open; rescue moves the owner task, never the host; ops from a non-host owner go by `Submit`/`Completion` mail as for a `connect`ed stream. `Op::Adopt` is answered at submit, so no task ever parks on an `Open` record and no rescue happens mid-adoption.

Pipes from `Exec.spawn` on the virtual clock: moot, with one guard that S2 already has (S2 line 402): `spawn_raw` raises `NotAvailable` ``cannot run `<prog>`: the test driver has no processes`` when `driver_kind == Virtual` before touching any driver, on every worker. Without that guard worker k >= 1 would hit the internal fault `a worker without a driver` (verified sched.rs:829-832 does not make one). Required: keep the guard, and add `tests/ui/run-fail/async/exec_needs_the_real_driver.em` (no `-- clock: real` mark, so it runs under seeds and at N = 2/8) whose task calls `Exec.spawn` and expects that failure; its outcome is the same on every worker.

Opener cancelled before settling (review A-3): required behaviour, which is the existing `connect` path and needs no new mechanism: (1) `OpKind::Open` settles like `Connect`: the owner builds the `IoStream` from `Done::Handle` at settle whether or not the record is `cancelling` (verified: the `Connect` arm at cells.rs:3435-3452 does not look at `cancelling`); (2) the wrapper's `Cancelled` branch calls `finish op` (Net.em:89-90 shape), which drops the record's `made`, so `Bind::close` pushes the handle to its host's bin and the host closes it at its next collection (verified cells.rs:795-800, vm.md:516 "落とした worker が host を起こして bin を空にさせる"); (3) a run that ends with a settled-but-untaken record is swept by `sweep_all`, which discards `made` (verified cells.rs:2749). Required test: `cells::an_open_cancelled_after_adoption_bins_its_handle` (fake driver, `cancelled: true` completion on an `Open` record, then assert the handle is in the host's bin), and `run/async/stream_open_cancelled.em` (cancel a task parked at `open`'s `io_wait`, assert the run ends without `a task is left at the end` and the test driver's file table is empty at `quiesce`).

## Q2: G-12 outcome codes (S2 D5, outcomes 0/6 only)

Ruling: **violates** G-12 and S1 A6. Required change below; no amendment.

Reason:
- A6's rule costs nothing: `outcome()` already classifies `BrokenPipe` with `ConnectionReset`/`ConnectionAborted` as code 2 for every `OpKind` (verified cells.rs:541-546), so a pipe write's EPIPE is code 2 with the existing function and D5 has to add a class branch to *remove* it, plus a `BrokenPipe -> ConnectionReset` special case in `finish`'s kind map that the unchanged path never needs.
- D5's "Why" (the wrapper cannot know the path or program) is contradicted by S2 itself: code-6 messages are built in Rust from the record's prefix (verified `Taken::failure()` at cells.rs:500-507 uses `kind.prefix()`), and S2 already puts an `Origin`-derived `Prefix` on every record at submit. The same prefix can be read by the wrapper.
- The unreachable `op_failed ()` tail defeats the "one wrapper shape" the criterion protects: a wrapper whose branch is wrong still type-checks, and `Net`'s branch structure (`finish` then `fail_code code prefix`) is what G-12 names.

Required change:
1. `outcome()` and `io_kind` unchanged; no class branch; `Taken::failure()` keeps building code-6 messages as `"<prefix>: <io::Error>"` with the record's `Prefix` (D5's prefix texts and D5's `Origin`-at-submit choice stay for code 6).
2. One new borrowing row shared by `Std.Stream` and `Std.Exec`: `op_prefix : Async.OpId -> <IO> (String, Async.OpId)` (`[Int] -> Obj`, `[Unr] -> Unr`, `&[true]`, 0, Effectful, `is_async`), returning the record's prefix (``cannot read `p` ``, ``cannot write to `prog` ``, ``cannot wait for `prog` ``, ...). `Net` does not get it (its prefixes stay literal).
3. Each module's wrapper follows `Net`'s branch exactly, with the prefix read before `finish`:
   ```haskell
   | Async.Woke ->
     if code == 0 then (take_bytes op, r)
     else
       let (prefix, op) = op_prefix op
       finish op            -- code 6 は `finish` が投げる
       drop r
       fail_code code prefix
   ```
   `fail_code : Int -> String -> <IO> a` is module-local in Stream.em and Exec.em (Net's is non-`pub`, modules.md:85): code 2 -> `fail_io ConnectionReset "\{prefix}: broken pipe"` (A6's kind, with the pipe wording), codes 3-5 -> the same kinds and texts as Net.em:248-251. `op_failed` is deleted.
4. Code 6's kinds are the `io_kind` map unchanged (`NotFound`, `PermissionDenied`, `Unsupported -> NotAvailable`, else `Other`), which equals S1 A6's list; `BrokenPipe` never reaches `finish` as code 6.
5. Tests: `cells::a_pipe_write_broken_pipe_is_outcome_reset` (fake driver completion `Err(BrokenPipe)` on an `OpKind::Write` with a child prefix -> `outcome() == 2`), and a `run-fail/async/exec_*.em` real-clock case (within G-8's limits) whose child exits before the parent writes, expecting kind `ConnectionReset` and ``cannot write to `eml`: broken pipe``.

Completion criteria: `git grep -n "op_failed" -- std/` prints nothing; `git grep -n "op_prefix" -- std/Stream.em std/Exec.em` shows one non-`pub` `extern` row per module and the uses in the `Woke` branches; `git grep -n "OUTCOME_FAILED" -- crates/eml_vm/src/cells.rs` shows no class-conditional branch in `outcome()`.

## Additional checks

- cells.rs:4165-4190 `bind_stream` (owner thread, `here.host != here.me` fault) and 3976-3980 (host never touches the owner's words): read.
- sched.rs:829-839 `own_driver`: no-op when `io_worker(k) != k`, so worker k >= 1 on the virtual clock keeps `driver == None`.
- vm.md:516: `connect` binds to the accepting worker; virtual clock hosts all I/O on worker 0; only the owning worker touches a driver; a non-host drop wakes the host to empty its bin.
- cells.rs:3435-3452 (`Connect` settle builds `made` without checking `cancelling`), 2741-2752 (`sweep_all` discards `made`), 795-800 (`Bind::close` bins a bound handle).
- cells.rs:500-507 and 530-549: `Taken::failure()` prefix from `kind.prefix()`; `outcome()` maps `BrokenPipe` to `OUTCOME_RESET` for every kind except `ShutdownWrite`/`SetNodelay`.
- Net.em:238-251: `raise_cancelled`, `fail_code` with literal prefixes and codes 2-5.
- S2 line 402: the `spawn_raw` virtual-clock guard already exists; S2 Q2 table: `source` is `Reader`-only, which is why Q2 uses a record-side `op_prefix` rather than per-handle name rows.
