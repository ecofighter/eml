-- 標準エラーのハンドル (docs/spec/stdlib.md の「`Stdout` と `Stderr`」)。書くたびに書き出すので `flush` を持たない。

pub extern data Stderr
pub extern open : Unit -> <IO, Panic> Stderr
pub extern close : Stderr -> <IO> Unit
pub extern write : String -> Stderr -> <IO> Stderr
pub extern write_bytes : Bytes -> Stderr -> <IO> Stderr
pub extern write_line : String -> Stderr -> <IO> Stderr
