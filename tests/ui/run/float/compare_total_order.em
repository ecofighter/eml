-- `compare` で並べる挿入ソートは、`nan`、`-0.0`、`0.0`、`-inf`、`1.0` を含むリストを全順序
-- (`-inf < -0.0 < 0.0 < 1.0 < nan`) に並べる。演算子の `<` は IEEE のとおりで NaN を順序に入れないので、`compare`
-- を使う。
insert : Float -> List Float -> List Float
insert x xs =
  match xs with
    | [] -> [x]
    | y :: rest ->
      match compare x y with
        | GT -> y :: insert x rest
        | _ -> x :: xs

sort : List Float -> List Float
sort xs =
  match xs with
    | [] -> []
    | x :: rest -> insert x (sort rest)

main : Unit -> <IO, Panic> Unit
main () =
  let inf = 1.0 / 0.0
  println (show (sort [Float.nan, -0.0, 0.0, -inf, 1.0]))
  println (show (sort [1.0, 0.0, Float.nan, -0.0, -inf]))
