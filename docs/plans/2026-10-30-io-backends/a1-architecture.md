# 付録 A1 (S1): 自前の I/O driver の長期の構成と指針

> この付録は計画の本体 (`../2026-10-30-io-backends.md`) の一部である。本体の「5.3 方針の修正と食い違いの解き方」がこの付録より優先する。`[V]` は設計の時に scratch の試作や probe で確かめた事実を指す。試作と probe のファイル (`s1-probe/` などの名前で引いているもの) は残っていない。S1〜S4 は付録 A1〜A4 を指す。


対象: `io-backends` 段とその後 (`net-windows`、`io-uring`、`async-files`、`async-threads`、`ffi-blocking`)。前提 (roadmap の項、`IoDriver` の trait は変えてよい、driver は作ったスレッドでだけ使う) は所与とする。
[V] は確かめた事実 (方法を併記)、[I] は推論。r1 からの変更は review の所見に沿う (末尾の表)。

# 第1部: 改訂した成果物 (全文)

## 1. 結論

### Q1. 核と backend の境界

- 核 (`Driver<B: Backend>`、`src/driver/`) が持つもの: `OpKey` で引く保留の表 `slots`、届いた完了 `done`、`IoHandle` → ソケットの表 `handles` (遅延して閉じるための保留数と閉じた印を併せ持つ)、札 → `OpKey` の表 `tickets`、プール、解決器、`HandleBin`、壊れた印。続き (短い書きの残り、`ConnectionAborted` の受けの出し直し、接続の次の番地、解決の答えからの接続) はすべて核が出す。
- backend (`src/sys/`) が持つもの: OS の資源 (epoll/kqueue/IOCP のハンドル、ソケット、起こしの物体)、時計、スレッドの起こし方、fd ごとの状態 (readiness の記憶、向きごとの待ち手1つ、接続中の印、世代)。backend は `start` 1回につき「その歩の1回の試み」を OS に出し、同じ歩を自分で試し直さない。1回の試みは複数の syscall でよい (`accept4` + 受けたソケットの登録、`Connect` の再試行の `SO_ERROR` + `getpeername`、`EINTR` の即時のやり直し)。
- fd ごとの状態は backend に置く。理由: readiness の記憶は epoll/kqueue にだけ意味があり、IOCP と io_uring には存在しない。核に置くと核が ET の意味論を知ることになり、完了型 backend で死んだ欄になる。核が handle ごとに持つのは「保留数と閉じた印」だけで、これはどの backend にも要る (`close` の遅延)。
- `Sock` と backend の表の結び付き: `type Sock` は backend が決める型で、backend の表の行番号と世代を中に持ってよい (例: `Sock { fd: OwnedFd, row: u32, gen: u32 }`)。「核は中を見ない」の意味は、核が `Sock` のフィールドを読まず、写しを作らず、`backend.close` 以外で落とさないことである。backend の表の行は `listen`/`socket`/`Accept` の `Accepted` で作り、`close(sock)` でだけ放して世代を進める。OS に渡す識別子 (epoll の `data`、kqueue の `udata`) は `row | gen` で、`wait` は世代の合わない event を捨てる (同じ `epoll_wait` の束に閉じた行の event が残っていても当たらない)。核が持つすべての `Sock` (`handles`、`Connect` の欄、捨てる完了の中の `Accepted`) は `backend.close` を通して放す。
- 完了型 backend (IOCP のソケット、io_uring) への適合: `wait` が返す `Arrival` に `Retry` (readiness: もう1度 `start` せよ) と `Done`/`Cancelled` (完了型: OS が終えた) の両方があり、核は両方を同じ続きの規則で処理する。`Start::Pending(Hold)` で「カーネルが buffer を持っているか」を、`Cancel::{Done, Pending}` で「取り消しがその場で終わったか OS を待つか」を backend が申告する。net-windows の「まず非ブロックの `recv` を試し、ブロックするなら長さ0の `WSARecv` で待ってもう1度 `recv`」は、IOCP backend が `Recv` に `Pending(Hold::None)` を返し、packet が来たら `Retry` を出す形そのままである。
- 暫定の印: `Hold::Buffer`、`Cancel::Pending`、`Arrival::Done`/`Cancelled` はこの段では偽の backend だけが使う。最初の完了型 backend (`net-windows` か `io-uring`) が載るときに形を変えてよく、その段の計画に変更を書く。trait のコメントにもこの旨を書く。

Rust の境界 (`crates/eml_io/src/driver/backend.rs`。すべて `pub(crate)`):

```rust
/// 1つの操作の1歩。核が出し、backend は OS にその歩の1回の試みを出す。
pub(crate) enum Step<'a, S> {
    /// `Readable` の1歩。読めるデータか EOF があれば `Size(0)` で終わる
    Peek { sock: &'a mut S },
    Recv { sock: &'a mut S, buf: RawBuf },
    Send { sock: &'a mut S, buf: RawBuf },
    Accept { listener: &'a mut S },
    Connect { sock: &'a mut S, peer: SocketAddr },
}

pub(crate) enum Outcome<S> {
    Size(usize),
    Accepted { sock: S, peer: SocketAddr },
    Connected,
}

/// `Pending` の間、カーネルが `Step` の buffer に触れうるか (暫定。完了型の backend が載るときに見直す)
pub(crate) enum Hold { None, Buffer }

pub(crate) enum Start<S> {
    Done(io::Result<Outcome<S>>),
    Pending(Hold),
}

/// `Done`: backend は鍵と待ち手を忘れ、OS も buffer に触れない。`Pending`: 終わりは `wait` の `Event` で届く (暫定)
pub(crate) enum Cancel { Done, Pending }

pub(crate) enum Arrival<S> {
    /// readiness: 同じ `Step` をもう1度 `start` せよ。余分に届いても害がない
    Retry,
    /// 完了型: OS がこの歩を終えた (暫定)
    Done(io::Result<Outcome<S>>),
    /// 完了型: 核が頼んだ取り消しで終わった (暫定)
    Cancelled,
}

pub(crate) struct Event<S> { pub key: OpKey, pub arrival: Arrival<S> }

pub(crate) trait Backend: Sized {
    /// backend が所有する OS のハンドル。核はフィールドを読まず、写しを作らず、`close` 以外で落とさない
    type Sock;
    /// ソケットを扱うか。偽なら核は `Listen`、`Accept`、`Connect` を `Unsupported` で終え、backend を呼ばない
    const SOCKETS: bool;

    fn now(&self) -> Duration;
    fn waker(&self) -> Arc<dyn IoWaker>;
    /// プールと解決器のスレッドの作り方。本物は `mask::spawn_masked`、偽の backend は `Builder::spawn`
    fn spawn(&self, builder: thread::Builder, body: Box<dyn FnOnce() + Send + 'static>) -> io::Result<thread::JoinHandle<()>>;

    /// 作って登録まで済ませたリスナー。ブロックせず CLOEXEC で、unix は `SO_REUSEADDR`、backlog 1024
    fn listen(&mut self, addr: SocketAddr) -> io::Result<(Self::Sock, u16)>;
    /// `peer` へ接続するための、作ってブロックしない設定にしたソケット。登録は `start(Connect)` の中で行う (下)
    fn socket(&mut self, peer: SocketAddr) -> io::Result<Self::Sock>;
    fn shutdown_write(&mut self, sock: &mut Self::Sock) -> io::Result<()>;
    fn set_nodelay(&mut self, sock: &mut Self::Sock, on: bool) -> io::Result<()>;
    /// backend の表の行を放して世代を進め、OS の資源を閉じる
    fn close(&mut self, sock: Self::Sock);

    fn start(&mut self, key: OpKey, step: Step<'_, Self::Sock>) -> Start<Self::Sock>;
    fn cancel(&mut self, key: OpKey, step: Step<'_, Self::Sock>) -> io::Result<Cancel>;
    /// `timeout` まで、起こしか event まで待つ。時間切れ、起こし、`EINTR` は `Ok(())` で events が空。`Err` は壊れた
    fn wait(&mut self, timeout: Option<Duration>, events: &mut Vec<Event<Self::Sock>>) -> io::Result<()>;
}
```

核の表 (`crates/eml_io/src/driver/mod.rs`):

