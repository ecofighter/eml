-- `drop` はハンドルを放すだけでタスクを取り消さない。落としたタスクの `Lin` の結果は終わったときに放し、検査モードで
-- 漏れが出ない (docs/spec/stdlib.md の「`Std.Async`」)。
import Async (Async)
import MutArray

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () ->
    let _ = spin 40
    println "still runs"
    MutArray.make 2 0)
  drop t
  let _ = spin 40
  ()

main : Unit -> <IO, Panic> Unit
main () =
  Async.run body
  println "after run"
