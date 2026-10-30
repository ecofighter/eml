# io-backends: 自前の I/O driver への差し替え

- 作成: 2026-10-30
- 基準: `main` @ 625d088 (「3. 今の状態」と、付録を含むすべての `file:line` はこの状態を指す)
- 状態: 計画は確定。T-8 の macOS の計測など、利用者の手が要る項目が残る (「10. 危険と未決」)
- 付録: `2026-10-30-io-backends/` に、設計の全文 (A1〜A4) と、CI と flake の差分 (`ci-flake.diff`)、`bench/net/run.py` の差分 (`bench-peek.diff`) を置く。付録は決定の細部 (Rust の型、擬似コード、テストの名前、YAML、表) を持つ。本文の「5.3 方針の修正と食い違いの解き方」は付録より優先する

## 1. 目標と完了の条件

- 目標: `crates/eml_io` の本物の driver を、compio-driver の `Proactor` と `Asyncify` から、OS を呼ばない核と OS ごとの backend (Linux は epoll、Apple は kqueue、Windows は IOCP。Windows はソケットを扱わない) に替え、compio-driver、compio-buf、socket2 を外す。aarch64-linux を CI の対象に足し、syscall と CPU の目標を満たす。
- 完了の条件 (確かめ方は「9. 検証」):
  - `cargo nextest run`、clippy、fmt、feature の組の clippy が通り、CI の `test` が `ubuntu-24.04`、`ubuntu-24.04-arm`、`macos-15`、`windows-2025` で緑。
  - `eml_io` の核の Miri (2つの `MIRIFLAGS`) と ASan のジョブが緑 (ASan の脚を外したときは、計画に足して承認を得ている)。
  - `grep -n "compio\|socket2" Cargo.toml crates/*/Cargo.toml Cargo.lock` が空。
  - `bench/net` の目標の表 (D-14) の全行が合格する。主な目標: epoll の定常状態で、データのある読みは syscall 1回、書きは 1回、待つ読みは 3回、操作ごとの `epoll_ctl` は 0。many-self の 2000 本 ÷ 250 本の1接続あたりの CPU が 1.5 以下。
  - 書きの重みの単位 (`WRITE_UNIT`、512 バイト) を自前の driver の上で測り直し、決めた値と数が `docs/implementation/benchmarks.md` にある。

## 2. 範囲

- 含む: 核 (`src/driver/`)、backend (`src/sys/`)、自前のプール (32 本まで) と名前の解決のスレッド、`quiesce` と `Drop` と壊れた driver の規則、`unsafe` を閉じ込めるソースを読むテスト (`src/trust.rs`)、偽の backend の上の Miri、ASan と aarch64-linux の CI、flake の `aarch64-linux`、`bench/net` の覗きの数え方と目標の表、512 の測り直し、`docs/` の書き直し、段を閉じる作業。
- 含まない: Windows のソケット (`net-windows`)、io_uring (`io-uring`)、非同期のファイルとパイプ (`async-files`)、worker をまたぐ受け付け (`async-threads`)。ただし核と backend の境界は、これらを作り直しなしで載せられる形にする (D-1)。
- `IoDriver` の trait の関数の形、VM、std、`eml_extern` は変えない。trait の doc コメントと `docs/spec/vm.md` の規則の文は変える (D-6)。利用者は trait と spec の変更の提案を許したが、検討の結果、関数の形を変える利点はなかった。

## 3. 今の状態

- `IoDriver` は `crates/eml_rt/src/driver.rs:93` にある。`Op` は `Readable`、`Read`、`Write`、`Listen`、`Accept`、`Connect { host, port }`、`Job` で、`Submit` は `Done(io::Result<Done>)` か `Pending`。`wait`、`submit`、`cancel`、`completions`、`pending`、`waker`、`bin`、`shutdown_write`、`set_nodelay`、`close`、`quiesce` を持つ。
- 本物の driver は `crates/eml_io/src/real.rs` (900 行)。定数は `CANCEL_POLL` 10 ms、`POOL_LIMIT` 32、`CANCEL_LIMIT` 5 s、`BACKLOG` 1024。短い書きの残り、`ConnectionAborted` の受けの出し直し、接続の次の番地を `real.rs` が出す。`wait` のたびに操作の表を作り直し、1往復に2回確保する (`docs/implementation/benchmarks.md` の「34cad91 と e942eeb」)。操作ごとに compio に登録し直す (`docs/implementation/status.md:147`)。
- `unsafe` は `real.rs:135`、`:155` (`Mem` が `RawBuf` から slice を作る) と `mask.rs:16-34` (`pthread_sigmask`)、`test.rs:213`、`tests/real.rs:509`。
- 名前の解決は `resolve.rs` の自前のスレッド (生きているもの 8、手放したもの 64 まで、スタック 256 KiB) で、unix だけ。Windows は `Listen`、`Accept`、`Connect`、`shutdown_write`、`set_nodelay` が `Unsupported` を返す。
- VM の側: `POLL_EVERY` 16 (`crates/eml_vm/src/sched.rs:191`)、`WAIT_CAP_MS` 10 (`sched.rs:187`)、`BUDGET` 128 と `WRITE_UNIT` 512 (`crates/eml_vm/src/cells.rs:55-58`)。VM は `Op::Job` を作らず、`driver.close` を呼ばない (ハンドルは `HandleBin` で閉じる)。
- CI (`.github/workflows/ci.yml`) は `test` (ubuntu-24.04、macos-15、windows-2025)、`fmt`、`verus`、`miri` の4ジョブ。Miri は `eml_rt` の `trusted::`、`eml_vm` の `miri::` と `cells::a_read cells::a_write` だけで、`eml_io` はない。ASan も aarch64-linux もない。flake の `systems` は `x86_64-linux` と `aarch64-darwin`。nightly は `.#miri` だけにある。
- ソースを読んで `unsafe` を固定するテストの前例は `crates/eml_rt/src/trust.rs` (`include_str!` の `FILES`、コメントと文字列を消す、`every_source_file_is_listed`)。
- 依存: `compio-buf =0.8.3`、`compio-driver =0.12.5`、`socket2 =0.6.5`、`libc 0.2`、`windows-sys 0.61.2` (`Cargo.lock` に 0.61.2 だけがある)。
- 基準の状態で `cargo test -p eml_io` は integration 68 件が通る。x86_64 の ASan (`.#miri` の nightly、`-Zbuild-std`) で `eml_io` 68 件と `eml_vm` の `api::` と `signal::` 37 件が通る。リポジトリは public で、`ubuntu-24.04-arm` を無料で使える。

## 4. 前提

| 要るもの | 用途 | 確かめ方 | ないとき |
|---|---|---|---|
| Nix と devShell (`.#miri`、`.#cross`) | Miri、ASan、cross clippy | `nix develop .#miri -c rustc -V` → `rustc 1.101.0-nightly` 系。`nix develop .#cross -c cargo -V` → 0 | 止めて利用者に聞く |
| `strace`、`python3` | T-7、T-10 | `strace -V`、`python3 -V` → 0 | 止めて利用者に聞く |
| push と PR の CI | T-1 以降の macOS、Windows、arm、ASan の確かめ | 決定: 利用者は作業中の push と PR の CI を許した (ブランチ `claude/amazing-pascal-e12enm`) | - |
| macOS の機械 | T-8 の 512 の macOS の計測、T-11 の `bench/*.sh` | 決定: 利用者の Mac で流す (頼むときに止める) | D-14 の退路 (512 のまま、後の項目に回す) |
| 静かな Linux の機械 (4 vCPU 以上) | T-8、T-10 の計測 | `nproc`、`uname -r` を記録する | 同じ機械で前と後を交互に流せば判定は成り立つ |

## 5. 決定

### 5.1 長い方針 (変えるときは見直しが要る)

実装がこれらとぶつかったら、止めて報告する。各方針の細部は付録 A1 (S1) にある。適合の基準 (G-1〜G-19) は 5.4 に、修正を当てた形で置く。

#### D-1: 核と backend の境界

