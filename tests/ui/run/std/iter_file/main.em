-- `FileIn.read_line` で読む生産者を `Iter.generate` で包む (docs/spec/stdlib.md)。`Iter.take` が途中でやめると、残りの
-- 継続ごと `File` を閉じる。閉じなければ checked mode の漏れの検査で落ちる
import Iter
import Yield (Yield(..))
import FileIn

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
  Iter.each (fn line -> println "take \{line}") (Iter.take 2 (Iter.generate (lines "lines.txt")))
  match try_io (fn () -> Iter.each (fn line -> println "each \{line}") (Iter.generate (lines "lines.txt"))) with
    | Ok () -> println "done"
    | Err e -> println "error: \{e}"
  match try_io (fn () -> Iter.each (fn line -> println line) (Iter.generate (lines "missing.txt"))) with
    | Ok () -> println "done"
    | Err e -> println "error: \{e}"