```rust
pub(crate) struct Driver<B: Backend> {
    backend: B,
    slots: HashMap<OpKey, Slot<B::Sock>>,
    done: Vec<Completion>,
    handles: HashMap<IoHandle, Handle<B::Sock>>,
    next_handle: IoHandle,
    tickets: HashMap<Ticket, OpKey>,
    inbox: Arc<Inbox>,          // プールと解決器の答えの受け箱。`Ticket` 付き
    pool: Pool,
    resolver: Resolver,
    bin: Arc<HandleBin>,
    broken: Option<Broken>,     // Broken { kind: io::ErrorKind, msg: String, by: By::{Wait, Cancel, Timeout} }
}
struct Handle<S> { sock: S, ops: u32, closed: bool }
struct Slot<S> { kind: Kind<S>, cancelled: bool, hold: Hold, cancel_sent: Option<Duration> }
enum Kind<S> {
    Readable { handle: IoHandle },
    Read { handle: IoHandle, buf: RawBuf },
    Write { handle: IoHandle, buf: RawBuf, written: usize },
    Accept { listener: IoHandle },
    Connect { sock: S, peer: SocketAddr, rest: std::vec::IntoIter<SocketAddr> },
    Lookup { ticket: Ticket, port: u16 },
    Job { ticket: Ticket },
}
pub(crate) type Ticket = u64;
```

- 向きの規則 (roadmap の「読み・書きの列」からの逸脱。計画に書く): `Peek`/`Recv`/`Accept` は読みの向き、`Send`/`Connect` は書きの向き。核は1つの handle の1つの向きに保留を1つしか出さない (2つ目は `Submit::Done(Err(InvalidInput, "the handle has a pending op in this direction"))`)。backend の fd ごとの待ち手は向きごとに `Option<OpKey>` 1つで、列を持たない。根拠: std の `TcpStream`/`TcpListener` は `Lin` で記録へ移してから待つので、VM は1つの handle に同時に2つの操作を出さない [V: `std/Net.em:7-11`、`cells.rs` の記録の流れ]。`Cancel::Pending` の欄も完了まで向きを占めるが、VM は取り消した操作の完了まで park する [V: reviewer が `cells.rs:2262-2269` で確認]。後で双方向の handle を同じ向きで2つ待たせるときは、backend の待ち手を `VecDeque<OpKey>` にして核の守りを外す局所の変更で足りる。
- `close(handle)`: `closed = true` にし、`ops == 0` なら `backend.close`、そうでなければ最後の操作が終わったときに閉じる。ソケットの写し (`SharedFd`) は作らない。
- `Connect` の歩 (readiness backend の規則。A-3): `socket()` は作るだけで登録しない。`start(Connect)` の1回目は `connect()` を出し、その直後に (結果が `EINPROGRESS` でも即時の成功でも) edge で登録し、行に `connecting = true` を立てて `Pending(Hold::None)` を返す (即時の成功なら `Done(Connected)`)。`Retry` 後の `start(Connect)` は行の `connecting` を見て、`connect()` を出し直さずに `getsockopt(SO_ERROR)` を読み、誤りなら `Done(Err)`、0 なら `getpeername` が通れば `Done(Connected)`、`ENOTCONN` なら `Pending` のまま。接続の前に登録すると `EPOLLOUT|EPOLLHUP` の余分な event が1つ来るが、接続の後に登録すればそれが来ず、既に接続が終わっていても `EPOLLOUT` が届く [V: `s1-probe/epoll_connect2.py`、Linux 6.18.44]。したがってこの段の `Connect` は「ソケットごとに1回の登録」を保ったまま余分な `Retry` を避ける。kqueue でも同じ順にし、S2 が macOS の CI で確かめる。
- `Readable` (`Peek`) の歩 (readiness backend の規則。roadmap の「記憶が立っていれば1バイトを覗いて確かめ」からの逸脱。計画に書く): 行は読みの向きに `ready: bool` と `fresh: bool` を持つ。`fresh` は event が読みの readiness を立てたときに立ち、その fd への読みの向きの syscall (`recv`/`peek`/`accept`) で下りる。`Peek` は、`ready && fresh` なら syscall なしで `Done(Size(0))`、`ready && !fresh` なら1バイトの `MSG_PEEK|MSG_DONTWAIT` で確かめ (`EAGAIN` なら `ready = false` にして `Pending`)、`!ready` なら syscall なしで `Pending`。ET の辺は `EAGAIN` の後の新しいデータか EOF か誤りでだけ立つので、`fresh` な記憶は次の読みがブロックしないことを保証する。これで待つ読みは空きの有無によらず 3 syscall (`EAGAIN` の読みか覗き、`epoll_pwait`、読み) になり、データのある空きなしの読みは辺の直後なら 1、短い読みの後なら 2 (覗き + 読み) になる。

### Q2. 操作の表

- 鍵は呼ぶ側の `OpKey` そのまま。表は `HashMap<OpKey, Slot>` を作り直さず使い回す (`mem::take` しない)。`wait` は event が名指しした欄と、札で当たった欄だけに触れ、表を歩かない。
- 鍵の使い回しと古い知らせ:
  - readiness backend の `Retry` は「もう1度試せ」で冪等なので、使い回した鍵に当たっても害がない。それでも backend は、今登録されている待ち手の鍵でだけ `Retry` を出す。待ち手を忘れるのは、`start` が `Done` を返したとき、`cancel` が `Done` を返したとき、`close` のとき、の3つすべてである (A-4)。
  - 完了型 backend は OS に渡す値 (`user_data`、`OVERLAPPED`) を自分の表の番号と世代にし、`OpKey` を直接渡さない。`Cancel::Pending` の欄は event が来るまで核が保留に残すので、その鍵は使い回されない。
  - プールと解決器の答えは `Ticket` (driver の中で1度しか使わない `u64` の連番) で当てる。`tickets: HashMap<Ticket, OpKey>` を引き、なければ捨てる。解決の取り消し (`cancel`、`quiesce`) は札を外すので、遅れて来た答えは使い回した鍵に当たらない。
- `completions(out)`: `empty_bin()` の後 `out.append(&mut self.done)` だけ。OS を見ず、表を歩かない。
- 1回分の内部の手順 `turn(timeout)` (`wait` と `quiesce` と `Drop` が共有する): (a) 受け箱を引き取り、札で欄に当てる (仕事は完了か `resume_unwind`、解決は接続の試行の始まり)。(b) `backend.wait(timeout, events)`。`Err` なら壊れ (`By::Wait`) にして `Err` を返す。(c) event ごとに欄を引き、`Retry` なら同じ歩を `start` し直し、`Done`/`Cancelled` なら続きの規則を当てる。欄がなければ捨てる。(d) 受け箱をもう1度引き取る。
- `wait(deadline)` の手順 (A-1): (1) `empty_bin`。(2) 壊れていれば `Err`。(3) `timeout` を決める: `done` が空でなければ 0。そうでなく `deadline == None` かつ `slots` と `done` がともに空なら、OS を見ずに `Ok(false)` を返す。`deadline == None` (保留あり) と `Some(u64::MAX)` は上限なし (`u64::MAX` は保留がなくても `Ok(false)` を返さず起こしを待つ)。`Some(d)` は `d ms − now()` を飽和で引く。(4) `turn(timeout)`。(5) `Ok(true)`。内部の刻みは持たない。timeout は backend が OS の上限 (epoll は `i32::MAX` ms、IOCP は `DWORD`) で切ってよく、切った待ちは早く戻るだけである。ミリ秒への丸めは切り上げる。
- `pending() == slots.len() + done.len()`。

### Q3. プールと解決器

