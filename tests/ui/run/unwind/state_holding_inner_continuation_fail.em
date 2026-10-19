-- 外側の handler の状態に内側の handler の継続をしまったまま、さらに外側の `Fail.to_result` へ中断する。継続が
-- 外側の evidence を指すので、evidence と状態は輪になる。巻き戻しが外側の handler を外すときに状態を取り出して
-- 手放し、輪を切る。
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
