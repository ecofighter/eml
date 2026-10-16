-- E4003: ローカル関数の引数は反駁できないパターンでなければならない。
data Option a = | None | Some a

main : Unit -> <IO> Unit
main () =
  let get (Some x) = x
  println (show (get (Some 1)))
