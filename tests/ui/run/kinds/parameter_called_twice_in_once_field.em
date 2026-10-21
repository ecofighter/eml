-- 2回呼んだ引数も、`once` のフィールドに入れられる。引数を読む式の矢印が新しい Kind 変数になり、`Unr` のまま
-- 呼んだ後でも `Lin` の位置に渡せる (docs/spec/types.md の「値を読む式の矢印の線形性」)。
data Once =
  | Once (once Int -> Int)

keep : (Int -> Int) -> (Int, Once)
keep f = (f 1, Once f)

both : (Int -> Int) -> Once
both f =
  let n = f 0
  Once f

call : Once -> Int -> Int
call o x =
  match o with
    | Once f -> f x

main : Unit -> <IO> Unit
main () =
  let (n, o) = keep (fn x -> x)
  println (show (n, call o 2))
  println (show (call (both (fn x -> x)) 1))
