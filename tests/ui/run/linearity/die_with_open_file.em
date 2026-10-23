-- 自作の `die : String -> <IO> a` も戻らない呼び出しなので、開いたままの `FileIn` を呼び出しの前に手放す。
import FileIn

die : String -> <IO> a
die message =
  eprintln message
  exit 2

main : Unit -> <IO> Unit
main () =
  let f = FileIn.open "input.txt"
  println "before"
  die "fatal"
