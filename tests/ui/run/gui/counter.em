-- gui: stub (scripted events in counter.gui)
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
