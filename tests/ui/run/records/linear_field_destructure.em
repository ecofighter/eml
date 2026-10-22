-- A destructuring `let` takes a linear field out of a record, so the file can be closed.
import Fs

data Job =
  | Job { name : String, log : Fs.File }

main : Unit -> <IO> Unit
main () =
  let j = Job { name = "build", log = Fs.open "input.txt" }
  let Job { name, log } = j
  let (text, log) = Fs.read_all log
  Fs.close log
  println "\{name}: \{text}"