- プール (`src/driver/pool.rs`、OS を呼ばない): `Arc<Shared { state: Mutex<State>, cv: Condvar, inbox: Arc<Inbox> }>` と、持ち主側の `threads: Vec<JoinHandle<()>>`。`State { queue: VecDeque<(Ticket, Job)>, threads: usize, idle: usize, running: HashSet<ThreadId>, closed: bool }`。
  - `submit(Op::Job)`: lock の下で札を振り、列に積み、`idle == 0 && threads < POOL_LIMIT (32)` なら `threads += 1` してから lock を放し、`Backend::spawn` でスレッドを1本作り、`notify_one` して戻る。呼んだスレッドを止める道はない。スレッドを作れなかったときは lock の下で `threads -= 1` し、その札の仕事が列にまだあれば取り除いて `Err(e)` の完了を `done` に積み、別のスレッドが既に取っていれば何もしない (A-5。二重の完了はない)。`threads` には始まる前のスレッドも数えるので、同時に積んだ仕事の数だけスレッドが増えうるが 32 を超えない。
  - スレッド: `loop { lock; closed なら抜ける; 列が空なら idle += 1 して cv.wait_timeout(POOL_IDLE = 10 s)、起きたら idle -= 1、時間切れで列も空なら抜ける; 仕事を取り running.insert(自分); unlock; catch_unwind で走らせ; inbox.push(ticket, Ok(result) | Err(payload)); waker.wake(); lock; running.remove(自分) }`。抜けるときに `threads -= 1`。
  - panic の起こし直し: `turn` が受け箱から `Err(payload)` を取り出したら、その欄を外し、`in_flight` の帳尻のために完了は積まずに `std::panic::resume_unwind(payload)`。spec の「仕事の panic を driver を持つスレッドの上で起こし直す」を保つ。
  - `Drop` (プール): lock の下で `closed = true`、`notify_all`、`running` にないスレッドの `JoinHandle` を選んで lock を放し、それらを `join` する (空いているスレッドは `closed` を見てすぐ抜けるので待ちは短い)。`running` のものは `JoinHandle` を落として切り離す。切り離したスレッドは `Arc<Shared>` と `Arc<Inbox>` を自分で持つので、driver が消えた後も答えを積んで終わる。`mem::forget` も `Box::leak` も要らない。
- 解決器 (`src/driver/resolve.rs`、今の `src/resolve.rs` を移す): 鍵の代わりに札を持ち、`JoinHandle` を持つほかは今と同じ (生きた解決 8 本 `RESOLVE_LIMIT`、手放した解決 64 本 `ABANDONED_LIMIT` で新しい解決を即時に誤り、スレッド名 `eml-resolve`、スタック 256 KiB、`Lookup` の関数ポインタの差し替え)。答えは同じ `inbox` に `(Ticket, io::Result<Vec<SocketAddr>>)` で届く。`turn` の (a) で答えを受け取った札の `JoinHandle` を `join` する (答えた後のスレッドはすぐ終わる)。`Drop` は答えのまだないスレッドを切り離す。`with_lookup` は OS によらず提供する (Windows では `SOCKETS == false` なので解決の前に `Unsupported` で終わる)。
- シグナルの塞ぎ: プールと解決器のスレッドはどちらも `Backend::spawn` で作り、本物の backend はこれを `mask::spawn_masked` で実装する (塞ぎは継承で届く)。`Masked` で `push` を囲む今の形は要らなくなる。`pool_threads_block_interrupt_and_terminate` はそのまま通る。
- 起こし: `Inbox::push` は積んだ後に `waker.wake()` を呼ぶ。起こしの物体 (eventfd、kqueue の `EVFILT_USER` を鳴らす kq か自己パイプの書き側、IOCP のポート) は waker 自身が `Arc` で所有し、backend はその写しを持つ。driver が落ちた後にスレッドが鳴らしても、自分が所有する生きたハンドルに書くだけで、使い回された fd 番号に書く道がない。
- 落とすときに待たないことの意味 (B-1): VM は `vm::run` の直後、`Finish`、回復の前に `quiesce` を呼び、`quiesce` は仕事を待ちきる (取り消せず、VM の記録が結果を待つ。spec 477 の理由はそのまま)。したがって壊れていない run では `Drop` に仕事は残らない。roadmap の「落とすときに仕事と名前の解決を待たない」が効くのは、壊れた driver と、`quiesce` を呼ばない埋め込み先である。得るものは「壊れた driver の鍵を漏らさない」「`Drop` の正しさが待ちに依存しない (compio の鍵の数え方の都合が消える)」であり、Ctrl-C の後の終わりの速さではない。

### Q4. 壊れた driver と終わり

- 壊れた = 次のどれか: `backend.wait` が `Err` を返した (`By::Wait`)、`backend.cancel` が `Err` を返した (`By::Cancel`)、`quiesce` が取り消した完了型の操作の完了を `CANCEL_LIMIT` (1 s) 待っても受け取れなかった (`By::Timeout`。C-5)。プールや解決のスレッドを作れないことは壊れではない (その操作だけ誤りで終える)。
- 壊れた後: `wait` は `Err(kind, msg)`、`submit` は `Op::Job` を含むどの操作も `Submit::Done(Err(同じ誤り))` (OS の資源も仕事も新しく作らない。spec 461 の「`Op::Job` はいつも `Pending`」に「壊れていない driver では」を付ける。A-6)、`completions` は何も返さない (今と同じ)、`cancel` は OS を呼ばず印だけ付ける、`quiesce` は `empty_bin` と `done` の破棄だけして保留を残して戻る (VM は `pending()` で見て `Machine` を汚す)。
- `quiesce` (壊れていないとき): `empty_bin`、`done` を捨てて中身のハンドルを閉じる、解決の欄は札を外して手放す、仕事以外の欄を `cancel` する (readiness は `Cancel::Done` でその場で終わる。`Cancel::Pending` の欄に `cancel_sent = now()` を記す)。欄が残る間 `turn(timeout)` を繰り返して届いた完了を捨てる (B-7。受け箱の引き取りも `turn` の中にある)。`timeout` は、`Cancel::Pending` の欄があればその最古の `cancel_sent + CANCEL_LIMIT (1 s)` までの残り、仕事だけなら上限なし。`CANCEL_LIMIT` を過ぎて `Cancel::Pending` の欄が残れば壊れ (`By::Timeout`、文言 `a cancelled op did not complete`) にして戻る。仕事は上限なしに待つ (取り消せない)。
- `Drop` (壊れていてもいなくても同じ手順。B-2): `empty_bin`。`broken.by == By::Wait` なら、`Hold::Buffer` の欄が1つでもあれば標準エラーに1行書いて即座に `process::abort()` (OS の終わりを読む道がない)。そうでなければ、`Hold::Buffer` の欄で未取り消しのものに `backend.cancel` を出し (`Err` は無視して待ちに進む)、`Hold::Buffer` の欄が残る間、合計 `HELD_LIMIT` (1 s) まで `turn(Some(残り))` を繰り返して `Done`/`Cancelled` の event で欄を外す。`turn` が `Err` を返すか 1 s を過ぎて欄が残れば abort。それ以外の欄 (readiness の保留、仕事、解決) は待たずに捨て、`Connect` の欄のソケットと `handles` は `backend.close` で閉じ、プールと解決器の `Drop` が上のとおりスレッドを選んで join する。`Machine` が heap より先に driver を落とす今の順はそのまま要る (貸した記憶をこの 1 s の間生かすため)。
- カーネルが buffer を持ちうる backend: この段では存在しない (epoll/kqueue は `Hold::None` だけを返し、Windows の IOCP は `SOCKETS == false` で `start` に至らない)。後の段では IOCP のソケット (`WSASend` の buffer、`AcceptEx` の backend 所有の番地 buffer。長さ0の `WSARecv` は `Hold::None`) と io_uring (`recv`/`send`) が `Hold::Buffer` を返す。`Drop` の 1 s の規則はこれらのためで、この段では `Drop` は待たない。

### Q5. `unsafe` の方針

