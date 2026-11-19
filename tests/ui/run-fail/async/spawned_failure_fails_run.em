-- 待たない子のIOの失敗はスコープで起こし直り、本体の値は捨てる (docs/spec/stdlib.mdの「`Std.Async`」)。
import Async (Async)
import FileIn

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

body : Unit -> <IO, Panic, Async> Int
body () =
  Async.spawn (fn () ->
    let _ = spin 40
    let f = FileIn.open "missing.txt"
    FileIn.close f)
  let _ = spin 40
  1

main : Unit -> <IO, Panic, Async> Unit
main () =
  println "before"
  let n = Async.scope (fn _ -> body ())
  println "unreachable \{show n}"
