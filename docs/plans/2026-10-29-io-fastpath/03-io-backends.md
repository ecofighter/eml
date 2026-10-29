# 付録3: 後の段 io-backends の設計

この付録は後の段 `io-backends` の設計である。io-fastpath では実装しない。`io-backends` の段の最初のコミット (この付録の18節の T-0) が、この付録から段の計画書を作る。そのとき、本体の5.1節の D-1、D-2、D-5、D-8、D-9 の規則の文を計画書の方針の節に写す。io-fastpath の本体と食い違うときは本体に従う。

## 0. 読み方と前提

- 前回の計画書 (41b4f3d の `docs/plans/2026-10-29-io-backends.md` と付録 `01-core-unix.md`、`02-iocp.md`。io-fastpath の T-0 が消した) の中身のうち、今も成り立つものはこの付録に写してある。前回の文を git から写し直さない。前回の文には、この付録で置き換えた古い形 (level の登録、`IDLE_TICK`、`completions` の並べ替え、鍵を振る `AtomicU64`、`Lent::of(&mut Vec<u8>)`、読みと書きをまとめて「一意に保ち」と書く SAFETY) が残っている。
- 試作は scratch の上で、新しい trait に直した核と epoll と偽の backend を組んで流したものである。試作のファイルはリポジトリに残らないので、要る中身はここに書き写してある。kqueue と IOCP と Miri は試作で流していない。
- 行番号は 41b4f3d で読んだものである。io-fastpath で動くので、関数、テスト、節の名前で引く。
- コード片のコメントは仮の文である。写すときに日本語の規則を当て直し、`docs/` の節を引く形にする。計画の番号と `docs/plans/` は書かない。

io-fastpath が閉じた後の形として、次を前提にする。崩れたときの読み替えは19節の U-6 にある。

| ID | 前提 | T-0 の確かめ |
|---|---|---|
| A-1 | `driver.rs` は io-fastpath の付録1 の1節の文のとおりで、compio に固有の文 (プールが満ちたら待つ、`quiesce` の 5 秒) を持たない。`wait` の `u64::MAX` は期限なし、ほかの期限は driver が `i32::MAX` ミリ秒で切ってよい (早く戻るのは約束の内)。`cancel` は呼んだ時点で保留の鍵にだけ効き、後で同じ鍵で出した操作に効いてはならない。`Submit::Done` と完了を返した鍵はすぐ使い回してよい | `git grep -n '安全点を通らずに' crates/eml_rt/src/driver.rs` → 0行 |
| A-2 | compio の `real.rs` は `submit(key, op) -> Submit` を持ち、`IDLE_TICK` を持たない。`quiesce` だけが私的な `CANCEL_POLL` (10 ms) と `CANCEL_LIMIT` を持つ。`lib.rs` は `IDLE_TICK` を出さない | `grep -rn IDLE_TICK crates` → 0行 |
| A-3 | `tests/real.rs` は呼ぶ側の鍵 (テストが 1 から振る) と補助 `run_op` を使い、`Submit::Done` と `Pending` のどちらで終わっても値を比べられる。io-fastpath が足した `a_read_after_a_finished_probe_returns_done` (`#[cfg(unix)]`)、`a_wake_ends_a_wait_with_a_far_deadline`、`a_wait_with_no_ops_and_a_far_deadline_wakes`、`a_wait_without_ops_sleeps_until_its_deadline`、`a_pending_key_submitted_twice_panics_in_debug`、`a_cancel_after_completion_does_not_touch_the_next_op_of_the_key` がある。`live_lookups_beyond_the_limit_wait_in_order` は `poll_for` で待ち、刻みに頼らない | `git grep -n 'fn a_wait_with_no_ops_and_a_far_deadline_wakes' crates/eml_io/tests/real.rs` → 1行 |
| A-4 | VM の集める地点は io-fastpath の形である。`switch_away` の頭の集める地点は OS を見ない。切り替えが続くときは 16 回に1回、横取りと `NOTICE` の集める地点ではいつも、`wait(Some(now))` で OS を1回見てから集める | `git grep -n 'POLL_EVERY' crates/eml_vm/src/sched.rs` → 定数の行 |
| A-5 | `Machine::drop` のコメントは中立の文になっている。io-backends は `crates/eml_vm/src` に触れない | `git grep -n '仕事を待ちきる' crates/eml_vm/src/lib.rs` → 0行 |
| A-6 | `bench/net/` に `bench.py`、`run.py`、`report.py`、`cases.tsv` (`compute-fib` と `busy-self` の行を含む) があり、`eml run --stats` がある。benchmarks.md の「ネットワークの計測」に1点目と2点目の列がある | `ls bench/net/cases.tsv` と `grep -n 'busy-self' bench/net/cases.tsv` |
| A-7 | `docs/plans/` が空である | `ls docs/plans` → 空 |

## 1. 前回の設計から変わる所と変わらない所

| 事柄 | 前回 | この設計 |
|---|---|---|
| 鍵 | `Shared::key()` の `AtomicU64` で driver が振る | 呼ぶ側が振る。`Shared` は受け箱と waker だけ。表は呼ぶ側の鍵で引く (`HashMap<OpKey, Record<B>>` のまま) |
| `submit` の答え | その場の終わりも `done` に積む | 核の関数が `Settled` を返し、`submit` は `Submit::Done` で返す。`done` に積むのは待ちの後の終わり、取り消し、受け箱の誤りだけ |
| 名前の解決 | 鍵で答えを当てる | driver の中の札 (`u64`) で当てる。手放した解決の答えが、使い回した鍵の別の操作に当たらない |
| `wait` | `IDLE_TICK` で1回の待ちを切る | 刻みなし。完了、期限、起こしでだけ戻る。何も終えない事象と割り込みの後は核が待ち直す。`u64::MAX` の期限は期限なし |
| `cancel` | 鍵で表を引く | 同じ。表にある (保留の) 鍵にだけ効き、返した後の鍵や、後で同じ鍵で出した操作に効かない |
| `completions` | I/O があれば毎回 `poll(ZERO)`、鍵で並べる | 届いた完了を移すだけで、OS を見ない。並べない。OS を見るのは `wait` だけで、`wait` は期限が今以前でも OS を1回確かめてから戻る |
| epoll と kqueue の関心 | level。列が空でない間だけ登録 (ADD/MOD/DEL) | 作ったときに1回だけ `EPOLLET` / `EV_CLEAR` で登録し、操作ごとの `epoll_ctl` を出さない。fd ごとに読める記憶と、読み・書きの列を持つ |
| `Readable` | 試さずに列に積む | 記憶が下りていれば syscall なしで列に積む。立っていれば1バイトを覗いて (`MSG_PEEK`) 確かめる。読み尽くした後は `Pending` |
| fd の状態の表 | `HashMap<RawFd, FdQueue>` | fd の番号で引く `Vec<FdState>` と登録の世代。閉じた fd の欄は、同じ番号の次の登録で初めに戻す |
| `quiesce` と `Drop` の待ち | `poll(Some(IDLE_TICK))` | release は `poll(None)`。debug は見張りの残りを期限にする |
| 核のテスト | 23 本と ignored 1 本 | 31 本と ignored 1 本 (9節)。`sys::tests` は 5 本から 9 本 |
| `tests/real.rs` の新しいテスト | 6 本 | 7 本 (12節)。G-4 の 1 ms の上限のテストを含む |
| 変えないもの | | 核と backend の分担 (2.2節の表)、`Backend` の trait の形、`Lent` と unsafe の一覧と数えるテスト (読みと書きの SAFETY の文は分ける)、プールと `Shared`/`Local` の分け方、`Drop` と `drain_broken` と abort の規則、偽の backend の台本、aarch64-linux の flake と CI、ASan、Miri の手順、compio を消す順 (差し替えの直後) |

## 2. 核 (`src/real/`)

### 2.1 置き場所と可視性

- module は private にし、中の項目は `pub` にする。`pub(crate)` にすると、`pub type RealDriver = real::Driver<sys::Backend>;` が `Driver<B>` と `B: Backend` を公開の型に出し、`private_interfaces` と `private_bounds` の lint が `-D warnings` で落ちる (前回の試作で確かめた)。
- 最後の形 (T-7 の後) のファイル:

| ファイル | 中身 | unsafe |
|---|---|---|
| `src/lib.rs` | `#![deny(unsafe_code)]`。公開は `RealDriver`、`TestDriver`、`spawn_masked` | なし |
| `src/buf.rs` | `Lent` (8節) | 許す |
| `src/mask.rs` | 今の `mask.rs`。`Masked` は `cfg(any(not(unix), miri))` で何もしない型 | 許す |
| `src/real/mod.rs` | `Driver<B>`、`Local<B>`、`IoDriver` の実装 | なし |
| `src/real/backend.rs` | `trait Backend` と境界の型 (2.3節) | なし |
| `src/real/table.rs` | `Table<B>`、`Record<B>`、`Work<B>`、`IoOp<B>`、`Io<S>` (2.4節) | なし |
| `src/real/shared.rs` | `Shared`、`Message` (2.4節) | なし |
| `src/real/pool.rs` | プール (6節) | なし |
| `src/real/resolve.rs` | 解決器 (6節) | なし |
| `src/real/tests/{mod,fake}.rs` | 偽の backend と核の単体テスト (9節) | なし |
| `src/sys/mod.rs` | `sys::Backend` の cfg の表 (5節) | なし |
| `src/sys/socket.rs` | socket2 の上の安全な補助 (5節) | なし |
| `src/sys/tests.rs` | OS の backend のテスト (10節) | なし |
| `src/sys/unix/{mod,ops}.rs` | 両方の backend が共有する手順 (3節) | なし |
| `src/sys/unix/doorbell.rs` | 起こしの物 (4節) | 許す |
| `src/sys/unix/epoll.rs` | Linux の backend | 許す |
| `src/sys/unix/kqueue.rs` | Apple の backend | 許す |
| `src/sys/windows/{mod,iocp}.rs` | Windows の backend (13節) | `iocp.rs` だけ許す |
| `src/test.rs`、`src/test/net.rs` | テストの driver。`buf.rs` を使う | なし |
| `tests/main.rs` | `#![deny(unsafe_code)]`、module の宣言、`use eml_io::RealDriver;` | なし |
| `tests/real.rs` | 本物のソケットのテスト | 許す |
| `tests/unsafe_code.rs` | unsafe を数えるテスト (8節) | なし |

`mask.rs` は `src/` の直下に残す。Windows の何もしない型と全 OS の `spawn_masked` が同じファイルにあるためである。

### 2.2 核と backend の分担 (G-2 の表)

| 事柄 | 置く所 |
|---|---|
| `EINTR` でその syscall をやり直す | backend |
| `EAGAIN` / `EWOULDBLOCK` で列に積み、読めた後にもう一度 `EAGAIN` なら列の先頭に戻す (1つの操作の中で待ちを張り直す) | backend |
| `connect` の `EINPROGRESS` の後、書けるようになってから `SO_ERROR` を読んで結果にする | backend |
| `accept4` の `SOCK_NONBLOCK \| SOCK_CLOEXEC`、Apple では受けた後の `set_nonblocking` と `set_cloexec` と `SO_NOSIGPIPE` | backend |
| epoll の `HUP` / `ERR` / `RDHUP`、kqueue の `EV_EOF` は、読める (書ける) として扱い、次の syscall に誤りを言わせる | backend |
| 1つの fd に方向ごとの列と読める記憶を持つ | backend |
| ソケットを作るときの登録 (`listen`、`socket`、受けたソケット) | backend |
| IOCP の packet の後の `recv` がブロックするときに長さ 0 の受けを出し直す | backend (net-windows) |
| `ECONNABORTED` で `Accept` を出し直す | 核 |
| 短く終わった `Write` を残りの位置から出し直す。0 バイトの進みは `WriteZero` | 核 |
| `Connect` の次の番地を試す (新しいソケットを作って `start`)。最後の番地の誤りを返す | 核 |
| `cancelled` の印がある記録の続きを出さず、`cancelled` の誤りで終える | 核 |
| `Completion::cancelled` を立てるかの判定 (取り消しを頼み、結果が backend の取り消しの誤りのとき) | 核 (`Finished.cancelled` を見る) |
| `Read` と `Write` の `cap == 0` を backend を呼ばずに `Ok(Size(0))` で終える | 核 |
| `Listen` (socket2 で同期) | backend の `listen` (`sys/socket.rs` の補助を呼ぶ) |

- 読み: この設計の核は OS をまったく呼ばない。ソケットの作成と `Listen` は backend の `listen` と `socket` が `sys/socket.rs` の補助を呼んで行う。設計の検討で、この形は G-2 の範囲と認めた。
- `src/real/` には語 `EINTR`、`Interrupted`、`SO_ERROR`、`EINPROGRESS` をコメントにも書かない (「割り込み」と書く)。`src/sys/` には `ECONNABORTED` と `WriteZero` を書かない (IOCP は 10053 を数で書く)。

### 2.3 `Backend` と境界の型 (`src/real/backend.rs`)

```rust
pub trait Backend: Sized {
    /// 記録ごとに backend が持つ欄。readiness は ()、IOCP は OVERLAPPED の箱など
    type Slot: Default;
    /// OS のソケット。境界を付けない。backend はソケットごとの状態を中に持ってよい。核は Rc<Self::Socket> で持ち、
    /// &Self::Socket しか渡さないので、変わる状態は Cell に置く (driver は Send でない)
    type Socket;
    /// 取り消しを頼んだ後もカーネルが記録の記憶に書きうるか。readiness は false、IOCP は true。
    /// true の backend は、カーネルが書きうる箱を、終わりを確かめる前に放さない。確かめずに放す道に入った箱は abort してよい
    const KERNEL_WRITES_AFTER_CANCEL: bool;

    /// backend と、それを起こす waker を作る。waker の OS の物は Arc の中にあり、backend も同じ Arc を持つ
    fn new() -> io::Result<(Self, Arc<dyn IoWaker>)>;
    /// 待ち受けを作り、待ち受けるポートを返す。作ったソケットを、待つのに要る OS の物 (epoll、kqueue、
    /// 完了ポート) にここで1回だけ結び付ける
    fn listen(&mut self, addr: SocketAddr) -> io::Result<(Self::Socket, u16)>;
    /// 接続に使う、ブロックしない新しいソケット。listen と同じく結び付けてから返す
    fn socket(&mut self, peer: SocketAddr) -> io::Result<Self::Socket>;
    fn shutdown_write(&mut self, socket: &Self::Socket) -> io::Result<()>;
    fn set_nodelay(&mut self, socket: &Self::Socket, on: bool) -> io::Result<()>;
    /// 1つの操作を OS に出す。核の続き (短い書きの残り、受けの出し直し、次の番地) は出さない。
    /// 1つの操作の中で OS の待ちを張り直すこと (ブロックした後に次の辺を待つ、長さ 0 の受けを出し直す) は続きに当たらない。
    /// Accepted で返すソケットは、listen と socket と同じく結び付けてから返す
    fn start(&mut self, key: OpKey, op: &mut IoOp<Self>) -> Start<Self::Socket>;
    /// 取り消しを頼む。readiness はその場で out に Finished { cancelled: true } を積んでよい。完了の形は wait で届ける。
    /// 出せなかった取り消しは次の wait の Err になる
    fn cancel(&mut self, key: OpKey, op: &mut IoOp<Self>, out: &mut Vec<Finished<Self::Socket>>);
    /// 待つ。何も終えずに戻ってよい (核が期限まで待ち直す)。割り込みで戻った待ちは Ok(Woke::TimedOut)。
    /// Err は OS の待ちそのものの失敗か、覚えた取り消しの失敗のときだけ
    fn wait(&mut self, timeout: Option<Duration>, table: &mut Table<Self>,
            out: &mut Vec<Finished<Self::Socket>>) -> io::Result<Woke>;
    /// 壊れた driver を落とす前に呼ぶ。表に残る I/O の操作について取り消しを出し、合わせて 1 秒までカーネルの終わりを
    /// 確かめる。終わった箱は放す。終わらない操作が残れば、残った鍵を標準エラーに書いて process を abort する。
    /// カーネルが書きうる記憶を放す道も、放されうる状態に置く道も作らないためである。readiness は何もしない
    fn drain_broken(&mut self, table: &mut Table<Self>);
}

pub enum Start<S> { Done(io::Result<Outcome<S>>), Pending }
pub struct Finished<S> { pub key: OpKey, pub result: io::Result<Outcome<S>>, pub cancelled: bool }
pub enum Outcome<S> { Size(usize), Accepted(S, SocketAddr), Connected }
#[derive(Debug, PartialEq, Eq)]
pub enum Woke { Finished, Woken, TimedOut }   // Finished は out に1つでも積んだとき
```

境界の型の約束:
- `Finished::cancelled` は「backend が、この終わりは取り消しのためだと判定した」ことを表す。readiness では `cancel` が作った終わり、IOCP では 995 (`ERROR_OPERATION_ABORTED`) である。`true` のとき、核は backend の誤りを捨てて核の誤り `cancelled` に替える (2.5節)。Windows の 995 は std で `TimedOut` に写り、OS の時間切れと区別できないためである。
- `Outcome::Accepted` は相手の番地を `SocketAddr` で持つ。IP でない番地の誤り (`the peer has no IP address`) は backend が作る。`Outcome::Connected` は何も持たず、相手の番地は核が記録から読む。
- `Woke` の `Finished` は `out` に積んだときだけで、何も終えない事象の後は `TimedOut` を返してよい (核が待ち直す)。
- `close` は trait に置かない。記録が `Rc<B::Socket>` を持つので、fd は最後の記録が落ちるときに閉じる。

### 2.4 表、受け箱、driver の型

`src/real/table.rs`:

