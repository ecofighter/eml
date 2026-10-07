-- E3002: `once` の `k` を捕まえたラムダは1回しか呼べないので、2回呼ぶと誤りになる。
effect Ask where
  ask : Unit -> Int

twice : Unit -> Int
twice () =
  handle ask () with
    | ask () k ->
        let f = fn x -> k x
        f 1 + f 2
