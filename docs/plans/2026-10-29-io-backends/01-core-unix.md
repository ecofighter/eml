# 付録1: 核、unix の backend、プール、aarch64-linux の設計

この付録は、本体の T-1 から T-4、T-6 から T-8 の細部を持つ。本体と食い違うときは本体に従う。

- 行番号は `origin/claude/cool-goodall-w7phpm` @ `d1dd4f3` (async-net の T-4 の後) で読んだものである。async-net の残りの作業で動くので、関数、テスト、節の名前で引く。
- コード片のコメントは仮の文である。写すときに日本語の規則を当て直し、`docs/` の節を引く形にする。計画の番号 (D-x、T-x) と `docs/plans/` は書かない。
- 「試作で確かめた」は、この計画を作るときに async-net の写しの上で組んで流したことを表す。試作のファイルはリポジトリに残らないので、要る中身はここに書き写してある。

## 1. 置き場所と可視性

- module は private にし、中の項目は `pub` にする。`pub(crate)` にすると、`pub type RealDriver = real::Driver<sys::Backend>;` が `Driver<B>` と `B: Backend` を公開の型に出し、`private_interfaces` と `private_bounds` の lint が `-D warnings` で落ちる。試作で確かめた。
- 最後の形 (T-7 の後) のファイル:

| ファイル | 中身 | unsafe |
|---|---|---|
| `src/lib.rs` | `#![deny(unsafe_code)]`。公開は `RealDriver`、`TestDriver`、`IDLE_TICK`、`spawn_masked` | なし |
| `src/buf.rs` | `Lent` (§11) | 許す |
| `src/mask.rs` | 今の `mask.rs`。`Masked` は `cfg(any(not(unix), miri))` で何もしない型 | 許す |
| `src/real/mod.rs` | `Driver<B>`、`Local<B>`、`IoDriver` の実装、`IDLE_TICK` | なし |
| `src/real/backend.rs` | `trait Backend` と境界の型 (§2) | なし |
| `src/real/table.rs` | `Table<B>`、`Record<B>`、`Work<B>`、`IoOp<B>`、`Io<S>` (§3) | なし |
| `src/real/shared.rs` | `Shared`、`Message` (§3) | なし |
| `src/real/pool.rs` | プール (§9) | なし |
| `src/real/resolve.rs` | 解決器 (§9) | なし |
| `src/real/tests/{mod,fake}.rs` | 偽の backend と核の単体テスト (§12) | なし |
| `src/sys/mod.rs` | `sys::Backend` の cfg の表 (§8) | なし |
| `src/sys/socket.rs` | socket2 の上の安全な補助 (§8) | なし |
| `src/sys/tests.rs` | OS の backend の5本 (§13) | なし |
| `src/sys/unix/{mod,ops}.rs` | `ops::first` と `ops::ready` (§6) | なし |
| `src/sys/unix/doorbell.rs` | 起こしの物 (§7) | 許す |
| `src/sys/unix/epoll.rs` | Linux の backend | 許す |
| `src/sys/unix/kqueue.rs` | Apple の backend | 許す |
| `src/sys/windows/{mod,iocp}.rs` | Windows の backend (付録2) | `iocp.rs` だけ許す |
| `src/test.rs`、`src/test/net.rs` | テストの driver。`buf.rs` を使う | なし |
| `tests/main.rs` | `#![deny(unsafe_code)]`、module の宣言、`use eml_io::RealDriver;` | なし |
| `tests/real.rs` | 本物のソケットのテスト | 許す |
| `tests/unsafe_code.rs` | unsafe を数えるテスト (§11) | なし |

`mask.rs` は `src/` の直下に残す。Windows の何もしない型と全 OS の `spawn_masked` が同じファイルにあるためである。

## 2. `Backend` と境界の型 (`src/real/backend.rs`)

```rust
pub trait Backend: Sized {
    /// 記録ごとに backend が持つ欄。readiness は ()、IOCP は OVERLAPPED の箱など
    type Slot: Default;
    /// OS のソケット。backend はソケットごとの状態を中に持ってよい。核は Rc<Self::Socket> で持ち、
    /// 接続の試みと受けたソケットはそのままハンドルになるので、状態はソケットと一緒に動く。
    /// 核は &Self::Socket しか渡さないので、変わる状態は Cell に置く (driver は Send でない)
    type Socket;
    /// 取り消しを頼んだ後もカーネルが記録の記憶に書きうるか。readiness は false、IOCP は true。
    /// true の backend は、カーネルが書きうる箱を、終わりを確かめる前に放さない。確かめずに放す道に入った箱は abort してよい
    const KERNEL_WRITES_AFTER_CANCEL: bool;

    /// backend と、それを起こす waker を作る。waker の OS の物は Arc の中にあり、backend も同じ Arc を持つ
    fn new() -> io::Result<(Self, Arc<dyn IoWaker>)>;
    /// 待ち受けを作り、待ち受けるポートを返す
    fn listen(&mut self, addr: SocketAddr) -> io::Result<(Self::Socket, u16)>;
    /// 接続に使う、ブロックしない新しいソケット
    fn socket(&mut self, peer: SocketAddr) -> io::Result<Self::Socket>;
    fn shutdown_write(&mut self, socket: &Self::Socket) -> io::Result<()>;
    fn set_nodelay(&mut self, socket: &Self::Socket, on: bool) -> io::Result<()>;
    /// 1つの操作を1回だけ OS に出す。続きは出さない
    fn start(&mut self, key: OpKey, op: &mut IoOp<Self>) -> Start<Self::Socket>;
    /// 取り消しを頼む。readiness はその場で out に Finished { cancelled: true } を積んでよい。完了の形は wait で届ける。
    /// 出せなかった取り消しは次の wait の Err になる
    fn cancel(&mut self, key: OpKey, op: &mut IoOp<Self>, out: &mut Vec<Finished<Self::Socket>>);
    /// 待つ。割り込みで戻った待ちは Ok(Woke::TimedOut) にする。Err は、OS の待ちそのものが失敗したときか、
    /// backend が覚えた取り消しの失敗を返すときだけである
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
pub enum Woke { Finished, Woken, TimedOut }
```

`src/real/` の中では語 `EINTR`、`Interrupted`、`SO_ERROR`、`EINPROGRESS` をコメントにも書かない (本体 D-2 の grep)。上の `wait` の doc が「割り込み」と書くのはそのためである。

境界の型の約束:
- `Finished::cancelled` は「backend が、この終わりは取り消しのためだと判定した」ことを表す。readiness では `cancel` が作った終わり、IOCP では 995 (`ERROR_OPERATION_ABORTED`) である。`true` のとき、核は backend の誤りを捨てて核の誤り `cancelled` に替える (§4)。Windows の 995 は std で `TimedOut` に写り、OS の時間切れと区別できないためである。
- `Outcome::Accepted` は相手の番地を `SocketAddr` で持つ。IP でない番地の誤り (`the peer has no IP address`) は backend が作る。`Outcome::Connected` は何も持たず、相手の番地は核が記録から読む。
- `Woke` は核の振る舞いを変えない (どれでも `Ok(true)` を返す)。起こしと時間切れの区別は §13 の5本が確かめる。

方針を決めた段階の素案 (`close` を trait に置く、`start` が `io::Result` を返す、核がソケットを作る) と違う点:
1. `close` を trait に置かない。記録が `Rc<B::Socket>` を持つので、fd は最後の操作の記録を落としたときに閉じる。readiness の backend は fd の列が空になった時点で関心を外すので、fd が閉じる前に必ず外れている。
2. `start` は `Start` を返す。出せなかった誤りは `Start::Done(Err)` にする。
3. ソケットの作成と設定を `listen`、`socket`、`shutdown_write`、`set_nodelay` として backend に置く。核が OS を呼ばなくなり、偽の backend の上で `Listen`、`Accept`、`Connect` まで Miri で流せる。
4. 壊れた driver の後始末は `drain_broken(&mut Table<Self>)` の1つにする。1 秒まで終わりを確かめ、終わらない操作が残れば abort する。漏らす形は採らない。貸した記憶は VM のヒープの一部で、run からの panic の巻き戻しでは driver が `quiesce` を通らずに落ち、`Machine` はヒープを放す。そのため、箱だけを漏らしても使用後解放になる。上限を置くのは、壊れた状態では OS の取り消しも失敗しうるので、上限がないと埋め込んだ eml の `Drop` が止まり続けるためである。

## 3. 表、受け箱、driver の型

`src/real/table.rs`:

```rust
pub struct Record<B: Backend> { pub cancelled: bool, pub work: Work<B> }
pub enum Work<B: Backend> { Io(IoOp<B>), Job, Lookup }
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
pub struct Table<B: Backend> { records: HashMap<OpKey, Record<B>>, io: usize /* Work::Io の数 */ }
impl<B: Backend> Table<B> {
    pub fn new() -> Table<B>;
    pub fn insert(&mut self, key: OpKey, record: Record<B>);   // 重複は debug_assert
    pub fn remove(&mut self, key: OpKey) -> Option<Record<B>>;
    pub fn get_mut(&mut self, key: OpKey) -> Option<&mut Record<B>>;
    pub fn io_mut(&mut self, key: OpKey) -> Option<&mut IoOp<B>>; // backend の wait が鍵で引く
    pub fn len(&self) -> usize;
    pub fn io_len(&self) -> usize;
    /// quiesce、Drop、drain_broken だけが使う。ほかの道で表を歩かない
    pub fn keys(&self) -> Vec<OpKey>;
}
```

- 仕事と名前の解決も同じ表に記録を持つ。`pending()` は `table.len() + done.len()` で、手放していない解決も含む。
- `Work::Lookup` は欄を持たない (読む所がないので dead_code になる)。

`src/real/shared.rs`:

