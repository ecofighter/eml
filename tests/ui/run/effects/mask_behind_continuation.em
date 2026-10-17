-- 節が同じエフェクトの handler を内側に設けて、その中で `k ()` を呼ぶ。`k ()` の `mask` は `Log@1` で、内側の
-- handler を残し、`twice` の外側の `Log` を飛ばす。再開した区間で節が2回目に起こす `log` は、内側の handler に届く
-- (docs/spec/effects.md の「健全性」)。
effect Log where
  log : String -> Unit

printing : (Unit -> <Log, IO | e> a) -> <IO | e> a
printing action =
  handle action () with
    | log s k ->
      println s
      k ()

twice : (Unit -> <Log | e> a) -> <Log | e> a
twice action =
  handle action () with
    | log s k ->
      log "outer sees \{s}"
      handle k () with
        | log t k2 ->
          log "inner sees \{t}"
          k2 ()

main : Unit -> <IO> Unit
main () =
  use printing
  use twice
  log "a"
  log "b"
