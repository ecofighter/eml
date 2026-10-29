-- 入力を読み終えてから出力を書く子には、pipe の buffer (ふつう 64 KiB) より多く書いてよい (docs/spec/stdlib.md の
-- 「`Proc`」)。子に 256 KiB を書き、`close_stdin` してから読む。子はこのファイルを `child` の引数で流した eml で、
-- 標準入力の行を並べ替えて書く
import Env
import Proc
import Stdin
import String
import List

main : Unit -> <IO, Panic> Unit
main () =
  match Env.args () with
    | ["child"] -> println (String.join "\n" (List.sort (String.lines (Stdin.read_text ()))))
    | _ -> parent ()

-- 1行は16バイトで、16384 行で 256 KiB になる
feed : Int -> Proc.Child -> <IO, Panic> Proc.Child
feed n c = if n == 0 then c else feed (n - 1) (Proc.write_line "\{100000 + n}-padding!" c)

parent : Unit -> <IO, Panic> Unit
parent () =
  let c = Proc.close_stdin (feed 16384 (Proc.spawn `eml run proc_spawn_large.em child`))
  let (out, c) = Proc.read_all c
  let lines = String.lines out
  println "bytes \{String.byte_length out}"
  println "lines \{List.length lines}"
  println "first \{show (List.nth 0 lines)}"
  println "sort \{Proc.wait c}"
