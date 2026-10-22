-- 引数を宣言の矢印より多く渡す `die "x" 1 2` も戻らない呼び出しなので、開いたままの `File` を呼び出しの前に手放す。
import FileIn

die : String -> <IO> a
die message =
  eprintln message
  exit 3

main : Unit -> <IO> Unit
main () =
  let f = FileIn.open "input.txt"
  println "before"
  die "x" 1 2
