# 付録2: Windows の IOCP の backend (この段の最小の形と、後の段 `net-windows`)

この付録は3つの部分からなる。本体と食い違うときは本体に従う。

- §A: この段の T-5 で入れる最小の IOCP の backend。完了ポート、waker、時間の待ちだけを持ち、ソケットの関数は `Unsupported` を返す
- §B: 後の段 `net-windows` の設計。この段では実装しない。`net-windows` の計画は、その段の最初のコミットで別に作る。そのときの出発点は、T-9 が roadmap の `net-windows` の項に書き写す要点 (§C) と、この付録の写しである
- §C: T-9 が roadmap の `net-windows` の項に書き写す文案

核の形 (付録1 の trait、表、`Drop` の表、`buf.rs`、`sys/socket.rs`) に依る所は §A.3 にまとめてある。

§B の形は、この計画を作る段階で試作していない。試作で `.#cross` の clippy を通したのは、`Backend` の trait を使わない古い形の `iocp.rs` と `wsa.rs` である。§B の unsafe の部分 (`drain_broken` の確かめ、`failed`、packet だけの読み方、`WSAGetOverlappedResult`、`Lent::of` を使うテスト) は、`net-windows` の N-1 の完了の基準にある `.#cross` の clippy と、windows-2025 の CI が初めて確かめる。

## A. この段の最小の IOCP の backend (T-5、`src/sys/windows/iocp.rs`)

### A.1 形

```rust
// 起こしの相手。完了ポートのハンドルと旗を Arc に置き、waker として配る
pub struct Port { handle: OwnedHandle, flag: AtomicU8 }   // IDLE = 0、NOTIFIED = 1、AWAKE = 2
impl IoWaker for Port { fn wake(&self) { … } }

pub struct Iocp { port: Arc<Port>, entries: [OVERLAPPED_ENTRY; 128] }
impl Backend for Iocp {
    type Slot = ();                        // net-windows で IocpSlot に替える
    type Socket = socket2::Socket;
    const KERNEL_WRITES_AFTER_CANCEL: bool = true;
    …
}
```

- `Port` は `unsafe impl` なしで `Send + Sync` になる (試作の crate で `send_sync::<Port>()` を `.#cross` の clippy で通した)。
- `new()`: `CreateIoCompletionPort(INVALID_HANDLE_VALUE, null_mut(), 0, 1)`。null なら `Err(io::Error::last_os_error())`。`OwnedHandle::from_raw_handle` で包む。`Arc<Port>` を backend が持ち、同じ `Arc` を `Arc<dyn IoWaker>` として返す。並行のスレッドの数を 1 にするのは、ポートを読むのが持ち主のスレッドだけだからである。
- 完了の鍵: `SOCKET_KEY = 0` (ソケットの結び付け。`net-windows`)、`WAKE_KEY = 1` (起こしの packet。`lpOverlapped` は null)。
- `wake`: `if flag.fetch_or(NOTIFIED, AcqRel) == IDLE { PostQueuedCompletionStatus(port, 0, WAKE_KEY, null()) }`。戻り値は捨てる。確保も lock もしない。driver を落とした後も `Arc<Port>` が残る限りポートは開いており、packet は誰も読まないポートに積まれて、最後の `Arc` と一緒に消える。
- `wait(timeout, table, out)`:
  1. `notified = flag.swap(IDLE, AcqRel) & NOTIFIED != 0`
  2. 待ちの長さ: `notified` なら 0、`None` なら `INFINITE`、`Some(d)` なら `d.as_nanos().div_ceil(1_000_000)` を `INFINITE - 1` で抑えた値 (epoll と同じ切り上げ)
  3. `GetQueuedCompletionStatusEx(port, entries, 128, &mut removed, ms, FALSE)` の後に `flag.store(AWAKE, Release)`
  4. FALSE で `GetLastError() == WAIT_TIMEOUT` なら時間切れ。ほかの誤りは `Err` (核が `broken` にする)
  5. TRUE なら各項目を見る。`lpCompletionKey == WAKE_KEY` は起こし、ほかは I/O の完了 (`net-windows` で処理する。この段では来ない)。`removed == 128` なら待ちの長さ 0 で取り直す
  6. 戻り値: I/O の完了を1つでも積めば `Woke::Finished`、起こし (packet か手順1の旗) なら `Woke::Woken`、ほかは `Woke::TimedOut`

起こしを失わないことの場合分け:

| `wake` を呼んだ時点 | `fetch_or` の前の値 | 起きること |
|---|---|---|
| `wait` の外 (`AWAKE`) | 2 | packet は出さない。`NOTIFIED` が立ち、次の手順1が待ちの長さを 0 にする |
| 手順1の後、`GetQueuedCompletionStatusEx` の前か中 (`IDLE`) | 0 | packet を出す。待ちはその packet で戻る |
| 戻った後、手順3の `store` の前 (`IDLE`) | 0 | packet を出す。`store(AWAKE)` が `NOTIFIED` を消すが、packet が残り、次の `wait` が待たずに `Woken` で戻る (余分な起こしが1回ある。正しさは保つ) |
| 2回目以降 (`NOTIFIED` が立っている) | 1 か 3 | packet は出さない。1回目の packet か旗が受け持つ |

- 期限より早く戻ること: Windows の待ちは刻み (約 15.6 ms) の都合で、`Instant` から見て期限の手前で戻りうる。`IoDriver::wait` は「早く戻ってよい」契約で、呼ぶ側 (VM と `a_timed_wait_returns_at_or_after_its_deadline` のループ) は時刻を確かめて待ち直す。backend は待ち直さない。
- 仕事と解決の結果: プールと解決器は `shared.post` で受け箱に積んでから waker を鳴らす。backend は受け箱を読まない。核は `poll` の後に必ず `absorb` する。
- ソケットの関数: `listen`、`socket`、`shutdown_write`、`set_nodelay` は `Err(ErrorKind::Unsupported)`。Windows にはハンドルができないので、後の2つは届かない。`start` は `Start::Done(Err(Unsupported))`、`cancel` は何もしない、`drain_broken` は何もしない。核に Windows の分岐はない。
- `iocp.rs` の先頭のコメントに書く契約: 「カーネルが記録の `OVERLAPPED` と貸した記憶に書き終えたと backend が確かめるまで、その記録を放さない。落とすときは、核が I/O の記録をすべて取り消し、`wait` で完了を取り出しきってから表を放す。壊れて待てないときだけ `drain_broken` が引き受ける」。この段では I/O の記録がないので待ちはいつも空である。

### A.2 依存とファイル