- 決定: 核 `Driver<B: Backend>` (`src/driver/`) が、`OpKey` で引く保留の表、届いた完了、`IoHandle` → ソケットの表 (保留数と閉じた印)、札 → `OpKey` の表、プール、解決器、`HandleBin`、壊れた印を持つ。続き (短い書きの残り、`ConnectionAborted` の受けの出し直し、接続の次の番地、解決の答えからの接続) は核だけが出す。backend (`src/sys/`) は OS の資源、時計、スレッドの作り方、fd ごとの状態 (読める記憶、向きごとの待ち手1つ、接続中の印、世代) を持ち、`start` 1回で「その歩の1回の試み」だけを OS に出す。trait は `Step`、`Outcome`、`Hold`、`Start`、`Cancel`、`Arrival`、`Event`、`Backend` で、全文は A1 の Q1。readiness backend は `Arrival::Retry` と `Start::Pending(Hold::None)` と `Cancel::Done` だけを使う。完了型の値 (`Hold::Buffer`、`Cancel::Pending`、`Arrival::Done`/`Cancelled`) は暫定で、最初の完了型 backend の段 (`net-windows` か `io-uring`) で形を変えてよい。
- 理由: 続きの規則を OS ごとに重複させず、偽の backend の上で Miri で確かめるため。readiness の記憶は epoll と kqueue にだけ意味があるので backend に置く。
- 退けた案: fd ごとの状態を核に置く — 完了型 backend で死んだ欄になる。mio — Windows で AFD の readiness を真似る。
- 及ぶ所: `eml_io` のすべて。後の `net-windows`、`io-uring`、`async-files`。
- 見直す条件: 完了型 backend を載せる段。
- リポジトリに残す所: `docs/implementation/architecture.md` の「`eml_io` の内部」(T-6)。trait のコメントに暫定の旨 (T-3)。

#### D-2: 操作の表と札

- 決定: 鍵は呼ぶ側の `OpKey`。表 (`HashMap`) は作り直さず使い回す。`completions` は `done` を `append` するだけ。`turn(timeout)` (`wait`、`quiesce`、`Drop` が共有) は受け箱を引き取り、`backend.wait` に空の `events` を渡し、event が名指しした欄と札で当たった欄だけに触れる。プールと解決器の答えは driver の中で使い回さない `u64` の札 `Ticket` で当て、表にない札の答えは捨てる。`wait(deadline)` が `Ok(false)` を返すのは、`deadline == None` で保留の欄と届いた完了がともに空のときだけ。`u64::MAX` は `Ok(false)` を返さない。ミリ秒は切り上げ、OS の上限で切るのは早く戻る向きだけ。
- 理由: 保留が多くても1接続の費用が本数によらないため。使い回した鍵に古い答えを当てないため。
- 退けた案: `wait` で表を歩く — 今の driver の費用そのもの。
- 及ぶ所: 核。
- 見直す条件: many-self の比が 1.5 を越えたとき。
- リポジトリに残す所: `docs/spec/vm.md` の「時計と driver」(T-6)。

#### D-3: プールと解決器のスレッド

- 決定: プールは自前の std のスレッド (`Mutex` と `Condvar`、32 本まで、満ちたら driver の中の列で待つ)。`submit` は呼んだスレッドを止めない。スレッドを作る条件は `queue.len() > idle && threads < limit`。仕事は `catch_unwind` で走り、panic は `wait` か `quiesce` の終わりで持ち主のスレッドで起こし直す (`Drop` では起こし直さない)。スレッドを作れなかった仕事は1回だけ `Err` で終わる。スレッドは lock の下で `running` から自分を外し、lock を放してから答えを受け箱に積む。プールと解決器のスレッドは `Backend::spawn` で作り、本物の backend は `spawn_masked` で実装する。`JoinHandle` を持ち、`Drop` は走っていないスレッドを join し、走っているものを切り離す。空いたスレッドは 10 s で抜ける。解決器は今の上限 (8 と 64) を保ったまま核に移し、札で当てる。OS の解決 (`std_lookup`) は `sys::lookup` に移す。`with_lookup` は全 OS に置く。`Pool::new(inbox, limit)` で、核は `POOL_LIMIT` (32) を渡す。
- 理由: 安全点を通らない待ちをなくすため。spec の panic の規則とシグナルの塞ぎを保つため。Miri のテストがスレッドを残さないため (Miri は残ったスレッドを誤りにする)。
- 退けた案: S1 の元の「`idle == 0` なら起こす」 — 1つの空きスレッドに2つ積むと起こしが失われ、試作で止まった。答えを積んでから `running` を外す順 — driver をすぐ落とすと答えたスレッドが切り離されうる。
- 及ぶ所: `driver/pool.rs`、`driver/resolve.rs`、`driver/inbox.rs`、本物の backend の `spawn`。
- 見直す条件: `ffi-blocking` で利用者の仕事がプールに載るとき。
- リポジトリに残す所: `docs/spec/vm.md` の「時計と driver」、architecture.md (T-6)。

#### D-4: 壊れた driver、`quiesce`、`Drop`

- 決定: 壊れるのは `backend.wait` の `Err`、`backend.cancel` の `Err`、`quiesce` が取り消しの完了を `CANCEL_LIMIT` (1 s) まで待っても届かないときの3つだけ。壊れた後は `wait` が `Err`、`submit` が `Op::Job` も含めて `Submit::Done(Err)`、`completions` が空、`cancel` は OS を呼ばず、`quiesce` は保留を残して戻る。`quiesce` は仕事を待ちきり (VM の記録の寿命のため、上限なし)、解決を手放し、ほかを取り消して完了を 1 s まで待つ。`Drop` は仕事も解決も待たず、`Hold::Buffer` の欄だけを `HELD_LIMIT` (1 s) まで待ち、残れば `process::abort()`。`backend.wait` の `Err` で壊れていれば待たずに abort する。この段の backend は `Hold::Buffer` を返さないので、`Drop` は待たない。`mem::forget` と `Box::leak` を書かない。今の `CANCEL_POLL` (10 ms) と `CANCEL_LIMIT` (5 s) は消す。
- 理由: roadmap の規則。失われた完了で終わりが止まらないため。
- 退けた案: `quiesce` も仕事を待たない — VM の操作の記録が仕事より先に消える。
- 及ぶ所: 核。
- 見直す条件: 完了型 backend を載せる段。
- リポジトリに残す所: `docs/spec/vm.md` の「時計と driver」、`crates/eml_rt/src/driver.rs` のコメント (T-6)。

#### D-5: `unsafe` と検証の組

- 決定: `unsafe` は `UNSAFE_FILES` (`mask.rs`、`test.rs`、`sys/unix.rs`、`sys/epoll.rs`、`sys/kqueue.rs`、`sys/iocp.rs`、`driver/tests/fake.rs`) にだけ置く。各ファイルは `#![allow(unsafe_code)]` で始まり、`lib.rs` は `#![deny(unsafe_code)]`。範囲は `src/` で、結合テスト (`tests/`) は対象外。本物の driver は `RawBuf` の `ptr` を `sys/*` の OS を呼ぶ関数の中でだけ読み、slice を作らない。偽の backend も slice を作らず `copy_nonoverlapping` で書く。`src/trust.rs` (前例は `eml_rt/src/trust.rs`) がこれを固定する。核は偽の backend の上で Miri (今の2つの `MIRIFLAGS`) で流す。OS を呼ぶ部分は4つの対象の CI と、x86_64-linux、aarch64-linux、aarch64-apple-darwin の ASan で確かめる。道具の不具合で緑にできない ASan の脚は、計画と testing.md とロードマップに書いて外せる。
- 理由: roadmap の「unsafe は一覧のファイルに閉じ、ソースを読むテストで固定する」と「Miri で動かせない部分は CI と ASan で確かめる」。
- 退けた案: 偽の backend の写しを `test.rs` に置いて一覧を6つに保つ — 関係のない2つの driver が関数を共有する (利用者が一覧に足す案を選んだ)。
- 及ぶ所: `eml_io` の `src/`、CI。
- 見直す条件: 新しい backend か新しい `unsafe` のファイルを足すとき。
- リポジトリに残す所: `src/trust.rs` (T-2、T-3、T-6)、testing.md の新しい節「`eml_io` の検証」(T-1、T-2、T-3)。

#### D-6: `IoDriver` の契約の変更

