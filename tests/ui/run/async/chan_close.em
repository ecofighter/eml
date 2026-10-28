-- `close` は待っている受け手と送り手をすべて起こす。受け手は残った品物を取り切った後に `None` を、送り手は値を
-- `Rejected` で受け取る。2回目の `close` は何もせず、閉じて空になった channel は放した後も閉じて空の channel として
-- 振る舞う (docs/spec/stdlib.md の「`Std.Chan`」)。待ち手は `yield_now` の間に待ち始めるが、seed の方針では閉じた
-- 後に動くこともあり、そのときも同じ結果になる。
import Async (Async)
import Async
import Chan

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

-- 空の channel で待つ受け手は、閉じると `None` を受け取る
waiting_receivers : Unit -> <IO, Panic, Async> Unit
waiting_receivers () =
  let c : Chan.Chan Int = Chan.new 1
  let a = Async.start (fn () -> Chan.recv c)
  let b = Async.start (fn () -> Chan.recv c)
  let _ = spin 40
  Async.yield_now ()
  Chan.close c
  println "receivers: \{show (Async.await a)} \{show (Async.await b)}"

-- 待ち合わせの channel で待つ送り手は、閉じると値を取り戻す
waiting_sender : Unit -> <IO, Panic, Async> Unit
waiting_sender () =
  let c = Chan.new 0
  let s = Async.start (fn () -> Chan.send "kept" c)
  let _ = spin 40
  Async.yield_now ()
  Chan.close c
  println "sender: \{show (Async.await s)}"

-- 閉じた後も、積んであった品物は順に取れる
drained : Unit -> <IO, Panic, Async> Unit
drained () =
  let c = Chan.new 3
  let _ = Chan.send 1 c
  let _ = Chan.send 2 c
  Chan.close c
  Chan.close c
  println "send after close: \{show (Chan.send 3 c)}"
  println "try_send after close: \{show (Chan.try_send 4 c)}"
  let x = Chan.recv c
  let y = Chan.recv c
  let z = Chan.recv c
  println "recv: \{show x} \{show y} \{show z}"
  Chan.close c
  println "try_recv after release: \{show (Chan.try_recv c)}"

body : Unit -> <IO, Panic, Async> Unit
body () =
  waiting_receivers ()
  waiting_sender ()
  drained ()

main : Unit -> <IO, Panic> Unit
main () = Async.run body
