# S1 appendix r2: sources and expected snapshots for stage std-http

Every file below ran in a scratch copy of the repository at 9bb8963 with regenerated fixtures: `cargo test -p eml_cli --test integration -- ui:: citations::` passed (checked and fast modes), and so did `cargo test -p eml_tls -p eml_hir -p eml_core_ir`. Code comments are drafts; apply the natural-japanese skill before committing.

## A.1 std/Http.em

```haskell
-- HTTP/1.1 のクライアント (docs/spec/stdlib.md の「`Std.Http`」)。I/O はすべて `Std.Net` と `Std.Tls` が行い、
-- 待ち、取り消し、期限はそのまま使う。要求ごとに接続を開き、応答を読み終えたら閉じる
import Async (Async)
import Byte
import Bytes
import Char
import List
import Net
import String
import Tls

pub data Options =
  | Options { headers : List (String, String), ca : String, max_redirects : Int }
  deriving (Eq, Show)

pub data Response =
  | Response { status : Int, headers : List (String, String), body : Bytes }
  deriving (Eq, Show)

pub defaults : Options
defaults = Options { headers = [], ca = "", max_redirects = 5 }

pub get : String -> <IO, Panic, Async> Response
get url = request "GET" url (Bytes.empty ()) defaults

-- 接続の前に、メソッド、ヘッダー、URL の順に確かめる
pub request : String -> String -> Bytes -> Options -> <IO, Panic, Async> Response
request method url body options =
  if not (is_token method) then fail_io Other "invalid request: the method \{method} is not valid"
  else
    check_headers options.headers
    match parse_url url with
      | Err reason -> fail_io Other "invalid URL \{url}: \{reason}"
      | Ok u ->
        let limit = if options.max_redirects < 0 then 0 else options.max_redirects
        follow method u body options.headers limit limit options.ca

check_headers : List (String, String) -> <IO, Panic> Unit
check_headers hs =
  match hs with
    | Nil -> ()
    | (name, value) :: rest ->
      if not (is_token name) || not (is_field_value value) then
        fail_io Other "invalid request: the header \{name} is not valid"
      else if List.contains (lower name) reserved then
        fail_io Other "invalid request: the header \{name} is set by Std.Http"
      else check_headers rest

-- 本文の区切りと接続の扱いを決めるヘッダーは、呼び出し側に書かせない
reserved : List String
reserved = ["host", "connection", "content-length", "transfer-encoding"]

-- host は小文字にしてある。target は送る path と query で、`/` で始まる
data Url =
  | Url { secure : Bool, host : String, port : Int, target : String }

parse_url : String -> <Panic> Result String Url
parse_url text =
  -- fragment は送らない
  let s = match String.find "#" text with
    | Some i -> String.slice 0 i text
    | None -> text
  if not (all_bytes (fn b -> b >= '!'u8 && b <= '~'u8) s) then
    Err "the URL contains a space, a control or a non-ASCII character"
  else
    match String.find "://" s with
      | None -> Err "the scheme is not http or https"
      | Some i ->
        let scheme = lower (String.slice 0 i s)
        if scheme != "http" && scheme != "https" then Err "the scheme is not http or https"
        else
          let len = String.byte_length s
          let stop = authority_end s (i + 3) len
          authority (scheme == "https") (String.slice (i + 3) stop s) (target_of (String.slice stop len s))

authority_end : String -> Int -> Int -> <Panic> Int
authority_end s i len =
  if i >= len then len
  else
    let b = String.byte_at i s
    if b == '/'u8 || b == '?'u8 then i else authority_end s (i + 1) len

target_of : String -> <Panic> String
target_of t =
  if String.byte_length t == 0 then "/"
  else if String.byte_at 0 t == '?'u8 then "/" ++ t
  else t

authority : Bool -> String -> String -> <Panic> Result String Url
authority secure a target =
  if String.contains "@" a then Err "userinfo is not supported"
  else if String.starts_with "[" a then Err "IPv6 addresses are not supported"
  else
    let (host, port) = match String.find ":" a with
      | None -> (a, Some (if secure then 443 else 80))
      | Some j -> (String.slice 0 j a, port_of (String.slice (j + 1) (String.byte_length a) a))
    if String.byte_length host == 0 || not (all_bytes is_host_byte host) then Err "the host is not valid"
    else match port with
      | None -> Err "the port is not valid"
      | Some p -> Ok (Url { secure, host = lower host, port = p, target })

port_of : String -> <Panic> Option Int
port_of t =
  let n = String.byte_length t
  if n == 0 || n > 5 || not (all_bytes is_digit t) then None
  else match String.to_int t with
    | Some p -> if p >= 1 && p <= 65535 then Some p else None
    | None -> None

is_host_byte : Byte -> <Panic> Bool
is_host_byte b = is_alnum b || b == '-'u8 || b == '.'u8 || b == '_'u8

host_field : Url -> String
host_field u =
  let default = if u.secure then 443 else 80
  if u.port == default then u.host else "\{u.host}:\{show u.port}"

same_origin : Url -> Url -> Bool
same_origin a b = a.secure == b.secure && a.host == b.host && a.port == b.port

-- `Location` は絶対パスか、`http://` か `https://` で始まる絶対 URL である。`//` で始まる形と相対パスは読まない
resolve : Url -> String -> <Panic> Result String Url
resolve base loc =
  if String.starts_with "/" loc && not (String.starts_with "//" loc) then
    let scheme = if base.secure then "https" else "http"
    parse_url "\{scheme}://\{host_field base}\{loc}"
  else if has_http_scheme loc then parse_url loc
  else Err "the location is not an absolute URL or an absolute path"

has_http_scheme : String -> <Panic> Bool
has_http_scheme loc =
  let l = lower loc
  String.starts_with "http://" l || String.starts_with "https://" l

-- リダイレクトの応答は本文を読まずに閉じ、状態と `Location` だけを返す
data Outcome =
  | Done Response
  | Moved Int String

follow : String -> Url -> Bytes -> List (String, String) -> Int -> Int -> String -> <IO, Panic, Async> Response
follow method u body hs left limit ca =
  match exchange method u body hs ca (limit > 0) with
    | Done r -> r
    | Moved status loc ->
      let prefix = "cannot follow the redirect to \{loc}"
      if left <= 0 then fail_io Other "\{prefix}: too many redirects (the limit is \{show limit})"
      else match resolve u loc with
        | Err reason -> fail_io Other "\{prefix}: \{reason}"
        | Ok next ->
          if u.secure && not next.secure then fail_io Other "\{prefix}: it goes from https to http"
          else
            let keep = status == 307 || status == 308
            let method = if keep || method == "HEAD" then method else "GET"
            let body = if keep then body else Bytes.empty ()
            let hs = if keep then hs else without ["content-type"] hs
            -- 落とした資格は、同じオリジンへ戻っても戻さない
            let hs = if same_origin u next then hs else without ["authorization", "cookie", "proxy-authorization"] hs
            follow method next body hs (left - 1) limit ca

