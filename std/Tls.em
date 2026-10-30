-- TLS のクライアント (docs/spec/stdlib.md の「`Std.Tls`」)。ソケットの I/O はすべて `Std.Net` が行い、ここでは
-- `Conn` と `TcpStream` の間でバイトを汲む。待ち、取り消し、期限は `Net` のもので、ここでは何も足さない
import Async (Async)
import Bytes
import Net
import String

-- rustls の接続と、復号してまだ返していない平文の列を持つ。I/O を持たない。Kind は `Lin` で、落とすとメモリだけを放す
pub extern data Conn

-- TLS の状態とソケットの組。落とすとソケットを閉じ、close_notify は送らない
pub data Stream =
  | Stream { conn : Conn, sock : Net.TcpStream }

-- `host` の `port` へ接続し、`host` を SNI と証明書の名前に使って handshake まで行う。根は既定の根だけである
pub connect : String -> Int -> <IO, Async> Stream
connect host port = connect_with_ca "" host port

-- `connect` に、根に足す CA の証明書 (PEM) を渡す。空の文字列なら既定の根だけを使う
pub connect_with_ca : String -> String -> Int -> <IO, Async> Stream
connect_with_ca ca host port =
  -- 名前と PEM の誤りは、接続を開く前に投げる
  let conn = client_raw host ca
  let sock = Net.connect host port
  handshake host conn sock

-- 既にある `TcpStream` の上で handshake する。`host` は SNI と証明書の名前で、根は既定の根だけである
pub wrap : String -> Net.TcpStream -> <IO, Async> Stream
wrap host sock =
  let conn = client_raw host ""
  handshake host conn sock

-- handshake の間、送るものを書き出しては読んで `Conn` に入れる。最後の書き出しがクライアントの Finished を送る
handshake : String -> Conn -> Net.TcpStream -> <IO, Async> Stream
handshake host conn sock =
  let prefix = "cannot complete the TLS handshake with \{host}"
  let (conn, sock) = flush conn sock
  let (st, conn) = status conn
  if st &&& handshaking == 0 then Stream { conn, sock }
  else if st &&& (peer_closed ||| truncated) != 0 then
    drop conn
    drop sock
    fail_io ConnectionReset "\{prefix}: the peer closed the connection"
  else
    let (conn, sock) = fill prefix conn sock
    handshake host conn sock

-- 平文を `b` の後ろに 1 バイト以上 65 536 バイト以下足して返す。相手が close_notify で閉じた後は何も足さない。
-- close_notify のない EOF は `ConnectionReset` の IO の失敗である。取り消しの地点である
pub read_chunk : Bytes -> Stream -> <IO, Async> (Bytes, Stream)
read_chunk b s =
  if Async.cancel_requested () then
    drop s
    raise_cancelled ()
  else
    let Stream { conn, sock } = s
    read_loop b conn sock

read_loop : Bytes -> Conn -> Net.TcpStream -> <IO, Async> (Bytes, Stream)
read_loop b conn sock =
  let before = Bytes.length b
  let (b, conn) = read_plain b conn
  if Bytes.length b > before then (b, Stream { conn, sock })
  else
    let (st, conn) = status conn
    if st &&& peer_closed != 0 then (b, Stream { conn, sock })
    else if st &&& truncated != 0 then truncated_eof conn sock
    else
      let (conn, sock) = fill read_prefix conn sock
      read_loop b conn sock

-- 1行を読み、終わりの改行を除いて返す。意味、上限、文言は `Net.read_line` と同じである。読みすぎた平文は `Conn` に
-- 残り、次の読みが先に返す。取り消しの地点である
pub read_line : Stream -> <IO, Panic, Async> (Option String, Stream)
read_line s =
  if Async.cancel_requested () then
    drop s
    raise_cancelled ()
  else
    let Stream { conn, sock } = s
    line_loop conn sock

line_loop : Conn -> Net.TcpStream -> <IO, Panic, Async> (Option String, Stream)
line_loop conn sock =
  let (code, conn) = line_ready conn
  if code == 0 then
    let (st, conn) = status conn
    if st &&& truncated != 0 then truncated_eof conn sock
    else
      let (conn, sock) = fill read_prefix conn sock
      line_loop conn sock
  else line_result code conn sock

-- `line_ready` の 0 でない番号を行か IO の失敗にする。1 は行 (残りのない EOF なら空)、2 は上限を超えた行、3 は UTF-8
-- でない行である
line_result : Int -> Conn -> Net.TcpStream -> <IO, Panic> (Option String, Stream)
line_result code conn sock =
  if code == 1 then
    let (line, conn) = take_line conn
    (line_of line, Stream { conn, sock })
  else
    drop conn
    drop sock
    if code == 2 then fail_io Other "\{read_prefix}: a line is longer than 1048576 bytes"
    else fail_io NotUtf8 "\{read_prefix}: the line is not valid UTF-8"

