-- native: skip (the depth limit is the thread's stack, not a word count)
-- max-depth: 4096
-- 1つのタスクは、深さの超過を続けて何度でも捕まえられる。着地の後は限度が元に戻る
-- (docs/spec/vm.md の「`Async` のタスク」)。
import Async (Async)
import String

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

dive : Int -> <Panic> Int
dive n =
  let label = show n
  let below = dive (n + 1)
  below + String.byte_length label

catch_once : Int -> <IO, Panic> Unit
catch_once i =
  match try_io (fn () -> dive 0) with
    | Ok n -> println "unreachable \{show n}"
    | Err e -> println "\{show i} \{show e.kind}: \{e}"

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () ->
    catch_once 1
    catch_once 2
    catch_once 3)
  let _ = spin 40
  Async.await t

main : Unit -> <IO, Panic> Unit
main () = Async.run body
