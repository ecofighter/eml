# 付録4: 後の段 net-windows の設計

この付録は後の段 `net-windows` の設計である。io-fastpath でも io-backends でも実装しない。段の前提は `io-backends` で、`net-windows` の段の最初のコミット (5節の N-0) が、この付録から段の計画書を作る。io-fastpath の本体と食い違うときは本体に従う。

- 核の形 (trait、表、`Drop` の表、`buf.rs`、`sys/socket.rs`) は付録3のものである。io-backends の最小の IOCP の backend (`Port`、`wait`、`millis`、`type Socket = Infallible`) は付録3の13節にある。この段はその上にソケットを載せる。付録3の形がこの付録の前提と違えば、1.7節の表と `impl Backend for Iocp` の腕だけを読み替える。
- 試作は scratch の `net.rs` (371 行。核は最小の代役) で、`.#cross` の clippy を `-D warnings` で通した。試作は前の形で、`after_read_packet`、`probe`、`Rig`、`drain_broken` で箱を放す形、`millis` の切り詰めは後で足した設計で、試作に入れていない。スタイルの lint 3つは試作の先頭で許したので、実装では書き直す。
- docs の節の名前は io-fastpath と io-backends の後のもので引く。

## 1. 決めたこと

### 1.1 W-1〜W-12

- **W-1 読みはカーネルに buffer を渡さない。** `Read` の `start` はブロックしない `recv` を先に試す (socket2 の `Socket::recv`。重なり合わない呼び出し)。終われば `Start::Done`。10035 (`WSAEWOULDBLOCK`) なら長さ 0 の重なり合った `WSARecv` を出して `Pending`。packet が届いたら、`wait` の中で `after_read_packet` (1.4節) の判定に従う。成功の packet ならもう一度 `recv` を呼び、データか EOF か誤りで `Finished` を積む。この `recv` が 10035 なら、取り消しを頼まれていなければ長さ 0 の `WSARecv` を出し直して保留のままにし、頼まれていれば `cancelled: true` で終える。packet が誤り (995 を除く) なら `recv` を呼ばず、その誤りを写して終える。`Lent` に触るのは呼び出しの間だけ続く `recv` だけである。この出し直しは G-2 の文のとおり backend の仕事である (1.9節)。
- **W-2 書きも先に試す。** `Write` の `start` はブロックしない `send` を先に試す。`Ok(n)` は `Start::Done(Ok(Size(n)))` で、短い書きの続きは核が `start` を呼び直して出す。10035 なら残りを重なり合った `WSASend` で出す。カーネルが VM のヒープを持つのはこの `WSASend` の間だけで、読むだけである (書きの貸しは一意を求めない。本体 D-5)。
- **W-3 受けも先に試す。** `Accept` の `start` はブロックしないリスナーで `accept` を先に試し、backlog があれば `Start::Done(Ok(Accepted))`、10035 なら `AcceptEx` を出す。`accept` と `AcceptEx` の 10054 は `ConnectionAborted` にし、核が出し直す。`Connect` はいつも `ConnectEx`。`accept` と `AcceptEx` の併用は N-1 が確かめ、崩れたら N-2 を書く前に `Accept` を `AcceptEx` だけにする。
- **W-4 重なり合った呼び出しの結果はいつも packet で読む。** 同期に失敗した呼び出し (`WSAGetLastError() != WSA_IO_PENDING`) だけを `Start::Done(Err)` にし、同期の成功も `Pending`。成功のときに packet を省くモードは使わない。
- **W-5 `Readable` は長さ 0 の `WSARecv` で、読みの記憶を IOCP の側で持つ。** ソケットの型を `IocpSocket { socket: Socket, readable: Cell<bool> }` にする。初めは `false`。`recv` が 10035 なら `false`、ほかの結果 (データ、EOF、誤り) なら `true`。長さ 0 の `WSARecv` の packet が 995 以外で届いたら `true`。記憶が `false` の `Readable` は長さ 0 の `WSARecv` を出す。記憶が `true` の `Readable` は `MSG_PEEK` で1バイト覗いて確かめ、記憶だけで `Done` にしない (1.8節、G-19)。書きの記憶は持たない。
- **W-6 箱は「カーネルが持つ間」だけ記録の `Slot` に置き、packet で取り戻した箱は backend が使い回す。** 箱 `OpState` はカーネルに渡す間だけ `InFlight` 型で `IocpSlot::op` に入る。`InFlight` の `Drop` はいつも abort する (契約違反でしか届かない)。packet で取り戻した箱と、同期に失敗した呼び出しの箱は `Box<OpState>` に戻して `free: Vec<Box<OpState>>` に積み、次の重なり合った呼び出しで使い回す。`drain_broken` で終わりを確かめた箱は使い回さずに放す (1.5節)。不変条件は「`slot.op` が `Some` ⇔ カーネルが持つ」である。
- **W-7 `Drop` の待ちと abort の対象は変わらない。** 長さ 0 の `WSARecv` でもカーネルは `OVERLAPPED` に書くので、`KERNEL_WRITES_AFTER_CANCEL = true` のままで、待ちきる対象は `slot.op` が `Some` の記録すべて (`Readable`、`Read`、`Write`、`Accept`、`Connect`) である。変わるのは、カーネルが触る VM のヒープが `WSASend` の読みだけになることである。`drain_broken` の 1 秒と abort、`CancelIoEx` の失敗で壊れた状態、`Box::leak` と `mem::forget` を書かないこと、SKIP のモードを使わないことは本体 D-9 のまま。
- **W-8 ソケットはどれもブロックしない設定にする。** W-1〜W-3 がこれに頼る。結び付けの関数 `attach(&Port, Socket) -> io::Result<IocpSocket>` が `set_nonblocking(true)`、`CreateIoCompletionPort(socket, port, SOCKET_KEY, 0)`、`SetFileCompletionNotificationModes(socket, FILE_SKIP_SET_EVENT_ON_HANDLE)` の順に呼び、`listen`、`socket`、`accept` と `AcceptEx` で受けたソケットのすべてがこれを通る。どれかの失敗はソケットを作る関数の `Err` にする。関数は `&Port` を受ける (生の `HANDLE` を受ける公開の関数は clippy の `not_unsafe_ptr_arg_deref` で落ちる)。`sys/socket.rs` の `listener` と `stream` は io-backends で全 OS に `set_nonblocking(true)` を呼ぶ形になっている。net-windows はこのファイルに触れない。
- **W-9 誤りの写しは1.4節の表に従う。** 先に試す呼び出しは WSA の番号を直に返す。packet の誤りは `WSAGetOverlappedResult` で読み、`normalize` の4行を通す。
- **W-10 io-backends の最小の IOCP は `type Socket = std::convert::Infallible` にする。** この段で `IocpSocket` に替える (付録3の13節)。
- **W-11 遠い期限で回らない。** `wait` の `Some(d)` は `min(ceil(d / 1 ms), INFINITE - 1)` ms にする (付録3の13節の `millis`)。核が期限まで待ち直す。
- **W-12 net-windows は N-0〜N-4 の5コミット** (5.2節)。N-1 が前提を Windows の CI で確かめるテストだけのコミット、N-2 がコードと docs と単体テスト、N-3 が結合テストを Windows で流し始める作業、N-4 が閉じる作業である。

### 1.2 型 (N-2 の後の `src/sys/windows/iocp.rs`)

```rust
pub struct IocpSocket { socket: Socket, readable: Cell<bool> }   // Backend::Socket
#[repr(C)]
struct OpState {
    ovl: OVERLAPPED,                        // 先頭。packet の lpOverlapped をこの型の番地として読む
    key: OpKey,                             // packet から table.io_mut(key) を引く
    flags: u32,                             // WSARecv の lpFlags
    addrs: [MaybeUninit<u8>; 2 * ADDR_LEN], // AcceptEx の出力と ConnectEx の入力。ADDR_LEN = size_of::<SOCKADDR_STORAGE>() + 16
}
struct InFlight(*mut OpState);              // Box::into_raw で作る。Drop は鍵を書いて abort
#[derive(Default)]
pub struct IocpSlot { op: Option<InFlight>, accept: Option<Socket>, cancel_requested: bool }  // Backend::Slot
pub struct Iocp { port: Arc<Port>, entries: [OVERLAPPED_ENTRY; 128], free: Vec<Box<OpState>>, failed: Option<io::Error>, drained: bool }
```

