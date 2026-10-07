-- ファイルを読む標準モジュール。`File` は線形な型で、`close` で必ず閉じる (docs/spec/linearity.md)。
-- `extern` の宣言は `Std.Fs.<名前>` の正式な名前で crates/eml_extern の表の行を指し、実装は処理系が持つ。
-- `IO` は Prelude の組み込みのエフェクトである。
pub extern data File
pub extern open : String -> <IO> File
pub extern read_all : File -> <IO> (File, String)
pub extern close : File -> <IO> Unit
