-- E3004: `: Lin` と宣言した型の値は、`_` で捨てられない。手放すには `drop` を書く。
data Token : Lin =
  | Token

lose : Token -> Int
lose _ = 0

main : Unit -> <IO> Unit
main () = println (show (lose Token))
