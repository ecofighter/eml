-- E0011: braces without a constructor are not a record, in an expression or in a pattern.
data P = | P { a : Int }

f : P -> Int
f p = match p with
  | { a } -> 0

main : Unit -> <IO> Unit
main () =
  let q = { a = 1 }
  println "done"
