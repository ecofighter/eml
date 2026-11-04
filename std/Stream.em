-- driver で読み書きするファイルと子の pipe のモジュール (docs/spec/stdlib.md の「`Std.Stream`」)。待つ関数は `Std.Net` と同じく、
-- 受け付けから取り出しまでを1つの呼び出しの中で行い、token の `OpId` を外へ出さない
import Async (Async)
import Bytes
import String

-- driver のファイルか子の pipe のハンドル、開いたパス (子の pipe ならプログラムの名前)、`read_line` が読みすぎたバイトを
-- 中身に持つ。Kind は `Lin` で、落とすとファイルか pipe の端を閉じる
pub extern data Reader

-- driver のファイルか子の pipe のハンドルと、開いたパス (子の pipe ならプログラムの名前) を中身に持つ。書きの buffer は
-- 持たない。Kind は `Lin` で、落とすとファイルか pipe の端を閉じる
pub extern data Writer

-- 通常のファイル `path` を読むために開く。取り消しの地点である
pub open : String -> <IO, Async> Reader
open path =
  if Async.cancel_requested () then
    raise_cancelled ()
  else
    let (w, op) = Async.io_wait (submit_open path 0)
    let (code, op) = op_outcome op
    match w with
      | Async.Cancelled ->
        finish op
        raise_cancelled ()
      | Async.Woke ->
        if code == 0 then take_reader op
        else
          let (prefix, op) = op_prefix op
          finish op
          fail_code code prefix

-- 通常のファイル `path` を作るか切り詰めて、書くために開く。取り消しの地点である
pub create : String -> <IO, Async> Writer
create path = open_writer 1 path

-- 通常のファイル `path` を作るか、末尾に足す形で書くために開く。取り消しの地点である
pub append : String -> <IO, Async> Writer
append path = open_writer 2 path

-- `create` と `append`。`mode` は `submit_open` の向きの番号である
open_writer : Int -> String -> <IO, Async> Writer
open_writer mode path =
  if Async.cancel_requested () then
    raise_cancelled ()
  else
    let (w, op) = Async.io_wait (submit_open path mode)
    let (code, op) = op_outcome op
    match w with
      | Async.Cancelled ->
        finish op
        raise_cancelled ()
      | Async.Woke ->
        if code == 0 then take_writer op
        else
          let (prefix, op) = op_prefix op
          finish op
          fail_code code prefix

-- `r` から 1 バイト以上 65 536 バイト以下を読んで `b` の後ろに足し、その `Bytes` と読み手を返す。EOF では何も足さない。
-- `b` が一意で空きが 8 バイト以上あれば、その空きの 65 536 バイトまでに読む。なければ受け付けで容量を広げる。
-- 取り消しの地点である
pub read_chunk : Bytes -> Reader -> <IO, Async> (Bytes, Reader)
read_chunk b r =
  if Async.cancel_requested () then
    drop r
    raise_cancelled ()
  else
    let (w, op) = Async.io_wait (submit_read b r)
    let (r, op) = op_reader op
    let (code, op) = op_outcome op
    match w with
      | Async.Cancelled ->
        finish op
        drop r
        raise_cancelled ()
      | Async.Woke ->
        if code == 0 then (take_bytes op, r)
        else
          let (prefix, op) = op_prefix op
          finish op
          drop r
          fail_code code prefix

-- `r` から1行を読み、終わりの改行 (`\n` か `\r\n`) を除いた行と読み手を返す。残りのない EOF なら `None` である。
-- 読みすぎたバイトは読み手の buffer に残り、次の読みが先に返す。取り消しの地点である
pub read_line : Reader -> <IO, Panic, Async> (Option String, Reader)
read_line r =
  if Async.cancel_requested () then
    drop r
    raise_cancelled ()
  else
    let (w, op) = Async.io_wait (submit_read_line r)
    let (r, op) = op_reader op
    let (code, op) = op_outcome op
    match w with
      | Async.Cancelled ->
        finish op
        drop r
        raise_cancelled ()
      | Async.Woke ->
        if code == 0 then (line_of (take_line op), r)
        else
          let (prefix, op) = op_prefix op
          finish op
          drop r
          fail_code code prefix

-- `take_line` の行を `Stdin.read_line` と同じ形にする。空の行は残りのない EOF である
line_of : String -> <Panic> Option String
line_of line = if String.byte_length line == 0 then None else Some (String.trim_line_end line)

-- `read_line` と同じ1行を、UTF-8 かどうかによらずバイト列で返す。取り消しの地点である
pub read_line_bytes : Reader -> <IO, Panic, Async> (Option Bytes, Reader)
read_line_bytes r =
  if Async.cancel_requested () then
    drop r
    raise_cancelled ()
  else
    let (w, op) = Async.io_wait (submit_read_line r)
    let (r, op) = op_reader op
    let (code, op) = op_outcome op
    match w with
      | Async.Cancelled ->
        finish op
        drop r
        raise_cancelled ()
      | Async.Woke ->
        if code == 0 then (bytes_line_of (take_line_bytes op), r)
        else
          let (prefix, op) = op_prefix op
          finish op
          drop r
          fail_code code prefix

