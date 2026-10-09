-- Tuples pass through a multi-shot handler: an operation clause takes its tuple argument apart, and each
-- resumption returns a tuple holding a string that the clause destructures.
effect Pick where
  multi pick : (String, String) -> String

both : Unit -> <Pick> (String, Int)
both () =
  let s = pick ("left", "right")
  (s ++ "!", 1)

collect : Unit -> (String, Int)
collect () =
  handle both () with
    | pick (a, b) k ->
      let (x, n) = k a
      let (y, m) = k b
      (x ++ " " ++ y, n + m)

main : Unit -> <IO> Unit
main () =
  let (text, count) = collect ()
  println text
  println (show count)