```rust
pub struct Shared { next_key: AtomicU64, inbox: Mutex<Vec<Message>>, pub waker: Arc<dyn IoWaker> }
pub enum Message {
    Job { key: OpKey, result: std::thread::Result<io::Result<usize>> },
    Lookup { key: OpKey, result: io::Result<Vec<SocketAddr>> },
}
impl Shared {
    pub fn new(waker: Arc<dyn IoWaker>) -> Shared;
    pub fn key(&self) -> OpKey;                       // fetch_add(1, Relaxed)
    pub fn post(&self, message: Message);             // lock して push し、lock を放してから waker.wake()
    pub fn take(&self, into: &mut Vec<Message>);      // 空の into と中身を swap する (容量を使い回す)
}
```

`Shared` は総称にせず、`RawBuf` も `Lent` も持たない。lock の毒は `unwrap_or_else(|p| p.into_inner())` で受ける。

`src/real/mod.rs`:

```rust
pub const IDLE_TICK: Duration = Duration::from_millis(10);
#[cfg(debug_assertions)] const QUIESCE_WATCH: Duration = Duration::from_secs(5);

pub struct Driver<B: Backend> { shared: Arc<Shared>, local: Local<B>, bin: Arc<HandleBin>, epoch: Instant }
struct Local<B: Backend> {
    backend: B, table: Table<B>, done: Vec<Completion>,
    handles: HashMap<IoHandle, Rc<B::Socket>>, next_handle: IoHandle,
    pool: Pool, resolver: Resolver,
    broken: Option<(io::ErrorKind, String)>,
    finished: Vec<Finished<B::Socket>>,   // backend が積む作業の列 (使い回す)
    messages: Vec<Message>,               // 受け箱から移す作業の列 (使い回す)
}
impl Driver<sys::Backend> { pub fn new() -> io::Result<Self>; pub fn with_lookup(lookup: Lookup) -> io::Result<Self>; }
impl<B: Backend> Driver<B> { fn build(lookup: Lookup) -> io::Result<Driver<B>>; /* テストは偽の backend でこれを呼ぶ */ }
#[cfg(test)]
impl<B: Backend> Driver<B> {
    /// waker と backend の結び付きを確かめるため、IDLE_TICK で切らずに backend を1回待つ
    pub fn wait_backend(&mut self, timeout: Option<Duration>) -> io::Result<Woke>;
}
```

- `new` は `Self::with_lookup(resolve::std_lookup)` と道で書く。`use resolve::std_lookup` にすると、`impl Driver<sys::Backend>` を `cfg(unix)` に閉じる T-4 の時点で、Windows の `unused_imports` が `-D warnings` で落ちる。試作の Windows への cross clippy で確かめた。
- 記録が `Lent` (生のポインタ) と `Rc` を持つので、`RealDriver` は `Send` でない。`unsafe impl Send` は書かない。

## 4. 核の方針

`Local<B>` の内部の関数 (本体 D-2 の分担の表はこの名前で引く):
- `advance(key, record)`: `backend.start` を呼び、`Pending` なら表に入れ、`Done` なら `settle` へ渡す。
- `settle(key, record, result, by_cancel)`: 方針の全部 (下の表)。
- `connect(key, cancelled, addrs, last)`: 番地を順に `backend.socket` で作って `advance` する。どれも作れなければ `last` で終える。`last` がなければ `NotFound` の `no address`。
- `drain_finished()`: `finished` の各項を `table.remove` して `settle` に渡す。
- `absorb(&Shared)`: 受け箱を移して処理し、最後に `fill_lookups()` を呼ぶ。
  - `Message::Job` は記録を表から外す。`Err(panic)` は持ち主のスレッドの上で `resume_unwind` する (`quiesce` の中を除く。§5)。
  - `Message::Lookup` は、`resolver.returned(key)` が偽 (手放した解決) なら捨てる。真なら記録を外して `connect` か誤りの完了へ渡す。
- `poll(timeout)`: 壊れていなければ `backend.wait` を呼んで `drain_finished` する。`Err` で `broken` にする。壊れた状態にする道はここだけである。
- `cancel(key)`:
  - 記録がなければ何もしない。`Job` も何もしない。
  - `Lookup` は記録を外し、`resolver.abandon(key)` と `fill_lookups()` を呼び、`Err(cancelled)` の `cancelled: true` で終える。
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

## 5. `IoDriver` の各メソッドの手順

- `now`: `epoch.elapsed()`
- `wait(deadline_ms)`:
  1. `empty_bin` を呼び、壊れていれば `Err` を返す。
  2. `absorb` を呼び、`done` が空でなければ `Ok(true)` を返す。
  3. 待つ長さを決める。期限があれば `min(期限 - now, IDLE_TICK)`。期限がなく表が空なら `Ok(false)` を返す。期限がなく表が空でなければ `IDLE_TICK`。
  4. `poll(Some(timeout))` を呼び、壊れれば `Err` を返す。
  5. `Woke` の値によらず `absorb` を呼び、`Ok(true)` を返す。
- `submit(op)`: `shared.key()` で鍵を振る。
  - `Readable`、`Read`、`Write`、`Accept` はハンドルを引き、なければ `no such handle` で終える。
  - `Read` と `Write` で `buf.cap == 0` なら、backend を呼ばずにその場で `Ok(Size(0))` にする。IOCP の長さ 0 の受けは読めるまで待つので、OS で振る舞いが分かれないようにするためである。
  - それ以外は、`Lent::new(buf)` と `Rc` の写しで `Io` の記録を作って `advance` する。
  - `Job` は `Work::Job` の記録を入れて `pool.submit` する。プールが仕事を返したら (スレッドを1本も始められない)、記録を外して誤りの完了にする。
  - `Listen` は `backend.listen` の結果をその場の完了にする。
  - `Connect` は、綴りが IP なら1つの番地で `connect` に入る。名前なら、まず `absorb` で戻った解決を数えから外し、それから `resolver.submit` する。断られたら誤りの完了、受けたら `Work::Lookup` の記録を入れて `fill_lookups` する。
- `cancel`: `Local::cancel`
- `completions(out)`: 表を歩かない。
  1. `empty_bin` を呼び、壊れていれば何もしない。
  2. `table.io_len() > 0` なら `poll(Some(ZERO))` を呼ぶ。壊れたら何もしない。
  3. `absorb` を呼ぶ。
  4. `out.append(&mut done)` で移し、足した区間を `OpKey` で並べる。
- `pending`: `table.len() + done.len()`
- `waker`: `Arc::clone(&shared.waker)`。`bin`: `Arc::clone(&bin)`
- `shutdown_write`、`set_nodelay`: ハンドルを引き (なければ `no such handle`)、`backend` の同名の関数を呼ぶ。
- `close`: `handles.remove`。保留の操作は自分の `Rc` で OS の資源を生かす。
- `quiesce`:
  1. `empty_bin` を呼ぶ。
  2. `resolver.abandon_all()` を呼び、`Lookup` の記録をすべて外す (完了は作らない)。
  3. `discard` し、表のすべての鍵を `cancel` し、もう一度 `discard` する。
  4. 表が空になるか壊れるまで、`poll(Some(IDLE_TICK))`、`absorb`、`discard` を繰り返す。仕事は時間の上限なしに待つ。この間の `absorb` は仕事の panic をその場で起こし直さず、最初の1つを覚えておく。繰り返しを抜けたら、覚えた panic を `resume_unwind` する。完了の形の backend で、`quiesce` の途中の巻き戻しが `Drop` の I/O の待ちに入らないためである。`Drop` は `absorb` を呼ばない。
  5. debug のビルドでは、始めから `QUIESCE_WATCH` (5 秒) たっても `io_len() > 0` なら、残った I/O の鍵を昇順に並べて `panic!("cancelled I/O ops did not complete within 5 s: {keys:?}")` する。見張りは driver の状態を変えない。
- `Drop`: §10 の表のとおり。

VM との約束: 1つのハンドルの同じ向きに、保留の操作を2つ以上出すことはない。VM は受け付けでストリームを記録に移す (vm.md の「`Async` のタスク」の操作の記録、`submit_write` の箇条) ので、ストリームごとに保留の操作は高々1つである。driver はこの約束に頼らない。同じ向きの操作が待っていれば、後の操作は試さずに列の後ろに積む (§6)。ただし短い書きの続きは列の後ろに戻るので、同じハンドルに書きを重ねた呼び手には、バイトの順を約束しない。`tests/real.rs` の `quiesce_drains_a_pending_probe_a_full_write_and_a_running_job` は同じハンドルに2つの書きを出すが、どちらも終わらない前提なので順に依らない。

## 6. epoll と kqueue の backend

関心の方式は level にする。fd ごとに `FdQueue { readers: VecDeque<OpKey>, writers: VecDeque<OpKey> }` を `HashMap<RawFd, FdQueue>` に持ち、列が空でない間だけ OS に登録する。

epoll:
- 読みの関心は `EPOLLIN | EPOLLRDHUP`、書きの関心は `EPOLLOUT`
- 列が空から空でなくなれば `EPOLL_CTL_ADD`、関心の組が変われば `EPOLL_CTL_MOD`、両方の列が空になれば `HashMap` から外して `EPOLL_CTL_DEL`
- `data` は fd の番号で、doorbell は `u64::MAX`

kqueue:
- 向きごとに独立に、`EVFILT_READ` と `EVFILT_WRITE` を、列が空でなくなったときに `EV_ADD`、空になったときに `EV_DELETE` する
- `EV_CLEAR`、`EV_ONESHOT`、`EV_DISPATCH` は使わない。旗のない filter は今の状態を返し続ける (XNU の `bsd/man/man2/kqueue.2` の EV_CLEAR の項から読んだ)
- 変更は1回の `kevent` に1つで、事象の列は渡さない。誤りは `-1` と errno で返る。`EV_RECEIPT` は使わない
- `close` で knote は消える (同じ man)

