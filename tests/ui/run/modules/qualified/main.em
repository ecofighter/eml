-- Qualified names reach a value, a type, and a constructor of `Report.Csv`, which sits in `Report/` under the root.
import Report.Csv

first : Csv.Row
first = Csv.make "a" 1

main : Unit -> <IO> Unit
main () =
  println (Csv.show first)
  let Csv.Row name n = Csv.Row "b" 2
  println (name ++ show n)
