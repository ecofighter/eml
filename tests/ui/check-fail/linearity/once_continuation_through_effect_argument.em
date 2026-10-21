-- E3002: `once` の継続を捕まえたラムダは、エフェクトのラベルの型引数を通って操作に届いても線形のままである。先に `inc` を
-- 渡すと、読む式で広がった `inc` の矢印が `Unr` 以上に決まる。ラベルが `Store (Int -> <Panic> Int)` で1つの型になるので、
-- 2つ目の `put` の `k` を再開するラムダも同じ型を持ち、節で `f` を2回呼ぶことが使用回数の誤りになる
-- (docs/spec/types.md の「値を読む式の矢印の線形性」)。
effect Ask where
  ask : Unit -> Int

effect Store s where
  put : s -> Unit

data Inc =
  | Inc (Int -> <Panic> Int)

seq : Unit -> Int -> Int
seq _ n = n

run : Inc -> <Panic> Int
run b =
  match b with
    | Inc inc ->
        handle ask () with
          | ctl ask () k ->
              handle seq (put inc) (seq (put (fn x -> k x)) 0) with
                | ctl put f k2 -> f 1 + f 2 + k2 ()

main : Unit -> <IO, Panic> Unit
main () = println (show (run (Inc (fn x -> x + 1))))
