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
  -- 300 要素の配列は長さの語が拡張される。境界の添字 0, 254, 255, 299 を、一意な場合 (その場) と共有された場合 (写し)
  -- の両方で読み書きする
  let big = Array.tabulate 300 (fn i -> i * 2)
  let edges = [0, 254, 255, 299]
  println (show (List.map (fn i -> Array.get big i) edges, Array.length big))
  let unique = List.fold (fn arr i -> Array.set arr i (0 - i)) (Array.tabulate 300 (fn i -> i * 2)) edges
  println (show (List.map (fn i -> Array.get unique i) edges))
  let copied = List.fold (fn arr i -> Array.set arr i (i + 1000)) big edges
  println (show (List.map (fn i -> Array.get copied i) edges, List.map (fn i -> Array.get big i) edges))
  let shared = Array.set big 255 7
  println (show (Array.get shared 255, Array.get big 255, Array.length shared))
  let bm = fill (MutArray.make 300 0) 0
  let bm = List.fold (fn m i -> MutArray.write m i (i + 1)) bm edges
  let (r0, bm) = MutArray.read bm 0
  let (r254, bm) = MutArray.read bm 254
  let (r255, bm) = MutArray.read bm 255
  let (r299, bm) = MutArray.read bm 299
  let (bn, bm) = MutArray.length bm
  println (show (r0, r254, r255, r299, bn))
  let _ = MutArray.freeze bm
  println (show (try_panic (fn () -> Array.get big 300)))
  println (show (try_panic (fn () -> Array.length (Array.set big 300 1))))
  println (show (try_panic (fn () ->
    let (v, m) = MutArray.read (MutArray.make 300 0) 300
    let _ = MutArray.freeze m
    v)))
  println (show (try_panic (fn () -> Array.length (MutArray.freeze (MutArray.write (MutArray.make 300 0) 300 1)))))
