-- E3001: `once` の継続を捕まえたラムダは、エフェクトのラベルの型引数を通って操作に届いても線形のままである。先に `inc` を
-- 渡すとラベルが `Store (Int -> <Panic> Int)` の `Unr` に決まるので、2つ目の `put` は `k` を再開するラムダを受け取れない。
-- 受け取れると `f` が `k` を2回再開してしまう。`inc` を `->` のフィールドから取り出すのは、トップレベルの関数の参照なら
-- 一番外側の矢印が参照ごとの変数になり、ラベルを `Unr` に決めないためである (docs/spec/types.md の「トップレベルの関数の参照」)。
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
