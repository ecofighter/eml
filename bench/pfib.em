-- 自分で書いた`par`の再帰。`pfib`はカットオフの2以上の呼び出しでforkするので、ほとんどの呼び出しがforkする。
-- forkあたりの仕事は数命令で、この行はforkとjoinそのものの費用になる
-- (docs/implementation/benchmarks.mdの「基準のプログラム」)。引数がなければ大きさ20 (回数のテストと`bench/run.sh`の
-- 大きさ)、引数があれば何であれ大きさ30で動く。
import Std.Par
import Env

fib : Int -> <Panic> Int
fib n = if n < 2 then n else fib (n - 1) + fib (n - 2)

pfib : Int -> Int -> <Panic> Int
pfib cutoff n =
  if n < cutoff then fib n
  else
    let (a, b) = Par.par (fn () -> pfib cutoff (n - 1)) (fn () -> pfib cutoff (n - 2))
    a + b

size : Unit -> <IO, Panic> Int
size () =
  match Env.args () with
    | Nil -> 20
    | _ -> 30

main : Unit -> <IO, Panic> Unit
main () = println (show (pfib 2 (size ())))
