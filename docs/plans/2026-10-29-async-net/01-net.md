# 付録1: `Std.Net` と名前の解決の設計

TCP の `Std.Net`、名前の解決、`Std.IoTest` の扱いを決める。この付録は実装の担当者がそのまま読めるように、決めた設計と作業単位 (Net-U1 から Net-U6)、テストの変更を書く。「(推定)」は確かめていない推定、「確かめた」は実際に確かめた事実に付ける。

## 1 結論

### 1.1 `Std.Net` の API

```haskell
pub extern data TcpListener                                                           -- Lin Foreign
pub extern data TcpStream                                                             -- Lin Foreign
pub listen            : String -> Int -> <IO, Async> TcpListener
pub local_port        : TcpListener -> (Int, TcpListener)
pub accept            : TcpListener -> <IO, Async> (TcpStream, TcpListener)
pub accept_within     : Int -> TcpListener -> <IO, Async> (Option TcpStream, TcpListener)
pub close_listener    : TcpListener -> <IO> Unit                                      -- eml: drop l
pub connect           : String -> Int -> <IO, Async> TcpStream
pub read_chunk        : Bytes -> TcpStream -> <IO, Async> (Bytes, TcpStream)
pub read_chunk_within : Int -> Bytes -> TcpStream -> <IO, Async> (Option Bytes, TcpStream)
pub read_line         : TcpStream -> <IO, Panic, Async> (Option String, TcpStream)
pub read_line_within  : Int -> TcpStream -> <IO, Panic, Async> (Option (Option String), TcpStream)
pub write             : String -> TcpStream -> <IO, Async> TcpStream                  -- eml: write_bytes (Bytes.of_string s)
pub write_bytes       : Bytes -> TcpStream -> <IO, Async> TcpStream
pub write_line        : String -> TcpStream -> <IO, Async> TcpStream
pub shutdown_write    : TcpStream -> <IO> TcpStream
pub close             : TcpStream -> <IO> Unit                                        -- eml: drop s
pub peer_addr         : TcpStream -> (String, TcpStream)
pub set_nodelay       : Bool -> TcpStream -> <IO> TcpStream
```

決めたこと:

- **`listen host port`** は `Async` を持つ。これは driver の操作なので、`AddrInUse` は `fail_io` を通ってユーザーに届く。決定 #8 は、extern が作る種類を最初の5つだけとしている。
  - `host` は IP アドレスの綴り (`"0.0.0.0"`, `"127.0.0.1"`, `"::"`, `"::1"`) に限る。名前は拒む。
  - `port` は 0 以上 65535 以下。0 なら空いているポートを選ぶ。
  - 本物の driver のソケットのオプションは、SO_REUSEADDR (unix)、backlog 1024、ノンブロッキング、CLOEXEC である。
- **`connect host port`** の `host` は IP アドレスの綴りか名前である。`port` は 1 以上 65535 以下。
  - 名前は、compio のプールではなく、本物の driver の解決のスレッド (§1.4) で解決する。
  - 取り消し、期限、Ctrl-C、`quiesce` は名前の解決を待たない。
  - 番地は、解決器が返した順に1つずつ試す。最後に試した番地の誤りを報告する。Happy Eyeballs は使わない。
- この stage に `resolve` は入れない (後に回す項目)。
- **`peer_addr`** は Rust の `SocketAddr` の Display を返す。例は `"127.0.0.1:49153"` や `"[::1]:8080"`。作ったときに保存しておく。
  - `peer_addr` と `local_port` は、効果を持たない、借用する行にする。これで確定である。先例は `Std.MutArray.length` で、`Lin` の引数を借用する `with_arg` 付きの `Pure` の行である (確かめた: eml_extern lib.rs:983-993)。
- **誤りの種類。** `IOErrorKind` に **`ConnectionRefused`** を tag 10 として、`ResourceLimit` の後ろに足す。既存の tag は動かさない。
  - `fail_io` が作る種類 (extern は作らない) は6つになる。`Cancelled`, `TimedOut`, `ConnectionReset`, `AddrInUse`, `ResourceLimit`, `ConnectionRefused` である。
  - Net の対応は次のとおり。
    - ECONNREFUSED → `ConnectionRefused`
    - ストリームでの ECONNRESET、EPIPE、ECONNABORTED → `ConnectionReset`
    - EADDRINUSE → `AddrInUse`
    - OS の ETIMEDOUT → `TimedOut`
    - 名前の解決の失敗 → `NotFound`
    - `Unsupported` → `NotAvailable`
    - ほかはすべて `Other`
  - EPIPE を `ConnectionReset` にするのは Net だけである。`Child` と `FileOut` の書きは変えない。これで roadmap の後に回した項目が片づく。
- **`accept`** は ECONNABORTED を driver の中で再試行する。EINTR は compio がすでに再試行している (確かめた: compio sys/pal/unix/mod.rs:47-62)。EMFILE は `Other` である。
- **`write_within` は作らない。** 書きの期限は `Async.timeout ms (fn () -> Net.write_bytes b s)` で付ける。期限が来ると巻き戻しがストリームを閉じる (後に回す項目)。
- **`_within` の形は規則である。** `f_within ms args` は、`f args` が返すものを `Option` に包んで返す。`None` は期限が過ぎたことを表す。このため型が `Option Bytes`、`Option TcpStream`、`Option (Option String)` になる。
- **読みの保証は、TLS の設計の §5.6 の約束である。**
  - `read_chunk b s` は 1 バイト以上 65 536 バイト以下を足す。EOF のちょうどそのときだけ何も足さない。EOF は失敗ではない。
  - 受け取ったバイトはすべて、順序どおりに `read_chunk` に届く。
  - `read_line` で一度も読んでいないストリームは、buffer にバイトを持たない。
  - `read_line` の後は、その行を読んで余ったバイトがストリームの buffer に残る。次の `read_chunk` はそれを先に、1回に 65 536 バイトまで、driver の操作なしで返す。このため `read_line` と `read_chunk` を混ぜても、失うバイトも順序の入れ替わりもない。
  - `read_chunk_within` が `None` を返すのは、期限そのものの取り消しで読みが終わったときだけである。このとき読んだバイトはない。期限と競ったデータは `Some` で返る。期限と競った本物の誤り (reset など) は、ふつうの失敗と同じく投げる。したがって `None` はバイトを失わず、誤りも隠さない。
- **`read_line` は終端なしの行を返す。** `\n` と `\r\n` は取り除く。`Stdin.read_line` と同じである。改行のない最後の行はそのまま返す。`None` は、残りなしの EOF を表す。
- **`read_line` はストリームごとの buffer `rest: Vec<u8>`** を、ストリームの Rust の値の中に持つ。
  - 登録表の1つの操作が、probe → `rest` への読み → `\n` の探索を繰り返す。このため `read_line_within` の期限は1行の全体に付く。
  - `None` を返しても、読みかけの行は `rest` に残る。
  - 1行は `LINE_LIMIT = 1_048_576` バイトまでである。超えると、操作は `Other` の `cannot read the stream: a line is longer than 1048576 bytes` で失敗し、バイトは残る。
  - `rest` が空になったら、その確保を解放する。
- **`write`、`write_bytes`、`write_line` はすべてを書く。** 本物の driver は、短く終わった送信を出し直す。

### 1.2 `Std.IoTest` を全部取り除く

取り除くもの:
- `std/IoTest.em` とその STD の項
- `Std.IoTest.*` の10行と型の行 `IoTestStream`
- vm.rs の該当する arm
- cells.rs の `IoStream` と `OpKind::Job`
- `crates/eml_vm/src/jobs.rs`
- `IoDriver::pair`

残して、Rust のテストで確かめるもの:
- eml_rt の `Op::Job`
- 本物の driver のプール。`Slot::Job`、`POOL_LIMIT = 32`、`quiesce` での仕事の待ち、`Drop` での鍵の漏れ
- 仕事をその場で動かすテストの driver

これは `ffi-blocking` が仕事を出すときに通る driver の trait である。
- eml_io の仕事のテストは `Op::Job` を直接出す。このテストは変えない。
- eml_vm には、仕事を出すものがなくなる。そのため、VM 側の「`exit` や `quiesce` は終わっていない driver の仕事を待つ」確かめは、stub の driver を使うテストに移す (§5.4)。

テスト専用の std API は残らない。仮想のシグナルは Signal の設計が持つ (`RunConfig` と UI テストの印)。

### 1.3 `--clock` を取り除く (Net-U1)

