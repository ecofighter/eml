-- TCP のモジュール (docs/spec/stdlib.md の「`Std.Net`」)。待つ関数は受け付けから取り出しまでを1つの呼び出しの中で
-- 行い、token の `OpId` を外へ出さない
import Async (Async)

-- driver のリスナーのハンドルと待ち受けるポートを中身に持つ。Kind は `Lin` で、落とすとソケットを閉じる
pub extern data TcpListener

-- driver のストリームのハンドルと相手の番地を中身に持つ。Kind は `Lin` で、落とすとソケットを閉じる
pub extern data TcpStream

-- IP アドレスの綴り `host` の `port` で接続を待ち受ける。`port` が 0 なら空いているポートを使う。取り消しの地点である
pub listen : String -> Int -> <IO, Async> TcpListener
listen host port =
  if Async.cancel_requested () then
    raise_cancelled ()
  else
    let (w, op) = Async.io_wait (submit_listen host port)
    let (code, op) = op_outcome op
    match w with
      | Async.Cancelled ->
        finish op
        raise_cancelled ()
      | Async.Woke ->
        if code == 0 then take_listener op
        else
          finish op
          fail_code code "cannot listen on \{host}:\{show port}"

-- リスナーに来た接続を1つ受け、そのストリームとリスナーを返す。取り消しの地点である
pub accept : TcpListener -> <IO, Async> (TcpStream, TcpListener)
accept l =
  if Async.cancel_requested () then
    drop l
    raise_cancelled ()
  else
    let (w, op) = Async.io_wait (submit_accept l)
    let (l, op) = op_listener op
    let (code, op) = op_outcome op
    match w with
      | Async.Cancelled ->
        finish op
        drop l
        raise_cancelled ()
      | Async.Woke ->
        if code == 0 then (take_stream op, l)
        else
          finish op
          drop l
          fail_code code "cannot accept a connection"

-- `accept` に `ms` ミリ秒の期限を付ける。期限が過ぎれば `None` とリスナーを返し、リスナーは続けて使える。取り消しの
-- 地点である (docs/spec/stdlib.md の「`Std.Net`」)
pub accept_within : Int -> TcpListener -> <IO, Async> (Option TcpStream, TcpListener)
accept_within ms l =
  if Async.cancel_requested () then
    drop l
    raise_cancelled ()
  else
    let (w, op) = Async.io_wait_until (submit_accept l) ms
    let (l, op) = op_listener op
    let (code, op) = op_outcome op
    match w with
      | Async.Cancelled ->
        finish op
        drop l
        raise_cancelled ()
      | Async.Woke ->
        if code == 0 then (Some (take_stream op), l)
        else if code == 1 then
          finish op
          (None, l)
        else
          finish op
          drop l
          fail_code code "cannot accept a connection"

-- `host` (IP アドレスの綴りか名前) の `port` へ接続する。名前は driver が解決し、得た番地を順に試す。取り消しの地点である
pub connect : String -> Int -> <IO, Async> TcpStream
connect host port =
  if Async.cancel_requested () then
    raise_cancelled ()
  else
    let (w, op) = Async.io_wait (submit_connect host port)
    let (code, op) = op_outcome op
    match w with
      | Async.Cancelled ->
        finish op
        raise_cancelled ()
      | Async.Woke ->
        if code == 0 then take_stream op
        else
          finish op
          fail_code code "cannot connect to \{host}:\{show port}"

pub close : TcpStream -> <IO> Unit
close s = drop s

pub close_listener : TcpListener -> <IO> Unit
close_listener l = drop l

-- `Std.Async` の取り消しの地点と同じ失敗 (docs/spec/stdlib.md の「取り消し」)
raise_cancelled : Unit -> <IO> a
raise_cancelled () = fail_io Prelude.Cancelled "task was cancelled"

-- `op_outcome` の 2 から 5 の結果を、種類を決めた IO の失敗にする。ほかの誤りは `finish` が投げる
fail_code : Int -> String -> <IO> a
fail_code code prefix =
  if code == 2 then fail_io ConnectionReset "\{prefix}: connection reset"
  else if code == 3 then fail_io ConnectionRefused "\{prefix}: connection refused"
  else if code == 4 then fail_io AddrInUse "\{prefix}: address in use"
  else fail_io TimedOut "\{prefix}: timed out"

-- 作ったときに記録したポートと相手の番地を読む。借りるだけで、効果を持たない
pub extern local_port : TcpListener -> (Int, TcpListener)
pub extern peer_addr  : TcpStream -> (String, TcpStream)

-- 送る側を閉じる。相手は残りを読みきってから EOF を受ける
pub extern shutdown_write : TcpStream -> <IO> TcpStream

-- TCP_NODELAY を設定する
pub extern set_nodelay : Bool -> TcpStream -> <IO> TcpStream

-- ランタイムのインターフェース。受け付けの行は引数を操作の記録に移して token を返す。`op_listener` と `op_stream` は
-- 終わった記録から受け付けに渡したものを取り戻し、`op_outcome` は終わった記録の結果の番号を読む。取り出しの行は結果を
-- 受け取り、`finish` は結果を受け取らずに、どちらも token を消費して記録を放す (docs/spec/vm.md の「操作の記録」)
extern submit_listen  : String -> Int -> <IO> Async.OpId
extern submit_accept  : TcpListener -> <IO> Async.OpId
extern submit_connect : String -> Int -> <IO> Async.OpId
extern op_listener    : Async.OpId -> <IO> (TcpListener, Async.OpId)
extern op_stream      : Async.OpId -> <IO> (TcpStream, Async.OpId)
extern op_outcome     : Async.OpId -> <IO> (Int, Async.OpId)
extern take_listener  : Async.OpId -> <IO> TcpListener
extern take_stream    : Async.OpId -> <IO> TcpStream
extern finish         : Async.OpId -> <IO> Unit