- `unsafe` を書けるファイル (`UNSAFE_FILES`): `src/mask.rs`、`src/test.rs` (テストの driver が `RawBuf` に slice を作って読み書きする今の `unsafe`。変えない)、`src/sys/unix.rs`、`src/sys/epoll.rs`、`src/sys/kqueue.rs`、`src/sys/iocp.rs`。それ以外は 0。`lib.rs` に `#![deny(unsafe_code)]`、一覧のファイルの先頭に `#![allow(unsafe_code)]`。
- 固定するテスト `src/trust.rs` (`#[cfg(test)] mod trust;`、`eml_rt/src/trust.rs` の形を写す): `FILES: &[(&str, &str)]` に `include_str!` で全ソースを並べ、`SKIPPED = ["trust.rs", "driver/tests/"]`。テスト: `unsafe_stays_in_the_listed_files` (コメントと文字列を消した後の `unsafe` トークンが `UNSAFE_FILES` にだけ現れ、`allow(unsafe_code)` を持つファイルの集合が `UNSAFE_FILES` と一致する)、`every_source_file_is_listed` (`read_dir` の歩きと `FILES ∪ SKIPPED` が一致)、`the_real_driver_makes_no_slices_and_leaks_nothing` (`mem::forget`、`Box::leak`、`from_raw_parts` が `test.rs` 以外の `FILES` に現れない。`trust.rs` 自身は `SKIPPED` で、文字列は消してから探す)、消し方の自己テスト。
- 貸しの規則の本物の driver 側 (C-2。spec には「本物の driver」の規則として書く): 核は `RawBuf` を値 (`Copy`) として欄に持つだけで `ptr` を参照しない。`ptr` と `cap` を読むのは `sys/*` の OS を呼ぶ関数の中だけで、slice にせず生のポインタと長さを `libc`/`WSABUF` に渡す。完了型 backend が `Hold::Buffer` の間 buffer を OS に預けるときも、その参照は backend の表の欄にだけあり、`Done`/`Cancelled` の event を出す前に外す。テストの driver は貸しの規則 (読みの貸しは一意、書きの貸しは生かして書き換えない) を守って slice を作る今の形のままで、これは roadmap の規則の範囲内である。
- Miri で確かめるもの: 核、プール、解決器 (差し替えた `Lookup`)、受け箱を、偽の backend (`src/driver/tests/fake.rs`、台本どおりに `Start`/`Arrival` を返し、`Recv` の buffer にテストの指示で書く) の上で流す。`cargo miri test -p eml_io --lib -- driver::tests::` を今の2つの `MIRIFLAGS` で。1テスト 10 s 以下。Miri のテストは、終わる前に出した仕事と解決をすべて完了まで進めてから driver を落とす (プールと解決器の `Drop` が全スレッドを join でき、走っているスレッドが残らない。B-5)。
- CI と ASan で確かめるもの: `tests/real.rs` (本物の backend) と `--lib` を、`ubuntu-24.04` (x86_64)、`ubuntu-24.04-arm` (aarch64)、`macos-15` (aarch64) で `RUSTFLAGS=-Zsanitizer=address cargo test -p eml_io -Zbuild-std --target <triple>`。Windows は Rust の ASan の対象外なので [V: R3 の unstable book の一覧] 普通のテストだけ。ふだんのテスト (`cargo nextest run`) は4つの対象で流す。「落とした後も仕事が走る」テストは、仕事を channel で止め、`drop` が戻った後に channel を放し、仕事の終わりの合図 (2つ目の channel) を受けてから終わる (B-5)。

### Q6. 契約の変更

trait の関数の形は変えない。文言と規則を次のとおり変える (`crates/eml_rt/src/driver.rs` のコメントと `docs/spec/vm.md` の「時計と driver」を同じ commit で):

| 変更 | 利点 | VM 側の追随 | 書き直す所 |
|---|---|---|---|
| `cancel`: 「届かなければ quiesce で5秒待った後に壊れた状態になる」→「readiness の backend はその場で終え、完了型の backend は OS の終わりを 1 s まで待つ。OS が取り消しを断るか 1 s で届かなければ壊れた状態になる」 | 5 s と 10 ms の刻みが消え、readiness では取り消しが OS を呼ばない | なし | vm.md 462、470 |
| 落とすとき: 「quiesce で仕事の終わりを待つ。壊れていれば鍵を漏らす」→「仕事と解決を待たず、カーネルが buffer を持ちうる操作だけ 1 s まで待ち、終わらなければ abort する」。`quiesce` が仕事を待ちきる規則は変えない | 壊れた driver が漏らさない。`Drop` の正しさが待ちに依存しない | なし (`Machine` は driver を heap より先に落とす今の順のまま) | vm.md 470、476、478 |
| `submit` の `Op::Job`: 「32 本すべてが仕事中なら呼んだスレッドの上で空回りで待つ」→「driver の中の列で待ち、`submit` は止まらない」。「いつも `Pending`」→「壊れていない driver ではいつも `Pending`」 | 安全点を通らない待ちが消える | なし (`settle_op` は `Done` を扱える) | vm.md 461、466、driver.rs の `submit` のコメント |
| `submit`: 「compio の鍵をいくつか使う」→「核が同じ鍵の下で歩を出し直す」、壊れた driver では `Done(Err)` を返す | 壊れた後に OS の資源を作らない | なし (`wait` の `Err` で run は終わる) | vm.md 461、468 |
| `wait`: 「OS を見るのは wait だけ」→「OS の readiness と完了を待つのは wait だけ。`submit` と `cancel` はブロックしない呼び出しを出す」 | 実装と一致する文言 | なし | vm.md 460、driver.rs |
| 本物の driver の規則として「OS を呼ぶ関数の中でだけ `ptr` に触れ、slice を作らない」を足す (465 の貸しの規則は変えない) | 貸しの規則の driver 側を明文化。テストの driver は対象外 | なし | vm.md 468 |

変えないもの (検討して据え置き): `Ok(false)` の条件、`Submit::Done` の即時の決着 (`settle_op` のループが頼り、データのある読みの syscall 1回の道そのもの [V: `cells.rs:2331-2356`])、`Readable` の意味 (状態の問い。覗きの省略は backend の中の最適化で意味は変わらない)、`close` の意味 (遅延して閉じる実装で保つ)、`HandleBin`、`Completion::cancelled`、操作の予算 (trait と無関係。512 の再計測は S4)、`Op::Job` の取り消し不可。

### Q7. crate とモジュール

crate は `eml_io` のまま。新しい crate は作らない。module 名 `core` は使わない (`core` crate と衝突する。B-4)。新しい木は今の `real.rs` と名前が重ならない `driver/` と `sys/` に置き、`real.rs` は差し替えの commit で書き換える。

```
crates/eml_io/src/
  lib.rs              #![deny(unsafe_code)]; mod driver; mod flag; mod mask; mod real; mod sys; mod test; #[cfg(test)] mod trust;
                      pub use { mask::spawn_masked, real::RealDriver, test::TestDriver }
  flag.rs             (変えない)
  mask.rs             #![allow(unsafe_code)] (変えない)
  test.rs, test/net.rs  test.rs に #![allow(unsafe_code)] (ほかは変えない)
  real.rs             pub struct RealDriver(Driver<sys::Os>); new(), with_lookup(lookup); IoDriver を委譲
  driver/mod.rs       Driver<B>、Slot、Handle、turn、続きの規則、IoDriver の実装
  driver/backend.rs   Backend, Step, Outcome, Hold, Start, Cancel, Arrival, Event
  driver/inbox.rs     Inbox { arrived: Mutex<Vec<Arrived>>, waker: Arc<dyn IoWaker> }、Ticket
  driver/pool.rs      Pool (std のスレッドと Mutex/Condvar だけ)
  driver/resolve.rs   Resolver (今の src/resolve.rs を札に替えて移す)
  driver/tests/mod.rs #[cfg(test)]。核のテスト (Miri の対象)
  driver/tests/fake.rs  偽の backend
  sys/mod.rs          #[cfg(target_os = "linux")] mod epoll; #[cfg(target_vendor = "apple")] mod kqueue;
                      #[cfg(unix)] mod unix; #[cfg(windows)] mod iocp; pub(crate) type Os = ...;
                      #[cfg(not(any(...)))] compile_error!("eml_io has no I/O backend for this OS")
  sys/unix.rs         #![allow(unsafe_code)] libc のソケット: socket/bind/listen/accept4 (Apple は accept + fcntl)/
                      connect/getsockopt(SO_ERROR)/getpeername/recv(MSG_PEEK|MSG_DONTWAIT)/recv/send(MSG_NOSIGNAL)/
                      shutdown/TCP_NODELAY/SO_NOSIGPIPE/SO_REUSEADDR、SocketAddr <-> sockaddr の写し、OwnedFd
  sys/epoll.rs        #![allow(unsafe_code)] epoll + eventfd の waker + Instant の時計 + fd の表 (行、世代、ready、fresh、waiter、connecting)
  sys/kqueue.rs       #![allow(unsafe_code)] kqueue + EVFILT_USER (か自己パイプ) の waker + 時計 + 同じ表
  sys/iocp.rs         #![allow(unsafe_code)] IOCP のポート + PostQueuedCompletionStatus の waker + 時計。SOCKETS = false
  trust.rs            #[cfg(test)]。unsafe の一覧のテスト
crates/eml_io/tests/  main.rs, real.rs (本物の backend), test_driver.rs (変えない)
```

