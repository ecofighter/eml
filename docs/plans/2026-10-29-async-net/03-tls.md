# 付録3: TLS と HTTPS クライアントの設計 (後の段 `net-tls` と `std-http`)

この付録の単位 (`Tls-` で始まるもの) は、後の段 `net-tls` と `std-http` のためのもので、`async-net` では実装しない。

`net-tls` の単位は Tls-U0 から Tls-U6、`std-http` の単位は Tls-U7 から Tls-U11 とする。

## 1. 結論

1. TLS と HTTP の両方を、`async-net` の直後、`net-windows` の前に置く2つの段として入れる。段の列は roadmap.md の「段の列」である。
   - `net-tls`: `Std.Tls` (TLS 1.2/1.3 の**クライアントのみ**)。新しい workspace の crate `crates/eml_tls` (rustls 0.23 + aws-lc-rs + webpki-roots) が支え、`eml_vm` の cargo feature `tls` (既定で有効) で結ぶ。テストの相手は、`eml_io` の `TestDriver` (feature `tls`) の中にある TLS の終端器で、eml のプログラムからは仮想のホスト `tls.localhost` として見える。前提: `async-net`。
   - `std-http`: `Std.Http`。`Net` と `Tls` の上に eml で書く HTTP/1.1 のクライアントで、extern は足さない。前提: `net-tls`。
   - 「TLS のプラグインは `gui-plugin` を待つ」という後回しは**取り下げる**。TLS はプラグインにしない。`Plugin` trait は GUI のために予約したままにする (ffi.md の「拡張」は変えない)。roadmap の後回しの項目「TLS のプラグイン」と「`Http`」は消し、段の項目と新しい後回しの項目 (5.8 の (b)) に置き換える。
2. 作り方は sans-IO にする。`Std.Tls.Conn` は、rustls の接続と、Rust 側の復号した平文の列を持つ `Lin` の `Foreign` で、I/O を行わない。`std/Tls.em` の eml のコードが、`Conn` と `Net.TcpStream` の間でバイトを汲む。ソケットの I/O、取り消しの地点、期限、決定的なテストの driver はすべて `Std.Net` のものにする。TLS の extern は普通の同期の行である。`eml_rt` は変えない。確かめた: `Heap::alloc_foreign` (heap/mod.rs:3894) と `alloc_bytes`・`alloc_bytes_room` (heap/string.rs:507, 525) は公開されていて、eml_vm の cells.rs:1700, 1990, 2051 がすでに使っている。
3. 暗号の provider は aws-lc-rs、根の証明書は webpki-roots とし、追加の根は `connect_with_ca` に PEM で渡す。`flake.nix` は変えない。`.#cross` のコマンドは TLS を除く形に変える (正確な形は 5.7)。CI には clippy の行を1つ足す。`nix build` の確認は `net-tls` の最初の単位 Tls-U0 で行う (5.5)。
4. API は 5.3、HTTP の範囲は 5.4 に書く。
5. `Std.Net` とテストの driver の仮想のホスト (Net の設計) への必須の依存は 5.6 に書く。
6. 日本語の文書の案は 5.8 に書く。

## 2. 理由

### 2.1 rustls を使う設計

- **プラグインにしない。** プラグインの設計は GUI の要求 (スレッドの固定、イベントループ、`Session::run` への登録) のためにある (ffi.md:116-120。コードはまだない)。sans-IO の `Conn` はそのどれも要らない。プラグインが与える性質は C の provider なしで組めることだけで、それは ffi.md:120 がすでに計画している `--features` の形で、`eml_vm` の cargo feature が与える。
- **driver 側の `TlsStream` にせず、sans-IO にする。** driver が見るのは `RawBuf` の貸し出しだけである (async.md:231、T10)。TLS の操作を置けば、本物の driver に rustls が入り、取り消しと期限の意味が別に要る。sans-IO なら、待ち、取り消し、期限が `Net` の1か所に集まり、`Tls` は組み合わせでそれを引き継ぐ。テストの driver は TLS の終端器を持つが、それは仮想の相手としてだけで、eml 側の `TcpStream` の側には置かない。
- **eml が汲む。** eml は約50行で、レコードの処理は Rust にある。読みの道の写しは次のとおりである。カーネルから `Bytes` へ (driver が `Bytes` に直接書く)、`Bytes` から rustls の deframer の buffer へ (1)、rustls から `Conn.plaintext` へ (2。rustls の受け取った平文の上限が 16 KiB なので必ずこうなる。5.2)、`Conn.plaintext` から `Bytes` へ (3)。書きの道は、`Bytes` から rustls へ (1)、レコードから `Bytes` へ (2)。値は `std-http` で測り (5.5)、断言はしない。
- **aws-lc-rs を選ぶ理由。** rustls の既定で推奨の provider である。確かめた: rustls と各 crate の文書。ring の最後のリリースは 2025-03 で、README が依存を勧めていない。確かめた: rustls と各 crate の文書。純 Rust の provider は既定にならない。rustls-pure は TLS 1.3 だけで、ダウンロードは318回、持ち主は1人である。確かめた: crates.io の API。graviola は x86_64 と aarch64 だけで、"very new" とされている。確かめた: 各 crate の文書。測った値 (Linux x86_64、4コア、1.99.1、release、手元の試作の build): ring の試作は 32 秒・2.17 MB、aws-lc-rs の試作は 51 秒・4.40 MB。eml を組む環境にはどこにも C のツールチェーンがある。rustc が unix では `cc`、Windows では MSVC の `link.exe` でリンクするからである。aws-lc-sys は、それ以上を要らない (FIPS でなければ CMake・Go・bindgen は要らない。確かめた: rustls と各 crate の文書)。C のツールチェーンがない場所は `.#cross` だけで、そこはリンクしない。
- **`.#cross` の事実。** ring も aws-lc-rs も `.#cross` の Windows の clippy で失敗する。確かめた: 試作。ring は cc-rs の `ar -out:` で、aws-lc-sys は host の gcc で C を組もうとして失敗した。直したコマンド (`--exclude eml_tls --no-default-features --features eml_cli/run,eml_test_support/run`) は、`#[cfg(windows)] compile_error!` を持つ `eml_tls` の stub を入れた workspace の手元の写しで通った (43秒)。確かめた。stub を除いたので、`eml_vm` と `eml_cli` のテストの対象を含むほかのすべての member は msvc でも組めている。
- **feature の仕組み (確かめた)。** workspace はすでに `eml_cli = { path = ..., default-features = false }` を宣言している (Cargo.toml:21)。`-p eml_vm` の build に TLS が入らないのは、`eml_vm` と `eml_io` の `tls` が既定で無効で、それを有効にするのが `eml_cli` の `tls` (`default` に入る) だけだからである。`eml_test_support` の `run` が有効にするのは `eml_cli/run` で、`eml_cli/tls` ではない。`cargo tree -p eml_vm -e normal,dev -i eml_tls` は "did not match any packages" になる。確かめた: 手元の写し。そのため Miri の `-p eml_vm` のジョブは aws-lc-sys を組まない。workspace 全体の `cargo nextest run` は `eml_tls` を組む。確かめた。
- **Windows の CI。** aws-lc-sys は NASM があれば使い、なければ (rustls の `aws_lc_rs` が有効にする `prebuilt-nasm` により) prebuilt の object を使う。確かめた: rustls と各 crate の文書。どちらでも CI は変えない。C のコンパイルはジョブごとに約50秒 (推定。Linux の測定から)。Tls-U0 が CI で動くまでは確かめられていない (A3)。
- **webpki-roots を選ぶ理由。** プラットフォームの FFI の依存がなく、3つの OS と `nix build` で同じ動きをし、ureq と Roc と同じ選び方である。確かめた: 各 crate の文書。企業の CA は `connect_with_ca` で足す。
- **2段に分ける理由。** `net-tls` には基盤の仕事 (feature、cross、CI、nix build) と TLS のテストがある。`std-http` は純粋な eml で、HTTP のテストと処理量の測定を持つ。
- **テストの相手はテストの driver に置き、std には置かない。** handshake は録音で再生できない (クライアントの乱数が毎回変わる) ので、UI テストには生きた TLS の server が要る。公開の server の API はなく、テスト専用の std のモジュールもないので、決定的にできる場所はテストの driver の仮想のホストだけである。終端器は `eml_tls::Conn::server` を再利用する (同じ sans-IO の状態機械を `TestDriver::settle` の中で汲む)。そのため Rust への追加は小さく、UI テストの eml 側の server は普通の `Net` のコードのままになる。本物のソケットは Rust のテストに残す (testing.md:194, 325)。