```rust
pub struct Record<B: Backend> { pub cancelled: bool, pub work: Work<B> }
pub enum Work<B: Backend> { Io(IoOp<B>), Job, Lookup { ticket: u64 } }
pub struct IoOp<B: Backend> { pub io: Io<B::Socket>, pub slot: B::Slot }
pub enum Io<S> {
    Readable { socket: Rc<S> },
    Read { socket: Rc<S>, buf: Lent },
    Write { socket: Rc<S>, buf: Lent, written: usize },
    Accept { listener: Rc<S> },
    /// rest は核だけが使う
    Connect { socket: Rc<S>, peer: SocketAddr, rest: std::vec::IntoIter<SocketAddr> },
}
impl<S> Io<S> { pub fn socket(&self) -> &S; pub fn reads(&self) -> bool /* Readable、Read、Accept */ }
pub struct Table<B: Backend> {
    records: HashMap<OpKey, Record<B>>, io: usize /* Work::Io の数 */,
    /// 表を歩いた回数。歩いてよいのは quiesce、Drop、drain_broken だけであることをテストが確かめる
    #[cfg(test)] pub walks: Cell<usize>,
}
impl<B: Backend> Table<B> {
    pub fn new() -> Table<B>;
    pub fn insert(&mut self, key: OpKey, record: Record<B>);   // 重複は debug_assert!(old.is_none())
    pub fn remove(&mut self, key: OpKey) -> Option<Record<B>>;
    pub fn get_mut(&mut self, key: OpKey) -> Option<&mut Record<B>>;
    pub fn io_mut(&mut self, key: OpKey) -> Option<&mut IoOp<B>>; // backend の wait が鍵で引く
    pub fn len(&self) -> usize;
    pub fn io_len(&self) -> usize;
    /// quiesce、Drop、drain_broken だけが使う。ほかの道で表を歩かない (walks を足す)
    pub fn keys(&self) -> Vec<OpKey>;
}
```

- 仕事と名前の解決も同じ表に記録を持つ。`pending()` は `table.len() + done.len()` で、手放していない解決も含む。

`src/real/shared.rs`:

```rust
pub struct Shared { inbox: Mutex<Vec<Message>>, pub waker: Arc<dyn IoWaker> }   // 鍵を振る欄はない。鍵は呼ぶ側が振る
pub enum Message {
    Job { key: OpKey, result: std::thread::Result<io::Result<usize>> },
    Lookup { ticket: u64, result: io::Result<Vec<SocketAddr>> },
}
impl Shared {
    pub fn new(waker: Arc<dyn IoWaker>) -> Shared;
    pub fn post(&self, message: Message);             // lock して push し、lock を放してから waker.wake()
    pub fn take(&self, into: &mut Vec<Message>);      // 空の into と中身を swap する (容量を使い回す)
}
```

`Shared` は総称にせず、`RawBuf` も `Lent` も持たない。lock の毒は `unwrap_or_else(|p| p.into_inner())` で受ける。

`src/real/mod.rs`:

```rust
#[cfg(debug_assertions)] const QUIESCE_WATCH: Duration = Duration::from_secs(5);

pub struct Driver<B: Backend> { shared: Arc<Shared>, local: Local<B>, bin: Arc<HandleBin>, epoch: Instant }
struct Local<B: Backend> {
    backend: B, table: Table<B>, done: Vec<Completion>,
    handles: HashMap<IoHandle, Rc<B::Socket>>, next_handle: IoHandle,
    pool: Pool, resolver: Resolver, broken: Option<(io::ErrorKind, String)>,
    finished: Vec<Finished<B::Socket>>,   // backend が積む作業の列 (使い回す)
    messages: Vec<Message>,               // 受け箱から移す作業の列 (使い回す)
    deferred: Option<Box<dyn Any + Send>>, defer_panics: bool,   // quiesce の中の仕事の panic
}
/// 方針を当てた後の記録の行き先。完了を作るのは呼び手である
enum Settled { Finished(io::Result<Done>, bool /* cancelled */), Waiting }
impl Driver<sys::Backend> { pub fn new() -> io::Result<Self>; pub fn with_lookup(lookup: Lookup) -> io::Result<Self>; }
impl<B: Backend> Driver<B> { fn build(lookup: Lookup) -> io::Result<Driver<B>>; /* テストは偽の backend でこれを呼ぶ */ }
#[cfg(test)]
impl<B: Backend> Driver<B> {
    /// waker と backend の結び付きを確かめるため、核の待ち直しを通さずに backend を1回待つ
    pub fn wait_backend(&mut self, timeout: Option<Duration>) -> io::Result<Woke>;
}
```

- `new` は `Self::with_lookup(resolve::std_lookup)` と道で書く。`use resolve::std_lookup` にすると、`impl Driver<sys::Backend>` を `cfg(unix)` に閉じる T-4 の時点で、Windows の `unused_imports` が `-D warnings` で落ちる (前回の試作の Windows への cross clippy で確かめた)。
- 記録が `Lent` (生のポインタ) と `Rc` を持つので、`RealDriver` は `Send` でない。`unsafe impl Send` は書かない。

### 2.5 核の方針

`Local<B>` の内部の関数:
- `advance(key, record) -> Settled`: `backend.start` を呼び、`Pending` なら表に入れて `Waiting`、`Done` なら `settle`。
- `settle(key, record, result, by_cancel) -> Settled`: 方針の全部 (下の表)。
- `connect(key, cancelled, addrs, last) -> Settled`: 番地を順に `backend.socket` で作って `advance` する。どれも作れなければ `last` で終える。`last` がなければ `NotFound` の `no address`。
- `land(key, Settled)`: `Finished` なら `done` に `Completion` を積む。待ちの後の道 (`drain_finished`、受け箱の解決の答え) だけが呼ぶ。`submit` の中の道は `Settled` を `Submit` にする。
- `drain_finished()`: `finished` の各項を `table.remove` して `settle` に渡し、`land` する。
- `absorb(&Shared)`: 受け箱を移して処理し、最後に `fill_lookups()` を呼ぶ。
  - `Message::Job` は記録を表から外す。`Err(panic)` は持ち主のスレッドの上で `resume_unwind` する (`quiesce` の中を除く。2.6節)。
  - `Message::Lookup` は、`resolver.returned(ticket)` が `None` (手放した札) なら捨てる。`Some(key)` なら記録を外して `connect` か誤りの完了へ渡す。
- `poll(timeout) -> Option<Woke>`: 壊れていなければ `backend.wait` を呼んで `drain_finished` する。`Err` で `broken` にする。壊れた状態にする道はここだけである。壊れていれば backend を待たない。
- `cancel(key)`:
  - 記録がなければ何もしない。`Job` も何もしない。
  - `Lookup` は記録を外し、`resolver.abandon(ticket)` と `fill_lookups()` を呼び、`Err(cancelled)` の `cancelled: true` で終える。
  - `Io` は、`cancelled` が立っていれば何もしない。立っていなければ立てて `backend.cancel` を呼び、`drain_finished` する。
- `discard()`: `done` を捨て、中身の `Listener` と `Stream` のハンドルを閉じる。

`settle` は最初に結果を正規化する。
- `by_cancel && record.cancelled` の `Err` は、核の `Err(cancelled)` に替え、`cancelled: true` にする。
- `by_cancel` なのに記録に印がない `Err` は、`debug_assert!(false)` で落とす。release では `io::Error::other("the operation was aborted")` で `cancelled: false` にする。

| 操作と (正規化した) 結果 | 扱い |
|---|---|
| `Write` が `Ok(Size(n))` | `written += n`。`written >= len` なら `Ok(Size(len))`。`n == 0` なら `WriteZero`。`record.cancelled` なら `Err(cancelled)` で `cancelled: true`。どれでもなければ `advance` (残りを出す) |
| `Accept` が `Err(e)` で `e.kind() == ConnectionAborted` | `cancelled` なら `Err(cancelled)`、でなければ `advance` |
| `Accept` が `Ok(Accepted(s, peer))` | `Rc::new(s)` をハンドルにして `Done::Stream` |
| `Connect` が `Ok(Connected)` | 記録のソケットの `Rc` をハンドルにして `Done::Stream { peer }` |
| `Connect` が `Err(e)` で `rest` が残る | `cancelled` なら `Err(cancelled)`、でなければ `connect(rest, Some(e))` |
| ほかの `Ok(Size(n))` | `Done::Size(n)` |
| ほかの `Err(e)` | `Err(e)`。`cancelled` は `record.cancelled && by_cancel` |
| 操作に合わない `Outcome` | `unreachable!` (backend の契約違反) |

誤り `cancelled` は今と同じく `io::Error::other("cancelled")` で、`no such handle` は `InvalidInput` である。

### 2.6 `IoDriver` の各メソッドの手順

- `now`: `epoch.elapsed()`
- `wait(deadline_ms)`:
  1. `empty_bin`。壊れていれば `Err`。
  2. `absorb`。
  3. 期限が `None` で、表も `done` も空なら `Ok(false)`。この確かめの後に、期限 `u64::MAX` を期限なしとして扱う (保留がなくても、`Some(u64::MAX)` は起こしまで待つ)。
  4. 繰り返す: `done` が空でなければ `timeout = Some(0)`、空なら `timeout = deadline.map(|d| d.saturating_sub(now))` で `poll(timeout)` する。OS を見るのは `wait` だけなので、届いた完了があっても、期限が今以前でも、OS を1回は見る。壊れれば `Err`。`absorb`。`done` が空でない、`Woke::Woken`、`timeout` が 0、`now >= deadline` のどれかなら `Ok(true)`。ほかは待ち直す (作ったソケットの最初の辺、続きを出した書き、割り込み)。
- `submit(key, op) -> Submit`: 下の `Settled` を、`Finished(r, _)` なら `Submit::Done(r)`、`Waiting` なら `Pending` にする。
  - `Readable`、`Read`、`Write`、`Accept`: ハンドルがなければ `Finished(Err(no such handle))`。`Read` と `Write` の `cap == 0` は backend を呼ばずに `Finished(Ok(Size(0)))` (IOCP の長さ 0 の受けは読めるまで待つので、OS で振る舞いが分かれないようにする)。ほかは `Lent::new(buf)` と `Rc` の写しで記録を作って `advance`。
  - `Job`: 記録を入れて `pool.submit`。プールが仕事を返したら (スレッドを1本も始められない) 記録を外し、誤りを `done` に積んで `Pending` を返す (仕事はいつも `Pending`)。
  - `Listen`: `backend.listen` の結果を `Finished`。
  - `Connect`: IP の綴りなら1つの番地で `connect`。名前なら `absorb` の後に `resolver.submit`。断られたら `Finished(Err)`、受けたら `Work::Lookup { ticket }` を入れて `fill_lookups`、`Waiting`。
  - 保留の鍵と同じ鍵の `submit` は `Table::insert` の `debug_assert!` で落ちる。
- `cancel`: `Local::cancel`。表 (保留の記録) だけを引くので、`Submit::Done` や `completions` で返した鍵、`done` に積んだがまだ返していない鍵には何もしない。readiness の取り消しはその場で終わり、後に OS へ出す取り消しはない。解決は札で手放すので、後で同じ鍵で出した操作に効かない。
- `completions(out)`: `empty_bin`。壊れていれば何もしない。`absorb`。`out.append(&mut done)`。OS を見ず、表を歩かず、並べない (VM は鍵で記録を引く)。
- `pending`: `table.len() + done.len()`。`waker`: `Arc::clone(&shared.waker)`。`bin`: `Arc::clone(&bin)`。
- `shutdown_write`、`set_nodelay`: ハンドルを引き (なければ `no such handle`)、`backend` の同名の関数を呼ぶ。
- `close`: `handles.remove`。保留の操作は自分の `Rc` で OS の資源を生かす。
- `quiesce`:
  1. `empty_bin` を呼ぶ。
  2. `resolver.abandon_all()` を呼び、`Lookup` の記録をすべて外す (完了は作らない)。
  3. `discard` し、表のすべての鍵を `cancel` し、もう一度 `discard` する。
  4. 表が空になるか壊れるまで、`poll`、`absorb`、`discard` を繰り返す。待ちは、debug では `io_len() > 0` の間だけ `Some(見張りの残り)`、ほかは `None`。release は `None`。仕事は受け箱と waker で、取り消しは backend で届くので、刻まずに待てる。仕事は時間の上限なしに待つ。この間の `absorb` は仕事の panic をその場で起こし直さず、最初の1つを覚えておく。繰り返しを抜けたら、覚えた panic を `resume_unwind` する。完了の形の backend で、`quiesce` の途中の巻き戻しが `Drop` の I/O の待ちに入らないためである。`Drop` は `absorb` を呼ばない。
  5. debug のビルドでは、始めから `QUIESCE_WATCH` (5 秒) たっても `io_len() > 0` なら、残った I/O の鍵を昇順に並べて `panic!("cancelled I/O ops did not complete within 5 s: {keys:?}")` する。見張りは driver の状態を変えない。
- `Drop`: 7節の表のとおり。完了の形の backend の待ちは、debug では一度書くまで `Some(見張りの残り)`、書いた後と release は `None`。

VM との約束: 1つのハンドルの同じ向きに、保留の操作を2つ以上出すことはない (VM は受け付けでストリームを記録に移す)。driver はこの約束に頼らない。同じ向きの操作が待っていれば、後の操作は試さずに列の後ろに積む。重ねた呼び手には、終わる順もバイトの順も約束しない。`tests/real.rs` の `quiesce_drains_a_pending_probe_a_full_write_and_a_running_job` は同じハンドルに2つの書きを出すが、どちらも終わらない前提なので順に依らない。

## 3. epoll と kqueue (`src/sys/unix/`)

fd ごとの状態 (両方の backend で同じ形):

```rust
#[derive(Default)]
struct FdState {
    generation: u32,          // 登録の世代。事象の data / udata の上位 32 ビットと比べる
    readable: bool,           // 読める記憶。立てるのは事象だけ、下ろすのは読みの向きのブロック (EAGAIN) だけ
    readers: VecDeque<OpKey>, // Readable、Read、Accept
    writers: VecDeque<OpKey>, // Write、Connect
}
pub struct Epoll { epoll: OwnedFd, doorbell: Arc<Doorbell>, fds: Vec<FdState>, generation: u32, events: Vec<libc::epoll_event> /* 256 */ }
```

- 書ける向きの記憶は持たない。`Write` と `Connect` はいつも先に試し、待つのは列だけなので、記憶を読む者がいない。HUP と ERR は読める記憶を立て、両方の列を進める。
- fd の番号で引く `Vec` にする。ハッシュも歩きもない。大きさは backend が登録した最大の fd の番号 + 1 で、1欄は約 72 バイトなので、fd の番号が 65 535 に届いても約 4.7 MB で止まる (推定。欄の大きさは型から数えた)。番号が疎に大きいときも、上限は soft の `RLIMIT_NOFILE` で決まる。閉じた fd の欄は残るが列は空で (保留の記録の `Rc` が fd を開いたままにするため)、同じ番号を次に登録するときに世代を進めて記憶を下ろす。世代の違う事象 (fork した子が同じ file を持っていて登録が残った場合) は捨てる。

登録:

| いつ | epoll | kqueue |
|---|---|---|
| `listen` の後 | `EPOLL_CTL_ADD`、`EPOLLIN \| EPOLLRDHUP \| EPOLLET` | `EVFILT_READ` を `EV_ADD \| EV_CLEAR` |
| `socket` の後 (接続の前) | `EPOLL_CTL_ADD`、`EPOLLIN \| EPOLLRDHUP \| EPOLLOUT \| EPOLLET` | `EVFILT_READ` と `EVFILT_WRITE` を `EV_ADD \| EV_CLEAR` (1回の `kevent` に変更2つ、事象の列なし) |
| `accept` の結果を返す前 (`start` と事象の後の両方) | 同上 | 同上 |
| data / udata | `(generation << 32) \| fd`、doorbell は `u64::MAX` | 同じ値を `usize` で |
| 外すとき | 外さない。`close` で消える | 外さない。`close` で消える |

- 接続の前の登録で、まだつながらないソケットの事象が積まれても、`epoll_wait` は返す前に今の状態を読み直すので、`SYN_SENT` の間は返さない (推定。カーネルの `ep_send_events` が `ep_item_poll` で読み直す)。閉じたポートへの接続、受けた接続、名前の接続のテストは試作で通った。
- 初めの記憶は下ろしておく。`EPOLL_CTL_ADD` は今読めるなら辺を1つ積むので取りこぼさない。待つ前に着いた辺も取りこぼさない (`sys::tests::an_edge_seen_before_any_op_is_not_lost`)。
- `Connect` の `start` は `connect` を呼ぶ前に読める記憶を下ろす。登録から接続までの間に `wait` が走ると、つながる前のソケットの `EPOLLHUP` で記憶が立ちうる。核は `socket` の直後に同じ呼び出しの中で `start` を呼ぶので今は起きないが、呼び順に頼らない。
- 登録に失敗したソケットは落とし、その操作を誤りで終える (`listen` と `socket` は `Err`、受けは `Accepted` の代わりに `Err`)。

`start` と事象の手順 (`src/sys/unix/ops.rs` の安全な関数を2つの backend が共有する):

| 操作 | `start` (同じ向きの列に先客がいれば試さずに後ろに積む) | 向きの事象の後 (`serve`、列の先頭から) |
|---|---|---|
| `Readable` | `ops::probe(socket, readable)`: 下りていれば読みの列に積む (syscall なし)。立っていれば1バイトの `MSG_PEEK` で確かめ、データか EOF で `Done(Ok(Size(0)))`、`EAGAIN` で記憶を下ろして積む、誤りで `Done(Err)` (覗きは誤りを消すので、次の読みに言わせられない) | `Ok(Size(0))` |
| `Read` | `recv(room)`。終われば `Done`。`EAGAIN` なら記憶を下ろして積む | `recv`。`EAGAIN` なら記憶を下ろして先頭に残す |
| `Accept` | Linux は `accept4(SOCK_NONBLOCK \| SOCK_CLOEXEC)`、Apple は `accept` の後に `set_nonblocking(true)` と `set_nosigpipe(true)` (socket2 0.6.5 の `accept` は Apple で CLOEXEC を立てる)。終われば受けたソケットを登録して `Done` | 同じ |
| `Write` | Linux は `send_with_flags(bytes(written), MSG_NOSIGNAL)`、Apple は `send` (ソケットに `SO_NOSIGPIPE`)。`EAGAIN` なら書きの列に積む | 同じ |
| `Connect` | 記憶を下ろし、`connect`。`Ok` は `Connected`、`EINPROGRESS` か `EINTR` は書きの列に積む | `take_error` (`SO_ERROR`)。`None` は `Connected`、`Some(e)` は `Err(e)` |

