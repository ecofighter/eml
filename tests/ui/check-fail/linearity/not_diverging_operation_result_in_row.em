-- E3003: `S.get` returns the state type of its row, so its call does not end the path.
import Std.State as S

peek : Unit -> <S.State Int, IO> Int
peek () =
  let f = Fs.open "input.txt"
  S.get ()

main : Unit -> <IO> Unit
main () =
  let (n, _) = S.run 0 peek
  println "\{n}"