### 2.2 広げて検討した2案

**(A) libcurl と `curl` crate。** worker ごとの multi interface への組み込みは可能である。`CURLMOPT_SOCKETFUNCTION` と `TIMERFUNCTION` を `Op::Readable` と登録表のタイマーの輪に結び、書き込み可能の待ちの操作と、fd を包む道を新しく作る (今はない。確かめた: 基点のコード)。取り消しは `curl_multi_remove_handle` で行う (推定)。curl のスレッド版の名前解決は自分のスレッドを動かす (推定)。本物のソケットを要るので、決定的なテストの driver の上では何もテストできない。`Net`・`Tls`・`Http` が持つ仮想のホストの上の UI テストが書けなくなり、残るのは testing.md:194 が述べる少数の本物のソケットの Rust のテストだけである。TLS の backend は次のとおりである。`curl` 0.4.51 は既定で unix では OpenSSL、Windows では Schannel を使う。Secure Transport は `apple-sectrust` と `static-curl` のときだけで、rustls は `rustls-ffi` 経由になる。`curl-sys` 0.4.91 は常に `cc`、`libz-sys`、`pkg-config`、`vcpkg` (msvc) に依存する。確かめた: crates.io の API。nix build には `curl`、`openssl`、`pkg-config` の入力か、vendor した C の build が要る。`.#cross` はほかの C の依存と同じく失敗する。`Net` を通らないので、ほかのプロトコルが使える TLS の層にならない。判断: 採らない。

**(B) musl の静的な Linux の実行ファイル。** aws-lc-rs も ring も musl で組めるが、musl を対象にする C のコンパイラが要り、それはどの eml の shell にもない。確かめた: flake.nix。`rustls-native-certs` は実行時に `/etc/ssl` のファイルを読むので、musl だから webpki-roots が必須になるわけではない。ただし webpki-roots は自己完結の実行ファイルに向く。compio の `polling` の backend は rustix の syscall で epoll を使い、`libc` とシグナルは musl で動く (推定)。musl の `getaddrinfo` は NSS を持たず、確保を多く行う Rust のプログラムでは確保が遅い (推定)。nix には2つ目のパッケージと CI のジョブが要る。AOT (native.md:127-128。`eml build` は利用者の `cc` でリンクし、`libeml_rt.a` を同梱する) では、musl の配布物は musl 向けの `libeml_rt.a` とリンクの設定を別に要るので、既定の build を置き換えるだけでは済まない (推定)。判断: 採らず、配布の後回しの項目にする。

## 3. 退けた案

- 最初に TLS を載せる Plugin trait: 2.1。
- driver 側の `TlsStream`: 2.1。
- Rust 側で汲む案: extern は待てない (async.md:209)。
- rustls の unbuffered API: buffer の扱いを作り直すことになり、16 KiB の上限は、どちらでも平文を移し出して扱う。
- ring、rustls-pure、graviola、rustcrypto: 2.1。
- 既定の根に rustls-native-certs や platform-verifier を使う案: OS ごとの依存と動きの違いが出る。
- httparse や ureq-proto: 自明な構文解析のために parser の extern を持つことになる。
- feature で分けない案: `.#cross` が壊れる。確かめた。
- `tls` を既定で無効にする案: HTTPS のない `eml` の実行ファイルと `nix build` の成果物を配ることになる。節約できる C のツールチェーンは、rustc がリンクのためにすでに要求している。既定で有効のままにし、`nix build` を最初に確かめる。
- 公開の `Std.Tls.accept` や、server のためのテスト専用の std のモジュール: 設計の前提により置かない。
- TLS の相手を Rust のテストだけにする案 (UI テストなし): eml の汲み上げと、https 上の `Http` を、決定的に全 OS でテストできなくなる。終端器をテストの driver に作れない場合の代替としてだけ残す (6)。
- 終端器を `RunConfig` の設定や先頭行の印で切り替える案: eml 側の listener の仮想のポートは動的 (`listen ... 0`) なので、静的な印では名指しできない。予約した仮想の名前なら、harness を変えずに済む。
- close_notify なしの EOF を既定で許す案: EOF までの HTTPS の本文が黙って切れる。既定は厳格にし、必要になるまで緩い版は作らない。
- TLS の UI テストを `tests/ui/run/async/` に置く案: seed を変えた実行では `completions` を比べる (testing.md:365) が、TLS の flight がタスクと終端器の間でどうまとまるかに依存する。
- (A) libcurl は退け、(B) musl は後回しにする (2.2、5.8 の (b))。

## 4. 前提にした仮定

