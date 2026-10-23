-- `FileIn` を持つ生産者を、特殊化した `Iter.generate` で引いて途中で捨てる。継続の閉包が `FileIn` を捕獲するので、
-- `Iter.take` が残りを捨てると `FileIn` も閉じる。閉じなければ checked mode の漏れの検査で落ちる
import Iter
import Yield (Yield(..))
import FileIn
import Fs

lines : String -> Unit -> <Yield String, IO, Panic> Unit
lines path () =
  let go file =
    match FileIn.read_line file with
      | (None, file) -> FileIn.close file
      | (Some line, file) ->
        yield line
        go file
  go (FileIn.open path)

main : Unit -> <IO, Panic> Unit
main () =
  Fs.write_text "lines.txt" "one\ntwo\nthree\nfour\n"
  Iter.each (fn line -> println line) (Iter.take 2 (Iter.generate (lines "lines.txt")))
