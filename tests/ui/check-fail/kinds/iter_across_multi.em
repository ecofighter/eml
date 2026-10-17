-- E3006: `once` の継続を持つ `Iter` は `Lin` なので、持ち越し規則により `multi` の操作をまたいで持てない。
-- `multi` の継続を2回再開すると、`Iter` の中の継続も2回再開されてしまう (docs/spec/effects.md の
-- 「継続の多重度と持ち越し規則」)。
data Iter <e> a =
  | Done
  | More a (once Unit -> <e> Iter <e> a)

effect Choose where
  multi choose : Unit -> Bool

pick : Iter <> Int -> <Choose> Int
pick it =
  let first = choose ()
  match it with
    | Done -> 0
    | More x next ->
        drop next
        if first then x else 0

main : Unit -> <IO> Unit
main () = println "unreachable"