- `crates/eml_io/Cargo.toml` の `[target.'cfg(windows)'.dependencies]`: `windows-sys = { workspace = true, features = ["Win32_System_IO", "Win32_System_Threading"] }`。`Win32_Foundation` はどの `Win32_*` からも入る (windows-sys 0.61.2 の Cargo.toml)。使う記号は `CreateIoCompletionPort`、`GetQueuedCompletionStatusEx`、`PostQueuedCompletionStatus`、`OVERLAPPED_ENTRY`、`INFINITE`、`INVALID_HANDLE_VALUE`、`WAIT_TIMEOUT`、`GetLastError`、`HANDLE`。この組だけで iocp.rs の試作が `.#cross` の clippy を `-D warnings` で通った。
- `Cargo.lock` に新しい package は入らない (windows-sys 0.61.2 はすでにある)。
- 触るファイル: `crates/eml_io/Cargo.toml`、`src/sys/mod.rs` (`#[cfg(windows)] mod windows;` と `pub type Backend = windows::iocp::Iocp;`)、`src/sys/windows/mod.rs`、`src/sys/windows/iocp.rs` (`#![allow(unsafe_code)]`、日本語の SAFETY のコメント)、`src/sys/tests.rs` (`mod tests` を `cfg(test)` に)、`src/real/mod.rs` (`impl Driver<sys::Backend>` の `cfg(unix)` を外す)、`tests/unsafe_code.rs` (`LISTED` に `src/sys/windows/iocp.rs`)。
- docs はこのタスクに入れない。vm.md の Windows の文、architecture.md の `sys/windows/iocp.rs` の行、status.md の Windows の文は T-6 が書く。
- テスト: 付録1 §13 の5本を Windows でも流す。`iocp.rs` には同じ中身のテストを置かない。
- 実装者に任せること: `entries` の数 (既定 128)、関数の名前、SAFETY の文。

### A.3 IOCP の backend が満たす核の約束

`net-windows` の backend も含めて、IOCP の backend は次を守る。
- 型: `type Socket` は `socket2::Socket` のままでよい (ソケットを作るときにポートへ結び付けるので、ソケットごとの状態が要らない)。結び付けを後で遅らせるときは、`Cell` の印を持つ型に替える。関連型なので核は変わらない。`const KERNEL_WRITES_AFTER_CANCEL: bool = true`。`type Slot: Default` に、`OVERLAPPED` の箱 (`OpKey` を入れておくと `wait` で鍵を引ける) と、`AcceptEx` の受けるソケットと番地の buffer を置く。核は記録を表から出し入れする (`HashMap` の中で動く) ので、カーネルに渡す番地は箱の中に置く。
- `start`: 同期に失敗した呼び出しだけを `Start::Done(Err)` で返す。同期の成功も `Pending` にし、結果は packet で読む。続きの書きは、核が同じ記録で `start` し直すので、箱を使い回せる。
- `cancel`: OS に取り消しを頼むだけで、`out` には積まない。「取り消すものがない」(`ERROR_NOT_FOUND`) は捨てる。ほかの誤りで断られたら、その誤りを覚え、次の `wait` が OS を待たずに `Err` で返す。終わりは `wait` で `Finished` にし、995 なら `cancelled: true` にする。誤りの値は何でもよい (核が替える)。
- `wait`: 起こしの packet を `Woke::Woken` に、時間切れを `Woke::TimedOut` にする。`Err` は、OS の待ちそのものの失敗か、覚えた取り消しの失敗である。プールと解決器の結果は受け箱と waker (`PostQueuedCompletionStatus`) で届くので、packet の中身を見なくてよい。核は `wait` の後にいつも受け箱を空ける。
- `listen`、`socket`、`Accepted` で返すソケット: 作るときに結び付ける。この段では `listen` と `socket` は `Unsupported` を返し、`start` には届かない (ハンドルがない)。
- `drain_broken(&mut Table<Self>)`: 壊れた driver の `Drop` からだけ呼ばれる。表の `Work::Io` の記録を鍵で引き、取り消しを出し、合わせて 1 秒までカーネルの終わりを確かめる。終わった箱は放す。終わらない操作が残れば、残った鍵を `let _ = writeln!(std::io::stderr(), …)` で書いて `std::process::abort()` する。
- 契約違反の箱 (カーネルが書きうるのに放される箱) は、箱の `Drop` で abort する。
- 付録1 §13 の5本を Windows で通す。
- `Lent::len`、`Lent::room`、`Lent::bytes(from)` を使い、slice は呼び出しの間だけ持つ。`wsa.rs` のテストは `Lent::of` を使い、`Lent::new` を書かない。
- `sys/socket.rs` の `listener` と `stream` を使う (`net-windows`)。

### A.4 完了の基準 (本体 T-5 に写したもの)

- `nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings` → 終了コード 0
- Linux で `cargo nextest run -p eml_io` → 失敗なし
- `grep -rn "forget\|Box::leak" crates/eml_io/src` → 0行。`grep -rn "unsafe impl" crates/eml_io` → 0行
- CI の `test (windows-2025)` が緑で、出力に次の5行の `PASS` がある: `eml_io sys::tests::a_wake_from_another_thread_ends_a_long_wait`、`eml_io sys::tests::a_wait_without_a_wake_times_out`、`eml_io sys::tests::wakes_before_a_wait_merge_into_one`、`eml_io sys::tests::a_wake_after_the_backend_is_dropped_is_harmless`、`eml_io sys::tests::the_driver_waker_wakes_its_backend`

## B. 後の段 `net-windows` の設計

前提: `io-backends` (この段)。段の作業の単位は N-0 から N-3 である。

### B.1 箱と `Slot`

```rust
#[repr(C)]
struct OpState {
    ovl: OVERLAPPED,             // 先頭。packet の lpOverlapped をこの型の番地として読む
    key: OpKey,                  // packet から table.io_mut(key) を引く
    socket: SOCKET,              // WSAGetOverlappedResult と CancelIoEx に渡す
    flags: u32,                  // WSARecv の lpFlags
    addrs: [u8; 2 * ADDR_LEN],   // AcceptEx の番地。ADDR_LEN = size_of::<SOCKADDR_STORAGE>() + 16
    peer: Option<SockAddr>,      // ConnectEx に渡した番地を完了まで生かす
}
struct OvlBox { ptr: NonNull<OpState>, in_flight: bool }   // Box::into_raw で作る
#[derive(Default)]
pub struct IocpSlot {
    op: Option<OvlBox>,          // 記録の最初の start で作り、続きで使い回す
    accept: Option<Socket>,      // AcceptEx の受けるソケット
    cancel_requested: bool,      // cancel が来たか。頼んでいない中断の誤りの写しに使う
}
```

- 箱は生の番地で持つ。核の表は記録を `HashMap` の中で動かすので、`Box` のまま持つと記録が動くたびに一意の借用が付け直され、カーネルに渡した番地と食い違う。
- 1つの型にまとめるのは、記録ごとの確保を1回にするためである (`addrs` と `peer` で約 300 バイト増えるが、確保の回数は変わらない)。
- カーネルが書く間は、箱に Rust の参照を作らない。`OVERLAPPED` を先頭に置く。

寿命:

| 時点 | 箱 | `in_flight` |
|---|---|---|
| 記録の最初の `start` | 確保する | |
| OS に出す直前 | `ovl` を 0 で埋め直す | `true` |
| 同期に失敗した (`WSAGetLastError() != WSA_IO_PENDING`。`Start::Done(Err)`)。完了の知らせはない (WSARecv の Return value) | 残す | `false` |
| 同期の成功 (戻り値 0 か TRUE) と `WSA_IO_PENDING` (どちらも `Start::Pending`)。成功のときに packet を省くモードを立てないので、同期の成功も packet を積む | カーネルのもの | `true` |
| `wait` で packet を取り出した | 結果を読む。核の続き (書きの残り、受けの出し直し) は同じ箱で出す | `false` |
| 核が記録を落とす | `OvlBox::drop` が `Box::from_raw` で放す | `false` であること |
| 壊れた driver の `drain_broken` で終わりを確かめた | `OvlBox::drop` が放す (記録は核が落とす) | `false` |
| `drain_broken` で 1 秒たっても終わらない | 残った鍵を書いて `std::process::abort()`。箱は放さず漏らしもしない | `true` のまま |

- `OvlBox::drop` で `in_flight` が `true` なのは、核か backend の契約違反だけである。カーネルが書く記憶を放さないよう、`let _ = writeln!(std::io::stderr(), …)` で鍵を書いて `std::process::abort()` する。黙って漏らす道は作らない。`Drop` の debug の見張りは panic しないので、正しい道ではこの abort に届かない。
- `wait` の I/O の項目: `entry.lpOverlapped` を `*mut OpState` として `key` だけを読み (カーネルはもう書かない)、`table.io_mut(key)` で `IoOp` を引き、`debug_assert!` で鍵の記録があることと `slot.op` の番地が `entry.lpOverlapped` と等しいことを確かめる。成功のときに packet を省くモードを使わないので、packet はいつも `in_flight` の箱のものである。`in_flight = false` にし、B.4 の読み方で `Finished` を積む。表は歩かない。

### B.2 結び付けとソケットの作成

ソケットを作るのは backend (`listen`、`socket`、`Accepted`) なので、作ったその場で結び付ける。ソケットごとの状態は要らない。
- 結び付け: `CreateIoCompletionPort(socket, port, SOCKET_KEY, 0)` と `SetFileCompletionNotificationModes(socket, FILE_SKIP_SET_EVENT_ON_HANDLE)`。どちらかの失敗はソケットを作る関数の `Err` にする。関数は `&Port` を受ける (生の `HANDLE` を受ける公開の関数は clippy の `not_unsafe_ptr_arg_deref` で落ちる)。
- 成功のときに packet を省くモード (`SetFileCompletionNotificationModes` の SKIP の旗) は立てない。非 IFS の LSP がある機械ではこのモードが正しく動かず、eml はそれを実行時に見分けない。効かなかったときの packet は、放した箱の読みか、使い回した箱の誤った `in_flight = false` になる。libuv も非 IFS の LSP を調べたときだけ立てる (libuv v1.x の `src/win/tcp.c` の `uv__tcp_set_socket`)。費用は、同期に成功した呼び出し1回につき packet を1つ取り出すことだけである。モードは一度立てると外せない。
- コードとコメントに SKIP の旗の定数の名前を書かない (N-1 の grep の基準)。「成功のときに packet を省くモード」と書く。
- `listen(addr)`: `sys/socket.rs` の `listener` を呼ぶ (`Socket::new(domain, STREAM, TCP)`、`set_nonblocking(true)`、`SO_REUSEADDR` は `cfg(unix)` だけ、bind、`listen(1024)`、`local_addr` のポート)。その後で結び付ける。socket2 は Windows で `WSA_FLAG_OVERLAPPED` と継がない印を付ける (socket2 0.6.5 の `sys/windows.rs`)。
- ブロックしない設定: Windows でも `listen`、`socket`、受けたソケットのすべてで `set_nonblocking(true)` (`ioctlsocket(FIONBIO)`) を呼ぶ。libuv も IOCP のソケットに `FIONBIO` を立てる。重なり合った呼び出しは `lpOverlapped` を渡す限り非同期で、この設定の影響を受けない (推定。CI で確かめる)。
- Windows の `SO_REUSEADDR` は使っている番地への bind を許し、2つ目のリスナーが `AddrInUse` にならない。std も Windows では立てない (rust-src 1.99.1 の `sys/net/connection/socket/mod.rs`)。
- `socket(peer)`: `sys/socket.rs` の `stream(peer)` を呼ぶ。続けて同じ domain の未指定の番地のポート 0 に bind (`ConnectEx` は bind 済みを求める)、相手がループバックなら B.3 の送り直しの設定、結び付け。
- `Accepted`: `AcceptEx` の完了の後に `SO_UPDATE_ACCEPT_CONTEXT`、`set_nonblocking(true)`、結び付けを行ってから返す。
- 後で async.md の「受けたソケットは最初に読む worker に縛る」形をとるときは、結び付けを最初の操作に遅らせる形に変える。今は worker が1つなので要らない。

`sys/socket.rs` はこの段の T-4 で全 OS の形になっている (`SO_REUSEADDR` は unix だけ)。`net-windows` で直す所はない。

### B.3 操作ごとの `start`