- `eml run` と `eml repl` から `--clock` を外す。CLI は常に `Clock::Real` を使う。
- `Clock`、`RunConfig::with_clock`、`Machine::with_clock` は今のまま残す。`Clock::Virtual` を選ぶのは、テストの道具 (UI テスト、REPL のトランスクリプト、Rust のテスト) だけである。
- あわせて、`Clock` の `FromStr` とその誤りの文面、main.rs の値を取るフラグの一覧にある `--clock` の項 (main.rs:167) も外す。
- ui.rs:274 の seed が合わないときのメッセージは `` `eml run --debug-heap --sched seeded:{seed}` on the virtual clock `` にする。CLI のフラグでは再現できなくなるためである。
- フラグを外すと、roadmap の後に回した項目「`Clock` と `--clock` の名前」が片づく。Net-U1 を最初に置くので、後の単位はフラグに触れない。

### 1.4 driver の trait (eml_rt) と本物の driver

```rust
pub enum Op {
    Readable { handle: IoHandle },
    Read { handle: IoHandle, buf: RawBuf },
    Write { handle: IoHandle, buf: RawBuf },           // writes all of ptr[0..cap]
    Listen { addr: SocketAddr },
    Accept { listener: IoHandle },
    Connect { host: String, port: u16 },
    Job(Box<dyn FnOnce() -> io::Result<usize> + Send + 'static>),   // as today
}
pub enum Done {
    Size(usize),                                       // Readable, Read, Write, Job
    Listener { handle: IoHandle, port: u16 },          // Listen
    Stream { handle: IoHandle, peer: SocketAddr },     // Accept, Connect
}
pub struct Completion {
    pub key: OpKey,
    pub result: io::Result<Done>,
    /// The op ended only because `cancel` (or `quiesce`) was called; `result` is then an `Err`.
    /// `false` for a result the op reached by itself, even if a cancel came later.
    pub cancelled: bool,
}
pub trait IoDriver {
    // now, wait, submit, cancel, completions, pending, waker, bin, close, quiesce: as today
    fn shutdown_write(&mut self, handle: IoHandle) -> io::Result<()>;          // synchronous
    fn set_nodelay(&mut self, handle: IoHandle, on: bool) -> io::Result<()>;  // synchronous
}
```

- **鍵。** 出した `Op` ごとに、その一生を通じて `OpKey` を1つ持つ。本物の driver の中では、1つの slot が compio の鍵をいくつか通ることがある。write の出し直し、accept の再試行、connect の試行である。
  - `cancel(op_key)` は `OpKey` で slot を探し、その時点の compio の鍵を取り消す。
  - slot は `cancelled: bool` も持つ。出し直しや次の試行を起こすはずの完了は、代わりに `Err(other("cancelled"))` で操作を終える。
  - 出し直しは `reap` の中で行うので、2つの compio の鍵の間に `cancel` の呼び出しが入ることはない。
  - `Completion::cancelled` の付け方:
    - 本物の driver は、取り消しを求められた slot が `ECANCELED` で終わったときに立てる。compio の poll backend は取り消しをこの形で報告する (確かめた: compio sys/driver/poll/mod.rs:541-542)。
    - driver 自身が `cancel` のために操作を終えたときも立てる。出し直しを止めたときや、解決を放棄したときである。
    - テストの driver は、`cancel` が settle で終わらせた操作に立てる。
    - それ以外の結果は `cancelled: false` である。取り消しを求めた後に届いた誤りも含む。
- **`Listen`** は `submit` の中で (owner のスレッドで socket2 の create、bind、listen を) 同期に行い、`done` を通して完了する。compio の `Decision::Blocking` のプールの道は通らない (確かめた: op/socket/poll.rs)。
- **`Accept`** は compio の `Accept` を使う。ペイロードは `pop` と `into_inner` から `(Socket2, SockAddr)` として得る (確かめた: 試作)。
- **`Connect`**: IP アドレスの綴りは、そのまま試行に進む。名前は `Resolver` に渡す。
  - スレッドの名前は `eml-resolve`、スタックは 256 KiB である。スレッドは `eml_io::spawn_masked` で作る。SIGINT と SIGTERM を止めておくので、これらは VM のスレッドに届く。「上限と割り込み」(vm.md) が、止めたスレッドの規則を持つ。その段落は Sig-U0 が書き、Net の設計はそれを引く。
  - **生きている解決。** 同時に走る生きている (放棄されていない) 解決は、`RESOLVE_LIMIT = 8` 本までである。それ以上の解決は、driver の中の FIFO で待つ。
  - **放棄した解決。** 放棄したスレッドは `RESOLVE_LIMIT` に数えない。詰まった解決が新しい解決を止めることはない。まだ戻っていない放棄したスレッドは `ABANDONED_LIMIT = 64` に数える。64 本が詰まっている間は、新しい解決はすぐ `Err(other("too many name lookups are stuck"))` で失敗する。ユーザーには `Other` の `cannot connect to {h}:{p}: too many name lookups are stuck` として見える。解決のスレッドはプロセスで最大 72 本となり、壊れた解決器は、止まったままではなく誤りとして現れる。
  - 各スレッドは関数 `lookup` を走らせ、`(OpKey, io::Result<Vec<SocketAddr>>)` を mpsc channel に送る。`wait` と `completions` は `try_recv` で取り出す。
  - **解決の取り消し。**
    - 走っている解決は、すぐ `Err(other("cancelled"))` と `cancelled: true` で完了する。スレッドは放棄され、後から出る結果は捨てる。
    - まだ待っている解決は、列から外し、スレッドを使わずに同じ形で完了する。
  - `quiesce` はすべての解決を放棄し、待たない。driver を落とすと受信側が落ち、走っているスレッドは自分で終わる。スレッドが持つのは `String` と `u16` だけだからである。
  - 既定の `lookup` は `std` の `(host, port).to_socket_addrs()` である。誤りは `io::Error::new(NotFound, e.to_string())` にする。空の一覧は `NotFound` の `no address` にする。
  - `RealDriver::with_lookup(lookup: fn(&str, u16) -> io::Result<Vec<SocketAddr>>)` は、テストと組み込み先が差し替えるための継ぎ目である。これはふつうの `fn` ポインタで、クロージャの状態を持たない。driver はスレッド間で動かすものを持たずに済む。`RealDriver::new()` は `with_lookup(std_lookup)` である。architecture.md の「`eml_io` の内部」に「テストと組み込み先のため」として書く。
- **`Write`**: `Ok(n)` で残りのバイトがあるときは、`RawBuf { ptr: buf.ptr.wrapping_add(off), cap: buf.cap - off }` で `Send` をもう一度出す。新しい `unsafe` は要らない。Linux では `SendFlags::NOSIGNAL` を付ける。Apple では、ストリームのソケットすべてに `set_nosigpipe(true)` を設定する。
- **`pending()`** は、compio の slot の数、`done` の数、放棄されていない待ちまたは実行中の解決の数の和である。`wait(None)` が `Ok(false)` を返すのは、この3つがすべて 0 のときだけである。
- **`quiesce`** は、捨てる完了のペイロードが持つハンドルを閉じる。
- **Windows (この stage):**
  - `Listen`、`Accept`、`Connect` は `done` を通して `Err(Unsupported)` で完了する。ユーザーには種類 `NotAvailable` で見える。例は `cannot listen on 127.0.0.1:0: unsupported` である。
  - `shutdown_write` と `set_nodelay` は `Err(Unsupported)` を返す。ストリームが作られないので、呼ばれることはない。
  - ソケットと `Resolver` は `#[cfg(unix)]` にする。socket2 は eml_io の `[target.'cfg(unix)'.dependencies]` の依存にする。
  - `spawn_masked` は2つの版を持ち、どちらも eml_io に移す。unix の版はシグナルを止め、`#[cfg(not(unix))]` の版はふつうの `builder.spawn` である。監視のスレッドはどの OS でもこれを使う。
  - `Done`、`Op`、`HandleBin` は OS によらない。テストの driver はどの OS でも同じに動く。
- **プール。** `Op::Job` のプールの意味は今のまま残す。`Asyncify`、`POOL_LIMIT = 32` スレッドまで、取り消せない、`quiesce` が待つ。名前の解決はプールを使わない。

### 1.5 テストの driver: 仮想のネットワーク

- **ホスト。** 仮想のホストは1つである。IP アドレスの綴りはどれもこのホストを指し、`localhost` は `127.0.0.1` に解決する。ほかの名前は `Err(NotFound, "unknown host")` になる。
- **ポート。** すべてのポートは1つの名前空間を共有する。リスナーかストリームの端が持つ間、そのポートは使用中である。
  - listen でも、接続するクライアントでも、ポート 0 は counter の次の空きポートを取る。counter は 49152 から始まり、65535 まで上がって 49152 に戻り、使用中のポートを飛ばす。
  - 16 384 すべてが使用中なら、listen は `Err(AddrInUse, "no free port")`、connect は `Err(other("no free port"))` になる。
  - 使用中のポートを明示した listen は `Err(AddrInUse)` になる。
