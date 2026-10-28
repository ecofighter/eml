-- 閉じない channel は run の終わりに放し、残った品物をそのとき落とす。`Lin` の品物も `Unr` の品物も漏れない
-- (docs/spec/stdlib.md の「`Std.Chan`」)。
import Async (Async)
import Async
import Chan
import MutArray

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

put : a -> Chan.Chan a -> <IO, Async> Unit
put x c =
  match Chan.send x c with
    | Chan.Sent -> ()
    | Chan.Rejected x -> drop x

body : Unit -> <IO, Panic, Async> Unit
body () =
  let arrays = Chan.new 2
  let words = Chan.new 2
  let t = Async.start (fn () ->
    put (MutArray.make 2 1) arrays
    put ("word " ++ show 1) words)
  let _ = spin 40
  Async.await t
  println "left in the channels"

main : Unit -> <IO, Panic> Unit
main () =
  Async.run body
  println "done"
