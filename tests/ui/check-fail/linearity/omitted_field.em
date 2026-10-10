-- E3004: a pattern that leaves out a linear field discards it.
data Job = | Job { name : String, log : Fs.File }

main : Unit -> <IO> Unit
main () =
  let job = Job { name = "build", log = Fs.open "log.txt" }
  match job with
    | Job { name } -> println name
