-- A `never` operation aborts through several frames that each hold a file; every file is released once.
effect Fail where
  never fail : String -> a

deep : Int -> <Fail, IO> Int
deep n =
  let f = Fs.open "input.txt"
  let r = if n == 0 then fail "stop" else deep (n - 1) + 1
  Fs.close f
  r

main : Unit -> <IO> Unit
main () =
  let r =
    handle deep 3 with
      | fail msg -> 0
  println (show r)
