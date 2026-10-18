-- `Yield.fold` で畳む生産者が、外側の `State.run` の `get` と `put` も呼ぶ。`Yield` の handler を特殊化した後も、
-- `State` の操作は外側の handler に届き、畳み込みの結果と外側の状態がどちらも合う
-- (docs/superpowers/specs/2026-10-18-s13e-specialization-design.md の「意味が変わらない理由」)。
import Yield (Yield(..))
import Std.State (State(..))

count_up : Int -> Unit -> <Yield Int, State Int> Unit
count_up n () =
  let go i =
    if i > n then ()
    else
      put (get () + i)
      yield i
      go (i + 1)
  go 1

main : Unit -> <IO> Unit
main () =
  let (total, st) = State.run 100 (fn () -> Yield.fold (fn acc x -> acc + x * 2) 0 (count_up 10))
  println (show total)
  println (show st)
