-- 仕事の中の範囲外の読みは `Panic` なので、呼んだタスクで起こし直され、捕まえなければ `run` から実行時エラーとして
-- 出る (docs/spec/stdlib.md の「`compute`」)。
import Async (Async)
import MutArray

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let _ = spin 40
  let n = Async.compute (fn () ->
    let (v, m) = MutArray.read (MutArray.make 2 1) 5
    drop m
    v)
  let _ = spin 40
  println "unreachable \{show n}"

main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  Async.run body
  println "unreachable"
