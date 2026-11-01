-- spread.em と同じ仕事を `par` で分ける。k 個の fib m を、2つに分けることを繰り返して並べる
import Std.Par
import Env
import String

fib : Int -> <Panic> Int
fib n = if n < 2 then n else fib (n - 1) + fib (n - 2)

-- k 個の fib m の和
split : Int -> Int -> <Panic> Int
split k m =
  if k == 1 then fib m
  else
    let h = k / 2
    let (a, b) = Par.par (fn () -> split h m) (fn () -> split (k - h) m)
    a + b

main : Unit -> <IO, Panic> Unit
main () =
  let k = int_arg 0 8
  let m = int_arg 1 27
  println "spread done \{show (split k m)}"

int_arg : Int -> Int -> <IO, Panic> Int
int_arg i default =
  match nth i (Env.args ()) with
    | Some s ->
      match String.to_int s with
        | Some n -> n
        | None -> default
    | None -> default

nth : Int -> List String -> <Panic> Option String
nth i xs =
  match xs with
    | Nil -> None
    | x :: rest -> if i == 0 then Some x else nth (i - 1) rest
