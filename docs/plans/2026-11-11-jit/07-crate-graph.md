# Appendix 07: the workspace crate graph (jit-externs refresh)

(Written 2026-11-13 by the designer "S4" during the refresh of jit-externs; reviewed once; the user approved AM-1..AM-13 and AM-12b, the graph units at the head of jit-externs (U-1 default) and U-3 = (a).)

**Read with appendix 06 and 00 §F.** This text overrides appendix 06 where they differ (06's `eml_sched` placement of the list, `ffi`, `tls`, `gui`, `constants` becomes `eml_rows`; `c_signature` goes to `eml_clink`), and 00 §F overrides both. The errata found while the units were cut (F-1..F-6) and the resolution of U-S1-3 are in the last section of this file; they correct some checks of section 8. The executable plan `docs/plans/2026-11-11-jit-externs.md` states the corrected checks.

Names in this text: "S0" is appendix 06; "S1" / "S2" are the unit designs folded into the executable plan (S1: units G-a..G-d, U2a..U6; S2: units 7a..10); "01", "05" are the appendices of this directory.


Revision of r1 after review round 1. The response table for the findings is at the end.

Tags: [V] verified (method given, or item of section 7), [I] inferred. Base fddd44f2. "01" = `docs/plans/2026-11-11-jit/01-contract.md`, "05" = `05-jit-effects-tasks-repl.md`, "S0" = appendix 06 (`06-extern-boundary.md`). The user's instruction on licensing, portability, speed and size of a shipped native app is a primary criterion; sections 1, 5.2, 5.9 and G-C10..G-C17 answer it.

## 1. Conclusion

One organising rule: the workspace has three strata, and an edge never points from a lower one to a higher one.

- **Runtime closure** (what a shipped native app links): `eml_rt`, `eml_extern`, `eml_io`, `eml_cabi`, `eml_sched`, `eml_rows`, `eml_nrt`, and behind features `eml_tls`, `eml_gui`. No compiler, no Cranelift, no VM, no dynamic loader, no test fixture.
- **Compilers and engines** (inside the `eml` tool only): the front end, `eml_lir`, `eml_core_ir`, `eml_vm`, `eml_native`, `eml_clink`.
- **Artifacts** (the only crates that turn features on): `eml_cli` (the tool) and, from native-aot, the runtime static library crate. `eml_test_support` is dev-only.

Per crate:

| Crate | Verdict | One-line responsibility |
|---|---|---|
| `eml_diagnostics`, `eml_syntax`, `eml_hir`, `eml_types`, `eml_core_ir`, `eml_lir` | keep | the front end and the two IRs, a linear chain |
| `eml_extern` | keep | declarations of built-ins and of user C functions; no dependency, no `unsafe` |
| `eml_rt` | keep | verified core; only vstd |
| `eml_tls`, `eml_gui` | keep | leaf services with no workspace edge |
| `eml_io` | **grows by about 25 lines** | OS-facing side of a run: the `IoDriver` implementations, the signal masks, and the two real-host constructors `stdio_host` / `script_host` (from `eml_vm`) |
| `eml_cabi` | keep | code pointers to C and their proof obligation (`Linked::new` / `fits`); the boundary between who links C and who calls it |
| `eml_sched` | **shrinks to its name** | the task layer and the run interface: cells, doorbell, park, clock choice, `RunConfig` / `Finished` / `RunStats` / `RuntimeError` / `RunFault`; from jit-tasks the worker table, monitor, jobs, `Exec`. Loses `rows.rs`, `constants.rs` and the `eml_extern` edge; never gets `ffi`, `tls`, `gui` |
| `eml_rows` | **new** | what the built-ins are at run time, for both engines: the `extern_rows!` list and `rows::call`, `ffi` (handles), `tls`, `gui`, `constants`. Owns the features `tls`, `gui`, `gui-native` |
| `eml_clink` | **new, split from `eml_native`** | links user C for a run: library and symbol resolution, bridges, `Linker` -> `Linked`; from jit-externs also `c_signature` |
| `eml_native` | **shrinks** | LIR -> CLIF and the JIT host, nothing else; no `bridge.rs` exemption remains in its placement rules |
| `eml_nrt` | keep | what native code calls: entries, stubs, `Image`, the native run driver |
| `eml_vm` | **loses its facade role** | bytecode compiler and interpreter; no `tls` / `gui` features; keeps the capture fixtures (`CaptureHost`, `Capture`, `Written`), which are test hosts and stay out of the runtime closure; from jit-tasks no worker table |
| `eml_cli` | keep, edges made direct | the only pipeline driver; names the crate that defines what it uses |
| `eml_test_support` | keep | dev-only wrapper over `Session`; stage features stay |

Features: `tls`, `gui`, `gui-native` are declared by `eml_rows` (plus the leaf features `eml_io/tls`, `eml_gui/native`) and switched on only by artifact crates, which may declare features of the same names for that purpose. `eml_vm`, `eml_nrt`, `eml_native`, `eml_sched` have none of them. The forwarding chain of 01 §1.1 is not built.

Shipped apps: the graph makes four variants of the runtime static library buildable, `{base, tls, gui, tls+gui}`, each with its own notice file. **Which variant an app gets is a user decision that is still open (U-3)**; every criterion below is written to hold under either answer. The base variant needs only a Rust toolchain to build; its third-party content is the Rust standard library, `vstd`, and `libc` / `windows-sys`.

Sequencing: four kind-3 units at the head of jit-externs (`eml_rows`, `eml_clink`, the real hosts, direct edges); S0's Tls / Gui / ffi units then land in `eml_rows`. The worker-table move stays in jit-tasks. The static library crate, its profile and the variants are native-aot.

## 2. Rationale

- **`eml_sched` is two crates by its own imports.** Without `rows.rs` it needs only `eml_rt`, `eml_io` and libc [V: 7.1]; `rows.rs` alone brings `eml_extern`, and S0 would add `eml_cabi`, `eml_tls`, `eml_gui` and three features to the crate that holds the 10,017-line `cells.rs`. The task layer is mechanism (multicore.md 「層の構成」: threads, inboxes, cells, monitor, waits, `Exec`); row bodies are the meaning of the extern table. They change for different reasons, are checked differently (Miri and TSan on the task layer, per-kind entry tests on the rows), and the features touch only the second.
- **Four contract criteria carry a "`bridge.rs` is exempt" clause** (N-13, N-15, N-34, N-37) and four placement tests name it [V: `crates/eml_native/tests/placement.rs:265, 299, 411, 420`]. The C linker predates the code generator and shares no code with it: `linker.rs`, `library.rs`, `bridge.rs`, `error.rs` reference only each other [V: 7.1]. Split, each rule about `eml_native` holds without exception and `eml_native` loses libloading.
- **A shipped app needs the real host constructors, and only those.** `stdio_host`, `script_host` and `unmasked` (`eml_vm/src/hosts.rs:611-632`) wrap `eml_rt::StdHost` with `eml_io`'s signal mask; the other 600 lines of the file are the capture fixtures [V: read the file's item list]. No plan moves either part [V: grep of `docs/plans` and `docs/future` for `hosts.rs|stdio_host` is empty].
- **Features decide what an app carries, so they are declared once and switched on by whoever defines an artifact.** Cargo unifies features across a build, so an intermediate crate needs a feature only if its own code has a `cfg` on it. After S0's moves `eml_vm/src` has none [V: 7.4]. The chain of 01 §1.1 exists only because `eml_cli` reaches the owner through the engines; a direct edge removes nine feature lines.
- **Licensing by construction needs variants, not linker luck.** Exactly one Rust static library can be linked (native.md 「AOT と配布」), so "no Tls code or licence in an app that does not use it" is a property of which library is linked. With `tls` and `gui` as independent features, `cargo tree` of each variant is the proof [V: 7.2].
- **Engines are peers.** `eml_cli` hands the same `RunConfig`, `Linked` and `HostRef` to either engine; taking those names from `eml_vm` makes the VM the native engine's facade.

## 3. Alternatives considered and rejected

