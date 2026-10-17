-- E1010, E1012, E1013, E1014: a clause with the wrong number of parameters, clauses of two effects, a missing
-- clause, duplicate clauses, and a handler without operation clauses are reported in one run.
effect Ask where
  ask : String -> String
  ask_twice : String -> String

effect Log where
  log : String -> Unit

effect Fail where
  never fail : String -> a

run : Unit -> <Ask, Log, Fail> String
run () = ask "x"

arity : Unit -> <Log, Fail> String
arity () =
  handle run () with
    | ctl ask key -> key
    | ctl ask_twice key k -> k key

mixed : Unit -> <Ask, Fail> String
mixed () =
  handle run () with
    | ctl log message k -> k ()
    | ctl ask key k -> k key

missing : Unit -> <Log, Fail> String
missing () =
  handle run () with
    | ctl ask key k -> k key

duplicate : Unit -> <Log, Fail> String
duplicate () =
  handle run () with
    | ctl ask key k -> k key
    | ctl ask_twice key k -> k key
    | ctl ask key k -> k "again"
    | return x -> x
    | return y -> y

never_with_k : Unit -> <Ask, Log> String
never_with_k () =
  handle run () with
    | ctl fail message k -> message

only_return : Unit -> <Ask, Log, Fail> String
only_return () =
  handle run () with
    | return x -> x
