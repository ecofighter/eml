-- 子のプロセスを driver の上で起動して待つモジュール (docs/spec/stdlib.md の「`Std.Exec`」)。`Cmd` は `Std.Proc` と
-- 同じものを使い、起動の引数は `Proc.launch_args` が作る。待つ関数は `Std.Stream` と同じく、受け付けから取り出し
-- までを1つの呼び出しの中で行い、token の `OpId` を外へ出さない
import Async (Async)
import Bytes
import List
import Proc
import Stream

-- 段ごとの子のプロセスと、まだ取り出していない標準入力、標準出力、標準エラーの pipe の端を中身に持つ。Kind は `Lin`
-- で、落とすと取り出していない端を閉じ、まだ待っていない段を kill してから待つ
pub extern data Child

-- 標準入力と標準出力を pipe にして起動する。端は `stdin` と `stdout` で取り出す。最後の段に `pipe_stderr` を
-- 設定していれば、標準エラーも pipe にして `stderr` で取り出す。取り消しの地点である
pub spawn : Cmd -> <IO, Async> Child
spawn c = launch True 2 2 c

-- 標準入出力を受け継いで起動し、すべての段を待って最後の段の終了コードを返す。取り消しの地点である
pub run : Cmd -> <IO, Async> Int
run c = wait (launch False 0 0 c)

-- 標準出力を終わりまで読んで文字列にし、待った終了コードと組で返す。標準入力は空である。標準出力をリダイレクトした
-- ときは、`Proc.output` と同じく何も読まずに空の文字列を返す
pub output : Cmd -> <IO, Async> (String, Int)
output c =
  let ch = launch False 1 2 c
  if c.stdout == DefaultStdout then
    let (r, ch) = stdout ch
    let (s, r) = Stream.read_all r
    Stream.close r
    (s, wait ch)
  else ("", wait ch)

pub output_bytes : Cmd -> <IO, Async> (Bytes, Int)
output_bytes c =
  let ch = launch False 1 2 c
  if c.stdout == DefaultStdout then
    let (r, ch) = stdout ch
    let (b, r) = Stream.read_all_bytes r
    Stream.close r
    (b, wait ch)
  else (Bytes.empty (), wait ch)

-- 終了コードが 0 でなければ、`Proc.capture` と同じ IO の失敗である
pub capture : Cmd -> <IO, Async> String
capture c =
  let (out, code) = output c
  if code == 0 then out else exited c.program code

pub capture_bytes : Cmd -> <IO, Async> Bytes
capture_bytes c =
  let (out, code) = output_bytes c
  if code == 0 then out else exited c.program code

exited : String -> Int -> <IO> a
exited program code = fail_io Other "command `\{program}` exited with code \{show code}"

-- `DefaultStdin` と `DefaultStdout` を向き `stdin` と `stdout` にして起動する。`pipe_stderr` は最後の段の
-- `PipeStderr` を許すかである。取り消しが立っていれば起動しない
launch : Bool -> Int -> Int -> Cmd -> <IO, Async> Child
launch pipe_stderr stdin stdout c =
  if Async.cancel_requested () then raise_cancelled ()
  else
    let (stages, si, payload, so, path) = Proc.launch_args stdin stdout pipe_stderr c
    spawn_raw stages si payload so path

-- 最後の段の標準エラーを pipe にする。使えるのは `spawn` だけで、ほかの起動の形と `Proc` では起動の誤りである
pub pipe_stderr : Cmd -> Cmd
pipe_stderr c = { c | stderr = PipeStderr }

-- 標準入力を閉じてから、すべての段を先頭から待ち、最後の段の終了コードを返す。シグナルで終わった段は 128 + 番号で
-- ある。取り消しの地点である
pub wait : Child -> <IO, Async> Int
wait c = last_code (wait_all c)

last_code : List Int -> Int
last_code (code :: Nil) = code
last_code (_ :: rest) = last_code rest
last_code Nil = 0

-- すべての段の終了コードを先頭から並べる。`wait` と同じく、すべての段を先頭から待つ。取り消しの地点である
pub wait_all : Child -> <IO, Async> List Int
wait_all c =
  let (n, c) = stage_count c
  wait_from 0 n c Nil

wait_from : Int -> Int -> Child -> List Int -> <IO, Async> List Int
wait_from k n c acc =
  if k == n then
    drop c
    List.reverse acc
  else
    let (code, c) = wait_stage k c
    wait_from (next_stage k n) n c (code :: acc)

-- 段の数は小さく、`+` は桁あふれしない。型から `Panic` を除くために `try_panic` で包み、届かない `Err` には段の数
-- `n` を返して、待つ繰り返しを終える
next_stage : Int -> Int -> Int
next_stage k n =
  match try_panic (fn () -> k + 1) with
    | Ok next -> next
    | Err _ -> n

-- 段 `k` の終わりを driver で待ってから、その段を回収して終了コードを返す
wait_stage : Int -> Child -> <IO, Async> (Int, Child)
wait_stage k c =
  if Async.cancel_requested () then
    drop c
    raise_cancelled ()
  else
    let (w, op) = Async.io_wait (submit_wait k c)
    let (c, op) = op_child op
    let (code, op) = op_outcome op
    match w with
      | Async.Cancelled ->
        finish op
        drop c
        raise_cancelled ()
      | Async.Woke ->
        if code == 0 then
          finish op
          reap k c
        else
          let (prefix, op) = op_prefix op
          finish op
          drop c
          fail_code code prefix

-- 標準入力の pipe の書く端と、最後の段の標準出力と標準エラーの pipe の読む端を取り出す。2回目と、pipe にしなかった
-- 端は `NotAvailable` の失敗である
pub extern stdin  : Child -> <IO> (Stream.Writer, Child)
pub extern stdout : Child -> <IO> (Stream.Reader, Child)
pub extern stderr : Child -> <IO> (Stream.Reader, Child)

-- `Std.Async` の取り消しの地点と同じ失敗 (docs/spec/stdlib.md の「取り消し」)
raise_cancelled : Unit -> <IO> a
raise_cancelled () = fail_io Prelude.Cancelled "task was cancelled"

-- `op_outcome` の 2 から 5 の結果を、種類を決めた IO の失敗にする。ほかの誤りは `finish` が投げる。2 の文言は
-- `Std.Stream` と同じく `broken pipe` である
fail_code : Int -> String -> <IO> a
fail_code code prefix =
  if code == 2 then fail_io ConnectionReset "\{prefix}: broken pipe"
  else if code == 3 then fail_io ConnectionRefused "\{prefix}: connection refused"
  else if code == 4 then fail_io AddrInUse "\{prefix}: address in use"
  else fail_io TimedOut "\{prefix}: timed out"

-- ランタイムのインターフェース。`spawn_raw` の引数は `Std.Proc.spawn_raw` と同じである。`submit_wait` は段 `k` の
-- 終わりを待つ記録を作り、`reap` はその知らせの後に段 `k` を回収して終了コードを返す。ほかの行の形と意味は
-- `Std.Stream` の同じ名前の行と同じである (docs/spec/vm.md の「操作の記録」)
extern spawn_raw   : String -> Int -> String -> Int -> String -> <IO> Child
extern stage_count : Child -> (Int, Child)
extern submit_wait : Int -> Child -> <IO> Async.OpId
extern op_child    : Async.OpId -> <IO> (Child, Async.OpId)
extern op_outcome  : Async.OpId -> <IO> (Int, Async.OpId)
extern op_prefix   : Async.OpId -> <IO> (String, Async.OpId)
extern finish      : Async.OpId -> <IO> Unit
extern reap        : Int -> Child -> <IO> (Int, Child)