- `recv`、`send`、`accept` は `Interrupted` でその場でやり直す。
- 事象: epoll は `events & (EPOLLIN|EPOLLRDHUP|EPOLLHUP|EPOLLERR)` で記憶を立てて読みの列を、`events & (EPOLLOUT|EPOLLHUP|EPOLLERR)` で書きの列を進める。kqueue は `EVFILT_READ` で記憶を立てて読みの列を、`EVFILT_WRITE` で書きの列を進め、`EV_EOF` に `fflags != 0` (ソケットの誤り) があれば両方を進める。`EV_EOF` だけでは向きを変えない (読みの片側の終わりで書きを進めない)。
- 不変条件: `start`、`serve`、`cancel` が戻った後は「読める記憶が立っていれば読みの列は空」である。記憶を立てるのは事象だけで、事象の後の `serve` は列を先頭から、空になるか先頭がブロックする (記憶を下ろす) まで進める。列に積むのは記憶が下りているときか先客がいるときだけである。これが崩れると、記憶が立ったまま待つ操作に辺が来ず、待ちが終わらない。`start` の `Pending` の前と事象ごとの `serve` の後に `debug_assert!(!readable || readers.is_empty())` を置く (試作に入れ、全テストが debug で通った)。書きの列は記憶を持たないので、積むのはブロックした後か先客の後ろだけで、`EPOLLOUT` の辺が必ず来る。
- 記憶を部分的な読み (`n < cap`) で下ろさない。TCP の urgent の印で `recv` が短く終わり、データが残ったまま記憶が下りて、辺の来ない待ちになりうるためである (推定)。
- `cancel`: 列から鍵を外し、`Finished { key, Err(ECANCELED), cancelled: true }` を積む。OS を呼ばないので失敗しない。
- `wait`: epoll は `timeout.as_nanos().div_ceil(1_000_000)` を `i32::MAX` で切った ms (切り上げ。1 ms 未満の残りで 0 を渡して回らないため)。`None` は `-1`。kqueue は `timespec` にそのまま写す。`EINTR` は `Ok(Woke::TimedOut)`。ほかの誤りは `Err`。doorbell の事象で `clear` する。戻り値は、`Finished` を1つでも積めば `Woke::Finished`、doorbell だけなら `Woke::Woken`、何もなければ `Woke::TimedOut`。
- 事象の列: `vec![…; 256]` を `new` で1回作って使い回す。`epoll_event` は値で読み、欄の参照を作らない。x86_64 の `libc::epoll_event` は packed である。kqueue の `udata` は `*mut c_void` なので `usize` を経て写す。
- fd の CLOEXEC: epoll は `epoll_create1(EPOLL_CLOEXEC)`、kqueue は `kqueue()` の後に `fcntl(F_SETFD, FD_CLOEXEC)`。
- `drain_broken`: 空の本体。readiness ではカーネルが記録の記憶に後から書かず、fd が閉じれば登録も消える。
- `process::abort` は `src/sys/unix/` に書かない。

短い書きの続きは、登録を残すので `send`、`send` (EAGAIN)、`epoll_wait`、`send` になり、前回の level の形の5回から減る (推定)。後に回す項目にしない。

## 4. doorbell (`src/sys/unix/doorbell.rs`)

`Arc<Doorbell>` を `IoWaker` として `Shared` が持ち、backend も同じ `Arc` を持つ。

```rust
pub struct Doorbell {
    rung: AtomicBool,
    /// Linux は eventfd、Apple はパイプの読む端
    fd: File,
    #[cfg(target_vendor = "apple")] write: File,
}
impl Doorbell { pub fn new() -> io::Result<Doorbell>; pub fn fd(&self) -> RawFd; pub fn clear(&self); }
impl IoWaker for Doorbell { fn wake(&self) { if !self.rung.swap(true, AcqRel) { let _ = write(...); } } }
```

- Linux: `libc::eventfd(0, EFD_CLOEXEC | EFD_NONBLOCK)` を `OwnedFd::from_raw_fd` で `File` にする。`(&file).write(&1u64.to_ne_bytes())` と `(&file).read(&mut [0; 8])` は安全な std で行う。
- Apple: `std::io::pipe()` の両端を `fcntl(F_SETFL, O_NONBLOCK)` にする。書くのは1バイト。std は Apple で `pipe` の後に両端へ CLOEXEC を立てる。
- doorbell は level で登録する (epoll では `EPOLLIN`、kqueue では `EV_CLEAR` なしの `EVFILT_READ`)。
- `clear` は、backend が doorbell の fd が読めると知ったときだけ呼ぶ。順序は「fd を読む → `rung.swap(false, AcqRel)`」である。逆にすると、reset の後の起こしの書きを読み捨て、旗が立ったまま以後の起こしが syscall を出さなくなる。`swap` の acquire は起こした側の `post` の lock の解放と同期するので、`wait` の後の `absorb` は起こした側が積んだ受け箱の中身を見る。
- 旗が立つのは書きの前だけなので、fd に残るのは高々1つの書きで、パイプは満ちない。`wake` は確保も lock もしない。
- driver を落とした後の `wake` は、`Arc` が両端を生かすので、EPIPE にも SIGPIPE にもならない。読む端を先に閉じると、埋め込んだ eml では SIGPIPE が既定のままで process が終わりうる (Rust の std が `main` の前に SIGPIPE を無視にするのは実行ファイルのときだけである)。
- `eml_vm` の監視のスレッドの自己パイプとは型を分ける。あちらはシグナルの handler が書くので、別の規則に従う。
- 本番で driver の waker を鳴らすのは、解決のスレッド、プールのスレッド (`Shared::post`)、`eml_vm` の監視のスレッドである。どれもシグナルの handler ではない。

## 5. `sys::Backend` の選び方と `socket.rs`

`src/sys/mod.rs`:

| cfg | `sys::Backend` |
|---|---|
| `target_os = "linux"` | `unix::epoll::Epoll` |
| `target_vendor = "apple"` | `unix::kqueue::Kqueue` |
| `windows` | `windows::iocp::Iocp` (T-5 から) |
| どれでもない | `compile_error!` (eml_io は Linux、Apple の OS、Windows を対象にする) |

`src/sys/socket.rs` (全 OS、unsafe なし。backend の関数が呼ぶ):
- `stream(peer)`: `Socket::new` と `set_nonblocking(true)`、Apple は `set_nosigpipe(true)`
- `listener(addr)`: unix だけ `SO_REUSEADDR` を立てる (Windows の `SO_REUSEADDR` は使っている番地への bind を許すので立てない。std も Windows では立てない)。ブロックしないソケットにし、bind、`listen(1024)`、`local_addr` のポートを返す
- `shutdown_write`、`set_nodelay`
- `set_nonblocking(true)` は全 OS で呼ぶ (net-windows の IOCP の backend がこれに頼る)

## 6. プールと解決器

プール (`src/real/pool.rs`):

```rust
pub const POOL_LIMIT: usize = 32;
const IDLE_LIFETIME: Duration = Duration::from_secs(60);
pub type Job = Box<dyn FnOnce() -> io::Result<usize> + Send + 'static>;
struct State { work: VecDeque<(OpKey, Job)>, threads: usize, idle: usize, busy: usize, closed: bool }
struct Queue { state: Mutex<State>, ready: Condvar }
pub struct Pool { limit: usize, queue: Arc<Queue>, shared: Arc<Shared>, handles: Vec<JoinHandle<()>> }
impl Pool {
    pub fn new(shared: Arc<Shared>, limit: usize) -> Pool;   // driver は POOL_LIMIT を渡す
    /// 列に積む。スレッドを始められず、動かすスレッドが1本もなければ仕事を返す
    pub fn submit(&mut self, key: OpKey, job: Job) -> Result<(), (Job, io::Error)>;
}
```

| 事柄 | 振る舞い |
|---|---|
| `submit` | lock して列の後ろに積む。`idle >= work.len()` なら `notify_one`。そうでなく `threads < limit` なら `threads += 1` し、lock を放してから `spawn_masked` で始める (名前 `eml-pool`、スタックは既定)。上限なら積むだけ。どの場合も待たない |
| スレッドを始められない | `threads -= 1`。`threads == 0` なら積んだ仕事を取り出して返し、核は誤りの完了にする。1本でもあれば列に残す |
| スレッドの本体 | 列の先頭を取り、`busy += 1` して lock を放し、`catch_unwind(AssertUnwindSafe(job))` を走らせ、`shared.post(Message::Job)` で積み、`busy -= 1`。列が空で `closed` なら終わる。空なら `idle += 1` で `wait_timeout(IDLE_LIFETIME)`。時間切れで列が空なら終わる |
| 終わったスレッドの `JoinHandle` | `submit` がスレッドを始めるたびに、`is_finished()` のものを捨てる |
| `Drop` | lock して `closed = true` にし、まだ始まっていない仕事を列から取り出し (lock を放してから落とす)、`notify_all`。`busy == 0` ならすべてのスレッドを join する。`busy > 0` なら join しない。走っている仕事は後で終わり、結果は生きている `Shared` に積まれて捨てられ、スレッドは `closed` を見て終わる |
| `quiesce` | 仕事の記録は列にあるものも走っているものも表にあるので、核の `quiesce` の待ちがすべてを待つ |
| Miri | `Masked` は `cfg(any(not(unix), miri))` で何もしない型にする (Miri に `pthread_sigmask` の shim がない)。`busy > 0` の道は `dropping_a_driver_does_not_wait_for_a_running_job` が Miri でも通り、残ったスレッドの報告は出ない (前回の試作で `-Zmiri-ignore-leaks` なしで通った) |

受け付けを閉じる形は `closed` の旗と `notify_all` にする。`mpsc` を `Mutex<Receiver>` で共有すると、受ける側が lock を持ったまま待つので、ほかの空きのスレッドの 60 秒の数えが止まるためである。列の長さに上限はない。

解決器 (`src/real/resolve.rs`): 今の `resolve.rs` から mpsc の channel、`ready` の列、waker を外し、`Arc<Shared>` を持たせる。全 OS でコンパイルする。Windows では `Backend::socket` が `Unsupported` を返すので、名前の接続は解決の後に `Unsupported` で終わる。答えは鍵でなく札で当てる。

```rust
pub type Lookup = fn(&str, u16) -> io::Result<Vec<SocketAddr>>;
pub fn std_lookup(host: &str, port: u16) -> io::Result<Vec<SocketAddr>>;   // 今と同じ
pub struct Resolver { lookup: Lookup, shared: Arc<Shared>, next: u64,
                      live: HashMap<u64, OpKey>, queue: VecDeque<(u64, OpKey, String, u16)>, abandoned: HashSet<u64> }
impl Resolver {
    pub fn submit(&mut self, key: OpKey, host: String, port: u16) -> io::Result<u64>;  // 札を返す。64 本詰まっていれば Err
    pub fn fill(&mut self) -> Vec<(OpKey, io::Error)>;   // 8 本まで spawn_masked で始め、始められなかった鍵を返す
    pub fn returned(&mut self, ticket: u64) -> Option<OpKey>;   // 手放した札なら None (数えから外す)
    pub fn abandon(&mut self, ticket: u64);              // live を abandoned へ移し、queue からは外す
    pub fn abandon_all(&mut self);
}
```

- 札にする理由: 鍵の約束は「保留の鍵は互いに違う。それ以外の鍵は使い回してよい」で、取り消した解決の完了を渡した後は、同じ鍵の別の操作が来てよい。手放したスレッドの答えを鍵で当てると、その操作に当たる。札は `Local` の中の数で、スレッドをまたがない (G-8 に当たらない)。
- スレッドは `lookup` の結果を `shared.post(Message::Lookup { ticket, result })` で積む (積んでから waker が鳴る)。driver を落とした後も `Arc<Shared>` が生きているので積みは成功し、結果は捨てられる。
- 名前 `eml-resolve`、スタック 256 KiB、`RESOLVE_LIMIT = 8`、`ABANDONED_LIMIT = 64`、誤りの文言は今と同じ。

## 7. `Drop` と `quiesce`、記録の寿命

| | readiness (epoll、kqueue、`Fake<false>`) | 完了の形 (IOCP、`Fake<true>`) |
|---|---|---|
| `quiesce` | 取り消しの完了がその場で積まれるので、仕事だけを待つ | 取り消しの完了を `poll` で受け、仕事も待つ |
| `Drop` (壊れていない) | `empty_bin` だけで何も待たない。欄が落ちると、記録とハンドルの `Rc` が落ちて fd が閉じ、プールの `Drop` が走る | `io_len() > 0` なら I/O の鍵をすべて `cancel` し、`io_len() == 0` になるか壊れるまで `poll` を繰り返す (release は `None`、debug は一度書くまで見張りの残り)。時間の上限はない。debug では 5 秒たっても残れば `eprintln!` で1回書き、panic しない (巻き戻しがカーネルの書く記憶を放さないため) |
| `Drop` (壊れている、または待ちの途中で壊れた) | `drain_broken` は何もしない | `backend.drain_broken(&mut table)` を呼び、戻ってから欄を落とす。`drain_broken` は取り消しを出し、合わせて 1 秒まで終わりを確かめる。終わった箱は放す。終わらない操作が残れば、残った鍵を標準エラーに書いて process を abort する |
| 仕事と解決 | 待たない | 待たない |

```
submit ─ Io ─ advance ─ start ─┬─ Done ─ settle ─┬─ Finished ─ Submit::Done (done へ積まない)
                               │                 └─ advance (続き)
                               └─ Pending ─ table.insert ─ (backend の wait / cancel) ─ Finished ─ table.remove ─ settle ─ land ─ done へ
submit ─ Read/Write で cap == 0 ─ Submit::Done (backend を呼ばない)
submit ─ Job ─ table.insert ─ pool ─ Message::Job ─ absorb ─ table.remove ─ done へ
submit ─ Lookup ─ table.insert ─ Message::Lookup (札) ─ absorb ─ table.remove ─ connect ─ advance … / done へ
quiesce ─ Lookup の記録は完了を作らずに外す
Drop ─ 完了の形の backend だけ: 壊れていなければ取り消して待つ / 壊れていれば drain_broken (1 秒まで確かめ、終わらなければ abort)
```

`mem::forget` と `Box::leak` は `eml_io` のどこにも書かない。abort の前の書き出しは `let _ = writeln!(std::io::stderr(), …)` で書き、書きの誤りを捨てる (`eprintln!` は標準エラーに書けないと panic する)。

## 8. `buf.rs`、unsafe の一覧、数えるテスト

`src/buf.rs`:

```rust
#![allow(unsafe_code)]
/// driver に貸された記憶。`IoDriver::submit` が受けた `Op` の `RawBuf` からだけ作る。
/// 守る約束は `RawBuf` の doc (eml_rt) にある
pub struct Lent(RawBuf);
impl Lent {
    pub fn new(buf: RawBuf) -> Lent;
    pub fn len(&self) -> usize;                           // cap
    /// 書きの [from..cap]。from == cap なら空の slice。from > cap は panic
    pub fn bytes(&self, from: usize) -> &[u8];
    /// 読みの空き全体。cap == 0 なら空の slice
    pub fn room(&mut self) -> &mut [MaybeUninit<u8>];
}
#[cfg(test)]
impl Lent {
    /// テストが持つ slice の `bytes[len..]` を貸す。ptr はその先頭、cap は残りの長さで、呼び手は完了まで slice を生かし、動かさない
    pub fn of(bytes: &mut [u8], len: usize) -> Lent;
}
```

- slice を作るのは `bytes` と `room` の2つだけである。どちらも安全な関数で、前提は `RawBuf` の約束である。約束は読みと書きで違うので、SAFETY の文を分ける。
  - `room` (読みの貸し): 「貸した側は完了を取り出すまで、物体を生かし、一意に保ち、動かさない。カーネルとこの slice のほかに、この記憶を読み書きする者はいない」
  - `bytes` (書きの貸し): 「貸した側は完了を取り出すまで、物体を生かし、書き換えず、動かさない。一意は求めない。ほかの参照が読んでも、この slice は読むだけなので競合しない」
- `Lent::new` は安全な関数のままにする。`RawBuf` の約束を負うのは `IoDriver::submit` の呼び手 (VM) で、`eml_rt` の `submit` と `RawBuf` の doc がそれを言う。`Lent::new` を `unsafe fn` にしても、その上の `submit` が安全な関数である限り、義務の場所は変わらない。
- `Lent::new` を呼ぶのは、`real/mod.rs` の `submit`、テストの driver の決着 (`test.rs`)、`buf.rs` の中 (`Lent::of`) だけにする。
- `Lent::of` は T-3 で足す。核の `a_lent_slices_nothing_past_its_end` と、net-windows の `iocp/tests.rs` が使う。
- テストの driver は `Lent::new(buf).bytes(0)` で写し、読みは `room()` の各要素に `write` する。`test.rs` の4か所の `unsafe` と `unsafe fn bytes` は消える (前回の試作で `test_driver.rs` のテストがすべて通った)。
- IOCP は `room().as_mut_ptr()` と `bytes(from).as_ptr()` を `WSABUF` に写し、slice を持ち続けない。

`#![deny(unsafe_code)]` は `crates/eml_io/src/lib.rs` と `crates/eml_io/tests/main.rs` の頭に置く。`#![allow(unsafe_code)]` は次のファイルの頭にだけ書く (最後の形)。

| ファイル | unsafe の中身 |
|---|---|
| `src/buf.rs` | `slice::from_raw_parts`、`from_raw_parts_mut`、`ptr.add` |
| `src/mask.rs` | `pthread_sigmask`、`sigemptyset`、`sigaddset` (`cfg(all(unix, not(miri)))`) |
| `src/sys/unix/doorbell.rs` | Linux の `eventfd` と `from_raw_fd`、Apple の `fcntl` |
| `src/sys/unix/epoll.rs` | `epoll_create1` と `from_raw_fd`、`epoll_ctl` (登録と doorbell だけ)、`epoll_wait` |
| `src/sys/unix/kqueue.rs` | `kqueue` と `from_raw_fd`、`fcntl(F_SETFD)`、`kevent` (変更と待ち) |
| `src/sys/windows/iocp.rs` | 13節 (T-5 から) |
| `tests/real.rs` | `pthread_sigmask` の読み、`signal(SIGPIPE)` の設定と戻し |

途中のコミットの `LISTED`:

| コミット | `LISTED` |
|---|---|
| T-2 | `src/buf.rs`、`src/mask.rs`、`src/real.rs`、`tests/real.rs` |
| T-3 | `src/real.rs` を `src/compio.rs` に替える |
| T-4 | `src/sys/unix/doorbell.rs`、`src/sys/unix/epoll.rs`、`src/sys/unix/kqueue.rs` を足す |
| T-5 | `src/sys/windows/iocp.rs` を足す |
| T-7 | `src/compio.rs` を外す |

`src/sys/unix/ops.rs` と `src/sys/socket.rs` には unsafe を置かない。`tests/real.rs` に `setrlimit` の unsafe は足さない (11節のテストは既定の soft 1024 に収まる)。

数えるテスト (`crates/eml_io/tests/unsafe_code.rs`、module `unsafe_code`、`tests/main.rs` で宣言する):
- テストは `unsafe_lives_only_in_the_listed_files` と `the_search_ignores_comments_strings_and_chars` の2本。Miri では流さない (隔離でファイルを読めない)。
- 読み方: `env!("CARGO_MANIFEST_DIR")` の下の `src` と `tests` を `std::fs::read_dir` で再帰に歩き、`.rs` をすべて読む。道は `/` で書く (Windows の `\` を直す)。新しいファイルも漏れない。
- 前処理 (`blank`): コメント (`//` から行末、入れ子の `/* … */`)、文字列 (`"…"` と `\` の escape、`r"…"`、`r#"…"#`)、文字の literal (`'"'`、`'\''` など。生存期間の `'a` は閉じる `'` がないので残す) を空白に替える。除くのはコードでない所だけなので、一覧の外の `unsafe` を見逃さない。
- 数え方: 語 `unsafe` は、前後の文字が識別子の文字 (英数字と `_`) でない出現だけを数える。`unsafe_code` は数えない。
- allow の判定: 語 `unsafe_code` の出現ごとに、その前の最も近い `#[` か `#![` から属性の名前 (最初の `(` か `]` の前) を読む。`deny` と `forbid` は禁じる側、それ以外 (`allow`、`expect`、`warn`、`cfg_attr`、属性の外の出現) は外す側として数える。
- 比べるもの: 語を持つファイルの集合が `LISTED` に等しい。外す側を持つファイルの集合が `LISTED` に等しい。禁じる側を持つファイルの集合が `DENIED = ["src/lib.rs", "tests/main.rs"]` に等しい。語 `unsafe` の次の語が `impl` の出現がどのファイルにもない。
- `LISTED` は在るファイルで絞らない。前処理で文字列を空白にするので、このテストのファイルは語を2つに分けて書かなくてよい。
- 2本目のテストは、前処理と判定の規則を小さい入力で確かめる。入れ子のコメント、escape と raw の文字列、`'"'`、3つの属性の形を使う。

## 9. 偽の backend と核の単体テスト、Miri

`src/real/tests/fake.rs` (unsafe なし):

```rust
pub struct FakeSocket(pub u32);
#[derive(Default)] pub struct Bell { rung: Mutex<bool>, turn: Condvar }   // IoWaker。Condvar で wait を起こす
pub enum OnCancel { Abort, Race(io::Result<Outcome<FakeSocket>>), Never }
pub struct Fake<const COMPLETION: bool> {
    bell: Arc<Bell>,
    pub starts: VecDeque<Start<FakeSocket>>,   // start が先頭から1つずつ使う。空なら Pending
    pub ready: VecDeque<Finished<FakeSocket>>, // 次の wait で届ける
    pub pending: BTreeSet<OpKey>,              // Pending を返し、まだ届けていない鍵
    pub on_cancel: HashMap<OpKey, OnCancel>,   // 取り消しの終わり方。既定は Abort
    pub started: Vec<OpKey>,                   // start を受けた鍵の順
    pub sent: Vec<u8>,                         // Write の Done(Size(n)) で bytes(written)[..n] を写す
    pub incoming: VecDeque<u8>,                // Read の Done(Size(n)) で room()[..n] に書く
    pub waits: usize,
    pub idle: usize,                           // wait が何も届けずに TimedOut で戻る回数
    pub fail_wait: Option<io::Error>,          // 次の wait をこの誤りにする
    pub log: Arc<Mutex<Vec<String>>>,          // 落とした後も読める ("cancel 3"、"deliver 3"、"drain 3")
    next_socket: u32,
}
impl<const C: bool> Fake<C> { pub fn finish(&mut self, key: OpKey, result: io::Result<Outcome<FakeSocket>>); }
impl<const C: bool> Backend for Fake<C> { type Slot = (); type Socket = FakeSocket; const KERNEL_WRITES_AFTER_CANCEL: bool = C; … }
```

台本の約束:
- `C = false` は readiness の台本、`C = true` は完了の台本である。
- `cancel` は `on_cancel` に従って終わりを作る。`Abort` は誤りの `cancelled: true`、`Race(r)` は `r` の `cancelled: false`、`Never` は何も作らない。終わりを置く所は台本で違う。readiness は `out` に、完了の形は `ready` に置く。
- 届けるとき `pending` から外す。外せなければ `op {key} finished twice` で panic する。「ちょうど1回」を backend の側からも確かめる。
- `wait` は、`fail_wait` があればそれを `Err` で返す。`idle` が残っていれば1つ減らして `TimedOut` を返す。`ready` があれば全部を移して `Finished` を返す。なければ `Bell` を `timeout` まで待ち、`Woken` か `TimedOut` を返す。
- `drain_broken` は `pending` の鍵ごとに `drain k` を `log` に積み、`pending` を空にする。abort の道は偽の backend で確かめない。
- `listen` と `socket` は番号を振るだけで OS を呼ばない。そのため `Accepted` の台本も Miri で流せる。
- 鍵はテストが振る。

`src/real/tests/mod.rs` のテスト。どれも `cargo test` と Miri で流す。待つ補助の見張りは、Miri の仮想の時計で数えるので 60 秒にする。完了を待つ補助は、待っている鍵のほかの完了を捨てずに取っておく (試作で、捨てる補助が同じ辺で終わった後ろの操作の完了を落とし、テストが偽に落ちた)。

| テスト | 確かめること |
|---|---|
| `a_short_write_resumes_from_where_it_stopped` | `Done(3)`、`Done(2)`、`Pending`、後で `5` の台本で、完了は1つの `Size(10)`、`started` は同じ鍵の3回、`sent` は先頭からの5バイト |
| `a_write_without_progress_ends_with_write_zero` | `Done(0)` で `WriteZero` (`submit` の戻り値で受ける) |
| `a_read_fills_the_lent_room` | `Done(3)` で、貸した記憶の先頭3バイトが台本の中身になる (`submit` の戻り値で受ける) |
| `ops_without_room_end_at_once` | `cap == 0` の `Read` と `Write` が `Submit::Done(Ok(Size(0)))` で、`started` が空 |
| `a_lent_slices_nothing_past_its_end` | `Lent::of(&mut buf, 0)` で貸した3バイトの `bytes(1)` が後ろの2バイト、`bytes(3)` が空。`Lent::of(&mut buf, 3)` の `room()` が空 |
| `a_readiness_cancel_completes_without_waiting` | 取り消した `Readable` が次の `completions` で `cancelled: true`。`waits == 0`、`pending() == 0` |
| `a_pending_op_finishes_once_with_or_without_a_cancel` | 完了の台本で、2回取り消した鍵と取り消さない鍵が、それぞれ1回だけ届く。次の `completions` は空 |
| `a_cancel_error_from_the_backend_becomes_the_cancelled_error` | 完了の台本の `Abort` の誤り (`fake cancel`) が、誤り `cancelled` と `cancelled: true` で届く |
| `a_result_that_beats_the_cancel_is_not_cancelled` | `Race(Ok(Size(4)))` で `Ok(4)`、`cancelled: false` |
| `a_cancelled_write_does_not_resume` | 取り消しと競って短い書きが着いても出し直さず、`Err(cancelled)` で `cancelled: true`。`started` は1回 |
| `an_aborted_accept_is_retried_under_the_same_key` | `ConnectionAborted` の後の `Accepted` で `Done::Stream { peer }`。`started` は同じ鍵の2回 |
| `a_connect_by_name_tries_the_next_address` | 解決器のスレッドが返す2つの番地で、1つ目の拒否の後に2つ目で `Stream`。`peer` は2つ目 |
| `completions_return_only_the_finished_ops` | 保留5つのうち2つを終えると、`wait` の後の `completions` が2つ (集合で比べる) を返し、`pending() == 3` |
| `a_job_completes_through_the_inbox` | 仕事の値が受け箱を通って `Size(7)` で届く |
| `jobs_beyond_the_pool_limit_wait_in_order` | 上限 2 のプール (`driver.local.pool = Pool::new(.., 2)`) に3つ出す。`pending() == 3`。3つ目は先の1つが終わってから始まり、受け付けの順に始まる |
| `quiesce_waits_for_queued_and_running_jobs` | 同じ組で、別のスレッドが門を開ける間 `quiesce` が待つ。3つとも走り、`pending() == 0` |
| `dropping_a_driver_does_not_wait_for_a_running_job` | 仕事が始まった後に落とし、落とした後に門を開けると仕事が終わる (`busy > 0` の道) |
| `a_job_panic_resumes_on_the_owner_thread` | `#[should_panic(expected = "boom")]`。仕事の panic を、届けた後の `wait` か `completions` の中で起こし直す |
| `dropping_a_completion_driver_waits_for_cancelled_io` | `Fake<true>` を落とすと、`log` が `cancel k`、`deliver k` |
| `dropping_a_readiness_driver_does_not_cancel` | `Fake<false>` を落としても `log` は空 |
| `a_broken_completion_driver_drains_its_io_on_drop` | `fail_wait` で `wait` が `Err` になり、`quiesce` の後も `pending() == 1`。落とすと `Drop` が戻り、`log` が `cancel k`、`drain k` |
| `quiesce_raises_a_job_panic_after_draining_the_io` | 完了の台本で保留の `Readable` と panic する仕事を持つ。`catch_unwind` した `quiesce` の panic が `boom` で、そのとき `pending() == 0`、`log` に `deliver k` がある |
| `the_shared_state_and_the_pool_work_cross_threads` | `send_sync::<Shared>()`、`send::<pool::Job>()`、`send::<Message>()` の型の assert |
| `a_done_start_returns_done_and_never_completes` | `Start::Done(Size(2))` の台本の `Read` が `Submit::Done(Ok(Size(2)))`、`pending() == 0`、続く `completions` が空 (G-5) |
| `the_same_key_twice_while_pending_panics_in_debug` | `#[cfg(debug_assertions)]`、`#[should_panic]`。保留の鍵で2回 `submit` (G-3) |
| `completions_and_wait_do_not_walk_the_table` | 保留 100 のうち1つを終え、`wait(Some(now))` で OS を見て、`completions` が1つを返し、もう1つを終えて `wait(Some(now))` が戻った後で `table.walks == 0` (G-4) |
| `a_wait_returns_only_on_a_finish_a_wake_or_its_deadline` | 終わりを置き `idle = 3` にすると、`wait(None)` は backend を4回待ってから `Ok(true)` (`waits == 4`)。終わる物がなく `idle = 3` なら、`wait(Some(now + 30))` は期限まで待ち直す (`now() >= 期限`、`waits` が4以上増える) (G-6) |
| `a_wait_with_no_ops_and_a_far_deadline_wakes` | 表も `done` も空で `wait(Some(u64::MAX))` を待ち、別のスレッドが `Bell` を鳴らすと `Ok(true)` で戻る。`Ok(false)` なら落ちる (`wait` の契約) |
| `completions_never_look_at_the_backend` | 保留1つの終わりを `ready` に置き、`completions` を 100 回呼んでも空で `waits == 0`。`wait(Some(now))` で `waits == 1` になり、続く `completions` が1つ返す (G-4) |
| `a_lookup_answer_for_a_reused_key_is_dropped` | 門で止まる解決の `Connect` (鍵 7) を取り消し (完了は `cancelled`)、鍵 7 で `Readable` を出し、門を開けて 200 ms 待った後も `Readable` が保留のまま (`pending() == 1`)、手放した答えが接続を始めていない (`started` は `Readable` の1回だけ) |
| `a_cancelled_connect_does_not_try_the_next_address` | `Fake<true>`、2つの番地の名前の接続で、1つ目の試行に `Race(Err(refused))` を置いて取り消すと、`Err(cancelled)`、`cancelled: true`、`started` は1回 |
| `quiesce_panics_when_a_cancelled_op_never_finishes` | `#[cfg(debug_assertions)]`、`#[ignore = "5 秒の見張りを待つ"]`、`#[should_panic(expected = "did not complete within 5 s: [0]")]`。`Fake<false>` と `OnCancel::Never` で書く。完了の台本では、巻き戻しの `Drop` が終わらない操作を待ち続けるためである |

数: 31 本と、ignored の1本。release で流すと `the_same_key_twice_while_pending_panics_in_debug` が外れて 30 本になる。`a_wait_with_no_ops_and_a_far_deadline_wakes` を除く 30 本は試作で通った (試作には設計に入れない古い1本も残っていた)。

同じ module に、`RealDriver` が `Send` でないことの型の assert を置く。形は `const _: fn() = || { … AmbiguousIfSend … }` で、`eml_rt/src/lib.rs` の `AmbiguousIfSync` と同じ方法である。

Miri:
- コマンド: `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_io --lib real::`。Tree Borrows は `MIRIFLAGS` に `-Zmiri-tree-borrows` を足す。
- 前回の試作 (23 本) の `user` は、4 CPU の Linux のコンテナで Stacked Borrows 8.9 秒、Tree Borrows 8.9 秒だった。この設計の核の差では流していない。
- 予算 (testing.md に書く): Stacked Borrows 30 秒、Tree Borrows 30 秒。1本のテストを足して増える `user` は、2つのモードの合計で 2 秒以内。T-3 で実装の全テストを `/usr/bin/time -p` で測り直し、測った値の約3倍に丸めて書く。数と機械はコミットのメッセージに書く (testing.md の「Miri」の規則)。
- 33 本の仕事のテストは Tree Borrows で1本 4〜5 秒かかった。そのため核のテストは上限 2 のプールで流し、32 本の上限そのものは `tests/real.rs` の本物のテストで確かめる。
- `src/sys/tests.rs` は名前に `real::` を含まないので、Miri の絞り込みに掛からない。T-4 の取り込みの module 名を `driver` にするのも同じ理由である。

## 10. `sys::tests` (OS の backend の9本)

`src/sys/tests.rs`。Miri では流さない。epoll と kqueue の両方で流れるので、kqueue の記憶の誤りの診断にもなる。IOCP は前の5本を通し、`iocp.rs` に同じテストを置かない。

| テスト | 確かめること |
|---|---|
| `a_wake_from_another_thread_ends_a_long_wait` | 50 ms 後の別のスレッドの `wake` で、`Backend::wait(Some(900 ms))` が `Woke::Woken` で戻る。経過は 50 ms 以上 (下限だけを比べる) |
| `a_wait_without_a_wake_times_out` | `wait(Some(20 ms))` が `Woke::TimedOut`。経過の下限は比べない (Windows の待ちの粒度のため) |
| `wakes_before_a_wait_merge_into_one` | `wake` を2回 → `wait(Some(0))` が `Woken`、次の `wait(Some(0))` が `TimedOut` |
| `a_wake_after_the_backend_is_dropped_is_harmless` | waker を残して backend を落とし、`wake` を2回呼ぶ。panic しない |
| `the_driver_waker_wakes_its_backend` | `RealDriver::new()` の `waker()` を別のスレッドで 50 ms 後に鳴らすと、`wait_backend(Some(900 ms))` が `Woken`。`IoDriver::waker` と backend の結び付きを、核の待ち直しを通さずに確かめる |
| `a_partial_read_leaves_the_rest_for_the_next_read` | unix。相手が 10 バイト書き、4 バイトの空きで読み続けると、10 バイトがそろう (部分的な読みの後も記憶が残り、次の読みが回る) |
| `an_edge_seen_before_any_op_is_not_lost` | unix。相手が書いた後、操作のない間に `wait` で辺を受けて捨て、その後の `Readable` が終わり、`Read` が 2 バイトを返す |
| `a_read_queued_behind_a_probe_runs_on_the_same_edge` | unix。`Readable` と、その後ろに積んだ `Read` が、相手の1回の書きの辺でどちらも終わる |
| `a_probe_after_a_drain_waits` | unix。読み尽くした後の `Readable` は `Pending` で、相手が次に書くと終わる (G-19) |

- 前の文の「経過は 50 ms 以上 500 ms 未満」(前回) の上限は比べない。通常のテストに実時間の上限を書かない規則のためである。
- 後ろの4本は試作の epoll で通った。
- T-4 から T-5 の間は `mod tests` を `#[cfg(all(test, unix))]` にする。T-5 で `#[cfg(test)]` にし、後ろの4本は `#[cfg(unix)]` のまま残す。

T-4 の取り込みの形 (`src/sys/tests.rs` の頭):

```rust
/// `tests/real.rs` を新しい driver の上で流す。差し替えで外す
type RealDriver = crate::real::Driver<super::Backend>;
#[path = "../../tests/real.rs"]
mod driver;
```

`#[path]` は `src/sys/` からの相対の道になる (前回の試作で `../../../tests/real.rs` は見つからず、`../../tests/real.rs` で通った)。

## 11. G-4 の 1 ms の上限のテスト

`tests/real.rs` (`#[cfg(unix)]`) の `a_wait_with_two_hundred_pending_ops_returns_only_the_finished_one`:

1. 200 本の待ち受けを `Listen` し、それぞれに `Accept` を出す。どれも `Submit::Pending`。`RLIMIT_NOFILE` は上げない (200 本と数本の接続は既定の soft 1024 に収まる)。
2. 5 回繰り返す: `i * 39` 本目の待ち受けへ std の `TcpStream::connect` でつなぐ。期限が今の `wait(Some(now_ms))` (OS を1回見る) と `completions` を、受けの完了が返るまで繰り返し (1 s で返らなければ落ちる)、完了を返した `wait` の時間を測る。返ったのがその1つの鍵だけであることを確かめる。
3. 5 回の最小が 1 ms 以下。最後に `quiesce` して `pending() == 0`。

- 最小を比べるのは、1回の横取りで落ちないためである。測った値は debug のビルドで 2.4〜5.2 µs だった (試作を4回)。1 ms は 100 倍以上の余裕がある。
- 表を歩かないことの構造の確かめは、核の `completions_and_wait_do_not_walk_the_table` と `completions_never_look_at_the_backend` が持つ。
- このテストは、通常のテストに実時間の上限を書かない規則の唯一の例外である。testing.md の「性能のテスト」に、このテストを例外として1文足す (T-6)。

## 12. 既存のテストの扱いと足すテスト

`crates/eml_io/tests/real.rs` (io-fastpath の後):
- T-2 で頭に `#![allow(unsafe_code)]` を足す (種類3)。
- T-4 で `use eml_io::RealDriver;` を `use super::RealDriver;` にし、`tests/main.rs` に `use eml_io::RealDriver;` を置く (種類3)。
- T-6 から、結合テストとして新しい driver の上で流れる。

| テスト | 扱い | 種類 | コミット |
|---|---|---|---|
| 時計の4本 (`a_real_clock_does_not_go_back`、`a_real_clock_reaches_a_deadline`、`a_timed_wait_returns_at_or_after_its_deadline`、`a_real_driver_without_work_or_deadline_cannot_progress`) | 残す | — | — |
| `a_wake_from_another_thread_leaves_a_wait_working` | 残す。説明の文を新しい driver の言葉に直す | 3 | T-6 |
| `dropping_a_driver_waits_for_a_running_job` | 消す。新しい driver は落とすときに仕事を待たない。T-4 から新しい driver の上でも流すので、その前に消す | 1 | T-4 |
| `a_job_completes_with_its_value`、`a_job_pool_is_capped` | 残す (33 本目は列で待つ) | — | — |
| ストリームの6本、TCP と解決器の 17 本、`an_op_on_an_unknown_handle_fails`、`pool_threads_block_interrupt_and_terminate` | 残す (下の2本を除く) | — | — |
| `a_cancelled_connect_does_not_try_the_next_address` | 手順を書き直す: 解決の答えを受けて最初の試行を出したら、終わる前に取り消し、`cancelled` の完了を確かめた後、2つ目の番地の待ち受けに出した `Accept` が `poll_for(20)` の後も保留であること (次の番地を試していない) を確かめる。compio の中で試行を終わらせる手順は、新しい driver では取り消しの前に次の番地へ進む。新しい形は compio と新しい driver の両方で通る (試作で HEAD の compio と新しい driver の両方で 10 回と 8 回通った) | 2 | T-4 |
| compio の振る舞いを書いたコメント (基点の 588、612、636、669、680、711 行付近) | 新しい driver の言葉に直す | 3 | T-6 |
| `listen_accept_and_connect_are_unsupported` (Windows) | `Listen` と IP の `Connect` が `Unsupported` であることだけを確かめる形にする。`Accept { listener: 0 }` と `shutdown_write(0)` は、compio では `Unsupported`、新しい driver では核が `no such handle` で返す。新しい形はどちらの driver でも通るので、取り込みを Windows でも流す T-5 で直す | 2 | T-5 |
| io-fastpath が足したテスト (0節の A-3) | 残す。期待値は変えない | — | — |

T-6 で足すテスト (新しい driver でしか成り立たないので、取り込みの間は足さない):

| テスト | cfg | 確かめること |
|---|---|---|
| `dropping_a_driver_does_not_wait_for_a_running_job` | なし | 仕事は門 (`Arc<(Mutex<bool>, Condvar)>`) が開くまで待ち、門の後に旗 (`AtomicBool`) を立てて終わる。仕事が始まったこと (始まりの旗) を見てから driver を落とし、落とした後にテストが門を開け、仕事の旗が立つことを待つ (1 s の番犬)。`drop` が仕事を待てば門を開ける前に戻らないので、番犬が門を開けて「番犬が開けた」旗を立て、テストはその旗で落ちる。時間は比べない (G-7) |
| `a_job_beyond_the_pool_limit_does_not_block_submit` | なし | 門で止まる仕事を 32 本出し、32 本が始まった (数が 32) のを見てから 33 本目を出す。33 本目の `submit` が門を開ける前に `Pending` で戻ること、33 本目の仕事の中で読んだスレッドの ID が呼んだスレッドの ID と違うこと、門を開けると 33 本目が `Size(33)` で終わることを確かめる。`submit` が止まれば番犬 (1 s) が門を開けて旗を立て、テストはその旗で落ちる。時間は比べない (G-7) |
| `a_wake_after_the_driver_is_dropped_raises_no_sigpipe` | unix | `SIGPIPE` を `SIG_DFL` にし、落とした driver の waker を別のスレッドから2回鳴らして落とし、前の処理に戻す。process が終わらない (G-6) |
| `a_cancelled_probe_completes_without_a_wait` | unix | 取り消した `Readable` が `wait` なしの次の `completions` で `cancelled: true` で届き、`pending() == 0` (readiness の性質) |
| `a_wait_with_a_pending_probe_does_not_tick` | unix | データのない接続に `Readable` を出し、`wait(Some(now + 200))` が 150 ms より前に戻らない (G-6。io-fastpath の保留なしの版に、保留ありを足す。試作で 0.20 s) |
| `a_readable_probe_follows_the_stream` | unix | 新しい接続の `Readable` は `Pending` で、相手が書いた後に終わる。データが残る間の `Readable` は `Submit::Done(Ok(Size(0)))`。`Read` が `Done`。`Read` が `Pending` (EAGAIN) になって取り消した後の `Readable` は `Pending` |
| `a_wait_with_two_hundred_pending_ops_returns_only_the_finished_one` | unix | 11節 (G-4) |

- 7本のうち後ろの3本は試作で通った。前の4本は前回の設計どおりで、試作にない。
- 前回の `completions_return_only_the_finished_ops` (本物の8本) と `a_wake_from_another_thread_ends_a_wait_with_a_far_deadline` は、11節のテストと io-fastpath の `a_wake_ends_a_wait_with_a_far_deadline` が同じことを確かめるので足さない。
- 番犬のスレッドは、テストが成功したときは門が開いた後に何もせずに終わる形にする (番犬が待つ条件変数をテストの終わりに鳴らす)。

ほか:
- `src/compio.rs` (T-3 で `real.rs` から名前を変える) に単体テストはない (io-fastpath の T-4 が `tests/real.rs` に移した)。
- `tests/test_driver.rs`: 触れない (テストの driver は io-fastpath のもの)。
- `crates/eml_vm/tests/api.rs`、`signal.rs`、`io.rs`、UI と REPL: 期待値を変えない。差し替えの後も同じ値で通ることを T-6 の完了の基準にする (io.rs と UI はテストの driver で流れ、api.rs と signal.rs は本物の driver の振る舞いを io-fastpath が compio で先に入れている)。

## 13. 最小の IOCP の backend (T-5、`src/sys/windows/iocp.rs`)

Windows の backend を、完了ポート、waker、刻みのない `wait`、仕事と解決の起こしだけを持つ自前の形で足す。T-5 の時点の Windows の `RealDriver` はまだ compio で、T-6 で差し替わる。ソケットは後の段 net-windows が載せる (付録4)。

```rust
// 起こしの相手。完了ポートのハンドルと旗を Arc に置き、waker として配る
pub struct Port { handle: OwnedHandle, flag: AtomicU8 }   // IDLE = 0、NOTIFIED = 1、AWAKE = 2
impl IoWaker for Port { fn wake(&self) { … } }

pub struct Iocp { port: Arc<Port>, entries: [OVERLAPPED_ENTRY; 128] }
impl Backend for Iocp {
    type Slot = ();
    type Socket = std::convert::Infallible;   // ハンドルが作れない間は start に届かない
    const KERNEL_WRITES_AFTER_CANCEL: bool = true;
    …
}
```

- `Port` は `unsafe impl` なしで `Send + Sync` になる (前回の試作の crate で `send_sync::<Port>()` を `.#cross` の clippy で通した)。
- `new()`: `CreateIoCompletionPort(INVALID_HANDLE_VALUE, null_mut(), 0, 1)`。null なら `Err(io::Error::last_os_error())`。`OwnedHandle::from_raw_handle` で包む。`Arc<Port>` を backend が持ち、同じ `Arc` を `Arc<dyn IoWaker>` として返す。並行のスレッドの数を 1 にするのは、ポートを読むのが持ち主のスレッドだけだからである。
- 完了の鍵: `SOCKET_KEY = 0` (ソケットの結び付け。net-windows)、`WAKE_KEY = 1` (起こしの packet。`lpOverlapped` は null)。
- `wake`: `if flag.fetch_or(NOTIFIED, AcqRel) == IDLE { PostQueuedCompletionStatus(port, 0, WAKE_KEY, null()) }`。戻り値は捨てる。確保も lock もしない。driver を落とした後も `Arc<Port>` が残る限りポートは開いており、packet は誰も読まないポートに積まれて、最後の `Arc` と一緒に消える。
- `wait(timeout, table, out)`:
  1. `notified = flag.swap(IDLE, AcqRel) & NOTIFIED != 0`
  2. 待ちの長さ: `notified` なら 0、ほかは純粋な関数 `fn millis(timeout: Option<Duration>) -> u32` の値 (`None` は `INFINITE`、`Some(d)` は `min(ceil(d / 1 ms), INFINITE - 1)`。`u64::MAX` ms の期限でも 0xFFFF_FFFE ms (約 49.7 日) 待って `TimedOut` で戻り、核が期限まで待ち直す。`Some(d)` が `INFINITE` になることはなく、0 になるのは `d == 0` のときだけである)
  3. `GetQueuedCompletionStatusEx(port, entries, 128, &mut removed, ms, FALSE)` の後に `flag.store(AWAKE, Release)`
  4. FALSE で `GetLastError() == WAIT_TIMEOUT` なら時間切れ。ほかの誤りは `Err` (核が `broken` にする)
  5. TRUE なら各項目を見る。`lpCompletionKey == WAKE_KEY` は起こし。ほかの packet はこの段では来ないので `Err(io::Error::other("a completion packet names no op"))` を返し、核に壊れた状態にさせる。`removed == 128` なら待ちの長さ 0 で取り直す
  6. 戻り値: 起こし (packet か手順1の旗) なら `Woke::Woken`、ほかは `Woke::TimedOut`

起こしを失わないことの場合分け:

| `wake` を呼んだ時点 | `fetch_or` の前の値 | 起きること |
|---|---|---|
| `wait` の外 (`AWAKE`) | 2 | packet は出さない。`NOTIFIED` が立ち、次の手順1が待ちの長さを 0 にする |
| 手順1の後、`GetQueuedCompletionStatusEx` の前か中 (`IDLE`) | 0 | packet を出す。待ちはその packet で戻る |
| 戻った後、手順3の `store` の前 (`IDLE`) | 0 | packet を出す。`store(AWAKE)` が `NOTIFIED` を消すが、packet が残り、次の `wait` が待たずに `Woken` で戻る (余分な起こしが1回ある。正しさは保つ) |
| 2回目以降 (`NOTIFIED` が立っている) | 1 か 3 | packet は出さない。1回目の packet か旗が受け持つ |

- Windows の待ちは刻み (約 15.6 ms) の都合で、`Instant` から見て期限の手前で戻りうる。`IoDriver::wait` は早く戻ってよく、核が時刻を確かめて待ち直す。backend は待ち直さない。
- 仕事と解決の結果: プールと解決器は `shared.post` で受け箱に積んでから waker を鳴らす。backend は受け箱を読まない。核は `poll` の後に必ず `absorb` する。
- ソケットの関数: `listen` と `socket` は `Err(io::ErrorKind::Unsupported.into())`。`start`、`cancel`、`shutdown_write`、`set_nodelay` は `match *socket {}` (空の型なので届かない腕を型が保証する。`unreachable!()` で書かない)。`drain_broken` は空の本体。
- `iocp.rs` の先頭のコメントに書く契約: 「カーネルが記録の `OVERLAPPED` と貸した記憶に書き終えたと backend が確かめるまで、その記録を放さない。落とすときは、核が I/O の記録をすべて取り消し、`wait` で完了を取り出しきってから表を放す。壊れて待てないときだけ `drain_broken` が引き受ける」。この段では I/O の記録がないので待ちはいつも空である。
- 依存: `crates/eml_io/Cargo.toml` の `[target.'cfg(windows)'.dependencies]` に `windows-sys = { workspace = true, features = ["Win32_System_IO", "Win32_System_Threading"] }`。使う記号は `CreateIoCompletionPort`、`GetQueuedCompletionStatusEx`、`PostQueuedCompletionStatus`、`OVERLAPPED_ENTRY`、`INFINITE`、`INVALID_HANDLE_VALUE`、`WAIT_TIMEOUT`、`GetLastError`、`HANDLE`。`Cargo.lock` に新しい package は入らない (windows-sys 0.61.2 はすでにある)。
- 触るファイル: `crates/eml_io/Cargo.toml`、`src/sys/mod.rs` (`#[cfg(windows)] mod windows;` と `pub type Backend = windows::iocp::Iocp;`)、`src/sys/windows/mod.rs`、`src/sys/windows/iocp.rs` (`#![allow(unsafe_code)]`、日本語の SAFETY のコメント)、`src/sys/tests.rs` (`mod tests` を `cfg(test)` に)、`src/real/mod.rs` (`impl Driver<sys::Backend>` の `cfg(unix)` を外す)、`tests/unsafe_code.rs` (`LISTED` に `src/sys/windows/iocp.rs`)。
- docs はこのタスクに入れない (T-6 が書く)。
- テスト: `iocp.rs` の `#[cfg(test)] mod tests` に `timeouts_round_up_and_saturate` (`millis(None) == INFINITE`、`Some(0) → 0`、`Some(1 ns) → 1`、`Some(1 ms) → 1`、`Some(1 ms + 1 ns) → 2`、`Some(Duration::from_millis(u64::MAX)) → INFINITE - 1`)。起こしの5本は `sys/tests.rs` にある。
- 生の `HANDLE` を受ける公開の関数は clippy の `not_unsafe_ptr_arg_deref` で落ちるので、`&Port` を受ける。`cfg(windows)` だけで要る記号を unix で import しない。
- 実装者に任せること: `entries` の数 (既定 128)、関数の名前、SAFETY の文。

IOCP の backend (net-windows の形も含む) が依る核の点:

| 点 | 核の形 |
|---|---|
| `Backend::Socket` に境界がない | 2.3節 |
| `start` の `Readable` が `Start::Done(Ok(Size(0)))` を返してよく、核は `Submit::Done` にする | 2.6節 |
| 1つの操作の中で待ちを張り直すことは backend の仕事 | 2.3節の `start` の doc |
| 完了の形の `Drop` と `quiesce` の待ちが debug の 5 秒の見張りに届く (debug は見張りの残りを期限、release は `poll(None)`) | 2.6節、7節 |
| `Read` と `Write` の `cap == 0` は核が backend を呼ばずに終える | 2.6節 |
| `cancel` の `out` に積まない backend を許す | 2.3節 |
| `Table::io_mut(key)` と公開の欄 `io`、`slot` | 2.4節 |
| `#[cfg(test)] Lent::of(&mut [u8], len)` と `Lent::room()` が `&mut [MaybeUninit<u8>]` | 8節 |
| `sys/socket.rs` がブロックしない設定を全 OS で呼ぶ | 5節 |
| `Woke::Finished` は `out` に積んだときだけで、何も終えない packet の後は `TimedOut` でよい | 2.3節 |
| `Table::insert(key, Record { cancelled, work: Work::Io(IoOp) })` と `keys()` (net-windows のテストの `Rig` が使う) | 2.4節 |
| `completions` は OS を見ず、`GetQueuedCompletionStatusEx(0)` は `wait(Some(now))` でだけ出る | 2.6節 |

## 14. aarch64-linux: flake、CI、ASan

`flake.nix` の差分 (前回の試作の flake で `nix eval` して確かめた。aarch64-linux の `devShells` が `[ "cross" "default" "miri" ]`、`packages` が `[ "default" "eml" ]`、x86_64-linux の `packages` が `[ "default" "eml" "verus" "verusfmt" ]`)。flake は今も2つの system である。

```nix
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
...
          # Verus と verusfmt の release は aarch64-linux の物を出さないので、その system では作らない
          # (docs/implementation/testing.md の「Verus」)
          hasVerus = inputs ? "verus-${system}";
          verusTools = pkgs.lib.optionalAttrs hasVerus {
            verus = pkgs.callPackage ./nix/verus.nix { toolchain = stable.rustc; src = inputs."verus-${system}"; };
            verusfmt = pkgs.callPackage ./nix/verusfmt.nix { src = inputs."verusfmt-${system}"; };
          };
...
            ++ map (target: (fenix.targets.${target}.toolchainOf pin).rust-std) [
              "x86_64-pc-windows-msvc"
              "x86_64-unknown-linux-gnu"
              "aarch64-unknown-linux-gnu"
              "aarch64-apple-darwin"
            ]
...
          packages = rec { eml = …; default = eml; } // verusTools;
...
            default = pkgs.mkShell { packages = [ devToolchain pkgs.cargo-insta pkgs.cargo-nextest ] ++ builtins.attrValues verusTools; inherit shellHook; };
```

- `nix/verus.nix` と `nix/verusfmt.nix` の `meta.platforms` は変えない。Verus の input がない system で `inputs."verus-${system}"` を読むと評価が落ちるので、`hasVerus` で包む。
- `.#miri` は aarch64-linux でも出る (nightly の `complete` は system によらない)。
- `.#cross` に `aarch64-unknown-linux-gnu` と `aarch64-apple-darwin` の rust-std を足す。Linux の開発機で、kqueue のコードと aarch64 の構造体の差を型の検査にかけるためである。link はしない。前回の試作の container で `cargo clippy --workspace --all-targets --target aarch64-apple-darwin -- -D warnings` と `--target aarch64-unknown-linux-gnu` が通った。

`.github/workflows/ci.yml`:
- `test` の matrix を `os: [ubuntu-24.04, ubuntu-24.04-arm, macos-15, windows-2025]` にする。手順は変えない。`taiki-e/install-action` の固定した SHA の manifest に `cargo-nextest` 0.9.146 の `aarch64_linux_gnu` があるので、`fallback: none` のままでよい。
- `fmt`、`verus`、`miri` は `ubuntu-24.04` のまま。
- T-3 で `miri` ジョブに次の手順を足す。`MIRIFLAGS` は matrix の env で与えているので、この手順も2つの設定で流れる。

```yaml
      - name: "cargo miri test -p eml_io --lib real::"
        if: ${{ !cancelled() && steps.rt.outcome != 'skipped' }}
        run: |
          nix develop .#miri -c cargo miri test -p eml_io --lib real::
```

- T-8 で `asan` ジョブを足す。前回の試作の container で `.#miri` の nightly の `RUSTFLAGS=-Zsanitizer=address cargo test -p eml_io --target x86_64-unknown-linux-gnu` が組め、冷えた build で壁時計 32 秒で通った。LeakSanitizer の報告は出なかった。

```yaml
  asan:
    runs-on: ubuntu-24.04
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@<ほかのジョブと同じ SHA>
        with:
          persist-credentials: false
      - uses: cachix/install-nix-action@<ほかのジョブと同じ SHA>
        with:
          github_access_token: ${{ secrets.GITHUB_TOKEN }}
      # --target を付けて build script と proc macro を計測から外す
      - name: cargo test -p eml_io under AddressSanitizer
        run: |
          nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=address CARGO_TARGET_DIR=target/asan cargo test -p eml_io --target x86_64-unknown-linux-gnu
```

action の SHA は、着手のときの ci.yml のほかのジョブと同じ値にそろえる。ASan が壊れたときの扱いは `miri` ジョブと同じで、`flake.lock` を上げる人が直す。直せなければ、`flake.lock` を上げるコミットでジョブの手順を外し、理由をメッセージに書く。この扱いを testing.md の「CI」に1文で書く。

T-1 の docs の差分:
- testing.md「OS ごとのテスト」
  - 頭の文: 「テストは macOS (arm64)、Linux (x86_64 と aarch64)、Windows (x86_64) の4つの対象で流す。…」
  - `.#cross` の段落: 「開発機の OS か arch で組まれないコード (`cfg(windows)`、`cfg(target_vendor = "apple")`、`cfg(target_os = "linux")` の部分) は、`nix develop .#cross` の clippy で型を検査する。この devShell の toolchain は、ふだんと同じバージョンに `x86_64-pc-windows-msvc`、`x86_64-unknown-linux-gnu`、`aarch64-unknown-linux-gnu`、`aarch64-apple-darwin` の `rust-std` を足したものである」
  - 足す文: 「ワークスペースのコードに `target_arch` の cfg を書かない。arch で形の変わる OS の構造体 (x86_64 の `epoll_event` は packed) は、`libc` と `windows-sys` の定義をそのまま使い、欄を値で読む」
- testing.md「CI」: `test` の箇条を「macOS (macos-15)、Linux の x86_64 (ubuntu-24.04) と aarch64 (ubuntu-24.04-arm)、Windows (windows-2025) のそれぞれで …」にする。
- testing.md「Verus」: 「Verus と verusfmt の release は aarch64-linux の物を出さないので、flake は aarch64-linux の devShell に `verus`、`cargo-verus`、`verusfmt` を入れない。`eml_rt` を変える作業の関門 (`cargo verus verify -p eml_rt` と verusfmt の検査) は、x86_64-linux か aarch64-darwin の機械で流す」
- testing.md「よく使うコマンド」: `.#cross` の行の注記に、aarch64 の2つの target を足す。
- architecture.md の toolchain の箇条: Verus と verusfmt の箇条に「aarch64-linux の devShell にはない」を足す。
- CLAUDE.md
  - Commands の前の devShell の段落に足す: 「On aarch64-linux the devShell has no `verus`, `cargo-verus` or `verusfmt` (Verus ships no aarch64-linux release); run the `eml_rt` gate on x86_64-linux or aarch64-darwin.」
  - `.#cross` の説明 (「the `.#cross` shell adds the Windows msvc and Linux `rust-std`」) と、Commands の `.#cross` の行の注記に、`aarch64-unknown-linux-gnu` と `aarch64-apple-darwin` を足す。
  - Testing の CI の行を「on macOS, Linux (x86_64 and aarch64) and Windows」にする。
- `crates/eml_rt/CLAUDE.md`: 最初の箇条に「(not available on aarch64-linux; see docs/implementation/testing.md の「Verus」)」を足す。

T-1 で arm の行が落ちたとき (19節の U-8): T-1 は、arm の行と arm の文を入れない形でコミットする。
- 入れるもの: flake の3つの system、`.#cross` の2つの target、testing.md の `.#cross` と `target_arch` と Verus の文、architecture.md、CLAUDE.md の devShell と `.#cross` の文、`eml_rt/CLAUDE.md`
- 入れないもの: ci.yml の matrix の4つ目、testing.md「OS ごとのテスト」の頭の文の変更 (3つの OS のまま)、testing.md「CI」の arm の文、CLAUDE.md の Testing の CI の行
- 落ちたテストの名前と誤りは、T-1 のコミットのメッセージに書く。入れなかった部分は T-6 (差し替え) に入れ、T-6 の完了の基準に `test (ubuntu-24.04-arm)` の緑を足す。

## 15. docs の割り当てと文案

io-fastpath の後の文書の行は動いているので、着手のときに `git grep -n -i compio -- . ':!docs/plans'` を流し直し、中身で当てる。

### 15.1 compio の行を消すタスク

| ファイル | 中身 | 消すタスク |
|---|---|---|
| CLAUDE.md の Architecture の `eml_io` の行 | `the compio-driver real driver …` | T-6 (15.4節の文) |
| `crates/eml_io/src/lib.rs` の crate の doc | compio の上の driver | T-6 |
| docs/future/async.md の決めの表の行 10 | 「本物の driver は `compio-driver` の上に作り…」 | T-6 (「本物の driver は OS ごとの backend (epoll、kqueue、IOCP) を自前で書く」に) |
| docs/future/async.md の「`IoDriver` と `eml_io`」の compio の箇条と `Proactor::attach` の箇条 | | T-6 (15.2節) |
| docs/future/async.md の「ブロックする呼び出しのプール」の `Asyncify` | | T-6 (「本物の driver の中のプール (32本まで。すべて仕事中なら列で待つ)」に) |
| docs/future/async.md の「ネットワークとシグナル」の Windows の compio の文 | | T-6 (「Windows の本物の driver はまだソケットを扱わない」だけを残す) |
| docs/future/async.md の「ファイルとパイプ」の `Decision::Blocking` の箇条と、macOS の通常のファイルの箇条の最後の文 (「compio-driver も、macOS のファイルの操作は `Decision::Blocking` としてプールで動かす」) | | T-6 (`Decision::Blocking` の箇条は15.2節の文に。macOS の箇条は最後の文だけを消し、ほかの文は残す) |
| docs/future/async.md の「未決の論点」の「I/O の driver の crate」と下の2つの箇条 (io-fastpath の T-0 が書き換えた形) | | T-6 (消す。中の事実は「`IoDriver` と `eml_io`」の新しい箇条と roadmap の `io-uring` にある) |
| docs/future/roadmap.md の「`compio-driver` の 0.13 の系列」 | | T-7 (項目を消す) |
| docs/implementation/architecture.md の crate の配置の2行 | | T-6 |
| docs/implementation/architecture.md の「`eml_io` の内部」 | | T-6 (書き直し。15.4節) |
| docs/implementation/architecture.md の「外部 crate」の compio の2行、`socket2` の行の compio の理由、確かめる項目の一覧 | | T-7 |
| docs/implementation/status.md の本物の driver の文 | | T-6 |
| docs/spec/vm.md の「時計と driver」の本物の driver の箇条 (compio の鍵、5 秒、安全点を通らずに待つ、compio の取り消しの誤り、compio のプール、`quiesce` の compio の文) | | T-6 (15.3節) |
| `Cargo.toml` の `compio-*` の2行、`Cargo.lock`、`crates/eml_io/Cargo.toml` の2行 | 依存 | T-7 |
| `crates/eml_io/src/compio.rs`、古い `resolve.rs` | compio の driver と古い解決器 | T-7 (消す) |
| `crates/eml_io/tests/real.rs` のコメント (基点の 588、612、636、669、680、711 行付近) | | T-6 |

T-6 の後、`git grep -l -i compio -- . ':!docs/plans'` が出すファイルは次だけである。`Cargo.lock`、`Cargo.toml`、`crates/eml_io/Cargo.toml`、`crates/eml_io/src/compio.rs`、`crates/eml_io/src/lib.rs` (`mod compio;` の行)、`crates/eml_io/src/resolve.rs`、`crates/eml_io/tests/unsafe_code.rs` (`LISTED` の `src/compio.rs`)、`docs/future/roadmap.md` (0.13 の項目だけ)、`docs/implementation/architecture.md` (「外部 crate」だけ)。T-7 の後は0行である。

### 15.2 T-6 の async.md の文

- 決めの表の行 10: 「本物の driver は OS ごとの backend (epoll、kqueue、IOCP) を自前で書く」
- 「`IoDriver` と `eml_io`」の compio の箇条を次の文に替える。「本物の driver は、OS ごとに1つの backend (Linux は epoll、Apple は kqueue、Windows は IOCP) を `libc` と `windows-sys` で直接呼ぶ。核は呼ぶ側の鍵で引く1つの表で操作を持ち、backend は1つの操作を1回ずつ出す。記録は持ち主のスレッドの表にだけあるので、driver を落とすときは仕事を待たない ([アーキテクチャ](../implementation/architecture.md) の「`eml_io` の内部」)」
- `Proactor::attach` の箇条: 「Windows では、ソケットを作るときに IOCP のポートに結び付けてから受けと送りを出す。今の Windows の backend はまだソケットを扱わない」
- 「ファイルとパイプ」の `Decision::Blocking` の箇条: 「`RawBuf` を持つ操作はプールに送らない。ファイルの読み書きをプールに回すときは、仕事が所有する buffer に読み、`Op::Job` の結果の型を読んだバイト列も返せる形に広げ、持ち主のスレッドが `buf.rs` で写す」
- 「未決の論点」の「受け付けをスレッドをまたいで安全に出せる backend (IOCP、SQ を lock する io_uring) では、受け箱を通す回り道を省いてよい。既定は backend ごとに `eml_io` が決める」の後段を「既定は `Shared` の受け箱を通し、`async-threads` の計測で見直す」にする。

### 15.3 T-6 の vm.md と `eml_rt` の文

vm.md「時計と driver」の本物の driver の箇条 (`git grep -n -i 'compio\|polling\|5秒\|5 秒\|安全点を通らずに' docs/spec/vm.md` の行):
- `submit` の箇条の末尾: 「本物の driver は、書きの出し直し、受けの再試行、接続の次の試行のために、1つの鍵の下で OS の操作を何度か出す。1つのハンドルの同じ向きに保留の操作を重ねた呼び手には、終わる順もバイトの順も約束しない。VM はストリームを記録に移すので、重ねない」
- `cancel` の箇条: 「取り消しても `key` の完了はかならず届く。取り消しの完了か、取り消しと競って着いた結果である」。5 秒と壊れた状態の文を消す。
- `Completion::cancelled` の箇条: 「本物の driver は、取り消しを頼んだ操作が backend の取り消しの誤り (unix は `ECANCELED`、Windows は `ERROR_OPERATION_ABORTED`) で終わったとき、誤りを `cancelled` に替えて立てる。取り消しのために続きをやめたときと、名前の解決を手放したときも立てる」
- `Read` と `Write` の箇条: 「`cap == 0` の読みと書きは、OS に出さずに `Ok(0)` で終わる」
- `Readable` の箇条: 「本物の driver は、読める記憶が立っていれば1バイトを覗いて確かめ、下りていれば待つ。読み尽くした後の `Readable` は待つ」
- `Job` の箇条のプールの文: 「本物の driver のプールのスレッドは 32 本までで、すべてが仕事中なら、仕事は driver の中の列で受け付けの順に待つ。`submit` は呼んだスレッドを止めない。空いたスレッドは 60 秒で終わる」。「安全点を通らずに待つ」とその理由を消す。
- 解決の箇条: 「名前の解決をプールでなく、driver の中の解決のスレッドで行う」、「結果を driver の受け箱に積んでから waker を鳴らす。答えは driver の中の札で当てるので、手放した解決の答えが同じ鍵の後の操作に当たらない」
- `quiesce` の箇条: compio の文と 5 秒の文を消し、「本物の driver を落とすときは、仕事と名前の解決を待たない。仕事は終わるまでプールのスレッドで動き、結果は捨てる。まだ始まっていない仕事は捨てる。カーネルが取り消しの後も記憶に書きうる backend (IOCP) では、落とすときに I/O の操作をすべて取り消し、完了を取り出しきってから記録を放す」と書く。
- 壊れた driver の箇条: 5 秒の文を消し、「壊れるのは、OS の待ち (`epoll_wait`、`kevent`、`GetQueuedCompletionStatusEx`) そのものが失敗したときだけである。壊れた driver を落とすときは、カーネルがまだ書きうる操作の終わりを 1 秒まで確かめ、終わらなければ process を abort する。放した記憶にカーネルが書く道を残さないためである」と書く。
- Windows の文: 「Windows の本物の driver は IOCP の上にあり、まだソケットを扱わないので、`Listen` と `Connect` は `Unsupported` の誤りで終わる」
- `completions` と集める地点の方針は io-fastpath が書いた文のまま。
- vm.md「上限と割り込み」: 監視のスレッドと解決のスレッドの文にプールのスレッドを足し、「どれも `eml_io::spawn_masked` で始まる」とする。「プールへ仕事を渡す間、VM のスレッドで塞ぐ」と「プールのスレッドは仕事を渡したスレッドから生まれ」を消す。
- `crates/eml_rt/src/driver.rs` の doc は変えない (G-14。io-fastpath で最終の文になっている)。

### 15.4 T-6 のそのほかの文

- architecture.md「`eml_io` の内部」を次の項目で書き直す。
  - 核 (`real/`)、backend (`sys/`)、`buf.rs`、プール、解決器、doorbell (2.1節の表の形)、cfg の表 (5節)、`Settled`
  - 作るときの1回の登録と `EPOLLET`/`EV_CLEAR`、fd の記憶と世代、記憶の不変条件、`Readable` は記憶が立っていれば覗くこと (読み尽くした後は `Pending`)
  - `completions` は OS を見ず `wait` だけが見ること、刻みのない `wait`
  - unsafe の一覧と数えるテスト、`Drop` の表 (7節)。syscall の周りの `unsafe` は Miri で動かせないので、4つの対象の CI の本物のソケットのテストと ASan で確かめること
  - テストの置き場所 (`real::tests` の偽の backend と Miri、`sys::tests`、`tests/real.rs`、`tests/unsafe_code.rs`)
  - `sys/windows/iocp.rs` の行 (13節)
- roadmap「プールの仕事の予算と、放さずに終わる方針」: 本文から「安全点を通らずに待つ」を消し、「列の長さに上限がない」を足す。
- status.md: 本物の driver の文を「本物の driver は OS ごとの自前の backend (Linux は epoll、macOS は kqueue、Windows は IOCP) の上にある。Windows ではまだソケットを扱わない」にする。プールの文も直す。
- CLAUDE.md の Architecture の `eml_io` の行: 「IoDriver implementations: the real driver (own backends over libc and windows-sys: epoll on Linux, kqueue on Apple, IOCP on Windows) and the deterministic test driver; jobs run on the driver's own pool (at most 32 threads, queued beyond that)」。解決のスレッドの句は残す。
- testing.md「性能のテスト」: 11節のテストを例外として1文。「OS ごとのテスト」: `sys::tests` と `tests/real.rs` の分け方。

### 15.5 T-7 の文

- architecture.md「外部 crate」: compio の2行と、`compio-driver` の版の段落と、上げるときに確かめる項目の一覧を消す。`libc` の行に epoll、kqueue、eventfd、パイプを足し、`socket2` の行から compio の理由を消し、`windows-sys` の行に `eml_io` の feature を足す。
- roadmap: 「`compio-driver` の 0.13 の系列」を消す。

## 16. io_uring への当てはめ (G-16、12 行)

実装しない。段 `io-uring` の試作はこの表に従う。

| # | `Backend` の要素 | io_uring の形 | 核を変えるか |
|---|---|---|---|
| 1 | `submit` の `Done`/`Pending` | `Read`、`Write`、`Accept` は readiness と同じく先にブロックしない syscall を1回試し、終われば `Start::Done` (G-2、G-18)。`EAGAIN` のときだけ SQE を積んで `Pending`。`RECV`/`SEND` は `IORING_RECVSEND_POLL_FIRST` (5.19 以降で probe が通るとき) を付けて、空と分かっている読みをカーネルにもう一度試させない (推定)。`Connect` は `CONNECT` の SQE で `Pending`。`Listen` は同期。仕事はプールで `Pending`。試作は「先に試さない」形 (SQE だけ) とも `bench/net` で比べる | 変えない |
| 2 | 鍵と `user_data` | `user_data` は backend の slab の番号 (下位 32 ビット)、その欄を使い回すたびに進める世代、種類 (上位: 操作、取り消し、doorbell)。CQE の世代が欄の今の世代と違えば捨てる。slab の欄に `OpKey` を持ち、`IoOp::slot` に番号を置く。CQE から O(1) で鍵を得て `Table::io_mut` で引く。`OpKey` は u64 の全域を取りうるので、取り消しと doorbell の CQE を見分ける値を残せず、`user_data` に鍵をそのまま使わない | 変えない |
| 3 | `start` の `Readable` | 記憶 (`recv` の結果と CQE で立て下ろす) が立っていれば `MSG_PEEK` で覗いて、データか EOF なら `Done`。下りているか覗きが `EAGAIN` なら `IORING_OP_POLL_ADD` (`POLLIN \| POLLRDHUP`、1回)。CQE の `res` は revents なので `Outcome::Size(0)` にそろえる | 変えない |
| 4 | `start` の `Read` | `IORING_OP_RECV`、番地と長さは `Lent::room()`。カーネルが記憶を持つので `KERNEL_WRITES_AFTER_CANCEL = true`。`res >= 0` は `Size(res)`、負は `from_raw_os_error(-res)` | 変えない |
| 5 | `start` の `Write` | `IORING_OP_SEND` (`MSG_NOSIGNAL`)、`Lent::bytes(written)`。短い送りは `Size(n)` で返し、核が残りを出す | 変えない |
| 6 | `start` の `Accept` | `IORING_OP_ACCEPT` (`SOCK_NONBLOCK \| SOCK_CLOEXEC`)。`sockaddr_storage` と長さは `Slot` の箱。CQE の fd から `Socket` を作り `Accepted` | 変えない |
| 7 | `start` の `Connect` | `IORING_OP_CONNECT`。番地は `Slot` の箱に写す。`res == 0` で `Connected` | 変えない |
| 8 | `cancel` | `IORING_OP_ASYNC_CANCEL` (`addr` は対象の世代つきの `user_data`、自分は取り消しの印)。世代つきなので、積んだ後に対象が終わって欄が次の操作に使われても、遅れた取り消しは新しい操作に当たらない。CQE は2つ届く。取り消しの CQE は捨てる。本体の CQE が `-ECANCELED` なら `Finished { cancelled: true }`、ふつうの結果なら `false`。SQ に積めなかった取り消しは覚えて次の `wait` の `Err` | 変えない |
| 9 | `wait` | SQ を出し、`IORING_ENTER_GETEVENTS \| IORING_ENTER_EXT_ARG` の時間付きの `io_uring_enter`。doorbell は eventfd の multishot `POLL_ADD` (doorbell の印) で、その CQE で `clear` して `Woken`。`IORING_CQE_F_MORE` がなければ張り直す。`-ETIME`、空の CQ、`-EINTR` は `TimedOut` (核が待ち直す)。`wait(Some(now))` は、SQ が空なら syscall なしで CQ の環を読める (`completions` は OS も環も見ない) | 変えない |
| 10 | `close` | trait に置かない。記録が `Rc<Socket>` を持つので、fd を閉じるのは最後の CQE を受けて記録を落とした後で、保留の SQE が閉じた fd を指すことはない | 変えない |
| 11 | `Slot` | 操作ごとの箱 (`Box<SqeArgs>`: `sockaddr_storage` と長さ、接続の番地、slab の番号)。記録が表の中で動いても箱の番地は変わらない | 変えない |
| 12 | `KERNEL_WRITES_AFTER_CANCEL = true` の `Drop` | 核の `Drop` が I/O の鍵をすべて取り消し、`io_len() == 0` まで待って CQE を取り出してから記録を放す。ring の fd を閉じるのはその後 (backend の欄が落ちるとき)。`enter` が失敗して壊れたら、`drain_broken` が `ASYNC_CANCEL` を出し、合わせて 1 秒まで CQ を読み、終わらない操作が残れば鍵を標準エラーに書いて abort してから ring を閉じる | 変えない |

- 最低の kernel の版 (`EXT_ARG` は 5.11、`IORING_RECVSEND_POLL_FIRST` は 5.19) は推定で、試作が起動時の `IORING_REGISTER_PROBE` と `io_uring_setup` の旗で確かめ、使えない機能は使わない形にする。行 2 の slab の番号は `IoOp::slot` (`Backend::Slot` の公開の欄) に置くので、核の型は変わらない。
- io_uring は readiness の登録を持たないので、`listen`、`socket`、受けの結果での登録は何もしない (固定の file に登録する形は試作で測る)。`process::abort` の grep の基準は、そのとき「各 backend の `drain_broken` の中だけ」に読み替える。
- 選び方は G-9 に従う。`RealDriver::new` が起動時に使えるかを調べて選び、`EML_IO_BACKEND` で固定でき、選んだ名前を返す関数を足す。

## 17. syscall の目標 (G-23) と3点目の数字の取り方

試作の `examples/probe.rs` (VM を通さない Rust の driver の直の呼び出し) を release で組み、`strace -c` で数えた。VM の上の数は T-10 の `bench/net` で確かめる。

| 1つの操作 (Linux、epoll、定常状態) | 目標 (G-23) | この設計 | 確かめ |
|---|---|---|---|
| 書けるソケットへの `write_bytes` | 1 | `sendto` 1 | echo 1000 往復で `sendto` 1000 |
| データのある `read_chunk` (空きあり) | 1 | `recvfrom` 1 | self 2000 往復で、データの来ている側の `recvfrom` が往復に1回 |
| 待つ `read_chunk` | 3 | `recvfrom` (EAGAIN)、`epoll_wait`、`recvfrom` | self 2000 往復で `recvfrom` 6000 (うち EAGAIN 2000)、`epoll_wait` 2001 |
| 空きのない `read_chunk` (`Bytes.empty ()`) で待つ | 定めなし | 覗き (EAGAIN)、`epoll_wait`、`recvfrom` の3回 | echo-empty 1000 往復で `recvfrom` 1999 (EAGAIN 698)、`epoll_wait` 711、`sendto` 1000。`epoll_wait` が往復より少ないのは、覗きの時点でデータが着いていた往復があるため |
| backlog のある `accept` | `accept4` 1 と受けたソケットの登録 1 | `accept4` 1 と `epoll_ctl` 1 | conn 1000 本で `accept4` 1000、EAGAIN 0 |
| 空きのループの1回の待ち | 1 | `epoll_wait` 1 | 上の各件で `epoll_wait` が待ちの数と同じ。`timerfd` なし |
| 操作ごとの `epoll_ctl` | 0 | 0。ソケットを作るときに1回 | echo 1000 往復で `epoll_ctl` 3 (doorbell、待ち受け、受けたソケット)、self で 4 |
| 起こしの eventfd の読み | 起こされたときだけ | 起こされたときだけ | 上の件で `read` は起動の 6 回だけ |
| 1往復 (eml が片側、相手は別のスレッド) | 4 以下 | 4 (`sendto` 1、`recvfrom` 2、`epoll_wait` 1) | echo と echo-empty。相手を 300 µs 遅らせて待つ道を通した |
| 1往復 (両端が同じ driver) | 定めなし (基点 34.02) | 6 | self |
| 1接続 (両端が同じ driver、connect、accept、close) | 定めなし (基点 41.99) | 11 (`socket` 1、`fcntl` 2、`epoll_ctl` 2、`connect` 1、`accept4` 1、`epoll_wait` 1、`getsockopt` 1、`close` 2) | conn 1000 本 |

「1往復 4」は、空きの待ちの直前の集める地点で OS を見ないことに依る。`completions` が OS を見ないので driver の側に条件はなく、VM の `switch_away` の頭の集める地点は io-fastpath で OS を見ない形になっている。`completions` が毎回 OS を見る形では、待つ読み1回に `epoll_wait` が2回になった (試作で毎回見る形の echo の `epoll_wait` が 1000 往復で 1988)。

3点目の取り方 (付録2 の6節の手順に従う):

1. 機械: 1点目と2点目を取った機械と同じ形の Linux の container で、ほかに負荷のない状態。benchmarks.md の「ネットワークの計測」に書いた機械と kernel を確かめる。
2. 2つの worktree を作る。io-fastpath を閉じたコミット (`Close io-fastpath`) と、この段の T-9 の後のコミットである。それぞれ別の `CARGO_TARGET_DIR` で `timeout 1200 cargo build --release -p eml_cli` し、`eml-fastpath` と `eml-backends` に写す。nix の devShell が worktree で止まるときは fenix の cargo (1.99.1) を直に使う。
3. `cases.tsv` は今の形 (io-fastpath が足した `compute-fib` と `busy-self` の行を含む) のまま使い、行を足さない。`many-self-100` は足さない。`busy-self` も3点目で測る。
4. `python3 -I bench/net/bench.py --runs 11 --strace-runs 3 --stats fastpath=./eml-fastpath backends=./eml-backends > results-io-backends.jsonl` (約 10 分)。`python3 -I bench/net/report.py results-io-backends.jsonl > summary.md`。
5. 判定: 主指標は user+sys の CPU (床を引いて単位あたり)。付録2 の4節の規則で読む。
6. G-23: strace の main のスレッドの数で、上の表の各項目を付録2 の7.2節の対応の件と列で確かめる。3 回のどれもが目標以下であること。many-self の1接続あたりの CPU の 2000 本 ÷ 250 本が 1.5 以下。
7. `RunStats`: `--stats` の欄を2列で並べる。`parks` は2列で同じ (プログラムで決まる)。`completions` は操作の数で決まるので同じになる見込みで、`allocations` は driver の差し替えで変わらない見込みである。差があればそのまま書く。
8. benchmarks.md の「ネットワークの計測」の表に3列目 (列の名前はコミット) を足し、G-23 の対応の表 (目標、2点目、3点目) を置く。これは T-10 (`Close io-backends`) のコミットに入れる。CI では流さない。1点目と2点目を取った形の Linux の機械が使えなければ、段を閉じずに止めて利用者に報告する。macOS でしか動かない「今の数値」の3つの表だけを、閉じた直後の別のコミット `Measure io-backends` にする。

## 18. タスクの列

### 共通の完了の基準

各コミットで、失敗も警告もなく終わる (T-0 は文書だけなので最初と最後の行だけ):

```sh
timeout 3000 cargo nextest run
cargo clippy --all-targets -- -D warnings
cargo fmt --check
nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings
nix develop .#cross -c cargo clippy --target aarch64-apple-darwin --workspace --all-targets -- -D warnings       # T-1 から
nix develop .#cross -c cargo clippy --target aarch64-unknown-linux-gnu --workspace --all-targets -- -D warnings  # T-1 から
cargo nextest run -p eml_cli --test integration -E 'test(citations::)'
git diff --stat HEAD~1 -- crates/eml_rt/src crates/eml_vm/src std crates/eml_extern/src   # 何も出ない (G-14)
```

`crates/eml_vm/src` にはどのコミットも触れない。CI は `test` (T-1 から4行)、`fmt`、`verus`、`miri` の2行が緑。T-8 からは `asan` も。

核に触れるコミット (T-3〜T-7、T-10) は Miri の2つも通す:

```sh
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_io --lib real::
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows" cargo miri test -p eml_io --lib real::
```

grep の基準 (T-3 から):

```sh
grep -rn "forget\|Box::leak" crates/eml_io/src                                  # 0行
grep -rn "unsafe impl" crates/eml_io                                            # 0行
grep -rn "SO_ERROR\|EINPROGRESS\|EINTR\|Interrupted" crates/eml_io/src/real     # 0行
grep -rn "AtomicU64" crates/eml_io/src/real                                     # 0行
grep -rln "process::abort" crates/eml_io/src | grep -v '^crates/eml_io/src/sys/windows/'   # 0行 (abort を消して通さない)
```

nix の devShell が worktree で止まるときは `nix develop "path:$PWD" ...` か fenix の cargo を使う。ad-hoc の `.em` は `timeout 20` の下で流す。コメントと docs は日本語で、`docs/plans/` を引かない。新しいテストは、どれも単独で 1 s 以内に終わり、実時間を測るテストは下限だけを確かめる。例外は11節のテスト (1 ms の上限) と、`#[ignore]` の `quiesce_panics_when_a_cancelled_op_never_finishes` だけである。

### T-0 `Plan io-backends`
- 触るファイル: `docs/plans/<日付>-io-backends.md` (この付録を計画にしたもの。付録を分けるなら `docs/plans/<日付>-io-backends/`)、`docs/future/roadmap.md` (`io-backends` の項)。
- 作業: 0節の A-1〜A-7 を確かめる。崩れていれば19節の U-6 に従う。roadmap の `io-backends` の項 (io-fastpath の T-0 が足したもの) の前提が、閉じた `io-fastpath` を外した「なし」であることを確かめる。基点で `cargo nextest run` を流し、通った数をメッセージに書く。
- テストの変更 (計画に書く): 種類 1 は `dropping_a_driver_waits_for_a_running_job` の削除 (T-4) だけ。種類 2 は `a_cancelled_connect_does_not_try_the_next_address` の手順 (T-4) と `listen_accept_and_connect_are_unsupported` の期待値 (T-5)。種類 3 は12節の残り。
- 完了の基準: `git grep -n 'io-backends' docs/future/roadmap.md` が段の列の項を出す。`cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` が通る。
- 任せること: 計画書の構成。計画書に写す日本語は、写す人が natural-japanese の skill を当ててから書く。

### T-1 `Run the tests on aarch64-linux`
- 作業: 14節のとおり (flake、`.#cross`、ci.yml の matrix、testing.md、architecture.md、CLAUDE.md、`crates/eml_rt/CLAUDE.md` の文)。compio のままで arm のジョブが通ることを確かめる。
- 完了の基準: 共通。`nix eval .#devShells.aarch64-linux --apply builtins.attrNames` → `[ "cross" "default" "miri" ]`。`nix eval .#packages.aarch64-linux --apply builtins.attrNames` → `[ "default" "eml" ]`。`nix eval .#packages.x86_64-linux --apply builtins.attrNames` と aarch64-darwin → どちらも `[ "default" "eml" "verus" "verusfmt" ]`。`nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- .github/workflows/ci.yml` → 何も出さずに 0 で終わる。`grep -n aarch64-linux CLAUDE.md docs/implementation/testing.md` → Verus の条件の行が出る。`git grep -n target_arch -- '*.rs'` → 0行。CI の `test (ubuntu-24.04-arm)` → 緑。
- 条件: arm の行が落ちたら14節の末尾の形 (arm の行と文を T-6 に回す)。

### T-2 `Lend driver memory through buf.rs and deny unsafe code in eml_io`
- 作業: 8節の `Lent` (`Lent::of` を除く) を足し、テストの driver を `Lent` の上に書き直す。`lib.rs` と `tests/main.rs` に `#![deny(unsafe_code)]`、`buf.rs`、`mask.rs`、`real.rs` (compio のまま。頭に `#![allow(unsafe_code)]` を足すだけ)、`tests/real.rs` に `#![allow(unsafe_code)]` を置く。`mask.rs` の `Masked` を `cfg(any(not(unix), miri))` で何もしない型にする (`Masked` と `Masked::new` は compio が使うので `pub(crate)` のまま残し、T-7 で private にする)。`tests/unsafe_code.rs` の2本を足し、`tests/main.rs` で宣言する。`LISTED` は `src/buf.rs`、`src/mask.rs`、`src/real.rs`、`tests/real.rs`。
- SAFETY の文: `Lent::room` は読みの貸しの文 (一意に保つ)、`Lent::bytes` は書きの貸しの文 (生かし、書き換えない。一意は求めない) を8節のとおり分けて書く。`real.rs` の `Mem` の SAFETY の文 (io-fastpath が分けた形) をまとめて写さない。
- 完了の基準: 共通。`cargo nextest run -p eml_io --test integration -E 'test(unsafe_code::)'` → 2本が通る。`grep -rn unsafe crates/eml_io/src/test.rs crates/eml_io/src/test` → 0行。`grep -rn from_raw_parts crates/eml_io/src` → `buf.rs` と `real.rs` の行だけ。`grep -rn "Lent::new" crates/eml_io/src` → `buf.rs` と `test.rs` の行だけ。`grep -rn "一意に保ち" crates docs | grep -v docs/plans` の行が、どれも読みの貸し (`buf.rs` の `room`、`cells.rs` と `test.rs` の読みの側、`driver.rs` と vm.md の `Read` の文) を述べる所。
- 落とし穴: 数えるテストの前処理で文字列を空白にするので、テストのファイルに語 `unsafe` を分けて書く必要はない。`LISTED` は在るファイルで絞らない。

### T-3 `Add the driver core with a fake backend`
- 触るファイル: `git mv crates/eml_io/src/real.rs crates/eml_io/src/compio.rs` と `lib.rs` の `pub use compio::RealDriver`。足す: `src/real/{mod.rs,backend.rs,table.rs,shared.rs,pool.rs,resolve.rs}`、`src/real/tests/{mod.rs,fake.rs}`、`buf.rs` の `Lent::of`。直す: `tests/unsafe_code.rs` (`src/real.rs` → `src/compio.rs`)、`ci.yml` の `miri` の手順 (14節)、testing.md の「Miri」と「よく使うコマンド」、CLAUDE.md の Commands の Miri の行。
- 作業: 2節と6節と7節と9節のとおりに書く。`Driver<sys::Backend>` の `impl` (`new`、`with_lookup`) はまだ置かない。`lib.rs` の `mod real;` に `#[allow(dead_code, reason = "差し替えるまで公開の型から届かない")]`。古い `resolve.rs` は compio の側に残す (新しい解決器は `real/resolve.rs`)。
  - testing.md の「Miri」に `eml_io` の項を足す。中身は、核の単体テストを偽の backend の上で流すこと、コマンド、予算、`cfg(miri)` で `Masked` が何もしないこと、`sys::tests` は流さないこと。「ほかの crate は、次の `eml_vm` を除いて Miri で流さない」の文に `eml_io` を足す。
  - CLAUDE.md の Commands に足す: `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_io --lib real::   # Miri on eml_io's driver core over the fake backend (same two settings)`
  - Miri の2つを `/usr/bin/time -p` で測り、`user` の約3倍に丸めた値を testing.md に書く。測った数と機械をコミットのメッセージに書く。
- テスト: 9節の 31 本と ignored の1本 (新規)。
- 完了の基準: 共通、Miri の2つ、grep の基準。`cargo nextest run -p eml_io --lib -E 'test(real::)'` → 31 本が通り、1本が ignored (release で流すと 30 本)。`cargo test -p eml_io --lib -- --ignored real::tests::quiesce_panics_when_a_cancelled_op_never_finishes` → 通る (約 5 秒)。Miri の2つのコマンドの `/usr/bin/time -p` の `user` が testing.md に書いた予算の中。`grep -rn "Lent::new" crates/eml_io/src` → `buf.rs`、`real/mod.rs`、`test.rs` の行だけ。`grep -n "sort_by_key" crates/eml_io/src/real/mod.rs` → 0行 (`completions` が並べない)。`cargo clippy -p eml_io --all-targets -- -D warnings` と `.#cross` の Windows の clippy が通る (中間の形で dead_code が出ない)。
- 任せること: 補助の関数の分け方、Miri の予算の値 (既定は Stacked Borrows 30 秒、Tree Borrows 30 秒)。
- 落とし穴: `src/real/` の doc とコメントに `EINTR` と `Interrupted` を書かない (「割り込み」と書く)。`cfg_attr(not(test), expect(dead_code))` は test の build で落ちるので `allow` を使う。待つ補助の見張りは Miri の仮想の時計に合わせて 60 秒。`completions` は OS を見ないので、完了を拾うテストは `wait` を通す。完了を待つ補助は、待っている鍵のほかの完了を捨てずに取っておく。

### T-4 `Add the epoll and kqueue backends and run tests/real.rs on them`
- 触るファイル: 足す: `src/sys/{mod.rs,socket.rs,tests.rs}`、`src/sys/unix/{mod.rs,doorbell.rs,ops.rs,epoll.rs,kqueue.rs}`。`lib.rs` の `mod sys;` に T-3 と同じ `allow(dead_code)`。`real/mod.rs` に `#[cfg(unix)] impl Driver<crate::sys::Backend> { new, with_lookup }` (`Self::with_lookup(resolve::std_lookup)` と道で書く)。`Cargo.toml` の `socket2` を全 OS に。`tests/unsafe_code.rs` に3つのファイル。`tests/real.rs` を `use super::RealDriver;` に、`tests/main.rs` に `use eml_io::RealDriver;`。
- 作業: 3節、4節、5節、10節。`src/sys/tests.rs` の頭に10節の取り込み。`mod tests` は `#[cfg(all(test, unix))]`。
- テスト: 種類 1 `dropping_a_driver_waits_for_a_running_job` を消す。種類 2 `a_cancelled_connect_does_not_try_the_next_address` を12節の形に書き直す (compio と新しい driver の両方で通る)。新規 `sys::tests` の9本。種類 3 は `use` の追随。
- 完了の基準: 共通、Miri の2つ、grep の基準。Linux で `cargo nextest run -p eml_io --lib -E 'test(sys::)'` → `sys::tests` の9本と `sys::tests::driver::` の組がすべて通る。`grep -n 'readers.is_empty()' crates/eml_io/src/sys/unix/epoll.rs crates/eml_io/src/sys/unix/kqueue.rs` → 不変条件の `debug_assert!` が各ファイルに2つ以上。`cargo nextest run -p eml_io --test integration` → compio の上で同じ組が通る。`grep -rn "ECONNABORTED\|WriteZero" crates/eml_io/src/sys` → 0行。`grep -rn "EPOLL_CTL_MOD\|EPOLL_CTL_DEL\|EV_DELETE" crates/eml_io/src/sys/unix` → 0行 (操作ごとの登録の変更がない)。CI の macos-15 で `sys::` の組 (kqueue の上の9本と `tests/real.rs`) が通る。落ちたら、9本のどれが落ちたかで記憶の誤りか登録の誤りかを分ける (19節の U-4)。
- 任せること: 事象の列の長さ (既定 256)、`serve` の分け方。
- 落とし穴: `#[path]` は `src/sys/` からの相対で `../../tests/real.rs`。取り込みの module の名前を `real` を含む名前にしない (Miri の絞り込み `real::` に掛かる)。Windows では T-5 まで新しい driver を組まない。x86_64 の `libc::epoll_event` は packed なので欄を値で読む。kqueue の `udata` は `*mut c_void` なので `usize` を経て写す。

### T-5 `Add the minimal IOCP backend`
- 作業: 13節。`src/sys/mod.rs` の `mod tests` を `cfg(test)` にし、取り込み (`sys::tests::driver`) も全 OS で流す。`sys::tests` の記憶の4本は `#[cfg(unix)]` のまま。`real/mod.rs` の `impl Driver<sys::Backend>` の `cfg(unix)` を外す。
- テスト: 種類 2 `listen_accept_and_connect_are_unsupported` を12節の形に直す (compio と新しい driver の両方で通る形)。新規 `iocp.rs` の `timeouts_round_up_and_saturate`。
- 完了の基準: 共通、Miri の2つ、grep の基準。`nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings` → 0。Linux で `cargo nextest run -p eml_io` → 失敗なし。`git grep -c 'Unsupported' -- crates/eml_io/src/sys/windows/iocp.rs` → `crates/eml_io/src/sys/windows/iocp.rs:2` (コメントに語を書かない)。CI の `test (windows-2025)` が緑で、`eml_io sys::tests::` の5本、`eml_io sys::windows::iocp::tests::timeouts_round_up_and_saturate`、`sys::tests::driver::` の印のないテスト (時計、仕事、起こし、知らないハンドル、`listen_accept_and_connect_are_unsupported`) が IOCP の上で `PASS`。
- 条件: IOCP が核の trait で成り立たない点が出たら、このタスクの前に trait を直し、偽の backend のテストを同じコミットで直す (19節の U-8 の Q-13)。

### T-6 `Run the real driver on the own backends`
- 触るファイル: `lib.rs` (`pub type RealDriver = real::Driver<sys::Backend>;`、新しい module の `allow(dead_code)` を外し、`mod compio;` と古い `mod resolve;` に `#[allow(dead_code, reason = "次のコミットで消す")]`)、`src/sys/tests.rs` (取り込みを外す)、`tests/real.rs` (12節の7本、compio を書いたコメント)。docs (15節の T-6 の行と、15.2〜15.4節)。
- テスト: 新規12節の7本。種類 3 は取り込みを外すこと (同じテストは結合テストで新しい driver の上を流れる) と、compio を書いたコメント。
- 完了の基準: 共通、Miri の2つ、grep の基準。`cargo nextest run -p eml_io --test integration` → 失敗なし、12節の7本が `PASS`。`cargo nextest run -j 1 -p eml_io` → どのテストも 1 s 以内 (ignored を除く)。`cargo nextest run -p eml_vm --test integration -E 'test(api::) | test(signal::) | test(io::) | test(bench::)'` → 失敗なし (期待値を変えない)。`git grep -l -i compio -- . ':!docs/plans'` → 15.1節の末尾の9つだけ。`git grep -n '安全点を通らずに\|5秒' -- docs/spec/vm.md` → 0行。CI の4行の `test` が緑。windows-2025 の出力で、印のない `tests/real.rs` のテスト (時計、仕事、プール、`a_wake_ends_a_wait_with_a_far_deadline`、`a_wait_with_no_ops_and_a_far_deadline_wakes`、`a_wait_without_ops_sleeps_until_its_deadline`、`dropping_a_driver_does_not_wait_for_a_running_job`、`a_job_beyond_the_pool_limit_does_not_block_submit`) がすべて `PASS` (遠い期限のテストが13節の `millis` の道を通る)。
- 任せること: architecture.md の書き方。
- 落とし穴: docs の行は io-fastpath で動いているので中身で当てる。`crates/eml_vm/src` に触れない。

### T-7 `Remove compio`
- T-6 の直後に置き、間にほかの作業を入れない。消す: `src/compio.rs`、`src/resolve.rs`、`Cargo.toml` の `compio-*` と `Cargo.lock` の該当 (`cargo build` で作り直す。`compio-log` と `compio-send-wrapper` も消える。手で書かない)、`mask.rs` の `Masked` を private に、`tests/unsafe_code.rs` の `src/compio.rs`。docs は15.5節。
- テスト: 機械的な追随 (種類3) だけ。
- 完了の基準: 共通、Miri の2つ、grep の基準。`git grep -n -i compio -- . ':!docs/plans'` → 0行。`grep -n "compio\|polling\|rustix\|mio\|io-uring" crates/eml_io/Cargo.toml` → 0行。`grep -n 'name = "compio-\|name = "polling"' Cargo.lock` → 0行。`grep -rn "CANCEL_LIMIT\|CANCEL_POLL\|IDLE_TICK" crates/eml_io/src` → 0行。`grep -rn from_raw_parts crates/eml_io/src` → `buf.rs` の行だけ。`git log -1 --format=%s HEAD~1` → `Run the real driver on the own backends`。

### T-8 `Check eml_io under AddressSanitizer`
- 作業: 14節の `asan` ジョブを足す。testing.md の「CI」に、ASan のジョブと壊れたときの扱いを書く。CLAUDE.md の CI の行を「…, and Verus, verusfmt, Miri and AddressSanitizer (eml_io) on Linux through nix」にする。
- 完了の基準: `nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=address CARGO_TARGET_DIR=target/asan cargo test -p eml_io --target x86_64-unknown-linux-gnu` → 通る。actionlint (T-1 のコマンド) → 何も出さない。CI の `asan` → 緑。
- 条件: GitHub の runner で組めなければ置かず、理由を T-10 のメッセージに書く (19節の U-8 の Q-4)。

### T-9 `Fix the remaining text on the old driver`
- 作業: `net-windows` の要点は io-fastpath の T-0 が roadmap の `net-windows` の項に書いてある。その文が今の形と食い違う所 (この段で名前の変わった型や関数) だけを直す。`git grep -n 'Asyncify\|Proactor\|CANCEL_LIMIT\|polling (epoll\|IDLE_TICK' -- docs CLAUDE.md README.md ':!docs/plans' ':!docs/reports'` を流し、古い driver を書いた文を今の状態に直す。`yield_now` は `Std.Async` の関数の名前なので探さない。
- 完了の基準: citations が通る。上の grep が 0行。`git grep -n 'WSAGetOverlappedResult' docs/future/roadmap.md` → `net-windows` の項の1行。`uv run <natural-japanese のスキルのディレクトリ>/scripts/lint.py --genre tech docs/future/roadmap.md` の新しい指摘を、直すか理由を付けて残す (スキルが使えないときは手で確かめる)。

### T-10 `Close io-backends`
- 作業 (roadmap の「進め方」の順):
  1. spec の段の名前と予告の確かめ。vm.md の Windows の文 (ソケットを扱わない) は今の状態の記述なので残す。
  2. G-25 の3点目と G-23 (17節)。benchmarks.md の「ネットワークの計測」に3列目と G-23 の表を、このコミットに入れる。Linux の機械がなければ閉じずに止める。
  3. 「今の数値」の3つの表: macOS でしか流せないので、閉じた直後の別のコミット `Measure io-backends` にする。
  4. status.md (本物の driver は自前の backend、Windows はまだソケットを扱わない、aarch64-linux を CI で流す)。architecture.md の crate の構成と「外部 crate」の確かめ。roadmap の `io-backends` の項を消し、`net-windows`、`io-uring`、`async-threads`、`async-files`、`ffi-blocking` の前提から外す。overview.md と README.md の地図の変わった所。CLAUDE.md の確かめ。
  5. 計画を消す。消す前に、`docs/` に残っていない決めがないことを確かめる。
  6. コミットのメッセージに、何をどこへ移したか、テストの変更の種類と理由、計測の目立つ差、Docker Desktop か OrbStack の上の aarch64 の Linux のコンテナで `cargo nextest run -p eml_io` を流した結果 (流せる機械がなければその旨)、T-8 を置かなかったときはその理由を書く。
- 完了の基準: 共通、Miri の2つ、grep の基準。`git grep -n -i compio -- . ':!docs/plans'` → 0行。`git grep -n target_arch -- '*.rs'` → 0行。`grep -rn "EML_IO_BACKEND" crates` → 0行。`git log -p --format= <io-backends の T-0>..HEAD -- crates/eml_rt/src/driver.rs` → 何も出ない。`git diff --stat <io-backends の T-0>~1 HEAD -- crates/eml_vm/src std crates/eml_extern/src` → 何も出ない。benchmarks.md の I/O の表に3列あり、このコミットに入っている。`ls docs/plans` → 空。

### 実行の順

T-0 → T-1 → T-2 → T-3 → T-4 → T-5 → T-6 → T-7 → T-8 → T-10 → (`Measure io-backends`)。T-9 は T-7 の後、T-10 の前。T-6 と T-7 は隣り合わせ。

### 利用者が手で確かめること

1. Apple Silicon の Mac の上の Docker Desktop か OrbStack の aarch64 の Linux のコンテナで、`timeout 120 cargo nextest run -p eml_io` が通ること (結果を T-10 のメッセージに書く)
2. macOS か Linux で、`Net.listen "127.0.0.1" 0` で受けるサーバを書き、別の端末から `nc 127.0.0.1 <port>` でつなぎ、`read_line` と `write_line` の往復ができること。待ちの間に Ctrl-C を1回押すと 130 で終わること

## 19. 決めていない点 (既定つき)

| ID | 項目 | 既定 |
|---|---|---|
| U-3 | fd の状態を `Vec` で持つか `HashMap` か | `Vec` (上限は soft の `RLIMIT_NOFILE` × 約 72 バイト)。T-10 の many-self の RSS で目立てば `HashMap` に替える |
| U-4 | kqueue の `EV_CLEAR` と接続の前の登録の振る舞い (macOS の機械がなく試作していない) | CI の macos-15 で落ちたテストで分ける。接続のテストだけが落ちれば、接続のソケットの登録を `connect` が戻った後にする。`sys::tests` の記憶の4本が落ちれば、kqueue の事象の読み (`EV_EOF`、`fflags`、filter ごとの記憶) を直す。記憶が立ったままになる誤りは覗きが吸収するので、残るのは記憶が立たない誤り (待ちが終わらない) で、それは事象の読みを直す |
| U-6 | io-fastpath が0節の A-1〜A-7 と違う形で閉じたとき | A-1 が崩れた (`driver.rs` に compio の文が残る): io-backends では `driver.rs` を直さず、止めて報告する (G-14)。A-3: T-4 で種類 3 として直す。A-4、A-5: T-0 で確かめ、崩れていれば止めて報告する。A-6: 3点目は付録2 の6節の手順で、ある道具の上で取る |
| U-7 | 名前の解決の札で、手放した答えが使い回した鍵に当たらないことを本物で確かめるか | 核の `a_lookup_answer_for_a_reused_key_is_dropped` だけ |
| U-8 | 前回の決めていない点のうち残るもの: Q-2 (`ubuntu-24.04-arm` の runner がこのリポジトリで使えるか、compio のままで arm のテストが通るか)、Q-3 (`taiki-e/install-action` が arm で `cargo-nextest` を入れられるか)、Q-4 (GitHub の runner の nix で `-Zsanitizer=address` の build が通るか)、Q-5 (Mac の上の Linux コンテナで流すこと)、Q-6 (Miri の予算)、Q-7 (debug の 5 秒の見張りが遅い runner で誤って鳴らないか)、Q-8 (プールの空きのスレッドの寿命)、Q-13 (IOCP が核の trait で成り立たない点が T-5 で出るか) | Q-2: 14節の末尾の形で T-1 をコミットし、arm の行と文を T-6 に入れる。T-6 でも通らなければ CI では流さず、roadmap の「その後の項目」に runner の条件を書く。Q-3: 入れられなければその行だけ `cargo install --locked cargo-nextest` に替える。Q-4: 通らなければ T-8 を置かず、T-10 のメッセージに理由を書く。Q-5: 流せる機械がなければ、T-10 のメッセージにその旨を書く。Q-6: Stacked Borrows 30 秒、Tree Borrows 30 秒、1本あたり2つのモードの合計で 2 秒。Q-7: 鳴ったら 10 秒に伸ばし、コミットのメッセージに書く。見張りを外さない。Q-8: 60 秒。Q-13: T-5 の前に trait を直し、偽の backend のテストを同じコミットで直す |
| U-10 | Rosetta for Linux の上の epoll の動き (一次の出典がない) | 利用者の手の確かめ1で見る。設計は変わらない。崩れたら x86_64 のイメージを Mac で動かす使い方が対象から外れるだけで、status.md に書く |

## 20. 設計のときに確かめたこと

- 試作の組み立て: scratch で `driver.rs` に `Submit` と `submit(key, op)` を当て、`eml_io` を前回の試作の核と epoll に替えて新しい trait に直した。`cargo build -p eml_io`、`cargo clippy -p eml_io --all-targets` が警告なしで通った。
- `tests/real.rs`: 基点の 34 本を鍵を振る補助で包んで新しい driver の上で1本ずつ `timeout 10` で流し、`dropping_a_driver_waits_for_a_running_job` (消す予定) を除く全部が通った。`a_cancelled_connect_does_not_try_the_next_address` は書き直した形で通り、同じ形が基点の compio でも通った。`live_lookups_beyond_the_limit_wait_in_order` は `wait(None)` の繰り返しのままでは新しい driver で 10 秒で止まり、`poll_for(&mut driver, 5)` に替えて通った (io-fastpath の T-4 がこの形に直す)。
- 核のテスト: `cargo test -p eml_io --lib` で、核 30 本 (この設計の 31 本から `a_wait_with_no_ops_and_a_far_deadline_wakes` を除いたもの) と `sys::tests` の9本が通り、1本が ignored だった。3回流して同じ。`--test integration` で 40 本が通った。
- `ops::probe` を覗く形だけにし、`sys::tests::a_probe_after_a_drain_waits` と `tests/real.rs` の `a_readable_probe_follows_the_stream` が通った。
- 核の `completions` を `empty_bin`、`absorb`、`out.append` だけにし、`wait` は `done` があっても期限が過ぎていても OS を1回見てから戻る形にした。`completions_never_look_at_the_backend` と `completions_and_wait_do_not_walk_the_table` が通った。
- G-4 のテストを 200 本、`setrlimit` なし、`wait(Some(now))` の時間だけの形にし、4回流して最小 2.4〜5.2 µs (debug) だった。
- `wait(Some(u64::MAX))` を期限なしに写し、`a_wake_ends_a_wait_without_a_deadline` (io-fastpath の `a_wait_with_no_ops_and_a_far_deadline_wakes` の前の名前) が通った。そのとき保留の操作があったかは記録にない。保留のない形は、2.6節の手順3の順 (`Ok(false)` の確かめの後に写す) で T-3 と T-4 が確かめる。
- syscall の数: `examples/probe.rs` (echo、echo-empty、self、conn、many、idle) を release で組み、`strace -c` で数えた (17節の表)。待つ道を通すため、echo の相手のスレッドを返事の前に 300 µs 回した。
- 刻みなしの `wait`: 保留の `Readable` を持つ 200 ms の待ちが、待ち直す形で `wait` 1回、200 ms で戻った。待ち直さない試作では `wait` が2回呼ばれた。
- 確かめていないこと: kqueue と IOCP の上の動き (macOS と Windows の機械がない。kqueue.rs は試作で新しい形に直していない)、Miri (この設計の核の差では流していない)、`eml_vm` の上の数 (VM を新しい trait に合わせていない)、aarch64-linux の実機。