- ソケットは `wait`、`cancel`、`drain_broken` のどれでも記録の `io.socket()` から引く。`ConnectEx` の番地は `SockAddr` を `addrs` に写して渡し、完了まで箱の中に生かす (`name` の寿命は文書に書かれていない)。
- 箱は生の番地で持つ。核の表は記録を `HashMap` の中で動かすので、`Box` のまま持つと記録が動くたびに一意の借用が付け直され、カーネルに渡した番地と食い違う。カーネルが書く間は、箱に Rust の参照を作らない。
- `free` は `#[allow(clippy::vec_box)]` を要する。確保を使い回すために `Box` のまま持つ、と理由を書く。
- `InFlight::reclaim(self) -> Box<OpState>` は `unsafe fn` で、「カーネルが書き終えたと確かめた後だけ呼ぶ」を SAFETY に書く。`ManuallyDrop::new(self)` から番地を取り出して `Box::from_raw` する (試作で `grep -n "forget\|Box::leak"` が0行)。
- `InFlight` の doc に書くこと: 保留の操作を持つ `Table` を抱えたまま panic が巻き戻ると process は abort しうる。`wait` の中の `debug_assert!` が落ちたときも同じで、panic の hook が文言を先に標準エラーに書くので診断は残る。カーネルが書きうる箱を放さないための意図した振る舞いである。

### 1.3 箱の寿命

| 時点 | 箱 | `slot.op` | カーネルが触る VM のヒープ |
|---|---|---|---|
| 先に試した `recv`、`send`、`accept` が終わった (`Start::Done`) | 使わない | `None` | なし (呼び出しの間だけ) |
| `Readable` が覗きで `Start::Done` (1.8節) | 使わない | `None` | なし |
| 重なり合った呼び出しを出す直前 | `free` から取る (空なら確保)。`ovl`、`key`、`flags` を埋め直す | — | — |
| その呼び出しが同期に失敗した (知らせは来ない。WSARecv の Return value) | すぐ `free` に戻す | `None` | なし |
| 同期の成功か `WSA_IO_PENDING` | `InFlight` | `Some` | `Write` の `WSASend` だけが `Lent` を読む |
| packet を取り出した | 結果を読んでから `free` に戻す | `None` | なし |
| `Read` の packet の後の `recv` が 10035 (取り消しなし) | `free` から取った箱で長さ 0 の `WSARecv` を出し直す | `Some` | なし |
| `cancel` | `CancelIoEx` を出すだけ | `Some` のまま | 変わらない |
| 核が記録を落とした | `slot.op` が `Some` なら `InFlight::drop` が鍵を書いて abort | — | — |
| `drain_broken` で終わりを確かめた | `free` に戻さずその場で放す | `None` | なし |
| `drain_broken` で 1 秒たっても終わらない | 残った鍵を書いて abort。放さず漏らさない | `Some` | — |
| backend が落ちる | `free` の `Box` を放す (どれもカーネルの手を離れている) | — | — |

### 1.4 結果の読み方と誤りの写し

- 先に試す呼び出し: socket2 の `recv`、`send`、`accept` の `Err` は `io::Error::last_os_error()` で WSA の番号そのもの (socket2 0.6.5 の `sys/windows.rs` の `syscall!`)。socket2 の `recv` は `WSAESHUTDOWN` を `Ok(0)` に写す。
- packet: 長さと成否は `WSAGetOverlappedResult(io.socket(), ovl, &mut n, FALSE, &mut flags)` で読み、`normalize` (64→10054、1236→10053、1225→10061、121→10060) を通す。NTSTATUS を `RtlNtStatusToDosError` で写す形は使わない (reset が `ERROR_NETNAME_DELETED` (64) になり、std はこれを `Uncategorized` に写す)。`WSAGetOverlappedResult` が WSA の番号を返すことは推定なので `normalize` を挟む。CI で別の番号を見たら同じ形で行を足す。
- `Read` の packet の後の手順は純粋な関数 `fn after_read_packet(packet: io::Result<u32>, recv: impl FnOnce() -> Option<io::Result<usize>>, cancel_requested: bool) -> Step` に切り出し、`enum Step { Finish(io::Result<usize>, bool /* cancelled */), Rearm }` を返す。`wait` は `Rearm` のときだけ長さ 0 の `WSARecv` を出し直す。表の各行を単体テストで確かめる。

| 出どころ | 値 | backend の結果 | 種類と VM の扱い |
|---|---|---|---|
| 先に試す `recv` | 10035 | 記憶を下ろし、長さ 0 の `WSARecv` を出す | 保留 |
| 先に試す `send` | 10035 | 残りを `WSASend` で出す | 保留 |
| 先に試す `accept` | 10035 | `AcceptEx` を出す | 保留 |
| 先に試す `accept`、`AcceptEx` | 10054 (受ける前に相手が切った) | `io::Error::from(ErrorKind::ConnectionAborted)` | 核が同じ鍵で出し直す |
| `Read` (`recv` か packet)、`Write` | 10054 / 10053 / 10058 | そのまま | `ConnectionReset` / `ConnectionAborted` / `BrokenPipe` (code 2) |
| `Readable` の packet | 995 以外のどれでも | `Ok(Size(0))`、記憶を立てる | 次の `Read` が誤りを言う |
| `ConnectEx` | 10061 / 10060 | そのまま | `ConnectionRefused` (code 3) / `TimedOut` (code 5) |
| `listen` の bind | 10048 | そのまま | `AddrInUse` (code 4) |
| 重なり合った呼び出しの同期の失敗 | 10035 (保留の要求が多すぎる) | そのまま | 写さない。まれ (推定) |
| どれでも | 995 で `cancel_requested` | `Err(from_raw_os_error(995))`、`cancelled: true` | 核が `cancelled` に替える |
| どれでも | 995 で `cancel_requested` でない | `Err(io::Error::other("the operation was aborted"))`、`cancelled: false` | std は 995 を `TimedOut` に写すので、OS の時間切れと読まれないよう替える |
| `Read` の成功の packet の後の `recv` | データ、EOF、誤りで `cancel_requested` | その結果、`cancelled: false` | 取り消しとの競走。G-5 のとおり核が結果を受ける |
| 同 | 10035 で `cancel_requested` | `Err(from_raw_os_error(995))`、`cancelled: true` | 出し直さない |
| 同 | 10035 で `cancel_requested` でない | `Step::Rearm` | 保留のまま。`Finished` を積まない |
| `Readable`、`Write`、`Accept`、`Connect` の packet | 成功で `cancel_requested` | 成功の結果、`cancelled: false` | 競走。核が続きを止める |

定数の名前 (`WSAECONNABORTED` など) は `src/sys` に書かず数で書く (G-2 の grep)。成功のときに packet を省くモードの定数の名前もコードとコメントに書かず、「成功のときに packet を省くモード」と書く。

### 1.5 `wait`、`cancel`、`drain_broken`

