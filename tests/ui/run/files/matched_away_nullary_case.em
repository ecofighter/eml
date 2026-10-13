-- `File` を持つ `data` の、フィールドのない case の葉は、調べた値を drop する
-- (docs/superpowers/specs/2026-10-13-s8d-kind-marks-design.md の「明示の drop」)。
data Res =
  | Empty
  | Has Fs.File

fin : Res -> <IO> Unit
fin r =
  match r with
    | Empty -> println "empty"
    | Has f ->
        Fs.close f
        println "closed"

main : Unit -> <IO> Unit
main () =
  fin Empty
  fin (Has (Fs.open "input.txt"))