without : List String -> List (String, String) -> <Panic> List (String, String)
without names hs = List.filter (fn h -> not (List.contains (lower h.0) names)) hs

is_redirect : Int -> Bool
is_redirect st = st == 301 || st == 302 || st == 303 || st == 307 || st == 308

-- `http` と `https` の接続を1つの型で扱う。`Lin` で、落とすとソケットを閉じ、close_notify は送らない
data Conn =
  | Plain Net.TcpStream
  | Secure Tls.Stream

open_conn : Url -> String -> <IO, Async> Conn
open_conn u ca =
  if u.secure then Secure (Tls.connect_with_ca ca u.host u.port)
  else Plain (Net.connect u.host u.port)

-- `b` の後ろに読んだバイトを足す。EOF では何も足さない
read_more : Bytes -> Conn -> <IO, Async> (Bytes, Conn)
read_more b c =
  match c with
    | Plain s ->
      let (b, s) = Net.read_chunk b s
      (b, Plain s)
    | Secure s ->
      let (b, s) = Tls.read_chunk b s
      (b, Secure s)

write_all : Bytes -> Conn -> <IO, Async> Conn
write_all b c =
  match c with
    | Plain s ->
      let (_, s) = Net.write_bytes b s
      Plain s
    | Secure s -> Secure (Tls.write_bytes b s)

close_conn : Conn -> <IO, Async> Unit
close_conn c =
  match c with
    | Plain s -> Net.close s
    | Secure s -> Tls.close s

-- `redirects` が偽 (`max_redirects` が 0 以下) なら、リダイレクトの応答もほかの応答と同じく返す
exchange : String -> Url -> Bytes -> List (String, String) -> String -> Bool -> <IO, Panic, Async> Outcome
exchange method u body hs ca redirects =
  let head = request_head method u (Bytes.length body) hs
  let c = open_conn u ca
  let c = write_all (Bytes.of_string head) c
  -- 本文は別に書き、要求の頭と1つの `Bytes` にまとめるための写しを省く
  let c = if Bytes.length body == 0 then c else write_all body c
  let (status, headers, r) = read_head (Reader { conn = c, buf = Bytes.empty (), pos = 0 })
  match (if redirects && is_redirect status then List.lookup "location" headers else None) with
    | Some loc ->
      let Reader { conn, buf = _, pos = _ } = r
      close_conn conn
      Moved status loc
    | None ->
      let (body, conn) = read_body method status headers r
      close_conn conn
      Done (Response { status, headers, body })

request_head : String -> Url -> Int -> List (String, String) -> <Panic> String
request_head method u n hs =
  let agent = if List.contains "user-agent" (List.map (fn h -> lower h.0) hs) then "" else "User-Agent: eml\r\n"
  -- 本文に意味のあるメソッドは、空の本文でも長さを送る (RFC 9110 の 8.6)
  let length =
    if n > 0 || method == "POST" || method == "PUT" || method == "PATCH" then "Content-Length: \{show n}\r\n" else ""
  let fields = List.fold (fn acc h -> acc ++ h.0 ++ ": " ++ h.1 ++ "\r\n") "" hs
  "\{method} \{u.target} HTTP/1.1\r\nHost: \{host_field u}\r\n\{agent}Connection: close\r\n\{length}\{fields}\r\n"

-- 応答の読み手。`buf` の `pos` から後ろがまだ使っていないバイトである。`read_chunk` は読みすぎたバイトを戻せない
-- ので、chunk の区切りを越えて読んだバイトをここに持つ
data Reader =
  | Reader { conn : Conn, buf : Bytes, pos : Int }

max_line : Int
max_line = 65536

-- LF で終わる1行を、LF と直前の CR を除いて返す。LF の前の EOF は `None` である
read_line : Reader -> <IO, Panic, Async> (Option String, Reader)
read_line r =
  let Reader { conn, buf, pos } = r
  line_from conn buf pos pos

-- `at` より前に LF がないことは確かめてあるので、探すのは新しく届いたバイトだけである
line_from : Conn -> Bytes -> Int -> Int -> <IO, Panic, Async> (Option String, Reader)
line_from conn buf pos at =
  let len = Bytes.length buf
  let i = find_lf at len buf
  if i < len then
    let stop = if i > pos && Bytes.get (i - 1) buf == '\r'u8 then i - 1 else i
    if stop - pos > max_line then line_too_long conn
    else match String.from_utf8 (Bytes.slice pos stop buf) with
      | Some line -> (Some line, Reader { conn, buf, pos = i + 1 })
      | None ->
        drop conn
        fail_io NotUtf8 "cannot read the response: a line is not valid UTF-8"
  -- 最後のバイトが CR でも中身が上限を超える
  else if len - pos > max_line + 1 then line_too_long conn
  else
    -- 使い終えた buffer は容量を残して空にし、途中の行は1回だけ前へ詰める
    let (buf, pos) =
      if pos == len then (Bytes.truncate 0 buf, 0)
      else if pos > 0 then (Bytes.slice pos len buf, 0)
      else (buf, pos)
    let before = Bytes.length buf
    let (buf, conn) = read_more buf conn
    if Bytes.length buf == before then (None, Reader { conn, buf, pos })
    else line_from conn buf pos before

line_too_long : Conn -> <IO> a
line_too_long conn =
  drop conn
  fail_io Other "cannot read the response: a line is longer than 65536 bytes"

find_lf : Int -> Int -> Bytes -> <Panic> Int
find_lf i len buf =
  if i >= len || Bytes.get i buf == '\n'u8 then i else find_lf (i + 1) len buf

-- 1xx の中間の応答は読んで捨て、最後の応答の状態とヘッダーを返す
read_head : Reader -> <IO, Panic, Async> (Int, List (String, String), Reader)
read_head r =
  let (line, r) = read_line r
  match line with
    | None -> closed_in_head r
    | Some l ->
      let st = status_of l
      if st < 0 then
        drop r
        fail_io Other "cannot read the response: the status line is not valid"
      else
        let (hs, r) = read_fields r Nil
        if st < 200 then read_head r else (st, hs, r)

closed_in_head : Reader -> <IO> a
closed_in_head r =
  drop r
  fail_io ConnectionReset "cannot read the response: the connection closed before the end of the headers"

-- `HTTP/1.<数字> <3桁>` の後に、空白と理由の句が続いてもよい。誤りは -1 である
status_of : String -> <Panic> Int
status_of l =
  let n = String.byte_length l
  if n < 12 || not (String.starts_with "HTTP/1." l) || not (is_digit (String.byte_at 7 l))
    || String.byte_at 8 l != ' 'u8 || (n > 12 && String.byte_at 12 l != ' 'u8) then -1
  else
    let code = String.slice 9 12 l
    if not (all_bytes is_digit code) then -1
    else match String.to_int code with
      | Some c -> if c >= 100 && c <= 599 then c else -1
      | None -> -1

