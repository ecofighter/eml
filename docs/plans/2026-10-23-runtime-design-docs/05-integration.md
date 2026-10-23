# S4: integration of S1/S2/S3, application validation, first stage, stage sequence (r2)

Inputs: 01-runtime.md + 02-runtime-addendum.md ("S1"), 03-async.md ("S2"), 04-native-ffi.md ("S3"), S4-review-r1.md, repo at 4a0fbdf6. Tags: [V] verified (how), [I] inferred.

Settled by the user since r1: U1 `par` core goes before Async; U2 FFI goes before GUI; U3 the scheduler lives in a new crate `eml_sched`, outside Verus, checked with Miri and TSan, while `eml_rt` keeps word-level ops and Host-like traits; U4 `platform-ci` (GitHub Actions on macOS, Linux and Windows) is the first implementation stage.

Labels in this document (C*, G-*, NU*, T*, milestone codes M*/M-A*/N*) are for the implementer only and are never written into `docs/`. Stage slugs appear only in roadmap.md 「段の列」 and in `docs/plans/` file names.

## 1. Conclusion

### 1.1 Cross-consistency resolutions

Every row is decided, except the four rows marked NU, which need the user. Each NU row has a default that T1-T5 write unless the user overrides it. The docs therefore carry no provisional markers.

| # | Mismatch | Resolution written into the docs |
|---|---|---|
| C1 | Cancellation: S1 r3 drops `k` (hard drop); S2 delivers `Wake.Cancelled` at checkpoints | S2's model, as the S1 addendum accepted it. A hard drop of `k` happens only at `exit`, at a fatal error, at run end and in recovery |
| C2 | `exit` in a task: S2 propagates it to `main`'s bottom; the addendum uses a stop protocol | The addendum. `exit` unwinds only its own task, writes `Control`'s end slot and sets the stop bit. Every worker cuts to its entry and the host thread joins them. In-flight pool jobs are waited for (C11). Then the heaps are dropped (finalizers run) and the process exits with `n & 255`. `try_*` never catch `exit`, so nothing observable changes |
| C3 | Signals: S2 uses `signalfd`/`EVFILT_SIGNAL`; the addendum routes them through the monitor | The monitor. The handler touches only `Control` (`INTERRUPT`, plus a new `TERMINATE`). The monitor delivers a set bit to a registered `Signal.wait` waiter as a completion; with no waiter it stays fatal, as today. On Windows, the console handler (C30) sets the same bits |
| C4 | Root and entry rows differ | `Async.run : (Unit -> <IO, Panic, Async> a) -> <IO, Panic> a` is the user function. `run_scheduler` is the VM built-in behind it. `start_task : (Unit -> <IO, Panic, Async> a) -> <IO> Unit` |
| C5 | Soft-cancel names | `check_cancelled`, `cancel_requested` |
| C6 | GUI thread: S3 uses worker 0; S1 says worker 0 is a spawned thread | The GUI runs on worker M, the process main thread made a dedicated worker. Plugin assertions check that the current worker is M. `Main.run` targets M |
| C7 | GUI loop shape | S3's pull style (`Gui.next_event`). The plugin installs M's park at `Gui.open`. Toolkit callbacks call `rt_run_pending(M)` |
| C8 | Parallel in-place writes | Range primitives (S1); S3's `split_at`/`concat` pattern is dropped. `MutFloatArray` has no `split_at`. `concat` and `copy_slice` on immutable arrays stay as sequential operations |
| C9 | When `scan = 255` lands | In `raw-arrays`, before any native element access. The Verus specs of `arrays.rs` change once |
| C10 | `blocking` | It requires `<IO, Async>` and is lowered to submit/await/take on the blocking pool. `blocking` on a row without `Async` is a check error (a new E1xxx). Std's synchronous rows get no flag; async files come from the I/O driver |
| C11 | Blocking pool: S1 has it in M2/M3 with pool heaps; S2 needs it at N = 1 for DNS | The pool has no heap. Pool threads never touch RC and never allocate. A job record holds counted references (invariant 9) and is registered in the run-level registry (C14). The pool reads input payloads, writes the payloads of moved `Mut*Array`s and returns scalars, handles, the moved words or Rust data; `take_*` builds the eml objects on the owner worker. Completion goes through the owner's mailbox mutex (happens-before). The case table in §5.2 covers cancel, rescue, `exit`/fatal, recovery and checked mode. S1 invariant 7 becomes: "offloaded code reaches words only through its job record, which the run keeps alive until the job completes" |
| C12 | `Lin` payload whose receiver is unknown (MPMC `Chan`, `Task` result) | The sender runs the transfer mode with receiver = a reserved transit id. Only objects on an all-rc == 1 path from the `Lin` root get the transit owner; the first rc > 1 object switches its subtree to marking (rc < 0). The receiver's transfer walk claims objects with rc > 0 and owner == transit (owner := receiver, descend) and flips rc == -1 as before; it stops at everything else. Owner ids exist only in checked mode; in fast mode the sender's marking still happens and the receiver's flip walk is unchanged |
| C13 | Logical-id granularity for tasks | In checked mode every task gets a fresh logical id, like a `par` child. Rescue's sender id is the task's id. The run-level live-count table grows with tasks (checked mode only). Fast mode is unaffected |
| C14 | Root registration scope | A run-level registry holds shared Rust-side word holders: channel buffers, task-cell results, scope failure slots, join nodes, pool job records. Leak check and recovery (all parked) walk it |
| C15 | Scheduler placement | U3, settled. Crate order: `eml_rt` (word ops, plus `Host`, `Sched`, `IoDriver` traits reached through `Ctx`) ← `eml_sched` (threads, mailboxes, cells, monitor, pool, deques, park, and an `Exec` trait the worker loop calls) ← `eml_vm` (implements `Exec`). `eml_io` → `eml_rt` (implements `IoDriver`). `eml_native` → `eml_rt`, `eml_lir`, `eml_sched`, Cranelift (codegen, asm entry stubs, FFI trampolines; implements `Exec`). `eml_cli` → all. `Session::run` builds the runtime with the chosen executor and I/O driver, so `Session` stays the only pipeline driver. Each stage that adds a crate updates architecture.md and the CLAUDE.md diagram at its close (roadmap 「進め方」 steps 4 and 7) |
| C16 | "Driver" means three things | The terms are "I/O driver" (`IoDriver`, `eml_io`) and "host thread". `eml_native` holds the entry stubs and the trampoline generator |
| C17 | Extern faults in `par` children in native code | The native cold block applies the VM's rule: marker evidence leads to the `fork$` clause |
| C18 | Native runs and seeds | Async UI tests must give the same output for every seed (S2), so they also match natively. `// native: skip` is introduced by `jit-core`, which writes its testing.md section and lists each marked test (kind 1) |
| C19 | Pinned tasks | Tasks on a dedicated worker are never rescued. Dedicated workers are not placement targets. Children started by a pinned task inherit its worker. Work leaves M through `compute`, through channels to unpinned tasks, or through `Gui.post` |
| C20 | GUI handle signatures | `next_event : Window -> <IO, Async> (Event, Window)`, `begin_frame : Window -> <IO> Ui`, `end_frame : Ui -> <IO> Window` |
| C21 NU1 | C integer widths | CBLAS/LAPACKE (LP64) take C `int`, so a width must be expressible. Apple arm64 packs stack arguments by natural size [I], which only adds weight. Arrays pass a single pointer and lengths are explicit `Int` arguments. Default spelling: `Int as int` after a parameter type, with C type names (`int`, `int32_t`, `int64_t`, `size_t`); a bare `Int` is `int64_t` |
| C22 NU2 | Per-OS libraries | Default spelling: `link on linux "openblas"`, `link on macos framework "Accelerate"`, `link on windows "mkl_rt"`, plus a CLI override `--link <name>` for `run` and `build` |
| C23 | multicore.md decision 1 (scheduler as a library) | Mechanism in `eml_sched` and `eml_rt`, semantics in std. `tests/ui/run/kinds/scheduler.em` stays as a language test |
| C24 | Channel as `Data` objects | A Rust structure holding counted references in the registry (C14) |
| C25 | 「VM も並列にする理由」 reason 1 (`FnEntry` mixing) | Replaced: the VM is the reference implementation and the only checked mode |
| C26 | a1/a2 levels | One `par` with wait semantics. Cancellation is deferred. The a1–a4 labels leave the docs |
| C27 | 対象外 vs `Atomic.Counter` | Users get no memory model. The runtime offers `Chan` and `Atomic.Counter` under `IO`, and `par` children cannot reach them |
| C28 | Milestone overlaps | Each item is assigned once in §1.4 |
| C29 | Raw-array order | `raw-arrays` comes before `ffi-c`, `std-par` and `native-inline` |
| C30 | Windows Ctrl-C (review B-4) | `eml_cli/src/console.rs` (`cfg(windows)`) installs `SetConsoleCtrlHandler`. CTRL_C sets `INTERRUPT`, and a second one exits with 130. CTRL_CLOSE/SHUTDOWN set `TERMINATE`. It is tested by calling the handler function directly; no console events are sent in CI |
| C31 | Per-task resource limits (review C-1) | Stage `task-limits`, with the mechanism of §1.2(a). Inside a task, exceeding the depth or stack limit, or being picked by the memory soft limit, raises the IO failure kind `ResourceLimit` in that task, which `try_all` catches. The hard memory limit stays whole-run fatal |
| C32 NU3 | Cost of C31 | Every poll site and every non-tail call site becomes an unwind point. The VM pays nothing at run time (rows are data). Native code pays one flag check per non-tail call site that is not already a yield point. Default: accept, measured in `task-limits` (VM) and `jit-effects` (native) |
| C33 NU4 | Native track position | Default: the native track (`borrowed-params` … `native-par`) and `par-steal`/`std-par` come after `gui-plugin`. This follows the stated priority (server, then FFI, then GUI). FFI and GUI no longer need native code (review C-4) |
| C34 | Net handles on cancellation (review C-7) | `TcpListener` and `TcpStream` are `Lin` `Foreign` values whose drop glue closes the socket. A cancelled task's unwind therefore closes them; no explicit `close` is needed on the cancelled path |

