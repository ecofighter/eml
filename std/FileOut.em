-- ファイルを書く標準モジュール。`FileOut` は線形な型で、`close` で必ず閉じる (docs/spec/stdlib.md の「`FileOut`」)。
-- `extern` の宣言は `Std.FileOut.<名前>` の正式な名前で crates/eml_extern の表の行を指し、実装は処理系が持つ。
-- `IO` は Prelude の組み込みのエフェクトである。
import String
import Bytes

pub extern data FileOut
-- 作るか切り詰めて開く
pub extern create : String -> <IO> FileOut
-- 作るか、末尾に足す形で開く
pub extern append : String -> <IO> FileOut
-- 書き込みはデータを借りて、受け取った `FileOut` を返す。書くのは `FileOut` の buffer で、あふれたときと `flush` と
-- `close` のときに書き出す
pub extern write : String -> FileOut -> <IO> FileOut
pub extern write_bytes : Bytes -> FileOut -> <IO> FileOut
pub extern write_line : String -> FileOut -> <IO> FileOut
pub extern flush : FileOut -> <IO> FileOut
extern close_raw : FileOut -> <IO> Unit

-- 書き出してから閉じる。書き出せなければ `close_raw` に進まず、誤りを返す前にハンドルは閉じている
pub close : FileOut -> <IO> Unit
close h = close_raw (flush h)
