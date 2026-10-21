data P =
  | P Char Int
  deriving (Eq, Ord)

main : Unit -> <IO, Panic> Unit
main () =
  println (show ('a' < 'b'))
  println (show ('é' > 'z'))
  println (show ('a' == 'a'))
  println (show ('a' != 'a'))
  println (show (compare 'b' 'a'))
  println (show (P 'a' 2 < P 'b' 1))
