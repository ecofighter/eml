-- E1051: `never` の操作は再開しないので、その節に `fn` は書けない (docs/spec/expressions.md の「handler」)。
effect Fail where
  never fail : String -> a

main : Unit -> <IO> Unit
main () =
  let n =
    handle fail "no" with
      | fn fail message -> 0
  println (show n)