read_fields : Reader -> List (String, String) -> <IO, Panic, Async> (List (String, String), Reader)
read_fields r acc =
  let (line, r) = read_line r
  match line with
    | None -> closed_in_head r
    | Some l ->
      if String.byte_length l == 0 then (List.reverse acc, r)
      else
        let b = String.byte_at 0 l
        match (if b == ' 'u8 || b == '\t'u8 then folded l acc else field_of l acc) with
          | Some acc -> read_fields r acc
          | None ->
            drop r
            fail_io Other "cannot read the response: a header line is not valid"

-- obs-fold の行は、直前の値に空白1つでつなぐ (RFC 9112 の 5.2)
folded : String -> List (String, String) -> <Panic> Option (List (String, String))
folded l acc =
  match acc with
    | Nil -> None
    | (name, value) :: rest ->
      if not (is_field_value l) then None
      else Some ((name, String.trim (value ++ " " ++ String.trim l)) :: rest)

field_of : String -> List (String, String) -> <Panic> Option (List (String, String))
field_of l acc =
  match String.find ":" l with
    | None -> None
    | Some i ->
      let name = String.slice 0 i l
      let value = String.slice (i + 1) (String.byte_length l) l
      if not (is_token name) || not (is_field_value value) then None
      else Some ((lower name, String.trim value) :: acc)

values : String -> List (String, String) -> List String
values name hs = List.map (fn h -> h.1) (List.filter (fn h -> h.0 == name) hs)

-- 本文の長さは `Transfer-Encoding`、`Content-Length`、EOF の順に決める (RFC 9112 の 6.3)
read_body : String -> Int -> List (String, String) -> Reader -> <IO, Panic, Async> (Bytes, Conn)
read_body method status hs r =
  if method == "HEAD" || status == 204 || status == 304 then
    let Reader { conn, buf = _, pos = _ } = r
    (Bytes.empty (), conn)
  else
    let te = values "transfer-encoding" hs
    match te with
      | _ :: _ ->
        let coding = String.join ", " te
        if lower coding == "chunked" then read_chunked r (Bytes.empty ())
        else
          drop r
          fail_io Other "cannot read the response: the transfer coding \{coding} is not supported"
      | Nil ->
        match values "content-length" hs with
          | Nil -> read_to_eof r
          | cl ->
            match content_length cl with
              | Some n -> read_exact n r
              | None ->
                drop r
                fail_io Other "cannot read the response: the Content-Length is not valid"

-- 同じ値の並び (`5, 5` や同じヘッダーの繰り返し) は1つの値とみなす (RFC 9110 の 8.6)
content_length : List String -> <Panic> Option Int
content_length cl =
  let parts = List.map String.trim (List.fold (fn acc v -> List.append acc (String.split "," v)) Nil cl)
  let ns = List.map (fn p -> if String.byte_length p > 0 && all_bytes is_digit p then String.to_int p else None) parts
  match ns with
    | Some n :: rest -> if List.fold (fn ok m -> ok && m == Some n) True rest then Some n else None
    | _ -> None

-- 偽りの `Content-Length` で大きな確保をしないよう、前もって取る容量に上限を置く
prealloc_cap : Int
prealloc_cap = 67108864

-- 本文は `read_chunk` で直に読む。容量に 8 バイトの余りを足すのは、`Net.read_chunk` が空きの 8 バイト未満の
-- buffer を広げるためである (docs/spec/stdlib.md の「`Std.Net`」)
read_exact : Int -> Reader -> <IO, Panic, Async> (Bytes, Conn)
read_exact n r =
  let Reader { conn, buf, pos } = r
  let len = Bytes.length buf
  let stop = if len - pos > n then pos + n else len
  let cap = if n < prealloc_cap then n else prealloc_cap
  fill_to n (Bytes.append_slice pos stop buf (Bytes.with_capacity (cap + 8))) conn

fill_to : Int -> Bytes -> Conn -> <IO, Panic, Async> (Bytes, Conn)
fill_to n body conn =
  let before = Bytes.length body
  if before >= n then (Bytes.truncate n body, conn)
  else
    let (body, conn) = read_more body conn
    if Bytes.length body == before then closed_in_body conn
    else fill_to n body conn

closed_in_body : Conn -> <IO> a
closed_in_body conn =
  drop conn
  fail_io ConnectionReset "cannot read the response: the connection closed before the end of the body"

read_to_eof : Reader -> <IO, Panic, Async> (Bytes, Conn)
read_to_eof r =
  let Reader { conn, buf, pos } = r
  to_eof (Bytes.append_slice pos (Bytes.length buf) buf (Bytes.with_capacity 65536)) conn

to_eof : Bytes -> Conn -> <IO, Async> (Bytes, Conn)
to_eof body conn =
  let before = Bytes.length body
  let (body, conn) = read_more body conn
  if Bytes.length body == before then (body, conn) else to_eof body conn

read_chunked : Reader -> Bytes -> <IO, Panic, Async> (Bytes, Conn)
read_chunked r body =
  let (line, r) = read_line r
  match line with
    | None -> chunk_eof r
    | Some l ->
      let n = chunk_size l
      if n < 0 then bad_chunk r
      else if n == 0 then skip_trailer r body
      else
        let (body, r) = take n body r
        let (line, r) = read_line r
        match line with
          | None -> chunk_eof r
          | Some rest -> if String.byte_length rest == 0 then read_chunked r body else bad_chunk r

bad_chunk : Reader -> <IO> a
bad_chunk r =
  drop r
  fail_io Other "cannot read the response: the chunked body is not valid"

chunk_eof : Reader -> <IO> a
chunk_eof r =
  let Reader { conn, buf = _, pos = _ } = r
  closed_in_body conn

-- trailer の行は捨てる
skip_trailer : Reader -> Bytes -> <IO, Panic, Async> (Bytes, Conn)
skip_trailer r body =
  let (line, r) = read_line r
  match line with
    | None -> chunk_eof r
    | Some l ->
      if String.byte_length l == 0 then
        let Reader { conn, buf = _, pos = _ } = r
        (body, conn)
      else skip_trailer r body

-- chunk の中身の `n` バイトを `body` に足す。buffer を使いきったら容量を残して空にし、そこへ読む
take : Int -> Bytes -> Reader -> <IO, Panic, Async> (Bytes, Reader)
take n body r =
  let Reader { conn, buf, pos } = r
  let len = Bytes.length buf
  if len - pos >= n then (Bytes.append_slice pos (pos + n) buf body, Reader { conn, buf, pos = pos + n })
  else
    let body = Bytes.append_slice pos len buf body
    let (buf, conn) = read_more (Bytes.truncate 0 buf) conn
    if Bytes.length buf == 0 then closed_in_body conn
    else take (n - (len - pos)) body (Reader { conn, buf, pos = 0 })