依存: `eml_rt`; `[target.'cfg(unix)'.dependencies] libc`; `[target.'cfg(windows)'.dependencies] windows-sys = { workspace, features = ["Win32_Foundation", "Win32_System_IO"] }` (workspace の 0.61.2 と1つの項を共有する。feature は和になる)。`compio-driver`、`compio-buf`、`socket2` は workspace からも外す。`socket2` を外す理由: 使うのはソケットの作成と `sockaddr` の写しだけで 50 行ほどの `unsafe` で済み、それを一覧のファイル (`unix.rs`) に置けば crate を1つ減らせる。std の `TcpListener`/`TcpStream` は backlog 128、`accept` 後の `ioctl(FIONBIO)` が1回増え、ブロックしない `connect` がないので使わない [I: std のソースの記憶。確かめていないが設計には影響しない]。

### 後の段への伸ばし方 (B-6)

- `async-files` (パイプと子プロセスのパイプ): `Sock` の行に種類 (`Socket`/`Pipe`) を持ち、`Recv`/`Send` を unix backend が種類で `recv`/`send` と `read`/`write` に振り分ける。`Peek` はパイプでは `poll` 1回 (`POLLIN`、timeout 0) か、readiness の記憶から答える。`Step` と `Outcome` は変えない。普通のファイルは readiness で待てないので、readiness backend では `Op::Job` (プール) に下ろし、io_uring backend では `Step::ReadAt`/`WriteAt { offset }` を足す。variant を足すのは、核の続きの表に行を足す追加の変更で、trait の形は変わらない。
- `ffi-blocking`: `Op::Job` そのまま。仕事が記録の語に触れるようになるので、`quiesce` が仕事を待ちきる規則に乗る。
- `async-threads`: backend は `Send` を要らない。別の worker からの受け付けは受け箱 (`Submit` の知らせ) を通す (multicore.md)。スレッドをまたいで安全に出せる backend を選ぶ道 (async.md 484) は、そのとき `Backend` に `Send` な提出口を足す。
- `io-uring` の歩の対応: `Peek` → `IORING_OP_RECV` (`MSG_PEEK`、1 バイト、`Hold::Buffer` でなく backend 所有の1バイト) で `Done(Size(0))`、または `IORING_OP_POLL_ADD` で `Retry`。`Recv`/`Send` → `IORING_OP_RECV`/`SEND` (`Hold::Buffer`、`Done(Size)`)。`Accept` → `IORING_OP_ACCEPT` (multishot は使わず、`start` 1回に1つ。受けた fd と `sockaddr` は backend 所有)。`Connect` → `IORING_OP_CONNECT` (`sockaddr` は backend の表の行が CQE まで持つ)。取り消し → `IORING_OP_ASYNC_CANCEL` で `Cancel::Pending`、CQE の `-ECANCELED` を `Arrival::Cancelled` に写す。backend 所有の記憶をカーネルに渡すときは CQE まで放さない (G-16)。
- 暫定の部分: `Hold`、`Cancel::Pending`、`Arrival::Done`/`Cancelled` はこの段では偽の backend だけが使う。形を変えるのは最初の完了型 backend の段の計画に書く。

## 2. 根拠

- 核が OS を呼ばないこと: roadmap の「核は OS を呼ばず、偽の backend の上で Miri で確かめる」。時計とスレッドの作り方も backend に出す。Miri は隔離の既定で `Instant::now` を使えない可能性があり [I]、`pthread_sigmask` を支えない [I]。`Backend::now`/`Backend::spawn` で避ける。核は `Mutex`/`Condvar` (`wait_timeout` を含む) と `thread::JoinHandle` を使う。
- `Retry`/`Done` の二本立て: readiness の OS は結果を返さず「試せ」としか言えず、完了型の OS は結果しか返さない。両方を1つの `Arrival` に載せれば、核の続きの規則は1つで済む。
- `Hold`: 壊れた driver を落とすときに 1 s 待つ対象を「カーネルが buffer を持つ操作」に限るには、backend がそれを歩ごとに申告する必要がある。readiness では常に `None` なので、この段では落とすときに待たない。
- `Cancel::{Done, Pending}`: epoll/kqueue は待ち手を外すだけで取り消せる (OS を呼ばない)。IOCP/io_uring は OS に出して完了を待つ。完了が失われる場合に備えて `CANCEL_LIMIT` (1 s) で壊れとし、`Drop` の 1 s が最後の網になる (C-5)。
- 札: プールと解決のスレッドの答えは、どのスレッドからも鍵の使い回しと競う。driver の中の連番ならその競いが消える。
- 1つの handle に1つの向きの保留1つ: `TcpStream`/`TcpListener` は `Lin` で記録へ移る [V: `std/Net.em:7-11`]。
- `SOCKETS` の定数: Windows の `listen_accept_and_connect_are_unsupported` は `Accept { listener: 0 }` にも `Unsupported` を期待する [V: `tests/real.rs:1357-1379`]。
- epoll の事実 [V: `s1-probe/epoll_connect.py` と `epoll_connect2.py`、Linux 6.18.44、x86_64]: 未接続の TCP ソケットを ET で登録すると直後に `EPOLLOUT|EPOLLHUP` が1回来る。`connect` → `EINPROGRESS` の後に登録すると、成功では `EPOLLOUT` だけ、拒否では `EPOLLIN|EPOLLOUT|EPOLLERR|EPOLLHUP|EPOLLRDHUP` が届き、接続が登録より先に終わっていても `EPOLLOUT` が届く。`SO_ERROR` は 0 / `ECONNREFUSED`。拒否の後の2回目の `connect` は `ECONNABORTED` を返す (再 `connect` で状態を読むのは不安定なので `SO_ERROR` + `getpeername` にする)。リスナーは backlog が空でなくても新しい接続ごとに `EPOLLIN` の辺を出す。
- 今のテストの基準: scratch の clone で `cargo test -p eml_io` が 68 件通る [V: 実行、0.30 s]。
- VM の呼び方: `settle_op` は `Submit::Done` の間ループして次の歩を同じ鍵で出す [V: `cells.rs:2331-2356`]。`complete` は `in_flight > 0` のときだけ `completions` を呼ぶ [V: `cells.rs:2299`]。`driver.close` は VM から呼ばれず、`HandleBin` 経由 [V: R1 §4]。`quiesce` は `vm::run` の直後に呼ばれる [V: R1 §4 `lib.rs:778`]。

## 3. 退けた案

- backend を `Interest`/readiness の語で切る (mio 風): IOCP/io_uring が載らない。
- backend が操作の表を持つ (compio 風): 核を Miri で流せず、続きが OS ごとに重複する。
- readiness の記憶を核に置く: 完了型では死んだ欄になり、核が ET を知ることになる。
- ソケットを `Rc`/`SharedFd` で写して保留と handle の表の両方に置く: 遅延の閉じ (保留数) の方が所有が1つで単純。
- `socket2` を残す / std の `TcpStream` で済ます: Q7。
- 解決をプールの仕事にする: 解決は手放す必要があり、プールの仕事は待ちきる。
- 仕事も札で手放せるようにして `cancel` を効かせる、`quiesce` が仕事を待たない: spec の「`Op::Job` は取り消せない」と VM の記録の都合で据え置き。`Drop` が待たないだけで roadmap の文は満たせる。
- `Readable` の `Retry` 後に毎回覗く (roadmap の文言どおり): 待つ読みが 4 syscall になり目標の 3 に届かない。辺の鮮度で省く (Q1)。
- 接続の前に登録する (roadmap の「作るときに1回」の字義): 余分な `EPOLLOUT|EPOLLHUP` が来る。接続の直後に1回登録する。
- テストの driver を核の偽の backend にする、backend ごとに crate を分ける、`#![forbid(unsafe_code)]`、核の表に世代を持つ: r1 と同じ理由で退ける。
- 完了型の `quiesce` に上限を置かない (r1 の案): 失われた完了が終わりの道を止める。1 s で壊れにする (C-5)。

## 4. 置いた仮定

- Miri が隔離の既定で `pthread_sigmask` を支えず、`Condvar::wait_timeout` は支える [I]: 前者が偽でも `Backend::spawn` は害がない。後者が偽なら、プールの空いたスレッドは `wait` (上限なし) で `closed` を待つ形にし `POOL_IDLE` を外す。
- Miri は main が終わるときに走っているスレッドを誤りにする [I]: 偽でも join の規則は害がない。
- fenix の `complete` の nightly が `-Zsanitizer=address` と `-Zbuild-std` を使える (`rust-src` はある [V: flake.nix:73-79])。偽なら S4 が ASan のジョブだけ rustup の nightly を使い、その例外を `testing.md` に書く。
- `ubuntu-24.04-arm` の runner が使える (public repo のみ [V: R3])。private なら aarch64-linux は `.#cross` の clippy だけになる。
- `macos-15` が arm64 [V: R3]。
- kqueue でも接続の直後の登録で `EVFILT_WRITE` の辺が成り立つ [I: epoll で確かめた。S2 が macOS の CI で確かめる]。
- IOCP/io_uring は出した操作に完了を返す [I]。偽でも `CANCEL_LIMIT` と `HELD_LIMIT` が止まりを防ぐ。

