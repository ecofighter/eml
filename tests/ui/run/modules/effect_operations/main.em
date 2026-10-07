-- `Counter(..)` brings the operation `tick` in unqualified, in calls and in clause heads. The operations of `State`
-- stay qualified in calls and in clause heads, and the row names the effect as `State.State Int`.
import Counter (Counter(..))
import State

count : Unit -> <Counter> Int
count () = tick () + tick ()

bump : Unit -> <State.State Int> Int
bump () =
  State.put (State.get () + 1)
  State.get ()

main : Unit -> <IO> Unit
main () =
  let n =
    handle count () with
      | tick () k -> k 10
  println (show_int n)
  let (m, final) =
    handle bump () from 41 with
      | State.get () k st -> k st st
      | State.put v k _ -> k () v
      | return x st -> (x, st)
  println (show_int m)
  println (show_int final)
