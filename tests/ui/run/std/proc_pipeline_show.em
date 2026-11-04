-- パイプラインの `show` は前の段を `StdinFrom` に入れ子にし、`display` は段を先頭から ` | ` でつなぐ。入力の設定は
-- 先頭の段に、ほかの設定は最後の段に当たる (docs/spec/stdlib.md の「`Cmd`」)。子は起動しない
import Proc ((.|))

main : Unit -> <IO> Unit
main () =
  let p = `sort -r` .| `uniq -c` |> Proc.discard_stderr
  println (show p)
  println (display (`cat a.txt` .| p))
  let q = Proc.stdin_text "x" (`a` .| `b`)
  println (show q)
  println (show (q == (Proc.stdin_text "x" `a` .| `b`)))
  println (show ((`x` .| (`a` .| `b`)) == (`x` .| `a` .| `b`)))
