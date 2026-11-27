# Stage winui-layout: WinUI windows open at the size of their real content, crisp at any DPI

- Created: 2026-11-27 (executable plan).
- Base: main @ 3acd7d87. Branch: `plan/winui-layout`.
- Status: executable. The facts below were measured on Windows 11 Pro 10.0.26200, display at 150 % (144 DPI), Windows App SDK runtime 2.7.0.0, with a prototype (`2026-11-27-winui-layout/prototype.patch`, applies to 3acd7d87 together with the bindings regenerated from its `filter.txt`).
- Appendix: `docs/plans/2026-11-27-winui-layout/` (the prototype, and `bounds.ps1` / `measure.ps1`, the manual checks of section 8). Deleted at this stage's Close.
- Not on the roadmap's 「段の列」: it fixes two defects of the WinUI backend and adds no feature (docs/future/roadmap.md の「進め方」).

## 1. Goal and completion conditions

- Goal: on Windows, every window of `tests/ui/run/gui/` opens with every control inside its client area, at its real natural size, without the user resizing it, and renders at the monitor's DPI instead of being bitmap-stretched.
- Completion conditions:
  - C-1: `eml.exe` is per-monitor-v2 DPI aware once the WinUI backend is created: `GetAwarenessFromDpiAwarenessContext(GetWindowDpiAwarenessContext(hwnd))` is 2 and `GetDpiForWindow` equals the monitor's DPI (`measure.ps1`).
  - C-2: at 100 % and 150 %, `bounds.ps1` reports no control outside the client area for `counter`, `form` and `progress`, and form's Submit has a non-empty bounding rectangle.
  - C-3: a Button measures at its WinUI size (the prototype measured `-` 31.3×32 and `+` 36×32 logical px, Submit and Start likewise 32 high) in the size the window opens at, not the 0×18 measured before the XAML island is live.
  - C-4: the manual check of docs/implementation/testing.md の「GUIの手での確認」 passes on Windows 11 (items 1–8; 9 needs a machine without the runtime and is not part of this stage).
  - C-5: `cargo nextest run`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, the Windows cross clippy with and without `eml_cli/gui-native` (CLAUDE.md), `nix/crate-graph.sh --check` and `nix/winui-bindings.sh --check` pass.

## 2. Scope