- **Connect。** backlog の端が 1024 未満のリスナーが要る。そのときは端の対を作り、サーバー側の端を backlog に入れ、`Done::Stream{client end, peer = 127.0.0.1:<listener port>}` で完了する。
  - backlog が満杯なら、accept が席を空けるまで connect は待つ。
  - リスナーがなければ `Err(ConnectionRefused)` になる。
- **Accept。** backlog が空の間は待つ。空でなければ取り出し、`Done::Stream{server end, peer = 127.0.0.1:<client port>}` で完了する。
  - リスナーを閉じると、backlog に残る端も閉じる。そのクライアントは EOF を読む。
- **端** に `shut` の旗を足す。
  - 読み: 相手が閉じているか shut なら、受信箱が空になった後は EOF になる。
  - 書き: 相手が閉じているか、この端が shut なら `BrokenPipe` になる (ユーザーには `ConnectionReset` に見える)。
  - `shutdown_write` は `shut` を立てる。`set_nodelay` は `Ok` を返す。
- settle の順、seed によるシャッフル、仮想の時計は変えない。
- **net-tls のための継ぎ目 (TLS の設計の §5.6 の項目 9)。** net-tls は、eml_io のテストの driver にある次の2つの関数を、別の eml_io のモジュールから呼ぶ。
  - 仮想の名前の確認。`localhost` か IP アドレスの綴りならそのホスト、それ以外は `NotFound`。
  - `virtual_connect(port) -> Refused | Pending | Connected { client_end }`。ポートの割り当て、端の対、backlog への登録を行う。
  - どちらも `try_finish` にインライン化してはならない。

### 1.6 UI テストが driver を選ぶ方法

UI テストは常にテストの driver で走る (道具の側では `Clock::Virtual`)。今の testing.md:325 のとおりである。

- **本物のソケットを使うのは Rust のテストだけである。**
  - `crates/eml_io` の real.rs と src の単体テスト
  - `Clock::Real` を使う `crates/eml_vm/tests/api.rs`
  - どちらも `#[cfg(unix)]` とし、それぞれに `#[cfg(windows)]` の NotAvailable のテストを1つ足す。
  - 使うのは `127.0.0.1` の綴りだけである。名前の道は差し込んだ `lookup` 関数で試すので、結果は `/etc/hosts` に左右されない。
- **Net の UI テスト** は `tests/ui/{run,run-fail}/async/net_*.em` に置く。seed を変えた実行、`spin 40` の規則、RunStats の比較は、道具を変えずにそのまま受け継ぐ。
  - `tests/ui/run/net/` というディレクトリは作らない。
  - TLS の設計の後の `run/tls/` や `run/http/` の分類を妨げない。それは TLS の設計が行う道具の変更である。
  - 公開の `Tls.accept` は作らないので、TLS の後の stage は Rust 側の TLS の相手を仮想のポートにつなげられる。テストの driver はリスナーの表を内部に持ち続けるので、これは形を変えずに足せる。

### 1.7 RunStats

新しい欄は足さない。`completions` を操作ごとに次のように定義し、vm.md の「`RunStats`」に書く。

| 操作 | completions |
|---|---|
| listen, accept, connect | 1 (再試行、試行、解決は driver の中に留まる) |
| write_bytes | 1 (出し直しは driver の中に留まる) |
| read_chunk | 2 (probe と read)。readable になる前に取り消された、または期限が過ぎたときは 1。本物の driver で、期限が読みの途中に来たときは 2。`rest` から返したときは 0 |
| read_line | 読む周回ごとに 2。最後の周回は、readable になる前に取り消された、または期限が過ぎたときは 1。1行がすでに `rest` にあるときは 0 |

bench.rs (`completions: 0` が44) は影響を受けない。

## 2 理由

- **複数段の仕事は driver の中で行い、記録は3段の形を保つ。** connect、accept の再試行、write-all は、それぞれ1つの `OpKey` の下でつながる。登録表がつなぐのは `read_line` だけである。`rest` を持つのは VM だけだからである。これは Probe→Transfer の先例に合う (確かめた: cells.rs:1838-1860)。
- **プールではなく解決のスレッドを使う。** compio は、仕事が終わる前に `Asyncify` の鍵を落とすことを禁じている (確かめた: real.rs の Drop のコメント)。プールで解決すると放棄できない。切り離したスレッドなら次のようになる。
  - `quiesce`、Ctrl-C、`timeout`、`race` がすぐ動く。
  - 生きている解決は 8 本、放棄したスレッドは 64 本までなので、スレッド数に上限がある。詰まった解決が新しい解決を待たせることはなく、壊れた解決器は誤りとして現れる。
- **分類の道は1本** (`op_outcome` と `finish`、§5.2) で、決定 #8 を満たす。`Lin` を返す take は、値なしでは呼ばない。
- **`listen` は owner のスレッドで同期に動かす。** bind と listen はブロックしない。compio の `Blocking` の道を通すと、プールのスレッドを使うことになる。
- **`ConnectionRefused`** は connect の失敗でいちばん多く、`Other` とは違う対応 (再試行や間隔を置く) を求める。末尾に tag を足すだけなので、既存の tag は変わらない。
- **socket2 は直接の依存にする。** compio の `Connect::new` と `Bind::new` は `socket2::SockAddr` を取るが、compio はそれを `pub(super)` でしか再公開しない (確かめた: compio src/sys/mod.rs:50、unix の `pal::reexport` は空)。型が一致するよう、版は同じ 0.6 にする (確かめた: 試作)。
- **`--clock` の除去を最初に置く。** 後の文書がフラグに触れないで済む。

## 3 退けた案

- **compio のプールで名前を解決する。** 放棄できないので、`quiesce` と最初の Ctrl-C が際限なく待つことになる。
- **`Std.IoTest.checksum` をプールの足場として残す。** テスト専用の std API は置かないと決めた。
- **`Op::Job` とプールを今取り除く。** プールは Rust でテストして残す。`ffi-blocking` がちょうどこの trait を必要とする。残しても実行時の費用はかからない。
- **名前の解決を `Op::Job` で走らせる。** プールの仕事は放棄できない (compio の鍵が固定される) ので、`quiesce` と Ctrl-C が際限なく待つ。
- **`listen : <IO>`。** 決定 #8 の下では `AddrInUse` を作れない。
- **eml で書く、Rust の buffer 越しの `read_line`。** 期限が1行ではなく chunk ごとに付くことになり、行も3つ増える。
- **ストリームごとの buffer を eml のレコードにする。** 決定 #19 に反する。
- **`read_line_within` に `Line` の直和型を使う。** `_within` の一様な規則と、`Stdin` / `FileIn.read_line` の形が崩れる。
- **`peer_addr` に構造を持つ番地を返す。** 使うものがなく (TLS の設計の §5.6)、extern はタプルを作れない。
- **本物のソケットを使う UI テスト。** OS ごとの文言と時間が、testing.md:325 と :451 に反する。
- **`write_within`。** §1.1 を見る。

## 4 前提にした仮定

- **SIGPIPE。** Rust のバイナリは SIGPIPE を無視する (推定)。明示した NOSIGNAL と nosigpipe の設定は、組み込み先のためにある。
- **macOS。** kqueue の `Accept`、`Connect`、`PollOnce` は epoll と同じに動く (推定、CI で確かめる)。Apple では、compio が accept したソケットに CLOEXEC とノンブロッキングを設定する (確かめた: compio unix.rs の Accept::call)。Linux で accept したソケットはノンブロッキングである (確かめた: 試作)。
- **AddrInUse。** SO_REUSEADDR だけなら、同じ `127.0.0.1:p` への2つ目のリスナーは Linux と macOS で EADDRINUSE で失敗する (推定)。BSD では、REUSEADDR は特定のアドレスがワイルドカードの bind と共存することを許すだけである。
  - macOS の CI でこのテストが落ちたら driver の不具合である。Apple では SO_REUSEADDR を外し、Linux では残す。テストは変えない。
- **名前の解決の失敗。** getaddrinfo の失敗は `NotFound` に対応づける。文面は OS ごとに違い、どのテストも文面を確かめない。

## 5 実装への影響

### 5.1 制約

