-- 待たない子の IO の失敗は `run` で起こし直り、本体の値は捨てる (docs/spec/stdlib.md の「`Std.Async`」)。
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

main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  let n = Async.run body
  println "unreachable \{show n}"
