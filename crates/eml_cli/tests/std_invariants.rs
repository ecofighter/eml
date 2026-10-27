//! 標準ライブラリの抽象型の、外から見えない不変条件のテスト。`Map` と `Set` のコンストラクタはモジュールの外から書けない
//! (docs/spec/modules.md の「公開の範囲」) ので、本物の std のファイルの後ろに木を確かめる `pub valid` を足した std に
//! 差し替え、公開の関数だけで木を組んでから確かめる。

use eml_test_support::with_std;

/// `eml_hir::STD` の `file` を、本物の本文の後ろに `extra` を足したものに替えて `entry` を流し、stdout を返す。
fn run_with_overlay(file: &str, original: &str, extra: &str, entry: &str) -> String {
    let overlaid = format!("{original}{extra}");
    let std: Vec<(&str, &str)> = eml_hir::STD
        .iter()
        .map(|&(name, text)| {
            if name == file {
                (name, overlaid.as_str())
            } else {
                (name, text)
            }
        })
        .collect();
    let (out, result) = eml_test_support::run(with_std(&std, entry));
    assert!(result.is_ok(), "{out}\n{result:?}");
    out
}

/// 印と高さが合い、キーが `lo` から `hi` の間で昇順なら高さを返す。`Std.Map` の中に足すので、コンストラクタが見える。
const MAP_VALID: &str = r##"

pub valid : Map Int v -> Int -> Int -> <Panic> Option Int
valid m lo hi =
  let node l k r want =
    if k < lo || k > hi then None
    else
      match (valid l lo (k - 1), valid r (k + 1) hi) with
        | (Some hl, Some hr) -> if hl - hr == want then Some ((if hl > hr then hl else hr) + 1) else None
        | _ -> None
  match m with
    | Empty -> Some 0
    | LeftHigh l k _ r -> node l k r 1
    | Even l k _ r -> node l k r 0
    | RightHigh l k _ r -> node l k r (0 - 1)
"##;

const MAP_ENTRY: &str = r##"import List
import Map

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
  if n == 0 then println "random: \{List.length model} keys, \{bad} bad, height \{Map.valid m 0 1000}"
  else
    let next = (seed * 1103515245 + 12345) % 2147483648
    let key = (next / 65536) % 300
    let (m2, model2) =
      if (next / 4096) % 3 == 0 then (Map.delete key m, List.filter (fn y -> y != key) model)
      else (Map.insert key key m, model_insert key model)
    let ok =
      match Map.valid m2 0 1000 with
        | Some _ -> keys m2 == model2
        | None -> False
    run (n - 1) next m2 model2 (if ok then bad else bad + 1)

count_up : Int -> Int -> List Int -> <Panic> List Int
count_up i n acc = if i == n then List.reverse acc else count_up (i + 1) n (i :: acc)

main : Unit -> <IO, Panic> Unit
main () =
  run 1000 7 Map.empty Nil 0
  let ascending = List.fold (fn m k -> Map.insert k k m) Map.empty (count_up 0 500 Nil)
  println "ascending: \{Map.size ascending} keys, height \{Map.valid ascending 0 1000}"
  let descending = List.fold (fn m k -> Map.insert k k m) Map.empty (List.reverse (count_up 0 500 Nil))
  println "descending: \{Map.size descending} keys, height \{Map.valid descending 0 1000}"
  let halved = List.fold (fn m k -> Map.delete k m) ascending (List.filter (fn k -> k % 2 == 0) (count_up 0 500 Nil))
  println "halved: \{Map.size halved} keys, height \{Map.valid halved 0 1000}"
"##;

/// `Map` の木は、挿入と削除のどの順でも AVL 木の印と高さが合い、キーが昇順に並ぶ (docs/spec/stdlib.md の「`Std.Map`」)。
#[test]
fn the_map_avl_invariant_holds_through_the_api() {
    let out = run_with_overlay(
        "Map.em",
        include_str!("../../../std/Map.em"),
        MAP_VALID,
        MAP_ENTRY,
    );
    assert_eq!(
        out,
        "random: 194 keys, 0 bad, height Some 9\n\
         ascending: 500 keys, height Some 9\n\
         descending: 500 keys, height Some 9\n\
         halved: 250 keys, height Some 8\n"
    );
}

/// 印と高さが合い、要素が `lo` から `hi` の間で昇順なら高さを返す。`Std.Set` の中に足すので、コンストラクタが見える。
const SET_VALID: &str = r##"

pub valid : Set Int -> Int -> Int -> <Panic> Option Int
valid s lo hi =
  let node l x r want =
    if x < lo || x > hi then None
    else
      match (valid l lo (x - 1), valid r (x + 1) hi) with
        | (Some hl, Some hr) -> if hl - hr == want then Some ((if hl > hr then hl else hr) + 1) else None
        | _ -> None
  match s with
    | Empty -> Some 0
    | LeftHigh l x r -> node l x r 1
    | Even l x r -> node l x r 0
    | RightHigh l x r -> node l x r (0 - 1)
"##;

const SET_ENTRY: &str = r##"import List
import Set

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
  match Set.valid s 0 100000 with
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
"##;

/// `Set` の木は、挿入、削除、集合の演算のどれの後でも AVL 木の印と高さが合い、要素が昇順に並ぶ
/// (docs/spec/stdlib.md の「`Std.Set`」)。結果を、昇順のリストで計算した値と比べる。
#[test]
fn the_set_avl_invariant_holds_through_the_api() {
    let out = run_with_overlay(
        "Set.em",
        include_str!("../../../std/Set.em"),
        SET_VALID,
        SET_ENTRY,
    );
    assert_eq!(
        out,
        "1000 1000: union True, intersection True, difference True, delete True\n\
         1000 20: union True, intersection True, difference True, delete True\n\
         20 1000: union True, intersection True, difference True, delete True\n\
         0 500: union True, intersection True, difference True, delete True\n"
    );
}
