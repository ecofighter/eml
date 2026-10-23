-- `Child` を持ったまま `try_io` で巻き戻ると、破棄処理が pipe を閉じ、子を kill して待つ (docs/spec/stdlib.md の
-- 「`Proc`」)。kill しなければ `sleep` を30秒待つ。検査モードでは、手放していなければ漏れの検査で落ちる
import Proc
import Fs

main : Unit -> <IO, Panic> Unit
main () =
  let r = try_io (fn () ->
    let c = Proc.write_line "hello" (Proc.spawn `sleep 30`)
    let text = Fs.read_text "missing.txt"
    let code = Proc.wait c
    "\{text} \{code}")
  match r with
    | Ok s -> println s
    | Err e -> println "\{show e.kind}: \{e}"
  println (show (Proc.capture `echo after`))
