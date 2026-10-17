-- `State.run` の本体から、yield しない `<IO>` の補助の関数が `exit 5` を呼ぶ。巻き戻しは handler を外して
-- `Finish` まで届く。
import State (State(..))

stop : Int -> <IO> Unit
stop n =
  println "stop at \{n}"
  exit 5

count : Unit -> <State Int, IO> Unit
count () =
  State.put (State.get () + 1)
  stop (State.get ())

main : Unit -> <IO> Unit
main () =
  let (_, n) = State.run 0 count
  println "never \{n}"
