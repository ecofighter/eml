-- 入れ子の `match` の枝の中で yield が起きる。枝が束縛した `s` は 2 段の分解を通って `p` から借りているので、Perceus は
-- 節の呼び出しの前に `p` と `opt` を親から順に `release` して `s` を受け取る。`None` の枝では `p` を `decref` するだけである。
effect Ask where
  ask : Unit -> Int

data P =
  | P (Option String) Int

go : P -> <Ask> String
go p = match p with
  | P opt n -> match opt with
      | Some s -> s ++ show (ask () + n)
      | None -> "none"

main : Unit -> <IO> Unit
main () =
  let r = handle go (P (Some "v") 1) with
    | ask () k -> k 10
  println r
  let r2 = handle go (P None 1) with
    | ask () k -> k 10
  println r2
