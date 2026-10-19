-- 外側の handler の状態に内側の handler の継続をしまったまま、`exit` で中断する。継続が外側の evidence を
-- 指すので、evidence と状態は輪になる。巻き戻しが外側の handler を外すときに状態を取り出して手放し、輪を切る。
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

body : Unit -> <Keep, IO, Panic> Int
body () =
  stash (step ())
  exit 3

main : Unit -> <IO, Panic> Unit
main () =
  let r =
    handle body () from Done 0 with
      | ctl stash v k _ -> k () v
      | return x _ -> x
  println (show r)
