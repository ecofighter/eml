-- 子のプロセスを起動する標準モジュール (docs/spec/stdlib.md の「`Proc`」)。`Cmd` は Prelude の型で、コマンドリテラルと
-- このモジュールの設定の関数で組む。子はシェルを介さずに起動する
import String
import Bytes
import List

pub extern data Child

-- 左の `Cmd` の標準出力を、右の `Cmd` の先頭の段の標準入力につなぐ (docs/spec/stdlib.md の「`Cmd`」)。`|>` より強く結ぶ
pub infixl 2 .|

-- 引数は `launch_args` の5つ (段の列、標準入力の向きと中身、標準出力の向きとパス) である。列は各要素を「バイト長の
-- 十進、NUL、本文」の順に並べたものである
extern spawn_raw : String -> Int -> String -> Int -> String -> <IO> Child
-- 0 でない終了コードを、種類が `Other` の IO の失敗にする (docs/spec/stdlib.md の「`Proc`」)
exited : String -> Int -> <IO> a
exited program code = fail_io Other "command `\{program}` exited with code \{show code}"

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

-- 入力の設定 (`.|`、`stdin_text`、`stdin_file`、`no_stdin`) はパイプラインの先頭の段に当て、ほかの設定は最後の段に
-- 当てる (docs/spec/stdlib.md の「`Cmd`」)
pub (.|) : Cmd -> Cmd -> Cmd
left .| right = to_head (StdinFrom left) right

-- 標準入力を文字列 `s` にする。子には、`s` を入れた名前のない一時ファイルとして見える
pub stdin_text : String -> Cmd -> Cmd
stdin_text s c = to_head (StdinText s) c

-- 標準入力をファイルから読ませる (`< path`)。相対パスは `FileIn.open` と同じ基準である
pub stdin_file : String -> Cmd -> Cmd
stdin_file path c = to_head (StdinFile path) c

-- 標準入力を空にする (`< /dev/null`)
pub no_stdin : Cmd -> Cmd
no_stdin c = to_head NullStdin c

-- `StdinFrom` の鎖をたどり、先頭の段の標準入力を `mode` に置き換える
to_head : StdinMode -> Cmd -> Cmd
to_head mode c = { c | stdin = head_stdin mode c.stdin }

head_stdin : StdinMode -> StdinMode -> StdinMode
head_stdin mode (StdinFrom prev) = StdinFrom (to_head mode prev)
head_stdin mode _ = mode

-- 標準出力をファイルに書かせる (`> path`)。ファイルは作るか切り詰める
pub stdout_file : String -> Cmd -> Cmd
stdout_file path c = { c | stdout = StdoutFile path }

-- 標準出力をファイルの末尾に足させる (`>> path`)
pub stdout_append : String -> Cmd -> Cmd
stdout_append path c = { c | stdout = StdoutAppend path }

-- 標準入出力を受け継いで起動し、終わるのを待って終了コードを返す。パイプラインなら最後の段の終了コードである
pub run : Cmd -> <IO> Int
run c = wait (spawn_from 0 0 c)

-- 標準出力を文字列に取り、終了コードと組で返す。標準入力は空である。標準出力をリダイレクトしたときは何も読まず、
-- 空の文字列を返す
pub output : Cmd -> <IO> (String, Int)
output c =
  let child = spawn_from 1 2 c
  if piped c then
    let (out, child) = read_all child
    (out, wait child)
  else ("", wait child)

pub output_bytes : Cmd -> <IO> (Bytes, Int)
output_bytes c =
  let child = spawn_from 1 2 c
  if piped c then
    let (out, child) = read_all_bytes child
    (out, wait child)
  else (Bytes.empty (), wait child)

-- 終了コードが 0 でなければ IO の失敗である。誤りは、起動、読み、終了コードの順に、本体の順で出る
pub capture : Cmd -> <IO> String
capture c =
  let (out, code) = output c
  if code == 0 then out else exited c.program code

pub capture_bytes : Cmd -> <IO> Bytes
capture_bytes c =
  let (out, code) = output_bytes c
  if code == 0 then out else exited c.program code

-- 標準入力と標準出力を pipe にして起動する。読み書きの順の規則は docs/spec/stdlib.md の「`Proc`」にある
pub spawn : Cmd -> <IO> Child
spawn c = spawn_from 2 2 c

-- `DefaultStdin` と `DefaultStdout` を、起動の形の向き `stdin` と `stdout` にして起動する
spawn_from : Int -> Int -> Cmd -> <IO> Child
spawn_from stdin stdout c =
  let (stages, si, payload, so, path) = launch_args stdin stdout False c
  spawn_raw stages si payload so path