Further decisions T1–T4 write, all from S2/S3 defaults:
- The `IOErrorKind` additions are `Cancelled`, `TimedOut`, `ConnectionReset` (EPIPE maps here), `AddrInUse` and `ResourceLimit`.
- The raw Async ops are visible and documented as the runtime ABI.
- Time is `Int` milliseconds. The op budget is 128.
- The I/O backend is `compio-driver`; an own driver over `polling` plus IOCP is the listed fallback in 未決.
- The idle-read rule (G-a1): a read buffer is reserved only once the socket is readable (kqueue/epoll), and IOCP uses a zero-byte receive first.
- Names: `Async.drain_within`, `Par.reduce`, `Par.tabulate_floats`/`tabulate_ints`/`for_mut_floats`/`for_mut_ints`, `Gui.proxy`/`Gui.post`/`Gui.request_redraw`, and `Main.run : (Unit -> <IO, Panic, Async> a) -> <IO, Panic, Async> a`.
- The GUI plugin keeps one event loop per process.
- G-b3 is written to multicore.md 未決 as an open question, with two options.

### 1.2 Application validation

✓ = covered by the named stage; G = gap, with its owner stage.

#### (a) Large-scale async network server

| Requirement | Status |
|---|---|
| Thousands of cheap parked tasks | ✓ `async-tasks`. A parked task holds a cell, a κ chain and its captures, with no VM stack [I]. `async-net` measures 10k idle tasks with `bench/scripts.sh`-style RSS |
| Idle connections must not pin read buffers | G-a1 → `io-driver` (readiness first; zero-byte receive on IOCP) |
| Per-connection isolation (IO failures, `Panic`) | ✓ `async-tasks` (`try_all`) |
| Per-connection isolation (depth, stack, memory) | G-a2 → `task-limits` (C31). The mechanism is decided now: (1) every poll site and every non-tail call site gets an unwind row; (2) inside a task, a depth or stack overrun at a poll raises `ResourceLimit` there; (3) `Control` gets a soft limit (default 7/8 of `max_bytes`). Each task cell counts the bytes allocated while the task runs, charged on the allocator's slow path. When the soft limit is crossed, the monitor flags the running or ready task with the largest count, and that task's next poll raises `ResourceLimit`. Remaining risk, stated in async.md: memory held by parked tasks is not attributed, and one allocation between the soft and hard limits is still fatal |
| Back-pressure | ✓ `async-chan` (bounded), `async-net` (writes finish when the kernel takes the data). Accept limit idiom: a capacity-n `Chan Unit`; `send` acquires a slot and `try_recv` releases it |
| Timeouts and deadlines | ✓ `async-time`, `async-net` (`_within`) |
| Graceful shutdown | ✓ `Signal.wait` (`async-net`), scopes, plus `Async.drain_within` (G-a3 → `async-time`) |
| Multi-core scaling | ✓ `async-threads`, `async-rescue` |
| CPU-heavy work, blocking calls | ✓ `compute` (`async-threads`); DNS on the pool (`io-driver`/`async-net`); files (`async-files`) |
| Windows | ✓ `platform-ci`, `io-driver`, `net-windows` |
| Closing handles on cancellation | ✓ C34 |
| TLS, HTTP, time of day | deferred |

```haskell
import Async
import Net
import Chan
import Signal

session : TcpStream -> <IO, Panic, Async> Unit
session conn =
  match Net.read_line_within 10000 conn with          -- request deadline; the stream comes back
    | (Some (Some line), conn) -> session (Net.write_line (answer line) conn)
    | (_, conn) -> Net.close conn                     -- on cancellation, drop glue closes conn (C34)

acceptor : Async.Scope -> Chan Unit -> TcpListener -> <IO, Panic, Async> Unit
acceptor conns slots l =
  let _ = Chan.send () slots                          -- parks while 10000 connections are live
  let (conn, l) = Net.accept l                        -- G-a1
  Async.spawn_in conns (fn () ->
    let _ = Async.try_all (fn () -> session conn)     -- IO failures, Panic, ResourceLimit stay here (G-a2)
    let _ = Chan.try_recv slots
    ())
  acceptor conns slots l                              -- when cancelled, the unwind drops l

main : Unit -> <IO, Panic> Unit
main () =
  use Async.run
  let slots = Chan.new 10000
  use conns <- Async.scope
  let _ = Async.race (fn () -> acceptor conns slots (Net.listen "0.0.0.0" 8080))
                     (fn () -> Signal.wait Terminate)
  Async.drain_within 30000 conns                      -- G-a3
```

#### (b) Data-parallel pure eml, native multi-core

| Requirement | Status |
|---|---|
| `par` and `both`, with children that may `Panic`/`Fail` | ✓ `never-rows`, `par-core` |
| Unboxed numeric arrays | ✓ `raw-arrays` |
| Real parallelism in the VM | ✓ `par-steal` |
| Combinators, determinism, grain | ✓ `std-par` |
| Raw-array range primitives | G-b1 → `std-par`. Names are monomorphic, because classes do not take `Lin` types |
| Index-range reduction | G-b2 → `std-par` (`Par.reduce`) |
| No `Float` box per element | G-b3 → decided in `std-par` by measurement. Options: a typed-result closure entry (`f$ret`, now deferred), or leaf-range primitives with first-order loops |
| ≥ 0.6 × N natively at N ≤ 4 | ✓ `native-par`, after `native-inline` |

```haskell
import Par
import FloatArray

step : Int -> FloatArray -> <Panic> FloatArray
step n g =
  Par.tabulate_floats (n * n) (fn i ->                 -- G-b1; result Float boxed per element (G-b3)
    let r = i / n
    let c = i % n
    if r == 0 || c == 0 || r == n - 1 || c == n - 1 then FloatArray.get g i
    else 0.25 * (FloatArray.get g (i - 1) + FloatArray.get g (i + 1)
               + FloatArray.get g (i - n) + FloatArray.get g (i + n)))

residual : Int -> FloatArray -> FloatArray -> <Panic> Float
residual n a b =
  Par.reduce (n * n) 0.0 (fn x y -> x + y)              -- G-b2
    (fn i -> let d = FloatArray.get a i - FloatArray.get b i in d * d)
```

