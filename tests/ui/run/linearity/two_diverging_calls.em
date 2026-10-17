-- 1つの経路に戻らない呼び出しが2つあるときは、評価の順で最初の `fail "a"` の前でだけ `File` を手放す。
import Fail (Fail(..))

g : Int -> Int -> Int
g a b = a + b

both : Unit -> <Fail String, IO> Int
both () =
  let f = Fs.open "input.txt"
  g (Fail.fail "a") (Fail.fail "b")

main : Unit -> <IO> Unit
main () =
  println (show (Fail.to_result both))
