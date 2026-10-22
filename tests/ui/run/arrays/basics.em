-- `Std.Array` と `Std.MutArray` のすべての関数。`set` と `thaw` は共有された配列を写し、一意な配列はその場で
-- 書き換える。範囲外の添字と負の長さは `Panic` なので、`try_panic` が捕まえる。
import Array
import List
import MutArray

noisy : Int -> <IO, Panic> String
noisy i =
  println ("tabulate " ++ show i)
  show (i * 10)

no_ints : List Int
no_ints = []

-- 型変数の要素は一様な位置で受け渡す
first_two : Array a -> <Panic> (a, a)
first_two a = (Array.get a 0, Array.get a 1)

-- 部分適用した `read` と `length` は、組を作る包む関数になる
read_later : MutArray a -> Int -> <Panic> (a, MutArray a)
read_later m i =
  let reader = MutArray.read m
  reader i

main : Unit -> <IO, Panic> Unit
main () =
  let a = Array.make 3 0
  let b = Array.set a 1 5
  println (show (Array.to_list a))
  println (show (Array.to_list b))
  println (show (Array.get b 1, Array.length b))
  println (show (Array.to_list (Array.tabulate 5 (fn i -> i * i))))
  println (show (Array.to_list (Array.from_list ["x", "y", "z"])))
  println (show (Array.length (Array.from_list no_ints)))
  println (show (Array.to_list (Array.make 0 "never")))
  println (show (try_panic (fn () -> Array.get b 3)))
  println (show (try_panic (fn () -> Array.get b (-1))))
  println (show (try_panic (fn () -> Array.length (Array.set b 3 1))))
  println (show (try_panic (fn () -> Array.length (Array.make (-2) 0))))
  println (show (try_panic (fn () -> Array.length (Array.tabulate (-1) (fn i -> i)))))
  -- 共有された `b` の `thaw` は写すので、`b` は変わらない
  let m = Array.thaw b
  let m = MutArray.write m 0 9
  let (x, m) = MutArray.read m 0
  let (n, m) = MutArray.length m
  let c = MutArray.freeze m
  println (show (x, n, Array.to_list c, Array.to_list b))
  -- 一意な `c` の `thaw` は写さない
  let m = MutArray.write (Array.thaw c) 2 7
  println (show (Array.to_list (MutArray.freeze m)))
  let e = MutArray.empty ()
  let (n, e) = MutArray.length e
  let _ = MutArray.freeze e
  println (show n)
  let t = MutArray.tabulate 3 noisy
  let (s, t) = MutArray.read t 2
  println s
  println (show (Array.to_list (MutArray.freeze t)))
  let z = MutArray.make 2 "z"
  println (show (Array.to_list (MutArray.freeze z)))
  println (show (try_panic (fn () -> Array.length (MutArray.freeze (MutArray.make (-1) 0)))))
  println (show (try_panic (fn () -> Array.length (MutArray.freeze (MutArray.tabulate 0 (fn i -> i))))))
  -- 範囲外の読み書きで中断すると、借りていた配列と書くはずだった値は巻き戻しが手放す
  println (show (try_panic (fn () ->
    let (v, m) = MutArray.read (MutArray.make 2 "a") 5
    let _ = MutArray.freeze m
    v)))
  println (show (try_panic (fn () ->
    let (n, m) = MutArray.length (MutArray.make 2 "a")
    Array.length (MutArray.freeze (MutArray.write m n "b")))))
  println (show (first_two (Array.from_list [1, 2, 3]), first_two (Array.from_list ["a", "b"])))
  let (q, m) = read_later (MutArray.make 2 "q") 1
  let len = MutArray.length
  let (n, m) = len m
  let _ = MutArray.freeze m
  println (q ++ show n)
  println (show (List.map Array.length [Array.make 2 0, Array.make 3 1]))
