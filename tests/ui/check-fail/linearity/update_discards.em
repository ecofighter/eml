-- E3004: an update overwrites the old value of a field, so it cannot drop a linear field.
import FileIn

data Job = | Job { name : String, log : FileIn.FileIn }

main : Unit -> <IO> Unit
main () =
  let job = Job { name = "build", log = FileIn.open "a.txt" }
  let moved = { job | log = FileIn.open "b.txt" }
  match moved with
    | Job { log } -> FileIn.close log
