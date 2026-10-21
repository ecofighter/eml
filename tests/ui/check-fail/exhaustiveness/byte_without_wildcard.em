-- E4001: `Byte` のリテラルの `match` にワイルドカードの腕がない。
quote : Byte -> Bool
quote b = match b with
  | 34u8 -> True
  | 39u8 -> True

main : Unit -> <IO> Unit
main () = println (show (quote 34u8))
