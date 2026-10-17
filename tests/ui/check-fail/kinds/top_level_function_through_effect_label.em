-- E3002: トップレベルの関数の参照は、一番外側の矢印が参照ごとの Kind 変数になる。`put inc` はラベル `Store` の型
-- 引数を `Unr` に決めないので、`k` を捕まえたラムダを渡す2つ目の `put` がそれを `Lin` に決め、節で `f` を2回呼ぶと
-- 誤りになる (docs/spec/types.md の「トップレベルの関数の参照」)。
effect Ask where
  ask : Unit -> Int

effect Store s where
  put : s -> Unit

seq : Unit -> Int -> Int
seq _ n = n

inc : Int -> Int
inc x = x + 1

run : Unit -> Int
run () =
  handle ask () with
    | ask () k ->
        handle seq (put inc) (seq (put (fn x -> k x)) 0) with
          | put f k2 -> f 1 + f 2 + k2 ()

main : Unit -> <IO> Unit
main () = println (show (run ()))
