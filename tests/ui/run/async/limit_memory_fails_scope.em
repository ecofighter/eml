-- max-bytes: 268435456 soft 4194304
-- spawn したタスクがメモリの柔らかい上限の印を捕まえなければスコープの失敗になり、`run` が `ResourceLimit` の
-- IO の失敗として起こし直す
-- (docs/spec/stdlib.md の「`Std.Async`」)
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

build : Int -> List Int -> <Panic> List Int
build n acc = if n == 0 then acc else build (n - 1) (n :: acc)

-- 検査モードは解放したブロックを使い回さないので、`yield_now` の確保も数に入る。1回に積む量を多くして、
-- 増えるのがこのタスクだけだとはっきりさせる
grow : Int -> List Int -> <IO, Panic, Async> Unit
grow n acc =
  Async.yield_now ()
  grow (n + 1) (build 1000 acc)

body : Unit -> <IO, Panic, Async> Unit
body () =
  Async.spawn (fn () ->
    let _ = spin 40
    ())
  Async.spawn (fn () ->
    let _ = spin 40
    ())
  Async.spawn (fn () -> grow 0 [])

main : Unit -> <IO, Panic> Unit
main () =
  match try_io (fn () -> Async.run body) with
    | Ok _ -> println "unreachable"
    | Err e -> println "run: \{show e.kind}: \{e}"
