-- メインのworkerのタスクが始めた子と孫の結果が、親に戻る。子もメインのworkerに置かれることは、`eml_vm`の
-- `pinned.rs`が確かめる (docs/spec/stdlib.md の「`Std.Async`」の「メインの worker」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

pinned : Unit -> <IO, Panic, Async> Int
pinned () =
  let a = Async.start (fn () -> spin 40 + 1)
  let b = Async.start (fn () ->
    let c = Async.start (fn () -> spin 40 + 100)
    let _ = spin 40
    Async.await c + 10)
  let _ = spin 40
  Async.await a + Async.await b

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start_on_main pinned
  let _ = spin 40
  let n = Async.await t
  println "sum: \{show n}"

main : Unit -> <IO, Panic> Unit
main () =
  Async.run body
  println "done"
