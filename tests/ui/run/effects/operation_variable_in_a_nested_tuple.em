-- 操作の型変数 (`OpVar`) が入れ子のタプルや data の中にある引数を、節で分解する。節の中の型は、入れ子の型変数の
-- ために構造体にしない (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「構造体にする型」)
data Pair a b = | Pair a b

effect Put where
  put : (Int, (a, Int)) -> Int
  put_pair : (Int, Pair a Int) -> Int

run : Unit -> <Put, Panic> Int
run () = put (1, (2.5, 3)) + put_pair (4, Pair "s" 5)

main : Unit -> <IO, Panic> Unit
main () =
  let n = handle run () with
    | ctl put p k ->
      let (x, (_, y)) = p
      k (x + y)
    | ctl put_pair p k ->
      let (x, Pair _ y) = p
      k (x + y)
  println (show n)
