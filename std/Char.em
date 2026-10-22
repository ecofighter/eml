-- 文字の変換 (docs/spec/stdlib.md の「`Std.Char`」)。`Char` は Unicode のスカラー値を持つ
import Byte

pub extern to_int : Char -> Int
-- スカラー値でない `Int` (負の数、サロゲート、0x10FFFF を超える値) は `Panic` である
pub extern of_int : Int -> <Panic> Char
-- Latin-1 の読みである。UTF-8 のバイトはデコードしない (docs/spec/stdlib.md の「`Std.Char`」)
pub extern of_byte : Byte -> Char

-- 255 を超える文字は `Panic` である
pub to_byte : Char -> <Panic> Byte
to_byte c =
  if c <= '\u{ff}' then Byte.wrap (to_int c) else panic "char \{show c} does not fit in `Byte`"

-- UTF-8 でのバイト数である
pub utf8_length : Char -> Int
utf8_length c =
  if c < '\u{80}' then 1 else if c < '\u{800}' then 2 else if c < '\u{10000}' then 3 else 4

-- ASCII の判定。ASCII 以外の文字はどれも偽である (docs/spec/stdlib.md の「`Std.Char`」)
pub is_ascii : Char -> Bool
is_ascii c = c <= '\u{7f}'

pub is_ascii_digit : Char -> Bool
is_ascii_digit c = c >= '0' && c <= '9'

pub is_ascii_upper : Char -> Bool
is_ascii_upper c = c >= 'A' && c <= 'Z'

pub is_ascii_lower : Char -> Bool
is_ascii_lower c = c >= 'a' && c <= 'z'

pub is_ascii_alpha : Char -> Bool
is_ascii_alpha c = is_ascii_upper c || is_ascii_lower c

pub is_ascii_alphanumeric : Char -> Bool
is_ascii_alphanumeric c = is_ascii_alpha c || is_ascii_digit c

-- `Std.String` の空白の判定と同じ範囲である
pub is_ascii_space : Char -> Bool
is_ascii_space c = c == ' ' || (c >= '\t' && c <= '\r')

-- 大文字と小文字の差は ASCII で 32 (1 ビット) である。ASCII 以外はそのまま返す
pub to_ascii_upper : Char -> Char
to_ascii_upper c = if is_ascii_lower c then of_byte (Byte.wrap (to_int c ^^^ 32)) else c

pub to_ascii_lower : Char -> Char
to_ascii_lower c = if is_ascii_upper c then of_byte (Byte.wrap (to_int c ^^^ 32)) else c