- A1 `nix build` は、stdenv の `cc` で CMake なしに aws-lc-sys を組める (推定。Tls-U0 で確かめる)。
- A2 macOS の devShell は stdenv の clang を使える (推定)。
- A3 windows-2025 の runner は aws-lc-sys を組める (推定。CI の Tls-U0 で確かめる)。
- A4 `-p eml_vm` の build は `eml_tls` に届かない (確かめた)。
- A5 rustls は 0.23.x のままで、`builder_with_provider(Arc::new(aws_lc_rs::default_provider()))` を使い、`install_default` は呼ばない。
- A6 `rustls::ClientConnection` と `ServerConnection` は `Send + 'static` である (推定。コンパイルで確かめる)。
- A7 UI テストは、harness が `RunConfig` で選ぶ仮想の driver の上で動く。seed を変えて動かすのは `async/` と `par/` だけである。確かめた: testing.md:346-350。
- A8 Net の設計の `Std.Net` とテストの driver が 5.6 を満たす。項目1から8は Net の設計の提案が含んでいる。項目9は新しく、`net-tls` が持ち、Net の設計の仮想のホストの構造の上に作る。
- A9 (A) と (B) について: curl の multi の callback と musl の名前解決・確保の動きは、文書と一般的な知識によるもので、動かして確かめてはいない。

## 5. 実装への影響

### 5.1 制約

- `eml_rt` は変えない。TLS の Rust のコードは次の3か所にあり、いずれも `verus!` の外に置く。
  - `crates/eml_tls` (eml の依存なし)
  - `crates/eml_vm/src/tls.rs` (腕と `Foreign` の包み)
  - `crates/eml_io/src/test_tls.rs` (`TestDriver` の中の終端器。feature `tls`)
- `Std.Tls` の行は一般の extern 命令を使う。`Extern::is_async()` はこれらを挙げない。
- `Conn` を取る行は、`Conn` を借りて組にして返す (`borrowing` と `with_arg: Some(k)`)。この形は、`with_arg` の借りの形である (eml_extern lib.rs:1322-1330 の HEAD にある `Std.IoTest.op_stream` の行が使っていた形。`async-net` はその行を消すが、`FunctionRow` のフィールドは残る)。`Conn` に対する extern は、`client_raw` を除いて**投げない**。TLS の誤りは `Conn` に記録する (状態の bit `FAILED`)。こうして、汲み上げ側が、待たせている alert を送ってから失敗にできる。
- extern の境界に `Option` と `List` は出さない。印の値は次のとおり。CA の PEM が空なら既定の根だけを使う。`pull_tls` は送るものがなければ空の `Bytes` を返す。
- feature:
  - `eml_tls` は feature を持たない。
  - `eml_io` の `[features] tls = ["dep:eml_tls"]` は、`src/test_tls.rs` と `TestDriver` の `tls.localhost` の名前だけを分ける。`real.rs` は `eml_tls` を参照しない (feature があってもなくても、本物の driver は TLS を持たない)。
  - `eml_vm` の `[features] tls = ["dep:eml_tls"]`。
  - `eml_cli` の `tls = ["run", "eml_vm/tls", "eml_io/tls"]`、`default = ["run", "tls"]`。
  - `eml_test_support` は変えない。
  - `tls` が無効のとき、`Std.Tls` の腕はすべて `Raise::Io { kind: IO_NOT_AVAILABLE, message: "cannot start a TLS session: not available" }` を投げる (型の検査は clippy の組み合わせでだけ行う)。テストの driver は `tls.localhost` を知らない名前として扱う (`NotFound`)。
- 依存 (workspace の `Cargo.toml`。行は architecture.md の「外部 crate」): `rustls = { version = "0.23", default-features = false, features = ["std", "tls12", "aws_lc_rs"] }`、`webpki-roots = "1"` (CDLA-Permissive-2.0 と注記する)。PEM は `rustls::pki_types` で扱う。
- fixture: `crates/eml_tls/fixtures/{ca.pem,cert.pem,key.pem,gen.sh}`。
  - `gen.sh` (openssl の CLI) は、CA (ECDSA P-256、自己署名、notAfter 2126) と、`tls.localhost` の leaf (SAN は DNS `tls.localhost`、DNS `localhost`、IP `127.0.0.1`。notAfter 2126) を作り直し、スクリプトに挙げた UI テストの各ディレクトリへ `ca.pem` を写す。
  - `eml_tls` はこれらを `pub mod fixtures { pub const CA_PEM: &str; pub const CERT_PEM: &str; pub const KEY_PEM: &str; }` として出す。ライブラリの中にあるテスト用の素材なので、モジュールの doc と stdlib.md にそう書く。テストの driver の終端器のほかに、これを読む製品の道はない。
  - UI のプログラムは、自分のディレクトリの `ca.pem` を `FileIn.read_all "ca.pem"` で読む (testing.md:329)。鍵はテスト専用で、`gen.sh` の先頭にそう書く。
  - rustls はシステムの時刻に対して検証するので、有効期間を長くしてある。
- UI テストの置き場: `tests/ui/run/tls/<name>/main.em` と `ca.pem`。`tests/ui/run-fail/tls/<name>/` も同じ。`tests/ui/run/http/<name>/main.em` (https の例には `ca.pem` も)。`async/` の下には置かない。
- 誤りの文言 (IO の失敗):
  - `cannot start a TLS session: <rustls Display>` (kind は `Other`。PEM の誤りやサーバ名の誤り)
  - `cannot continue the TLS session: the peer certificate was rejected` (kind は `Other`。`rustls::Error::InvalidCertificate(_)` のすべて。rustls の詳細は付けず、rustls を更新しても snapshot が変わらないようにする)
  - `cannot continue the TLS session: <rustls Display>` (その他の rustls の誤り。snapshot にはしない)
  - `the peer closed the connection during the TLS handshake` (kind は `ConnectionReset`)
  - `the peer closed the connection without close_notify` (kind は `ConnectionReset`。既定は厳格)
- メモリ: `Conn.plaintext` は、汲み上げが次を入れる前に移し出すので、`Net.read_chunk` 1回分以下に収まる。rustls の送る側の buffer は、`write_bytes` に渡した1つの `Bytes` で押さえられる (`set_buffer_limit(None)` にし、同じ呼び出しの中で書き出す)。どちらも `max_bytes` には数えない。`Http` の本文は eml の `Bytes` (数える) に全部溜め、上限は置かない。stdlib.md に書く。

### 5.2 `eml_tls` の Rust の API