両方に共通の決め:
- 関心を外す呼び出し (`EPOLL_CTL_DEL`、`EV_DELETE`) の誤りは捨てる。完了は表から作るので、操作が落ちることはない。
- 表にない fd の事象は起きない。登録があるのは列が空でない間だけで、記録の `Rc` がその fd を開いたままにし、1回の待ちで1つの fd の事象は1つ (kqueue では filter ごとに1つ) だからである。`debug_assert!(false, "an event for fd {fd} without a queue")` を置き、release では捨てる。
- 登録の寿命は「その fd に保留の操作がある間」に一致する。fd が閉じた後に番号が使い回されても古い登録が残らず、backend に `close` の知らせが要らない。保留の操作のない fd を登録しない。

1周の syscall の数 (設計から数えた):

| 1周 | level (この設計) | oneshot |
|---|---|---|
| `Readable` の後に `Read` | ADD、wait、DEL、recv の4つ | MOD、wait、recv の3つ |
| データのない `Read` | recv (EAGAIN)、ADD、wait、recv、DEL の5つ | recv、MOD、wait、recv の4つ |
| 短い書きの続き1回 (送りの buffer が満ちた後) | wait の中の send、DEL、`start` の send (EAGAIN)、ADD、wait の5つ | send、send (EAGAIN)、MOD、wait の4つ |

最適化は今は入れない (本体 10 章)。入れるときの最初の対象は書きの続きで、形は「列が空になった fd の登録を、その fd を持つ最後の記録が落ちるまで外さない。backend に `release(&Socket)` を1つ足す」である。登録を外すのを遅らせるだけの形は、kqueue で番号の使い回しに気づけないので採らない。

`Op` ごとの手順 (`src/sys/unix/ops.rs` の安全な関数 `first` と `ready` を epoll と kqueue が共有する):

| 操作 | `start` の中 (`ops::first`) | 読める・書けると知らされた後 (`ops::ready`) | 列 |
|---|---|---|---|
| `Readable` | 試さずに列に積む | `Ok(Size(0))` | 読み |
| `Read` | `socket.recv(buf.room())` | 同じ | 読み |
| `Write` | Linux は `send_with_flags(bytes(written), MSG_NOSIGNAL)`、Apple は `send` (ソケットに `SO_NOSIGPIPE`) | 同じ | 書き |
| `Accept` | Linux は `accept4(SOCK_NONBLOCK \| SOCK_CLOEXEC)`、Apple は `accept` の後に `set_nonblocking(true)` と `set_nosigpipe(true)` (socket2 0.6.5 の `accept` は Apple で CLOEXEC を立てる) | 同じ | 読み |
| `Connect` | `socket.connect(peer)`。`Ok` は `Connected`、`EINPROGRESS` か `EINTR` は列に積む | `socket.take_error()` (`SO_ERROR`)。`None` は `Connected`、`Some(e)` は `Err(e)` | 書き |

`start` の手順:
- その fd の同じ向きの列に先に待つ操作があれば、試さずに `Try::Wait` として後ろに積む (追い越さない)。
- `recv`、`send`、`accept` は `Interrupted` でその場でやり直し、`WouldBlock` で `Try::Wait` を返す。
- `Try::Wait` なら鍵を列の後ろに積んで登録を合わせる。登録に失敗したら列から外して `Start::Done(Err(e))` を返す。

`wait` の手順:
- 事象の fd の列を引く。
  - epoll: `events & (読み | EPOLLHUP | EPOLLERR)` なら読みの列を、`events & (EPOLLOUT | EPOLLHUP | EPOLLERR)` なら書きの列を見る。
  - kqueue: `filter` で列を選ぶ。`EV_EOF` は見ず、次の syscall に誤りを言わせる。
- 列の先頭から `ops::ready` を呼ぶ。`Try::Wait` なら先頭に残して止める。`Done` なら外して `Finished { cancelled: false }` を積み、次の先頭へ進む。
- 最後に登録を合わせる。登録に失敗した fd の残りの操作は、その誤りの写しで終える。

そのほか:
- `cancel`: その fd の列から鍵を外して登録を合わせ、`Finished { key, Err(ECANCELED), cancelled: true }` を積む (誤りは核が替える)。OS を呼ばないので失敗しない。
- 時間: epoll は `timeout.as_nanos().div_ceil(1_000_000)` を `i32::MAX` で切った ms にする (切り上げ。1 ms 未満の残りで 0 を渡して回らないため)。`None` は `-1`。kqueue は `timespec` にそのまま写す。
- `wait` の `EINTR` は `Ok(Woke::TimedOut)` で戻す。ほかの誤りは `Err`。
- `wait` の戻り値: `Finished` を1つでも積めば `Woke::Finished`、doorbell だけなら `Woke::Woken`、何もなければ `Woke::TimedOut`。
- 事象の列: `vec![…; 256]` を `new` で1回作って使い回す。256 を超える事象は level なので次の `wait` で届く。`epoll_event` は値で読み、欄の参照を作らない。x86_64 の `libc::epoll_event` は packed である。
- fd の CLOEXEC: epoll は `epoll_create1(EPOLL_CLOEXEC)`、kqueue は `kqueue()` の後に `fcntl(F_SETFD, FD_CLOEXEC)`。
- `drain_broken`: 空の本体。readiness ではカーネルが記録の記憶に後から書かず、fd が閉じれば登録も消える。

epoll と kqueue の backend、`ops.rs`、doorbell は、試作で合わせて約 550 行だった。

## 7. doorbell (`src/sys/unix/doorbell.rs`)

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
- Apple: `std::io::pipe()` の両端を `fcntl(F_SETFL, O_NONBLOCK)` にする。書くのは1バイト。std は Apple で `pipe` の後に両端へ CLOEXEC を立てる (rust-src 1.99.1 の `std/src/sys/pipe/unix.rs`)。
- `clear` は、backend が doorbell の fd が読めると知ったときだけ呼ぶ。順序は「fd を読む → `rung.swap(false, AcqRel)`」である。逆にすると、reset の後の起こしの書きを読み捨て、旗が立ったまま以後の起こしが syscall を出さなくなる。`swap` の acquire は起こした側の `post` の lock の解放と同期するので、`wait` の後の `absorb` は起こした側が積んだ受け箱の中身を見る。
- 旗が立つのは書きの前だけなので、fd に残るのは高々1つの書きで、パイプは満ちない。`wake` は確保も lock もしない。
- driver を落とした後の `wake` は、`Arc` が両端を生かすので、EPIPE にも SIGPIPE にもならない。読む端を先に閉じると、埋め込んだ eml では SIGPIPE が既定のままで process が終わりうる (Rust の std が `main` の前に SIGPIPE を無視にするのは実行ファイルのときだけである)。
- `eml_vm` の監視のスレッドの自己パイプとは型を分ける。あちらはシグナルの handler が書くので、別の規則に従う。
- 本番で driver の waker を鳴らすのは、解決のスレッド、プールのスレッド (`Shared::post`)、`eml_vm` の監視のスレッドである。どれもシグナルの handler ではない。

## 8. `sys::Backend` の選び方と `socket.rs`

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

IOCP の最小の形 (T-5) の `listen` と `socket` は `Unsupported` を返す。`net-windows` で `listener` と `stream` を呼ぶ形に替える (付録2)。

## 9. プールと解決器

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
| `Drop` | lock して `closed = true` にし、まだ始まっていない仕事を列から取り出し (lock を放してから落とす)、`notify_all`。`busy == 0` ならすべてのスレッドを join する (空きのスレッドは知らせで終わるので短い)。`busy > 0` なら join しない。走っている仕事は後で終わり、結果は生きている `Shared` に積まれて捨てられ、スレッドは `closed` を見て終わる |
| `Drop` の後に残るもの | `busy > 0` のとき、残るのは走っている仕事のスレッドと空きのスレッドで、どれも `Arc<Shared>` と `Arc<Queue>` を持つ。空きのスレッドは `notify_all` で起きて終わり、走っているスレッドはその仕事が終われば終わる。`Shared` (受け箱と waker) は最後のスレッドとともに落ちる |
| `quiesce` | 仕事の記録は列にあるものも走っているものも表にあるので、核の `quiesce` の待ちがすべてを待つ |
| Miri | `Masked` は `cfg(any(not(unix), miri))` で何もしない型にする (Miri に `pthread_sigmask` の shim がない)。`busy > 0` の道は `dropping_a_driver_does_not_wait_for_a_running_job` が Miri でも通り、残ったスレッドの報告は出ない (試作で `-Zmiri-ignore-leaks` なしで通った) |

受け付けを閉じる形は `closed` の旗と `notify_all` にする。`mpsc` を `Mutex<Receiver>` で共有すると、受ける側が lock を持ったまま待つので、ほかの空きのスレッドの 60 秒の数えが止まるためである。列の長さに上限はない。

解決器 (`src/real/resolve.rs`): 今の `resolve.rs` から mpsc の channel、`ready` の列、waker を外し、`Arc<Shared>` を持たせる。全 OS でコンパイルする。Windows では `Backend::socket` が `Unsupported` を返すので、名前の接続は解決の後に `Unsupported` で終わる。

