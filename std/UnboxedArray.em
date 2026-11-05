-- 要素を生の 8 バイトの語で並べ、意味の上で書き換わらない配列 (docs/spec/stdlib.md の「`Std.UnboxedArray`」)。
-- instance は `Float` と `Int` だけである。`set` は配列が一意ならその場で書き換え、共有されていれば写す。範囲外の
-- 添字と負の長さは `Panic` である。
import Array
import List
import MutUnboxedArray (MutUnboxed)

pub class MutUnboxed a => Unboxed a where
  make : Int -> a -> <Panic> UnboxedArray a
  get : UnboxedArray a -> Int -> <Panic> a
  set : UnboxedArray a -> Int -> a -> <Panic> UnboxedArray a

instance Unboxed Float where
  extern make
  extern get
  extern set

instance Unboxed Int where
  extern make
  extern get
  extern set

pub extern length : UnboxedArray a -> Int
-- 一意ならそのまま、共有されていれば写して `MutUnboxedArray` にする
pub extern thaw : UnboxedArray a -> MutUnboxedArray a

-- `f` を添字の順に呼ぶ
pub tabulate : Unboxed a => Int -> (Int -> <e> a) -> <Panic | e> UnboxedArray a
tabulate n f = MutUnboxedArray.freeze (MutUnboxedArray.tabulate n f)

-- 添字はどれも範囲の中なので `Panic` は起こらない。型から `Panic` を除くために `try_panic` で包み、届かない `Err`
-- には空の配列を返す
pub from_list : Unboxed a => List a -> UnboxedArray a
from_list xs =
  let go i ys m =
    match ys with
      | Nil -> m
      | y :: rest -> go (i + 1) rest (MutUnboxedArray.write m i y)
  let build () =
    match xs with
      | Nil -> MutUnboxedArray.freeze (MutUnboxedArray.empty ())
      | x :: rest -> MutUnboxedArray.freeze (go 1 rest (MutUnboxedArray.make (List.length xs) x))
  match try_panic build with
    | Ok a -> a
    | Err _ -> MutUnboxedArray.freeze (MutUnboxedArray.empty ())

-- `from_list` と同じく、届かない `Panic` を `try_panic` で除く
pub to_list : Unboxed a => UnboxedArray a -> List a
to_list a =
  let collect i acc =
    if i < 0 then acc else collect (i - 1) (get a i :: acc)
  match try_panic (fn () -> collect (length a - 1) Nil) with
    | Ok xs -> xs
    | Err _ -> Nil

-- `from_list` と同じく、届かない `Panic` を `try_panic` で除く
pub of_array : Unboxed a => Array a -> UnboxedArray a
of_array a =
  match try_panic (fn () -> tabulate (Array.length a) (fn i -> Array.get a i)) with
    | Ok u -> u
    | Err _ -> from_list Nil

-- `from_list` と同じく、届かない `Panic` を `try_panic` で除く
pub to_array : Unboxed a => UnboxedArray a -> Array a
to_array u =
  match try_panic (fn () -> Array.tabulate (length u) (fn i -> get u i)) with
    | Ok a -> a
    | Err _ -> Array.from_list Nil
