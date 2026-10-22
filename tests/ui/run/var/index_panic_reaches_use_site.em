-- 添字の範囲は使う所で確かめるので、範囲の中の `try_panic` が範囲外の添字を捕まえる
import MutArray

main : Unit -> <IO, Panic> Unit
main () =
  var a = MutArray.make 3 1
  match try_panic (fn () -> a[5]) with
    | Ok n -> println (show n)
    | Err message -> println message
  println (show a[2])
  drop a