- `wait(timeout, table, out)`: 冒頭で `failed.take()` があれば OS を待たずに `Err`。`debug_assert!(!self.drained)`。起こしの旗と `GetQueuedCompletionStatusEx` は付録3の13節のまま (`None` は `INFINITE`、`Some(d)` は `millis`、128 項目が満ちたら待ちの長さ 0 で取り直す)。I/O の項目は `lpOverlapped` から `key` だけを読み (カーネルはもう書かない)、`table.io_mut(key)` で記録を引き、`slot.op` の番地が `lpOverlapped` と等しいことを `debug_assert!` で確かめてから `take()` し、`WSAGetOverlappedResult` で結果を読み、箱を `free` に戻し、1.4節で `Finished` を積むか出し直す。表は歩かない (G-4)。戻り値は `out` に1つでも積めば `Woke::Finished`、起こしだけなら `Woken`、ほかは `TimedOut` (出し直しだけの packet もここに入る。核が待ち直す)。
- `cancel(key, op, out)`: `slot.cancel_requested = true`。保留の記録の `slot.op` はいつも `Some` で (`debug_assert!`)、`CancelIoEx(socket, ovl)` を出す。`ERROR_NOT_FOUND` は捨てる (終わって packet が積まれたことを表す)。ほかの失敗は `failed` に覚える (先に覚えた誤りがあれば残す)。`out` には積まない。取り消せない操作はカーネルの手の中に残るので、`Finished` の誤りにして核に記録を放させることはしない。その後は壊れた driver の道で、`quiesce` は保留を残して戻り、`Machine` は poisoned になり、`Drop` は `drain_broken` に入る。
- `drain_broken(table)`: 表の記録で `slot.op` が `Some` のものに `CancelIoEx` を出し (失敗は捨てる)、合わせて 1 秒まで 1 ms ずつ `WSAGetOverlappedResult(.., FALSE, ..)` が `WSA_IO_INCOMPLETE` 以外を返すかを確かめ、終わった箱を `reclaim` してその場で放す。残れば鍵を `let _ = writeln!(std::io::stderr(), …)` で書いて `std::process::abort()`。最後に `drained = true`。
- `drain_broken` の後の不変条件: `drain_broken` を呼ぶのは壊れた driver の `Drop` だけで、核は壊れた後に `backend.wait` も `start` も呼ばない (核の `poll` は壊れていれば backend を待たない)。確かめた箱の packet はポートに残るが、ポートを読むのは `Iocp::wait` だけで、それはもう走らない。`Arc<Port>` は waker として残りうるが、waker は packet を積むだけで読まない。念のため箱は `free` に戻さず放し、後で同じ番地が使い回されて古い packet と重なる道を作らない。`drain_broken` の doc にこの2文を書く。
- 壊れた状態は、`GetQueuedCompletionStatusEx` が `WAIT_TIMEOUT` 以外で失敗したときと、`CancelIoEx` が `ERROR_NOT_FOUND` 以外で失敗したときだけで、どちらも欠陥でしか起きない (推定)。`CancelIoEx` から 1 秒以内に終わらない操作は、実際にはない (推定)。
- `Drop` (壊れていない) と `quiesce`: 核の手順 (付録3の2.6節。release は `poll(None)`、debug は見張りの残りを期限)。packet が箱を `free` に戻すので、待ちきった後の記録の `slot.op` はどれも `None`。
- `closesocket` と保留の操作: ソケットは `Rc` で記録が持ち、記録は `Finished` の後にしか落ちない。受けるソケットは `slot.accept`、接続のソケットは `Io::Connect` の `Rc` にある。そのため保留の操作があるソケットは閉じられない。

### 1.6 1つの操作あたりの OS の呼び出し (設計から数えた。測っていない)

| 操作 | この設計 | 前回の形 (いつも重なり合った呼び出し) |
|---|---|---|
| 書けるソケットへの `write_bytes` | `send` 1。`Submit::Done` で park なし | `WSASend` 1 と待ちの `GetQueuedCompletionStatusEx`、park と再開 |
| データのある `read_chunk` (空きあり) | `recv` 1。park なし | `WSARecv` 1 と待ち、park と再開 |
| 待つ `read_chunk` | `recv` (10035)、`WSARecv` (長さ 0)、`GetQueuedCompletionStatusEx` (待ちの1回に共有)、`recv` の 4 | `WSARecv` 1 と待ちの 2 |
| 記憶が立っていてデータがあるときの `Readable` | `recv(MSG_PEEK)` 1 | 長さ 0 の `WSARecv` と待ち |
| 読み尽くした後の `Readable` と続く読み | `recv(MSG_PEEK)` (10035)、`WSARecv`、待ちの 3 で `Done`、続く `Read` の `recv` 1 | 同じく待つ |
| backlog のある `accept` | `accept` 1 と `attach` の 3 | `AcceptEx` と待ち、受けたソケットの設定 |

`WSAGetOverlappedResult` は `fWait = FALSE` では `OVERLAPPED` を読むだけと見る (推定)。待つ読みはこの設計のほうが2回多い。libuv も同じ回数を払う。データのある読みと書きで park を払わない利益と、待つ読みの2回の得失は Windows で測っていない (6節の U-10)。

### 1.7 付録3の形への依存 (ここだけに置く)

付録3の13節の末尾の表 (「IOCP の backend が依る核の点」) がこの設計の依る点のすべてである。付録3の形が変われば、1.2節の型と `impl Backend for Iocp` の腕だけを読み替える。

### 1.8 `Readable` の答え方 (覗く形)

`iocp.rs` の `Readable` の腕を1つの関数 `fn probe(&mut self, key, socket, slot) -> Start<IocpSocket>` にする。

- 記憶が `true`: `socket.peek(&mut [MaybeUninit::uninit(); 1])` (`MSG_PEEK` の `recv`) で確かめる。`Ok(_)` (データか EOF) なら `Start::Done(Ok(Size(0)))`。10035 なら記憶を下ろして長さ 0 の `WSARecv` を出し `Pending`。誤りなら `Start::Done(Err(normalize(e)))` (付録3の `ops::probe` と同じ)
- 記憶が `false`: OS を覗かずに長さ 0 の `WSARecv` を出す
- 呼び出しの数: データが着いているときは `recv(MSG_PEEK)` 1。読み尽くした後は `recv(MSG_PEEK)` (10035)、`WSARecv`、待ちの 3 で終わり、待つ間 VM は buffer を広げない (G-19 の「読み尽くした後は `Pending`」)
- 崩れるのは同じソケットに読みを2つ重ねたときだけで、VM はそうしない
- `Readable` の packet の誤りは `Ok(Size(0))` にし記憶を立てる (A-4 により誤りはソケットに残り、次の覗きか `Read` が言う)
- architecture.md には「`Readable` は記憶が立っていれば覗いて確かめ、下りていれば長さ 0 の受けで待つ」の1文を書く

### 1.9 G-2 と長さ 0 の `WSARecv` の出し直し

G-2 の文のとおりである: 「1つの操作の中で OS の待ちを張り直すこと (readiness の `EAGAIN` の後に次の辺を待つ、IOCP の packet の後の `recv` がブロックするときに長さ 0 の受けを出し直す) は続きに当たらず、backend の仕事である」。W-1 の `Step::Rearm` はこれに当たる。核の続き (短く終わった書きの残り、受けの出し直し、接続の次の番地) は backend から出さない。付録3の2.3節の `start` の doc の2文はそのまま。

### 1.10 前提の確かめ (N-1)

設計の重い前提は N-1 の Windows の CI で、コードを書く前に確かめる。N-1 は `src/sys/windows/assumptions.rs` (`#[cfg(all(test, windows))]`、`#![allow(unsafe_code)]`) に windows-sys を直に呼ぶテストだけを置く。

| テスト | 確かめる前提 | 崩れたとき |
|---|---|---|
| `a_zero_length_recv_waits_and_leaves_the_data` | A-2: 長さ 0 の重なり合った `WSARecv` はデータが来るまで packet を積まず、来たら成功の packet を積み、データを残す (続く `recv` が全部読む) | 設計の前提が崩れる。N-2 に入らず、`Read` を「buffer を渡す `WSARecv`」に戻す計画の変更を起こす |
| `a_zero_length_recv_completes_on_eof_and_reset` | A-2: 相手が閉じても reset しても packet が積まれる | 同じ |
| `a_zero_length_recv_completes_even_if_the_data_is_taken` | データが着いた時点で長さ 0 の受けが終わり、その後に別の `recv` がデータを取っても packet は届く | `a_read_whose_data_was_taken_waits_again` を、packet が来ない場合も受ける形にする |
| `a_nonoverlapped_recv_without_data_would_block_and_queues_no_packet` | A-3: 結び付けた `FIONBIO` のソケットの重なり合わない `recv` と `accept` は 10035 を返し、packet を積まない | W-1〜W-3 の前提が崩れる。計画の変更 |
| `a_reset_stays_on_the_socket` | A-4: reset の後の `recv` が続けて 10054 | `Readable` の packet の誤りを `Err` で返す形にする |
| `an_accepted_socket_takes_overlapped_receives` | A-5: `accept` で受けて結び付けたソケットに長さ 0 の `WSARecv` が出せる | `Accept` を `AcceptEx` だけにする (W-3) |
| `accept_works_while_an_acceptex_is_pending` | A-5: `AcceptEx` を保留にしたリスナーで `accept` が backlog を受け、取り消した `AcceptEx` が 995 で終わる | 同じ |

