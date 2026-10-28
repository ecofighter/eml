-- `timeout ms f` は本体を呼んだタスクで動かす。期限が先に来れば本体を取り消して `None` を返し、本体が値を返せば
-- `Some` で返す。過ぎた期限では本体は最初の取り消しの地点で取り消される。本体の失敗はそのまま出る
-- (docs/spec/stdlib.md の「時間」)。
import Async (Async)
import MutArray

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

after : Int -> a -> <IO, Async> a
after ms x =
  Async.sleep_ms ms
  x

lookup : Unit -> <IO, Async> Int
lookup () = fail_io NotFound "no such host"

body : Unit -> <IO, Panic, Async> Unit
body () =
  let late = Async.timeout 50 (fn () ->
    let _ = spin 40
    after 100 "late")
  println (show late)
  let early = Async.timeout 100 (fn () ->
    let _ = spin 40
    after 50 "early")
  println (show early)
  println (show (Async.timeout 0 (fn () -> 1)))
  println (show (Async.timeout (-5) (fn () -> after 10 2)))
  match Async.try_all (fn () -> Async.timeout 100 lookup) with
    | Ok r -> println (show r)
    | Err e -> println "failed: \{show e}"
  match Async.timeout 100 (fn () -> MutArray.make 2 7) with
    | Some a ->
      let (x, a) = MutArray.read a 1
      drop a
      println "array \{show x}"
    | None -> println "unreachable"

main : Unit -> <IO, Panic> Unit
main () = Async.run body
