-- タスクの `exit 3` はプログラムを終了コード 3 で終える。待っている本体と `main` はそれぞれ `FileIn` を持つが、巻き戻さず、
-- ヒープを落とすときに閉じるので、検査モードで漏れが出ない (docs/spec/stdlib.md の「`exit`」)。
import Async (Async)
import FileIn

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let f = FileIn.open "exit_in_task.em"
  let x = Async.start (fn () ->
    let _ = spin 40
    println "exiting"
    (exit 3 : Unit))
  let _ = spin 40
  Async.await x
  FileIn.close f

main : Unit -> <IO, Panic> Unit
main () =
  let g = FileIn.open "exit_in_task.em"
  println "before"
  Async.run body
  FileIn.close g
  println "unreachable"
