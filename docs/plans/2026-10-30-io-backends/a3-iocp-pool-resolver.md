# 付録 A3 (S3): IOCP の backend、プール、解決器、受け箱

> この付録は計画の本体 (`../2026-10-30-io-backends.md`) の一部である。本体の「5.3 方針の修正と食い違いの解き方」がこの付録より優先する。`[V]` は設計の時に scratch の試作や probe で確かめた事実を指す。試作と probe のファイル (`s1-probe/` などの名前で引いているもの) は残っていない。S1〜S4 は付録 A1〜A4 を指す。


[V] は確かめた事実 (方法を併記)、[I] は推論。試作は `s3-probe/` にある (r2 の形に更新済み)。S1 第1部の決定は変えない。プールの中身を S1 の文から直した点 (起こす条件と、`running` を外す順) は、この設計として採り、§6 に S1 の文の補いとして並べる。

# 第1部: 改訂した成果物 (全文)

## 1. 結論

### Q0. `sys/mod.rs` の共有の項目 (S3 が持つ。S2 はこれを使う)

```rust
#[cfg(target_os = "linux")]     pub(crate) type Os = unix::Unix<epoll::Epoll>;
#[cfg(target_vendor = "apple")] pub(crate) type Os = unix::Unix<kqueue::Kqueue>;
#[cfg(windows)]                 pub(crate) type Os = iocp::Iocp;
#[cfg(not(any(target_os = "linux", target_vendor = "apple", windows)))]
compile_error!("eml_io has no I/O backend for this OS");

/// `d` をミリ秒へ切り上げ、`max` で切る。切った待ちは早く戻るだけである (G-8)。
/// 期限のない待ち (`None`) は呼ぶ側が自分の OS の値 (epoll の -1、IOCP の `INFINITE`) に写し、ここへは渡さない
pub(crate) fn millis(d: Duration, max: u32) -> u32 {
    u32::try_from(d.as_nanos().div_ceil(1_000_000)).map_or(max, |ms| ms.min(max))
}

/// 機械の解決器。誤りも空の一覧も `NotFound` にする (今の `resolve.rs` の `std_lookup` をそのまま移す)
pub(crate) fn lookup(host: &str, port: u16) -> io::Result<Vec<SocketAddr>>;
```

- 呼び方: epoll は `timeout.map_or(-1, |d| millis(d, c_int::MAX as u32) as c_int)`、IOCP は `timeout.map_or(INFINITE, |d| millis(d, INFINITE - 1))`。kqueue は timespec を使うので呼ばなくてよい。
- 構築: どの backend も固有メソッド `pub(crate) fn new() -> io::Result<Self>` を持つ (S2 の `Unix<P>::new`、ここの `Iocp::new`)。`real.rs` は次の形にする:
  ```rust
  pub struct RealDriver(Driver<sys::Os>);
  impl RealDriver {
      pub fn new() -> io::Result<RealDriver> { RealDriver::with_lookup(sys::lookup) }
      /// 名前の解決に `lookup` を使う。テストと組み込み先が機械の解決器の代わりを差すためにあり、全 OS にある
      pub fn with_lookup(lookup: fn(&str, u16) -> io::Result<Vec<SocketAddr>>) -> io::Result<RealDriver> {
          Ok(RealDriver(Driver::new(sys::Os::new()?, lookup)))
      }
  }
  ```
  `Driver::new(backend: B, lookup: Lookup) -> Driver<B>` は失敗しない (受け箱は `backend.waker()` から作り、プールは `POOL_LIMIT` で作る)。
- `lookup` を `sys/` に置くので、核 (`driver/`) は差し込まれた関数しか呼ばない。G-1 を grep でなく構造で満たす。
- `millis` の単体テストは `sys/mod.rs` の `#[cfg(test)]` に置き、全 OS で走る: 0 → 0、1 ns → 1、1 ms → 1、1 ms + 1 ns → 2、`max` を超える値 → `max`、`u32::MAX` ms を超える値 → `max`、`Duration::MAX` → `max` [V: `s3-probe/src/sysmod.rs` で通る]。

### Q1. `src/sys/iocp.rs` (この段。`SOCKETS = false`)

```rust
#![allow(unsafe_code)]
//! Windows の backend。ソケットは `net-windows` で扱い、ここでは IOCP のポートで起こしと時間切れだけを待つ
//! (docs/spec/vm.md の「時計と driver」)。

/// 起こしの packet の completion key。後のソケットの packet は別の key を使う
const WAKE: usize = 0;
/// 1回の `GetQueuedCompletionStatusEx` で受け取る packet の数
const ENTRIES: usize = 64;
/// 起こしの packet を積めなかったとき、出し直すまでの間
const POST_RETRY: Duration = Duration::from_millis(1);

/// ソケットを作らないので値がない。`Sock` を受け取る関数が呼ばれえないことを型で示す
pub(crate) enum Sock {}

pub(crate) struct Iocp { waker: Arc<PortWaker>, epoch: Instant }

/// ポートを所有する起こし。driver が落ちた後も、ほかのスレッドが持つ写しがポートを生かす
struct PortWaker {
    port: OwnedHandle,       // std::os::windows::io::OwnedHandle。Send + Sync で、落とすと CloseHandle
    /// 起こしの packet がポートにあるか、積む途中か。鳴らしを1つの packet にまとめる
    posted: AtomicBool,
}
```

- `Iocp::new() -> io::Result<Iocp>`: `CreateIoCompletionPort(INVALID_HANDLE_VALUE, null_mut(), 0, 1)` を呼ぶ。失敗すると null が返るので、`OwnedHandle::try_from(HandleOrNull::from_raw_handle(raw)).map_err(|_| io::Error::last_os_error())` で受ける。`epoch = Instant::now()`。
- `waker()`: `Arc::clone(&self.waker) as Arc<dyn IoWaker>`。
- `PortWaker::wake`:
  ```rust
  if self.posted.swap(true, AcqRel) { return }   // packet があるか、ほかの鳴らしが積む途中
  loop {
      let ok = unsafe { PostQueuedCompletionStatus(self.port.as_raw_handle(), 0, WAKE, null()) };
      if ok != 0 { return }
      thread::sleep(POST_RETRY);                  // 積めるまで `posted` を立てたまま出し直す
  }
  ```
  `posted == true` の意味は「packet がポートにあるか、積めるまで出し直す鳴らしがいる」である。だから、`swap` が true を読んで何もしなかった鳴らしの分も、その packet が必ず届ける。`PostQueuedCompletionStatus` が失敗するのは非ページプールが尽きたときだけで [I]、そのときこの鳴らしは空くまで待つ。待つのは鳴らしたスレッド (プール、解決、監視) だけで、driver のスレッドは鳴らさない (Q5 の `put`)。
