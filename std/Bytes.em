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
-- 一意なら写さずに同じブロックを使い、そうでなければ中身を写す
pub extern of_string : String -> Bytes
-- 探す列、開始位置、対象。開始位置以降で最初に見つかった位置で、なければ -1 である
extern find_from : Bytes -> Int -> Bytes -> <Panic> Int

pub find : Bytes -> Bytes -> <Panic> Option Int
find needle b =
  let i = find_from needle 0 b
  if i < 0 then None else Some i

-- `n` バイトの、どのバイトも `b` のバイト列を返す。`n` が負なら `Panic` である
pub extern make : Int -> Byte -> <Panic> Bytes
-- 容量が `n` バイト以上の空のバイト列を返す。`n` が負なら `Panic` である
pub extern with_capacity : Int -> <Panic> Bytes
-- 位置 `i` のバイト。範囲の外は `Panic` である
pub extern get : Int -> Bytes -> <Panic> Byte
-- 位置 `i` のバイトを `b` にしたバイト列を返す。一意なら、その場で書き換える
pub extern set : Int -> Byte -> Bytes -> <Panic> Bytes
-- 末尾に `b` を足す。一意なら、その場で伸ばす
pub extern push : Byte -> Bytes -> Bytes
-- 探すバイト、開始位置、対象。開始位置以降で最初に見つかった位置で、なければ -1 である
extern find_byte_from : Byte -> Int -> Bytes -> <Panic> Int

pub find_byte : Byte -> Bytes -> <Panic> Option Int
find_byte x b =
  let i = find_byte_from x 0 b
  if i < 0 then None else Some i

-- 最後の行の終わり (`\n` か `\r\n`) を1つ除く。なければそのまま返す。一意なら長さだけを書き換える (`truncate`)
pub trim_line_end : Bytes -> <Panic> Bytes
trim_line_end line =
  let n = length line
  if n == 0 || get (n - 1) line != '\n'u8 then line
  else if n >= 2 && get (n - 2) line == '\r'u8 then truncate (n - 2) line
  else truncate (n - 1) line
