-- `merge` は入力の品物をすべて1つの channel に集め、入力がすべて閉じて空になると出力を閉じる。順はスケジュールで
-- 決まるので並べ替えて出力する。空の並びの `merge` は閉じた channel を返す。出力を先に閉じると、転送のタスクは次に
-- 受け取った品物を落として終わり、`Lin` の品物も漏れない。閉じない入力を待つ転送は、スコープを取り消すと止まる
-- (docs/spec/stdlib.md の「`Std.Chan`」)。
import Async (Async)
import Async
import Chan
import List
import MutArray

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

put : a -> Chan.Chan a -> <IO, Async> Unit
put x c =
  match Chan.send x c with
    | Chan.Sent -> ()
    | Chan.Rejected x -> drop x

produce : Int -> Int -> Chan.Chan Int -> <IO, Panic, Async> Unit
produce i n c =
  if i > n then Chan.close c
  else
    put i c
    produce (i + 1) n c

-- `lo` から `hi` までを送るタスクの channel
source : Int -> Int -> <IO, Panic, Async> Chan.Chan Int
source lo hi =
  let c = Chan.new 1
  Async.spawn (fn () -> produce lo hi c)
  let _ = spin 40
  c

collect : Chan.Chan a -> List a -> <IO, Async> List a
collect c acc =
  match Chan.recv c with
    | Some x -> collect c (x :: acc)
    | None -> acc

-- 配列を2つ積んで閉じた channel
filled : Unit -> <IO, Panic, Async> Chan.Chan (MutArray Int)
filled () =
  let c = Chan.new 2
  put (MutArray.make 1 0) c
  put (MutArray.make 1 0) c
  Chan.close c
  c

body : Unit -> <IO, Panic, Async> Unit
body () =
  let a = source 1 3
  let b = source 4 6
  let c = source 7 9
  println (show (List.sort (collect (Chan.merge [a, b, c]) Nil)))
  let none : Chan.Chan Int = Chan.merge []
  println (show (Chan.recv none))
  let out = Chan.merge [filled (), filled ()]
  match Chan.recv out with
    | Some x -> drop x
    | None -> ()
  Chan.close out
  println "closed early"
  let open : Chan.Chan Int = Chan.new 1
  Async.scope (fn s ->
    let merged = Chan.merge [open]
    let _ = spin 40
    Chan.close merged
    Async.cancel_scope s)
  Chan.close open
  println "forwarders stopped"

main : Unit -> <IO, Panic> Unit
main () = Async.run body
