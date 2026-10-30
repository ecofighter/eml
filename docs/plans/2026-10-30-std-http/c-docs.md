# 付録 C: 文書の下書き

各タスクが文書に書く文の下書きである。入れる前に natural-japanese の skill で直す。見出しの番号は設計の作業のときのもので、本文の 5.3 から 5.6 にあたる。

### 5.3 `docs/spec/stdlib.md` section (complete; between 「`Std.Tls`」 and 「`Std.Signal`」)

The failure table follows the `Std.Tls` table: Japanese case labels, the kind, and the message as English code text.

````markdown
## `Std.Http`

`Std.Http` は HTTP/1.1 のクライアントのモジュールである。`std/Http.em` が `Std.Net` と `Std.Tls` の上に eml で書いてあり、extern を持たない。要求ごとに接続を開き、応答を読み終えると閉じる。`http` の URL は `Net.connect` で、`https` の URL は `Tls.connect_with_ca` でつなぐ。待ち、取り消し、期限は `Net` と `Tls` のもので、`Std.Http` は何も足さない。

```haskell
import Async
import Bytes
import Fs
import Http
import List
import String

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let options = { Http.defaults | ca = Fs.read_text "ca.pem", headers = [("Accept", "text/plain")] }
  let r = Http.request "GET" "https://localhost:8443/hello" (Bytes.empty ()) options
  println (show r.status)
  match List.lookup "content-type" r.headers with
    | Some t -> println t
    | None -> println "no type"
  println (String.decode r.body))
```

| 名前 | 型 | 実装 |
|---|---|---|
| `Options` | `Options { headers : List (String, String), ca : String, max_redirects : Int }` | eml |
| `Response` | `Response { status : Int, headers : List (String, String), body : Bytes }` | eml |
| `defaults` | `Options` | eml |
| `get` | `String -> <IO, Panic, Async> Response` | eml |
| `request` | `String -> String -> Bytes -> Options -> <IO, Panic, Async> Response` | eml |

