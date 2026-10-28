-- `send` と `recv` は取り消しの地点で、速い道で済むときも、取り消されていれば品物に触れずに投げる。`try_send` と
-- `try_recv` は取り消しの地点でない。`shield` の中の `send` は外で開いたスコープの旗を見ない
-- (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)
import Async
import Chan
import List

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

attempt : String -> (Unit -> <IO, Panic, Async> String) -> <IO, Panic, Async> Unit
attempt name f =
  match try_io f with
    | Ok s -> println "\{name}: \{s}"
    | Err e -> println "\{name}: \{show e.kind}: \{e}"

-- 本体の旗を立ててから、channel の操作を1つずつ試す
flagged : Chan.Chan Int -> Async.Scope -> <IO, Panic, Async> Unit
flagged c s =
  Async.cancel_scope s
  attempt "try_send" (fn () -> show (Chan.try_send 1 c))
  attempt "send" (fn () -> show (Chan.send 2 c))
  attempt "recv" (fn () -> show (Chan.recv c))
  attempt "shielded send" (fn () -> show (Async.shield (fn () -> Chan.send 3 c)))

collect : Chan.Chan Int -> List Int -> <IO, Async> List Int
collect c acc =
  match Chan.recv c with
    | Some x -> collect c (x :: acc)
    | None -> List.reverse acc

body : Unit -> <IO, Panic, Async> Unit
body () =
  let c = Chan.new 2
  Async.scope (fn s -> flagged c s)
  let t = Async.start (fn () -> collect c Nil)
  let _ = spin 40
  Chan.close c
  println "left: \{show (Async.await t)}"

main : Unit -> <IO, Panic> Unit
main () = Async.run body
