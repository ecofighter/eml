-- E0011: 宣言の型引数の `<…>` に書けるのは row 変数の名前1つだけである。閉じた row や開いた row は書けない。
data Fixed <IO> =
  | Fixed Int

data Open <e | f> =
  | Open Int

main : Unit -> <IO> Unit
main () = println "unreachable"
