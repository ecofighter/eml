-- バイトの変換 (docs/spec/stdlib.md の「`Std.Byte`」)。`Byte` は 0 から 255 の値を持つ
pub extern to_int : Byte -> Int
-- 0 から 255 の外の `Int` は `Panic` である
pub extern of_int : Int -> <Panic> Byte
-- 下位8bit を残す。2の補数で見るので、`wrap (-1)` は `255u8` である
pub extern wrap : Int -> Byte