line_of : String -> <Panic> Option String
line_of line = if String.byte_length line == 0 then None else Some (String.trim_line_end line)

-- 平文を暗号にして、同じ呼び出しの中で書き出す
pub write_bytes : Bytes -> Stream -> <IO, Async> Stream
write_bytes b s =
  let Stream { conn, sock } = s
  let conn = write_plain b conn
  let (conn, sock) = flush conn sock
  Stream { conn, sock }

pub write : String -> Stream -> <IO, Async> Stream
write str s = write_bytes (Bytes.of_string str) s

pub write_line : String -> Stream -> <IO, Async> Stream
write_line str s = write_bytes (Bytes.of_string "\{str}\n") s

-- close_notify を書き出し、送る側を閉じる
pub shutdown_write : Stream -> <IO, Async> Stream
shutdown_write s =
  let Stream { conn, sock } = s
  let conn = send_close_notify conn
  let (conn, sock) = flush conn sock
  let sock = Net.shutdown_write sock
  Stream { conn, sock }

-- `shutdown_write` してから落とす。相手がもう閉じていれば close_notify は書けないので、書きの失敗は捨てる。取り消し
-- だけは取り消しのまま投げる
pub close : Stream -> <IO, Async> Unit
close s =
  match try_io (fn () -> drop (shutdown_write s)) with
    | Ok () -> ()
    | Err (IOError { kind = k, message = m }) -> if k == Prelude.Cancelled then fail_io k m else ()

-- 送るものがあれば、すべて書き出す
flush : Conn -> Net.TcpStream -> <IO, Async> (Conn, Net.TcpStream)
flush conn sock =
  let (st, conn) = status conn
  if st &&& has_outgoing == 0 then (conn, sock)
  else
    let (out, conn) = pull_tls conn
    let (_, sock) = Net.write_bytes out sock
    (conn, sock)

-- ソケットを1回読んで `Conn` に入れる。空の読みは EOF である
fill : String -> Conn -> Net.TcpStream -> <IO, Async> (Conn, Net.TcpStream)
fill prefix conn sock =
  let (b, sock) = Net.read_chunk (Bytes.empty ()) sock
  let conn = if Bytes.length b == 0 then push_eof conn else push_tls b conn
  checked prefix conn sock

-- TLS の誤りなら alert を書き出してから IO の失敗にし、同じ押し込みで復号した平文も返さない。誤りでなければ送るもの
-- (TLS 1.3 の KeyUpdate への応答など) を書き出す
checked : String -> Conn -> Net.TcpStream -> <IO, Async> (Conn, Net.TcpStream)
checked prefix conn sock =
  let (st, conn) = status conn
  if st &&& failed == 0 then flush conn sock
  else
    let (reason, conn) = error_message conn
    -- alert を書けなくても TLS の誤りのほうを報告する。取り消しだけは取り消しのまま投げる
    match try_io (fn () -> drop (flush conn sock)) with
      | Err (IOError { kind = k, message = m }) ->
        if k == Prelude.Cancelled then fail_io k m else fail_io Other "\{prefix}: \{reason}"
      | Ok () -> fail_io Other "\{prefix}: \{reason}"

truncated_eof : Conn -> Net.TcpStream -> <IO> a
truncated_eof conn sock =
  drop conn
  drop sock
  fail_io ConnectionReset "\{read_prefix}: the peer closed the connection without close_notify"

-- `Std.Net` と同じ取り消しの失敗 (docs/spec/stdlib.md の「取り消し」)
raise_cancelled : Unit -> <IO> a
raise_cancelled () = fail_io Prelude.Cancelled "task was cancelled"

read_prefix : String
read_prefix = "cannot read the stream"

-- `status` の印 (docs/spec/stdlib.md の「`Std.Tls`」)
handshaking : Int
handshaking = 1

has_outgoing : Int
has_outgoing = 2

peer_closed : Int
peer_closed = 4

truncated : Int
truncated = 8

failed : Int
failed = 16

-- ランタイムのインターフェース。`client_raw` のほかは投げず、TLS の誤りは `status` の 16 と `error_message` に残る
extern client_raw        : String -> String -> <IO> Conn
extern push_tls          : Bytes -> Conn -> <IO> Conn
extern push_eof          : Conn -> <IO> Conn
extern pull_tls          : Conn -> <IO> (Bytes, Conn)
extern write_plain       : Bytes -> Conn -> <IO> Conn
extern read_plain        : Bytes -> Conn -> <IO> (Bytes, Conn)
extern status            : Conn -> <IO> (Int, Conn)
extern error_message     : Conn -> <IO> (String, Conn)
extern send_close_notify : Conn -> <IO> Conn
extern line_ready        : Conn -> <IO> (Int, Conn)
extern take_line         : Conn -> <IO> (String, Conn)
