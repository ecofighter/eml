-- 状態のある handler で `k v` を部分適用して別の関数に渡し、渡した先で状態を与えて再開する。
effect Ask where
  ask : Unit -> Int

later : (Int -> <e> Int) -> Int -> <e> Int
later f s = f s

main : Unit -> <IO> Unit
main () =
  let r =
    handle ask () * 2 from 0 with
      | ask () k st -> later (k 5) (st + 100)
      | return x s -> x + s
  println (show_int r)
