-- `iter_pipeline.em` の各段を、ラムダを書き込んだ専用の `evens` / `squares` / `sum` にした手書きの版。
import Yield (Yield(..))
import Iter (Iter(..))

nums : Int -> Unit -> <Yield Int> Unit
nums n () =
  let go i =
    if i > n then ()
    else
      yield i
      go (i + 1)
  go 1

evens : Iter <> Int -> Iter <> Int
evens it =
  Iter (fn () ->
    match Iter.next it with
      | None -> None
      | Some (x, rest) ->
        if x % 2 == 0 then Some (x, evens rest) else Iter.next (evens rest))

squares : Iter <> Int -> Iter <> Int
squares it =
  Iter (fn () ->
    match Iter.next it with
      | None -> None
      | Some (x, rest) -> Some (x * x, squares rest))

sum : Int -> Iter <> Int -> Int
sum acc it =
  match Iter.next it with
    | None -> acc
    | Some (x, rest) -> sum (acc + x) rest

main : Unit -> <IO> Unit
main () =
  let it = squares (evens (Iter.generate (nums 100000)))
  println "\{sum 0 it}"
