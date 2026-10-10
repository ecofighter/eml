-- E3004: a projection discards the other fields, so it cannot leave a linear field behind.
data Job = | Job { name : String, log : Fs.File }

main : Unit -> <IO> Unit
main () =
  let job = Job { name = "build", log = Fs.open "log.txt" }
  println job.name
