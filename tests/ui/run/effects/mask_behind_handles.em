-- 余った `Log` は `outer` の外側の handle が足したもので、`outer` のシグネチャの row は `Log` を並べていない。
-- `both action` の `mask` は `Log@1` で、`both` 自身の `log` は内側の handler に届き、`action` の `log` は外側の
-- handler を飛ばして `outer` の外の handler に届く (docs/spec/effects.md の「健全性」)。
effect Log where
  log : String -> Unit

both : (Unit -> <e> a) -> <Log | e> a
both action =
  log "x"
  action ()

outer : (Unit -> <IO | e> a) -> <IO | e> a
outer action =
  handle (handle both action with
            | ctl log s k ->
              println "inner: \{s}"
              k ()) with
    | ctl log s k ->
      println "middle: \{s}"
      k ()

tagged : String -> (Unit -> <Log, IO | e> a) -> <IO | e> a
tagged tag action =
  handle action () with
    | ctl log s k ->
      println "\{tag}: \{s}"
      k ()

main : Unit -> <IO> Unit
main () =
  use tagged "outside"
  outer (fn () -> log "y")