```rust
pub struct Conn { conn: rustls::Connection, plaintext: Vec<u8>, failed: Option<Error>, eof: bool }
impl Conn {
    pub fn client(server_name: &str, extra_ca_pem: &[u8]) -> Result<Conn, Error>;   // webpki-roots + extra; SNI = server_name
    pub fn server(cert_chain_pem: &[u8], key_pem: &[u8]) -> Result<Conn, Error>;    // used by eml_io's terminator and by eml_tls tests only
    pub fn push_tls(&mut self, mut bytes: &[u8]);   // loop while !bytes.is_empty() && failed.is_none(): read_tls(&mut bytes) (consumes a prefix);
                                                    // process_new_packets() (Err -> failed = Some(e), stop); drain(): reader().read into
                                                    // self.plaintext until WouldBlock / Ok(0) (PEER_CLOSED) / UnexpectedEof (TRUNCATED)
    pub fn push_eof(&mut self);                     // read_tls(&mut &[][..]); process_new_packets; drain()
    pub fn pull_tls(&mut self, out: &mut Vec<u8>);  // write_tls into out while wants_write() — also after a failure (queued alert)
    pub fn write_plain(&mut self, bytes: &[u8]);    // writer().write_all; set_buffer_limit(None) at construction
    pub fn read_plain(&mut self, out: &mut Vec<u8>) -> usize;  // moves self.plaintext into out, returns count
    pub fn status(&self) -> u32;
    pub fn error_message(&self) -> String;          // "" when !failed; the mapped text of §5.1 otherwise
    pub fn send_close_notify(&mut self);
}
pub const HANDSHAKING: u32 = 1; pub const HAS_OUTGOING: u32 = 2; pub const PEER_CLOSED: u32 = 4; pub const TRUNCATED: u32 = 8; pub const FAILED: u32 = 16;
pub mod fixtures { pub const CA_PEM: &str = include_str!("../fixtures/ca.pem"); /* CERT_PEM, KEY_PEM likewise */ }
```

`process_new_packets` のたびに平文を移し出すと、rustls の `received_plaintext` (上限 16 KiB。`set_buffer_limit` の対象外) が満ちない。そのため `read_tls` が "received plaintext buffer full" を返さず、`push_tls` は終わる。確かめた: rustls 0.23.46 の src/conn.rs:784-788、common_state.rs:1135、conn.rs:543-546。

テスト (`crates/eml_tls/tests/`、`integration` のバイナリ1つ):
- メモリの中での handshake とエコー (client と、fixture を使う `server`)
- 読み側の大量の流入 (1回の `push_tls` に 256 KiB)
- 不正な CA での `FAILED`、対応づけた文言、空でない `pull_tls` (alert)
- close_notify での `PEER_CLOSED`
- close_notify なしの `push_eof` での `TRUNCATED`
- クライアントの Finished の後の TLS 1.3 の ticket (`HAS_OUTGOING`)

### 5.3 `Std.Tls` (std/Tls.em) とテストの相手

```haskell
pub extern data Conn                                              -- Lin。rustls の接続の状態。I/O は持たない
pub data Stream = | Stream { conn : Conn, sock : Net.TcpStream }   -- Lin (types.md:20)

pub connect           : String -> Int -> <IO, Async> Stream                    -- host port。Net.connect host port、host を SNI に、handshake まで
pub connect_with_ca   : String -> String -> Int -> <IO, Async> Stream          -- 追加の CA (PEM) host port
pub wrap              : String -> Net.TcpStream -> <IO, Async> Stream          -- 既にある TcpStream の上で client の handshake
pub read_chunk        : Bytes -> Stream -> <IO, Async> (Bytes, Stream)         -- 何も足さなければ、相手が close_notify で閉じた
pub read_chunk_within : Int -> Bytes -> Stream -> <IO, Async> (Option Bytes, Stream)
pub read_line         : Stream -> <IO, Panic, Async> (Option String, Stream)         -- Net.read_line と同じ意味 (行の上限 1 MiB、同じ文言)
pub read_line_within  : Int -> Stream -> <IO, Panic, Async> (Option (Option String), Stream)
pub write             : String -> Stream -> <IO, Async> Stream
pub write_bytes       : Bytes -> Stream -> <IO, Async> Stream
pub write_line        : String -> Stream -> <IO, Async> Stream
pub shutdown_write    : Stream -> <IO, Async> Stream                           -- close_notify を書き出し、Net.shutdown_write
pub close             : Stream -> <IO, Async> Unit                             -- shutdown_write してから落とす

extern client_raw        : String -> String -> <IO> Conn     -- host, 追加の CA の PEM (空なら既定の根だけ)。失敗は IO の失敗
extern push_tls          : Bytes -> Conn -> <IO> Conn        -- 投げない。TLS の誤りは status の 16
extern push_eof          : Conn -> <IO> Conn
extern pull_tls          : Conn -> <IO> (Bytes, Conn)
extern write_plain       : Bytes -> Conn -> <IO> Conn
extern read_plain        : Bytes -> Conn -> <IO> (Bytes, Conn)
extern status            : Conn -> <IO> (Int, Conn)          -- 1 handshaking, 2 has_outgoing, 4 peer_closed, 8 truncated, 16 failed
extern error_message     : Conn -> <IO> (String, Conn)
extern send_close_notify : Conn -> <IO> Conn
```

行の形:
- `client_raw` は `[Obj, Obj]`、`[Unr, Unr]` で、結果は `Obj` の `Lin`、`Effectful` である。
- それ以外の行は、`Conn` (と `Bytes`) を借り、`with_arg` で `Conn` を返す。結果が `Conn` だけの行は、`Unit` を組の最初に置くと translate が扱いにくい場合、最初の要素に `Int` の 0 を返す (包みはそれを無視する)。
- 型と関数の行は全部で10行 (型が1、関数が9)。

汲み上げ (eml、`std/Tls.em` の中の非公開の補助):
- `flush s`: `pull_tls` を繰り返し、空なら止まり、そうでなければ `Net.write_bytes` で書く。
- `check s`: `status & 16` なら `flush s` (alert を送る) し、`fail_io Other ("cannot continue the TLS session: " ++ error_message)` を投げる。
- `handshake s`: `flush` し、`status & 1` の間、`Net.read_chunk (Bytes.empty ()) sock` を繰り返す。何も足されなければ `fail_io ConnectionReset "the peer closed the connection during the TLS handshake"`。足されたら `push_tls`、`check`、`flush`。ループの後にもう1回 `flush` する (クライアントの Finished)。
- `read_chunk b s`: `flush` し、`read_plain b` する。増えたなら返す。増えず `status & 4` なら `(b, s)` を返す (EOF)。`status & 8` なら `fail_io ConnectionReset "the peer closed the connection without close_notify"`。それ以外は `Net.read_chunk` を呼び、空なら `push_eof`、そうでなければ `push_tls` し、`check` して繰り返す。
- `read_chunk_within ms`: 同じで、`Net.read_chunk_within` を使う。Net が `None` を返したら `(None, s)`。
- `write_bytes b s`: `write_plain b`、`check`、`flush`。
- `shutdown_write`: `send_close_notify`、`flush`、`Net.shutdown_write`。
- `close`: `shutdown_write` してから落とす。
- `connect` と `wrap` の中に期限はない。黙る相手には、呼び出しを包む `Async.timeout` が取り消すまで待つ (stdlib.md に書く)。取り消しの地点は `Net.*` の中だけにある。巻き戻しは `Stream` を落とす (ソケットは閉じ、close_notify は送らない)。

