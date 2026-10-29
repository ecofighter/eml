-- `Net` の失敗の種類。拒まれた接続は `ConnectionRefused`、解決できない名前は `NotFound`、使用中のポートは
-- `AddrInUse` である。`listen` の名前と範囲の外のポートは、操作を出す前に種類 `Other` で断る
-- (docs/spec/stdlib.md の「`Std.Net`」)。
import Async
import Net

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

report : Result IOError a -> <IO> Unit
report r =
  match r with
    | Ok _ -> println "ok"
    | Err e -> println "\{show e.kind}: \{e.message}"

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let t = Async.start (fn () -> spin 40)
  let _ = spin 40
  report (try_io (fn () -> Net.close (Net.connect "127.0.0.1" 9)))
  report (try_io (fn () -> Net.close (Net.connect "nowhere.invalid" 80)))
  report (try_io (fn () -> Net.close (Net.connect "127.0.0.1" 0)))
  report (try_io (fn () -> Net.close (Net.connect "127.0.0.1" 65536)))
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  report (try_io (fn () -> Net.close_listener (Net.listen "127.0.0.1" port)))
  report (try_io (fn () -> Net.close_listener (Net.listen "localhost" 0)))
  report (try_io (fn () -> Net.close_listener (Net.listen "127.0.0.1" (-1))))
  Net.close_listener l
  let _ = Async.await t
  println "done")