- 決定: trait の関数の形は変えない。変えるのは規則の文で、`docs/spec/vm.md` の「時計と driver」と `driver.rs` の doc コメントに書く: 取り消しと `quiesce` と `Drop` の規則 (D-4)、プールは止めない (D-3)、壊れた driver の `submit` は `Op::Job` も `Done(Err)`(「`Op::Job` はいつも `Pending`」を「壊れていない driver では」に絞る)、「OS を見るのは `wait` だけ」の言い方、本物の driver は slice を作らない (vm.md 468 付近。`RawBuf` の貸しの規則 465 は変えない)、`!SOCKETS` の backend では `shutdown_write` と `set_nodelay` も `Unsupported` で、どれもハンドルを引く前に決まる、`quiesce` は仕事の panic を待ちきった後に起こし直す。据え置くもの: `Ok(false)` の条件、`Submit::Done` の即時の決着、`Readable` の意味、`close` と `HandleBin`、`Completion::cancelled`、操作の予算。
- 理由: 関数の形を変えても VM の側に得がなく、規則の文だけが変わるため。
- 及ぶ所: `docs/spec/vm.md`、`crates/eml_rt/src/driver.rs` のコメント (`verus!` の外なので Verus の門はない)。
- リポジトリに残す所: 同上 (T-6)。

#### D-7: crate とモジュールの配置

- 決定: `eml_io` は1つの crate のまま。木は次のとおり。依存は `eml_rt`、unix の `libc`、windows の `windows-sys` 0.61.2 (`Win32_Foundation`、`Win32_System_IO`、`Win32_System_Threading`)。対応しない OS は `compile_error!`。

```
src/lib.rs           #![deny(unsafe_code)]、mod driver; mod sys; mod real; mod test; mod mask; mod flag; #[cfg(test)] mod trust;
src/real.rs          RealDriver = Driver<sys::Os> の包み。new() は with_lookup(sys::lookup)
src/driver/mod.rs    核 (表、turn、wait、submit、cancel、quiesce、Drop)
src/driver/backend.rs  Backend の trait と型
src/driver/inbox.rs  受け箱 (札付きの答え)
src/driver/pool.rs   プール
src/driver/resolve.rs  解決器 (札、上限)
src/driver/tests/{mod.rs,fake.rs,abort.rs}  核の単体テスト、偽の backend、abort の子プロセスのテスト
src/sys/mod.rs       type Os、millis、lookup
src/sys/unix.rs      Unix<P: Poller>: fd の表、歩、Backend の実装、sockaddr、spawn
src/sys/epoll.rs     Poller (登録、待ち、eventfd の waker)
src/sys/kqueue.rs    Poller (登録、待ち、EVFILT_USER の waker)
src/sys/iocp.rs      Iocp (SOCKETS = false、waker、待ち)
src/trust.rs         ソースを読む unsafe のテスト
```

- 理由: 表と歩を `unix.rs` にまとめると epoll と kqueue で重複しない (A2 U-0)。`core` という名前は `core` crate と紛れるので使わない。
- 及ぶ所: `eml_io`。
- リポジトリに残す所: architecture.md の「`eml_io` の内部」と「外部 crate」(T-6)。

#### roadmap の文からの逸脱 (段の計画に書くと決めたもの)

- 向きの規則 (G-5): 1つの handle の1つの向きに保留は1つ。核が `submit` で `InvalidInput` を返して守り、backend の fd ごとの待ち手は向きごとに1つで列を持たない。std の `TcpStream` と `TcpListener` は `Lin` なので VM はこれを破らない。後で双方向の handle を同じ向きで2つ待たせるときは、待ち手を `VecDeque<OpKey>` にして核の守りを外す。
- `Readable` の覗きの省き方: roadmap は「記憶が立っていれば1バイトを覗いて確かめ」と書くが、行に `ready` と `fresh` を持ち、`ready && fresh` なら syscall なしで答える。記憶で答えた `Peek` も `fresh` を下ろす (これがないと `a_probe_on_a_reset_stream_fails_at_submit` が落ちる)。`ready && !fresh` なら1バイト覗き、`!ready` なら待つ。kqueue は `a_drained_event_does_not_make_a_probe_fresh` が macOS で落ちたら記憶で答えない (`fresh: false` の1行)。これで待つ読みは空きの有無によらず 3 syscall。
- 接続のソケットの登録: `connect()` の直後に edge で登録する (前に登録すると `EPOLLOUT|EPOLLHUP` の余分な event が来る。Linux 6.18 で確かめた)。再試行は `connect()` を出し直さず、`SO_ERROR` と `getpeername` を読む。
- リスナーは読みの関心だけを登録する。

### 5.2 短い期間の設計

理由があれば実装中に直してよい。直したらコミットメッセージに書く。

#### D-10: epoll と kqueue の backend (付録 A2)

- `Unix<P: Poller>` が表と歩と `Backend` を持ち、`Poller` は `new() -> io::Result<(Self, Arc<dyn IoWaker>)>`、`register`、`poll` の3つ。`Poller` と `Ready` は `pub(crate)` (`pub(super)` は `private_bounds` に当たる)。`type Os = unix::Unix<epoll::Epoll>` (Linux)、`unix::Unix<kqueue::Kqueue>` (Apple)。
- `Sock { fd: OwnedFd, row: u32, gen: u32 }`。OS に渡す識別子は `gen << 32 | row` を1か所で作り、waker は `u64::MAX`。`close` は `EPOLL_CTL_DEL` を出さない。新しい行は `ready = false`。
- epoll の event: 読み = `IN|RDHUP|HUP|ERR`、書き = `OUT|HUP|ERR`。kqueue は各フィルタが自分の向きだけを起こす。
- `recv`/`send` はいつも `MSG_DONTWAIT`。Linux は `MSG_NOSIGNAL`、`accept4` と `socket` に `SOCK_NONBLOCK|SOCK_CLOEXEC`。Apple は `socket`/`accept` の後に `fcntl(FD_CLOEXEC)`、`ioctl(FIONBIO)`、`SO_NOSIGPIPE`。
- `accept` の一時的な誤り (Linux の accept(2) が出し直せと書く8つ) は backend が `ConnectionAborted` に包み、核の出し直しに乗せる。`EMFILE` などの資源の誤りはそのまま返す。
- `connect` の `EINTR` は「接続中」と読み、出し直さない。`kevent` の変更の呼び出しが `EINTR` なら出し直す。
- waker: Linux は level の eventfd (event が来たら読む)、Apple は `EVFILT_USER` と `NOTE_TRIGGER`。待ちは `epoll_pwait` (sigmask は null、`millis(d, i32::MAX as u32)`)、kevent の timespec (秒は `i32::MAX` まで)。`EINTR` は `Ok(())` で events が空。
- `epoll_ctl` の数: driver を作るときに eventfd の1回、登録したソケットごとに1回、往復あたり 0。
- 単体テスト9件 (`sys/unix.rs`) と結合テスト6件 (`tests/real.rs`)。名前と中身は A2 §1 の表。

#### D-11: Windows の IOCP の backend (付録 A3)

- `SOCKETS = false`。`Sock` は値のない enum で、`start`、`cancel`、`close`、`shutdown_write`、`set_nodelay` は `match *sock {}`。`listen` と `socket` は `Unsupported`。
- waker は `PortWaker { port: OwnedHandle, posted: AtomicBool }` を `Arc` で共有し、鍵 `WAKE = 0` の packet を高々1つ積む。積むのに失敗したら `posted` を立てたまま 1 ms ごとに出し直す。`wait` は packet を取った後で `posted` を下ろす。driver のスレッド自身が積む答え (スレッドを作れなかったとき) は `Inbox::put` で、鳴らさない。
- `wait` は `GetQueuedCompletionStatusEx` (64 件まで)。ミリ秒は `millis(d, INFINITE - 1)`、`None` は `INFINITE`。`WAIT_TIMEOUT` の失敗は `Ok(())`、ほかの失敗は `Err`。`unsafe` は4か所。

#### D-12: プール、解決器、受け箱の細部 (付録 A3)