**テストの相手: `TestDriver` の中の TLS の終端器 (`crates/eml_io/src/test_tls.rs`、feature `tls`)。**
- 名前: 仮想のホストは `tls.localhost` を解決する (`tls` が有効なときだけ)。eml からの `Net.connect "tls.localhost" p` は、次の3つを作る。
  - (a) 暗号文の端の組。eml 側の端が、返される `TcpStream` になる。
  - (b) 平文の端の組。その遠い側は、普通の `Net.connect "localhost" p` と同じく、仮想の listener `localhost:p` の `accept` の待ち行列に入る。listener がなければ `ConnectionRefused` (Net の設計で新しく決まった kind、tag 10)。backlog が満ちていれば保留になる。どちらも、普通の `connect` と同じテストの driver の関数を通す。
  - (c) driver が持つ `Vec` の中の `Terminator { conn: eml_tls::Conn::server(CERT_PEM, KEY_PEM), cipher: IoHandle, plain: IoHandle }`。
- 汲み上げ: `settle()` の最初に、どの終端器も進まなくなるまで行う。
  - 暗号文の inbox から `push_tls`、`read_plain` を経て、平文の相手の inbox に足す。
  - 平文の inbox から `write_plain`、`pull_tls` を経て、暗号文の相手の inbox に足す。
  - 閉じる知らせの伝え方:
    - eml のクライアントが自分の端を閉じた (暗号文の inbox が EOF) なら、`push_eof` し、平文の端を閉じる (eml の server が EOF を見る)。
    - eml の server が自分の端を閉じたなら、`send_close_notify` し、`pull_tls` の結果を暗号文の inbox に入れ、暗号文の端を閉じる (eml のクライアントは close_notify の後に EOF を見る)。
    - `FAILED` なら、alert のバイトを暗号文の inbox に入れてから両方の端を閉じる。
  - 両方の端が閉じたら、終端器を取り除く。
  - 届くのは書き手より `settle` が1回遅れるので、`read_chunk` の readable と read の2段は変わらない。
- fixture: `eml_tls::fixtures::{CERT_PEM, KEY_PEM}`。leaf の SAN が `tls.localhost` を覆うので、テストの隣の `ca.pem` を `ca` にして `Tls.connect_with_ca ca "tls.localhost" p` を呼ぶクライアントは検証を通る。`Tls.connect "tls.localhost" p` (既定の根) は `the peer certificate was rejected` で失敗する。これが `unknown_issuer` のテストである。
- 切り詰めのテスト: 終端器は `tls-cut.localhost` も解決する。動きは同じだが、eml の server が自分の端を閉じるとき、暗号文の端を close_notify **なしで**閉じる。
- 終端器の Rust のテストは `crates/eml_io/tests/test_driver.rs` (`#[cfg(feature = "tls")]`) に置く。Rust 側の `eml_tls::Conn::client` を、`tls.localhost` の接続に対する driver の `Op::Write` と `Op::Read` で動かし、平文の listener の端に対して handshake を終え、バイトを往復させる。cut の版では、クライアントが `TRUNCATED` になる。本物のソケットは使わない。

### 5.4 `Std.Http` (std/Http.em、段 `std-http`)

```haskell
pub data Options  = | Options { headers : List (String, String), ca : String, max_redirects : Int }   -- ca: 追加の CA の PEM。"" なら既定の根
pub data Response = | Response { status : Int, headers : List (String, String), body : Bytes }
pub defaults : Options                                                   -- [] "" 5
pub get          : String -> <IO, Panic, Async> Response                 -- get url = request "GET" url (Bytes.empty ()) defaults
pub request      : String -> String -> Bytes -> Options -> <IO, Panic, Async> Response   -- method url body options
```

- URL: `http://` と `https://`、`host[:port]`、path?query。既定のポートは 80 と 443。userinfo、IDN、fragment は扱わない。
- 要求: HTTP/1.1、`Host`、`Connection: close`、`User-Agent: eml`、本文が空でなければ `Content-Length`。`options.headers` を末尾に足す。
- 応答: 状態行とヘッダーを `read_line` で読む (それぞれ 64 KiB まで。超えたら `fail_io Other "malformed HTTP response: ..."`)。ヘッダー名は大文字小文字を区別しない。本文は、`Content-Length` があればそれで、なければ `Transfer-Encoding: chunked` (拡張は無視し、trailer は捨てる)、それもなければ EOF まで読む (`Tls.read_chunk` は切り詰めで失敗する。5.1)。`HEAD`、1xx、204、304 には本文がない。応答の後に stream を閉じる。
- リダイレクト: 301、302、303、307、308 を、どのメソッドでも追う。回数は `options.max_redirects` まで (超えたら `fail_io Other "too many redirects"`)。
  - 301、302、303 は `GET` にし、本文を捨て、呼び出し側の `Content-Length` と `Content-Type` を落とす。
  - 307、308 は同じメソッドと本文で送る。
  - `Location` は絶対 URL か絶対パスにする。
  - 別のオリジン (scheme、host、port のどれかが違う) へは、呼び出し側の `Authorization`、`Cookie`、`Proxy-Authorization` を落とす。
  - https から http は断る (`fail_io Other "redirect from https to http refused"`)。http から https は許す。
- 範囲の外: gzip、HTTP/2 と ALPN、keep-alive とプール、ストリーミングの本文、プロキシ、cookie、本文の上限、期限 (`Async.timeout` で行う)。
- テスト:
  - `tests/ui/run/http/`: 仮想のホストの上で、普通の `Net` で書いた eml の server のタスク (`Net.listen "127.0.0.1" 0` から `local_port` を得る) を使う。content-length、chunked、EOF までの本文、リダイレクトの連鎖 (POST の後の 303、オリジンが変わったときのヘッダーの除去を含む) を扱う。https の例は `https://tls.localhost:PORT/` に、`ca.pem` から作った `options.ca` で接続する。
  - `tests/ui/run-fail/http/`: リダイレクトの回数超過、https から http への格下げ、切り詰められた EOF までの https の本文 (`tls-cut.localhost`)。
  - `#[cfg(unix)]` の本物のソケットの Rust のテストを1つ置く (std の `TcpListener` と rustls の `ServerConnection` のスレッド、本物の driver)。
  - 測定: `bench/scripts.sh` の行 `https-get` (eml の `Http.get` で、手元の rustls の server から 64 MiB の本文を取る。比べる相手は Python の `urllib.request`)。結果は段を閉じるときに benchmarks.md に記録する。CI に上限は置かない。roadmap の「Python の 1.5 倍以内」という目標を目安にする。

### 5.5 コミット単位、順に

`net-tls`:

- **Tls-U0 (build の関門を最初に)。** `crates/eml_tls` に依存と `Conn::client` だけを置いて branch を push する。`nix build` が通ること、CI の `test` ジョブが3つの OS で通ることを確かめ、ジョブの時間の前後を計画に記録する。aws-lc-sys で `nix build` が失敗したら、rustls の provider の feature を `ring` に替えてやり直す。それも失敗したら止まって決め直す。基準: Tls-U1 に進む前に、`nix build` と CI が緑であること。
- **Tls-U1.** `eml_tls` の API 全部 (5.2)、`fixtures/`、`gen.sh`、テスト、architecture.md の「外部 crate」の行。
- **Tls-U2.** 表の行 (`ExternType::TlsConn`、`Extern::Tls*`)、`std/Tls.em` の extern、`eml_vm` の feature と `src/tls.rs` の腕 (TLS が無効のときの腕を含む)、`eml_cli` の feature、`crates/eml_core_ir/tests/externs.rs` の許可一覧の更新 (テストの変更の種類は2。範囲は「`Std.Tls` の行を Lin/Poly と borrow/pair の許可一覧に足す」)、仕様は stdlib.md の「`Std.Tls`」と object-model.md の extern の表。基準: `cargo nextest run` と、`cargo clippy -p eml_cli --no-default-features --features run -- -D warnings`。
- **Tls-U3.** `eml_io` の終端器 (`test_tls.rs`、feature `tls`、`tls.localhost` と `tls-cut.localhost`) とその Rust のテスト。vm.md の「時計と driver」のテストの driver の段落に終端器を書く。基準: `cargo nextest run -p eml_io --features tls`。
- **Tls-U4.** eml の汲み上げと UI テスト。
  - `run/tls/echo/` (eml の server のタスクが `localhost:p` で行を返し、client は `tls.localhost` 経由)
  - `run/tls/large_body/` (200 KiB)
  - `run/tls/close_notify_eof/`
  - `run-fail/tls/unknown_issuer/` (CA なしの `Tls.connect`)
  - `run-fail/tls/truncated/` (`tls-cut.localhost`)
  - `run-fail/tls/handshake_eof/` (client の handshake が終わる前に server が閉じる)

  基準: UI テストが検査モードと速いモードで通り、どれも1秒未満。
- **Tls-U5.** 本物のソケットの Rust のテスト (`#[cfg(unix)]`、本物の driver、std の `TcpListener` と `rustls::ServerConnection` のスレッド、`Tls.connect_with_ca ca "localhost" port`) を `crates/eml_cli/tests/` に置く。testing.md:576 と CLAUDE.md の `.#cross` のコマンドを更新し、CI に clippy の行を足し、status.md を更新する。基準: 5.7。
- **Tls-U6.** 閉じる。roadmap.md と async.md の編集 (5.8)、bench の表。

`std-http`:

- **Tls-U7.** `Options`、URL、要求、状態行とヘッダーの解析と、平文の `Net` の UI テスト。
- **Tls-U8.** 本文と、切り詰めのテスト。
- **Tls-U9.** リダイレクトの規則、https のテスト (`tls.localhost`)、unix の本物のソケットのテスト。
- **Tls-U10.** `bench/scripts.sh` の行 `https-get`。
- **Tls-U11.** 閉じる。

### 5.6 `Std.Net` とテストの driver への必須の依存 (持ち主は Net の設計。明記したものを除く。括弧は Net の設計の提案)

1. `TcpStream` は `Lin` の `Foreign` で、破棄処理がソケットを閉じる (Net の設計: 同意)。
2. `connect : String -> Int -> <IO, Async> TcpStream` はホスト名を解決する (Net の設計: プールの `getaddrinfo`。名前を指す connect は残る)。受け入れの基準: `run/tls/echo` が `"tls.localhost"` に接続する。手で行う `https://example.com/` の確認は本物の resolver を使う。
3. `read_chunk` は 64 KiB までを足し、ちょうど EOF のときは何も足さず、EOF は失敗ではない。`read_line` の `rest` を先に返す (Net の設計: 同意)。そのため `Tls.wrap` は `Net.read_line` の後でも使える。
4. `read_chunk_within` は、stream を損なわず、バイトも失わずに `None` を返す (Net の設計: 従来どおり)。
5. `write_bytes` はすべてを書く。`write_within` はない (Net の設計: 同じ)。
6. `shutdown_write : TcpStream -> <IO> TcpStream`。
7. テストの driver の仮想のホスト: `listen` (IP リテラル、ポート 0 から `local_port`)、`accept`、`connect "localhost"`。`localhost` と IP リテラル以外の名前は `NotFound` (Net の設計: 同意。ポートは 49152 から)。`read_chunk` は、待ち行列にあるものをそのまま返す。
8. ECONNRESET、EPIPE、ECONNABORTED は `ConnectionReset` にする (Net の設計: 同意)。`Tls` は Net の失敗をそのまま通す。
9. **(持ち主: `net-tls`。Net の設計の構造の上に作る)** `TestDriver` は、終端器のモジュールが再利用する非公開の関数を2つ持つ。
   - (a) 名前の検査の関数1つ `fn virtual_name(&str) -> Option<Ipv4Addr-or-marker>`。`localhost` と IP リテラルを対応づけ、そのほかは `NotFound` にする。`tls` のモジュールが `tls.localhost` と `tls-cut.localhost` をここへ足す。
   - (b) `fn virtual_connect(&mut self, port: u16) -> Refused | Pending | Connected { client_end: IoHandle }`。クライアントのポートを割り当て、端の組を作り、server 側の端を listener の backlog の待ち行列に入れる。普通の `Op::Connect` が使うのと同じ関数である。

   終端器は平文の組のために (b) を呼び、暗号文の組は同じ端の組を作る補助で割り当てる。そのため、`tls.localhost` への1つの接続は、49152 から数えるカウンタのクライアントのポートを2つ持つ。eml の server の `peer_addr` は、平文の組のクライアントのポートを見せる。終端器は `settle()` の最初、操作を試す前に汲む。そのため Net の設計の settle の順、seed での並べ替え、仮想の時計は変わらない。Net の設計への要求は、(a) と (b) を単一の関数として保つこと (Net の設計の「実装に任せる点」の行に名前を挙げる) だけで、ほかにはない。

### 5.7 完了の基準

- `cargo nextest run` が通り、`cargo nextest run -j 1` で1秒を超えるテストがない。
- `cargo clippy --all-targets -- -D warnings && cargo fmt --check` が警告なしで通る。
- `cargo clippy -p eml_cli --no-default-features --features run -- -D warnings` が警告なしで通る (CI に足す新しい行)。
- `nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run -- -D warnings` が警告なしで通る。確かめた: stub を入れた手元の写しで 43 秒。`x86_64-unknown-linux-gnu` も同じ。
- `nix build` が成功する (Tls-U0)。
- `cargo tree -p eml_vm -e normal,dev -i eml_tls` と `cargo tree -p eml_io -e normal,dev -i eml_tls` が "did not match any packages" になる。確かめた: `eml_vm` は手元の写しで。`eml_io` は同じ仕組みによる。Miri のジョブは変わらない。
- CI が3つの OS で緑で、ジョブごとの時間の増加を計画に記録する。
- 手で確かめる: `timeout 20 cargo run -p eml_cli -- run <scratch>/https_get.em` が、`https://example.com/` に対して `200` を出力する (`std-http` の後)。
- テストの変更はすべて新規。`externs.rs` の許可一覧は種類2 (範囲は Tls-U2 に書いた)。

