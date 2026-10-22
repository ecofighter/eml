-- logstat: アクセスログを集計する。`logstat [-n N] [--whole]` で、標準入力の各行
-- `時刻 メソッド パス ステータス 応答時間` を読む。行を流す生産者、行を解析する handler、集計する handler の3段を
-- `use` で重ねる。`--whole` は、1行ずつでなく全体を読んでから行に分ける (2つの読み方の性能を比べるため)
import Yield (Yield(..))
import Env
import List
import Map
import Stdin
import String

-- 解析した1行。形の崩れた行は行番号だけを持つ
data Parsed =
  | Entry { path : String, status : Int, duration : Int }
  | Malformed Int

effect Record where
  record : Parsed -> Unit

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
    , statuses : Map.Map Int Int
    , paths : Map.Map String Int
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
      , statuses = Map.upsert status increment st.statuses
      , paths = Map.upsert path increment st.paths
      , count = st.count + 1
      , sum = st.sum + d
      , max = if st.count == 0 || d > st.max then d else st.max
      }

aggregate : (Unit -> <Record, IO | e> Unit) -> <IO, Panic | e> Stats
aggregate body =
  let empty = Stats { total = 0, malformed = 0, statuses = Map.empty, paths = Map.empty, count = 0, sum = 0, max = 0 }
  handle body () from empty with
    | fn record parsed st -> ((), add st parsed)
    | return _ st -> st

report : Int -> Stats -> <IO, Panic> Unit
report top st =
  println "lines \{st.total}"
  println "malformed \{st.malformed}"
  List.each (fn (status, n) -> println "status \{status} \{n}") (Map.to_list st.statuses)
  -- 木はパスの昇順に並ぶので、件数で安定に並べ替えれば、同じ件数のパスは昇順のまま残る
  let by_count = List.sort_by (fn (_, a) (_, b) -> compare b a) (Map.to_list st.paths)
  List.each (fn (path, n) -> println "path \{path} \{n}") (List.take top by_count)
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
