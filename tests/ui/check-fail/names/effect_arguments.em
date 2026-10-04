-- E1015: an effect in a row takes as many type arguments as its declaration has. E2001: the type arguments of a
-- performed effect must match those in the row. E1008: the result of a `never` operation cannot be a type
-- parameter of its effect. E1003: the type parameters of an effect have distinct names.
effect State s where
  get : Unit -> s
  put : s -> Unit

effect Pair a a where
  first : Unit -> a

effect Fail e where
  never raise : Unit -> e

missing : Unit -> <State> Int
missing () = 0

extra : Unit -> <State Int String> Int
extra () = 0

wrong : Unit -> <State Int> Unit
wrong () = put "text"
