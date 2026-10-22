-- 同じ変数を `src` と `dst` に渡すと、Perceus が参照を1つ足すので、`b` を倍にできる。
double : Int -> Bytes -> <Panic> Bytes
double n b = if n == 0 then b else double (n - 1) (Bytes.append_slice 0 (Bytes.length b) b b)

main : Unit -> <IO, Panic> Unit
main () =
  let b = Bytes.of_string "abc"
  let twice = Bytes.append_slice 0 3 b b
  println (show (twice == Bytes.of_string "abcabc"))
  println (show (Bytes.length b))
  let grown = double 10 (Bytes.of_string "xy")
  println (show (Bytes.length grown))
  println (show (Bytes.slice 2046 2048 grown == Bytes.of_string "xy"))
