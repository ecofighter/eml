-- 注釈のないラムダの引数 `f` を、先に `once` の位置へ渡してから呼ぶ。この時点で `f` は未決の変数なので、
-- 規則 F が `f` を `once` 以下の矢印の写しに決める。決まらないと、後の `f 2` と食い違う (docs/spec/types.md の
-- 「値を読む式の矢印の線形性」)。
run_once : (once Int -> Int) -> Int
run_once f = f 1

idf : Int -> Int
idf x = x

main : Unit -> <IO> Unit
main () =
  let h = fn f ->
    let b = run_once f
    let a = f 2
    (a, b)
  println (show (h idf).0)
