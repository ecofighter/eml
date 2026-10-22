-- A `data` value with a file field is linear and is taken apart to reach the file.
import FileIn

data Handle =
  | Handle String FileIn.FileIn

main : Unit -> <IO> Unit
main () =
  let h = Handle "input" (FileIn.open "input.txt")
  match h with
    | Handle name f ->
        let (text, f) = FileIn.read_all f
        FileIn.close f
        println (name ++ ": " ++ text)
