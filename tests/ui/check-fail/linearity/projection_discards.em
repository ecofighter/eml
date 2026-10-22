-- E3004: a projection discards the other fields, so it cannot leave a linear field behind.
import FileIn

data Job = | Job { name : String, log : FileIn.FileIn }

main : Unit -> <IO> Unit
main () =
  let job = Job { name = "build", log = FileIn.open "log.txt" }
  println job.name
