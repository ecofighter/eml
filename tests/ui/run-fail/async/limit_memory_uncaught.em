-- max-bytes: 268435456 soft 4194304
-- 捕まえなかったメモリの柔らかい上限の印は、`main` でも捕まえなければ `run_result` の位置の実行時エラーで run を止める
-- (docs/spec/stdlib.md の「実行時エラー」)
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
  println "before"
  Async.run body
  println "unreachable"
