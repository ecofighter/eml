-- ファイルを読む標準モジュール。`File` は線形な型で、`close` で必ず閉じる (docs/spec/linearity.md)。
-- `extern` の宣言は `Std.Fs.<名前>` の正式な名前で crates/eml_extern の表の行を指し、実装は処理系が持つ。
-- `IO` は Prelude の組み込みのエフェクトである。
pub extern data File
pub extern open : String -> <IO> File
pub extern read_all : File -> <IO> (File, String)
pub extern close : File -> <IO> Unit
-- `path` のファイルを作るか切り詰めて、`text` を書く
pub extern write_text : String -> String -> <IO> Unit

-- ファイル全体を文字列として読む
pub read_text : String -> <IO> String
read_text path =
  let f = open path
  let (f, text) = read_all f
  close f
  text
