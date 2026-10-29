-- 実行系のテストのための、driver の操作のインターフェース (docs/spec/stdlib.md の「`Std.IoTest`」)。利用者の API ではなく、断りなく
-- 変わる。待つ関数は受け付けから取り出しまでを1つの呼び出しの中で行い、token の `OpId` を外へ出さない
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

-- `s` を1回読み、読んだバイトを `b` の後ろに足して返す。EOF では何も足さない。取り消しの地点である
pub read_chunk : Bytes -> Stream -> <IO, Async> (Bytes, Stream)
read_chunk b s =
  if Async.cancel_requested () then
    drop s
    raise_cancelled ()
  else
    let (w, op) = Async.io_wait (submit_read b s)
    let (s, op) = op_stream op
    let r = take_read op
    match w with
      | Async.Woke -> (r, s)
      | Async.Cancelled ->
        drop s
        raise_cancelled ()

-- `read_chunk` に `ms` ミリ秒の期限を付ける。期限までに読めなければ `None` と端を返し、端は続けて使える。期限と
-- 競って届いたデータは `Some` で返す。取り消しの地点である (docs/spec/stdlib.md の「`Std.IoTest`」)
pub read_chunk_within : Int -> Bytes -> Stream -> <IO, Async> (Option Bytes, Stream)
read_chunk_within ms b s =
  if Async.cancel_requested () then
    drop s
    raise_cancelled ()
  else
    let (w, op) = Async.io_wait_until (submit_read b s) ms
    let (s, op) = op_stream op
    let (late, op) = op_timed_out op
    let r = take_read op
    match w with
      | Async.Woke -> if late then (None, s) else (Some r, s)
      | Async.Cancelled ->
        drop s
        raise_cancelled ()

-- `b` のバイトの和を、ブロックする呼び出しのプールの仕事で求める。仕事は `delay` ミリ秒眠ってから和を求める。遅れは
-- 本物の時計でだけ効き、0 から 1000 の間に収める。取り消しの地点である (docs/spec/stdlib.md の「`Std.IoTest`」)
pub checksum : Int -> Bytes -> <IO, Async> Int
checksum delay b =
  if Async.cancel_requested () then
    raise_cancelled ()
  else
    let (w, op) = Async.io_wait (submit_checksum delay b)
    let n = take_checksum op
    match w with
      | Async.Woke -> n
      | Async.Cancelled -> raise_cancelled ()

-- `Std.Async` の取り消しの地点と同じ失敗 (docs/spec/stdlib.md の「取り消し」)
raise_cancelled : Unit -> <IO> a
raise_cancelled () = fail_io Prelude.Cancelled "task was cancelled"

-- ランタイムのインターフェース。受け付けの行は buffer とストリームを操作の記録に移して token を返し、`op_stream` は終わった記録から
-- ストリームを取り出し、`op_timed_out` は終わった記録が期限で誤りになったかを読み、取り出しの行は token を消費して
-- 記録を放す。`submit_checksum` は buffer の中身を写して仕事に渡す (docs/spec/vm.md の「`Async` のタスク」)
extern open_pair       : Unit -> <IO> Stream
extern peer            : Stream -> <IO> (Stream, Stream)
extern submit_write    : Bytes -> Stream -> <IO> Async.OpId
extern op_stream       : Async.OpId -> <IO> (Stream, Async.OpId)
extern take_write      : Async.OpId -> <IO> Int
extern submit_read     : Bytes -> Stream -> <IO> Async.OpId
extern take_read       : Async.OpId -> <IO> Bytes
extern op_timed_out    : Async.OpId -> <IO> (Bool, Async.OpId)
extern submit_checksum : Int -> Bytes -> <IO> Async.OpId
extern take_checksum   : Async.OpId -> <IO> Int
