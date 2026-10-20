-- ループの条件が2本以上の変数を `&&`、`||`、`if` で読む。条件の関数の複製は状態の組を返し、融合がその組を合流の
-- ブロックで開く。合流を飛ばす経路でも、合流の先で使う状態が定義される (docs/spec/core-ir.md の「既知のコンストラクタ」)。
both : Unit -> <Panic> Int
both () =
  var j = 0
  var i = 0
  while (fn () -> i < 10 && j == 0) (fn () -> i <- i + 1)
  i

collatz : Int -> <Panic> Int
collatz n0 =
  var n = n0
  var steps = 0
  var peak = n0
  while (fn () -> n != 1 || steps == 0) (fn () ->
    if n % 2 == 0 then n <- n / 2 else n <- 3 * n + 1
    if n > peak then peak <- n else ()
    steps <- steps + 1)
  steps * 1000000 + peak

nested : Int -> <Panic> Int
nested n =
  var i = 0
  var j = 0
  var k = 0
  while (fn () -> i < n) (fn () ->
    j <- 0
    while (fn () -> j < n && (i + j) % 3 != 2) (fn () ->
      k <- k + 1
      j <- j + 1)
    i <- i + 1)
  k

branch : Unit -> <Panic> Int
branch () =
  var i = 0
  var j = 0
  while (fn () -> if i < 10 then j == 0 else False) (fn () -> i <- i + 1)
  i + j

main : Unit -> <IO, Panic> Unit
main () =
  println (show (both ()))
  println (show (collatz 27))
  println (show (collatz 1))
  println (show (nested 10))
  println (show (branch ()))
