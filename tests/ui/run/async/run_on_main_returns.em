-- `run_on_main` はメインの worker で動かした関数の値を返す。値は待つ側のタスクに戻る
-- (docs/spec/stdlib.md の「`Std.Async`」の「メインの worker」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () -> spin 40 + 1)
  let _ = spin 40
  let n = Async.run_on_main (fn () -> spin 40 + 20)
  let m = Async.await t
  println "main worker: \{show n}"
  println "plain task: \{show m}"

main : Unit -> <IO, Panic> Unit
main () =
  Async.run body
  let k = Async.run (fn () -> Async.run_on_main (fn () -> 7))
  println "after run: \{show k}"