- **Drop の規則。** `NetStream { handle, peer: SocketAddr, bin, rest: Vec<u8>, start: usize }` と `NetListener { handle, port: u16, bin }` は `Foreign` かつ `lin` である。`Drop` はハンドルを `HandleBin` に積むだけにする。
- **ペイロードの Foreign。** `Done::Listener` や `Done::Stream` のペイロードは、回収の地点である `Registry::complete` で `Foreign` のワードになる。それ以降は `finish`、`take_*`、`sweep_all` が解放し、ハンドルは bin を通して閉じる。
- **`rest` を貸す。**
  - **probe が成功したとき:**
    - `start > 0` なら、先に `rest.drain(..start)` して `start = 0` にする。
    - 次に `rest.resize(len + READ_ROOM, 0)` する。`READ_ROOM = 65_536` (cells.rs:2006)。
    - `RawBuf { ptr: rest[len..].as_mut_ptr(), cap: READ_ROOM }` で `Op::Read` を出す。
  - `Ok(n)` のときは `rest.truncate(len + n)`、`Err` のときは `rest.truncate(len)`。
  - `start == rest.len()` になったら `rest = Vec::new()`。
  - eml_vm に新しい `unsafe` は入れない。1回の読みの周回で 64 KiB を 0 で埋めるが、安全なコードで済む代わりとして受け入れる。
- **`rest` の貸しが健全な理由。貸す記憶は eml の heap ではなく、`Vec` 自身の Rust の確保である。**
  - `Vec` や `Foreign` を動かしても、確保は動かない。
  - 確保を解放するのは `Vec` の drop だけである。それは `Foreign` の後始末を意味するが、操作が進行中の間は、操作の記録がそのストリームの唯一の参照を持つ (`Lin`、submit が消費する)。
  - 記録を解放するのは、完了の後の `take_*` / `finish`、または `quiesce` の後の `sweep_all` だけである。
  - `Ctx::recover` も `quiesce` の後に走る (vm.md:451)。そもそも `Foreign` は回復でコピーされない (`Unrecoverable`、object-model.md:123)。
  - 壊れた `Machine` は heap を落とさず漏らす (vm.md:452) ので、完了しなかった貸しが解放されることはない。
  - エイリアス:
    - カーネルまたは driver が書くのは、`Vec` の buffer のうち `rest[len..len+cap]` のバイトだけである。この buffer は eml の heap の外にある別の Rust の確保である。
    - 完了を回収するまで、そのバイトへの参照は作られない。VM が `Foreign` のペイロードに届くのは、`Heap::foreign`、`foreign_unique` (T3) と後始末を通してだけである。
    - 記録は、submit から `take_*` / `finish` まで、ストリームのワードを持つ。その間に extern がそれを借りることはできない。
    - checked モードの heap の走査は `Foreign` のペイロードを読まない。walk.rs は `Kind::Foreign` の block を freeze の確かめから外し (walk.rs:89-93, 158)、後始末は heap を落とすときだけ走らせる (walk.rs:328) (確かめた: walk.rs の grep)。walk.rs のほかの走査も同じであることを、実装者が確かめる。
    - `Vec` のヘッダ (ポインタ、長さ、容量) を読んでも、貸したバイトには触れない。
  - したがって T10 と eml_rt の信頼の一覧は変えない。
  - vm.md の `RawBuf` の規則と、real.rs の `Mem` の SAFETY コメントを書き直す。「記憶は `Bytes` の物体の空き (T10) か、操作の記録が持つ `Foreign` の `Vec<u8>` の空きである」。
  - Miri: cells.rs に足す新しいテストは `a_read_line_*` と名づけ、`cargo miri test -p eml_vm --lib cells::a_read` が走らせるようにする。
    - そのテストの stub driver は、完了を返す前に、貸した `RawBuf` のポインタ越しに書く。カーネルの代わりである。これで Miri は、貸し → 書き → truncate の一周のプロベナンスとエイリアスを確かめる。
    - Miri は本物のカーネルの書きを確かめない。その道は、Net-U4 と Net-U5 の api.rs にある本物の driver のテストが受け持つ。
- **読みの上限。** `lend_read_room` は、buffer にもっと空きがあっても、ちょうど `cap = READ_ROOM` を貸す (TLS の設計の 64 KiB の上限)。
- **worker。** ハンドルは、操作を完了した driver が作る。`async-threads` は、accept したソケットを、どの worker にも結びつけない状態で作らなければならない。

### 5.2 行、結果の表、包み

- **型の行:** `NetTcpListener` (`"Std.Net.TcpListener"`、`Lin`、`Obj`) と `NetTcpStream` (`"Std.Net.TcpStream"`、`Lin`、`Obj`)。
- **関数の行:** すべて `Extern::is_async` に入れる。`shutdown_write` と `set_nodelay` 以外はすべて private にする。

| 行 | 型 | 借用 |
|---|---|---|
| `submit_listen` | `String -> Int -> <IO> Async.OpId` | – |
| `submit_accept` | `TcpListener -> <IO> Async.OpId` | – |
| `submit_connect` | `String -> Int -> <IO> Async.OpId` | – |
| `submit_read` | `Bytes -> TcpStream -> <IO> Async.OpId` | – |
| `submit_read_line` | `TcpStream -> <IO> Async.OpId` | – |
| `submit_write` | `Bytes -> TcpStream -> <IO> Async.OpId` | – |
| `op_stream` | `Async.OpId -> <IO> (TcpStream, Async.OpId)` | token |
| `op_listener` | `Async.OpId -> <IO> (TcpListener, Async.OpId)` | token |
| `op_outcome` | `Async.OpId -> <IO> (Int, Async.OpId)` | token |
| `take_listener` | `Async.OpId -> <IO> TcpListener` | – |
| `take_stream` | `Async.OpId -> <IO> TcpStream` | – |
| `take_read` | `Async.OpId -> <IO> Bytes` | – |
| `take_line` | `Async.OpId -> <IO> String` | – |
| `finish` | `Async.OpId -> <IO> Unit` | – |
| `local_port` | `TcpListener -> (Int, TcpListener)` (Pure) | listener |
| `peer_addr` | `TcpStream -> (String, TcpStream)` (Pure) | stream |
| `shutdown_write` | `TcpStream -> <IO> TcpStream` | – |
| `set_nodelay` | `Bool -> TcpStream -> <IO> TcpStream` | – |

- **Wake。** 再開する `Wake` が `Cancelled` になるのは、タスクが取り消されたために記録が取り消しを求められた (`cancelling`) ときだけである。それ以外は、期限が来た (`timed_out`) ときも含めて `Woke` である。これは今の規則である (確かめた: cells.rs の complete)。
- **結果の表** (`op_outcome`)。最初に合った行を採る。

| 記録の状態 (最初に合った行を採る) | code | 包みがすること |
|---|---|---|
| `cancelling` (w = Cancelled) | 何でも (見ない) | `finish`、ハンドルを落とし、`raise_cancelled` |
| `Ok(_)` (`timed_out` でも同じ。データが期限と競った) | 0 ok | `take_*` → 結果 (`_within` は `Some`) |
| `timed_out` で、完了が `cancelled: true` (期限そのものの取り消しが終わらせた、または登録表が期限のために Probe を終わらせた) | 1 late | `finish` → `(None, handle)` |
| `Err` の ConnectionReset、BrokenPipe、ConnectionAborted (`timed_out` の有無を問わない) | 2 | `finish`、`fail_io ConnectionReset "{prefix}: connection reset"` |
| `Err` の ConnectionRefused | 3 | `finish`、`fail_io ConnectionRefused "{prefix}: connection refused"` |
| `Err` の AddrInUse | 4 | `finish`、`fail_io AddrInUse "{prefix}: address in use"` |
| `Err` の TimedOut | 5 | `finish`、`fail_io TimedOut "{prefix}: timed out"` |
| そのほかの `Err` (`timed_out` の有無を問わない) | 6 | `finish` が、種類 `io_kind(e)` (最初の5つ) で `"{prefix}: {e}"` を投げる |

- 記録は、完了の `cancelled` の旗を `OpPhase::Done` に保存する。
- 期限と競った本物の誤りは、`None` にせず報告する。誤りはそれを報告した syscall が消費するので、後の操作が同じ誤りをもう一度見ることはできない。code 1 は期限を要するので、見えるのは `_within` の包みだけである。
- **`take_*` と `finish`。**
  - `take_*` は code 0 を前提にする。そうでなければ内部の誤り `an op is taken after it failed` になる。
  - `take_line` は終端つきの行を返し、残りなしの EOF なら `""` を返す。UTF-8 でなければ `NotUtf8` を投げる。理由の文面は `Stdin.read_line` のものの前に `cannot read the stream: ` を付ける。その行は消費される。
  - `finish` はトークンを消費し、Done であることと submit したタスクを確かめ、記録とそれがまだ持つものを解放する。code 6 では、`cancelling` が立っていない限り投げる。