- **Rename `eml_sched` to a general "runtime" crate and keep everything in it.** One crate with five optional edges and three features in front of the scheduler; every `tls` / `gui` combination rebuilds and re-tests `cells.rs`.
- **Merge `eml_cabi` into `eml_extern`.** `eml_extern` is `unsafe`-free and read by the front end.
- **Merge `eml_cabi` into `eml_clink`.** `eml_vm` and the runtime closure take `Linked` / `CCall` / `Dtor`; they would gain Cranelift and libloading.
- **Merge `eml_cabi` into `eml_rows::ffi`.** The linker would depend on the runtime chain for three type definitions.
- **Split `eml_native` into lowering / JIT host / AOT crates.** All three are always in the `eml` binary; N-13 holds the boundary with one grep. Revisit in native-aot.
- **Split `eml_vm` into bytecode compiler and interpreter.** No second consumer of `CodeBase`.
- **Split `eml_cli` into a driver library and a binary.** Saves clap and rustyline for front-end test builds only. Revisit when `eml build` adds the object and link driver.
- **Replace the stage features `types < core < run` by per-stage driver crates.** The features are additive, follow the pipeline, and let `cargo test -p eml_hir` skip Cranelift and the runtime.
- **Inject Tls / Gui through traits instead of features.** A trait as wide as `eml_tls::Conn` plus the toolkit; the app variants would still be build variants.
- **Delete `eml_sched/testing` now**, or **move the worker table before jit-tasks.** Both belong to the stage that designs `Exec` (N-41, U-12 stand).
- **One static library and rely on dead-stripping for Tls.** Not a construction, and the notice file could not follow it.
- **Keep forwarding features on `eml_vm` so its feature-gated tests stay put.** `eml_cli` would have to enable `eml_vm/<f>` as well or `tls_off::` would run with Tls on; that is the chain again.
- **Move all of `hosts.rs` to `eml_io`** (r1). Puts 600 lines of test fixtures into the crate graph of every shipped app.
- **Move the capture fixtures to `eml_test_support` now.** `eml_vm/src/monitor.rs`'s unit tests use `crate::capture_host`, and `src/` may not use `eml_test_support`.
- **A Rust test that parses every `Cargo.toml` for the graph check** (r1). Needs a TOML parser or fragile text matching, and a test in `eml_cli` runs only under `run`. Cargo's own resolution in a script needs neither.

## 4. Assumptions

| # | Assumption | If false |
|---|---|---|
| A-1 [V: 7.1] | `rows.rs` + `constants.rs` and the four linker files with their two test files move with import edits only; the workspace type-checks with all targets | - |
| A-2 [I] | The async row bodies and `ffi` submit / take (jit-tasks) need only public items of `eml_sched` (`WorkerCore`, cells), never the reverse | a body the worker loop itself must call stays in `eml_sched` as a method and its list line calls it; the edge direction does not change |
| A-3 [V: 7.3 for the mechanism; I for the three real files] | Integration tests in `crates/eml_rows/tests/` can run a program through `eml_test_support` and `eml_vm` (a dev-dependency cycle), and a run of `-p eml_io -p eml_rows` leaves `tls` and `gui` off | the gated tests go to `crates/eml_cli/tests/` |
| A-4 [I] | The unit tests inside `tls.rs` and `gui.rs` name no item of `eml_vm` or `eml_test_support` (today's `tls.rs` tests are pure) | such a test becomes an integration test in `crates/eml_rows/tests/`; G-C7b holds the rule |
| A-5 [V: 7.5 for the linker behaviour on aarch64 macOS; I for ELF and COFF] | An exported entry no generated object references, and what only it calls, is removed by the app link's dead-strip option, unless retained code reads a table that holds its address | native-aot's symbol test (G-C16) fails on that target and the reader is moved behind the tool, or entries get one section each by a linker option |
| A-6 [I] | `THIRD-PARTY-LICENSES.md` does not change: no external dependency is added or removed, dev-dependencies included | regenerate in the same commit (`nix/third-party-licenses.sh`) |
| A-7 [I] | The licences of `vstd`, `verus_builtin`, `libc`, `windows-sys`, `windows-link` are within `about.toml`'s `accepted` (they are in the tool's dependency tree today and the licences job passes) | found by the first `cargo-about` run on the runtime manifest in native-aot |
| A-8 [I] | The pinned toolchain's distribution carries a licence record for the standard library and the crates it vendors that the runtime notice can include verbatim | native-aot generates that section from the toolchain's `rust-src` lock file; the requirement of G-C17 does not change |

## 5. Implications for implementation

### 5.1 Target graph (direct normal edges)

`[f]` = only under feature `f`. "+stage" = added by that stage.