-- 16進の数字の後に空白と `;` で始まる拡張が続いてもよい。誤りと `Int` を超える値は -1 である
chunk_size : String -> <Panic> Int
chunk_size l =
  let len = String.byte_length l
  let tail_ok i =
    if i >= len then True
    else
      let b = String.byte_at i l
      if b == ' 'u8 || b == '\t'u8 then tail_ok (i + 1) else b == ';'u8
  let digits i acc =
    let d = if i < len then hex_of (String.byte_at i l) else -1
    if d >= 0 then
      if acc > (9223372036854775807 - d) / 16 then -1 else digits (i + 1) (acc * 16 + d)
    else if i == 0 || not (tail_ok i) then -1
    else acc
  digits 0 0

hex_of : Byte -> <Panic> Int
hex_of b =
  if b >= '0'u8 && b <= '9'u8 then Byte.to_int b - 48
  else if b >= 'a'u8 && b <= 'f'u8 then Byte.to_int b - 87
  else if b >= 'A'u8 && b <= 'F'u8 then Byte.to_int b - 55
  else -1

is_digit : Byte -> Bool
is_digit b = b >= '0'u8 && b <= '9'u8

is_alnum : Byte -> Bool
is_alnum b = is_digit b || (b >= 'a'u8 && b <= 'z'u8) || (b >= 'A'u8 && b <= 'Z'u8)

-- RFC 9110 の token。空の名前は token でない
is_token : String -> <Panic> Bool
is_token s = String.byte_length s > 0 && all_bytes is_tchar s

is_tchar : Byte -> <Panic> Bool
is_tchar b = is_alnum b || String.contains (String.of_char (Char.of_byte b)) "!#$%&'*+-.^_`|~"

-- HTAB のほかの制御文字 (CR、LF、NUL を含む) と DEL を拒む。0x80 からのバイトは UTF-8 の文字として通す
is_field_value : String -> <Panic> Bool
is_field_value s = all_bytes (fn b -> b == '\t'u8 || (b >= ' 'u8 && b != 0x7Fu8)) s

all_bytes : (Byte -> <e> Bool) -> String -> <Panic | e> Bool
all_bytes p s =
  let n = String.byte_length s
  let go i = if i >= n then True else if p (String.byte_at i s) then go (i + 1) else False
  go 0

lower : String -> <Panic> String
lower s = String.fold (fn acc c -> String.push (Char.to_ascii_lower c) acc) "" s
```

## A.2 UI tests

### tests/ui/run/http/chunked.em

```haskell
-- chunked の本文を読む。大文字の `Chunked`、chunk の拡張、16進の大文字と小文字、読みをまたぐ 100 000 バイトの chunk、
-- trailer を確かめる。`Content-Length` が一緒にあっても chunked で読み、trailer は `headers` に入れない
-- (docs/spec/stdlib.md の「`Std.Http`」)
import Async (Async)
import Async
import Bytes
import List
import Net
import String
import Http

skip_request : Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
skip_request s =
  let (line, s) = Net.read_line s
  match line with
    | Some "" -> s
    | Some _ -> skip_request s
    | None -> s

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let server = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    let s = skip_request s
    let s = Net.write "HTTP/1.1 200 OK\r\nTransfer-Encoding: Chunked\r\nContent-Length: 3\r\n\r\n" s
    let s = Net.write "5;name=value\r\nhello\r\nb \r\n, chunked! \r\n186a0\r\n" s
    let (_, s) = Net.write_bytes (Bytes.make 100000 120u8) s
    Net.close (Net.write "\r\n1F\r\n and the last one, of 31 bytes.\r\n0\r\nX-Trailer: t\r\n\r\n" s))
  let r = Http.get "http://localhost:\{show port}/"
  println "status: \{show r.status}"
  List.each (fn h -> println "header: \{h.0}: \{h.1}") r.headers
  let n = Bytes.length r.body
  println "length: \{show n}"
  println "head: \{String.decode (Bytes.slice 0 16 r.body)}"
  println "tail: \{String.decode (Bytes.slice (n - 32) n r.body)}"
  Async.await server)
```

### tests/ui/run/http/content_length.em

```haskell
-- `Content-Length` の本文を読む。要求の行と順、応答のヘッダーの名前を小文字にすること、obs-fold をつなぐこと、
-- 同じ名前のヘッダーを並べたまま残すこと、`Content-Length` の後のバイトを読まないことを確かめる
-- (docs/spec/stdlib.md の「`Std.Http`」)
import Async (Async)
import Async
import List
import Net
import String
import Http

-- 要求の行を空の行まで印字する
print_request : Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
print_request s =
  let (line, s) = Net.read_line s
  match line with
    | Some "" -> s
    | Some text ->
      println "server: \{text}"
      print_request s
    | None -> s

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let server = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    let s = print_request s
    Net.close (Net.write "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nX-Multi: a\r\nX-MULTI: b\r\nX-Folded: one\r\n \t two \r\nContent-Length:  5 \r\n\r\nhelloEXTRA" s))
  let r = Http.get "http://localhost:\{show port}/index.html?q=1"
  println "status: \{show r.status}"
  List.each (fn h -> println "header: \{h.0}: \{h.1}") r.headers
  println "body: \{String.decode r.body}"
  Async.await server)
```

### tests/ui/run/http/eof_body.em

```haskell
-- `Content-Length` も `Transfer-Encoding` もない応答は、EOF までを本文にする。server は本文を2回に分けて書く
-- (docs/spec/stdlib.md の「`Std.Http`」)
import Async (Async)
import Async
import Net
import String
import Http

skip_request : Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
skip_request s =
  let (line, s) = Net.read_line s
  match line with
    | Some "" -> s
    | Some _ -> skip_request s
    | None -> s

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let server = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    let s = skip_request s
    let s = Net.write "HTTP/1.0 200 OK\r\nServer: test\r\n\r\nfirst part, " s
    Net.close (Net.write "second part" s))
  let r = Http.get "http://localhost:\{show port}"
  println "status: \{show r.status}"
  println "body: \{String.decode r.body}"
  Async.await server)
```

### tests/ui/run/http/failures.em

```haskell
-- 読めない応答と読めない `Location` の IO の失敗。接続ごとに1つの壊れた応答を書き、クライアントは失敗の種類と文言を印字する
-- (docs/spec/stdlib.md の「`Std.Http`」)
import Async (Async)
import Async
import Bytes
import List
import Net
import String
import Http

skip_request : Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
skip_request s =
  let (line, s) = Net.read_line s
  match line with
    | Some "" -> s
    | Some _ -> skip_request s
    | None -> s

