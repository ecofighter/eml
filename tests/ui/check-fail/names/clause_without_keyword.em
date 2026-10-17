-- E1051: 操作の節には `fn` か `ctl` を書く。どちらもない節には、`ctl ` を足す fix が付く
-- (docs/spec/expressions.md の「handler」)。
effect Ask where
  ask : Unit -> Int

main : Unit -> <IO> Unit
main () =
  let n =
    handle ask () + 1 with
      | ask () k -> k 41
  println (show n)
