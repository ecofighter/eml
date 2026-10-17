-- 協調的なスケジューラ。`once` の操作 `suspend` の継続を `Task` に入れてキューに積み、順に取り出して再開する。
-- 最初の仕事はトップレベルの関数を `Task worker_a` のように `once` のフィールドへそのまま入れる。2つの仕事の
-- 出力は交互に出る (docs/spec/types.md の「トップレベルの関数の参照」)。
data Task <e> =
  | Task (once Unit -> <e> Unit)

effect Async where
  suspend : Unit -> Unit

effect Queue where
  enqueue : Task <Queue, IO> -> Unit
  dequeue : Unit -> Option (Task <Queue, IO>)

run_next : Unit -> <Queue, IO> Unit
run_next () =
  match dequeue () with
    | None -> ()
    | Some (Task resume) -> resume ()

async : (Unit -> <Async, Queue, IO> Unit) -> <Queue, IO> Unit
async body =
  handle body () with
    | suspend () k ->
        enqueue (Task k)
        run_next ()
    | return () -> run_next ()

spawn : Task <Async, Queue, IO> -> <Queue, IO> Unit
spawn task =
  match task with
    | Task body -> enqueue (Task (fn () -> async body))

worker_a : Unit -> <Async, Queue, IO> Unit
worker_a () =
  println "a: 1"
  suspend ()
  println "a: 2"
  suspend ()
  println "a: 3"

worker_b : Unit -> <Async, Queue, IO> Unit
worker_b () =
  println "b: 1"
  suspend ()
  println "b: 2"

schedule : Unit -> <Queue, IO> Unit
schedule () =
  spawn (Task worker_a)
  spawn (Task worker_b)
  run_next ()

main : Unit -> <IO> Unit
main () =
  handle schedule () from [] with
    | enqueue task k queue -> k () (List.append queue [task])
    | dequeue () k queue ->
        match queue with
          | [] -> k None []
          | task :: rest -> k (Some task) rest
    | return () queue ->
        drop queue
        println "all done"