- `Options` は要求の設定である。`headers` は要求に足すヘッダー、`ca` は根に足す CA の証明書の PEM (`Tls.connect_with_ca` の `ca` で、空なら既定の根だけ)、`max_redirects` は追うリダイレクトの回数の上限である。`defaults` は `Options { headers = [], ca = "", max_redirects = 5 }` である。`Options` と `Response` は `Eq` と `Show` を導出する
- `get url` は `request "GET" url (Bytes.empty ()) defaults` である
- `request method url body options` は、接続の前に `method`、`options.headers` の各項目、`url` の順に確かめる。`method` は RFC 9110 の token (英数字と ``!#$%&'*+-.^_`|~`` の1文字以上) で、綴りのまま送る。メソッドは書いたとおりに比べるので、下の `HEAD`、`POST`、`PUT`、`PATCH` の扱いは大文字の綴りにだけ当たる。ヘッダーの名前は token で、値は HTAB のほかの制御文字 (CR、LF、NUL を含む) と DEL を含まない。0x80 からのバイトは UTF-8 の文字として通す。名前が大文字小文字を区別せずに `Host`、`Connection`、`Content-Length`、`Transfer-Encoding` のどれかなら失敗にする。この4つは本文の区切りと接続の扱いを決めるので、`Std.Http` が書く
- URL は `scheme://authority` に path と query が続く形である。最初の `#` から後ろの fragment は除き、送らない。残りのバイトはどれも 0x21 から 0x7E でなければならない。空白、制御文字、ASCII でない文字は失敗で、百分率の符号化は呼ぶ側が行う。scheme は最初の `://` の前で、大文字小文字を区別せずに `http` か `https` である。authority は `://` の後から最初の `/` か `?` の手前までで、`host` か `host:port` である。`@` を含む authority (userinfo) と `[` で始まる authority (IPv6 のアドレス) は扱わない。host は英数字、`-`、`.`、`_` の1文字以上で、小文字にして使う。port は1桁から5桁の数字で、1 から 65535 である。省くと、`http` は 80、`https` は 443 である。空の path は `/` にし、`?` で始まるときは前に `/` を足す。オリジンは scheme、host、port の組である
- 要求の頭は、要求の行 `{method} {path と query} HTTP/1.1` に続けて、`Host`、`User-Agent: eml`、`Connection: close`、`Content-Length`、`options.headers` の順に並べ、空の行で終える。行の終わりは CRLF である。`Host` の値は host で、port が scheme の既定でなければ `host:port` にする。`options.headers` に `User-Agent` があれば、既定の `User-Agent: eml` を省く。`Content-Length` は、本文が空でないときと、メソッドが `POST`、`PUT`、`PATCH` のときに付ける (RFC 9110 の 8.6)。`options.headers` は渡した順と綴りのまま書く。頭と本文は別々に書く
- https の接続は `Tls.connect_with_ca options.ca host port` で開き、host を SNI と証明書の名前に使う。http の接続は `Net.connect host port` で開く
- 本文を書き終える前に相手が応答を送って閉じると、本文の書きは `Net` か `Tls` の書きの失敗になり、応答は読まない。書きの包みはストリームを落としてから投げるので、読むストリームが残らないためである
- 応答は `Net` か `Tls` の `read_chunk` で読み、行は `Std.Http` が切り出す。`read_chunk` は読みすぎたバイトを戻せないので、chunk の区切りを越えて読んだバイトを `Std.Http` の buffer に残すためである。行は LF で終わり、直前の CR を1つ除く。行の中身 (LF と直前の CR を除いたバイト) は 65 536 バイトまでで、超えた行は、LF が届いたときか、LF のないまま 65 537 バイトを超えて届いたときに失敗になる。そのため結果は、バイトが読みにどう分かれて届くかによらない。UTF-8 でない行も失敗である。LF の前の EOF では、途中の行を捨てて EOF とみなす。状態行、ヘッダー、chunk の大きさの行、chunk の後の行、trailer の行が、どれもこの規則に従う。ヘッダーと trailer の行の数には上限を置かないので、短い行を送り続ける相手からは戻らない。期限は `Async.timeout` で付ける
- 状態行は、`HTTP/1.` と数字1つ、空白、100 から 599 の3桁の番号で、その後に空白と理由の句が続いてもよい。理由の句は捨てる。100 から 199 の応答は中間の応答で、ヘッダーまで読んで捨て、次の応答を読む
- ヘッダーの行は `名前:値` で、名前は token である。名前と `:` の間に空白は置けない。値は HTAB のほかの制御文字を含まず、前後の空白と HTAB を除く。空白か HTAB で始まる行 (obs-fold) は、直前のヘッダーの値に空白1つでつなぐ (RFC 9112 の 5.2)。最初のヘッダーの行が obs-fold なら失敗である
- `Response` の `status` は最後の応答の番号で、`headers` はそのヘッダーを届いた順に並べたものである。名前は ASCII の小文字にし、同じ名前のヘッダーはまとめずに残す。中間の応答のヘッダーと trailer は入れない。名前は `List.lookup "content-type" r.headers` のように小文字で引く
- 本文の長さは次の順に決める (RFC 9112 の 6.3)。メソッドが `HEAD` か、番号が 204 か 304 なら、本文は空で、何も読まない。`Transfer-Encoding` があれば、すべての値を `, ` でつないで小文字にした列が `chunked` のときだけ chunked で読み、`Content-Length` は見ない。ほかの coding は失敗にする。`Content-Length` があれば、その長さを読む。すべての `Content-Length` の値を `,` で分けた部分は、前後の空白を除いてどれも1桁以上の数字で、`Int` に収まる同じ値でなければならない (RFC 9110 の 8.6)。どちらのヘッダーもなければ、EOF までを本文にする
- `Content-Length` の本文は、その長さのバイトである。後ろに届いたバイトは捨てる。長さに届く前の EOF は失敗である
- chunked の本文では、chunk の大きさの行は1つ以上の16進の数字 (大文字と小文字) で、後に空白と HTAB、`;` で始まる拡張が続いてもよい。拡張は捨てる。値が `Int` に収まらない行は失敗である。chunk の中身の後の行は空でなければならない。大きさ 0 の chunk の後は、空の行まで trailer の行を読んで捨てる。終わりの空の行の前の EOF は失敗である
- EOF までの本文は、`read_chunk` が何も足さなくなるまで読む。https で close_notify のないまま閉じた接続は、`Tls.read_chunk` の `ConnectionReset` の失敗をそのまま伝える。`Content-Length` と chunked の本文は終わりまで読んだところで閉じるので、その後の close_notify のない EOF は失敗にならない
- 本文は `read_chunk` で1つの `Bytes` の後ろに足していき、足した部分を写し直さない。`Content-Length` の本文は、長さと 64 MiB の小さいほうに 8 バイトを足した容量を前もって取るので、64 MiB までの本文は広げ直さない。8 バイトは、`Net.read_chunk` が空きの 8 バイト未満の buffer を広げる分である (上の「`Std.Net`」)
- `max_redirects` が 1 以上なら、番号が 301、302、303、307、308 で `Location` のある応答のリダイレクトを追う。`Location` のない応答はほかの応答と同じく返す。`max_redirects` が 0 以下なら追わずに、リダイレクトの応答もほかの応答と同じく返す。追うときは、リダイレクトの応答の本文を読まずに接続を閉じる。追った回数が `max_redirects` に達していれば失敗にする。`Location` は前後の空白を除いた最初の値である。`/` で始まって `//` で始まらなければ絶対パスとして今のオリジンに付け、大文字小文字を区別せずに `http://` か `https://` で始まれば絶対 URL として上の規則で読む。ほかの形は失敗である。https から http へのリダイレクトは失敗にし、http から https へは追う
- 301、302、303 では、メソッドが `HEAD` ならそのまま、ほかは `GET` にし、本文を空にして、呼ぶ側の `Content-Type` を落とす。307 と 308 は同じメソッド、本文、ヘッダーで送る。行き先のオリジンが今の URL のオリジンと違えば、呼ぶ側の `Authorization`、`Cookie`、`Proxy-Authorization` を落とす。落としたヘッダーは、後で元のオリジンに戻っても戻さない
- 接続は、応答を返す前と、リダイレクトを追う前に閉じる。https は `Tls.close` で、http は `Net.close` で閉じる。`Std.Http` が失敗を投げるときは、close_notify を送らずに接続を落としてから投げる。`Net` と `Tls` の関数が失敗したときは、その関数がストリームを落としている
- 取り消しの地点は `Net` と `Tls` の待つ関数で、取り消されると巻き戻しが接続を落とす。期限は `Async.timeout ms (fn () -> Http.get url)` で付ける
- `Std.Http` が投げるのは IO の失敗だけで、`Panic` は起こさない。row の `Panic` は、使う `String` と `Bytes` の関数から来る
- 本文の大きさの上限、gzip などの coding、HTTP/2、keep-alive、プロキシ、cookie、IPv6 のアドレスの URL はない

失敗の種類と文言は次のとおりである。

| 場合 | 種類 | 文言 |
|---|---|---|
| token でないメソッド | `Other` | `invalid request: the method {method} is not valid` |
| token でないヘッダーの名前か、制御文字を含む値 | `Other` | `invalid request: the header {name} is not valid` |
| `Std.Http` が書くヘッダーを渡した | `Other` | `invalid request: the header {name} is set by Std.Http` |
| URL の誤り | `Other` | `invalid URL {url}: {理由}` |
| 長すぎる行 | `Other` | `cannot read the response: a line is longer than 65536 bytes` |
| UTF-8 でない行 | `NotUtf8` | `cannot read the response: a line is not valid UTF-8` |
| 状態行の誤り | `Other` | `cannot read the response: the status line is not valid` |
| ヘッダーの行の誤り | `Other` | `cannot read the response: a header line is not valid` |
| `Content-Length` の誤り | `Other` | `cannot read the response: the Content-Length is not valid` |
| chunked のほかの transfer coding | `Other` | `cannot read the response: the transfer coding {coding} is not supported` |
| chunked の本文の誤り | `Other` | `cannot read the response: the chunked body is not valid` |
| ヘッダーの終わりの前の EOF | `ConnectionReset` | `cannot read the response: the connection closed before the end of the headers` |
| `Content-Length` か chunked の本文の終わりの前の EOF | `ConnectionReset` | `cannot read the response: the connection closed before the end of the body` |
| リダイレクトの回数が上限を超えた | `Other` | `cannot follow the redirect to {location}: too many redirects (the limit is {n})` |
| `Location` の誤り | `Other` | `cannot follow the redirect to {location}: {理由}` |
| https から http へのリダイレクト | `Other` | `cannot follow the redirect to {location}: it goes from https to http` |
| 接続、TLS、読み書き、取り消しの失敗 | `Std.Net` と `Std.Tls` と同じ | `Std.Net` と `Std.Tls` の文言 |

URL の誤りの理由は、`the URL contains a space, a control or a non-ASCII character`、`the scheme is not http or https`、`userinfo is not supported`、`IPv6 addresses are not supported`、`the host is not valid`、`the port is not valid` のどれかである。`Location` の誤りの理由は、これに `the location is not an absolute URL or an absolute path` を足したものである。`{url}` は渡した URL、`{coding}` は `Transfer-Encoding` の値を `, ` でつないだもの、`{location}` は前後の空白を除いた `Location` の値、`{n}` は `max_redirects` である。
````

### 5.4 `docs/implementation/testing.md` bullet (after `tls/`, C2)

```markdown
  - `http/`: `Std.Http` のクライアント。相手はプログラムの中の `Net` の server のタスクで、https は `tls/` と同じく `tls.localhost` と `tls-cut.localhost` の終端器を通る。https のテストは `Fs.read_text "ca.pem"` を `Options` の `ca` に渡す。`tls/` と同じ理由で seed では流さない。server のタスクは要求を読んでから応答を書いて閉じ、印字は応答を書く前だけにする。クライアントは要求が返ってから印字するので、出力の順はスケジュールによらない。エントリのファイルを `http.em` にしない (上の `std/` と同じく、`import Http` が E1026 になる)
```

### 5.5 UI tests

All sources are in A.2 and all expected snapshots in A.3; every one was produced by the harness on the test driver.
- **Pattern:**
  - The server task accepts each connection in turn and reads the request (plus the Content-Length body where needed).
  - It prints only before writing, writes the whole response, and closes.
  - The `run-fail` tests catch with `try_io`, print `show e.kind` and rethrow with `fail_io e.kind e.message`, so the location is in the test file.
- **No Windows marks.**
- **Coverage:**

| File | Covers |
|---|---|
| `run/http/content_length.em` | Request serialization and order; lowercase names; obs-fold; duplicates; CL surplus ignored |
| `run/http/eof_body.em` | HTTP/1.0 EOF body in two writes |
| `run/http/no_body.em` | HEAD, 204, 304 with CL; 100 and 103 interim responses (the server stays open until the client closes, so reading a body would deadlock) |
| `run/http/post.em` | POST body; `Content-Length: 0` for an empty POST; `User-Agent` override; DELETE without CL |
| `run/http/url.em` | Lowercasing, empty path, query-only path, fragment; every URL reason; `unknown host` pass-through; every request-validation failure |
| `run/http/chunked.em` | `Chunked`, extensions, BWS, both hex cases, a 100000-byte chunk across reads, trailer, TE+CL |
| `run/http/failures.em` | 16 failure rows, including line too long and the relative Location `next?u=http://x` |
| `run/http/redirects.em` | POST, then 303 (GET; Content-Type dropped; credentials kept), then a cross-origin 301 (Authorization and Cookie dropped); 302 without Location returned |
| `run/http/redirect_methods.em` | 307 and 308 keep PUT and its body; 302 becomes GET; exactly `max_redirects = 3`; HEAD stays HEAD on 301; `max_redirects = 0` returns `302 move` |
| `run/http/https_get.em` | https CL body via `tls.localhost`; http to https redirect; EOF body ended by close_notify |
| `run-fail/http/truncated.em` | `cannot read the stream: the peer closed the connection without close_notify` at `run-fail/http/truncated.em:35:7` |
| `run-fail/http/too_many_redirects.em` | `cannot follow the redirect to /again: too many redirects (the limit is 2)` at `:37:7` |
| `run-fail/http/downgrade.em` | `cannot follow the redirect to http://localhost:49152/plain: it goes from https to http` at `:35:7` |

### 5.6 Close-commit texts (Japanese drafts) and ownership

- **status.md 範囲** (after the TLS bullet; S1): 「- HTTP: `Std.Http` の HTTP/1.1 のクライアントが、`Std.Net` と `Std.Tls` の上で `http` と `https` の URL に要求を出す。要求ごとに接続を開いて閉じ、本文は `Transfer-Encoding` の chunked、`Content-Length`、EOF で区切る。リダイレクトを追い、https から http へは追わない ([標準ライブラリ](../spec/stdlib.md) の「`Std.Http`」)」
- **status.md `### Async` limits** (S1): 「- `Std.Http` に keep-alive、HTTP/2、gzip などの coding、プロキシ、cookie、ストリーミングの本文、本文の大きさの上限、IPv6 のアドレスの URL はない。ヘッダーと trailer の行の数にも上限がなく、期限は `Async.timeout` で付ける。本文を書き終える前に閉じた相手の応答は読めない ([標準ライブラリ](../spec/stdlib.md) の「`Std.Http`」、[ロードマップ](../future/roadmap.md) の「非同期」)」
- **overview.md row** after `| TLS |` (S1): 「| HTTP | `Std.Http` は `Net` と `Tls` の上に eml で書いた HTTP/1.1 のクライアントで、extern を持たない。要求ごとに `Connection: close` で接続を開いて閉じる。本文は `Transfer-Encoding`、`Content-Length`、EOF の順に区切り、1つの `Bytes` に読む。リダイレクトは回数の上限まで追い、https から http へは追わない | [標準ライブラリ](spec/stdlib.md) の「`Std.Http`」 |」
- **roadmap.md** (S1):
  - Delete line 31 (`std-http`).
  - In 「その後の項目」 / 「非同期」, the existing bullet 「`Http` の HTTP/2 と ALPN、gzip、keep-alive の接続のプール、ストリーミングの本文、本文の大きさの上限、プロキシ」 gets 「、cookie、IPv6 のアドレスの URL」 appended.
  - Append to its 見出し 見直す条件: 「IPv6 のアドレスの URL は、IPv6 だけのホストへつなぐプログラムが出たとき。」
- **async.md 「TLS と HTTP」** (S1 owns the whole trim, including the lines S2 cites):
  - Delete:
    - the `Std.Http` code block;
    - the bullets starting 「URL は」, 「要求は」, 「応答の状態行」, 「リダイレクトは」 and 「gzip、HTTP/2」 (:351-356);
    - 「テストは次のとおりである。」 and its three bullets, including the bench bullet at :363.
  - Rewrite the intro line to: 「`Std.Http` は `Net` と `Tls` の上に eml で書く HTTP/1.1 のクライアントで、extern を足さない。面と規則は [標準ライブラリ](../spec/stdlib.md) の「`Std.Http`」にある。」
  - Add three bullets:
    - 「- 接続は要求ごとに開いて閉じる。keep-alive がなければ、本文の終わりを読み違えても次の応答に響かず、EOF までの本文も同じ形で読める」
    - 「- 行は `read_line` でなく、`read_chunk` の上に自分の buffer で切り出す。chunk の区切りを越えて読んだバイトを `read_chunk` は戻せないので本文に自分の buffer が要り、状態行とヘッダーも同じ buffer で読んで行の上限を 64 KiB にそろえる」
    - 「- 呼ぶ側に `Host`、`Connection`、`Content-Length`、`Transfer-Encoding` を書かせない。本文の区切りが2つになるのと、EOF までの本文が終わらなくなるのを防ぐためである」
  - Keep the libcurl bullet.
  - In the Tls copy bullet (:357), replace 「この費用は下の `https-get` の行で測る」 with 「この費用は [計測の基準](../implementation/benchmarks.md) の「スクリプトの計測」の `https-get` の行で測る」 (as S2 requested).
- **benchmarks.md and bench/:** S2.
