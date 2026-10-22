-- 一方の枝の `match` か分解する `let` だけが参照するタプルを、もう一方の枝の戻らない呼び出しの前で暗黙に手放す。
-- 暗黙の `drop` も参照に数えるので、決定木はタプルを開かずに分解し、`File` を二重に持たない。
import Fail (Fail(..))
import FileIn

by_match : Bool -> <Fail String, IO> Unit
by_match c =
  let f = FileIn.open "input.txt"
  let p = (f, 1)
  if c then
    match p with
      | (g, _) -> FileIn.close g
  else Fail.fail "no"

by_let : Bool -> <IO> Unit
by_let c =
  let f = FileIn.open "input.txt"
  let p = (f, 1)
  if c then
    let (g, _) = p
    FileIn.close g
  else exit 4

main : Unit -> <IO> Unit
main () =
  println (show (Fail.to_result (fn () -> by_match True)))
  println (show (Fail.to_result (fn () -> by_match False)))
  by_let True
  println "closed"
  by_let False
