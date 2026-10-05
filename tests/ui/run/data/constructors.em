-- Values of constructors with fields live on the heap. They are passed around, captured by
-- closures, built through a partially applied constructor, and freed when no longer used.
data Option a =
  | None
  | Some a

data Pair a b =
  | Pair a b

keep : Option String -> Option String -> Option String
keep a b = a

label : Pair String (Option String) -> String
label p = "pair"

main : Unit -> <IO> Unit
main () =
  let some = Some "kept"
  let chosen = keep some None
  let held = fn u -> keep some u
  let picked = held (Some "other")
  let make = Pair "left"
  let p = make (Some "right")
  println (label p)
  let nested = Some (Some "nested")
  println "done"
