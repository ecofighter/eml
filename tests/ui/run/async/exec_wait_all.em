-- clock: real (the test driver has no processes)
-- `Exec.wait_all` はすべての段を先頭から待ち、終了コードを先頭から並べる。`Exec.output` は、標準出力を
-- リダイレクトしたとき `Proc.output` と同じく何も読まず、空の文字列を返す。書いたファイルはテストの Host が持つ
-- (docs/spec/stdlib.md の「`Std.Exec`」)。段はこのファイルを引数で流した eml である
import Async (Async)
import Env
import Exec
import Proc ((.|))
import Proc
import Stdin
import Stream
import String

main : Unit -> <IO, Panic> Unit
main () =
  match Env.args () with
    | ["exit3"] ->
      println "one"
      exit 3
    | ["copy"] -> copy (Stdin.open ())
    | _ -> Async.run parent

copy : Stdin.Stdin -> <IO, Panic> Unit
copy h =
  match Stdin.read_line h with
    | (Some line, h) ->
      println "copied \{line}"
      copy h
    | (None, h) -> Stdin.close h

parent : Unit -> <IO, Panic, Async> Unit
parent () =
  let child = Exec.spawn (`eml run exec_wait_all.em exit3` .| `eml run exec_wait_all.em copy`)
  let (r, child) = Exec.stdout child
  let (out, r) = Stream.read_all r
  Stream.close r
  println (String.trim out)
  println "wait_all \{show (Exec.wait_all child)}"
  let (out, code) = Exec.output (Proc.stdout_file "out.txt" (Proc.stdin_text "two\n" `eml run exec_wait_all.em copy`))
  println "output \{show out} exit \{show code}"