- どの腕も、同期に失敗したとき (`WSAGetLastError() != WSA_IO_PENDING`) だけ `Start::Done(Err)` を返し、同期の成功も `WSA_IO_PENDING` も `Start::Pending` を返す。
- `Readable { socket }`: 長さ 0 の `WSARecv` (`WSABUF { len: 0, buf: NonNull::dangling() }`)。packet の成功は `Ok(Size(0))`。誤り (reset など) も、`cancel_requested` の中断を除いて `Ok(Size(0))` にする。unix の `Readable` は誤りでも「読める」として `Ok(0)` を返し、次の `Read` が誤りを言うので、それにそろえる。Windows の reset は以後の呼び出しでも誤りを返す (WSARecv の文書の `WSAECONNRESET` の項から推定)。vm.md の `Readable` の文は変えない。
- `Read { socket, buf }`: `cap == 0` の読み書きは核が backend を呼ばずに終えるので、`start` は `debug_assert!(buf.len() > 0)` を置くだけにする (`Write` も残りの長さで同じ)。`WSARecv(s, &WSABUF { len: min(len, u32::MAX), buf: buf.room().as_mut_ptr() }, 1, null_mut(), &mut state.flags, ovl, None)`。`lpNumberOfBytesRecvd` は文書が `lpOverlapped` のあるときに勧める NULL にする。`WSABUF` はスタックでよい (service provider が写す)。
- `Write { socket, buf, written }`: `WSASend(s, &WSABUF { len: min(rest, u32::MAX), buf: buf.bytes(written).as_ptr() }, 1, null_mut(), 0, ovl, None)`。短い完了の続きは核の `settle` が出す。
- `Accept { listener }`: 受けるソケットを `Socket::new(リスナーの domain, STREAM, TCP)` で作って `slot.accept` に置き、`AcceptEx(listener, accepted, addrs, 0, ADDR_LEN, ADDR_LEN, &mut ignored, ovl)` (`lpdwBytesReceived` は同期のときだけ入るので読まない)。終わったら `setsockopt(accepted, SOL_SOCKET, SO_UPDATE_ACCEPT_CONTEXT, &listener, size_of::<SOCKET>())`、`set_nonblocking(true)`、結び付け、`accepted.peer_addr()` を `SocketAddr` にして `Ok(Accepted(slot.accept.take(), peer))`。受信の長さ 0 なら接続が来た時点で終わり、`SO_UPDATE_ACCEPT_CONTEXT` の後なら `getpeername` が使える (AcceptEx の文書)。核が出し直すときは新しい受けるソケットを作る (古いものは落とす。操作は終わっている)。
- `Connect { socket, peer, .. }`: 番地を `state.peer` に写し、`ConnectEx(s, peer, len, null, 0, null_mut(), ovl)`。終わったら `setsockopt(s, SOL_SOCKET, SO_UPDATE_CONNECT_CONTEXT, null, 0)` をして `Ok(Connected)`。この設定がないと `shutdown` と `getpeername` が使えない。`name` の寿命は文書に書かれていないので、完了まで `state.peer` に生かす。
- ループバックへの接続の設定 (`socket` の中): `WSAIoctl(s, SIO_TCP_INITIAL_RTO, &TCP_INITIAL_RTO_PARAMETERS { Rtt: TCP_INITIAL_RTO_DEFAULT_RTT as u16, MaxSynRetransmissions: TCP_INITIAL_RTO_NO_SYN_RETRANSMISSIONS as u8 }, ..)`。失敗は捨てる (古い Windows は今の振る舞いのまま)。
  - 理由: Windows は拒まれた接続でも SYN を送り直し、閉じたループバックのポートへの接続が約 2 秒かかる。送り直しを 0 にすると約 0.3 ms で拒まれる (curl の mailing list の測定)。libuv の `uv__tcp_try_connect` が同じ設定を、ループバックで Windows 10 1709 以降のときに出している。ループバックには失われる packet がないので、送り直しを消しても失うものはない。`localhost` が `::1` を先に返し、相手が IPv4 だけで待つときの名前での接続も 2 秒止まらない。
  - windows-sys の定数は `u16` の 65534、欄は `u8` なので `as u8` で 254 (C の `(UCHAR)-2`)。
- 拡張の関数: `AcceptEx` と `ConnectEx` の番地は `WSAIoctl(s, SIO_GET_EXTENSION_FUNCTION_POINTER, &GUID, ..)` で、手元のソケットから取る (AcceptEx の文書の Note)。process 全体の `static ACCEPT_EX: OnceLock<AcceptExFn>` と `CONNECT_EX` に置く。`get()` が空なら取って `set()` し、失敗は覚えずその操作の `Start::Done(Err)` にする。関数の番地の型は `Send + Sync` なので `unsafe impl` は要らない。
- 名前の解決: 解決器は全 OS で組んであるので、触る所はない。`socket` が動けば名前での接続も動く。

### B.4 結果の読み方

- 同期の成功: 読まない。`Start::Pending` にし、packet で読む。
- 同期の失敗: `WSAGetLastError()` が `WSA_IO_PENDING` 以外なら、その番号で `Start::Done(Err)`。
- packet (読み方はこれ1つ): 長さは `entry.dwNumberOfBytesTransferred`。成否と誤りの番号は `WSAGetOverlappedResult(state.socket, ovl, &mut n, FALSE, &mut flags)` で読む (WSARecv の文書の Overlapped Socket I/O の節)。
- NTSTATUS を `RtlNtStatusToDosError` で写す形は使わない。reset が `ERROR_NETNAME_DELETED` (64) になり、std はこれを `Uncategorized` に写す。`WSAGetOverlappedResult` が WSA の番号を返すことは推定なので、`normalize` を挟む。表はこの4行に固定し、CI で別の番号を見たら同じ形で足す: 64 → 10054、1236 (`ERROR_CONNECTION_ABORTED`) → 10053、1225 (`ERROR_CONNECTION_REFUSED`) → 10061、121 (`ERROR_SEM_TIMEOUT`) → 10060。`a_reset_peer_is_connection_reset` と `a_connect_to_a_closed_port_is_refused` がこの道を確かめる。

| 出どころ | 値 | backend の結果 | 種類と VM の扱い |
|---|---|---|---|
| `Read`、`Write` | 10054 | そのまま | `ConnectionReset` (code 2) |
| 同 | 10053 | そのまま | `ConnectionAborted` (code 2) |
| `Write` | 10058 | そのまま | `BrokenPipe` (code 2) |
| `Readable` | 中断以外のどれでも | `Ok(Size(0))` | 次の `Read` が誤りを言う |
| `ConnectEx` | 10061 | そのまま | `ConnectionRefused` (code 3) |
| `ConnectEx` | 10060 | そのまま | `TimedOut` (code 5) |
| `listen` の bind | 10048 | そのまま | `AddrInUse` (code 4) |
| `AcceptEx` | 10054 (受ける前に相手が切った。AcceptEx の Return value) | `io::Error::from(ErrorKind::ConnectionAborted)` | 核が同じ鍵で出し直す |
| どれでも | 995 で `cancel_requested` | `Err(from_raw_os_error(995))`、`cancelled: true` | 核が `Completion::cancelled` を立てる。VM は種類を見ない |
| どれでも | 995 で `cancel_requested` でない | `Err(io::Error::other("the operation was aborted"))`、`cancelled: false` | std は 995 を `TimedOut` に写すので、OS の時間切れと読まれないよう替える |

本体 D-2 の grep (`ECONNABORTED` が `src/sys` に0行) を満たすため、`WSAECONNABORTED` の名前は書かず、10053 は数で書く。`ConnectionAborted` への写しは backend の `normalize` にあり、出し直しの判定は核にある。

### B.5 取り消し

- `cancel(key, op, out)`: `slot.cancel_requested = true`。箱が `in_flight` なら `CancelIoEx(state.socket as HANDLE, ovl)`。`ERROR_NOT_FOUND` は捨てる (終わって packet が積まれたことを表す)。ほかの誤りで失敗したら、その誤りを `Iocp` の欄 `failed: Option<io::Error>` に覚える (先に覚えた誤りがあれば残す)。`wait` は冒頭で `failed.take()` を見て、あれば OS を待たずに `Err` を返し、核が `broken` にする。
  - 取り消せない操作はカーネルの手の中に残るので、`Finished` の誤りにして核に記録を放させることはしない。
  - その後は壊れた driver の道で、`quiesce` は保留を残して戻り、`Machine` は poisoned になり、`Drop` は `drain_broken` に入る。
  - `out` には積まない。終わりは `wait` で届き、ふつうの結果、`ERROR_OPERATION_ABORTED`、ほかの誤りのどれかになる (CancelIoEx の Remarks)。
