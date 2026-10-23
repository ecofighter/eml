-- E1057: 操作のシグネチャの row 変数には `never` を書けない。
effect Run where
  run : (Unit -> <never e> Int) -> Int

main : Unit -> <IO> Unit
main () = println "unreachable"
