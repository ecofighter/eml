-- `Map` の木は、挿入と削除のどの順でも AVL 木の印と高さが合い、キーが昇順に並ぶ (docs/spec/stdlib.md の「`Std.Map`」)
import List
import Map

-- 印と高さが合い、キーが `lo` から `hi` の間で昇順なら高さを返す
valid : Map.Map Int Int -> Int -> Int -> <Panic> Option Int
valid m lo hi =
  let node l k r want =
    if k < lo || k > hi then None
    else
      match (valid l lo (k - 1), valid r (k + 1) hi) with
        | (Some hl, Some hr) -> if hl - hr == want then Some ((if hl > hr then hl else hr) + 1) else None
        | _ -> None
  match m with
    | Map.Empty -> Some 0
    | Map.LeftHigh l k _ r -> node l k r 1
    | Map.Even l k _ r -> node l k r 0
    | Map.RightHigh l k _ r -> node l k r (0 - 1)

-- 比べる相手の、昇順で重複のないリスト
model_insert : Int -> List Int -> List Int
model_insert x xs =
  match xs with
    | Nil -> [x]
    | y :: rest -> if x < y then x :: xs else if x == y then xs else y :: model_insert x rest

keys : Map.Map Int Int -> List Int
keys m = List.map (fn (k, _) -> k) (Map.to_list m)

-- 線形合同法の乱数で、3回に1回消し、ほかは入れる。毎回、木を確かめる
run : Int -> Int -> Map.Map Int Int -> List Int -> Int -> <IO, Panic> Unit
run n seed m model bad =
  if n == 0 then println "random: \{List.length model} keys, \{bad} bad, height \{valid m 0 1000}"
  else
    let next = (seed * 1103515245 + 12345) % 2147483648
    let key = (next / 65536) % 300
    let (m2, model2) =
      if (next / 4096) % 3 == 0 then (Map.delete key m, List.filter (fn y -> y != key) model)
      else (Map.insert key key m, model_insert key model)
    let ok =
      match valid m2 0 1000 with
        | Some _ -> keys m2 == model2
        | None -> False
    run (n - 1) next m2 model2 (if ok then bad else bad + 1)

count_up : Int -> Int -> List Int -> <Panic> List Int
count_up i n acc = if i == n then List.reverse acc else count_up (i + 1) n (i :: acc)

main : Unit -> <IO, Panic> Unit
main () =
  run 2000 7 Map.empty Nil 0
  let ascending = List.fold (fn m k -> Map.insert k k m) Map.empty (count_up 0 1000 Nil)
  println "ascending: \{Map.size ascending} keys, height \{valid ascending 0 1000}"
  let descending = List.fold (fn m k -> Map.insert k k m) Map.empty (List.reverse (count_up 0 1000 Nil))
  println "descending: \{Map.size descending} keys, height \{valid descending 0 1000}"
  let halved = List.fold (fn m k -> Map.delete k m) ascending (List.filter (fn k -> k % 2 == 0) (count_up 0 1000 Nil))
  println "halved: \{Map.size halved} keys, height \{valid halved 0 1000}"
