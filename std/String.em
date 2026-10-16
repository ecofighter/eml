-- 文字列の関数 (docs/spec/stdlib.md)。位置は UTF-8 のバイトの位置である。extern は核の4つ (`byte_length`、
-- `byte_at`、`slice`、`find_from`) だけで、ほかは eml で書く。
import List

pub extern byte_length : String -> Int
pub extern byte_at : Int -> String -> Int
pub extern slice : Int -> Int -> String -> String
-- 探す文字列、開始位置、対象。開始位置以降で最初に見つかった位置で、なければ -1 である
extern find_from : String -> Int -> String -> Int

pub find : String -> String -> Option Int
find needle s =
  let i = find_from needle 0 s
  if i < 0 then None else Some i

pub contains : String -> String -> Bool
contains needle s = find_from needle 0 s >= 0

pub starts_with : String -> String -> Bool
starts_with prefix s = matches_at prefix s 0

pub ends_with : String -> String -> Bool
ends_with suffix s = matches_at suffix s (byte_length s - byte_length suffix)

-- `needle` が `s` の位置 `at` から始まるか。`slice` でなくバイトで比べるので、`at` が文字の境界でなくても誤りに
-- ならない
matches_at : String -> String -> Int -> Bool
matches_at needle s at =
  let n = byte_length needle
  let same_bytes i =
    if i >= n then True
    else if byte_at i needle == byte_at (at + i) s then same_bytes (i + 1)
    else False
  at >= 0 && at + n <= byte_length s && same_bytes 0

-- 空の `sep` では区切らない。見つかる位置は空でない UTF-8 の列の始まりなので、文字の境界である
pub split : String -> String -> List String
split sep s =
  let split_from at acc =
    let i = find_from sep at s
    if i < 0 then List.reverse (slice at (byte_length s) s :: acc)
    else split_from (i + byte_length sep) (slice at i s :: acc)
  if byte_length sep == 0 then [s] else split_from 0 Nil

-- 行の終わりは `\n` か `\r\n` で、最後の行の終わりは省ける (Rust の `str::lines` と同じ)
pub lines : String -> List String
lines s =
  let lines_from at acc =
    let len = byte_length s
    if at >= len then List.reverse acc
    else
      let i = find_from "\n" at s
      if i < 0 then List.reverse (slice at len s :: acc)
      else lines_from (i + 1) (without_cr (slice at i s) :: acc)
  lines_from 0 Nil

-- `\r\n` で終わった行の `\r` を除く
without_cr : String -> String
without_cr line =
  let n = byte_length line
  if n > 0 && byte_at (n - 1) line == 13 then slice 0 (n - 1) line else line

pub join : String -> List String -> String
join sep xs =
  -- 左を一意な文字列として伸ばすので、長さに比例する時間で済む (docs/spec/object-model.md の「文字列の操作」)
  let join_onto acc ys =
    match ys with
      | Nil -> acc
      | x :: rest -> join_onto ((acc ++ sep) ++ x) rest
  match xs with
    | Nil -> ""
    | x :: rest -> join_onto x rest

-- 両端の ASCII の空白 (空白、`\t`、`\n`、`\x0B`、`\x0C`、`\r`) を除く
pub trim : String -> String
trim s =
  let len = byte_length s
  let skip_space i = if i < len && is_space (byte_at i s) then skip_space (i + 1) else i
  let start = skip_space 0
  let trim_back stop =
    if stop > start && is_space (byte_at (stop - 1) s) then trim_back (stop - 1) else stop
  slice start (trim_back len) s

is_space : Int -> Bool
is_space b = b == 32 || (b >= 9 && b <= 13)

-- 省ける符号 `+` か `-` と、1つ以上の ASCII の数字 (Rust の `i64::from_str` と同じ)。範囲を超えれば `None` である
pub to_int : String -> Option Int
to_int s =
  let len = byte_length s
  let sign = if len > 0 then byte_at 0 s else 0
  let start = if sign == 43 || sign == 45 then 1 else 0
  -- 値を負の側で積む。`-9223372036854775808` の絶対値は `Int` に収まらないためである。掛ける前に、結果が下限より
  -- 小さくならないことを確かめる
  let accumulate i negative acc =
    if i >= len then
      if negative then Some acc else if acc == min_int then None else Some (0 - acc)
    else
      let d = byte_at i s - 48
      if d < 0 || d > 9 then None
      else if acc < (min_int + d) / 10 then None
      else accumulate (i + 1) negative (acc * 10 - d)
  if start >= len then None else accumulate start (sign == 45) 0

min_int : Int
min_int = 0 - 9223372036854775807 - 1
