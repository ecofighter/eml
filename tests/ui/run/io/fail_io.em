-- `fail_io` は種類と文言で IO の失敗を起こし、`try_io` が種類ごと捕まえる (docs/spec/stdlib.md の「`fail_io`」)
import FileIn

finish : FileIn.FileIn -> <IO> Int
finish f =
  FileIn.close f
  1

guarded : Bool -> <IO> Int
guarded bad =
  let f = FileIn.open "fail_io.em"
  if bad then fail_io Other "bad input" else finish f

main : Unit -> <IO, Panic> Unit
main () =
  println (show [Cancelled, TimedOut, ConnectionReset, AddrInUse, ResourceLimit])
  match try_io (fn () -> (fail_io ConnectionReset "peer went away" : Unit)) with
    | Ok () -> println "ok"
    | Err e -> println "\{show e.kind}: \{e}"
  match try_io (fn () -> guarded True) with
    | Ok n -> println (show n)
    | Err e -> println "\{show e.kind}: \{e}"
