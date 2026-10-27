-- 外側の handler の状態に内側の handler の継続をしまったまま、さらに外側の `Fail.to_result` へ中断する。継続が
-- 捕まえる内側の evidence は退いているので、状態から外側の evidence には届かない。巻き戻しは外側の handler を
-- 外すときに状態を取り出して手放す。
import Fail (Fail(..))

data R =
  | Done Int
  | Paused (Int -> <Panic> R)

effect Ask where
  multi ask : Unit -> Int

effect Keep where
  stash : R -> Unit

step : Unit -> <Panic> R
step () =
  handle Done (ask () + 1) with
    | ctl ask () k -> Paused k

body : Unit -> <Keep, Fail String, Panic> Int
body () =
  stash (step ())
  Fail.fail "stop"

outer : Unit -> <Fail String, Panic> Int
outer () =
  handle body () from Done 0 with
    | ctl stash v k _ -> k () v
    | return x _ -> x

main : Unit -> <IO, Panic> Unit
main () =
  println (show (Fail.to_result outer))
  println "after"
