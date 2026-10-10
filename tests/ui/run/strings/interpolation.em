-- Holes call `display`: a `String` goes in without quotes, other values as `show` writes them, and strings inside a
-- hole's value keep their quotes. Holes run left to right, and `\\{` is not a hole.
data Color = | Red | Green deriving (Show)

noisy : Int -> <IO> Int
noisy n =
  println "eval \{n}"
  n

main : Unit -> <IO> Unit
main () =
  let name = "eml"
  println "Hello, \{name}!"
  println "n = \{1 + 2}, xs = \{[1, 2]}, o = \{Some "x"}, c = \{Red}"
  println "nested: \{[Some "x"]} \{(1, "a")}"
  println "inner: \{"<\{name}>"}"
  println "order: \{noisy 1} \{noisy 2}"
  println "literal \\{name}"
  println "\{-5} \{if True then "yes" else "no"}"
