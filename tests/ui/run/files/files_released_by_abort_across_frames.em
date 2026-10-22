-- A `never` operation aborts through several frames that each hold a file; every file is released once.
import Fs

effect Fail where
  never fail : String -> a

deep : Int -> <Fail, IO, Panic> Int
deep n =
  let f = Fs.open "input.txt"
  let r = if n == 0 then fail "stop" else deep (n - 1) + 1
  Fs.close f
  r

main : Unit -> <IO, Panic> Unit
main () =
  let r =
    handle deep 3 with
      | ctl fail msg -> 0
  println (show r)