- **prefix:**
  - `cannot listen on {host}:{port}` と `cannot connect to {host}:{port}`。記録はこの prefix を `Box<str>` として持つ。
  - `cannot accept a connection`、`cannot read the stream`、`cannot write to the stream`。
  - submit のときの失敗は、submit の extern が種類 `Other` で投げる。
    - `cannot listen on {h}:{p}: the host is not an IP address` と `…: the port is out of range`
    - connect にも同じ2つがある。connect のポートは 1 以上 65535 以下である。
  - `shutdown_write` の失敗は `cannot shut down the stream: {e}`、`set_nodelay` の失敗は `cannot set the no-delay option: {e}` である。
- **std/Net.em の private な eml の補助:**
  - `raise_cancelled : Unit -> <IO> a`
  - `fail_code : Int -> String -> <IO> a` (code 2〜5 を、上の種類と接尾辞で `fail_io` にする)
- **包みの雛形:**

```haskell
read_chunk b s =
  if Async.cancel_requested () then
    drop s
    raise_cancelled ()
  else
    let (w, op) = Async.io_wait (submit_read b s)
    let (s, op) = op_stream op
    let (code, op) = op_outcome op
    match w with
      | Async.Cancelled ->
        finish op
        drop s
        raise_cancelled ()
      | Async.Woke ->
        if code == 0 then (take_read op, s)
        else
          finish op          -- raises for code 6; unwinding drops s
          drop s
          fail_code code "cannot read the stream"
```

- **登録表の動き:**
  - `submit_read` で `rest` にバイトがあるとき: `rest[start..]` から `min(残り, 65 536)` バイトを buffer に足し (必要なら広げる)、`start` を進め、driver の操作なしで `Done(Ok(n))` の記録を作る。`park_op` は Done の記録ならすぐ再開する (確かめた: cells.rs の park_op)。
  - `submit_read_line` で `rest[start..]` に `\n` があるときは、すぐ Done にする。
  - そうでなければ、行の連鎖を行う。Probe → `rest` への Read → `Ok(n)` のとき:
    - 改行があるか `n == 0` なら Done
    - `len - start ≥ LINE_LIMIT` なら `Done(Err(other("a line is longer than 1048576 bytes")))` (code 6)
    - それ以外は、新しい鍵でもう一度 Probe する
  - 取り消されたか期限が来たときは、確定したバイトを残して止める。

### 5.3 コミット単位の作業 (順に)

**stage をまたぐ順序:** Net-U1 → Sig-U0 → Sig-U1 → Net-U2 → Sig-U2 → Net-U3 → Net-U4 → Net-U5 → Net-U6 → Sig-U3 → Sig-U4 → Sig-U5。

Signal の設計と共有するファイルがあるので、順序が効く。
- `crates/eml_io/Cargo.toml` (libc は Sig-U0、socket2 は Net-U2)
- `crates/eml_io/src/real.rs` (`submit` の guard は Sig-U0、net の slot は Net-U2)
- `crates/eml_vm/src/monitor.rs` (`spawn_masked` の移動は Net-U2、監視を `Scheduler` に入れるのは Sig-U2)
- `crates/eml_vm/src/cells.rs`
- `crates/eml_cli/tests/ui.rs` (ラベルは Net-U1、印は Sig-U3)
- vm.md の「時計と driver」と「上限と割り込み」
- `crates/eml_vm/tests/io.rs`

Sig-U3 は、種類 2 の値を Net-U6 の後で計算する。

**どの単位も**、実装する内容の仕様の文書を含み、次をすべて、失敗も警告もなく通す。

```
cargo nextest run
cargo clippy --all-targets -- -D warnings
cargo fmt --check
nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings
```

**eml_vm を変える単位**は、Miri の2つの設定も通す。

```
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --lib cells::a_read
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::
```

それぞれ、`MIRIFLAGS` に `-Zmiri-tree-borrows` を足して、もう一度走らせる。

**Net-U1. `--clock` を取り除く。** 前提: HEAD。
- コード:
  - eml_cli の `main.rs`: `run` と `repl` からフラグを外し、main.rs:167 の項も外す。
  - `terminal.rs`: `clock` の引数をなくし、常に `Clock::Real` にする。
  - eml_vm の `driver.rs`: `Clock` の `FromStr` を外す。
  - ui.rs:274: ラベルを `` `eml run --debug-heap --sched seeded:{seed}` on the virtual driver `` にする (Sig-U3 がこれに続けて書き足す)。
- 文書:
  - vm.md 「時計と driver」の `Clock` の項: CLI は常に `Real` を使い、テストの道具が `Virtual` を選ぶ。
  - stdlib.md の「起動」と repl.md: フラグを外す。
  - testing.md の「seed で流す」: 再現の文面。
  - CLAUDE.md の Commands の行: `[--clock real|virtual]` とその説明を外す。
- テスト: §5.4 (cli.rs) を見る。
- 完了の条件:
  - `git grep -n -- '--clock'` が空である。
  - `cargo clippy -p eml_cli --no-default-features --features types -- -D warnings` (と `--features core`) が通る。

**Net-U2. driver の trait、本物の driver の net の操作、`Resolver`、仮想のネットワーク** (eml_rt、eml_io、eml_vm の機械的な追従)。前提: Net-U1、Sig-U0、Sig-U1。
- Sig-U0 が、eml_io に `libc` と、プールへ積むときのシグナルを止める guard (SIGINT と SIGTERM を止める) を足している。Net-U2 は `submit` に自前の guard を足さない。
- 内容:
  - §1.4 と §1.5、`Completion::cancelled`。
  - `Op::Write` を write-all にする。
  - `pair` は Net-U6 まで残す。`Op::Job` はずっと残す。
  - eml_vm 側の追従として、`Done::Size` を読み、`cancelled` を保存する。
- **`spawn_masked`:**
  - eml_vm の `monitor.rs` (両方の版、monitor.rs:97-124) から、Sig-U0 の guard を持つ eml_io のモジュールに移し、`pub fn spawn_masked` にする。
  - unix の版は、その guard で SIGINT と SIGTERM を止めて `builder.spawn` を呼び、元のマスクに戻す。今の「すべてのシグナル」より狭い。これは Signal の設計に合わせた。
  - `#[cfg(not(unix))]` の版は、ふつうの `builder.spawn` である。
  - `monitor.rs` は `eml_io::spawn_masked` を呼ぶ。解決のスレッドも使う。
- 仕様:
  - vm.md 「時計と driver」: trait、`Done`、`cancelled`、操作の意味、鍵、解決器とその2つの上限、テストの driver の仮想のネットワーク。
  - vm.md 「上限と割り込み」: Sig-U0 の、止めたスレッドの段落を引き、解決のスレッドをその一覧に足す。
  - stdlib.md の `Std.IoTest`: 「本物の driver では短いことがある」を消す。
  - architecture.md 「外部 crate」: socket2 の行 `=0.6.5` と、確認項目「socket2 の版は compio-driver が使う版に合わせる」。
  - architecture.md 「`eml_io` の内部」: `with_lookup` (「テストと組み込み先のため」)。
- real.rs に足す eml_io の新しいテスト。すべて `#[cfg(unix)]`:
  - `a_listener_on_port_zero_gets_a_port`
  - `a_second_listener_on_the_same_port_is_addr_in_use`
  - `an_accept_returns_the_connected_stream_and_its_peer` (127.0.0.1)
  - `a_connect_to_a_closed_port_is_refused`
  - `a_write_to_a_closed_peer_fails_with_reset_or_broken_pipe`: 書きは最大 1000 回。種類は {BrokenPipe, ConnectionReset} のどちらか。
  - `a_shutdown_write_gives_the_peer_eof`
  - `a_cancelled_accept_completes_with_an_error`: `cancelled: true` も確かめる。
  - `a_long_write_completes_once_with_its_whole_length`: 8 MiB。相手はスレッドで読む。
  - `a_cancelled_long_write_stops_resubmitting`: 相手は読まない。取り消し、完了がちょうど1つ届き、`pending() == 0` であることを確かめる。
  - `quiesce_drains_a_pending_accept_and_closes_discarded_streams`
  - `a_connect_by_name_tries_the_looked_up_addresses_in_order`: `with_lookup` が `[127.0.0.1:<closed>, 127.0.0.1:<listening>]` を返す。
  - `a_cancelled_lookup_completes_at_once`
  - `quiesce_does_not_wait_for_a_lookup`
  - `live_lookups_beyond_the_limit_wait_in_order`: ブロックする名前の解決を9つ出す。9つ目は、最初の8つのどれかが解放されたときにやっと始まる。
  - `abandoned_lookups_do_not_hold_the_limit`: ブロックする解決を8つ出してすべて取り消す。その後、速い名前の解決が完了する。
  - `a_lookup_fails_while_64_abandoned_lookups_are_stuck`: ブロックする解決を64出して取り消す。65個目は、詰まっているというメッセージですぐ失敗する。解放した後は、新しい解決が成功する。
  - `lookup` の `fn` は、ホスト名で動作を選ぶ。`block-N` は、テストごとの static な `(Mutex<bool>, Condvar)` で待つ。それ以外の名前は、すぐ `127.0.0.1` を返す。各テストは、終わる前にスレッドを解放する。
