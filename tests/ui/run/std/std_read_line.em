-- `Fs.read_line` は行の終わり (`\n` か `\r\n`) を除いた1行を返し、終わりなら `None` を返す (docs/spec/stdlib.md)。
-- 入力はテストの Host がメモリに持つファイルで、`Fs.write_text` で書いてから読む
import Fs
import List
import String

print_lines : Fs.File -> <IO, Panic> Unit
print_lines file =
  match Fs.read_line file with
    | (None, file) ->
      println "end"
      Fs.close file
    | (Some line, file) ->
      println (show line)
      print_lines file

main : Unit -> <IO, Panic> Unit
main () =
  Fs.write_text "lines.txt" "first\nsecond\r\n\n\r\nlast"
  print_lines (Fs.open "lines.txt")
  Fs.write_text "empty.txt" ""
  print_lines (Fs.open "empty.txt")
  -- 全部読んだ後は終わりにいる
  let (all, f) = Fs.read_all (Fs.open "lines.txt")
  let (line, f) = Fs.read_line f
  Fs.close f
  println (show (String.byte_length all))
  println (show line)
  println (show (List.map String.trim_line_end ["a\n", "a\r\n", "a\n\n", "a\r", "a", "\n", ""]))
