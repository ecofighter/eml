-- 1つの送り手が積んだ品物は、積んだ順に受け手に届く。枠のない `send` は枠が空くまで待ち、`close` の後の `recv` は
-- 残った品物を取り切ってから `None` を返す (docs/spec/stdlib.md の「`Std.Chan`」)。
import Async (Async)
import Chan
import List

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

produce : Int -> Chan.Chan Int -> <IO, Panic, Async> Unit
produce n c =
  if n > 5 then Chan.close c
  else match Chan.send n c with
    | Chan.Sent -> produce (n + 1) c
    | Chan.Rejected _ -> println "unreachable"

collect : Chan.Chan Int -> List Int -> <IO, Async> List Int
collect c acc =
  match Chan.recv c with
    | Some x -> collect c (x :: acc)
    | None -> List.reverse acc

body : Unit -> <IO, Panic, Async> Unit
body () =
  let c = Chan.new 2
  Async.spawn (fn () -> produce 1 c)
  let _ = spin 40
  println (show (collect c Nil))
  println (show (Chan.recv c))

main : Unit -> <IO, Panic> Unit
main () = Async.run body
