-- CSV の欄と行を読む状態機械を、1つの `let … and …` の組で書く。欄の始め、引用符のない欄、引用符のある欄、
-- 引用符のある欄の中の引用符、次の行の5つの状態を関数にし、組の関数はどれも入力の `s` と長さ `n` を捕まえる。
-- 引用符のある欄はコンマと、2つ続けた引用符を含む (docs/spec/expressions.md の「ローカル関数」)。
import List
import String

quote_byte : Int
quote_byte = 34

comma_byte : Int
comma_byte = 44

newline_byte : Int
newline_byte = 10

parse : String -> <Panic> List (List String)
parse s =
  let n = String.byte_length s
  let start i row rows =
    if i == n then List.reverse (List.reverse ("" :: row) :: rows)
    else if String.byte_at i s == quote_byte then quoted (i + 1) (i + 1) "" row rows
    else plain i i row rows
  and plain i begin row rows =
    if i == n then List.reverse (List.reverse (String.slice begin i s :: row) :: rows)
    else
      let b = String.byte_at i s
      if b == comma_byte then start (i + 1) (String.slice begin i s :: row) rows
      else if b == newline_byte then next_row (i + 1) (String.slice begin i s :: row) rows
      else plain (i + 1) begin row rows
  and quoted i begin text row rows =
    if i == n then List.reverse (List.reverse ((text ++ String.slice begin i s) :: row) :: rows)
    else if String.byte_at i s == quote_byte then closing (i + 1) (text ++ String.slice begin i s) row rows
    else quoted (i + 1) begin text row rows
  and closing i text row rows =
    if i == n then List.reverse (List.reverse (text :: row) :: rows)
    else
      let b = String.byte_at i s
      -- 2つ続けた引用符は1つの引用符で、欄の中身は位置 `i` の引用符から続く
      if b == quote_byte then quoted (i + 1) i text row rows
      else if b == comma_byte then start (i + 1) (text :: row) rows
      else if b == newline_byte then next_row (i + 1) (text :: row) rows
      else closing (i + 1) text row rows
  and next_row i row rows =
    let done = List.reverse row :: rows
    if i == n then List.reverse done else start i [] done
  if n == 0 then [] else start 0 [] []

print_rows : String -> <IO, Panic> Unit
print_rows text =
  List.each (fn row -> println (String.join "|" row)) (parse text)
  println "--"

main : Unit -> <IO, Panic> Unit
main () =
  print_rows "name,age,quote\nAda,36,\"Hello, world\"\n\"Bob \"\"B\"\" Ross\",52,\"\"\n"
  print_rows "last,,x\n\"a,b\",\"c\"\"\",d\ntrailing,"
  print_rows "\"unterminated, still one field"
  println (show (parse "x,\"y\"\"z\"\n1,2"))