### 5.8 日本語の文書の案

(a) roadmap.md の「段の列」の、`async-net` の後、`net-windows` の前に置く。

```
- `net-tls`: `Std.Tls` のクライアントを入れる。TLS の状態は rustls の上の crate `eml_tls` が持ち、`eml_vm` の feature `tls` (既定で有効) で結ぶ。ソケットの I/O は `Std.Net` が行い、`Tls` の eml のコードが `TcpStream` と TLS の状態の間でバイトを汲む。server は作らない。テストの相手は、テストの driver の仮想のホスト `tls.localhost` が Rust で TLS を終端し、平文を `localhost` の listener へつなぐ。暗号は aws-lc-rs、根の証明書は webpki-roots で、追加の CA は PEM で渡す。最初に `nix build` と CI で C の依存が組めることを確かめる。前提: `async-net`。設計: [非同期の設計](async.md) の「TLS と HTTP」
- `std-http`: `Std.Http` の HTTP/1.1 のクライアントを、`Net` と `Tls` の上に eml で書く。`Content-Length`、chunked、EOF までの本文、リダイレクトを扱い、接続は要求ごとに閉じる。速さは `bench/scripts.sh` の行で測る。前提: `net-tls`。設計: [非同期の設計](async.md) の「TLS と HTTP」
```

(b) roadmap.md の「その後の項目」の「非同期」から、「TLS のプラグイン」と「`Http`」を消し、次を足す。

```
- TLS の server (`Tls.accept`、SNI で証明書を選ぶこと、クライアント認証、ALPN)
  - 後に回した理由: 目標は HTTPS のクライアントで、server は作らない。テストの相手はテストの driver が Rust で終端する。
  - 見直す条件: eml で HTTPS のサーバを書くとき。
- close_notify なしの EOF を EOF として読む `Tls` の読み
  - 後に回した理由: 既定は IO の失敗 `ConnectionReset` にした。長さを持たない本文 (EOF までの本文) が黙って切れるのを防ぐためである。
  - 見直す条件: close_notify を送らないサーバと話す必要が出たとき。
- OS の証明書ストアで検証すること (`rustls-platform-verifier`)
  - 後に回した理由: webpki-roots の根と `Tls.connect_with_ca` の追加の CA で足り、OS ごとの依存を増やさない。
  - 見直す条件: OS のストアにしかない CA を使うプログラムが出たとき。
- `Http` の HTTP/2 と ALPN、gzip、keep-alive の接続のプール、ストリーミングの本文、本文の大きさの上限、プロキシ
  - 後に回した理由: スクリプトの HTTPS のクライアントには、要求ごとに閉じる HTTP/1.1 で足りる。gzip は展開の extern がまだない。
  - 見直す条件: 大きな本文を少しずつ読むプログラムか、同じホストへ多数の要求を出すプログラムが出たとき。
- libcurl の上の HTTP のクライアント: 採らない。本物のソケットを要るので、テストの driver の仮想のホストの上の UI テストが書けず、`Net` を通らないのでほかのプロトコルに使える TLS の層にならない。OS ごとに違う TLS (OpenSSL、Schannel) と libcurl、zlib、pkg-config の依存も持ち込む。
  - 見直す条件: `Http` に HTTP/2、プロキシ、圧縮が要り、eml で書くより libcurl を結ぶほうが小さいと分かったとき。
- musl で静的に結んだ Linux の配布物 (`x86_64-unknown-linux-musl`)
  - 後に回した理由: TLS とは別の配布の話である。aws-lc-sys のために musl を対象にする C のツールチェーンを flake に足し、`nix build` の2つ目のパッケージと CI のジョブが要る。musl の名前の解決と確保の速さは glibc と違う。AOT の配布物は `libeml_rt.a` を同梱して利用者の `cc` で結ぶので、musl の配布物には musl 向けの `libeml_rt.a` と結び方を別に用意する必要がある。根の証明書は webpki-roots なので、静的な配布物でも OS のストアは要らない。
  - 見直す条件: Linux の tarball を配布するとき。既定の配布物とは別の配布物にする。
```

(c) async.md の編集:

- 12行目を次にする: `- 範囲の外: 時刻。下の「未決の論点と将来の拡張」に、後で足す形を書く。TLS と HTTP は下の「TLS と HTTP」にある`
- 「後で足すもの」から、TLS と HTTP の行を消す。405行目を次にする: `UDP、`SO_REUSEPORT`、非同期の名前の解決: 上の「ネットワークとシグナル」の「後で足すもの」にある`
- 「テストと決定性」(384行目) の、仮想のホストを述べた文の後に次を足す: `仮想のホストは、feature `tls` のある処理系では名前 `tls.localhost` を持ち、そこへの接続を Rust で TLS として終端して、平文を `localhost` の同じポートの listener へつなぐ (下の「TLS と HTTP」)`
- 「シグナル」の後に次の節を足す。