- 受け箱は `Mutex<Vec<(Ticket, Answer)>>` と waker。`push` の後に `wake`。核の `arrived` は使い回す `VecDeque` で、`pop_front` で引き取る (`mem::take` を使わない)。引き取るのは `receive` 1つで、解決器のスレッドを join してから札を引く。`turn` は `backend.wait` の前後で引き取り、前の引き取りで完了が着いていれば timeout 0 で待つ。
- 解決器の `submit` は `is_finished` で答えたスレッドを手放した数から外してから上限を当てる。
- 偽の backend の答えの型の名前は `Reply` にする (受け箱の `Answer` と `driver::tests` の `use super::*` で紛れないため)。

#### D-13: 検証と CI (付録 A4 の Q1〜Q3、`ci-flake.diff`)

- 偽の backend (`driver/tests/fake.rs`) は鍵ごとの台本で `start` に答え、呼び出しを記録する。`wait(Some(d))` は偽の時計を `d` 進めて戻る。本物のスレッドが走っている間に `d > 0` で呼ぶと panic する。本物のスレッドを持つテストは `wait(None)` か、偽の backend の `finished` を待ってから `wait(Some(0))` を使う。これで `CANCEL_LIMIT` と `HELD_LIMIT` の道を実時間なしで通す。
- abort のテストは `driver/tests/abort.rs` で、テストのバイナリを子として流す。子のテストの道は `module_path!()` から `eml_io::` を除いて作る。`#[cfg_attr(miri, ignore)]`。
- Miri: `cargo miri test -p eml_io --lib -- driver::tests::` を CI の `miri` ジョブの4つ目の step にする。予算は既定 Stacked 120 s、Tree 160 s で、T-3 で測って testing.md の規則で置き換える。
- `trust.rs`: `src/` のすべての `.rs` を `FILES` に載せ、`SKIPPED = ["trust.rs"]`。テストは7つ (A4 Q2) で、禁じる字面は G-1、G-3、G-8、G-13、G-15、G-16 の grep と同じにし、それより広げない。`FILES` は実際にあるファイルと一致させる (`driver/tests/pool.rs` はファイルを分けたときだけ載せる)。
- CI: `test` の matrix に `ubuntu-24.04-arm`。新しい `asan` ジョブ (Linux x86_64、Linux aarch64、macOS。`.#miri` の nightly、`-Zsanitizer=address`、`-Zbuild-std`、`--target`、`eml_io` と `eml_vm` の `api::` と `signal::`)。flake の `systems` に `aarch64-linux`、`hasVerus` の切り替えで aarch64-linux の devShell から Verus を外す。`.#cross` に `aarch64-unknown-linux-gnu`。差分は `ci-flake.diff` (actionlint と `nix eval` で確かめてある)。
- 新しい対象 (arm、ASan) で落ちたテストの仕分け: ASan の本物の報告は直すべき不具合。道具の失敗 (runtime がない、リンク、rpath、時間切れ) は、その脚を fenix と同じ日付に固めた rustup の nightly にし、timeout を 90 分にする。それでも緑にならなければ脚を外し、計画、testing.md、ロードマップに書く。arm の失敗は、コードの不具合 (直す)、機械に依る値 (期待値の変更)、OS にない振る舞い (成否の変更と cfg の印)、見張りの時間切れ (測って延ばす) に分け、不具合以外は1件ずつ計画に足して承認を得る。

#### D-14: 計測と目標 (付録 A4 の Q4、`bench-peek.diff`)

- `bench/net/run.py` は `MSG_PEEK` の `recvfrom` を `recvfrom(peek)` として別に数え、`report.py` は目標の表を出す。
- 目標の表 (strace の3回すべてで満たす。記号と 625d088 の値は A4 Q4):

| 項目 | 件 | 合格 |
|---|---|---|
| 書き 1 | bulk-send-py | ≤ 1.05 |
| データのある読み (空きあり) 1 | echo-py-reuse | ≤ 1.05 |
| 1往復 | echo-py-reuse | ≤ 4 |
| 空きなしの読みは覗きと読みの 2 まで | bulk-recv-py、echo-py-fresh | (R + P) ÷ R ≤ 2.02 |
| 待つ読み 3 | echo-self-fresh | ≤ 3.05、かつ E ≥ 1.5 /往復 |
| 操作ごとの `epoll_ctl` 0 | echo-self-fresh、echo-py-reuse | ≤ 0.005 /往復 |
| `epoll_ctl` の総数 | echo-py-reuse | ≤ 2 (eventfd 1 + 接続のソケット 1) |
| 受けと登録 | conn-self-32 | `accept4` ≤ 1.05、`epoll_ctl` ≤ 2.005 /接続 |
| 待ち1回の費用 | echo-self-fresh | ≤ 0.05 |
| 本数によらない費用 | many-self-250、-2000 | 1接続あたり CPU の比 ≤ 1.5 |

- 外れたら不具合として扱い、`strace -f -o` で出どころを特定する。核か backend にあれば直してから閉じる。driver の外にあれば、原因と数を benchmarks.md の目標の表に書き、ロードマップの「その後の項目」に見直す条件付きで足す。目標の数を書き換えるときは計画に書いて承認を得る。many-self の比が外れたら `perf record -g` で many-self-2000 を見る。
- 512 の測り直し: 候補 256 と 1024。手順と判定 (a) bulk のどれかで候補に「差」、(b) どの件にも候補に不利な「差」がない、(c) bulk-self/shared/reuse の単位あたりの sys が 1.10 倍以下で、echo-self-fresh/reuse の実時間の比が 1.10 以下、を Linux と macOS の両方で満たすときだけ替える。両方が通れば bulk の比の幾何平均が小さいほう。コマンドは A4 Q4。macOS の run が段の中でできなければ 512 のままにし、ロードマップの「その後の項目」に「macOS での書きの重みの単位」を足す。
- 前と後の比べは 512 を決めた後、コードの最後のコミットで流す (`git archive 625d088` の eml を before にする)。比べの表はコミットメッセージに書き、benchmarks.md には今の値だけを書く。

#### D-15: 書き直す文書

付録 A4 Q5 の表に、次を加えて従う。

- `docs/spec/vm.md` の「時計と driver」: D-6 の文 (S3 の補い: `!SOCKETS` の `shutdown_write`/`set_nodelay`、`quiesce` の panic の起こし直し、プールの `Drop` が列の仕事を走らせずに捨てること、起こす条件 `queue.len() > idle`) を含める。
- `docs/future/async.md:28` の compio の記述も直す。
- `docs/future/roadmap.md` の `net-windows` の項に未決を1つ足す: IOCP では重なった I/O の間カーネルが backend の `OVERLAPPED` に書くので、長さ 0 の `WSARecv` を含むすべての保留を `Hold::Buffer` にするか、3つ目の値を足すか、IOCP の backend の `Drop` で待つかを、その段の計画で決める (A3 §6-6)。
- 歴史は書かない。benchmarks.md の新しい節は測るものの名前 (「driver の syscall の目標」「書きの重みの単位」) で立て、今の値だけを置く。「34cad91 と e942eeb」の節は消しも広げもしない。

#### D-16: テストの変更

- 成否の変更 (1件): `crates/eml_io/tests/real.rs` の `dropping_a_driver_waits_for_a_running_job` を消し、`dropping_a_driver_does_not_wait_for_a_running_job` を足す (中身は A4 Q6。時間を測らず、仕事の中の 10 s の見張りで確かめる)。理由: D-4 で `Drop` が仕事を待たなくなる。
- 期待値の変更 (範囲): 512 を変えるときだけ、`cells.rs` の `writes_spend_budget_by_their_length` の長さと `spent`、書きの重みで変わる UI テストの出力と `RunStats` の snapshot (`crates/eml_vm/tests/*.rs`、`tests/ui/run/`)。arm と ASan の仕分けで見つかったものは、見た後で1件ずつ足す。
- 機械的な追随: `tests/real.rs:394` の socket2 の linger を `reset_on_close` (`libc::setsockopt` の `SO_LINGER`。`TcpStream::set_linger` は 1.99.1 で unstable) に替える。`tests/real.rs:930` のコメントの compio を driver にする。`a_job_pool_is_capped` の説明を「それを超える仕事は driver の中の列で待ち、スレッドが空いてから動く」にする。
- `a_job_pool_is_capped` に、32 の仕事を門で止めた間に 33 本目の `submit` が戻ることの確かめを足す (核の `POOL_LIMIT` の配線を本物の driver で確かめるため)。
- 残すもの: `tests/real.rs` の 1 s の見張り6か所と `eml_vm/tests/api.rs:376` の 2 s。どれも ms の桁の待ちの見張りで、測った時間の 10 倍以上の規則を満たす。新しい runner で切れたら D-13 の仕分けで延ばす。誤りの文言 `cancelled`、`too many name lookups are stuck`、`no such handle`、`unsupported` は保つ。
- 足すテスト: 核とプールと解決器 (`src/driver/tests/`、Miri の対象)、本物の backend (`tests/real.rs`)、backend の中 (`sys/unix.rs` の `mod tests`、`sys/mod.rs` の `millis`)。名前は A2 §1、A3 Q6、A4 Q1 と Q6。新しいテストはこの一覧に載せない。