-- クライアントは誤りを見つけたところで接続を落とすので、書きの失敗は捨てる
serve : Net.TcpListener -> String -> <IO, Panic, Async> Net.TcpListener
serve l response =
  let (s, l) = Net.accept l
  let _ = try_io (fn () -> Net.close (Net.write response (skip_request s)))
  l

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let long = String.decode (Bytes.make 65537 97u8)
  let responses = [
    "",
    "HTTP/1.1 200 OK\r\nContent-Length: 5\r\n",
    "HTTP/2 200 OK\r\n\r\n",
    "HTTP/1.1 20 OK\r\n\r\n",
    "HTTP/1.1 200 OK\r\nX-Long: \{long}\r\n\r\n",
    "HTTP/1.1 200 OK\r\nBad Name: x\r\n\r\n",
    "HTTP/1.1 200 OK\r\n folded first\r\n\r\n",
    "HTTP/1.1 200 OK\r\nContent-Length: 5, 6\r\n\r\nhello",
    "HTTP/1.1 200 OK\r\nContent-Length: -1\r\n\r\n",
    "HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nhello",
    "HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip, chunked\r\n\r\n",
    "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n",
    "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabcd\r\n0\r\n\r\n",
    "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhel",
    "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nX-Trailer: t\r\n",
    "HTTP/1.1 302 Found\r\nLocation: next?u=http://x\r\n\r\n"]
  let server = Async.start (fn () ->
    let l = List.fold serve l responses
    Net.close_listener l)
  let url = "http://localhost:\{show port}/"
  List.each (fn _ ->
    match try_io (fn () -> Http.get url) with
      | Ok r -> println "ok: \{show r.status}"
      | Err e -> println "\{show e.kind}: \{e.message}") responses
  Async.await server)
```

### tests/ui/run/http/https_get.em

```haskell
-- https の要求。`tls.localhost` の終端器が平文を同じリスナーに渡す。http から https へのリダイレクトは追い、
-- EOF までの本文は close_notify で終わる (docs/spec/stdlib.md の「`Std.Http`」、docs/spec/vm.md の「時計と driver」)
import Async (Async)
import Async
import Bytes
import Fs
import Net
import String
import Http

-- 要求の行を空の行まで印字する。閉じる前に読むので、終端器は handshake を終えてから close_notify を送る
print_request : Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
print_request s =
  let (line, s) = Net.read_line s
  match line with
    | Some "" -> s
    | Some text ->
      println "server: \{text}"
      print_request s
    | None -> s

serve : String -> Net.TcpListener -> <IO, Panic, Async> Net.TcpListener
serve response l =
  let (s, l) = Net.accept l
  Net.close (Net.write response (print_request s))
  l

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let options = { Http.defaults | ca = Fs.read_text "ca.pem" }
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let server = Async.start (fn () ->
    let l = serve "HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nsecure" l
    let l = serve "HTTP/1.1 301 Moved Permanently\r\nLocation: https://tls.localhost:\{show port}/upgraded\r\n\r\n" l
    let l = serve "HTTP/1.1 200 OK\r\n\r\nuntil close_notify" l
    Net.close_listener l)
  let r = Http.request "GET" "https://tls.localhost:\{show port}/" (Bytes.empty ()) options
  println "client: \{show r.status} \{String.decode r.body}"
  let r = Http.request "GET" "http://localhost:\{show port}/plain" (Bytes.empty ()) options
  println "client: \{show r.status} \{String.decode r.body}"
  Async.await server)
```

### tests/ui/run/http/no_body.em

```haskell
-- `HEAD` の応答と、204 と 304 の応答は、`Content-Length` があっても本文を読まない。1xx の中間の応答は読んで捨て、
-- 次の応答を返す (docs/spec/stdlib.md の「`Std.Http`」)
import Async (Async)
import Async
import Bytes
import List
import Net
import Http

skip_request : Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
skip_request s =
  let (line, s) = Net.read_line s
  match line with
    | Some "" -> s
    | Some _ -> skip_request s
    | None -> s

-- 1つの接続を受けて応答を書く。本文を読まないことを確かめるため、接続はクライアントが閉じるまで閉じない
serve : String -> Net.TcpListener -> <IO, Panic, Async> Net.TcpListener
serve response l =
  let (s, l) = Net.accept l
  let s = Net.write response (skip_request s)
  let (_, s) = Net.read_chunk (Bytes.empty ()) s
  Net.close s
  l

report : Http.Response -> <IO, Panic> Unit
report r =
  println "status: \{show r.status}"
  List.each (fn h -> println "header: \{h.0}: \{h.1}") r.headers
  println "length: \{show (Bytes.length r.body)}"

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let server = Async.start (fn () ->
    let l = serve "HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n" l
    let l = serve "HTTP/1.1 204 No Content\r\nContent-Length: 5\r\n\r\n" l
    let l = serve "HTTP/1.1 304 Not Modified\r\nContent-Length: 5\r\n\r\n" l
    let l = serve "HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 103 Early Hints\r\nLink: </a.css>\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok" l
    Net.close_listener l)
  let url = "http://localhost:\{show port}/"
  report (Http.request "HEAD" url (Bytes.empty ()) Http.defaults)
  report (Http.get url)
  report (Http.get url)
  report (Http.get url)
  Async.await server)
```

### tests/ui/run/http/post.em

```haskell
-- 本文のある要求。`Content-Length` は本文が空でないときと、`POST`、`PUT`、`PATCH` で付く。呼び出し側のヘッダーは
-- 書いた順と綴りのまま後ろに並び、`User-Agent` を渡すと既定の `User-Agent: eml` の代わりになる
-- (docs/spec/stdlib.md の「`Std.Http`」)
import Async (Async)
import Async
import Bytes
import Net
import String
import Http

-- 要求の行を印字し、`Content-Length` の本文を読んで印字してから、応答を書いて閉じる
serve : Net.TcpListener -> <IO, Panic, Async> Net.TcpListener
serve l =
  let (s, l) = Net.accept l
  let (n, s) = print_head 0 s
  let (body, s) = read_body n (Bytes.empty ()) s
  println "server: body \{String.decode body}"
  Net.close (Net.write "HTTP/1.1 201 Created\r\nContent-Length: 4\r\n\r\ndone" s)
  l

print_head : Int -> Net.TcpStream -> <IO, Panic, Async> (Int, Net.TcpStream)
print_head n s =
  let (line, s) = Net.read_line s
  match line with
    | Some "" -> (n, s)
    | Some text ->
      println "server: \{text}"
      let n = if String.starts_with "Content-Length: " text then
          match String.to_int (String.slice 16 (String.byte_length text) text) with
            | Some m -> m
            | None -> n
        else n
      print_head n s
    | None -> (n, s)

read_body : Int -> Bytes -> Net.TcpStream -> <IO, Panic, Async> (Bytes, Net.TcpStream)
read_body n b s =
  if Bytes.length b >= n then (b, s)
  else
    let (b, s) = Net.read_chunk b s
    read_body n b s

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let server = Async.start (fn () ->
    let l = serve (serve (serve (serve l)))
    Net.close_listener l)
  let url = "http://localhost:\{show port}/items"
  let headers = [("Content-Type", "application/json"), ("x-trace", "a b")]
  let r = Http.request "POST" url (Bytes.of_string "{\"name\": \"eml\"}") { Http.defaults | headers }
  println "client: \{show r.status} \{String.decode r.body}"
  let r = Http.request "POST" url (Bytes.empty ()) Http.defaults
  println "client: \{show r.status} \{String.decode r.body}"
  let r = Http.request "PUT" url (Bytes.of_string "x") { Http.defaults | headers = [("User-Agent", "probe/1")] }
  println "client: \{show r.status} \{String.decode r.body}"
  let r = Http.request "DELETE" url (Bytes.empty ()) Http.defaults
  println "client: \{show r.status} \{String.decode r.body}"
  Async.await server)