```rust
pub type Lookup = fn(&str, u16) -> io::Result<Vec<SocketAddr>>;
pub fn std_lookup(host: &str, port: u16) -> io::Result<Vec<SocketAddr>>;   // 今と同じ
pub struct Resolver { lookup: Lookup, shared: Arc<Shared>, live: HashSet<OpKey>,
                      queue: VecDeque<(OpKey, String, u16)>, abandoned: HashSet<OpKey> }
impl Resolver {
    pub fn submit(&mut self, key: OpKey, host: String, port: u16) -> io::Result<()>; // 64 本詰まっていれば Err
    pub fn fill(&mut self) -> Vec<(OpKey, io::Error)>;  // 8 本まで spawn_masked で始め、始められなかった鍵を返す
    pub fn returned(&mut self, key: OpKey) -> bool;     // live から外せたら真。abandoned なら数えから外して偽
    pub fn abandon(&mut self, key: OpKey);              // live を abandoned へ移し、queue からは外す
    pub fn abandon_all(&mut self);
}
```

- スレッドは `lookup` の結果を `shared.post(Message::Lookup { key, result })` で積む (積んでから waker が鳴る)。driver を落とした後も `Arc<Shared>` が生きているので積みは成功し、結果は捨てられる。
- 名前 `eml-resolve`、スタック 256 KiB、`RESOLVE_LIMIT = 8`、`ABANDONED_LIMIT = 64`、誤りの文言は今と同じ。

## 10. `Drop` と `quiesce`、`Record` の寿命

| | readiness (epoll、kqueue、`Fake<false>`) | 完了の形 (IOCP、`Fake<true>`) |
|---|---|---|
| `quiesce` | 取り消しの完了がその場で積まれるので、仕事だけを待つ | 取り消しの完了を `poll` で受け、仕事も待つ |
| `Drop` (壊れていない) | `empty_bin` だけで何も待たない。欄が落ちると、記録とハンドルの `Rc` が落ちて fd が閉じ、プールの `Drop` が走る | `io_len() > 0` なら I/O の鍵をすべて `cancel` し、`io_len() == 0` になるか壊れるまで `poll(Some(IDLE_TICK))` を繰り返す。時間の上限はない。debug では 5 秒たっても残れば `eprintln!` で1回書き、panic しない (巻き戻しがカーネルの書く記憶を放さないため) |
| `Drop` (壊れている、または待ちの途中で壊れた) | `drain_broken` は何もしない | `backend.drain_broken(&mut table)` を呼び、戻ってから欄を落とす。`drain_broken` は取り消しを出し、合わせて 1 秒まで終わりを確かめる。終わった箱は放す。終わらない操作が残れば、残った鍵を標準エラーに書いて process を abort する |
| 仕事と解決 | 待たない | 待たない |

```
submit ─ Io ─ advance ─ start ─┬─ Done ─ settle ─┬─ done へ ── completions で渡す / quiesce と Drop で捨てる
                               │                 └─ advance (続き)
                               └─ Pending ─ table.insert ─ (backend の wait / cancel) ─ Finished ─ table.remove ─ settle
submit ─ Read/Write で cap == 0 ─ done へ (backend を呼ばない)
submit ─ Job ─ table.insert ─ pool ─ Message::Job ─ absorb ─ table.remove ─ done へ
submit ─ Lookup ─ table.insert ─ Message::Lookup ─ absorb ─ table.remove ─ connect ─ advance … / done へ
quiesce ─ Lookup の記録は完了を作らずに外す
Drop ─ 完了の形の backend だけ: 壊れていなければ取り消して待つ / 壊れていれば drain_broken (1 秒まで確かめ、終わらなければ abort)
```

`mem::forget` と `Box::leak` は `eml_io` のどこにも書かない。abort の前の書き出しは `let _ = writeln!(std::io::stderr(), …)` で書き、書きの誤りを捨てる (`eprintln!` は標準エラーに書けないと panic する)。

## 11. `buf.rs`、unsafe の一覧、数えるテスト

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
    /// テストが持つ Vec を貸す。ptr と cap は Vec の先頭と長さで、呼び手は完了まで Vec を生かし、動かさない
    pub fn of(bytes: &mut Vec<u8>) -> Lent;
}
```

- slice を作るのは `bytes` と `room` の2つだけである。どちらも安全な関数で、前提は `RawBuf` の約束 (貸した側が完了を取り出すまで記憶を生かし、一意に保ち、動かさない) である。SAFETY のコメントは今の `real.rs` の `Mem` の文を写す。
- `Lent::new` は安全な関数のままにする。`RawBuf` の約束を負うのは `IoDriver::submit` の呼び手 (VM) で、`eml_rt` の `submit` と `RawBuf` の doc がそれを言う。`Lent::new` を `unsafe fn` にしても、その上の `submit` が安全な関数である限り、義務の場所は変わらない。
- `Lent::new` を呼ぶのは、`real/mod.rs` の `submit`、テストの driver の決着 (`test.rs`)、`buf.rs` の中 (`Lent::of`) だけにする。`grep -rn "Lent::new" crates/eml_io/src` で確かめる。
- `Lent::of` は T-3 で足す。`net-windows` の `wsa.rs` のテストが使う。この段では §12 の `a_lent_slices_nothing_past_its_end` が使う (使う所がないと test の build で dead_code になるため)。
- テストの driver は `Lent::new(buf).bytes(0)` で写し、読みは `room()` の各要素に `write` する。`test.rs` の4か所の `unsafe` と `unsafe fn bytes` は消える。試作で `test_driver.rs` のテストがすべて通った。
- IOCP は `room().as_mut_ptr()` と `bytes(from).as_ptr()` を `WSABUF` に写し、slice を持ち続けない。

`#![deny(unsafe_code)]` は `crates/eml_io/src/lib.rs` と `crates/eml_io/tests/main.rs` の頭に置く。`#![allow(unsafe_code)]` は次のファイルの頭にだけ書く (最後の形)。

| ファイル | unsafe の中身 |
|---|---|
| `src/buf.rs` | `slice::from_raw_parts`、`from_raw_parts_mut`、`ptr.add` |
| `src/mask.rs` | `pthread_sigmask`、`sigemptyset`、`sigaddset` (`cfg(all(unix, not(miri)))`) |
| `src/sys/unix/doorbell.rs` | Linux の `eventfd` と `from_raw_fd`、Apple の `fcntl` |
| `src/sys/unix/epoll.rs` | `epoll_create1` と `from_raw_fd`、`epoll_ctl`、`epoll_wait` |
| `src/sys/unix/kqueue.rs` | `kqueue` と `from_raw_fd`、`fcntl(F_SETFD)`、`kevent` (変更と待ち) |
| `src/sys/windows/iocp.rs` | 付録2 (T-5 から) |
| `tests/real.rs` | `pthread_sigmask` の読み、`signal(SIGPIPE)` の設定と戻し |

途中のコミットの `LISTED`:

| コミット | `LISTED` |
|---|---|
| T-2 | `src/buf.rs`、`src/mask.rs`、`src/real.rs`、`tests/real.rs` |
| T-3 | `src/real.rs` を `src/compio.rs` に替える |
| T-4 | `src/sys/unix/doorbell.rs`、`src/sys/unix/epoll.rs`、`src/sys/unix/kqueue.rs` を足す |
| T-5 | `src/sys/windows/iocp.rs` を足す |
| T-7 | `src/compio.rs` を外す |

`src/sys/unix/ops.rs` と `src/sys/socket.rs` には unsafe を置かない。

