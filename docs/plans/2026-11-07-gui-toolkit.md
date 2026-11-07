# GUI toolkit

- Created: 2026-11-07
- Base: main @ 7f6d580b. File and line references were read there; when earlier commits of this stage move them, search by function or section name.
- Status: Plan finalized. Undecided details are in section 10 with defaults.

This document is the design and plan of the `gui-toolkit` stage in docs/future/roadmap.md 「段の列」. Following roadmap 「進め方」, it is committed as the stage's first commit and deleted when the stage closes. Code and `docs/` do not cite it. Code that must be reproduced verbatim (the full `std/Gui.em`, the extern rows, the CI YAML) is in the appendix `docs/plans/2026-11-07-gui-toolkit/code.md`.

## 1. Goal and success criteria

- Goal
  - Native GUI for eml programs on Linux (Qt 6), Windows 11 (WinUI 3, Windows App SDK 2.x) and macOS (AppKit), through one generic toolkit layer that uses only what the three toolkits share, plus a headless stub backend for tests.
  - Layout is automatic: a grid laid out by Taffy, with auto-placement only. Placement flexibility is limited on purpose.
  - The API uses effects: a program builds its UI in direct style (`Gui.window`, `Gui.grid`, `Gui.button`, ...) and runs it under the handler `Gui.run`, then pulls user messages with `Gui.wait`.
  - Distribution and licensing are settled now, including the rules the later native/AOT stages must follow.
- Success criteria (checks in section 9)
  - The three example programs (counter, form, progress) run under `--gui stub` on all CI OSes with the expected output, and open working windows on each OS (manual check).
  - `cargo build` with default features succeeds on a Linux host without Qt; `nix build` produces an `eml` that links Qt 6 dynamically.
  - Every conformance check in 5.1 holds; all commands in 9 pass.

## 2. Scope

- In scope
  - `crates/eml_gui` (generic layer, Taffy layout, stub, Qt 6, WinUI 3, AppKit backends) (D-1, D-2, D-3)
  - `std/Gui.em` and the effect `Gui msg` (D-4)
  - `Async.start_off_main` / `Async.spawn_off_main` (D-4)
  - Main-worker I/O for Parks: `Pending::io`, `RealDriver::poll_fd`, the 10 ms cap (D-6)
  - The Gate cell kind, `eml_vm/src/gui.rs` (GuiPark, two-phase open, reaper, holds) (D-6)
  - Features `gui` (default) and `gui-native` (not default), CLI `--gui native|stub` / `EML_GUI` (D-7)
  - flake.nix, CI lanes, license notices, `docs/implementation/distribution.md` (D-8, D-9)
  - Spec sections, testing docs, and the close commit's `docs/future/` rewrites
- Out of scope (moved to roadmap 「その後の項目」 by the close commit, with 後に回した理由 / 見直す条件; list in T-12)
  - A declarative re-run + diff layer on top of these primitives
  - More widgets (text area, slider, list, image, radio group, tabs, scroll view), dialogs, menus, clipboard, drag and drop, keyboard shortcuts, fonts / colours / icons, custom painting
  - Explicit grid placement (`at row col`), named areas, user-set gap and padding
  - Windows IOCP as a GUI loop source; Windows 10
  - The `Plugin` trait in `eml_rt`
  - Repeated native `Gui.run` in one process outside `eml run` (Machine API hosts)
  - REPL GUI and Slint (stay deferred as they are)
  - The native-aot stage's implementation (only its rules are recorded, D-9)

## 3. Background context

