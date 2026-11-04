-- clock: real (the test driver has no processes)
-- `Exec.wait` で待つタスクを取り消すと、driver の待ちを取り消して `Cancelled` を投げ、巻き戻しが落とす子は段を kill
-- してから待つ。取り出した標準入力の書き手も一緒に落ちる (docs/spec/stdlib.md の「`Std.Exec`」)。子はこのファイルを
-- 引数で流した eml で、標準入力を終わりまで読むだけで何も書かないので、kill より先に EOF を受けても出力は変わらない。
-- 取り消しが待ちの前に届いても後に届いても、子は同じく片づき、run は終わる
import Async (Async)
import Chan
import Env
import Exec
import Stdin
import Stream

main : Unit -> <IO, Panic> Unit
main () =
  match Env.args () with
    | ["hold"] ->
      let _ = Stdin.read_text ()
      ()
    | _ ->
      Async.run parent
      println "the run ended"

-- 子の標準入力を開いたまま、子を待つ。待ちの前に `ready` に知らせる
holder : Chan.Chan Unit -> Unit -> <IO, Panic, Async> Unit
holder ready () =
  let child = Exec.spawn `eml run exec_child_released_on_abort.em hold`
  let (w, child) = Exec.stdin child
  match Chan.send () ready with
    | Chan.Sent -> ()
    | Chan.Rejected _ -> println "unreachable"
  let code = Exec.wait child
  Stream.close_writer w
  println "unreachable \{show code}"

parent : Unit -> <IO, Panic, Async> Unit
parent () =
  let ready = Chan.new 1
  let t = Async.start (holder ready)
  match Chan.recv ready with
    | Some () -> println "the child is running"
    | None -> println "unreachable"
  Async.cancel t
  println "cancelled the holder"
