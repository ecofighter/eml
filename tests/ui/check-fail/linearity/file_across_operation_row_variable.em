-- E3006: the performer of `mk` may put a `multi` operation into the row `e` of the callback `g`, so the clause must not
-- keep a file across the call `g ()`.
import Fs

effect Mk where
  mk : Unit -> ((Unit -> <IO | e> Int) -> <IO | e> Int, Int)

effect Choice where
  multi pick : Unit -> Bool

body : Unit -> <Mk, Choice, IO> Int
body () =
  match mk () with
    | (run, _) -> run (fn () -> if pick () then 1 else 2)

main : Unit -> <IO, Panic> Unit
main () =
  let r =
    handle (handle body () with
      | ctl mk () k ->
          k (fn g ->
            let h = Fs.open "input.txt"
            let x = g ()
            Fs.close h
            x, 0)) with
      | ctl pick () k ->
          let a = k True
          let b = k False
          a + b
  println (show r)
