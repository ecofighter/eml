-- E2001: `->` のフィールドから取り出した関数は矢印が `Unr` に決まっているので、`once` のフィールドに入れられない。
-- 期待した型には `once` が出て、矢印の線形性の note は付かない。
data Many =
  | Many (Int -> Int)

data Once =
  | Once (once Int -> Int)

convert : Many -> Once
convert m =
  match m with
    | Many f -> Once f

main : Unit -> <IO> Unit
main () = println "unreachable"
