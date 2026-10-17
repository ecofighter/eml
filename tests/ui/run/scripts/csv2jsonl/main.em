-- csv2jsonl: ヘッダ付きの CSV の選んだ列を JSON Lines に変換する (S12d のスクリプト)。
-- `csv2jsonl --columns NAME,NAME... INPUT OUTPUT`。引用符の欄は `,`、改行、`""` を含められる。誤りは `never` の操作を
-- 持つ `Fail` で中断し、全体を組み立ててから書くので、誤りがあれば何も書かない
effect Fail where
  never fail : Int -> String -> a

effect Yield a where
  yield : a -> Unit

-- `Fail` を、終了コードと文言の `Err` にする
catching : (Unit -> <Fail | e> a) -> <e> Result (Int, String) a
catching body =
  handle body () with
    | ctl fail code message -> Err (code, message)
    | return x -> Ok x

-- 1つの行の欄と、その行が始まる入力の行の番号
data Row =
  | Row { line : Int, fields : List String }

-- `text` の位置 `start` から、引用符で囲んだ欄の中身を読む。`start` は開きの引用符の次である。中身と、閉じの引用符の
-- 次の位置と、中身の改行の数を返す
quoted : Int -> Int -> String -> Int -> <Fail> (String, Int, Int)
quoted row_line start text n =
  let go at acc newlines =
    let close = find_byte 34 at text n
    if close >= n then fail 1 "line \{row_line}: unclosed quote"
    else
      let piece = String.slice at close text
      let newlines = newlines + count_byte 10 piece
      if close + 1 < n && String.byte_at (close + 1) text == 34 then
        go (close + 2) ((acc ++ piece) ++ "\"") newlines
      else ((acc ++ piece), close + 1, newlines)
  go start "" 0

-- `at` 以降で最初の `byte` の位置。なければ `n` である
find_byte : Int -> Int -> String -> Int -> Int
find_byte byte at text n =
  if at >= n || String.byte_at at text == byte then at else find_byte byte (at + 1) text n

count_byte : Int -> String -> Int
count_byte byte s =
  let n = String.byte_length s
  let go i acc = if i >= n then acc else go (i + 1) (if String.byte_at i s == byte then acc + 1 else acc)
  go 0 0

-- 位置 `at` から1つの行を読む。欄と、次の行の位置と、次の行の番号を返す。`row_line` は、その行が始まる入力の行の
-- 番号である
read_row : String -> Int -> Int -> Int -> <Fail> (List String, Int, Int)
read_row text n at row_line =
  -- 位置 `at` から1つの欄を読み、行の残りを続ける。`fields` は逆順に積む
  let field at line fields =
    if at < n && String.byte_at at text == 34 then
      let (value, after, newlines) = quoted row_line (at + 1) text n
      let line = line + newlines
      let fields = value :: fields
      if after >= n then (List.reverse fields, n, line + 1)
      else
        let b = String.byte_at after text
        if b == 44 then field (after + 1) line fields
        else if b == 10 then (List.reverse fields, after + 1, line + 1)
        else if b == 13 && after + 1 < n && String.byte_at (after + 1) text == 10 then
          (List.reverse fields, after + 2, line + 1)
        else fail 1 "line \{line}: unexpected text after a closing quote"
    else
      let stop = unquoted_end at text n
      let fields = without_cr (String.slice at stop text) :: fields
      if stop < n && String.byte_at stop text == 44 then field (stop + 1) line fields
      else (List.reverse fields, if stop < n then stop + 1 else n, line + 1)
  field at row_line Nil

-- 入力を行ごとに流す。空の行は飛ばす
rows : String -> <Yield Row, Fail> Unit
rows text =
  let n = String.byte_length text
  let go at line =
    if at >= n then ()
    else if String.byte_at at text == 10 then go (at + 1) (line + 1)
    else if String.byte_at at text == 13 && at + 1 < n && String.byte_at (at + 1) text == 10 then
      go (at + 2) (line + 1)
    else
      let (fields, next, next_line) = read_row text n at line
      yield (Row { line = line, fields = fields })
      go next next_line
  go 0 1

-- 引用符で囲まない欄の終わり (`,` か `\n` の位置、なければ `n`)
unquoted_end : Int -> String -> Int -> Int
unquoted_end at text n =
  if at >= n then n
  else
    let b = String.byte_at at text
    if b == 44 || b == 10 then at else unquoted_end (at + 1) text n

without_cr : String -> String
without_cr s =
  let n = String.byte_length s
  if n > 0 && String.byte_at (n - 1) s == 13 then String.slice 0 (n - 1) s else s

-- `json.dumps` と同じエスケープで、JSON の文字列にする
json_string : String -> String
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

escape : Int -> String
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

hex_digit : Int -> String
hex_digit d = String.slice d (d + 1) "0123456789abcdef"

index_of : String -> List String -> Option Int
index_of x xs =
  let go i rest =
    match rest with
      | Nil -> None
      | y :: more -> if x == y then Some i else go (i + 1) more
  go 0 xs

nth : Int -> List a -> Option a
nth i xs =
  match xs with
    | Nil -> None
    | x :: rest -> if i == 0 then Some x else nth (i - 1) rest

-- 選んだ列の位置。ヘッダにない列名は終了コード 2 の誤りである
column_indexes : List String -> List String -> <Fail> List Int
column_indexes header columns =
  List.map
    (fn name ->
      match index_of name header with
        | Some i -> i
        | None -> fail 2 "unknown column `\{name}`")
    columns

json_line : List String -> List Int -> Row -> <Fail> String
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
convert : List String -> (Unit -> <Yield Row, Fail | e> Unit) -> <Fail | e> String
convert columns body =
  handle body () from (None, "") with
    | ctl yield row k (picked, out) ->
      match picked with
        | None -> k () (Some (List.length row.fields, column_indexes row.fields columns), out)
        | Some (width, indexes) ->
          let found = List.length row.fields
          if found != width then
            drop k
            fail 1 "line \{row.line}: expected \{width} fields, found \{found}"
          else k () (picked, out ++ json_line columns indexes row)
    | return _ (picked, out) ->
      match picked with
        | None -> fail 1 "line 1: missing header"
        | Some _ -> out

read_text : String -> <IO> Result IOError String
read_text path =
  use try_io
  let f = Fs.open path
  let (f, text) = Fs.read_all f
  Fs.close f
  text

main : Unit -> <IO> Unit
main () =
  match Env.args () with
    | ["--columns", names, source, target] ->
      match read_text source with
        | Err e ->
          eprintln "csv2jsonl: \{e}"
          exit 1
        | Ok text ->
          let result =
            use catching
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
