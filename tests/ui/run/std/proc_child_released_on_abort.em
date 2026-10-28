-- `Child` を持ったまま `try_io` で巻き戻ると、破棄処理が pipe を閉じ、子を kill して待つ (docs/spec/stdlib.md の
-- 「`Proc`」)。子はこのファイルを `child` の引数で流した eml で、30秒眠る。kill しなければ30秒待つ。検査モードでは、
-- 手放していなければ漏れの検査で落ちる
import Env
import Proc
import Fs
import Async

main : Unit -> <IO, Panic> Unit
main () =
  match Env.args () with
    | ["child", "sleep"] -> Async.run (fn () -> Async.sleep_ms 30000)
    | ["child", word] -> println word
    | _ -> parent ()

parent : Unit -> <IO, Panic> Unit
parent () =
  let r = try_io (fn () ->
    let c = Proc.write_line "hello" (Proc.spawn `eml run proc_child_released_on_abort.em child sleep`)
    let text = Fs.read_text "missing.txt"
    let code = Proc.wait c
    "\{text} \{code}")
  match r with
    | Ok s -> println s
    | Err e -> println "\{show e.kind}: \{e}"
  println (show (Proc.capture `eml run proc_child_released_on_abort.em child after`))
