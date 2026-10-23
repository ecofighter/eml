-- 子のプロセスを起動する標準モジュール (docs/spec/stdlib.md の「`Proc`」)。`Cmd` は Prelude の型で、コマンドリテラルと
-- このモジュールの設定の関数で組む。子はシェルを介さずに起動する
import String
import Bytes
import List

pub extern data Child

-- 引数は、プログラム、引数の列、環境の列、作業ディレクトリ (空なら設定なし) と、標準入力、標準出力、標準エラーの
-- 向きである。列は各要素を「バイト長の十進、NUL、本文」の順に並べたものである。向きの値は `spawn_with` が決める
extern spawn_raw : String -> String -> String -> String -> Int -> Int -> Int -> <IO> Child
-- いつも ``command `<program>` exited with code <code>`` の IO の失敗で終わる。eml から `io_failed` を呼べないので、
-- `capture` が失敗を起こす窓口である
extern exited : String -> Int -> <IO> a

-- 作業ディレクトリ。相対パスは `FileIn.open` と同じ基準である
pub cwd : String -> Cmd -> Cmd
cwd dir c = { c | cwd = Some dir }

-- 環境変数を1つ上書きする。同じ名前を2回設定したら、後の値が勝つ
pub env : String -> String -> Cmd -> Cmd
env name value c = { c | env = List.append c.env [(name, value)] }

pub arg : String -> Cmd -> Cmd
arg a c = { c | args = List.append c.args [a] }

pub args : List String -> Cmd -> Cmd
args more c = { c | args = List.append c.args more }

-- 標準エラーを標準出力と同じ先に向ける (`2>&1`)
pub merge_stderr : Cmd -> Cmd
merge_stderr c = { c | stderr = MergeStderr }

-- 標準エラーを捨てる (`2>/dev/null`)
pub discard_stderr : Cmd -> Cmd
discard_stderr c = { c | stderr = DiscardStderr }

-- 標準入出力を受け継いで起動し、終わるのを待って終了コードを返す
pub run : Cmd -> <IO> Int
run c = wait (spawn_with 0 0 c)

-- 標準出力を文字列に取り、終了コードと組で返す。標準入力は空である
pub output : Cmd -> <IO> (String, Int)
output c =
  let (out, child) = read_all (spawn_with 1 2 c)
  (out, wait child)

pub output_bytes : Cmd -> <IO> (Bytes, Int)
output_bytes c =
  let (out, child) = read_all_bytes (spawn_with 1 2 c)
  (out, wait child)

-- 終了コードが 0 でなければ IO の失敗である。誤りは、起動、読み、終了コードの順に、本体の順で出る
pub capture : Cmd -> <IO> String
capture c =
  let (out, child) = read_all (spawn_with 1 2 c)
  let code = wait child
  if code == 0 then out else exited c.program code

pub capture_bytes : Cmd -> <IO> Bytes
capture_bytes c =
  let (out, child) = read_all_bytes (spawn_with 1 2 c)
  let code = wait child
  if code == 0 then out else exited c.program code

-- 標準入力と標準出力を pipe にして起動する。読み書きの順の規則は docs/spec/stdlib.md の「`Proc`」にある
pub spawn : Cmd -> <IO> Child
spawn c = spawn_with 2 2 c

-- 向きの値は、標準入力と標準出力が 0 受け継ぐ、1 空、2 pipe である
spawn_with : Int -> Int -> Cmd -> <IO> Child
spawn_with stdin stdout c =
  spawn_raw c.program (fields c.args "") (env_fields c.env "") (dir c.cwd) stdin stdout (stderr_code c.stderr)

dir : Option String -> String
dir (Some d) = d
dir None = ""

stderr_code : StderrMode -> Int
stderr_code InheritStderr = 0
stderr_code MergeStderr = 1
stderr_code DiscardStderr = 2

-- 長さを先に置くので、本文が NUL を含んでも区切りを取り違えない
field : String -> String -> String
field s acc = acc ++ "\{String.byte_length s}\0" ++ s

fields : List String -> String -> String
fields Nil acc = acc
fields (s :: rest) acc = fields rest (field s acc)

env_fields : List (String, String) -> String -> String
env_fields Nil acc = acc
env_fields ((k, v) :: rest) acc = env_fields rest (field v (field k acc))

-- 子の標準入力に書く。`FileOut` と同じく、書けなければハンドルを手放して失敗する
pub extern write : String -> Child -> <IO> Child
pub extern write_bytes : Bytes -> Child -> <IO> Child
pub extern write_line : String -> Child -> <IO> Child
-- 子の標準入力を閉じて、子に EOF を届ける
pub extern close_stdin : Child -> <IO> Child
-- 標準入力を閉じてから子を待ち、終了コードを返す。シグナルで終わったら 128 + 番号である
pub extern wait : Child -> <IO> Int

-- 子の標準出力を読む。形は `Stdin` の読みと同じである
pub extern read_chunk : Bytes -> Child -> <IO> (Bytes, Child)
pub extern read_all_bytes : Child -> <IO> (Bytes, Child)
pub extern read_all : Child -> <IO> (String, Child)
-- 行の終わりを含む1行で、終わりなら空である
extern read_line_bytes_raw : Child -> <IO> (Bytes, Child)
extern read_line_raw : Child -> <IO> (String, Child)

pub read_line : Child -> <IO, Panic> (Option String, Child)
read_line h =
  let (line, h) = read_line_raw h
  if String.byte_length line == 0 then (None, h) else (Some (String.trim_line_end line), h)

pub read_line_bytes : Child -> <IO, Panic> (Option Bytes, Child)
read_line_bytes h =
  let (line, h) = read_line_bytes_raw h
  if Bytes.length line == 0 then (None, h) else (Some (Bytes.trim_line_end line), h)
