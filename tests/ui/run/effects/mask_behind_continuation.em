-- 節が同じエフェクトの handler を内側に設けて、その中で `k ()` を呼ぶ。`k ()` の `mask` は `Log@1` で、内側の
-- handler を残し、`twice` の外側の `Log` (`P1`) を飛ばす。再開した区間で節が2回目に起こす `log` は内側の handler に
-- 届き、`cb ()` は節の row の最初の `Log` を飛ばして `P2` に届く。`k ()` に `mask` がなければ、2回目の `cb ()` は
-- 内側の handler を飛ばして `P1` に届いてしまう (docs/spec/effects.md の「健全性」)。
effect Log where
  log : String -> Unit

tagged : String -> (Unit -> <Log, IO | e> a) -> <IO | e> a
tagged tag action =
  handle action () with
    | log s k ->
      println "\{tag}: \{s}"
      k ()

twice : (Unit -> <e> Unit) -> (Unit -> <Log | e> a) -> <Log | e> a
twice cb action =
  handle action () with
    | log s k ->
      log "outer sees \{s}"
      cb ()
      handle k () with
        | log t k2 ->
          log "inner sees \{t}"
          k2 ()

main : Unit -> <IO> Unit
main () =
  use tagged "P2"
  use tagged "P1"
  use twice (fn () -> log "cb")
  log "a"
  log "b"
