-- E4001: a `match` on character literals has no wildcard arm.
vowel : Char -> Bool
vowel c = match c with
  | 'a' -> True
  | 'e' -> True

main : Unit -> <IO> Unit
main () = println (show (vowel 'a'))
