-- logstat: アクセスログを集計する。`logstat [-n N] [--whole]` で、標準入力の各行
-- `時刻 メソッド パス ステータス 応答時間` を読む。行を流す生産者、行を解析する handler、集計する handler の3段を
-- `use` で重ねる。`--whole` は、1行ずつでなく全体を読んでから行に分ける (2つの読み方の性能を比べるため)
import Yield (Yield(..))
import Env
import List
import Stdin
import String

-- 解析した1行。形の崩れた行は行番号だけを持つ
data Parsed =
  | Entry { path : String, status : Int, duration : Int }
  | Malformed Int

effect Record where
  record : Parsed -> Unit

-- 釣り合いを取った二分探索木 (AA 木)。`Node` の最初の欄は節の高さの段である。パスに ID を含むログでは、キーが
-- 昇順に増えるので、釣り合いを取らないと挿入が全体で2乗の時間になる
data Tree k v =
  | Leaf
  | Node Int (Tree k v) k v (Tree k v)

-- 左の子が同じ段なら、右に回す
skew : Tree k v -> Tree k v
skew tree =
  match tree with
    | Node level (Node left_level a lk lv b) k v right ->
      if left_level == level then Node level a lk lv (Node level b k v right) else tree
    | _ -> tree

-- 右の孫が同じ段なら、右の子を1段上げる
split : Tree k v -> <Panic> Tree k v
split tree =
  match tree with
    | Node level a k v (Node right_level b rk rv (Node grand_level c gk gv d)) ->
      if grand_level == level then
        Node (right_level + 1) (Node level a k v b) rk rv (Node grand_level c gk gv d)
      else tree
    | _ -> tree

-- `k` の値を `f` で更新する。なければ `f None` を入れる
upsert : Ord k => k -> (Option v -> <Panic> v) -> Tree k v -> <Panic> Tree k v
upsert key f tree =
  match tree with
    | Leaf -> Node 1 Leaf key (f None) Leaf
    | Node level left k v right ->
      match compare key k with
        | LT -> split (skew (Node level (upsert key f left) k v right))
        | GT -> split (skew (Node level left k v (upsert key f right)))
        | EQ -> Node level left k (f (Some v)) right

-- キーの昇順の組の列
entries : Tree k v -> List (k, v)
entries tree =
  let go t acc =
    match t with
      | Leaf -> acc
      | Node _ left k v right -> go left ((k, v) :: go right acc)
  go tree Nil

-- 安定なマージソート。`before a b` は `a` を `b` より前に置くときに `True` である
sort_by : (a -> a -> Bool) -> List a -> <Panic> List a
sort_by before xs =
  let merge acc left right =
    match (left, right) with
      | (Nil, _) -> List.append (List.reverse acc) right
      | (_, Nil) -> List.append (List.reverse acc) left
      | (l :: ls, r :: rs) ->
        if before r l then merge (r :: acc) left rs else merge (l :: acc) ls right
  let n = List.length xs
  if n <= 1 then xs
  else
    let (left, right) = split_at (n / 2) xs
    merge Nil (sort_by before left) (sort_by before right)

split_at : Int -> List a -> <Panic> (List a, List a)
split_at n xs =
  let go i acc rest =
    match rest with
      | x :: more -> if i > 0 then go (i - 1) (x :: acc) more else (List.reverse acc, rest)
      | Nil -> (List.reverse acc, Nil)
  go n Nil xs

take : Int -> List a -> <Panic> List a
take n xs = (split_at n xs).0

stdin_lines : Unit -> <Yield String, IO, Panic> Unit
stdin_lines () =
  let go h =
    match Stdin.read_line h with
      | (Some line, h) ->
        yield line
        go h
      | (None, h) -> Stdin.close h
  go (Stdin.open ())

each_line : List String -> <Yield String> Unit
each_line lines = List.each yield lines

parse : Int -> String -> <Panic> Parsed
parse number line =
  match String.split " " line with
    | [_, _, path, status, duration] ->
      match (String.to_int status, String.to_int duration) with
        | (Some s, Some d) -> Entry { path = path, status = s, duration = d }
        | _ -> Malformed number
    | _ -> Malformed number

-- 流れてきた行を解析して記録する。行番号を状態に持つ
parsing : (Unit -> <Yield String, Record | e> Unit) -> <Record, Panic | e> Unit
parsing lines =
  handle lines () from 1 with
    | fn yield line number -> (record (parse number line), number + 1)
    | return _ _ -> ()

data Stats =
  | Stats
    { total : Int
    , malformed : Int
    , statuses : Tree Int Int
    , paths : Tree String Int
    , count : Int
    , sum : Int
    , max : Int
    }

increment : Option Int -> <Panic> Int
increment n =
  match n with
    | Some c -> c + 1
    | None -> 1

add : Stats -> Parsed -> <IO, Panic> Stats
add st parsed =
  match parsed with
    | Malformed number ->
      if st.malformed == 0 then eprintln "logstat: line \{number}: malformed" else ()
      { st | total = st.total + 1, malformed = st.malformed + 1 }
    | Entry { path = path, status = status, duration = d } ->
      { st | total = st.total + 1
      , statuses = upsert status increment st.statuses
      , paths = upsert path increment st.paths
      , count = st.count + 1
      , sum = st.sum + d
      , max = if st.count == 0 || d > st.max then d else st.max
      }

aggregate : (Unit -> <Record, IO | e> Unit) -> <IO, Panic | e> Stats
aggregate body =
  let empty = Stats { total = 0, malformed = 0, statuses = Leaf, paths = Leaf, count = 0, sum = 0, max = 0 }
  handle body () from empty with
    | fn record parsed st -> ((), add st parsed)
    | return _ st -> st

report : Int -> Stats -> <IO, Panic> Unit
report top st =
  println "lines \{st.total}"
  println "malformed \{st.malformed}"
  List.each (fn (status, n) -> println "status \{status} \{n}") (entries st.statuses)
  -- 木はパスの昇順に並ぶので、件数で安定に並べ替えれば、同じ件数のパスは昇順のまま残る
  let by_count = sort_by (fn (_, a) (_, b) -> a > b) (entries st.paths)
  List.each (fn (path, n) -> println "path \{path} \{n}") (take top by_count)
  let average = if st.count == 0 then 0 else st.sum / st.count
  println "duration avg \{average} max \{st.max}"

-- `-n` の値と `--whole` を読む。誤りなら `None` である
parse_args : (Int, Bool) -> List String -> <Panic> Option (Int, Bool)
parse_args (top, whole) args =
  match args with
    | Nil -> Some (top, whole)
    | "--whole" :: rest -> parse_args (top, True) rest
    | "-n" :: value :: rest ->
      match String.to_int value with
        | Some n -> if n >= 0 then parse_args (n, whole) rest else None
        | None -> None
    | _ -> None

main : Unit -> <IO, Panic> Unit
main () =
  match parse_args (10, False) (Env.args ()) with
    | None ->
      eprintln "usage: logstat [-n N] [--whole]"
      exit 2
    | Some (top, whole) ->
      let result =
        use try_io
        use aggregate
        use parsing
        if whole then each_line (String.lines (Stdin.read_text ())) else stdin_lines ()
      match result with
        | Ok stats -> report top stats
        | Err e ->
          eprintln "logstat: \{e}"
          exit 1