N-4 は、これらの事実が N-2 と N-3 のテストで確かめられた後に `assumptions.rs` を消す (5.4節の対応の表)。1つの事実を2か所で確かめないためである。

## 2. 理由

- 長さ 0 の `WSARecv` とブロックしない `recv` の併用は libuv の Windows の TCP の読みそのものである (libuv v1.x の `src/win/tcp.c` を取得して読んだ。`uv__tcp_queue_read` はいつも長さ 0 の重なり合った `WSARecv` を出し、`uv__process_tcp_read_req` は packet の後に `lpOverlapped` を NULL にした `WSARecv` を最大 32 回繰り返し、`WSAEWOULDBLOCK` を 0 バイトの読みとして長さ 0 の読みを出し直す。`uv__tcp_set_socket` は `FIONBIO` を立てる)。書きの先の試みも libuv の `uv__tcp_try_write` が重なり合わない `WSASend` で行う。`ConnectEx` も同じ `FIONBIO` のソケットで出す。
- Microsoft の文書: `lpOverlapped` と `lpCompletionRoutine` が NULL なら重なり合わないソケットとして扱われ、ブロックしないソケットでデータがなければ `WSAEWOULDBLOCK`。同期に失敗した重なり合った呼び出しには知らせがない。長さ 0 の受けの振る舞いは文書にないので、N-1 で最初に確かめる。
- 先に試す形の利益: データのある読みと書けるソケットへの書きが呼び出し1回で `Submit::Done` になり、park も packet もない (本体 D-3 の速い道)。待つ読みは2回増える (1.6節)。性能の得失は測っていない。
- 読みの buffer をカーネルに持たせない: 待つ読みがカーネルの中に VM のヒープを固定しない (前回の形では保留の `WSARecv` ごとに貸した buffer がカーネルにあった)。
- W-6 の箱: 「カーネルが持つ間だけ `Slot` に置く」と、前回の `in_flight` の旗の場合分けが消え、`InFlight` を落とすことがそのまま契約違反になる。使い回しで、保留の操作の最大の数を超えて確保しない。
- W-7: 長さ 0 の `WSARecv` でも `OVERLAPPED` の `Internal` と `InternalHigh` はカーネルが完了のときに書く (推定)。読みの箱も待ちきるか abort するしかない。
- 成功のときに packet を省くモードを使わない: 非 IFS の LSP がある機械ではこのモードが正しく動かず、eml はそれを実行時に見分けない。効かなかったときの packet は放した箱を読む。libuv も非 IFS の LSP を調べたときだけ立てる。費用は、同期に成功した呼び出し1回につき packet を1つ取り出すことだけである。モードは一度立てると外せない。
- Windows の `SO_REUSEADDR` は使っている番地への bind を許し、2つ目のリスナーが `AddrInUse` にならない。std も Windows では立てない。
- ループバックへの接続の設定: Windows は拒まれた接続でも SYN を送り直し、閉じたループバックのポートへの接続が約 2 秒かかる。送り直しを 0 にすると約 0.3 ms で拒まれる (curl の mailing list の測定)。libuv の `uv__tcp_try_connect` が同じ設定を、ループバックで Windows 10 1709 以降のときに出している。ループバックには失われる packet がないので、送り直しを消しても失うものはない。`localhost` が `::1` を先に返し、相手が IPv4 だけで待つときの名前での接続も 2 秒止まらない。

## 3. 退けた案

- 読みも重なり合った `WSARecv` に buffer を渡す (前回): カーネルが待つ間 VM のヒープを持ち、データのある読みも packet と park を払う。
- 長さ 0 の `WSARecv` を先に出す (libuv の形): データのある読みが3回と park になる。
- packet の後の 10035 を核に返す (`Outcome::Again`): 核に IOCP だけの結果の種類を足す。G-2 の Why (核に OS 固有の結果の種類を足さない) に反する。
- 記憶が立っていれば覗かずに `Done` を返す: 読み尽くした後の `Readable` が `Done` になり、`Readable` の契約と G-19 に反する。
- 記憶を持たず `Readable` をいつも長さ 0 の `WSARecv` にする: 空きのない buffer の読みでデータがあっても park と packet を払う。
- 書きの記憶を持つ: 省けるのは試しの `send` 1回だけ。
- `Accept` をいつも `AcceptEx` にする (前回): backlog のある受けも packet と park を払う。N-1 で併用が崩れたときの形として残す。
- 箱を backend の `HashMap<OpKey, 箱>` に置く: packet ごとに表を2回引く。
- `drain_broken` で確かめた箱も `free` に戻す: 古い packet の番地と使い回しが重なる道が残る。壊れた後は使い回す利益がない。
- 前提を CI の最後に確かめる: 崩れたときに W-1、W-5、W-6、W-7 とテストを書き直す。テストだけのコミットを先に置けば、崩れても書き直すコードがない。
- 単体テストで保留を残したまま `Table` を落とす: `InFlight::drop` が abort し、成功するテストも落ちる。`Rig` で取り消して待ちきる。
- 遠い期限を `INFINITE` にする: 期限が 49.7 日を超える待ちを期限なしと区別できなくなる。
- IOCP の `CancelIoEx` の失敗を捨てる: 取り消せない操作の記録が `quiesce` を終えられず、`Drop` が待ち続ける道が残る。

## 4. 仮定

| ID | 仮定 | 確かめ / 崩れたとき |
|---|---|---|
| A-1 | 核と trait は付録3の2.3節と2.6節の形 | 1.7節の表だけを読み替える |
| A-2 | 長さ 0 の重なり合った `WSARecv` はデータか EOF か誤りまで待ち、データを消費しない (libuv が頼る) | N-1 の2本 |
| A-3 | 結び付けた `FIONBIO` のソケットの重なり合わない `recv`、`send`、`accept` は packet を積まず、データがなければ 10035 (非重なりの扱いは文書にある。packet を積まないことは推定) | N-1 |
| A-4 | reset を一度返したソケットは以後の `recv` でも 10054 (推定) | N-1 |
| A-5 | `accept` で受けたソケットは重なり合った呼び出しを受け、`AcceptEx` を保留にしたリスナーでも `accept` が動く (推定) | N-1 |
| A-6 | `WSAGetOverlappedResult` は packet の WSA の番号を返す | N-2 の `a_reset_peer_is_connection_reset`。違えば `normalize` に行を足す |
| A-7 | io-fastpath と io-backends の後、ソケットを使う `tests/real.rs` と `api.rs` のテストは `#[cfg(unix)]` を付けている | N-0 の列挙のコマンドがそのまま使える |
| A-8 | windows-2025 は `TCP_INITIAL_RTO_NO_SYN_RETRANSMISSIONS` を受け付ける (Windows 10 1709 以降) | 崩れたら種類2の範囲 (c) を使い、vm.md の文は「Windows 10 1709 以降」と書く |
| A-9 | 読まない相手への書きが、先の `send` の後で `WSASend` の保留になる | 崩れたら種類2の範囲 (b) |
| A-10 | 今の Windows に非 IFS の LSP はない | SKIP のモードを使わないので、崩れても安全は保つ |
| A-11 | 箱の生の番地の読みは Miri の対象外 | 人が読んで確かめる |

## 5. 実装の作業

### 5.1 io-backends の中の最小の IOCP

付録3の13節と18節の T-5 にある。この段の前提である。

### 5.2 net-windows のコミット