- `closesocket` と保留の操作: ソケットは `Rc` で記録が持ち、記録は `Finished` の後にしか落ちない。受けるソケットは `slot.accept`、接続のソケットは `Io::Connect` の `Rc` にある。そのため保留の操作があるソケットは閉じられない。`quiesce` と `Drop` は取り消しの packet を取り出してから記録を落とす。

### B.6 `Drop`、`quiesce`、`drain_broken`

| | readiness (epoll、kqueue) | IOCP |
|---|---|---|
| `quiesce` | 取り消しの完了がその場で積まれ、仕事だけを待つ | `CancelIoEx` の完了を `poll` で受ける。時間の上限なし。debug は 5 秒で panic |
| `Drop` (壊れていない) | 何も待たない | 核が I/O の鍵をすべて `cancel`、`io_len() == 0` になるか壊れるまで `poll(Some(IDLE_TICK))`。時間の上限はない。debug では 5 秒たっても残れば `eprintln!` を1回し、panic しない |
| `Drop` (壊れている、または待ちの途中で壊れた) | `drain_broken` は何もしない | `backend.drain_broken(&mut table)` を呼び、戻ってから核が欄を落とす |
| 仕事と解決 | 待たない | 待たない |

`drain_broken(&mut self, table: &mut Table<Self>)`:
- 壊れた状態は、`GetQueuedCompletionStatusEx` が `WAIT_TIMEOUT` 以外で失敗したときと、`CancelIoEx` が `ERROR_NOT_FOUND` 以外で失敗したときだけで、どちらも欠陥でしか起きない (推定)。
- `table.keys()` で鍵を集め (`Drop` で表を歩くことは許してある)、`table.io_mut(key)` が返す `Work::Io` の記録それぞれについて、箱が `in_flight` なら `CancelIoEx` を出す。ここでの `CancelIoEx` の失敗は捨てる (確かめで終わりを見る)。
- 続けて各箱について、`WSAGetOverlappedResult(.., FALSE, ..)` が `WSA_IO_INCOMPLETE` 以外を返すまで 1 ms ずつ眠って確かめる。待つ長さは全部の操作で合わせて 1 秒まで (`drain_broken` の始めに `Instant::now() + 1 s` を決める)。
- 終わった箱は `in_flight = false` にし、記録と一緒に核が落とすときに `OvlBox::drop` が放す。
- 1 秒たっても終わらない操作が1つでもあれば、残った鍵を `let _ = writeln!(std::io::stderr(), …)` で書いて `std::process::abort()` する。箱は放さず漏らしもしない。
- 戻った後、核が記録を落とす (ソケットの `Rc` が落ち、最後なら `closesocket`)。
- abort にする理由: 貸した記憶は VM のヒープの一部で、driver は生かし続けることも漏らすこともできない。run からの panic の巻き戻しでは driver が `quiesce` を通らずに `Scheduler` の中で落ち、`Machine` はヒープを放す。箱だけを漏らしても使用後解放になるので、終わらない操作が残れば process を終える。`CancelIoEx` から 1 秒以内に終わらない操作は、実際にはない (推定)。
- `eml_io` に `Box::leak` も `mem::forget` もない。

### B.7 分担 (IOCP の行)

| 事柄 | IOCP での置き場所 |
|---|---|
| `EINTR`、`EAGAIN` の後の登録、`SO_ERROR`、HUP/ERR、fd ごとの列 | 当たらない (完了の形で、誤りは完了の状態で届く) |
| ソケットの作成 (`sys/socket.rs` の `listener` と `stream`)、ブロックしない設定、bind、結び付け (`FILE_SKIP_SET_EVENT_ON_HANDLE` だけ)、ループバックの設定、`SO_REUSEADDR` を立てないこと | `Iocp::listen`、`Iocp::socket` |
| 同期の失敗だけを `Start::Done(Err)` に、同期の成功と `WSA_IO_PENDING` を `Start::Pending` に | `Iocp::start` |
| `CancelIoEx` の失敗 (`ERROR_NOT_FOUND` 以外) を覚え、次の `wait` を `Err` に | `Iocp::cancel`、`Iocp::wait` |
| `SO_UPDATE_*_CONTEXT`、受けたソケットの結び付け、拡張の関数 | `Iocp::wait` と `Iocp::start` の中 (`wsa.rs`) |
| 誤りの写し (B.4 の表)、`Readable` の誤りを `Ok(0)` に | `wsa.rs` |
| 受けの出し直し (`ConnectionAborted`)、短い書きの続き、`WriteZero`、接続の次の番地、取り消しの印のある記録の続きをやめること、`Completion::cancelled` | 核 (`Local::settle`、`Local::connect`) |

`wsa.rs` の関数は核の型を受けない (ソケット、番地、`*mut u8` と長さ、`&Port` を受ける)。核の型を読むのは `iocp.rs` の `impl Backend for Iocp` だけにする。核の形が変わっても、直すのは `impl` の腕だけで済む。

### B.8 テスト

テストの変更の種類: `#[cfg(unix)]` を外して Windows で流し始めるテストは、本体と確かめる値を変えないが、Windows での扱いが「流さない」から「流して通す」に変わる。testing.md の「成否の変更」の文言 (run/run-fail/check-fail の移動と削除) には当たらないが、合意の取り方は成否の変更に合わせ、計画に1件ずつ挙げる (「種類1に準じる」と書く)。削除は種類1、期待値を変えるものは種類2である。

前提: async-net の後、`pair` を使っていた unix のテストはループバックの TCP の上に名前を保って書き直されている。io-backends が足したテストは付録1 §14 の名前で書く。

`crates/eml_io/tests/real.rs` (変更はどれも N-2):

| テスト | 扱い | 種類 |
|---|---|---|
| 時計の4本、`a_job_completes_with_its_value`、`a_job_pool_is_capped`、`an_op_on_an_unknown_handle_fails`、`dropping_a_driver_does_not_wait_for_a_running_job`、`a_job_beyond_the_pool_limit_does_not_block_submit`、`a_wake_from_another_thread_ends_a_wait_with_a_far_deadline` | 印がなく、io-backends の T-6 から IOCP の上で流れる。変えない | — |
| `a_wake_from_another_thread_leaves_a_wait_working` | `#[cfg(windows)]` の仕事の枝を消し、unix の枝 (ループバックの上の `Readable`) をすべての OS で使う | 3 (確かめる値は同じ) |
| `a_readable_probe_stays_pending_without_data`、`a_read_after_a_probe_returns_the_bytes`、`a_write_is_read_by_the_peer`、`a_cancelled_probe_completes_with_an_error`、`a_closed_peer_reads_eof`、`quiesce_drains_a_pending_probe_a_full_write_and_a_running_job`、`a_listener_on_port_zero_gets_a_port`、`a_second_listener_on_the_same_port_is_addr_in_use`、`an_accept_returns_the_connected_stream_and_its_peer`、`a_connect_to_a_closed_port_is_refused`、`a_write_to_a_closed_peer_fails_with_reset_or_broken_pipe`、`a_shutdown_write_gives_the_peer_eof`、`a_cancelled_accept_completes_with_an_error`、`a_long_write_completes_once_with_its_whole_length`、`a_cancelled_long_write_stops_resubmitting`、`quiesce_drains_a_pending_accept_and_closes_discarded_streams`、`a_connect_by_name_tries_the_looked_up_addresses_in_order`、`a_cancelled_lookup_completes_at_once`、`quiesce_does_not_wait_for_a_lookup`、`live_lookups_beyond_the_limit_wait_in_order`、`abandoned_lookups_do_not_hold_the_limit`、`a_lookup_fails_while_64_abandoned_lookups_are_stuck`、`completions_return_only_the_finished_ops` (23本) | `#[cfg(unix)]` を外す。補助の関数 (`raw`、`loopback`、`listen`、`stream`、`connection`、`closed_port`、解決の関数と static) の印も外す | 1 に準じる |
| `pool_threads_block_interrupt_and_terminate`、`a_wake_after_the_driver_is_dropped_raises_no_sigpipe`、`a_cancelled_probe_completes_without_a_wait` | `#[cfg(unix)]` のまま。前の2つは unix のシグナル、最後は readiness の backend だけの性質で、Windows にない動き | — |
| `listen_accept_and_connect_are_unsupported` | 消す。Windows でソケットが動き、確かめる振る舞いがなくなる | 1 |

