-- E0004: `data T : Unr` はまだ書けない。書ける Kind は `Lin` だけである。
data Plain : Unr =
  | Plain Int

main : Unit -> <IO> Unit
main () = println "unreachable"
