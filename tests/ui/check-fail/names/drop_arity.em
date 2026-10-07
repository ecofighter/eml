-- E1011: `drop` は値を1つだけ取る。個数を誤った `drop` に渡しただけの継続は、未使用として重ねて報告しない。
effect Ask where
  ask : Unit -> Int

two : Unit -> Int
two () =
  handle ask () with
    | ask () k ->
        drop k 1
        0
