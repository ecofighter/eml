-- `iter_pipeline.em` の各段を、ラムダを書き込んだ専用の `evens` / `squares` / `sum` にした手書きの版。
import Yield (Yield(..))
import Iter (Iter)

nums : Int -> Unit -> <Yield Int, Panic> Unit
nums n () =
  let go i =
    if i > n then ()
    else
      yield i
      go (i + 1)
  go 1

evens : Iter <Panic> Int -> Iter <Panic> Int
evens it =
  Iter.from_fn (fn () ->
    match Iter.next it with
      | None -> None
      | Some (x, rest) ->
        if x % 2 == 0 then Some (x, evens rest) else Iter.next (evens rest))

squares : Iter <Panic> Int -> Iter <Panic> Int
squares it =
  Iter.from_fn (fn () ->
    match Iter.next it with
      | None -> None
      | Some (x, rest) -> Some (x * x, squares rest))

sum : Int -> Iter <Panic> Int -> <Panic> Int
sum acc it =
  match Iter.next it with
    | None -> acc
    | Some (x, rest) -> sum (acc + x) rest

main : Unit -> <IO, Panic> Unit
main () =
  let it = squares (evens (Iter.generate (nums 100000)))
  println "\{sum 0 it}"
