-- k 個のタスクがそれぞれ fib m を計算する。I/O も待ちもない。worker の数を変えて実時間を比べる
-- (docs/implementation/benchmarks.md の「ネットワークの計測」の `bench/async/scale.sh`)
-- 引数: k m
import Async (Async)
import Async
import Env
import String

fib : Int -> <Panic> Int
fib n = if n < 2 then n else fib (n - 1) + fib (n - 2)

starts : Int -> Int -> List (Async.Task Int) -> <IO, Panic, Async> List (Async.Task Int)
starts k m acc =
  if k == 0 then acc
  else starts (k - 1) m (Async.start (fn () -> fib m) :: acc)

awaits : List (Async.Task Int) -> Int -> <IO, Panic, Async> Int
awaits ts sum =
  match ts with
    | Nil -> sum
    | t :: rest -> awaits rest (sum + Async.await t)

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let k = int_arg 0 8
  let m = int_arg 1 27
  println "spread done \{show (awaits (starts k m Nil) 0)}")

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