**N-0 `Plan net-windows`**
- `docs/plans/<日付>-net-windows.md` にこの設計と次のテストの変更を書く
  - 種類1 (削除): `crates/eml_io/tests/real.rs` の `listen_accept_and_connect_are_unsupported`、`crates/eml_vm/tests/api.rs` の `a_real_listen_is_not_available` (N-2)。N-1 で足した `assumptions.rs` の7本 (N-4。同じ段で足したテストで、事実は5.4節の対応の表のテストが引き継ぐ)
  - 種類1 (Windows で流し始める。成否が「流さない」から「流して通す」に変わる): 次のコマンドで列挙した `#[cfg(unix)]` の項目のうち、5.4節の「unix のまま」を除くものを名前で1件ずつ (N-3)。`git grep -n -A1 '#\[cfg(unix)\]' -- crates/eml_io/tests/real.rs crates/eml_vm/tests/api.rs`。この出力と、N-3 の後に残るべき行 (「unix のまま」のテストと、それだけが使う補助と import) の一覧を計画に写す。5.4節の表の名前は既知のもので、列挙の出力が正である
  - 種類3: `a_wake_from_another_thread_leaves_a_wait_working` の `#[cfg(windows)]` の仕事の枝を消し、unix の枝をすべての OS で使う (N-3)
  - 種類2 の範囲 (CI で起きたときだけ使う): (a) 誤りの種類の集合を確かめるテストに、VM が同じ行で扱う種類 (`ConnectionReset`、`ConnectionAborted`、`BrokenPipe`) を足してよい (`a_write_to_a_closed_peer_fails_with_reset_or_broken_pipe` のため)。(b) 保留の書きを前提にするテスト (`quiesce_drains_a_pending_probe_a_full_write_and_a_running_job`、`a_cancelled_long_write_stops_resubmitting`、`a_long_write_completes_once_with_its_whole_length`) は、`SO_SNDBUF` と相手の `SO_RCVBUF` を小さくするか、保留になるまで書き足す形にしてよい。(c) ループバックの設定が効かず拒まれた接続が見張りを越えるときは、接続のテストの見張りだけを 5 秒にしてよい
- 完了: `cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` → 通る

**N-1 `Check the Windows socket assumptions`**
- 目的: 1.10節の前提を、設計のコードを書く前に windows-2025 で確かめる
- 触るファイル: `crates/eml_io/Cargo.toml` (features に `"Win32_Networking_WinSock"`、`"Win32_Storage_FileSystem"`、`"Win32_System_WindowsProgramming"` を足す。試作がこの5つの組で通った。`WSA_IO_INCOMPLETE` は WinSock の module にある)、`src/sys/windows/mod.rs` (`#[cfg(test)] mod assumptions;`)、`src/sys/windows/assumptions.rs` (新しい。`#![allow(unsafe_code)]`)、`tests/unsafe_code.rs` (`LISTED` に足す)
- 作業: 1.10節の7本。ソケットは std の `TcpListener` と `TcpStream` から作り、`set_nonblocking(true)` と `CreateIoCompletionPort` で付録3の `Port` に結び付け、`GetQueuedCompletionStatusEx` を期限つき (データを待つ側は 1 秒、来ないことを見る側は 50 ms) で読む。`OVERLAPPED` はテストの中の `Box` に置き、packet を取り出してから落とす。取り出せなかったら `CancelIoEx` を出して packet を待ってから `panic!` する
- 完了の基準
  - `.#cross` の clippy → 0。Linux で `cargo nextest run` → 失敗なし (Windows だけのテストなので Linux では増えない)
  - CI の `test (windows-2025)` が緑で、`eml_io sys::windows::assumptions::` の7本が `PASS`
- 崩れたとき: 1.10節の「崩れたとき」の列に従い、N-2 に入る前に計画を直す (計画の変更は計画の更新として記録する)
- 任せること: テストの中の補助の形

**N-2 `Run sockets on the IOCP backend`**
- 目的: 1節の形 (W-1〜W-9、1.8節) を入れ、Windows で `Std.Net` が動くようにする
- 触るファイル: `src/sys/windows/iocp.rs` (型、`impl Backend for Iocp` の全部の腕、箱、`free`、`after_read_packet`、`probe`)、`src/sys/windows/wsa.rs` (新しい。`#![allow(unsafe_code)]`。核の型を受けない薄い包み: `attach`、`zero_recv`、`wsa_send`、`accept_ex`、`connect_ex`、`cancel_io`、`overlapped_result`、`update_accept`、`update_connect`、拡張の関数の `OnceLock`、ループバックの `SIO_TCP_INITIAL_RTO`、`normalize`、`accept_error`)、`src/sys/windows/iocp/tests.rs` (新しい)、`tests/unsafe_code.rs`、`crates/eml_io/tests/real.rs` と `crates/eml_vm/tests/api.rs` (種類1 の2本を消す)
- 作業
  - `listen(addr)`: `sys/socket.rs` の `listener` の後に `attach`。ポートは `local_addr`。socket2 は Windows で `WSA_FLAG_OVERLAPPED` と継がない印を付ける
  - `socket(peer)`: `sys/socket.rs` の `stream(peer)`、同じ domain の未指定の番地のポート 0 に bind (`ConnectEx` は bind 済みを求める)、相手がループバックなら下のループバックの設定、`attach`
  - ループバックの設定: `WSAIoctl(s, SIO_TCP_INITIAL_RTO, &TCP_INITIAL_RTO_PARAMETERS { Rtt: TCP_INITIAL_RTO_DEFAULT_RTT as u16, MaxSynRetransmissions: TCP_INITIAL_RTO_NO_SYN_RETRANSMISSIONS as u8 }, ..)`。失敗は捨てる (古い Windows は今の振る舞いのまま)。windows-sys の定数は `u16` の 65534 で、欄は `u8` なので `as u8` で 254 (C の `(UCHAR)-2`)
  - `start`: W-1、W-2、W-3、1.8節。`AcceptEx` の受けるソケットは `Socket::new(リスナーの domain, STREAM, TCP)` で作って `slot.accept` に置き、`AcceptEx(listener, accepted, addrs, 0, ADDR_LEN, ADDR_LEN, &mut ignored, ovl)` (`lpdwBytesReceived` は同期のときだけ入るので読まない)。`Connect` は `SockAddr` を箱の `addrs` に写して `ConnectEx(s, peer, len, null, 0, null_mut(), ovl)`。核が受けを出し直すときは新しい受けるソケットを作る
  - 拡張の関数: `AcceptEx` と `ConnectEx` の番地は `WSAIoctl(s, SIO_GET_EXTENSION_FUNCTION_POINTER, &GUID, ..)` で手元のソケットから取り、process 全体の `static ACCEPT_EX: OnceLock<AcceptExFn>` と `CONNECT_EX` に置く。`get()` が空なら取って `set()` し、失敗は覚えずその操作の `Start::Done(Err)` にする。関数の番地の型は `Send + Sync` なので `unsafe impl` は要らない
  - `wait`、`cancel`、`drain_broken`: 1.4節と1.5節。`AcceptEx` の packet の後は `SO_UPDATE_ACCEPT_CONTEXT` (`setsockopt(accepted, SOL_SOCKET, SO_UPDATE_ACCEPT_CONTEXT, &listener, size_of::<SOCKET>())`)、`attach`、`peer_addr` (IP でなければ `the peer has no IP address`)。`ConnectEx` の packet の後は `SO_UPDATE_CONNECT_CONTEXT` をして `Connected` (この設定がないと `shutdown` と `getpeername` が使えない)
  - `shutdown_write`、`set_nodelay`: socket2 の `shutdown(Write)` と `set_tcp_nodelay`
  - 名前の解決: 解決器は全 OS で組んであるので、触る所はない。`socket` が動けば名前での接続も動く
