-- `if` の片方の腕だけが `put` し、合流の後も続ける。状態を渡す形の特殊化では、合流のブロックが状態を引数で受け、
-- どちらの腕からもその腕の今の状態が届く (docs/spec/core-ir.md の「handler の複製」)。
import Std.State (State(..))

tally : List Int -> <State Int, Panic> Unit
tally xs = match xs with
  | Nil -> ()
  | x :: rest ->
    if x % 2 == 0 then put (get () + x) else ()
    tally rest

main : Unit -> <IO, Panic> Unit
main () = println (show (State.exec 0 (fn () -> tally [1, 2, 3, 4, 5, 6])))
