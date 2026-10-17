-- `mask_behind_continuation.em` と同じ形を、状態に数を持つ handler で書く。`k () (n + 1)` の `mask` は最後の矢印に
-- 付く `Log@1` で、再開した区間で節が2回目に起こす `log` は内側の handler に届く (docs/spec/effects.md の「健全性」)。
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
  handle action () from 1 with
    | log s k n ->
      log "outer #\{n} sees \{s}"
      handle k () (n + 1) with
        | log t k2 ->
          log "inner sees \{t}"
          k2 ()
    | return x _ -> x

main : Unit -> <IO> Unit
main () =
  use printing
  use twice
  log "a"
  log "b"
