-- ほかの参照がある値を手放す。`ends`と`middles`はつなぎごとに違う欄を残し、`flip`は共有した鎖から新しい鎖を作るので、
-- tokenは空になり、元の鎖はそのまま残る。最後の呼び出しはただ1つの参照を持ち、一意な道を通る
-- (docs/spec/native.md の「データ」)。
data Chain =
  | End
  | Link String String Int String Chain

build : Int -> <Panic> Chain
build n = if n == 0 then End else Link (show n) (show (n * 10)) n (show (n * 100)) (build (n - 1))

ends : Chain -> <Panic> String
ends c =
  match c with
    | End -> "."
    | Link a _ _ d rest -> a ++ d ++ " " ++ ends rest

middles : Chain -> <Panic> String
middles c =
  match c with
    | End -> "."
    | Link _ b n _ rest -> b ++ show n ++ " " ++ middles rest

flip : Chain -> <Panic> Chain
flip c =
  match c with
    | End -> End
    | Link a b n d rest -> Link d b (n + 1) a (flip rest)

main : Unit -> <IO, Panic> Unit
main () =
  let c = build 3
  println (ends c)
  println (middles c)
  let f = flip c
  println (middles f)
  println (ends f)
  println (ends c)
