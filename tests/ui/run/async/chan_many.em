-- 複数の送り手と受け手が1つの channel を分け合う。品物はどれも1回だけ、どれか1つの受け手に届く。受け手ごとの数は
-- スケジュールで決まるので、合計だけを出力する (docs/spec/stdlib.md の「`Std.Chan`」)。
import Async (Async)
import Async
import Chan

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

produce : Int -> Int -> Chan.Chan Int -> <IO, Panic, Async> Unit
produce i n c =
  if i > n then ()
  else match Chan.send i c with
    | Chan.Sent -> produce (i + 1) n c
    | Chan.Rejected _ -> println "unreachable"

-- 受け取った数と和を返す
consume : Chan.Chan Int -> Int -> Int -> <IO, Panic, Async> (Int, Int)
consume c count sum =
  match Chan.recv c with
    | Some x -> consume c (count + 1) (sum + x)
    | None -> (count, sum)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let c = Chan.new 2
  let p = Async.start (fn () -> produce 1 20 c)
  let _ = spin 40
  let q = Async.start (fn () -> produce 21 40 c)
  let _ = spin 40
  let r = Async.start (fn () -> produce 41 60 c)
  let _ = spin 40
  let u = Async.start (fn () -> consume c 0 0)
  let _ = spin 40
  let v = Async.start (fn () -> consume c 0 0)
  let _ = spin 40
  Async.await p
  Async.await q
  Async.await r
  Chan.close c
  let (n1, s1) = Async.await u
  let (n2, s2) = Async.await v
  println "\{show (n1 + n2)} items, sum \{show (s1 + s2)}"

main : Unit -> <IO, Panic> Unit
main () = Async.run body