- `wait(timeout, _events)`:
  ```rust
  let mut entries = [OVERLAPPED_ENTRY::default(); ENTRIES];
  let mut removed = 0u32;
  let ok = unsafe { GetQueuedCompletionStatusEx(self.waker.port.as_raw_handle(), entries.as_mut_ptr(), ENTRIES as u32,
                                               &mut removed, timeout.map_or(INFINITE, |d| sys::millis(d, INFINITE - 1)), FALSE) };
  if ok == 0 {
      let e = io::Error::last_os_error();
      return if e.raw_os_error() == Some(WAIT_TIMEOUT as i32) { Ok(()) } else { Err(e) };
  }
  for entry in &entries[..removed as usize] {
      // この段でポートに来るのは起こしの packet だけである。ポートにハンドルを結び付けず、WAKE の key でしか積まないため
      debug_assert_eq!(entry.lpCompletionKey, WAKE);
      self.waker.posted.store(false, Release);
  }
  Ok(())
  ```
  取り出した entry はすべてループで扱い、取り残しはない。WAKE 以外の key はこの段では作られない。release で来ても `posted` を下ろすだけで、余分な起床が1回起きるのが最悪の害である。`net-windows` はこのループを key の `match` に替える。`events` には何も積まない。起こしも時間切れも `Ok(())` で、events は空のまま。
  - 起こしを失わないこと: `posted` を下ろすのは packet を取り出した後で、核は `wait` が戻ってから受け箱を引き取る (Q5 の (d))。packet を取り出してから下ろすまでの間に鳴らしが来て、`swap` が true を読み何もしなかったとする。そのときの順は「その鳴らしの `push` → `swap` → `store(false)` → (d) の引き取り」で、`swap` が true を読んだことから `swap` は `store(false)` より前にある。受け箱の `Mutex` がこの順を引き取りまで運ぶので、(d) はその答えを見る [I: この順序づけによる推論]。下ろした後の鳴らしは新しい packet を積み、次の `wait` がすぐ戻るだけで済む。
- `now()`: `self.epoch.elapsed()`。
- `spawn(builder, body)`: `crate::mask::spawn_masked(builder, body)` (Windows ではただの `spawn`。G-10 の字義どおり、全 backend で同じ関数を通す)。
- 呼ばれない関数は `unreachable!` でなく型で閉じる: `listen`、`socket` は `Err(io::ErrorKind::Unsupported.into())`。`shutdown_write`、`set_nodelay` は `match *sock {}`、`close` は `match sock {}`。`start`、`cancel` は `match step { Step::Peek { sock } | Step::Recv { sock, .. } | Step::Send { sock, .. } | Step::Accept { listener: sock } | Step::Connect { sock, .. } => match *sock {} }`。この段の IOCP は `Start` も `Event` も一切作らない。したがって `Pending` も `Hold::Buffer` も返さない (`Sock` の値がないので `start` に届かない)。
- 核の側に要る規則 (S1 の `SOCKETS` の説明への補い。§6-3): `!B::SOCKETS` のとき、核は `submit` の `Listen`/`Accept`/`Connect`、それに `shutdown_write`/`set_nodelay` を、handle を引く前に `Unsupported` で終える。`Readable`/`Read`/`Write` は handle を引いて `InvalidInput` で終える。根拠: `listen_accept_and_connect_are_unsupported` が `Accept { listener: 0 }` と `shutdown_write(0)` に `Unsupported` を期待し [V: `tests/real.rs:1357-1379`]、`an_op_on_an_unknown_handle_fails` は全 OS で `InvalidInput` を期待する [V]。
- ポートの寿命: `PortWaker` が `OwnedHandle` を持ち、`Iocp`、`Inbox`、VM の監視がそれぞれ `Arc` の写しを持つ。最後の `Arc` が落ちたときに `CloseHandle` が呼ばれる。driver が落ちた後の鳴らしは、生きたポートに packet を高々1つ積むだけである (G-11)。`Drop` は書かない。
- windows-sys: `crates/eml_io/Cargo.toml` に `[target.'cfg(windows)'.dependencies] windows-sys = { workspace = true, features = ["Win32_Foundation", "Win32_System_IO", "Win32_System_Threading"] }`。`INFINITE` は Threading に [V: `windows-sys-0.61.2/src/Windows/Win32/System/Threading/mod.rs:497`]、`CreateIoCompletionPort`、`GetQueuedCompletionStatusEx`、`PostQueuedCompletionStatus`、`OVERLAPPED_ENTRY` は `System/IO` に [V: `mod.rs:5,10,11,67`]、`FALSE`、`INVALID_HANDLE_VALUE`、`WAIT_TIMEOUT` は Foundation にある [V]。
- `unsafe` は4か所:
  1. `CreateIoCompletionPort`。SAFETY: 既存のハンドルを結び付けず新しいポートを作るだけで、渡すポインタはない。失敗は null で返る。
  2. `HandleOrNull::from_raw_handle(raw)`。SAFETY: `raw` は今作ったポートか null で、ほかに所有者がいない。
  3. `PostQueuedCompletionStatus`。SAFETY: `port` はこの物体が所有していて開いている。OVERLAPPED は null で、OS は読まずにそのまま返す。
  4. `GetQueuedCompletionStatusEx`。SAFETY: ポートは `waker` が所有していて開いている。`entries` は長さ `ENTRIES` の局所の配列、`removed` は局所変数で、OS が書くのは戻るまでの間だけ。`alertable = FALSE` なので APC は走らない。