- docs (同じコミット。N-1 で前提を確かめた後に書く):
  - architecture.md「`eml_io` の内部」: 先に試す呼び出しと長さ 0 の `WSARecv`、読みの記憶と覗く `Readable` (1.8節の1文)、箱と `InFlight` と使い回し、packet だけで読むこと (SKIP のモードを使わない理由)、`CancelIoEx` の失敗を壊れた状態にすること、`drain_broken` の 1 秒と abort とその後に待たないこと、`wsa.rs` の行 (拡張の関数、`SO_UPDATE_*_CONTEXT`、作るときの結び付け)。結合テストの段落の Windows の `Unsupported` の文を消す
  - vm.md「時計と driver」: Windows の「まだソケットを扱わない」の文を消す。壊れた driver の箇条の「OS の待ちそのものが失敗したとき」に「と、OS が取り消しの要求を断ったとき」を足す。`Listen` に「Windows では `SO_REUSEADDR` を立てない (使っている番地への bind を許すため)」、`Connect` に「Windows では、ループバックの番地への接続で SYN を送り直さない。拒まれた接続がすぐ誤りになる」
  - stdlib.md「`Std.Net`」、status.md (`Std.Net` の Windows の文と「### Windows」の節)、testing.md (`api.rs` の段落)、async.md (「`IoDriver` と `eml_io`」、「ネットワークとシグナル」): Windows で `listen`、`accept`、`connect` が `NotAvailable` になる文と Windows の予告 (結び付け、`update_context`) を消す
- テスト: 5.4節の単体テスト 24 本と、後始末の型 `Rig`
- 完了の基準
  - `.#cross` の clippy → 0。Linux で `cargo nextest run` → 失敗なし
  - `git grep -n 'Unsupported' -- crates/eml_io/src/sys/windows` → 0行
  - `grep -rn "ECONNABORTED" crates/eml_io/src/sys` → 0行。`grep -rn "forget\|Box::leak" crates/eml_io/src` → 0行。`grep -rn "unsafe impl" crates/eml_io/src` → 0行。`grep -rln "process::abort" crates/eml_io/src` → `crates/eml_io/src/sys/windows/iocp.rs` の1行だけ
  - `grep -rn "FILE_SKIP_COMPLETION_PORT_ON_SUCCESS" crates/eml_io/src` → 0行。`grep -rn "Lent::new" crates/eml_io/src` → `buf.rs`、`real/mod.rs`、`test.rs` の行だけ。`grep -rn "from_raw_parts" crates/eml_io/src` → `buf.rs` だけ (G-11)
  - `git grep -n 'NotAvailable' -- docs/spec/stdlib.md docs/implementation/status.md docs/implementation/testing.md` の出力にネットの Windows の文がない (3つの文の消去を目で確かめる。ほかの `NotAvailable` の行は残る)
  - CI の4行 (`ubuntu-24.04`、`ubuntu-24.04-arm`、`macos-15`、`windows-2025`) が緑。windows-2025 の出力で `eml_io sys::windows::iocp::tests::` の 24 本と `assumptions::` の7本が `PASS`
- 任せること: `OpState` の欄の並び (先頭が `OVERLAPPED` は守る)、`drain_broken` の眠る長さ (既定 1 ms)、拡張の関数を取るソケット、`free` の上限 (既定なし)、`wsa.rs` と `iocp.rs` の関数の分け方 (核の型を読むのは `iocp.rs` だけ)
- 落とし穴
  - packet を取り出してから `free` に戻すまでに `WSAGetOverlappedResult` で結果を読み終える
  - `Rearm` の出し直しは `free` から取った箱で行い、`slot.op` に入れ直す
  - 先に試す `recv` は記憶を見ずに呼ぶ (記憶は `Readable` の答えにだけ使う)
  - `accept` で受けたソケットにも `attach` を通す。`SO_UPDATE_ACCEPT_CONTEXT` は `AcceptEx` で受けたソケットにだけ要る
  - `send` の長さは socket2 が `i32::MAX` で切り、`WSASend` は `u32::MAX` で切る。短い書きは核が続ける
- レビューで読む所 (単体テストがない): `drain_broken` の abort の道、`CancelIoEx` の失敗を覚えて `wait` が `Err` を返す道 (偽の `CancelIoEx` を差し込めない)

**N-3 `Run the network tests on Windows`**
- 目的: N-0 に挙げたテストの `#[cfg(unix)]` を外し、本物のソケットの組を Windows で流す
- 触るファイル: `crates/eml_io/tests/real.rs`、`crates/eml_vm/tests/api.rs` (印の削除、補助の関数と import の印の削除、種類3 の1件)。範囲 (a)(b)(c) を使ったら、その箇所
- docs: なし。範囲を使ったら計画とコミットの文に書く
- 完了の基準
  - Linux で `cargo nextest run` → 失敗なし。`.#cross` の clippy → 0
  - `git grep -n -A1 '#\[cfg(unix)\]' -- crates/eml_io/tests/real.rs crates/eml_vm/tests/api.rs` の出力が、N-0 の計画に写した「残るべき行」の一覧と同じ
  - `git grep -n 'cfg(windows)' -- crates/eml_io/tests crates/eml_vm/tests/api.rs` → 0行
  - `git grep -n '^-- windows: skip (' -- tests/ui` の出力が N-2 の後と同じ
  - CI の4行が緑。windows-2025 の出力で、`eml_io real::` のテストのうち「unix のまま」の名前がなく、ほかがすべて `PASS`
- 任せること: 範囲 (a)(b)(c) の使い方の細部

**N-4 `Close net-windows`**
- `src/sys/windows/assumptions.rs` と `mod assumptions;` と `LISTED` の行を消す (種類1。5.4節の対応の表)
- roadmap の `net-windows` の項を消し、ほかの項の「前提」から段の名前を消す。io-fastpath の T-0 がこの項に書いた `Terminate` を CTRL_CLOSE_EVENT と CTRL_SHUTDOWN_EVENT に写す文は、「その後の項目」の「非同期」に項目として移す。後に回した理由は「IOCP のソケットと別の話題で、コンソールの handler が戻ると系が 5 秒以内に process を終えるので、run を止める時間の決め方が要る (HandlerRoutine の文書)」、見直す条件は「Windows でサービスや閉じられるコンソールの中で eml を動かす使い道が出たとき」
- `docs/plans/` の計画を消す。`eml_io` とテストだけの変更なので benchmarks.md の表は替えない (roadmap の「進め方」の計測の対象の crate に `eml_io` は入らない)
- 完了: `git grep -n 'net-windows' -- docs CLAUDE.md ':!docs/plans'` → 0行。`git grep -n 'CTRL_CLOSE_EVENT' -- docs/future/roadmap.md` → 「その後の項目」の1行。`git grep -n 'assumptions' -- crates/eml_io` → 0行。CI の4行が緑

### 5.3 CI (windows-2025) でしか確かめられないこと

型の検査は `.#cross` の clippy で、`--all-targets` なので Windows の単体テストと `tests/real.rs` も検査されるが、link しないので流れない。git の worktree で flake の参照が止まるときは `nix develop "path:$PWD#cross" -c …` を使う。Miri は Windows の道を通らない。

1. 時間切れが `WAIT_TIMEOUT` で、起こしの packet と区別できる (付録3の `sys/tests.rs`)
2. SKIP のモードなしで、同期に成功した重なり合った呼び出しも packet を1つ積む (`a_zero_length_recv_with_data_still_completes_through_the_port`)
3. `CancelIoEx` の後の3つの終わり方と、取り消しの packet が届く
4. 拡張の関数を取れ、`SO_UPDATE_*_CONTEXT` の後に `peer_addr` と `shutdown` が動く
5. `WSAGetOverlappedResult` の誤りの番号 (`normalize` の要否)
6. ループバックの設定で拒まれた接続がすぐ終わる
7. `SO_REUSEADDR` なしで2つ目のリスナーが `AddrInUse` になる
8. 読まない相手への書きが、先の `send` の後で `WSASend` の保留になる
9. 長さ 0 の `WSARecv` の振る舞い (N-1)
10. 刻みの粗い待ちの下でも時間のテストが下限だけで通る
11. `FIONBIO` のソケットで、重なり合った `WSARecv`、`WSASend`、`AcceptEx`、`ConnectEx` が動く
12. 重なり合わない呼び出しが packet を積まない (N-1)
13. `accept` と `AcceptEx` の併用 (N-1)
14. reset が残る (N-1)
15. 遠い期限の待ちが回らない (`a_wake_ends_a_wait_with_a_far_deadline` が起こしで戻る)

実装者の流れ: Linux で全体のテスト、`.#cross` の clippy、push して CI の4行を見る。Windows で落ちたら `cargo nextest run --failure-output immediate-final` の出力で直す。

### 5.4 テストの表

`crates/eml_io/tests/real.rs`:

| テスト | 扱い | 種類 | コミット |
|---|---|---|---|
| 印のないテスト (付録3の18節の T-6 に挙げたもの、`an_op_on_an_unknown_handle_fails`) | io-backends の T-6 から IOCP の上で流れる | — | — |
| `a_wake_from_another_thread_leaves_a_wait_working` | `#[cfg(windows)]` の枝を消し、ループバックの `Readable` の枝をすべての OS で | 3 | N-3 |
| 基点の unix のソケットと解決のテスト 23 本 (`a_readable_probe_stays_pending_without_data`、`a_read_after_a_probe_returns_the_bytes`、`a_write_is_read_by_the_peer`、`a_cancelled_probe_completes_with_an_error`、`a_closed_peer_reads_eof`、`quiesce_drains_a_pending_probe_a_full_write_and_a_running_job`、`a_listener_on_port_zero_gets_a_port`、`a_second_listener_on_the_same_port_is_addr_in_use`、`an_accept_returns_the_connected_stream_and_its_peer`、`a_connect_to_a_closed_port_is_refused`、`a_write_to_a_closed_peer_fails_with_reset_or_broken_pipe`、`a_shutdown_write_gives_the_peer_eof`、`a_cancelled_accept_completes_with_an_error`、`a_long_write_completes_once_with_its_whole_length`、`a_cancelled_long_write_stops_resubmitting`、`a_cancelled_connect_does_not_try_the_next_address`、`quiesce_drains_a_pending_accept_and_closes_discarded_streams`、`a_connect_by_name_tries_the_looked_up_addresses_in_order`、`a_cancelled_lookup_completes_at_once`、`quiesce_does_not_wait_for_a_lookup`、`live_lookups_beyond_the_limit_wait_in_order`、`abandoned_lookups_do_not_hold_the_limit`、`a_lookup_fails_while_64_abandoned_lookups_are_stuck`)、io-backends の T-6 の `a_wait_with_a_pending_probe_does_not_tick`、`a_readable_probe_follows_the_stream` (覗く規則で IOCP も同じ条件で通る)、`a_wait_with_two_hundred_pending_ops_returns_only_the_finished_one` (G-4 はどの backend にも当たる)、io-fastpath の `a_read_after_a_finished_probe_returns_done` (G-2 の How to check)、`a_cancel_after_completion_does_not_touch_the_next_op_of_the_key`、`a_pending_key_submitted_twice_panics_in_debug` (印があれば) | `#[cfg(unix)]` を外す。補助の関数 (`raw`、`loopback`、`listen`、`stream`、`connection`、`closed_port`、解決の関数と static) の印も外す。io-backends がすでに外したものは対象にならない | 1 | N-3 |
| `pool_threads_block_interrupt_and_terminate`、`a_wake_after_the_driver_is_dropped_raises_no_sigpipe`、`a_cancelled_probe_completes_without_a_wait` | unix のまま (unix のシグナル、readiness の取り消しがその場で終わる性質) | — | — |
| `listen_accept_and_connect_are_unsupported` | 消す | 1 | N-2 |

`crates/eml_vm/tests/api.rs`:

| テスト | 扱い | 種類 | コミット |
|---|---|---|---|
| `a_real_connection_is_accepted`、`an_interrupt_stops_a_run_blocked_in_a_real_accept`、`an_interrupt_stops_a_run_waiting_on_a_lookup`、`a_timeout_around_a_slow_lookup_returns_none`、`a_real_connection_carries_bytes_between_tasks`、`a_real_read_chunk_within_keeps_the_stream`、`a_real_write_of_8_mib_arrives_whole`、`a_real_read_line_reads_lines_split_across_writes` (8本) | `#[cfg(unix)]` を外す。割り込みは `Control` で起こす | 1 | N-3 |
| io-fastpath の T-3 の `an_injected_driver_waits_at_most_10_ms_at_a_time` | 印がなく、もう Windows で流れている | — | — |
| io-fastpath の T-3 の `a_real_run_with_a_doorbell_has_a_monitor_from_its_start` | `#[cfg(target_os = "linux")]` のまま (`/proc` を読む) | — | — |
| io-fastpath の T-7 の `a_real_read_after_a_finished_probe_does_not_wait` (本物の driver で、`Readable` が終わった後の `Read` が `Done` で、その後に `wait` がない) | unix のまま (VM の上の Windows の版は作らない)。Windows の速い道は `a_read_after_a_finished_probe_returns_done` (driver) と単体テスト `a_read_after_a_finished_probe_is_done_without_a_packet` (backend) が確かめる | — | — |
| `a_real_listen_is_not_available` | 消す | 1 | N-2 |

ほか: `crates/eml_vm/tests/signal.rs` は印がなくもう Windows で流れている。`crates/eml_cli/tests/cli.rs` の OS のシグナルのテストは unix のまま。`Net` の UI テストはテストの driver の上で流れ、付ける印も外す印もない。

単体テスト (`src/sys/windows/iocp/tests.rs`、N-2) の作り:
- 後始末の型: `struct Rig { iocp: Iocp, table: Table<Iocp>, next: OpKey }`。`Rig::new()`、`Rig::pair() -> (Rc<IocpSocket>, TcpStream)` (std の対を作り、片方を `wsa::attach`)、`Rig::start(io) -> (OpKey, Start<IocpSocket>)` (`Record { cancelled: false, work: Work::Io(IoOp { io, slot: IocpSlot::default() }) }` を入れて `start` を呼ぶ。`Done` なら記録を外す)、`Rig::wait(ms) -> Vec<Finished<IocpSocket>>` (終わった記録を外す)。`impl Drop for Rig`: 表の記録で `slot.op` が `Some` のものを `cancel` し、どれも `None` になるまで 1 秒まで `wait(Some(10 ms))` を繰り返す。それでも残れば何もしない (欄が落ちるときに `InFlight::drop` が鍵を書いて abort する)。panic の巻き戻しの中でも同じで、panic の hook が先に文言を書くので、失敗したテストの診断は残る
- ソケットは std の `TcpListener` と `TcpStream`。貸す記憶は `Lent::of(&mut [u8], len)` で作り、`Lent::new` は書かない。slice はテストが完了まで生かす
- データが着くまでの待ちは、相手の書きの後に `Readable` を出して packet を待つ形にする。例外は記憶を立てずに着いたことを知る2本 (`a_zero_length_recv_with_data_still_completes_through_the_port`、`a_read_whose_data_was_taken_waits_again`) で、socket2 の `peek` か `recv` を `thread::yield_now` を挟んで繰り返す (上限 1 秒で `panic!`)

| # | テスト | 確かめること |
|---|---|---|
| 1 | `a_read_after_a_finished_probe_is_done_without_a_packet` | 相手が書き、`Readable` が `Size(0)` で終わった後の `Read` が `Start::Done(Ok(Size(n)))` で、次の `wait(Some(0))` が `TimedOut` |
| 2 | `a_read_without_data_completes_through_a_zero_length_recv` | データのない `Read` が `Pending` で `slot.op` が `Some`。相手が書くと `wait` が `Size(n)` を返し、slice の先頭 `n` バイトが書いたバイト |
| 3 | `a_zero_length_recv_with_data_still_completes_through_the_port` | 記憶 `false` のソケットに相手が書き、`peek` で着いたのを見てから `Readable` を出すと `Pending` で、`wait` が `Size(0)` を返し、次の `Read` がデータを読む |
| 4 | `a_read_whose_data_was_taken_waits_again` | 保留の `Read` (長さ 0 の受け) の間に相手が書き、テストがソケットから直に `recv` でデータを取ると、次の `wait` は `Finished` を積まず (`TimedOut` か `Woken`)、`slot.op` が `Some` (出し直した)。続けて相手が書くと `Size(n)` で終わる |
| 5 | `a_probe_on_a_fresh_stream_waits` | 新しいソケットの `Readable` が `Pending` |
| 6 | `a_probe_after_a_read_peeks` | 相手が 10 バイト書き、4 バイトの空きの `Read` が 4 を返した後の `Readable` が覗きで `Start::Done(Ok(Size(0)))`、`slot.op` が `None`、続く `Read` が残りの 6 バイトを読む (覗きはデータを消さない) |
| 7 | `a_probe_after_a_would_block_waits` | `Read` が `Pending` になって取り消した後の `Readable` が `Pending` |
| 8 | `a_write_to_a_reading_peer_is_done` | 小さい `Write` が `Start::Done(Ok(Size(len)))` |
| 9 | `a_write_to_a_full_peer_completes_through_the_port` | 小さい `SO_SNDBUF` と相手の小さい `SO_RCVBUF` で書き続けると `Pending` になり、相手が読みきると `Size(m)` (`m > 0`) |
| 10 | `a_cancelled_zero_length_recv_ends_with_the_cancel_mark` | 保留の `Read` を `cancel` すると `Finished { cancelled: true }`、`slot.op` が `None`、`free` が1つ増える |
| 11 | `a_reset_peer_is_connection_reset` | 相手が `set_linger(Some(Duration::ZERO))` で閉じると、保留の `Read` が `ConnectionReset` |
| 12 | `a_reset_peer_still_makes_the_stream_readable` | 同じ形で `Readable` が `Ok(Size(0))`、次の `Read` が `Start::Done(Err(ConnectionReset))` |
| 13 | `a_reset_stays_on_the_stream` | reset の後の `Read` を2回出すと、どちらも `ConnectionReset`。その後の `Readable` (記憶 `true`) が覗きで `Start::Done(Err(ConnectionReset))` |
| 14 | `drain_broken_frees_a_box_whose_cancel_finished` | 保留の `Read` を持つ表で `drain_broken` を呼ぶと戻り (abort しない)、`slot.op` が `None`、`free` の数が呼ぶ前と同じ (使い回さない) |
| 15 | `a_finished_box_is_reused` | packet の後に `free.len() == 1`、次の保留の操作の後に `free.len() == 0` |
| 16 | `an_accept_with_a_backlog_is_done` | 相手が接続した後の `Accept` が `Start::Done(Ok(Accepted(_, peer)))` で `peer` が相手の `local_addr`。受けたソケットの `Readable` が `Pending` で、相手が書くと終わる |
| 17 | `an_accept_without_a_backlog_completes_through_the_port` | 接続のない `Accept` が `Pending`、相手が接続すると `Accepted`、受けたソケットの `peer_addr` と `shutdown(Write)` が動く |
| 18 | `an_accept_while_an_acceptex_is_pending_is_done` | 保留の `Accept` を取り消して 995 で終えた後、相手が接続してから出した `Accept` が `Start::Done` |
| 19 | `a_connect_completes_through_the_port` | `socket(peer)` と `Connect` が `Pending`、`wait` で `Connected`、`shutdown_write` の後に相手が EOF を読む |
| 20 | `codes_are_normalized` | `normalize` の4行とほかの値の素通し |
| 21 | `a_reset_before_accept_is_an_aborted_accept` | `accept_error(10054)` が `ConnectionAborted` |
| 22 | `an_unrequested_abort_is_not_cancelled` | 995 の扱い (1.4節の2行) |
| 23 | `the_step_after_a_read_packet_follows_the_table` | `after_read_packet` の、成功の packet と `recv` のデータ / EOF / 誤り / 10035 × `cancel_requested` の真偽、誤りの packet、995 の packet のすべての組が1.4節の行どおり |
| 24 | `a_probe_after_draining_the_data_waits` | 相手が書いたバイトを `Read` で読み尽くした後 (記憶は `true` のまま) の `Readable` が覗きの 10035 で `Pending`、`slot.op` が `Some`、相手が書くと `Size(0)` で終わる (G-19 の「読み尽くした後は `Pending`」) |

計 24 本。足さないテスト:
- 落とした後に相手が書くテスト: driver を落とすとソケットも閉じるので、相手の書きはどの道でも届かず、欠陥を見つけない。落とす前にカーネルが終わることは10番と、付録3の `dropping_a_completion_driver_waits_for_cancelled_io` (核の順序) が確かめる。
- 受けの前に相手が切った接続を本物のソケットで起こすテスト: 順序を決めて起こせず、Linux では切れた接続も `accept` が返すので OS で結果が違う。出し直しは付録3の `an_aborted_accept_is_retried_under_the_same_key`、写しは純粋な関数のテストが受け持つ。

N-4 で `assumptions.rs` を消すときの対応:

| `assumptions.rs` | 引き継ぐテスト |
|---|---|
| `a_zero_length_recv_waits_and_leaves_the_data` | 2、3 |
| `a_zero_length_recv_completes_on_eof_and_reset` | `tests/real.rs` の `a_closed_peer_reads_eof` (N-3)、12 |
| `a_zero_length_recv_completes_even_if_the_data_is_taken` | 4 |
| `a_nonoverlapped_recv_without_data_would_block_and_queues_no_packet` | 1、5、17 |
| `a_reset_stays_on_the_socket` | 13 |
| `an_accepted_socket_takes_overlapped_receives` | 16 |
| `accept_works_while_an_acceptex_is_pending` | 18 |

## 6. 決めていない点 (既定つき)

| ID | 項目 | 既定 |
|---|---|---|
| U-2 | `free` の箱の数の上限 | 置かない (保留の操作の最大の数で止まる) |
| U-3 | `Accept` の先の `accept` | 試す。N-1 で崩れたら `AcceptEx` だけ |
| U-4 | io-fastpath の T-7 の `a_real_read_after_a_finished_probe_does_not_wait` を Windows で流すか | 流さない (5.4節) |
| U-5 | `FILE_SKIP_SET_EVENT_ON_HANDLE` を立てるか | 立てる (libuv と同じ。ソケットごとに1回) |
| U-6 | 重なり合った呼び出しの同期の 10035 を写すか | 写さない |
| U-7 | `GetQueuedCompletionStatusEx` の1回の項目の数 | 128。満ちたら待ちの長さ 0 で取り直す |
| U-8 | `accept` で受けたソケットに `set_nonblocking(true)` を呼ぶか | 呼ぶ (`attach` を通す) |
| U-10 | Windows の I/O の数字 | net-windows では取らない (`bench/net` も Windows を流していない)。待つ読みの2回増え (1.6節) と速い道の利益の得失は未計測と記録する。Windows の機械か runner で `bench/net` を流せるようになったら、閉じた後の別の作業で測る |
| U-11 | `drain_broken` は本物の壊れた状態を CI で起こせない。単体テストは「取り消しの後に呼ぶと戻り、abort しない」だけを確かめる | abort の道と `CancelIoEx` の失敗の道はレビューで読む。計画に書く |
| U-12 | 後で async.md の「受けたソケットは最初に読む worker に縛る」形をとるとき | 結び付けを最初の操作に遅らせる形に変える。今は worker が1つなので要らない |

`Terminate` (CTRL_CLOSE_EVENT と CTRL_SHUTDOWN_EVENT) は net-windows の実装に入れない。IOCP と別の話題である。N-4 で「その後の項目」へ移す。

## 7. 出典

[WSARecv function](https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-wsarecv)、[AcceptEx function](https://learn.microsoft.com/en-us/windows/win32/api/mswsock/nf-mswsock-acceptex)、[SetFileCompletionNotificationModes](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setfilecompletionnotificationmodes)、[CancelIoEx](https://learn.microsoft.com/en-us/windows/win32/fileio/cancelioex-func)、[SIO_TCP_INITIAL_RTO](https://learn.microsoft.com/en-us/windows/win32/winsock/sio-tcp-initial-rto)、[HandlerRoutine](https://learn.microsoft.com/en-us/windows/console/handlerroutine)、[curl の mailing list](https://curl.se/mail/lib-2026-08/0002.html)、[libuv src/win/tcp.c](https://github.com/libuv/libuv/blob/v1.x/src/win/tcp.c)、[socket2 0.6.5 の sys/windows.rs](https://docs.rs/crate/socket2/0.6.5/source/src/sys/windows.rs)
