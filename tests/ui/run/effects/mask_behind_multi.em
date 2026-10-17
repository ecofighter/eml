-- `mask_behind_continuation.em` と同じ形を、`multi` の操作の継続を2回再開して書く。`k True` と `k False` の `mask` は
-- どちらも `Choice@1` で、どちらの再開でも、再開した区間で節が起こす `choose` は内側の handler に届く
-- (docs/spec/effects.md の「健全性」)。
effect Choice where
  multi choose : String -> Bool

answering : (Unit -> <Choice, IO | e> a) -> <IO | e> a
answering action =
  handle action () with
    | choose q k ->
      println "outermost answers \{q}"
      k True

both : (Unit -> <Choice | e> Unit) -> <Choice, IO | e> Unit
both action =
  handle action () with
    | choose q k ->
      let _ = choose "outer sees \{q}"
      handle (let _ = k True in k False) with
        | choose t k2 ->
          println "inner answers \{t}"
          k2 False

main : Unit -> <IO> Unit
main () =
  use answering
  use both
  let a = choose "a"
  let b = choose "b"
  println "a=\{a} b=\{b}"
