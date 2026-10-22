-- 特殊化しない handler の下で、構造体を返す呼び出しの途中で yield する。`point` は `ask` で中断し、yield の確認の後の
-- 遅い道は、構造体の結果の代わりにダミーを返す。呼ぶ側はダミーを受けても葉を使わずに中断を続け、再開した後で本当の
-- 値を受ける (docs/spec/lir.md の「ダミーの値」)。`point` は
-- 展開されないように再帰させる。
import Float

effect Ask where
  ask : Int -> Int

data Vec2 =
  | Vec2 Float Float

point : Int -> <Ask, Panic> (Vec2, Int)
point n = if n > 100 then point (n - 100) else
  let a = ask n
  let b = ask (n + 1)
  (Vec2 (Float.of_int a) (Float.of_int b), a + b)

collect : Int -> <Ask, Panic> Float
collect n = if n == 0 then 0.0 else
  let (Vec2 x y, k) = point n
  x * y + Float.of_int k + collect (n - 1)

main : Unit -> <IO, Panic> Unit
main () =
  let total =
    handle collect 3 with
      | ctl ask n k ->
        let r = k (n * 2)
        r + 1.0
  println (show total)