-- `line_of` のバイト列の形
bytes_line_of : Bytes -> <Panic> Option Bytes
bytes_line_of line = if Bytes.length line == 0 then None else Some (Bytes.trim_line_end line)

-- 残りをすべて読む。1つの `Bytes` に足し続け、何も足さない読みで終える
pub read_all_bytes : Reader -> <IO, Async> (Bytes, Reader)
read_all_bytes r = read_rest (Bytes.empty ()) r

read_rest : Bytes -> Reader -> <IO, Async> (Bytes, Reader)
read_rest b r =
  let n = Bytes.length b
  let (b, r) = read_chunk b r
  if Bytes.length b == n then (b, r) else read_rest b r

-- 残りをすべて読み、UTF-8 の文字列にする。UTF-8 でなければ、読み手を落として `NotUtf8` の失敗にする
pub read_all : Reader -> <IO, Async> (String, Reader)
read_all r =
  let (b, r) = read_all_bytes r
  match String.from_utf8 b with
    | Some s -> (s, r)
    | None ->
      let (name, r) = source r
      drop r
      fail_io NotUtf8 "cannot read \{name}: the text is not valid UTF-8"

-- `b` をすべて書き、写さずに `b` そのものと書き手を返す。取り消しの地点である
pub write_bytes : Bytes -> Writer -> <IO, Async> (Bytes, Writer)
write_bytes b w =
  if Async.cancel_requested () then
    drop w
    raise_cancelled ()
  else
    let (wake, op) = Async.io_wait (submit_write b w)
    let (w, op) = op_writer op
    let (code, op) = op_outcome op
    match wake with
      | Async.Cancelled ->
        finish op
        drop w
        raise_cancelled ()
      | Async.Woke ->
        if code == 0 then (take_bytes op, w)
        else
          let (prefix, op) = op_prefix op
          finish op
          drop w
          fail_code code prefix

-- 文字列を UTF-8 のバイトで書く
pub write : String -> Writer -> <IO, Async> Writer
write s w =
  let (_, w) = write_bytes (Bytes.of_string s) w
  w

-- 文字列と改行を書く
pub write_line : String -> Writer -> <IO, Async> Writer
write_line s w = write "\{s}\n" w

pub close : Reader -> <IO> Unit
close r = drop r

-- 書きの buffer がないので、書き出すものはない
pub close_writer : Writer -> <IO> Unit
close_writer w = drop w

-- `Std.Async` の取り消しの地点と同じ失敗 (docs/spec/stdlib.md の「取り消し」)
raise_cancelled : Unit -> <IO> a
raise_cancelled () = fail_io Prelude.Cancelled "task was cancelled"

-- `op_outcome` の 2 から 5 の結果を、種類を決めた IO の失敗にする。ほかの誤りは `finish` が投げる。2 の文言は
-- `broken pipe` である (docs/spec/stdlib.md の「`Std.Stream`」)
fail_code : Int -> String -> <IO> a
fail_code code prefix =
  if code == 2 then fail_io ConnectionReset "\{prefix}: broken pipe"
  else if code == 3 then fail_io ConnectionRefused "\{prefix}: connection refused"
  else if code == 4 then fail_io AddrInUse "\{prefix}: address in use"
  else fail_io TimedOut "\{prefix}: timed out"

-- 読み手を開いたときの名前 (パスを引用符で囲んだもの。子の pipe では ``the output of `<program>` ``) を読む。借りるだけで、
-- 効果を持たない
extern source : Reader -> (String, Reader)

-- ランタイムのインターフェース。`submit_open` の向きは 0 が読み、1 が作る、2 が足すである。形と意味は `Std.Net` の同じ
-- 名前の行と同じで、`op_prefix` は終わった記録の誤りの文言の前に付ける語を読む (docs/spec/vm.md の「操作の記録」)
extern submit_open      : String -> Int -> <IO> Async.OpId
extern submit_read      : Bytes -> Reader -> <IO> Async.OpId
extern submit_read_line : Reader -> <IO> Async.OpId
extern submit_write     : Bytes -> Writer -> <IO> Async.OpId
extern op_reader        : Async.OpId -> <IO> (Reader, Async.OpId)
extern op_writer        : Async.OpId -> <IO> (Writer, Async.OpId)
extern op_outcome       : Async.OpId -> <IO> (Int, Async.OpId)
extern op_prefix        : Async.OpId -> <IO> (String, Async.OpId)
extern take_reader      : Async.OpId -> <IO> Reader
extern take_writer      : Async.OpId -> <IO> Writer
extern take_bytes       : Async.OpId -> <IO> Bytes
extern take_line        : Async.OpId -> <IO> String
extern take_line_bytes  : Async.OpId -> <IO> Bytes
extern finish           : Async.OpId -> <IO> Unit
