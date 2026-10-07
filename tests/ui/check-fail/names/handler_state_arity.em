-- E1010 for handlers with a state, and a continuation called with too many arguments.
effect Ask where
  ask : Unit -> Int

effect Fail where
  never fail : Unit -> a

clause : Unit -> Int
clause () =
  handle ask () from 0 with
    | ask () k -> k 1 0
    | return x st -> x

never_clause : Unit -> Int
never_clause () =
  handle fail () from 0 with
    | fail () -> 0
    | return x st -> x

return_clause : Unit -> Int
return_clause () =
  handle ask () from 0 with
    | ask () k st -> k 1 st
    | return x -> x

continuation_arity : Unit -> Int
continuation_arity () =
  handle ask () from 0 with
    | ask () k st -> k 1 st st
    | return x st -> x