`crates/eml_vm/tests/api.rs` (async-net の後の名前。N-2):

| テスト | 扱い | 種類 |
|---|---|---|
| `a_real_connection_is_accepted`、`an_interrupt_stops_a_run_blocked_in_a_real_accept`、`an_interrupt_stops_a_run_waiting_on_a_lookup`、`a_timeout_around_a_slow_lookup_returns_none`、`a_real_connection_carries_bytes_between_tasks`、`a_real_read_chunk_within_keeps_the_stream`、`a_real_write_of_8_mib_arrives_whole`、`a_real_read_line_reads_lines_split_across_writes` (8本) | `#[cfg(unix)]` を外す。割り込みは `Control` で起こし、OS のシグナルを使わない | 1 に準じる |
| `a_real_listen_is_not_available` | 消す | 1 |

そのほか:
- `crates/eml_vm/tests/signal.rs` は印がなく、`defer(TERMINATE)` で起こすので、もう Windows で流れている。`crates/eml_cli/tests/cli.rs` の OS のシグナルのテストは `#[cfg(unix)]` のまま。
- 期待値の変更 (種類2) の範囲を `net-windows` の計画に先に書く。どれも CI で起きたときだけ使う。
  - (a) Windows で流し始めたテストのうち誤りの種類の集合を確かめるものは、VM が同じ行で扱う種類 (`ConnectionReset`、`ConnectionAborted`、`BrokenPipe`) を足してよい (`a_write_to_a_closed_peer_fails_with_reset_or_broken_pipe` のため)。
  - (b) 読まない相手への 8 MiB の書きが保留にならない Windows では、保留を作る書きを、保留になるまで書き足す形にしてよい (`quiesce_drains_a_pending_probe_a_full_write_and_a_running_job` のため)。
  - (c) ループバックの設定が効かず、拒まれた接続が `complete` の 1 秒の見張りを越えるときは、接続のテストの見張りだけを 5 秒にしてよい。
- UI テスト: `Net` の UI テストは `tests/ui/{run,run-fail}/async/net_*.em` でテストの driver の上で流れ、Windows でも印なしで流れる。付ける印も外す印もない。

新しく足すテスト (`src/sys/windows/wsa.rs` の `#[cfg(all(test, windows))] mod tests`)。std の `TcpListener` と `TcpStream` の対を `Socket::from` で包み、`Iocp` を直に使う。記録は `IoOp { io, slot: IocpSlot::default() }` を手で作り、`Table` に入れて `wait` に渡す。貸す記憶は `buf.rs` の `Lent::of(&mut Vec<u8>)` で作り、`Lent::new` は書かない。`Vec` はテストが完了まで生かす。

| テスト | 確かめること |
|---|---|
| `an_immediate_recv_still_completes_through_the_port` | データが届いた後の `Read` も `Start::Pending` で、続く `wait` が packet を1つ取り出して `Finished` の `Size(n)` (`n` は書いた長さ) を返し、その次の `wait(Some(0))` が `TimedOut` |
| `a_pending_recv_completes_through_the_port` | 出した後に相手が書くと `wait` が `Finished` で `Size(n)` を返す |
| `a_zero_length_recv_waits_for_data_and_leaves_it` | `Readable` が相手の書きで `Ok(Size(0))`、次の `Read` がそのバイトを読む |
| `a_cancelled_recv_ends_with_the_cancel_mark` | `cancel` の後の `wait` が `Finished { cancelled: true }`、その後 `WSAGetOverlappedResult` が `WSA_IO_INCOMPLETE` を返さない (落とす前にカーネルの書き込みが終わったことの確かめ) |
| `a_reset_peer_is_connection_reset` | 相手が `set_linger(Some(Duration::ZERO))` で閉じると、保留の `Read` が `ConnectionReset` |
| `a_reset_peer_still_makes_the_stream_readable` | 同じ形で `Readable` が `Ok(Size(0))`、次の `Read` が `ConnectionReset` |
| `drain_broken_frees_a_box_whose_cancel_finished` | 保留の `Read` を持つ表で `drain_broken` を呼ぶと戻り (abort しない)、箱が `Slot` に残って `in_flight` が `false` |
| `codes_are_normalized`、`a_reset_before_accept_is_an_aborted_accept`、`an_unrequested_abort_is_not_cancelled` | 写しの純粋な関数のテスト |

足さないテスト:
- 落とした後に相手が書くテスト: driver を落とすとソケットも閉じるので、相手の書きはどの道でも届かず、欠陥を見つけない。落とす前にカーネルが終わることは `a_cancelled_recv_ends_with_the_cancel_mark` と、付録1の `dropping_a_completion_driver_waits_for_cancelled_io` (核の順序) が確かめる。
- 受けの前に相手が切った接続を本物のソケットで起こすテスト: 順序を決めて起こせず、Linux では切れた接続も `accept` が返すので OS で結果が違う。出し直しは付録1の `an_aborted_accept_is_retried_under_the_same_key`、写しは純粋な関数のテストが受け持つ。

### B.9 作業の単位

N-0 `Plan net-windows`
- `docs/plans/<日付>-net-windows.md` を書く。B.8 の種類1の2件 (`listen_accept_and_connect_are_unsupported`、`a_real_listen_is_not_available`)、種類1に準じる31件 (real.rs の23本と api.rs の8本)、種類3の1件 (`a_wake_from_another_thread_leaves_a_wait_working`)、種類2の範囲 (a)(b)(c) を書く。
- 完了: `cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` → 通る

