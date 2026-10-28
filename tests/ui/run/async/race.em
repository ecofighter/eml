-- `race f g` は2つの本体をタスクで競わせ、先に値を返した側を `Left` か `Right` で返し、もう一方を取り消して終わる
-- まで待つ。負けた側の `Lin` の値は巻き戻しが落とし、取り消しを捕まえて値を返した側も勝たない
-- (docs/spec/stdlib.md の「時間」)。`shield` の中の `race` には外の取り消しが届かない。
import Async (Async)
import MutArray

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

after : Int -> a -> <IO, Async> a
after ms x =
  Async.sleep_ms ms
  x

arm : Int -> a -> Unit -> <IO, Panic, Async> a
arm ms x () =
  let _ = spin 40
  after ms x

holding : Unit -> <IO, Panic, Async> MutArray Int
holding () =
  let a = MutArray.make 3 0
  let _ = spin 40
  after 100 a

caught : Unit -> <IO, Panic, Async> String
caught () =
  let _ = spin 40
  match try_io (fn () -> Async.sleep_ms 100) with
    | Ok () -> "slept"
    | Err e ->
      println "loser: \{show e.kind}"
      "caught"

-- 子を `s` に始め、子が `shield` の中で待つ間に `s` を取り消す。子のビットは、子が `shield` の中でスコープを開く前に
-- 立つ
bit_first : (Unit -> <IO, Panic, Async> Unit) -> <IO, Panic, Async> Unit
bit_first child =
  Async.scope (fn s ->
    Async.spawn_in s (fn () ->
      match try_io child with
        | Ok () -> println "unreachable"
        | Err e -> println "after the shield: \{show e.kind}")
    let _ = spin 40
    Async.yield_now ()
    Async.cancel_scope s)

body : Unit -> <IO, Panic, Async> Unit
body () =
  println (show (Async.race (arm 100 "slow") (arm 50 2)))
  println (show (Async.race (arm 30 1) (arm 60 "b")))
  match Async.race holding (fn () -> after 50 (MutArray.make 2 9)) with
    | Left a ->
      drop a
      println "unreachable"
    | Right a ->
      let (x, a) = MutArray.read a 0
      drop a
      println "array \{show x}"
  println (show (Async.race caught (arm 50 "winner")))
  let shielded = try_io (fn () ->
    Async.scope (fn s ->
      Async.cancel_scope s
      Async.shield (fn () -> Async.race (arm 50 "cleanup") (arm 100 0))))
  match shielded with
    | Ok r -> println "shielded: \{show r}"
    | Err e -> println "unreachable \{show e.kind}"
  bit_first (fn () ->
    Async.shield (fn () ->
      Async.yield_now ()
      let r = Async.race (arm 50 "cleanup") (arm 100 0)
      println "bit first: \{show r}")
    Async.yield_now ())

main : Unit -> <IO, Panic> Unit
main () = Async.run body
