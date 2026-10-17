-- 自作の `die : String -> <IO> a` も戻らない呼び出しなので、開いたままの `File` を呼び出しの前に手放す。
die : String -> <IO> a
die message =
  eprintln message
  exit 2

main : Unit -> <IO> Unit
main () =
  let f = Fs.open "input.txt"
  println "before"
  die "fatal"