- real.rs に足す eml_io の新しいテスト。`#[cfg(windows)]`: `listen_accept_and_connect_are_unsupported`。
- test_driver.rs に足す eml_io の新しいテスト:
  - `a_virtual_listen_on_port_zero_counts_from_49152`
  - `a_virtual_port_counter_skips_ports_in_use_and_wraps`
  - `a_virtual_port_in_use_is_addr_in_use`
  - `a_virtual_connect_without_a_listener_is_refused`
  - `a_virtual_name_other_than_localhost_is_not_found`
  - `an_accept_waits_for_a_connect`
  - `a_connect_to_a_full_backlog_waits_for_an_accept`
  - `a_shutdown_write_gives_eof_and_the_peer_can_reply`
  - `closing_a_listener_closes_its_backlog`
  - `quiesce_closes_the_handles_of_discarded_completions`
  - `a_cancel_before_settle_marks_the_completion_cancelled`
- 完了の条件:
  - `cargo nextest run -p eml_io --test integration` が通る。
  - eml_rt の関門が通る。`cargo verus verify -p eml_rt` (誤り 0)、`verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` (指摘なし)、`cargo nextest run -p eml_rt`。
  - io.rs の期待値が変わらない。テストの driver は、すでに write を1回の完了で終える (確かめた: test.rs の `try_finish`)。
- 実装者に任せるもの: slot の配置、解決の待ち列、テストの driver の中のモジュールと型の名前。ただし §1.5 の継ぎ目の2つの関数は、別々の関数のままにする。

**Net-U3. `Std.Net` の接続の準備。** 前提: Net-U2 と Sig-U2。監視はすでに `eml_io::spawn_masked` を使っている。
- 行: `submit_listen`, `submit_accept`, `submit_connect`, `op_listener`, `op_stream`, `op_outcome`, `take_listener`, `take_stream`, `finish`, `local_port`, `peer_addr`, `shutdown_write`, `set_nodelay`。
- eml: `listen`, `accept`, `accept_within`, `connect`, `close`, `close_listener`, `raise_cancelled`, `fail_code`。
- あわせて、STD の項 `("Net.em", …)`、`NetStream`、`NetListener`、記録の種類 Listen、Accept、Connect、ペイロードの `Foreign` も入れる。
- **`ConnectionRefused` (tag 10)** を std/Prelude.em の `IOErrorKind` に入れる。tag を固定している場所は次のとおり。
  - `crates/eml_core_ir/tests/externs.rs:749-779`: `("ConnectionRefused", 10)` を末尾に足す (種類 2)。
  - `eml_rt/src/externs.rs:30-40` と `eml_vm/tests/tags.rs` が持つのは、最初の5つの tag だけである。extern は種類 10 を作らないので、変えない。
  - `eml_lir/src/lib.rs:1017-1023` (`IO_NOT_FOUND` から `IO_CANCELLED`) は変えない。包みは構築子を eml で名指しするので、Rust の定数は要らない。
  - **この単位では eml_rt のファイルを変えないので、Verus の関門は当てはまらない。**
- 仕様:
  - stdlib.md: 準備の部分を持つ新しい `## Std.Net`。
  - stdlib.md 「`try_io`」: 構築子は11、そのうち `fail_io` が作るのは6つ。
  - stdlib.md 「取り消し」と「時間」: Net の待ちの関数を挙げる。
  - stdlib.md: Windows での動き。
  - vm.md 「操作の記録」。
  - object-model.md: ハンドルの表。
  - core-ir.md: 行。
  - modules.md: std の一覧。
- 新しいテスト:
  - UI `run/async/net_listen_connect.em`
  - UI `net_accept_within.em`
  - UI `net_connect_errors.em`: 拒否、NotFound、AddrInUse、名前での listen、範囲外のポート。
  - UI `run-fail/async/net_accept_deadlock.em`
  - io.rs: `a_listen_counts_one_completion`, `an_accept_and_a_connect_count_three_completions`
  - cells.rs: `a_deadline_that_races_a_reset_reports_the_reset`。stub の driver が、期限が切れた後に `cancelled: false` の `Err(ConnectionReset)` を渡す。code 2 を期待する。
  - cells.rs: `a_deadline_cancel_ends_late` (同じで、`cancelled: true`。code 1 を期待する)。
  - api.rs の `#[cfg(unix)]`:
    - `a_real_connection_is_accepted`
    - `an_interrupt_stops_a_run_blocked_in_a_real_accept`
    - `an_interrupt_stops_a_run_waiting_on_a_lookup` と `a_timeout_around_a_slow_lookup_returns_none`。どちらも `Machine::with_driver(Box::new(RealDriver::with_lookup(block_10_s)))` と api.rs の3秒の番犬を使い、時間の上限は確かめない。
  - api.rs の `#[cfg(windows)]`: `a_real_listen_is_not_available`
  - `crates/eml_core_ir/tests/externs.rs` と `crates/eml_hir/tests/externs.rs` の許可の一覧。
- 新しい UI のファイルにはすべて `spin` を置き、start や spawn のたびにその後ろで `spin 40` を呼ぶ。
- 新しいテストはどれも、単独で1秒以内に通る (`cargo nextest run -j 1`)。
- 完了の条件は、次が通ること。
  - `cargo nextest run -p eml_hir --test integration externs`
  - `cargo nextest run -p eml_core_ir --test integration externs`
  - `cargo nextest run -p eml_vm`
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::)'`

**Net-U4. `Std.Net` のストリームの読みと書き。** 前提: Net-U3。
- 行 `submit_read`, `submit_write`, `take_read`。
- eml: `read_chunk`, `read_chunk_within`, `write`, `write_bytes`, `write_line`。
- Net のストリームでの記録の種類 Read と Write。`lend_read_room` はちょうど `READ_ROOM` を貸す。
- 仕様:
  - stdlib.md: `Std.Net` の読みと書きの部分と、§1.1 の読みの保証。
  - vm.md 「`RunStats`」: completions の表。
- 新しいテスト:
  - UI: `net_echo.em`, `net_read_before_write.em`, `net_read_chunk_within.em`, `net_cancel_read.em`, `net_write_after_close.em`, `net_shutdown_write.em`, `net_server_try_all.em`, `net_failed_child_cancels_accept.em`。
  - UI: `run-fail/async/net_read_deadlock.em`。
  - io.rs: `a_net_write_and_read_count_two_more_completions_than_setup`, `a_timed_out_net_read_counts_one_completion_and_one_timer`, `a_cancelled_net_write_wait_raises_cancelled`。
  - api.rs の `#[cfg(unix)]`:
    - `a_real_connection_carries_bytes_between_tasks`
    - `a_real_read_chunk_within_keeps_the_stream`: データがなく、20 ms の期限で `None` になる。その後、相手のタスクが書き、次の `read_chunk` がそのバイトを返す。
    - `a_real_write_of_8_mib_arrives_whole`: 相手のタスクが読んだ量を数える間に、8 MiB の `write_bytes` を行う。VM を通した出し直しを確かめる。
- 種類 2: cells.rs の `a_read_lends_a_roomy_buffer_in_place_and_grows_a_short_one` は `>=` の代わりに `==` を確かめる。
- 完了の条件: Net-U3 と同じ。

**Net-U5. `read_line` とストリームの buffer。** 前提: Net-U4。
- 行 `submit_read_line`, `take_line`。
- 行の連鎖、`LINE_LIMIT`、`rest` と `start`、`rest` から返す `read_chunk`。
- 仕様:
  - stdlib.md: `read_line` の項目 (終端の規則を含む) と、`_within` の規則。
  - vm.md 「操作の記録」: 行の連鎖と、submit の時点で Done になる記録。
  - vm.md: `Vec` を貸すときの `RawBuf` の規則。
