-- `File` を読む生産者を `Iter.take 2` と `Iter.fold` で引き、残りを捨てる。融合した後も、`take` が残りを捨てるときに
-- `File` はちょうど1回閉じる。閉じなければ checked mode の漏れの検査で落ちる
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
  Fs.write_text "lines.txt" "one\ntwo\nthree\nfour\n"
  println (Iter.fold (fn acc line -> acc ++ line ++ ";") "" (Iter.take 2 (Iter.generate (lines "lines.txt"))))
  println (show (Iter.fold (fn n _ -> n + 1) 0 (Iter.generate (lines "lines.txt"))))