```
### TLS と HTTP

- TLS はプラグインにせず、普通の std のモジュール `Std.Tls` にする。利用者に見せるのはクライアントだけである。TLS の状態 `Tls.Conn` は rustls の接続と復号した平文の列を持つ `Lin` の `Foreign` で、I/O を持たない。`eml_vm` が `Foreign` を作り、extern の行の腕を持つ。`eml_rt` は変わらない
- ソケットの I/O はすべて `Std.Net` が行う。`Tls.Stream` は `Conn` と `TcpStream` を持つ eml の `data` で、`Tls` の eml のコードが、送る TLS のバイトを `Conn` から取り出して `Net.write_bytes` で書き、`Net.read_chunk` で読んだバイトを `Conn` に入れ、復号した平文を返す。待ち、取り消し、期限は `Net` のもので、`Tls` は何も足さない。`Tls` の extern は同期の行で、`Async` の行にしない。`Conn` の行は投げず、TLS の誤りは `Conn` に記録して、eml の側が警告を書き出してから IO の失敗にする
- rustls は受け取った平文を 16 KiB までしか持たないので、`Conn` は TLS のバイトを処理するたびに平文を自分の列へ移す。読みの道では、カーネルが書いた `Bytes` から rustls へ、rustls から `Conn` の列へ、列から `Bytes` へと3回写す。費用は `std-http` の `bench/scripts.sh` の行で測る
- 取り消しの巻き戻しは `Stream` を落とし、`Conn` はメモリだけを放し、`TcpStream` の破棄処理がソケットを閉じる。close_notify は送らない。`Tls.shutdown_write` と `Tls.close` が close_notify を書き出してから `Net.shutdown_write` する
- 相手が close_notify を送って閉じたら `read_chunk` は EOF を返す。close_notify なしに EOF が来たら IO の失敗 `ConnectionReset` にする。EOF までの本文が黙って切れないためである。`connect` と handshake に期限はなく、黙る相手は `Async.timeout` で切る
- rustls の crate `eml_tls` は `eml_vm` と `eml_io` の feature `tls` で結ぶ。既定で有効で、`eml_cli` の `--no-default-features --features run` で外せる。外した処理系では `Std.Tls` の行は IO の失敗 `not available` になる。`.#cross` の clippy は Windows の C のツールチェーンを持たないので、`eml_tls` を外して流す ([テスト戦略](../implementation/testing.md) の「OS ごとのテスト」)。`eml_vm` だけを組む Miri は `eml_tls` に届かない
- 暗号は rustls の既定の aws-lc-rs で、要るのは rustc がリンクに使うのと同じ C のツールチェーンだけである。Windows では NASM があれば使い、なければ rustls の feature が有効にする prebuilt の object を使う。根の証明書は webpki-roots で、`Tls.connect_with_ca` が PEM の CA を足す。OS の証明書ストアは使わない
- テストの相手は、テストの driver の仮想のホストが持つ。名前 `tls.localhost` への接続は、driver が `eml_tls` の server の状態で TLS を終端し、平文の端を `localhost` の同じポートの listener へ普通の接続としてつなぐ。`settle` のたびに暗号文と平文を両方向へ汲み、eml の側が閉じたら close_notify を送って閉じる。`tls-cut.localhost` は close_notify なしに閉じる相手である。証明書と鍵は `eml_tls` が持つテスト用の固定のもので、UI テストは隣に置いた `ca.pem` を `Tls.connect_with_ca` に渡す。handshake は録音で再生できない (クライアントの乱数が毎回変わる) ので、相手は生きた server でなければならない
- `Std.Http` は `Net` と `Tls` の上に eml で書く HTTP/1.1 のクライアントで、extern を足さない。`Http.get` と `Http.request` が `Connection: close` の要求を出し、`Content-Length`、chunked、EOF までの本文を読み、301、302、303、307、308 を `Options` の回数 (既定 5) まで追う。301、302、303 では GET に替えて本文を捨て、307 と 308 では同じメソッドと本文で送る。別のオリジンへのリダイレクトでは `Authorization` と `Cookie` を落とし、https から http へのリダイレクトは追わずに IO の失敗にする。`Options` は追加の CA も持つ。gzip、HTTP/2、keep-alive、ストリーミングの本文は持たない ([ロードマップ](roadmap.md) の「非同期」)
- libcurl は使わない。本物のソケットを要るので、テストの driver の仮想のホストの上の UI テストが書けず、`Net` を通らないので TLS の層にならない ([ロードマップ](roadmap.md) の「非同期」)
- TLS と HTTP の UI テストは `tests/ui/run/tls/` と `tests/ui/run/http/` に置き、仮想のホストの上で、平文の server のタスクと `tls.localhost` へつなぐ client のタスクを1つのプログラムで動かす。`async/` には置かない。TLS のレコードがまとまる数が seed で変わり、`completions` が seed によらないという `async/` の規則に合わないためである
```

- ffi.md の「拡張」と native.md は変えない。

## 6. 決めていない点と既定

- 借りた `Conn` を返すだけの `Unit` 結果の行: 既定は `with_arg` で、組の最初の要素に `Int` の 0 を返す。
- `Tls.read_line` と `Net.read_line` の共有: 既定は、`read_chunk` の関数を取る、ハンドルによらない補助とする。`Tls.Stream` は専用の `rest : Bytes` のフィールドを持つ。見て分かる規則は Net の設計の `Net.read_line` と同じにする。
  - 行の上限は `LINE_LIMIT = 1_048_576` バイトで、超えたら `fail_io Other "cannot read the stream: a line is longer than 1048576 bytes"` (バイトは残す)。
  - 不正な UTF-8 は `NotUtf8`。
  - `_within` は平文の結果の `Option` を返し、途中までの行を保つ。
  - 終端の扱いも `Net.read_line` と同じにする (終端を取り除くかどうかは Net の設計が決め、`Tls` はそれに従う)。
- provider の代替: 既定は aws-lc-rs。Tls-U0 が aws-lc-sys で失敗したときだけ `ring` を使う。
- テストの driver に終端器を作れない場合 (Net の設計の仮想のホストの形が、feature で分けたモジュールから拡張できない場合): 既定の代替は、Rust のテストだけにすること (本物の driver、`net-windows` までは `#[cfg(unix)]`)。`tls/` と https の UI テストは置かない。Tls-U3 で決める。
- 緩い EOF の版: 作らない (後回し)。`Http` の本文の上限: 置かない (後回し)。musl: この2つの段では扱わない。
- 終端器のホスト名: 既定は `tls.localhost` と `tls-cut.localhost` (予約済みの TLD `.localhost` の下。RFC 6761)。

## 7. 追加で確かめたこと

- workspace の `Cargo.toml:21-22`: `eml_cli` と `eml_test_support` の項目は、すでに `default-features = false` である。確かめた。
- 手元の workspace の写し (リポジトリの写しに、`#[cfg(windows)] compile_error!` を持つ stub の `crates/eml_tls` を足し、`eml_vm` に `tls = ["dep:eml_tls"]`、`eml_cli` に `default = ["run", "tls"]` と `tls = ["run", "eml_vm/tls"]` を足したもの) での結果。確かめた。
  - `cargo tree -p eml_cli -e normal -i eml_tls`: 届く。
  - `cargo tree -p eml_vm -e normal,dev -i eml_tls`: 一致なし。
  - `cargo tree --workspace -e normal,dev -i eml_tls`: 届く。
  - 5.7 の `.#cross` のコマンドそのもの: 終了コード 0、43 秒。
  - (`eml_io/tls` の feature はこの確認の後に足した。同じ仕組みに従う。)
- rustls 0.23.46 のソース: `received_plaintext.is_full()` のとき `read_tls` は `Other("received plaintext buffer full")` を返す (conn.rs:784-788)。`DEFAULT_RECEIVED_PLAINTEXT_LIMIT = 16 * 1024` (common_state.rs:1135)。`set_buffer_limit` が設定するのは `sendable_plaintext` と `sendable_tls` だけである (conn.rs:543-546)。確かめた。
- `StdHost::open` は `..` の検査なしに `file_root.join(path)` を結合する (host.rs:1306-1307)。確かめた。fixture はいずれにせよ各テストの隣に写す (testing.md:329)。
- devShell の `cc` は stdenv のものである。2つの C の provider はどちらも 1.99.1 で組める。どちらも `.#cross` の Windows の clippy で失敗する。rustls-pure の metadata。curl と curl-sys の feature と依存。以上、確かめた (curl は crates.io の API)。
- 確かめていないこと: aws-lc-sys での `nix build` (Tls-U0)、Windows の CI、macOS の devShell のコンパイラ、musl、curl の multi の動き、項目9に関するテストの driver の最終的な内部。
