-- ローカル関数の基本の形を確かめる。accumulator のループ (100,000 要素でも深さの上限に当たらない)、外側の引数を捕まえる再帰、
-- `let … in`、値としての受け渡し、部分適用と過剰適用、自分を値として渡す形、再定義、入れ子の関数、row の多相な外側の関数。
-- (docs/spec/expressions.md の「ローカル関数」)
effect Ask where
  ask : Unit -> Int

sum_to : Int -> <Panic> Int
sum_to n =
  let go k acc = if k == 0 then acc else go (k - 1) (acc + k)
  go n 0

build : Int -> <Panic> List Int
build n =
  let go k acc = if k == 0 then acc else go (k - 1) (k :: acc)
  go n []

total : List Int -> <Panic> Int
total xs =
  let go ys acc =
    match ys with
      | [] -> acc
      | y :: rest -> go rest (acc + y)
  go xs 0

scale_all : (Int -> <e> Int) -> List Int -> <e> List Int
scale_all f xs =
  let go acc ys =
    match ys with
      | [] -> List.reverse acc
      | y :: rest -> go (f y :: acc) rest
  go [] xs

show_all : List Int -> List String
show_all xs =
  let go ys =
    match ys with
      | [] -> []
      | y :: rest -> show y :: go rest
  go xs

apply_twice : (Int -> <Panic> Int) -> Int -> <Panic> Int
apply_twice f x =
  let twice g = fn y -> g (g y)
  twice f x

pair_up : a -> b -> (a, b)
pair_up a b =
  let first () = a
  let second () = b
  (first (), second ())

pairs : Unit -> (Int, String)
pairs () = pair_up 1 "one"

over_applied : Int -> <Panic> Int
over_applied n =
  let mk m = fn k -> m + k
  mk n 10

partially_applied : Int -> <Panic> List Int
partially_applied n =
  let add x y = x + y
  let add_n = add n
  List.map add_n [1, 2, 3]

passes_itself : Int -> <Panic> Int
passes_itself n =
  let go f k = if k == 0 then f 0 else go (fn x -> f x + 1) (k - 1)
  go (fn x -> x) n

called_twice : Int -> <Panic> Int
called_twice n =
  let step k = if k == 0 then n else step (k - 1)
  step 3 + step 2

never_called_outside : Int -> <Panic> Int
never_called_outside n =
  let loop k = if k == 0 then n else loop (k - 1)
  n + 1

polymorphic_outer : a -> (a -> <e> b) -> <e> b
polymorphic_outer x f =
  let go y = f y
  go x

with_handler : Unit -> <IO, Panic> Int
with_handler () =
  let go y = y + 1
  handle go 1 + ask () with
    | ctl ask () k -> k (go 40)

higher_order : Unit -> <IO> Int
higher_order () =
  let app f = f ()
  handle app (fn () -> ask ()) with
    | ctl ask () k -> k 7

walk : Int -> <IO, Panic> Unit
walk n =
  let each k = if k == 0 then () else walk (k - 1)
  if n == 0 then println "x" else each n

annotated_parameter : Int -> <Panic> Int
annotated_parameter n =
  let go (acc : Int) k = if k == 0 then acc else go (acc + k) (k - 1)
  go 0 n

main : Unit -> <IO, Panic> Unit
main () =
  println (show (sum_to 100000))
  println (show (total (build 100000)))
  println (show (scale_all (fn x -> x * 2) [1, 2, 3]))
  println (show (show_all [1, 2]))
  println (show (apply_twice (fn x -> x + 3) 1))
  println (show (pairs ()))
  println (show (over_applied 5))
  println (show (partially_applied 1))
  println (show (passes_itself 3))
  println (show (called_twice 4))
  println (show (never_called_outside 4))
  println (show (polymorphic_outer 1 (fn x -> x + 1)))
  println (polymorphic_outer "s" (fn x -> x ++ "!"))
  println (show (handle polymorphic_outer 1 (fn x -> x + ask ()) with
    | ctl ask () k -> k 10))
  println (show (with_handler ()))
  println (show (higher_order ()))
  println (show (let twice x = x + x in twice 21))
  let go x = x + 1
  println (show (go 1))
  let go x = x * 10
  println (show (go 1))
  let x = 5
  let x = x + 1
  println (show x)
  walk 3
  println (show (annotated_parameter 4))
