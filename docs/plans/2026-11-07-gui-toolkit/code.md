# GUI toolkit: code appendix

The appendix of `docs/plans/2026-11-07-gui-toolkit.md`. It holds the code the implementer reproduces verbatim. Section numbers of the plan are cited as "D-n" and "T-n". Like the plan, it is deleted when the stage closes.

## 1. Public types of eml_gui (T-2)

```rust
pub struct Id(pub u64);                 // (window_no << 32) | seq; window_no starts at 1 per Toolkit, seq 0 is the window
pub enum Kind { Window, Grid, Label, Button, TextField, CheckBox, Choice, ProgressBar }
pub enum Prop { Text(String), Value(i64), Checked(bool), Enabled(bool) }
pub enum Init { Label(String), Button(String), TextField(String), CheckBox(String, bool), Choice, ProgressBar(i64) }
pub enum Event { Click(Id), Text(Id, String), Toggle(Id, bool), Select(Id, i64), CloseRequest(Id) }
pub enum RawEvent { User(Event), Resized(Id, Size), Invalidate(Id), QuitRequested }
pub enum Track { Auto, Fr(u16), Px(u32) }
pub enum Align { Start, Center, End, Stretch }
pub struct Cell { pub col_span: u16, pub row_span: u16, pub halign: Align, pub valign: Align }
pub enum GuiError { NoContainer, NothingOpen, UnknownId, WrongKind, OutOfRange, NotAvailable(String), Backend(String) }
pub enum Reason { Woken, Events, Io, Timeout }
pub enum Step { Continue(Option<Duration>), Stop }
pub enum Measure { Exact(f32), AtMost(f32), Unbounded }
pub struct Size { pub w: f32, pub h: f32 }
pub struct Rect { pub x: f32, pub y: f32, pub w: f32, pub h: f32 }   // logical px, parent-relative, top-left origin
pub struct Script(pub Vec<RawEvent>);   // Script::parse(&str) -> Result<Script, String>; Default
pub enum Backend { Native, Stub(Script) }
pub struct Native { pub widgets: Rc<RefCell<dyn Widgets>>, pub event_loop: Box<dyn EventLoop>, pub queue: EventQueue }

pub trait Widgets {                     // !Send
    fn create_window(&mut self, id: Id, title: &str) -> Result<(), GuiError>;
    fn create(&mut self, id: Id, kind: Kind, parent: Id, init: &Init) -> Result<(), GuiError>;
    fn add_item(&mut self, w: Id, item: &str) -> Result<(), GuiError>;      // never emits
    fn set(&mut self, w: Id, prop: &Prop) -> Result<(), GuiError>;          // never emits
    fn measure(&mut self, w: Id, width: Measure, height: Measure) -> Size;  // logical px
    fn place(&mut self, w: Id, rect: Rect);
    fn show_window(&mut self, w: Id, size: Size, min: Size);
    fn destroy(&mut self, w: Id);
    fn set_presence(&mut self, active: bool);  // AppKit: activation policy Regular / Prohibited; others: no-op
}

pub trait EventLoop {                   // !Send
    // calls step(Woken) once as soon as the loop is ready, then returns only when step returns Stop (or on an OS error)
    fn run_loop(&mut self, io: Option<PollSource>, timeout: Option<Duration>,
                step: &mut dyn FnMut(Reason) -> Step) -> io::Result<()>;
    fn waker(&self) -> Arc<dyn Fn() + Send + Sync>;
}
```

