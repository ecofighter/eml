-- ファイルの書き込みと読み (docs/spec/stdlib.md)。テストの Host は書いたファイルをメモリに持ち、スナップショットに
-- 書き出す
main : Unit -> <IO, Panic> Unit
main () =
  Fs.write_text "report.txt" "line 1\nline 2\n"
  println (Fs.read_text "report.txt")
  Fs.write_text "report.txt" "again\n"
  println (show (String.lines (Fs.read_text "report.txt")))
