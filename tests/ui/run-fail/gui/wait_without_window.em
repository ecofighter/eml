-- gui: stub (no window is open when it waits)
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
