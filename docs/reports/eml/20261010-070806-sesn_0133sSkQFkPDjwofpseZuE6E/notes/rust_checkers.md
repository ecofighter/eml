# Rust tools for verifying unsafe concurrent runtime code (loom, shuttle, Miri, TSan, ownership debug checks, DST) — for eml_rt

Convention: "Cited Findings" are things read directly in the linked primary source (READMEs, docs, source files, CI scripts, papers) during this session (2026-10-10). "Inferences" are my reasoning applied to eml_rt's planned RC scheme (signed AtomicI32: >0 thread-local with Relaxed load/store, <0 shared with fetch_sub(AcqRel), 0 immortal; sender flips the sign before publishing).

## 1. loom: model, code structure, limitations, users

### Takeaway
loom is a bounded, (near-)exhaustive stateless model checker over the C11 memory model that only sees operations routed through its replacement types (`loom::sync::atomic`, `loom::cell::UnsafeCell`, `loom::alloc`); it is the right tool for a small RC/handoff core, but it needs a `cfg(loom)` shim layer, a preemption bound of ~2-3, and it does not model all of C11 (SeqCst accesses treated as AcqRel, no load buffering).

### Cited Findings
- Model: loom runs a test closure many times, "permuting the possible concurrent executions of that test under the C11 memory model", using state-reduction techniques; docs cite CDSChecker as the basis — [loom README](https://github.com/tokio-rs/loom); [loom docs.rs](https://docs.rs/loom/latest/loom/)
- Not full C11: README says loom "does not implement the full C11 memory model"; SeqCst *accesses* "are regarded as AcqRel" (can produce false alarms, issue #180); `fence(SeqCst)` is supported; loom "does not explore some executions that are possible in the C11 memory model" (load buffering; see `tests/litmus.rs`), so it is not sound — [loom README](https://github.com/tokio-rs/loom)
- Relaxed: docs say Relaxed "allows particularly strange executions" and loom cannot emulate a later operation in a thread appearing before an earlier one — [loom docs.rs](https://docs.rs/loom/latest/loom/)
- Intrusive: "Any code that does not use loom's replacement types is invisible to loom." Tests must be deterministic; other nondeterminism (RNG, syscalls) must be mocked — [loom docs.rs](https://docs.rs/loom/latest/loom/)
- Structure: put loom under `[target.'cfg(loom)'.dependencies]`, re-export `loom::sync::atomic::*` vs `std::sync::atomic::*` behind `#[cfg(loom)]`/`#[cfg(not(loom))]`; loom's `UnsafeCell` has `with`/`with_mut` (closure-scoped access) instead of `get`, so non-loom builds need a shim wrapping `std::cell::UnsafeCell`; spin loops must call `loom::thread::yield_now` because the scheduler is not fair; run with `RUSTFLAGS="--cfg loom"` and `--release` — [loom docs.rs](https://docs.rs/loom/latest/loom/)
- UnsafeCell tracking: `with`/`with_mut` "track the start and end of the access"; `get()`/`get_mut()` return `ConstPtr`/`MutPtr` guards, and conflicting access while a guard lives panics as "not valid under the Rust memory model" — [loom::cell::UnsafeCell](https://docs.rs/loom/latest/loom/cell/struct.UnsafeCell.html). The runtime panics with "Causality violation: Concurrent read and write accesses to `UnsafeCell`." / "Concurrent write accesses" (vector-clock causality check) — [loom src/rt/cell.rs](https://github.com/tokio-rs/loom/blob/master/src/rt/cell.rs)
- Leak tracking: `loom::alloc::alloc` is std alloc "with the addition of leak tracking"; "Loom's leak tracking will not function for allocations not performed via this method" — [loom src/alloc.rs](https://github.com/tokio-rs/loom/blob/master/src/alloc.rs)
- State explosion: bound with `LOOM_MAX_PREEMPTIONS` / `Builder::preemption_bound`; "setting the thread pre-emption bound to 2 or 3 is enough to catch most bugs"; thread count capped (`MAX_THREADS`). Debugging: `LOOM_CHECKPOINT_FILE` + `LOOM_CHECKPOINT_INTERVAL=1` replays the failing permutation; `LOOM_LOG` marks every thread switch; `LOOM_LOCATION` adds locations to panics — [loom docs.rs](https://docs.rs/loom/latest/loom/)
- Users: tokio wraps all sync primitives in `tokio/src/loom/mod.rs` (`#[cfg(all(test, loom))] mod mocked` vs `mod std`) and runs a dedicated loom CI workflow with `RUSTFLAGS: --cfg loom ... -C debug_assertions`, `LOOM_MAX_PREEMPTIONS: 2`, sharded into scopes (`loom_multi_thread::group_a..d`, `sync::tests`, `runtime::time`, ...) — [tokio loom/mod.rs](https://github.com/tokio-rs/tokio/blob/master/tokio/src/loom/mod.rs); [tokio .github/workflows/loom.yml](https://github.com/tokio-rs/tokio/blob/master/.github/workflows/loom.yml)
- crossbeam-epoch runs loom in CI with `--cfg crossbeam_loom`, `LOOM_MAX_PREEMPTIONS=2`; a script comment says the tests take ~11 minutes at 2 preemptions and several times longer at 3 — [crossbeam ci/crossbeam-epoch-loom.sh](https://github.com/crossbeam-rs/crossbeam/blob/master/ci/crossbeam-epoch-loom.sh)
- AWS ShardStore (S3) uses loom "to soundly check all interleavings of small, correctness-critical code such as custom concurrency primitives" and caught an LSM-tree bug "before even reaching code review" — [ShardStore SOSP'21 paper](https://www.cs.utexas.edu/~bornholt/papers/shardstore-sosp21.pdf)

### Inferences
- eml_rt's planned orderings (Relaxed load/store on the local path, AcqRel RMW on the shared path, no SeqCst accesses needed, no fences needed) sit in the part of C11 that loom models well; avoid SeqCst *accesses* in the core (they would be weakened to AcqRel under loom anyway).
- To make loom useful for eml_rt: route the RC word through a `cfg(loom)` `AtomicI32` alias, allocate objects via a `cfg(loom)` alias of `loom::alloc` (leak detection), and model object *fields* as `loom::cell::UnsafeCell` (or a field-access shim that calls `with`/`with_mut`) so that an unsynchronized field read/write across threads panics with "Causality violation". Without the field shim, loom only sees the RC word, and a forgotten mark-shared would show up only as a wrong final count / leak / double free, not as a race.
- Good loom targets for eml_rt: (a) mark-shared + publish via channel/queue + concurrent dup/decref on both sides + last-decref free; (b) the join protocol of a `par` fork/join (owner pops vs thief steals); (c) linear-continuation handoff between workers. Keep each model to 2-3 threads and a handful of objects.

### Gaps
- I did not read loom's source for exactly how it represents Relaxed loads (which stale values it can return); docs only say it is partial.
- I did not confirm whether loom's "load buffering" omission affects any RC-style pattern in practice (it is generally considered harmless for RC, but that is not sourced here).

## 2. shuttle (AWS): randomized/PCT scheduling vs loom

### Takeaway
shuttle trades soundness for scale: randomized and PCT schedulers (plus bounded DFS and replay) over its own sync/thread/tokio wrappers, but it models every atomic as SeqCst, so it cannot find weak-memory bugs; use loom for the small atomic core and shuttle for larger end-to-end scheduler/VM tests.

### Cited Findings
- "Shuttle is a library for testing concurrent Rust code" implementing randomized concurrency testing including PCT ("A Randomized Scheduler with Probabilistic Guarantees of Finding Bugs"); wrappers for `std::sync`, `tokio`, `rand`, `std::collections`; "Shuttle is not sound" but "scales to much larger test cases than Loom" — [shuttle README](https://github.com/awslabs/shuttle)
- Entry points: `check_random`, `check_random_with_seed`, `check_urw` (uniform random), `check_pct` (bug depth + iterations), `check_dfs` (exhaustive DFS, "not recommended" except for very simple programs), `replay` / `replay_from_file` (failing schedule printed as an encoded string), `check_uncontrolled_nondeterminism`, `Runner`, `PortfolioRunner` (several schedulers in parallel), `FailurePersistence` — [shuttle docs.rs](https://docs.rs/shuttle/latest/shuttle/)
- Memory model: "Shuttle does not faithfully model behaviors of relaxed atomic orderings ... **Shuttle models *all* atomic operations as if they were using SeqCst ordering.**" The message-passing-with-Relaxed example "Shuttle cannot find"; it points users at loom for Acquire/Release and partial Relaxed support — [shuttle-std/src/sync/atomic/mod.rs](https://github.com/awslabs/shuttle/blob/main/shuttle-std/src/sync/atomic/mod.rs)
- ShardStore: "sound stateless model checking tools like Loom are not scalable enough to check linearizability of end-to-end" harnesses; "the two tools offer a soundness-scalability trade-off. We use Loom to soundly check all interleavings of small, correctness-critical code ... and Shuttle to randomly check interleavings of larger test harnesses to which Loom does not scale" — [ShardStore SOSP'21](https://www.cs.utexas.edu/~bornholt/papers/shardstore-sosp21.pdf). The paper reports the overall approach prevented 16 issues (including concurrency and crash-consistency bugs) from reaching production — [Amazon Science blog](https://amazon.science/blog/aws-team-wins-best-paper-award-for-work-on-automated-reasoning)

### Inferences
- shuttle cannot detect eml_rt's interesting RC bugs that depend on weak orderings (e.g., using Relaxed where Release is needed on the publish path), because all atomics become SeqCst. It *can* find pure interleaving bugs (e.g., lost update from a non-RMW load/store pair on an object that should have been marked shared), since that bug exists even under SC.
- I did not find a shuttle equivalent of loom's `UnsafeCell` causality checking; so shuttle will not report a non-atomic field race by itself — the bug must surface as an assertion/panic (e.g., an owner-thread assertion, see §5).
- Natural fit for eml: a shuttle-based test of the future work-stealing scheduler and `par` join with many tasks (where loom explodes), with the failing schedule string kept for `replay`.

### Gaps
- No primary source found on whether shuttle tracks happens-before for plain memory (it has a `VectorClock` type internally, but I did not verify any data-race detection feature).

## 3. Miri: data races, weak memory, threads, provenance, speed

### Takeaway
Miri is an interpreter that detects data races (vector clocks, including mixed atomic/non-atomic and mixed-size atomic races), emulates some C++20 weak-memory behaviors via per-location store buffers, randomizes scheduling, and checks provenance/aliasing — but it explores one execution per seed, misses some weak behaviors (load buffering, 2+2W), and is ~3000-7000x slower than native, so it suits unit tests of the eml_rt core, not interpreting real eml programs.

### Cited Findings
- Paper exists: "Miri: Practical Undefined Behavior Detection for Rust", POPL 2026 (Jung, Kimock, Poveda, Sánchez Muñoz, Scherer, Wang), PACMPL Vol. 10 POPL Article 48 — [POPL 2026 page](https://popl26.sigplan.org/details/POPL-2026-popl-research-papers/50/Miri-Practical-Undefined-Behavior-Detection-for-Rust); [PDF](https://research.ralfj.de/papers/2026-popl-miri.pdf). Abstract: "first tool that can find all de-facto Undefined Behavior in deterministic Rust programs"; ran >70% of tests across >100,000 libraries; in CI for std — [blog post](https://www.ralfj.de/blog/2025/12/22/miri.html)
- Data races: vector-clock detector based on Lidbury & Donaldson's C++ race detector (FastTrack-style); reports both conflicting accesses. The aliasing model "does not substitute a data race detector" because it "deliberately does not restrict raw pointers and interior mutable shared references" — [Miri paper §4](https://research.ralfj.de/papers/2026-popl-miri.pdf)
- Mixed atomic/non-atomic: "all locations start out non-atomic; they become atomic when an atomic access (read or write) is performed on them; and they become non-atomic again when a non-atomic write is performed on them. A non-atomic read can be freely mixed with atomic reads in Rust." Unsynchronized mixed-*size* atomic accesses are detected via a per-byte `size` field (novel approach) — [Miri paper §4.2](https://research.ralfj.de/papers/2026-popl-miri.pdf)
- Detection rates (with many seeds): the `static mut` two-thread race example is found in ~90% of executions; the Relaxed message-passing assertion (`X.store(Relaxed); Y.store(Relaxed)` vs `if Y==1 { assert X==1 }`) fails in ~25% of executions — [Miri paper §2](https://research.ralfj.de/papers/2026-popl-miri.pdf)
- Weak memory: per-location store buffer; on an atomic load, picks randomly among permitted values. Limitations: rules out po∪rf cycles (to avoid out-of-thin-air), which "excludes some load buffering behaviors"; cannot generate po∪mo cycles because Miri "still executes all operations in a single, global order" (e.g., 2+2W outcome never produced) — [Miri paper §4.3](https://research.ralfj.de/papers/2026-popl-miri.pdf). README: "Weak memory emulation is not complete: there are legal behaviors that Miri will never produce"; recommends loom for complicated atomic code — [Miri README](https://github.com/rust-lang/miri)
- Weak memory now follows C++20 semantics (adjusted from C++11); two flaws in the core implementation were found and fixed while writing the paper — [Ralf Jung blog 2025-12-22](https://www.ralfj.de/blog/2025/12/22/miri.html)
- Real concurrency bugs found: `once_cell` mishandling spurious `compare_exchange_weak` failures (Miri injects spurious failures, 80% by default); a weak-memory-induced leak in Windows TLS in std (stale load after a release store); a weak-memory ABA bug in std's reentrant lock owner check (address reuse), hit ~5% of runs — [Miri paper §6, Bugs 6-8](https://research.ralfj.de/papers/2026-popl-miri.pdf)
- Scheduling/flags: random scheduling, `-Zmiri-preemption-rate` (default 0.01; 0 disables), `-Zmiri-fixed-schedule` (round-robin, no preemption), `-Zmiri-deterministic-concurrency`, `-Zmiri-seed` (controls allocation addresses, weak CAS failures, store buffering), `-Zmiri-many-seeds` (default 0..64, "can be quite slow"), `-Zmiri-many-seeds-keep-going`, `-Zmiri-track-weak-memory-loads`, `-Zmiri-disable-weak-memory-emulation`; disabling the race detector is unsound and also disables weak memory — [Miri README](https://github.com/rust-lang/miri)
- Provenance/aliasing: `-Zmiri-tree-borrows` ("even more experimental than Stacked Borrows"), `-Zmiri-strict-provenance` (int-to-ptr casts stop execution), `-Zmiri-permissive-provenance` — [Miri README](https://github.com/rust-lang/miri). Tree Borrows errors now "on par with" Stacked Borrows; wildcard provenance added for int-to-ptr code — [blog](https://www.ralfj.de/blog/2025/12/22/miri.html)
- GenMC: experimental integration to enumerate all weak-memory behaviors of loop-bounded programs; "highly experimental, slow, and requires a custom build of Miri" — [blog](https://www.ralfj.de/blog/2025/12/22/miri.html)
- Speed: on the paper's benchmark Miri is "about 3000x slower than the native code" at small n and ~7000x at large n; "too slow for typical fuzzing setups" (Valgrind, for comparison, is 20x-50x) — [Miri paper §5/§7](https://research.ralfj.de/papers/2026-popl-miri.pdf)
- Limits: "Miri tests one of many possible executions of your program"; no FFI/networking in general — [Miri README](https://github.com/rust-lang/miri)

### Inferences
- "Non-atomic access racing with atomic access": yes, Miri reports this (a non-atomic *write* vs an atomic access is a race; per the paper, only non-atomic *reads* may mix with atomic reads). Deallocation counts as a write-like access, so "thread B frees an object while thread A still reads a field" is reported (as a race or use-after-free) if the explored schedule hits it.
- "Relaxed store on an object another thread now owns": if *both* threads use atomic ops on the RC word (eml's local path uses `AtomicI32` Relaxed load/store), this is **not a data race** in the Rust/C++ model, so neither Miri's race detector nor TSan nor loom's race checks flag it directly. The bug manifests only as a lost update → premature free (then UAF/race on fields, which Miri does catch) or a leak (Miri's leak check). Detection is therefore probabilistic and indirect; seed sweeps (`-Zmiri-many-seeds`) plus a nonzero preemption rate matter.
- Perceus-style in-place reuse makes the forgotten-mark bug more detectable: if the owner sees rc==1 (stale) and destructively updates fields non-atomically while another thread reads them, that is a non-atomic race Miri/TSan/loom report.
- Practical Miri use for eml_rt: run the `unsafe` core's unit tests and small multi-thread RC/handoff tests under `cargo miri test` with `-Zmiri-strict-provenance` (the heap is raw-pointer based; avoid int-to-ptr), optionally a Tree Borrows job, and a many-seeds job for concurrency tests. Running the full VM on eml programs under Miri is possible only for tiny programs given 3000x+ slowdown.

### Gaps
- I did not find a primary statement on Miri's exact handling of SeqCst fences beyond the paper's note that SC fence behavior changed for C++20.

## 4. ThreadSanitizer with Rust

### Takeaway
TSan (`-Zsanitizer=thread`, nightly, rebuild std with `-Zbuild-std`) runs real optimized binaries at ~5-15x slowdown, so it is the tool for running the eml interpreter on real parallel programs; but it does not understand `atomic::fence` (std's `Arc` swaps the fence for an acquire load under TSan) and, like Miri, it does not report atomic-vs-atomic logic bugs.

### Cited Findings
- Targets: aarch64/x86_64 apple-darwin, aarch64/x86_64 linux-gnu, x86_64 freebsd. TSan must be "aware" of all synchronization; "Using it without instrumenting all the program code can lead to false positive reports." "ThreadSanitizer does not support atomic fences `std::sync::atomic::fence`, nor synchronization performed using inline assembly code." Example run: `RUSTFLAGS=-Zsanitizer=thread cargo run -Zbuild-std --target x86_64-unknown-linux-gnu`; "strongly recommended to combine sanitizers with recompiled and instrumented standard library" — [Rust unstable book: sanitizer](https://doc.rust-lang.org/beta/unstable-book/compiler-flags/sanitizer.html)
- std's `Arc` uses `atomic::fence(Acquire)` normally but under `#[cfg(sanitize = "thread")]` uses `$x.load(Acquire)` instead: "ThreadSanitizer does not support memory fences. To avoid false positive reports in Arc / Weak implementation use atomic loads for synchronization instead." — [library/alloc/src/rcs/arc.rs](https://github.com/rust-lang/rust/blob/main/library/alloc/src/rcs/arc.rs)
- Overhead "about 5x-15x" slowdown and "5x-10x" memory; requires all code instrumented or it "may fail to detect races or may report false positives"; `report_atomic_races` controls "races between atomic and plain memory accesses"; `history_size` 0-7 — [Clang ThreadSanitizer docs](https://clang.llvm.org/docs/ThreadSanitizer.html)
- crossbeam runs `cargo test -Z build-std ... --target x86_64-unknown-linux-gnutsan` with `TSAN_OPTIONS="enable_adaptive_delay=1 suppressions=ci/tsan"`, `--release`, `--test-threads=1`, and a `--cfg crossbeam_sanitize` flag; its suppression file silences `crossbeam_deque*push`/`*steal` ("such data races are safe ... the value read by `steal` is forgotten and the steal operation is then retried") and AtomicCell's SeqLock (uses fences) — [crossbeam ci/san.sh](https://github.com/crossbeam-rs/crossbeam/blob/master/ci/san.sh); [crossbeam ci/tsan](https://github.com/crossbeam-rs/crossbeam/blob/master/ci/tsan)
- Lean 4's runtime (same sign-encoded RC as eml's plan: `m_rc` > 0 single-threaded, < 0 multi-threaded, == 0 persistent) switches `m_rc` accesses to SeqCst atomics under TSan "so that the otherwise non-atomic single-threaded fast paths are not flagged as data races" — [lean4 src/include/lean/lean.h](https://github.com/leanprover/lean4/blob/master/src/include/lean/lean.h)
- Miri paper: sanitizers "are not able to exhibit rare non-deterministic behavior akin to Miri's weak memory implementation", and some UB can be compiled away before LLVM-level instrumentation — [Miri paper §7](https://research.ralfj.de/papers/2026-popl-miri.pdf)

### Inferences
- eml_rt's design (AcqRel `fetch_sub` on each shared decref instead of Release + `fence(Acquire)`) is TSan-friendly as is; if a fence-based last-decref optimization is adopted later, copy std's `cfg(sanitize = "thread")` trick.
- Because eml's local path already uses `AtomicI32` (Relaxed), TSan will treat the RC word as atomic and never flag a forgotten mark-shared on the RC word itself (same blind spot Lean creates deliberately). TSan will flag the *consequences* on non-atomic field accesses (destructive reuse, free vs read), which is valuable when running whole eml programs under `par`.
- TSan is x86_64/aarch64 Linux/macOS only and needs nightly + `-Zbuild-std`; developer machine is macOS (aarch64-apple-darwin is supported). Run in `--release` with debug assertions to keep runtime acceptable.

### Gaps
- No primary source measured TSan overhead specifically for Rust interpreters.

## 5. Ownership debug checks (owner thread ID assertions) in runtimes

### Takeaway
The closest precedents to "detect an unmarked object touched from a non-owner thread" are CPython's biased RC (`ob_tid` owner field), Lean's `lean_mark_mt` sign-flip traversal (same scheme as eml), and Rust's `fragile` crate (owner-thread check that panics); none of the race tools catch the forgotten mark directly, so a debug-mode owner check in eml_rt's header is the recommended primary defense.

### Cited Findings
- CPython free-threading (PEP 703) biased RC: header has `ob_tid` (owner; 0 = unowned), `ob_ref_local`, `ob_ref_shared`; "Reference counting operations from the owning thread use non-atomic instructions"; "Other threads use atomic instructions to modify a 'shared' reference count"; when the owner has terminated, the acting thread merges counts; immortal objects set `ob_ref_local = UINT32_MAX` and `Py_INCREF`/`Py_DECREF` become no-ops; "the implementation should use 'relaxed atomics' to access `ob_tid` and `ob_ref_local`" — [PEP 703](https://peps.python.org/pep-0703/)
- Lean 4: `m_rc` encodes single-threaded (>0), multi-threaded (<0), persistent (==0); MT increments use `atomic_fetch_sub(..., relaxed)` because counts are stored negated; `lean_mark_mt` walks the object graph with an explicit worklist, flipping every still-unshared object to negative and stopping at already-shared ones; overflow lands in a "sticky" frozen range; a Lean file `tests/elab/rc_model.lean` models `incRefN`/`decRef` and proves the count refines an ideal `Nat` count, including arbitrary interleavings of the test-then-RMW halves — [lean.h](https://github.com/leanprover/lean4/blob/master/src/include/lean/lean.h); [object.cpp](https://github.com/leanprover/lean4/blob/master/src/runtime/object.cpp); [rc_model.lean](https://github.com/leanprover/lean4/blob/master/tests/elab/rc_model.lean)
- `fragile` crate: `Fragile<T>`/`Sticky<T>` "use runtime checks to ensure safety"; once sent to another thread `try_get()` errors and `Fragile` panics if dropped in another thread — [fragile README](https://github.com/mitsuhiko/fragile)
- Swift exclusivity enforcement: dynamic checks for "escaping closures, properties of class types, static properties, and global variables", enabled by default in Release builds since Swift 5 with small overhead; the post does not claim cross-thread race detection — [Swift 5 exclusivity blog](https://www.swift.org/blog/swift-5-exclusivity/)

### Inferences
- Recommended design for eml_rt (debug_heap only): add a header word `owner: AtomicU32` (worker/thread id; `SHARED` sentinel), written Relaxed. `alloc` sets owner = current worker; `mark_shared` (the sign flip traversal) sets `SHARED`; an explicit `transfer(obj, to)` (for moving a linear continuation or a unique value to another worker without sharing) rewrites the owner, and the receiver asserts on first touch. In `dup`/`decref`/field write/in-place reuse: if `rc > 0` then `assert_eq!(owner, current_worker)`. This turns the silent lost-update bug into a deterministic panic at the first wrong access, independent of interleaving, and works in plain `cargo test`, loom, shuttle, Miri and TSan runs.
- Make the owner field atomic (Relaxed), otherwise the check itself is a data race that Miri/TSan would report (CPython does the same with `ob_tid`).
- Additional cheap invariant checks: after `mark_shared`, assert every reachable heap child is shared or immortal (Lean's traversal invariant); on publish (channel send / queue push / fork), assert the root is shared or explicitly transferred.
- Lean's `rc_model.lean` is a precedent for proving the RC state machine (sign classes, overflow) separately from the memory-model checking; eml could do the analogous thing with a small loom model plus property tests.

### Gaps
- Koka's runtime (kklib) uses a similar thread-shared RC encoding by reputation, but I could not fetch its header in this session; not verified.
- I found no primary source for an allocator that asserts thread ownership on access (mimalloc has owner-thread free lists, but I did not verify any debug assertion).

## 6. How crossbeam-deque and rayon test themselves

### Takeaway
crossbeam runs Miri (strict provenance, symbolic alignment, Tree Borrows matrix), ASan/MSan/TSan with `-Zbuild-std`, and loom (epoch only); notably the Chase-Lev deque contains a deliberate data race (volatile read/write of buffer slots) that Miri and TSan both flag and that CI works around; rayon's CI runs only ordinary tests and relies on crossbeam-deque.

### Cited Findings
- crossbeam CI jobs: `miri` (matrix `miriflags: ['', '-Zmiri-tree-borrows']`), `careful` (cargo-careful), `san` (asan/tsan/msan targets), `loom` (epoch), plus cross-target tests — [crossbeam ci.yml](https://github.com/crossbeam-rs/crossbeam/blob/master/.github/workflows/ci.yml)
- `ci/miri.sh`: `RUSTFLAGS=-Z randomize-layout`, `MIRIFLAGS=-Zmiri-strict-provenance -Zmiri-symbolic-alignment-check -Zmiri-disable-isolation`; crossbeam-deque runs with `-Zmiri-preemption-rate=0` because "this code technically has UB and Miri catches that", and `-Zmiri-compare-exchange-weak-failure-rate=0.0` for tests that "incorrectly assume that sequential weak CAS will never fail" — [crossbeam ci/miri.sh](https://github.com/crossbeam-rs/crossbeam/blob/master/ci/miri.sh)
- The deque's `Buffer::write`/`read` "might be concurrently called with another `read`/`write` at the same index, which is technically speaking a data race and therefore UB. We should use an atomic store here, but that would be more expensive ... Hence, as a hack, we use a volatile write instead." — [crossbeam-deque src/deque.rs](https://github.com/crossbeam-rs/crossbeam/blob/master/crossbeam-deque/src/deque.rs)
- rayon CI (`ci.yaml`, `main.yaml`, `pr.yaml`) runs `cargo test` for `rayon`/`rayon-core`, i686, wasm/wasi, and `ci/highlander.sh`; no Miri/sanitizer/loom jobs. RELEASES.md mentions fixing "miri-reported UB" and "several Stacked Borrow and provenance issues found by `cargo miri`" — [rayon workflows](https://github.com/rayon-rs/rayon/tree/main/.github/workflows); [rayon RELEASES.md](https://github.com/rayon-rs/rayon/blob/main/RELEASES.md)

### Inferences
- If eml_rt builds its own work-stealing deque rather than using crossbeam-deque, the speculative slot read in Chase-Lev is the place where Miri/TSan will complain; storing slots as `AtomicPtr`/`AtomicUsize` (eml values are word-sized pointers or tagged scalars) avoids the UB and the CI workarounds entirely. Using crossbeam-deque directly inherits its tested-but-technically-UB status and requires the same Miri flags.
- A reasonable eml_rt CI matrix modeled on crossbeam: unit tests; Miri with strict provenance (+ Tree Borrows job, + many-seeds for concurrency tests); TSan with `-Zbuild-std` on the VM running a `par` test corpus; loom on the RC/handoff core with `LOOM_MAX_PREEMPTIONS=2`.

### Gaps
- I did not look at rayon-core's test suite contents (stress tests, etc.) in detail.

## 7. Deterministic simulation testing and seeded deterministic schedulers

### Takeaway
FoundationDB-style DST (single-threaded deterministic simulation, seeded randomness, simulated time) is the model for madsim and turmoil, but these target async/distributed I/O, not shared-memory weak-ordering bugs; for eml the transferable idea is a seeded, single-OS-thread deterministic scheduler for VM workers (reproducible interleavings by seed), complementing loom/shuttle/Miri rather than replacing them.

### Cited Findings
- FoundationDB: Simulation "is enabled by and tightly integrated with Flow", supports "both production execution and deterministic simulated execution", "can conduct a deterministic simulation of an entire FoundationDB cluster within a single-threaded process"; "Determinism is crucial in that it allows perfect repeatability"; "tens of thousands of simulations every night"; estimate of ~one trillion CPU-hours simulated — [FoundationDB: Simulation and Testing](https://apple.github.io/foundationdb/testing.html)
- madsim: "a Rust async runtime similar to tokio, but with a key feature called deterministic simulation"; "All I/O-related interfaces must be mocked during the simulation, and all uncertainties should be eliminated"; enabled with `RUSTFLAGS="--cfg madsim" cargo test`; used by RisingWave — [madsim README](https://github.com/madsim-rs/madsim)
- turmoil: "a family of crates for deterministic simulation testing of distributed systems. It runs multiple concurrent hosts within a single thread and injects 'hardship' — latency, drops, partitions, crashes, torn writes — ... under manual control or a seeded RNG" — [turmoil README](https://github.com/tokio-rs/turmoil)
- Miri itself now has a "fully non-deterministic" scheduler driven by a seed, plus virtual time for a deterministic monotone clock — [Ralf Jung blog](https://www.ralfj.de/blog/2025/12/22/miri.html); shuttle exposes `check_random_with_seed` and `replay` of a schedule string — [shuttle docs.rs](https://docs.rs/shuttle/latest/shuttle/)

### Inferences
- DST runs everything on one OS thread, so it explores scheduling (task interleavings at yield points) but never weak-memory reorderings or true data races; it is useful for eml's scheduler logic (work stealing policy, join, continuation migration, effect handler + `par` interactions) and for reproducing failures by seed, not for the atomic RC core.
- Cheap eml-specific variant: make the VM's worker loop pluggable so a test build runs N logical workers on one thread with a seeded scheduler that switches at bytecode/safepoint boundaries; combine with the owner-thread debug check (§5) keyed on the *logical* worker id, so a forgotten mark-shared panics deterministically and the seed reproduces it. This is a shuttle-like approach without needing shuttle's wrappers.

### Gaps
- I did not verify madsim's exact determinism mechanisms (seeded RNG env vars, libc overrides) from primary docs; the README does not describe them.

## 8. Synthesis for eml_rt: detecting a forgotten "mark shared"

### Takeaway
No off-the-shelf checker flags a forgotten mark-shared directly, because the RC word is accessed atomically on both paths; layer (1) a debug owner-id assertion in the header, (2) loom models of the small RC/publish/join core with fields as loom `UnsafeCell`, (3) Miri (strict provenance, many seeds) on core unit tests, (4) TSan on whole-program `par` runs, and (5) shuttle or a seeded in-VM scheduler for scheduler-scale tests.

### Cited Findings
- Race detectors define races over non-atomic accesses (Miri: atomic/non-atomic mixing rules; non-atomic reads may mix with atomic reads) — [Miri paper §4.2](https://research.ralfj.de/papers/2026-popl-miri.pdf); TSan's `report_atomic_races` covers "races between atomic and plain memory accesses" — [Clang TSan](https://clang.llvm.org/docs/ThreadSanitizer.html)
- Lean (same encoding) deliberately makes the ST path look atomic under TSan to avoid reports — [lean.h](https://github.com/leanprover/lean4/blob/master/src/include/lean/lean.h); CPython keeps an explicit owner thread id in the header — [PEP 703](https://peps.python.org/pep-0703/)
- loom covers Relaxed only partially and SeqCst as AcqRel; Miri misses load buffering and 2+2W; shuttle treats all atomics as SeqCst — [loom README](https://github.com/tokio-rs/loom); [Miri paper §4.3](https://research.ralfj.de/papers/2026-popl-miri.pdf); [shuttle atomic docs](https://github.com/awslabs/shuttle/blob/main/shuttle-std/src/sync/atomic/mod.rs)

### Inferences
- Tool-to-bug matrix (inferred):
  - Forgotten mark-shared, RC word only (Relaxed load/store vs fetch_sub): owner assertion = deterministic; loom = finds lost update/double free/leak if modeled (alloc via loom); shuttle = finds if interleaving hit (SC suffices); Miri = indirect (UAF/leak) with seeds; TSan = no report on RC word, maybe on fields.
  - Forgotten mark-shared + Perceus in-place reuse / field writes: loom (UnsafeCell fields), Miri, TSan all report the non-atomic race; owner assertion catches earlier.
  - Wrong ordering on publish (e.g., Relaxed instead of Release when pushing to a queue): loom and Miri (weak memory) can find; shuttle cannot; TSan can report subsequent field races since it tracks happens-before via atomics.
  - Using `fence` in decref fast path: TSan false positives unless the `cfg(sanitize = "thread")` load trick is applied.
- Keep the unsafe core small and generic over an "atomics/cell/alloc facade" module (`cfg(loom)` vs std), mirroring tokio's `src/loom/mod.rs`, so the same code runs under loom, Miri, TSan, and normal builds.
- Since eml_interp already has `debug_heap` checks (per CLAUDE.md), the owner-id field fits naturally there and should be on in all UI `run` tests once `par` exists.

### Gaps
- No primary source found that evaluates any of these tools on a language runtime with sign-encoded biased RC; the matrix above is reasoning, not measured.