N-1 `Add overlapped reads and writes to the IOCP backend`
- 目的: 箱、`IocpSlot`、`WSARecv`、`WSASend`、取り消し、結果の読み方、`drain_broken`。ソケットの関数はまだ `Unsupported` なので、利用者に見える振る舞いは変わらない。
- 触るファイル: `Cargo.toml` (`"Win32_Networking_WinSock"`、`"Win32_Storage_FileSystem"`、`"Win32_System_WindowsProgramming"` を足す。試作で5つの feature の組で wsa.rs が通った。試作で使っていない `WSA_IO_INCOMPLETE` は WinSock の module にあることを grep で確かめた)、`src/sys/windows/wsa.rs` (新しい。`#![allow(unsafe_code)]`)、`src/sys/windows/iocp.rs` (`type Slot = IocpSlot`、`wait` の I/O の項目、`start` の `Readable`/`Read`/`Write`、`cancel`、`drain_broken`)、`tests/unsafe_code.rs` (`LISTED` に `src/sys/windows/wsa.rs`)
- docs: architecture.md の「`eml_io` の内部」に、箱と寿命、誤りの写し、成功のときに packet を省くモードを使わず結果を packet だけで読むこと、`CancelIoEx` の失敗を壊れた状態にすること、`drain_broken` の 1 秒と abort を書く。vm.md「時計と driver」の壊れた driver の箇条の「壊れるのは、OS の待ち … そのものが失敗したときだけである」に、「と、OS が取り消しの要求を断ったとき」を足す。
- テスト: B.8 の `wsa.rs` の単体テストのうち `a_reset_before_accept_is_an_aborted_accept` を除く9本 (テストのソケットは std で作って結び付けの関数に渡す)
- 完了の基準:
  - `.#cross` の clippy (`--target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings`) → 0
  - Linux で `cargo nextest run` → 失敗なし
  - `grep -rn "ECONNABORTED" crates/eml_io/src/sys` → 0行
  - `grep -rn "forget\|Box::leak" crates/eml_io/src` → 0行、`grep -rn "unsafe impl" crates/eml_io/src` → 0行、`grep -rn "process::abort" crates/eml_io/src` → `src/sys/windows/` の行だけ
  - `grep -rn "FILE_SKIP_COMPLETION_PORT_ON_SUCCESS" crates/eml_io/src` → 0行。`grep -rn "Lent::new" crates/eml_io/src` → `buf.rs`、`real/mod.rs`、`test.rs` の行だけ
  - CI の `test (windows-2025)` が緑で、`eml_io sys::windows::wsa::tests::` の9本が `PASS`
- 任せること: `OpState` の欄の並び (先頭が `OVERLAPPED` は守る)、`drain_broken` の眠る長さ (既定 1 ms)
- 確かめはレビューで読む: `drain_broken` の abort の道と、`CancelIoEx` の失敗を覚えて `wait` が `Err` を返す道 (偽の `CancelIoEx` を差し込めないので単体テストがない)

N-2 `Run sockets on the IOCP backend`
- 目的: `listen`、`socket`、`AcceptEx`、`ConnectEx`、結び付け、ループバックの設定を入れ、ネットのテストの組を Windows で流す。
- 触るファイル: `src/sys/windows/wsa.rs`、`src/sys/windows/iocp.rs` (`listen`、`socket`、`shutdown_write`、`set_nodelay`、`start` の `Accept`/`Connect`)、`crates/eml_io/tests/real.rs`、`crates/eml_vm/tests/api.rs`。`src/sys/socket.rs` は io-backends の形のまま使い、触らない。
- docs:
  - vm.md「時計と driver」: Windows の「まだソケットを扱わない」の文を消す。`Listen` の文に「Windows では `SO_REUSEADDR` を立てない (使っている番地への bind を許すため)」を足す。`Connect` に「Windows では、ループバックの番地への接続で SYN を送り直さない。拒まれた接続がすぐ誤りになる」を足す。
  - stdlib.md の `Std.Net`: Windows で `listen`、`accept`、`connect` が `NotAvailable` になる文を消す。
  - architecture.md: `wsa.rs` の行 (拡張の関数、`SO_UPDATE_*_CONTEXT`、作るときの結び付け)。結合テストの段落の Windows の `Unsupported` の文を消す。
  - status.md: `Std.Net` の Windows の文と「### Windows」の節。
  - testing.md: `api.rs` の段落の Windows の `NotAvailable` の文を消す。
  - async.md: 「`IoDriver` と `eml_io`」と「ネットワークとシグナル」に残る Windows の予告 (結び付け、`update_context`) を消す。
- テスト: B.8 の表と `a_reset_before_accept_is_an_aborted_accept`
- 完了の基準:
  - `.#cross` の clippy → 0。Linux で `cargo nextest run` → 失敗なし
  - `git grep -n 'Unsupported' -- crates/eml_io/src/sys/windows` → 0行
  - `git grep -c 'cfg(unix)\]' -- crates/eml_io/tests/real.rs` → `crates/eml_io/tests/real.rs:3` (`pool_threads_block_interrupt_and_terminate`、`a_wake_after_the_driver_is_dropped_raises_no_sigpipe`、`a_cancelled_probe_completes_without_a_wait` の3つ。補助の関数に印が残らない)
  - `git grep -n 'cfg(unix)\]' -- crates/eml_vm/tests/api.rs` → 0行、`git grep -n 'cfg(windows)' -- crates/eml_io/tests crates/eml_vm/tests/api.rs` → 0行
  - `git grep -n 'NotAvailable' -- docs/spec/stdlib.md docs/implementation/status.md docs/implementation/testing.md` の出力にネットの Windows の文がない (3つの文の消去を目で確かめる。ほかの `NotAvailable` の行は残る)
  - `git grep -n '^-- windows: skip (' -- tests/ui` の出力が N-1 の後と同じ
  - CI の `test (ubuntu-24.04)`、`test (ubuntu-24.04-arm)`、`test (macos-15)`、`test (windows-2025)` が緑。windows-2025 の出力で、`eml_io real::` のテストに上の3つの名前がなく、ほかの `real::` のテストがすべて `PASS`
- 任せること: 拡張の関数を取るときに使うソケット

N-3 `Close net-windows`
- roadmap の `net-windows` の項を消し、ほかの項の「前提」から段の名前を消す。async-net の閉じる作業がこの項に足した「`Terminate` を CTRL_CLOSE_EVENT と CTRL_SHUTDOWN_EVENT に写す」は、「その後の項目」の「非同期」に項目として移す。後に回した理由は「IOCP のソケットと別の話題で、コンソールの handler が戻ると系が 5 秒以内に process を終えるので、run を止める時間の決め方が要る (HandlerRoutine の文書)」、見直す条件は「Windows でサービスや閉じられるコンソールの中で eml を動かす使い道が出たとき」。
- `docs/plans/` の計画を消す。`eml_io` だけの変更なので計測は要らない。
- 完了: `git grep -n 'net-windows' -- docs CLAUDE.md ':!docs/plans'` → 0行。`git grep -n 'CTRL_CLOSE_EVENT' -- docs/future/roadmap.md` → 「その後の項目」の1行。CI の4行が緑

### B.10 CI (windows-2025) でしか確かめられないこと

