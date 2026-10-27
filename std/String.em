-- 文字列の関数 (docs/spec/stdlib.md)。位置は UTF-8 のバイトの位置である。extern は `byte_length`、`byte_at`、
-- `char_at`、`slice`、`push`、`find_from`、`decode` だけで、ほかは eml で書く。
import List
import Char
import Iter (Iter)
import Yield (Yield(..))

pub extern byte_length : String -> Int
pub extern byte_at : Int -> String -> <Panic> Byte
-- 位置から始まる文字を読む。次の文字の位置は `next` で求める
pub extern char_at : Int -> String -> <Panic> Char
pub extern slice : Int -> Int -> String -> <Panic> String
-- `s` を消費する。一意ならその場で伸ばす
pub extern push : Char -> String -> String
-- 探す文字列、開始位置、対象。開始位置以降で最初に見つかった位置で、なければ -1 である
extern find_from : String -> Int -> String -> <Panic> Int

-- `Bytes` を消費する。一意なら写さずに同じブロックを使い、そうでなければ中身を写す。妥当でない UTF-8 は `Panic` になる
pub extern decode : Bytes -> <Panic> String

-- 妥当な UTF-8 なら `Some` で `String` にし、そうでなければ `None` である (docs/spec/stdlib.md の「`Bytes` との変換」)
pub from_utf8 : Bytes -> Option String
from_utf8 b =
  match try_panic (fn () -> decode b) with
    | Ok s -> Some s
    | Err _ -> None

-- 位置 `i` の文字の次の位置。`i` が文字の境界でなければ `Panic` である
pub next : Int -> String -> <Panic> Int
next i s = i + Char.utf8_length (char_at i s)

pub of_char : Char -> String
of_char c = display c

pub of_chars : List Char -> String
of_chars cs = List.fold (fn acc c -> push c acc) "" cs

-- 先頭から順に `f` を呼んでたたむ。文字ごとに `char_at` で読み、幅だけ進む
pub fold : (b -> Char -> <e> b) -> b -> String -> <Panic | e> b
fold f init s =
  let len = byte_length s
  let go acc i =
    if i >= len then acc
    else
      let c = char_at i s
      go (f acc c) (i + Char.utf8_length c)
  go init 0

pub each : (Char -> <e> Unit) -> String -> <Panic | e> Unit
each f s = fold (fn _ c -> f c) () s

pub to_chars : String -> <Panic> List Char
to_chars s = List.reverse (fold (fn acc c -> c :: acc) Nil s)

pub chars : String -> Iter <Panic | e> Char
chars s = Iter.generate (fn () -> each yield s)

pub find : String -> String -> <Panic> Option Int
find needle s =
  let i = find_from needle 0 s
  if i < 0 then None else Some i

pub contains : String -> String -> <Panic> Bool
contains needle s = find_from needle 0 s >= 0

pub starts_with : String -> String -> <Panic> Bool
starts_with prefix s = matches_at prefix s 0

pub ends_with : String -> String -> <Panic> Bool
ends_with suffix s = matches_at suffix s (byte_length s - byte_length suffix)

-- `needle` が `s` の位置 `at` から始まるか。`slice` でなくバイトで比べるので、`at` が文字の境界でなくても誤りに
-- ならない
matches_at : String -> String -> Int -> <Panic> Bool
matches_at needle s at =
  let n = byte_length needle
  let same_bytes i =
    if i >= n then True
    else if byte_at i needle == byte_at (at + i) s then same_bytes (i + 1)
    else False
  at >= 0 && at + n <= byte_length s && same_bytes 0

-- 空の `sep` では区切らない。見つかる位置は空でない UTF-8 の列の始まりなので、文字の境界である
pub split : String -> String -> <Panic> List String
split sep s =
  let split_from at acc =
    let i = find_from sep at s
    if i < 0 then List.reverse (slice at (byte_length s) s :: acc)
    else split_from (i + byte_length sep) (slice at i s :: acc)
  if byte_length sep == 0 then [s] else split_from 0 Nil

-- 行の終わりは `\n` か `\r\n` で、最後の行の終わりは省ける (Rust の `str::lines` と同じ)
pub lines : String -> <Panic> List String
lines s =
  let lines_from at acc =
    let len = byte_length s
    if at >= len then List.reverse acc
    else
      let i = find_from "\n" at s
      if i < 0 then List.reverse (slice at len s :: acc)
      else lines_from (i + 1) (without_cr (slice at i s) :: acc)
  lines_from 0 Nil

-- 最後の行の終わり (`\n` か `\r\n`) を1つ除く。なければそのまま返す
pub trim_line_end : String -> <Panic> String
trim_line_end line =
  let n = byte_length line
  if n == 0 || byte_at (n - 1) line != '\n'u8 then line
  else if n >= 2 && byte_at (n - 2) line == '\r'u8 then slice 0 (n - 2) line
  else slice 0 (n - 1) line

-- `\r\n` で終わった行の `\r` を除く
without_cr : String -> <Panic> String
without_cr line =
  let n = byte_length line
  if n > 0 && byte_at (n - 1) line == '\r'u8 then slice 0 (n - 1) line else line

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
pub trim : String -> <Panic> String
trim s =
  let len = byte_length s
  let skip_space i = if i < len && is_space_byte (byte_at i s) then skip_space (i + 1) else i
  let start = skip_space 0
  let trim_back stop =
    if stop > start && is_space_byte (byte_at (stop - 1) s) then trim_back (stop - 1) else stop
  slice start (trim_back len) s

-- `Char.is_ascii_space` と同じ範囲である。バイトで見るので、ここに別に持つ
is_space_byte : Byte -> Bool
is_space_byte b = b == ' 'u8 || (b >= '\t'u8 && b <= '\r'u8)

-- 省ける符号 `+` か `-` と、1つ以上の ASCII の数字 (Rust の `i64::from_str` と同じ)。範囲を超えれば `None` である
pub to_int : String -> <Panic> Option Int
to_int s =
  let len = byte_length s
  let negative = len > 0 && byte_at 0 s == '-'u8
  let start = if negative || (len > 0 && byte_at 0 s == '+'u8) then 1 else 0
  -- `Int` の下限。字句は `-9223372036854775808` を範囲外の誤りにするので、1を引いて作る
  let min_int = -9223372036854775807 - 1
  -- 値を負の側で積む。`-9223372036854775808` の絶対値は `Int` に収まらないためである。掛ける前に、結果が下限より
  -- 小さくならないことを確かめる
  let accumulate i negative acc =
    if i >= len then
      if negative then Some acc else if acc == min_int then None else Some (0 - acc)
    else
      let d = Char.to_int (char_at i s) - 48
      if d < 0 || d > 9 then None
      else if acc < (min_int + d) / 10 then None
      else accumulate (i + 1) negative (acc * 10 - d)
  if start >= len then None else accumulate start negative 0
