-- A destructuring `let` takes a linear field out of a record, so the file can be closed.
import FileIn

data Job =
  | Job { name : String, log : FileIn.FileIn }

main : Unit -> <IO> Unit
main () =
  let j = Job { name = "build", log = FileIn.open "input.txt" }
  let Job { name, log } = j
  let (text, log) = FileIn.read_all log
  FileIn.close log
  println "\{name}: \{text}"
