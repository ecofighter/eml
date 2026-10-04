-- E0004: handlers with `from` come in a later stage.
counter : Unit -> Int
counter () = 0

with_state : Int -> Int
with_state n =
  handle counter () from n with
    | return x st -> x
