-- clock: real (the test driver has no processes)
-- 読まずに終わった子の標準入力への書きは、pipe の読む端がもうないので `broken pipe` の失敗になる。種類はソケットの
-- 切断と同じ `ConnectionReset` で、捕まえなければ run の失敗である (docs/spec/stdlib.md の「`Std.Exec`」)。子は
-- このファイルを引数で流した eml で、すぐに終わる
import Async (Async)
import Env
import Exec
import Stream

main : Unit -> <IO, Panic> Unit
main () =
  match Env.args () with
    | ["quit"] -> ()
    | _ -> Async.run parent

parent : Unit -> <IO, Panic, Async> Unit
parent () =
  let child = Exec.spawn `eml run exec_write_after_exit.em quit`
  let (w, child) = Exec.stdin child
  println "exit \{show (Exec.wait child)}"
  let w = Stream.write "too late\n" w
  Stream.close_writer w
