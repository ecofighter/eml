-- E1031: a qualifier is one segment, so `Report.Csv.parse` fails and the help shows `Csv.parse`; `Json` is not
-- imported at all.
import Report.Csv

main : Unit -> <IO> Unit
main () =
  println (show (Report.Csv.parse "a"))
  println (show (Json.parse "b"))
