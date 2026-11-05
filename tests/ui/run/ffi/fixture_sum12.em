-- fixtures: only runnable through the test harness
-- fixture ids: 4..5
-- 12 個の引数を C の関数に渡す。レジスタに乗らない引数はスタックで渡り、整数と浮動小数点のレジスタが混ざる。引数ごとに
-- 重みが違うので、取り違えと符号の広げ損ねが結果に出る。`neg` は引数のない関数の `int32_t` の結果を符号付きで読む。

extern "C" pure symbol "eml_fixture_sum12" sum12 : Int as int -> Int -> Int as int -> Int as size_t -> Int as int -> Int as int -> Int as int -> Int as int -> Int as int -> Int as int -> Float -> Float -> <Panic> Int
extern "C" pure symbol "eml_fixture_neg" neg : Unit -> Int as int
extern "C" pure symbol "eml_fixture_add_i32" add : Int as int -> Int as int -> <Panic> Int as int

main : Unit -> <IO, Panic> Unit
main () =
  println (show (sum12 (0 - 1) 2 (0 - 3) 4 5 6 7 8 9 10 0.5 0.25))
  println (show (neg ()))
  println (show (add 2147483647 1))