[V: `s3-probe/src/iocp.rs` (r2 の形) が `nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --all-targets -- -D warnings` を通り、`unsafe {` は 4 つ。Windows での実行はしていない]

### Q2. `net-windows` に残す余地 (設計はしない)

- 行と記録: `Sock { socket: OwnedSocket, row: u32, gen: u32 }`。行の向きごとに `Option<Box<Record>>` を持ち、`Record { overlapped: OVERLAPPED (repr(C) の先頭), key, row, gen, AcceptEx の番地の buffer / ConnectEx の sockaddr }` とする。ソケットの packet は WAKE 以外の key にし、`lpOverlapped` から記録を引いて世代を確かめる (G-6)。Q1 の `wait` のループは key の `match` になる。
- 歩: `Peek`/`Recv` はブロックしない `recv` を先に試し、ブロックするなら長さ0の `WSARecv` を出して `Pending` にし、packet で `Arrival::Retry` を返す。`Send` は `WSASend` で `Pending(Hold::Buffer)` とし、`Arrival::Done(Ok(Size(n)))` を返す。`Accept` は `AcceptEx` を出し、packet の後に `SO_UPDATE_ACCEPT_CONTEXT` を設定し、行を作ってポートに結び、`Done(Ok(Accepted))` を返す。`Connect` は bind してから `ConnectEx` を出し、`SO_UPDATE_CONNECT_CONTEXT` の後に `Done(Connected)` を返す。取り消しは `CancelIoEx` で `Cancel::Pending` を返し、`ERROR_OPERATION_ABORTED` を `Arrival::Cancelled` に写す。`ERROR_NOT_FOUND` は packet が既に積まれた印なので、`Err` にせず `Cancel::Pending` を返す。
- 結論: `Start`、`Cancel`、`Arrival`、`Event` は形を変えずに載る。ただし `Hold` は2値のままでは足りない可能性がある。backend が所有する OVERLAPPED には、完了の packet までカーネルが書くからである [I]。この問いは §6-6 に `net-windows` 向けの未決として書き、この段では何も変えない。

### Q3. プール (`src/driver/pool.rs`。OS を呼ばない)

```rust
pub(crate) const POOL_LIMIT: usize = 32;
pub(crate) const POOL_IDLE: Duration = Duration::from_secs(10);
pub(crate) type Job = Box<dyn FnOnce() -> io::Result<usize> + Send + 'static>;   // `Op::Job` の中身そのまま

struct Shared { state: Mutex<State>, cv: Condvar, inbox: Arc<Inbox> }
#[derive(Default)]
struct State {
    queue: VecDeque<(Ticket, Job)>,
    threads: usize,              // 起こした数 (始まる前のものも含む) − 抜けた数
    idle: usize,                 // `idle += 1` してから `idle -= 1` するまでのスレッド (cv で待つか、起こされて lock を待つ)
    running: HashSet<ThreadId>,  // 仕事を取ってから、答えを積む直前までのスレッド
    closed: bool,
}
pub(crate) struct Pool { shared: Arc<Shared>, handles: Vec<JoinHandle<()>>, limit: usize }
impl Pool {
    /// 核は `POOL_LIMIT` を渡す。プールの単体テストは小さな上限で上限の振る舞いを確かめる
    pub(crate) fn new(inbox: Arc<Inbox>, limit: usize) -> Pool;
    /// 止まらない。完了はいつも受け箱で届く
    pub(crate) fn submit<B: Backend>(&mut self, backend: &B, ticket: Ticket, job: Job);
}
```

- `submit` の手順:
  1. lock の下で `queue.push_back((ticket, job))` し、`spawn = queue.len() > idle && threads < limit` を求める (積んだ後の長さで比べる)。`spawn` なら `threads += 1`。
  2. unlock して `cv.notify_one()`。`spawn` でなければここで戻る。
  3. `handles` から `is_finished()` のものを `swap_remove` して `join` する (空き時間で抜けたスレッドの片付け。待ちは生じない)。
  4. `backend.spawn(Builder::new().name("eml-pool".into()), Box::new(move || work(&shared)))` を呼ぶ (スタックは既定)。`Ok(h)` なら `handles.push(h)`。
  5. `Err(e)` なら lock の下で `threads -= 1` とし、自分の札がまだ列にあれば取り除いて unlock し、`inbox.put(ticket, Answer::Job(Ok(Err(e))))` を積む (driver のスレッドなので鳴らさない)。ほかのスレッドが既に取っていれば何もしない。どちらの道でも完了は1回になる。
- 核の `submit(Op::Job)` は、壊れていなければいつも `Pending` を返す。失敗の答えも受け箱を通り、同じ `turn` の (a) で `done` に入る。`(a)` の後に `done` が空でなければ timeout は 0 になる (Q5) ので、取りこぼしはない。
- `submit` が止まらない理由: lock の下では列の操作と数の更新しかせず、利用者のコードも syscall も走らない (下のスレッドの形)。`backend.spawn` はスレッドを作る syscall で、ほかのスレッドを待たない。
- スレッド (`fn work(shared: &Shared)`):
  ```rust
  let me = thread::current().id();
  let mut state = shared.lock();                  // PoisonError は into_inner で受ける
  loop {
      if state.closed { break }
      let Some((ticket, job)) = state.queue.pop_front() else {
          state.idle += 1;
          let (next, waited) = shared.cv.wait_timeout(state, POOL_IDLE).unwrap_or_else(PoisonError::into_inner);
          state = next;
          state.idle -= 1;
          if waited.timed_out() && state.queue.is_empty() { break }
          continue;
      };
      state.running.insert(me);
      drop(state);
      let answer = panic::catch_unwind(AssertUnwindSafe(job));
      // 答えを積む前に走っている印を外す。答えを見た driver が落ちても、このスレッドは join される側にいる。
      // 積む (受け箱の lock と起こしの syscall) のはプールの lock の外で行う
      shared.lock().running.remove(&me);
      shared.inbox.push(ticket, Answer::Job(answer));
      state = shared.lock();
  }
  state.threads -= 1;
  ```
  `running` から外れて、まだ積んでいないスレッドを `Drop` が join することはありうる。それでも `push` はブロックしない (起こしは syscall 1回。IOCP で積めないときの出し直しだけが例外) ので、join の待ちは短い。lock を2つ同時に持つ所はない。
