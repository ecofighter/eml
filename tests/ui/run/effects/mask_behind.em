-- `both` は自分で `Log` を起こし、row 変数を通る `Log` も外側の handler へ渡す。`outer` の中の `both action` の
-- `mask` は `Log@1` で、`both` 自身の `log` は1つ目の handler (`first`) に届き、`action` の `log` は2つ目
-- (`second`) を飛ばして3つ目 (`third`) に届く (docs/spec/effects.md の「健全性」)。
effect Log where
  log : String -> Unit

both : (Unit -> <e> a) -> <Log | e> a
both action =
  log "x"
  action ()

outer : (Unit -> <e> a) -> <Log, Log | e> a
outer action = both action

tagged : String -> (Unit -> <Log, IO | e> a) -> <IO | e> a
tagged tag action =
  handle action () with
    | log s k ->
      println "\{tag}: \{s}"
      k ()

main : Unit -> <IO> Unit
main () =
  use tagged "third"
  use tagged "second"
  use tagged "first"
  outer (fn () -> log "y")
