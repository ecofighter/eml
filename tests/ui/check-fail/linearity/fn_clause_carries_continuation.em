-- E3006: `once` の操作の `fn` の節の継続は、本体を評価している間ずっと生きている。本体が `multi` の操作を起こすと、
-- その継続が区間をまたぐので、持ち越し規則の誤りになる (docs/spec/effects.md の「継続の多重度と持ち越し規則」)。
effect Ask where
  ask : Unit -> Int

effect Choice where
  multi choose : Unit -> Bool

inner : Unit -> <Choice> Int
inner () =
  handle ask () with
    | fn ask () -> if choose () then 1 else 2

main : Unit -> <IO> Unit
main () =
  let total =
    handle inner () with
      | ctl choose () c -> c True + c False
  println (show total)
