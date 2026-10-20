-- 状態を渡す形に特殊化した `State` の経路の関数が、外側の `Iter.generate` の `yield` でも止まる。`Iter.generate` の
-- handler は EP の道に残るので、状態は EP の継続にふつうの変数として捕まり、再開の後も続く
-- (docs/spec/core-ir.md の「handler の特殊化で意味が変わらない理由」)。
import Yield (Yield(..))
import Std.State (State(..))

sums : Int -> Int -> <State Int, Yield Int, Panic> Unit
sums i n =
  if i > n then ()
  else
    put (get () + i)
    yield (get ())
    sums (i + 1) n

running : Int -> Unit -> <Yield Int, Panic> Unit
running n () =
  let total = State.exec 0 (fn () -> sums 1 n)
  yield total

main : Unit -> <IO, Panic> Unit
main () = println (show (Iter.to_list (Iter.generate (running 4))))
