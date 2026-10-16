-- ローカル関数と代数的効果の組み合わせを確かめる。純粋な関数を handle の中と外から呼ぶ形、自分が handler を張る再帰、
-- `multi` の handler の中で純粋な関数を呼んでも `File` を持てること、節の中で `k` を捕まえる関数。
-- (docs/spec/core-ir.md の「ローカル関数の持ち上げ」)
effect Ask where
  ask : Unit -> Int

effect Choice where
  multi choose : Unit -> Bool

inside_and_outside : Unit -> <IO> Unit
inside_and_outside () =
  let inc n = n + 1
  println (show (inc 1))
  let r = handle inc (ask ()) with
    | ask () k -> k (inc 10)
  println (show r)
  println (show (inc 2))

outside_then_inside : Unit -> <IO> Unit
outside_then_inside () =
  let inc n = n + 1
  let r = handle inc (ask ()) with
    | ask () k -> k 5
  println (show (r + inc 1))

nested_handlers : Int -> <IO> Int
nested_handlers n =
  let go k =
    if k == 0 then 0
    else handle go (k - 1) + ask () with
      | ask () r -> r k
  go n

clause_captures_k : Unit -> <IO> Int
clause_captures_k () =
  handle ask () + 1 with
    | ask () k ->
        let finish v = k v
        finish 1

pure_under_multi : Unit -> <IO> Unit
pure_under_multi () =
  let f = Fs.open "input.txt"
  let double n = n + n
  let b = handle (if choose () then double 1 else double 2) with
    | choose () k -> double (k True) + double (k False)
  Fs.close f
  println (show b)

main : Unit -> <IO> Unit
main () =
  inside_and_outside ()
  outside_then_inside ()
  println (show (nested_handlers 3))
  println (show (clause_captures_k ()))
  pure_under_multi ()
