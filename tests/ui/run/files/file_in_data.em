-- A `data` value with a file field is linear and is taken apart to reach the file.
data Handle =
  | Handle String File

main : Unit -> <IO> Unit
main () =
  let h = Handle "input" (open "input.txt")
  match h with
    | Handle name f ->
        let (f, text) = read_all f
        close f
        println (name ++ ": " ++ text)
