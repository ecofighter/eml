-- タスクがシグナルを待つモジュール (docs/spec/stdlib.md の「`Std.Signal`」)。`Async` の操作は外のモジュールから
-- perform できないので、待ちは `Std.Async` の `signal_wait` を通す (docs/spec/effects.md の「抽象エフェクト」)
import Async (Async)

-- `Interrupt` は Ctrl-C (unix の SIGINT と Windows の `CTRL_C_EVENT`)、`Terminate` は unix の SIGTERM と、`eml run` での
-- Windows のコンソールを閉じることとシャットダウンである
pub data Signal =
  | Interrupt
  | Terminate
  deriving (Eq, Show)

-- シグナル `s` が来るまで待つ。シグナルは貯めないので、待ち始める前に来たものは届かない。取り消しの地点である
pub wait : Signal -> <IO, Async> Unit
wait s =
  match Async.signal_wait (code s) with
    | Async.Woke -> ()
    | Async.Cancelled -> raise_cancelled ()

-- `Std.Async.signal_wait` に渡す番号
code : Signal -> Int
code s = match s with
  | Interrupt -> 0
  | Terminate -> 1

raise_cancelled : Unit -> <IO> a
raise_cancelled () = fail_io Prelude.Cancelled "task was cancelled"
