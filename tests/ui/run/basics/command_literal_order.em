-- The holes of a command literal run from left to right: the program's hole first, then the arguments' holes in the
-- order they are written, whether plain, spread, inside a word, or inside quotes.
say : String -> <IO> String
say s =
  println s
  s

says : String -> <IO> List String
says s =
  println s
  [s, s]

main : Unit -> <IO> Unit
main () =
  let c = `\{say "p"} a\{say "1"}b \{..says "2"} '\{say "3"} \{say "4"}' \{say "5"}\{say "6"} \{..says "7"}`
  println (display c)
