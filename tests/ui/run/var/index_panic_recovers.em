-- 範囲の中の `try_panic` が範囲外の添字を捕まえた後も、変数は配列を持ったままで、書いて `freeze` できる。
-- `recover` の `var` は `try_panic` が経路にあるので evidence で動き、`specialized` の `var` は特殊化されて
-- `try_panic` の外側で範囲ごと中断する
recover : Unit -> <Panic> List Int
recover () =
  var a = MutArray.make 3 0
  match try_panic (fn () -> a[7] <- 1) with
    | Ok _ -> ()
    | Err _ -> a[0] <- 1
  match try_panic (fn () -> a[9]) with
    | Ok n -> a[1] <- n
    | Err _ -> a[1] <- 2
  a[2] <- 3
  Array.to_list (MutArray.freeze a)

specialized : Int -> <Panic> Int
specialized i =
  var a = MutArray.make 3 0
  for 0 3 (fn j -> a[j] <- j)
  let n = a[i]
  (fn _ -> n) (MutArray.freeze a)

main : Unit -> <IO, Panic> Unit
main () =
  println (show (recover ()))
  match try_panic (fn () -> specialized 5) with
    | Ok n -> println (show n)
    | Err message -> println message
  match try_panic (fn () -> specialized 2) with
    | Ok n -> println (show n)
    | Err message -> println message
