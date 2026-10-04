-- E0004: `multi` operations, effect type parameters, effects with type arguments, and handlers with `from` come
-- in later stages.
effect Choice where
  multi choose : Unit -> Bool

effect State s where
  get : Unit -> s

counter : Unit -> <State Int> Int
counter () = 0

with_state : Int -> Int
with_state n =
  handle counter () from n with
    | return x st -> x
