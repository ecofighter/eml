-- max-bytes: 268435456 soft 4194304
-- メモリの柔らかい上限を越えると、確保の数がいちばん大きいタスクに `ResourceLimit` の IO の失敗が起きて、
-- `try_io` が捕まえる。ほかのタスクは巻き込まない (docs/spec/stdlib.md の「`Std.Async`」)。
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

culprit : Unit -> <IO, Panic, Async> Unit
culprit () =
  match try_io (fn () -> grow 0 []) with
    | Ok _ -> println "unreachable"
    | Err e -> println "\{show e.kind}: \{e}"

body : Unit -> <IO, Panic, Async> Unit
body () =
  Async.spawn (fn () ->
    let _ = spin 40
    ())
  Async.spawn (fn () ->
    let _ = spin 40
    ())
  let t = Async.start culprit
  let _ = spin 40
  Async.await t
  println "done"

main : Unit -> <IO, Panic> Unit
main () = Async.run body