## 5. 実装への含意

### 作業の単位

1. `src/driver/` (核、受け箱、プール、解決器、偽の backend、核のテスト) を足す。差し替えまでは `lib.rs` で `mod driver;` を宣言せず `#[cfg(test)] mod driver;` にし、`driver/mod.rs` の先頭に `#![allow(dead_code)]` (理由のコメント付き。差し替えの commit で外す) を置く。今の `real.rs` と `tests/real.rs` はそのまま通る。
2. `sys/unix.rs` + `sys/epoll.rs` (S2)、`sys/kqueue.rs` (S2)、`sys/iocp.rs` (S3) を足し、同じ commit で `real.rs` を `Driver<sys::Os>` に差し替え、compio と socket2 を外す。`tests/real.rs` は下の例外を除いて変えずに通す。
3. `trust.rs`、`#![deny(unsafe_code)]`。
4. CI: aarch64-linux のジョブ、ASan のジョブ、Miri に `eml_io` を足す (S4)。flake の `systems` に `aarch64-linux` を足し、verus の入力がない system では verus のパッケージを `lib.optionals` で外す。
5. `bench/net` の前後比較と 512 の再計測 (S4)。

### テストの変更 (段の計画に書く)

- 成否の変更 (1件ずつ): `dropping_a_driver_waits_for_a_running_job` を削り、`dropping_a_driver_does_not_wait_for_a_running_job` を足す (仕事は channel で止め、`drop` が戻った後に channel を放し、仕事の終わりの合図を受けてから終わる。時間を測らない)。
- 期待値の変更 (範囲): `tests/real.rs` の `real.rs:930` のコメント (compio の言及) と、`a_job_pool_is_capped` の説明。文言の変更だけで成否は変えない。
- 新しいテスト (核、偽の backend、Miri の対象): 短い書きの出し直しが同じ鍵で1回の完了になる、`ConnectionAborted` の受けの出し直し、接続の次の番地と `Cancelled` で次を試さないこと、`Retry` が2回届いても完了が1回であること、`Cancel::Done` がその場で `done` に積まれること、`cancel` の後の `wait(None)` が `Ok(true)` で完了を届けること (A-1)、解決を取り消して同じ鍵を使い回した後に遅れた答えが当たらないこと、33 本目の仕事の `submit` が止まらずに戻ること (仕事は channel で止める)、仕事の panic が `wait` で起こし直されること (`#[should_panic]`)、スレッドを作れない `spawn` で仕事が1回だけ `Err` で終わること、`Hold::Buffer` の欄が `Drop` の中で届けば戻ること、`backend.wait` の `Err` で壊れて `submit` が `Done(Err)` を返すこと (`Job` も)、`quiesce` が `Cancel::Pending` の完了を 1 s 待って壊れになること (偽の時計を進める)、`close` が保留の間は遅れ `backend.close` が1回だけ呼ばれること、同じ handle の同じ向きの2つ目が `InvalidInput` であること。
- 本物の backend のテスト (`tests/real.rs` に足す): 接続の成功と拒否、接続の `Retry` の後に `connect` が出し直されないこと (strace でなく結果で: 拒否が `ConnectionRefused` で1回で終わる)、`Readable` が辺の直後に syscall なしで終わること (`ProbeRecorder` でなく `RunStats` の `completions` で見る必要はない。S2 が決める)。
- `Drop` の abort の道: 偽の backend で、現在のテストバイナリを `std::env::current_exe()` で子として起こし (環境変数で合図)、子が 0 でなく終わり標準エラーに文言があることを確かめる。Miri では流さない (`driver::abort::` の名前空間に置き、Miri のフィルタ `driver::tests::` の外にする)。

### 定数

`POOL_LIMIT = 32`、`POOL_IDLE = 10 s`、`RESOLVE_LIMIT = 8`、`ABANDONED_LIMIT = 64`、`RESOLVE_STACK = 256 KiB`、`CANCEL_LIMIT = 1 s` (`quiesce` の完了型の待ち)、`HELD_LIMIT = 1 s` (`Drop`)、`BACKLOG = 1024`。消えるもの: 5 s の `CANCEL_LIMIT`、`CANCEL_POLL`、`MAX_POLL` (backend の中へ)。

### 書き直す文書 (実装と同じ commit)

- `docs/spec/vm.md` 「時計と driver」: 460 (OS を見る文言)、461 (compio の鍵 → 核の歩、`Op::Job` の `Pending` は壊れていないとき、壊れた driver の `submit`)、462 (取り消しの規則と 1 s)、464 (`cancelled` を立てる場合: readiness はその場の取り消し、完了型は `Arrival::Cancelled`、解決の手放し、続きをやめたとき)、466 (プールは列で待ち `submit` は止まらない)、468 (全文を核と backend の説明に: 接続の直後の1回の edge の登録、fd ごとの記憶と鮮度と向きごとの待ち手、`Readable` の覗きの省き方、`wait` が event の欄だけに触れること、本物の driver が `ptr` に触れる所)、469 (札、join)、470 (`close` の遅延、`quiesce` の 1 s、落とすときの規則)、476〜478 (落とす順、壊れの定義)、485 (waker を鳴らすのはプール・解決・監視)。trait のコードブロック (411-457) は `driver.rs` と同一に保つ。
- `docs/spec/vm.md` 「上限と割り込み」 379: プールのスレッドの塞ぎは `spawn_masked` の継承で得ると書き直す。
- `docs/spec/stdlib.md` 「操作の予算」: 512 が変わるときだけ (S4)。「`Std.Net`」 1319 は変えない。
- `docs/implementation/architecture.md`: 「`eml_io` の内部」を全文書き直し、「外部 crate」の `compio-driver`/`compio-buf`/`socket2` の行を消し `windows-sys` の行に `eml_io` を足し、`libc` の行を更新、47 と 77 の `Asyncify` の記述、425-431 の compio の更新手順を消す。
- `docs/implementation/testing.md`: 「Miri」に `eml_io` の核の command と予算と join の規則、「CI」に aarch64-linux と ASan のジョブ、「OS ごとのテスト」に `.#cross` の aarch64 の target、「テストの置き場所」に核の単体テストが偽の backend の上にある旨。
- `docs/implementation/benchmarks.md` の目標の表: 「データのある `read_chunk` (空きなし)」の目標を「辺の直後 1、短い読みの後 2」に書き直す (S4 が実測と共に)。
- `docs/implementation/status.md` 146、147、149。
- `docs/future/async.md` 「`IoDriver` と `eml_io`」の compio の段落と「未決の論点」の最初の項。`docs/future/roadmap.md` の `io-backends` の項は段を閉じるときに消す。
- `CLAUDE.md`: アーキテクチャの表の `eml_io` の行、Miri の command の行。
- 段の計画 (`docs/plans/`) に roadmap からの逸脱を2つ明記する: 向きごとの待ち手1つ (列でない)、`Readable` の覗きを辺の直後に省くこと、接続の直後に登録すること (3つ目は「作るときに1回」の読み替え)。

## 6. 未決の項目

- `POOL_IDLE` の値と、空いたスレッドが終わることのテスト: 既定 10 s、ふだんの実行では確かめない。
- ASan の leak 検出: 既定は Linux で有効 (ASan の既定)、macOS では無効。偽陽性が出たら S4 が `ASAN_OPTIONS` を決める。
- 壊れた後の `completions` が何も返さないこと: 既定は今のまま。
- `eml_io` の Miri の CPU の予算: 既定 120 s (Stacked) / 160 s (Tree)。S4 が測って置き換える。
- kqueue の `fresh` の立て方 (EVFILT_READ の `data` でデータの量が分かるので、覗きの代わりに使ってよい): 既定は epoll と同じ規則。S2 が決める。

## 7. 追加で調べたこと

