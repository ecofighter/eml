-- 1 から 100000 までの合計を直接の末尾再帰で求める。`iter_generate.em` と `push_fold.em` の比べる基準になる。
sum_to : Int -> Int -> Int -> Int
sum_to i n acc = if i > n then acc else sum_to (i + 1) n (acc + i)

main : Unit -> <IO> Unit
main () =
  println "\{sum_to 1 100000 0}"
