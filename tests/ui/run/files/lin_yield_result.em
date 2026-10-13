-- `take ()` の結果は `File` で、yield の地点の結果が `lin` になる。遅い道ではその結果はダミーなので、EP が遅い道の先頭で
-- drop する (docs/spec/core-ir.md の「EP の明示の `drop`」)。1つ目の handle の節は
-- すぐに再開するので速い道だけを通り、2つ目の handle の節は再開の後に続きがあるので、実行時に遅い道を通る。
effect Src where
  take : Unit -> Fs.File

use_it : Unit -> <Src, IO> Unit
use_it () =
  let f = take ()
  Fs.close f
  println "closed"

main : Unit -> <IO> Unit
main () =
  handle use_it () with
    | take () k -> k (Fs.open "input.txt")
  handle use_it () with
    | take () k ->
      let u = k (Fs.open "input.txt")
      println "resumed"
      u
