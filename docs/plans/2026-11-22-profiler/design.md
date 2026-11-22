# The profiler: design shared by `ir-origins` and `native-prof`

Base: main @ 0f04d6a2. Tags: [V: how] verified when the design was written, [I] inferred. This is the third revision of the design: Appendix A answers the adversarial review of the first, Appendix B the soundness review of the sampler in the second. The prototype is `prototype.patch` in this directory (390 lines against 0f04d6a2, macOS arm64; it is evidence, not code to apply). The stages planned from this design are `docs/plans/2026-11-22-ir-origins.md` and `docs/plans/2026-11-22-native-prof.md`; this directory is deleted at the Close of `native-prof`, the last of them. The user decisions are recorded at the end and in `docs/plans/2026-11-17-runtime/00-decisions.md` §D D-20.

## 0. Summary of the decision

1. The profiler is three recordings that share one attribution model and one report tool:
   - a time profile: a sampler in the runtime, on unchanged fast-mode native code (`eml run --prof FILE`);
   - a count profile: deterministic LIR segment counts, in checked mode on one worker, beside `RunStats` (`eml run --prof-counts FILE`);
   - a heap profile: heap-growth samples taken in the checked-mode entries (`eml run --prof-heap FILE`).
2. Attribution is by source function, not by machine symbol. This needs a compiler change: every Core IR and LIR statement carries an origin `At { loc, scope }`, every function carries an inline tree (`scopes`), and the program carries a table of source functions (`sources`). Native code gets, on request, a per-function table "code offset -> point", where a point is `(At, role, callee)`.
3. This reverses D-MT-1 / A-37 ("positions on LIR calls only") of `docs/plans/2026-11-17-runtime/00-decisions.md` (D-1). Origins do not change machine code: verified on ten `bench/` programs (F-17).
4. Sampling on Linux and macOS is a directed `SIGPROF` whose handler walks its own thread's frame chain to the root. Windows `--prof` is a later item with its own design and soundness review. Generated code is the same with the profiler on or off.
5. New stages: `ir-origins` (right after `native-checked`), `native-prof` (right after `jit-tasks`), then the later items `prof-counts`, `prof-tasks`, `prof-heap`, `prof-windows`, `native-backtrace` (after `vm-retire`) and `prof-aot` (after `native-aot`).
6. `native-checked` needs no change of decision. Section 8.1 lists two small additions and one ordering rule.
7. The VM gets none of it beyond the mechanical adaptation to the new statement type.
8. Section 6 is written to be read alone. A strong advisor reviewed the sampler's soundness on r2 (CLAUDE.md's rule on `unsafe` boundaries); its verdict is that the unix mechanism is sound enough to put to the user once its findings are folded in, and Appendix B folds them in. The mechanism adds three trust rows to the runtime closure (6.7 item 7), which is part of the user's decision D-3.

## 1. Facts the design stands on

| # | Fact | Tag |
|---|---|---|
| F-1 | Core IR and LIR are block lists of statements; positions exist only as `at: Option<Loc>` on `Rhs::Extern` and `Call::Par` | [V: `crates/eml_lir/src/lib.rs` L505-510, L719-724, L1136; docs/spec/core-ir.md 「位置」] |
| F-2 | optimize runs `direct::lifted`, `specialize`, `handlers`, `fuse`, `direct::type_direct_only`, `retain_reachable`, with `recompute_yields` between. Inlining takes non-recursive functions of at most 40 statements; copies are keyed by known closures (`"Std.List.map@[Int, Int]{0=main$lambda0}"`); names over 64 characters are hashed; every decision depends only on names and code | [V: `crates/eml_core_ir/src/optimize/mod.rs` L21-37; docs/spec/core-ir.md 「鍵と名前」「インライン化」] |
| F-3 | Three places copy code across functions inside a block: fuse's inlining, handler specialisation's clause expansion (`handlers/piece.rs` `inline_clause`), and the wrapper unwrapping of `known`. Blocks with one predecessor are merged. So a block does not stay inside one source function | [V: `optimize/handlers/mod.rs` L1-14, `handlers/piece.rs` header, `optimize/known.rs` header; docs/spec/core-ir.md 「インライン化」] |
| F-4 | All generated code is compiled by one ISA constructor with `preserve_frame_pointers` on; the three entry stubs build a standard frame record | [V: `crates/eml_native/src/abi.rs` L19-33; `crates/eml_nrt/src/arch/aarch64.rs` L52-90; docs/spec/native.md L79, L83] |
| F-5 | On macOS arm64 a `SIGPROF` handler that walks the frame chain from the interrupted `fp` reaches `eml_enter` and then the thread's start, through generated code, tail-called functions, the thunk and Rust runtime entries. No chain was broken in any run (`broken=0` over more than 20,000 samples on `fib`, `rbtree`, `list`) | [V: prototype] |
| F-6 | Overhead with a baseline: `fib 43` (dev build) takes 1,981 and 1,982 ms unprofiled and 1,984 and 2,000 ms with 1,317 and 1,327 samples (a nominal 1 kHz gives about 664 Hz with a sleeping timer thread): 0.15 % and 0.9 %. The walk itself is about 0.1 us at 38 records; the cost is the signal delivery | [V: prototype, two runs each] |
| F-7 | The machine names hide the user's code: the `fold_state` loop appears as `f:count_to$let$go{H=Std.Yield.fold@#4e8feccd4de420b7}`, and the user's lambda of `list` appears only as `f:map@[Int, Int]{0=rounds$lambda0}` | [V: prototype output] |
| F-8 | Non-tail recursion is deep: about three quarters of the samples of `list` (a 100,000-element non-tail `range` / `map`) have more than 64 frames (75 of 95; 332 of 419) | [V: prototype, `depth_full`] |
| F-9 | Table size, with one source location per LIR statement and one per entry call. Rows per program: nbody 1,972 for 37,804 code bytes; rbtree 800 for 14,224; fannkuch_var 1,089 for 21,064. Encoded as two varints per row (offset delta, point delta) the rows take 4,049, 1,823 and 2,181 bytes: 10.7 %, 12.8 % and 10.4 % of the code bytes. Distinct points: 798, 414, 464 | [V: prototype counters] |
| F-10 | Setting source locations does not change load time measurably at 45 functions (`run --native --load-only bench/nbody.em`: 0.02..0.04 s user with and without) | [V: prototype, three runs each; too small to be a bound] |
| F-11 | Allocation is an entry call in fast mode (`eml_rt_alloc_data`, `eml_rt_alloc_int`, `eml_rt_alloc_float`); rows allocate inside Rust; PAPs are allocated in `eml_nrt/src/closures.rs`; the byte budget is charged per page, not per object | [V: `crates/eml_native/src/lower/data.rs` L125, L335, L360; `crates/eml_rt/src/alloc.rs` L3387; `control.rs` L601-613; review S-13 for the PAP] |
| F-12 | The JIT binds entries by address (`builder.symbol(entry.symbol, (entry.address)())`) | [V: `crates/eml_native/src/jit.rs` L291] |
| F-13 | No `force-frame-pointers` in the workspace. Production code installs only SIGINT and SIGTERM handlers, without `SA_RESTART`, in `eml_cli` (the SIGUSR2 in `eml_io/src/mask.rs` is in its test module). The epoll and kqueue backends handle `Interrupted`, and `sys/unix.rs` retries on `EINTR` | [V: grep `.cargo`, `Cargo.toml`, `flake.nix`; `crates/eml_cli/src/interrupt/unix.rs` L38; `crates/eml_io/src/sys/epoll.rs` L141, `sys/kqueue.rs` L159, `sys/unix.rs` L190-201] |
| F-14 | The planned stages lay hooks H-1..H-7; `jit-tasks` forbids thread-locals in `eml_nrt` / `eml_native` (G-10); workers get `hard = base + 1 MiB` and `real = hard + 2 MiB` of stack headroom (11.1) | [V: `docs/plans/2026-11-17-runtime/01-design.md` 15.2; `docs/plans/2026-11-17-jit-tasks.md` L13, L43] |
| F-15 | `multi` operations exist; a continuation is a list of closures, copied when a shared continuation is resumed | [V: docs/spec/effects.md L23-39] |
| F-16 | Rust's standard library is built with frame pointers since 1.79; aarch64 targets keep them in non-leaf functions | [I: release notes from memory; `native-prof` unit 1 checks the pinned toolchain on x86_64 Linux] |
| F-17 | O-5 holds on the prototype: the hash of every function's code bytes is equal with a source location per statement and per entry call, with one per statement only, and with none, on `fib`, `rbtree`, `list`, `orders`, `records`, `iter_pipeline`, `nbody`, `mandelbrot`, `fold_state`, `push_fold` (`fannkuch_var`: the two variants with locations are equal; the run without was not taken) | [V: prototype, FNV-1a over `code_buffer()` of every defined function] |
| F-18 | A walk that reads only the saved `fp` costs about 1 ns per record: on `list` every sample reached the root (95 of 95; 419 of 419) at an average of 38,000 records in 36..38 us and a maximum of 99,806 records in 139..154 us | [V: prototype, `clock_gettime` around the walk] |
| F-19 | Frameless leaf entries exist on aarch64: in 8 of 142 samples whose `pc` was outside generated code, `lr` was inside generated code and differed from the first walked return address | [V: prototype, `list` at 4 kHz] |
| F-20 | `$uniform` has a thin form and a whole-body copy form; `$owned` is a whole-body copy made by the borrow pass, which also demotes some tail calls to `let r = call; return r` | [V: `crates/eml_lir/src/variants.rs` L286-293; `borrow.rs` L555-558; docs/spec/lir.md L65, L248] |
| F-21 | The REPL renumbers files and modules per input; the VM's `extend` re-keys positions by path and effects by name; `Image` already has a per-load path table `files` | [V: docs/spec/vm.md L81; docs/spec/modules.md 「REPL のセッション」 (review S-2); `crates/eml_nrt/src/image.rs` `Image.files`] |
| F-22 | A `blocking` C function runs on a driver pool thread while its worker goes on; file jobs and name lookups run on `eml_io`'s pool and resolver threads; synchronous stdin, `FileIn` and `Child` reads block the worker inside a row | [V: docs/spec/vm.md L1090, L402; CLAUDE.md's `eml_io` row] |

## 2. The product

### 2.1 Questions and which recording answers them

| Question | Recording | View of `eml prof report` |
|---|---|---|
| Where does time go, by function | time | `--by function` (default): self and total per source function |
| By line | time | `--by line`: self time per `file:line`, with the source function |
| By call stack | time | `eml prof folded` (folded stacks), `eml prof export` (Firefox Profiler) |
| How much is RC, allocation, boxing, effect plumbing, checks | time | `--by role`: the roles of section 4.2, overall and per function |
| Time in handlers and resumptions | time | clause frames are frames of kind `Clause`; resumed continuations show as `f (resumed)`; `--by role` gives `Ev` and `Yield` |
| My code or std | time | `--user`: each sample goes to the innermost frame whose source is not a std module |
| Per worker | time | `--by worker`, and the worker timeline: a state sample per tick for a worker that is idle, in I/O, in a join or parked |
| Per task, and where a task waited (which `read`, which `await`, which `blocking` call) | events (`prof-tasks`) | `--by task`, `--by wait` |
| CPU time inside `blocking` C functions and pool jobs | not in `native-prof` (4.4); pool threads are sampled from `prof-tasks` on | `--by wait` shows them as waits until then |
| How often each function, call edge, allocation site, dup and drop runs | counts | `eml prof report` on a counts file |
| Who makes the heap grow, who holds the peak | heap | `--by growth`; `--at-peak` restricts to growth up to the peak |

