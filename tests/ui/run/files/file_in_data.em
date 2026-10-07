-- A `data` value with a file field is linear and is taken apart to reach the file.
data Handle =
  | Handle String Fs.File

main : Unit -> <IO> Unit
main () =
  let h = Handle "input" (Fs.open "input.txt")
  match h with
    | Handle name f ->
        let (f, text) = Fs.read_all f
        Fs.close f
        println (name ++ ": " ++ text)