- scratch の clone (`repo-s1`、625d088) で `cargo test -p eml_io`: 68 件通過 [V]。
- `s1-probe/epoll_connect.py`、`epoll_connect2.py` (Python の `select.epoll`、`EPOLLET`): 上の「根拠」の epoll の事実 [V]。
- `std/Net.em`: `TcpListener`/`TcpStream` は `Lin` [V]。
- `cells.rs` 2120-2480: `record`/`settle_op`/`advance` の流れ、`Probe` → `Transfer` の出し直しが同じ鍵、`complete` の `in_flight` の守り [V]。
- `tests/real.rs:1357-1379`: Windows の `Unsupported` の期待 [V]。
- `docs/future/multicore.md` の `Submit` の知らせと「縛った worker」 [V]。

## 8. 適合の基準

- **G-1** 核 (`src/driver/` の `tests/` を除くすべてと `src/real.rs`) は OS を呼ばず、`libc`/`windows_sys`/`std::time::Instant` を名指ししない。OS に触れるのは `src/sys/*` と `mask.rs` だけである。核が使ってよいスレッドの道具は `std::sync` と `std::thread::JoinHandle` (起こすのは `Backend::spawn`)。
  - Why: 核を偽の backend の上で Miri で流すため。
  - Applies to: 本物の driver。`test.rs` は対象外。
  - How to check: `grep -rn "libc::\|windows_sys::\|Instant\|thread::spawn\|Builder::spawn\|\.spawn(" crates/eml_io/src/driver crates/eml_io/src/real.rs --exclude-dir=tests` が空 (`Backend::spawn` の呼び出しは `backend.spawn(` と書くので `\.spawn(` に当たる。核では `self.backend.spawn(` の形だけを許し、grep は `--exclude` でなくこの1行を目で除く。確かめる行は `grep -rn "\.spawn(" crates/eml_io/src/driver --exclude-dir=tests` の結果が `backend.spawn(` の行だけであること)。
- **G-2** 続き (短い書きの残り、`ConnectionAborted` の受けの出し直し、接続の次の番地、解決の答えからの接続) は核だけが出す。backend の `start` は歩の1回の試みだけを出し、同じ歩を自分で試し直さない (`EINTR` の即時のやり直しだけ例外)。1回の試みが複数の syscall を持つことは許す (`accept4` + 登録、`connect` + 登録、`SO_ERROR` + `getpeername`)。
  - Why: 続きの規則を OS ごとに重複させず、Miri で確かめるため。
  - Applies to: すべての backend。
  - How to check: 設計では `start` の各歩の擬似コードに「試し直し」がない。コードでは `sys/*` の `start` の中のループが `EINTR` のものだけ。核のテストが偽の backend の上で4つの続きを確かめる。
- **G-3** backend は §1 の `Backend` trait をそのまま実装する。readiness backend は `wait` で `Arrival::Retry` だけを出し、`Start::Pending(Hold::None)` と `Cancel::Done` だけを返す。完了型 backend は `Arrival::Done`/`Cancelled` を出し、カーネルが buffer を持つ歩で `Hold::Buffer`、OS に出した取り消しで `Cancel::Pending` を返す。これら完了型の値は暫定で、最初の完了型 backend の段で見直す。
  - Why: 核が両方の種類を同じ規則で扱うため。
  - Applies to: epoll、kqueue、IOCP、io_uring。
  - How to check: `grep -n "Arrival::Done\|Arrival::Cancelled\|Hold::Buffer\|Cancel::Pending" crates/eml_io/src/sys/epoll.rs crates/eml_io/src/sys/kqueue.rs` が空。
- **G-4** 歩は再試行に耐える。`Retry` が余分に届いても結果は変わらず、完了は1回である。backend は今登録されている待ち手の鍵でだけ `Retry` を出し、待ち手は `start` が `Done` を返したとき、`cancel` が `Done` を返したとき、`close` のときに忘れる。`Connect` の再試行は `connect()` を出し直さず、行の `connecting` の印で `SO_ERROR` + `getpeername` に進む。
  - Why: ET の辺は保証でなく手がかりで、正しさは非ブロックの呼び出しの結果だけに頼るため。
  - Applies to: readiness backend と核。
  - How to check: 核のテスト「`Retry` を2回届けても完了が1回」。`sys/*` の `start(Connect)` に `connecting` の分岐がある。本物のテスト「拒否された接続が `ConnectionRefused` で1回で終わる」。
- **G-5** 1つの handle の1つの向き (読み: `Peek`/`Recv`/`Accept`、書き: `Send`/`Connect`) に保留は1つ。核が `submit` で `InvalidInput` を返して守る。backend の fd ごとの待ち手は向きごとに1つで、列を持たない。roadmap の「列」からの逸脱として段の計画に書く。
  - Why: `Lin` のハンドルがこれを保証し、backend の状態を最小にするため。
  - Applies to: 核と readiness backend。
  - How to check: 核のテスト「同じ handle の読みを2つ出すと2つ目が `Done(Err(InvalidInput))`」。段の計画に逸脱の項がある。
- **G-6** fd ごとの状態 (記憶、鮮度、待ち手、接続中、世代) は backend の自分の表の行に置き、`Sock` は行番号と世代を持ってよい。OS に渡す識別子 (epoll の `data`、kqueue の `udata`、io_uring の `user_data`、IOCP の `OVERLAPPED`) は行番号と世代で、`OpKey`、`IoHandle`、fd 番号をそのまま渡さない。行は `close` でだけ放して世代を進め、`wait` は世代の合わない event を捨てる。読みの記憶は読みの向きの `EAGAIN` でだけ下ろす。
  - Why: 使い回した fd や鍵に古い知らせが当たらず、核と backend の表がずれないため。
  - Applies to: すべての backend。
  - How to check: 設計では行の型と `Sock` の型。コードでは `epoll_event.u64`/`kevent.udata` を作る所が1か所で、世代を含み、`wait` に世代の比較がある。
- **G-7** ソケットは1回だけ edge (`EPOLLET`/`EV_CLEAR`) で読み書き両方の関心を登録し (リスナーと受けたソケットは作ったとき、接続のソケットは `connect()` の直後)、以後 `close` まで登録を変えない。操作ごとの `epoll_ctl`/`kevent` の変更は 0。
  - Why: roadmap の目標 (操作ごとの `epoll_ctl` 0)。
  - Applies to: epoll、kqueue。
  - How to check: `bench/net` の echo-py-reuse (接続 1 本) の main の `epoll_ctl` の総数が 2 以下 (リスナー 1 + 受けたソケット 1) で、往復あたり 0。conn-self-32 の `epoll_ctl` が接続あたり 2 (受けたソケットと接続のソケット)。
- **G-8** 核の表は使い回す (`mem::take` で作り直さない)。`completions` は `done` を `append` するだけで OS を見ず表を歩かない。`turn` は event が名指しした欄と受け箱の札の欄にだけ触れ、内部の刻みを持たない。timeout の丸めは切り上げ、OS の上限で切るのは早く戻る方向だけ。
  - Why: 保留が多くても1接続の費用が本数によらないため (many-self 2000/250 ≤ 1.5)。
  - Applies to: 核とすべての backend。
  - How to check: `grep -n "mem::take\|slots.iter\|slots.values\|slots.keys\|slots.drain" crates/eml_io/src/driver/mod.rs` の結果が `quiesce` と `Drop` の中の行だけ。`bench/net` の many-self の比。
- **G-9** スレッドからの答え (仕事、解決) は `Ticket` で当てる。札は driver の中で使い回さない `u64` の連番で、`tickets` にない札の答えは捨てる。解決の取り消しと `quiesce` は札を外す。
  - Why: 使い回した鍵の操作に古い答えを当てないため。
  - Applies to: 核、プール、解決器。
  - How to check: 核のテスト「解決を取り消して同じ鍵で別の操作を出し、遅れた答えを届けても当たらない」。
- **G-10** プールは 32 本まで、満ちたら driver の中の列で待ち、`submit` は呼んだスレッドを止めない。仕事は `catch_unwind` で走り、panic は `turn` が持ち主のスレッドで `resume_unwind` する。スレッドを作れなかった仕事は1回だけ `Err` で終わる。プールと解決器のスレッドは `Backend::spawn` で作り、本物の backend は `spawn_masked` で実装する。`JoinHandle` を持ち、`Drop` は走っていないスレッドだけを join し、走っているものを切り離す。
  - Why: 安全点を通らない待ちをなくし、spec の panic の規則と塞ぎを保ち、Miri のテストがスレッドを残さないため。
  - Applies to: `pool.rs`、`resolve.rs`、本物の backend の `spawn`。
  - How to check: テスト「33 本目の `submit` が止まらない」「panic が起こし直される」「`spawn` の失敗で完了が1回」「`pool_threads_block_interrupt_and_terminate`」。G-1 の grep。