- 起こしを取りこぼさないこと (`queue.len() > idle`):
  - 列を見に来るスレッドを `L = 起こされたか時間切れで起きた idle のスレッド + 始まる前のスレッド + running のスレッド` と数える。不変条件は「`queue.len() ≤ L` か、`threads == limit`」である。
  - spawn しない `submit` は `queue.len() ≤ idle` のときに起きる。cv で待つスレッドが1本でもいれば、`notify_one` がそのどれかを起こし、L が1増える。1本もいなければ、idle のスレッドはすべて既に起きていて L に入っているので、`L ≥ idle ≥ queue.len()` が成り立つ。spawn する `submit` は、始まる前のスレッドで L が1増える。
  - スレッドは lock の下で列を見て、仕事を取るか (`queue.len()` が1減る)、待ちに戻る (列は空) か、列が空のときだけ抜ける。どの動きも不変条件を保つ。
  - S1 の条件 `idle == 0` では、起こされてまだ lock を取り直していないスレッドが idle に数えられたままになる。そのため2つ目の仕事が spawn も起こしもされず、1本のスレッドに並ぶ (§6-1 に証拠)。
- panic: 仕事は `catch_unwind` の中で走り、payload は `Answer::Job(Err(payload))` で届く。核は最初の payload を `panic` の欄に取っておき、受け箱の束は最後まで処理する (Q5)。
- `Drop` (プール):
  ```rust
  let mut state = self.shared.lock();
  state.closed = true;
  let queued = mem::take(&mut state.queue);              // 始まっていない仕事は走らせずに落とす
  let (running, idle): (Vec<_>, Vec<_>) = mem::take(&mut self.handles)
      .into_iter().partition(|h| state.running.contains(&h.thread().id()));
  drop(state);
  self.shared.cv.notify_all();
  drop(queued);                                          // 利用者の Drop は lock の外で走る
  drop(running);                                         // 切り離す
  for h in idle { let _ = h.join(); }                    // closed を見てすぐ抜ける
  ```
  切り離したスレッドは `Arc<Shared>` と `Arc<Inbox>` (その中の waker) を持ち、答えを積んで終わる。`mem::forget` と `Box::leak` は使わない。`mem::take` は `pool.rs` にあるが、G-8 の grep は `driver/mod.rs` だけを見る。

### Q4. 解決器 (`src/driver/resolve.rs`。今の `src/resolve.rs` を移す)

```rust
pub(crate) type Lookup = fn(&str, u16) -> io::Result<Vec<SocketAddr>>;
pub(crate) const RESOLVE_LIMIT: usize = 8;
pub(crate) const ABANDONED_LIMIT: usize = 64;
const STACK: usize = 256 * 1024;

pub(crate) struct Resolver {
    lookup: Lookup,
    inbox: Arc<Inbox>,
    live: HashMap<Ticket, JoinHandle<()>>,       // 走っていて手放していない
    queue: VecDeque<(Ticket, String, u16)>,      // 始めていない。受け付けの順
    abandoned: HashMap<Ticket, JoinHandle<()>>,  // 手放して、まだ答えを引き取っていない
}
impl Resolver {
    pub(crate) fn new(lookup: Lookup, inbox: Arc<Inbox>) -> Resolver;
    pub(crate) fn submit<B: Backend>(&mut self, backend: &B, ticket: Ticket, host: String, port: u16) -> io::Result<()>;
    pub(crate) fn answered<B: Backend>(&mut self, backend: &B, ticket: Ticket);
    pub(crate) fn cancel<B: Backend>(&mut self, backend: &B, ticket: Ticket);
    pub(crate) fn abandon_all(&mut self);
}
```

- `submit`: まず `for (_, h) in self.abandoned.extract_if(|_, h| h.is_finished()) { let _ = h.join(); }` で、答えを積んで終わったスレッドを数えから外す (受け箱を引き取る前でもよい)。残りが `ABANDONED_LIMIT` 以上なら `Err(io::Error::other("too many name lookups are stuck"))` を返す。そうでなければ列に積んで `fill`。
- `fill`: `live.len() < RESOLVE_LIMIT` の間、列から取って `backend.spawn(Builder::new().name("eml-resolve".into()).stack_size(STACK), Box::new(move || inbox.push(ticket, Answer::Lookup(lookup(&host, port)))))` を呼ぶ。`Ok(h)` なら `live.insert(ticket, h)`。`Err(e)` なら `inbox.put(ticket, Answer::Lookup(Err(e)))` を積む (driver のスレッドなので鳴らさない)。
- `answered`: `live.remove(t)` があれば `join` して `fill` し、`abandoned.remove(t)` があれば `join` する。どちらにもなければ何もしない (`spawn` の失敗の答えか、`submit` の掃除で既に外れたもの)。
- `cancel`: `live` にあれば `abandoned` へ移して `fill` し、列にあれば取り除く。核は札と欄を外し、`Completion { cancelled: true, result: Err(io::Error::other("cancelled")) }` を積む。
- `abandon_all` (`quiesce`): `queue.clear(); abandoned.extend(live.drain())`。核は `Lookup` の欄と札を完了なしで外す。
- 期限は VM の `cancel(key)` を通り、Ctrl-C は `quiesce` を通る。解決器に別の道はない。
- `Drop` は書かない。`JoinHandle` が落ちて切り離される。止まった解決のスレッドは残るが、これは意図どおりである。
- 答えから接続の歩へ: Q5 の `receive` が `Answer::Lookup(r)` の札を鍵に当てたら、`Err(e)` はそのまま完了にする。`Ok(addrs)` なら欄を `Kind::Connect` にして `connect_next(key, addrs.into_iter(), last = NotFound("no address"))` を呼ぶ。`connect_next` は次を繰り返す: 次の番地がなければ `last` で完了する。`backend.socket(peer)` が `Err(e)` なら `last = e` で次の番地へ進む。`start(key, Step::Connect { sock: &mut sock, peer })` の結果が `Done(Ok(Connected))` なら handle を作って `Done::Stream { handle, peer }` で完了し、`Done(Err(e))` なら `backend.close(sock)` して `last = e` で次の番地へ、`Pending(h)` なら欄を `Connect { sock, peer, rest }` にして戻る。後の `Done(Err)` も同じ `connect_next` に入り、取り消された欄は次を試さない。

