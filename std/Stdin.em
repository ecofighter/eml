-- 標準入力を読む標準モジュール (docs/spec/stdlib.md)。どの関数も、読む前に標準出力を書き出す。
import String

pub extern read_all : Unit -> <IO> String
-- 行の終わりを含む1行で、終わりなら空の文字列である
extern read_line_raw : Unit -> <IO> String

-- 行の終わり (`\n` か `\r\n`) を除いた1行で、終わりなら `None` である
pub read_line : Unit -> <IO> Option String
read_line () =
  let line = read_line_raw ()
  if String.byte_length line == 0 then None else Some (String.trim_line_end line)
