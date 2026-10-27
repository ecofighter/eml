-- 外側の handler の状態に内側の handler の継続をしまったまま、`exit` で中断する。継続が捕まえる内側の evidence は
-- 退いているので、状態から外側の evidence には届かない。巻き戻しは外側の handler を外すときに状態を取り出して手放す。
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
