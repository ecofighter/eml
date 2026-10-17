-- パラメータ付き handler の `fn` の節は、操作の結果と次の状態の組を返す (docs/spec/expressions.md の
-- 「パラメータ付き handler」)。`run_state` は `get` と `put` を `fn` の節で書く。`Token` を状態にした handler は、
-- `Lin` の状態を節から次の状態へ渡し、最後にプログラムがそれを消費する。
effect State s where
  get : Unit -> s
  put : s -> Unit

run_state : s -> (Unit -> <State s | e> a) -> <e> (a, s)
run_state init action =
  handle action () from init with
    | fn get () st -> (st, st)
    | fn put st2 _ -> ((), st2)
    | return x st -> (x, st)

counter : Unit -> <State Int> Int
counter () =
  put (get () + 1)
  put (get () * 10)
  get () + 1

data Token : Lin =
  | Token

spend : Token -> String
spend t =
  match t with
    | Token -> "token spent"

effect Tick where
  tick : Unit -> Int

ticks : Unit -> <Tick> Int
ticks () = tick () + tick () + tick ()

main : Unit -> <IO> Unit
main () =
  let (result, final) = run_state 1 counter
  println "\{result} \{final}"
  let (total, token) =
    handle ticks () from Token with
      | fn tick () t -> (2, t)
      | return x t -> (x, t)
  println (show total)
  println (spend token)