型の検査は `nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings` で、`--all-targets` なので Windows の単体テストと `tests/real.rs` も検査される。link しないのでテストは流れない。git の worktree では flake の参照が libgit2 の誤りで止まることがあり、そのときは `nix develop "path:$PWD#cross" -c …` を使う。Miri は Windows の道を通らないので、Windows の unsafe は人が読むことと CI の本物のソケットのテストで確かめる。実装者の流れは、Linux で全体のテスト、`.#cross` の clippy、push して CI の4行を見る、の順である。Windows で落ちたら `cargo nextest run --failure-output immediate-final` の出力で直す。

1. 時間切れが `WAIT_TIMEOUT` で、起こしの packet と区別できること
2. 成功のときに packet を省くモードを立てないとき、同期に成功した呼び出しも packet を1つ積むこと
3. `CancelIoEx` の後の3つの終わり方と、取り消しの packet が届くこと
4. 拡張の関数を取れること、`SO_UPDATE_*_CONTEXT` の後に `peer_addr` と `shutdown` が動くこと
5. `WSAGetOverlappedResult` の誤りの番号 (`normalize` の要否)
6. ループバックの設定で拒まれた接続がすぐ終わること
7. `SO_REUSEADDR` なしで2つ目のリスナーが `AddrInUse` になること
8. 読まない相手への 8 MiB の `WSASend` が保留になること
9. 長さ 0 の `WSARecv` が読めるまで待ち、データを残すこと
10. 刻みの粗い待ちの下でも時間のテストが下限だけで通ること
11. ブロックしない設定 (`FIONBIO`) のソケットの上で、重なり合った `WSARecv`、`WSASend`、`AcceptEx`、`ConnectEx` が動くこと

### B.11 前提にした仮定と決めていない点

| ID | 項目 | 既定 |
|---|---|---|
| A-1 | async-net が計画どおりに閉じる (`pair` が消え、B.8 の名前のテストがある) | 崩れたら B.8 の表を名前で読み替える。`pair` が残れば IOCP の `pair` は `Unsupported` を返す |
| A-2 | 核の形は付録1のとおり | 変われば A.3 の一覧と `impl Backend for Iocp` の腕だけを読み替える |
| A-3 | `WSAGetOverlappedResult` は packet の WSA の番号を返す | 違えば `normalize` に行を足す |
| A-4 | windows-2025 は `TCP_INITIAL_RTO_NO_SYN_RETRANSMISSIONS` を受け付ける (Windows 10 1709 以降) | 崩れたら種類2の範囲 (c) を使い、vm.md の文は「Windows 10 1709 以降」と書く |
| A-5 | 読まない相手への 8 MiB の `WSASend` は保留になる | 崩れたら範囲 (b) |
| A-6 | 長さ 0 の `WSARecv` はデータを消費しない (compio と今の Windows の道が使う) | CI で確かめる |
| A-7 | 今の Windows に非 IFS の LSP はない | SKIP のモードを使わないので、崩れても安全は保つ |
| A-8 | `OvlBox` の生の番地と `addr_of!` の読みは Miri の対象外 | 人が読んで確かめる |
| U-1 | `a_write_to_a_closed_peer_fails_with_reset_or_broken_pipe` が `ConnectionAborted` を見るか | 変えずに流し、見たら範囲 (a) |
| U-2 | ループバックの設定の効き目 | 効くと見る。効かなければ範囲 (c) と vm.md の版の条件 |
| U-3 | `drain_broken` は本物の壊れた状態を CI で起こせない。単体テストは「取り消しの後に呼ぶと戻り、abort しない」だけを確かめる | abort の道と `CancelIoEx` の失敗の道はレビューで読む。計画に書く |
| U-4 | `GetQueuedCompletionStatusEx` の1回の項目の数 | 128。満ちたら待ちの長さ 0 で取り直す |
| U-5 | 核が trait を変えたとき | A.3 の一覧と `impl Backend for Iocp` の腕だけを直す。IOCP が成り立たない点が出たら、核の trait と偽の backend のテストを同じコミットで直す |

`Terminate` (CTRL_CLOSE_EVENT と CTRL_SHUTDOWN_EVENT) は `net-windows` に入れない。IOCP と別の話題で、前提のコード (async-net の Windows の形) が要るためである。N-3 で「その後の項目」へ移す。

出典: [WSARecv function](https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-wsarecv)、[AcceptEx function](https://learn.microsoft.com/en-us/windows/win32/api/mswsock/nf-mswsock-acceptex)、[SetFileCompletionNotificationModes](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setfilecompletionnotificationmodes)、[CancelIoEx](https://learn.microsoft.com/en-us/windows/win32/fileio/cancelioex-func)、[SIO_TCP_INITIAL_RTO](https://learn.microsoft.com/en-us/windows/win32/winsock/sio-tcp-initial-rto)、[HandlerRoutine](https://learn.microsoft.com/en-us/windows/console/handlerroutine)、[curl の mailing list](https://curl.se/mail/lib-2026-08/0002.html)、[libuv src/win/tcp.c](https://github.com/libuv/libuv/blob/v1.x/src/win/tcp.c)、[socket2 0.6.5 の sys/windows.rs](https://docs.rs/crate/socket2/0.6.5/source/src/sys/windows.rs)

## C. T-9 が roadmap の `net-windows` の項に書き写す文案

T-0 の後の `net-windows` の項 (「自前の IOCP の backend にソケットを載せ、同じネットのテストの組を Windows で流す。前提: `io-backends`」と、async-net が足した `Terminate` の文) の下に、次の箇条を足す。書き写すときに日本語の規則を当て直す。`docs/plans/` は引かない。

```
  - 記録ごとに `OVERLAPPED` の箱を1つ持ち、`WSARecv`、`WSASend`、`AcceptEx`、`ConnectEx` を出す。箱はカーネルが書き終えたと packet で確かめるまで放さない。確かめずに放す道に入った箱は abort する
  - ソケットは backend が作るときにブロックしない設定にし、IOCP のポートへ結び付ける。成功のときに packet を省くモードは使わない。非 IFS の LSP の下でこのモードが効かず、放した箱を読む道を残さないためである。同期に失敗した呼び出しのほかは、結果をすべて packet で読む
  - packet の誤りは `WSAGetOverlappedResult` で読み、WSA の番号にそろえる。頼んでいない中断 (995) は、取り消しの誤りにも OS の時間切れにもしない。`Readable` の誤りは unix と同じく `Ok(0)` にし、次の `Read` に誤りを言わせる
  - OS が取り消しの要求を断ったら、driver を壊れた状態にする。壊れた driver を落とすときは、取り消しを出して 1 秒まで終わりを確かめ、終わらなければ process を abort する
  - Windows の `SO_REUSEADDR` は立てない。ループバックへの接続では SYN を送り直さない設定を出し、拒まれた接続をすぐ誤りにする
  - Windows で流し始める `tests/real.rs` と `eml_vm/tests/api.rs` のテストは、成否の変更に準じて段の計画に1件ずつ挙げる
```
