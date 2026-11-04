-- clock: real (the test driver has no processes)
-- `Exec.pipe_stderr` の子の標準出力と標準エラーを、2つのタスクで読み分ける (docs/spec/stdlib.md の「`Std.Exec`」)。
-- 子はこのファイルを引数で流した eml で、標準出力と標準エラーにそれぞれ pipe の buffer より多い 128 KiB 余りを
-- 交互に書く。1つのタスクが片方を読み切ってからもう片方を読むと、子は読まれない側の書き込みで止まり、読みも終わらない。
-- 結果は両方のタスクが終わってから決まった順に出すので、出力は読みの速さによらない
import Async (Async)
import Async
import Env
import Exec
import List
import Stream
import String

main : Unit -> <IO, Panic> Unit
main () =
  match Env.args () with
    | ["talk"] -> talk 0
    | _ -> Async.run parent

-- 1行は60バイト余りで、2048 行で 128 KiB を超える
talk : Int -> <IO, Panic> Unit
talk n =
  if n == 2048 then ()
  else
    println "out \{show n} 0123456789abcdef0123456789abcdef0123456789abcdef0123456"
    eprintln "err \{show n} 0123456789abcdef0123456789abcdef0123456789abcdef0123456"
    talk (n + 1)

read_lines : Stream.Reader -> <IO, Panic, Async> List String
read_lines r =
  let (s, r) = Stream.read_all r
  Stream.close r
  String.lines s

summary : String -> List String -> <IO, Panic> Unit
summary name lines =
  println "\{name}: \{show (List.length lines)} lines"
  match List.nth 2047 lines with
    | Some last -> println last
    | None -> println "no last line"

parent : Unit -> <IO, Panic, Async> Unit
parent () =
  let child = Exec.spawn (Exec.pipe_stderr `eml run exec_stderr_split.em talk`)
  let (out, child) = Exec.stdout child
  let (err, child) = Exec.stderr child
  let outs = Async.start (fn () -> read_lines out)
  let errs = Async.start (fn () -> read_lines err)
  let out_lines = Async.await outs
  let err_lines = Async.await errs
  let code = Exec.wait child
  summary "stdout" out_lines
  summary "stderr" err_lines
  println "exit \{show code}"
