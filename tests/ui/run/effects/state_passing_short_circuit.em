-- 状態つきの2つの handler の下で、ループの条件が `if` で2つの状態を読む。条件の関数の複製は状態の組を返し、融合が
-- その組を合流のブロックで開く。合流を飛ばす経路でも、合流の先で使う状態が定義される (docs/spec/core-ir.md の
-- 「既知のコンストラクタ」)。
effect I where
  get_i : Unit -> Int
  set_i : Int -> Unit

effect J where
  get_j : Unit -> Int

body : Unit -> <I, J, Panic> Int
body () =
  let c () = if get_i () < 10 then get_j () == 0 else False
  let go () = if c () then
      set_i (get_i () + 1)
      go ()
    else ()
  go ()
  get_i ()

run_i : (Unit -> <I | e> x) -> <e> x
run_i action =
  handle action () from 0 with
    | fn get_i () s -> (s, s)
    | fn set_i v _ -> ((), v)
    | return x _ -> x

run_j : (Unit -> <J | e> x) -> <e> x
run_j action =
  handle action () from 0 with
    | fn get_j () s -> (s, s)
    | return x _ -> x

main : Unit -> <IO, Panic> Unit
main () = println (show (run_j (fn () -> run_i body)))
