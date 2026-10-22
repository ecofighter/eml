-- `Set` の木は、挿入、削除、集合の演算のどれの後でも AVL 木の印と高さが合い、要素が昇順に並ぶ
-- (docs/spec/stdlib.md の「`Std.Set`」)。結果を、昇順のリストで計算した値と比べる
import List
import Set

valid : Set.Set Int -> Int -> Int -> <Panic> Option Int
valid s lo hi =
  let node l x r want =
    if x < lo || x > hi then None
    else
      match (valid l lo (x - 1), valid r (x + 1) hi) with
        | (Some hl, Some hr) -> if hl - hr == want then Some ((if hl > hr then hl else hr) + 1) else None
        | _ -> None
  match s with
    | Set.Empty -> Some 0
    | Set.LeftHigh l x r -> node l x r 1
    | Set.Even l x r -> node l x r 0
    | Set.RightHigh l x r -> node l x r (0 - 1)

-- 線形合同法の乱数で、0 から `range - 1` の数を `n` 個
numbers : Int -> Int -> Int -> List Int -> <Panic> List Int
numbers n range seed acc =
  if n == 0 then acc
  else
    let next = (seed * 1103515245 + 12345) % 2147483648
    numbers (n - 1) range next ((next / 65536) % range :: acc)

-- 昇順で重複のないリスト
model : List Int -> List Int
model xs =
  let go acc ys =
    match ys with
      | Nil -> List.reverse acc
      | y :: rest ->
        match acc with
          | z :: _ -> if y == z then go acc rest else go (y :: acc) rest
          | Nil -> go [y] rest
  go Nil (List.sort xs)

same : Set.Set Int -> List Int -> <Panic> Bool
same s expected =
  match valid s 0 100000 with
    | Some _ -> Set.to_list s == expected
    | None -> False

check : Int -> Int -> Int -> <IO, Panic> Unit
check n m seed =
  let xs = numbers n 3000 seed Nil
  let ys = numbers m 3000 (seed + 1) Nil
  let a = Set.from_list xs
  let b = Set.from_list ys
  let ma = model xs
  let mb = model ys
  let union = same (Set.union a b) (model (List.append xs ys))
  let inter = same (Set.intersection a b) (List.filter (fn x -> List.contains x mb) ma)
  let diff = same (Set.difference a b) (List.filter (fn x -> not (List.contains x mb)) ma)
  let removed = same (List.fold (fn s y -> Set.delete y s) a ys) (List.filter (fn x -> not (List.contains x mb)) ma)
  println "\{n} \{m}: union \{union}, intersection \{inter}, difference \{diff}, delete \{removed}"

main : Unit -> <IO, Panic> Unit
main () =
  check 1000 1000 1
  check 1000 20 2
  check 20 1000 3
  check 0 500 4
