-- E2008: `both` 自身の `Log` と、row 変数を通って外側の handler に届くべき `Log` を区別できない。
effect Log where
  log : String -> Unit

both : (Unit -> <e> a) -> <Log | e> a
both action =
  log "x"
  action ()

outer : (Unit -> <e> a) -> <Log, Log | e> a
outer action = both action

main : Unit -> <IO> Unit
main () = println "x"
