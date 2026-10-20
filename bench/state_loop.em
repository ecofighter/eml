-- 末尾でない `put` を `Std.State` の handler の下で回すループ (S14c-2)。`bump` の呼び出しは末尾でない
import Std.State (State(..))

bump : Int -> <State Int, Panic> Unit
bump i = put (get () + i)

loop : Int -> Int -> <State Int, Panic> Unit
loop i n =
  if i > n then ()
  else
    bump i
    loop (i + 1) n

main : Unit -> <IO, Panic> Unit
main () = println (show (State.exec 0 (fn () -> loop 1 100000)))
