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
