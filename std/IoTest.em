-- 実行系のテストのための、driver の操作の口 (docs/spec/stdlib.md の「`Std.IoTest`」)。利用者の API ではなく、断りなく
-- 変わる。待つ関数は、受け付けから取り出しまでを1つの呼び出しの中で行い、token の `OpId` を外へ出さない
import Async (Async)

-- driver の `pair` の片方の端を中身に持つ。Kind は `Lin` で、落とすと端を閉じる
pub extern data Stream

-- つながった2つの端を作る。タスクの外でも動く
pub pair : Unit -> <IO> (Stream, Stream)
pair () = peer (open_pair ())

-- `b` を書き、書いたバイトの数を返す。本物の driver では短いことがある。取り消しの地点である
pub write : Bytes -> Stream -> <IO, Async> (Int, Stream)
write b s =
  if Async.cancel_requested () then
    drop s
    raise_cancelled ()
  else
    let (w, op) = Async.io_wait (submit_write b s)
    let (s, op) = op_stream op
    let n = take_write op
    match w with
      | Async.Woke -> (n, s)
      | Async.Cancelled ->
        drop s
        raise_cancelled ()

-- `Std.Async` の取り消しの地点と同じ失敗 (docs/spec/stdlib.md の「取り消し」)
raise_cancelled : Unit -> <IO> a
raise_cancelled () = fail_io Prelude.Cancelled "task was cancelled"

-- ランタイムの口。受け付けの行は buffer とストリームを操作の記録に移して token を返し、`op_stream` は終わった記録から
-- ストリームを取り出し、取り出しの行は token を消費して記録を放す (docs/spec/vm.md の「`Async` のタスク」)
extern open_pair    : Unit -> <IO> Stream
extern peer         : Stream -> <IO> (Stream, Stream)
extern submit_write : Bytes -> Stream -> <IO> Async.OpId
extern op_stream    : Async.OpId -> <IO> (Stream, Async.OpId)
extern take_write   : Async.OpId -> <IO> Int