数えるテスト (`crates/eml_io/tests/unsafe_code.rs`、module `unsafe_code`、`tests/main.rs` で宣言する):
- テストは `unsafe_lives_only_in_the_listed_files` と `the_search_ignores_comments_strings_and_chars` の2本。
- 読み方: `env!("CARGO_MANIFEST_DIR")` の下の `src` と `tests` を `std::fs::read_dir` で再帰に歩き、`.rs` をすべて読む。道は `/` で書く (Windows の `\` を直す)。新しいファイルも漏れない。Miri では流さない (隔離でファイルを読めない)。
- 前処理 (`blank`): コメント (`//` から行末、入れ子の `/* … */`)、文字列 (`"…"` と `\` の escape、`r"…"`、`r#"…"#`)、文字の literal (`'"'`、`'\''` など。生存期間の `'a` は閉じる `'` がないので残す) を空白に替える。除くのはコードでない所だけなので、一覧の外の `unsafe` を見逃さない。
- 数え方: 語 `unsafe` は、前後の文字が識別子の文字 (英数字と `_`) でない出現だけを数える。`unsafe_code` は数えない。
- allow の判定: 語 `unsafe_code` の出現ごとに、その前の最も近い `#[` か `#![` から属性の名前 (最初の `(` か `]` の前) を読む。
  - `deny` と `forbid` は禁じる側として数える。
  - それ以外 (`allow`、`expect`、`warn`、`cfg_attr`、属性の外の出現) は外す側として数える。`#![allow(dead_code, unsafe_code)]` も `#[cfg_attr(test, expect(unsafe_code))]` も外す側に入る。
- 比べるもの:
  - 語を持つファイルの集合が `LISTED` に等しい。
  - 外す側を持つファイルの集合が `LISTED` に等しい。
  - 禁じる側を持つファイルの集合が `DENIED = ["src/lib.rs", "tests/main.rs"]` に等しい。
  - 語 `unsafe` の次の語が `impl` の出現がどのファイルにもない。
- `LISTED` は在るファイルで絞らない。前処理で文字列を空白にするので、このテストのファイルは語を2つに分けて書かなくてよい。
- 2本目のテストは、前処理と判定の規則を小さい入力で確かめる。入れ子のコメント、escape と raw の文字列、`'"'`、3つの属性の形を使う。

試作で、この2本が完成の形と T-4 の形の木で通った。

## 12. 偽の backend と核の単体テスト、Miri

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
    pub fail_wait: Option<io::Error>,          // 次の wait をこの誤りにする
    pub log: Arc<Mutex<Vec<String>>>,          // 落とした後も読める ("cancel 3"、"deliver 3"、"drain 3")
    next_socket: u32,
}
impl<const C: bool> Fake<C> { pub fn finish(&mut self, key: OpKey, result: io::Result<Outcome<FakeSocket>>); }
impl<const C: bool> Backend for Fake<C> { type Slot = (); type Socket = FakeSocket; const KERNEL_WRITES_AFTER_CANCEL: bool = C; … }
```

台本の約束:
- `C = false` は readiness の台本、`C = true` は完了の台本である。
- `cancel` は `on_cancel` に従って終わりを作る。`Abort` は誤りの `cancelled: true`、`Race(r)` は `r` の `cancelled: false`、`Never` は何も作らない。
- 終わりを置く所は台本で違う。readiness は `out` に、完了の形は `ready` に置く。
- 届けるとき `pending` から外す。外せなければ `op {key} finished twice` で panic する。「ちょうど1回」を backend の側からも確かめる。
- `wait` は、`fail_wait` があればそれを `Err` で返す。`ready` があれば全部を移して `Finished` を返す。なければ `Bell` を `timeout` まで待ち、`Woken` か `TimedOut` を返す。
- `drain_broken` は `pending` の鍵ごとに `drain k` を `log` に積み、`pending` を空にする。abort の道は偽の backend で確かめない。
- `listen` と `socket` は番号を振るだけで OS を呼ばない。そのため `Accepted` の台本も Miri で流せる。

`src/real/tests/mod.rs` のテスト。どれも `cargo test` と Miri で流す。待つ補助の見張りは、Miri の仮想の時計で数えるので 60 秒にする (Miri の下では 33 本の仕事のテストが 1 秒の見張りを越えた)。

| テスト | 確かめること |
|---|---|
| `a_short_write_resumes_from_where_it_stopped` | `Done(3)`、`Done(2)`、`Pending`、後で `5` の台本で、完了は1つの `Size(10)`、`started` は同じ鍵の3回、`sent` は先頭からの5バイト |
| `a_write_without_progress_ends_with_write_zero` | `Done(0)` で `WriteZero` |
| `a_read_fills_the_lent_room` | `Done(3)` で、貸した記憶の先頭3バイトが台本の中身になる (`Lent::room`) |
| `ops_without_room_end_at_once` | `cap == 0` の `Read` と `Write` が `Size(0)` で終わり、`started` が空 |
| `a_lent_slices_nothing_past_its_end` | `Lent::of` で貸した3バイトの `bytes(1)` が後ろの2バイト、`bytes(3)` が空、長さ 0 の `Vec` の `room()` が空 |
| `a_readiness_cancel_completes_without_waiting` | 取り消した `Readable` が次の `completions` で `cancelled: true`。`waits == 0`、`pending() == 0` |
| `a_pending_op_finishes_once_with_or_without_a_cancel` | 完了の台本で、2回取り消した鍵と取り消さない鍵が、それぞれ1回だけ届く。次の `completions` は空 |
| `a_cancel_error_from_the_backend_becomes_the_cancelled_error` | 完了の台本の `Abort` の誤り (`fake cancel`) が、誤り `cancelled` と `cancelled: true` で届く |
| `a_result_that_beats_the_cancel_is_not_cancelled` | `Race(Ok(Size(4)))` で `Ok(4)`、`cancelled: false` |
| `a_cancelled_write_does_not_resume` | 取り消しと競って短い書きが着いても出し直さず、`Err(cancelled)` で `cancelled: true`。`started` は1回 |
| `an_aborted_accept_is_retried_under_the_same_key` | `ConnectionAborted` の後の `Accepted` で `Done::Stream { peer }`。`started` は同じ鍵の2回 |
| `a_connect_by_name_tries_the_next_address` | 解決器のスレッドが返す2つの番地で、1つ目の拒否の後に2つ目で `Stream`。`peer` は2つ目 |
| `completions_return_only_the_finished_ops` | 保留5つのうち2つを終えると、`completions` は鍵の昇順で2つを返し、`pending() == 3` |
| `a_job_completes_through_the_inbox` | 仕事の値が受け箱を通って `Size(7)` で届く |
| `jobs_beyond_the_pool_limit_wait_in_order` | 上限 2 のプール (`driver.local.pool = Pool::new(.., 2)`) に3つ出す。`pending() == 3`。3つ目は先の1つが終わってから始まり、受け付けの順に始まる |
| `quiesce_waits_for_queued_and_running_jobs` | 同じ組で、別のスレッドが門を開ける間 `quiesce` が待つ。3つとも走り、`pending() == 0` |
| `dropping_a_driver_does_not_wait_for_a_running_job` | 仕事が始まった後に落とし、落とした後に門を開けると仕事が終わる (`busy > 0` の道) |
| `a_job_panic_resumes_on_the_owner_thread` | `#[should_panic(expected = "boom")]`。仕事の panic を `completions` の中で起こし直す |
| `dropping_a_completion_driver_waits_for_cancelled_io` | `Fake<true>` を落とすと、`log` が `cancel k`、`deliver k` |
| `dropping_a_readiness_driver_does_not_cancel` | `Fake<false>` を落としても `log` は空 |
| `a_broken_completion_driver_drains_its_io_on_drop` | `fail_wait` で `wait` が `Err` になり、`quiesce` の後も `pending() == 1`。落とすと `Drop` が戻り、`log` が `cancel k`、`drain k` |
| `quiesce_raises_a_job_panic_after_draining_the_io` | 完了の台本で保留の `Readable` と panic する仕事を持つ。`catch_unwind` した `quiesce` の panic が `boom` で、そのとき `pending() == 0`、`log` に `deliver k` がある |
| `quiesce_panics_when_a_cancelled_op_never_finishes` | `#[cfg(debug_assertions)]`、`#[ignore = "5 秒の見張りを待つ"]`、`#[should_panic(expected = "did not complete within 5 s: [0]")]`。`Fake<false>` と `OnCancel::Never` で書く。完了の台本では、巻き戻しの `Drop` が終わらない操作を待ち続けるためである (試作の `--ignored` の実行が 5.1 秒で通った) |
| `the_shared_state_and_the_pool_work_cross_threads` | `send_sync::<Shared>()`、`send::<pool::Job>()`、`send::<Message>()` の型の assert |

数: 23 本と、ignored の1本。

同じ module に、`RealDriver` が `Send` でないことの型の assert を置く。形は `const _: fn() = || { … AmbiguousIfSend … }` で、`eml_rt/src/lib.rs` の `AmbiguousIfSync` と同じ方法である。

Miri:
- コマンド: `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_io --lib real::`。Tree Borrows は `MIRIFLAGS` に `-Zmiri-tree-borrows` を足す。
- 試作 (この表から `a_lent_slices_nothing_past_its_end` を除き、後で外した1本を足した23本) の `user` は、4 CPU の Linux のコンテナでほかのビルドのない状態で、Stacked Borrows 8.9 秒、Tree Borrows 8.9 秒だった (bash の `time` で1回ずつ)。
- 予算 (testing.md に書く): Stacked Borrows 30 秒、Tree Borrows 30 秒。1本のテストを足して増える `user` は、2つのモードの合計で 2 秒以内。T-3 で実装の全テストを `/usr/bin/time -p` で測り直し、測った値の約3倍に丸めて書く。数と機械はコミットのメッセージに書く (testing.md の「Miri」の規則)。
- 33 本の仕事のテストは Tree Borrows で1本 4〜5 秒かかった。そのため核のテストは上限 2 のプールで流し、32 本の上限そのものは `tests/real.rs` の本物のテストで確かめる。
- `src/sys/tests.rs` は名前に `real::` を含まないので、Miri の絞り込みに掛からない。T-4 の取り込みの module 名を `driver` にするのも同じ理由である。

## 13. `sys::tests` (OS の backend の5本)

`src/sys/tests.rs`。Miri では流さない。IOCP も同じ5本を通し、`iocp.rs` に同じテストを置かない。

| テスト | 確かめること |
|---|---|
| `a_wake_from_another_thread_ends_a_long_wait` | 50 ms 後の別のスレッドの `wake` で、`Backend::wait(Some(900 ms))` が `Woke::Woken` で戻る。経過は 50 ms 以上 500 ms 未満 |
| `a_wait_without_a_wake_times_out` | `wait(Some(20 ms))` が `Woke::TimedOut`。経過の下限は比べない (Windows の待ちの粒度のため) |
| `wakes_before_a_wait_merge_into_one` | `wake` を2回 → `wait(Some(0))` が `Woken`、次の `wait(Some(0))` が `TimedOut` |
| `a_wake_after_the_backend_is_dropped_is_harmless` | waker を残して backend を落とし、`wake` を2回呼ぶ。panic しない |
| `the_driver_waker_wakes_its_backend` | `RealDriver::new()` の `waker()` を別のスレッドで 50 ms 後に鳴らすと、`wait_backend(Some(900 ms))` が `Woken`。`IoDriver::waker` と backend の結び付きを、`IDLE_TICK` を通さずに確かめる |

T-4 から T-5 の間は `mod tests` を `#[cfg(all(test, unix))]` にする。T-5 で `#[cfg(test)]` にし、取り込み (`driver`) だけを `cfg(all(test, unix))` に残す。

T-4 の取り込みの形 (`src/sys/tests.rs` の頭):

```rust
/// `tests/real.rs` を新しい driver の上で流す。差し替えで外す
type RealDriver = crate::real::Driver<super::Backend>;
#[path = "../../tests/real.rs"]
mod driver;
```

`#[path]` は `src/sys/` からの相対の道になる。試作で `../../../tests/real.rs` は見つからず、`../../tests/real.rs` で通った。

## 14. 既存のテストの扱いと足すテスト

前提: async-net の段で `pair` が消え、`pair` を使っていた unix のテストは名前を保ってループバックの TCP の上に書き直され、`pair_is_unsupported` は消えている。名前が違えば、本体 10 章の読み替えに従う。

