-- 構造体を返す関数の中の戻らない呼び出し (`panic` と `fail`)。戻らない呼び出しが構造体を束縛しても、葉はダミーの値で
-- 定義され、実行は届かない。`try_panic` と `Fail.to_result` が中断を受ける
-- (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「LIR」の開くパス)。
import Fail (Fail(..))

data Vec2 =
  | Vec2 Float Float

checked : Int -> <Panic> (Vec2, Int)
checked n = if n > 2 then panic ("too big: " ++ show n) else (Vec2 (Float.of_int n) 0.5, n * 3)

parsed : Int -> <Fail String, Panic> Vec2
parsed n = if n < 0 then Fail.fail "negative" else Vec2 (Float.of_int n) (Float.of_int (n + 1))

total : Int -> <Panic> Float
total n =
  let (Vec2 x y, k) = checked n
  x + y + Float.of_int k

sum : Int -> <Fail String, Panic> Float
sum n =
  let Vec2 a b = parsed n
  a * b

main : Unit -> <IO, Panic> Unit
main () =
  println (show (try_panic (fn () -> total 2)))
  println (show (try_panic (fn () -> total 5)))
  println (show (Fail.to_result (fn () -> sum 3)))
  println (show (Fail.to_result (fn () -> sum (-1))))
