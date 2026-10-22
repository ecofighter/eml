-- バイト列の関数 (docs/spec/stdlib.md の「`Bytes`」)。位置はバイトの位置で、範囲の外は `Panic` である
pub extern empty : Unit -> Bytes
pub extern length : Bytes -> Int
-- `a` の後ろに `b` を足す。`a` が一意なら、その場で伸ばす
pub extern append : Bytes -> Bytes -> Bytes
-- `from to src dst`。`dst` の後ろに `src` の位置 `from` から `to` の手前までを足す。`dst` が一意なら、その場で伸ばす。
-- `src` と `dst` が同じ変数でもよい
pub extern append_slice : Int -> Int -> Bytes -> Bytes -> <Panic> Bytes
-- 位置 `from` から `to` の手前までを写した、新しいバイト列を返す
pub extern slice : Int -> Int -> Bytes -> <Panic> Bytes
-- 前の `n` バイトを返す。`0 <= n <= length b` で、一意なら長さだけを書き換える
pub extern truncate : Int -> Bytes -> <Panic> Bytes
-- 中身を写す
pub extern of_string : String -> Bytes
-- 探す列、開始位置、対象。開始位置以降で最初に見つかった位置で、なければ -1 である
extern find_from : Bytes -> Int -> Bytes -> <Panic> Int

pub find : Bytes -> Bytes -> <Panic> Option Int
find needle b =
  let i = find_from needle 0 b
  if i < 0 then None else Some i
