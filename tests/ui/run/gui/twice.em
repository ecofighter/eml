-- gui: stub (no script, so each shown window gets one close request)
import Async (Async)
import Async
import Gui (Gui(..), Track(..))
import Gui

data Msg =
  | Quit

app : String -> Unit -> <Gui Msg, IO, Panic, Async> Unit
app name () =
  use _ <- Gui.window name Quit
  use Gui.grid [Fr 1] []
  let _ = Gui.label name
  match Gui.wait () with
    | Quit -> println "closed \{name}"

main : Unit -> <IO, Panic> Unit
main () =
  use Async.run
  Gui.run (app "first")
  Gui.run (app "second")
