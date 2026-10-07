-- E1026: `Report.Missing` has no file under the root. The names used through the broken import report nothing more.
import Report.Missing (load)

main : Unit -> <IO> Unit
main () =
  println (Missing.load "a")
  println (load "b")
