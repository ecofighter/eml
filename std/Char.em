-- 文字の変換 (docs/spec/stdlib.md の「`Std.Char`」)。`Char` は Unicode のスカラー値を持つ
pub extern to_int : Char -> Int
-- スカラー値でない `Int` (負の数、サロゲート、0x10FFFF を超える値) は `Panic` である
pub extern of_int : Int -> <Panic> Char

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
