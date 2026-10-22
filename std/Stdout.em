-- 標準出力のハンドル (docs/spec/stdlib.md の「`Stdout` と `Stderr`」)。

pub extern data Stdout
pub extern open : Unit -> <IO, Panic> Stdout
pub extern close : Stdout -> <IO> Unit
pub extern write : Bytes -> Stdout -> <IO> Stdout
pub extern write_string : String -> Stdout -> <IO> Stdout
pub extern write_line : String -> Stdout -> <IO> Stdout
pub extern flush : Stdout -> <IO> Stdout
