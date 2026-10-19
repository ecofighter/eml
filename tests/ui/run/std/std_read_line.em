-- `Fs.read_line` は行の終わり (`\n` か `\r\n`) を除いた1行を返し、終わりなら `None` を返す (docs/spec/stdlib.md)。
-- 入力はテストの Host がメモリに持つファイルで、`Fs.write_text` で書いてから読む
print_lines : Fs.File -> <IO, Panic> Unit
print_lines file =
  match Fs.read_line file with
    | (file, None) ->
      println "end"
      Fs.close file
    | (file, Some line) ->
      println (show line)
      print_lines file

main : Unit -> <IO, Panic> Unit
main () =
  Fs.write_text "lines.txt" "first\nsecond\r\n\n\r\nlast"
  print_lines (Fs.open "lines.txt")
  Fs.write_text "empty.txt" ""
  print_lines (Fs.open "empty.txt")
  -- 全部読んだ後は終わりにいる
  let (f, all) = Fs.read_all (Fs.open "lines.txt")
  let (f, line) = Fs.read_line f
  Fs.close f
  println (show (String.byte_length all))
  println (show line)
  println (show (List.map String.trim_line_end ["a\n", "a\r\n", "a\n\n", "a\r", "a", "\n", ""]))