#### (c) Matrix computation via FFI

| Requirement | Status |
|---|---|
| Zero-copy pointer passing; in-place results | ✓ `raw-arrays`, `ffi-c` |
| C `int` arguments; pointer-only arrays | C21 (NU1) → `ffi-c` |
| Per-OS library | C22 (NU2) → `ffi-c` |
| Calls from the VM (trampolines generated by `eml_native`'s FFI module, which `ffi-c` creates) and from native code (`jit-core`) | ✓ |
| `pure` calls from `par` children | ✓ (S3 Q4) |
| Long calls inside a server | ✓ `ffi-blocking` |
| BLAS threads vs eml workers | advice in ffi.md |
| LAPACK | through LAPACKE. Accelerate's new LAPACK symbols carry a suffix [I] |
| Safety | the declaration is trusted; wrong dimensions corrupt memory |

```haskell
link on linux "openblas"                               -- NU2 default spelling
link on macos framework "Accelerate"

pub extern "C" pure symbol "cblas_dgemm" dgemm
  : Int as int -> Int as int -> Int as int -> Int as int -> Int as int -> Int as int
    -> Float -> FloatArray -> Int as int -> FloatArray -> Int as int
    -> Float -> MutFloatArray -> Int as int -> (Unit, MutFloatArray)   -- NU1 default spelling

matmul : Int -> FloatArray -> FloatArray -> <Panic> FloatArray
matmul n a b =
  let ((), c) = dgemm 101 111 111 n n n 1.0 a n b n 0.0 (MutFloatArray.make (n * n) 0.0) n
  MutFloatArray.freeze c
```

#### (d) GUI (egui; Slint later)

| Requirement | Status |
|---|---|
| Event loop on the process main thread; modal loops keep eml running | ✓ `main-pinning` |
| GUI calls only on M; children of the GUI task stay on M | ✓ `gui-plugin`, C19 |
| Background results reach the GUI task | ✓ `Gui.post` (`gui-plugin`) |
| Handle-returning signatures | ✓ C20 |
| `Main.run` | ✓ decided signature (`main-pinning`) |
| Event-loop lifecycle | one loop per process (`gui-plugin`) |
| Redraw requests | ✓ `Gui.request_redraw` |
| Closure-free egui containers | risk (S3 A7), resolved by a spike at the start of `gui-plugin`, with S3's fallback |
| CI without a GPU | builds and headless plugin tests in CI; an interactive check per OS by hand, as a measurement rather than a test |
| Runs in the VM; natively later | ✓ (`jit-effects` runs the headless plugin tests natively) |

```haskell
import Async
import Gui
import Main

counter : Gui.Window -> Int -> <IO, Panic, Async> Unit
counter w n =
  match Gui.next_event w with
    | (Gui.Closed, w) -> Gui.close w
    | (_, w) ->
      let ui = Gui.begin_frame w
      let (clicked, ui) = Gui.button ui "+1"
      let ui = Gui.label ui "count: \{n}"
      counter (Gui.end_frame ui) (if clicked then n + 1 else n)

main : Unit -> <IO, Panic> Unit
main () =
  use Async.run
  Main.run (fn () -> counter (Gui.open "counter" 400 300) 0)
```

### 1.3 First stage `runtime-design-docs`

The implementer writes the content in Japanese, applying yomiyasu. Headings that code or `spec/` cite are kept verbatim [V: grep for `multicore.md) の「…」` and `roadmap.md) の「…」`]:
- multicore.md: 「VM の並列化の備え」 (keeping 制約 1, 2 and 5), 「確定した設計判断」, 「共有の印方式」, 「`par` の印付け」, 「`Lin` の物体の所有者の移し替え」, 「論理的な worker ID」, 「スレッドに縛られたリソース」, 「可変状態」, 「row の Kind」, 「テスト」, 「未決の論点と将来の拡張」.
- roadmap.md: 「進め方」, 「段の列」, 「その後の項目」, 「型システム」, 「言語機能と構文」, 「処理系」, 「REPL」, 「マルチコア対応」.

The design is split into four future documents, for four different crate sets and readers. Outlines (heading — content):

**docs/future/native.md (new; ネイティブ化の設計)**
- 目的と範囲 — criteria and goals, moved from roadmap 169–171.
- 確定した設計判断 — Cranelift with JIT and AOT from one code generator; whole program, compiled eagerly; no VM/native mixing; the VM is the reference and checked mode; `tail` inside, the platform convention outside; explicit `ctx`; aborts by return; own setjmp/longjmp; polls; a 1 GiB stack reserve; the cache key.
- IR の層 — moved from 190.
- 呼び出し規約と入口 — 187, 188, 191, 192; `eml_enter`, `enter$k`.
- 中断と実行時エラー — 178 and 179, rewritten; `Ctx.fault`; `eml_fatal`; par-child faults (C17); poll-site aborts for task limits (C31).
- 安全点とスタック — polls, `PREEMPT` masked, guard, probes.
- extern の入口 — 180; wrappers and VM arms generated from the table; the `faults` flag.
- 展開と最適化の前提 — 172, 173, 177.
- 生の要素の配列 — 186 and the design point of 195–198: the `Raw` kind and inline access.
- 並列の実行 — workers enter closures with their own `Ctx`; helping; the fatal protocol.
- AOT と配布 — archives, the linker, Windows; 193, 194.
- テスト — the native UI rule and `// native: skip` (owned by the stage that introduces it); `RunStats` is VM-only.
- 却下した案 — `try_call` plus an unwinder, mixing, lazy stubs, OS unwinding, CRT longjmp, a `stack_limit` global value.
- 未決の論点と将来の拡張 — S3 §6 native items.

**docs/future/ffi.md (new; FFI と拡張の設計)**
- 目的と範囲 — BLAS and LAPACK first; GUI through plugins.
- 確定した設計判断 — table.
- `extern "C"` の宣言 — attributes, `extern "C" data … drop`, `link` with an OS condition (NU2).
- 型の対応 — C widths (NU1), pointer-only arrays, `Mut*Array` with_arg, handles, results, the NUL rule (moved from 181, 182).
- 効果と中断の分類 — `<IO>` unless `pure`; `faults: false`.
- 長く走る呼び出し — `blocking` (C10), the pool rule (C11), BLAS-thread advice.
- VM からの呼び出し — trampolines from `eml_native`'s FFI module; `Op::Ffi`.
- データの配置 — 185.
- C のハンドル — 183, 184.
- 拡張 — the `Plugin` trait, plugin rows as full table rows, the load error, the trust class.
- GUI — the egui plugin on worker M, park, `next_event`, the `Ui` handle (C20), `Gui.post`, one loop per process, Slint later.
- 却下した案 — C shim, GUI inside `eml_rt`, Bigarray, views, libffi.
- 未決の論点と将来の拡張 — callbacks, structs, a C-level inbox, `lld`.

**docs/future/async.md (new; 非同期の設計)**
- 目的と範囲.
- 確定した設計判断 — table (§1.1 C1–C5, C10–C14, C34 and the further decisions).
- `Async` のエフェクト — raw ops, `Wake`, checkpoints, op budget.
- スコープとタスク — API, the scope exit protocol, `use Async.run`, nested `run`, escaped handles.
- 失敗と `exit` — `Failure`, `try_all`, `fail_io`, the new kinds, `exit`, whole-run errors; per-task limits (C31) and their remaining risk.
- 取り消し — delivery, sticky, `shield`, soft checks, disarm and the race rule, in-flight I/O, hard drop.
- 時間 — `sleep_ms`, `timeout`, `race`, deadlines, `drain_within`.
- チャネルと共有の状態 — `Chan`, marking and transit claim, the semaphore idiom, `merge`, `Atomic.Counter`, no `select`.
- I/O の driver — the triple, owned buffers, the idle-read rule, `IoDriver`, `eml_io`, `compio-driver`, binding and forwarding, inline blocking rows, the pool (C11).
- ネットワークとシグナル — `Net` (C34), `Signal`, DNS, later items.
- ファイルとパイプ — async files and pipes; the three `Proc` items.
- `compute`.
- テストと決定性 — the seeded policy, `TestDriver`, the virtual clock, the loopback smoke set on port 0, the Windows set.
- 未決の論点と将来の拡張 — S2 §6.

**docs/future/multicore.md (rewrite; マルチコア対応の設計)**
- 目的と範囲 / 目的 / 対象 / 対象外 — C27.
- 確定した設計判断 — rows 1–12 revised per S1 Q7 and C23–C27; new rows 13 (thread model), 14 (scheduler), 15 (preemption), 16 (long calls), 17 (stop protocol).
- 層の構成 — the crate order of C15 and a diagram.
- スレッドと worker (new) — one OS thread = one worker = one `Ctx`/`Heap`; N; worker 0; the host thread; the monitor; the pool; worker M.
- スケジューラ (new) — the loop; `par` deques, helping and parking; mailboxes, cells, `start_task`/`resume_task`, `run` parking; placement; fairness; rescue; pinned tasks (C19); per-task limits (C31).
- 可変状態 (keep) — the `split_at_mut` sentence becomes range primitives.
- スレッドをまたぐ RC (keep) — subsections kept (crossing list, 共有の印方式, 却下した案, `par` の印付け with transfer mode, 論理的な worker ID with C13, `Lin` の物体の所有者の移し替え with C12); new 移し戻し, 印付けの表, 別のスレッドからの解放, Rust の側が持つ語 (C14).
- 並列と並行の API (keep) — row の Kind (`Panic` is `Never`, writable `<never e>`), 子タスクに `never` の操作だけを許す理由, `par` の意味 (replaces 「`par` の段階」), データ並列での使い勝手, すぐに再開する handler の最適化, 並行処理 (short, with a link to async.md).
- 継続の移動と不変条件 (keep) — 不変条件 (amended invariants, C11, C14), `once` の継続の移動 (only rescue moves a started task), `multi` の継続の並列な再開, スレッドに縛られたリソース (worker M; link to ffi.md 「GUI」).
- 止める手順 (new) — the end slot, the mirror, cutting to entry, `exit`, fatal, signals, waiting for pool jobs, recovery.
- VM とネイティブのランタイム (keep) — VM も並列にする理由 (C25), VM の並列化の備え (制約 1–5 kept, new rows after 5), 1つの worker での実行 (replaces 「`par` の a1 と協調的マルチタスク」), ネイティブの並列 (link to native.md).
- テスト (keep).
- 未決の論点と将来の拡張 (keep) — `par_ordered`, `IO` in children, `par` cancellation, read-only sharing, `poly` lin-bit propagation, `Par.fork`/`Par.join`, a lock-free mailbox, page return, G-b3; with no a1–a4 labels.

**docs/future/roadmap.md**
- 段の列 — 29 entries in the order of §1.4. Each entry has the form ``- `slug`: content. 前提: … 。設計: [..](..) の「…」``. `borrowed-params` takes the moved sub-bullets 219–224. `async-files` takes the moved items 122–124, 128–130 and 137–139. `raw-arrays` takes 195–198.
- その後の項目 — the 処理系 「ネイティブ化」 bullet becomes one line linking native.md. `read_into` stays in 処理系 (benchmarks.md:331 cites 「処理系」 for it [V]). 206 「見直す条件」 is reworded to "`Std.Async` のタスクの継続の確保が計測で目立ったとき". 「JIT (投機的)」 becomes 前提: `native-aot`. 「ループ化」 stays, with 見直す条件 "ネイティブの展開で巻き上げの不足を測ったとき". 40 (`Async@lin`) is unchanged.
- 「### マルチコア対応」 holds only deferred items. New subsections: 「### 非同期」, 「### ネイティブ化」 (with `f$ret` moved from 189), 「### FFI と拡張」. Every item in these four subsections has both 後に回した理由 and 見直す条件.

**docs/overview.md** — line 20: Cranelift with JIT and AOT, the VM kept as reference and checked mode, a link to native.md. The コンパイラと実行系 paragraph gets one sentence on workers. The 用語 a1/a2 row is replaced by `worker`. The Perceus row's citation changes from 「処理系」 to 「段の列」. The `Panic`/`Never` rows (lines 31 and 33) are not touched here; `never-rows` changes them in its close.

**docs/README.md** — map rows for async.md, native.md and ffi.md; the multicore row; the 「文書の扱い」 line.

**docs/future/stdlib.md** — 「マルチコア対応の設計からの申し送り」 rewritten (`try_map`, `both`, links to async.md); `Async` dropped from 「モジュールの候補」.

**CLAUDE.md** — line 8 only.

**spec/ wording** (no rule change): expressions.md 「並行処理」 (`Async.run`, a `Net` example), lir.md:290, object-model.md:358 and 732–735, vm.md:391, 400 and 402.

Link rule (review B-8): a document links only to headings that exist in its own commit. The commit that creates a heading also adds the inbound links that earlier documents need. The order is native → ffi → async → multicore → roadmap → maps → spec, so every commit passes the citations test.

| Task | Commit | Files |
|---|---|---|
| T0 | `Plan runtime design docs` | `docs/plans/<date>-runtime-design-docs.md` |
| T1 | `Add the native design` | docs/future/native.md, roadmap.md (ネイティブ化 bullet reduced; 170–173, 177–180, 186–188, 190–194 moved) |
| T2 | `Add the FFI and plugin design` | docs/future/ffi.md, roadmap.md (181–185 moved), native.md (link from 「extern の入口」 to ffi.md) |
| T3 | `Add the async design` | docs/future/async.md, native.md and ffi.md (inbound links) |
| T4 | `Rewrite the multicore design` | docs/future/multicore.md; inbound links to its new headings from native.md, ffi.md and async.md |
| T5 | `Lay out the stage sequence` | docs/future/roadmap.md |
| T6 | `Point the maps at the new designs` | docs/overview.md, docs/README.md, docs/future/stdlib.md, CLAUDE.md |
| T7 | `Align spec prose with the runtime design` | docs/spec/expressions.md, lir.md, object-model.md, vm.md |
| T8 | `Close runtime design docs` | removes docs/plans/<date>-runtime-design-docs.md |

### 1.4 Stage sequence (execution order)

| # | Slug | Goal | Scope | Prerequisites |
|---|---|---|---|---|
| 1 | `platform-ci` | the existing suite on three OSes | CI workflow; Windows build; C30; `rust-toolchain.toml` pinned to 1.98.1; a listed Windows exclusion set | — |
| 2 | `never-rows` | `Panic` is `Never`; writable `<never e>` | M0 types | — |
| 3 | `par-core` | `par`/`both` on one worker | M0 runtime | `never-rows` |
| 4 | `async-tasks` | single-worker tasks | M1 + M-A0 core: mailbox, cells, `run` parking, entries, `Stop::Preempt`, stop word, monitor, implicit scopes with failure recorded (siblings run to completion here), `Task`, `await`, `try_all`, `fail_io`, `Either`, kinds, `yield_now`, per-task ids, registry, end slot, `RunStats` task fields including a shared-object count | `par-core` |
| 5 | `async-cancel` | cancellation and named scopes | cancel bit, `Wake.Cancelled`, disarm for cells and ready tasks, failure fan-out, `spawn_in`/`start_in`/`cancel_scope`, `shield`, soft checks, op budget | `async-tasks` |
| 6 | `async-time` | timers on a virtual clock | `await_time`, timer wheel, `sleep_ms`, `timeout`, `race`, `drain_within`, `TestDriver` clock, park with timeout, draft `IoDriver` | `async-cancel` |
| 7 | `async-chan` | channels at N = 1 | M-A1, C12 | `async-cancel` |
| 8 | `io-driver` | real I/O driver and pool | M-A2s spike (go/no-go in the plan), final `IoDriver`, `eml_io` real and test, pool (C11), G-a1 | `async-time`, `platform-ci` |
| 9 | `async-net` | TCP, signals, DNS on macOS/Linux | M-A2 | `io-driver`, `async-chan` |
| 10 | `net-windows` | the net set on IOCP | M-A2w | `async-net` |
| 11 | `worker-threads` | worker threads, per-worker heaps, cross-thread messages | M2 shared part: threads, heaps, remote free and drains, acceptance, N-heap recovery, SIGINT masking, registry; `par` children stay on the parent's worker | `par-core`, `platform-ci` |
| 12 | `async-threads` | Async across workers | cross-thread mailboxes, placement, `Submit` forwarding, cross-thread `Chan`, `compute`, M-A4 scaling | `worker-threads`, `async-net` |
| 13 | `async-rescue` | stuck-worker rescue | M2 rescue | `async-threads` |
| 14 | `task-limits` | per-task depth, stack and memory limits | C31, C32 | `async-threads` |
| 15 | `async-files` | async files and pipes | M-A3 | `async-net` |
| 16 | `raw-arrays` | unboxed scalar arrays | O-1/N4, `scan = 255` | — |
| 17 | `ffi-c` | user `extern "C"` in the VM | N5 VM part; creates `eml_native` with its FFI trampoline module (`cranelift-jit`); C21, C22 | `raw-arrays` |
| 18 | `ffi-blocking` | long C calls in tasks | N6 | `ffi-c`, `io-driver`, `async-cancel` |
| 19 | `main-pinning` | worker M, `Main.run` | M3 without offload; C19 | `async-threads` |
| 20 | `gui-plugin` | egui plugin in the VM | N7; C20 | `main-pinning` |
| 21 | `borrowed-params` | borrowed parameters | N0 | — |
| 22 | `jit-core` | JIT of the first-order subset | `eml_native` codegen: CLIF for scalars, data, calls, tail calls, closures/`apply`, RC; `eml_enter`/`eml_fatal` for AAPCS64 and SysV x86_64; polls; `Ctx` `#[repr(C)]` fields; native calls to user C rows; `// native: skip` and refusal handling in the runner | `borrowed-params`, `ffi-c` |
| 23 | `jit-externs` | every extern and plugin row natively | wrappers and VM arms generated from the table, `Ctx.fault`, fault cold blocks to fatal, `catch_unwind` | `jit-core` |
| 24 | `jit-effects` | the whole language natively | yields, by-return aborts, handlers, C17, Async entries over `Exec` and `IoDriver`, native `run` nesting, `par` at one worker per parent, fatal protocol, poll-site aborts (C31) | `jit-externs`, `task-limits`, `async-rescue`, `async-files`, `gui-plugin` |
| 25 | `native-aot` | `eml build` on three OSes | N2, Win64 stub | `jit-effects`, `platform-ci` |
| 26 | `native-inline` | inline byte and element access | N3 | `native-aot`, `raw-arrays` |
| 27 | `par-steal` | `par` children on other workers in the VM | M2 deque, stealing, cross-thread join parking | `worker-threads` |
| 28 | `std-par` | data-parallel combinators | M0 std part + M2b-VM; G-b1, G-b2, G-b3 | `par-steal`, `raw-arrays` |
| 29 | `native-par` | native fork/join with helping | M2b-native | `native-inline`, `std-par`, `jit-effects` |

For every stage, "Tests" are `cargo nextest run` items: each runs in ≤ 1 s and its outcome does not depend on machine speed. Real-socket tests bind port 0 and read `local_port`, use no deadlines (deadlines run only on the virtual clock), and use one connection at a time or order-independent output. "Measurements" are `#[ignore]` tests or `bench/*.sh` rows recorded in benchmarks.md at close. Every stage that touches `eml_rt` passes Verus, `verusfmt` and both Miri settings.

1. `platform-ci`
   - Tests: the workflow runs `cargo nextest run` and `cargo clippy --all-targets -- -D warnings` on `macos-latest`, `ubuntu-latest` and `windows-latest`. Verus and Miri run on Linux through nix. Windows exclusions are listed one by one in the plan (kind 1) and marked `// platform: unix`. A Rust test calls the console handler function directly and checks the `Control` bits (C30).
   - Done: one workflow run passes on all three runners.
   - Risks: `Proc`/`Cmd` on Windows, CRLF and `\` in snapshots, terminal code.
   - Spec: vm.md 「上限と割り込み」 (Windows paragraph); testing.md (CI, the platform marker).
2. `never-rows`
   - Tests: Kind export snapshots for `Panic` (kind 2); a `<never e>` signature accepts a `Panic` body and rejects an `IO` body with the new E3xxx (check-fail UI).
   - Spec: types.md, effects.md 「組み込みの `Panic`」, declarations.md, diagnostics.md; overview.md rows (close step 6).
   - Risks: interaction with carry-over (`σ ≤ Never`).
3. `par-core`
   - Tests: `tests/ui/run/par/`; every run/run-fail UI test identical under `--sched sequential` and two seeds; `RunStats` pinned under sequential; negative tests (no fork marking gives `WrongOwner`; no join walk gives `TransferLeak`); poly `Lin` at rc == -1; runtime-error precedence.
   - Risks: transfer-walk soundness; `fork$` code size.
   - Spec: core-ir.md, vm.md, object-model.md 「`share`」 and the new 移し戻し section, 「論理的な worker ID」; stdlib.md `Std.Par` (`par`, `both`); testing.md.
4. `async-tasks`
   - Tests: `scheduler.em`-style programs on `std/Async`; `tests/ui/run/async/` identical for two seeds; `exit` in a child; nested `run`; a CPU-bound task vs a ping-pong pair under a test tick; an interrupt seen within one tick; leak check sees mailbox words.
   - Measurements: `bench/rt.sh` `poll` within noise; a new bench program spawning 10k tasks with pinned `RunStats` (shared objects per spawn = objects captured).
   - Spec: stdlib.md `Std.Async` (core), Prelude, vm.md (scheduler, preemption), object-model.md (`Ctx`, the control end slot), effects.md, expressions.md 「並行処理」.
5. `async-cancel`
   - Tests (S2 Q9, no timers): a failing child cancels its siblings and a body parked on a cell; `try_all` isolation; sticky cancellation; `shield`; the op budget switches tasks after 128 operations. Kind 2: tests from `async-tasks` whose siblings now get cancelled.
   - Spec: stdlib.md (cancellation, named scopes), vm.md (cancel delivery).
6. `async-time`
   - Tests: `timeout`, `race`, `sleep_ms` and `drain_within` on the virtual clock; a nested scope under `timeout` leaves no orphan.
   - Measurement: a real-clock smoke test.
   - Spec: stdlib.md (time), object-model.md (draft `IoDriver`).
7. `async-chan`
   - Tests: ping-pong, pipeline and fan-in; a `Lin` payload finalized by the receiver in checked mode (C12); `close` wakes parked sides; a cancelled rendezvous leaves no item.
   - Spec: stdlib.md `Std.Chan`, `Std.Atomic`; object-model.md (channel, registry, transit id).
8. `io-driver`
   - Tests: Rust integration tests in `eml_io` on all three CI runners: owned-buffer read, write and accept; cancelling an in-flight read returns its buffer; a cross-thread wake; timers; pool completion; the C11 case table.
   - Measurement: RSS of 10k idle loopback connections.
   - Spec: object-model.md (`IoDriver`, pending table, trust row).
   - Risks: compio's standalone use (S2 A1).
9. `async-net`
   - Tests: an echo test over virtual streams (1000 sequential connections); a failing connection without `try_all` stops an idle server; `read_chunk_within` keeps its stream; drain under a virtual `SIGTERM`; the loopback smoke set; EPIPE maps to `ConnectionReset`.
   - Measurement: 100 concurrent connections.
   - Spec: stdlib.md `Std.Net`, `Std.Signal`; vm.md (signal routing).
10. `net-windows`
    - Tests: the same set on `windows-latest`.
    - Spec: stdlib.md (Windows notes).
11. `worker-threads`
    - Tests: snapshots at N = 1 unchanged; `par/` and `async/` order-independent tests at N ∈ {2, 8} (with `par` children kept local); Miri (remote push and drain, rc flip, mailbox hand-off); TSan; recovery with N heaps.
    - Spec: object-model.md 「確保器」, 「受け入れる制約」, 「回復」; vm.md (workers); architecture.md (`eml_sched`, close).
12. `async-threads`
    - Tests: order-independent async and net tests at N ∈ {2, 8}; `compute` parks its task.
    - Measurement: echo scales monotonically at N ∈ {1, 2, 4}.
    - Spec: vm.md (multi-worker scheduler), stdlib.md `compute`.
13. `async-rescue`
    - Tests: a stuck worker rescued within 3 test ticks (with a pending `Place` and `Wake`); a rescuer without marking gives `WrongOwner`; leak and owner checks under `--sched rescue-always`.
    - Spec: vm.md (rescue).
14. `task-limits`
    - Tests: a depth bomb in one connection (virtual streams) fails that connection with `ResourceLimit` while the server keeps serving; the same for an allocation bomb under a test soft limit; programs without tasks keep their `RunStats` (kind 3); `bench.rs` snapshots byte-identical.
    - Spec: lir.md (unwind points), vm.md 「上限と割り込み」, stdlib.md (`ResourceLimit`), object-model.md (soft limit).
    - Risks: unwind-row table size; how well the heaviest-allocator heuristic picks the culprit.
15. `async-files`
    - Tests: `.|`, a string as stdin for `run`/`output`, `Child` stderr on the test driver and on loopback pipes; a file read does not stall a ping-pong pair.
    - Spec: stdlib.md `Std.Proc` and the file sections.
16. `raw-arrays`
    - Tests: the `FloatArray`/`IntArray` API; Verus `arrays.rs` with `scan = 255`; spectral-norm `boxes` = 0 in `bench.rs` (kind 2).
    - Spec: object-model.md (kinds, 「配列」), stdlib.md.
17. `ffi-c`
    - Tests: UI tests with a `cc`-built fixture library (scalars with `int` widths, pointer arrays, with_arg, handles with `drop`, a missing symbol as a load error, `link` conditions).
    - Measurement: `dgemm` through OpenBLAS on Linux and Accelerate on macOS. E1033 is narrowed.
    - Spec: declarations.md, stdlib.md 「extern の境界」, diagnostics.md, object-model.md (`CHandle`).
18. `ffi-blocking`
    - Tests: a `blocking` sleep in one task does not stall others (virtual clock plus a fixture); `blocking` without `Async` is rejected.
    - Spec: declarations.md, stdlib.md.
19. `main-pinning`
    - Tests: the thread identity inside `Main.run`; children stay on M; an interrupt reaches a parked M.
    - Spec: stdlib.md `Std.Main`; vm.md (dedicated worker).
20. `gui-plugin`
    - Tests: headless plugin tests (frame build, nesting errors, `post`); a plugin-less build reports the load error.
    - Measurement: the counter app by hand on each OS. The spike result (S3 A7, A8) goes in the plan.
    - Spec: stdlib.md `Std.Gui`; object-model.md (plugin trait, trust row).
21. `borrowed-params`
    - Tests: `bench.rs` snapshots without the per-byte `dup` (kind 2); the verifier checks borrowed block parameters.
    - Spec: lir.md.
22. `jit-core`
    - Tests: `eml run --native` over the UI suite. Each test either matches the VM's output byte for byte or is refused with E0004. The refused count is pinned in a snapshot, and `// native: skip` tests are listed in the plan (kind 1).
    - Measurement: compile latency.
    - Spec: native ABI, entry and polls (a new spec/native.md); object-model.md `Ctx` fields; testing.md (native rule).
23. `jit-externs`
    - Tests: the refused count drops to the effect constructs only.
    - Spec: native extern entry; object-model.md 「extern」 (`Ctx.fault`).
24. `jit-effects`
    - Tests (B-1): the whole UI suite natively at the CI's worker settings. This covers `--workers 1` for all tests and N ∈ {2, 8} for the order-independent async, chan and net set, since native Async on several workers needs only per-worker entries. Before `par-steal`, `par` children run on their parent's worker in both backends, so the `par/` tests mean the same thing at every N. The refused count is 0, and only the listed `// native: skip` tests are skipped.
    - Measurement: `wc` natively not slower than the VM.
    - Spec: native aborts, faults and Async entries; vm.md 「上限と割り込み」 (native paragraph).
25. `native-aot`
    - Tests: the four scripts build and pass on the three runners; Windows run-fail tests exercise `eml_fatal`.
    - Spec: native AOT; architecture.md (CLI, at close).
26. `native-inline`
    - Measurements: the four scripts ≤ 2x Rust; scripts and numeric benches ≤ 1.5x Python.
    - Spec: native inlining; lir.md if R2 relaxes.
27. `par-steal`
    - Tests: `par/` at N ∈ {2, 8} equal to N = 1, float folds included; Miri for the deque; TSan; a deep unbalanced tree at N = 2 terminates.
    - Spec: vm.md (`par` across workers).
28. `std-par`
    - Tests: determinism across grains and N; `try_map` returns the leftmost `Err` under a right-first schedule; raw variants.
    - Measurement: G-b3 decided on a pinned `RunStats` row; VM speedup.
    - Spec: stdlib.md `Std.Par`; object-model.md (range primitives).
29. `native-par`
    - Tests: `par/` natively at N ∈ {2, 8}.
    - Measurement: a compute-bound kernel ≥ 0.6 × N at N ≤ 4.
    - Spec: native parallel execution.

### 1.5 Deferred items (each with 後に回した理由 and 見直す条件)

- **マルチコア対応**
  - `par` cancellation (deterministic wait first / a left failure with a long right child, or `timeout` around `compute`)
  - `par_ordered` (needs a stop-before-perform primitive / non-`never` children with sequential meaning)
  - `IO` in children (aborts would cross tasks / output from children beyond `both`)
  - `Par.fork`/`Par.join` (`par` plus combinators suffice / irregular parallelism)
  - private deque (it would break pinned `RunStats` / marking dominates profiles)
  - lock-free mailbox and channel (a mutex checks under Miri / contention at N ≥ 8)
  - shortest-queue placement (round-robin meets the echo criterion / measured imbalance)
  - page return (RSS = sum of peaks / a server's RSS grows)
  - attributing retained memory to parked tasks (counting allocations is cheap / the soft limit picks the wrong task)
  - `poly` lin-bit propagation (unchanged)
- **非同期**
  - multi-channel `select` (`race`, `timeout` and `merge` / a loop on ≥ 2 channels where `race` is costly)
  - broadcast/watch (pub/sub need)
  - `Atomic` beyond `Counter` (no memory model / an actor is too slow)
  - UDP (first program)
  - `SO_REUSEPORT` (accept is not the bottleneck at N ≤ 4 / it is at N ≥ 8)
  - TLS plugin (plugin infrastructure comes with the GUI plugin / an HTTPS need)
  - `Http` (after TLS or a plain-HTTP program)
  - `Time` (`Int` ms suffices / timestamps or rate limits)
  - returning the `write_bytes` buffer (profiles)
  - MultiError (debugging need)
  - async DNS (pool saturation)
  - `spawn_here` (affinity beyond `Main.run`)
- **ネイティブ化**
  - `f$ret` (moved; reopen on the G-b3 measurement)
  - bounds-check elimination (checks dominate)
  - native code cache (compile latency missed)
  - Windows ARM64 (a user target)
  - bundling `lld` (no VS Build Tools)
- **FFI と拡張**
  - synchronous C callbacks (first library needing one)
  - C structs and variadics (a library needs them)
  - C-level inbox (a C library with its own threads)
  - Slint (after egui, on demand)
  - 32-bit and byte raw arrays (`float` buffers)
  - splittable PRNG (Monte-Carlo `par`)

## 2. Rationale

- Each mechanism follows the owner the briefing assigned, with the later accepted text winning (the addendum over r3).
- The user settled the order and the crate placement (U1–U4).
- C11 removes the one dependency inversion (DNS needs the pool at N = 1) and shrinks the unchecked surface, as the case table in §5.2 shows.
- C12–C14 come from tracing S2's channel and `await` through S1's checked-mode rules at N = 1.
- C31 gives the top-priority server per-connection isolation for limits too. The mechanism reuses existing parts: unwind rows (static data in the VM), the abort path, `ResourceLimit` as an IO failure, and the monitor. No new semantics are needed.
- Splitting `worker-threads` from `par-steal` keeps work stealing off the server path (review B-2).
- FFI and GUI depend only on the VM once `ffi-c` owns the trampoline generator (review C-4). With U2, this puts FFI and then GUI right after the server, and the native track after them (NU4).
- Stages are split where a plan can name its tests: M1/M-A0 into tasks, cancellation and time; N1 into core, externs and effects; M2 into worker threads, async threads, rescue and stealing.

## 3. Alternatives considered and rejected

- Async before `par`: rejected by the user (U1).
- `par-threads` as one stage: it would put stealing on the server path.
- The native track before FFI and GUI: it contradicts the stated priority now that FFI and GUI need no native code. It stays the NU4 alternative.
- `ffi-c` after `jit-core` (S3): it would delay (c) by the whole native track only for the trampoline location.
- Per-task limits deferred: a remote DoS for the top-priority application (review C-1).
- Hard-killing an over-limit task: a scope failure would cancel every connection, and it needs owned-slot tables at safe points.
- Owned-slot tables instead of unwind rows: a second unwinding mechanism.
- Writing all four documents in one commit: hard to review; the link rule makes four commits work.

## 4. Assumptions

- A1 [V] Running `cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` on 4a0fbdf6 prints `Summary […] 4 tests run: 4 passed, 173 skipped`.
- A2 [V] There is no `.github/workflows`; origin is GitHub; there is no `rust-toolchain` file.
- A3 [V] Baselines on 4a0fbdf6: the label grep L of §5.1 yields exactly 1 line (object-model.md:952, legitimate); the slug and date greps yield 0; the `par`-label grep P yields multicore.md 17, roadmap.md 4, overview.md 1, stdlib.md 1, lir.md 1, object-model.md 3 and vm.md 2.
- A4 [I] Rust ignores SIGPIPE in binaries, so writes fail with EPIPE.
- A5 [I] Apple arm64 packs stack arguments by natural size. This only supports C21; it is not its basis.
- A6 [I] Adding unwind rows changes no VM run-time work (only `eml_lir`'s unwind lists and the tables).

## 5. Implications for implementation (first stage)

### 5.1 Commands (exact, each valid at the point it is listed)

Define once in the shell:
```sh
SLUGS='runtime-design-docs|platform-ci|never-rows|par-core|async-tasks|async-cancel|async-time|async-chan|io-driver|async-net|net-windows|worker-threads|async-threads|async-rescue|task-limits|async-files|raw-arrays|ffi-c|ffi-blocking|main-pinning|gui-plugin|borrowed-params|jit-core|jit-externs|jit-effects|native-aot|native-inline|par-steal|std-par|native-par'
L='\bC[0-9]{1,2}\b|\bG-[a-d][0-9]\b|\bNU[0-9]\b|\bN[0-7]\b|M-A[0-9]|\bM[0-3]b?\b|\bO-1\b|REQ-S1|R-S1|\bS[1-4]\b'
P='(の|は|で|、|: ) ?a[1-4]\b|a2-(wait|cancel)|\(a[34]\)|a1 →|a1 /'
```
Before T0, check that `grep -rnE "$L" docs/future docs/overview.md docs/README.md docs/spec CLAUDE.md | wc -l` prints 1 (object-model.md:952 "C1 の制御文字") [V on 4a0fbdf6]; that number is `Lbase`. Task labels T0-T8 are not in L because the trust list T0-T13 uses the same form [V]; never write a task label into docs.

Run after every task Tk (k = 0…8):
- V1 `cargo nextest run -p eml_cli --test integration -E 'test(citations::)' 2>&1 | grep -c '4 tests run: 4 passed'` → `1`
- V2 `grep -rnwE "$SLUGS" docs CLAUDE.md | grep -v '^docs/future/roadmap.md:' | grep -v '^docs/plans/' | wc -l` → `0`
- V3 `grep -rnE "$L" docs/future docs/overview.md docs/README.md docs/spec CLAUDE.md | wc -l` → `1`
- V4 `grep -rnE '20[0-9]{2}-[0-9]{2}-[0-9]{2}' docs/future docs/overview.md docs/README.md CLAUDE.md | wc -l` → `0`
- V5 `grep -rn 'plans/' docs/future docs/spec docs/overview.md crates std | wc -l` → `2` (roadmap 「進め方」 lines 9 and 22)
- V6 `git show --name-only --format= HEAD | sort` → exactly the task's files

Run with `grep -cE "$P" FILE` → the expected count per task:

| After | multicore | roadmap | overview | future/stdlib | spec/lir | spec/object-model | spec/vm |
|---|---|---|---|---|---|---|---|
| T0–T3 | 17 | 4 | 1 | 1 | 1 | 3 | 2 |
| T4 | 0 | 4 | 1 | 1 | 1 | 3 | 2 |
| T5 | 0 | 0 | 1 | 1 | 1 | 3 | 2 |
| T6 | 0 | 0 | 0 | 0 | 1 | 3 | 2 |
| T7, T8 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

### 5.2 Per task

**T0**
- Content: the reviewed plan.
- V6 → `docs/plans/<date>-runtime-design-docs.md`.

**T1** — native.md
- Moved bullets, checked by the code tokens they carry. The roadmap count must be 0 and the native.md count ≥ 1: `grep -cF 'jump threading'`, `'RetN'`, `'MAX_STRUCT_WORDS'`, `'catch_unwind'`, `'scan = 255'`. Exact expected values:
  - roadmap.md: each `0`.
  - native.md: each `≥ 1`.
  - `grep -cF 'FnEntry' docs/future/roadmap.md` → `0`.
  - `grep -cF 'repr(C)' docs/future/roadmap.md` → `1` (185 waits for T2).
  - `grep -cF '$uniform' docs/future/roadmap.md` → `1` (223).
  - `grep -cF 'read_into' docs/future/roadmap.md` → `1`.
  - `grep -cF '](native.md)' docs/future/roadmap.md` → `1`.
- native.md links only to `spec/`, implementation docs and roadmap.md headings.
- V6 → `docs/future/native.md`, `docs/future/roadmap.md`.

**T2** — ffi.md
- `grep -cF` in roadmap.md → `0` for each of `'int64_t'`, `'Bigarray'`, `'CHandle'`, `'repr(C)'`; in ffi.md → `≥ 1` for each.
- `grep -cF '](ffi.md' docs/future/native.md` → `≥ 1`.
- V6 → `docs/future/ffi.md`, `docs/future/native.md`, `docs/future/roadmap.md`.

**T3** — async.md
- It links only to kept multicore headings, native.md, ffi.md and spec.
- `grep -cF '](async.md' docs/future/ffi.md` → `≥ 1` (from 「長く走る呼び出し」).
- `grep -cF '](async.md' docs/future/native.md` → `≥ 1` (from 「並列の実行」).
- V6 → `docs/future/async.md`, `docs/future/ffi.md`, `docs/future/native.md`.

**T4** — multicore.md
- `grep -cE '^## (スレッドと worker|スケジューラ|止める手順)$' docs/future/multicore.md` → `3`.
- `grep -c '^### 移し戻し$' docs/future/multicore.md` → `1`.
- `grep -cF 'FnEntry' docs/future/multicore.md` → `0`.
- `grep -cF '](multicore.md) の「スケジューラ」' docs/future/async.md` → `≥ 1`.
- P column per the table.
- V6 → `docs/future/multicore.md`, plus async.md, ffi.md and native.md when they gain inbound links.

**T5** — roadmap.md
- Entry count: `awk '/^## 段の列/{on=1;next} /^## /{on=0} on && /^- `[a-z-]+`:/' docs/future/roadmap.md | wc -l` → `29`.
- Headings: `grep -cE '^## (進め方|段の列|その後の項目)$|^### (型システム|言語機能と構文|診断|処理系|REPL|マルチコア対応|非同期|ネイティブ化|FFI と拡張)$' docs/future/roadmap.md` → `12`.
- Pairing check:
  ```sh
  awk 'function chk(){ if(item!="" && !(r&&c)) print item; item=""; r=c=0 }
       /^#/{chk(); on=($0 ~ /^### (マルチコア対応|非同期|ネイティブ化|FFI と拡張)$/); next}
       on && /^- /{chk(); item=$0}
       on && /後に回した理由/{r=1}
       on && /見直す条件/{c=1}
       END{chk()}' docs/future/roadmap.md | wc -l
  ```
  → `0`.
- `grep -c 'Async` が入ったとき' docs/future/roadmap.md` → `0`.
- `grep -cF '`Async` を作るとき' docs/future/roadmap.md` → `0`.
- V6 → `docs/future/roadmap.md`.

**T6**
- `grep -cF 'Cranelift か LLVM' docs/overview.md` → `0`.
- `grep -cE '\]\(future/(async|native|ffi)\.md\)' docs/README.md` → `3`.
- `grep -cF 'roadmap.md) の「段の列」' docs/overview.md` → `1`.
- `git show HEAD -- CLAUDE.md | grep -cE '^[-+][^-+]'` → `2`.
- V6 → `CLAUDE.md`, `docs/README.md`, `docs/future/stdlib.md`, `docs/overview.md`.

**T7**
- `grep -cF 'run_scheduler' docs/spec/expressions.md` → `0`.
- `grep -rcF 'roadmap.md) の「マルチコア対応」' docs/spec | awk -F: '{s+=$2} END{print s}'` → `0`.
- P column per the table.
- V6 → `docs/spec/expressions.md`, `docs/spec/lir.md`, `docs/spec/object-model.md`, `docs/spec/vm.md`.

**T8**
- `test -e docs/plans && ls docs/plans | wc -l` → `0`, or the directory is absent.
- `cargo nextest run 2>&1 | grep -cE '[1-9][0-9]* failed'` → `0`.
- Close message: what moved where; no test changes; no measurement (no crate, std or bench file changed); the decision date for the deferrals and NU1–NU4.
- Close steps: 1 → T7; 2 skipped; 3 and 4 unchanged; 5 the docs stage is not in 段の列; 6 and 7 → T6; 8 → T8.

Every task: the Japanese text went through yomiyasu (process step, noted in the commit message).

**C11 case table** (written into multicore.md 「印付けの表」 and async.md 「I/O の driver」):

| Case | What holds the words | Rule |
|---|---|---|
| Normal completion | the job record in the registry | The completion goes to `cell.owner`; `take_*` moves the words back and the record is freed |
| Task cancelled while the job runs | the record | The job cannot be interrupted. The task resumes with `Cancelled` only after the completion (addendum); the unwind drops the returned words |
| Task rescued while waiting | the record, reachable from `Waiting { reason: Pool(job) }` | Under the victim's lock, the rescuer handles the record's words as it handles κ: `Unr` words are marked with the victim as sender, `Lin` words get transfer mode. Only headers change. The pool touches only payloads, so nothing races |
| `exit` or fatal error | the record | The host thread waits for in-flight jobs before dropping heaps. A second Ctrl-C still `_exit`s with 130 |
| REPL recovery | the record | Recovery waits for in-flight jobs, like the I/O driver's drain, then copies |
| The owner mutates an input in place | — | It cannot: the record's reference makes rc ≥ 2, so `is_unique` fails and the write copies |
| Checked mode | — | Pool payload writes have no owner check, the same as inline C calls writing `Mut*Array` (the declaration is trusted) |

## 6. Unresolved (needs user; the defaults are what T1–T5 write)

- NU1 C width spelling — default: `Int as int`.
- NU2 per-OS `link` spelling — default: `link on <os> …` plus `--link`.
- NU3 the cost of per-task limits — default: accept unwind rows at every poll and non-tail call site, measured in `task-limits` and `jit-effects`; the hard memory limit stays fatal.
- NU4 the position of the native track — default: after `gui-plugin`.

Stage plans settle the open spellings of new E-codes; they need no user decision.

## 7. Additional checks

- Read: S4-review-r1.md; all r1 inputs; roadmap.md lines 122–139, 169–226 and 40 [V]; citations of roadmap.md 「処理系」 (benchmarks.md:331 for `read_into`, overview.md:101 for the Perceus row) [V].
- Ran the citations tests (4 passed), the baseline greps L, P, slugs, dates and plans, and token counts in the roadmap.
- Not run: Verus, Miri, anything on Windows.

## 8. Response to the review

| Finding | Verdict | Resolution |
|---|---|---|
| A-1 wrong U cross-references | accepted | U items renumbered. Only NU1–NU4 remain, cross-referenced from C21, C22, C32 and C33 |
| A-2 "Decided" rows with open items | accepted | Every C row is decided (C11 with a case table, C13, C15 by U3, C19, C10) or marked NU with the default the docs write; §1.1 lists the further decisions T1–T4 write. No provisional markers |
| A-3 Apple ABI as the basis of C21 | accepted | C21 rests on CBLAS/LAPACKE's 32-bit `int`; the Apple rule is only supporting [I] |
| B-1 `jit-effects` criterion vs net/chan/N>1 | accepted | Prerequisites now include `task-limits`, `async-rescue`, `async-files` and `gui-plugin` (and through them `io-driver`, `async-net`, `async-chan`). The criterion covers the full suite at the CI's worker settings: N = 1 for all tests, N ∈ {2, 8} for the order-independent async/chan/net set. `par` children stay on the parent's worker until `par-steal`, so `par/` means the same at every N. The refused count is 0; skips are only the listed `// native: skip` tests (introduced and listed by `jit-core`) |
| B-2 `par` machinery ahead of the server | partially accepted | Order settled by U1. Accepted the split: `worker-threads` (threads, heaps, remote free, messages) is on the server path; `par-steal` moves after the native track |
| B-3 oversized stages | accepted | Split now: `async-tasks` / `async-cancel` / `async-time`; `jit-core` / `jit-externs` / `jit-effects`; `worker-threads` / `async-threads` / `async-rescue` / `par-steal`. `platform-ci` is capped at "the existing suite passes on three runners with a listed exclusion set" |
| B-4 Ctrl-C test on Windows CI | accepted | C30: the handler in `eml_cli/src/console.rs`, tested by calling it directly; spec owner vm.md 「上限と割り込み」 in `platform-ci` |
| B-5 label grep scope and missing labels | accepted | Grep L covers C*, G-*, NU*, N* and the milestone codes, with an exact baseline (1); task labels T* are excluded because the trust list uses T0-T13. P counts per file per task. Labels are declared implementer-only |
| B-6 Cranelift count | accepted | Replaced by per-bullet code tokens with exact expected counts at T1 and T2 |
| B-7 deferred-item pairing; `Async` 見直す条件 | accepted | awk pairing check over the four subsections → 0. Items 122–139 move into `async-files`; 206 is reworded; 40 is unchanged (checked by grep) |
| B-8 link order | accepted | Order native → ffi → async → multicore with the link rule; inbound links are added by the commit that creates the heading; V1 passes after every commit |
| B-9 timing and speed criteria | accepted | Each stage separates tests (≤ 1 s, speed-independent) from measurements (`#[ignore]`/bench). Real sockets use port 0, no deadlines and order-independent output |
| B-10 overview `Panic` rows | accepted | Left to `never-rows`' close (listed in its spec line); T6 states it does not touch them |
| C-1 per-connection limits as a remote DoS | accepted | Stage `task-limits` (C31) with a decided mechanism. Remaining risk stated; cost acceptance is NU3 |
| C-2 C11 cases | accepted | Case table in §5.2 (cancel, rescue, `exit`/fatal, recovery, in-place mutation, checked mode). The run waits for in-flight jobs before dropping heaps |
| C-3 cost of per-task ids and marking | partially accepted | Per-task ids are checked mode only (no fast-mode cost). Fast-mode marking at spawn/send is S1's settled rule, bounded by the not-yet-shared captured or payload graph; `async-tasks` adds a shared-object count to `RunStats` and a pinned 10k-spawn bench. The transit claim is specified exactly in C12 |
| C-4 GUI/FFI depending on native | accepted | `ffi-c` creates `eml_native`'s FFI trampoline module (`cranelift-jit` only); `gui-plugin` needs only `main-pinning`; native runs of plugin tests happen in `jit-effects`. Position of the native track: NU4 |
| C-5 crate layering | accepted | C15 gives the dependency order and the executor trait; `Session` stays the only pipeline driver; architecture.md and the CLAUDE.md diagram are updated by each stage that adds a crate |
| C-6 `// native: skip` owner | accepted | `jit-core` introduces the marker, writes its testing.md section and lists the marked tests (kind 1) |
| C-7 resources in the server sketch | accepted | C34: `Net` handles close through drop glue, so cancelled tasks close them; comments in the sketch |
