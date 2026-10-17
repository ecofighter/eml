-- `Fs.read_line` で読む生産者を `Iter.generate` で包む (docs/spec/stdlib.md)。`Iter.take` が途中でやめると、残りの
-- 継続ごと `File` を閉じる。閉じなければ checked mode の漏れの検査で落ちる
import Iter
import Yield (Yield(..))

lines : String -> Unit -> <Yield String, IO> Unit
lines path () =
  let go file =
    match Fs.read_line file with
      | (file, None) -> Fs.close file
      | (file, Some line) ->
        yield line
        go file
  go (Fs.open path)

main : Unit -> <IO> Unit
main () =
  Iter.each (fn line -> println "take \{line}") (Iter.take 2 (Iter.generate (lines "lines.txt")))
  match try_io (fn () -> Iter.each (fn line -> println "each \{line}") (Iter.generate (lines "lines.txt"))) with
    | Ok () -> println "done"
    | Err e -> println "error: \{e}"
  match try_io (fn () -> Iter.each (fn line -> println line) (Iter.generate (lines "missing.txt"))) with
    | Ok () -> println "done"
    | Err e -> println "error: \{e}"
