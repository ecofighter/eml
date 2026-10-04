-- Non-tail `if`s become join points: an `if` inside a branch, an `if` without `else` used as a statement,
-- and strings that live across the joins or are dropped before them. Every string must be freed exactly once.
label : Bool -> Bool -> String -> String
label a b s =
  let unused = "dropped"
  let t =
    if a then
      let u = if b then s ++ "!" else "plain"
      u ++ "?"
    else s
  t ++ s

main : Unit -> <IO> Unit
main () =
  println (label True True "a")
  println (label True False "b")
  println (label False True "c")
  let s = "kept"
  if True then println "then"
  println s