- 新しいテスト:
  - UI `net_read_line.em`
  - UI `net_read_line_within.em`
  - cells.rs: `a_read_line_lends_the_stream_buffer_and_frees_it_when_empty`, `a_read_line_cancelled_during_its_transfer_keeps_the_bytes`, `a_read_line_past_the_limit_fails_and_keeps_the_bytes`
  - io.rs: `a_buffered_read_line_counts_no_completion`
  - api.rs の `#[cfg(unix)]`: `a_real_read_line_reads_lines_split_across_writes`。相手は `"ab"` を書き、20 ms 眠り、`"c\nd\n"`、続いて `"tail"` を書いて閉じる。期待するのは `abc`、`d`、`tail`、`None` である。その後、2本目の接続で `read_line` の後の `read_chunk` が、余ったバイトを返す。
- 完了の条件: Net-U3 と同じ。さらに、Miri の `cells::a_read` の2回の実行が、3つの `a_read_line_*` テストを挙げる。

**Net-U6. `Std.IoTest` と `pair` を取り除く。** 前提: Net-U5。
- §1.2 に挙げたものをすべて取り除く。
- `vm.rs` の `io_kind` (vm.rs:2124-2125): コメントは `Std.IoTest` の代わりに stdlib.md の `Std.Net` を引く。
- 仕様:
  - stdlib.md: 「`Std.IoTest`」を消す。
  - vm.md: checksum の遅延と `Std.IoTest` への言及を消す。`Op::Job` の記述は残し、「今は std に仕事を出す行はない」と書く。
  - object-model.md:145。
  - core-ir.md:76 と 217-245。
  - testing.md の `async/` の項: `io_*.em` を `net_*.em` に替える。プールのテストは Rust だけにする。
  - docs/README.md: 地図。
- テストの変更: §5.4。
- 完了の条件:
  - `crates/ std/ docs/spec docs/implementation tests/` に対する `git grep -n 'IoTest\|IoStream\|jobs::\|fn pair\|checksum'` が空である。
  - `Op::Job` が現れるのは、eml_rt、eml_io、文書だけである。
  - 共通のコマンドが通る。

**閉じるときの項目** (stage を閉じるコミットで行う)。
- status.md:
  - `Std.Net` は macOS と Linux で動き、Windows では NotAvailable になる。
  - `resolve` と `write_within` はない。
  - std の行でプールに仕事を出すものはない。
  - CLI は常に本物の時計で動く。
- architecture.md: eml_io の内部 (net の slot、`Resolver` とその2つの上限、`with_lookup`、`Op::Job` のために残したプール)。
- CLAUDE.md の eml_io の Architecture の行に「名前の解決は driver の解決のスレッドで行う (生きているのは 8 本、放棄したものは 64 本まで)」を足す。
- roadmap.md:
  - `async-net` の項と、後に回した項目「EPIPE を … ConnectionReset」「`Clock` と `--clock` の名前」を消す。
  - 「非同期の名前の解決」:
    - 理由: 「名前の解決は driver の解決のスレッド (同時に8本) で行い、取り消しと `quiesce` は待たない」
    - 症状: 「戻らない解決のスレッドが64本たまると、名前での `connect` は IO の失敗になる」
    - 見直す条件: 「その失敗か、8本の待ちが計測で見えたとき」
  - 「プールの仕事の予算」: 理由は「今は std に仕事を出す行がなく、プールは Rust のテストだけが使う」。見直す条件 (`ffi-blocking`) は変えない。
  - §6 の項目を足す。
- overview.md: 決定 #8 に `ConnectionRefused` を足す。
- async.md: プールでの DNS についての文を、解決のスレッドを指すように直す。
- ベンチマークを測り直す。

### 5.4 テストの変更とその種類

**種類 1: 成否の変更 (削除)。1件ずつ挙げる。** どれも理由は、`Std.IoTest`、`pair`、VM の仕事の記録、`--clock` を取り除くことである。各行には、意図を引き継ぐテストを挙げる。それは前の単位か同じ単位で足す。

UI テスト (Net-U6):
- `tests/ui/run/async/io_checksum.em`: 削除して Rust に移す。eml_io の `a_job_completes_with_its_value` が残る。
- `tests/ui/run/async/io_write_read.em` → `net_echo.em` (Net-U4)。
- `tests/ui/run/async/io_read_before_write.em` → `net_read_before_write.em` (Net-U4)。
- `tests/ui/run/async/io_read_within.em` → `net_read_chunk_within.em` (Net-U4)。
- `tests/ui/run/async/io_cancel_read.em` → `net_cancel_read.em` (Net-U4)。
- `tests/ui/run/async/io_write.em` → `net_write_after_close.em` (Net-U4)。
- `tests/ui/run-fail/async/io_read_deadlock.em` → `net_read_deadlock.em` (Net-U4)。

`crates/eml_vm/tests/io.rs` (Net-U6):
- `a_checksum_counts_one_completion`: 削除。操作ごとの数は Net のテストが固定する。
- `the_jobs_module_imports_only_std_and_eml_rt`: `jobs.rs` がなくなるので削除。
- `a_job_record_left_at_an_exit_does_not_leak` → 新しい `an_op_record_left_at_an_exit_does_not_leak`: accept が待っている間に `exit` し、その後に漏れを確かめる。

`crates/eml_vm/tests/api.rs` (Net-U6):
- `a_checksum_runs_on_the_real_pool`: 削除。プールは eml_io の `a_job_completes_with_its_value` と `a_job_pool_is_capped` が受け持つ。
- `a_cancelled_job_resumes_after_it_completes` → 新しい machine.rs の `a_cancelled_wait_resumes_only_after_the_driver_completes`。stub の driver は `cancel` を無視し、1回の `wait` の後で完了する。
- `an_exit_waits_for_a_running_job` → 新しい machine.rs の `an_exit_quiesces_pending_driver_work`。stub の driver の `quiesce` が、待っている操作を完了する。
- `a_real_pair_carries_bytes_between_tasks` → `a_real_connection_carries_bytes_between_tasks` (Net-U4)。
- `an_interrupt_stops_a_run_blocked_in_a_real_read` → `an_interrupt_stops_a_run_blocked_in_a_real_accept` (Net-U3)。
- `a_real_pair_is_not_available` (windows) → `a_real_listen_is_not_available` (Net-U3)。

そのほか:
- `crates/eml_io/tests/real.rs` の `pair_is_unsupported` (windows) → `listen_accept_and_connect_are_unsupported` (Net-U2 で足し、古いテストは Net-U6 で消す)。
- `crates/eml_cli/tests/cli.rs` の `clock_flag_is_accepted_and_rejected` (Net-U1): 削除。このテストは、ファイル名の後のフラグがスクリプトに渡る規則も確かめていた。その半分は、cli.rs の既存のテストが同じ規則を確かめていない限り、新しい `flags_after_the_file_go_to_the_script` (`--anything` を使う) として残す。

**受け入れる損失。** Net-U6 の後は、本物の driver で、プールの仕事を端から端まで走らせる VM レベルのテストがなくなる。本物の `quiesce` が走っている仕事を待つこと (vm.md:451) は、eml_io のテスト (`dropping_a_driver_waits_for_a_running_job`、`quiesce_drains_…`) だけが確かめる。stub の driver のテストが確かめるのは VM 側だけである。これは、テスト専用の std API を置かない方針の結果である。端から端までのテストは `ffi-blocking` が取り戻す。

**種類 2: 期待値の変更 (範囲)。**
- (a) REPL のトランスクリプト `crates/eml_cli/tests/repl/async/io_in_flight.repl` とその `.snap` を、その場で `Net` に書き直す (Net-U6)。
- (b) io.rs のストリームのテストを `Net` で書き直し、`(completions, parks, cancels)` の組が変わる (Net-U6): `a_write_counts_one_completion`, `a_write_and_a_read_count_three_completions`, `a_read_at_the_peer_eof_returns_the_buffer_unchanged`, `a_timed_out_read_counts_one_completion_and_one_timer`, `a_cancelled_write_wait_raises_cancelled`, `a_read_cancelled_during_its_transfer_raises_cancelled`。
- (c) **`pair` から接続に組み直す eml_io のテスト (Net-U6)。** 範囲は「`pair` を使っていた eml_io のテストはすべて、loopback の TCP 接続 (real.rs) か仮想の接続 (test_driver.rs) で組み直す。TCP や追加の準備の操作が違うところでは、確かめる値が変わりうる。誤りの種類、`pending()` の数、completions の数、送信 buffer を満たすのに要る大きさ (満杯の書きは 8 MiB) である」。
  - real.rs: `a_readable_probe_stays_pending_without_data`, `a_read_after_a_probe_returns_the_bytes`, `a_write_is_read_by_the_peer`, `a_cancelled_probe_completes_with_an_error`, `a_closed_peer_reads_eof`, `quiesce_drains_a_pending_probe_a_full_write_and_a_running_job`, `an_op_on_an_unknown_handle_fails`, `a_wake_from_another_thread_leaves_a_wait_working`。
  - test_driver.rs: `a_closed_peer_leaves_its_bytes_before_eof`, `a_write_to_a_closed_peer_is_a_broken_pipe`, `a_cancel_before_return_ends_the_op_at_the_next_settle`, `a_cancel_after_the_op_finished_keeps_its_result`, `a_seed_does_not_change_which_ops_finish`, `the_bin_is_emptied_before_ops_settle`。
