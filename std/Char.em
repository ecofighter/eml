-- 文字の変換 (docs/spec/stdlib.md の「`Std.Char`」)。`Char` は Unicode のスカラー値を持つ
pub extern to_int : Char -> Int
-- スカラー値でない `Int` (負の数、サロゲート、0x10FFFF を超える値) は `Panic` である
pub extern of_int : Int -> <Panic> Char