`crates/eml_io/tests/real.rs` の書き換え:
- T-2 で頭に `#![allow(unsafe_code)]` を足す (種類3)。
- T-4 で `use eml_io::RealDriver;` を `use super::RealDriver;` にし、`tests/main.rs` に `use eml_io::RealDriver;` を置く (種類3)。
- T-6 から、結合テストとして新しい driver の上で流れる。

| テスト | 扱い | 種類 |
|---|---|---|
| `a_real_clock_does_not_go_back`、`a_real_clock_reaches_a_deadline`、`a_timed_wait_returns_at_or_after_its_deadline`、`a_real_driver_without_work_or_deadline_cannot_progress` | 残す | — |
| `a_wake_from_another_thread_leaves_a_wait_working` | 残す。説明の「`real.rs` の単体テスト」を `sys::tests` に直す | 3 |
| `dropping_a_driver_waits_for_a_running_job` | T-4 で消す。新しい driver は落とすときに仕事を待たない。T-4 から新しい driver の上でも流すので、差し替えより前に消す | 1 |
| `a_job_completes_with_its_value`、`a_job_pool_is_capped` | 残す (33 本目は列で待つ。説明の文はそのまま正しい) | — |
| `a_readable_probe_stays_pending_without_data`、`a_read_after_a_probe_returns_the_bytes`、`a_write_is_read_by_the_peer`、`a_cancelled_probe_completes_with_an_error`、`a_closed_peer_reads_eof`、`quiesce_drains_a_pending_probe_a_full_write_and_a_running_job` | 残す | — |
| `an_op_on_an_unknown_handle_fails` | 残す | — |
| `pool_threads_block_interrupt_and_terminate` | 残す (プールのスレッドは `spawn_masked` で始まる) | — |
| `a_listener_on_port_zero_gets_a_port` から `a_lookup_fails_while_64_abandoned_lookups_are_stuck` までの TCP と解決器の16本 | 残す | — |
| `listen_accept_and_connect_are_unsupported` (Windows) | T-6 で期待値を直す。`Listen` と IP の `Connect` は `Unsupported` のまま。`Accept { listener: 0 }` と `shutdown_write(0)` は `InvalidInput` (`no such handle`) にする。核は Windows の分岐を持たず、Windows ではハンドルが作られないので、ハンドルの検査が先に当たるためである。`net-windows` で消す | 2 |

試作で、上の unix の組が新しい driver の上で通った。T-4 の形の木では、同じ組が compio の上でも通った。Windows の行は Windows の機械がないので確かめていない。

T-6 で足すテスト。新しい driver でしか成り立たないので、取り込みの間 (T-4、T-5) は足さない。どれも試作の Linux で通った。

| テスト | cfg | 確かめること |
|---|---|---|
| `dropping_a_driver_does_not_wait_for_a_running_job` | なし | 200 ms 眠る仕事が始まった後に落とすと、50 ms 以内に戻る。仕事は後で終わる (1 秒の見張り) |
| `a_job_beyond_the_pool_limit_does_not_block_submit` | なし | 100 ms の仕事を32本出した後の33本目の `submit` が 10 ms 以内に戻り、`Size(33)` で終わる |
| `a_wake_from_another_thread_ends_a_wait_with_a_far_deadline` | なし | 60 秒先の期限の `wait` が、別のスレッドの `wake` の後に `Ok(true)` で 1 秒以内に戻る。`IDLE_TICK` があるので、これだけでは起こしの道を確かめられない。起こしの道は §13 の `the_driver_waker_wakes_its_backend` が確かめる |
| `a_wake_after_the_driver_is_dropped_raises_no_sigpipe` | unix | `SIGPIPE` を `SIG_DFL` にし、落とした driver の waker を別のスレッドから2回鳴らして落とし、前の処理に戻す。process が終わらない |
| `completions_return_only_the_finished_ops` | unix | 8本の接続に `Readable` を出し、3本の相手に書いて3つを受け取る。その後の `completions` は空で `pending() == 5`、`quiesce` の後は 0 |
| `a_cancelled_probe_completes_without_a_wait` | unix | 取り消した `Readable` が、`wait` なしの次の `completions` で `cancelled: true` で届き、`pending() == 0` (readiness の backend の性質) |

ほかのテスト:
- `src/real.rs` (compio) の単体テスト `a_wake_from_another_thread_ends_a_long_poll`: T-3 で `compio::tests::…` に名前が変わり (種類3)、T-6 で消す (種類1)。代わりは T-4 の `sys::tests::a_wake_from_another_thread_ends_a_long_wait` である。
- `crates/eml_vm/tests/api.rs` の本物の driver のテスト: すべて残し、期待値も変えない。`exit` が仕事を待つのは `vm::run` の後の `quiesce` で、`Drop` ではないので、新しい driver でも成り立つ。試作で `cargo test -p eml_vm --test integration api::` の23本が通った。
- `crates/eml_io/tests/test_driver.rs`: すべて残す。`test.rs` の書き換えは期待値を変えない (種類3)。

## 15. aarch64-linux: flake、CI、ASan

`flake.nix` の差分。試作の flake で `nix eval` して確かめた (aarch64-linux の `devShells` が `[ "cross" "default" "miri" ]`、`packages` が `[ "default" "eml" ]`、x86_64-linux の `packages` が `[ "default" "eml" "verus" "verusfmt" ]`)。

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

- `nix/verus.nix` と `nix/verusfmt.nix` の `meta.platforms` は変えない。
- `.#miri` は aarch64-linux でも出る (nightly の `complete` は system によらない)。
- `.#cross` に `aarch64-unknown-linux-gnu` と `aarch64-apple-darwin` の rust-std を足す。Linux の開発機で、kqueue のコードと aarch64 の構造体の差を型の検査にかけるためである。link はしない。この container で `cargo clippy --workspace --all-targets --target aarch64-apple-darwin -- -D warnings` と `--target aarch64-unknown-linux-gnu` が、compio の木と新しい driver の木の両方で通った。

`.github/workflows/ci.yml`:
- `test` の matrix を `os: [ubuntu-24.04, ubuntu-24.04-arm, macos-15, windows-2025]` にする。手順は変えない。`taiki-e/install-action` の固定した SHA の manifest (`manifests/cargo-nextest.json`) に `cargo-nextest` 0.9.146 の `aarch64_linux_gnu` があるので、`fallback: none` のままでよい。
- `fmt`、`verus`、`miri` は `ubuntu-24.04` のまま。
- T-3 で `miri` ジョブに次の手順を足す。`MIRIFLAGS` は matrix の env で与えているので、この手順も2つの設定で流れる。

```yaml
      - name: "cargo miri test -p eml_io --lib real::"
        if: ${{ !cancelled() && steps.rt.outcome != 'skipped' }}
        run: |
          nix develop .#miri -c cargo miri test -p eml_io --lib real::
```

- T-8 で `asan` ジョブを足す。この container で `.#miri` の nightly の `RUSTFLAGS=-Zsanitizer=address cargo test -p eml_io --target x86_64-unknown-linux-gnu` が組め、壁時計 32 秒 (冷えた build) で通った。LeakSanitizer の報告は出なかった。

```yaml
  asan:
    runs-on: ubuntu-24.04
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false
      - uses: cachix/install-nix-action@13d8dd58da0234aa297dedd986986ccb8e7f3e24 # v31.11.1
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
- testing.md「Verus」: flake の段落に足す。「Verus と verusfmt の release は aarch64-linux の物を出さないので、flake は aarch64-linux の devShell に `verus`、`cargo-verus`、`verusfmt` を入れない。`eml_rt` を変える作業の関門 (`cargo verus verify -p eml_rt` と verusfmt の検査) は、x86_64-linux か aarch64-darwin の機械で流す」
- testing.md「よく使うコマンド」: `.#cross` の行の注記に、aarch64 の2つの target を足す。
- architecture.md の toolchain の箇条: Verus と verusfmt の箇条に「aarch64-linux の devShell にはない」を足す。
- CLAUDE.md
  - Commands の前の devShell の段落に足す: 「On aarch64-linux the devShell has no `verus`, `cargo-verus` or `verusfmt` (Verus ships no aarch64-linux release); run the `eml_rt` gate on x86_64-linux or aarch64-darwin.」
  - `.#cross` の説明 (「the `.#cross` shell adds the Windows msvc and Linux `rust-std`」) と、Commands の `.#cross` の行の注記に、`aarch64-unknown-linux-gnu` と `aarch64-apple-darwin` を足す。
  - Testing の CI の行を「on macOS, Linux (x86_64 and aarch64) and Windows」にする。
- `crates/eml_rt/CLAUDE.md`: 最初の箇条に「(not available on aarch64-linux; see docs/implementation/testing.md の「Verus」)」を足す。

T-1 で arm の行が落ちたとき (本体 10 章の Q-2): T-1 は、arm の行と arm の文を入れない形でコミットする。
- 入れるもの: flake の3つの system、`.#cross` の2つの target、testing.md の `.#cross` と `target_arch` と Verus の文、architecture.md、CLAUDE.md の devShell と `.#cross` の文、`eml_rt/CLAUDE.md`
- 入れないもの: ci.yml の matrix の4つ目、testing.md「OS ごとのテスト」の頭の文の変更 (3つの OS のまま)、testing.md「CI」の arm の文、CLAUDE.md の Testing の CI の行
- 落ちたテストの名前と誤りは、T-1 のコミットのメッセージに書く。
- 入れなかった部分は T-6 (差し替え) に入れる。T-6 の完了の基準に `test (ubuntu-24.04-arm)` の緑を足す。
- この形なら、docs は各コミットの今の状態 (flake は aarch64-linux を出すが、CI は arm で流さない) を書く。

