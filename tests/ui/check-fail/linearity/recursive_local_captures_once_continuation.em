-- E3001: 自分を呼ぶローカル関数は、`once` の継続 `k` を捕まえられない。
effect Ask where
  ask : Unit -> Int

drain : Unit -> Int
drain () =
  handle ask () with
    | ask () k ->
        let go n = if n == 0 then k 0 else k (go (n - 1))
        go 3
