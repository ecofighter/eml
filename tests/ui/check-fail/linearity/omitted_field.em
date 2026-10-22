-- E3004: a pattern that leaves out a linear field discards it.
import FileIn

data Job = | Job { name : String, log : FileIn.FileIn }

main : Unit -> <IO> Unit
main () =
  let job = Job { name = "build", log = FileIn.open "log.txt" }
  match job with
    | Job { name } -> println name
