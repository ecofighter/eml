-- 文字列を読むだけの関数の中でpanicする。呼ぶ側が貸している文字列は巻き戻しが手放し、検査モードは
-- 漏れを見つけない (docs/spec/vm.mdの「巻き戻し」)。
import Byte
import String

find_bang : String -> Int -> <Panic> Int
find_bang s i =
  if i >= String.byte_length s then panic "no bang"
  else if Byte.to_int (String.byte_at i s) == 33 then i
  else find_bang s (i + 1)

describe : String -> <Panic> String
describe tag =
  let label = tag ++ "/" ++ tag
  label ++ " at " ++ show (find_bang tag 0)

main : Unit -> <IO, Panic> Unit
main () =
  println (describe ("ab" ++ "!"))
  println (describe ("abc" ++ "d"))
  println "unreachable"