### 2.2 CLI

Recording is a flag of `run` (and, after `prof-aot`, an environment variable of a built app); reporting is the subcommand `eml prof`.

```
eml run --prof FILE [--prof-hz N] [run options] prog.em [args]     # time; fast mode, native; default N = 1000
eml run --prof-counts FILE [run options] prog.em [args]            # counts; checked mode, one worker; the file also holds RunStats
eml run --prof-heap FILE [--prof-heap-bytes N] ... prog.em [args]  # heap growth; checked mode
eml prof report FILE [--by function|line|role|worker|task|wait|growth] [--user] [--split] [--top N] [--at-peak]
eml prof folded FILE [--user] [--split] [--lines]                  # one "frame;frame;... weight" line per stack
eml prof export FILE -o OUT.json                                   # Firefox Profiler JSON
eml build --prof prog.em ; EML_PROF=FILE ./prog ; eml prof report FILE --symbols prog.emlsym   # after prof-aot
```

- `--prof` with the VM is a usage error (exit 2), and so is `--prof` on Windows until `prof-windows`. Until `jit-repl` makes native the default, `--prof` requires `--native`. `--prof-hz` is clamped to 1,000. `--prof` is refused when the process already has a SIGPROF handler (6.3).
- `--prof` with `--debug-heap` or `--stats` is a usage error: a time profile of checked code describes the audits. `--prof-counts` and `--prof-heap` imply checked mode and may be combined with each other and with `--stats`. `--prof-counts` with a worker count other than 1 is a usage error.
- `--split` shows machine functions (instance and copy names) under each source function.
- The file is usable after any end of the process (6.9): a second Ctrl-C, `exit`, a fatal error or a kill loses at most the last 100 ms.
- Output formats: text tables, folded stacks (flamegraph.pl, inferno, speedscope) and Firefox Profiler JSON in its processed-profile format with a pinned `meta.preprocessedProfileVersion` (the UI upgrades older versions [I]; if the export unit finds that unreliable, it writes the Gecko format instead). Threads are workers, categories are roles, and inlined frames and task markers are carried. pprof is not written (section 9).

### 2.3 Overhead budget

| Item | Budget | Basis |
|---|---|---|
| Generated code, profiler off or on | byte-identical to today's fast code | F-17; the sampler reads frame records that F-4 already keeps |
| Runtime entries, profiler off | no added instruction | the sampler is a signal handler or another thread; no entry tests a flag |
| Time profile at 1 kHz, ordinary depth | at most 2 % wall on `bench/` | F-6: 0.15..0.9 % |
| Time profile, deep recursion | at most 5 % wall spent walking, by the governor of 6.4 | F-18: 1 ns per record |
| Offset tables | built only on request (`--prof*`, the test API, later a backtrace consumer); encoded size at most 0.6 x the code bytes, expected about 0.3 x | section 5.4's computation from F-9 |
| Compile time with tables | `bench/native.sh` load rows within +3 % instructions | F-10 is too small to bind; `ir-origins` measures |
| Compile time without tables | unchanged apart from copying a 16-byte field | 5.6 |
| Count and heap profiles | checked mode's cost plus one increment per LIR segment | they are deterministic recordings, not timings |

### 2.4 Beside `RunStats`

`RunStats` stays what it is: whole-run counts of runtime work, pinned by snapshots. The count profile is the per-source breakdown of what the code executes (statements), not of what the runtime does. The report prints the two side by side with the terms that connect them: executed allocating statements, minus `RunStats.reuses` (a `con` with a live token does not allocate), plus allocations made by helpers and rows (PAPs in `eml_nrt/src/closures.rs`, strings and arrays inside rows), equals `RunStats.allocations`. The helper-and-row term is not counted per site, so the equation is exact and tested only on programs without PAPs and allocating rows; on others the report prints the remainder as "helpers and rows". A deterministic profile is worth having (a default of section 11): it turns "this pipeline fuses into a loop with no closure allocation" into a test.

## 3. Two designs considered

### 3.1 Design A: symbols and positions on calls (the plan on file)

01-design 15: frames are machine symbols named by LIR names; positions enter LIR on calls only; one table "offset -> Loc" per function.

- Cost: small.
- What the user sees: F-7. Inlined code has no frame and no position, so the time of a user lambda inlined into a std copy is reported against a std function with a generated name; a clause body expanded into a handler-specialised loop is reported as the loop; a 40-statement inlined function with no call inside has no position at all (F-3); every instance and copy of a function is a separate row; RC, boxing and evidence plumbing cannot be told apart from the user's arithmetic.

### 3.2 Design B: origins on every statement (chosen)

Every statement carries where it came from and through which inlined calls. The table maps an offset to a point with its inline chain and role.

- Cost: one `Copy` field on every statement and terminator of both IRs, one rule per pass (5.3), a structural verifier, about 370 pattern sites to adapt in `eml_core_ir` and `eml_lir`, 36 in `eml_native`, 14 in `eml_vm` [V: `grep -c 'Stmt::'`], and tables of about 0.3 x the code bytes when asked for.
- What the user sees: source functions, lines, the inlined frames, and the role of the instructions.

B is chosen because eml's optimizer is name-keyed whole-function specialisation plus inlining and clause expansion (F-2, F-3): under A the more the optimizer succeeds, the less the profile says. The cost of B is mechanical and is paid once; it is smaller now than after `jit-tasks` and `native-inline` add lowering code.

Two further designs are rejected in section 9: instrumentation with cost centres or a shadow stack, and relying on external profilers only.

## 4. The attribution model

### 4.1 Frames

