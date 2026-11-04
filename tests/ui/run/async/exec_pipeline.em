-- clock: real (the test driver has no processes)
-- `Exec.capture` は `.|` でつないだ3段を1回で起動し、最後の段の標準出力を driver の pipe で読み切ってから、すべての段を
-- 待つ (docs/spec/stdlib.md の「`Std.Exec`」)。段はこのファイルを引数で流した eml である。先頭の段は pipe の buffer
-- より多い 256 KiB 余りを書き、後の2段は行に印を付けて写す。どの段も入力を終わりまで読むので、出力は段の速さによらない
import Async (Async)
import Env
import Exec
import List
import Proc ((.|))
import Stdin
import String

main : Unit -> <IO, Panic> Unit
main () =
  match Env.args () with
    | ["emit"] -> emit 0
    | ["copy", prefix] -> copy (Stdin.open ()) prefix
    | _ -> Async.run parent

-- 1行は60バイト余りで、4096 行で 256 KiB を超える
emit : Int -> <IO, Panic> Unit
emit n =
  if n == 4096 then ()
  else
    println "line \{show n} 0123456789abcdef0123456789abcdef0123456789abcdef0123456"
    emit (n + 1)

copy : Stdin.Stdin -> String -> <IO, Panic> Unit
copy h prefix =
  match Stdin.read_line h with
    | (Some line, h) ->
      println (prefix ++ line)
      copy h prefix
    | (None, h) -> Stdin.close h

parent : Unit -> <IO, Panic, Async> Unit
parent () =
  let out = Exec.capture (`eml run exec_pipeline.em emit` .| `eml run exec_pipeline.em copy a:` .| `eml run exec_pipeline.em copy b:`)
  let lines = String.lines out
  println "lines \{show (List.length lines)} bytes \{show (String.byte_length out)}"
  match lines with
    | first :: _ -> println first
    | Nil -> println "no lines"
  match List.nth 4095 lines with
    | Some last -> println last
    | None -> println "no last line"
