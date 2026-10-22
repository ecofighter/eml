-- An IO failure under `try_io` releases the file of every frame on the way, and the file of a pair from
-- `Fs.read_all`, once each.
deep : Int -> <IO, Panic> Int
deep n =
  let f = Fs.open "input.txt"
  let r = if n == 0 then String.byte_length (Fs.read_text "missing.txt") else deep (n - 1) + 1
  Fs.close f
  r

held : Unit -> <IO, Panic> Int
held () =
  let (text, f) = Fs.read_all (Fs.open "input.txt")
  let n = String.byte_length (Fs.read_text "missing.txt")
  Fs.close f
  n + String.byte_length text

main : Unit -> <IO, Panic> Unit
main () =
  match try_io (fn () -> deep 3) with
    | Ok r -> println "ok \{r}"
    | Err e -> println "deep: \{e}"
  match try_io held with
    | Ok r -> println "ok \{r}"
    | Err e -> println "held: \{e}"
