#!/usr/bin/env eml run
-- | S1 の構文をひととおり使うプログラム
{- ブロックコメントは {- 入れ子に -} できる -}

infixr 5 ::
infixl 6 +, -

data List a =
  | Nil
  | a :: List a

data Option a = | None | Some a

effect State s where
  get : Unit -> s
  put : s -> Unit

effect Fail where
  never fail : String -> a

effect Choose where
  multi choose : Unit -> Bool

len : List a -> Int
len Nil = 0
len (_ :: rest) = 1 + len rest

(<+>) : Int -> Int -> Int
a <+> b = a * 2 + b

try : (Unit -> <Fail | e> a) -> <e> Option a
try action =
  handle action () with
    | fail _ -> None
    | return x -> Some x

run_state : s -> (Unit -> <State s | e> a) -> <e> (a, s)
run_state init action =
  handle action () from init with
    | get () k st -> resume k st st
    | put st2 k _ -> resume k () st2
    | return x st -> (x, st)

counter : Unit -> <State Int> Int
counter () =
  let n = get ()
  put (n + 1)
  n

first : (Unit -> <Choose | e> a) -> <e> a
first action =
  handle action () with | choose () k -> resume k True

classify : Int -> String
classify n =
  match n with
    | 0 -> "zero"
    | -1 -> "minus one"
    | _ ->
        if n > 0 then "positive"
        else "negative"

main : Unit -> <IO> Unit
main () =
  let f = open "data.txt"
  let (f, text) = read_all f
  close f
  let total =
    lines text
      |> map parse_int
      |> fold (+) 0
  if total > 100 then
    println "big"
  else
    println (show_int (total : Int))
  each (Cons 1 Nil) (fn x ->
    println (show_int (x <+> 1))) -- 行末のコメント
  let pair = (total, classify total)
  println pair.1
  let r = run_state 0 (fn () -> counter ())
  println (show_int (r.0 + negate 1))
  let inc = (+ 1)
  let half = (/ 2)
  let name_of = (.name)
  use tmp <- with_temp_dir
  println tmp
