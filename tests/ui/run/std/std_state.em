import State (State(..))

counter : Unit -> <State Int> Int
counter () =
  State.put (State.get () + 1)
  State.put (State.get () * 10)
  State.get () + 1

count_up : Int -> Unit -> <State Int> Unit
count_up n () =
  let go i =
    if i > n then ()
    else
      State.put (State.get () + i)
      go (i + 1)
  go 1

main : Unit -> <IO> Unit
main () =
  let (result, final) = State.run 1 counter
  println "\{result} \{final}"
  println (show (State.eval 1 counter))
  println (show (State.exec 1 counter))
  println (show (State.exec 0 (count_up 10)))
