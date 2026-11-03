-- 独立した4組の ping と pong が、容量 0 の channel 2本で 20000 回往復する。pong は受けた値を足し、ping が閉じたら
-- 返しの channel を閉じて和を返す。worker の数を変えて、worker をまたぐ channel の費用を見る
-- (docs/implementation/benchmarks.md の「ネットワークの計測」の `bench/async/scale.sh`)
import Async (Async)
import Async
import Chan

ping : Int -> Int -> Chan.Chan Int -> Chan.Chan Int -> <IO, Panic, Async> Unit
ping i n req rep =
  if i > n then Chan.close req
  else
    match Chan.send i req with
      | Chan.Sent ->
        match Chan.recv rep with
          | Some _ -> ping (i + 1) n req rep
          | None -> panic "pong closed early"
      | Chan.Rejected _ -> panic "req closed early"

pong : Int -> Chan.Chan Int -> Chan.Chan Int -> <IO, Panic, Async> Int
pong sum req rep =
  match Chan.recv req with
    | Some x ->
      match Chan.send x rep with
        | Chan.Sent -> pong (sum + x) req rep
        | Chan.Rejected _ -> panic "rep closed early"
    | None ->
      Chan.close rep
      sum

pair : Int -> <IO, Panic, Async> Async.Task Int
pair n =
  let req = Chan.new 0
  let rep = Chan.new 0
  let t = Async.start (fn () -> pong 0 req rep)
  Async.spawn (fn () -> ping 1 n req rep)
  t

pairs : Int -> Int -> List (Async.Task Int) -> <IO, Panic, Async> List (Async.Task Int)
pairs k n acc = if k == 0 then acc else pairs (k - 1) n (pair n :: acc)

awaits : List (Async.Task Int) -> Int -> <IO, Panic, Async> Int
awaits ts sum =
  match ts with
    | Nil -> sum
    | t :: rest -> awaits rest (sum + Async.await t)

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () -> println (show (awaits (pairs 4 20000 Nil) 0)))