### 5.3 方針の修正と食い違いの解き方

付録は設計の時点の文で、次の修正と解き方がそれに優先する。

- 利用者が決めた方針の修正 (適合の確かめの後):
  - G-15: `UNSAFE_FILES` に `driver/tests/fake.rs` を足して7つにし、範囲を `src/` とする (付録 A4 の C1 を採る)。
  - G-1: 核が使ってよいスレッドの道具に `thread::current`、`ThreadId`、`thread::Builder` を足す (スレッドを作るのは `backend.spawn` だけのまま)。
  - A1 Q1 と Q4 の「長さ 0 の `WSARecv` は `Hold::None`」を消し、`net-windows` の未決にする (D-15)。この段のコードは変わらない。
  - G-17: 道具の不具合で緑にできない ASan の脚を、計画と testing.md とロードマップに書いて外せる。
- 実装順の食い違い (適合の確かめの Cf-1): A2 の `epoll.rs` は `super::millis` を呼ぶので、`sys/mod.rs` と `millis` は epoll と kqueue と同じコミット (T-4) で作る。T-5 は Windows の `Os` の行と `iocp.rs` を足す。`sys::lookup` と `real.rs` の `with_lookup` は差し替え (T-6) で入れる。A2 の U-S2-1/U-S2-2、A3 §5 の 1〜3、A4 の 0〜8 の番号は、この計画の T 番号に読み替える。
- G-7 の確かめ方の内訳は「eventfd 1 + 接続のソケット 1」(echo-py-reuse では eml は接続する側)。「読み書き両方の関心」はリスナーを除く。
- G-19 の grep は `benchmarks.md` を除く (A4 C2)。代わりに `grep -n compio docs/implementation/benchmarks.md` の当たりが「34cad91 と e942eeb」の節の中だけであることを確かめる。
- abort のテストは `driver/tests/abort.rs` に置く (A1 §5 の `driver::abort::` ではない)。
- A1 Q3 のプールの手順は D-3 の形 (A3 の補い) に従う。
- A4 の r1 の E1 と E3 は取り下げ済み。
- 実行中の決定 (U3 のレビュー): `wait` は届いた完了があっても OS を timeout 0 で1回確かめる (A1 Q2 の形)。A4 Q1 の `a_wait_after_a_cancel_returns_true` の「`calls` に `Wait` がない」は「`Wait(Some(0))` の1回だけ」と読む。
- 実行中の決定 (U3 のレビュー): 完了型の値が暫定であることのコメントは `driver/backend.rs` の trait と型に置く。`crates/eml_rt/src/driver.rs` のコメントは T-6 で D-6 の文と一緒に直す。

### 5.4 適合の基準

5.3 の修正を当てた形。各タスクのレビューはこれでコードを確かめる。`Why` と `Applies to` の全文は A1 §8 にある。

- G-1 核 (`src/driver/` の `tests/` を除くすべてと `src/real.rs`) は OS を呼ばず、`libc`、`windows_sys`、`std::time::Instant` を名指ししない。スレッドの道具は `std::sync`、`JoinHandle`、`thread::current`、`ThreadId`、`thread::Builder` で、作るのは `backend.spawn(` だけ。確かめ: `trust.rs` の `the_core_names_no_os`。`grep -rn "\.spawn(" crates/eml_io/src/driver --exclude-dir=tests` が `backend.spawn(` の行 (pool.rs と resolve.rs) だけ。
- G-2 続きは核だけが出す。`start` は歩の1回の試みだけで、`EINTR` のほかは試し直さない。確かめ: `sys/unix.rs` の `mod tests` より上のループが `EINTR` を繰り返す `syscall` の関数だけ。核のテストが4つの続きを確かめる。
- G-3 readiness backend は `Arrival::Retry`、`Hold::None`、`Cancel::Done` だけを使う。確かめ: `trust.rs` の `readiness_backends_report_only_retries`。
- G-4 歩は再試行に耐え、完了は1回。待ち手は `start` の `Done`、`cancel` の `Done`、`close` で忘れる。`Connect` の再試行は `connect()` を出し直さない。確かめ: 核のテスト「`Retry` を2回届けても完了が1回」、`a_refused_connect_completes_once`。
- G-5 1つの handle の1つの向きに保留は1つ (2つ目は `InvalidInput`)。確かめ: `a_second_op_in_one_direction_is_invalid_input`。
- G-6 fd ごとの状態は backend の行に置き、OS に渡す識別子は行番号と世代。行は `close` でだけ放し、`wait` は世代の合わない event を捨てる。確かめ: 識別子を作る所が1か所で世代を含む。
- G-7 ソケットは1回だけ edge で登録し、以後変えない。確かめ: D-14 の `epoll_ctl` の3行。`grep -c "epoll_ctl(" crates/eml_io/src/sys/epoll.rs` が 2、`grep -c "register_row(" crates/eml_io/src/sys/unix.rs` が 4。
- G-8 核の表は使い回し、`completions` は `append` だけで、`turn` は名指しされた欄にだけ触れる。確かめ: `trust.rs` の `the_core_walks_its_table_only_to_shut_down`。many-self の比。
- G-9 スレッドの答えは札で当て、取り消しと `quiesce` は札を外す。確かめ: `a_late_answer_of_a_cancelled_lookup_does_not_hit_the_reused_key` (保留の欄を残して `wait(Some(0))` で `turn` を通す形)。
- G-10 プールは D-3 のとおり。確かめ: プールのテスト、`a_job_pool_is_capped` の 33 本目の確かめ、`pool_threads_block_interrupt_and_terminate`。
- G-11 waker は鳴らす OS の物体を自分で所有し、鳴らした印は次の `wait` が消費するまで残る。`push` の後に `wake`。確かめ: `a_wake_after_the_driver_drops_is_harmless`、`wakes_before_a_wait_end_only_that_wait`。
- G-12 壊れる条件と壊れた後の振る舞いは D-4 のとおり。確かめ: `a_failed_wait_breaks_the_driver`、`quiesce_breaks_on_a_cancel_that_never_completes`。
- G-13 `quiesce` と `Drop` は D-4 のとおり。`mem::forget` と `Box::leak` を書かない。確かめ: `grep -rn "mem::forget\|Box::leak" crates/eml_io/src --exclude=trust.rs` が空。`dropping_a_driver_does_not_wait_for_a_running_job`、abort のテスト。
- G-14 `close` は保留があれば遅らせる。ソケットの所有は1つで、核の `Sock` は `backend.close` でだけ落とす。確かめ: 偽の `FakeSock` は `Clone` を持たない。G-14 の核のテスト2件。
- G-15 `unsafe` は `src/` の `UNSAFE_FILES` (7つ) にだけある。本物の driver と偽の backend は slice を作らない。確かめ: `cargo nextest run -p eml_io trust::`。`grep -rn "from_raw_parts" crates/eml_io/src --exclude=trust.rs --exclude=test.rs` が空。
- G-16 核は errno を使わず `io::ErrorKind` だけで判断する。backend が自分の記憶をカーネルに渡したら、OS が終わりを告げるまで放さない。確かめ: `trust.rs` の `the_core_names_no_os`。
- G-17 検証の組は D-5 と D-13 のとおり。ふだんのテストは1件 1 s 以下で機械の速さに依らず、Miri のテストは出したスレッドの仕事を終えてから driver を落とす。確かめ: `ci.yml` に該当のジョブと step。`cargo nextest run -j 1 -p eml_io` で `SLOW` が出ない。
- G-18 依存は D-7 のとおり。確かめ: `grep -n "compio\|socket2" Cargo.toml crates/*/Cargo.toml Cargo.lock` が空。Windows の cross clippy が通る。
- G-19 文書は実装と同じコミットで書き換え、コードのコメントは `docs/` の見出しを引く。`docs/plans/` を引かない。確かめ: `cargo nextest run -p eml_cli --test integration citations::`。`grep -rn compio docs/spec docs/implementation CLAUDE.md --exclude=benchmarks.md` が空。

