-- タスクの中の `try_all` は、Panic、IO の失敗、`fail_io` の失敗をタスクの値に分け、スコープの失敗にしない
-- (docs/spec/stdlib.md の「`Std.Async`」)。
import Async (Async)
import FileIn

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

open_missing : Unit -> <IO> Int
open_missing () =
  let f = FileIn.open "missing.txt"
  FileIn.close f
  0

describe : Result Async.Failure Int -> String
describe r = match r with
  | Ok n -> "ok \{show n}"
  | Err (Async.Panicked m) -> "panicked: \{m}"
  | Err (Async.IOFailed e) -> "io failed: \{show e.kind}: \{e}"

body : Unit -> <IO, Panic, Async> Unit
body () =
  let a = Async.start (fn () -> Async.try_all (fn () -> spin 40 + 1))
  let b = Async.start (fn () -> Async.try_all (fn () -> (panic "boom" : Int)))
  let c = Async.start (fn () -> Async.try_all (fn () -> open_missing ()))
  let d = Async.start (fn () -> Async.try_all (fn () -> fail_io TimedOut "too slow"))
  let _ = spin 40
  println (describe (Async.await a))
  println (describe (Async.await b))
  println (describe (Async.await c))
  println (describe (Async.await d))

main : Unit -> <IO, Panic> Unit
main () =
  Async.run body
  println "run finished"
