-- windows: skip (シグナルで終わる子は unix にだけある)
-- clock: real (the test driver has no processes)
-- どのworkerのタスクが始めた子も、SIGINTとSIGTERMを塞がずに始まる (docs/spec/stdlib.md の「`Proc`」「`Std.Exec`」)。
-- 2つは同じ集合で外すので、SIGTERMで確かめる。子のシェルは自分にSIGTERMを送り、既定の動作で終わる。塞いだまま
-- 始まった子は生き残り、`survived`を書く。
-- 2つ以上のworkerでは、`Async.start`のタスクはworker 1で、`Async.start_on_main`のタスクはメインのworkerで動く
import Async (Async)
import Async
import Exec
import Proc

report : String -> Unit -> <IO, Async> Unit
report name () =
  let (out, code) = Proc.output `sh -c 'kill -TERM $$; echo survived'`
  println "\{name} Proc: \{show out} exit \{show code}"
  let (out, code) = Exec.output `sh -c 'kill -TERM $$; echo survived'`
  println "\{name} Exec: \{show out} exit \{show code}"

body : Unit -> <IO, Panic, Async> Unit
body () =
  Async.await (Async.start (report "started"))
  Async.await (Async.start_on_main (report "main-worker"))

main : Unit -> <IO, Panic> Unit
main () = Async.run body