### Q5. 受け箱 (`src/driver/inbox.rs`) と `turn`

```rust
pub(crate) type Ticket = u64;
pub(crate) enum Answer {
    /// `Err` は仕事の panic の payload
    Job(thread::Result<io::Result<usize>>),
    Lookup(io::Result<Vec<SocketAddr>>),
}
pub(crate) struct Inbox { arrived: Mutex<Vec<(Ticket, Answer)>>, waker: Arc<dyn IoWaker> }
impl Inbox {
    pub(crate) fn new(waker: Arc<dyn IoWaker>) -> Arc<Inbox>;
    /// ほかのスレッドから。積んでから鳴らす (G-11)
    pub(crate) fn push(&self, ticket: Ticket, answer: Answer);
    /// driver のスレッドから (`spawn` の失敗)。鳴らさない。次の `turn` の (a) が拾う
    pub(crate) fn put(&self, ticket: Ticket, answer: Answer);
    /// 着いた答えを `out` の後ろへ移す。受け箱の `Vec` の容量は残る
    pub(crate) fn take(&self, out: &mut VecDeque<(Ticket, Answer)>) { out.extend(lock().drain(..)); }
}
```

- 核の表に足す欄 (S1 の `Driver` にないもの): `next_ticket: Ticket` (1 から数え、使い回さない)、`arrived: VecDeque<(Ticket, Answer)>` (使い回す)、`events: Vec<Event<B::Sock>>` (使い回す)、`panic: Option<Box<dyn Any + Send>>`。
- `receive(&mut self)` は答えを引き取る唯一の関数で、`turn` の (a) と (d) から呼ぶ。`turn` を使う `wait`、`quiesce`、`Drop` の `Hold::Buffer` の待ちのどれもこれを通る:
  ```rust
  self.inbox.take(&mut self.arrived);
  while let Some((ticket, answer)) = self.arrived.pop_front() {
      if let Answer::Lookup(_) = &answer { self.resolver.answered(&self.backend, ticket); }   // 札より先に join する
      let Some(key) = self.tickets.remove(&ticket) else { continue };                       // 外した札の答えは捨てる (G-9)
      match answer {
          Answer::Job(Ok(r)) => { 欄を外し、`done` に `r.map(Done::Size)` を積む }
          Answer::Job(Err(p)) => { 欄を外し、完了は積まない; self.panic.get_or_insert(p); }
          Answer::Lookup(r) => { Q4 の接続の歩へ (`&mut self` のメソッドを呼んでよい) }
      }
  }
  ```
  `pop_front` は1回ごとに借用を返すので、ループの中で `&mut self` のメソッドを呼べる。`VecDeque` も受け箱の `Vec` も容量を保つので、`mem::take` は要らない (G-8)。[V: `s3-probe/src/tests.rs` の `Mini::drain` がこの形で build でき、`answer(&mut self, ..)` を呼んで通る]
- `turn(timeout)`:
  - (a) `receive()`。`done` が空でなければ `timeout = Some(ZERO)` にする。
  - (b) `debug_assert!(self.events.is_empty()); self.backend.wait(timeout, &mut self.events)`。核はいつも空の `events` を渡す (S2 の要望)。
  - (c) `while let Some(event) = self.events.pop() { … }` で event を処理しきる。1つの束の中の順は意味を持たない。こうすると次の (b) では `events` が必ず空になる。
  - (d) `receive()`。(b) が消費した起こしの答えをここで拾う。
- panic の起こし直し: `receive` は束を最後まで処理し、最初の payload だけを `self.panic` に残す (2つ目以降は落とす)。束の途中で起こし直すと、残りの答えが失われて欄が永遠に残る。`wait` は `turn` の後に `self.panic.take()` があれば `resume_unwind` する。`quiesce` は仕事を待ちきるループを終えてから、同じく `resume_unwind` する (仕事を待ちきる規則を保つ。`quiesce` は `catch_unwind` の下にない [V: `eml_vm/src/lib.rs:427,778`、`vm.rs:1762` に `catch_unwind` がない] ので、panic は run の外へ抜ける。これは仕事の panic が driver の不具合であることに合う)。`Drop` は `panic` を落とすだけで起こし直さない。
- driver が落ちた後の鳴らし: 受け箱は `Arc` で、切り離したプールのスレッドと解決のスレッドが持つ。積んだ答えは最後の `Arc` と一緒に、そのスレッドの上で落ちる。waker は自分の OS の物体を所有するので、生きた物体に書くだけで済む。この段の `Drop` は `Hold::Buffer` の欄を持たないので `turn` を呼ばず、`receive` も呼ばれない。

### Q6. テスト

成否の変更 (S1 の決定。S4 が一覧に入れる): `tests/real.rs` の `dropping_a_driver_waits_for_a_running_job` を削り、`dropping_a_driver_does_not_wait_for_a_running_job` を足す (全 OS)。形: 仕事は `started.send(())` の後に `gate.recv()` で止まり、最後に `end.send(())` を送る。テストは `started.recv()`、`drop(driver)` (ここへ戻れたことが証拠で、時間は測らない)、`gate_tx.send(())`、`end.recv()` の順に進める。driver を落とした後の積みと鳴らしもこの道で通る。Miri の対象外。

核の単体テスト (`src/driver/tests/`。Miri の対象。偽の backend の上で動かす):

