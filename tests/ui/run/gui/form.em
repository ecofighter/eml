-- gui: stub (scripted events in form.gui)
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