`EventQueue` is a cheap `Rc` handle shared by `Native` and `Toolkit`: `push(RawEvent)` (callbacks only), `take_control(&self) -> Vec<RawEvent>` (removes `QuitRequested`, `Resized`, `Invalidate`; the Park's `step` calls it; user events that arrive while no `Toolkit` exists are dropped), `pop_user(&self) -> Option<Event>` (only `Toolkit::poll`). `PollSource` is defined in `eml_vm`'s park.rs; `eml_gui` declares its own identical `IoSource { #[cfg(unix)] Fd(RawFd) }` and `eml_vm` converts, because `eml_gui` depends on no workspace crate.

## 2. Stub (T-2)

Transcript lines (exact; `eml_gui::stub::take_transcript() -> Vec<String>`, thread-local):

```
run_loop
presence on | presence off
create_window #1:0 "Counter"
create Button #1:3 in #1:1 "-"          (Init text, `checked=true`, `value=0`, nothing for Choice/Grid)
add_item #1:5 "red"
set #1:2 Text("1") | Value(30) | Checked(true) | Enabled(false)
measure #1:2
place #1:2 (0, 0) 54x24
show_window #1:0 78x80 min 78x80
destroy #1:0
stop
```

Script (`.gui` side file): one event per line; blank lines and `#` comments are skipped.

```
click W:S
text W:S <rest of line>
toggle W:S true|false
select W:S <int>
close W:S
quit
```

Loop: (1) `run_loop` logs `run_loop` and calls `step(Woken)`, even if the waker rang before. (2) After each `Continue`, if a scripted event remains, push it and call `step(Events)`; scripted events go before rings and timeouts. (3) A script that is empty from the start pushes, after the first `Continue` with shown windows, one `CloseRequest` per shown window, once. (4) Otherwise wait on a condvar for a ring (`step(Woken)`) or the timeout (`step(Timeout)`). (5) After 5 s with nothing: `Err(io::Error::other("stub: nothing woke the loop for 5 s"))`; a pending timeout longer than 5 s does not count, so a stub run with such a timer fails rather than finishing late. (5a) When `run_loop` is given an I/O source, the stub does not look at it (eml_gui has no OS polling); instead it never waits longer than 10 ms and calls `step(Io)` each time that wait ends without a ring, so pinned-task I/O on the real driver still completes. The 5 s failure does not apply while an I/O source is given. (6) `Stop` logs `stop` and returns.

## 3. Extern rows (T-4, T-6)

In `crates/eml_extern/src/lib.rs`. Run rustfmt over them.

```rust
        // after TlsConn (around line 279)
        GuiUi => type_row("Std.Gui.Ui", Lin, Repr::Obj),

        // after AsyncStartMainRaw (around line 1259)
        AsyncStartOffMainRaw => holding(
            &[true],
            kinded("Std.Async.start_off_main_raw", &[TObj], &[Poly], Obj, Lin, Purity::Effectful),
        ),

        // after TlsTakeLine (around line 2144)
        GuiWantRaw => row("Std.Gui.want_gui_raw", &[Unit], Int, Purity::Effectful),
        GuiOpenUiRaw => kinded("Std.Gui.open_ui_raw", &[Int], &[Unr], Obj, Lin, Purity::Effectful),
        GuiCloseUi => kinded("Std.Gui.close_ui", &[Obj], &[Lin], Unit, Unr, Purity::Effectful),
        GuiBeginWindowRaw => borrowing(
            &[true, true],
            FunctionRow {
                with_args: &[1],
                ..kinded("Std.Gui.begin_window_raw", &[Obj, Obj], &[Unr, Lin], Int, Unr, Purity::Effectful)
            },
        ),
        GuiBeginGridRaw => borrowing(
            &[true, true, true],
            FunctionRow {
                with_args: &[2],
                ..kinded("Std.Gui.begin_grid_raw", &[Obj, Obj, Obj], &[Unr, Unr, Lin], Int, Unr, Purity::Effectful)
            },
        ),
        GuiEndRaw => kinded("Std.Gui.end_raw", &[Obj], &[Lin], Obj, Lin, Purity::Effectful),
        GuiNextCellRaw => kinded(
            "Std.Gui.next_cell_raw",
            &[Int, Int, Int, Int, Obj],
            &[Unr, Unr, Unr, Unr, Lin],
            Obj,
            Lin,
            Purity::Effectful,
        ),
        GuiCreateRaw => borrowing(
            &[false, true, false, true],
            FunctionRow {
                with_args: &[3],
                ..kinded("Std.Gui.create_raw", &[Int, Obj, Int, Obj], &[Unr, Unr, Unr, Lin], Int, Unr, Purity::Effectful)
            },
        ),
        GuiAddItemRaw => borrowing(
            &[false, true, false],
            kinded("Std.Gui.add_item_raw", &[Int, Obj, Obj], &[Unr, Unr, Lin], Obj, Lin, Purity::Effectful),
        ),
        GuiSetTextRaw => borrowing(
            &[false, true, false],
            kinded("Std.Gui.set_text_raw", &[Int, Obj, Obj], &[Unr, Unr, Lin], Obj, Lin, Purity::Effectful),
        ),
        GuiSetIntRaw => kinded(
            "Std.Gui.set_int_raw",
            &[Int, Int, Int, Obj],
            &[Unr, Unr, Unr, Lin],
            Obj,
            Lin,
            Purity::Effectful,
        ),
        GuiCloseWindowRaw => kinded("Std.Gui.close_window_raw", &[Int, Obj], &[Unr, Lin], Obj, Lin, Purity::Effectful),
        GuiOpenWindowsRaw => borrowing(
            &[true],
            FunctionRow { with_args: &[0], ..kinded("Std.Gui.open_windows_raw", &[Obj], &[Lin], Int, Unr, Purity::Effectful) },
        ),
        GuiFlushRaw => kinded("Std.Gui.flush_raw", &[Obj], &[Lin], Obj, Lin, Purity::Effectful),
        GuiPollRaw => borrowing(
            &[true],
            FunctionRow { with_args: &[0], ..kinded("Std.Gui.poll_raw", &[Obj], &[Lin], Int, Unr, Purity::Effectful) },
        ),
        GuiEventIntRaw => borrowing(
            &[true],
            FunctionRow { with_args: &[0], ..kinded("Std.Gui.event_int_raw", &[Obj], &[Lin], Int, Unr, Purity::Effectful) },
        ),
        GuiEventTextRaw => borrowing(
            &[true],
            FunctionRow { with_args: &[0], ..kinded("Std.Gui.event_text_raw", &[Obj], &[Lin], Obj, Unr, Purity::Effectful) },
        ),
        GuiPokeRaw => row("Std.Gui.poke_raw", &[Int], Unit, Purity::Effectful),
        GuiAbandonRaw => row("Std.Gui.abandon_raw", &[Int], Unit, Purity::Effectful),
```

The comment on the `GuiUi` row: `// 実行の世代と回収の旗を持つ`Foreign`。Kindは`Lin` (docs/spec/stdlib.md の「`Std.Gui`」)`.

Every row is `Purity::Effectful`, so `faults = true`; `writes = false`; `held` is all false except `AsyncStartOffMainRaw`. `AsyncStartOffMainRaw`, `GuiWantRaw`, `GuiOpenUiRaw`, `GuiPokeRaw` and `GuiAbandonRaw` go into `is_async` (around lines 2162-2265) and the `Op::Async` list (`vm.rs:4674-4776`); the other 15 are `run_extern` arms. Rows that consume the `Ui` (`end_raw`, `next_cell_raw`, `add_item_raw`, `set_text_raw`, `set_int_raw`, `close_window_raw`, `flush_raw`, `close_ui`) call `ctx.heap_mut().decref(ui)` before raising (docs/spec/lir.md around line 274).

Test tables (`crates/eml_core_ir/tests/externs.rs`):

- `PROBES`: add `probe_gui_ui : Gui.Ui -> Gui.Ui` / `probe_gui_ui u = u`; the `probe` match gets `ExternType::GuiUi => "probe_gui_ui"`.
- `only_the_listed_rows_borrow_or_return_a_pair` (around line 320):
  - `"Std.Gui.begin_window_raw" => (&[true, true], &[1])`
  - `"Std.Gui.begin_grid_raw" => (&[true, true, true], &[2])`
  - `"Std.Gui.create_raw" => (&[false, true, false, true], &[3])`
  - `"Std.Gui.add_item_raw" | "Std.Gui.set_text_raw" => (&[false, true, false], &[])`
  - `open_windows_raw | poll_raw | event_int_raw | event_text_raw => (&[true], &[0])`
- `only_the_listed_rows_carry_a_lin_or_poly_mark` (around line 569):
  - add `start_off_main_raw` to the `start_raw | start_main_raw | run_raw => (&[Poly], Lin)` arm (T-4)
  - `open_ui_raw (&[Unr], Lin)`
  - `close_ui | open_windows_raw | poll_raw | event_int_raw | event_text_raw (&[Lin], Unr)`
  - `begin_window_raw (&[Unr, Lin], Unr)`
  - `begin_grid_raw (&[Unr, Unr, Lin], Unr)`
  - `create_raw (&[Unr, Unr, Unr, Lin], Unr)`
  - `end_raw | flush_raw (&[Lin], Lin)`
  - `next_cell_raw (&[Unr, Unr, Unr, Unr, Lin], Lin)`
  - `add_item_raw | set_text_raw (&[Unr, Unr, Lin], Lin)`
  - `set_int_raw (&[Unr, Unr, Unr, Lin], Lin)`
  - `close_window_raw (&[Unr, Lin], Lin)`
- `only_the_listed_rows_hold_an_argument` (around line 701): insert `"Std.Async.start_off_main_raw"` after `start_main_raw`; its doc comment's 「19行」 becomes 「20行」 (T-4).

`crates/eml_hir/tests/externs.rs` needs no edit. `eml_hir::STD` (`crates/eml_hir/src/lib.rs:26-68`) gets `("Gui.em", include_str!("../../../std/Gui.em"))` after `Http.em` (T-6).

## 4. `std/Async.em` additions

After `run_on_main` (around lines 185-186), in T-4:

```haskell
-- `start`と同じだが、始めたタスクが固定されていても、新しいタスクはworkerの輪に置く (docs/spec/stdlib.md の「メインのworker」)
pub start_off_main : (Unit -> <IO, Panic, Async> a) -> <IO, Async> Task a
start_off_main f = start_off_main_raw (bottom f)

pub spawn_off_main : (Unit -> <IO, Panic, Async> Unit) -> <IO, Async> Unit
spawn_off_main f = detach (start_off_main_raw (bottom f))
```

After them, in T-6:

```haskell
-- ゲートのセルで待つ。前の待ちの後にゲートへ知らせが届いていれば、すぐに戻る (docs/spec/vm.md の「セルと登録表」)
pub gui_wait : Cell -> <IO, Async> Wake
gui_wait c = await_cell c
```

At the end of the extern list, after `start_main_raw` (around line 393), in T-4:

```haskell
extern start_off_main_raw : (Unit -> <IO, Async> Unit) -> <IO> Task a
```

## 5. `std/Gui.em` (T-6)

The whole module, verbatim. It type-checks against the rows above (with the arms stubbed), and the seven probe programs, including the three examples below, compile through the LIR verifier.

```haskell
-- ネイティブのGUIのtoolkit (docs/spec/stdlib.md の「`Std.Gui`」)。`run`は、固定したタスクの上で、toolkitのハンドル`Ui`を
-- handlerの状態として持つ。部品はUnrのidで、メッセージは作るときに部品に結び付け、`wait`で引き出す
import Async (Async)
import Async
import Chan
import Map
import List
import String

pub data Track =
  | Auto
  | Fr Int
  | Px Int
  deriving (Eq, Show)

pub data Align =
  | Start
  | Center
  | End
  | Stretch
  deriving (Eq, Show)

pub data Cell =
  | Cell { col_span : Int, row_span : Int, halign : Align, valign : Align }
  deriving (Eq, Show)

pub default_cell : Cell
default_cell = Cell { col_span = 1, row_span = 1, halign = Stretch, valign = Stretch }

pub abstract data Window = | Window Int
pub abstract data Grid = | Grid Int
pub abstract data Label = | Label Int
pub abstract data Button = | Button Int
pub abstract data TextField = | TextField Int
pub abstract data CheckBox = | CheckBox Int
pub abstract data Choice = | Choice Int
pub abstract data ProgressBar = | ProgressBar Int
-- 1つの`run`へ送る端。そのchannelとゲートのセルを持つ
pub abstract data Sink msg = | Sink (Chan.Chan msg) Async.Cell

pub class Widget w where
  wid : w -> Int

instance Widget Window where
  wid (Window h) = h
instance Widget Grid where
  wid (Grid h) = h
instance Widget Label where
  wid (Label h) = h
instance Widget Button where
  wid (Button h) = h
instance Widget TextField where
  wid (TextField h) = h
instance Widget CheckBox where
  wid (CheckBox h) = h
instance Widget Choice where
  wid (Choice h) = h
instance Widget ProgressBar where
  wid (ProgressBar h) = h

pub effect Gui msg where
  begin_window : String -> msg -> Window
  begin_grid   : List Track -> List Track -> Grid
  end          : Unit -> Unit
  next_cell    : Cell -> Unit
  label        : String -> Label
  button       : String -> msg -> Button
  text_field   : String -> (String -> msg) -> TextField
  check_box    : String -> Bool -> (Bool -> msg) -> CheckBox
  choice       : List String -> Int -> (Int -> msg) -> Choice
  progress_bar : Int -> ProgressBar
  set_text     : Int -> String -> Unit
  set_value    : Int -> Int -> Unit
  set_checked  : Int -> Bool -> Unit
  set_enabled  : Int -> Bool -> Unit
  close_window : Window -> Unit
  sink         : Unit -> Sink msg
  wait         : Unit -> msg

pub window : String -> msg -> (Window -> <Gui msg | e> a) -> <Gui msg | e> a
window title on_close body =
  let w = begin_window title on_close
  let r = body w
  end ()
  r

pub grid : List Track -> List Track -> (Unit -> <Gui msg | e> a) -> <Gui msg | e> a
grid cols rows body =
  let _ = begin_grid cols rows
  let r = body ()
  end ()
  r

pub cell : Cell -> (Unit -> <Gui msg | e> a) -> <Gui msg | e> a
cell c body =
  next_cell c
  body ()

pub text : Widget w => w -> String -> <Gui msg> Unit
text w s = set_text (wid w) s

pub enable : Widget w => w -> Bool -> <Gui msg> Unit
enable w b = set_enabled (wid w) b

pub value : ProgressBar -> Int -> <Gui msg> Unit
value p n = set_value (wid p) n

pub select : Choice -> Int -> <Gui msg> Unit
select c n = set_value (wid c) n

pub checked : CheckBox -> Bool -> <Gui msg> Unit
checked c b = set_checked (wid c) b

-- どのworkerのどのタスクからも呼べる。`run`が終わった後に送ったメッセージは捨てる
pub post : Sink msg -> msg -> <IO, Async> Unit
post (Sink c g) m =
  match Chan.send m c with
    | Chan.Sent -> poke_raw g
    | Chan.Rejected _ -> ()

pub run : (Unit -> <Gui msg, IO, Panic, Async> a) -> <IO, Panic, Async> a
run app = Async.run_on_main (fn () -> run_here app)

data Handler msg =
  | OnClick msg
  | OnText (String -> msg)
  | OnToggle (Bool -> msg)
  | OnSelect (Int -> msg)
  | OnClose msg

data State msg : Lin =
  | State { ui : Ui, chan : Chan.Chan msg, gate : Async.Cell, has_sink : Bool, handlers : Map.Map Int (Handler msg) }

-- `create_raw`の種類の番号と`set_int_raw`の性質の番号 (docs/spec/stdlib.md の「`Std.Gui`」)
label_kind : Int
label_kind = 1
button_kind : Int
button_kind = 2
text_field_kind : Int
text_field_kind = 3
check_box_kind : Int
check_box_kind = 4
choice_kind : Int
choice_kind = 5
progress_bar_kind : Int
progress_bar_kind = 6
value_prop : Int
value_prop = 1
checked_prop : Int
checked_prop = 2
enabled_prop : Int
enabled_prop = 3

run_here : (Unit -> <Gui msg, IO, Panic, Async> a) -> <IO, Panic, Async> a
run_here app =
  handle app () from open_state () with
    | ctl begin_window title m k st ->
        let (h, st) = step_int (fn ui -> begin_window_raw title ui) st
        k (Window h) (listen h (OnClose m) st)
    | ctl begin_grid cols rows k st ->
        let (h, st) = step_int (fn ui -> begin_grid_raw (tracks cols) (tracks rows) ui) st
        k (Grid h) st
    | ctl end () k st -> k () (step_ui (fn ui -> end_raw ui) st)
    | ctl next_cell c k st ->
        let Cell { col_span, row_span, halign, valign } = c
        k () (step_ui (fn ui -> next_cell_raw col_span row_span (align halign) (align valign) ui) st)
    | ctl label s k st ->
        let (h, st) = create label_kind s 0 st
        k (Label h) st
    | ctl button s m k st ->
        let (h, st) = create button_kind s 0 st
        k (Button h) (listen h (OnClick m) st)
    | ctl text_field s f k st ->
        let (h, st) = create text_field_kind s 0 st
        k (TextField h) (listen h (OnText f) st)
    | ctl check_box s b f k st ->
        let (h, st) = create check_box_kind s (flag b) st
        k (CheckBox h) (listen h (OnToggle f) st)
    | ctl choice items i f k st ->
        let (h, st) = create choice_kind "" 0 st
        let st = add_items h items st
        let st = match items with
          | Nil -> st
          | _ -> step_ui (fn ui -> set_int_raw h value_prop i ui) st
        k (Choice h) (listen h (OnSelect f) st)
    | ctl progress_bar v k st ->
        let (h, st) = create progress_bar_kind "" v st
        k (ProgressBar h) st
    | ctl set_text h s k st -> k () (step_ui (fn ui -> set_text_raw h s ui) st)
    | ctl set_value h v k st -> k () (step_ui (fn ui -> set_int_raw h value_prop v ui) st)
    | ctl set_checked h b k st -> k () (step_ui (fn ui -> set_int_raw h checked_prop (flag b) ui) st)
    | ctl set_enabled h b k st -> k () (step_ui (fn ui -> set_int_raw h enabled_prop (flag b) ui) st)
    | ctl close_window w k st ->
        let h = wid w
        k () (forget_window h (step_ui (fn ui -> close_window_raw h ui) st))
    | ctl sink () k st ->
        let State { ui, chan, gate, has_sink = _, handlers } = st
        k (Sink chan gate) (State { ui, chan, gate, has_sink = True, handlers })
    | ctl wait () k st ->
        let (m, st) = next_message st
        k m st
    | return x st ->
        close_state st
        x

-- 2段階で開く。メインのworkerの`Park`がネイティブのループを動かすまで、タスクはゲートで待つ。そのため、toolkitの
-- 呼び出しはすべてそのループの中で動く (docs/spec/vm.md の「GUI の Park」)
open_state : Unit -> <IO, Async> State msg
open_state () =
  let gate = want_gui_raw ()
  match Async.gui_wait gate with
    | Async.Woke ->
      let ui = open_ui_raw gate
      State { ui, chan = Chan.new 64, gate, has_sink = False, handlers = Map.empty }
    | Async.Cancelled ->
      abandon_raw gate
      fail_io Cancelled "task was cancelled"

close_state : State msg -> <IO> Unit
close_state st =
  let State { ui, chan, gate = _, has_sink = _, handlers = _ } = st
  Chan.close chan
  close_ui ui

-- ネイティブのイベントを先に、次に送られたメッセージを見て、ウィンドウがないことを確かめ、最後にゲートで待つ。引き出しと
-- 待ちの間に届いた知らせはゲートの旗を立てるので、待ちはすぐに戻る
next_message : State msg -> <IO, Panic, Async> (msg, State msg)
next_message st =
  if Async.cancel_requested () then
    close_state st
    fail_io Cancelled "task was cancelled"
  else
    let State { ui, chan, gate, has_sink, handlers } = st
    let ui = flush_raw ui
    let (id, ui) = poll_raw ui
    if id != 0 then
      let (m, ui) = message (Map.lookup id handlers) ui
      (m, State { ui, chan, gate, has_sink, handlers })
    else
      match Chan.try_recv chan with
        | Chan.Got m -> (m, State { ui, chan, gate, has_sink, handlers })
        | _ ->
          let (n, ui) = open_windows_raw ui
          if n == 0 && not has_sink then
            close_state (State { ui, chan, gate, has_sink, handlers })
            fail_io Other "gui: wait with no open window and no sink"
          else
            match Async.gui_wait gate with
              | Async.Woke -> next_message (State { ui, chan, gate, has_sink, handlers })
              | Async.Cancelled ->
                close_state (State { ui, chan, gate, has_sink, handlers })
                fail_io Cancelled "task was cancelled"

message : Option (Handler msg) -> Ui -> <IO, Panic> (msg, Ui)
message h ui =
  match h with
    | Some (OnClick m) -> (m, ui)
    | Some (OnClose m) -> (m, ui)
    | Some (OnText f) ->
      let (s, ui) = event_text_raw ui
      (f s, ui)
    | Some (OnToggle f) ->
      let (n, ui) = event_int_raw ui
      (f (n != 0), ui)
    | Some (OnSelect f) ->
      let (n, ui) = event_int_raw ui
      (f n, ui)
    | None ->
      close_ui ui
      panic "gui: an event names a widget without a handler"

step_int : (Ui -> <IO> (Int, Ui)) -> State msg -> <IO> (Int, State msg)
step_int f st =
  let State { ui, chan, gate, has_sink, handlers } = st
  let (n, ui) = f ui
  (n, State { ui, chan, gate, has_sink, handlers })

step_ui : (Ui -> <IO> Ui) -> State msg -> <IO> State msg
step_ui f st =
  let State { ui, chan, gate, has_sink, handlers } = st
  State { ui = f ui, chan, gate, has_sink, handlers }

create : Int -> String -> Int -> State msg -> <IO> (Int, State msg)
create kind s n st = step_int (fn ui -> create_raw kind s n ui) st

add_items : Int -> List String -> State msg -> <IO> State msg
add_items h items st =
  match items with
    | Nil -> st
    | item :: rest -> add_items h rest (step_ui (fn ui -> add_item_raw h item ui) st)

listen : Int -> Handler msg -> State msg -> State msg
listen h f st =
  let State { ui, chan, gate, has_sink, handlers } = st
  State { ui, chan, gate, has_sink, handlers = Map.insert h f handlers }

-- 閉じたウィンドウの部品は、idの上位32ビットがそのウィンドウと同じである (docs/spec/stdlib.md の「`Std.Gui`」)
forget_window : Int -> State msg -> State msg
forget_window h st =
  let State { ui, chan, gate, has_sink, handlers } = st
  let w = h >>> 32
  let kept = Map.fold (fn acc id f -> if id >>> 32 == w then acc else Map.insert id f acc) Map.empty handlers
  State { ui, chan, gate, has_sink, handlers = kept }

tracks : List Track -> String
tracks ts = String.join "," (List.map track ts)

track : Track -> String
track t =
  match t with
    | Auto -> "a"
    | Fr n -> "f\{show n}"
    | Px n -> "p\{show n}"

align : Align -> Int
align a =
  match a with
    | Start -> 0
    | Center -> 1
    | End -> 2
    | Stretch -> 3

flag : Bool -> Int
flag b = if b then 1 else 0

-- toolkitのハンドル。runの世代と回収の旗を持つ`Foreign`で、手放すと`Park`の次の段でウィンドウを閉じる。Kindは`Lin`
pub extern data Ui

extern want_gui_raw     : Unit -> <IO> Async.Cell
extern open_ui_raw      : Async.Cell -> <IO> Ui
extern close_ui         : Ui -> <IO> Unit
extern begin_window_raw : String -> Ui -> <IO> (Int, Ui)
extern begin_grid_raw   : String -> String -> Ui -> <IO> (Int, Ui)
extern end_raw          : Ui -> <IO> Ui
extern next_cell_raw    : Int -> Int -> Int -> Int -> Ui -> <IO> Ui
extern create_raw       : Int -> String -> Int -> Ui -> <IO> (Int, Ui)
extern add_item_raw     : Int -> String -> Ui -> <IO> Ui
extern set_text_raw     : Int -> String -> Ui -> <IO> Ui
extern set_int_raw      : Int -> Int -> Int -> Ui -> <IO> Ui
extern close_window_raw : Int -> Ui -> <IO> Ui
extern open_windows_raw : Ui -> <IO> (Int, Ui)
extern flush_raw        : Ui -> <IO> Ui
extern poll_raw         : Ui -> <IO> (Int, Ui)
extern event_int_raw    : Ui -> <IO> (Int, Ui)
extern event_text_raw   : Ui -> <IO> (String, Ui)
extern poke_raw         : Async.Cell -> <IO> Unit
extern abandon_raw      : Async.Cell -> <IO> Unit
```

## 6. Example programs (T-7: `tests/ui/run/gui/`)

`counter.em`:

```haskell
import Async (Async)
import Async
import Gui (Gui(..), Track(..), Align(..), Cell(..))
import Gui

data Msg =
  | Inc
  | Dec
  | Quit

counter : Unit -> <Gui Msg, IO, Panic, Async> Unit
counter () =
  use _ <- Gui.window "Counter" Quit
  use Gui.grid [Fr 1, Fr 1] []
  let shown = Gui.cell { Gui.default_cell | col_span = 2 } (fn () -> Gui.label "0")
  let _ = Gui.button "-" Dec
  let _ = Gui.button "+" Inc
  let loop n =
    match Gui.wait () with
      | Inc ->
        Gui.text shown (show (n + 1))
        loop (n + 1)
      | Dec ->
        Gui.text shown (show (n - 1))
        loop (n - 1)
      | Quit -> println "final \{show n}"
  loop 0

main : Unit -> <IO, Panic> Unit
main () =
  use Async.run
  Gui.run counter
```

`form.em`:

```haskell
import Async (Async)
import Async
import Gui (Gui(..), Track(..), Align(..), Cell(..))
import Gui

data Msg =
  | Name String
  | Agree Bool
  | Submit
  | Quit

data Form =
  | Form { name : String, agree : Bool }

form : Unit -> <Gui Msg, IO, Panic, Async> Unit
form () =
  use _ <- Gui.window "Sign up" Quit
  use Gui.grid [Auto, Fr 1] []
  let _ = Gui.label "Name"
  let _ = Gui.text_field "" Name
  let _ = Gui.cell { Gui.default_cell | col_span = 2 } (fn () -> Gui.check_box "I agree" False Agree)
  let submit = Gui.cell { Gui.default_cell | col_span = 2, halign = End } (fn () -> Gui.button "Submit" Submit)
  Gui.enable submit False
  let loop (f : Form) =
    match Gui.wait () with
      | Name s ->
        let f = { f | name = s }
        Gui.enable submit (valid f)
        loop f
      | Agree b ->
        let f = { f | agree = b }
        Gui.enable submit (valid f)
        loop f
      | Submit ->
        println "submitted \{f.name}"
        loop f
      | Quit -> ()
  loop (Form { name = "", agree = False })

valid : Form -> Bool
valid f = f.agree && f.name != ""

main : Unit -> <IO, Panic> Unit
main () =
  use Async.run
  Gui.run form
```

`progress.em`:

```haskell
import Async (Async)
import Async
import Gui (Gui(..), Track(..), Align(..), Cell(..))
import Gui

data Msg =
  | Start
  | Step Int
  | Done Int
  | Quit

chunk : Int -> <Panic> Int
chunk i = if i == 0 then 0 else 1 + chunk (i - 1)

work : Gui.Sink Msg -> <IO, Panic, Async> Unit
work sink =
  let go i acc =
    if i == 10 then Gui.post sink (Done acc)
    else
      let r = Async.compute (fn () -> chunk 1000)
      Gui.post sink (Step (i + 1))
      go (i + 1) (acc + r)
  go 0 0

app : Unit -> <Gui Msg, IO, Panic, Async> Unit
app () =
  use _ <- Gui.window "Progress" Quit
  use Gui.grid [Fr 1] []
  let bar = Gui.progress_bar 0
  let start = Gui.button "Start" Start
  let sink = Gui.sink ()
  let loop () =
    match Gui.wait () with
      | Start ->
        Gui.enable start False
        Async.spawn_off_main (fn () -> work sink)
        loop ()
      | Step i ->
        Gui.value bar (i * 10)
        loop ()
      | Done total ->
        Gui.enable start True
        println "total \{show total}"
      | Quit -> ()
  loop ()

main : Unit -> <IO, Panic> Unit
main () =
  use Async.run
  Gui.run app
```

`tests/ui/run-fail/gui/wait_without_window.em`:

```haskell
import Async (Async)
import Async
import Gui (Gui(..))
import Gui

data Msg =
  | Quit

app : Unit -> <Gui Msg, IO, Panic, Async> Unit
app () =
  let w = Gui.window "Once" Quit (fn w -> w)
  Gui.close_window w
  match Gui.wait () with
    | Quit -> println "unreachable"

main : Unit -> <IO, Panic> Unit
main () =
  use Async.run
  Gui.run app
```

Each file starts with the mark line `-- gui: stub (scripted events in <name>.gui)` (the run-fail one: `-- gui: stub (no window is open when it waits)`), and the scripts are in T-7.

## 7. eml_vm code

`main_timeout` (`crates/eml_vm/src/workers.rs:1320-1325`), whole body (T-5):

```rust
let k = self.index;
let driver = self.driver.as_deref().filter(|_| self.run.io_worker(k) == k)?;
let until = self.own_deadline.map(|d| Duration::from_millis(d).saturating_sub(driver.now()));
// OSの源を渡せない`Park`は、driverに出した操作の完了を見に、間をおいて`run`を呼ぶ (docs/spec/vm.md の「メインのworkerの待ち」)
let cap = (self.poll.is_none() && driver.pending() > 0).then_some(MAIN_POLL_CAP);
match (until, cap) { (Some(a), Some(b)) => Some(a.min(b)), (a, b) => a.or(b) }
```

with `const MAIN_POLL_CAP: Duration = Duration::from_millis(10);`.

`Pending` additions (T-5, T-6):

```rust
/// メインのworkerのdriverが待つOSの源。仮想の時計、Windows、driverを作る前は`None`である。
fn io(&self) -> Option<PollSource>;
/// `poke_raw`と同じ。ゲートの旗を立てるか、待つタスクを起こす。
fn poke(&mut self, gate: i64);
/// GUIを閉じたrunのゲートを登録表から外す。待つタスクがあれば内部の誤り。
fn release_gate(&mut self, gate: i64);
```

`gui.rs` (T-6):

```rust
pub struct Slot {
    backend: Cell<Option<eml_gui::Backend>>,      // taken at the first create
    native: RefCell<Option<eml_gui::Native>>,     // widgets + queue; event_loop moves into GuiPark
    create_error: RefCell<Option<GuiError>>,      // sticky for the run (Backend is consumed)
    opening: Cell<Option<i64>>,                   // gate of a want_gui_raw not yet opened
    poked: Cell<bool>,                            // step has poked `opening`
    toolkit: RefCell<Option<eml_gui::Toolkit>>,
    ui: RefCell<Option<UiState>>,                 // UiState { gen: u32, gate: i64, reap: Arc<AtomicBool> }
    event: RefCell<Option<CurrentEvent>>,         // set by poll_raw for event_int_raw / event_text_raw
    gens: Cell<u32>,
    waker: Arc<GuiWaker>,                         // { rang: AtomicBool, inner: Mutex<Option<Arc<dyn Fn() + Send + Sync>>> }
}
thread_local! { static GUI_SLOT: RefCell<Option<Rc<Slot>>> = const { RefCell::new(None) }; }
pub struct GuiPark { slot: Rc<Slot>, event_loop: Option<Box<dyn EventLoop>>, interrupt: Arc<dyn Fn() + Send + Sync> }
pub fn park(backend: eml_gui::Backend, interrupt: Arc<dyn Fn() + Send + Sync>) -> ParkFactory;
```

The factory creates the `Rc<Slot>`, installs it in `GUI_SLOT` and returns a `GuiPark`. `GuiPark::drop` runs `close_all` and `set_presence(false)` on a remaining Ui (only when a run ends with a live Ui, e.g. `exit` inside the app) and clears `GUI_SLOT`; a remaining gate is freed by `sweep_all`.

`GuiPark::wait(timeout, pending)`:

1. No loop and `opening` is `None`: `pending.default_wait()` and return (exactly `DefaultPark`).
2. No loop and `opening` is `Some`: try `Backend::create` once per run. On failure (now or earlier): store `create_error`, `pending.poke(opening)`, set `poked`, call `pending.run()` once and return; `open_ui_raw` reports the error. On success: install the waker, then `run_loop(pending.io(), timeout, step)` — the only `run_loop` of the run, which returns only on `Stop`.
3. `waker()` returns the `GuiWaker`.

`step(reason)`:

```
loop {
    reap(pending);
    for e in queue.take_control():
        QuitRequested -> ui live ? toolkit.request_close_all() : (self.interrupt)()
        Resized / Invalidate -> toolkit.settle()
    if let Some(g) = opening && !poked { pending.poke(g); poked = true }
    if ui live && (reason == Events || toolkit.has_events()) { pending.poke(ui.gate) }
    match pending.run() {
        Stopping => { reap(pending); return Stop }
        Wait(t) => last = t,
        Busy => {}
    }
    if !(ui reap-flagged || (opening.is_some() && !poked)) { return Continue(last) }
}
```

`reap(pending)` runs only when a Ui exists and its reap flag is set: `toolkit.close_all()`, `set_presence(false)`, `slot.toolkit = None`, `let s = slot.ui.take()`, `pending.release_gate(s.gate)`, `pending.set_holds(false)`.

Second `Gui.run` in sequence: the first run's `close_ui` sets the reap flag; in the same `pending.run()` the second `want_gui_raw` succeeds (the old Ui is flagged), sets `opening = g2` and parks on `g2`; `run` returns `Wait`; the loop repeats because the old Ui is flagged; that pass reaps it (releasing `g1`, lowering holds), pokes `g2` and runs again; `open_ui_raw(g2)` raises holds. Totals: raised 2, lowered 2.

## 8. CLI (T-7)

`Command::Run` gets `#[arg(long, value_name = "native|stub")] gui: Option<String>`; under `cfg(feature = "gui")` the config gets `config.with_main_park(eml_cli::gui_park(backend, interrupt))` with `interrupt = { control.interrupt(); doorbell.ring() }` (`control.rs:507`); the config is built around `main.rs:110-117`. `eml_cli` re-exports `gui_park`, `Backend` and `Script` under `gui`.

## 9. flake.nix (T-9)

In `packages.eml` after `cargoBuildFlags` (around line 104):

```nix
              buildFeatures = [ "gui-native" ];
              # Qt 6 はシステムのライブラリとして動的にリンクする (docs/implementation/distribution.md)
              nativeBuildInputs = pkgs.lib.optionals pkgs.stdenv.isLinux [
                pkgs.pkg-config
                pkgs.qt6.qtbase
                pkgs.qt6.wrapQtAppsHook
              ];
              buildInputs = pkgs.lib.optionals pkgs.stdenv.isLinux [ pkgs.qt6.qtbase ];
```

`postInstall` adds `cp ${./licenses/GPL-3.0.txt} ${./licenses/LGPL-3.0.txt} $out/share/doc/eml/`. The default devShell's package list gets `pkgs.lib.optionals pkgs.stdenv.isLinux [ pkgs.qt6.qtbase pkgs.pkg-config ]`. The shared `shellHook` is not touched.

## 10. CI (`.github/workflows/ci.yml`)

T-2: a new step in the `test` job after 「dependency edges of eml_native」:

```yaml
      # eml_gui はワークスペースのどの crate にも依存しない (docs/implementation/architecture.md の「crate の依存関係」)
      - name: dependency edges of eml_gui
        if: ${{ !cancelled() && steps.test.outcome != 'skipped' }}
        run: |
          test "$(cargo tree -p eml_gui -e normal --target all --prefix none | grep -v '^eml_gui ' | grep -c '^eml_')" -eq 0
```

T-7:
- 「cargo nextest run without tls」 becomes `name: cargo nextest run without tls and gui` with `run: cargo nextest run -p eml_io -p eml_vm --failure-output immediate-final -E 'test(tls_off::) | test(gui_off::)'`.
- The non-default-features block adds `cargo clippy -p eml_cli --no-default-features --features run,gui -- -D warnings`.
- The 「dependency edges of eml_gui」 step adds `test "$(cargo tree -p eml_cli -e normal,build --target all | grep -c -E 'cxx v|objc2 v|winio-|qt-build-utils v')" -eq 0`.

T-8 adds the job below with `os: [macos-15]`; T-9 adds `ubuntu-24.04` and the `nix build` step; T-10 adds `windows-2025`. Check the action pins against the `test` job when implementing (they are copied from ci.yml lines 36-56 and 113-122).

```yaml
  # native の GUI の backend を組み、stub のテストを流す。画面を開くテストはない (docs/implementation/testing.md の「CI」)
  gui-native:
    name: gui-native (${{ matrix.os }})
    strategy:
      fail-fast: false
      matrix:
        os: [macos-15]
    runs-on: ${{ matrix.os }}
    timeout-minutes: 60
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false
      - name: Install the pinned toolchain
        if: runner.os != 'Linux'
        run: rustup toolchain install
      - uses: cachix/install-nix-action@13d8dd58da0234aa297dedd986986ccb8e7f3e24 # v31.11.1
        if: runner.os == 'Linux'
        with:
          github_access_token: ${{ secrets.GITHUB_TOKEN }}
      - uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6 # v2.9.2
        if: runner.os != 'Linux'
        with:
          save-if: ${{ github.ref == 'refs/heads/main' }}
      - uses: taiki-e/install-action@f7e5d7c961414b23f5b25b2da9294395d08513ad # v2.87.26
        if: runner.os != 'Linux'
        with:
          tool: cargo-nextest@0.9.146
          fallback: none
      - name: cargo clippy --features gui-native
        id: clippy
        run: ${{ runner.os == 'Linux' && 'nix develop -c' || '' }} cargo clippy --all-targets --features eml_cli/gui-native -- -D warnings
      - name: cargo nextest run --features gui-native
        if: ${{ !cancelled() && steps.clippy.outcome != 'skipped' }}
        run: ${{ runner.os == 'Linux' && 'nix develop -c' || '' }} cargo nextest run --features eml_cli/gui-native --failure-output immediate-final --no-capture
      # Qt は動的にリンクし、配るものに Qt と Windows App SDK のファイルを含めない (docs/implementation/distribution.md)
      - name: nix build links Qt dynamically
        if: ${{ runner.os == 'Linux' && !cancelled() && steps.clippy.outcome != 'skipped' }}
        run: |
          nix build
          readelf -d result/bin/.eml-wrapped | grep -q 'libQt6Widgets.so.6'
          test "$(find -L result \( -name 'libQt6*' -o -name '*.dll' -o -name '*.msix' \) | wc -l)" -eq 0
```

T-10: the 「dependency edges of eml_gui」 step adds `test "$(cargo tree -p eml_cli --features gui-native -e normal,build --target all | grep -c -E 'cxx v|objc2 v|winio-winui3 v|qt-build-utils v')" -ge 4`.

T-8: `nix/third-party-licenses.sh` runs `cargo-about generate --frozen --fail --features gui-native -m crates/eml_cli/Cargo.toml -o "$out" about.hbs`.
