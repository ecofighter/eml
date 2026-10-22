-- パスで1回に済むファイルの操作の標準モジュール。ハンドルを使う読みは `FileIn` にある。
-- `extern` の宣言は `Std.Fs.<名前>` の正式な名前で crates/eml_extern の表の行を指し、実装は処理系が持つ。
-- `IO` は Prelude の組み込みのエフェクトである。
import String
import FileIn

-- `path` のファイルを作るか切り詰めて、`text` を書く
pub extern write_text : String -> String -> <IO> Unit

-- ファイル全体を文字列として読む
pub read_text : String -> <IO> String
read_text path =
  let (text, f) = FileIn.read_all (FileIn.open path)
  FileIn.close f
  text