- **G-11** waker は鳴らす OS の物体を `Arc` で自ら所有し (backend は写しを持つ)、鳴らした印は次の `wait` が消費するまで残る (eventfd の値、`EV_CLEAR` の `EVFILT_USER`、IOCP の packet)。受け箱への `push` の後に `wake` する。シグナルの handler からは呼ばない。
  - Why: driver が落ちた後の起こしが使い回されたハンドルに触れず、起こしが失われないため。
  - Applies to: すべての backend と `Inbox`。
  - How to check: 設計では waker の構造体の所有の図。コードでは waker 型が `OwnedFd`/`HANDLE` を直接持つか `Arc` で共有する。テスト `a_wake_from_another_thread_leaves_a_wait_working`。
- **G-12** 壊れる条件は `backend.wait` の `Err`、`backend.cancel` の `Err`、`quiesce` の `CANCEL_LIMIT` (1 s) の超過の3つだけ。壊れた後は `wait` が `Err`、`submit` が `Op::Job` を含めて `Submit::Done(Err)`、`completions` が空、`cancel` は OS を呼ばず、`quiesce` は保留を残して戻る。
  - Why: 壊れた OS の資源に触れ続けず、失われた完了で終わりが止まらないため。
  - Applies to: 核。
  - How to check: 核のテスト (偽の backend の `wait` を `Err` にする、`Cancel::Pending` の完了を届けずに偽の時計を 1 s 進める)。
- **G-13** `quiesce` は仕事を待ちきり、解決を手放し、ほかを取り消して、`Cancel::Pending` の完了を 1 s まで待つ。`Drop` は仕事も解決も待たず、`Hold::Buffer` の欄だけ `HELD_LIMIT` (1 s) まで待ち (`By::Wait` で壊れていれば待たず)、残れば `process::abort()`。`eml_io` の本物の driver に `mem::forget` と `Box::leak` を書かない。
  - Why: roadmap の規則。
  - Applies to: 核。
  - How to check: `grep -rn "mem::forget\|Box::leak" crates/eml_io/src --exclude=trust.rs` が空。テスト `dropping_a_driver_does_not_wait_for_a_running_job`、abort の子プロセスのテスト。
- **G-14** `close(handle)` は保留があれば遅らせ、最後の操作が終わったときに `backend.close` する。ソケットの所有は1つ (`Rc`/`SharedFd` の写しを作らない)。核が持つ `Sock` は `backend.close` 以外で落とさない。
  - Why: 保留の操作の間 OS の資源を生かす spec を、所有を増やさずに満たすため。
  - Applies to: 核。
  - How to check: 核のテスト「保留のある handle を `close` し、完了の後に偽の backend の `close` が1回呼ばれる」「`quiesce` で捨てた `Accepted` のソケットに `close` が呼ばれる」。
- **G-15** `unsafe` は `UNSAFE_FILES` (`mask.rs`、`test.rs`、`sys/unix.rs`、`sys/epoll.rs`、`sys/kqueue.rs`、`sys/iocp.rs`) にだけあり、各ファイルは `#![allow(unsafe_code)]` で始まり、`lib.rs` は `#![deny(unsafe_code)]`。本物の driver では `RawBuf` の `ptr` を `sys/*` の OS を呼ぶ関数の中でだけ読み、slice を作らず (`from_raw_parts` を書かない)、完了型 backend は `Hold::Buffer` の間だけ参照を表の行に置く。`test.rs` は貸しの規則を守って slice を作る今の形のまま。
  - Why: roadmap の「unsafe は一覧のファイルに閉じ、ソースを読むテストで固定する」。
  - Applies to: `eml_io` 全体。slice の禁止は本物の driver (`test.rs` 以外)。
  - How to check: `cargo nextest run -p eml_io trust::` が通る。`grep -rn "from_raw_parts" crates/eml_io/src --exclude=trust.rs --exclude=test.rs` が空。新しいファイルを `FILES` に載せないと `every_source_file_is_listed` が落ちる。
- **G-16** 核は OS の errno 定数を使わず `io::ErrorKind` だけで判断する (受けの出し直しは `ConnectionAborted`、取り消しの誤りは核が作る `io::Error::other("cancelled")`)。backend が自分の所有する記憶をカーネルに渡すとき (`sockaddr`、`OVERLAPPED`、覗きの1バイト) は、OS がその終わりを告げるまで放さない。
  - Why: 核を OS から切り、backend 所有の記憶の寿命を1つの規則にするため。
  - Applies to: 核 (前半)、完了型 backend (後半)。
  - How to check: `grep -rn "raw_os_error\|ECANCELED\|ECONNABORTED\|EAGAIN" crates/eml_io/src/driver --exclude-dir=tests` が空。
- **G-17** 検証の組: 核は Miri (`cargo miri test -p eml_io --lib -- driver::tests::`、今の2つの `MIRIFLAGS`、1テスト 10 s 以下、どのテストも出した仕事と解決を完了まで進めてから driver を落とす)、OS を呼ぶ部分は4つの対象の CI の普通のテストと、x86_64-linux・aarch64-linux・aarch64-apple-darwin の ASan (`-Zsanitizer=address`、`-Zbuild-std`、`--target` 明示)。ふだんのテストは1件 1 s 以下で機械の速さに依らず、本物の時計のテストは下限と終わることだけを確かめる。切り離したスレッドを使うテストは、そのスレッドの終わりの合図を受けてから終わる。
  - Why: roadmap と `testing.md` の規則。
  - Applies to: `eml_io` のテストと CI。
  - How to check: `ci.yml` に miri の `-p eml_io` の step、`ubuntu-24.04-arm` の `test` の行、ASan のジョブがある。`cargo nextest run -j 1 -p eml_io` で `SLOW` が出ない。
- **G-18** 依存は `eml_rt`、unix の `libc`、windows の `windows-sys` だけ。`compio-driver`、`compio-buf`、`socket2` は workspace からも消す。対応しない OS は `compile_error!`。
  - Why: roadmap の「libc と windows-sys で自前に書く」。
  - Applies to: `Cargo.toml` (workspace と `eml_io`)、`Cargo.lock`。
  - How to check: `grep -n "compio\|socket2" Cargo.toml crates/*/Cargo.toml Cargo.lock` が空。`nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc -p eml_io --all-targets -- -D warnings` が通る。
- **G-19** 文書は実装と同じ commit で書き換え (§5 の一覧)、コードのコメントは `docs/spec/vm.md` の「時計と driver」などの見出しを引く。`docs/plans/` はコードからも `docs/` からも引かない。roadmap からの逸脱 (G-5、`Readable` の覗きの省き方、接続の直後の登録) は段の計画に書く。
  - Why: CLAUDE.md の規則。`citations.rs` が見出しの存在を確かめる。
  - Applies to: 段のすべての commit。
  - How to check: `cargo nextest run -p eml_cli --test integration citations::` が通る。`grep -rn compio docs/spec docs/implementation CLAUDE.md` が空。

### 完了の基準 (段全体の候補)

- `cargo nextest run` が全件通り、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check`、feature の組の clippy が通る。
- CI の `test` が `ubuntu-24.04`、`ubuntu-24.04-arm`、`macos-15`、`windows-2025` の4つで緑。
- `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_io --lib -- driver::tests::` と `-Zmiri-tree-borrows` 付きが通る。
- ASan のジョブが3つの対象で緑。
- `cargo nextest run -p eml_io trust::` が通り、`grep -rn "mem::forget\|Box::leak" crates/eml_io/src --exclude=trust.rs` と `grep -rn "from_raw_parts" crates/eml_io/src --exclude=trust.rs --exclude=test.rs` が空。
- `grep -n "compio\|socket2" Cargo.toml crates/*/Cargo.toml Cargo.lock` が空。
- `bench/net` の strace (S4 が手順と表を作る): echo-py-reuse の main で、往復あたりの `epoll_ctl` が 0 で総数が 2 以下、データのある読みが 1、書きが 1; echo-self-fresh の待つ読みが 3 (`EAGAIN` の読みか覗き + `epoll_pwait` + 読み); many-self 2000/250 の1接続の CPU の比が 1.5 以下。

