-- `Std.Stream` は通常のファイルだけを開く。ディレクトリは、読む向きでも作る向きでも、どの OS でも種類 `NotAvailable` の
-- `not a regular file` で断り、driver に渡さない。ないファイルは `FileIn.open` と同じく `NotFound` である
-- (docs/spec/stdlib.md の「`Std.Stream`」)。
import Async (Async)
import Async
import Stream

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

report : Result IOError a -> <IO> Unit
report r =
  match r with
    | Ok _ -> println "ok"
    | Err e -> println "\{show e.kind}: \{e.message}"

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let t = Async.start (fn () -> spin 40)
  let _ = spin 40
  report (try_io (fn () -> Stream.close (Stream.open ".")))
  report (try_io (fn () -> Stream.close_writer (Stream.create ".")))
  report (try_io (fn () -> Stream.close_writer (Stream.append ".")))
  report (try_io (fn () -> Stream.close (Stream.open "missing.txt")))
  println (show (Async.await t)))
