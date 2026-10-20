-- 意味の上で書き換わらない配列 (docs/spec/stdlib.md の「`Std.Array`」)。要素の型変数は `Unr` に固定するので、
-- `Lin` の要素の配列にはどの関数も使えない。`set` は配列が一意ならその場で書き換え、共有されていれば写す。
-- 範囲外の添字と負の長さは `Panic` である。
import List
import MutArray

pub extern make : Int -> a -> <Panic> Array a
pub extern length : Array a -> Int
pub extern get : Array a -> Int -> <Panic> a
pub extern set : Array a -> Int -> a -> <Panic> Array a
-- 一意ならそのまま、共有されていれば写して `MutArray` にする
pub extern thaw : Array a -> MutArray a

-- `f` を添字の順に呼ぶ
pub tabulate : Int -> (Int -> <e> a) -> <Panic | e> Array a
tabulate n f = MutArray.freeze (MutArray.tabulate n f)

-- 添字はどれも範囲の中なので `Panic` は起こらない。型から `Panic` を除くために `try_panic` で包み、届かない `Err`
-- には空の配列を返す
pub from_list : List a -> Array a
from_list xs =
  let fill i ys m =
    match ys with
      | Nil -> m
      | y :: rest -> fill (i + 1) rest (MutArray.write m i y)
  let build () =
    match xs with
      | Nil -> MutArray.freeze (MutArray.empty ())
      | x :: rest -> MutArray.freeze (fill 1 rest (MutArray.make (List.length xs) x))
  match try_panic build with
    | Ok a -> a
    | Err _ -> MutArray.freeze (MutArray.empty ())

-- `from_list` と同じく、届かない `Panic` を `try_panic` で除く
pub to_list : Array a -> List a
to_list a =
  let collect i acc =
    if i < 0 then acc else collect (i - 1) (get a i :: acc)
  match try_panic (fn () -> collect (length a - 1) Nil) with
    | Ok xs -> xs
    | Err _ -> Nil
