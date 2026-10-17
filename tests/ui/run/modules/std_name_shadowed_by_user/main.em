-- 利用者の `Iter.em` が `Std.Iter` より先に選ばれ、`import Iter` も `Iter.` の修飾も利用者のものを指す
-- (docs/spec/modules.md)。
import Iter (next)

main : Unit -> <IO> Unit
main () =
  println (show (Iter.next 1))
  println (show (next 2))