- In: the WinUI backend (`crates/eml_gui/src/winui/`), the toolkit's handling of one new control event (`crates/eml_gui/src/{lib,queue,toolkit}.rs`), its routing in `crates/eml_rows/src/gui.rs`, the bindings filter and the regenerated bindings, docs.
- Out: updating the minimum size when content changes later (theme or text changes keep today's rule 「最小の大きさは後から変えない」 apart from the one remeasure of D-2); following a DPI change when a window moves to another monitor (see R-3); the AppKit and Qt backends (they measure correctly before the window shows and never push the new event).

## 3. Facts found (main @ 3acd7d87, measured)

- F-1 (DPI): the process is DPI-unaware (awareness 0). `GetDpiForWindow` returns 96 on a 144-DPI monitor, so the multiplication in `show` (`winui/mod.rs`, `fn show`) is a no-op and Windows stretches the window as a bitmap (client 120×68 logical became 180×102 physical). Nothing in the repository sets DPI awareness (no manifest, no API call).
- F-2 (measurement): `Toolkit::flush` measures a new window (`Layout::natural`) before `show_window`, i.e. before `Window::Activate`. Until the window's XAML island is live, a `Button` measures 0×18 (no template, no theme resources): counter's buttons were placed 18 px high with invisible labels, form's TextBox (placed at its real 32 px) overflowed into the CheckBox row and Submit was placed outside the window. After a resize, the same controls measure correctly; this is why resizing by hand repaired the layout.
- F-3 (what does not fix F-2): calling `Window::Activate` before measuring (it is asynchronous), `Control::ApplyTemplate` (returns true, sizes unchanged) and `UIElement::UpdateLayout` before `Measure` all leave the 0×18 result.
- F-4 (what fixes F-2): the root `Canvas`'s `FrameworkElement::Loaded` fires after the island's first layout pass; measuring again there gives the WinUI sizes. With a new control event that makes the toolkit compute `natural` again, relayout and call `show_window` again, all three examples pass C-2 (prototype, screenshots in review).
- F-5: `nix/winui-bindings.sh` runs on Windows under Git Bash (curl, sha256sum, cargo); no nix is needed. Adding `Loaded` (and, for F-3, `ApplyTemplate` / `UpdateLayout`) to `filter.txt` changed only `bindings.rs` by additions.

## 4. Decisions

- D-1 (DPI): the WinUI backend calls `SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)` in `winui::create`, after the main-thread and Windows 11 checks and before `bootstrap` creates anything, through `windows_core::link!` as `GetDpiForWindow` is. A failure (the awareness is already set, e.g. by a host process) is ignored: the call is about rendering quality, never about correctness. Not a manifest: a manifest would change every `eml` process, including those that never open a GUI, and needs a resource step in `eml_cli`'s build; the call keeps the effect inside the backend that needs it and keeps the rule that a program without GUI does not touch the toolkit (docs/implementation/architecture.md の「`eml_gui`の内部」).
- D-2 (remeasure): a new `RawEvent::Remeasure(Id)` (window id): "the backend's measurements of this window's content were not final when it was shown". `Toolkit::settle` handles it for a shown window by invalidating every measured size of the window, computing `natural` again, relayouting at that size and calling `show_window(w, size, min)` again; for an unknown or not yet shown window it does nothing (the next `flush` measures anyway). `take_control` returns it as a control event; `eml_rows`'s `settle` forwards it with `Resized` and `Invalidate`. It counts as one layout in `Toolkit::layouts`.
- D-3: the WinUI backend registers the root `Canvas`'s `Loaded` in `create_window` (kept with the other `EventRevoker`s of the window) and pushes `Remeasure(id)` from it. Only WinUI pushes it; the `Widgets::show_window` doc comment states that a backend which pushes `Remeasure` gets `show_window` more than once for the same window and must resize it in place. WinUI's `show` already resizes in place (`ResizeClient`, preferred minimum); `Activate` on an active window is harmless, but the implementation calls it only on the first show.
- D-4: the bindings filter gains only `FrameworkElement::Loaded`; the prototype's `ApplyTemplate` and `UpdateLayout` are not kept (F-3).
- D-5: the comment on `show` and the WinUI measuring paragraph cite architecture.md; no `docs/spec/` rule changes: `docs/spec/stdlib.md` の「`Std.Gui`」 already says a window opens at its natural size, which this stage makes true on Windows.

## 5. Units (in order)

1. WL-1 `Start winui-layout`: this plan and its appendix (this commit).
2. WL-2 toolkit (TDD): tests first in `toolkit.rs` and `queue.rs`, then `RawEvent::Remeasure`, `Toolkit::settle`, `take_control`, `eml_rows`'s forwarding, the `show_window` doc comment. Runs on every OS.
3. WL-3 WinUI: `filter.txt` + regenerated `bindings.rs` (D-4), `Loaded` → `Remeasure` (D-3), DPI awareness (D-1), first-show-only `Activate`. Checked by hand on Windows (section 8) and by the Windows cross clippy.
4. WL-4 docs (section 7).
5. WL-5 `Close winui-layout`: deletes this file and the appendix directory; the manual check results go in the commit message.

## 6. Test changes by kind (scope)

- No kind 1 or kind 2 change: every existing expected value stays byte-identical (the prototype passes `eml_gui`'s 49 unit tests unchanged).
- New tests (no agreement needed): `toolkit.rs` — after `flush`, `settle(&RawEvent::Remeasure(w))` with a `Counting`-style backend whose measured sizes grew asks every leaf again, places the children at the new natural size and records a second `show_window #1:0 …` with the new size and minimum; `Remeasure` of an unknown window and of a window not yet flushed records nothing and does not count a layout. `queue.rs` — `take_control` returns `Remeasure` in order with the other control events and leaves user events.
- The WinUI backend keeps no automated test of the layout: it opens only on the process's main thread and needs the runtime (docs/implementation/testing.md の「GUIの手での確認」).

## 7. docs written

- docs/implementation/architecture.md の「`eml_gui`の内部」: the layout paragraph (a window is measured again once on `Remeasure`, which is the one exception to 「最小の大きさは後から変えない」); the event paragraph (`take_control` also returns `Remeasure`); the WinUI measuring paragraph (measurements are final only after the root's `Loaded`, which pushes `Remeasure`; the backend makes the process per-monitor-v2 DPI aware when it is created, so `GetDpiForWindow` is the monitor's DPI).
- docs/implementation/testing.md の「GUIの手での確認」: item 1 also says every control is inside the window when it opens; the Windows paragraph says to run the check at 150 % as well as 100 %.
- docs/implementation/status.md: the line that says the WinUI backend has never run on a Windows with the runtime is replaced by what was run (Windows 11, runtime 2.7.0.0; bootstrap finds it).
- Japanese text is drafted in English and translated with PLaMo (CLAUDE.md); on a machine without it, the `natural-japanese` skill.

## 8. Acceptance checks

- Automated: C-5.
- On Windows 11 with the runtime, at 100 % and then 150 % display scale (Settings → Display → Scale; sign out if Windows asks):
  - `powershell -File docs/plans/2026-11-27-winui-layout/measure.ps1 -Example counter` in a fresh PowerShell process prints `awareness=2` and `windowDpi` equal to `monitorDpi` (C-1).
  - `powershell -File docs/plans/2026-11-27-winui-layout/bounds.ps1 -Example <counter|form|progress>` (one process per example: the scripts define the same types) prints no `outside client` line and a finite rectangle for every control (C-2); the screenshot it writes to `%TEMP%\eml-gui-<example>.png` shows every label.
  - The nine items of 「GUIの手での確認」, by hand (C-4); watch whether the window visibly opens small and then grows (R-1).
- The scripts run `target\debug\eml.exe` of the checkout they live in (build it with `cargo build -p eml_cli --features gui-native` first) and stop the program after measuring.

## 9. Risks with defaults

- R-1 (a visible jump): the window is shown at the size measured before `Loaded`, then resized. In the prototype no jump was noticed in screenshots taken after 1 s, but the first frames were not recorded. Default: accept it if the manual check does not see it; if it is visible, show the window off-screen or with `AppWindow::Move` outside the work area until the first `Remeasure` (needs `Move` in the filter) — decided in WL-3 with the user.
- R-2 (`Loaded` more than once): `Loaded` can fire again if the root leaves and re-enters the tree; the root never does. Default: remeasure on every `Loaded` (resizing to the natural size again is the defined behaviour of D-2); no guard.
- R-3 (moving between monitors of different DPI): with per-monitor v2, WinUI rescales the content and Windows proposes a new window size, but the minimum set through `OverlappedPresenter` is in physical px of the first monitor. Default: out of scope; written in roadmap.md 「その後の項目」 at the Close with 「後に回した理由」 and 「見直す条件」 (a second monitor with another scale is available for testing).
- R-4 (`SetProcessDpiAwarenessContext` refused): if another component set the awareness first, the call fails and the window stays bitmap-stretched; layout is still correct (D-1).
