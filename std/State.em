-- 状態を持ち回る `State` エフェクト (docs/spec/stdlib.md)。パラメータ付き handler の `fn` の節で、
-- 結果と次の状態の組を返す (docs/spec/expressions.md)。

pub effect State s where
  get : Unit -> s
  put : s -> Unit

pub run : s -> (Unit -> <State s | e> a) -> <e> (a, s)
run init action =
  handle action () from init with
    | fn get () st -> (st, st)
    | fn put st2 _ -> ((), st2)
    | return x st -> (x, st)

pub eval : s -> (Unit -> <State s | e> a) -> <e> a
eval init action =
  let (x, _) = run init action
  x

pub exec : s -> (Unit -> <State s | e> a) -> <e> s
exec init action =
  let (_, st) = run init action
  st