| Crate | Depends on (workspace) | External | Features |
|---|---|---|---|
| `eml_diagnostics` | - | ariadne, text-size | - |
| `eml_extern` | - | - | - |
| `eml_syntax` | diagnostics | logos, rowan | - |
| `eml_hir` | diagnostics, extern, syntax | la-arena, rustc-hash | - |
| `eml_types` | diagnostics, extern, hir | la-arena, rustc-hash | - |
| `eml_lir` | extern | rustc-hash | - |
| `eml_core_ir` | diagnostics, extern, hir, types, lir | la-arena, rustc-hash | - |
| `eml_rt` | - | vstd | - |
| `eml_tls` | - | rustls, rustls-platform-verifier | - |
| `eml_gui` | - | taffy; `[native]` objc2*, cxx, windows-* | `native` |
| `eml_io` | rt, `[tls]` tls | libc / windows-sys | `tls` (the test driver's terminator only) |
| `eml_cabi` | extern | - | - |
| `eml_sched` | rt, io | libc | `testing` (dev-only, until jit-tasks) |
| `eml_rows` | rt, extern, sched, cabi (+ffi unit), `[tls]` tls, `[gui]` gui | - | `tls = ["dep:eml_tls"]`, `gui = ["dep:eml_gui"]`, `gui-native = ["gui", "eml_gui/native"]` |
| `eml_nrt` | rt, extern, sched, rows; +jit-tasks: cabi, io | - | - |
| `eml_vm` | rt, extern, lir, sched, rows, cabi, io (until jit-tasks moves the monitor and worker threads) | rustc-hash | - |
| `eml_clink` | extern, cabi | cranelift-*, libloading | - |
| `eml_native` | extern, lir, rt (`mem.rs` only), nrt, cabi, clink (+C unit of jit-externs) | cranelift-* | - |
| `eml_cli` (artifact) | diagnostics, syntax, hir; `[types]` types; `[core]` core_ir, lir; `[run]` vm, native, clink, sched, rows, io; `[tls]` tls (test server) | clap; `[run]` rustyline, libc / windows-sys | `types < core < run`; `tls = ["run", "eml_rows/tls", "eml_io/tls", "dep:eml_tls"]`; `gui = ["run", "eml_rows/gui"]`; `gui-native = ["gui", "eml_rows/gui-native"]`; default `run, tls, gui` |
| `eml_test_support` | diagnostics, syntax; `[hir]` hir, cli; `[types]` types; `[core]` core_ir, lir; `[run]` vm, native, nrt, clink, sched, io | - | `hir < types < core < run` |
| `eml_rtlib` (artifact; +native-aot) | nrt, rows, io | - | `tls = ["eml_rows/tls"]`, `gui = ["eml_rows/gui-native"]` |

Dev-only edges that matter: `eml_rows` -> `eml_vm`, `eml_test_support` (the gated tests); `eml_native` -> `eml_rows` (N-27's differential tests); `eml_vm` -> `eml_sched/testing`.

Rules behind the table:

1. **Strata.** No crate of the runtime closure depends on a compiler or engine crate. `eml_rt`, `eml_extern`, `eml_tls`, `eml_gui` depend on nothing in the workspace.
2. **Features.** A non-artifact crate declares a feature only if its own `src/` or `tests/` has a `cfg` on it. Only an artifact crate (`eml_cli`, `eml_rtlib`) declares a feature whose content is another crate's feature. `eml_rows/tls` does not enable `eml_io/tls`: the terminator behind `tls.localhost` is a test facility of the tool, and a shipped app links no test facility. `eml_cli` enables both. A run of `eml_rows` alone with Tls is `cargo nextest run -p eml_rows --features eml_rows/tls,eml_io/tls`.
3. **Re-exports.** A crate re-exports from another workspace crate only types that occur in its own public signatures. No re-exported function, module, constant or macro, except `#[doc(hidden)] pub use eml_rt as __rt` for macro hygiene. Kept: `eml_nrt::{HostRef, RunConfig, Finished, RunFault, RuntimeError}` (so `eml_native` names no `eml_sched`, as today), `eml_sched::{Control, Limits, Stop, HostRef}` (the run interface is complete in one crate, so `eml_cli`'s `src/` still names no `eml_rt`), `eml_clink::Linked`, `eml_rows::ffi::Dtor`, `eml_rows::gui::{Backend, Script}`, `eml_vm`'s and `eml_native`'s `execute` types. Deleted: `eml_vm`'s `stdio_host` / `script_host` / `Masked` / `Doorbell` / `eml_gui` / `gui_park`; `eml_nrt::shared_rows`, `LISTED`, `Listed`, `Section`; `eml_native::{Linker, LinkError, Bridges, candidates, running_os}`; `eml_native::SUPPORTED` becomes its own `pub const`. `eml_vm::{capture_host, Capture, CaptureHost, Written}` are its own items and stay.
4. **`eml_native` and the runtime.** `eml_native`'s `[dependencies]` never name `eml_sched`, `eml_rows`, `eml_io` or `eml_vm`.
5. **Tests in the runtime closure.** `#[cfg(test)]` code in the `src/` of `eml_rows`, `eml_sched`, `eml_nrt`, `eml_io` names no engine crate and not `eml_test_support`: through the dev-dependency cycle the engine links a second copy of the crate under test, whose types do not match [V: 7.3 shows the cycle builds; the mismatch is Cargo's documented behaviour]. Such a test is an integration test in `tests/`.

### 5.2 The shipped app (licensing, portability, speed, size)

**What is linked.** Generated objects plus one runtime static library, built by the artifact crate `eml_rtlib` (`crate-type = ["staticlib"]`, created in native-aot; the file names of the libraries are native-aot's choice). Variants:

| Variant | Features | Workspace crates inside | Third-party code inside | Build needs |
|---|---|---|---|---|
| base | - | rt, extern, io, cabi, sched, rows, nrt | the Rust standard library with the crates it vendors and the compiler builtins (a `staticlib` embeds them [V: 7.5]); vstd, verus_builtin; libc (unix); windows-sys / windows-link (Windows) [V: 7.2] | Rust toolchain only |
| tls | `tls` | + tls | + rustls, rustls-webpki, rustls-pki-types, rustls-platform-verifier and its per-OS crates (security-framework, core-foundation on macOS; schannel on Windows; rustls-native-certs, openssl-probe, webpki-root-certs elsewhere), aws-lc-rs, aws-lc-sys (C), zeroize, subtle, untrusted, once_cell, log [V: 7.2] | + C toolchain for the target (NASM or the prebuilt objects on Windows x86_64) |
| gui | `gui` | + gui | + taffy, slotmap, smallvec, and per OS objc2* (macOS), cxx + link-cplusplus + the Qt shim objects (Linux), windows-core family 0.100 (Windows). Qt 6 and AppKit are linked dynamically; no Windows App SDK file is linked | + Qt 6 headers and a C++17 compiler on Linux |
| tls+gui | both | both | both | both |

Never inside any variant: Cranelift, libloading, `eml_clink`, `eml_native`, `eml_vm` (so no bytecode, no interpreter, no capture fixture), `eml_lir`, `eml_core_ir`, the front end, clap, rustyline, ariadne, logos, rowan [V: 7.2 for the closure of `eml_nrt`; `eml_rtlib` adds no edge outside it].

**Which variant an app gets (open, U-3).** The graph is the same under every answer; the answers differ in what `eml build` selects and which variants are shipped.

| Answer | Selection | Shipped | "An app without Tls carries no Tls code or licence" |
|---|---|---|---|
| (a) by import (default) | `gui` axis by profile; `tls` axis by whether the module graph contains `Std.Tls` (also through `Std.Http`) | 4 | by construction, with no action from the author |
| (b) by profile only | profiles `console`, `console-tls`, `gui`, `gui-tls`; a program that imports `Std.Tls` under a profile without it fails before the link, like `Std.Gui` under `console` today | 4 | by construction, the author names it |
| (c) native.md as written | profiles `console`, `gui`; Tls in both | 2 (tls, tls+gui) | not true; every app carries aws-lc and rustls and needs a C toolchain to build the library. This contradicts the user's licensing instruction and is listed only so that the choice is explicit |

(a) amends native.md's sentence "what is linked is decided by the profile alone" (AM-12). Reason for the default: the `gui` axis is an author's choice (subsystem, bundle); whether a program uses Tls is a fact of its module graph, the same kind of fact the `Std.Gui` check already reads.

**Notices.** One notice file per shipped variant. Each has three parts: (1) the crates of that variant, generated by `cargo-about` from `eml_rtlib`'s manifest with that variant's features and the same `about.toml`; (2) the Rust standard library and what it vendors, taken from the pinned toolchain (A-8), identical in every variant; (3) for `gui` variants only, the "System libraries" section of `about.hbs` (Qt under LGPL-3.0, AppKit, the Windows App SDK runtime) together with `GPL-3.0.txt` / `LGPL-3.0.txt`. `eml build` copies the file of the variant it linked. Part (2) is new relative to r1 and to the tool's own `THIRD-PARTY-LICENSES.md`, which does not list the standard library either [V: `cargo-about` reads the Cargo dependency graph; the sysroot is not in it]; whether the tool's file gains the same section is U-8.

**Portability.**

- The base closure builds with rustc alone on every target the cross shell has (G-C13).
- aws-lc's C build is reached only through `eml_tls`, the Qt headers only through `eml_gui/native`; neither is reachable through `eml_sched`, `eml_nrt`, `eml_vm` or `eml_native` unless an artifact asks.
- The Tls closure is built natively on the four CI targets today (the test job runs the default features, which include `tls`, on macOS, Linux x86_64, Linux aarch64 and Windows [V: `ci.yml` job `test`]). G-C13b pins that as a named step on the runtime closure, so the riskiest variant has a gate from jit-externs on. Cross-building a Tls or Gui variant is not supported and not planned: the libraries are built on a native runner per target.
- Matrix: 4 variants x 4 targets = 16 archives (8 under answer (c)). native-aot records their sizes and build times; if the matrix is too large the cut is to drop `tls+gui`-less combinations per OS, decided there with numbers.

**Speed and size.**

- The static library is built with a profile of its own (native-aot adds it): LTO inside the library (thin at least, so `eml_rt` inlines into the entries, the reason the workspace's release profile gives), `panic = "unwind"` (the shim rule N-10 needs `catch_unwind`), `codegen-units` and `opt-level` by measurement, recorded in benchmarks.md.
- The app link passes the platform's dead-strip option (`--gc-sections`, `-dead_strip`, `/OPT:REF`). Measured on a model [V: 7.5]: an exported entry of a dependency crate that nothing references, and the function only it calls, are absent from the linked program under thin and under fat LTO; without the option they stay; and one retained function that reads a table of entry addresses keeps every entry. Hence three rules from now on:
  1. **Code reachable from the run path of `eml_nrt` never reads the entry descriptor tables** (`entries()`, `ROWS`, `HEAP`, ..), and no table or entry carries `#[used]`. Only the code generator and tests read them. `pub fn entries()` itself is harmless: an unreferenced function is stripped with the table.
  2. **`eml_nrt` never calls `eml_rows::rows::call` or any other dispatcher over `Extern`**; an entry references exactly its own row function. This binds jit-tasks' async entries.
  3. `rows::call` and every `Mode`-generic function are generic, so a library that instantiates only `Ctx<Fast>` contains no checked-mode code and no VM dispatcher.
  Entries are not exported symbols today (the JIT takes addresses from the table [V: no `no_mangle` in `crates/eml_nrt/src`]); native-aot adds the export.
- Known residue, recorded for native-aot: `Box<dyn IoDriver>` and `HostRef` keep the whole real driver and `StdHost` in every app through their vtables; `RunClock::driver` keeps `TestDriver` reachable because the clock is a run-time value (native-aot passes a driver factory from the artifact, or gates it; then `eml_sched` loses its `eml_io` edge); the GUI stub backend stays in `gui` variants.

**What native-aot must move into the closure** (nothing here blocks it): the SIGINT / console handlers and the physical-memory query of `crates/eml_cli/src/interrupt/` and `lib.rs`, to `eml_io`; the app's `main` and `eml_run_pending`, to `eml_rtlib`; bridges for `blocking` C compiled into the app's object by a `define_bridge<M: Module>` that `eml_clink` exports and `eml_native` calls.

### 5.3 What moves where

| From | To | When |
|---|---|---|
| `eml_sched/src/rows.rs`, `constants.rs`, `tests/constants.rs` | `eml_rows/src/rows.rs`, `constants.rs`, `tests/constants.rs` | unit G-a |
| `eml_native/src/{linker,library,bridge,error}.rs`, `tests/{linker,bridge}.rs`, the C fixtures of `tests/common/mod.rs` (everything but `pub mod native`), their rows of `src/trust.rs` | `eml_clink/src/`, `tests/`, a `trust.rs` of its own | unit G-b |
| `stdio_host`, `script_host`, `unmasked` (`eml_vm/src/hosts.rs:610-632`) | `eml_io/src/hosts.rs` (`pub fn` at the crate root; listed in `eml_io`'s `trust.rs`; no `unsafe`) | unit G-c |
| `Capture`, `Written`, `CaptureHost`, `capture_host` and their three test modules | stay in `eml_vm/src/hosts.rs` | jit-tasks decides when `monitor.rs` and its unit tests leave `eml_vm` (default: to `eml_test_support`, with the moved unit tests on `StdHost` or a local host) |
| re-export lists of `eml_vm`, `eml_nrt`, `eml_native`, `eml_cli`; imports in tests | rule 3 of 5.1 | unit G-d |
| `eml_vm/src/tls.rs`; `eml_vm`'s feature `tls`; `tests/tls_off.rs` | `eml_rows/src/tls.rs`; `eml_rows` feature `tls`; `eml_rows/tests/tls_off.rs` | S1's U3 |
| `eml_vm/src/gui.rs` with `GateHost`; features `gui`, `gui-native`; `tests/gui.rs`, `gui_off.rs`; `RecordingHost` of `eml_vm/tests/common` | `eml_rows/src/gui.rs` (trait `eml_rows::gui::GateHost`, implemented by `eml_vm`'s `Scheduler<P>`); `eml_rows` features; `eml_rows/tests/`; `eml_test_support` (second user) | S1's U4 |
| `CHandle`, `HandleCell`, `Raise`, `handle_new`, `empty_mut_array` | `eml_rows::ffi`, `eml_rows::constants` | S1's U6 |
| `abi::c_signature` (S0 5.4) | `eml_clink::c_signature` (public; `bridge.rs` and `eml_native::abi::declare_import` call it) | S2's C unit |
| worker table, half of `Scheduler<P>`, `monitor.rs`, `jobs.rs`, `counters.rs`, the `Park` loop, recovery | `eml_sched` | jit-tasks, jit-repl (as 05) |
| async row bodies, `ffi` submit / take, `c_links` / `renumbered` | `eml_rows` (`rows.rs` task lines, `ffi`) | jit-tasks, jit-repl (05 said `eml_sched`) |
| `GateHost` | deleted; `gui.rs` takes `&mut WorkerCore<E>` | jit-tasks, once `WorkerCore` is in `eml_sched` (05's `Exec: GateHost` cannot hold: the trait would sit above `Exec`) |

### 5.4 Sequencing and units

Default: the graph units are the first units of jit-externs, each one commit, each kind 3, each green under gate G. (The alternative, a listed stage of its own, is U-1.)

- **G-a `eml_rows` and the graph script.** Create the crate; move `rows.rs` and `constants.rs`; `eml_sched` drops `eml_extern`; `eml_nrt` and `eml_vm` gain the edge. Add `nix/crate-graph.sh [--check]` (5.7) with the table as it is after this unit and call it from CI in place of the step `dependency edges of eml_native`. Every later unit that changes an edge or a feature edits the table in the same commit. Done when: `cargo nextest run` green with no snapshot change; `nix/crate-graph.sh --check` exits 0; `grep -rn 'eml_sched::rows\|eml_sched::constants\|eml_sched::extern_rows' crates` empty.
- **G-b `eml_clink`.** Create the crate; move the four files, their tests and trust rows; `eml_cli` and `eml_test_support` import `Linker` / `LinkError` from it; the four placement tests lose `bridge.rs` and are renamed (`jit_module_is_named_only_in_jit`, `memory_is_touched_only_in_mem_and_thunk`, `flags_are_built_only_in_abi`, `indirect_calls_are_only_in_apply_and_thunk`); `eml_native` drops libloading. Done when: gate G; `grep -c bridge crates/eml_native/tests/placement.rs` = 0; the script passes with `eml_clink`'s row `eml_cabi eml_extern`.
- **G-c real hosts.** Move the three functions; `eml_cli` and `eml_test_support` take `stdio_host` / `script_host` from `eml_io`. Done when: gate G; `grep -n 'fn stdio_host\|fn script_host' crates/eml_vm/src/hosts.rs` empty.
- **G-d direct edges.** Apply rule 3; `eml_cli` gains the edges to `eml_sched` and `eml_io` and takes the run interface from `eml_sched`; the script gains the re-export check (G-C6). Done when: gate G; the script passes; `grep -rn 'shared_rows' crates` empty.
- Then S1's units with paths replaced (5.5), S2's units with `eml_clink::c_signature`.

Order constraints: G-a before S1's U2a (the list is reshaped where it will live); G-b before S2's C unit; G-c and G-d any time before Close. If G-d is dropped (U-6), only G-C6 and AM-13 go; the script and every other criterion remain.

### 5.5 Effect on appendix 06 and on jit-externs' units

- Design unchanged: the list grammar, two expansions, `rows::call`, 232 entries, the C path, the soundness argument, M-1..M-7.
- Placement: read `eml_rows` for `eml_sched` wherever S0 names `rows`, `ffi`, `tls`, `gui`, `constants` (S0 §1, 5.1, 5.3, 5.5, 5.8, 5.12; S1 U2a..U6 targets and gates; S2's uses of `eml_sched::ffi`). `eml_sched::error::RunFault` and the run-interface types stay.
- S0 5.3 last bullet ("eml_nrt and eml_native forward `tls`, `gui`, `gui-native` to eml_sched") is deleted. S1's U3 / U4 instead: declare the features on `eml_rows`, delete them from `eml_vm`, point `eml_cli`'s features at `eml_rows` (and `eml_io/tls`), move the gated tests, edit the script's feature table.
- S0 5.1 "eml_nrt keeps `pub use eml_sched::rows as shared_rows`": dropped; `eml_native`'s tests dev-depend on `eml_rows`.
- S0 5.3 `dtor` token: `Dtor` is `eml_rows::ffi::Dtor` (a re-export of `eml_cabi::Dtor`, in `handle_new`'s signature); `eml_nrt` gains no edge in this stage.
- S0 5.4 `abi::c_signature`: defined in `eml_clink`; `Import`, `declare_import` stay in `eml_native::abi`.
- S1-A4 ("workspace unification builds eml_sched's lib tests with tls and gui") holds for `eml_rows`: a workspace run turns `eml_rows/tls` on through `eml_cli`'s default, a run of `-p eml_rows` leaves it off [V: 7.3].
- Gate lines that name `-p eml_sched` for row tests name `-p eml_rows`.

### 5.6 Test changes by kind

- **Kind 1**: none.
- **Kind 2**: none.
- **Kind 3** (expected values byte-identical; the commit message is the record): the moved test files (`eml_sched/tests/constants.rs`; `eml_native/tests/{linker,bridge}.rs`; `eml_vm/tests/{gui,gui_off,tls_off}.rs` and `tls.rs`'s unit tests); import paths in `eml_vm/tests`, `eml_cli`, `eml_test_support`; the four renamed placement tests, whose allowed-file lists shrink; the trust tables of `eml_native`, `eml_clink`, `eml_io`.
- **New**: `eml_clink`'s `trust::` tests (the rows moved from `eml_native`'s). The graph and re-export checks are a script, not tests.
- Four more test binaries (`eml_rows` lib and integration, `eml_clink` lib and integration); the timing check covers them.

### 5.7 CI, docs, CLAUDE.md

`nix/crate-graph.sh [--check]` (bash, like `nix/third-party-licenses.sh`; no new dependency, runs under any stage feature because it runs no test):

- For each workspace crate: `cargo tree -p <crate> -e normal --depth 1 --prefix none --all-features --target all | grep '^eml_' | sed 's/ .*//' | sort -u`, minus the crate itself, compared with that crate's row of a table in the script [V: 7.3, the command prints the direct edges including optional and target-specific ones]. Covers G-C1, G-C2.
- The `[features]` table of each crate compared with the script's table (plain text between `[features]` and the next `[` header). Covers G-C5.
- The closure greps of G-C3, G-C10, G-C11, G-C12, G-C13 (second half).
- The re-export scan of G-C6 (from G-d).

CI (`.github/workflows/ci.yml`):

- Step `dependency edges of eml_native` becomes `crate graph`: `nix/crate-graph.sh --check`; the `eml_sched.*testing` line moves into the script.
- Step `feature resolution of tls`: `eml_vm feature "tls"` -> `eml_rows feature "tls"`; the "no eml_tls" tree is `-p eml_rows -p eml_io -p eml_vm -p eml_nrt`; add the guard of G-C7c.
- `cargo nextest run -p eml_io -p eml_vm -E 'test(tls_off::) | test(gui_off::)'` -> `-p eml_io -p eml_rows`.
- Feature-combo clippy: the "without tls and gui `--all-targets`" row is `eml_io + eml_rows + eml_vm + eml_nrt`.
- New step in the test job, every OS (G-C13b): `cargo build -p eml_nrt --features eml_rows/tls`.
- ASan: add `cargo test -p eml_clink` (the bridge and linker tests ran inside `eml_native`'s). Miri and TSan lines unchanged until jit-tasks.

Docs, written in the commit that makes each true: architecture.md 「crate の依存関係」 (the block; the bullets on `eml_sched`, `eml_native`, `eml_nrt`, `eml_cabi`, `eml_cli`, `eml_test_support`; new bullets for `eml_rows`, `eml_clink`, the five rules of 5.1, the runtime closure and the script), 「`eml_native` の内部」, 「`eml_io` の内部」, new 「`eml_clink`の内部」, 「外部 crate」; spec/vm.md 「置き場所」 (the dependency list of `eml_vm`, the re-export bullets, where the hosts are, `execute`'s `linked`), 「ライブラリと記号」 and 「橋渡しの関数」 where they name `eml_native`; spec/native.md 「行の下ろし方」 (`shared_rows`); testing.md (feature combos, 「`eml_native` の検証」, where the gated tests live, rule 5); distribution.md 「AOTの出力」 and future/native.md 「AOT と配布」 (variants, notices, the standard-library part; the selection rule only after U-3 is answered); future/multicore.md 「層の構成」 (diagram and first two bullets); future/roadmap.md (5.8).

CLAUDE.md: the Architecture block (lines for `eml_sched`, `eml_io`, `eml_native`, `eml_nrt`, `eml_cabi`, `eml_vm`, `eml_cli`; new `eml_rows`, `eml_clink`); one new invariant bullet (strata, features switched on by artifacts only, re-exports of signature types only, rule 5); Commands (`nix/crate-graph.sh`, the clippy combo line, the `tls_off` / `gui_off` line, the ASan line). `crates/eml_rt/CLAUDE.md`: no change.

### 5.8 Effects on later stages (roadmap items)

- **jit-externs** (「段の列」 text): "…`eml_sched`へ移す" -> `eml_rows`; add the two new crates.
- **jit-effects**: `ForkTables` / `fork_evv` stay in `eml_sched` (fork is mechanism). No change.
- **jit-tasks**: worker table etc. to `eml_sched` as planned; task row bodies and submit / take to `eml_rows`; `GateHost` deleted; `eml_sched/testing` deleted (N-41); the capture fixtures leave `eml_vm`'s `src/` if no unit test there needs them; `eml_vm` drops `eml_io`; `eml_nrt` gains `eml_cabi` (the `Image` C table) and, if its workers spawn masked threads, `eml_io`; rules 1 and 2 of 5.2 on async entries.
- **jit-repl**: `c_links` / `renumbered` to `eml_rows::ffi`; recovery to `eml_sched`.
- **engine-trust**: `eml_lir::Trusted`; no graph change.
- **native-aot**: `eml_rtlib`, its profile, the variants and notices with the standard-library part, the selection rule of U-3, interrupt handling and the memory query into `eml_io`, `define_bridge`, the driver factory, exported entries, the symbol, link-mode and size tests (G-C15..G-C17). The roadmap item gains these as 決めたこと / 論点.
- **native-inline**, **par-steal**, **std-par**, **native-par**: land in `eml_native`, `eml_sched`, `eml_rows` + `eml_rt`, `eml_nrt` respectively; no new edge.
- Roadmap 「その後の項目」 item "`eml_sched`のcrate": text updated (rows are no longer there); its condition is unchanged.

### 5.9 Contract amendments

| # | Where | Old | New | Why |
|---|---|---|---|---|
| AM-1 | 01 §1.1 graph and table | two new crates; `eml_sched` holds the list, row functions, `tls.rs`, `gui.rs`, the user-C pieces | the table of 5.1: `eml_rows` holds those; `eml_clink` holds the linker and bridges; `eml_io` holds the real-host constructors | 2 |
| AM-2 | 01 §1.1 "Features" | `eml_sched` defines the three features; `eml_vm`, `eml_nrt`, `eml_native` forward them; `eml_cli` names `eml_vm/<f>` and `eml_native/<f>` | `eml_rows` defines them; no other runtime or engine crate has them; `eml_cli` names `eml_rows/<f>` and `eml_io/tls`; the clippy row gains `-p eml_rows` | rule 2 of 5.1; 5.2 |
| AM-3 | N-1 | "native runtime code goes to eml_nrt, shared engine-neutral code to eml_sched" | "native runtime code goes to eml_nrt; the task layer and the run interface to eml_sched; row bodies and the values of built-ins to eml_rows; OS-facing code a shipped app needs to eml_io; linking of user C to eml_clink" | one reason per crate |
| AM-4 | N-2 | CI step `dependency edges of eml_native` with five `cargo tree` greps and the `unsafe` grep | CI step `crate graph` = `nix/crate-graph.sh --check`, which holds every old pattern plus the direct-edge table, the feature tables and the closure greps of G-C3, G-C10..G-C13 | the graph changed; one place to read it |
| AM-5 | N-13, N-15, N-34, N-37 | each ends "… and `bridge.rs`" / "(`bridge.rs` exempt)" | the same rules without the clause; the bridges are `eml_clink`'s, which has its own flag constructor and is outside these rules | G-b |
| AM-6 | N-7 as amended by M-4 | "signature built from its `CSignature` by one helper (`c_signature`, which `bridge.rs` also uses)" | "… by one helper, `eml_clink::c_signature`, which the bridges also use" | the helper lives with the C ABI |
| AM-7 | N-22, N-23, N-30 | "the `extern_rows!` list in eml_sched"; N-30's grep over three crates | "in eml_rows"; N-23 unchanged (`RuntimeError` stays in `eml_sched`); N-30's grep adds `crates/eml_rows crates/eml_clink` | G-a |
| AM-8 | N-41 last sentence | "No other new cargo feature is introduced by the jit stages" | adds: "the features `tls`, `gui`, `gui-native` move from `eml_vm` to `eml_rows` in jit-externs; that is a move, not a new feature" | AM-2 |
| AM-9 | 01 §5.3 rows jit-externs and jit-tasks; 05 D-7, D-8, D-13, D-21 | Tls / Gui to `eml_sched` "with the feature forwarding of 1.1"; async row bodies, submit / take in `eml_sched`; `Exec: GateHost` | to `eml_rows`, no forwarding; bodies and submit / take in `eml_rows`; `GateHost` deleted when `WorkerCore` moves | 5.3 |
| AM-10 | S0 G-1, G-2, G-3, G-4, G-6, G-14, G-15, G-17, G-18 | paths and names in `eml_sched` (`crates/eml_sched/src/rows.rs`, `eml_sched::rows::call`, `eml_sched::ffi::*`, `eml_sched::constants::empty_mut_array`); G-2 "MUST NOT: eml_vm or eml_native invoke `extern_rows!`" | the same criteria with `eml_rows`; G-2 adds `eml_sched` to the crates that must not invoke the list | G-a |
| AM-11 | S0 G-10; S0 5.1, 5.3 | `grep 'fn c_signature' crates/eml_native/src` hits one; `shared_rows` kept; nrt / native forward features | `grep -rn 'fn c_signature' crates/*/src` hits one, in `eml_clink`; `shared_rows` deleted; no forwarding | 5.5 |
| AM-12 (needs U-3) | future/native.md 「AOT と配布」, roadmap native-aot, distribution.md 「AOTの出力」 | two libraries, `libeml_rt.a` (from `eml_nrt` with `eml_sched`, `eml_rt`) and `libeml_rt_gui.a`; "what is linked is decided by the profile alone, not inferred from imports"; one notice file | under (a): four variants of `eml_rtlib`; the profile decides the `gui` axis and the presence of `Std.Tls` in the module graph decides the `tls` axis; one notice file per variant. Under (b): the same four variants, four profiles, the sentence stays. Under (c): nothing changes but the crates the libraries are built from (adds `eml_rows`, `eml_io`, `eml_cabi`, `eml_extern`) | the user's licensing and size criterion; the old text makes every app carry Tls |
| AM-12b | distribution.md 「AOTの出力」, native.md 「AOT と配布」 (independent of U-3) | the runtime notice is the crates' licences | the runtime notice also carries the Rust standard library's licences and those of the crates it vendors | a static library embeds the standard library |
| AM-13 | vm.md 「置き場所」 | `eml_vm` re-exports the run interface, the hosts and `Control` for `eml_cli` | rule 3 of 5.1; `stdio_host` / `script_host` are `eml_io`'s | engines are peers |

U-12 (the `testing` feature) and N-5 are unchanged. No soundness criterion is touched; nothing in `eml_rt` is edited.

## 6. Unresolved items

| # | Item | Default |
|---|---|---|
| U-1 | Graph units inside jit-externs, or a listed refactor stage `crate-graph` that jit-externs names as its prerequisite | inside jit-externs, as its first four units: about 1,100 moved lines in four commits do not pay for a plan, a Close and a benchmark refresh of their own, and jit-externs rewrites the same doc sections |
| U-2 | Names `eml_rows`, `eml_clink`, `eml_rtlib` | as written. `eml_rows` matches `extern_rows!` and "externの行"; `eml_std` would be read as the `.em` library |
| U-3 (user) | How an app's runtime variant is selected: (a) Tls by import, (b) by profile only with four profiles, (c) native.md as written (table in 5.2) | (a). The answer is needed before native-aot's design, not before jit-externs: no unit of jit-externs depends on it |
| U-4 | Where the feature-gated engine tests go | `crates/eml_rows/tests/` (A-3); fallback `crates/eml_cli/tests/` |
| U-5 | `eml_nrt` -> `eml_cabi` in jit-externs (name `Dtor` directly) or the re-export `eml_rows::ffi::Dtor` | the re-export; the direct edge arrives in jit-tasks with `CCall` |
| U-6 | Whether G-d (re-export rule) is worth its import churn now | yes, last of the four, droppable without touching the others (5.4) |
| U-7 | Removing `TestDriver` from shipped apps | native-aot (a driver factory passed by the artifact), not a feature now |
| U-8 | Whether the tool's `THIRD-PARTY-LICENSES.md` gains the standard-library section too | yes, in native-aot together with the runtime notices, since the same mechanism produces it; it is a gap of today's file and not caused by this design |

## 7. Additional checks

1. [V] Scratch worktree at fddd44f2 (removed): created `eml_rows` (moved `rows.rs`, `constants.rs`, `tests/constants.rs`; `eml_sched` without `eml_extern`), created `eml_clink` (moved `linker.rs`, `library.rs`, `bridge.rs`, `error.rs`, `tests/linker.rs`, `tests/bridge.rs`, the C fixtures); edited only import paths, manifests and `eml_native/src/trust.rs`'s file list. `cargo check --workspace --all-targets` finishes with no error; the only warnings are eight unused C fixtures left in `eml_native/tests/common/mod.rs`. `cargo tree -p eml_sched -e normal`: workspace crates `eml_io`, `eml_rt`. `cargo tree -p eml_clink -e normal`: `eml_cabi`, `eml_extern`. `cargo tree -p eml_native -e normal | grep -c libloading` = 0. (That run also moved all of `hosts.rs` to `eml_io`, which type-checked; r2 moves three functions of it only.)
2. [V] Same tree, with `tls = ["dep:eml_tls"]` and `gui = ["dep:eml_gui"]` on `eml_rows`: the closure of `eml_nrt` (`cargo tree -p eml_nrt -e normal --target all`) has no line matching the compiler / tool / Tls / Gui pattern of r1's G-C10 and G-C11; with `--features eml_rows/tls` it contains `eml_tls` and aws-lc; with `--features eml_rows/gui` it contains `eml_gui` and no aws-lc. External crates of the base closure: `vstd`, `verus_builtin`, `libc`, `windows-sys`, `windows-link`, plus proc-macro crates that are not linked. The closure of `eml_tls` and of `eml_gui --features native` gave the crate lists of 5.2.
3. [V] Second scratch worktree (removed): `eml_rows` as in 1, with the two features, dev-dependencies on `eml_vm` and `eml_test_support` (`run`), `eml_cli`'s `tls` / `gui` naming `eml_rows/<f>`, and an integration test `tls_off::…` under `#[cfg(not(feature = "tls"))]` that runs a program through `eml_test_support::run`. `cargo nextest run -p eml_rows`: 8 tests pass, the gated one included, with the dev cycle. `cargo tree -p eml_io -p eml_rows -e features -i eml_rows | grep -c -E 'eml_rows feature "(tls|gui)"'` = 0; the same with `--workspace` = 1 for `tls`; with `--workspace --exclude eml_tls --no-default-features --features eml_cli/run,eml_test_support/run` = 0. (The non-inverted tree hides repeated nodes, so the `-i` form is the one to use.) `cargo tree -p <crate> -e normal --depth 1 --prefix none --all-features --target all | grep '^eml_'` lists the direct edges, optional ones included (`eml_rows`: extern, gui, rt, sched, tls). `cargo tree -p eml_nrt -e normal,build --target all --prefix none | grep -c -E '^(cc|cmake|cxx-build|bindgen|nasm|pkg-config) v'` = 0.
4. [V] `grep` of `feature = "` over `crates/*/{src,tests}`: outside `eml_gui`, `eml_io`, `eml_cli`, `eml_test_support` and `eml_sched/src/cells.rs` (`testing`), the sites are `eml_vm/src/{lib,gui,tls}.rs` and `eml_vm/tests/{main,gui}.rs`.
5. [V] Model of the app link, rustc 1.99.1, aarch64 macOS (deleted): a `staticlib` crate re-exporting a dependency crate that defines three `#[unsafe(no_mangle)] extern "C"` functions (`eml_x_Used`; `eml_x_TlsUnused`, whose body is a `catch_unwind` around a call of an `#[inline(never)]` marker; a run function), a `const` table of both entry addresses and `pub fn entries()`. `nm` of the archive shows both entries exported. A C `main` that calls `eml_x_Used` and the run function, linked with `-Wl,-dead_strip`: no `eml_x_TlsUnused` and no marker in the program (17,016 bytes with `lto = "thin"`, 17,608 with `"fat"`); linked without the option: both present (33,776 bytes); a `main` whose run function looks an entry up in the table: both present even with the option (33,736 bytes). The archive embeds the standard library and `compiler_builtins` objects. Not run on ELF or COFF.
6. [V] Read: every `Cargo.toml`; `lib.rs` of `eml_sched`, `eml_nrt`, `eml_native`, `eml_cabi`, `eml_vm` (head), `eml_io`; `eml_sched`'s imports per file; `eml_vm/src/hosts.rs` (item list and lines 600-634), `gui.rs` head and 455-545; `eml_vm/tests/main.rs`; `eml_nrt/src/entries.rs:120-160`; `placement.rs`; `ci.yml` step list; architecture.md 「crate の依存関係」; vm.md 「置き場所」; multicore.md 「層の構成」; native.md 「AOT と配布」; distribution.md; `about.toml`, `nix/third-party-licenses.sh`; 01 §1.1-1.3, §5, §8; 05 D-7; appendix 06 in full; S1's unit table.
7. Not checked: the whole test suite after the moves (only `-p eml_rows` in 3); the three real gated test files from `eml_rows/tests` (A-3); `cargo-about` on a runtime manifest; the toolchain's licence record for the standard library (A-8); dead-stripping on ELF and COFF; any link of a real native app (there is no `eml build`); build times before and after.

## 8. Conformance criteria

"Script" = `nix/crate-graph.sh --check`, run by the CI step `crate graph`. Patterns are matched on `cargo tree … --prefix none` output, anchored at the line start.

- **G-C1** MUST: the workspace members are the 18 crates of fddd44f2 plus `eml_rows` and `eml_clink`, and from native-aot `eml_rtlib`. Why: a new crate is a graph decision. Applies: from G-a. Check: the script's table has one row per directory of `crates/` and no other.
- **G-C2** MUST: every direct workspace edge (normal, optional and target-specific) is in the table of 5.1. MUST NOT: an edge outside it without editing the script's table in the same commit. Applies: from G-a. Check: the script's direct-edge comparison (5.7).
- **G-C3** MUST NOT: `eml_vm` reach Cranelift, `eml_native`, `eml_nrt` or `eml_clink`; `eml_native` reach `eml_vm`; `eml_lir` reach `eml_rt`; `eml_sched` reach `eml_extern`, `eml_rows`, `eml_cabi`, `eml_tls`, `eml_gui`, Cranelift, `eml_lir`, `eml_vm`, `eml_nrt`, `eml_native`. Check (script): `cargo tree -p <crate> -e normal --target all --prefix none | grep -c -E '^(<names>)'` = 0 for each.
- **G-C4** MUST NOT: `eml_native`'s `[dependencies]` name `eml_sched`, `eml_rows`, `eml_io` or `eml_vm`; `eml_native/src` name `eml_sched` or `eml_rows`. Why: run-time code is reached through `eml_nrt` only. Check: the direct-edge table; placement test `eml_native_has_no_edge_to_the_shared_runtime` (the present `…_to_eml_sched` test with the names added).
- **G-C5** MUST: among non-artifact crates, `tls`, `gui`, `gui-native` are declared by `eml_rows` only; `eml_io` declares `tls`, `eml_gui` declares `native`, `eml_sched` declares `testing` until jit-tasks; `eml_vm`, `eml_nrt`, `eml_native`, `eml_clink`, `eml_cabi`, `eml_rt`, `eml_extern` declare none. MUST NOT: a feature whose only content is another crate's feature in any crate but an artifact (`eml_cli`, `eml_rtlib`); `eml_test_support`'s stage features mirror `eml_cli`'s and are the one listed exception. Applies: from S1's U4; `eml_rtlib`'s row is added by native-aot. Check: the script's feature tables.
- **G-C6** MUST: a `pub use eml_<crate>::…` outside `eml_cli` and `eml_test_support` names only types, and only those of rule 3's kept list. MUST NOT: re-export a function, module, constant or macro of another workspace crate (`__rt` excepted). Applies: from G-d. Check: the script greps `crates/*/src` for `pub use eml_` statements and compares the sorted list with the one it holds.
- **G-C7** MUST: (a) `eml_vm/src`, `eml_nrt/src`, `eml_native/src` have no `feature = `, and `eml_sched/src` only `"testing"`; (b) `#[cfg(test)]` code under `crates/{eml_rows,eml_sched,eml_nrt,eml_io}/src` names neither an engine crate nor `eml_test_support`; (c) the feature-off run really has the features off. Applies: from S1's U4. Check: (a) `grep -rn 'feature = "' crates/eml_vm/src crates/eml_nrt/src crates/eml_native/src` empty; (b) `grep -rn 'eml_vm\|eml_native\|eml_test_support' crates/eml_rows/src crates/eml_sched/src crates/eml_nrt/src crates/eml_io/src` hits comments only; (c) CI, in the step `feature resolution of tls`: `cargo tree -p eml_io -p eml_rows -e features -i eml_rows | grep -c -E 'eml_rows feature "(tls|gui)"'` = 0 and `cargo tree --workspace -e features -i eml_rows | grep -c 'eml_rows feature "tls"'` >= 1.
- **G-C8** MUST: in `eml_native/src`, `JITModule` is named only in `jit.rs`, `settings::builder` only in `abi.rs`, `ins().load|store` only in `mem.rs` and `thunk.rs`, `call_indirect` only in `lower/apply.rs` and `thunk.rs`. MUST NOT: any file of `eml_native` listed as an exemption. Applies: from G-b. Check: the four placement tests; `grep -c bridge crates/eml_native/tests/placement.rs` = 0.
- **G-C9** MUST: the host constructors a shipped app uses (`stdio_host`, `script_host`) are defined in the runtime closure (`eml_io`), over `eml_rt::StdHost`. MUST NOT: a test fixture (`CaptureHost`, `Capture`, `Written`, `RecordingHost`, a test driver's fixture added later) be defined in a crate of the runtime closure outside `#[cfg(test)]`; the existing `eml_io::TestDriver` is the recorded exception until native-aot (U-7). Applies: from G-c. Check: `grep -rn 'fn stdio_host\|fn script_host' crates/*/src` hits `eml_io` only; `grep -rln 'CaptureHost\|RecordingHost' crates/{eml_rt,eml_io,eml_sched,eml_rows,eml_nrt,eml_cabi,eml_extern}/src` empty.
- **G-C10** MUST NOT: the runtime closure contain a compiler, an engine, a loader or a tool crate, in any variant. Why: it is what a shipped app links. Applies: from G-b. Check (script), for each of the feature sets `(none)`, `eml_rows/tls`, `eml_rows/gui-native`, `eml_rows/tls,eml_rows/gui-native`: `cargo tree -p eml_nrt -e normal --target all --prefix none [--features <set>] | grep -c -E '^(cranelift[a-z-]*|libloading|eml_lir|eml_core_ir|eml_types|eml_hir|eml_syntax|eml_diagnostics|eml_vm|eml_native|eml_clink|clap|rustyline|ariadne|logos|rowan) v'` = 0.
- **G-C11** MUST: the base variant carries no Tls and no Gui crate; the `tls` variant no Gui crate; the `gui` variant no Tls crate. Applies: from S1's U4. Check (script), same command form, with `TLS = '^(eml_tls|rustls[a-z-]*|aws-lc-rs|aws-lc-sys|ring|webpki-root-certs|security-framework[a-z-]*|schannel|openssl-probe|jni[a-z-]*) v'` and `GUI = '^(eml_gui|taffy|slotmap|cxx[a-z-]*|link-cplusplus|objc2[a-z-]*|windows-(core|collections|reference|time|implement|interface|result|strings)|qt-build-utils) v'`: no features: both counts 0; `eml_rows/tls`: `GUI` 0 and `^eml_tls v` >= 1; `eml_rows/gui-native`: `TLS` 0 and `^eml_gui v` >= 1.
- **G-C12** MUST NOT: `eml_rows/tls` enable `eml_io/tls`. Why: a shipped app links no test facility (the terminator of `tls.localhost` and its fixtures). Applies: from S1's U3. Check (script): `cargo tree -p eml_nrt -e features --features eml_rows/tls -i eml_io | grep -c 'eml_io feature "tls"'` = 0; `cargo tree -p eml_cli -e features -i eml_io | grep -c 'eml_io feature "tls"'` >= 1.
- **G-C13** MUST: the base runtime closure builds without a C or C++ toolchain. Applies: from G-a. Check: CI cross job: `nix develop .#cross -c cargo clippy --target <each of the three> -p eml_nrt -- -D warnings`; script: `cargo tree -p eml_nrt -e normal,build --target all --prefix none | grep -c -E '^(cc|cmake|cxx-build|bindgen|nasm|pkg-config) v'` = 0.
- **G-C13b** MUST: the Tls runtime closure builds natively on the four CI targets. Why: it is the variant with a C dependency, and nothing else gates it before native-aot. Applies: from S1's U3. Check: CI test job, every OS: `cargo build -p eml_nrt --features eml_rows/tls`. (The Gui closure is built by the existing `gui-native` job once its feature line names `eml_rows`.)
- **G-C14** MUST NOT: non-test code of `eml_nrt` call `eml_rows::rows::call` or match on `Extern` to choose a row function; code reachable from `eml_nrt::run` read `entries()` or a descriptor table; a table or an entry carry `#[used]`. Why: dead-stripping of unused rows (7.5). Applies: from S1's U2a; binds jit-tasks' entries. Check: `grep -rn 'rows::call\|#\[used\]' crates/eml_nrt/src` hits only `#[cfg(test)]` modules; the readers of `entries()` and of the tables outside their defining files are listed in each native stage's plan and are test modules only (today: `entries.rs`'s tests; `eml_native`'s `jit.rs` and `lower/mod.rs`, which are in the tool). The measured check is G-C16.
- **G-C15** (native-aot) MUST: the runtime static library is built from one artifact crate with independent features `tls` and `gui`, with LTO inside the library and `panic = "unwind"`; `eml build` passes the linker's dead-strip option; the variants shipped are those U-3's answer names (four under (a) and (b), two under (c)). Check: the distribution's build script; no `panic = "abort"` in the runtime profile.
- **G-C16** (native-aot) MUST: an app links exactly the variant U-3's rule selects for it, and nothing of a variant it did not select. Check, written for the answer in force: a test builds hello-world with the selection that excludes Tls and Gui ((a): no import, `console`; (b): profile `console`) and asserts `nm` shows no symbol matching `aws_lc|rustls|eml_x_Tls|eml_x_Gui`; builds a program using one `Std.Tls` function ((a): by its import; (b): profile `console-tls`) and asserts no `eml_x_Gui` and at least one `eml_x_Tls`; under (c) the first assertion is dropped and recorded as not met. Under every answer: a hello-world app has no `eml_x_` symbol of a row it does not call (rule 1 of 5.2, on each OS); a `gui` app's dynamic dependencies (`ldd` / `otool -L` / `dumpbin /dependents`) list Qt 6 (Linux) or AppKit (macOS) as shared libraries, and on Windows no Windows App SDK DLL; the sizes go into benchmarks.md.
- **G-C17** (native-aot) MUST: each shipped variant has a notice file with the three parts of 5.2, compared in CI like `THIRD-PARTY-LICENSES.md`. MUST NOT: a notice name a crate its variant does not contain. Check: `nix/third-party-licenses.sh --check` extended to the variants; required names present: every notice names the Rust standard library, `vstd` and `libc`; a `tls` notice names `aws-lc-sys` and `rustls`; a `gui` notice names `taffy` and has the System libraries section; names absent: `grep -c -i -E 'cranelift|libloading'` = 0 in every notice, `aws-lc|rustls` = 0 in notices without `tls`, `qt|taffy|cxx|objc2` = 0 in notices without `gui`.
- **G-C18** MUST: jit-tasks deletes `eml_sched/testing` (N-41) and `GateHost`, removes `eml_vm`'s edge to `eml_io`, and puts the task row bodies in `eml_rows`. Check: at jit-tasks' Close, `grep -rn 'testing' crates/eml_sched/Cargo.toml` and `grep -rn 'GateHost' crates` are empty and the script's row for `eml_vm` has no `eml_io`.
- **G-C19** MUST: every unit of this design is kind 3: no UI output, REPL transcript, `RunStats` snapshot or native snapshot changes, and `crates/eml_rt` is not edited. Check: no pending snapshot after each unit; `git diff <plan commit> --stat -- crates/eml_rt` empty.

---

## Response to review round 1

| Finding | Verdict | Reason and change |
|---|---|---|
| C-1 hosts fixtures | **Accepted** | r1's move was over-broad. Only `stdio_host`, `script_host`, `unmasked` go to `eml_io`; the capture fixtures stay in `eml_vm`. They cannot go to `eml_test_support` yet, because `monitor.rs`'s unit tests use `crate::capture_host` and `src/` may not use that crate; jit-tasks decides when those tests move (5.3). G-C9 now forbids fixtures in the runtime closure. r1's A-3 (ASan on moved host tests) is gone with the move |
| C-2 standard library in the notice | **Accepted** | Verified that the archive embeds the standard library and `compiler_builtins` (7.5). The claim "nothing beyond vstd, libc" is replaced; the notice has a standard-library part (5.2, AM-12b, A-8), G-C17 gains required names. The same gap exists in the tool's file today (U-8). The exact list std vendors is not re-verified |
| C-3 Tls by import | **Accepted** | The conclusion now says the selection is open. 5.2 gives the three answers side by side, including native.md as written and what it costs. G-C15..G-C17 are written per answer; AM-12 is marked as needing U-3 and gives old / new for each answer; AM-12b is split off because it holds under all of them. U-3 notes that no jit-externs unit depends on it |
| C-4 Tls portability gate | **Accepted** | G-C13b: the Tls runtime closure is built on the four CI runners from jit-externs on. 5.2 states that Tls and Gui variants are built natively, never cross, and sizes the matrix (16 archives, 8 under (c)). Correction to the finding: the Tls closure is already compiled on all four targets by the default-feature test job; what was missing is a step that names it |
| B-1 graph test | **Accepted** | The Rust test is replaced by `nix/crate-graph.sh`, which uses Cargo's own resolution (`cargo tree --depth 1 --all-features`, verified in 7.3). No parser, no dependency, no `Cargo.lock` change, and it does not depend on `eml_cli`'s `run` feature |
| B-2 G-C5 vs `eml_rtlib` | **Accepted** | Rule 2 and G-C5 now distinguish artifact crates, which may declare switching features; `eml_rtlib` is in the table of 5.1 and G-C1; its row in the script arrives with native-aot |
| B-3 feature-off tests | **Accepted** | Verified in a second scratch tree (7.3): the gated integration test runs from `eml_rows/tests` through the dev cycle; `-p eml_io -p eml_rows` leaves both features off; the workspace run turns `tls` on. Added: the guard (G-C7c, in the `-i` form, since the plain tree hides repeated nodes), rule 5 and G-C7b for the double crate, the command for a Tls run of `eml_rows` alone (rule 2). The three real test files were not moved (A-3 says so) |
| B-4 "no build.rs" | **Accepted** | G-C13 checks build-time C-compiler crates only |
| B-5 patterns | **Accepted** | The hedge is dropped: the script runs all four feature sets. Patterns are anchored on `--prefix none` output and cover the per-OS crates of rustls-platform-verifier |
| B-6 dead-strip evidence | **Accepted** | Model experiment added (7.5): unreferenced exported entries of a dependency crate are stripped under thin and fat LTO, kept without the option, and all kept when retained code reads the table. Rule 1 now says `pub fn entries()` is harmless and forbids `#[used]`; the text states that entries are not exported today. Limit kept visible: aarch64 macOS only (A-5); G-C16 is the per-OS check |
| C-5 rustls server side | **Accepted** | Rule 2 and G-C12 now give the real reason: a shipped app links no test facility |
| C-6 link mode | **Accepted** | G-C16 asserts the dynamic dependencies of a `gui` app on each OS |
| A-1 artifact name, A-7 | **Partially accepted** | The unsupported sentence about `libeml_rt.a` is removed; the file names are native-aot's. A-7 is reworded as an inference with its fallback. Rejected: naming each base licence in `about.toml` here; the first `cargo-about` run on the runtime manifest is the check, and no unit before native-aot needs it |
| A-2 criteria without a vehicle | **Accepted** | The script is created in G-a and edited by each unit, so G-C1 and G-C2 are enforced from the first unit; dropping G-d removes only G-C6 and AM-13 (5.4) |

## Errata from cutting the units (F-1..F-6) and U-S1-3

Adopted by the orchestrator; the plan's D-2 states the corrected checks.

| # | Finding | Resolution |
|---|---|---|
| F-1 | G-C9's grep `grep -rln 'CaptureHost\|RecordingHost' crates/{eml_rt,eml_io,eml_sched,eml_rows,eml_nrt,eml_cabi,eml_extern}/src` is not empty at fddd44f2: `crates/eml_io/src/mask.rs:180` names `CaptureHost` in a `///` comment | the check drops comment lines: `grep -rn 'CaptureHost\|RecordingHost' crates/{eml_rt,eml_io,eml_sched,eml_rows,eml_nrt,eml_cabi,eml_extern}/src \| grep -vE '^[^:]+:[0-9]+:\s*//'` is empty |
| F-2 | Rule 2's command `cargo nextest run -p eml_rows --features eml_rows/tls,eml_io/tls` fails: `eml_io` is not a (dev-)dependency of `eml_rows`, so cargo rejects `eml_io/tls` | the Tls run of `eml_rows` alone is `cargo nextest run -p eml_rows --features tls`; add `-p eml_io --features eml_io/tls` only when a test needs the `tls.localhost` terminator. testing.md gets this form |
| F-3 | 5.1 gives `eml_test_support` the edge `io`; it uses nothing of `eml_io`, and `RecordingHost` (moving there in U4) needs `eml_rt` | `eml_test_support` gains `eml_clink` (G-b), `eml_sched` (G-d) and `eml_rt` (U4), not `eml_io` |
| F-4 | G-d deletes `eml_vm::{eml_gui, gui_park}`, but `eml_cli` reaches `eml_gui::{Backend, Script}` only through `eml_vm::eml_gui`, and `eml_cli` has no `eml_gui` edge in 5.1 | both re-exports stay from G-d until U4, which deletes them (`eml_rows::gui` then re-exports `Backend`, `Script`) |
| F-5 | The renamed placement test (G-C4) must scope to `[dependencies]`: a whole-manifest check fails once G-d adds the `eml_rows` dev-dependency | the test reads the text from `[dependencies]` to the next line starting with `[` |
| F-6 | Rule 3 keeps "`eml_vm`'s and `eml_native`'s `execute` types" without a list | `eml_vm` re-exports exactly `Linked`, `RunConfig`, `HostRef`, `Finished`, `RuntimeError`; every other foreign re-export of `eml_vm` goes (users import from `eml_sched`, `eml_io` or `eml_cabi`) |
| U-S1-3 | G-C13's first half names a "CI cross job"; `ci.yml` has none | no new CI job. The half "no `cc` / `cmake` / `cxx-build` / `bindgen` / `nasm` / `pkg-config` in `cargo tree -p eml_nrt -e normal,build`" is in `nix/crate-graph.sh` (CI step `crate graph`); the cross-compile half runs locally, `nix develop .#cross -c cargo clippy --target <t> -p eml_nrt -- -D warnings` for the three cross targets, as CLAUDE.md already treats cross clippy |