| 名前 | 確かめること | 形 |
|---|---|---|
| `pool::a_job_beyond_the_limit_does_not_block_submit` | G-10 (上限、列で待つ、止まらない) | `Pool::new(inbox, 2)` に、`started.send(i)` してから自分の門で止まる仕事を3つ出す。2つの `recv` の後に `try_recv` が空、1つ開けると3つ目が始まる、全部開けて答えが3つ届く。答えは `inbox.take` と偽の waker の待ちで受け、最後に `Pool` を落とす |
| `pool::two_jobs_submitted_to_one_idle_thread_run_together` | 起こす条件 (§6-1) | 上限 `POOL_LIMIT` の driver で仕事0を完了させ、`thread::sleep(5 ms)` で空きのスレッドを作る (検出力のためだけ)。A は B の開始を `recv` で待ち、B はそれを送る。2つとも完了する |
| `a_job_panic_is_raised_on_the_driver_thread` | panic の起こし直しと、束のほかの答え | `panic!("boom")` と `Ok(7)` の仕事を出し、`catch_unwind(|| driver.wait(None))` の payload が `"boom"` であること。その後 `Ok(7)` の完了が届き `pending() == 0` |
| `a_job_whose_thread_cannot_start_completes_once_with_an_error` | `spawn` の失敗 | 偽の `spawn` が `Err(other("no threads"))` を返す。`Pending` → `wait(None)` → 完了が1回 (`"no threads"`)。次の `wait(None)` は `Ok(false)` |
| `a_late_answer_of_a_cancelled_lookup_does_not_hit_the_reused_key` | G-9 | 門で止まる解決を鍵 5 で出して `cancel`、鍵 5 で門のない解決を出して完了させる。門を開け、偽の backend の `finished` が 2 になるまで待つ。その後 `wait(Some(0))` (期限ありなので `Ok(false)` の近道を通らず `turn` が走り、(a) が遅れた答えを引き取る) を呼び、`completions` が空で `pending() == 0` であること |
| `quiesce_abandons_a_lookup_and_drops_its_late_answer` | 解決の手放し | `quiesce` の後 `pending() == 0`。門を開け、`finished` を待ってから `wait(Some(0))`、完了が届かないこと |
| `a_ninth_lookup_starts_when_an_answer_is_taken` | `RESOLVE_LIMIT` | 8 つが門で止まり、`started` の数が 8 のまま。1つ開けて、その本体の `finished` を待ってから `wait(Some(0))` を呼ぶと、9 本目が始まる。最後に全部を完了させる |
| `sockets_off_rejects_before_looking_up_the_handle` | Q1 の核の規則 | `SOCKETS = false` の偽の backend で、`Listen`、`Accept { listener: 0 }`、名前と IP の `Connect` が `Done(Err(Unsupported))`、`shutdown_write(0)` と `set_nodelay(0, true)` が `Unsupported`、`Readable { handle: 0 }` が `InvalidInput`。偽の backend のどの関数も呼ばれない |

偽の backend に要る部分 (S4 がまとめる): waker は `Bell { rung: Mutex<bool>, cv: Condvar }`。`wait(None)` は鳴るまで `cv.wait` し、`wait(Some(d))` は偽の時計を `d` 進め、鳴っていれば印を下ろしてすぐ戻る。`spawn` は `builder.spawn` で本体を包み、終わったら `finished: Arc<(Mutex<usize>, Condvar)>` を1つ増やす。テストの指示で `Err` を返せる。

Miri で安全に流すための形 (S4 が `testing.md` の「Miri」に書く):
1. 出した仕事と解決 (手放したものも含む) は、本体が答えを積み、核が引き取るまで進めてから driver を落とす。手放した解決は、`finished` を待ってから `wait(Some(0))` を呼ぶ (`wait(None)` は欄がないと `turn` を通らない)。
2. 順は門 (`mpsc`、`Mutex`+`Condvar`) で作り、`sleep` は合否に効かない検出力のためだけに使う。
3. 本物の時計を使わない。
4. 切り離したスレッドを使うテストは `tests/real.rs` に置き、Miri では流さない。
5. 上限の振る舞いはプールの単体テストで上限 2 で確かめ、32 本のスレッドは Miri で起こさない。32 は本物の `a_job_pool_is_capped` が確かめる。試作の6件は Stacked で user 3.6 s、Tree で 2.8 s、`-Zmiri-seed=1..4 -Zmiri-preemption-rate=0.1` でも通る [V]。

本物の backend のテスト (`tests/real.rs`。全 OS):
- `a_wake_after_the_driver_drops_is_harmless`: `let w = driver.waker(); drop(driver);` の後、別のスレッドから `w.wake()` を2回呼び、join する。
- `wakes_before_a_wait_end_only_that_wait`: `wake()` を 100 回呼んだ後、`wait(Some(u64::MAX))` が `Ok(true)`。続く `deadline = now + 100 ms` の `wait` を1回だけ呼び、経過が 75 ms 以上であること (下限だけを見る。G-11)。
- r1 の `a_wake_ends_a_wait_with_the_largest_finite_deadline` は削る (切り方を確かめられない。`millis` の単体テストが受け持つ)。
- Windows だけのテストは足さない。`listen_accept_and_connect_are_unsupported` は変えずに通す。IOCP の起こし、時間切れ、プールの道は、全 OS のテストと今の待ちのテストが `windows-2025` で確かめる。

移すテスト: 今の `#[cfg(unix)]` の解決のテスト6件 (`a_connect_by_name_tries_the_looked_up_addresses_in_order`、`a_cancelled_lookup_completes_at_once`、`quiesce_does_not_wait_for_a_lookup`、`live_lookups_beyond_the_limit_wait_in_order`、`abandoned_lookups_do_not_hold_the_limit`、`a_lookup_fails_while_64_abandoned_lookups_are_stuck`) と `pool_threads_block_interrupt_and_terminate` は、`tests/real.rs` に unix のまま残し、変えずに通す (機械的な追随)。

## 2. 根拠

