-- `mask_behind_continuation.em` と同じ形を、状態に数を持つ handler で書く。`k () (n + 1)` の `mask` は最後の矢印に
-- 付く `Log@1` で、再開した区間で節が2回目に起こす `log` は内側の handler に、`cb ()` は `P2` に届く
-- (docs/spec/effects.md の「健全性」)。
effect Log where
  log : String -> Unit

tagged : String -> (Unit -> <Log, IO | e> a) -> <IO | e> a
tagged tag action =
  handle action () with
    | ctl log s k ->
      println "\{tag}: \{s}"
      k ()

twice : (Unit -> <e> Unit) -> (Unit -> <Log | e> a) -> <Log | e> a
twice cb action =
  handle action () from 1 with
    | ctl log s k n ->
      log "outer #\{n} sees \{s}"
      cb ()
      handle k () (n + 1) with
        | ctl log t k2 ->
          log "inner sees \{t}"
          k2 ()
    | return x _ -> x

main : Unit -> <IO> Unit
main () =
  use tagged "P2"
  use tagged "P1"
  use twice (fn () -> log "cb")
  log "a"
  log "b"