```

### tests/ui/run/http/redirect_methods.em

```haskell
-- 307 と 308 は同じメソッドと本文で送り直す。301 と 302 は `GET` にするが、`HEAD` は `HEAD` のままにする。
-- 回数は `max_redirects` まで追え、`max_redirects` が 0 ならリダイレクトの応答をそのまま返す
-- (docs/spec/stdlib.md の「`Std.Http`」)
import Async (Async)
import Async
import Bytes
import Net
import String
import Http

serve : String -> Net.TcpListener -> <IO, Panic, Async> Net.TcpListener
serve response l =
  let (s, l) = Net.accept l
  let (n, s) = print_head 0 s
  let (body, s) = read_body n (Bytes.empty ()) s
  if n > 0 then println "server: body \{String.decode body}" else ()
  Net.close (Net.write response s)
  l

-- 要求の行と `Content-Length` の行だけを印字する
print_head : Int -> Net.TcpStream -> <IO, Panic, Async> (Int, Net.TcpStream)
print_head n s =
  let (line, s) = Net.read_line s
  match line with
    | Some "" -> (n, s)
    | Some text ->
      if String.starts_with "Content-Length: " text then
        println "server: \{text}"
        match String.to_int (String.slice 16 (String.byte_length text) text) with
          | Some m -> print_head m s
          | None -> print_head n s
      else if String.contains " HTTP/1.1" text then
        println "server: \{text}"
        print_head n s
      else print_head n s
    | None -> (n, s)

read_body : Int -> Bytes -> Net.TcpStream -> <IO, Panic, Async> (Bytes, Net.TcpStream)
read_body n b s =
  if Bytes.length b >= n then (b, s)
  else
    let (b, s) = Net.read_chunk b s
    read_body n b s

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let server = Async.start (fn () ->
    let l = serve "HTTP/1.1 307 Temporary Redirect\r\nLocation: /b\r\n\r\n" l
    let l = serve "HTTP/1.1 308 Permanent Redirect\r\nLocation: /c\r\n\r\n" l
    let l = serve "HTTP/1.1 302 Found\r\nLocation: /d\r\n\r\n" l
    let l = serve "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok" l
    let l = serve "HTTP/1.1 301 Moved Permanently\r\nLocation: /f\r\n\r\n" l
    let l = serve "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n" l
    let l = serve "HTTP/1.1 302 Found\r\nLocation: /h\r\nContent-Length: 4\r\n\r\nmove" l
    Net.close_listener l)
  let options = { Http.defaults | max_redirects = 3 }
  let r = Http.request "PUT" "http://localhost:\{show port}/a" (Bytes.of_string "data") options
  println "client: \{show r.status} \{String.decode r.body}"
  let r = Http.request "HEAD" "http://localhost:\{show port}/e" (Bytes.empty ()) options
  println "client: \{show r.status} \{show (Bytes.length r.body)}"
  let r = Http.request "GET" "http://localhost:\{show port}/g" (Bytes.empty ()) { Http.defaults | max_redirects = 0 }
  println "client: \{show r.status} \{String.decode r.body}"
  Async.await server)
```

### tests/ui/run/http/redirects.em

```haskell
-- リダイレクトの連鎖。`POST` の後の 303 は本文のない `GET` にして `Content-Type` を落とし、同じオリジンの
-- `Authorization` は残す。`localhost` から `127.0.0.1` へはオリジンが変わるので、`Authorization` と `Cookie` を
-- 落とす。`Location` のない 302 は、そのまま応答として返す (docs/spec/stdlib.md の「`Std.Http`」)
import Async (Async)
import Async
import Bytes
import Net
import String
import Http

-- 要求の行を印字し、`Content-Length` の本文を読んで印字してから、応答を書いて閉じる
serve : String -> Net.TcpListener -> <IO, Panic, Async> Net.TcpListener
serve response l =
  let (s, l) = Net.accept l
  let (n, s) = print_head 0 s
  let (body, s) = read_body n (Bytes.empty ()) s
  if n > 0 then println "server: body \{String.decode body}" else ()
  Net.close (Net.write response s)
  l

print_head : Int -> Net.TcpStream -> <IO, Panic, Async> (Int, Net.TcpStream)
print_head n s =
  let (line, s) = Net.read_line s
  match line with
    | Some "" -> (n, s)
    | Some text ->
      println "server: \{text}"
      let n = if String.starts_with "Content-Length: " text then
          match String.to_int (String.slice 16 (String.byte_length text) text) with
            | Some m -> m
            | None -> n
        else n
      print_head n s
    | None -> (n, s)

read_body : Int -> Bytes -> Net.TcpStream -> <IO, Panic, Async> (Bytes, Net.TcpStream)
read_body n b s =
  if Bytes.length b >= n then (b, s)
  else
    let (b, s) = Net.read_chunk b s
    read_body n b s

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let server = Async.start (fn () ->
    let l = serve "HTTP/1.1 303 See Other\r\nLocation: /see\r\nContent-Length: 4\r\n\r\nskip" l
    let l = serve "HTTP/1.1 301 Moved Permanently\r\nLocation:  http://127.0.0.1:\{show port}/moved \r\n\r\n" l
    let l = serve "HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nhere" l
    let l = serve "HTTP/1.1 302 Found\r\nContent-Length: 11\r\n\r\nno location" l
    Net.close_listener l)
  let headers = [("Content-Type", "text/plain"), ("Authorization", "Bearer t"), ("Cookie", "k=v"), ("Accept", "*/*")]
  let options = { Http.defaults | headers }
  let r = Http.request "POST" "http://localhost:\{show port}/form" (Bytes.of_string "a=1") options
  println "client: \{show r.status} \{String.decode r.body}"
  let r = Http.get "http://localhost:\{show port}/plain"
  println "client: \{show r.status} \{String.decode r.body}"
  Async.await server)
```

### tests/ui/run/http/url.em

```haskell
-- URL の読み方と、接続の前に投げる要求の誤り。scheme と host は大文字小文字を区別せず小文字にし、空の path は `/`
-- にし、fragment は送らない。既定のポートでなければ `Host` にポートを書く (docs/spec/stdlib.md の「`Std.Http`」)
import Async (Async)
import Async
import Bytes
import List
import Net
import Http

-- 要求の行と `Host` の行を印字し、空の本文で応える
serve : Net.TcpListener -> <IO, Panic, Async> Net.TcpListener
serve l =
  let (s, l) = Net.accept l
  let (line, s) = Net.read_line s
  let (host, s) = Net.read_line s
  match (line, host) with
    | (Some a, Some b) -> println "server: \{a} | \{b}"
    | _ -> println "server: no request"
  let s = skip_request s
  Net.close (Net.write "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n" s)
  l

skip_request : Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
skip_request s =
  let (line, s) = Net.read_line s
  match line with
    | Some "" -> s
    | Some _ -> skip_request s
    | None -> s