- A physical frame is one return address in generated code. The offset table expands it into source frames: the point's scope, then its parent scopes outward to the function's root scope. Each source frame is `(source function, file:line:col)`.
- A source function (`Source`) is what the programmer wrote: a top-level function, a local function, a lambda, a handler's body, a clause, a `return` clause, an instance method. Every instance (`@[Int, Int]`), copy (`{0=...}`, `{H}`, `{k}`), whole-body ABI variant (`$uniform` copy form, `$owned`) and continuation function (`f{H}$hN`, `f$kN`, `f$jJ`) of it has the same `Source`.
- Machine functions without source (`eml$apply$<m>`, `eml$pap$<m>$<r>`, thunks, closure entries, the thin `$uniform` wrappers, EP's helpers and `$entry` functions, translate's wrappers) are glue: hidden by default, their time goes to the caller's frame with role `Glue`. `--split` shows them.
- A frame in the Rust runtime directly above an eml frame is named by the callee of the eml call site (the point's `callee`: an entry and its `class`, a C function, or an eml function). Frames deeper in the runtime are folded into it unless `--runtime` is given (which needs the tool's or the binary's symbols).

### 4.2 Roles

A role is decided by the lowering from the LIR statement kind and the part of its instruction sequence; it is not stored in the IR.

| Role | Instructions |
|---|---|
| `Code` | what the statement computes: arithmetic, field reads, `switch`, moves |
| `Call` | argument setup and the call or tail call of an eml function or `apply` |
| `Row` | the call of a std row or a C function |
| `Alloc` | `con`, `closure`, `box`, `evidence`: the entry call and the field stores |
| `Reuse` | token tests, in-place construction, `drop_token` |
| `Rc` | `dup`, `decref`, `drop`, `release` (inline part and the slow call) |
| `Box` | `box` / `unbox` of scalars and structs, apart from the allocation |
| `Ev` | evidence lookup, the clause entry read, `evv` swaps, masks |
| `Yield` | the yield-status test after a call, continuation construction, `prompt$` landing |
| `Check` | the entry safepoint, stack probe, overflow and bounds tests, and in checked mode the audits |
| `Glue` | prologue, epilogue, adapters, thunks |

### 4.3 What each compilation feature shows, and what the compiler carries for it

| Feature | The profile shows | Carried |
|---|---|---|
| Specialisation, instance names | one row per source function; `--split` lists instances and copies | `scopes[0].source` on every function, copied by every copy |
| Inlining, known closures, fusion | the inlined function as its own frame under the call site | a scope per inlined call: `Scope { source, parent: Some(call's At) }` |
| Handler specialisation | a clause body expanded at a `perform` is a frame of kind `Clause` under the performing line; the specialised path function is its source function; `f{H}$hN` shows as `f (resumed)` | a scope per expanded clause with the `perform`'s `At` as parent; cloned scope tables |
| Evidence passing | plumbing as roles `Ev` and `Yield` on the user's line; EP's helpers are glue | EP gives each inserted statement the `At` of the `perform`, `handle` or call it serves |
| Tail calls | the physical stack: a tail-called function sits directly under its caller's caller. A function that some tail call targets is marked `(tail)`. Loops are one frame. The exact call graph, tail calls included, is in the count profile | `FnRow.flags` bit `TAIL_TARGET`; no shadow stack |
| In-place clauses (`Call::Clause`) | the clause is a frame called from the performing code | `Source.kind` |
| Yielding operations | the unwinding is role `Yield` in each frame it passes; the clause runs under the handler's frame; `resume` runs the continuation functions as frames `f (resumed)` under the clause. The stack is the physical one: after a resume, the resumed code is below the clause, not below its original caller | `FnRow.flags` bit `CONTINUATION` |
| `multi` | the same: each resume is a physical call; the copy of the captures is role `Yield` in the resume helper | nothing more |
| Perceus, reuse | roles `Rc` and `Reuse` on the line of the statement the instruction was inserted for | the inserted statement's `At` |
| Boxing | role `Box`; whole-body variants keep their source; thin wrappers are glue | the same |
| Closures, thunks, PAPs | `apply` helpers and adapters are glue; the closure's code is a frame of its `Source` (kind `Lambda`) | `Source.kind` |
| `par`, `compute`, `Async` on other workers | each worker's own physical stack; the child's or task's function is the outermost eml frame. From `prof-tasks` on, a task is named by its start site and `--stitch` puts the start stack above the task's frames | the task word in each sample; from `prof-tasks` a start event with an id and a stack |
| std | full stacks by default; `--user` attributes to the nearest user frame | `Source.std` |
| REPL | each load's tables are self-contained (their own files and sources); a function appended earlier keeps the tables of the load that compiled it; the tool merges sources across loads by path and range. A `:prof <input>` command is a later item | 5.4's per-load tables |

### 4.4 Limits stated to the user

- Time is wall-clock time of a worker while it runs eml code or a runtime entry, not CPU time: a worker descheduled by the OS still counts (D-3).
- `blocking` C functions, file jobs and name lookups run on pool threads that `native-prof` does not sample; their time shows as the worker's other work or idleness, and from `prof-tasks` on as waits and as samples of the pool threads.
- A direct (not `blocking`) C function runs in the worker and receives the signal: an interruptible system call inside it (`poll`, `nanosleep`, `usleep`) can return `EINTR` under `--prof`. A C function that sleeps or waits should be declared `blocking`.
- Line and role attribution is by instruction. Cranelift merges and hoists instructions across statements, so a value computed for two lines is charged to one of them; `Rc`, `Box` and `Yield` instructions are charged to the line of the neighbouring statement.
- A sample in a prologue or epilogue of generated code on x86_64 can miss the caller's frame (on aarch64 the link register repairs it, 6.4).
- Stacks deeper than 4,000,000 frames lose their root (`[deep]`).
- Under `--prof` the signal handler needs up to 16 KiB of the worker's stack. A direct C function that has used up the stack headroom below eml code (1 MiB) dies with a stack overflow where it would have survived without the profiler.
- User C code must not change SIGPROF's disposition or end a worker thread with `pthread_exit` during a profiled run; the first makes the next tick terminate the process.
- On x86_64, a sample inside foreign code without frame pointers has only its leaf or an untrusted stack (6.5), and a frameless foreign leaf called directly from eml code hides its eml caller's frame (6.8); the report prints the broken and untrusted shares.
- Before `prof-tasks`, samples are not grouped by task: the task word in a sample is a cell address that is reused after the task ends.

## 5. The compiler change (`ir-origins`)

### 5.1 Types (in `eml_lir`, shared by both IRs)

```rust
/// 文の出どころ。`loc.file == NO_FILE`は位置なしである。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct At { pub loc: Loc, pub scope: ScopeId }

pub struct Scope { pub source: SourceId, pub parent: Option<At> }   // scopes[0]: parent None

pub struct Source {
    pub name: String,          // the function's name without its instance suffix
    pub kind: SourceKind,      // Function, Local, Lambda, HandleBody, Clause { effect, op }, ReturnClause, Method, Synthetic
    pub file: u32,             // index into Program.files; NO_FILE for Synthetic
    pub start: (u32, u32),     // line, column of the definition's range
    pub end: (u32, u32),
    pub std: bool,
}

pub struct Stmt { pub at: At, pub kind: StmtKind }   // today's enum becomes StmtKind, in Core IR and in LIR
// Block gains `term_at: At`; CoreFn and LirFn gain `scopes: Vec<Scope>`; both Programs gain `sources: Vec<Source>`.
```

- `Rhs::Extern.at` and `Call::Par.at` stay. They are behaviour (the position a runtime error reports, pinned by UI tests and kept through inlining); `Stmt.at` is metadata that never changes code. O-6 keeps the two equal until `native-backtrace` merges them.
- A wrapper struct is chosen over a field per variant and over marker statements: a pass cannot construct a statement without an `At`, and no pass has to skip a pseudo-statement.
- Numbering: `Loc.file` and `SourceId` are indices of one program, like `FnIdx` and the effect numbers. Nothing compares them across programs. Where code of two programs is compared or joined (the REPL's append and its debug comparison), origins are re-keyed first: files by path, sources by `(path, start, end, kind, name)`; section 5.4 and the `jit-repl` row of 8.2.
- Text form: the default printer does not print origins, so every existing IR snapshot stays byte-identical (kind 3). A second printer mode prints a `scopes` header per function and a trailing `@<scope>:<line>:<col>` per statement and terminator; `parse` reads both. A statement parsed without an origin gets `At::NONE` (scope 0, no position), and a function parsed without a header gets one `Synthetic` root scope.

### 5.2 Invariants

Checked by the verifiers of both IRs after every pass in debug builds, except O-4 and O-5.

- O-1: every `At.scope` indexes the function's `scopes`; `scopes[0].parent` is `None`; every other scope's parent `At` has a smaller scope index (the tree is acyclic and rooted at 0).
- O-2: every scope's `source` indexes `Program.sources`. A function made from another (a copy, a whole-body variant, a continuation or join-block function) has the same `scopes[0].source`; this holds by construction, because such a function clones the table.
- O-3: for a statement with a position, the position lies inside the range of its scope's source: `loc.file == source.file` and `start <= (line, column) <= end`. `Synthetic` sources and position-less `At`s are exempt. Every pass preserves O-3 because passes only copy `At`s; a pass that gives a statement the wrong scope fails it.
- O-4 (its own check, called by `pipeline::check` for `Pass::Translate` only, not part of `verify_translated`, which also runs after optimize): every statement and terminator of a function whose root source is not `Synthetic` has a position. After translate a pass may create position-less statements where 5.3 says so.
- O-5 (a test, not a verifier): lowering with source locations set gives byte-identical machine code to lowering without. The test runs over the `bench/` programs and the programs of `crates/eml_native/tests` in the ordinary run. [V on the prototype: F-17.] Origins therefore never influence code, and "a function's name fixes its code and ABI" is unchanged. A function's origins are fixed by its name up to the numbering of files and sources, because every inlining, expansion and copy decision is fixed by the name (F-2).
- O-6: a statement whose kind carries `at: Some(l)` (`Extern`, `Par`) has `stmt.at.loc == l`.

### 5.3 The rule each pass obeys

General rules:

- R-keep: a statement that a pass moves, rewrites in place or replaces by one statement keeps its `At`. A function that is a clone of another clones its `scopes`.
- R-ins: a statement a pass inserts takes the `At` of the statement it is inserted for: the next statement (or the block's `term_at`) when it prepares or releases something before it, the previous statement when it post-processes that statement's result.
- R-term: a terminator made from a statement takes that statement's `At`, and a statement made from a terminator takes the `term_at`.
- R-inline: copying the body of function `g` into function `f` at statement `s` appends `g`'s scopes to `f`'s table; `g`'s root scope becomes `Scope { source: g's root source, parent: Some(s.at) }`; `g`'s other scopes keep their parents, re-indexed; every copied `At` is re-indexed. Jumps and block parameters created to join the body to `f` take `s.at`.

Core IR, in the order of `optimize()` and then EP:

| Pass (file) | Rule |
|---|---|
| translate | the builder holds a current `At`; `emit` stamps it. The current `At` is set from the HIR expression or pattern being translated (`Translator::loc`, which today serves only externs). Each translated function gets `scopes = [root]` with its `Source`; `sources` is interned by HIR function, lambda and clause. Wrappers (`$op$`, `$con$`, `$cont$`, `$cont$state`), derived functions, display functions and `entry$main` get `Synthetic` sources. For `Extern` and `Par` the statement's position is the `at` (O-6) |
| `direct::lifted` | reads only |
| `specialize`, `known` | a copy `g{pos=f/n}` is a clone of the callee's translate output (R-keep). `apply` rewritten to `call` keeps its `At`. A call of a wrapper turned back into the instruction: the instruction takes the call's `At` (the wrapper is `Synthetic`, so no scope is added). A re-created `closure` follows R-ins with the statement that needs the value |
| `recompute_yields` | no statement changes |
| `handlers`: the path copy `f{H}` (both forms) | a clone of `f` (R-keep); the `handle` statement replaced by the call of the copy keeps its `At` |
| `handlers`: clause expansion at a `perform` (`piece::inline_clause`, both forms) | R-inline with `g` = the clause function and `s` = the `perform`. The jump that replaces the clause's `resume` takes the `resume`'s `At` (R-term) |
| `handlers`: state threading (`passing`: state variables, the `Returned` values at calls of path copies) | R-ins with the call or `perform` they serve |
| `handlers`: continuation functions `f{H}$hN` and join-block functions `f{H}$hjJ` (`piece::region`) | clone `f{H}`'s scope table and keep its root source; statements moved into them keep their `At`; flag `CONTINUATION` |
| `handlers`: clause copies `<g>{k}` and `<g>{k}{...}` | clones of `g` (R-keep); the tail call of the clause copy at a `perform`, the closure of the continuation function and a `never` operation's tail call of the clause take the `perform`'s `At` |
| `handlers`: the `return` clause called at the handle site | the call takes the `handle`'s `At` |
| `handlers`, `fuse`: `blocks` (constant `switch`, pruning, joining) | R-keep; a folded `switch` leaves a `jump` with the `switch`'s `term_at` |
| `fuse/inline` | R-inline with `s` = the call |
| `fuse/known_con` | a resolved `switch` or `unpack` leaves substitutions and, for a `switch`, a `jump` with its `term_at`; an `apply` of a field's known closure rewritten to `call` keeps its `At` |
| `fuse`: known closures | as `known` |
| `fuse/copies` | a copy keyed by a `con` shape is a clone of the callee's rewritten code (R-keep); the `closure` and `con` re-created at its entry are position-less in scope 0 |
| `fuse`: unused pure `let` | removal only |
| `direct::type_direct_only` | changes `internal` and `split` only |
| `retain_reachable` | drops functions; `sources` is not renumbered |
| EP `split.rs`: `f$kN`, `f$jJ` | clone `f`'s scope table and keep its root source; statements copied into them keep their `At`; flag `CONTINUATION`. The yield test and the slow path after a call follow R-ins with that call |
| EP `handlers.rs` | the statements that replace `handle`, `perform`, `resume` and `mask` take that statement's `At`. The clause entry functions `<g>$entry` get `scopes = [root: g's source]` and position-less statements. Explicit drops follow R-ins |
| EP `helpers.rs`, the mask wrappers | `Synthetic` functions |
| `to_lir` | one to one |

LIR, in the order of `PASSES`:

| Pass | Rule |
|---|---|
| boxing, `variants.rs` | `box` / `unbox` follow R-ins with the consumer or producer. The copy form of `<f>$uniform` is a clone of `f` (R-keep). The thin form gets `scopes = [root: f's source]`, position-less statements and flag `GLUE` |
| flatten | the statements that replace one statement take its `At` |
| contract | R-term for `tail call`; a removed statement leaves nothing |
| borrow | `<f>$owned` is a clone of `f` (R-keep). A demoted tail call becomes `let r = call; return r`, both with the tail call's `term_at` (R-term) |
| Perceus | `dup`, `decref`, `drop`, `release` take the `At` of the statement or terminator they are inserted in front of (R-ins) |
| reuse | `drop_token` follows R-ins; a `con` that gains a token keeps its `At` |

### 5.4 Native tables (the last unit of `ir-origins`)

- Tables are built only when the machine is created with `origins: true` (`--prof*`, the test API; `native-backtrace` decides later whether errors need them always). Without it the lowering sets no source location. O-5 is what makes the two lowerings interchangeable.
- `lower/origin.rs` (new) owns `set_srcloc`: `FnLower::at(at, role)` interns a point and sets the builder's source location to the point's index + 1; `FnLower::calling(callee)` sets a point with a callee for one call instruction and restores the statement's point. Point indices stay below 2^31 (Cranelift keeps one bit) [V: `MaybeRelSourceLoc`]. A placement test checks that no other module of `lower/` calls `set_srcloc`. `stmt()` and `term()` set the point at each statement; `rc.rs`, `data.rs`, `effects.rs`, `unwind.rs`, the safepoint and the audit helpers set the role of their part.
- After `define_function`, `define` reads `compiled_code().buffer.get_srclocs_sorted()` (absolute locations, `loc.as_abs().bits()`) [V: prototype] and builds the rows. Instructions with the default location (prologue, epilogue) have no row and belong to the root scope with role `Glue`.
- Per function, two byte strings and a scope list; per load, a source list and the path table that `Image` already has:

```
lines:   per row  varint(offset - previous offset), varint(zigzag(point - previous point))    // sorted by offset; a row covers up to the next row
points:  per point  varint(zigzag(line - previous line)), varint(column), varint(scope), u8 role, varint(callee)   // file is the scope's source's file
scopes:  per scope  varint(source), varint(parent point + 1)                                  // 0: no parent
sources (per load): name, kind, std, file (index into this load's `files`), start, end, effect, op
// FnRow gains: lines: Slice<u8>, points: Slice<u8>, scopes: Slice<u8>, flags: u32 (TAIL_TARGET, CONTINUATION, GLUE)
// Image gains: sources: Slice<SourceRow>
```

- Every index in a load's tables refers to that load: `points` to the function's scopes, scopes to the load's `sources`, sources to the load's `files`. A load is self-contained, so the REPL's renumbering (F-21) needs no re-keying at run time; the tool merges sources of different loads by `(path, start, end, kind, name)`.
- `callee` is `NONE`, an index into the entry descriptor rows, `EML` (a direct or indirect eml call) or a C function index. A return address `ra` is looked up at `ra - 1`, inside the call instruction.
- Size. Measured (F-9, with a point per statement and per entry call): rows 10.4..12.8 % of the code bytes. Computed upper bound for the worst measured program (rbtree: 14,224 code bytes, 245 LIR statements, 800 rows, 414 points): a point per eml call instruction adds at most 2 rows and 1 point per statement, so at most 1,290 rows of 2.3 bytes (2,967 bytes) and 659 points of at most 7 bytes (4,613 bytes), plus scopes and sources below 200 bytes: at most 0.55 x the code bytes. The same computation for nbody gives 0.40 x. Budget: 0.6 x; `ir-origins` records the real figure.
- The CLIF dump used by the pins (`lower::clif`) clears source locations before printing, so every CLIF pin stays byte-identical (kind 3).
- Nothing on the run path reads these tables; a shipped app keeps them out of the binary (`prof-aot` writes them to a side file).
- Test API: `NativeMachine::points(name)` (doc-hidden) returns the rows resolved to `(source name, line, role)`.

### 5.5 The VM

`eml_vm` adapts to `Stmt { at, kind }` and ignores `at`. It keeps its own error-position table until `vm-retire`. No profiler, no tables.

### 5.6 Cost

- Memory: 16 bytes per statement and terminator, a few scopes per function.
- Code size: none in generated code; tables as 5.4, on request.
- Compile time: copies of a `Copy` field; with tables, one hash lookup per statement in the lowering and one pass over the source-location ranges per function. `ir-origins` records `bench/native.sh` load rows before and after (budget of 2.3).

## 6. The runtime: sampling (`native-prof`, Linux and macOS)

This section is the complete statement of the mechanism and of its safety argument. It covers unix only; Windows is 6.6.

### 6.1 Placement

- `eml_sched::prof` (new module; the runtime closure): the registry, the handler, the walk, the rings, the monitor's tick, the file writer. It uses `libc` (already a dependency on unix [V: `crates/eml_sched/Cargo.toml`]); no workspace edge changes. It does not depend on `eml_nrt`; the code ranges of 6.5 are plain values in `RunConfig`, used only by the tool.
- Where `unsafe` may be: `eml_sched` has no `#![deny(unsafe_code)]` and no `trust.rs` today [V: `crates/eml_sched/src/lib.rs`]. The sampler's `unsafe` operations all live in one file, `prof/sys.rs`: the handler's body (reading the `ucontext`), `Live` and its assembly load, `sigaction`, `pthread_kill`, `pthread_sigmask`, the stack-bounds calls, and the ring's raw storage. `prof/walk.rs`, `prof/ring.rs` (index logic), `prof/registry.rs` (the protocol over atomics) and `prof/file.rs` carry `#![forbid(unsafe_code)]`. A test in the model of `eml_io`'s and `eml_nrt`'s `trust.rs` pins that list. The assembly load is the only `asm!` in `eml_sched`; the rule "no asm but the entry stubs" of `jit-tasks` G-10 is about `eml_nrt` and `eml_native` and is not touched.
- `eml_cli`: `--prof`, the writer of each load's tables (6.9), and `eml prof`.
- `eml_rt`: no change. It exposes nothing new. The trust list T0-T18 is unchanged.
- Trust rows: docs/spec/runtime.md 「信頼の一覧」 (born at `sched-core` with ST-1..ST-3, which are assumptions of the scheduler's model) gets a second group for memory-safety assumptions of `eml_sched`, and the sampler's rows SM-1, SM-3, SM-4 (6.7) go there.

### 6.2 The registry and scoped registration

- A process-wide fixed array `REGISTRY: [Entry; 256]`. An entry is atomics only: `thread` (the key: the registering thread's `pthread_t` as a word, 0 when free), `lo`, `hi` (stack bounds), `ring` (pointer), `slot` (pointer to the worker's `Slot`), `run` (the run's id). No Rust thread-local (F-14).
- Registration is a scoped function, not a guard value:

```rust
/// 呼んだスレッドを登録して`body`を動かし、戻るときと巻き戻るときに登録を消す。
pub(crate) fn registered<R>(run: u64, slot: &Slot, ring: &Ring, body: impl FnOnce() -> R) -> R
```

  It measures the calling thread's stack (`pthread_getattr_np` on Linux, `pthread_get_stackaddr_np` and `pthread_get_stacksize_np` on macOS), unblocks SIGPROF on the thread (6.3), stores the entry, runs `body`, and clears the entry through a private drop guard on return and on unwind. The borrows of `slot` and `ring` outlive the entry by construction; there is no value to forget or to move to another thread. The worker thread functions (worker 0, workers 1..N-1, the main worker) call it around their loop. A full registry runs `body` unregistered and counts the worker in the file. Under Miri (`cfg(miri)`) it runs `body` unregistered, because the stack cannot be measured there.
- Store order and orderings. Registering, under the registry mutex: store `lo`, `hi`, `ring`, `slot`, `run` (`Relaxed`), then `thread = pthread_self()` (`Release`). Unregistering, under the mutex: store `thread = 0` (`Release`) first; the other fields are left as they are. Readers load `thread` with `Acquire` and the other fields after it.
- Why a reader never uses a torn entry. The handler on thread A takes only an entry whose `thread` equals A's own `pthread_self()`. A live thread's `pthread_t` is unique among live threads, so only A's own registration can have stored that key; A stored the other fields before the key, on the same thread, and A cannot unregister while it is inside its own handler. An entry of another thread, torn or reused, has another key and is skipped whatever its other fields hold. The match does not depend on any other entry's `lo` or `hi`.
- The mutex: registration, unregistration and the monitor's whole tick (6.3) take it. The handler never does. Poisoning is ignored (`lock().unwrap_or_else(PoisonError::into_inner)`): the state is atomics, and a worker unwinding through its unregistration must not panic again.
- The monitor of a run acts only on entries with its `run` id, so two profiled runs in one process (tests under `cargo test`, REPL sessions) do not signal each other's workers.

### 6.3 The directed signal

- Installation, once, when the first profiled run starts; the handler is never removed.
  - `sigaction(SIGPROF)` with `SA_SIGINFO | SA_RESTART` and an empty `sa_mask`. SIGPROF itself stays blocked during its handler (no `SA_NODEFER`), which makes the per-entry state single-writer. SIGINT and SIGTERM are not blocked: their handlers are `SA_NODEFER`, atomics, a `write` and `_exit` [V: `crates/eml_cli/src/interrupt/unix.rs`], so they may nest inside the SIGPROF handler, and blocking them for a walk of up to 4 ms would delay the second-Ctrl-C exit.
  - The previous disposition is read. If it is neither `SIG_DFL` nor `SIG_IGN` (user C code or a linked library owns SIGPROF, for example through `setitimer(ITIMER_PROF)`), `--prof` is refused with an error; the disposition is not replaced. SIGPROF has no other user in the repository (F-13).
  - Every libc function the handler calls (`pthread_self`, the clock function, the `errno` accessor) is called once here, so no lazy symbol binding can run inside the handler, whatever the link flags.
- `registered` unblocks SIGPROF on its thread with `pthread_sigmask`: threads inherit their creator's mask, and a parent can start eml with signals blocked (the repository already handles that for SIGINT, `unblock()` in the same file).
- The monitor thread (it exists for signal relay; with `--prof` it is started for one worker too) wakes at absolute deadlines `t0 + k / hz`. Its wait is the doorbell's `poll` with a millisecond timeout rounded up [V: `crates/eml_sched/src/doorbell.rs` L57-66], so `--prof-hz` is clamped to 1,000 and the effective rate is lower (F-6: about 664 Hz); the tool's time weighting (6.4) accounts for it. Each tick, holding the registry mutex, for each entry of its run with a non-zero `thread`, it reads `Slot.phase` (H-8) through the entry's `slot` pointer:
  - `Run`: `pthread_kill(thread, SIGPROF)`, with `thread` read from the entry. The thread is alive: its key is cleared only by its own unregistration, which needs the mutex, and that happens before its thread function returns.
  - `Idle`, `Io`, `Join`, `Park`: the monitor writes a state record itself (no signal), so a sleeping worker is not woken.
  The monitor thread is never registered and never signalled.
- The phase can change between the read and the delivery. The handler therefore reads `Slot.phase` again and records it; a sample whose phase is not `Run` is used by the tool as a state sample. A signal that lands in a blocking system call makes it return `EINTR` or restart; `epoll_pwait` and `kevent` return `Ok(())` on `Interrupted`, `sys/unix.rs` retries, the host's reads and writes retry when no stop bit is set, and SIGINT's own no-`SA_RESTART` behaviour is unchanged because restart is decided per interrupting signal [V: F-13; `crates/eml_rt/src/host.rs` L676, L728, L1675-1683]. No worker-side call turns a SIGPROF `EINTR` into a visible error.
- Rows that block the worker synchronously (the stdin, `FileIn` and `Child` reads of docs/spec/vm.md L402) set `Slot.phase` to `Io` through a guard that wraps the whole row's body in `eml_nrt` / `eml_rows`. The blocking call itself is inside `eml_rt`'s host code, which is not changed; a narrower guard would be an `eml_rt` change with its gate and is not proposed. `native-prof` unit 3 lists the rows.
- The handler does, in order:
  1. save `errno`;
  2. read `pc`, `fp`, `sp` and on aarch64 `lr` from the `ucontext`;
  3. `pthread_self()`; find the registry entry whose `thread` equals it (`Acquire`), then load its other fields and require `lo <= sp < hi` (no entry, or `sp` outside: count `foreign`, restore `errno`, return);
  4. test the ring's `busy` flag (set: count `lost`) and the governor's `resume_at` (not yet: count `skipped`); in both cases restore `errno` and return;
  5. read the time, `Slot.phase` and `Slot.current`. The clock is the same for the handler and the monitor's state records: `CLOCK_MONOTONIC` on Linux (vDSO), `clock_gettime_nsec_np(CLOCK_UPTIME_RAW)` on macOS (commpage) [I];
  6. walk (6.4);
  7. write one record into the ring and publish it by one atomic store; set `resume_at`;
  8. restore `errno`.
- The handler calls no allocator, takes no lock, touches no Rust thread-local, and cannot panic: it uses no indexing, no checked arithmetic (wrapping operations and explicit comparisons only), no formatting, and its `extern "C"` boundary would abort rather than unwind.
- Stack use: the handler runs on the worker's stack (no alternate stack), below the interrupted `sp`. It needs the kernel's signal frame (several KiB with vector state [I]) plus its own frames with the 64-slot record under construction (64 return addresses, 64 counts, the rolling root part: under 2 KiB), so under 16 KiB in all. A worker's stack limit leaves at least 1 MiB below the point where eml code stops (F-14), and the main worker 2 MiB. A direct C call that has itself consumed that headroom makes the kernel unable to push the signal frame; the process then dies with a stack-overflow report under `--prof` where it would have survived without (4.4).

### 6.4 The walk

```rust
/// スタックの語の読み手。範囲の外の番地には`None`を返す。
trait Words { fn word(&self, addr: usize) -> Option<usize>; }

fn walk<W: Words>(mem: &W, start: Regs, out: &mut Stack) -> Walk     // safe: every read goes through `mem`

/// 出荷する読み手。`[floor, hi)`の中の8にそろった番地だけを読む。
struct Live { floor: usize, hi: usize }
impl Live {
    /// # Safety
    /// `[floor, hi)`は、この値が生きている間、対応づけられた読めるメモリである。
    unsafe fn new(floor: usize, hi: usize) -> Live
}
```

- `walk` is safe code over the `Words` seam, in a file that forbids `unsafe`. The shipped reader `Live` checks `floor <= addr`, `addr + 8 <= hi`, `addr % 8 == 0` and then loads one word with one inline-assembly load (`ldr` on aarch64, `mov` on x86_64; `options(nostack, readonly, preserves_flags)`). Its contract is only that the range is mapped and readable: it says nothing about writers or initialisation, because the handler cannot establish either (other threads legitimately write into a thread's stack range: scoped threads writing a parent's locals, the thread control block at the top of a glibc stack). An assembly load has machine semantics for a word that belongs to another frame, is uninitialised or is written concurrently; it needs no provenance and is not instrumented by ASan or TSan. The handler builds `Live` with `floor = max(sp, lo)`, which equals `sp` after step 3.
- The Miri reader reads a `&[usize]` by offset, so the walk's logic runs under the repository's Miri flags (`-Zmiri-strict-provenance`, both borrow models); `Live` itself is outside Miri.
- A frame record at `fp` is used only if `mem.word(fp)` and `mem.word(fp + 8)` are `Some` and `fp` is greater than the previous record's address. Otherwise the walk stops with `Broken`. It ends with `Complete` at a null saved `fp`.
- The values read are untrusted integers: a return address is only stored; a saved `fp` is only compared and used as the next address under the same test. The strictly increasing `fp` bounds the loop by the size of the range.
- On aarch64 the first slot after `pc` is `lr`, marked as such. The tool keeps it as a frame when `pc` is outside generated code, `lr` is inside generated code and `lr` differs from the first walked return address (a frameless leaf entry, F-19), or when `pc` is in generated code within a function's first or last instructions without a row (prologue and epilogue); otherwise it drops it.
- Storage: 64 slots. The leaf-most 48 distinct return addresses and the root-most 16 are kept, with a repeat count per slot in a separate array (a return address equal to the previous one increments the count instead of taking a slot, so self-recursion through one call site is one slot). A flag says that the middle was elided. Return addresses are stored raw; `Header` records the machine's virtual-address width, and the tool masks return addresses to it (pointer authentication codes on aarch64 Linux).
- Reaching the root: after the leaf slots are full the walk reads only saved `fp`s and the return addresses for the rolling root part, up to 4,000,000 records (F-18: about 1 ns per record, so at most about 4 ms; beyond it the walk stops with `Deep`). So a 100,000-deep non-tail recursion is rooted in every sample [V: F-18].
- The governor: after a walk that took `t`, the handler sets `resume_at = now + 20 t`; ticks before `resume_at` return at step 4. Walking therefore takes at most 5 % of a worker's time whatever the depth. The tool weights each sample by the time since the worker's previous sample or state record, so skipped ticks do not bias the totals. Expected on `list`: an average walk of 37 us lowers the effective rate from 1,000 Hz to about 570 Hz during the deep phase, every sample rooted.

### 6.5 Classification of a sample (in the tool)

- Each load record carries the code range of every function of the load and the ranges of `eml_enter` and the thunks; `Header` carries the text ranges of the process's images.
- A stack is `rooted` if it contains a return address in `eml_enter`; the tool cuts the frames outside the outermost `eml_enter` by default (they are the worker loop).
- A stack is `trusted` only if the walk was `Complete`, every return address lies in a known code range (a load's generated code, `eml_enter`, the thunks, an image's text), and the first record is consistent with the kind of `pc`'s range. On x86_64, foreign code that uses `rbp` as a general register can leave in it an aligned address that points at a stale record of a returned call; the walk accepts such a chain as `Complete` with a plausible wrong stack, and this test is what catches it. The report prints the `Broken` share and the untrusted share; their sum is the figure of 6.8.

### 6.6 Windows: not in `native-prof`

`eml run --prof` is a usage error on Windows until a later item, `prof-windows`, with its own design and its own soundness review (D-4 of section 11). What that design starts from:

- The registry, the scoped registration, the ring, the file and the tool are shared. The entry's handle is `DuplicateHandle(GetCurrentThread())` with `THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT`, taken by the registering thread and closed at unregistration (`GetCurrentThread()` alone is a pseudo-handle that names the caller).
- The monitor, holding the registry mutex: `SuspendThread` (a failure skips the sample); `GetThreadContext`; require `lo <= Rsp < hi`; copy a bounded part of `[Rsp, hi)` into a preallocated buffer; `ResumeThread` on every path. The section is panic-free and calls nothing else. This much is deadlock-free: the mutex is taken before the suspend, and the two calls and the copy take no user-mode lock [I: the soundness review].
- Rust's Win64 frames have no standard record at `rbp` [I], so the frame chain does not cross runtime frames. `RtlVirtualUnwind` over a copy is not acceptable: it dereferences whatever the context and the unwind codes say, outside the copy and outside any bound the profiler sets. The design to evaluate is a decoder of the x64 unwind codes inside the profiler over the same `Words` reader on the copy, using the OS only to find the `RUNTIME_FUNCTION` after the resume; and the frame chain for generated code.
- Open there: the cost of the copy against the governor, deep stacks beyond the copy, unloaded user libraries.

### 6.7 Soundness

The `unsafe` surface and its classification under docs/implementation/architecture.md の「公開の面と`unsafe`」:

| Item | Classification | Why |
|---|---|---|
| `Live::new` | private `unsafe fn` with `# Safety` | the mapped, readable range `[floor, hi)` |
| `walk`, `Words` | safe, private, in a file that forbids `unsafe` | every read goes through the reader |
| `prof::registered` | safe, `pub(crate)` | the bounds and the thread key are taken on the calling thread inside the function; the borrows of `Slot` and the ring span the registration; unregistration is not skippable |
| the handler, the registry array, the OS calls | private, in `prof/sys.rs` | no other crate names them |
| `prof::start(&RunConfig)` and the `RunConfig` fields | safe | nothing unsafe under them relies on the caller: the rate is clamped, the file is a path, the code ranges are written into the file for the tool |

The argument that no safe function reaches undefined behaviour (docs/spec/lir.md 「エンジンの約束」), given the trust rows of item 7:

1. Which memory is read. The handler reads only through `Live { floor: sp, hi }`, where the entry was found by the handler's own thread key and `lo <= sp < hi` holds. The entry's bounds were measured by this thread (6.2). `sp` comes from the `ucontext`: it was a stack pointer of this thread at or after registration. So `[sp, hi)` is part of this thread's stack at or above a stack pointer it has had: mapped and readable on a `pthread_create` stack (one mapping, the guard page below `lo`), on the Linux main thread (the growing mapping is extended for accesses at or above the stack pointer [I]) and on the macOS main thread; untouched pages of a 1 GiB reserved stack are demand-zero and read as zero. There is no guard page inside the range. This wording also covers a runtime that delivers the signal late with a saved `ucontext` (TSan does, item 8): a once-valid `sp` of this thread is still inside the mapped stack.
2. No fault from the values. 6.4: values are compared and stored, never dereferenced except as the next record address under the reader's bounds; the loop is bounded by the range and by the 4,000,000 cap.
3. Races. The walk's loads may race with another thread's writes into this thread's stack range and may read uninitialised words; they are assembly loads whose values are only compared and stored (SM-1). The ring is single-producer: the handler of the registered thread (SIGPROF does not nest in itself; no other thread can match the entry, 6.2), or a synchronous writer on the same thread guarded by `busy`; and single-consumer: the monitor. Its indices are atomics. Registry entries are atomics, read in the order of 6.2.
4. Async-signal safety. 6.3's list. The handler can interrupt any code of a worker:
   - the allocator, any lock holder, or a Rust unwind in progress: the handler takes no lock and allocates nothing;
   - `eml_fatal`'s `longjmp` after it moved the stack pointer up while `fp` still points below it: `fp < floor` and the walk stops with `Broken`. At every other instruction boundary of `eml_enter` and `longjmp`, on aarch64 and SysV x86_64, `fp` is a valid record at or above `sp` [V: the soundness review read `crates/eml_nrt/src/arch/`];
   - a synchronous writer of the ring: `busy` makes the handler drop the sample;
   - `registered`'s own registration or unregistration: the key is stored last and cleared first, so the handler finds either a complete entry or none;
   - the SIGINT or SIGTERM handler, and they can interrupt it: they touch atomics, the self-pipe and `_exit` only.
5. Thread lifetime. `pthread_kill` is called under the mutex with the `thread` read from an entry whose key is non-zero; the key is the registering thread's own `pthread_self()`, and it is cleared under the same mutex inside `registered`, before the thread function returns. So the monitor never signals a value that is not a live `pthread_t`. A thread ended by `abort` or process exit ends the process. A user C function that ends a worker with `pthread_exit` skips the unregistration; that breaks the C function's contract and is outside the guarantee (4.4).
6. Signal disposition. The handler is installed once, over `SIG_DFL` or `SIG_IGN` only, and never removed, so a late signal after a run is handled (it finds no entry and returns). A thread that is not registered and receives SIGPROF from outside finds no entry and returns. User C code that resets SIGPROF to `SIG_DFL` during a profiled run makes the monitor's next `pthread_kill` terminate the process (4.4).
7. What stays outside the language's model, as trust rows of the runtime closure:
   - SM-1: the handler loads words of `[max(sp, lo), hi)` of its own thread's stack by an assembly load; a loaded word may belong to another frame, may be uninitialised, and may be written concurrently by another thread; the value is only compared and stored (6.4).
   - SM-3: the OS's report of the calling thread's stack is correct at its upper end (`hi`). With the identity key, the lookup depends on no entry's `lo`; `lo` only has to be at or below the thread's stack pointers, which is checked per sample (`lo <= sp`).
   - SM-4: the libc functions called in the handler (`pthread_self`, the clock function of 6.3 step 5, the `errno` accessor) are async-signal-safe on the supported targets, and a live thread's `pthread_t` is unique among live threads.
   (SM-2 was the Windows row of r2; it goes to `prof-windows`.)
8. Checks:
   - Miri: the walk's logic through the slice reader; the registry protocol and the ring's index logic as ordinary code. `registered` does not register under Miri.
   - ASan (Linux): the handler tests run; the assembly load is not instrumented.
   - TSan (Linux): TSan intercepts `sigaction` and delivers asynchronous signals later from its own runtime with the saved `ucontext` [I], so `sp`, `fp` and `pc` are stale there. The TSan line checks the registry, ring and monitor protocol with signals raised synchronously on the own thread (delivered at once [I]); stack contents are not asserted under TSan.
   - The synchronous tests of 6.10 on Linux (x86_64, aarch64) and macOS.

### 6.8 Frame pointers in the runtime's Rust code and in foreign code

- aarch64 (macOS, Linux): present in non-leaf functions; frameless leaves are repaired by `lr` (6.4) [V on macOS: F-5, F-19].
- x86_64 Linux: `.cargo/config.toml` gets `[target.x86_64-unknown-linux-gnu] rustflags = ["-C", "force-frame-pointers=yes"]` for the whole workspace. `build.rustflags` and a `RUSTFLAGS` environment variable replace each other, so the ASan and TSan command lines and the nix build add the same flag. Later `eml_rtlib`'s build does too. The bench tables are measured on aarch64 macOS and do not change.
- Foreign code without frame pointers on x86_64 Linux (libc internals, user C functions): a sample there may be `Broken` after its leaf, may be `Complete` but untrusted (6.5), and a frameless foreign leaf that leaves `rbp` alone hides the frame of its caller. `native-prof` measures the share of broken plus untrusted samples on `bench/` and `bench/scripts`. If it is over 2 % of samples, the stage brings the measurement to the user with the exit anchor as the proposal: generated code stores its frame pointer into a new `Native` field before a direct C call (not before runtime entries), and the walk restarts there. That would change `eml_rt`'s `Native` and fast code at C calls, so it is not taken without the user.

### 6.9 The file

- Opened at the start of the run; every record is self-delimiting (kind, length), and the reader stops at a truncated tail.
- `Header`: magic, version, target, `hz`, clock origin, the virtual-address width, the text ranges of the process's images.
- `Load` (written by `eml_cli` when a program or a REPL input is loaded, before its code runs): the load's path table, sources, per-function code ranges, names, flags and tables (5.4), the entry descriptor rows with `class`, and the ranges of `eml_enter` and the thunks.
- `Sample { time, worker, phase, walk, task word, n, slots[n], counts[n] }`, `State { time, worker, phase }`, and from `prof-tasks` on `Event`. The monitor drains the rings and writes to the file descriptor every 100 ms and at the end of the run.
- `Trailer` (optional, at a normal end): the counts `lost`, `foreign`, `skipped`, unprofiled workers, and names for runtime addresses seen in samples (for `--runtime`).
- So a process that ends by a second Ctrl-C, `exit`, a fatal error or a kill leaves a file with every load's tables and all samples but the last 100 ms.
- The runtime never resolves a name.

### 6.10 Tests within the 1 s rule

- `walk` over the slice reader: complete, broken at each test, deep, repeat counts, the elided middle, the aarch64 `lr` slot (unit tests; Miri).
- The registry protocol: an entry with another key is never matched whatever its other fields hold; a reused slot; the entry is gone after `registered` returns and after its body unwinds; a poisoned mutex does not make unregistration panic.
- A synchronous sample: a user `extern "C"` function linked through the existing FFI test path (no new std extern) raises `SIGPROF` on its own thread and the test reads the record; it resolves it through `points` and expects `eml_enter`, the thunk and the eml frames in order, with a tail-called function directly under its caller's caller. The outcome does not depend on time.
- The same hook raises SIGPROF inside the driver's wait path and inside a synchronous read, and the run's output is unchanged (the `EINTR` behaviour, deterministic).
- A multi-worker test raises one signal on each of two workers at a hook point (`Policy::at`); a test with two runs in one process checks that each monitor signals only its own entries.
- `--prof` is refused when SIGPROF already has a handler; a worker started with SIGPROF blocked still yields samples.
- A truncated file is read up to its last complete record.
- The rate, the 2 % and 5 % budgets, the x86_64 broken and untrusted shares and the `io::` / `signal::` suites under a 1 kHz sampler are `#[ignore]` measurements recorded in docs/implementation/benchmarks.md.

## 7. Counts, events and heap (later items, design-level)

### 7.1 `prof-counts`

- One worker only. A lowering flag adds one inline 64-bit increment at the head of every LIR segment, on a per-function array (`n:<name>` data symbol). A segment starts at a block's head and after every statement that is an unwinding point (docs/spec/lir.md 「巻き戻しの地点」: a call that can yield, raise or abort), so a statement after a call that did not return normally is not counted. The lowering is otherwise the checked lowering.
- The file holds, per function and segment, the count and the segment's statements with their points; the tool multiplies. It gives exact executions per source function and line, exact call edges for direct calls and tail calls, and exact counts of allocating statements, `dup`, `decref`, `box`, `unbox`, clause calls and yield tests.
- The relation to `RunStats` is 2.4.
- `crates/eml_vm/tests/bench.rs`' successor may pin selected rows of it.

### 7.2 `prof-tasks`

- With `--prof`, the task rows of class `Task` and the scheduler's points (`Policy::at`, H-6) write events through the ring's synchronous writer: task start (with a fresh id and the starting stack, walked synchronously), suspend (with the stack and the operation), resume, end, `par` fork and join, and the submission and completion of `blocking` calls and pool jobs.
- Pool and resolver threads of `eml_io` register themselves through a thread-start hook the driver's creator passes (so `eml_io` does not name `eml_sched`), tagged with the job's id; they are sampled like workers, and their samples are grouped under the submitting task and site.
- The tool derives: `--by task` (by id, named by the start site), wait time per suspension stack (`--by wait`), `--stitch`, and Firefox Profiler markers.
- This absorbs the roadmap item 「スケジューラの記録」.

### 7.3 `prof-heap`

- In checked mode only, the checked expansion of a `shim` entry compares `Control`'s charged bytes before and after its body when the run asked for a heap profile; on growth it writes a sample with the synchronous walk and the grown bytes. Budget charging is per page (F-11), so growth events are sparse and the fast table is untouched.
- The tool attributes fresh-page bytes to stacks, overall and up to the time of the peak. It does not track frees per object: that needs a hook in `eml_rt`'s release path and a mark per sampled object, which is a separate `eml_rt` stage with its gate if the growth view proves insufficient.

## 8. Stages

### 8.1 Changes to `native-checked` (being planned now)

No decision of D-NC-1..D-NC-10 changes. Per-site counters do not belong in it: the audits are verified `eml_rt` operations without a site, and segment counts (7.1) give per-site numbers without touching them. Position tables do not belong in it: they need `ir-origins`.

1. H-4: the descriptor's `class` is a `#[repr(u8)]` enum with fixed discriminants and a `name()`, because profile files store it. The H-4 test pins the discriminants.
2. `lower/audit.rs` emits through the `FnLower`'s builder (no cursor or builder of its own), so the audit calls fall under the statement's point once `ir-origins` sets it.
3. Ordering: `ir-origins` starts after `native-checked` closes. The twin CLIF pins that `native-checked` adds stay byte-identical through `ir-origins` (5.4: the dump clears source locations).

### 8.2 Changes to other planned stages

| Stage | Change |
|---|---|
| `sched-core` | SC-5 adds `Slot.phase: AtomicU8` (H-8) beside `Slot.current`: `Run`, `Idle`, `Io`, `Join`, `Park`, written where `drive` opens and closes an item and where the worker blocks; its test reads it from another thread, like H-2's. runtime.md 「信頼の一覧」 is defined with two groups (model assumptions, memory-safety assumptions) |
| `jit-tasks` | No new hook: the sampler's registry holds the thread handle, written by the registering thread (6.2), so r2's `Slot.thread` (H-9) is dropped. Every new lowering (the `par` sequence, `ACTION_RAISE`, the atomic rows, the task rows) sets its role through `lower/origin.rs`. The rows that block synchronously get the `Io` phase guard in `native-prof`, not here. H-1's test and H-3 stay as planned; the sampler measures its own bounds (6.2) and does not read `Slot.stack`. The worker thread bodies get the `prof::registered` call in `native-prof` unit 3 |
| `jit-repl` | D-JR-1's debug-build comparison of same-name code compares code and, re-keyed (files by path, sources by key), origins; it never compares raw `Loc.file` or `SourceId`. The session's machine keeps each load's tables until the session ends. `:prof <input>` is a later item |
| `vm-retire` | the VM's position table goes; the roadmap item 「ネイティブのコードの位置」 is renamed `native-backtrace` and keeps only "the fault path records return addresses; the report prints `at f (file:l:c)` lines and the nearest user frame", read from the tables of 5.4; it decides whether tables are always built and whether `Extern.at` merges into `Stmt.at` |
| `native-aot` | public symbols unchanged; the frame-pointer flag of 6.8 is in `eml_rtlib`'s build; the generated `main()` is where `prof-aot` adds the `EML_PROF` start |
| `native-inline` | every inline access it adds takes its role through `lower/origin.rs` |
| 01-design 15 | D-MT-1 is replaced by section 5; H-7 is unchanged; 15.3's `native-prof` and `native-lines` rows are replaced by 8.3; "macOS: suspend and read" is replaced by 6.3 (D-3); Windows is the later item `prof-windows` (D-4) |

### 8.3 The new stages and their place in 「段の列」

Order: `native-checked`, `ir-origins`, `sched-core`, `jit-tasks`, `native-prof`, `jit-repl`, `vm-retire`, `sched-proofs`, `par-direct`, `native-aot`, `native-inline`. Later items with prerequisites: `prof-counts` (`native-prof`), `prof-tasks` (`native-prof`), `prof-heap` (`native-prof`), `prof-windows` (`native-prof`; its own design and soundness review, 6.6), `native-backtrace` (`vm-retire`), `prof-aot` (`native-aot`, `native-prof`), `perf` map (unchanged).

**`ir-origins`** (prerequisite: none; placed after `native-checked` by 8.1 item 3). Units:

1. Plan; baseline (`bench/native.sh` load rows, the IR and CLIF snapshot hashes).
2. `eml_lir`: `At`, `Scope`, `Source`, `Stmt { at, kind }`, `term_at`, both printers, `parse`, O-1 and O-2 in the verifiers; the mechanical adaptation of every crate with `At::NONE` everywhere (kind 3: every snapshot byte-identical).
3. translate: sources, root scopes, the builder's current `At`, O-4 as its own check, O-6.
4. optimize: `specialize`, `known`, `fuse` (5.3's rows), O-3 on.
5. optimize: `handlers` (clause expansion, path copies, continuation and join-block functions, clause copies).
6. evidence passing.
7. The LIR passes, with the copy forms of `$uniform` and `$owned`.
8. `lower/origin.rs`, roles, the encoded tables, `origins: true`, `NativeMachine::points`, O-5, the placement test.
9. Close: docs/spec/lir.md 「出どころ」 (types, O-1..O-6, the pass rules), docs/spec/core-ir.md 「位置」, docs/spec/native.md 「位置の表」, architecture.md, CLAUDE.md's invariant "a function's name fixes its code and ABI" gains "and, up to the numbering of files and sources, its origins".

Completion: every IR and CLIF snapshot byte-identical; O-1..O-3 and O-6 on after every pass in debug builds over the whole UI suite, O-4 after translate; O-5 passes; in `points`, a user lambda inlined into a std copy resolves to the lambda's `Source` and line under the std function's scope, a clause expanded by handler specialisation resolves to a `Clause` source under the performing line, a continuation function (`f$kN` and `f{H}$hN`) and an `$owned` copy resolve to the original source, a `dup` resolves to role `Rc`; encoded tables at most 0.6 x code bytes on `bench/`; load rows with tables within +3 %.

**`native-prof`** (Linux and macOS; prerequisites: `ir-origins`, `jit-tasks`). Units:

1. Plan; the toolchain check of F-16; `prof/`'s file list and its `trust` test.
2. `walk`, `Words`, `Live` with its assembly load, the ring, the governor; the Miri line.
3. The registry and `prof::registered` in the worker thread bodies, the handler and its installation, the monitor's tick, state samples, the `Io` phase guard around the synchronously blocking rows (listed here).
4. The usage errors (Windows, an owned SIGPROF, the VM) and the deterministic `EINTR` tests.
5. The file, `Load` records and `--prof`.
6. `eml prof report` (function, line, role, worker, `--user`, `--split`), with the rooted, broken and untrusted shares.
7. `eml prof folded` and `eml prof export`.
8. The frame-pointer flag and the x86_64 measurement of broken plus untrusted samples; over 2 % the stage stops and brings the figure to the user (6.8).
9. CI lines (the synchronous tests on Linux x86_64 and aarch64 and on macOS; ASan with the handler tests; TSan with the registry, ring and monitor protocol only).
10. Close: docs/spec/runtime.md 「プロファイルの記録」 and trust rows SM-1, SM-3, SM-4; docs/implementation/profiling.md (the user's guide, with 4.4); benchmarks.md (overhead); the roadmap items.

Completion: the tests of 6.10; on macOS arm64 and x86_64 Linux a profile of each `bench/` program and of the deep `list` probe has at least 98 % rooted and trusted samples; overhead at 1 kHz at most 2 % wall on `bench/fib.em` and `bench/rbtree.em` sized to 2 s and at most 7 % on the deep probe; fast CLIF pins unchanged; `grep -rn 'thread_local' crates/eml_sched/src/prof*` empty; `unsafe` appears in `crates/eml_sched/src/prof/` only in `sys.rs`.

## 9. Rejected alternatives

| Alternative | Reason |
|---|---|
| Positions on calls only (D-MT-1) | section 3.1 |
| Marker statements (a `Stmt::At` that sets the position of what follows, as GHC's source notes) | every pass that matches adjacent statements must skip them; moving a statement across a marker changes its origin silently; a missing marker is not detectable. A field is checked by the type |
| Origins in a side table keyed by block and index | Core IR and LIR statements have no ids; every insertion would renumber |
| Storing the role in the IR | the lowering knows the statement kind and the part it is emitting; a stored role would need a rule per pass for no extra precision |
| Merging `Extern.at` into `Stmt.at` now | the first is behaviour pinned by UI tests, the second is metadata; merging changes error positions of generated code. Left to `native-backtrace`; O-6 keeps them equal meanwhile |
| Fixed-width table rows (8 bytes per row, 24 per point) | 0.35..0.4 x for rows alone and as much again for points; the varint form is the default |
| Always building the tables | nothing on the run path reads them; O-5 makes building on request safe |
| Cost-centre instrumentation, or a shadow stack of source frames | it changes the code being measured, distorts inlining and tail calls (the stack would grow in loops), and costs on every call when off unless there are two builds of everything. The count profile gives the exact numbers in a mode that is already not the fast one |
| A shadow record of tail callers | a store on every tail call in fast code; loops are tail calls |
| External profilers only (`perf`, Instruments, samply with a perf map or jitdump) | they cannot expand inlined scopes or roles, know nothing of tasks and worker phases, need a symbolised binary, and differ per OS. The `perf` map item stays as a cheap extra |
| DWARF as the table format | no writer for Windows, no reader in the tool, and inlined-subroutine entries are larger. 「AOTのデバッグ情報」 can still be generated from 5.4's tables later, now with inlining |
| `timer_create` per thread on Linux (CPU-time sampling) | a second unix mechanism; macOS has none; worker phases already separate running from waiting. The price is 4.4's first limit (D-3) |
| Suspend-and-read on macOS (the roadmap's text) | it reads another thread's stack where a signal reads its own, and needs Mach thread ports; one unix mechanism is verified (F-5). D-3 |
| A stack cache to shorten deep walks (stop at a record seen in the previous sample) | a record with the same saved `fp` and return address can have a different chain above it (the same recursion entered from another call site), so the cached root can be wrong. The full walk costs 1 ns per record and the governor bounds it |
| Storing the outermost `eml_enter` record in the registry (review S-4, second option) | it decides `rooted` without the walk but does not give the root frames (the callers of the recursion), which inclusive time needs |
| A walk limit of 1,024 records (r1) | F-8 and F-18: it loses the root of ordinary list code |
| An unwinder on unix (`.eh_frame`) instead of frame pointers | not async-signal-safe, needs unwind tables registered for JIT code, and F-4 already pays for frame pointers |
| The Windows sampler inside `native-prof` (r2's 6.6: copy the stack, then `RtlVirtualUnwind` over the copy) | `RtlVirtualUnwind` reads wherever the context and the unwind codes point, outside the copy; a truncated copy, an untranslated frame register or a garbage context make it read out of bounds or the live stack. Nothing was verified on a machine. It becomes `prof-windows` with an in-profiler unwind-code decoder as the design to evaluate (6.6) |
| A lookup of the registry entry by stack pointer (r1, r2) | the handler takes no lock, so it can read `lo` of one registration and `hi` of the next in the same slot and use another thread's ring and bounds. The thread key cannot be torn that way (6.2). A seqlock per entry would also work but keeps the dependence on every entry's `lo` |
| A registration guard value (r2) | it can be forgotten or outlive the `Slot` and ring it points to; the scoped function cannot |
| `read_volatile` through an exposed-provenance pointer for the walk's loads (r2) | Rust's model calls a racing or uninitialised read undefined, and the sanitizers instrument it; one assembly load has machine semantics |
| Cooperative sampling at safepoints and entries (no signal, no trust row) | it sees no time inside runtime entries and C calls, and its samples sit only at function entries. It is the fallback if the user declines the trust rows (D-3) |
| A heuristic stack scan when the chain breaks | stale frame records of returned calls chain to the same root and give plausible wrong stacks. The exit anchor of 6.8 is exact and is taken on measurement |
| A phase or marker around direct C calls, to skip the signal there | two stores per C call in fast code; `blocking` is the declared way to call C that waits |
| Sampling pool threads in `native-prof` | their samples need the job's tag to mean anything, and the tag arrives with `prof-tasks`' events |
| Sampling allocation in the fast entries, or a third entry table | a flag test in every allocation, or a third copy of the runtime's entries; checked mode already has a table that is not on the fast path |
| Per-worker counter arrays for the count profile | a deterministic profile is taken on one worker; N arrays and their merge buy nothing |
| pprof output | protobuf writer, no timeline, no per-thread phases or markers |
| Anything for the VM | the VM is leaving (`vm-retire`); native speed and one engine come first |
| A built-in UI | text, folded stacks and one exported format are enough |

## 10. Risks

| Risk | Default |
|---|---|
| A pass cannot keep O-3 (a statement built from parts of two scopes, for example fused constructors) | the statement takes the consumer's `At` (R-ins); O-3 still holds because the `At` is copied whole. If a pass must synthesise a position, it uses a position-less `At` in the right scope (allowed after translate, O-4) |
| A sub-pass or function form missing from 5.3 | unit 4 to 7 each start by listing the statement-creating sites of their files (`grep -n 'Stmt\b.*{' `, `push`, `insert`) against 5.3; a site without a rule stops the unit for a plan amendment. O-1 and O-3 catch a cloned statement whose table was not cloned |
| `Stmt { at, kind }` is a large mechanical diff across concurrent work | `ir-origins` runs alone, directly after `native-checked`; unit 2 is one commit with byte-identical snapshots |
| Source locations change Cranelift's output on some program (F-17 covers ten) | O-5 is a test in the ordinary run; if it fails, `--prof` profiles code compiled with locations and says so, and the stage records the difference |
| Table size over 0.6 x | drop the per-call points for eml calls (the callee is then read from the statement's point), which removes the computed excess over the measured 0.3 x |
| `SIGPROF` changes behaviour of a direct C call (4.4) | documented; 6.10 has a deterministic `EINTR` test and the `#[ignore]` stress run |
| Rust runtime frames lack frame pointers on x86_64 Linux despite the flag (prebuilt std) | F-16 is checked in unit 1; if std has none, the figure of 6.8 goes to the user with the exit anchor extended to runtime entries of class `Row` and `Task` as the proposal |
| An assembly load in `eml_sched` is read as breaking "no asm but the stubs" | that rule (G-10) names `eml_nrt` and `eml_native`; `prof/sys.rs`'s one load is listed in the `trust` test |
| A libc on a supported target implements `pthread_self` or the clock through lazy binding or a lock | SM-4; the calls are made once at installation; `prof-aot` re-checks for static linking |
| `ir-origins` delays `sched-core` by one stage | D-2 |

## 11. Decisions that are the user's

Four decisions change something already approved or add trust to the runtime; the rest are defaults this design takes and states.

| # | Decision | Options | Recommendation |
|---|---|---|---|
| D-1 | Reverse D-MT-1 / A-37 (approved under UJ-17): origins on every statement of both IRs | (a) section 5; (b) keep positions on calls and accept machine-symbol profiles | (a); F-17 shows it costs no code change |
| D-2 | The stage order: `ir-origins` directly after `native-checked` (it delays `sched-core` by one stage), and `native-prof` directly after `jit-tasks` (the roadmap has the profiler after `native-aot`) | (a) as stated; (b) `ir-origins` merged into `native-prof` after `jit-tasks`; (c) the profiler after `vm-retire` or `native-aot` | (a): the mechanical diff grows with every stage that adds lowering code, and `par-direct` and `native-inline` start from measurements |
| D-3 | The sampling mechanism on Linux and macOS. The roadmap text approved under UJ-17 has macOS suspend and read from the monitor thread | (a) a directed signal from the monitor's tick, as section 6: it adds trust rows SM-1, SM-3, SM-4 to the runtime closure that shipped apps link under `prof-aot`, so "no safe function reaches undefined behaviour" then holds modulo that group; time is wall-clock time of a worker in phase `Run`, not CPU time; (b) suspend and read on macOS: the monitor reads another thread's stack; (c) cooperative sampling at safepoints and entries: no trust row, no time inside entries and C calls | (a): the handler reads only its own stack through one bounds-checked assembly load, one mechanism for both systems, verified on macOS here and reviewed for soundness |
| D-4 | Windows | (a) `eml run --prof` is a usage error on Windows until the later item `prof-windows`, with its own design and soundness review; (b) do it inside `native-prof` with an in-profiler decoder of the x64 unwind codes | (a): the path is unverified on a machine and is the only part with foreign reads the profiler does not bound |

Defaults taken (say so if any should be a decision):

- The count profile (`prof-counts`), task events (`prof-tasks`) and the heap profile (`prof-heap`) are later items after `native-prof`.
- `blocking` C calls and pool jobs are not sampled until `prof-tasks` (4.4).
- The rich export format is Firefox Profiler JSON; pprof is not written.
- The workspace sets `force-frame-pointers` on x86_64 Linux for every build.
- Whether shipped apps contain the sampler (only with `eml build --prof`, or always) is decided in `prof-aot`'s design; this design assumes the first.
- The exit anchor (a new `Native` field in `eml_rt` and a store before direct C calls) is not taken by `native-prof`; if the broken plus untrusted share on x86_64 Linux is over 2 %, the stage stops and brings the measurement to the user (6.8).

## 12. Additional checks

- Prototype (scratch worktree, removed twice; the patch has both rounds): a source location per LIR statement and per entry call with switches to turn either off; counters, an FNV-1a hash of every function's code bytes and the varint size in `lower/mod.rs::define`; a `SIGPROF` handler in `eml_native` signalled by `pthread_kill` from a timer thread, walking from the `ucontext`'s `fp` within `[sp, pthread_get_stackaddr_np)` to the root, timing the walk, and counting the `lr` case. Runs: `eml run --native` (dev build, one worker) on probes `fib` (n = 41, 43), `rbtree` (600,000 inserts), `list` (60 rounds), `fold_state`, and on the `bench/` programs named in F-9 and F-17. Results are F-5..F-10 and F-17..F-19.
- One run of the first round on `bench/fib.em` (1 ms of run time) started while the other planner's Verus run was active, before the gate script was in place; every later run waited for a machine with no test binary, Verus, Miri or eml run active. Some runs gave up after the wait (`fannkuch_var` without locations, `rbtree` at 1 kHz in the second round) and were not repeated.
- Not verified here: Linux and Windows (no machine); x86_64 frame pointers in the pinned std (F-16); everything about Windows (6.6); pointer authentication in return addresses on aarch64 Linux; the [I] claims of the soundness review that 6.3 and 6.7 rely on (async-signal safety of `pthread_self` and the clock functions, TSan's late delivery, the mapping of main-thread stacks); that the Firefox Profiler UI upgrades a pinned processed-profile version. Section 8.1 is stated against the design-level plan `docs/plans/2026-11-17-native-checked.md`; that stage's executable plan takes its two requests.
- Comparable systems consulted from knowledge, not fetched [I]: Go (per-function inline tables and varint pc tables, `SIGPROF`, a 64-frame limit), OCaml 5 (frame pointers and `perf`; memprof), GHC (cost centres; source notes and the info-table map, whose interference with optimisation is why section 9 rejects marker statements), the JVM's async-profiler and inlining scope descriptors (the model for scopes), Chromium's stack sampler on Windows (copy the stack, then unwind the copy: 6.6), Wasmtime's guest profiler (Firefox Profiler output). Koka and Lean 4 rely on the C toolchain's debug information.

## Appendix A. Response to review r1

The "Where" column names r2's sections and decision numbers. In r3, U-1 is D-1, U-2 and U-3 are D-2, U-8 and U-9 are D-3, U-11 is D-4, and the others are the defaults of section 11; S-9's Windows design is superseded by Appendix B's B-3.

| Finding | Action | Where |
|---|---|---|
| B-1 handler specialisation has no rule | Accepted. Rules for clause expansion (R-inline at the `perform`), path copies, `f{H}$hN` / `f{H}$hjJ`, `<g>{k}` copies, the `return` clause, state threading; every sub-pass of `optimize()` is a row. One difference from the proposed fix: the jump that replaces `resume` takes the `resume`'s `At`, not the `perform`'s (it is the statement replaced, R-term) | F-2, F-3, 5.3, 4.3, 8.3 unit 5 and the completion line, risk row 2 |
| S-1 `$uniform` / `$owned` | Accepted. Copy forms are clones; only the thin wrapper is position-less; `$owned` is in the borrow row with the demotion rule | F-20, 5.3 LIR table, 4.1 |
| S-2 REPL re-keying | Accepted. Indices are per program and per load; each load's tables are self-contained with `Image.files`; the tool merges by key; O-5's wording is "up to the numbering"; a `jit-repl` row | F-21, 5.1, 5.2 O-5, 5.4, 4.3, 8.2 |
| S-3 `blocking` and pool threads | Accepted, second option: not sampled in `native-prof`, shown as waits and sampled with a job tag from `prof-tasks`; phase `Blocking` dropped; a stated default | F-22, 2.1, 4.4, 6.3, 7.2, 8.2, section 11 |
| S-4 deep recursion | Accepted, first option, measured: the walk continues to the root reading saved `fp`s (1 ns per record), keeps a root part, and a governor bounds walking at 5 %. The second option is rejected with a reason | F-18, 6.4, 2.3, 8.3 completion, section 9 |
| S-5 table size | Accepted. Varint pairs are the default; points, scopes and sources are counted; a computed upper bound; tables only on request | F-9, 2.3, 5.4, section 9 |
| S-6 phase race, `EINTR`, synchronous reads | Accepted. The handler records the phase it reads; synchronously blocking rows set `Io`; time is wall-clock per `Run` phase; `EINTR` in direct C calls is documented and stress-tested. The exit marker for direct C calls is rejected (two stores per call) | 6.3, 4.4, 6.10, section 9, U-9 |
| S-7 Miri seam | Accepted. `walk` is safe over `Words`; `Live` is the only unsafe reader; Miri uses a slice reader | 6.4, 6.7, 6.10 |
| S-8 registration | Accepted. An RAII `!Send` guard that measures and registers on its own thread and unregisters in `Drop`; the precondition is `[max(sp, lo), hi)`; entries carry a run id | 6.2, 6.7 |
| S-9 Windows | Accepted [I]. Copy while suspended, resume, unwind the copy (unwind data for image code, frame chain for generated code, the stub by its layout); checked first in the stage; a user decision if it fails | 6.6, 6.8, 8.3 unit 1 and 4, U-11 |
| S-10 aarch64 `lr`, PAC | Accepted. `lr` is recorded and used by the tool under a stated rule (the case was observed, F-19); addresses are masked in the tool; repeat counts are a separate array | F-19, 6.4 |
| S-11 file after an abnormal end | Accepted. `Load` records at load, a flush every 100 ms, self-delimiting records | 6.9, 2.2, 6.10 |
| S-12 buried decisions | Accepted. U-8 (macOS), U-9 (wall or CPU), U-10 (exit anchor, recommended to come back to the user); the soundness review is on this document, before the roadmap changes; the trust list gets a memory-safety group with SM-1..SM-3 | 0 item 8, 6.1, 6.7, section 11 |
| S-13 count profile | Accepted. The relation with reuse hits and helper allocations; one worker only (per-worker arrays rejected); segments end at unwinding points | 2.4, 7.1, 2.2, section 9 |
| S-14 O-4 placement | Accepted. O-4 is its own check after translate only | 5.2, 8.3 unit 3 |
| N-1 SIGUSR2 | Accepted | F-13 |
| N-2 baseline | Accepted: measured with a baseline | F-6 |
| N-3 O-5 untested | Accepted: verified on ten programs | F-17, 5.2 |
| N-4 rustflags | Accepted | 6.8 |
| N-5 `jit-tasks` lowering roles | Accepted | 8.2 |
| N-6 test hook on std | Accepted: a user `extern "C"` function | 6.10 |
| N-7 merged instructions | Accepted | 4.4 |
| N-8 reused task word | Accepted: `--by task` moves to `prof-tasks`, which gives tasks ids | 2.1, 4.4, 7.2 |
| N-9 headroom, `errno` | Accepted | 6.3 |
| N-10 Firefox format version | Accepted: pinned version, Gecko format as the fallback | 2.2 |
| N-11 two positions on externs | Accepted: O-6 | 5.2 |

## Appendix B. Response to the soundness review (on r2's section 6)

Every fix is the reviewer's unless the row says otherwise. No finding is rejected.

| Finding | Action | Where |
|---|---|---|
| B-1 a lookup by stack pointer can match a torn entry | Accepted, fix 1. The entry's key is the registering thread's `pthread_t`, stored last with `Release` and cleared first; the handler matches its own `pthread_self()` with `Acquire` and then requires `lo <= sp < hi`. The store order and orderings are stated. The seqlock is rejected (it keeps the dependence on every entry's `lo`) | 6.2, 6.3 step 3, 6.7 items 3 to 5, section 9 |
| B-2 the thread handle is outside what registration establishes | Accepted. The entry holds the handle, written by the registering thread; the monitor reads it from the entry under the mutex; H-9 (`Slot.thread`) is dropped from `jit-tasks`. With B-1 it is the same field as the key | 6.2, 6.3, 6.7 item 5, 8.2 |
| B-3 Windows: `RtlVirtualUnwind` over a copy is not bounded | Accepted, option 2. Windows leaves `native-prof`; `--prof` is a usage error there until the later item `prof-windows` with its own design and soundness review. 6.6 keeps the reviewer's constraints (duplicated handle, the `Rsp` range test, the panic-free suspended section, no `RtlVirtualUnwind`) and names the in-profiler unwind-code decoder as the design to evaluate. SM-2 goes with it | 0, 2.2, 6.6, 8.3, section 9, D-4 |
| S-1 `Live::new`'s contract; SM-1 | Accepted. The contract is "mapped and readable"; the load is one inline-assembly load; SM-1 is the reviewer's wording. One addition: the `trust` test lists that load as `eml_sched`'s only `asm!`, and a risk row answers the "no asm but the stubs" rule | 6.1, 6.4, 6.7 items 3 and 7, section 10 |
| S-2 the guard's borrows; forgetting | Accepted. `prof::registered(run, &slot, &ring, body)` replaces the guard; `Slot.phase` is read under the registry mutex | 6.2, 6.3, 6.7 table, 8.2, 8.3 unit 3 |
| S-3 TSan delivers signals late | Accepted. Item 1 is "a stack pointer this thread has had at or after registration"; the TSan line checks the registry, ring and monitor protocol with synchronous signals only | 6.7 items 1 and 8, 8.3 unit 9 |
| S-4 wrong but not `Broken` chains on x86_64 | Accepted. The tool's `trusted` test; the report prints broken and untrusted; their sum is the 2 % figure | 6.5, 6.8, 4.4, 8.3 units 6 and 8 and the completion line |
| S-5 the signal's process-wide state | Accepted, all five: empty `sa_mask`; SIGPROF unblocked at registration; refusal when SIGPROF is owned, and the warning about user C code resetting it; libc functions called once at installation; one clock per OS | 6.3, 2.2, 4.4, 6.10 |
| S-6 trust rows and where `unsafe` lives | Accepted. SM-4 added; SM-3 restated for the identity key; `unsafe` only in `prof/sys.rs`, the other files forbid it, a `trust` test pins the list; "the only `unsafe` item is `Live::new`" is withdrawn | 6.1, 6.7 item 7, 8.3 unit 1 and the completion line |
| S-7 what choosing the signal accepts | Accepted. D-3 states the trust rows and the cooperative alternative | section 11, section 9 |
| N-1 the handler's stack use | Accepted: under 16 KiB; the direct-C headroom case is a stated limit | 6.3, 4.4 |
| N-2 mutex poisoning | Accepted | 6.2, 6.10 |
| N-3 the address width for masking | Accepted: in `Header` | 6.4, 6.9 |
| N-4 the `Io` guard and `eml_rt` | Accepted: the guard wraps the whole row in `eml_nrt` / `eml_rows`; unit 3 lists the rows | 6.3, 8.3 unit 3 |
| N-5 a deterministic `EINTR` test | Accepted | 6.10, 8.3 unit 4 |
| N-6 registration under Miri | Accepted: `body` runs unregistered | 6.2, 6.7 item 8 |
| N-7 the monitor's wait | Accepted: `--prof-hz` is clamped to 1,000 and the effective rate is stated | 6.3, 2.2 |


## User decisions of 2026-11-22

- D-1: origins on every statement of both IRs (reverses D-MT-1 / A-37) — accepted.
- D-2: `ir-origins` directly after `native-checked`; `native-prof` directly after `jit-tasks` — accepted.
- D-3: directed SIGPROF with the handler walking its own stack, wall-clock time of a running worker, trust rows SM-1, SM-3, SM-4; replaces suspend-and-read on macOS — accepted.
- D-4: Windows `--prof` is a later item `prof-windows` with its own design and soundness review — accepted.
