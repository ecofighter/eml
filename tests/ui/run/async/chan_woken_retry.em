-- 起こされた受け手より先に、速い道の受け手が品物を取ることがある。そのとき起こされた受け手は列の後ろで待ち直す。
-- どの順でも、2つの品物は2つの受け手に1つずつ届く。どちらが何を受け取るかはスケジュールで決まるので、並べ替えて
-- 出力する (docs/spec/stdlib.md の「`Std.Chan`」)。
import Async (Async)
import Async
import Chan
import List

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

put : a -> Chan.Chan a -> <IO, Async> Unit
put x c =
  match Chan.send x c with
    | Chan.Sent -> ()
    | Chan.Rejected x -> drop x

body : Unit -> <IO, Panic, Async> Unit
body () =
  let c = Chan.new 1
  let a = Async.start (fn () -> Chan.recv c)
  let _ = spin 40
  let b = Async.start (fn () -> Chan.recv c)
  let _ = spin 40
  put 1 c
  let _ = spin 40
  put 2 c
  let x = Async.await a
  let y = Async.await b
  println (show (List.sort [x, y]))
  Chan.close c

main : Unit -> <IO, Panic> Unit
main () = Async.run body
