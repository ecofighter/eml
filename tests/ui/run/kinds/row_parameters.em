-- row の型引数を持つ `Iter` を、`<IO>` の生産者、外側に handler がある `Count` の生産者、純粋な生産者の3通りで
-- 引く。総称な `take` はどの row でも同じ関数で、n 個を返した後は生産者を進めない
-- (docs/spec/types.md の「row の型引数」)。
data Iter <e> a =
  | Done
  | More a (once Unit -> <e> Iter <e> a)

effect Yield a where
  yield : a -> Unit

effect Count where
  tick : Unit -> Unit

generate : (Unit -> <Yield a | e> Unit) -> <e> Iter <e> a
generate producer =
  handle producer () with
    | ctl yield x k -> More x k
    | return () -> Done

take : Int -> Iter <e> a -> <e> Iter <e> a
take n it =
  if n <= 0 then
    drop it
    Done
  else
    match it with
      | Done -> Done
      | More x next ->
          if n == 1 then
            drop next
            More x (fn () -> Done)
          else More x (fn () -> take (n - 1) (next ()))

to_list : Iter <e> a -> <e> List a
to_list it =
  match it with
    | Done -> []
    | More x next -> x :: to_list (next ())

print_all : Iter <IO | e> Int -> <IO | e> Unit
print_all it =
  match it with
    | Done -> println "done"
    | More x next ->
        println "got \{x}"
        print_all (next ())

noisy : Int -> <Yield Int, IO> Unit
noisy i =
  println "produce \{i}"
  yield i
  noisy (i + 1)

counted : Int -> Int -> <Yield Int, Count> Unit
counted i n =
  if i > n then ()
  else
    tick ()
    yield (i * 10)
    counted (i + 1) n

pure_count : Int -> Int -> <Yield Int> Unit
pure_count i n =
  if i > n then ()
  else
    yield i
    pure_count (i + 1) n

empty : Iter <> Int
empty = Done

main : Unit -> <IO> Unit
main () =
  let io : Iter <IO> Int = generate (fn () -> noisy 1)
  print_all (take 3 io)
  let report =
    handle to_list (take 3 (generate (fn () -> counted 1 5))) from 0 with
      | ctl tick () k n -> k () (n + 1)
      | return xs n -> "\{show xs} after \{n} ticks"
  println report
  let pure : Iter <> Int = generate (fn () -> pure_count 1 4)
  println (show (to_list (take 10 pure)))
  println (show (to_list (take 2 empty)))