- `Sock` を値のない型にする: 「呼ばれない」を実行時の約束でなく型で示せる。誤って呼んでも `Unsupported` か、コンパイルが通らないかのどちらかになる。
- 起こしをまとめる `posted` と出し直し: packet が数えられて積もるのを防ぎ (compio も同じ。R3)、eventfd や `EV_CLEAR` と同じ「印が1つ残る」意味になる。出し直しで「`posted == true` なら packet が必ず届く」が成り立つので、ほかの鳴らしの分も失われない。
- 失敗の答えを `put` で積み、鳴らさない: 核に別の完了の道がなく、driver のスレッドが自分のポートを鳴らす (そして出し直しで止まりうる) 道もない。
- `running` を外してから lock の外で積む: 答えを見た driver が落ちても、そのスレッドは join され、Miri の終わりが決まる。syscall をプールの lock の下に置かない。
- `Pool::new(inbox, limit)`: 上限の振る舞いを Miri で安く確かめるため。核は `POOL_LIMIT` しか渡さない。`cfg!(miri)` で大きさを変えないという規則 (`testing.md`) にも反しない。
- `VecDeque` の `pop_front`: 借用と G-8 の両方を満たす、いちばん短い形。

## 3. 退けた案

- `unreachable!()` で閉じる: 型で閉じられる。
- 鳴らしごとに packet を積む: packet が積もる。
- `Post` が失敗したら `posted` を下ろす (r1): 同時に来たほかの鳴らしの分が失われる (C-1)。`Post` の失敗を `wait` の `Err` にして伝える: 鳴らす側から driver へ誤りを渡す道がない。
- `kind() == TimedOut` で時間切れを見分ける: std は `ERROR_OPERATION_ABORTED` なども `TimedOut` に写す [V: std 1.99.1 の `sys/io/error/windows.rs:33-48`]。
- `idle == 0` で spawn する (S1 の文): §6-1。
- 空きのスレッドを起こすのに `notify_all` を使う: 証明は易しいが、仕事のたびに空きの全員を起こす。上の不変条件で `notify_one` が足りる。
- 33 本の仕事を Miri で流す (r1): Tree Borrows で 6.7 s かかり、予算に余裕がない。
- `mem::take(&mut self.arrived)` で戻す: G-8 の grep に当たる。`pop()` で後ろから取る: 解決の順が逆になる。
- `quiesce` でその場で起こし直す: 残りの仕事を待ちきらずに抜ける。
- `std_lookup` を `driver/` に残す: 核が OS を呼ぶ。

## 4. 置いた仮定

- `GetQueuedCompletionStatusEx` は時間切れで `FALSE` を返し、誤りは `WAIT_TIMEOUT` (258) になる [I]。偽なら、今の Windows の待ちのテスト (`a_timed_wait_returns_at_or_after_its_deadline` など) が CI で落ちる。直し方は、`removed == 0` かつ `WAIT_TIMEOUT` か `ERROR_TIMEOUT` (1460) なら `Ok(())` とすること。
- `PostQueuedCompletionStatus` は非ページプールが尽きたときだけ失敗する [I]。尽きたまま戻らなければ、鳴らしたスレッドが 1 ms ごとに出し直し続ける。
- Windows の待ちは時計の刻みのため少し早く戻りうる [I]。契約はそれを許し、テストは下限に 25% の余裕を持つ。
- 本物の核でも Miri の1件は試作と同じ桁に収まる [I]。32 本のスレッドを起こさないので、余裕は大きい。

## 5. 実装への含意

1. **`driver/inbox.rs`、`driver/pool.rs`、`driver/resolve.rs`、偽の backend の waker と `spawn`、Q6 の核の単体テスト** (S1 の作業1の一部)。
   - 完了の基準: `cargo nextest run -p eml_io --lib driver::tests::` が全件通る。`nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_io --lib -- driver::tests::` と `-Zmiri-tree-borrows` を足したものが通り、`the main thread terminated without waiting` が出ない。`grep -rn "\.spawn(" crates/eml_io/src/driver --exclude-dir=tests` が `backend.spawn(` の2行 (pool.rs と resolve.rs) だけ。`grep -rn "ToSocketAddrs\|to_socket_addrs" crates/eml_io/src/driver` が空。`grep -n "mem::take" crates/eml_io/src/driver/mod.rs` に `receive` と `turn` の行がない。
2. **`sys/mod.rs` (`Os`、`millis`、`lookup` とそのテスト)、`sys/iocp.rs`、`real.rs` の `new`/`with_lookup`** (S1 の作業2の差し替えの commit。S2 と同じ commit)。
   - 完了の基準: `nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc -p eml_io --all-targets -- -D warnings` が通る。`cargo nextest run -p eml_io --lib sys::` で `millis` のテストが通る。CI の `windows-2025` の `test` が緑。`grep -c "unsafe {" crates/eml_io/src/sys/iocp.rs` が 4。`grep -n "Arrival::\|Start::\|Hold::\|Cancel::Pending\|unreachable!" crates/eml_io/src/sys/iocp.rs` が空。
3. **`tests/real.rs` の変更** (成否の変更1件、新しい2件、`a_job_pool_is_capped` の説明の文言)。S4 の一覧に従う。
   - 完了の基準: `cargo nextest run -p eml_io --test integration` が全件通り、`cargo nextest run -j 1 -p eml_io` で `SLOW` が出ない。

実装者に任せること: `ENTRIES` の値、プールのスレッド名、偽の backend の細部。

## 6. 未決の項目と S1 の文への補い

