-- E3002: 値を読む式の矢印は、元の線形性以上の新しい Kind 変数になる。`put inc` はラベル `Store` の型引数を `Unr` に
-- 決めないので、`k` を捕まえたラムダを渡す2つ目の `put` がそれを `Lin` に決め、節で `f` を2回呼ぶと誤りになる
-- (docs/spec/types.md の「値を読む式の矢印の線形性」)。
effect Ask where
  ask : Unit -> Int

effect Store s where
  put : s -> Unit

seq : Unit -> Int -> Int
seq _ n = n

inc : Int -> <Panic> Int
inc x = x + 1

run : Unit -> <Panic> Int
run () =
  handle ask () with
    | ctl ask () k ->
        handle seq (put inc) (seq (put (fn x -> k x)) 0) with
          | ctl put f k2 -> f 1 + f 2 + k2 ()

main : Unit -> <IO, Panic> Unit
main () = println (show (run ()))
