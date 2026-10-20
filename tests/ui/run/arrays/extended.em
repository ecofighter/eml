-- 配列の専用の命令 (`get` / `length` / `set` / `read` / `write`) を、ループ、共有された要素、入れ子の配列、
-- 失敗の後の巻き戻しで使う。
sum_to : Array Int -> Int -> Int -> <Panic> Int
sum_to a i acc =
  if i == Array.length a then acc else sum_to a (i + 1) (acc + Array.get a i)

fill : MutArray Int -> Int -> <Panic> MutArray Int
fill m i =
  let (n, m) = MutArray.length m
  if i == n then m else fill (MutArray.write m i (i * i)) (i + 1)

bump_all : Array Int -> Int -> <Panic> Array Int
bump_all a i =
  if i == Array.length a then a else bump_all (Array.set a i (Array.get a i + 1)) (i + 1)

main : Unit -> <IO, Panic> Unit
main () =
  let a = Array.tabulate 10 (fn i -> i)
  println (show (sum_to a 0 0))
  let b = bump_all a 0
  println (show (sum_to a 0 0, sum_to b 0 0))
  let squares = MutArray.freeze (fill (MutArray.make 6 0) 0)
  println (show (Array.to_list squares))
  -- 文字列の要素は `get` で共有され、元の配列にも残る
  let words = Array.from_list ["alpha", "beta", "gamma"]
  let w = Array.get words 1
  let words2 = Array.set words 1 (w ++ "!")
  println (show (Array.to_list words, Array.to_list words2, w))
  -- 配列の配列
  let rows = Array.tabulate 3 (fn r -> Array.tabulate 3 (fn c -> r * 3 + c))
  println (show (Array.get (Array.get rows 2) 1, Array.length (Array.get rows 0)))
  let rows2 = Array.set rows 0 (Array.make 1 99)
  println (show (Array.length (Array.get rows 0), Array.get (Array.get rows2 0) 0))
  -- 失敗しても、借りていた配列は呼び出した側に残る
  let r = try_panic (fn () -> Array.get words 3)
  println (show r)
  println (show (Array.to_list words))
  let r2 = try_panic (fn () -> Array.length (Array.set words 5 "x"))
  println (show r2)
  println (show (Array.get words 2))
