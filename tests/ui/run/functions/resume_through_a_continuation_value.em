-- `ctl` の節で `k` を変数に束縛してから再開する。継続の値への `apply` は、状態のない handler でも状態を持つ handler
-- でも `resume` に戻る
effect Ask where
  ask : Int -> Int

run : Unit -> <Ask> Int
run () = ask 1 + ask 10

main : Unit -> <IO> Unit
main () =
  let r = handle run () with
    | ctl ask n k ->
      let again = k
      again (n + 1)
  println (show r)
  let s = handle run () from 100 with
    | ctl ask n k st ->
      let again = k
      again (n + st) (st + 1)
    | return x st -> x * 1000 + st
  println (show s)
