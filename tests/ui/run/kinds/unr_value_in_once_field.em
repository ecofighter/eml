-- `->` のフィールドから取り出した関数は、`once` のフィールドに入れられる。値を読む式の一番外側の矢印は、元の
-- 線形性以上の新しい Kind 変数になるためである (docs/spec/types.md の「値を読む式の矢印の線形性」)。
data Many =
  | Many (Int -> Int)

data Once =
  | Once (once Int -> Int)

convert : Many -> Once
convert m =
  match m with
    | Many f -> Once f

call : Once -> Int -> Int
call o x =
  match o with
    | Once f -> f x

main : Unit -> <IO> Unit
main () = println (show (call (convert (Many (fn x -> x))) 41))