attempt : String -> Bytes -> Http.Options -> String -> <IO, Panic, Async> Unit
attempt method body options url =
  match try_io (fn () -> Http.request method url body options) with
    | Ok r -> println "client: \{show r.status}"
    | Err e -> println "client: \{show e.kind}: \{e.message}"

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let server = Async.start (fn () ->
    let l = serve (serve (serve l))
    Net.close_listener l)
  let get = attempt "GET" (Bytes.empty ()) Http.defaults
  get "HTTP://LocalHost:\{show port}"
  get "http://localhost:\{show port}?a=1&b=2#top"
  get "http://127.0.0.1:\{show port}/a/b/../c?x#y#z"
  List.each get [
    "ftp://localhost/",
    "localhost/",
    "http://",
    "http:///path",
    "http://user:pw@localhost/",
    "http://[::1]:8080/",
    "http://localhost:0/",
    "http://localhost:65536/",
    "http://localhost:/",
    "http://localhost:8a/",
    "http://local host/",
    "http://localhost/caf\u{e9}",
    "http://local%68ost/",
    "http://nowhere.invalid/"]
  attempt "GET POST" (Bytes.empty ()) Http.defaults "http://localhost/"
  attempt "GET" (Bytes.empty ()) { Http.defaults | headers = [("Bad Name", "x")] } "http://localhost/"
  attempt "GET" (Bytes.empty ()) { Http.defaults | headers = [("X-Split", "a\r\nInjected: b")] } "http://localhost/"
  attempt "GET" (Bytes.empty ()) { Http.defaults | headers = [("content-LENGTH", "3")] } "http://localhost/"
  attempt "GET" (Bytes.empty ()) { Http.defaults | headers = [("Host", "elsewhere")] } "http://localhost/"
  Async.await server)
```

### tests/ui/run-fail/http/downgrade.em

```haskell
-- https から http へのリダイレクトは断る。リダイレクトの先には接続しない (docs/spec/stdlib.md の「`Std.Http`」)
import Async (Async)
import Async
import Bytes
import Fs
import Net
import Http

skip_request : Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
skip_request s =
  let (line, s) = Net.read_line s
  match line with
    | Some "" -> s
    | Some _ -> skip_request s
    | None -> s

main : Unit -> <IO, Panic> Unit
main () =
  let r = Async.run (fn () ->
    let options = { Http.defaults | ca = Fs.read_text "ca.pem" }
    let l = Net.listen "127.0.0.1" 0
    let (port, l) = Net.local_port l
    let server = Async.start (fn () ->
      let (s, l) = Net.accept l
      Net.close_listener l
      let response = "HTTP/1.1 301 Moved Permanently\r\nLocation: http://localhost:\{show port}/plain\r\n\r\n"
      Net.close (Net.write response (skip_request s)))
    let r = try_io (fn () -> Http.request "GET" "https://tls.localhost:\{show port}/" (Bytes.empty ()) options)
    Async.await server
    r)
  match r with
    | Ok _ -> println "followed"
    | Err e ->
      println (show e.kind)
      fail_io e.kind e.message
```

### tests/ui/run-fail/http/too_many_redirects.em

```haskell
-- `max_redirects` を超えるリダイレクトは IO の失敗になる。上限が 2 なら、3つ目の 302 で失敗する
-- (docs/spec/stdlib.md の「`Std.Http`」)
import Async (Async)
import Async
import Bytes
import Net
import Http

skip_request : Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
skip_request s =
  let (line, s) = Net.read_line s
  match line with
    | Some "" -> s
    | Some _ -> skip_request s
    | None -> s

serve : Net.TcpListener -> <IO, Panic, Async> Net.TcpListener
serve l =
  let (s, l) = Net.accept l
  Net.close (Net.write "HTTP/1.1 302 Found\r\nLocation: /again\r\n\r\n" (skip_request s))
  l

main : Unit -> <IO, Panic> Unit
main () =
  let r = Async.run (fn () ->
    let l = Net.listen "127.0.0.1" 0
    let (port, l) = Net.local_port l
    let server = Async.start (fn () -> Net.close_listener (serve (serve (serve l))))
    let options = { Http.defaults | max_redirects = 2 }
    let r = try_io (fn () -> Http.request "GET" "http://localhost:\{show port}/" (Bytes.empty ()) options)
    Async.await server
    r)
  match r with
    | Ok _ -> println "followed"
    | Err e ->
      println (show e.kind)
      fail_io e.kind e.message
```

### tests/ui/run-fail/http/truncated.em

```haskell
-- `tls-cut.localhost` の終端器は close_notify を送らずに閉じる。EOF までの本文を読むクライアントは、`Std.Tls` の
-- `ConnectionReset` の失敗をそのまま受ける (docs/spec/stdlib.md の「`Std.Http`」、docs/spec/vm.md の「時計と driver」)
import Async (Async)
import Async
import Bytes
import Fs
import Net
import Http

skip_request : Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
skip_request s =
  let (line, s) = Net.read_line s
  match line with
    | Some "" -> s
    | Some _ -> skip_request s
    | None -> s

main : Unit -> <IO, Panic> Unit
main () =
  let r = Async.run (fn () ->
    let options = { Http.defaults | ca = Fs.read_text "ca.pem" }
    let l = Net.listen "127.0.0.1" 0
    let (port, l) = Net.local_port l
    let server = Async.start (fn () ->
      let (s, l) = Net.accept l
      Net.close_listener l
      Net.close (Net.write "HTTP/1.1 200 OK\r\n\r\npartial" (skip_request s)))
    let r = try_io (fn () -> Http.request "GET" "https://tls-cut.localhost:\{show port}/" (Bytes.empty ()) options)
    Async.await server
    r)
  match r with
    | Ok _ -> println "read"
    | Err e ->
      println (show e.kind)
      fail_io e.kind e.message