- (d) `crates/eml_core_ir/tests/externs.rs` の許可の一覧と行の数 (~275, 471, 509, 541-544)、externs.rs:779 の tag の一覧 (`("ConnectionRefused", 10)`)、`crates/eml_hir/tests/externs.rs` (Net-U3 から Net-U6)。
- (e) `IOErrorKind` の構築子の後の `<std>/Prelude.em:N` の行を引く snapshot。範囲は、構築子を足したために位置が動くすべての snapshot (Net-U3)。
- (f) cells.rs の `a_read_lends_a_roomy_buffer_in_place_and_grows_a_short_one`: `>=` を `==` にする (Net-U4)。
- (g) ui.rs の seed が合わないときのラベル。`a_seeded_mismatch_names_the_seed` は今も `--sched seeded:7` を確かめる (Net-U1)。

**種類 3: 機械的な追従 (期待値はバイトまで同じ)。**
- すべての stub の driver に `Done::Size` と `cancelled` の欄を足す (Net-U2)。
- machine.rs の `WAITS_ON_A_WRITE` を、確かめる誤りは同じまま、`Net.listen` だけのプログラムにする (Net-U3)。
- cells.rs の単体テストを `IoStream` から `NetStream` に移す (Net-U6)。確かめる内容が変わるものは、範囲 (c) の下で種類 2 に移す。
- `monitor.rs` が `eml_io::spawn_masked` を呼ぶ (Net-U2)。
- eml_io の仕事のテストは変えない: `a_job_completes_with_its_value`, `a_job_pool_is_capped`, `dropping_a_driver_waits_for_a_running_job`, `quiesce_drops_every_pending_op_and_runs_jobs`。

### 5.5 仕様の文面の案

仕様に書く日本語の案である。書くときは natural-japanese の規則に従って整える。

- stdlib.md の `Std.Net` の冒頭: 「`Std.Net` は TCP のモジュールである。`TcpListener` と `TcpStream` は `Lin` の `Foreign` で、落とすとソケットを閉じる。待つ関数はどれも取り消しの地点で、受け付け、待ち、取り出しの3段で動く ([VM](vm.md) の「操作の記録」)。」
- 「`listen host port` の `host` は IP アドレスの綴りで、名前は受け付けない。`port` が 0 なら空いているポートを使い、`local_port` で読む。」
- 「`connect host port` は、`host` が名前なら driver の解決のスレッドで名前を解決し、得た番地を順に試す。最後に試した番地の誤りを報告する。取り消し、期限、`quiesce` は名前の解決を待たない。」
- 「誤りの種類は、拒まれた接続が `ConnectionRefused`、reset と EPIPE が `ConnectionReset`、使用中の番地が `AddrInUse`、OS の時間切れが `TimedOut` である。この4つは包みが `fail_io` で投げる。」
- 「`_within` の関数は、期限のない関数の結果を `Option` に包んで返す。`None` は期限が過ぎたことだけを表す。」
- 「`read_chunk` は1回に 65 536 バイトまでを足す。EOF では何も足さない。`read_line` が読みすぎたバイトはストリームの buffer に残り、次の `read_chunk` が先に返す。`read_chunk_within` が `None` を返したときは、読んだバイトはない。」
- 「`read_line_within` の期限は1行の全体に付く。期限で `None` を返しても、読んだ途中の行は buffer に残る。1行は 1 MiB までである。」
- 「書きの関数は buffer をすべて書く。期限付きの書きは `Async.timeout` で書き、期限が来るとストリームは巻き戻しで閉じる。」
- 「Windows の本物の driver はまだソケットを扱わないので、`listen` と `connect` は種類 `NotAvailable` の IO の失敗になる。」
- vm.md のテストの driver: 「テストの driver は仮想のホストを1つ持つ。IP アドレスの綴りはどれもこのホストを指し、`localhost` は `127.0.0.1` に解決する。ほかの名前は `NotFound` で終わる。ポート 0 は 49152 から 65535 の間で、使っていないポートを順に振る。」

## 6 決めていない点と既定

- stage を閉じるときに後に回した項目として足すもの (それぞれに「後に回した理由」と「見直す条件」を付ける):
  - `write_within`。既定は `Async.timeout` で、ストリームを失う。見直すのは、書きの期限の後もストリームを保たなければならないプロトコルが出たとき。
  - `resolve` と、名前での `listen`。既定は、`listen` は綴りだけを取り、名前を取るのは `connect` だけ。見直すのは、プログラムが番地の一覧や名前での bind を要するとき。
  - Happy Eyeballs。既定は、順に試す。見直すのは、IPv6 が壊れたネットワークで connect が遅れるとき。
- 固定する定数。ユーザーの設定は付けない: `LINE_LIMIT` (1 MiB)、`RESOLVE_LIMIT` (生きているもの 8)、`ABANDONED_LIMIT` (64)、仮想の backlog (1024)。
- Signal の設計との調整:
  - stage をまたぐ順序と共有するファイルは §5.3 にある。
  - Net の設計が足す timer wheel の項は、Sig-U3 が入った後は `Due::Task` になる。決めた順序では、Sig-U3 がそれを変える。
  - vm.md の、止めたスレッドの段落は Sig-U0 が持ち、Net-U2 がそれを引く。
  - Sig-U3 は、Net-U1 のラベル「on the virtual driver」の後ろに、シグナルの接尾辞を書き足す。
- TLS の設計: §5.6 の項目 1 から 9 は成り立つ。
  - 項目 3: `read_line` で一度も読んでいないストリームは、buffer にバイトを持たない。それ以外のストリームでは、`rest` を先に空にする。
  - 項目 9: §1.5 の継ぎ目の2つの関数。
  - `Tls.read_line` は `Net.read_line` と同じにする。1 MiB、同じ文面、`NotUtf8`、`None` を返しても読みかけの行を残すこと、結果に終端がないこと。

## 7 追加で確かめたこと

- 確かめた: 作業用のコピーの HEAD で `cargo test -p eml_io --test integration` が 25 件通った (Linux)。
- 確かめた: 作業用の試作 (`crates/eml_io/tests/net_probe.rs`、dev-dep の `socket2 = "0.6"`) が Linux で通った。
  - socket2 0.6 の `SockAddr` は compio のものと型が一致する。
  - `Asyncify` は `pop` と `into_inner` を通して `D` を返す。
  - ノンブロッキングの `Connect` は `Ok(0)` で完了する。
  - `Accept` は `(Socket2, SockAddr)` を返し、accept したソケットはノンブロッキングである。
  - 閉じたポートは `ConnectionRefused`、`Connection refused (os error 111)` になる。
  - この環境では `localhost` は 127.0.0.1 にだけ解決した。本物のテストが綴りと注入した lookup を使うのはこのためである。
- 確かめた: `READ_ROOM = 65_536` (cells.rs:2006)。今のテストは `cap >= READ_ROOM` を確かめている (cells.rs:2794)。今の貸しが 64 KiB を超えうるので、Net-U4 で変える。
- 確かめた: `Op::Job` を作るのは `jobs.rs` だけである (その import のテストも含む)。
- 確かめた: IO の tag の定数の場所。`eml_rt/src/externs.rs:32-40` (tag 0-4)、`eml_lir/src/lib.rs:1017-1023` (tag 0-5)、`eml_vm/tests/tags.rs` (tag 0-4)、`eml_core_ir/tests/externs.rs:749-779` (`("ResourceLimit", 9)` までの全部)。
- 確かめた: monitor.rs には `spawn_masked` が2つの版ある。unix はシグナルを止め、unix 以外はふつうの spawn を呼ぶ (monitor.rs:97-124)。
- 確かめていない: macOS (kqueue、REUSEADDR 付きの EADDRINUSE)。
