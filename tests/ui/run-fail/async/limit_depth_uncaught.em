-- native: skip (the depth limit is the thread's stack, not a word count)
-- max-depth: 4096
-- 捕まえなかった `ResourceLimit` はスコープの失敗になり、`main` で捕まえなければ実行時エラーで run を止める
-- (docs/spec/stdlib.md の「実行時エラー」)。
import Async (Async)
import String

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

dive : Int -> <Panic> Int
dive n =
  let label = show n
  let below = dive (n + 1)
  below + String.byte_length label

body : Unit -> <IO, Panic, Async> Int
body () =
  Async.spawn (fn () ->
    let _ = spin 40
    let n = dive 0
    println "unreachable \{show n}")
  let _ = spin 40
  1

main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  let n = Async.run body
  println "unreachable \{show n}"