## 16. docs の割り当てと文案

### 16.1 compio の行の割り当て

`git grep -n -i compio -- . ':!docs/plans'` の各行を、それを消すタスクに割り当てる。行の番号は d1dd4f3 のもので、async-net の T-9 から T-13 が async.md と roadmap を動かすので、着手のときに grep し直して中身で当てる。

| ファイル (d1dd4f3 の行) | 中身 | 消すタスク |
|---|---|---|
| CLAUDE.md:89 | Architecture の `eml_io` の行 | T-6 |
| crates/eml_io/src/lib.rs:1 | crate の doc | T-6 |
| docs/future/async.md:28 | 決めの表の行 10 の「本物の driver は `compio-driver` の上に作り…」 | T-6 (「本物の driver は OS ごとの backend (epoll、kqueue、IOCP) を自前で書く」に) |
| docs/future/async.md:242、243 | 「`IoDriver` と `eml_io`」の compio の2つの箇条 | T-6 |
| docs/future/async.md:264 | 「ブロックする呼び出しのプール」の最初の箇条の `Asyncify` | T-6 (「本物の driver の中のプール (32本まで。すべて仕事中なら列で待つ)」に) |
| docs/future/async.md:312 | 「ネットワークとシグナル」の「driver の操作は `compio-driver` の操作で出す」の箇条 (async-net の T-13 の後に残っていれば) | T-6 (「unix の backend はソケットを socket2 と libc で扱う。Windows の accept と connect の後の `update_context` と、ConnectEx の前の bind は IOCP の backend が行う」に) |
| docs/future/async.md:367 | 「ファイルとパイプ」の `Decision::Blocking` の箇条 | T-6 (§16.3 の文に) |
| docs/future/async.md (main だけ) | 「ファイルとパイプ」の macOS の箇条の「compio-driver も、macOS のファイルの操作は `Decision::Blocking` としてプールで動かす」の文 | T-6 (この文だけを消す。箇条のほかの文は残す) |
| docs/future/async.md:392 と、main だけにある下の2つの箇条 | 「未決の論点」の driver の crate の箇条 | T-6 (§16.3 のとおり消す) |
| docs/future/roadmap.md:366 | 「io_uring の backend」の後に回した理由 (main では「見直すときの形」と「io_uring で失敗するテストと直し方」も) | T-0 (§16.2 の文に) |
| docs/future/roadmap.md:369 | 「(waker の本番の呼び出しと) 10 ms の上限をなくすこと」の後に回した理由 | T-6 |
| docs/future/roadmap.md (main だけ) | 「完了を集める手間」の項目 | T-6 (項目を消す) |
| docs/future/roadmap.md:374 | 「`compio-driver` の 0.13 の系列」 | T-7 (項目を消す) |
| docs/implementation/architecture.md:47、77 | crate の配置の2行 | T-6 |
| docs/implementation/architecture.md:352、358、360、365 | 「`eml_io` の内部」 | T-6 (書き直し) |
| docs/implementation/architecture.md:418、419、420、422、428 | 「外部 crate」の compio の2行、`socket2` の行の compio の理由、確かめる項目の一覧 | T-7 |
| docs/implementation/status.md:143 | 本物の driver の文 | T-6 |
| docs/spec/vm.md:455、458、462 | 「時計と driver」の3つの箇条 | T-6 |
| Cargo.toml:25、26、Cargo.lock、crates/eml_io/Cargo.toml:13、14 | 依存 | T-7 |
| crates/eml_io/src/real.rs (T-3 から compio.rs)、resolve.rs | compio の driver と古い解決器 | T-7 (消す) |

T-6 の後、`git grep -l -i compio -- . ':!docs/plans'` が出すファイルは次だけである。
- `Cargo.lock`、`Cargo.toml`、`crates/eml_io/Cargo.toml`、`crates/eml_io/src/compio.rs`、`crates/eml_io/src/lib.rs` (`mod compio;` の行)、`crates/eml_io/src/resolve.rs`、`crates/eml_io/tests/unsafe_code.rs` (`LISTED` の `src/compio.rs`)、`docs/future/roadmap.md` (0.13 の項目だけ)、`docs/implementation/architecture.md` (「外部 crate」だけ)

T-7 の後は0行である。

compio の語を含まないが、古い driver を書いた行も直す。
- roadmap の「io_uring の backend」の頭の文「unix の本物の driver は polling (epoll と kqueue) を使い…」: T-0 で §16.2 の文に替える。
- vm.md の「時計と driver」と「上限と割り込み」の、5秒、壊れた状態、安全点を通らずに待つ、プールのスレッドの生まれ方の文: T-6 (§16.4)。

### 16.2 T-0 の roadmap の文

- 「段の列」の `async-net` があった位置 (段の列の先頭) に足す:
  「- `io-backends`: 本物の I/O の driver を、OS ごとに自前で書く backend (Linux は epoll、Apple は kqueue、Windows は IOCP) に替え、`compio-driver` を外す。aarch64-linux を対象に足す。Windows の backend はこの段ではソケットを扱わない。前提: なし。設計: [非同期の設計](async.md) の「`IoDriver` と `eml_io`」」
- `net-windows` の文を「自前の IOCP の backend にソケットを載せ、同じネットのテストの組を Windows で流す。前提: `io-backends`」にする。async-net の閉じる作業が足した `Terminate` の文は残す。設計の要点は T-9 が足す (付録2 §C)。
- `async-threads`、`async-files`、`ffi-blocking` の「前提」に `io-backends` を足す。
- 「io_uring の backend」の項目を次の文にする。下の箇条は今のものを置き換え、「io_uring で失敗するテストと直し方」の箇条は消す。
  ```
  - io_uring の backend: unix の本物の driver は epoll と kqueue で待ち、io_uring を使わない ([アーキテクチャ](../implementation/architecture.md) の「`eml_io` の内部」)
    - 後に回した理由: `Std.Net` の計測がまだない。新しい対象の Mac の上の Linux コンテナでは、既定の seccomp が io_uring を塞ぐので、epoll の道はどのみち要る。
    - 見直す条件: `Std.Net` の計測で、readiness の道が支配的になったとき。
    - 見直すときの形: 本物の driver の backend を1つ足す。起動時に `io_uring_setup` と `IORING_REGISTER_PROBE` で使えるかを調べ、使えなければ epoll を選ぶ。CI は環境変数 `EML_IO_BACKEND` で backend を固定して `eml_io` のテストを両方で流し、選んだ backend の名前が環境変数と同じことをテストで確かめる。最低の kernel は 5.11 (`EXT_ARG`、`SHUTDOWN`)。`Readable` の完了は poll の revents で届くので、backend が `Ok(0)` にそろえる。壊れた driver を落とすときは、取り消しを出して 1 秒まで完了を読み、終わらなければ abort してから ring を閉じる
  ```

### 16.3 T-6 の async.md の文

- 決めの表の行 10: 「本物の driver は OS ごとの backend (epoll、kqueue、IOCP) を自前で書く」
- 「`IoDriver` と `eml_io`」の compio の2つの箇条を次の文に替える。「本物の driver は、OS ごとに1つの backend (Linux は epoll、Apple は kqueue、Windows は IOCP) を `libc` と `windows-sys` で直接呼ぶ。核は `OpKey` を鍵にした1つの表で操作を持ち、backend は1つの操作を1回ずつ出す。記録は持ち主のスレッドの表にだけあるので、driver を落とすときは仕事を待たない ([アーキテクチャ](../implementation/architecture.md) の「`eml_io` の内部」)」
- `Proactor::attach` の箇条: 「Windows では、ソケットを作るときに IOCP のポートに結び付けてから受けと送りを出す。今の Windows の backend はまだソケットを扱わない」
- 「ファイルとパイプ」の `Decision::Blocking` の箇条: 「`RawBuf` を持つ操作はプールに送らない。ファイルの読み書きをプールに回すときは、仕事が所有する buffer に読み、`Op::Job` の結果の型を読んだバイト列も返せる形に広げ、持ち主のスレッドが `buf.rs` で写す」
- 「未決の論点」の driver の crate の箇条と、その下の2つの箇条 (替えるもう1つの条件、替えるときの順) を消す。中の事実のうち今も成り立つものは、上の「`IoDriver` と `eml_io`」の新しい箇条 (1つの表、落とすときに仕事を待たない) と、roadmap の「io_uring の backend」(io_uring を足しても epoll を残す) にある。syscall の周りの `unsafe` の確かめ方は architecture.md の「`eml_io` の内部」に書く。
- 「未決の論点」の「受け付けをスレッドをまたいで安全に出せる backend (IOCP、SQ を lock する io_uring) では、受け箱を通す回り道を省いてよい。既定は backend ごとに `eml_io` が決める」の後段を「既定は `Shared` の受け箱を通し、`async-threads` の計測で見直す」にする。

### 16.4 T-6 の vm.md と eml_rt、eml_vm の文

