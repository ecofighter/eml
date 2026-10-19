-- E0011: ローカル関数の引数の後ろに結果の型は書けない。引数に型を付けて注釈する。
main : Unit -> <IO, Panic> Unit
main () =
  let twice x : Int = x + x
  println (show (twice 1))