-- `output` と `capture` が標準出力を読むか
piped : Cmd -> Bool
piped c = c.stdout == DefaultStdout

-- `Std.Exec` のためのインターフェースで、`Std.Net` のための `io_wait` と同じ理由で `pub` である。`Cmd` を
-- `spawn_raw` の5つの引数 (段の列、標準入力の向きと中身、標準出力の向きとパス) にする。`DefaultStdin` と
-- `DefaultStdout` は、渡した `stdin` と `stdout` の向きにする。向きの値は、標準入力が 0 受け継ぐ、1 空、2 pipe、
-- 3 文字列、4 ファイル、標準出力が 0 受け継ぐ、1 空、2 pipe、3 ファイルに書く、4 ファイルに足す、である。`Bool` は
-- 最後の段が標準エラーを pipe にしてよいかで、今は使わない。最後でない段の標準出力を設定していれば、起動の誤りにする
pub launch_args : Int -> Int -> Bool -> Cmd -> <IO> (String, Int, String, Int, String)
launch_args stdin stdout _ c =
  let (si, payload) = stdin_code stdin c.stdin
  let (so, path) = stdout_code stdout c.stdout
  (stages (chain c Nil) "", si, payload, so, path)

-- パイプラインの段を先頭から並べる
chain : Cmd -> List Cmd -> List Cmd
chain c acc = chain_from c.stdin (c :: acc)

chain_from : StdinMode -> List Cmd -> List Cmd
chain_from (StdinFrom prev) acc = chain prev acc
chain_from _ acc = acc

-- 段の列。段ごとに、プログラム、引数の列、環境の列、作業ディレクトリ (空なら設定なし)、標準エラーの向きを並べる。
-- 最後でない段の標準出力は次の段への pipe なので、ほかの向きは黙って捨てずに誤りにする
stages : List Cmd -> String -> <IO> String
stages Nil acc = acc
stages (c :: Nil) acc = stage c acc
stages (c :: rest) acc =
  if c.stdout == DefaultStdout then stages rest (stage c acc)
  else fail_io Other "cannot run `\{c.program}`: the standard output of a stage that is not last is redirected"

stage : Cmd -> String -> String
stage c acc =
  fields [c.program, fields c.args "", env_fields c.env "", dir c.cwd, stderr_code c.stderr] acc

-- 標準入力の向きは先頭の段のものである
stdin_code : Int -> StdinMode -> (Int, String)
stdin_code d DefaultStdin = (d, "")
stdin_code _ NullStdin = (1, "")
stdin_code _ (StdinText s) = (3, s)
stdin_code _ (StdinFile path) = (4, path)
stdin_code d (StdinFrom prev) = stdin_code d prev.stdin

stdout_code : Int -> StdoutMode -> (Int, String)
stdout_code d DefaultStdout = (d, "")
stdout_code _ NullStdout = (1, "")
stdout_code _ (StdoutFile path) = (3, path)
stdout_code _ (StdoutAppend path) = (4, path)

dir : Option String -> String
dir (Some d) = d
dir None = ""

stderr_code : StderrMode -> String
stderr_code InheritStderr = "0"
stderr_code MergeStderr = "1"
stderr_code DiscardStderr = "2"

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
-- 標準入力を閉じてから、すべての段を先頭から待ち、最後の段の終了コードを返す。シグナルで終わったら 128 + 番号である
pub extern wait : Child -> <IO> Int

-- すべての段の終了コードを先頭から並べる。`wait` と同じく、すべての段を先頭から待つ
pub wait_all : Child -> <IO> List Int
wait_all c =
  let (n, c) = stage_count c
  wait_from 0 n c Nil

wait_from : Int -> Int -> Child -> List Int -> <IO> List Int
wait_from k n c acc =
  let next = next_stage k n
  if next == n then List.reverse (wait c :: acc)
  else
    let (code, c) = wait_stage k c
    wait_from next n c (code :: acc)

-- 段の数は小さく、`+` は桁あふれしない。型から `Panic` を除くために `try_panic` で包み、届かない `Err` には段の数
-- `n` を返して、待つ繰り返しを終える
next_stage : Int -> Int -> Int
next_stage k n =
  match try_panic (fn () -> k + 1) with
    | Ok next -> next
    | Err _ -> n

extern stage_count : Child -> (Int, Child)
-- 標準入力を閉じ、段 0 から段 `k` までを先頭から待ち、段 `k` の終了コードを返す
extern wait_stage : Int -> Child -> <IO> (Int, Child)

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
