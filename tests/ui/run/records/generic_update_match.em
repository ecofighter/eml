-- An update of a record with a type parameter keeps the other fields, and a later pattern reads them at the instantiated type.
data Box a =
  | Box { item : a, n : Int }

main : Unit -> <IO> Unit
main () =
  let b = { Box { item = 41, n = 1 } | n = 2 }
  match b with
    | Box { item, n } -> println (show (item + n))
  let s = { Box { item = "ab", n = 1 } | n = 3 }
  let Box { item } = s
  println (item ++ "c")
  match { Box { item = 7, n = 0 } | n = 5 } with
    | Box { item } -> println (show (item * 6))
  let job = { Box { item = Fs.open "input.txt", n = 0 } | n = 1 }
  let Box { item = file, n } = job
  Fs.close file
  println (show n)
