-- ローカル関数と代数的効果の組み合わせを確かめる。純粋な関数を handle の中と外から呼ぶ形、自分が handler を張る再帰、
-- `multi` の handler の中で純粋な関数を呼んでも `FileIn` を持てること、節の中で `k` を捕まえる関数。
-- (docs/spec/core-ir.md の「ローカル関数の持ち上げ」)
import FileIn

effect Ask where
  ask : Unit -> Int

effect Choice where
  multi choose : Unit -> Bool

inside_and_outside : Unit -> <IO, Panic> Unit
inside_and_outside () =
  let inc n = n + 1
  println (show (inc 1))
  let r = handle inc (ask ()) with
    | ctl ask () k -> k (inc 10)
  println (show r)
  println (show (inc 2))

outside_then_inside : Unit -> <IO, Panic> Unit
outside_then_inside () =
  let inc n = n + 1
  let r = handle inc (ask ()) with
    | ctl ask () k -> k 5
  println (show (r + inc 1))

nested_handlers : Int -> <IO, Panic> Int
nested_handlers n =
  let go k =
    if k == 0 then 0
    else handle go (k - 1) + ask () with
      | ctl ask () r -> r k
  go n

clause_captures_k : Unit -> <IO, Panic> Int
clause_captures_k () =
  handle ask () + 1 with
    | ctl ask () k ->
        let finish v = k v
        finish 1

pure_under_multi : Unit -> <IO, Panic> Unit
pure_under_multi () =
  let f = FileIn.open "input.txt"
  let double n = n + n
  let b = handle (if choose () then double 1 else double 2) with
    | ctl choose () k -> double (k True) + double (k False)
  FileIn.close f
  println (show b)

recursive_resuming_k_twice : Unit -> <Panic> Int
recursive_resuming_k_twice () =
  handle (if choose () then 1 else 10) with
    | ctl choose () k ->
        let both n = if n == 0 then 0 else k True + k False + both (n - 1)
        both 2

main : Unit -> <IO, Panic> Unit
main () =
  inside_and_outside ()
  outside_then_inside ()
  println (show (nested_handlers 3))
  println (show (clause_captures_k ()))
  pure_under_multi ()
  println (show (recursive_resuming_k_twice ()))
