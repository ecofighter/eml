-- std の `Map.upsert` で 20000 個の文字列のキーを数え、`List.sort_by` で件数の降順に並べる (`upsert.em` と同じ入力)。
-- 釣り合いを直す関数が受け取った節の箱を使い回すことと、並べ替えが手放すセルを使い回すことを数える
import Std.List
import Map

increment : Option Int -> <Panic> Int
increment old =
  match old with
    | None -> 1
    | Some n -> n + 1

count_keys : Int -> Int -> Map.Map String Int -> <Panic> Map.Map String Int
count_keys n seed m =
  if n == 0 then m
  else
    let next = (seed * 1103515245 + 12345) % 2147483648
    let key = "k" ++ show ((next / 65536) % 2000)
    count_keys (n - 1) next (Map.upsert key increment m)

main : Unit -> <IO, Panic> Unit
main () =
  let counts = count_keys 20000 42 Map.empty
  let by_count = List.sort_by (fn (_, a) (_, b) -> compare b a) (Map.to_list counts)
  println "\{List.length by_count} \{match by_count with | (k, v) :: _ -> "\{k} \{v}" | Nil -> ""}"