1. **補い (S1 Q3 の「`idle == 0 && threads < POOL_LIMIT` なら起こす」)**: 設計としては `queue.len() > idle && threads < limit` を採る。証拠: 試作で条件を S1 の形にすると、`two_jobs_submitted_to_one_idle_thread_run_together` がふつうの `cargo test` で止まり、60 s の `timeout` で切れた [V]。今の形では5回とも 0.1 s で通る [V]。正しさの論証は Q3 の不変条件。
2. **補い (S1 Q3 のスレッドの順「積む → 鳴らす → lock → `running.remove`」)**: 設計としては「lock → `running.remove` → unlock → 積む (鳴らす) → lock」を採る。答えを受け取った driver をすぐ落とすと、旧い順では答えたスレッドが切り離され、Miri が誤りにしうる。Miri が残ったスレッドを誤りにすることは確かめた [V: `leak_probe`]。この競合そのものは推論 [I]。
3. **補い (S1 の `SOCKETS` の説明)**: `!SOCKETS` のときは `shutdown_write`/`set_nodelay` も `Unsupported` を返し、どれも handle を引く前に判断する (Q1)。テスト `sockets_off_rejects_before_looking_up_the_handle`。
4. **補い (S1 Q2 の `turn`)**: 受け箱の引き取りは `receive` 1つで、束を処理しきる。仕事の panic は `wait` の終わりか `quiesce` の終わりで起こし直し、`Drop` では起こし直さない。核は `events` を空にして `backend.wait` に渡す。核の欄に `next_ticket`、`arrived`、`events`、`panic` を足す。
5. **補い (S1 Q3 のプールの構築)**: `Pool::new(inbox, limit)`。核は `POOL_LIMIT` を渡す。
6. **未決 (`net-windows` 向け。S1 Q4 の「長さ0の `WSARecv` は `Hold::None`」への異議)**: 重なった I/O は、取り消しても完了の packet まで backend の OVERLAPPED にカーネルが書く [I: Win32 の重なった I/O の規則]。したがって IOCP の保留はどれも、backend の記憶をカーネルに預けている。選べる道は3つある:
   - (a) IOCP の `Pending` をすべて `Hold::Buffer` にする。`Drop` は空きの読みを含むすべての保留に `CancelIoEx` を出し、packet を 1 s まで待つ。ふつうは取り消しの packet がすぐ来るので待ちは短く、1 s で来なければ abort になる。
   - (b) 3つ目の値 (例: `Hold::Record`、backend の記憶だけを預けている) を足し、核の `Drop` が `Buffer` と同じく待つ。
   - (c) IOCP の backend が自分の `Drop` で記録を待つ (待ちの規則が2か所に分かれる)。
   既定: `net-windows` の計画で決める。この段の型、コード、IOCP の振る舞いは変えない (この段の IOCP は `Pending` を返さない)。
7. `POOL_IDLE` (10 s) で空いたスレッドが抜けることは、ふだんのテストで確かめない (S1 と同じ)。

## 7. 追加で調べたこと

- `s3-probe/` (r2 の形: `Pool::new(inbox, limit)`、lock の外で積む、`Inbox::put`、`VecDeque` の `pop_front`、`sysmod::millis` とそのテスト、出し直す waker):
  - `cargo test --offline`: 6件が通る [V]。
  - Miri: Stacked で user 3.6 s、Tree Borrows で 2.8 s、6件とも通る。`-Zmiri-seed=1..4 -Zmiri-preemption-rate=0.1` でも通る [V]。
  - `nix develop ../repo-s3#cross -c cargo clippy --target x86_64-pc-windows-msvc --all-targets -- -D warnings`: 警告なし。`iocp.rs` の `unsafe {` は 4 [V]。
  - r1 での確認: S1 の起こす条件で止まること、Miri が残ったスレッドを誤りにすること、`&mut Sock` の上の `match *sock {}` が通ること [V]。
- windows-sys 0.61.2 のソース (`cargo fetch --target x86_64-pc-windows-msvc` で取得) で、関数、定数、feature を確かめた [V]。
- std 1.99.1 の `HandleOrNull`/`OwnedHandle` の安全の条件と、`TimedOut` に写す誤りの範囲を確かめた [V]。
- `quiesce` の呼び出し元 `eml_vm/src/lib.rs:427,778` と `vm.rs:1762` に `catch_unwind` がないこと [V: grep]。
- S2 r1 の §1 (構築と `Os`) と、§6 の要望 (固有の `new`、空の `events`) を読んだ。
- 日本語の文は natural-japanese の規則に沿うよう心がけたが、この環境では skill を呼び出せなかった。

## 8. 適合の自己確認

- G-1: 適合。核は `std::sync`、`JoinHandle`、`backend.spawn(` だけを使う。`Instant`、`ToSocketAddrs`、windows-sys は `sys/` にある。
- G-2: 適合。IOCP の `start` は型で閉じている。解決の答えからの接続は核の `connect_next` が出す。
- G-3: 適合。IOCP は `Backend` をそのまま実装し、この段では `Start`、`Event`、`Hold`、`Cancel` を作らない。
- G-4、G-5、G-7、G-14: 対象外 (この段の IOCP にはソケットがない)。
- G-6: 適合。OS に渡す識別子は WAKE の key だけで、`OpKey` も handle も渡さない。
- G-8: 適合。`millis` は切り上げ、`max` で切る。`receive` は `VecDeque::pop_front`、`turn` は `events.pop()` で、どちらも `mem::take` を使わない。`mem::take` は `pool.rs` の `Drop` だけ。
- G-9: 適合。札は連番で、`tickets` にない答えは捨てる。テストは `wait(Some(0))` で `turn` を必ず通す。
- G-10: 適合。上限、列で待つ、止まらない `submit`、`catch_unwind` → `resume_unwind`、`spawn` の失敗は1回、`backend.spawn` (本物は `spawn_masked`)、`Drop` は走っていないものを join し、走っているものを切り離す。
- G-11: 適合。`PortWaker` が `OwnedHandle` を所有する。packet は次の `wait` が消費するまで残り、出し直しで失われない。`push` の後に `wake`。
- G-12: 適合。`WAIT_TIMEOUT` でない失敗は `Err` で、核が壊れの扱いにする。
- G-13: 適合。`mem::forget` と `Box::leak` はない。プールと解決器は落とすときに走るスレッドを待たない。
- G-15: 適合。`iocp.rs` は `#![allow(unsafe_code)]` で始まり、`unsafe` は4か所。`RawBuf` に触れない。
- G-16: 適合。`raw_os_error` は `sys/` だけにあり、カーネルに渡す backend の記憶はない (起こしの OVERLAPPED は null)。
- G-17: 適合。Miri の形 (Q6 の1〜5) と試作の時間。切り離すテストは `tests/real.rs` で、終わりの合図を待つ。
- G-18: 適合。windows-sys の feature は3つで、compio と socket2 は使わない。
- G-19: 適合。コメントは `docs/spec/vm.md` の「時計と driver」を引く。