## 6. 実装の制約

- テストを先に書く (CLAUDE.md)。各タスクで、足すテストを先に入れて落ちることを見てから実装する。
- コメントと文書は日本語で、natural-japanese の skill に従う。コメントは理由を書き、規則は `docs/` の見出しを引く。D 番号、T 番号、この計画を引かない (G-19)。
- `.em` の試しの実行は `timeout 20` の下で流す。
- T-3 と T-4 と T-5 の間、新しいモジュールは `#[cfg(test)] mod driver;` と `#[cfg(test)] mod sys;` にし、先頭に `#![allow(dead_code)]` を置く (差し替えまで `real.rs` が使わないため)。T-6 で外す。
- `docs/` は今の状態だけを書く (D-15)。
- push と PR は利用者が許したときだけ (4. 前提)。

## 7. 実装のタスク

| ID | タスク | 基づく決定 | 前提 | 並べて進められるか |
|---|---|---|---|---|
| T-0 | 計画のコミット | - | - | - |
| T-1 | aarch64 と ASan の CI、flake | D-5、D-13 | T-0 | T-2 と |
| T-2 | `trust.rs` の最初の3つのテスト | D-5、D-13 | T-0 | T-1 と |
| T-3 | 核、偽の backend、Miri | D-1〜D-6、D-12、D-13 | T-2 | 不可 |
| T-4 | epoll と kqueue の backend | D-1、D-7、D-10 | T-3 | T-5 と |
| T-5 | IOCP の backend | D-1、D-7、D-11 | T-4 (`sys/mod.rs`) | 不可 |
| T-6 | 差し替え | D-1〜D-7、D-15、D-16 | T-4、T-5 | 不可 |
| T-7 | `bench/net` の覗きと目標の表 | D-14 | T-0 | T-1〜T-6 と |
| T-8 | 512 の測り直し | D-14 | T-6、T-7 | 不可 |
| T-9 | 512 を変える (条件付き) | D-14、D-16 | T-8 | 不可 |
| T-10 | 自前の driver の計測 | D-14、D-15 | T-8 (T-9) | 不可 |
| T-11 | 段を閉じる | D-15 | T-10 | 不可 |

### T-0: 計画のコミット

- 目的: 段の最初のコミットで設計と計画を入れる (ロードマップの「進め方」)。
- 対象: `docs/plans/2026-10-30-io-backends.md` と `docs/plans/2026-10-30-io-backends/`。
- 作業: そのままコミットする (`Plan io-backends`)。
- 実装者に任せること: なし。
- 完了の基準: `git status --short` → 空。`cargo nextest run -p eml_cli --test integration citations::` → 通る。

### T-1: aarch64 と ASan の CI、flake

- 目的: 新しい対象と道具の失敗を、差し替えの失敗と分けるため、compio の driver の上で先に緑にする。
- 対象: `.github/workflows/ci.yml`、`flake.nix`、testing.md (「CI」「OS ごとのテスト」、新しい節「`eml_io` の検証」の `### AddressSanitizer`、「よく使うコマンド」)、architecture.md 65 (nightly は Miri と ASan が使う)、`docs/README.md` の文書の地図、`CLAUDE.md` (Commands と Testing の ASan、`.#cross` の aarch64、CI の aarch64 と ASan、verus は x86_64-linux と aarch64-darwin だけ)。
- 作業: `ci-flake.diff` を当てる。PR の CI で落ちたテストを D-13 の仕分けで扱い、不具合は続くコミットで直し、不具合でないものは計画に1件ずつ足して利用者の承認を得る。
- 実装者に任せること: testing.md の文。
- 条件: いつも。push が許されないときは CI の基準を未確認として報告する。
- 完了の基準:
  - `nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- .github/workflows/ci.yml` → 出力なしで 0
  - `nix eval --raw .#devShells.aarch64-linux.default.drvPath` (`miri`、`cross` も) → `/nix/store/…drv`
  - `nix develop .#cross -c cargo clippy --target aarch64-unknown-linux-gnu --workspace --all-targets -- -D warnings` → 0
  - `nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=address CARGO_TARGET_DIR=target/asan cargo test -Zbuild-std --target x86_64-unknown-linux-gnu -p eml_io` → 68 passed
  - `nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=address CARGO_TARGET_DIR=target/asan cargo test -Zbuild-std --target x86_64-unknown-linux-gnu -p eml_vm --test integration -- api:: signal::` → 37 passed
  - `cargo nextest run -p eml_cli --test integration citations::` → 通る
  - PR の CI で `test` の4行と `asan` の3行 (外した脚を除く) が緑
- 落とし穴: cargo-nextest 0.9.146 に aarch64-linux の配布物がなければ版を上げる。arm の失敗を前もって期待値の変更として許さない。

### T-2: `trust.rs` の最初の3つのテスト

- 目的: `unsafe` の一覧を、新しいファイルが入る前に固定する。
- 対象: `crates/eml_io/src/lib.rs` (`#![deny(unsafe_code)]`、`#[cfg(test)] mod trust;`)、`src/trust.rs`、`mask.rs` と `test.rs` と `real.rs` の先頭の `#![allow(unsafe_code)]`、testing.md の `### unsafe の一覧`。
- 作業: A4 Q2 のテスト 1 (`unsafe_stays_in_the_listed_files`)、2 (`every_source_file_is_listed`)、7 (`the_scanner_skips_comments_and_strings`)。この時点の `UNSAFE_FILES` は `mask.rs`、`test.rs`、`real.rs`。消し方は `eml_rt/src/trust.rs` の `blank_comments_and_strings` を写す。
- 実装者に任せること: 補助関数の形。
- 完了の基準: `cargo nextest run -p eml_io trust::` → 3 passed。`cargo clippy --all-targets -- -D warnings` → 0。

### T-3: 核、偽の backend、Miri

