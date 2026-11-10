-- ネイティブのGUIのtoolkit (docs/spec/stdlib.mdの「`Std.Gui`」)。`run`は、固定したタスクの上で、toolkitのハンドル`Ui`を
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

-- どのworkerのどのタスクからも呼べる。`run`が終わった後に送ったメッセージは捨てる。GUIのタスクはchannelのただ1つの
-- 受け手で、満ちたchannelで待つと誰も空けないので、そのタスクの`post`は待たずに失敗する (docs/spec/stdlib.mdの「`Std.Gui`」)
pub post : Sink msg -> msg -> <IO, Async> Unit
post (Sink c g) m =
  if is_gui_task_raw g then
    match Chan.try_send m c with
      | Chan.Accepted -> poke_raw g
      | Chan.Full _ -> fail_io ResourceLimit "cannot post: the GUI's message queue is full"
      | Chan.Refused _ -> ()
  else
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

-- `create_raw`の種類の番号と`set_int_raw`の性質の番号 (docs/spec/stdlib.mdの「`Std.Gui`」)
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
-- 呼び出しはすべてそのループの中で動く (docs/spec/vm.mdの「GUIのPark」)
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

-- 閉じたウィンドウの部品は、idの上位32ビットがそのウィンドウと同じである (docs/spec/stdlib.mdの「`Std.Gui`」)
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
extern is_gui_task_raw  : Async.Cell -> <IO> Bool
