-- 操作の結果が関数のとき、余った引数は操作の結果に適用する (docs/spec/core-ir.md の eval/apply)。
effect Reader r where
  ask : Unit -> r

use_it : Unit -> <Reader (Int -> Int)> Int
use_it () = ask () 5

main : Unit -> <IO> Unit
main () =
  let n =
    handle use_it () with
      | ask () k -> resume k (fn x -> x + 1)
  println (show_int n)