- 目的: 核の規則を OS なしで確かめる。
- 対象: `src/driver/` (mod.rs、backend.rs、inbox.rs、pool.rs、resolve.rs、tests/mod.rs、tests/fake.rs、tests/abort.rs)、`ci.yml` の `miri` の4つ目の step、testing.md (「Miri」を `eml_io` の節を引く文に直し、同時に流せる実行を 6 から 8 に。`### 核の Miri`。「テストの置き場所」)、`CLAUDE.md` の Miri の行、`trust.rs` (`FILES`、`UNSAFE_FILES` に `driver/tests/fake.rs`、テスト 3、4、6 を `real.rs` を除く全ファイルに)、`crates/eml_rt/src/driver.rs` の trait のコメントに完了型の値が暫定であること。
- 作業: A1 Q1〜Q4 と A3 Q3〜Q6 と A4 Q1 のとおり。核のテストを先に書く。`#[cfg(test)] mod driver;`。3a (表と続き) と 3b (受け箱、プール、解決器) の2コミットに分けてよい。分けるときは、コードを足すコミットにその Miri のテストを入れる。
- 実装者に任せること: 偽の backend の台本を積む小さな関数。2つに分けるか。
- 完了の基準:
  - `cargo nextest run -p eml_io` → 全件通る
  - `cargo nextest run -j 1 -p eml_io` → `SLOW` なし
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_io --lib -- driver::tests::` と `-Zmiri-tree-borrows` を足したもの → `test result: ok`、`ignored` は abort の2件だけ、`the main thread terminated without waiting` が出ない。bash の `time` の `user` を測ってコミットメッセージに書き、testing.md の予算を置き換える
  - `cargo nextest run -p eml_io trust::` → 6 passed
  - `grep -rn "\.spawn(" crates/eml_io/src/driver --exclude-dir=tests` → `backend.spawn(` の2行だけ
- 落とし穴: `wait(None)` で保留が空だと `Ok(false)` で `turn` を通らない。遅れた答えを確かめるテストは保留の欄を残し、`finished` を待ってから `wait(Some(0))` を呼ぶ。Miri は `sigemptyset` と `current_exe` を持たないので、それを使うテストは `cfg_attr(miri, ignore)`。

### T-4: epoll と kqueue の backend

- 目的: unix の backend を差し替えの前に入れ、単体で確かめる。
- 対象: `src/sys/mod.rs` (`#![allow(dead_code)]`、unix と epoll と kqueue の `mod` の行、unix の `type Os` の2行、`millis(d: Duration, max: u32) -> u32` と全 OS の単体テスト)、`src/sys/unix.rs`、`src/sys/epoll.rs`、`src/sys/kqueue.rs`、`lib.rs` の `#[cfg(test)] mod sys;`、`trust.rs` (`FILES`、`UNSAFE_FILES`、テスト 5)、testing.md の「テストの置き場所」。
- 作業: A2 §1 のとおり。単体テスト9件を先に書く。
- 実装者に任せること: `syscall` の補助関数の正確な形、`Rows` のメソッド名、`EVENTS` (256) を変えるか。
- 完了の基準:
  - `cargo nextest run -p eml_io --lib sys::` → Linux で S2 の8件 (Apple だけの1件を除く) と `millis` が通る
  - `cargo clippy -p eml_io --all-targets -- -D warnings` → 0
  - `cargo nextest run -p eml_io trust::` → 7 passed
  - `grep -c "epoll_ctl(" crates/eml_io/src/sys/epoll.rs` → `2`。`grep -c "register_row(" crates/eml_io/src/sys/unix.rs` → `4`
  - `awk '/^mod tests/{exit} /loop \{|while /{print FILENAME": "FNR": "$0}' crates/eml_io/src/sys/unix.rs` → `EINTR` を繰り返す `syscall` の関数の行だけ
  - `nix develop .#cross -c cargo clippy --target aarch64-unknown-linux-gnu -p eml_io --all-targets -- -D warnings` → 0
  - CI の `macos-15` の `test` と `asan` (macOS) が緑
- 落とし穴: kqueue の事実はどれも macOS で確かめていない。`a_drained_event_does_not_make_a_probe_fresh` が落ちたら kqueue の `fresh` を `false` にする。拒否された接続で `SO_ERROR` が 0 のままなら A2 の U-3 の代わりの道を使う。

### T-5: IOCP の backend

- 目的: Windows の backend (ソケットなし) を入れる。
- 対象: `src/sys/iocp.rs`、`sys/mod.rs` の Windows の `mod` と `type Os` の行、`crates/eml_io/Cargo.toml` の windows の `windows-sys` (D-7 の feature)、`trust.rs` の `FILES`。
- 作業: A3 Q1 のとおり。
- 実装者に任せること: `ENTRIES` の値。
- 完了の基準:
  - `nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc -p eml_io --all-targets -- -D warnings` → 0
  - `grep -c "unsafe {" crates/eml_io/src/sys/iocp.rs` → 4
  - `grep -n "Arrival::\|Start::\|Hold::\|Cancel::Pending\|unreachable!" crates/eml_io/src/sys/iocp.rs` → 空
  - CI の `windows-2025` の `test` が緑
- 落とし穴: `GetQueuedCompletionStatusEx` の時間切れが `WAIT_TIMEOUT` で報告されることは推定。Windows の時間のテストが落ちたら A3 Q1 の代わりの道を使う。

### T-6: 差し替え

- 目的: `RealDriver` を自前の driver にし、compio と socket2 を外す。
- 対象: `src/real.rs` (`Driver<sys::Os>`、`new()` と `with_lookup` を全 OS に)、`sys/mod.rs` の `lookup`、`lib.rs` の `cfg(test)` と `allow(dead_code)` を外す、`resolve.rs` を消す、`crates/eml_io/Cargo.toml`、workspace の `Cargo.toml` と `Cargo.lock`、`tests/real.rs` (D-16 の成否の変更、機械的な追随、33 本目の確かめ、A2 の結合テスト6件、A3 の2件)、`trust.rs` (`real.rs` を `UNSAFE_FILES` から外し、テスト 3 と 4 に入れる)。文書: `docs/spec/vm.md` の「時計と driver」(D-6、D-15) と「上限と割り込み」379、`crates/eml_rt/src/driver.rs` のコメント、architecture.md 47、77、「`eml_io` の内部」(全文)、「外部 crate」(compio-driver、compio-buf、socket2 の行と 425〜431 の更新の手順を消し、`windows-sys` と `libc` の行を直す)、status.md 146、147、149、`docs/future/async.md` (28、「`IoDriver` と `eml_io`」243、244、265、325、447〜448、479〜481)、`CLAUDE.md` の Architecture の `eml_io` の行。
- 作業: 結合テストを先に足す。そのあと差し替える。
- 実装者に任せること: 文書の文。
- 完了の基準:
  - `cargo nextest run` → 全件通る
  - `cargo nextest run -j 1 -p eml_io` → `SLOW` なし
  - `cargo nextest run -p eml_io trust::` → 7 passed
  - `grep -n "compio\|socket2" Cargo.toml crates/*/Cargo.toml Cargo.lock crates/eml_io/tests/real.rs` → 空
  - `grep -rn "mem::forget\|Box::leak" crates/eml_io/src --exclude=trust.rs` → 空
  - `grep -rn "from_raw_parts" crates/eml_io/src --exclude=trust.rs --exclude=test.rs` → 空
  - `grep -rn compio docs CLAUDE.md --exclude-dir=reports --exclude-dir=plans --exclude=benchmarks.md` → 空。`grep -n compio docs/implementation/benchmarks.md` → 「34cad91 と e942eeb」の節の中だけ
  - `cargo nextest run -p eml_cli --test integration citations::` → 通る
  - `cargo clippy --all-targets -- -D warnings`、feature の組の clippy、両方の cross clippy → 0
  - CI の全ジョブが緑
- 落とし穴: `Unsupported` の `Display` の `unsupported` は `eml_vm/tests/api.rs:998` が見ている。Windows の `!SOCKETS` の誤りはハンドルを引く前に返す (`sockets_off_rejects_before_looking_up_the_handle`)。

### T-7: `bench/net` の覗きと目標の表

- 目的: 目標を仕組みとして判定できるようにする。
- 対象: `bench/net/run.py` (`bench-peek.diff`)、`bench/net/report.py` (`SYS` に `recvfrom(peek)`、`recvfrom(peek)!EAGAIN`、`sendto!EAGAIN`、`epoll_pwait2`、`getsockopt`、`getpeername`。D-14 の目標の表)、benchmarks.md の「ネットワークの計測」の本文と「主指標」。
- 実装者に任せること: `report.py` の表の見た目。
- 完了の基準: `cargo build --release -p eml_cli && python3 -I bench/net/bench.py --no-time --only '^(base|echo-self-fresh)$' x=target/release/eml > $S/x.jsonl && python3 -I bench/net/report.py $S/x.jsonl` (`$S` は scratch) → syscall の表に `recvfrom(peek)` の列があり、目標の表が出る。

### T-8: 512 の測り直し

- 目的: 書きの重みの単位を自前の driver の上で決める。
- 対象: benchmarks.md の新しい節「書きの重みの単位」。
- 作業: D-14 と A4 Q4 の手順で、Linux はこの機械で、macOS は利用者に頼んで流す。判定を書く。
- 条件: macOS が使えなければ D-14 の退路。
- 完了の基準: benchmarks.md に OS ごとの表 (件、512、候補、比、判定) と判定 (a)(b)(c) の結果がある。退路を使ったときは計画に書いて承認を得ている。

### T-9: 512 を変える (条件付き)

- 条件: T-8 の判定が候補を選んだときだけ。そうでなければ飛ばす。
- 対象: `crates/eml_vm/src/cells.rs` の `WRITE_UNIT` とそのコメント、`docs/spec/stdlib.md` の「操作の予算」、status.md 156、D-16 の期待値の変更。
- 完了の基準: `cargo nextest run` → 全件通る。変わった期待値が D-16 の範囲の中だけ (`git diff --stat` で確かめる)。

### T-10: 自前の driver の計測

- 目的: 目標の判定と、閉じるときのコードの数を残す。
- 対象: benchmarks.md の新しい節「driver の syscall の目標」(「34cad91 と e942eeb」の目標の表をここへ移し、過去の列は持ってこない。その節の「driver を差し替えたら、同じ機械で列を足す」の文を消す)、status.md 147。
- 作業: D-14 の前と後の比べを、コードの最後のコミットの clean な作業ツリーで流す (`busy-self` を含め全件、約 1 時間)。機械の情報 (CPU、vCPU、MemTotal、kernel) を記録する。前と後の表はコミットメッセージに書く。
- 完了の基準: D-14 の目標の表の全行が合格するか、外れた行が D-14 の「外れたら」で処理してある。後が「差」で遅くなった件には原因が書いてある。

### T-11: 段を閉じる

- 目的: ロードマップの「進め方」の閉じる手順を1コミット (`Close io-backends`) で行う。
- 作業: 次の順で進める。
  1. `grep -rn "io-backends\|compio" docs/spec` が空であることを確かめる。
  2. `bench/scripts.sh`、`bench/run.sh`、`bench/rt.sh` で「今の数値」の表を入れ替える (macOS がなければ後のコミット `Measure io-backends`)。
  3. status.md を直す。
  4. architecture.md を確かめる。
  5. ロードマップから `io-backends` の項を消し、「`compio-driver` の 0.13 の系列」の項を消し、「プールの仕事の予算と、放さずに終わる方針」を `quiesce` の上限なしの待ちだけにし、`net-windows`、`io-uring`、`async-files`、`async-threads`、`ffi-blocking` の前提から `io-backends` を外す。後に回した項目 (macOS の 512、外した ASan の脚) を「その後の項目」に足す。
  6. overview.md と `docs/README.md` を確かめる。
  7. `CLAUDE.md` を確かめる。
  8. `docs/plans/` のこの計画と付録を消す (消す前に、決めがすべて `docs/` に移っていることを確かめる)。
  9. コミットメッセージに、テストの変更の種類と理由、前後の比べの目立つ差、機種、後に回した項目の決めた日を書く。
- 完了の基準: `grep -rn "io-backends" docs --exclude-dir=plans --exclude-dir=reports` → 空。`ls docs/plans` に段のファイルがない。`cargo nextest run -j 1` → `SLOW` は集めて流す5つのテストだけ。`cargo nextest run -p eml_cli --test integration citations::` → 通る。

## 8. 実行の順

```
T-0 ─┬─ T-1 ─────────────┐
     ├─ T-2 ─ T-3 ─ T-4 ─ T-5 ─ T-6 ─┬─ T-8 ─ (T-9) ─ T-10 ─ T-11
     └─ T-7 ─────────────────────────┘
```

- T-1 と T-2 と T-7 は並べて進められる。T-1 の仕分け (計画への追記と承認) が済むまで T-6 に進まない。
- T-4 は `sys/mod.rs` を作るので T-5 より先。

## 9. 検証

- 始める前 (T-0 の上で):
  - `cargo nextest run` → 全件通る
  - `cargo test -p eml_io 2>&1 | grep "test result"` → integration 68 passed
  - `cargo nextest run -j 1 -p eml_io` → `SLOW` なし
  - `cargo clippy --all-targets -- -D warnings` → 0
  - `nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings` → 0
  - T-1 の ASan の2つのコマンド → 68 passed、37 passed
- すべての後:
  - `cargo nextest run` → 全件通る。`cargo clippy --all-targets -- -D warnings && cargo fmt --check` → 0。CLAUDE.md の feature の組の clippy → 0
  - CLAUDE.md の Miri のコマンド (`eml_rt`、`eml_vm`) と T-3 の `eml_io` の Miri を、2つの `MIRIFLAGS` で流す → 通る
  - CI の `test` (4つ)、`asan`、`miri`、`verus`、`fmt` が緑
  - D-14 の目標の表の全行が合格
  - `grep -n "compio\|socket2" Cargo.toml crates/*/Cargo.toml Cargo.lock` → 空
- 利用者の手による確かめ:
  - macOS で T-8 の 512 の run (完了の条件の「512 を測り直す」)。
  - macOS で `bench/*.sh` (T-11 の「今の数値」)。
  - PR の CI の結果を見て、T-1 の仕分けで足した項目を承認する。

## 10. 危険と未決

| ID | 項目 | 影響 | 決め方と時期 | 決まらないときの既定 |
|---|---|---|---|---|
| Q-1 | macOS で 512 を測れるか | T-8、T-9 | 決定: 利用者の Mac で流す。T-8 と T-11 で止めて、手順を渡して頼む | 512 のまま。ロードマップの「その後の項目」に足す |
| Q-2 | ASan の aarch64-linux と macOS の脚が緑になるか (fenix の nightly の runtime は x86_64 だけ確かめた) | T-1、G-17 | T-1 の CI | D-13 の退路。外した脚は計画と testing.md とロードマップに書く |
| Q-3 | arm で落ちるテスト | T-1 | T-1 の CI の後に仕分ける | 不具合以外は1件ずつ計画に足し、承認を得てから T-6 に進む |
| Q-4 | `eml_io` の Miri の予算 | T-3 | T-3 で測る | 120 s / 160 s |
| Q-5 | kqueue の `fresh` の近道が成り立つか | T-4 | macOS の CI のテスト | 落ちたら kqueue は `fresh: false` |
| Q-6 | macOS で拒否された接続の誤りが `SO_ERROR` に出るか | T-4 | macOS の CI | A2 の U-3 の代わりの道 |
| Q-7 | `GetQueuedCompletionStatusEx` の時間切れの誤りの番号 | T-5 | Windows の CI | A3 Q1 の代わりの道 |
| Q-8 | `net-windows` での `Hold` の意味 | 後の段 | `net-windows` の計画 | ロードマップに未決として書く (D-15) |
| Q-9 | 空いたプールのスレッドが抜ける時間 (`POOL_IDLE` 10 s) | なし | ふだんのテストでは確かめない | 10 s |
| Q-10 | macOS で `EVFILT_WRITE` が ACK ごとに起こしうる | macOS の性能 | T-8 の macOS の run で見る | 何もしない |

## 付録: レビューで変わった主な点

蒸し返さないために要る分だけを書く。

- `Drop` が仕事を待たない利点として「Ctrl-C の後の終わりが速くなる」を挙げていたが、`vm::run` の直後に `quiesce` が仕事を待つので成り立たない。主張を取り下げ、「`Drop` は正しさのために待ちを要らない」に絞った (D-4)。
- `quiesce` の取り消しの待ちを上限なしにしかけたが、失われた完了で終わりが止まるので、1 s で壊れた driver にする形にした (D-4)。
- `Readable` を毎回覗く既定では待つ読みが 4 syscall になり、目標の 3 に合わないので、`fresh` の近道にした。記憶で答えた覗きも `fresh` を下ろさないと既存のテストが落ちる (5.1 の逸脱)。
- `epoll_ctl` の総数の内訳を「リスナー + 受けたソケット」としていたが、echo-py-reuse では eml は接続する側で、eventfd の登録も1回数える (5.3)。
- プールの起こす条件と、`running` を外す順を試作で直した (D-3)。
- 偽の backend の待ちが実時間を使うと、1 s の上限のテストが1件 1 s の規則を破るので、偽の時計にした。本物のスレッドが走る間に時計を進めると panic する (D-13)。
- `trust.rs` で `ManuallyDrop` や `to_socket_addrs` まで禁じる案は、G の字面を越えるので取り下げた (D-13)。
- aarch64 の失敗を前もって期待値の変更として許す案は取り下げ、見た後に1件ずつ決める (D-13)。
- 前と後の比べの表を benchmarks.md に残す案は、CLAUDE.md の「docs は今の状態」に反するので、コミットメッセージに置く (D-14、D-15)。
