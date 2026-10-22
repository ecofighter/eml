-- パスで1回に済むファイルの操作の標準モジュール (docs/spec/stdlib.md の「`Fs`」)。
-- ハンドルを使う読みは `FileIn` に、書きは `FileOut` にある。
-- extern を持たず、どの関数も `FileIn` と `FileOut` の上に eml で書く。
import FileIn
import FileOut

-- ファイル全体を文字列として読む
pub read_text : String -> <IO> String
read_text path =
  let (text, f) = FileIn.read_all (FileIn.open path)
  FileIn.close f
  text

-- `path` のファイルを作るか切り詰めて、`text` を書く
pub write_text : String -> String -> <IO> Unit
write_text path text = FileOut.close (FileOut.write text (FileOut.create path))
