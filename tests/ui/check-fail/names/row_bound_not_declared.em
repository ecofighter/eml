-- E1057: シグネチャが上限を付けていない row 変数と、`data` の row の型引数には `never` を書けない。
data Box <e> =
  | Box (Unit -> <never e> Int)

plain : (Unit -> <e> Int) -> <e> Int
plain f = (f : Unit -> <never e> Int) ()

main : Unit -> <IO> Unit
main () = println "unreachable"
