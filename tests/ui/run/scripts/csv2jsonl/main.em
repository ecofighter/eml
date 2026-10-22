-- csv2jsonl: ヘッダ付きの CSV の選んだ列を JSON Lines に変換する。
-- `csv2jsonl --columns NAME,NAME... INPUT OUTPUT`。引用符の欄は `,`、改行、`""` を含められる。誤りは終了コードと文言の
-- 組を `fail` に渡して中断し、全体を組み立ててから書くので、誤りがあれば何も書かない
import Yield (Yield(..))
import Fail (Fail(..))
import Env
import FileIn
import Fs
import List
import String

-- 1つの行の欄と、その行が始まる入力の行の番号
data Row =
  | Row { line : Int, fields : List String }

-- `at` 以降で最初の `byte` の位置。なければ `n` である
find_byte : Int -> Int -> String -> Int -> <Panic> Int
find_byte byte at text n =
  if at >= n || String.byte_at at text == byte then at else find_byte byte (at + 1) text n

count_byte : Int -> String -> <Panic> Int
count_byte byte s =
  let n = String.byte_length s
  let go i acc = if i >= n then acc else go (i + 1) (if String.byte_at i s == byte then acc + 1 else acc)
  go 0 0

-- 入力を行ごとに流す。空の行は飛ばす。読み取りの状態 (行の始め、欄の中、引用符の中、引用符の閉じた後) を
-- 1つの `let … and …` の組の関数にする (docs/spec/expressions.md の「ローカル関数」)。`row_line` は行が始まる入力の
-- 行の番号、`line` は今読んでいる入力の行の番号で、引用符の中の改行の分だけ `row_line` から進む。欄は `fields` に
-- 逆順に積む
rows : String -> <Yield Row, Fail (Int, String), Panic> Unit
rows text =
  let n = String.byte_length text
  let row_start at line =
    if at >= n then ()
    else if String.byte_at at text == 10 then row_start (at + 1) (line + 1)
    else if String.byte_at at text == 13 && at + 1 < n && String.byte_at (at + 1) text == 10 then
      row_start (at + 2) (line + 1)
    else field at line line Nil
  and field at row_line line fields =
    if at < n && String.byte_at at text == 34 then in_quotes (at + 1) "" row_line line fields
    else
      let stop = unquoted_end at text n
      let fields = without_cr (String.slice at stop text) :: fields
      if stop < n && String.byte_at stop text == 44 then field (stop + 1) row_line line fields
      else
        yield (Row { line = row_line, fields = List.reverse fields })
        row_start (if stop < n then stop + 1 else n) (line + 1)
  -- `at` から閉じの引用符までを `acc` に足す。2つ続けた引用符は1つの引用符である
  and in_quotes at acc row_line line fields =
    let close = find_byte 34 at text n
    if close >= n then fail (1, "line \{row_line}: unclosed quote")
    else
      let piece = String.slice at close text
      let line = line + count_byte 10 piece
      if close + 1 < n && String.byte_at (close + 1) text == 34 then
        in_quotes (close + 2) ((acc ++ piece) ++ "\"") row_line line fields
      else after_quote (close + 1) row_line line ((acc ++ piece) :: fields)
  and after_quote at row_line line fields =
    if at < n && String.byte_at at text == 44 then field (at + 1) row_line line fields
    else
      let next =
        if at >= n then n
        else if String.byte_at at text == 10 then at + 1
        else if String.byte_at at text == 13 && at + 1 < n && String.byte_at (at + 1) text == 10 then at + 2
        else fail (1, "line \{line}: unexpected text after a closing quote")
      yield (Row { line = row_line, fields = List.reverse fields })
      row_start next (line + 1)
  row_start 0 1

-- 引用符で囲まない欄の終わり (`,` か `\n` の位置、なければ `n`)
unquoted_end : Int -> String -> Int -> <Panic> Int
unquoted_end at text n =
  if at >= n then n
  else
    let b = String.byte_at at text
    if b == 44 || b == 10 then at else unquoted_end (at + 1) text n

without_cr : String -> <Panic> String
without_cr s =
  let n = String.byte_length s
  if n > 0 && String.byte_at (n - 1) s == 13 then String.slice 0 (n - 1) s else s

