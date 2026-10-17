-- `yield` の `once` の継続を `data` のフィールドに入れ、後で1回だけ再開して値を引く。最後まで引く場合と、
-- 2つ引いて残りを `drop` する場合を確かめる (docs/spec/types.md の「1回だけ呼べる関数の型」)。
data Iter <e> a =
  | Done
  | More a (once Unit -> <e> Iter <e> a)

effect Yield a where
  yield : a -> Unit

generate : (Unit -> <Yield a | e> Unit) -> <e> Iter <e> a
generate producer =
  handle producer () with
    | yield x k -> More x k
    | return () -> Done

count : Int -> Int -> <Yield Int> Unit
count i n =
  if i > n then ()
  else
    yield i
    count (i + 1) n

to_list : Iter <e> a -> <e> List a
to_list it =
  match it with
    | Done -> []
    | More x next -> x :: to_list (next ())

take_two : Iter <e> a -> <e> List a
take_two it =
  match it with
    | Done -> []
    | More x next ->
        match next () with
          | Done -> [x]
          | More y rest ->
              drop rest
              [x, y]

main : Unit -> <IO> Unit
main () =
  println (show (to_list (generate (fn () -> count 1 5))))
  println (show (take_two (generate (fn () -> count 1 5))))
  println (show (take_two (generate (fn () -> count 1 1))))
