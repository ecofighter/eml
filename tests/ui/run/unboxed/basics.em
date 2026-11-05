-- `Std.UnboxedArray` と `Std.MutUnboxedArray` の、`Std.Array` と `Std.MutArray` に対応する関数。`set` と `thaw` は
-- 共有された配列を写し、一意な配列はその場で書き換える。範囲外の添字と負の長さは `Panic` なので、`try_panic` が
-- 捕まえる。
import Float
import List
import MutUnboxedArray
import UnboxedArray

noisy : Int -> <IO, Panic> Float
noisy i =
  println ("tabulate " ++ show i)
  Float.of_int (i * 10)

no_floats : List Float
no_floats = []

-- 要素の型ごとの行は、クラスの制約で選ぶ
first_two : UnboxedArray.Unboxed a => UnboxedArray a -> <Panic> (a, a)
first_two a = (UnboxedArray.get a 0, UnboxedArray.get a 1)

-- 部分適用した `read` と `length` は、組を作る包む関数になる
read_later : MutUnboxedArray.MutUnboxed a => MutUnboxedArray a -> Int -> <Panic> (a, MutUnboxedArray a)
read_later m i =
  let reader = MutUnboxedArray.read m
  reader i

main : Unit -> <IO, Panic> Unit
main () =
  let a = UnboxedArray.make 3 0.0
  let b = UnboxedArray.set a 1 5.0
  println (show (UnboxedArray.to_list a))
  println (show (UnboxedArray.to_list b))
  println (show (UnboxedArray.get b 1, UnboxedArray.length b))
  println (show (UnboxedArray.to_list (UnboxedArray.tabulate 5 (fn i -> Float.of_int (i * i)))))
  println (show (UnboxedArray.to_list (UnboxedArray.from_list [1.5, 2.5, 3.5])))
  println (show (UnboxedArray.length (UnboxedArray.from_list no_floats)))
  println (show (UnboxedArray.to_list (UnboxedArray.make 0 (-1.0))))
  println (show (try_panic (fn () -> UnboxedArray.get b 3)))
  println (show (try_panic (fn () -> UnboxedArray.get b (-1))))
  println (show (try_panic (fn () -> UnboxedArray.length (UnboxedArray.set b 3 1.0))))
  println (show (try_panic (fn () -> UnboxedArray.length (UnboxedArray.make (-2) 0.0))))
  println (show (try_panic (fn () -> UnboxedArray.length (UnboxedArray.tabulate (-1) (fn i -> Float.of_int i)))))
  -- 共有された `b` の `thaw` は写すので、`b` は変わらない
  let m = UnboxedArray.thaw b
  let m = MutUnboxedArray.write m 0 9.0
  let (x, m) = MutUnboxedArray.read m 0
  let (n, m) = MutUnboxedArray.length m
  let c = MutUnboxedArray.freeze m
  println (show (x, n, UnboxedArray.to_list c, UnboxedArray.to_list b))
  -- 一意な `c` の `thaw` は写さない
  let m = MutUnboxedArray.write (UnboxedArray.thaw c) 2 7.0
  println (show (UnboxedArray.to_list (MutUnboxedArray.freeze m)))
  let e = MutUnboxedArray.empty ()
  let (n, e) = MutUnboxedArray.length e
  let _ = MutUnboxedArray.freeze e
  println (show n)
  let t = MutUnboxedArray.tabulate 3 noisy
  let (s, t) = MutUnboxedArray.read t 2
  println (show s)
  println (show (UnboxedArray.to_list (MutUnboxedArray.freeze t)))
  let z = MutUnboxedArray.make 2 2.5
  println (show (UnboxedArray.to_list (MutUnboxedArray.freeze z)))
  println (show (try_panic (fn () -> UnboxedArray.length (MutUnboxedArray.freeze (MutUnboxedArray.make (-1) 0.0)))))
  println (show (try_panic (fn () -> UnboxedArray.length (MutUnboxedArray.freeze (MutUnboxedArray.tabulate 0 (fn i -> Float.of_int i))))))
  -- 範囲外の読み書きで中断すると、借りていた配列は巻き戻しが手放す
  println (show (try_panic (fn () ->
    let (v, m) = MutUnboxedArray.read (MutUnboxedArray.make 2 1.0) 5
    let _ = MutUnboxedArray.freeze m
    v)))
  println (show (try_panic (fn () ->
    let (n, m) = MutUnboxedArray.length (MutUnboxedArray.make 2 1.0)
    UnboxedArray.length (MutUnboxedArray.freeze (MutUnboxedArray.write m n 2.0)))))
  println (show (first_two (UnboxedArray.from_list [1, 2, 3]), first_two (UnboxedArray.from_list [0.5, 0.25])))
  let (q, m) = read_later (MutUnboxedArray.make 2 4.0) 1
  let len = MutUnboxedArray.length
  let (n, m) = len m
  let _ = MutUnboxedArray.freeze m
  println (show q ++ show n)
  println (show (List.map UnboxedArray.length [UnboxedArray.make 2 0.0, UnboxedArray.make 3 1.0]))
