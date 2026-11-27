-- gui: stub (scripted events in progress.gui)
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
      -- 手で確かめるときに棒の進みが見えるよう、段ごとに待つ。UIテストの仮想の時計では、待ちに実際の時間はかからない (docs/implementation/testing.mdの「GUIの手での確認」)
      Async.sleep_ms 300
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

main : Unit -> <IO, Panic, Async> Unit
main () =
  Gui.run app
