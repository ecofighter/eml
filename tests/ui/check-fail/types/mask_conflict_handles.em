-- E2008: 余った `Log` は囲む handle が足したもので、`outer` のシグネチャの row は `Log` を並べていない。シグネチャの矢印を secondary にしない。
effect Log where
  log : String -> Unit

both : (Unit -> <e> a) -> <Log | e> a
both action =
  log "x"
  action ()

outer : (Unit -> <e> a) -> <e> a
outer action =
  handle (handle both action with
            | log _ k -> k ()) with
    | log _ k -> k ()

main : Unit -> <IO> Unit
main () = println "x"