- `Park` and `Pending` exist in `crates/eml_vm/src/park.rs`. `Park` has `wait(&mut self, Option<Duration>, &mut dyn Pending)` and `waker() -> Option<Arc<dyn IoWaker>>`. `Pending` has `run() -> Progress`, `default_wait()`, `set_holds(bool)`. `Progress = Wait(Option<Duration>) | Stopping | Busy`. There is no `Pending::io()` or `has_work()`; docs/spec/vm.md says so.
- `RunConfig.main_park: Option<ParkFactory>` and `Machine::with_main_park` exist but only tests set them; `eml_cli` never does, so `DefaultPark` is used. The factory is called once per run on the Machine thread before worker 0 is spawned (`crates/eml_vm/src/lib.rs:1043-1046`). `execute` forces `Seat::Separate`; the REPL uses `Seat::Merged`, which has no Park.
- While `Run.holds` is true, worker 0 skips the deadlock verdict (`workers.rs:1213-1222`, `1262-1271`); lowering holds from true to false wakes worker 0 (`vm.rs:988`).
- Pinned tasks: `Async.start_on_main` / `run_on_main` (std/Async.em:182-186) go through the holding extern `start_main_raw` (VM arm `vm.rs:2643-2661`). A task started by a pinned task is pinned too (docs/spec/stdlib.md around line 1265, docs/spec/vm.md 「`Async` のタスク」). So GUI code needs `start_off_main` to start effectful background work off the main thread.
- There is no `Plugin` trait, no `eml_gui`, no `Std.Gui`. The `tls` feature is a cfg switch: `eml_cli` (default) → `eml_vm/tls` → `eml_io/tls`; `crates/eml_vm/src/tls.rs` has an uninhabited `Conn` and an always-failing `client_conn` when off (IO failure `NotAvailable`).
- An extern needs a table row in `crates/eml_extern/src/lib.rs`, a std declaration and a VM arm. `crates/eml_hir/tests/externs.rs` and `crates/eml_core_ir/tests/externs.rs` tie them. An extern cannot build a tuple: `ret` is one Repr and translate builds the tuple only from `with_args` (`eml_extern/src/lib.rs:2349-2352`).
- Tasks wait through held `once` continuations in `park_*` externs or `Async.io_wait` on an `OpId`. No API lets an external thread hand a value to a task; `Std.Chan` is the cross-worker path.
- Language facts the API relies on: handlers are deep; one handler per effect, every op handled; a `once` continuation `k` is `Lin` (call once or `drop`); clauses capture only `Unr` values; `handle e from init with` threads handler state; `use f a` makes the rest of the block the last argument; types are rank-1; there are no named handlers; `handle` is a keyword.
- winio (MIT, 0.13 beta, compio-rs/winio) has Qt / WinUI / AppKit backends and a Taffy-based `winio-layout`, but its loop is a compio `block_on`, its program model is an Elm `Component`, and it picks one backend per build. Its WinUI backend creates windows only after `Application::Start` (`winio-ui-winui/src/runtime.rs:112-160`).
- Taffy 0.15.1 (MIT) supports CSS Grid with `fr`, `auto`, lengths, spans and row-major auto-placement; spans under auto-placement were verified by a probe. The measure closure is `FnMut(LayoutInput, NodeId, Option<&mut Ctx>, &Style) -> LayoutOutput`.
- Distribution today is `nix build` only (`flake.nix:98-123`, `meta.license = mit`), with `LICENSE.txt` and `THIRD-PARTY-LICENSES.md` in `share/doc/eml`. `about.toml` accepts MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, ISC, BSD-3-Clause, BSL-1.0, Unicode-3.0, Zlib, and ignores build dependencies. The CI `licenses` job runs `nix/third-party-licenses.sh --check`.
- Qt Core/Gui/Widgets are `LGPL-3.0-only OR GPL-2.0/3.0` under the open-source offering. Windows App SDK 1.2 is out of support; 2.x is the current line (2.6.5, serviced to 2027-04). `AddPackageDependency` (used by `winio-winui3`'s bootstrap) needs Windows 11. The Rust Reference warns that several Rust `staticlib`s in one link are likely to conflict.

## 4. Prerequisites

| Needed | For | How to check | If missing |
|---|---|---|---|
| Nix devShell (direnv) on macOS | all tasks | `nix develop -c cargo --version` → `cargo 1.99.1` | stop and ask |
| macOS host with a display | T-8 manual check | the counter window opens | ask the user to run it |
| A Linux desktop (X11 or Wayland) | T-9 manual check | `echo $DISPLAY $WAYLAND_DISPLAY` non-empty | ask the user; CI still checks build and stub tests |
| A Windows 11 machine with the Windows App SDK 2.x runtime | T-10 manual check | `Get-AppxPackage *WindowsAppRuntime.2*` lists a package | ask the user; CI still checks build and stub tests |
| GitHub Actions for the macOS, Linux and Windows `gui-native` lanes | T-8, T-9, T-10 | push the branch; the lane runs | stop and ask |

Execution decisions (user, 2026-11-07): the branch `plan/gui-toolkit` is pushed to origin from T-8 onward (no PR) so the `gui-native` lanes run, and T-9 / T-10 are done only when their lanes are green. The manual screen checks on Linux and Windows 11 (and on macOS, which an agent cannot look at) do not block the units; they are listed for the user in the final report.

## 5. Decisions

### 5.1 Long-term guidelines (changing these requires re-examination)

If implementation runs into a conflict with any of these, stop and report. Each guideline lists its conformance criteria (G-n); reviewers check the code against them.

#### D-1: Our own toolkit crate; winio as reference only
- Decision: `crates/eml_gui` is our own crate. No winio crate is a dependency except the WinUI bindings `winio-winui3` (bindings and bootstrap only). Third-party: `taffy` 0.15 (`default-features = false, features = ["std", "taffy_tree", "grid"]`); Qt via `cxx` + `cxx-build` + `qt-build-utils` 0.10 with our own C++ shim, Qt 6 only; AppKit via `objc2` 0.6, `objc2-foundation` / `objc2-app-kit` / `objc2-core-foundation` 0.3, `block2` 0.6. winio's shims may be read; copied snippets (MIT) are attributed.
- Rationale: winio's compio loop, Elm `Component` model and one-backend-per-build do not fit a VM-owned retained tree whose loop is the main worker's Park; it is pre-1.0 with separately versioned crates. The stub and a native backend must coexist in one binary.
- Rejected alternatives: depend on `winio-layout` / `winio-ui-*` — loop and model mismatch; fork winio — maintenance of 20 crates for 8 widgets.
- Scope of impact: `crates/eml_gui`, workspace dependencies.
- Revisit when: winio grows a backend-only API usable without its runtime, or the shim surface grows past what we can maintain.
- Conformance checks:
  - G-1 `eml_gui` MUST depend on no workspace crate and MUST NOT name `Park`, `Pending`, `Ctx` or any `eml_io` type. Check: `cargo tree -p eml_gui -e normal --target all --prefix none | grep -v '^eml_gui ' | grep -c '^eml_'` → 0 (CI step from T-1).
- Kept in the repository: docs/implementation/architecture.md 「crate の依存関係」 and the `eml_gui` section (T-1).

#### D-2: The generic layer and the widget set
- Decision:
  - `Widgets` (create, add_item, set, measure, place, show_window, destroy, set_presence) and `EventLoop` (run_loop, waker) are two `!Send` traits; `Backend::{Native, Stub(Script)}::create` returns `Native { widgets: Rc<RefCell<dyn Widgets>>, event_loop: Box<dyn EventLoop>, queue: EventQueue }`. Dispatch is dynamic so the stub and the native backend coexist. Backends key widgets by the toolkit `Id` (`(window_no << 32) | seq`, never reused).
  - `Toolkit` (widget table, container stack, per-window Taffy tree, measure cache, flush, poll) is a plain value; native callbacks only push into `EventQueue`.
  - The widget set of this stage, each with a native class in all three toolkits:

    | Kind (code) | props | event | Qt 6 | WinUI 3 | AppKit |
    |---|---|---|---|---|---|
    | Window | Text (title) | CloseRequest | `QWidget` top level | `Window` + root `Canvas` | `NSWindow` + flipped `NSView` |
    | Grid | – | – | `QWidget` | `Canvas` | flipped `NSView` |
    | Label (1) | Text | – | `QLabel` (wordWrap) | `TextBlock` (Wrap) | `NSTextField` label |
    | Button (2) | Text, Enabled | Click | `QPushButton` | `Button` | `NSButton` push |
    | TextField (3) | Text, Enabled | Text on every edit | `QLineEdit` (`textEdited`) | `TextBox` (`TextChanged`) | `NSTextField` (`controlTextDidChange`) |
    | CheckBox (4) | Text, Checked, Enabled | Toggle | `QCheckBox` | `CheckBox` | `NSButton` switch |
    | Choice (5) | Value (index), Enabled | Select | `QComboBox` | `ComboBox` | `NSPopUpButton` |
    | ProgressBar (6) | Value (0..100) | – | `QProgressBar` | `ProgressBar` | `NSProgressIndicator` bar |

  - Adding a widget later: it must have a native class with the same props and events in all three toolkits and need no emulation; it lands in `Kind`, the stub, all three backends, `std/Gui.em` and the UI tests in one commit. Otherwise it is a composite in eml or it waits.
- Rationale: the common denominator keeps three backends small and identical in behaviour; the stub makes every behaviour testable without a display.
- Rejected alternatives: one `Backend` trait with `run_loop(&mut self)` — the loop's `&mut` aliases the extern arms that create widgets inside `step` (B-1 of the first review); enum dispatch — the stub and native would not coexist cleanly.
- Scope of impact: `eml_gui` and every backend.
- Revisit when: a widget the users need exists in only two toolkits.
- Conformance checks:
  - G-14 Every `Kind` MUST have a native class in all backends and the stub with the same props and events; no behaviour emulation in the generic layer. Check: exhaustive `match Kind` in every backend.
  - G-22 Callbacks MUST only push into `EventQueue`; `Widgets::set` / `add_item` MUST NOT emit events; no `&mut` toolkit borrow may be live across a callback. Check: stub unit test that `set` emits nothing; review of the callback wiring.
  - G-25 Ids are `(window_no << 32) | seq` and never reused; a stale id fails `NotFound`; events of destroyed widgets are dropped. Check: eml_gui unit tests.
- Kept in the repository: architecture.md `eml_gui` section (T-1), docs/spec/stdlib.md 「`Std.Gui`」 (T-6).

#### D-3: Layout is an auto-placed Taffy grid
- Decision:
  - What eml can say: tracks `Auto | Fr Int | Px Int` for columns and rows (`rows = []` means implicit auto rows); auto-placement, row-major; per child `col_span`, `row_span`, `halign`, `valign` in `{Start, Center, End, Stretch}`, default `Stretch`; grids nest. Gap 8 and window padding 12 logical px, fixed.
  - Mapping: grid node `display: Grid`, templates from tracks (`auto()`, `fr(n)`, `length(n)`), `grid_auto_rows: auto()`, `gap: length(8)`; root grid `padding: length(12)` and `size: percent(1.0)`; child `grid_column` / `grid_row` = `Line { start: Auto, end: Span(n) }`, `justify_self` / `align_self` from the cell.
  - Leaf sizes come from `Widgets::measure` through Taffy's measure function (Qt `minimumSizeHint` / `sizeHint` / `heightForWidth`; WinUI `Measure` + `DesiredSize`; AppKit `fittingSize` with `preferredMaxLayoutWidth`), cached per widget and request, invalidated by `Text`, `add_item` and `RawEvent::Invalidate` (font, theme, DPI).
  - Window initial size = root at max-content; native minimum = min-content width, and a height no greater than the natural height (the height at the min-content width, clamped to the natural height, because a wrapping label is taller when narrower); on `Resized` relayout with the definite size and `place` only children whose rounded rect changed. Layout runs only in `flush` (dirty windows), on `Resized` and on `Invalidate`.
  - All numbers are logical px; no backend scales. Layout lives in Rust only.
- Rationale: simplicity first (user premise); measuring instead of winio's fixed preferred sizes gives wrapping labels a width-dependent height at the same cost.
- Rejected alternatives: explicit placement and named areas — not needed for forms, deferred; per-kind default alignment — needs an "unset" alignment value.
- Scope of impact: `eml_gui/src/layout.rs`, the `Std.Gui` layout API.
- Revisit when: auto-placement proves insufficient in a real program.
- Conformance checks:
  - G-10 No coordinate, size or DPI value crosses into eml. Check: no extern row returns geometry; `Event` has no size variant.
  - G-11 Every number in `eml_gui` is logical px; backends apply no scale factor. Check: grep for `devicePixelRatio`, `GetDpiForWindow`, `backingScaleFactor` finds only window clamping.
  - G-12 Layout runs only in `flush` (dirty), on `Resized` and on `Invalidate`; `Toolkit::set` applies the native property at once. Check: stub transcript order; layout count in a unit test.
  - G-13 Leaf sizes come from `Widgets::measure` through Taffy's measure function with a per-widget cache invalidated by `Text`, `add_item`, `Invalidate`; no fixed preferred sizes in the tree. Check: re-measure count unit test.
- Kept in the repository: stdlib.md 「`Std.Gui`」 layout paragraph (T-6).

#### D-4: `Std.Gui`: build once, mutate by handle, pull messages
- Decision:
  - `Gui msg` is a parameterised effect; its handler `Gui.run : (Unit -> <Gui msg, IO, Panic, Async> a) -> <IO, Panic, Async> a` runs the app pinned on the main worker (`Async.run_on_main`) and keeps the `Lin` toolkit handle `Ui` as handler state. Widget handles are `Unr` ids; each widget binds its user message at creation (`Gui.button "+" Inc`); `Gui.wait ()` flushes, then returns the next message. No diffing.
  - Public API (full text in the appendix): data `Track`, `Align`, `Cell`, `default_cell`; abstract `Window`, `Grid`, `Label`, `Button`, `TextField`, `CheckBox`, `Choice`, `ProgressBar`, `Sink msg`; class `Widget w` with `wid : w -> Int`; effect `Gui msg` with 17 ops (`begin_window`, `begin_grid`, `end`, `next_cell`, `label`, `button`, `text_field`, `check_box`, `choice`, `progress_bar`, `set_text`, `set_value`, `set_checked`, `set_enabled`, `close_window`, `sink`, `wait`); wrappers `window`, `grid`, `cell`, `text`, `enable`, `value`, `select`, `checked`, `post`, `run`. The effect is not abstract, so tests may write their own handler.
  - Background work: `compute` for pure work; `Async.start_off_main` / `Async.spawn_off_main` (new, `<IO, Panic, Async>` like `start` / `spawn`) for effectful work, which goes on the worker ring even when the starter is pinned; results come back with `Gui.post sink msg` through `Std.Chan`.
  - Example (counter):

    ```haskell
    counter : Unit -> <Gui Msg, IO, Panic, Async> Unit
    counter () =
      use _ <- Gui.window "Counter" Quit
      use Gui.grid [Fr 1, Fr 1] []
      let shown = Gui.cell { Gui.default_cell | col_span = 2 } (fn () -> Gui.label "0")
      let _ = Gui.button "-" Dec
      let _ = Gui.button "+" Inc
      let loop n =
        match Gui.wait () with
          | Inc -> Gui.text shown (show (n + 1))
                   loop (n + 1)
          | Dec -> Gui.text shown (show (n - 1))
                   loop (n - 1)
          | Quit -> println "final \{show n}"
      loop 0

    main : Unit -> <IO, Panic> Unit
    main () =
      use Async.run
      Gui.run counter
    ```
- Rationale: maps one-to-one onto retained native toolkits; every handler clause resumes `k` exactly once, which fits one-shot deep handlers; binding messages at creation removes handle comparison (there are no guards). The three examples type-check against the real rows.
- Rejected alternatives: re-run + diff (Elm / Compose) — needs keyed reconciliation, deferred as a layer on top; Concur-style widgets that return on action — racing needs multi-shot continuations; a non-parameterised `Gui` whose events carry handles — forces `if b == inc` chains; posting through a Rust mailbox — loses typing.
- Scope of impact: `std/Gui.em`, `std/Async.em`, every GUI program.
- Revisit when: two real programs show that handle bookkeeping dominates (then add the declarative layer over these primitives).
- Conformance checks:
  - G-15 Per event only `Int`s and at most one `String` cross the boundary; `msg` values are built in eml; posted values travel only through `Std.Chan`. Check: row signatures; the `post` body.
  - G-17 `Gui.run` has the type above and pins through `Async.run_on_main`; `Gui` ops carry no `IO` / `Async`. Check: `eml check` of the examples.
  - G-18 Every handler clause in `std/Gui.em` calls `k` exactly once and captures only `Unr` values; `Ui` appears only in the handler state. Check: review; `eml check`.
  - G-19 `wait` order: cancellation point, flush, native events, posted messages, the no-window / no-sink failure, then `gui_wait`; the Gate's sticky flag closes the race between polling and parking. Check: eml_vm tests (flood, poke between poll and park, cancel).
  - G-24 `start_off_main` / `spawn_off_main` place the task on the worker ring even under a pinned starter and do not count it in `pinned_tasks`; otherwise they behave as `start` / `spawn`. Check: thread-id test in `crates/eml_vm/tests/pinned.rs`.
- Kept in the repository: stdlib.md 「`Std.Gui`」 and 「`Std.Async`」 (T-3, T-6).

#### D-5: The eml / Rust boundary
- Decision: one extern row per primitive, no command buffer. The widget tree, Taffy trees, measure cache and event queue live in Rust; eml holds ids and a `Map Int (Handler msg)`. `Ui` (`Std.Gui.Ui`, `Lin`, `Foreign`) is threaded as `(R, Ui)` like `Std.Net` handles. `poll_raw` returns the widget id of the next event (0 = none); `event_int_raw` / `event_text_raw` read its payload; the handler registered for the id fixes the kind. Kind codes Label 1 … ProgressBar 6; prop codes value 1, checked 2, enabled 3; align Start 0, Center 1, End 2, Stretch 3; tracks as text `"a" | "f<n>" | "p<n>"` joined by `,`. The 19 rows and their exact shapes are in the appendix.
- Rationale: property sets apply at once and `wait` is the natural flush point; lists cross as text rather than by walking eml lists in arms; an extern cannot return a 5-tuple.
- Rejected alternatives: a command buffer — a second representation for no gain; one 5-tuple `poll_raw` — impossible.
- Scope of impact: `eml_extern`, `std/Gui.em`, `eml_vm/src/gui.rs`.
- Revisit when: a widget needs a payload that is neither an `Int` nor one `String`.
- Conformance checks: G-10, G-15 above; G-20 each new row lands with its std declaration, VM arm and spec text in one commit, and `cargo nextest run -p eml_hir -p eml_core_ir -E 'test(externs::)'` passes at every commit.
- Kept in the repository: stdlib.md 「`Std.Gui`」 codes and error table (T-6).

#### D-6: Threading: the native loop runs inside one `Park::wait` on the main thread
- Decision:
  - `eml_vm/src/gui.rs` owns `GuiPark: Park` (eml_gui knows nothing of `Park`). `eml run` installs it for every run. Until a program asks for a GUI it behaves exactly as `DefaultPark`.
  - Two-phase open (works for WinUI, whose XAML objects exist only inside `Application::Start`): `want_gui_raw` allocates a Gate cell and marks it as opening; the task parks in `Async.gui_wait gate`; the main worker's next `Park::wait` creates the backend and enters `EventLoop::run_loop`, which calls `step(Woken)` once as soon as the loop is ready; that step pokes the gate; the task resumes and calls `open_ui_raw gate`, so every `Widgets` call runs inside the native loop. `run_loop` returns only when `step` returns `Stop` (after `Progress::Stopping`), so there is one `run_loop` per run.
  - The Gate is a new registry cell kind: a sticky flag and at most one waiter. `poke_raw` (any worker, used by `post`) and `Pending::poke` (main thread) set the flag or wake the waiter through the existing `Route::Forward(owner)` path. Each Ui owns its gate; a gate abandoned before opening is released by `abandon_raw`.
  - Holds: raised in `want_gui_raw`, lowered by the reaper, `abandon_raw` or a failed `open_ui_raw` once nothing is wanted or open; worker 0 is woken per GUI period, never per event.
  - Abort paths: `Ui`'s payload is `UiToken { gen, reap: Arc<AtomicBool> }`; its drop only sets the flag. `step` reaps at its start and again on `Stopping`: close all windows, presence off, release the gate (`Pending::release_gate`), lower holds. Normal end, panic and cancellation all go through it.
  - The loop's only VM entries are `Pending::{run, poke, io, release_gate, set_holds(false)}` and the waker. It never touches the clock, the deadlock verdict, `END`, the registry directly, or any `Ctx`.
  - Main-worker I/O: `Pending::io()` returns the main worker's driver fd (unix; `RealDriver::poll_fd`), registered by the loop as a native source; `want_gui_raw` creates that driver on the real clock so the fd exists when the loop starts. When there is no source (Windows, virtual clock) and the driver has operations in flight, `Progress::Wait` carries at most 10 ms. A run called from the Park with a driver looks at the OS once.
  - Quit: with a Ui open, the OS quit gesture becomes one `CloseRequest` per window; with no Ui open it becomes the run's interrupt (as Ctrl-C). The loop never ends by itself (Qt `setQuitOnLastWindowClosed(false)`, AppKit `TerminateCancel`, WinUI no `Exit` before `Stop`). AppKit's activation policy is `Regular` only while a Ui is open.
  - REPL: no GUI park; `want_gui_raw` fails `NotAvailable`.
- Rationale: WinUI owns its pump, so the loop cannot return per event; returning once per run also removes the per-event worker-0 wake the roadmap item worried about. `eml_rt` (Verus-gated) needs no change because the VM owns the concrete driver.
- Rejected alternatives: a Park that returns per event plus `Pending::has_work()` — impossible with WinUI; a WinUI-only deferred command buffer — breaks G-12 / G-13 for one backend; running the whole main worker inside `Application::Start` from run start — initialises a toolkit for non-GUI programs; an `IoDriver::poll_fd` trait method in `eml_rt` — Verus gate for a unix accessor.
- Scope of impact: `eml_vm` (park.rs, cells.rs, sched.rs, workers.rs, vm.rs, gui.rs), `eml_io` (sys, real.rs), every backend's `event_loop.rs`.
- Revisit when: a toolkit can only be pumped from an extern (modal loops), or Windows pinned I/O shows latency from the 10 ms poll.
- Conformance checks:
  - G-4 The native loop runs only inside `Park::wait` on the Machine thread; `run_loop` calls `step(Woken)` once when ready and returns only on `Stop`; one `run_loop` per run. Check: stub counts in `crates/eml_vm/tests/gui.rs`.
  - G-5 Holds are true exactly while a GUI is wanted or open: raised only in `want_gui_raw`, lowered only when neither an opening is pending nor a `Ui` exists, by the reaper (`Pending::set_holds(false)`, skipped while `opening` is `Some`), by `abandon_raw`, or by an `open_ui_raw` that reports a creation error; one raise and one lower per GUI period on the normal, panic and cancel paths, where consecutive `Gui.run`s whose second `want` precedes the first `Ui`'s reap form one period. Check: `Finished.holds_raised` / `holds_lowered` in eml_vm tests.
  - G-6 Before `want_gui_raw`, GuiPark equals `DefaultPark` and creates no backend. Check: `RunStats` equality test.
  - G-7 Arms reach the toolkit only through the main-thread `GUI_SLOT`, with `RefCell` borrows scoped to the arm; with no slot they fail `NotAvailable`. Check: `!Send` assertions; spawned-thread test.
  - G-8 `Ui`'s payload is `Send` and its drop only sets the reap flag; the reaper (at the next `step`) closes windows, resets presence, releases the Gate and, when no other opening is pending, lowers holds after `close_ui`, panic or cancel. Check: stub transcript after a panicking / cancelled `Gui.run`.
  - G-9 The loop never reads or writes the clock, the deadlock verdict, `END`, the registry or any `Ctx` directly. Its only VM entries are `Pending::{run, poke, io}`, `Pending::{release_gate, set_holds(false)}` called only from `Slot::reap()`, the waker, and the run's interrupt closure on `QuitRequested` with no Ui open (G-23). Before the first `want_gui_raw`, `GuiPark::wait` calls only `Pending::default_wait()` (G-6). The arms are not the loop: their registry `release_gate` and `swap_holds` calls (in `want_gui_raw`, `abandon_raw` and `open_ui_raw`) follow D-13 and G-5. Check: the `Pending` methods named in `eml_vm/src/gui.rs` are exactly `default_wait`, `run`, `poke`, `io`, `release_gate`, `set_holds`; the calls through `Pending` (`pending.release_gate`, `pending.set_holds`) appear only inside `Slot::reap()`; review.
  - G-23 With a Ui open, `QuitRequested` becomes one `CloseRequest` per window; with none open it becomes the run's interrupt; presence is off when no Ui is open. Check: stub tests for both states.
  - G-3 In `eml repl`, `want_gui_raw` fails `NotAvailable`. Check: REPL transcript.
- Kept in the repository: docs/spec/vm.md 「GUI の Park」, 「メインのworkerの待ち」, 「セルと登録表」 (T-4, T-6).

#### D-7: Seam and features: a cfg switch, no `Plugin` trait
- Decision: the toolkit arms, GuiPark, Gate and reaper live in `eml_vm/src/gui.rs` in the shape of `tls.rs`; no `Plugin` trait in `eml_rt` this stage (accepted by the user; ffi.md decision #12 is rewritten). Features: `eml_vm/gui = ["dep:eml_gui"]`, `eml_vm/gui-native = ["gui", "eml_gui/native"]`; `eml_cli/gui = ["run", "eml_vm/gui"]` in `default`; `eml_cli/gui-native = ["gui", "eml_vm/gui-native"]` not in `default`. Off: `Ui` is uninhabited and `want_gui_raw` fails `NotAvailable "cannot open the GUI: not available"`. `gui` only: the stub works; the native choice fails `NotAvailable "cannot open the GUI: built without the native toolkit"`. `nix build`, the default devShell (on Linux with Qt) and one `gui-native` CI lane per OS build the native backends; every other lane keeps default features and needs no Qt.
- Rationale: one plugin does not justify a trait in the verified crate; the stub must run in every default build; Qt headers must not become a precondition of `cargo build`.
- Rejected alternatives: `Plugin` trait now — one implementer and the Verus gate; `gui-native` in `default` — every Linux lane would need Qt.
- Scope of impact: Cargo features of eml_gui, eml_vm, eml_cli; flake.nix; CI.
- Revisit when: a second plugin appears, or `jit-externs` needs one typed entry per row across plugins (then introduce `Plugin`).
- Conformance checks:
  - G-2 The features are exactly as above; only `gui-native` pulls Qt / objc2 / winui3 crates. Check: the CI `cargo tree` checks (T-7, T-10); `cargo build -p eml_cli` on a Linux host without Qt succeeds.
  - G-16 Every automated test uses the stub; native backends are compiled and linked in the `gui-native` lanes, never executed in CI. Check: CI YAML has no display setup; GUI UI tests carry `-- gui: stub (<reason>)`.
- Kept in the repository: architecture.md, CLAUDE.md feature lines (T-6, T-7, T-8); ffi.md and roadmap (T-12).

#### D-8: Distribution and licensing
- Decision:
  - End-user prerequisites: Linux — `eml` from `nix build` (closure has Qt 6, wrapped by `wrapQtAppsHook`) or, outside Nix, `libQt6{Core,Gui,Widgets}.so.6`, `qt6-qpa-plugins` and a display. Windows 11 (build 22000+) — the Windows App SDK 2.x runtime installed by the user from Microsoft, and the VC++ redistributable. macOS — nothing.
  - Qt is linked dynamically only, in `eml` and in every later AOT output. eml's distribution carries the Qt notice, the GNU GPL v3 and LGPL v3 texts (LGPLv3 §4b requires both) in `share/doc/eml/`, and a "system libraries" section in THIRD-PARTY-LICENSES.md generated through `about.hbs`. Who conveys what: (A) eml / its Nix closure — notices and texts; a binary cache that publishes the closure conveys Qt and relies on nixpkgs' public source; a future tarball that bundles Qt owes a source pointer for that exact Qt. (B) an author of an AOT program linked against the user's system Qt — notice, texts, dynamic linking preserved, no Qt source duty. (C) an author who bundles Qt — also the source duty.
  - Windows: eml ships nothing of Microsoft's (no bootstrap DLL, no winmd, no installer); the bootstrap is `AddPackageDependency` for the family `Microsoft.WindowsAppRuntime.2_8wekyb3d8bbwe` (winui3 `V2`). A missing runtime fails `open_ui_raw` with `NotAvailable` naming Microsoft's download page. Self-contained deployment is out of scope.
  - macOS: no `.app` bundle; the linker's ad-hoc signature suffices locally; an author distributing a binary signs with Developer ID, hardened runtime and a secure timestamp, and notarizes it (a bare executable inside a zip, no stapling).
  - New crates (taffy, slotmap, smallvec, serde family, cxx family, foldhash, objc2 family, windows-* 0.100 family, winio-winui3, winio-windows-subset) all have an accepted license alternative; `about.toml` `accepted` does not change.
  - `docs/implementation/distribution.md` states all of the above for program authors.
- Rationale: user premise P9; dynamic linking is the LGPL route that puts no source duty on authors using system Qt; shipping nothing of Microsoft's keeps the Windows App SDK license terms off eml.
- Rejected alternatives: static Qt — relinking duty and risk; bundling the Windows App SDK bootstrap DLL — attaches Microsoft's distributable-code terms; self-contained WinUI — documented only for MSBuild.
- Scope of impact: flake.nix, about.hbs, licenses/, docs/implementation/distribution.md.
- Revisit when: the project publishes a binary cache or a tarball; Microsoft's 2.x license is reviewed.
- Conformance checks:
  - G-21 Qt 6 linked dynamically and Qt 5 rejected at build time; Windows 11 minimum and the Windows App SDK 2.x runtime via `AddPackageDependency`, no SDK files needed at build time or shipped; a missing runtime gives `NotAvailable` naming the installer page; AppKit uses a flipped content view and `set_presence`. Check: `build.rs` panics on Qt 5; the Windows lane builds without the SDK; the bootstrap test; `isFlipped` present.
  - G-26 eml's distribution ships no Windows App SDK files and no Qt binaries outside the Nix closure. Check: the CI `nix build` step (T-9).
  - G-27 Qt is dynamic everywhere. Check: `readelf -d result/bin/.eml-wrapped | grep libQt6Widgets.so.6`.
  - G-28 THIRD-PARTY-LICENSES.md carries the system-library section via `about.hbs`; `share/doc/eml/` carries the GPLv3 and LGPLv3 texts; `nix/third-party-licenses.sh --check` passes. Check: CI `licenses` job.
  - G-30 `docs/implementation/distribution.md` states the author duties (Qt, Windows runtime, macOS signing, AOT notices) and stdlib.md 「`Std.Gui`」 cites it.
- Kept in the repository: docs/implementation/distribution.md (T-11), about.hbs and licenses/ (T-9).

#### D-9: Rules for the native-aot stage
- Decision: `eml build` takes a build profile, `--profile console|gui`, default `console` (user decision). The profile alone decides what is linked; nothing is inferred from imports:

  | | `console` (default) | `gui` |
  |---|---|---|
  | runtime staticlib | `libeml_rt.a` | `libeml_rt_gui.a` |
  | a program that imports `Std.Gui` | build error: `` `Std.Gui` needs the gui profile (eml build --profile gui) `` | builds |
  | Windows | `/SUBSYSTEM:CONSOLE` | `/SUBSYSTEM:WINDOWS` with `/ENTRY:mainCRTStartup`, so the entry stays `main` and no console window appears |
  | macOS | bare executable | bare executable (an `.app` bundle is a later option of this profile) |
  | Linux | – | links Qt 6 dynamically |

  The `Std.Gui` check is a module-graph fact `eml build` reports before linking (a machine-fact error with exit code 2, like `--link` failures, not a source diagnostic: the program is valid eml, and `eml run` still runs it). Profiles name link configurations only; they never change the program's meaning, and a future plugin adds its code to the `gui` variant or becomes a new profile, never an add-on library. Exactly one Rust staticlib is linked into any AOT program; the two variants are `libeml_rt.a` (no GUI) and `libeml_rt_gui.a` (runtime, the toolkit arms, the Park adapter, holds / Gate / reaper logic, `eml_gui`, the shim objects). `eml run` (the VM) has no profile; its `--gui native|stub` and the `gui-native` feature are unrelated.
  The C runtime and the platform GUI libraries are linked dynamically by default: Linux glibc, `libstdc++` (only for the `gui` profile's Qt shim) and Qt 6; macOS `libSystem`, `libobjc`, AppKit and Foundation; Windows the UCRT and `vcruntime140.dll` (dynamic CRT, so the VC++ redistributable is a prerequisite), with the Windows App SDK never named at link time (bootstrapped at run time) and system DLLs imported through `raw-dylib`. Both runtime staticlibs and the final link use the same CRT setting. The full variant is built against floors (Qt 6.2 headers, glibc 2.35 / libstdc++ 12, i.e. Ubuntu 22.04) and records them; `eml build` refuses a host whose Qt is older (a machine-fact error, not a diagnostic). eml ships `THIRD-PARTY-LICENSES-runtime.md` beside the staticlibs and `eml build` copies it next to its output; redistributing the output requires it. Link inputs: Linux `cc`, `-lstdc++`, Qt by versioned soname (pkg-config preferred); Windows `link.exe` / `lld-link`, no SDK import libraries; macOS Xcode CLT, `-framework AppKit -framework Foundation`.
- Rationale: several Rust staticlibs in one link conflict (the personality-symbol fix lands in rustc 1.101, after the pinned 1.99.1); compiling the shim on the user's machine would need a C++ compiler and Qt dev packages.
- Rejected alternatives: a separate `libeml_gui.a` — staticlib conflict; build the shim at `eml build` time — extra user prerequisites; picking the variant from the imports — the Windows subsystem and later bundling are choices of the author, not facts of the program; allowing `Std.Gui` under `console` with the GUI runtime — the profile would no longer decide what is linked; letting `Std.Gui` fail at run time under `console` — a build that cannot work should not succeed.
- Scope of impact: the native-aot stage; docs/future/native.md 「AOT と配布」.
- Revisit when: the native-aot stage designs its packaging, or rustc makes several staticlibs safe.
- Conformance checks: G-29 one staticlib per program, chosen only by `--profile` (default `console`); a `console` build of a program that imports `Std.Gui` fails before linking; a `gui` build on Windows links `/SUBSYSTEM:WINDOWS /ENTRY:mainCRTStartup`; G-31 the runtime notices file is shipped and copied by `eml build`; G-32 the floors are recorded and checked. These are checked in the native-aot stage.
- Kept in the repository: docs/future/native.md 「AOT と配布」 (T-12).

### 5.2 Short-to-mid-term design

May be adjusted during implementation for good reason; record any adjustment in the commit message.

#### D-10: Commit split
11 commits plus the close (section 7). The Gate cell kind lands together with its only creator, the GUI arms (T-6), so no commit has dead code. Every commit leaves `cargo nextest run`, `cargo clippy --all-targets -- -D warnings`, the feature-combo clippy list and the CI `licenses` job green.

#### D-11: `Pending` and driver changes
- `Pending` gains `io(&self) -> Option<PollSource>` (T-5), `poke(&mut self, gate: i64)` and `release_gate(&mut self, gate: i64)` (T-6). `PollSource` is a new enum in park.rs (`#[cfg(unix)] Fd(RawFd)`, no Windows variant this stage).
- `eml_io`: the `Poller` trait gets `#[cfg(unix)] fn raw_fd(&self) -> RawFd`; `Driver::backend`; `#[cfg(unix)] pub fn RealDriver::poll_fd(&self) -> RawFd`. `RunClock::driver` (eml_vm `driver.rs:66`) stays the single place that picks the driver type and returns `(Box<dyn IoDriver>, Option<PollSource>)`; `Scheduler` keeps `poll: Option<PollSource>`, set in `own_driver` (`sched.rs:594-604`).
- `MainPending` gets `from_park: bool`; at the start of its `run`, after `unpark`, when called from the Park with a driver, it calls `driver.wait(Some(now))` once: a non-blocking OS check that clears a leftover wake on the driver fd and moves ready completions into the driver, which the first collect hands over as usual. `switch_away` is unchanged. (Adjusted during T-5: forcing the first collect's `look` left the fd readable whenever nothing was in flight, because a ring of the main worker also rings its driver waker, so a Park waiting on the fd spun at 100% CPU.) `retire_driver` also clears `Scheduler.poll`. The 10 ms cap is tested by an eml_vm unit test of `main_timeout` on every OS, because on unix a real-clock driver always has a source and the virtual clock gives the main worker no driver.
- `main_timeout` (`workers.rs:1320-1325`) is replaced so the 10 ms cap applies even with no own deadline (appendix).

#### D-12: Gate cell details
`CellKind::Gate(GateCell { flag: bool, waiter: Option<CellRef> })`. `open_gate`, `poke_gate` (stale or released target: no-op; waiter: wake WOKE; else set flag), `release_gate` (not live: Ok; with a waiter: `Internal`), `park_gate` (suspend, cancellation point, flag → clear and resume WOKE, second waiter → `Internal`, else wait). Parking on a released cell stays `Internal` for every kind, including Gate; std never does it because each Ui owns its gate and `abandon_raw` releases an unopened one. `unpark`'s cancellation path clears the waiter; `left_at_end` ignores gates; `sweep_all` frees them. With `gui` off, `open_gate` and the variant carry `#[cfg_attr(not(feature = "gui"), allow(dead_code, reason = "only the GUI arms open a gate (docs/spec/vm.md 「セルと登録表」)"))]`.

#### D-13: `gui.rs` structure
- `Slot` (on the main thread, `Rc`, also in thread-local `GUI_SLOT`): `backend: Cell<Option<Backend>>`, `parts: RefCell<Option<Parts { widgets, queue }>>` (the created `Native` without its `event_loop`, which moves into `GuiPark`), `create_error: RefCell<Option<GuiError>>` (sticky for the run, no retry), `opening: Cell<Option<i64>>`, `poked: Cell<bool>`, `toolkit: RefCell<Option<Toolkit>>`, `ui: RefCell<Option<UiState { gen, gate, reap }>>`, `event: RefCell<Option<CurrentEvent>>`, `gens: Cell<u32>`, `waker: Arc<GuiWaker>`. `GuiPark { slot, event_loop: Option<Box<dyn EventLoop>>, interrupt }`; `pub fn park(backend, interrupt) -> ParkFactory`.
- `GuiPark::wait` and `step`: pseudo-code in the appendix. `step` reaps first, drains control events (`QuitRequested`, `Resized`, `Invalidate`), pokes `opening` once, pokes the Ui's gate on events, runs `pending.run()`, reaps again on `Stopping` before returning `Stop`, and repeats while a reap or an unpoked opening is pending.
- Arms: `want_gui_raw` (no slot → `NotAvailable`; live unflagged Ui or pending opening → `Other "cannot open the GUI: it is already open"`; else create the main worker's driver on the real clock, open a gate, raise holds, set `opening`); `open_ui_raw gate` (reports `create_error`: no native toolkit, `no display (DISPLAY and WAYLAND_DISPLAY are unset)`, missing Windows runtime, after releasing the gate and lowering holds; else build the `Toolkit`, presence on, `UiState`, return the `Ui`); `abandon_raw` (releases the gate; lowers holds when it cleared `opening`); `close_ui` (decref; the drop sets `reap`); toolkit arms through `GUI_SLOT`. `GuiError` maps to IO failures: `NoContainer` / `NothingOpen` → `Other "gui: no open container"`, `UnknownId` → `NotFound "gui: unknown or closed widget"`, `WrongKind` → `Other "gui: the widget has no such property"`, `OutOfRange` → `Other "gui: a value is out of range"`, `NotAvailable(s)` → `NotAvailable s`, `Backend(s)` → `Other s`. Rows that consume the `Ui` call `ctx.heap_mut().decref(ui)` before raising.
- Values: ProgressBar clamps to 0..=100; a Choice index must be `0 ≤ i < items` (`select` on an empty Choice is `OutOfRange`; std skips the initial select for an empty list). Text applies to Window, Label, Button, TextField, CheckBox; Value to ProgressBar and Choice; Checked to CheckBox; Enabled to all but Window, Grid, Label.
- Holds are counted in `Run.holds_raised` / `holds_lowered` (`AtomicU64`, transitions in `swap_holds`, `workers.rs:721`) and surfaced as `Finished.holds_raised` / `Finished.holds_lowered` (not in `RunStats`, so `bench.rs` and `--stats` do not change).

#### D-14: Stub backend and scripts
Measure `w = 7 * chars + 16`, `h = 24` (Choice: longest item; ProgressBar: 0 chars). Transcript lines (thread-local, `eml_gui::stub::take_transcript()`), script format (`.gui` files: `click W:S`, `text W:S <rest>`, `toggle W:S true|false`, `select W:S <int>`, `close W:S`, `quit`) and loop rules are in the appendix. A script that is empty from the start sends one `CloseRequest` per shown window after the first step; otherwise one scripted event goes after each step. After 5 s with nothing to do, `run_loop` fails `stub: nothing woke the loop for 5 s`.

#### D-15: CLI and test marks
`eml run --gui native|stub` (absent: `EML_GUI`, then `native`); an invalid value prints `` `{v}` is not a GUI backend (native or stub) `` (prefixed `EML_GUI: ` when it came from the variable) and exits 2, like `workers_from_env` (`main.rs:189-210`). `eml run` installs `gui_park(backend, interrupt)` with `interrupt = { control.interrupt(); doorbell.ring() }`. GUI UI tests carry the mark `-- gui: stub (<reason>)` (parsed by `reason_mark`, only on run / run-fail entries) and read scripted events from a `<name>.gui` side file like `.args`.

#### D-16: Backend loop sketches (API facts unverified; fallbacks bound the change to `event_loop.rs` / `build.rs`)
- AppKit: `NSApp.run()` with a `CFRunLoopSource` waker, a `CFFileDescriptor` for the io fd (created with `closeOnInvalidate = false`, since the fd belongs to the driver, and invalidated before `run_loop` returns) and a one-shot `CFRunLoopTimer`; `step` behind a re-entrancy guard; `Stop` → `NSApp.stop` plus a posted application-defined event. Fallback: a manual `nextEventMatchingMask` / `sendEvent` / `updateWindows` pump with the same CF sources.
- Qt: `QApplication` singleton, `setQuitOnLastWindowClosed(false)`, loop on `processEvents(WaitForMoreEvents)`; waker `QAbstractEventDispatcher::instance()->wakeUp()`; `QSocketNotifier` for io; one-shot `QTimer`; signals blocked during `set`; `closeEvent` override ignores and pushes `CloseRequest`. build.rs via `qt-build-utils` `=0.10.0`, rejecting Qt 5; fallback: `qmake -query` directly.
- WinUI: the first `run_loop` enters `Application::Start`; `OnLaunched` stores the `DispatcherQueue`, installs the waker (`TryEnqueue`, coalesced) and a `DispatcherQueueTimer`, and calls `step(Woken)`; `Stop` → `Application::Current().Exit()`; `AppWindow.Closing` with `Cancel = true` pushes `CloseRequest`; `io` is `None`. Crates `winui3 = { package = "winio-winui3", version = "=0.7.0-alpha.8" }`, `windows = { package = "winio-windows-subset", version = "=0.3.0-alpha.3" }`, `windows-core = "0.100"`. Fallback: the newest alpha that builds, else vendor the generated bindings (MIT) into `src/winui/bindings.rs`.

## 6. Implementation constraints and conventions

- Test first: write the failing test, then the code (CLAUDE.md).
- Every test finishes within 1 s alone (`cargo nextest run -j 1`); the stub runs take milliseconds, and the longest sleep in the tests is 10 ms.
- Each new extern row lands with its std declaration, VM arm, test-table entries and spec text in one commit (D-5, G-20).
- `docs/spec/` sections are written in the commit that implements the rule, and code comments cite them, never this plan (roadmap 「進め方」).
- Japanese for docs and code comments, drafted in English and translated with PLaMo (`form: plain`, identifiers in the glossary); no space between Japanese and Latin characters or inline code.
- Native backends are never executed in CI (D-7, G-16); only the stub runs there.
- `eml_rt` does not change (D-6). If a task seems to need it, stop and report.
- Test changes: kind 3 (mechanical) — the two `Finished { .. }` literals in `crates/eml_cli/tests/ui.rs` (around lines 510 and 556) gain `holds_raised: 0, holds_lowered: 0` (T-6). Kind 2 (expected value) — T-6 changes the stub's rule 5a (appendix section 2) so that the 5 s idle limit also applies with an I/O source, counted from the last ring, scripted event or timeout; `step(Io)` alone does not reset it. The reason: under the old rule a real-clock stub run whose program is stuck (no ring, no script left) steps I/O every 10 ms forever until nextest kills it, instead of failing with the stub's idle error. The eml_gui unit test `run_loop_with_an_io_source_steps_io_every_10_ms_and_never_gives_up` becomes `run_loop_with_an_io_source_steps_io_every_10_ms` (its five 10 ms `Io` waits outlasted the test loop's 20 ms idle limit, which now fails the run, so it sets the limit back to the real 5 s and steps two `Io` waits instead of five: the expected `reasons` lose three `Io` entries and the `elapsed >= 50 ms` check goes), and the new `run_loop_with_an_io_source_fails_when_only_io_steps_follow` pins the failure. If T-5's look changes a counter in a `RunStats` snapshot of a test with a custom `Park` (`seat::`, `pinned::`), that is a kind-2 change of exactly those snapshots, recorded in T-5's commit message; no other existing expectation may change.
- If the LIR verifier panic "(lin) is released, but only unr can be in …" appears (it reproduces with a destructured `Lin` eml record inside a `try_panic` wrapper, not with the real module), fix it in a separate preparatory compiler commit with an `eml_lir` test, then continue.
- Every commit that touches ci.yml passes `nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- .github/workflows/ci.yml`.

## 7. Implementation tasks

| ID | Task | Based on | Depends on | Parallelizable |
|---|---|---|---|---|
| T-1 | Stage plan commit (this document and its appendix) | – | – | no |
| T-2 | `eml_gui` skeleton: types, traits, `Toolkit` without layout, `EventQueue`, stub, unit tests, CI G-1 step | D-1, D-2, D-14 | T-1 | no |
| T-3 | Taffy layout, measure cache, `Invalidate` / `Resized`, frame fixtures | D-3 | T-2 | with T-4, T-5 |
| T-4 | `Async.start_off_main` / `spawn_off_main` | D-4, D-5 | T-1 | with T-3 |
| T-5 | Main-worker I/O for Parks | D-6, D-11 | T-1 | with T-3, T-4 |
| T-6 | GUI core in eml_vm and `std/Gui.em` | D-4, D-5, D-6, D-7, D-12, D-13 | T-3, T-4, T-5 | no |
| T-7 | CLI, UI tests, REPL transcript, feature-combo CI | D-7, D-15 | T-6 | no |
| T-8 | AppKit backend, `gui-native` features, macOS lane | D-2, D-7, D-16 | T-7 | no |
| T-9 | Qt backend, flake.nix, notices and license texts, Linux lane | D-2, D-7, D-8, D-16 | T-8 | with T-10 |
| T-10 | WinUI backend, Windows lane, G-2 check | D-2, D-7, D-8, D-16 | T-8 | with T-9 |
| T-10b | eml's own WinUI 3 bindings and bootstrap (replaces winio-winui3; user decision) | D-8, D-16 | T-10, T-11 | no |
| T-11 | `docs/implementation/distribution.md` | D-8, D-9 | T-9, T-10 | no |
| T-12 | Close the stage | all | T-11 | no |

### T-1: Stage plan commit
- Purpose: roadmap 「進め方」 commits the plan first.
- Targets: `docs/plans/2026-11-07-gui-toolkit.md`, `docs/plans/2026-11-07-gui-toolkit/code.md`.
- Work: commit both files as `Plan gui-toolkit`.
- Left to the implementer: none.
- Condition: always.
- Completion criteria: `git show --stat HEAD` → only the two plan files.

### T-2: `eml_gui` skeleton
- Purpose: the generic layer and the stub everything else is tested against.
- Targets: `crates/eml_gui/{Cargo.toml, src/{lib,toolkit,backend,queue,stub}.rs}`, workspace `Cargo.toml`, `Cargo.lock`, `.github/workflows/ci.yml`, `docs/implementation/architecture.md`.
- Work:
  - Types and traits per D-2 and appendix section 1 (public types of eml_gui): `Id`, `Kind`, `Prop`, `Init`, `Event`, `RawEvent { User, Resized, Invalidate, QuitRequested }`, `Track`, `Align`, `Cell`, `GuiError { NoContainer, NothingOpen, UnknownId, WrongKind, OutOfRange, NotAvailable(String), Backend(String) }`, `Reason`, `Step`, `Measure { Exact, AtMost, Unbounded }`, `Size`, `Rect`, `Script`, `Backend`, `Native`, `Widgets`, `EventLoop`, `EventQueue` (`push`, `take_control`, `pop_user`).
  - `Toolkit`: `begin_window`, `begin_grid`, `end`, `next_cell`, `create`, `add_item`, `set`, `close_window`, `flush` (shows new windows; no layout yet: place at zero), `poll` (drops events of destroyed widgets), `open_windows`, `close_all`, `request_close_all`, `has_events`.
  - Stub per D-14 (measure, transcript, script parse, loop, auto-close, 5 s failure). Without the `native` feature, `Backend::Native` fails `NotAvailable("cannot open the GUI: built without the native toolkit")`.
  - Unit tests (in `src/*`): id encoding and no reuse; stale id → `UnknownId`; events of destroyed widgets dropped; `request_close_all` fan-out; `set` / `add_item` emit nothing; stub `run_loop` calls `step(Woken)` first even after an early ring; script parse errors; empty-script auto-close; exhaustive `match Kind`; `!Send` assertions; `take_control` leaves user events queued.
  - CI: the 「dependency edges of eml_gui」 step (appendix).
  - architecture.md: `eml_gui` section and its place in 「crate の依存関係」 (no workspace dependency).
  - Record the release binary size of `eml` before and after in the commit message.
- Left to the implementer: `Toolkit` internals.
- Condition: always.
- Completion criteria:
  - `cargo nextest run -p eml_gui` → all pass.
  - `cargo tree -p eml_gui -e normal --target all --prefix none | grep -v '^eml_gui ' | grep -c '^eml_'` → `0`.
  - `cargo nextest run` and `cargo clippy --all-targets -- -D warnings` → green.
  - actionlint → no output.
- Pitfalls: `eml_gui` must not be a dependency of any crate yet except as a workspace member.

### T-3: Taffy layout
- Purpose: D-3.
- Targets: `crates/eml_gui/src/{layout.rs,toolkit.rs}`, `Cargo.toml` (taffy), `Cargo.lock`, `THIRD-PARTY-LICENSES.md` only if the default graph changed (it does not: `eml_gui` is not yet reachable from `eml_cli`).
- Work: the mapping of D-3; measure cache invalidated by `Text`, `add_item`, `Invalidate`; frame diffing on `Resized`; `Toolkit::settle` for `Resized` / `Invalidate`. Fixtures (stub measure; window grid `[Fr 1]`, padding 12, gap 8):

  | Case | Frames |
  |---|---|
  | counter (`[Fr 1, Fr 1]`, label span 2, `-`, `+`), natural | window 78x80 min 78x80; grid (12,12) 54x56; label (0,0) 54x24; minus (0,32) 23x24; plus (31,32) 23x24 |
  | counter resized to 300x200 | grid (12,12) 276x176; label (0,0) 276x84; minus (0,92) 134x84; plus (142,92) 134x84 |
  | form (`[Auto, Fr 1]`; "Name", empty field, check box span 2, submit span 2 halign End) | window 92x112; grid (12,12) 68x88; name (0,0) 44x24; field (52,0) 16x24; agree (0,32) 68x24; submit (10,64) 58x24 |

  Plus: after a label's text changes, the next flush measures only that label, and each distinct (widget, request) pair at most once (Taffy's grid asks a leaf several different questions per layout, so "once" means once per request, not one call); an unchanged flush measures nothing (G-13); layout runs once per dirty flush and per resize (G-12). The T-2 test that asserts the zero placement (`flush_shows_new_windows_once_and_places_new_children`) gets the laid-out values: a kind-3 follow-up of the placeholder T-2 introduced, noted in the commit message. The native minimum is set when a window is first shown and not updated later in this stage (Q-14).
- Left to the implementer: whether the Taffy tree is reused across flushes.
- Condition: always.
- Completion criteria: `cargo nextest run -p eml_gui` → all pass, fixtures exact; workspace nextest and clippy green.
- Pitfalls: round with Taffy's `use_rounding`; compare rounded rects when diffing. `Measure { Exact, AtMost, Unbounded }` must still express Taffy's min-content request, which the window minimum needs (D-3): map `AvailableSpace::MinContent` to `AtMost(0.0)` and document it in the code.

### T-4: `Async.start_off_main` / `spawn_off_main`
- Purpose: D-4 (G-24).
- Targets: `crates/eml_extern/src/lib.rs` (row `AsyncStartOffMainRaw` after `AsyncStartMainRaw`, around line 1259), `std/Async.em` (wrappers after `run_on_main`, around lines 185-186; extern after `start_main_raw`, around line 393), `crates/eml_vm/src/vm.rs` (arm), `is_async` list, `crates/eml_core_ir/tests/externs.rs` tables, `crates/eml_vm/tests/pinned.rs`, docs/spec/stdlib.md 「`Std.Async`」「メインのworker」, docs/spec/vm.md 「`Async` のタスク」.
- Work: code in the appendix. In the `AsyncStartRaw | AsyncStartMainRaw` arm (`vm.rs:2643-2661`) add the new variant with `let pinned = (pinned || ext == AsyncStartMainRaw) && ext != AsyncStartOffMainRaw` and owner `place_owner(sched, false)` (`vm.rs:3666`). Tests: `spawn_off_main` from a pinned task runs on a non-main thread and is not counted in `pinned_tasks`; closing and cancelling a pinned scope with an unpinned child. Spec: the held list and row count in stdlib.md.
- Left to the implementer: how the test observes the thread (thread-id probe).
- Condition: always.
- Completion criteria:
  - `cargo nextest run -p eml_vm --test integration pinned::` → all pass.
  - `cargo nextest run -p eml_hir -p eml_core_ir` → all pass.
  - workspace nextest and clippy green.
- Pitfalls: the wrappers' rows are `<IO, Panic, Async>` as `start` / `spawn` (std/Async.em:108, 112, 182); only the raw extern is `<IO, Async>`.

### T-5: Main-worker I/O for Parks
- Purpose: D-6, D-11.
- Targets: `crates/eml_io/src/{sys/kqueue.rs, sys/epoll.rs, sys/unix.rs, driver/mod.rs, real.rs}`; `crates/eml_vm/src/{park.rs, driver.rs, workers.rs, sched.rs, vm.rs, lib.rs}`; tests; docs/spec/vm.md 「メインのworkerの待ち」「時計と driver」.
- Work: D-11 (code for `main_timeout` in the appendix). Tests: unix only, a `TestPark` polls `pending.io()`'s fd with `poll(2)` and calls `run`, and a pinned `Std.Net` exchange on the real driver finishes (`crates/eml_vm/tests/pinned.rs`); on every OS, with a Park that ignores `io()`, `main_timeout` gives at most 10 ms while an operation is in flight; `eml_io` unit test: `poll_fd` becomes readable when a completion is pending. Spec: replace the sentence that says there is no `Pending::io` with the rule.
- Left to the implementer: none beyond test plumbing.
- Condition: always.
- Completion criteria:
  - the new tests pass; `cargo nextest run -p eml_vm --test integration bench::` → unchanged snapshots.
  - `nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=address CARGO_TARGET_DIR=target/asan cargo test -Zbuild-std --target x86_64-unknown-linux-gnu -p eml_vm --test integration -- io:: pinned::` (Linux) → pass; on macOS skip and leave it to CI.
  - workspace nextest and clippy green.

### T-6: GUI core in eml_vm and `std/Gui.em`
- Purpose: D-4 to D-7, D-12, D-13.
- Targets: `crates/eml_vm/src/{cells.rs, park.rs, vm.rs, gui.rs (new), lib.rs, workers.rs}`, `crates/eml_vm/Cargo.toml`, `crates/eml_cli/Cargo.toml`, `crates/eml_extern/src/lib.rs` (19 Gui rows after `TlsTakeLine` around line 2144, `GuiUi` after `TlsConn` around line 279), `std/Gui.em` (new, appendix), `std/Async.em` (`gui_wait`), `crates/eml_hir/src/lib.rs` (STD list, after `Http.em`), `crates/eml_core_ir/tests/externs.rs` tables (appendix), `crates/eml_vm/tests/{gui.rs, gui_off.rs, main.rs}`, `crates/eml_cli/tests/ui.rs` (two `Finished` literals), `THIRD-PARTY-LICENSES.md`, docs/spec/{stdlib.md, vm.md, modules.md, object-model.md, core-ir.md}, docs/implementation/architecture.md, `CLAUDE.md` (architecture block line for `eml_gui`), `crates/eml_gui/src/stub.rs` (rule 5a with an I/O source and its unit tests; the kind 2 change in section 6).
- Work:
  - Gate cell (D-12) with cells.rs unit tests: poke before park sets the flag and park returns WOKE; poke after park wakes the waiter; the cancellation point wins over the flag; park on a released gate stays `Internal`; a second waiter is `Internal`; `left_at_end` ignores a gate; `unpark` clears the waiter; `release_gate` with a waiter is `Internal`; a stale poke does nothing.
  - `Pending::poke` / `release_gate` and their `MainPending` impls (helper shared with the `GuiPokeRaw` arm, in the pattern of `ChanCloseRaw`, `vm.rs:3018-3034`).
  - `gui.rs` per D-13 with the tls.rs shape for `gui` off. `GuiPokeRaw` and `GuiAbandonRaw` are feature-independent arms; `AsyncStartOffMainRaw` (already there), `GuiWantRaw`, `GuiOpenUiRaw`, `GuiPokeRaw`, `GuiAbandonRaw` go into `is_async` and the `Op::Async` list (`vm.rs:4674-4776`); the other 15 are `run_extern` arms next to the Tls arms.
  - Features per D-7 (`gui` default in eml_cli). Regenerate THIRD-PARTY-LICENSES.md: `nix develop -c nix/third-party-licenses.sh` (adds taffy, slotmap, smallvec, maybe serde).
  - `crates/eml_vm/tests/gui.rs` (`#![cfg(feature = "gui")]`, stub): 1 counter, exact transcript (inline insta), one `run_loop`, holds 1/1, `final 1`; 2 a non-GUI program under the GUI park has the same `RunStats` as under `DefaultPark` and an empty transcript; 3 two sequential `Gui.run`s issued from one pinned task: holds 1/1 (the second `want` lands in the same `pending.run()` as the first `close_ui`, before the reap, so the two form one GUI period; issued from `main` the reap may win, so the count is fixed only in this shape, which the test's comment says), one `run_loop`, both windows created and destroyed; 4 a panicking `Gui.run`: `destroy` and `presence off` in the transcript, a later deadlock in the same program is still reported (that report is the evidence that holds were lowered: a failed run has no `Finished`); 5 cancelled through `Async.timeout`: same checks; 6 cancelled before the open (`Async.timeout 0`): `abandon_raw` runs, a following `Gui.run` works, holds 2/2 (the abandoned want raises and `abandon_raw` lowers; the second run raises and its reap lowers); 7 flood: the app posts then sleeps 10 ms while a click is scripted, the first `wait` returns the click; 8 a virtual-signal interrupt ends the run (measured: an interrupted run has no `Finished`, so the holds count is not observable; the test pins the transcript instead: one `run_loop`, and `stop` followed by `GuiPark`'s drop closing the still-open window, `destroy #1:0`, `presence off`; the virtual clock also advances while the task waits on the gate, so the interrupt may arrive before the open, and then the transcript is exactly `run_loop`, `stop`; the test accepts both); 9 `quit` with a Ui open → `CloseRequest` → the close message; 10 `quit` after `Gui.run` returned → the interrupt ends the program; 11 two concurrent `Gui.run`s on two pinned tasks: the second fails `Other "cannot open the GUI: it is already open"`; 12 without the GUI park → `NotAvailable`; tests that catch a failing `Gui.run` wrap it in a nested `Async.run`, because `Gui.run`'s failure is a failure of the caller's scope (`run_on_main` is `await`); 13 progress with `spawn_off_main` at 2 workers prints `total 10000`; 14 a failed backend creation (`Backend::Native` in a build without `gui-native`): each `Gui.run` fails `NotAvailable "cannot open the GUI: built without the native toolkit"`, holds 2/2 for two attempts, and a later deadlock is still reported. The sticky flag between poll and park (G-19) is pinned by the cells.rs unit test, since a VM-level poke in that window is not deterministic. `gui.rs` unit tests: `Slot` and `GuiPark` are `!Send` (the type-inference ambiguity trick of eml_gui's toolkit.rs), and the slot made with a park is not seen from a spawned thread (G-7). `crates/eml_vm/tests/gui_off.rs` (`#![cfg(not(feature = "gui"))]`): `Gui.run` fails `NotAvailable "cannot open the GUI: not available"`.
  - Spec: stdlib.md new 「`Std.Gui`」 (API, `run` pinning, `wait` order and failures, codes, id encoding and stale ids, `post` drop rule, error and prop tables, layout, `abandon_raw`); stdlib.md 「`Std.Async`」 `gui_wait` and Gate cells; vm.md new 「GUI の Park」 (Slot, two-phase open, `step`, reaper, holds, quit, `Finished.holds_*`, the loop's VM entries); vm.md 「セルと登録表」 Gate and the `Pending` text; object-model.md Foreign table row `Std.Gui.Ui` (around line 151); modules.md std list `Gui.em` (around line 69).
- Left to the implementer: the transcript snapshot text (check it against the appendix line formats, then `cargo insta review`).
- Spec note: stdlib.md and vm.md state that parking on a released gate is an internal error, which std never reaches because each Ui owns its gate, `abandon_raw` releases an unopened one, and the only `Cell` that outlives a run is the one inside `Sink`, used only by `poke_raw`, for which a stale poke does nothing.
- Condition: always.
- Completion criteria:
  - `cargo nextest run` → green, including `gui::`.
  - `cargo nextest run -p eml_io -p eml_vm -E 'test(gui_off::)'` → pass.
  - `cargo clippy -p eml_io -p eml_vm --all-targets -- -D warnings` (gui off) → green.
  - `nix develop -c nix/third-party-licenses.sh --check` → pass.
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --lib -- cells::` → pass (and once more with `-Zmiri-tree-borrows` added).
- Pitfalls: the reaper must also run on `Stopping` before `Stop`, or holds stay raised on the normal path; the gate belongs to its `UiState`, never to a slot-wide field, or a second `Gui.run` loses its gate; holds must be raised in `want_gui_raw`, before the main worker's sleep that follows the park on the gate (a raise in `open_ui_raw` lets worker 0 declare a false deadlock before the loop exists); lowering holds while an opening is pending lets worker 0 declare a false deadlock between the reap and the poke.

### T-7: CLI, UI tests and feature-combo CI
- Purpose: D-7, D-15.
- Targets: `crates/eml_cli/src/{main.rs, lib.rs}`, `crates/eml_cli/tests/{ui.rs, common/mod.rs, cli.rs}`, `tests/ui/run/gui/{counter,form,progress}.{em,gui}`, `tests/ui/run-fail/gui/wait_without_window.{em,gui}`, `crates/eml_cli/tests/repl/gui/not_available.repl`, `.github/workflows/ci.yml`, `CLAUDE.md`, docs/implementation/testing.md, docs/spec/stdlib.md 「起動」, architecture.md 「CLI と lib API」.
- Work: D-15. UI tests (programs = the three examples in the appendix): counter script `click 1:4`, `click 1:4`, `click 1:3`, `close 1:0` → `final 1`; form `text 1:3 Ann`, `toggle 1:4 true`, `click 1:5`, `close 1:0` → `submitted Ann`; progress `click 1:3` → `total 10000`; run-fail `wait_without_window` with an empty script → `gui: wait with no open window and no sink`. Harness: `Marks` (`crates/eml_cli/tests/common/mod.rs:109-216`) gains `gui_stub`, parsed with `reason_mark(line, leader, "gui", "stub")`; the `leading_marks` doc's count goes up by one; one accepted and one rejected parser test; `execute_in` (`ui.rs:176-237`) installs the stub park with the side file's script. cli.rs: `--gui bogus` and `EML_GUI=bogus` exit 2 with the message; `--gui stub` on counter without a script prints `final 0`. CI (appendix): the tls-off step becomes `tls_off:: | gui_off::`; add the `run,gui` clippy combo; the default-graph negative `cargo tree` check. CLAUDE.md: the feature-combo comment `(none, types, core, run, run+gui; run without tls; …)`, the tls/gui-off line, `--gui native|stub` among the `run` flags. testing.md: the mark and side file in 「UI テスト」, `gui_off` and `run,gui` in the feature-combination section, 「CI」, 「REPL のトランスクリプト」.
- Left to the implementer: none.
- Condition: always.
- Completion criteria:
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::)'` → pass, including `gui/`.
  - `timeout 20 cargo run -p eml_cli -- run --gui stub tests/ui/run/gui/counter.em` → prints `final 0`.
  - `cargo clippy -p eml_cli --no-default-features --features run,gui -- -D warnings` → green.
  - actionlint → no output; workspace nextest and clippy green.

### T-8: AppKit backend
- Purpose: D-2, D-16; first native backend.
- Targets: `crates/eml_gui/src/appkit/*`, `gui-native` features in eml_gui, eml_vm, eml_cli, `.github/workflows/ci.yml` (`gui-native` job, macOS), `nix/third-party-licenses.sh` (`--features gui-native`), `THIRD-PARTY-LICENSES.md`, testing.md 「GUIの手での確認」 and 「CI」, architecture.md, `CLAUDE.md` (`cargo build --features eml_cli/gui-native` line).
- Work: D-16 AppKit; flipped content view; `set_presence` (`Regular` + activate / `Prohibited`); `applicationShouldTerminate` → `QuitRequested` + `TerminateCancel`; measure with `fittingSize`. The `gui-native` job (appendix) with macOS only; the licenses script change and regeneration. The manual checklist into testing.md (section 9).
- Left to the implementer: the objc2 class declarations; whether the primary loop or the fallback pump is needed (decide by the manual check).
- Condition: always.
- Completion criteria:
  - macOS: `cargo clippy --all-targets --features eml_cli/gui-native -- -D warnings` → green; `cargo nextest run --features eml_cli/gui-native` → green.
  - `nix develop -c nix/third-party-licenses.sh --check` → pass.
  - Manual (the implementer on this Mac): checklist items 1–8 pass with `timeout 60 cargo run -p eml_cli --features gui-native -- run <example>.em`.

### T-9: Qt backend, flake and notices
- Purpose: D-2, D-8, D-16.
- Targets: `crates/eml_gui/src/qt/{mod.rs, shim.hpp, shim.cpp, event_loop.rs}`, `crates/eml_gui/build.rs`, `flake.nix`, `about.hbs`, `licenses/{GPL-3.0.txt, LGPL-3.0.txt}`, `.github/workflows/ci.yml` (Linux row and `nix build` step), `THIRD-PARTY-LICENSES.md`, architecture.md 「外部 crate」, testing.md 「CI」 (the `gui-native` job's OS list, the `nix build` step) and 「GUIの手での確認」 (Linux), and the cfg gates of the failed-creation and laziness tests (`crates/eml_vm/tests/gui.rs`, `crates/eml_gui/src/backend.rs`): gate them on the OSes whose native backend exists, so the other rows still run the failed-creation test; add a laziness test for Qt if it can run under nextest.
- Work: D-16 Qt; build.rs panics `eml_gui needs Qt 6 (found Qt {v})` on Qt 5; flake.nix per the appendix (`buildFeatures = [ "gui-native" ]`, Qt in `nativeBuildInputs` / `buildInputs` on Linux, devShell packages on Linux, `postInstall` copies the two license texts; the shared `shellHook` is not touched); `about.hbs` system-library section (Qt 6 LGPL-3.0, dynamic, source at qt.io, relinking right; the Windows App SDK runtime is Microsoft's, installed separately; AppKit is part of macOS); the verbatim FSF texts; `open_ui_raw` fails `Other "cannot open the GUI: no display (DISPLAY and WAYLAND_DISPLAY are unset)"` when neither is set.
- Left to the implementer: the cxx bridge details; `QMAKE` only if `nix develop -c which qmake` fails (then set it as a derivation / mkShell attribute on Linux, never in `shellHook`).
- Condition: always.
- Completion criteria:
  - CI Linux `gui-native` lane → green (clippy, nextest, `nix build`, `readelf -d result/bin/.eml-wrapped | grep -q libQt6Widgets.so.6`, no `libQt6*` / `*.dll` / `*.msix` in `result`).
  - CI `licenses` job → green.
  - On a Linux host without Qt: `cargo build -p eml_cli` → success.
  - Manual (the user, Linux desktop): checklist items 1–8.

### T-10: WinUI backend
- Purpose: D-2, D-8, D-16.
- Targets: `crates/eml_gui/src/winui/*`, `.github/workflows/ci.yml` (Windows row, G-2 positive check), `THIRD-PARTY-LICENSES.md`, architecture.md, testing.md 「CI」 and 「GUIの手での確認」 (Windows notes and checklist item 9, the runtime message), and the same cfg gates as T-9 (widened to Windows).
- Work: D-16 WinUI; bootstrap `V2`; `open_ui_raw` fails `NotAvailable "cannot open the GUI: the Windows App SDK 2 runtime is not installed (https://learn.microsoft.com/windows/apps/windows-app-sdk/downloads)"` when it fails; a `#[cfg(all(test, feature = "native", windows))]` test in `src/winui/mod.rs` calls `bootstrap()`: on `Ok` it prints `skipped: the Windows App SDK 2 runtime is installed` and returns, on `Err(NotAvailable(m))` asserts that `m` contains the URL, otherwise fails.
- Left to the implementer: the XAML composition; the alpha pin fallback (D-16).
- Condition: always.
- Completion criteria:
  - CI Windows `gui-native` lane → green; the bootstrap test passes or logs its skip.
  - CI G-2 step: in `cargo tree -p eml_cli --features gui-native -e normal,build --target all`, each of `cxx v`, `objc2 v`, `winio-winui3 v`, `qt-build-utils v` matches at least once (one `grep -q` per toolkit crate; a single count would pass on objc2 alone).
  - Manual (the user, Windows 11): checklist items 1–9.

### T-10b: eml's own WinUI 3 bindings and bootstrap
- Purpose: D-16 amended by the user after T-10: the WinUI backend uses WinUI 3 directly (no `winio-winui3`): Rust bindings generated by eml with `windows-bindgen` (the windows-rs generator, at the version matching the tree's `windows-core`) from the Windows App SDK 2.x `.winmd` metadata, and eml's own bootstrap. The WinUI 3 route itself stays (the user prefers it over system XAML / XAML Islands).
- Targets: `crates/eml_gui/src/winui/*` (a committed generated bindings file filtered to the types the backend uses; a bootstrap module), a regenerate script (`nix/winui-bindings.sh` or a dev-only tool outside every shipped crate's graph; pinned NuGet package versions and SHA-256; `--check` mode; not run in CI), root and eml_gui `Cargo.toml` / `Cargo.lock` (drop `winio-winui3` and, if unused, `winio-windows-subset`), `.github/workflows/ci.yml` (the G-2 marker for the Windows graph), `THIRD-PARTY-LICENSES.md`, architecture.md, distribution.md, testing.md, CLAUDE.md lines naming winio-winui3.
- Work: license check of the `.winmd` source package first (stop if generating and committing bindings is not allowed); bootstrap = existing Windows 11 check, then kernelbase `TryCreatePackageDependency` + `AddPackageDependency` for family `Microsoft.WindowsAppRuntime.2_8wekyb3d8bbwe` with an explicit 2.x minimum version taken from Microsoft's docs, process lifetime, no runtime DLL loaded by name, no fallback; the same `NotAvailable` message and bootstrap test shape as T-10. Behaviour and error texts otherwise unchanged.
- Left to the implementer: the filter list; how bindgen is run; the G-2 marker crate (must appear only in the Windows `gui-native` graph).
- Condition: always (user decision).
- Completion criteria: macOS nextest/clippy both ways, `.#cross` Windows clippy both ways, the regenerate script's `--check`, licenses `--check`, actionlint, no `winio-` crate in the `gui-native` tree; CI Windows `gui-native` lane green with the bootstrap test passing or logging its skip; manual (the user, Windows 11 with the 2.x runtime): checklist items 1–9 (this settles Q-2).

### T-11: Distribution document
- Purpose: D-8, D-9 (G-30).
- Targets: `docs/implementation/distribution.md` (new), `docs/README.md` 「読む順」 and 「文書の地図」, stdlib.md 「`Std.Gui`」 (citation).
- Work: write D-8 (end-user prerequisites per OS, link-time needs, the three Qt conveyance cases, Windows runtime and bootstrap, macOS signing and notarization, notices) and the AOT rules of D-9 that program authors need, including that GUI programs are built with `eml build --profile gui`.
- Left to the implementer: wording.
- Condition: always.
- Completion criteria: the docs citation test passes (`cargo nextest run` green); `nix build` → `result/share/doc/eml/` contains `GPL-3.0.txt`, `LGPL-3.0.txt`, `THIRD-PARTY-LICENSES.md`.

### T-12: Close the stage
- Purpose: roadmap 「進め方」 closing steps, as one commit `Close gui-toolkit`.
- Targets: docs/spec (polish), docs/implementation/{status.md, architecture.md, benchmarks.md}, docs/future/{ffi.md, native.md, roadmap.md}, docs/overview.md, docs/README.md, CLAUDE.md, the plan files.
- Work:
  - status.md: GUI implemented; limits (one native GUI per process outside `eml run` is untested; no Plugin seam).
  - benchmarks.md: fresh `bench/run.sh`, `bench/rt.sh`, `bench/scripts.sh` tables (the stage touched eml_vm and std).
  - ffi.md 「拡張」「GUI」, decisions #12 and #13: the cfg seam, the `Plugin` condition (a second plugin or `jit-externs`), Windows App SDK 2.x, Windows 11, two-phase open, the new `Std.Gui` API; remove the old sketch.
  - native.md 「AOT と配布」: D-9, including the `console` / `gui` profiles and the Q-12 default; native.md's existing sentence that a plugin's static library (`libeml_gui.a`) is linked only when the program imports the module is replaced by the profile rule.
  - roadmap.md: remove the stage item; add 「その後の項目」 entries with 後に回した理由 / 見直す条件 for every out-of-scope item in section 2, and update the REPL-GUI and Slint items' 見直す条件 if they name the stage.
  - overview.md decision table, README map, CLAUDE.md pointer row for `Std.Gui`.
  - Delete `docs/plans/2026-11-07-gui-toolkit*`, after checking that every decision in 5.1 is written in `docs/`.
- Completion criteria: section 9 overall verification passes; `git grep -n 'docs/plans' -- crates docs std` → no matches.

## 8. Execution order

```
T-1 → T-2 → T-3 ┐
     T-4 ───────┼→ T-6 → T-7 → T-8 → { T-9, T-10 } → T-11 → T-12
     T-5 ───────┘
```

T-3, T-4 and T-5 are independent of each other (T-3 needs T-2). T-9 and T-10 are independent of each other but are committed one after the other on one branch.

## 9. Verification plan

- Baseline (before T-2): `cargo nextest run` → all pass; `cargo clippy --all-targets -- -D warnings` → green.
- Overall verification (after T-12):
  - `cargo nextest run` → all pass.
  - `cargo clippy --all-targets -- -D warnings && cargo fmt --check` → green.
  - The feature-combo clippy list in CLAUDE.md, including `run,gui` → green.
  - `cargo nextest run -p eml_io -p eml_vm -E 'test(tls_off::) | test(gui_off::)'` → pass.
  - The Miri, ASan and TSan commands in CLAUDE.md → pass (on Linux via CI).
  - `nix develop -c nix/third-party-licenses.sh --check` → pass.
  - CI on the branch: every job green, including the three `gui-native` lanes.
- Manual checks by the user (success criterion "opens working windows on each OS"), with `eml` built with `gui-native` (Linux: `nix build`):
  1. counter opens at its natural size, enforces the minimum size, and + / − update the label.
  2. Resizing stretches the cells (as in the second fixture).
  3. form: Submit is disabled until the name is non-empty and the box is checked.
  4. progress: the bar advances and the window stays draggable during the work.
  5. The close box ends the program with `final N`.
  6. The OS quit gesture closes the windows through CloseRequest.
  7. After `Gui.run` returns, there is no Dock icon (macOS), and Ctrl-C ends the program.
  8. HiDPI: the same logical layout.
  9. Windows without the runtime: the `NotAvailable` message names the download page.

## 10. Risks and unresolved items

| ID | Item | Impact | How / when it gets resolved | Default if unresolved |
|---|---|---|---|---|
| Q-1 | Backend API facts (objc2 names, `qt-build-utils` API, WinUI alpha pins) are unverified | T-8–T-10 | the implementer, while building each backend | the fallbacks in D-16 |
| Q-2 | Whether eml's own bootstrap (T-10b: family and minimum version from Microsoft's docs) finds an installed 2.x runtime | T-10b | the user's Windows check | if it fails on a machine with 2.x installed, correct the family name / minimum version from Microsoft's docs in the bootstrap module |
| Q-3 | Windows App SDK 2.x license and installer terms not reviewed | distribution.md | anyone who reads the 2.x texts | eml ships nothing of Microsoft's; distribution.md says authors who bundle the installer act under Microsoft's terms |
| Q-4 | Qt close request mechanism | T-9 | the implementer | a C++ `QWidget` subclass in shim.cpp |
| Q-5 | AppKit text-change notification | T-8 | the implementer | `controlTextDidChange:` on the delegate |
| Q-6 | `open_windows_raw` count | T-2 | fixed | windows not closed by `close_window`; a CloseRequest does not close a window |
| Q-7 | Order of `measure` lines in transcripts | T-6 | fixed | whatever Taffy produces; accept the snapshot |
| Q-8 | Qt minimum version for the VM build | T-9 | fixed | whatever the host provides, at least 6.2; build.rs rejects only Qt 5 |
| Q-9 | QPA plugin discovery outside Nix | T-9 | user reports | rely on the distro's `qt6-qpa-plugins` |
| Q-10 | Repeated native `Gui.run` in a process outside `eml run` | – | a Machine-API host that needs it | untested; noted in status.md |
| Q-11 | LGPL conveyance by binary caches of the Nix closure | distribution.md | when the project publishes a cache | distribution.md states the cache operator conveys Qt and relies on nixpkgs' public source |
| Q-12 | What `Std.Stdout` / `Std.Stderr` / `Std.Stdin` do in a Windows `gui`-profile program, which has no console | the native-aot stage | that stage's design | writes are discarded and reads see end of input, rather than IO failures; recorded in native.md 「AOT と配布」 by T-12 as the default |
| Q-14 | A window's native minimum is set at first show and not updated when content grows later | T-3, backends | when a program shows a clipped window | keep; add a `Widgets::set_min_size` later if needed (roadmap item at close) |
| Q-13 | Whether native-aot builds Windows programs with a static CRT (`+crt-static`), removing the VC++ redistributable prerequisite | the native-aot stage | that stage's design | dynamic CRT; recorded in native.md 「AOT と配布」 by T-12 |

## Appendix: Main changes from review

- Tasks started by a pinned task are pinned too, so the first design's progress example ran its work on the GUI thread → added `start_off_main` / `spawn_off_main`.
- One `Backend` trait with `run_loop(&mut self)` aliased the extern arms that create widgets inside `step` → split into `Widgets` (owned by the toolkit) and `EventLoop` (owned by the Park) with a shared `EventQueue`.
- WinUI creates XAML objects only inside `Application::Start` → two-phase open through a Gate cell, the same for every backend.
- `Pending::io()` would have needed a Verus-gated `IoDriver` change → `RealDriver::poll_fd` in eml_io; `eml_rt` unchanged.
- Holds lowered per step, never on the normal stop path, and a second `Gui.run` losing its gate → holds raised only at open and lowered only by the reaper (also on `Stopping`); each Ui owns its gate; `abandon_raw`.
- Default-on native backends would have forced Qt on every Linux lane → `gui` / `gui-native` split.
- Windows App SDK 1.2 is out of support with unclear redistribution terms → target 2.x and Windows 11 (user decision).
- The `Plugin` trait of ffi.md #12 → replaced by the cfg seam (user decision).
- Several Rust staticlibs conflict → one staticlib per AOT program, two variants.
- LGPL texts and who conveys what, AOT third-party notices → D-8, D-9.
- G-8 (the reaper releases the gate) and G-9 (the loop's VM entries) could not both hold → ruled: the reaper releases the gate through a new `Pending::release_gate`, and G-9's list of entries was amended to name it (deferring the release to the VM side was rejected: it splits the teardown path or couples `MainPending::run` to the GUI slot). The other deviations of the concrete design from the guideline (holds counted in `Finished`, no `NativeId`, `Script(Vec<RawEvent>)`, per-Ui gates, `abandon_raw`, stale-gate park stays `Internal`, AppKit `NSApp.run()`) were ruled conforming.
