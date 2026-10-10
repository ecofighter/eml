-- E3004: an update overwrites the old value of a field, so it cannot drop a linear field.
data Job = | Job { name : String, log : Fs.File }

main : Unit -> <IO> Unit
main () =
  let job = Job { name = "build", log = Fs.open "a.txt" }
  let moved = { job | log = Fs.open "b.txt" }
  match moved with
    | Job { log } -> Fs.close log