-- `json.dumps` と同じエスケープで、JSON の文字列にする
json_string : String -> <Panic> String
json_string s =
  let n = String.byte_length s
  -- `start` から `i` の手前までは、エスケープの要らないバイトの列である
  let go start i acc =
    if i >= n then (acc ++ String.slice start n s) ++ "\""
    else
      let b = String.byte_at i s
      if b == 34 || b == 92 || b < 32 then
        go (i + 1) (i + 1) ((acc ++ String.slice start i s) ++ escape b)
      else go start (i + 1) acc
  go 0 0 "\""

escape : Int -> <Panic> String
escape b =
  match b with
    | 34 -> "\\\""
    | 92 -> "\\\\"
    | 8 -> "\\b"
    | 12 -> "\\f"
    | 10 -> "\\n"
    | 13 -> "\\r"
    | 9 -> "\\t"
    | _ -> "\\u00" ++ hex_digit (b / 16) ++ hex_digit (b % 16)

hex_digit : Int -> <Panic> String
hex_digit d = String.slice d (d + 1) "0123456789abcdef"

index_of : String -> List String -> <Panic> Option Int
index_of x xs =
  let go i rest =
    match rest with
      | Nil -> None
      | y :: more -> if x == y then Some i else go (i + 1) more
  go 0 xs

nth : Int -> List a -> <Panic> Option a
nth i xs =
  match xs with
    | Nil -> None
    | x :: rest -> if i == 0 then Some x else nth (i - 1) rest

-- 選んだ列の位置。ヘッダにない列名は終了コード 2 の誤りである
column_indexes : List String -> List String -> <Fail (Int, String), Panic> List Int
column_indexes header columns =
  List.map
    (fn name ->
      match index_of name header with
        | Some i -> i
        | None -> fail (2, "unknown column `\{name}`"))
    columns

json_line : List String -> List Int -> Row -> <Panic> String
json_line columns indexes row =
  let pick name i =
    match nth i row.fields with
      | Some value -> (json_string name ++ ":") ++ json_string value
      | None -> ""
  let fields = List.map (fn (name, i) -> pick name i) (zip columns indexes)
  ("{" ++ String.join "," fields) ++ "}\n"

zip : List a -> List b -> List (a, b)
zip xs ys =
  let go acc l r =
    match (l, r) with
      | (x :: ls, y :: rs) -> go ((x, y) :: acc) ls rs
      | _ -> List.reverse acc
  go Nil xs ys

-- 最初の行をヘッダにし、残りの行を JSON Lines の文字列に変換する。状態は、ヘッダを読んだ後の列の位置と出力である
convert : List String -> (Unit -> <Yield Row, Fail (Int, String) | e> Unit) -> <Fail (Int, String), Panic | e> String
convert columns body =
  handle body () from (None, "") with
    | ctl yield row k (picked, out) ->
      match picked with
        | None -> k () (Some (List.length row.fields, column_indexes row.fields columns), out)
        | Some (width, indexes) ->
          let found = List.length row.fields
          if found != width then
            fail (1, "line \{row.line}: expected \{width} fields, found \{found}")
          else k () (picked, out ++ json_line columns indexes row)
    | return _ (picked, out) ->
      match picked with
        | None -> fail (1, "line 1: missing header")
        | Some _ -> out

read_text : String -> <IO> Result IOError String
read_text path =
  use try_io
  let f = FileIn.open path
  let (text, f) = FileIn.read_all f
  FileIn.close f
  text

main : Unit -> <IO, Panic> Unit
main () =
  match Env.args () with
    | ["--columns", names, source, target] ->
      match read_text source with
        | Err e ->
          eprintln "csv2jsonl: \{e}"
          exit 1
        | Ok text ->
          let result =
            use Fail.to_result
            use convert (String.split "," names)
            rows text
          match result with
            | Ok out ->
              match try_io (fn () -> Fs.write_text target out) with
                | Ok _ -> ()
                | Err e ->
                  eprintln "csv2jsonl: \{e}"
                  exit 1
            | Err (code, message) ->
              eprintln "csv2jsonl: \{message}"
              exit code
    | _ ->
      eprintln "usage: csv2jsonl --columns NAME,NAME... INPUT OUTPUT"
      exit 2
