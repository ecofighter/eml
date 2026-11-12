# Appendix 05: jit-effects, jit-tasks, jit-repl (design level)

(Where this file and 00-decisions.md differ, 00-decisions.md wins.)

Section C of 00-decisions.md is folded into this text; 00 remains the record. A line number is a hint; the named item decides.

# S4 response (r4): design-level plans for jit-effects, jit-tasks, jit-repl

Base 432fffb4. Tags: [V] verified (how is given inline or in section 7), [I] inferred, [S1] / [S2a] / [S2b] / [S3] taken from that deliverable (S1 = the contract r3, `S1-response-r3.md`; the others as adopted by it). This is Part 1 only, in full; it replaces r3. The r4 changes apply `X-resolutions-r1.md` (cited as XR-n to keep them apart from the contract's risk ids, cited as S1 R-n); the r3 changes applied `X-ruling-r1.md` section 3 (S4).

## Changelog r3 -> r4

- XR-5: `async_rows!` is dropped. The async rows are the `task` section lines of S3's `extern_rows!`; jit-tasks gives them bodies (D-8, 4.1 I-6, 5.2 unit 4). `CSubmit` / `CTake` are not `Extern` rows and stay outside the list (S3 D-12); D-13 now lowers them through two hand-written wrappers over `eml_sched::ffi`.
- XR-1 applied to the task lines: every line names its entry identifier with `as` (`fn AsyncSleep as eml_x_AsyncSleep (cx, ms: int) -> unit { .. }`). Probed (section 7, item 11).
- XR-6: no program-wide load-order number in generated code. `site` is S2b's function-relative site reference everywhere here (D-4, D-11, D-12, `JobSpec`, `Current`); by-name function values use S2b's per-name form of `fn_value`; the C function of a `blocking` call is reached by name. D-22's key is name + opened LIR text + layout hash (decision #10); the separate "effect names" input is dropped (they are in the LIR text).
- XR-7: jit-tasks lifts jit-core's usage error for `eml run --native` with `--workers` other than 1, `EML_WORKERS` other than 1 (00 C-7), `--gui` or `EML_GUI` (00 C-11) (5.2 unit 12, N-18 list, tests kind 1). The messages name no stage (00 C-10).
- XR-10: `eml_fatal(` appears only in `entries.rs` / `enter.rs`. Every re-raise here (`eml_nrt_par`, `eml_nrt_failed`'s request branch and its fatal outcomes, the safepoint) returns `Err(Fatal { code, site })` through the shim (D-4, D-11, D-15). 5.2's grep is S2b's standing one.
- XR-14: `doorbell.rs` and `Seat` are dropped from D-7's move list (sched-split moved them); request B-1 is dropped in favour of S2b's `Datum::Fn` / `fn_value`; the stale "S2b 5.3" citation is fixed.
- XR-2: re-raises keep the child's code (1, 2 = `FATAL_FAULT`, 3 = `FATAL_RECORDED`) and its payload; D-4's own fatal outcomes record and use `FATAL_RECORDED`.
- XR-8 / XR-11: `eml_nrt_par`, `eml_nrt_landed`, `eml_nrt_raise_arg` return data words whose only failure path is the fatal path (XR-8 wording); `WORKER_STACK` is S2b's one definition (D-20).

## Changelog r2 -> r3

- Premise (a) replaced: `Native` is the first field of `Ctx` (offset 0), `heap` the second (offset 72, one `add` to form `&mut ctx.heap`); every `Native` field is read through its `CTX_*` const (N-4: offsets <= 120, multiples of 8).
- `Fault::Engine` and `Heap::is_immortal` come from jit-core (S2b). D-15 uses `Fault::Engine` and adds nothing to eml_rt; jit-tasks edits no eml_rt file (S1 R-9). Removed from 5.2's N-18 list, its work units and its acceptance.
- D-4: the classification now starts with `take_fault()`; on `Engine` the worker payload is taken (a re-raise request first, then a panic word); otherwise the parked fault's rules apply.
- D-14: the geometry and `stack_limit = real` exist from jit-core (N-42); jit-tasks adds only the classes and `limits::set`, the single writer after thread start (grep check in 5.2).
- D-10 / 5.2: the nested-run rule and `BuriedRun` are N-43 difference (3), a user-visible semantics item of the jit-tasks plan; spec/native.md 「VMとの違い」 and stdlib.md 「`Std.Async`」 are written in the same commit; the U-S4-1 measurement is a plan task before marks are fixed; marked files cite N-43 (3) (kind 1).
- D-9: `Seeded` is a usage error natively, N-43 difference (2).
- 5.2: jit-tasks deletes the `eml_sched/testing` feature (N-41), with a unit that rewrites the remaining fixture uses on the public API (kind stated).
- D-1 / 5.1: `CTX_YIELD_ABORT` arrives in jit-effects under N-29's plain gate (`bench/rt-gate.sh`, <= +0.1 %).
- D-5: the fast-mode `Err` list is `WrongKind`, `StaleToken`, `RootMisuse`, `Unrecoverable` (Ch-1 accepted).
- D-17: `eml_run_pending` is a Rust function until native-aot; the C symbol is reserved under N-8 and arrives in native-aot.
- D-18 -> Close measurements under N-39 (`bench/native.sh` from jit-core; each Close replaces benchmarks.md 「ネイティブの数値」; the `--native` pass-through of `bench/async/scale.sh` and `bench/net.sh` is a jit-tasks unit).
- D-19: the `native-prof` proposal is dropped; the roadmap item 「ネイティブのプロファイラ」 is T0's (N-40, 前提 `native-aot`). Hooks are mapped to S1 1.7: this design lays down only task attribution (jit-tasks).
- Request to S2a removed (covered by N-35). File names are S2b's: `crates/eml_native/src/abi.rs`, `thunk.rs`, `mem.rs`.
- 1.0 cost table aligned with S1 1.7 (`perform`: 2 entry calls + 1 load + 1 indirect call; `eml_enter` round trip about 52 instructions).
- U-S4-3: the test-only per-entry budget is withdrawn (a test-only field); `max-depth` files are N-43 difference (1).
- Section 3: the line "one entry per prim with `catch_unwind`" was a stale contradiction of D-1 and is removed; Challenges Ch-1..Ch-4 are recorded as resolved.

Premises applied:
- (a) `Native` is the first field of `Ctx` and `heap` the second; every `Native` field is read through its `CTX_*` const.
- (b) Native performance is a first-class criterion, pursued through clean design only. Optimisations that need special cases, duplicated logic, fragile invariants or hard-to-verify code are forgone and listed in section 3.1. The VM is the reference / checking engine, and VM slowdowns are acceptable when they buy native speed.
- (c) Profiler readiness: the hooks of S1 1.7 that these stages own are laid down (D-19).

Owners whose interfaces are adopted unchanged:
- S2a: `eml_enter -> u64`, `enter() -> Result<(), Fatal { code, site }>`, `eml_fatal(ctx, code, site)` records nothing, `entries!`, the stub frame record (N-35).
- S2b: `After`, `after_unwind_point`, `eml$raise`, `eml_nrt_failed`, `eml_nrt_safepoint`, `Worker`, closures, `eml$apply$m`, `Const::Fn` / `Datum::Fn` / `fn_value`, `Image`, `Fault::Engine`, the function-relative site reference (XR-6), the fatal codes 1 / 2 / 3 (XR-2), the stack geometry consts, and the files `crates/eml_native/src/abi.rs` (the one signature helper, `CallConv::Tail`, `Linkage::Import`, the flag constructor), `thunk.rs`, `mem.rs` (the one memory-access module).
- S3: `extern_rows!` (one list, `task` section, `as` entry identifiers per XR-1), `GateHost`.

---

## 1. Conclusion

### 1.0 Native cost model of the paths these stages design

"Hot" means the case taken when nothing unusual happens. `L` = load, `B` = conditional branch, `C` = direct call, `I` = indirect call.

| Path | Fast path natively | Why minimal / what improves it later |
|---|---|---|
| Unwind check after a non-tail call (S2b's `After::Call`) | 1 L (`yielding.target`), 1 cmp, 1 B | Required by decision #6. Shared with the yield check (D-2) |
| Call that is a yield point (call; `prim yielding`; `switch`) | the unwind check (1 L + 1 B), then EP's inline check (1 L + cmp + switch) | Two checks, no special case. Folding them is forgone (3.1) |
| Yield passing a frame | 1 L + B (taken), 1 L (`abort`, a byte) + B, back to the hot path, EP's check, then the LIR slow path (κ closure, `yield_cont_take` / `yield_cont_set` calls) | κ allocation is semantic. The abort-test block is not marked cold (D-2) |
| `prim yielding` / `yield_status` elsewhere | 1-2 L, cmps, no call | Inline (D-1) |
| `perform E.op` (`ev_lookup` + `ev_clause` + `clause`) | 2 entry calls (`eml_rt_ev_lookup`, a walk of the evv by node tags; `eml_rt_ev_clause`) + 1 L (the clause's code word) + 1 I [S1 1.7] | Evidence reads stay out of generated code (N-24). Fusing the two calls and a per-site cache are forgone (3.1, U-S4-9) |
| Handler install (`handle`) | about 5 C (`evv_get`, `ev_new`, `ev_set_clause` × ops, `evv_push`, `evv_set`) + body | Allocation-bound and semantic. Recorded at Close (D-18) |
| Extern failure caught by `try_*` | cold | Not a hot path |
| Task switch (one `Async` wait and wake) | row body (registry lock, shared with the VM) + 1 `eml_enter` round trip (about 52 instructions [S1 1.7, S2a]: callee-saved registers, a `JmpBuf` link, the frame record, a thunk) + entry adapter | One enter per entry run, never per eml call. No stack switch (N-35). One enter per loop is forgone (3.1) |
| `par` fork / join | 1 C `eml_nrt_par`, fork marking + `fork_evv` (allocates one node pair per never-only handler), 2 × `eml_enter`, `transfer_back`, pair allocation | Same work as the VM plus the enter cost. Native-par adds stealing |
| Entry safepoint | S2b's: 2 L + 1 L + `get_stack_pointer` + 2 cmps + 1 B | jit-tasks adds nothing to the hot path. The limit state is `stack_limit` only (D-14) |

### 1.1 jit-effects

**D-1 Effect prims.**
- **Inline.** `prim yielding()`, `prim yield_status(ev)`: loads of `CTX_YIELD_TARGET` and `CTX_YIELD_ABORT`, the decision of `Ctx::yield_status` (RT ctx.rs L1238-1260 [V]). jit-effects adds `CTX_YIELD_ABORT` to eml_rt with its first reader (S1 R-table), under N-29's plain gate (`bench/rt-gate.sh`, <= +0.1 % per gated row; the N-29 exception belongs to jit-rt's layout task only). Like every `CTX_*` const it is `offset_of!`-derived next to `Ctx`, <= 120 and a multiple of 8 (N-4); `Native` is at offset 0.
- **One entry per prim.** Every other prim taken here is one `eml_rt_<prim>` entry calling the same-named `Ctx` method: `evv_get`, `evv_set`, `evv_push`, `evv_mask`, `ev_new`, `ev_set_clause`, `ev_renew`, `ev_lookup`, `ev_clause`, `ev_evv`, `ev_take_evv`, `ev_take_state`, `ev_set_state`, `yield_to`, `yield_abort`, `yield_cont_take`, `yield_cont_set`, `yield_finish`, `dummy`, `root_get`, `root_set`.
- These entries use the entry macro's ordinary shim body, `catch_unwind` included (U4 default, no exception list).
- **Effect numbers come from module data, not immediates** (review C-3). Each effect name `E` has one 8-byte data object, the data symbol `e:<E>` (00 C-6; declared once per machine, by name). `eml_nrt` writes the machine number into it at load: `NativeMachine` assigns numbers by name, as the VM's `Index.effects` does.
  - A use is `symbol_value` + 1 L.
  - `n_ops` (fixed by the effect's declaration) stays an immediate.
  - A function's code therefore depends only on names (S1 5.1), and native-aot's cache key needs no load order.
  - Cost: one L per effect prim site, next to a call.
- **Mask lists.** Each list is a data object named by content, the data symbol `m:<mask>` (00 C-6; e.g. `m:<E>/<keep>/<skip>;…`), using effect names; `DatumRow` gains the `e:` and `m:` kinds in jit-effects. eml_nrt fills it at load with machine numbers, sorted as the VM does at link (vm.md 「命令の形」 [R2]). It is passed as a pointer: `eml_rt_evv_mask(ctx, w, list)`. eml_nrt owns the layout of that object (it writes and reads it). eml_native only declares it with the size eml_nrt reports.
- **`prim yield_abort`.** The call is followed by `After::Never { raise: None }` (S2b D-4: unconditional jump to the site's cleanup, no load).
- **`empty_mut_array`** arrives with jit-externs ([S2b] C-6).
- Conformance: N-7, N-9, N-13, N-24, N-31.

**D-2 Unwind points that are calls: abort test after the unwind check; a yield returns to the hot path.**
S2b's `After::Call` hot check stays as is. jit-effects extends only `after_unwind_point`, one rule for every call point:
- `t = L target; brif (t != UNIT) split, hot`, then `split: a = L abort; brif a, abort, hot`.
- A yield returns to the hot path, where EP's own `prim yielding()` (inline, D-1) and `switch` take the LIR slow path. This is the VM's behaviour: a yield is an ordinary return of a dummy and is never unwound.
- `split` is not marked cold, because yields are the normal path of resumable effects.
- Folding EP's check into the unwind check by recognising its shape is forgone (3.1).
- **`abort` with a landing `v`.** `brif (t == v) land, other`:
  - `land: x = call eml_nrt_landed(ctx); jump hot` (the real dummy; jit-tasks restores the limit class in it);
  - `other: s = call eml_rt_ev_take_state(ctx, v); decref s; jump cleanup`.
- Review A-3: the abort test sits on the yield path, which is not cold. Per frame a yield pays 2 L + 2 B on top of the LIR slow path. Alternatives are in section 3.
- Conformance: N-9 (exactly one `target` load per call point; `abort` only after it). N-18: only `after_unwind_point` changes.

**D-3 Clause calls use S2b's closure entries.**
- A clause value is a closure or a static closure (`Const::Fn`) whose scanned word 0 is the code pointer of `c<k>:<g>` [S2b D-19/D-20]. For a clause function, `g`'s parameters after the captures are `(ev, op params…)` with typed leaves, so `c<k>:<g>` already has the op's signature (S2b D-20 and its interface table: "adapter `c0:<f>` typed for clause functions").
- `Call::Clause { c, args }` becomes:
  ```
  code = mem::closure_code(c)               // crates/eml_native/src/mem.rs
  r = call_indirect sig(op) code(ctx, c, ev, args…)   // sig from abi.rs, CallConv::Tail
  ```
  followed by `After::Call`. Clauses are always saturated (VM `clause_call`: arity must equal), so there is no tag test. `TailCall(Clause)` is `return_call_indirect`.
- **Functions reachable only by name** (review B-1):
  - the `fork$E.op` functions of `eml_lir::fork_plan` (D-6), for jit-tasks;
  - the clause functions of `Prelude.Panic.panicked` / `Prelude.IO.io_failed`, for `eml$raise`, when the program installs handlers for them. Their closures come from handler code, where they occur as values, so S2b's rule already covers them; only the op *signatures* are needed by `eml$raise`, from `Program.effects`.

  For the first set, jit-tasks calls S2b's `fn_value` for each `f` in `fork_plan` (a by-name root, which S2b already offers for `fork$E.op`; XR-14), giving a `Datum::Fn` static closure with `c0:<f>`. Under XR-6 the root is reached through a per-name data symbol, not a slot index.

**D-4 `eml$raise` and `eml_nrt_failed`.**
- `eml_nrt_failed(ctx, function, site, purity, out) -> i64`, called from `eml$raise(ctx, function, site, purity)` (00 C-2: `function` is the calling function's code address, `site` its local site index). jit-effects adds `out` on top of jit-core's signature (S1 1.4 procedure); `out` is a 2-word explicit stack slot of `eml$raise`.
- Classification order (review C-6; S1 1.4 "engine-held payload"): `f = ctx.take_fault()`, then
  1. `f == Fault::Engine`: take the worker payload. A re-raise request first (jit-tasks, D-15: `Propagate(fatal)` -> return `Err(fatal)` unchanged; `AbortStarted` -> `RAISE_DONE`), else the panic word (S2b `worker.panic`), which is taken as a `Panic` fault and continues at step 2 (catchable, so only the purity filter applies). jit-effects writes the request branch empty (`debug_assert!` no request) and jit-tasks fills it;
  2. any other `f`: if `!f.panics()` and `f.io_error_kind()` is `None` (this covers `Stopped`, `Heap`, `Output`), or the purity filter of VM `failed` (vm.rs L4083-4115 [V]) excludes it: record (`worker::record(ctx, fault, Some(At { function, site }))`) and return the `Fatal` it gives (`FATAL_RECORDED`; `Fatal.site` as jit-rt defines it, 00 C-2) **before any handler search**;
  3. `ev_find`. None: record with `worker::record` and `At`, and return the `Fatal` it gives. Found: `ev_clause`, write `[clause, ev]` to `out` and the arguments to the worker struct, and return the class `RAISE_PANIC = 0`, `RAISE_IO = 1`, (`RAISE_DONE = 2` comes only from step 1: an abort was already started).
- `eml$raise`:
  - `RAISE_PANIC`: `call_indirect` the clause with `(c, ev, msg)` by the signature of `Prelude.Panic.panicked`;
  - `RAISE_IO`: with `(c, ev, kind, msg)` by `Prelude.IO.io_failed`;
  - `RAISE_DONE`: nothing.

  Then it returns, and the site jumps to its cleanup. If the program has no `Prelude.Panic` / `Prelude.IO` effect, that branch is not emitted (no handler can exist).
- The message arguments are fetched by two calls `eml_nrt_raise_arg(ctx, i)` in the cold path. Rejected: putting them in `out` (a string word from Rust to a stack slot works too, but the worker struct already holds the panic word; one place).
- `site` is S2b's function-relative site reference (XR-6); this design passes it through and never forms a global number. The shim performs the `eml_fatal` for every `Err` (XR-10).
- Conformance: N-9 (`fault != 0` iff a failure is parked; engine-held payloads behind `Fault::Engine`), N-12 (`eml_nrt_failed` is in the list), N-18.

**D-5 Root prims.** `eml_rt_root_get(ctx, i)` and `eml_rt_root_set(ctx, i, w)`, fatal-only. `root_set`'s `Err(RootMisuse)` occurs in both modes; it is in the fast-mode `Err` list of spec/native.md 「エントリ」 (`WrongKind`, `StaleToken`, `RootMisuse`, `Unrecoverable`; Ch-1, accepted) and ends in the shim's fatal path as an internal-class error. They are tested by LIR-text codegen tests here and used by jit-repl.

**D-6 Evidence roles: constructors and readers in eml_rt; role numbers crate-private.** (Unchanged from r1.)
- **eml_rt** gets spec / const pairs `EV_TABLE`, `EV_STATE`, `EV_OUTER`, `NODE_EV`, `NODE_NEXT`, used by `ev_ok`, `node_ok`, `path` (ctx.rs L329-349 [V]) and every lemma. New `pub` items: `Heap::alloc_fork_table(&[Word])`, `Heap::alloc_fork_tail(panic, table)`, `Ctx::fork_ev(effect, table, depth)`, `Ctx::fork_of(ev) -> (u16, u32)`, `Ctx::evv_effects(w) -> Vec<u16>`. Nodes are built by `evv_push`, since a fork evidence satisfies `ev_ok` (ctx.rs L329-335 [V]).
- **eml_lir** gets `fork_plan(&Program) -> Vec<ForkPlan>`.
- **eml_sched** gets `ForkTables` and `fork_evv(ctx, &ForkTables, parent)`.
- **eml_vm** `compile/forks.rs` and `vm.rs` `fork_evv` / `child_abort` call these, byte-identical. The fork table entries become "function values" in both engines: `Word::imm(id)` in the VM (forks.rs L83-89 [V]) and `Const::Fn` words natively. The builders take words.

### 1.2 jit-tasks

**D-7 eml_sched takes the engine-neutral pieces and a small `Exec`; each engine keeps its loop driver.**
- **Moves** (VM-only commits first, byte-identical): from `workers.rs` (`Run`, `Slot`, `Notice<E>`, `Ready<E>`, `Entry`, `Workers`, spawn / join, stop procedure), the engine-neutral half of `Scheduler<P>` as `WorkerCore<P>` (sched.rs L359-442 [V]), `monitor.rs`, `jobs.rs`, `counters.rs`, the idle wait and deadlock verdict, the main-worker `Park` loop and `Pending` guard (vm.rs L877-1060), `join_outcomes`, `reraise_mask`, `ffi` submit / take, and the async-row bodies (D-8). `doorbell.rs`, `Seat`, `Sched`, `cells.rs`, `error.rs`, `park.rs` and the run-config types are already in eml_sched from sched-split (S2a D-20; XR-14).
- **`Exec`** (*illustrative*; review B-3 adds the construction hook):
  ```rust
  pub trait Exec: GateHost {                       // GateHost: S3's trait for the Gui helpers
      type M: Mode;
      type Held: Send;   type Child: Send;   type Join: Send;
      fn job(&mut self, core: &mut WorkerCore<Self>, spec: JobSpec) -> Self::Child;   // VM: Task::job; native: ChildEntry
      fn accept_child(&mut self, core: &mut WorkerCore<Self>, child: Self::Child);
      fn joined(&mut self, core: &mut WorkerCore<Self>, join: Self::Join, outcome: Outcome<Self>);
      fn limited(&mut self, core: &mut WorkerCore<Self>, cell: CellRef);
      fn token(&mut self, core: &mut WorkerCore<Self>, token: WaitToken);
  }
  pub struct JobSpec { pub cell: CellRef, pub worker: u32, pub evv: Word, pub g: Word, pub site: Site }   // Site: VM pc / native site reference (XR-6)
  ```
  Start and resume items stay frameless `Ready::Entry` (shared). The VM builds `Task::entry` from them in its own `pick`, as today (vm.rs L3866-3956).
- **`Current`** (the per-task state the rows read) is `{ cell, worker, depth_offset, site, outcome, exit, counted, added }`. The inventory of `task.` uses in `async_op` (vm.rs L2693-3680) is: `cell` 7, `depth_offset` 14, `entry` 1, `pc` 2, `worker` 19, plus operand reads and writes (`get` 45, `raw` 57, `set` 44, `set_raw` 29), which become row arguments and results [V: grep, §7]. `entry` and `pc` feed only `Task::job`'s location and become `site` (VM: the pc; native: S2b's function-relative site reference, XR-6).
- Native passes `depth_offset = 0`: native never uses `Ctx`'s depth limit (S2b's safepoint calls `poll(0)`), and `depth_offset` only rebases it (`switch_task`, ctx.rs L1714-1726 [V]).
- Native-speed trade: the VM keeps its own `switch_away` / `resume` (frames); native gets a short loop. Neither engine pays for the other.

**D-8 Async rows: the `task` section of `extern_rows!` gets bodies** (XR-5; S3 D-12).
- There is one list. jit-externs lists the 101 `is_async()` rows by name in `task { .. }`; jit-tasks replaces the names with lines of the common grammar, each naming its entry identifier (XR-1):
  ```
  task {
      fn AsyncRunRaw as eml_x_AsyncRunRaw (cx, g: obj) -> obj { .. }
      fn AsyncSleep as eml_x_AsyncSleep (cx, ms: int) -> unit { .. }
      ...
  }
  ```
  N-22's coverage test and `LISTED == Extern::ALL` see each row once.
- **Section convention** (owned here): the context `cx: &mut AsyncCx<'_, E>`, `AsyncCx = { core, ctx, cur, exec }`; the body returns `Result<RowEnd, RowFault>`, where the line's result kind is the kind of `RowEnd::Value`.
  - `RowEnd::{Value(Word), WaitRoot { root, handle }}`; `RowFault::{Fault(Fault), Raise(Raise), Reraise(Abort)}`.
- **Expansions** (S3's four; jit-tasks adds the `task` arms):
  - `defs` (eml_sched `rows.rs`): one generic function per line, as for the other sections;
  - `entries` (eml_nrt `rows.rs`): the `eml_x_<V>` wrapper through `entries!` `shim`. It runs the shared body; `Value` returns the word; `WaitRoot` runs the nested loop (D-10) and returns `handle`; faults park per the `faults` protocol (`run_raw` and `take_job` are `Effectful` rows, so `faults` is true [V: eml_extern lib.rs L1283-1292, L2546-2563]);
  - `vm_arms`: the `task` arms target `async_op` instead of `run_extern`; `WaitRoot` becomes `switch_away(WaitingCell)`;
  - `names`: the `task` section in `LISTED`.
- The unit test of XR-1 (entry identifier = `eml_x_` + variant) covers the task lines with the others.

**D-9 Native tasks are entries; switching happens only between entries; evv ownership is explicit** (review A-2).
- **Invariant:** between entries a worker's `Ctx.evv` is `()` and owns nothing.
- **Running a ready item** `Start | Resume | ResumeIo`:
  1. `cur := item`;
  2. `prev = ctx.switch_task((), cur.worker, 0)` (`prev` is `()`);
  3. `limits::enter_task(w, ctx)` (D-14);
  4. `enter(ctx, thunk, image.runtime_entries[kind], argv)`.
- **On return:** `debug_assert!(ctx.evv == ())`, since an entry returns with the evv it started with (lir.md 「evv の不変条件」 [R4 1.4]). After an abort that reached the entry (`exit`), the loop calls `yield_finish` and `evv_set(())`, as S2b D-13 does. Then `cells::finish(cur.outcome)`, the bookkeeping, and the publish (`Slot.current`).
- **Policies:** `Normal` and `Sequential`. With the native engine, `Seeded` is a usage error (N-43 difference (2)).

**D-10 Nested `Async.run`: a LIFO loop inside the `run_raw` wrapper.**
- The wrapper:
  1. runs the shared row body up to `WaitRoot`;
  2. pushes a level `{ token, caller_cur, caller_evv = ctx.switch_task((), …), caller_limit }` onto the worker struct's level stack; the caller's evv reference moves into the level;
  3. runs the native loop until `token(level)`;
  4. restores with `ctx.switch_task(caller_evv, caller_cur.worker, 0)` (which returns `()`), the limit, and `cur`;
  5. returns `handle`.
- **Ordering** (native.md L128 already accepts it: 「入れ子の順が逆になりうるので、`Async.run`は`main`から呼ぶのがよい」 [V: read]): a token for a level that is not the innermost marks the level `closed` and is acted on when the inner levels return.
- **Deadlock rule** (review C-4): the shared verdict counts a worker idle in a nested loop as parked. A buried closed level is not runnable work, so the verdict fires and reports. If the reporting worker has a buried closed level, the error is the new `RunFault::BuriedRun` with the text `deadlock: an Async.run waits under a nested Async.run on the same worker`; otherwise it is the usual deadlock. It is never a hang. The VM never produces `BuriedRun`.
- **Status** (Ch-3 accepted under N-43): this is difference (3) of spec/native.md 「VMとの違い」. The jit-tasks plan lists it as a user-visible semantics item, and the commit that adds the nested loop writes 「VMとの違い」 (3) and stdlib.md 「`Std.Async`」 together. A UI file or transcript that needs the inversion gets `-- native: skip (<reason citing 「VMとの違い」 (3)>)` (kind 1), fixed only after the U-S4-1 measurement task.
- **Depth** (review A-1): nested entries run on the caller's thread stack, so a task's depth budget is what the thread has left. Natively, a task inside a nested loop can reach `DepthLimit` / `ResourceLimit` earlier than in the VM, where each entry `Task` has the whole `max_depth`. This is part of N-43 difference (3) ("nested entries share the thread's depth budget") and is written in spec/native.md 「VMとの違い」 and 「タスクの上限」. With 1 GiB worker stacks it matters only for programs near the limit.

**D-11 `par`: a service that returns the pair or does not return; re-raise through a nested enter** (review C-1; fits the existing protocols).
- Lowering: `pair = call eml_nrt_par(ctx, f, g, site)`, then `After::Call` (par is a `Rhs::Call` unwind point; one `target` load; no `fault` load).
- `eml_nrt_par` is an `eml_nrt_` service returning a plain data word, the pair, whose only failure path is the fatal path (XR-8). `site` is the function-relative site reference (XR-6).
  1. Fork marking and `fork_evv`, as VM `fork_child` (vm.rs L1812-1834) does. Push a child record `{ parent_evv, child_worker }` onto the worker struct.
  2. Left child under the parent's id, then right child under the child id. Each is `enter(ctx, thunk1, eml$apply$1, [closure, ()])`, with its own `JmpBuf` and boundary. The children's limit class is `inherit(parent)` (VM `Limit::inherit`, vm.rs L136-145 [V]); without IO evidence in the fork evv, a depth overrun ends at step 9 of D-14 (fatal), as in the VM (vm.md L677).
  3. A child's `Err(Fatal { code, site })` (the error is already recorded in `worker.error` by its detector, [S2a] D-7: `eml_fatal` records nothing): pop the record, then return `Err(Fatal { code, site })` unchanged; the shim performs the `eml_fatal` (XR-10). The payload the code names stays where the detector left it (code 2: the fault parked in `Ctx`; code 3: the record in the worker struct; code 1: none; XR-2). Sequential order gives "left error before right error" (vm.md 「子の誤り」).
  4. Join (shared `join_outcomes`): `transfer_back`, `retire_worker`. On an abort of either side: `eml_sched::reraise_mask(ctx, effect, depth)`, then `enter(ctx, thunk1, eml$apply$1, [reperform, ()])`. That `reperform$` performs the `never` op in the parent; the parent handler's clause calls `yield_abort`, the enter returns with the abort state set, and `eml_nrt_par` returns a placeholder. The site's `After::Call` check sees the abort. Otherwise, allocate the pair.
  5. Its Rust frame holds only `Copy` values; every `Drop` value lives in the worker struct (D-15). A child's longjmp lands at the child's own `JmpBuf` inside `enter`, so it never crosses this frame.
- `prim child_abort(ev, c)` becomes `eml_nrt_child_abort(ctx, ev, c)`: it parks `Abort { fork_of(ev), c }` in the top child record and calls `Ctx::exit_abort()` (dummy target, ctx.rs L1401-1428 [V]); then `After::Never { raise: None }`. At the child's entry adapter the abort arrives with the dummy target, and `eml_nrt_par` takes the record and calls `yield_finish`.

**D-12 `compute` jobs** are frameless `Child` items run where a loop runs: placed on an idle worker, worker 0 included, or else locally (the VM rule).
- A job runs as `enter(eml$apply$1, [g, ()])` under the job id with the fork evv. Its outcome goes to the job cell, and a fatal error stops the run (D-15).
- `take_job` with an abort outcome:
  1. the wrapper runs `reraise_mask` + `enter(reperform)`, which sets the abort state;
  2. it stores the request `AbortStarted` in the worker struct;
  3. it parks `Fault::Engine` (jit-core's variant; D-15) and returns.

  Generated code tests `fault` (a `faults` row), `eml$raise` runs, `eml_nrt_failed` returns `RAISE_DONE`, and the site's cleanup follows the abort. This is the `faults`-row protocol unchanged.

**D-13 Blocking C.** `CSubmit` / `CTake` are `ExternRef` forms, not `Extern` rows, so they stay outside `extern_rows!` (S3 D-12; XR-5).
- The submit / take bodies move from VM `ffi.rs` to `eml_sched::ffi` (pinning and copying as vm.md L1099-1135 [V]); the VM's ops call them.
- Natively they are two hand-written wrappers in eml_nrt, `eml_x_c_submit(ctx, cfn, argv)` and `eml_x_c_take(ctx, cfn, token, argv)`, next to S3's `eml_x_c_*` helpers and under the same `eml_x_` wrapper protocol (both are `Effectful`, so `faults` is true). `cfn` is the address of jit-externs' per-name data symbol `c:<Module.f>`, whose contents are that C function's `CEntry` pointer (00 C-6; XR-6: no C-table index in code); jit-tasks adds no data symbol for it. The wait between them is the ordinary `Std.Async.io_wait` row (vm.md L1112-1116 [V]).

**D-14 Task limits: no new `Native` field; geometry from jit-core; one transition function.**
- **Fields.** `limit_class`, `stack_real` and `worker` have no generated-code reader, so by N-5 they live in the worker struct (Ch-4, accepted). Generated code reads only `stack_limit` (S2b D-5).
- **Geometry exists** (N-42, jit-core): `hard = base + 1 MiB` on workers and `stack_base + 2 MiB` on the main worker; `real = hard + LIMIT_HEADROOM_BYTES` (2 MiB); `stack_limit = real` from thread start. jit-tasks adds no const and does not change the depth a program reaches at class `Task`. Usable depth: 1 GiB − 3 MiB on workers; 8 − 4 = 4 MiB on the 8 MiB main thread.
- **Classes and the single writer** (review C-5). `eml_nrt/src/limits.rs` holds `enum Class { Stops, Task, Pending, Aborting { pending } }` and `fn set(w: &mut Worker, ctx, class)`, the only writer of `stack_limit` after thread start. The safepoint, `eml_nrt_failed`, `eml_nrt_landed`, the loop (`enter_task`), `eml_nrt_par` and the REPL recovery reset (D-21) all call `limits::set`. The transitions are VM `inherit` / `flagged` / `apply_limit` / `landed` (vm.rs L136-160, L683-770 [V]).

  | Class | `stack_limit` |
  |---|---|
  | `Stops` / `Task` | `real` |
  | `Pending` | `usize::MAX` |
  | `Aborting` | `hard` |

  Check (N-42): `grep -rn 'set_stack_limit(' crates/eml_nrt/src` hits the thread-start site and `limits::set` only.
- **`eml_nrt_safepoint`** follows the VM's `limit_hit` order (vm.rs L611-681 [V]):
  1. stops, then NOTICE (collect; a flag for `cur` makes the class `Pending`);
  2. `SP >= stack_limit`: `Continue`;
  3. `Stops` / `Aborting`: fatal `DepthLimit`;
  4. yielding: `Continue`;
  5. `SP < hard`: fatal;
  6. EP helper: `Continue`;
  7. pick the message; set `Aborting { false }`;
  8. IO handler found: park the ResourceLimit fault, clear the flag and counts, publish, return `ACTION_RAISE` (S2b's `leave` routes it to `eml$raise`);
  9. no handler: on a real overrun restore `Task` and fatal; on a flag only, set `Pending` and `Continue`.

**D-15 Fatal errors and stops across nested enters.**
- [S2a] `eml_fatal(ctx, code, site)` records nothing; the detector records into `worker.error` first ([S2b] D-6, D-13). So re-raising is "fail with the same `code` and `site`", and the payload stays where it is (XR-2).
- **No direct `eml_fatal`** (XR-10): `eml_fatal(` appears only in eml_nrt's entry shim and enter code. Services, the safepoint included, return `Err(Fatal { code, site })` and the shim performs the longjmp after the body has returned, so a longjmp from a service never crosses a live Rust local of that service.
- **Services that re-enter** (`eml_nrt_par`) re-raise by returning the child's `Err(fatal)`.
- **Rows that re-enter** (`eml_x_` of `run_raw`, `take_job`) may not call `eml_fatal` (N-9). They store the request `Propagate(Fatal)` in the worker struct and park `Fault::Engine` (from jit-core; jit-tasks adds no variant and no eml_rt edit, S1 R-9). `eml_nrt_failed` step 1 takes the request and returns it as `Err(fatal)`.
- This is N-9's wrapper protocol unchanged: a `faults` row parks `RunFault::Extern(f)` or, for an engine-held payload, writes the worker struct and `set_fault(Fault::Engine)`, so `fault != 0` iff a failure is parked; other `Err`s and caught panics end in the shim's fatal path.
- Rejected: an `eml_fatal` re-raise mode (it changes S2a's stub ABI).
- **At the base:**
  - worker k's loop on `Err`: `Run.fatal.first_writer(worker.error)` unless it is a stop, then `end_run`, leave;
  - worker 0's driver reports `Run.fatal` if set, else `worker.error`.

  An `exit` in a task is a `Propagate` of `Stopped(Ended)` after `cells::end_with_exit`.
- **Frames between a longjmp and its `JmpBuf`** (review C-2, N-12): every re-entering frame (`eml_nrt_par`, the `run_raw` wrapper, the job / task loop, `run_pending`) holds only `Copy` values (raw pointers, ids, `Word`s) across `enter`. Queues, drivers, child records and saved evv words live in the worker struct, which is reached through `ctx.runtime`. So N-12 holds literally, and a longjmp never crosses a value with a destructor.

**D-16 The worker struct and aliasing** (review B-4).
- `ctx.runtime` points at `Worker` (S2b's struct, extended). Rule: no `&`/`&mut Worker` and no `&`/`&mut Ctx` live across `enter`.
- Every access is `with_worker(ctx_ptr, |w, ctx| …)` (*illustrative*): it derives `&mut` from the raw pointers for the closure only. The closure type cannot call `enter`, because `enter` needs a `*mut Ctx` that the closure does not receive.
- Entries reached from generated code re-derive the same way, so their borrows never overlap with the loop's.
- **Miri test** `loops::a_row_inside_an_entry_reborrows_the_worker`: the fake `Enter` calls a Rust stand-in row that does `with_worker` through `ctx.runtime`. This exercises the re-derivation under Stacked and Tree Borrows.

**D-17 Main worker.**
- Native `Pending::run` is `eml_nrt::run_pending(ctx: *mut Ctx<Fast>) -> Progress` (a Rust function; review A-4). The `MainPending` object holds the main worker's raw `ctx`, as the VM's does (vm.rs L955-1040).
- `eml_run_pending` is a reserved N-8 symbol; its C symbol arrives in native-aot, when a C caller exists (accepted). Until then it is the Rust function above. The doc-revision task rewrites native.md's `-> Progress` to say so.
- The main worker's `hard = stack_base + 2 MiB` (N-42); its classes go through `limits::set` like any worker's.
- The stack floor comes from an OS query: macOS `pthread_get_stackaddr_np` / `pthread_get_stacksize_np`; Linux `pthread_getattr_np`; Windows `GetCurrentThreadStackLimits`.

**D-18 Close measurements** (N-39; record, never a gate on a ratio).
- Each Close runs `bench/native.sh` (added by jit-core: every `bench/*.em` under `eml run` and `eml run --native`, release, instructions retired, wall time, ratio, `rejected`) and replaces docs/implementation/benchmarks.md 「ネイティブの数値」.
- **jit-effects** completion condition: the native numbers of the effect benches are recorded: `bench/{state, state_loop, fold_state, ep_float, iter_generate, iter_pipeline}.em`, plus their `_by_hand` twins where they exist.
- **jit-tasks** work unit: the `--native` pass-through of `bench/async/scale.sh` and `bench/net.sh` (native rows next to the VM rows). Completion condition: the native numbers of `par_tree`, `bench/async/scale.sh` and `bench/net.sh` are recorded.
- **jit-repl**: `bench/native.sh` at Close (no new row).

**D-19 Profiler readiness** (premise (c); S1 1.7, N-34..N-37, N-40).
- The profiler is the roadmap item 「ネイティブのプロファイラ」 written by T0's doc-revision task (前提 `native-aot`, N-40). These stages propose no item of their own.
- Hook laid down here (the jit-tasks row of S1 1.7, check by review): **task attribution.** The published current-task word `Slot.current` (an `AtomicU64` cell word already published for the memory chooser, workers.rs L237 [V]) is written at each native entry start and end, and restored on nested-level return (D-10) and after a `par` child (D-11), so a sampler can attribute a sample to a task with one atomic read.
- Hooks owned elsewhere that these stages keep intact: frame pointers in all generated code (N-34, jit-rt; clause adapters and the thunks D-11 / D-12 use come from `abi.rs`'s one flag constructor); the stub frame record (N-35, S2a; this design adds no stack switch and no non-returning transfer besides `eml_fatal`); symbols and `Image.functions[i].{code, size}` (N-36, jit-core; `fork$` functions and the `e:` / `m:` data symbols are named by names only); direct calls colocated (N-37; clause calls are the `call_indirect` case N-37 allows).

### 1.3 jit-repl

**D-20 Native REPL runs on one persistent session thread, merged seat** (review B-2).
- The REPL thread spawns `eml-repl-worker` with S2b's `WORKER_STACK` (1 GiB; one definition, XR-11) once per session. The session thread is worker 0 and the main worker (merged seat, vm.md L469 [V]). The REPL thread sends each input's load-and-run to it and waits.
- No `Park` is used: the REPL has no GUI `Park`, and `Gui.run` raises `NotAvailable` (`crates/eml_cli/tests/repl/gui/not_available.repl` [V]). r1's GUI argument was wrong.
- The stack floor is the worker rule (`base + 1 MiB`), independent of the calling thread, so test threads with 2 MiB stacks run transcripts unchanged.
- While a run is in progress, the REPL thread blocks SIGINT / SIGTERM (the `eml_io` mask helper), so a signal interrupts blocking reads on worker 0. The `spawns::` test lists the site.
- The native orchestration runs with eml_sched's `Seat::Merged` (moved by sched-split; worker 0 = main worker on one thread, no `Park`), as jit-repl's first native unit.

**D-21 Recovery.** The VM's engine-neutral recovery sequence (`Ctx::recover` from the roots, dropping the registry, the `is_broken` classification) moves to eml_sched as a kind-3 change. Native adds resetting `Native` (yield state, `fault`, `jmp` null, and `stack_limit` to `real` through `limits::set(Stops)`, so N-42's single writer holds) and the `Worker` struct. JIT code, `Image`, the data objects and the frozen regions persist.

**D-22 Same name, same code.**
- `eml_native::reuse_key(program, f) -> [u8; 32]` = hash of the name, the opened LIR text and the hash of the layouts it names (decision #10). This is enough because generated code embeds no load-order number (XR-6: effect numbers, masks, by-name function values, C functions and sites are all reached by name or relative to the function); effect names are part of the LIR text.
- jit-repl adds it with a debug assertion in `NativeMachine::load` for known names. If S2b adds the assertion in jit-core, jit-repl only keeps the key (review B-6).
- native-aot uses the same function for its cache.

**D-23 Transcripts.** A native pass after the fast-mode pass in `crates/eml_cli/tests/repl.rs`; `# native: skip (<reason>)` marks are listed one by one in the plan (kind 1), and each reason cites a difference listed in spec/native.md 「VMとの違い」 (N-43).

## 2. Rationale

- **Effect semantics stay in eml_rt.** The native work is the propagation (D-2), handler entry (D-4) and the boundaries (D-11, D-15). Every decision lives in a shared routine.
- **Native speed through clean design** (premise):
  - every effect operation is one inline check or one entry call, with no extra layer;
  - effect numbers are data loads next to calls, not in hot loops;
  - no task-state write on the safepoint fast path (the class is encoded in `stack_limit`);
  - the VM keeps its own loop rather than sharing a slower generic one.

  Peephole special cases are forgone (3.1).
- **Protocol fit** (C-1): services return a value or do not return; rows park. `par` is a service; `run_raw` and `take_job` are rows; the cross-cutting requests ride in the worker struct behind one signal fault.
- **No `Drop` on re-entering frames:** this makes N-12 literal, and is also what makes the aliasing discipline checkable.

## 3. Alternatives considered and rejected

- **Inline the abort test on the hot path** (review A-3): an extra L and B on every call; aborts are rare. The yield-path folding is in 3.1.
- **Mark the whole yield path cold:** it moves the most frequent path of resumable effects out of line.
- **Effect numbers as immediates** (r1): load-order-dependent code (review C-3).
- **A shared generic worker loop:** it rewrites the VM's frame switching.
- **Fibers / per-task stacks** for nested `Async.run`: a stack switch, which N-35 forbids. **Baton threads**: a per-thread driver rework; kept as fallback F-1, a user decision only if N-43 (3) proves unacceptable.
- **Right `par` children on idle workers:** that needs join helping (native-par).
- **Re-raise through `Fault`-and-`eml$raise` for `par`** (r1): a fourth protocol (review C-1).
- **An `eml_fatal` re-raise mode:** a stub ABI change.
- **`limit_class` / `stack_real` / `worker` in `Native`:** no generated-code reader (N-5).
- **REPL on the calling thread** (r1 D-20): it fails on 2 MiB test threads, and the stated GUI reason does not hold.
- **One `eml_enter` per worker loop:** see 3.1.

### 3.1 Optimisations forgone (premise: clean design over special cases)

| Optimisation | Gain | Why forgone |
|---|---|---|
| Fold EP's `prim yielding` / `switch` into the unwind check at yield points | 1 L + 1 B per yield-point call | A special case that relies on EP's statement shape; the generic rule is correct for every shape |
| Fused `ev_lookup` + `ev_clause` entry with an out-slot | 1 C per `perform` | A peephole on adjacent prims, a second entry for the same work |
| No `catch_unwind` on the Verus-verified prim entries | Inlining inside the shim (zero measured fast-path cost, U4) | An exception list whose soundness argument (no panic in `verus!` code) must be re-checked for every prim |
| One `eml_enter` per worker loop | About 40 instructions per task switch | Puts a Rust loop under a `JmpBuf` and needs an N-11 exception (was U-S4-10) |
| `fork_evv` memo per worker | Allocations per `par` in recursive `par` trees | References held by a cache across runs; a fragile invariant (was U-S4-11) |
| Per-site `perform` cache | Most of the evv walk | Invalidation by evv identity (was U-S4-9) |

### Challenges (resolved by contract r3)

| # | Ruling | Where it lands here |
|---|---|---|
| Ch-1 | Accepted: the fast-mode `Err` list is `WrongKind`, `StaleToken`, `RootMisuse`, `Unrecoverable` | D-5 |
| Ch-2 | Accepted: geometry of N-42, present from jit-core | D-14, D-17 |
| Ch-3 | Accepted under N-43 (3): LIFO nesting and `BuriedRun` are a listed difference | D-10, 5.2, U-S4-1 |
| Ch-4 | Accepted: no new `Native` field; `Fault::Engine` from jit-core; jit-tasks edits no eml_rt file | D-14, D-15, 5.2 |

## 4. Assumptions and interfaces

### 4.1 Interfaces consumed (adopted from owners), and requests

| # | Owner | Interface adopted | Use here |
|---|---|---|---|
| I-1 | S2a | `eml_enter(ctx, thunk, entry, argv) -> u64`; `enter() -> Result<(), Fatal { code, site }>`; `eml_fatal(ctx, code, site)` records nothing; the stub frame record (N-35); `CTX_*` consts (`Native` at 0, `heap` at 72); `ctx.runtime()` / `set_runtime`; `detached(ctx)`; `entries!` | D-9, D-11, D-15, D-19, N-32 asserts |
| I-2 | S2b | `After::{Call, Faults, Never, Arith}`, `after_unwind_point(fx, point, after)`, `Point { owned, tokens, landing }`, cleanup blocks with constant placeholders | D-2 extends `Call` and the call lowering |
| I-3 | S2b | `eml$raise(ctx, function, site, purity)`; `eml_nrt_failed(ctx, function, site, purity) -> action` (00 C-2); `worker::record(ctx, RunFault, Option<At>)`, starting with `take_fault()`; `Fault::Engine`; `eml_nrt_safepoint(ctx, function, ep_helper) -> action`; `ACTION_{CONTINUE, UNWIND, RAISE}`; `leave` block | D-4 (adds `out`), D-14 |
| I-4 | S2b | `Worker { image, exit, panic, error }` behind `ctx.runtime` (the engine-held payload slot); `run` / `Start` / `Image`; the stack geometry consts and `stack_limit = real` (N-42) | Extended (D-9, D-10, D-14, D-15) |
| I-5 | S2b | Closures: tag = missing arguments, scanned word 0 = code of `c<k>:<f>`; `Datum::Fn(f)` static closures with `c0:<f>`; `fn_value` for by-name roots; `eml$apply$m`; generic PAPs; adapters with typed tails | D-3, D-11, D-12 |
| I-6 | S3 | `extern_rows!` with its `task` section and four expansions (D-12); `as` entry identifiers (XR-1); `GateHost`; `eml_x_c_*` helpers | D-8 gives the `task` lines bodies; D-13; native `Exec: GateHost` |
| I-9 | S2b | The jit-core CLI usage error for `--native` with `--workers` ≠ 1, `EML_WORKERS` ≠ 1, `--gui` or `EML_GUI` (XR-7; 00 C-7, C-10, C-11) | lifted in jit-tasks (5.2 unit 12) |
| I-7 | S2b | `worker.panic` holds the `panic` row's message word, marked by `Fault::Engine` (S1 1.4) | `eml_nrt_failed` step 1; `Raise::Panic` of `take_result` reuses it |
| I-8 | S2b | `crates/eml_native/src/abi.rs` (signatures, `CallConv::Tail`, `Linkage::Import`, the flag constructor), `thunk.rs`, `mem.rs` | D-1 (entry declarations), D-3, D-11, D-12 |

**Requests:**
- **To S2b:** none (B-1 withdrawn; `fn_value` covers it, XR-14).
- **To S3:** none; S3 D-12 already leaves the `task` section's context and result convention to this design.

### 4.2 Other assumptions

| # | Assumption | If false |
|---|---|---|
| A-1 [I] | After jit-externs, `async_op` is unchanged in eml_vm | the moves start from S3's state |
| A-2 [I] | `evv_push` accepts fork evidences on a frozen tail | `Ctx::fork_node` |
| A-3 [I] | No UI test has the nested-run inversion (7 files with ≥2 `Async.run`; the two read have one nested run per task [V]); S1 A9 | `-- native: skip` citing N-43 (3) (kind 1); F-1 only by user decision |
| A-4 [I] | The native UI pass at 1 / 2 / 8 workers fits 1 s per file | the 2 / 8 pass is restricted to `async/` without `clock: real`, or `#[ignore]`d with a reason |
| A-5 [V: grep §7] | `effects/`, `unwind/`, `panic/` use no async rows and no `par` | - |

## 5. Implications for implementation

### 5.1 jit-effects

**Changes to earlier code (N-18 list):**
- `after_unwind_point` (`Call`);
- `eml$raise` body; `eml_nrt_failed` signature (+`out`) and body; new `eml_nrt_landed`, `eml_nrt_raise_arg`;
- data symbols `e:<effect>`, `m:<mask>` and their `DatumRow` kinds (00 C-6);
- `Form::Prim` (except `ChildAbort`), `Form::Clause` and `Form::Prompt` removed;
- eml_rt: `CTX_YIELD_ABORT`, roles, constructors.

**Work units:**
1. Plan.
2. eml_rt roles, plus `CTX_YIELD_ABORT`. Gates (N-29, plain): Verus, verusfmt, Miri `trusted::`, `bench/rt-gate.sh` (<= +0.1 % per gated row).
3. eml_rt constructors and readers, with tests (Miri `trusted::` for the allocating ones).
4. `eml_lir::fork_plan`; eml_sched `ForkTables` / `fork_evv`; eml_vm rerouted (kind 3, G-10 grep).
5. Inline `yielding` / `yield_status`; the prim entries; effect data objects and mask lists. Codegen tests.
6. Clause calls (D-3). Tests: in-place clause, "under" form, tail clause.
7. D-2: the abort test, landing, `eml_nrt_landed`. Tests:
   - `never` abort through 3 frames with owned values and a token landing at `prompt$` (census);
   - a resumable yield through 3 frames;
   - multi-shot;
   - mask;
   - a CLIF pin `a_yield_point_call` showing the `target` load, the `abort` test and EP's check.
8. D-4. Tests:
   - `try_panic` around inline overflow, an index row and the `panic` row;
   - `try_io` around a failing open;
   - a `Stopped` fault is never caught (C-6);
   - no handler: the VM's text.
9. Root prims.
10. Native UI: snapshot (kind 2). Histogram: only `AsyncRow`, `BlockingC`, `Par` (S2b's order puts `Prim(ChildAbort)` files under `par` first). Expected from [S2b] 1.0 minus `prim` / `clause` / `prompt`: about 150 rejected and about 479 accepted `run` tests. This is not a bound (review A-5); the refresh measures it. 0 rejected in `effects/`, `unwind/`, `panic/`.
11. Close: the VM bench tables (eml_rt and eml_vm changed) and the native measurement of D-18 (`bench/native.sh`, 「ネイティブの数値」 replaced; effect benches recorded).

**docs/spec:**
- spec/native.md: 「エフェクト」 (prim table, data objects), 「節の呼び出し」, 「巻き戻しの地点」 (yield / abort split), 「extern の失敗」 (classes, order), 「エントリ」 (Ch-1);
- object-model.md 「evidence の3つの物体」 and 「基本操作の契約」;
- vm.md 「`par` の fork と join」;
- architecture.md; testing.md; benchmarks.md; status.md.

**Acceptance:** all CLAUDE.md gates; N-29's eml_rt gates (Verus, verusfmt, Miri `trusted::`, `bench/rt-gate.sh`); the histogram as above; the G-10 grep empty; N-24 review; the CLIF pin present; the native numbers of the effect benches recorded in 「ネイティブの数値」.

**Risks:**
- Lemma renaming cost. Fallback: name through the spec fns inside proofs only.
- Effect-heavy native performance below expectation. The D-18 numbers show it; 3.1 lists the optimisations that remain available.

### 5.2 jit-tasks

Scope as in S1's stage table; in particular the main worker (`hard = stack_base + 2 MiB`) and its pending loop as a Rust function (D-17). No eml_rt edit is expected (S1 R-9); if one turns out necessary, it takes N-29's gates and the plan lists it.

**User-visible semantics item** (N-43 difference (3)): the nested-run rule (LIFO levels; nested entries share the thread's depth budget) and `RunFault::BuriedRun`. The commit that adds the nested loop writes spec/native.md 「VMとの違い」 (3) and stdlib.md 「`Std.Async`」.

**Changes to earlier code (N-18 list):**
- `eml_nrt_safepoint` body; `eml_nrt_failed` steps 1 and `RAISE_DONE`; `eml_nrt_landed` body;
- `Worker` extensions (`cur`, levels, child records, request slot, `limits`); `run` replaced by the shared orchestration (Separate seat);
- the `task` section of `extern_rows!` (names -> lines) and its arms in the four expansions;
- the CLI usage error for `--native` with `--workers` ≠ 1, `EML_WORKERS` ≠ 1, `--gui` or `EML_GUI` removed (XR-7; 00 C-7, C-11);
- `Image.runtime_entries`, `fork_plan` statics;
- `Unsupported` deleted;
- harness worker counts;
- `eml_sched/testing` deleted (N-41).

**Work units** (VM-only first; each kind 3; `bench.rs` `RunStats` unchanged; `bench/run.sh` VM rows recorded, not gated, per the premise):
1. Plan.
2. eml_sched run table, `Exec`, spawn / stop.
3. `WorkerCore`, monitor, jobs, verdict.
4. The `task` section lines of `extern_rows!` with the VM arms (`async_op`); `Current` / `RowEnd`; `eml_sched::ffi` submit / take; async Gui / Tls arms (`GateHost`).
5. Main-worker loop, `join_outcomes`, `reraise_mask`.
6. Delete `eml_sched/testing` (N-41), once the files whose unit tests use it have moved: the remaining fixture uses (eml_vm's 54 `cells.rs` fixture uses, S1 A6, minus those already gone) are rewritten on eml_sched's public API. Kind 3 when every expected value stays byte-identical; a test whose assertion cannot be stated on the public API is listed one by one as kind 1 (deleted or moved) with its reason. Check: `grep -rn 'testing' crates/eml_sched/Cargo.toml` empty.
7. eml_nrt `Worker`, `with_worker`, `limits.rs`, the loop over an `Enter` trait. Miri and TSan loop tests, including the re-borrow test of D-16.
8. U-S4-1 measurement: run every `run` / `run-fail` file and transcript with ≥2 `Async.run` natively with the nested loop and list those that end in `BuriedRun`; the marks (kind 1, citing 「VMとの違い」 (3)) are fixed in the plan only after this task. Then async wrappers and the nested loop (D-10, `BuriedRun`), with the 「VMとの違い」 (3) and stdlib.md text in the same commit. Native `async/` at 1 worker.
9. `par`, `child_abort`, `compute`, re-raise. Tests:
   - aborts in the left, the right and both children;
   - a right fatal after a left abort;
   - nested `par` three deep with a fatal at the bottom (`ctx.jmp` restored; worker struct consistent);
   - a job abort re-raised by `take_job`.
10. Blocking C (`eml_x_c_submit` / `eml_x_c_take` over jit-externs' `c:` data symbols).
11. Limits. Tests: depth ResourceLimit caught in a task; EP-helper `Continue`; beyond `hard`; in a `par` child (fatal); flag-only `Pending`. Re-check the limit-mark files against U-S4-3.
12. Multi-worker: placement, stop procedure, main worker + `run_pending` and the GUI `Park`, signals. Harness 2 / 8; TSan. Lift XR-7's usage error: `--workers N`, `EML_WORKERS`, `--gui` and `EML_GUI` are accepted with `--native`; jit-core's CLI tests that expect the usage error are listed one by one (kind 1: deleted, or turned into run tests that check the worker count and the stub GUI natively).
13. `--native` pass-through of `bench/async/scale.sh` and `bench/net.sh` (D-18).
14. Delete `Unsupported` (kind 1 for the snapshot test).
15. Close: `bench/native.sh`, `par_tree`, `bench/async/scale.sh` and `bench/net.sh` native numbers into 「ネイティブの数値」; VM bench tables.

**docs/spec:**
- spec/native.md: 「VMとの違い」 (3), 「タスク」 (entries, evv rule, LIFO nesting, `BuriedRun`, depth per thread), 「`par`」, 「`compute`」, 「タスクの上限」 (geometry, table, single writer), 「止める手順」 (requests behind `Fault::Engine`, `Copy`-only frames), 「メインのworker」, 「エントリ」 (re-entering entries and `with_worker`);
- stdlib.md 「`Std.Async`」; vm.md (moved files); architecture.md; testing.md; benchmarks.md; status.md; diagnostics.md.

**Acceptance:** gates; `git diff --stat <stage base> -- crates/eml_rt` empty (else N-29's gates); `grep -rn Unsupported crates/` empty; rejected is 0; every `run` / `run-fail` file identical natively at 1 worker or marked with a reason citing an N-43 difference, and the order-independent `async` / `chan` / net files at 2 / 8 (`WORKER_RUNS`); `grep -rn 'set_stack_limit(' crates/eml_nrt/src` = thread start + `limits::set` (N-42); `grep -rn 'testing' crates/eml_sched/Cargo.toml` empty (N-41); stdlib.md and spec/native.md state the nested-run rule; native numbers of `par_tree`, `bench/async/scale.sh`, `bench/net.sh` recorded; TSan and Miri `loops::` green; `cargo tree -p eml_sched` clean; `grep -rn 'eml_fatal(' crates/eml_nrt/src | grep -v -e src/entries.rs -e src/enter.rs` empty (S2b's standing grep, XR-10); `eml run --native --workers 4` and `--gui stub` run, and so do `EML_WORKERS=4` and `EML_GUI=stub` (XR-7; 00 C-7, C-11).

**Risks:**
- Nested-run inversion (U-S4-1; N-43 (3)). If the measurement finds files whose inversion the user finds unacceptable, F-1 goes to the user.
- Size of the VM refactor: fallback is to split jit-tasks into `sched-exec` + `jit-tasks`, with user approval.
- Multi-worker time budget (A-4).
- Main-worker 4 MiB.
- Enter cost per switch (recorded at Close; the alternative is in 3.1).

### 5.3 jit-repl

Work units:
1. Plan.
2. Recovery to eml_sched (kind 3).
3. `Seat::Merged` in the native orchestration; the session thread (D-20); signal masking of the REPL thread during a run.
4. `NativeMachine` REPL surface (`run_shown`, `write_line`, roots, recovery reset), `reuse_key` with its debug assertion. Tests: two loads sharing functions; a fatal then recovery; roots surviving.
5. `eml repl --native`, conflicts, `Engine` in `Repl` (repl.rs L14 / L28 / L129).
6. Transcript native pass, marks (each citing an N-43 difference, kind 1), budget measurement.
7. Docs: repl.md, spec/native.md 「REPL」, testing.md, architecture.md, status.md.
8. Close (`bench/native.sh`, 「ネイティブの数値」).

**Acceptance:** the 67 transcripts identical to the VM fast pass except listed marks citing N-43; each within 1 s; all gates.

**Risks:** JIT memory grows until `close` (accepted).

## 6. Unresolved items

| # | Item | Default | Who / when |
|---|---|---|---|
| U-S4-1 | UI files and transcripts with the nested-run inversion | expect 0; mark citing 「VMとの違い」 (3) if found (kind 1) | jit-tasks unit 8, a measurement task before the marks are fixed |
| U-S4-2 | Split jit-tasks | one stage | user, at refresh if > ~25 commits |
| U-S4-3 | Limit-mark files natively | `max-depth` files keep jit-core's mark citing N-43 (1) (no word-count depth limit natively; a test-only budget would be a test-only field and is not proposed); `max-bytes` is honoured as is, so a file limited only by `max-bytes` whose result is now identical loses its mark (kind 1) | jit-tasks unit 11 |
| U-S4-6 | Native `RunStats` | VM-only counters zero | unchanged |
| U-S4-9 | Revisit any 3.1 item | forgone | only with the recorded Close numbers and a design that needs no special case |

## 7. Additional checks

All on the repository at 432fffb4, read-only.
1. [V: read] vm.rs: L59-70, L120-160 (`Limit`, `inherit`, `flagged`, `LIMIT_HEADROOM`), L611-770, L1888-1946, L2436-2585, L2693-2800, L3719-3760, L4027-4152.
2. [V: grep] `async_op` (vm.rs L2693-3680) `task.` uses: `cell` 7, `depth_offset` 14, `entry` 1, `pc` 2, `worker` 19, `get` 45, `raw` 57, `set` 44, `set_raw` 29; `Task::job` 1, `switch_away` 1, `schedule(` 14, `raise_again` 13. `depth_offset` flows into `Depot::discard` / `settle_*` in cells.rs (L1373, L3346-3397) for `switch_task` rebasing.
3. [V: read] `crates/eml_vm/tests/ir/mask.lir` L1-30: the shape `clause …; let y = prim yielding(); switch y { 0 -> b2, _ -> b1 }`, with the `decref` placed in the slow branch. This is the shape whose folding 3.1 forgoes.
4. [V: read] `crates/eml_cli/tests/repl/gui/not_available.repl`: the REPL has no GUI `Park`.
5. [V: read] RT ctx.rs L329-349, L1238-1260, L1401-1428, L1714-1726, L1831-1901, L1951-1965, L3036; roots.rs L75-137; externs.rs L17, L203-224.
6. [V: read] eml_extern lib.rs L1283-1292, L2251+, L2546-2563.
7. [V: read] [S2a] r1 D-6 / D-7 (enter / fatal), L58-81 (`Native`, `CTX_YIELD_ABORT` with its reader), L1737 (`preserve_frame_pointers`); [S2b] r1 D-4 … D-23, 5.3; [S3] r1 D-1, D-6, `GateHost`, U-S3-5, U-S3-8.
8. [V: grep] `effects/`, `unwind/`, `panic/` contain no async / `par` use; 7 files have ≥2 `Async.run`; 13 limit-mark and 9 signal-mark files.
9. [V: read] r3 inputs: `X-ruling-r1.md` section 3 (S4); `S1-response-r3.md` 1.7 (cost table, hook table), the stage table rows of jit-effects / jit-tasks / jit-repl, N-4, N-9, N-29, N-34..N-43, S1 R-9, A6, A9.
10. [V: probe, rustc 1.99.1, `scratchpad/jit/probe-r4/m.rs`] One `macro_rules!` list with a `generic` line `fn IntShow as eml_x_IntShow | ByteDisplay as eml_x_ByteDisplay (ctx, n: int) -> obj {..}` and `task` lines `fn AsyncRunRaw as eml_x_AsyncRunRaw (cx, g: obj) -> obj {..}` expands at one site into shared functions with a different context type and result (`Result<RowEnd, ()>`) for the task section, and into functions named by the `as` identifiers; it compiles and runs.
11. [V: read] vm.md L1099-1116 (`blocking`: `CSubmit` / `CTake` are `ExternRef` forms, `Effectful`; `io_wait` between them); `workers.rs` L208-213 (`Seat::{Merged, Separate}`); S3 r3 D-8 / D-12; S2b r3 D-19 / D-20 (`Datum::Fn`, `fn_value`).
12. Not run: any other probe (design level). Not measured: the yield-path cost, the Verus cost of roles. The enter cost is S2a's measured figure.
