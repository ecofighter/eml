-- `run` は根のスコープのすべてのタスクを、待たない孫も含めて待ってから戻る (docs/spec/stdlib.md の「`Std.Async`」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  Async.spawn (fn () ->
    Async.spawn (fn () ->
      let _ = spin 40
      println "grandchild")
    let _ = spin 40
    ())
  let _ = spin 40
  ()

main : Unit -> <IO, Panic> Unit
main () =
  Async.run body
  println "after run"