vm.md「時計と driver」:
- `submit` の箇条の末尾: 「本物の driver は、書きの出し直し、受けの再試行、接続の次の試行のために、1つの鍵の下で OS の操作を何度か出す」。足す文: 「1つのハンドルの同じ向きに保留の操作を重ねた呼び手には、終わる順もバイトの順も約束しない。VM はストリームを記録に移すので、重ねない」
- `cancel` の箇条: 「取り消しても `key` の完了はかならず届く。取り消しの完了か、取り消しと競って着いた結果である」。5秒と壊れた状態の文を消す。
- `Completion::cancelled` の箇条: 「本物の driver は、取り消しを頼んだ操作が backend の取り消しの誤り (unix は `ECANCELED`、Windows は `ERROR_OPERATION_ABORTED`) で終わったとき、誤りを `cancelled` に替えて立てる。取り消しのために続きをやめたときと、名前の解決を手放したときも立てる」
- `Read` と `Write` の箇条: 「`cap == 0` の読みと書きは、OS に出さずに `Ok(0)` で終わる」
- `Job` の箇条のプールの文: 「本物の driver のプールのスレッドは32本までで、すべてが仕事中なら、仕事は driver の中の列で受け付けの順に待つ。`submit` は呼んだスレッドを止めない。空いたスレッドは 60 秒で終わる」。「安全点を通らずに待つ」とその理由を消す。
- 解決の箇条: 「名前の解決をプールでなく、driver の中の解決のスレッドで行う」(compio の語を消す)、「結果を driver の受け箱に積んでから waker を鳴らす」
- `quiesce` の箇条: 「仕事は終わるまで待ち、名前の解決は待たずに手放す」の後の compio の文と5秒の文を消し、代わりに書く。「本物の driver を落とすときは、仕事と名前の解決を待たない。仕事は終わるまでプールのスレッドで動き、結果は捨てる。まだ始まっていない仕事は捨てる。カーネルが取り消しの後も記憶に書きうる backend (IOCP) では、落とすときに I/O の操作をすべて取り消し、完了を取り出しきってから記録を放す」
- 壊れた driver の箇条: 「本物の driver は、壊れた後は完了を返さない」の後の5秒の文を消し、足す。「壊れるのは、OS の待ち (`epoll_wait`、`kevent`、`GetQueuedCompletionStatusEx`) そのものが失敗したときだけである。壊れた driver を落とすときは、カーネルがまだ書きうる操作の終わりを 1 秒まで確かめ、終わらなければ process を abort する。放した記憶にカーネルが書く道を残さないためである」
- `Machine` を落とす順の理由: 「本物の driver には、落とすときにカーネルの書き込みの終わりを待つ backend があり、その間も貸した記憶を生かしておくためである」
- `IDLE_TICK` の箇条: compio と kqueue の通知に依る理由の文を消す。async-net の後の文のうち、今の状態と食い違わない理由だけを残す (本体 10 章の Q-11)。
- Windows の文: 「Windows の本物の driver は IOCP の上にあり、まだソケットを扱わないので、`Listen` と `Connect` は `Unsupported` の誤りで終わる」

vm.md「上限と割り込み」: 監視のスレッドと解決のスレッドの文にプールのスレッドを足し、「どれも `eml_io::spawn_masked` で始まる」とする。「プールへ仕事を渡す間、VM のスレッドで塞ぐ」と「プールのスレッドは仕事を渡したスレッドから生まれ」を消す。

`crates/eml_rt/src/driver.rs` の doc:
- `submit`: 「失敗しない。出せなかった操作は次の `completions` で誤りとして終わる。本物の driver の仕事は、プールのスレッドが空くまで driver の中の列で待ち、`submit` は呼んだスレッドを止めない」
- `cancel`: 「できるだけ取り消す。`key` の完了はかならず届く (取り消しの完了か、取り消しと競って着いた結果)。`Op::Job` には何もしない」
- `quiesce` の後段: 「driver が壊れているときは、保留を残して戻る (呼ぶ側は `pending()` で確かめる)」

`crates/eml_vm/src/lib.rs` の `Machine::drop` のコメント (d1dd4f3 の lib.rs:606): 「本物の driver には、落とすときにカーネルの書き込みの終わりを待つ backend があるので、その間はヒープを生かしておく」

### 16.5 T-6 のそのほかの文

- architecture.md「`eml_io` の内部」: 「`RealDriver` の作り」の箇条を次の項目で書き直す。
  - 核 (`real/`)、backend (`sys/`)、`buf.rs`、プール、解決器、doorbell (§1 の表の形)
  - unsafe の一覧と数えるテスト、`Drop` の表 (§10)。syscall の周りの `unsafe` は Miri で動かせないので、4つの対象の CI の本物のソケットのテストと ASan で確かめること
  - テストの置き場所 (`real::tests` の偽の backend と Miri、`sys::tests`、`tests/real.rs`、`tests/unsafe_code.rs`)。起こしが長い待ちを終わらせることは、`sys::tests` が `Backend::wait` を直に待って確かめる
  - `sys/windows/iocp.rs` の行 (付録2 §A)
- roadmap「(waker の本番の呼び出しと) 10 ms の上限をなくすこと」: 後に回した理由から compio と kqueue の通知の文を消す (本体 10 章の Q-11)。
- roadmap「プールの仕事の予算と、放さずに終わる方針」: 本文から「安全点を通らずに待つ」を消し、「列の長さに上限がない」を足す。
- roadmap「完了を集める手間」: 項目を消す (main だけにある)。
- status.md: 本物の driver の文を「本物の driver は OS ごとの自前の backend (Linux は epoll、macOS は kqueue、Windows は IOCP) の上にある。Windows ではまだソケットを扱わない」にする。
- CLAUDE.md の Architecture の `eml_io` の行: 「IoDriver implementations: the real driver (own backends over libc and windows-sys: epoll on Linux, kqueue on Apple, IOCP on Windows) and the deterministic test driver; jobs run on the driver's own pool (at most 32 threads, queued beyond that)」。async-net が足した解決のスレッドの句は残す。

### 16.6 T-7 の文

- architecture.md「外部 crate」: compio の2行と、`compio-driver` の版の段落と、上げるときに確かめる項目の一覧を消す。`libc` の行に epoll、kqueue、eventfd、パイプを足し、`socket2` の行から compio の理由を消し、`windows-sys` の行に `eml_io` の feature を足す。
- roadmap: 「`compio-driver` の 0.13 の系列」を消す。

## 17. io_uring への当てはめ

この節は実装しない。`Backend` の境界が完了の形の io_uring でも成り立つことを確かめるための表である。

`user_data` は `OpKey` にし、doorbell と取り消しの SQE には予約の値 (`u64::MAX`、`u64::MAX - 1`) を使う。SQE は `start` と `cancel` で SQ に積み、次の `wait` の `io_uring_enter` でまとめて出す。

| `Backend` の要素 | io_uring の形 | 核を変えるか |
|---|---|---|
| `start` の `Readable` | `IORING_OP_POLL_ADD` (`POLLIN \| POLLRDHUP`、1回)。CQE の `res` は revents なので、backend が `Outcome::Size(0)` にそろえる。`Start::Pending` | 変えない |
| `start` の `Read` | `IORING_OP_RECV`。番地と長さは `Lent::room()` の先頭と `len()`。CQE の `res` が n なら `Size(n)`、負なら `Err(from_raw_os_error(-res))`。`cap == 0` は核が先に終える | 変えない |
| `start` の `Write` | `IORING_OP_SEND` (`MSG_NOSIGNAL`)、`Lent::bytes(written)`。短い送りは `Size(n)` で返し、核が残りを `start` し直す | 変えない |
| `start` の `Accept` | `IORING_OP_ACCEPT` (`SOCK_NONBLOCK \| SOCK_CLOEXEC`)。`sockaddr_storage` と `socklen_t` は `Slot` の箱に置く。CQE の fd から `Socket` を作り、箱の番地を `SocketAddr` にして `Accepted` にする | 変えない |
| `start` の `Connect` | `IORING_OP_CONNECT`。番地は `Slot` の箱に `SockAddr` を写して渡す。`res == 0` で `Connected` | 変えない |
| `cancel` | `IORING_OP_ASYNC_CANCEL` (`addr` = 対象の `user_data`、自分の `user_data` は予約の値)。CQE は2つ届く。取り消しの CQE は捨てる。本体の CQE が `-ECANCELED` なら `Finished { cancelled: true }`、ふつうの結果なら `cancelled: false`。終わりは `wait` で届く。SQ に積めなかった取り消しは覚えて次の `wait` の `Err` にする | 変えない |
| `wait` | `IORING_ENTER_GETEVENTS \| IORING_ENTER_EXT_ARG` の時間付きの `io_uring_enter`。eventfd の doorbell は複数回の `POLL_ADD` (予約の `user_data`) で登録し、その CQE は doorbell を `clear` して `Woken` にする。`IORING_CQE_F_MORE` がなければ張り直す。`-ETIME` と空の CQ は `TimedOut`、`-EINTR` も `TimedOut` | 変えない |
| `close` | trait に置かない。記録が `Rc<Socket>` を持つので、fd を閉じるのは最後の CQE を受けて記録を落とした後になる。保留の SQE が閉じた fd を指すことはない | 変えない |
| `Slot` | 操作ごとの箱 (`Box<SqeArgs>`)。`Accept` の `sockaddr_storage` と長さ、`Connect` の番地を置く。将来 `RECVMSG` と `SENDMSG` を使うなら、`msghdr` と `iovec` もここに置く。記録が表の中で動いても、箱の中身の番地は変わらない | 変えない |
| `KERNEL_WRITES_AFTER_CANCEL = true` の `Drop` | 核の `Drop` が I/O の鍵をすべて `cancel` し、`io_len() == 0` まで `wait` で CQE を取り出してから記録を放す。ring の fd を閉じるのはその後 (backend の欄が落ちるとき)。`enter` が失敗して壊れたときは、`drain_broken` が `ASYNC_CANCEL` を出し、合わせて 1 秒まで CQ を読み、終わらない操作が残れば abort してから ring を閉じる | 変えない |

backend を足すときは本体 D-2 の選び方に従う。`RealDriver::new` が `EML_IO_BACKEND` で固定でき、選んだ名前を返す関数を足す。io_uring の backend は `sys/unix/` に `process::abort` を持つので、そのときは `grep -rn "process::abort"` の基準を「各 backend の `drain_broken` の中だけ」に読み替える。核の形はどの行でも変わらない (机上の当てはめ。完了の形の道そのものは `Fake<true>` のテストで確かめる)。
