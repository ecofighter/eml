-- ファイルを読む標準モジュール。`File` は線形な型で、`close` で必ず閉じる (docs/spec/linearity.md)。
-- `extern` の宣言は `Std.Fs.<名前>` の正式な名前で crates/eml_extern の表の行を指し、実装は処理系が持つ。
-- `IO` は Prelude の組み込みのエフェクトである。
import String

pub extern data File
pub extern open : String -> <IO> File
-- 読みは `File` を借りて、結果を先に、`File` を後に返す (docs/spec/effects.md の「組み込みの `IO`」)
pub extern read_all : File -> <IO> (String, File)
-- 行の終わりを含む1行で、終わりなら空の文字列である。extern は `Option` を作らないので、`read_line` が包む
-- (docs/spec/stdlib.md の「extern の境界」)
extern read_line_raw : File -> <IO> (String, File)
pub extern close : File -> <IO> Unit
-- `path` のファイルを作るか切り詰めて、`text` を書く
pub extern write_text : String -> String -> <IO> Unit

-- ファイル全体を文字列として読む
pub read_text : String -> <IO> String
read_text path =
  let (text, f) = read_all (open path)
  close f
  text

-- 行の終わり (`\n` か `\r\n`) を除いた1行で、終わりなら `None` である。`Stdin.read_line` と同じである
pub read_line : File -> <IO, Panic> (Option String, File)
read_line file =
  let (line, file) = read_line_raw file
  if String.byte_length line == 0 then (None, file) else (Some (String.trim_line_end line), file)
