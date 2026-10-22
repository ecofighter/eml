-- `take ()` の結果は `File` で、yield の地点の結果が `lin` になる。遅い道ではその結果はダミーなので、EP が遅い道の先頭で
-- drop する (docs/spec/core-ir.md の「EP の明示の `drop`」)。1つ目の handle の節は
-- すぐに再開するので速い道だけを通り、2つ目の handle の節は再開の後に続きがあるので、実行時に遅い道を通る。
import FileIn

effect Src where
  take : Unit -> FileIn.FileIn

use_it : Unit -> <Src, IO> Unit
use_it () =
  let f = take ()
  FileIn.close f
  println "closed"

main : Unit -> <IO> Unit
main () =
  handle use_it () with
    | ctl take () k -> k (FileIn.open "input.txt")
  handle use_it () with
    | ctl take () k ->
      let u = k (FileIn.open "input.txt")
      println "resumed"
      u