```

## A.3 Expected snapshots (body after the insta header)

### integration__ui__run@http__chunked.em.snap

```
input_file: tests/ui/run/http/chunked.em
---
--- stdout ---
status: 200
header: transfer-encoding: Chunked
header: content-length: 3
length: 100047
head: hello, chunked! 
tail: x and the last one, of 31 bytes.
--- stderr ---
```

### integration__ui__run@http__content_length.em.snap

```
input_file: tests/ui/run/http/content_length.em
---
--- stdout ---
server: GET /index.html?q=1 HTTP/1.1
server: Host: localhost:49152
server: User-Agent: eml
server: Connection: close
status: 200
header: content-type: text/plain
header: x-multi: a
header: x-multi: b
header: x-folded: one two
header: content-length: 5
body: hello
--- stderr ---
```

### integration__ui__run@http__eof_body.em.snap

```
input_file: tests/ui/run/http/eof_body.em
---
--- stdout ---
status: 200
body: first part, second part
--- stderr ---
```

### integration__ui__run@http__failures.em.snap

```
input_file: tests/ui/run/http/failures.em
---
--- stdout ---
ConnectionReset: cannot read the response: the connection closed before the end of the headers
ConnectionReset: cannot read the response: the connection closed before the end of the headers
Other: cannot read the response: the status line is not valid
Other: cannot read the response: the status line is not valid
Other: cannot read the response: a line is longer than 65536 bytes
Other: cannot read the response: a header line is not valid
Other: cannot read the response: a header line is not valid
Other: cannot read the response: the Content-Length is not valid
Other: cannot read the response: the Content-Length is not valid
ConnectionReset: cannot read the response: the connection closed before the end of the body
Other: cannot read the response: the transfer coding gzip, chunked is not supported
Other: cannot read the response: the chunked body is not valid
Other: cannot read the response: the chunked body is not valid
ConnectionReset: cannot read the response: the connection closed before the end of the body
ConnectionReset: cannot read the response: the connection closed before the end of the body
Other: cannot follow the redirect to next?u=http://x: the location is not an absolute URL or an absolute path
--- stderr ---
```

### integration__ui__run@http__https_get.em.snap

```
input_file: tests/ui/run/http/https_get.em
---
--- stdout ---
server: GET / HTTP/1.1
server: Host: tls.localhost:49152
server: User-Agent: eml
server: Connection: close
client: 200 secure
server: GET /plain HTTP/1.1
server: Host: localhost:49152
server: User-Agent: eml
server: Connection: close
server: GET /upgraded HTTP/1.1
server: Host: tls.localhost:49152
server: User-Agent: eml
server: Connection: close
client: 200 until close_notify
--- stderr ---
```

### integration__ui__run@http__no_body.em.snap

```
input_file: tests/ui/run/http/no_body.em
---
--- stdout ---
status: 200
header: content-length: 5
length: 0
status: 204
header: content-length: 5
length: 0
status: 304
header: content-length: 5
length: 0
status: 200
header: content-length: 2
length: 2
--- stderr ---
```

### integration__ui__run@http__post.em.snap

```
input_file: tests/ui/run/http/post.em
---
--- stdout ---
server: POST /items HTTP/1.1
server: Host: localhost:49152
server: User-Agent: eml
server: Connection: close
server: Content-Length: 15
server: Content-Type: application/json
server: x-trace: a b
server: body {"name": "eml"}
client: 201 done
server: POST /items HTTP/1.1
server: Host: localhost:49152
server: User-Agent: eml
server: Connection: close
server: Content-Length: 0
server: body 
client: 201 done
server: PUT /items HTTP/1.1
server: Host: localhost:49152
server: Connection: close
server: Content-Length: 1
server: User-Agent: probe/1
server: body x
client: 201 done
server: DELETE /items HTTP/1.1
server: Host: localhost:49152
server: User-Agent: eml
server: Connection: close
server: body 
client: 201 done
--- stderr ---
```

### integration__ui__run@http__redirect_methods.em.snap

```
input_file: tests/ui/run/http/redirect_methods.em
---
--- stdout ---
server: PUT /a HTTP/1.1
server: Content-Length: 4
server: body data
server: PUT /b HTTP/1.1
server: Content-Length: 4
server: body data
server: PUT /c HTTP/1.1
server: Content-Length: 4
server: body data
server: GET /d HTTP/1.1
client: 200 ok
server: HEAD /e HTTP/1.1
server: HEAD /f HTTP/1.1
client: 200 0
server: GET /g HTTP/1.1
client: 302 move
--- stderr ---
```

### integration__ui__run@http__redirects.em.snap

```
input_file: tests/ui/run/http/redirects.em
---
--- stdout ---
server: POST /form HTTP/1.1
server: Host: localhost:49152
server: User-Agent: eml
server: Connection: close
server: Content-Length: 3
server: Content-Type: text/plain
server: Authorization: Bearer t
server: Cookie: k=v
server: Accept: */*
server: body a=1
server: GET /see HTTP/1.1
server: Host: localhost:49152
server: User-Agent: eml
server: Connection: close
server: Authorization: Bearer t
server: Cookie: k=v
server: Accept: */*
server: GET /moved HTTP/1.1
server: Host: 127.0.0.1:49152
server: User-Agent: eml
server: Connection: close
server: Accept: */*
client: 200 here
server: GET /plain HTTP/1.1
server: Host: localhost:49152
server: User-Agent: eml
server: Connection: close
client: 302 no location
--- stderr ---
```

### integration__ui__run@http__url.em.snap

```
input_file: tests/ui/run/http/url.em
---
--- stdout ---
server: GET / HTTP/1.1 | Host: localhost:49152
client: 200
server: GET /?a=1&b=2 HTTP/1.1 | Host: localhost:49152
client: 200
server: GET /a/b/../c?x HTTP/1.1 | Host: 127.0.0.1:49152
client: 200
client: Other: invalid URL ftp://localhost/: the scheme is not http or https
client: Other: invalid URL localhost/: the scheme is not http or https
client: Other: invalid URL http://: the host is not valid
client: Other: invalid URL http:///path: the host is not valid
client: Other: invalid URL http://user:pw@localhost/: userinfo is not supported
client: Other: invalid URL http://[::1]:8080/: IPv6 addresses are not supported
client: Other: invalid URL http://localhost:0/: the port is not valid
client: Other: invalid URL http://localhost:65536/: the port is not valid
client: Other: invalid URL http://localhost:/: the port is not valid
client: Other: invalid URL http://localhost:8a/: the port is not valid
client: Other: invalid URL http://local host/: the URL contains a space, a control or a non-ASCII character
client: Other: invalid URL http://localhost/café: the URL contains a space, a control or a non-ASCII character
client: Other: invalid URL http://local%68ost/: the host is not valid
client: NotFound: cannot connect to nowhere.invalid:80: unknown host
client: Other: invalid request: the method GET POST is not valid
client: Other: invalid request: the header Bad Name is not valid
client: Other: invalid request: the header X-Split is not valid
client: Other: invalid request: the header content-LENGTH is set by Std.Http
client: Other: invalid request: the header Host is set by Std.Http
--- stderr ---
```

### integration__ui__run_fail@http__downgrade.em.snap

```
input_file: tests/ui/run-fail/http/downgrade.em
---
--- stdout ---
Other
--- stderr ---
--- runtime error ---
cannot follow the redirect to http://localhost:49152/plain: it goes from https to http
  at run-fail/http/downgrade.em:35:7
```

### integration__ui__run_fail@http__too_many_redirects.em.snap

```
input_file: tests/ui/run-fail/http/too_many_redirects.em
---
--- stdout ---
Other
--- stderr ---
--- runtime error ---
cannot follow the redirect to /again: too many redirects (the limit is 2)
  at run-fail/http/too_many_redirects.em:37:7
```

### integration__ui__run_fail@http__truncated.em.snap

```
input_file: tests/ui/run-fail/http/truncated.em
---
--- stdout ---
ConnectionReset
--- stderr ---
--- runtime error ---
cannot read the stream: the peer closed the connection without close_notify
  at run-fail/http/truncated.em:35:7
```

