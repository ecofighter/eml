-- `mask_behind_continuation.em` と同じ形を、`multi` の操作の継続を2回再開して書く。`k True` と `k False` の `mask` は
-- どちらも `Choice@1` で、どちらの再開でも、再開した区間で節が起こす `choose` は内側の handler に、`cb ()` は `P2` に
-- 届く (docs/spec/effects.md の「健全性」)。
effect Choice where
  multi choose : String -> Bool

answering : String -> (Unit -> <Choice, IO | e> a) -> <IO | e> a
answering tag action =
  handle action () with
    | choose q k ->
      println "\{tag} answers \{q}"
      k True

both : (Unit -> <e> Unit) -> (Unit -> <Choice | e> Unit) -> <Choice, IO | e> Unit
both cb action =
  handle action () with
    | choose q k ->
      let _ = choose "outer sees \{q}"
      cb ()
      handle (let _ = k True in k False) with
        | choose t k2 ->
          println "inner answers \{t}"
          k2 False

main : Unit -> <IO> Unit
main () =
  use answering "P2"
  use answering "P1"
  use both (fn () -> let _ = choose "cb" in ())
  let a = choose "a"
  let b = choose "b"
  println "a=\{a} b=\{b}"
