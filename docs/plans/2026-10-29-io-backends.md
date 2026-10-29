# io-backends

- 作成日: 2026-10-29
- 基点: async-net の段を閉じた後の `main` (4 章の最初の確かめ)。この計画の事実は、`origin/claude/cool-goodall-w7phpm` @ `d1dd4f3` (async-net の T-4 の後) と、その段の計画の残りの作業 (T-5 から T-13) から読んだ。行番号はこのコミットのもので、async-net の残りの作業で動く。行番号は探す手がかりにとどめ、関数、テスト、節の名前で引く
- 状態: 計画は確定。決めていない細部は 10 章に既定とともに置く

この計画は、本物の I/O の driver を `compio-driver` から自前の backend に移す段 `io-backends` の設計と計画である。roadmap の「段の列」にはまだこの段がなく、T-0 が足す。roadmap の「進め方」に従い、この文書と付録のディレクトリ `docs/plans/2026-10-29-io-backends/` を段の最初のコミット (T-0) で入れ、段を閉じるときに消す。コードと `docs/` はこの文書を引かない。

付録は2つある。本体は決定、作業の順、完了の条件を持ち、作業の中身の細部 (型と署名、手順の表、テストの名前、仕様の文案) は付録にある。

- 付録1 `01-core-unix.md`: 核、epoll と kqueue の backend、doorbell、プール、解決器、`buf.rs` と unsafe の一覧、偽の backend と核のテスト、既存のテストの扱い、aarch64-linux の flake と CI、docs の文案、io_uring への当てはめ
- 付録2 `02-iocp.md`: この段の最小の IOCP の backend (§A、T-5)、後の段 `net-windows` の設計 (§B)、roadmap の `net-windows` の項に書き写す文案 (§C)。`net-windows` はこの段では実装しない。T-9 が §C の要点を roadmap の `net-windows` の項に書き写す。`net-windows` の段の計画は、その段の最初のコミットで別に作る

付録と本体が食い違うときは本体に従う。

## 1. 目的と成功の基準

- 目的
  - 本物の driver (`RealDriver`) から `compio-driver` と `compio-buf` を外し、OS ごとの backend を自前で書く。Linux は epoll、Apple の OS は kqueue を `libc` で、Windows は IOCP を `windows-sys` で直接呼ぶ。
  - driver を「`OpKey` を鍵にした1つの表を持つ核」と「操作を1回ずつ OS に出す backend」に分ける。核は OS を呼ばず、偽の backend の上で Miri にかける。
  - compio の上で要った回り道をなくす。落とすときに仕事を待ちきること、壊れた driver の鍵を漏らすこと、`completions` が保留をすべて歩くこと、取り消しの 5 秒の期限、仕事の受け付けが呼んだスレッドを止めること、の5つである。
  - aarch64-linux を対象に足す。主な使い方は Apple Silicon の Mac の上の Linux コンテナで、そこでは既定の seccomp が io_uring を塞ぎ、epoll はどこでも使える。
  - io_uring はこの移行に入れない。`Backend` の境界は完了の形でも成り立つように決め、io_uring への当てはめを付録1 §17 に書く。
- 成功の基準 (確かめ方は 9 章)
  - 9 章の全体の検証のコマンドがすべて通る。
  - `git grep -n -i compio -- . ':!docs/plans'` が何も返さない。
  - GitHub の CI の `test` が ubuntu-24.04、ubuntu-24.04-arm、macos-15、windows-2025 の4つで緑になり、`miri` ジョブが `eml_io` の核を2つの設定で流す。
  - `grep -rn "forget\|Box::leak" crates/eml_io/src` と `grep -rn "unsafe impl" crates/eml_io` が何も返さない。

## 2. 範囲

- 含むもの
  - 核 (`src/real/`)、epoll と kqueue の backend (`src/sys/unix/`)、doorbell、プール、`Shared` の受け箱を使う解決器 (D-2 から D-6、D-10 から D-15)
  - 最小の IOCP の backend。完了ポート、waker、仕事、時計を持ち、ソケットの操作は今と同じく `Unsupported` を返す (D-21)
  - `buf.rs`、`#![deny(unsafe_code)]` と unsafe の一覧、数えるテスト (D-7、D-16、D-17)
  - 偽の backend と核の単体テスト、`eml_io` の Miri、ASan のジョブ (D-8、D-18、D-20)
  - aarch64-linux の flake と CI (D-8、D-20)
  - `RealDriver` の差し替えと compio の削除 (D-9、D-19)
  - docs の書き換え、`net-windows` の要点を roadmap に書くこと、段を閉じる作業
- 含まないもの
  - Windows のソケット (`WSARecv`、`WSASend`、`AcceptEx`、`ConnectEx`)。後の段 `net-windows` で入れる (D-22、付録2 §B)
  - io_uring の backend (D-1)。roadmap の「その後の項目」に見直す条件と形を書く
  - 別のスレッドから受け付けを出す API (D-6)。`async-threads` で作る
  - 書きの続きの syscall を減らす最適化、`Readable` で先に試す読み (10 章)
  - 非同期のファイル (段 `async-files`)。macOS の通常のファイルもプールの仕事にすることは、docs/future/async.md の「ファイルとパイプ」にすでに書いてある

## 3. 背景

基点の状態は、async-net の計画どおりに閉じた後の main として読む。`d1dd4f3` で確かめた事実と、async-net の計画が残りの作業で変えると書いた事実を分けて書く。

- `IoDriver` (`crates/eml_rt/src/driver.rs`) のメソッドは `now`、`wait`、`submit`、`cancel`、`completions`、`pending`、`waker`、`bin`、`shutdown_write`、`set_nodelay`、`close`、`quiesce` である。`Op` は `Readable`、`Read`、`Write`、`Listen`、`Accept`、`Connect`、`Job`、`Done` は `Size`、`Listener`、`Stream`、`Completion` は `cancelled` を持つ。`pair` は async-net の T-9 が消す。`RawBuf` の doc は「貸した側はその操作の完了を `completions` から取り出すまで、物体を生かし、一意に保ち、動かさない」と書く。この移行では trait の形を変えず、doc だけを直す。
- 本物の driver は `crates/eml_io/src/real.rs` (約 880 行) で、compio-driver 0.12.5 の `Proactor` の上にある (features `polling` と `sync`)。unix は polling の crate を通して epoll と kqueue、Windows は IOCP を使う。`Proactor` は `Send` でない。
  - 保留の操作は `HashMap<OpKey, Entry>` に compio の `Key` として持つ。`completions` は保留をすべて `pop` して終わったかを確かめ、手間は O(保留の数) である。
  - `quiesce` は、取り消した I/O の操作が 5 秒 (`CANCEL_LIMIT`) たっても終わらなければ壊れた状態にする。compio の poll の backend は、関心を外す `epoll_ctl` が失敗すると完了を出さずに操作を落とす (compio-driver の `poll/mod.rs:250-266`) ためである。
  - `Drop` は壊れていなければ `quiesce` で仕事を待ちきり、壊れていれば残った鍵を `mem::forget` で漏らす。compio は仕事の鍵の写し (数え方が atomic でない) をプールのスレッドへ送るので、持ち主が先に落とせないためである。
  - `poll` は `TimedOut` と `Interrupted` を戻っただけと読む。polling の backend は起こされたときも `ETIMEDOUT` を返す。
  - 仕事は compio の `Asyncify` のプール (32 本) で動く。プールが満ちると、仕事の `submit` は呼んだスレッドの上で安全点を通らずに待つ (compio の `yield_now` の空回り)。プールに出す間、`mask.rs` の `Masked` が SIGINT と SIGTERM を塞ぎ、プールのスレッドはその塞ぎを受け継ぐ。
  - 名前の解決は自前のスレッド (`resolve.rs`。`cfg(unix)`、同時に8本、手放したもの64本で打ち切り) で行い、結果を mpsc で送ってから compio の waker を鳴らす。`RealDriver::with_lookup` が解決の関数を差し替える。
  - Windows ではソケットの操作 (`Listen`、`Accept`、`Connect`、`shutdown_write`、`set_nodelay`) が `Unsupported` を返す。`libc` と `socket2` は unix だけの依存である。
- `eml_io` の `unsafe` は、`real.rs` の `Mem` の3か所 (`RawBuf` を slice にする)、`mask.rs` の2か所 (`pthread_sigmask`)、`test.rs` の4か所、`tests/real.rs` の1か所である。`deny(unsafe_code)` も `forbid` もない。`unsafe impl Send` は書いていない。
- `crates/eml_io/tests/real.rs` のテストは、async-net の後で、時計の4本、仕事の3本 (`dropping_a_driver_waits_for_a_running_job` を含む)、ループバックの上のストリームの6本、TCP と解決器の16本、`pool_threads_block_interrupt_and_terminate`、`an_op_on_an_unknown_handle_fails`、Windows だけの `listen_accept_and_connect_are_unsupported` などである。多くは `#[cfg(unix)]` である。`src/real.rs` に単体テスト `a_wake_from_another_thread_ends_a_long_poll` が1本ある。一覧は付録1 §14 にある。
- async-net の T-5 の後、`eml_vm` の監視のスレッドはシグナルの知らせで driver の waker を鳴らす (async-net の計画の D-15)。driver の waker を本番で鳴らすのは、解決のスレッドとこの監視のスレッドである。`IDLE_TICK` (10 ms) の上限は残る。
- flake の `systems` は `x86_64-linux` と `aarch64-darwin` の2つで、Verus と verusfmt の input もこの2つだけである。Verus は aarch64-linux の release を出さない。CI の `test` の matrix は `ubuntu-24.04`、`macos-15`、`windows-2025` で、`fmt`、`verus`、`miri` は `ubuntu-24.04` の nix で流す。Miri で流すのは `eml_rt` の `trusted::` と `eml_vm` の一部だけである。ワークスペースに `target_arch` の cfg はない。`libc::epoll_event` は x86_64 でだけ packed である。`ubuntu-24.04-arm` の runner は公開のリポジトリで無料で、ci.yml が固定した `taiki-e/install-action` の manifest には `cargo-nextest` 0.9.146 の `aarch64_linux_gnu` がある。
- Apple Silicon の Mac の上の Linux コンテナ: Docker (25.0 から)、containerd (2.0 から)、Podman の既定の seccomp は `io_uring_*` を許さず、`epoll_*` と `eventfd2` を許す。QEMU の user mode は io_uring を ENOSYS にし、gVisor は既定で io_uring を無効にする。Rosetta for Linux の上の epoll は一次の出典がなく、確かめていない。
- Miri には `pthread_sigmask`、`sigemptyset`、`sigaddset` の shim がない (rust-lang/miri の `src/shims/unix/foreign_items.rs`)。Miri は syscall を動かせない。
- IOCP の `CancelIoEx` は、成功を返した後も、取り消した操作が終わるまで `OVERLAPPED` を放したり使い回したりしてはならない。取り消した操作は、ふつうに終わるか、`ERROR_OPERATION_ABORTED` で終わるか、ほかの誤りで終わる。取り消すものがなければ `ERROR_NOT_FOUND` である (learn.microsoft.com の CancelIoEx)。
- Rust の std は、実行ファイルの `main` の前に SIGPIPE を無視にする (rust-src 1.99.1 の `library/std/src/sys/pal/unix/mod.rs` の `reset_sigpipe`)。ライブラリとして埋め込んだ eml にはこの初期化がない。
- `eml_vm` の `Machine` は、`vm::run` の間 driver を `Scheduler` に移す。`vm::run` から panic が巻き戻ると、driver は `quiesce` を通らずに `Scheduler` の中で落ち、`Machine::drop` は poisoned でないのでヒープを放す (`crates/eml_vm/src/lib.rs` の `Machine::run` と `Machine::drop`。`eml_vm` と `eml_cli` に `catch_unwind` はない)。poisoned の `Machine` は driver を落とした後にヒープを漏らす (vm.md の「時計と driver」)。
- `windows-sys` 0.61.2 と `socket2` 0.6.5 はすでに `Cargo.lock` にある。
- 文書 (main にあり、async-net の閉じる作業の後も残る見込みのもの)
  - docs/future/async.md の「未決の論点と将来の拡張」の「I/O の driver の crate」の箇条と、その下の2つの箇条 (自前の driver に替えるもう1つの条件、替えるときの順)。利用者の決定で、この計画どおりに書き換える (T-6)。
  - docs/future/async.md の「ファイルとパイプ」の macOS の通常のファイルをプールの仕事にする箇条 (中に compio の文が1つある)。
  - docs/future/roadmap.md の「その後の項目」の「io_uring の backend」(「見直すときの形」と「io_uring で失敗するテストと直し方」を含む)、「waker の本番の呼び出しと 10 ms の上限をなくすこと」(async-net が「10 ms の上限をなくすこと」に狭める)、「プールの仕事の予算と、放さずに終わる方針」、「完了を集める手間」、「`compio-driver` の 0.13 の系列」。
  - `git grep -n -i compio` が当たる行と、それを消すタスクの割り当ては付録1 §16.1 にある。

## 4. 前提の確認

| 要るもの | 用途 | 確かめ方 | ないとき |
|---|---|---|---|
| async-net の段が閉じて main に入っていること | すべての作業 | `git log origin/main --oneline \| grep 'Close async-net'` → 1行 | 止めて利用者に聞く。着手しない |
| async-net の後の driver の形 | T-3 から T-6 | `git grep -n 'fn pair' crates/eml_rt/src/driver.rs` → 何も出ない。`Op` の腕が3章のとおり | 10 章の「async-net の読み替え」に従う |
| main だけにある文書の箇条 | T-0、T-6 | `git grep -n '完了を集める手間' origin/main -- docs/future/roadmap.md` → 1行。`git grep -n '自前の driver に替えるもう1つの条件' origin/main -- docs/future/async.md` → 1行 | 無い箇条の書き換えを飛ばし、コミットのメッセージに書く (10 章の Q-12) |
| nix の devShell | すべての作業 | `nix develop -c cargo --version` → `cargo 1.99.1` | 止めて利用者に聞く |
| Verus | T-6 (`eml_rt` の doc を変える) | `nix develop -c verus --version` → 版が出る | 止めて利用者に聞く |
| Miri の shell | T-3 から T-7、T-10 | `nix develop .#miri -c cargo miri --version` → 版が出る | 止めて利用者に聞く |
| `.#cross` の shell | すべての作業 | `nix develop .#cross -c cargo clippy --version` → 版が出る | 止めて利用者に聞く |
| `/usr/bin/time` | T-3 の Miri の予算の測り | `ls /usr/bin/time` → ある | `nix shell nixpkgs#time -c time -p …` で測る |
| GitHub の CI (macOS、Windows、arm の Linux) | kqueue、IOCP、aarch64 の確かめ | 段の PR で CI が4つの `test` を流す | arm がなければ 10 章の Q-2。macOS と Windows がなければ段を閉じる前に利用者に聞く |
| macOS の機械 | T-10 の計測 (`bench/*.sh`) | `uname` → `Darwin` | 表の入れ替えを直後の別のコミット `Measure io-backends` にする (roadmap の「進め方」) |
| Docker Desktop か OrbStack のある Mac | 9 章の手の確かめ | 利用者に聞く | T-10 のコミットのメッセージに「流せる機械がなかった」と書く |

## 5. 決定

### 5.1 長く効く方針 (変えるときは見直しが要る)

実装がこの節の方針とぶつかったら、止めて報告する。各決定の「守る規則」は、設計の検討で決めた規則 (G-1 から G-16) の最終の文である。

#### D-1: backend は OS ごとに自前で書き、io_uring は入れない
- 決定
  - 本物の driver の backend は、Linux は epoll、Apple の OS は kqueue、Windows は IOCP にする。epoll と kqueue は `libc` で、IOCP は `windows-sys` で直接呼ぶ。ソケットの作成、bind、listen、accept の旗、shutdown、`TCP_NODELAY`、`SO_NOSIGPIPE` は3つの OS とも `socket2` で行う。
  - `polling`、`compio-driver`、`compio-buf`、`rustix`、`mio`、`io-uring` は使わない。
  - io_uring はこの移行に入れない。io_uring の段も置かない。`Backend` の境界を完了の形でも成り立つように決め、各メソッドを io_uring の操作に写す表を付録1 §17 に置く。
- 理由
  - `polling` を残すと、起こされても `ETIMEDOUT` になる道、kqueue の `notify` の確保、`add` の「落とす前に `delete` する」契約、epoll の1回の待ちに足される timerfd の syscall が残る。これらは compio の上で `IDLE_TICK` と壊れた状態を要した原因の一部で、自前にすれば設計で消せる。`polling` が依存する `rustix` と `concurrent-queue` も消える。
  - io_uring を入れない理由は3つある。`Std.Net` の計測がまだなく、readiness の道が支配的だという根拠がない。新しい対象の Mac の上のコンテナでは既定の seccomp が io_uring を塞ぐので、io_uring を足しても epoll の道はどのみち要る。io_uring は SQE に生のポインタを渡し、取り消しが非同期なので、unsafe と寿命の規則が増える。この移行はすでに3つの OS の backend、4つの対象の CI、unsafe の方針を同時に動かす。
  - 完了の形の経路 (`Start::Pending` の後に `wait` から `Finished` が届く) は、この段では偽の backend の完了の台本で、`net-windows` では IOCP のソケットで、コードとして確かめる。
- 退けた案
  - io_uring を含める: 計測の根拠がなく、unsafe の範囲と CI の組を同時に増やし、新しい対象では走らない。
  - epoll と kqueue を `polling` に任せる: 上の振る舞いを引き継ぐ。自前に書く量は OS ごとに 200〜300 行と見積もった (試作の epoll と kqueue と共有の部分と doorbell で約 550 行)。
  - `rustix` の安全な epoll と kqueue の API: unsafe の行は減るが依存が1つ増え、`libc` は `mask.rs` ですでに要る。
  - compio の fusion の driver で io_uring を試す: compio を外す決定と両立しない。
- 影響の範囲: `crates/eml_io/Cargo.toml`、`Cargo.lock`、`src/sys/`。
- 見直す条件: `Std.Net` の計測で readiness の道が支配的になったとき (io_uring)。
- 守る規則
  - G-1 MUST: 本物の driver の backend は、Linux は epoll、Apple は kqueue、Windows は IOCP で、それぞれ `libc`、`libc`、`windows-sys` を直接呼んで書く。`compio-driver`、`compio-buf`、`polling`、`rustix`、`mio`、`io-uring` を `eml_io` の依存に入れてはならない。対象は `crates/eml_io/Cargo.toml` と `Cargo.lock` で、`socket2`、`libc`、`windows-sys` は許す。ほかの crate の依存は対象外。
  - G-16 MUST: 設計に「io_uring への当てはめ」の節を置き、`Backend` の各メソッドと `Slot` と `KERNEL_WRITES_AFTER_CANCEL` を io_uring の操作に写す表を書く。行は少なくとも、`start` の `Readable` (`POLL_ADD`)、`Read` (`RECV`)、`Write` (`SEND`)、`Accept` (`ACCEPT`)、`Connect` (`CONNECT`)、`cancel` (`ASYNC_CANCEL` と、取り消しの CQE と本体の CQE の2つ)、`wait` (`EXT_ARG` の時間付きの `enter` と、eventfd の `POLL_ADD` による起こしの区別)、`close`、`Slot` (`sockaddr` と `msghdr` の所有)、`KERNEL_WRITES_AFTER_CANCEL = true` の `Drop` (ring の fd を閉じる前に CQE を取り出す。`drain_broken` は CQ を読み、1 秒までに終わらない操作が残れば abort してから ring を閉じる) の10行である。核の変更なしに書けない行があれば、核の形をその設計で直す。
- 適合の確かめ
  - `grep -n "compio\|polling\|rustix\|mio\|io-uring" crates/eml_io/Cargo.toml` → 0行
  - `grep -n 'name = "compio-\|name = "polling"' Cargo.lock` → 0行
  - 付録1 §17 に10行の表があり、どの行も「核を変えない」
- リポジトリに残す所: docs/implementation/architecture.md の「`eml_io` の内部」と「外部 crate」(T-6、T-7)、docs/future/async.md の「`IoDriver` と `eml_io`」(T-6)、roadmap の「その後の項目」の「io_uring の backend」(後に回した理由、見直す条件、見直すときの形。T-0)。当てはめの表そのものは残さない。

#### D-2: 核と backend の分担、backend の選び方
- 決定
  - 核 (`src/real/`) は、`OpKey` を鍵にした1つの表、終わった完了の列、ハンドルの表、書きの出し直し・受けの再試行・接続の次の試行の方針、プール、解決器、`quiesce` を持つ。核は OS を呼ばない。
  - backend (`src/sys/`) は、1つの操作を1回 OS に出し、終わりを報告するだけで、方針を持たない。ソケットの作成と設定も backend に置く (D-10)。
  - backend はコンパイル時に OS で1つに決める。`cfg(target_os = "linux")` は epoll、`cfg(target_vendor = "apple")` は kqueue、`cfg(windows)` は IOCP、どれでもなければ `compile_error!`。実行時の選択は作らない。
- 理由: 方針を OS ごとに3回書かないため。核に OS を呼ばせなければ、偽の backend の上で核の全部 (`Listen`、`Accept`、`Connect` まで) を Miri で流せる。compio も `OpCode` の `pre_submit` と `operate` で同じ分け方をしている。
- 退けた案: 核が `socket2::Socket` を直に持つ形 (記録を作るのに本物のソケットが要り、読み、書き、受け、接続の方針を Miri で流せない)。実行時に backend を選ぶ形 (使えない backend が黙って別の backend に落ちる)。
- 影響の範囲: `src/real/` と `src/sys/`。テストの driver (`src/test.rs`) は対象外。
- 見直す条件: Linux に2つ目の backend (io_uring) を足すとき。
- 守る規則
  - G-2 MUST: 核 (`src/real/`) と backend (`src/sys/`) の分担は下の表に従う。backend は同じ鍵の操作を自分から出し直してはならず、核は OS の呼び出しを `socket.rs` の安全な補助を通してしか行わない。
    - 読み: この設計の核は OS をまったく呼ばない。ソケットの作成と `Listen` は backend の `listen` と `socket` が `sys/socket.rs` の補助を呼んで行う (D-10)。設計の検討で、この形は規則の範囲と認めた。

    | 事柄 | 置く所 |
    |---|---|
    | `EINTR` でその syscall をやり直す | backend |
    | `EAGAIN` / `EWOULDBLOCK` で関心を登録し、読めた後にもう一度 `EAGAIN` なら列の先頭に戻す | backend |
    | `connect` の `EINPROGRESS` の後、書けるようになってから `SO_ERROR` を読んで結果にする | backend |
    | `accept4` の `SOCK_NONBLOCK \| SOCK_CLOEXEC`、Apple では受けた後の `set_nonblocking` と `set_cloexec` と `SO_NOSIGPIPE` | backend |
    | epoll の `HUP` / `ERR` / `RDHUP`、kqueue の `EV_EOF` は、読める (書ける) として扱い、次の syscall に誤りを言わせる | backend |
    | 1つの fd に方向ごとの列を持ち、登録する関心は先頭の読みと先頭の書きの和 | backend |
    | kqueue の `EV_RECEIPT` の誤りの返し方、`ENOENT` の無視 | backend (この設計は `EV_RECEIPT` を使わず、変更の誤りを `-1` と errno で受ける) |
    | `ECONNABORTED` で `Accept` を出し直す | 核 |
    | 短く終わった `Write` を残りの位置から出し直す。0 バイトの進みは `WriteZero` | 核 |
    | `Connect` の次の番地を試す (新しいソケットを作って `start`)。最後の番地の誤りを返す | 核 |
    | `cancelled` の印がある記録の続きを出さず、`cancelled` の誤りで終える | 核 |
    | `Completion::cancelled` を立てるかの判定 (取り消しを頼み、結果が backend の取り消しの誤りのとき) | 核 (`Finished.cancelled` を見る) |
    | `Listen` (socket2 で同期) | backend の `listen` (`sys/socket.rs` の補助を呼ぶ。上の読み) |
  - G-9 MUST: backend はコンパイル時に OS で1つに決める。将来 Linux に2つ目の backend を足すときは、`RealDriver::new` が環境変数 `EML_IO_BACKEND` で固定でき、CI がその値ごとに `eml_io` のテストを流し、選ばれた backend の名前を返す関数の値が環境変数と同じことをテストが確かめる。この移行では環境変数も選ぶ関数も作らない。
- 適合の確かめ
  - `grep -rn "SO_ERROR\|EINPROGRESS\|EINTR\|Interrupted" crates/eml_io/src/real` → 0行
  - `grep -rn "ECONNABORTED\|WriteZero" crates/eml_io/src/sys` → 0行 (IOCP は 10053 を数で書く)
  - `grep -rn "EML_IO_BACKEND" crates` → 0行
  - `src/sys/mod.rs` の cfg の表が付録1 §8 のとおり
- リポジトリに残す所: architecture.md の「`eml_io` の内部」に、核と backend の分担と cfg の表を書く (T-6)。`EML_IO_BACKEND` の規則は roadmap の「io_uring の backend」の「見直すときの形」に書く (T-0)。

#### D-3: 記録の寿命と、落とすときの後始末
- 決定
  - 保留の操作は1つの表にちょうど1つの記録を持つ。記録は持ち主のスレッドの表にだけあり、プールのスレッドには `Box<dyn FnOnce>` と `Arc<Shared>` だけを渡す。
  - `Drop` は仕事と名前の解決を待たない。待つのは、カーネルがまだ記憶に書きうる I/O の操作だけである (`KERNEL_WRITES_AFTER_CANCEL = true` の backend)。readiness の backend では何も待たない。
  - 壊れた driver を落とすときは、`Backend::drain_broken` が取り消しを出し、合わせて 1 秒まで終わりを確かめる。終わらない操作が残れば、残った鍵を標準エラーに書いて process を abort する。何も漏らさない。
- 理由
  - 記録が1か所にあれば、仕事を待つ理由も鍵を漏らす理由もない。IOCP では `OVERLAPPED` を完了の前に放せない。
  - 貸した記憶は VM のヒープの一部で、driver は生かし続けることも漏らすこともできない。箱を漏らしても、`quiesce` を通らずに落ちる道 (run からの panic の巻き戻し) では `Machine` がヒープを放すので、使用後解放になる。abort はそれを防ぐ唯一の形で、壊れた状態は欠陥でしか起きない。
  - 待ちに 1 秒の上限を置くのは、壊れた状態では OS の取り消しも失敗しうるので、上限がないと埋め込んだ eml の `Drop` が止まり続けるためである。
- 退けた案 (付録の外の記録も見よ)
  - 壊れた driver の終わらない箱を `Box::leak` で残す: 上の巻き戻しの道で使用後解放になる。
  - `Machine::drop` が driver を落とす前に `pending()` を見る: 巻き戻しの道では driver が `Scheduler` の中で落ちるので見られない。
  - `run_inner` を `catch_unwind` で包む: `quiesce` 自身の debug の panic の道が残り、`eml_vm` の変更も要る。
  - 上限なしに待つ: `Drop` が止まり続けうる。
- 影響の範囲: `src/real/` と `src/sys/`。テストの driver は対象外。`eml_vm` は変えない。
- 見直す条件: `CancelIoEx` から 1 秒以内に終わらない操作が実際に観測されたとき。
- 守る規則
  - G-3 MUST: 保留の操作は `OpKey` を鍵にした1つの表にちょうど1つの記録を持ち、記録を放すのは完了を `done` に積むときか、`quiesce` と `Drop` が完了を取り出して捨てるときだけである。`KERNEL_WRITES_AFTER_CANCEL` が `true` の backend では、`Drop` は取り消しを出して I/O の操作の完了をすべて取り出してから記録を放す。driver が壊れていて完了を取り出せないときは、`Backend::drain_broken` が取り消しを出し、合わせて 1 秒までカーネルの終わりを確かめ、終わらない操作が残れば、残った鍵を標準エラーに書いて process を abort する。カーネルが書きうる記憶を放す道も、放されうる状態に置く道 (漏らす) も作らない。`mem::forget` と `Box::leak` は `eml_io` のどこにも書かない。対象は `src/real/` と `src/sys/` で、テストの driver は対象外。
- 適合の確かめ
  - `grep -rn "forget\|Box::leak" crates/eml_io/src` → 0行
  - `grep -rn "process::abort" crates/eml_io/src` → この段では0行 (`net-windows` の後は `src/sys/windows/` の行だけ。io_uring を足せば `src/sys/unix/` も)
  - 偽の backend の `Fake<true>` に `drain_broken` の台本 (終わる場合) のテスト `a_broken_completion_driver_drains_its_io_on_drop` がある。`KERNEL_WRITES_AFTER_CANCEL` の両方の値のテスト (`dropping_a_completion_driver_waits_for_cancelled_io`、`dropping_a_readiness_driver_does_not_cancel`) がある
  - `net-windows` では、`wsa.rs` に取り消しの後に `WSAGetOverlappedResult` が `WSA_IO_INCOMPLETE` を返さなくなってから箱を放すテストがある (付録2 §B.8)
- リポジトリに残す所: vm.md の「時計と driver」の `quiesce` と壊れた driver の箇条、`Machine` を落とす順の理由 (T-6、付録1 §16.4)。architecture.md の「`eml_io` の内部」の `Drop` の表 (T-6)。

#### D-4: 完了の集め方と取り消し
- 決定
  - `completions` と `wait` は保留の表を歩かない。終わった操作は `done` に積み、`completions` は `done` を移して `OpKey` で並べるだけにする。1回の呼び出しの中では今と同じく `OpKey` の順に並べる (終わった数を k として O(k log k))。
  - 取り消し: readiness の backend は列から外してその場で取り消しの完了を作る。完了の backend は OS に取り消しを頼み、完了はカーネルから届く。取り消しの 5 秒の期限と、それで壊れた状態にする道はなくす。
  - `quiesce` は時間の上限なしに完了を待つ。debug のビルドでは、5 秒たっても I/O の操作が残れば、残った鍵を添えて panic する。release で止まったときは、2回目の Ctrl-C の `_exit(130)` が今までどおり効く。
- 理由: 保留が N 個でも集める地点の手間を終わった数に比例させる。完了を落とす道は compio の poll の backend にだけあった。上限で壊れた状態に落とすと backend の欠陥が隠れる。
- 退けた案: `quiesce` に時間の上限を残して壊れた状態にする (欠陥を隠す)。
- 影響の範囲: `IoDriver::completions`、`IoDriver::wait`、`Backend::wait`、すべての backend と `quiesce`。プールの仕事 (取り消せない) は G-5 の対象外。
- 見直す条件: debug の 5 秒の見張りが CI の遅い runner で誤って鳴るとき (10 章の Q-7)。
- 守る規則
  - G-4 MUST: `completions` と `wait` は保留の表を歩いてはならない。終わった操作は `done` に積み、`completions` は `done` を移して `OpKey` で並べるだけにする。backend の `wait` は fd から鍵を引く自分の表で鍵を得て、`Table<B>` を鍵で引く。`quiesce`、`Drop` (`drain_broken` を含む)、`pending` は対象外。
  - G-5 MUST: `Start::Pending` になった操作はちょうど1回 `Finished` になる。readiness の backend は取り消しの完了をその場で表から作り、完了の backend は OS の取り消しの完了を待つ。`quiesce` に時間の上限を置いて壊れた状態にしてはならない。debug のビルドでは、`quiesce` が 5 秒たっても I/O の操作を残せば、残った鍵を文言に入れて panic する。`Drop` は panic せず、debug では 5 秒で1回 `eprintln!` し、待ち続ける。`broken` にするのは、`wait` の OS の呼び出しそのものが失敗したときと、取り消しの要求 (`CancelIoEx`) が「取り消すものがない」以外の誤りで失敗したときだけである。後者は backend が誤りを覚え、次の `wait` が OS を待たずに `Err` で返す。
- 適合の確かめ
  - `tests/real.rs` の `completions_return_only_the_finished_ops` (保留 8 のうち 3 だけ終わる) と、核の同名のテストが通る
  - レビューで、`completions`、`wait`、`Backend::wait` の本体に `Table` の `iter`、`values`、`retain`、`keys` がないこと
  - `grep -rn "CANCEL_LIMIT" crates/eml_io/src` → 0行。`quiesce` の見張りが `cfg(debug_assertions)` の中にある
  - 偽の backend のテストに、取り消しの後にふつうの結果が届く競走 (`a_result_that_beats_the_cancel_is_not_cancelled`)、取り消した操作が1回だけ終わること (`a_pending_op_finishes_once_with_or_without_a_cancel`)、終わらない台本で debug の `quiesce` が panic すること (`quiesce_panics_when_a_cancelled_op_never_finishes`、`#[ignore]`) がある。`Drop` の見張りが panic でない
  - `net-windows` では、`wsa.rs` の `cancel` が `ERROR_NOT_FOUND` 以外の失敗を覚え、`wait` が `Err` を返す道があることをレビューで読む (偽の `CancelIoEx` を差し込めない)
- リポジトリに残す所: vm.md の「時計と driver」の `cancel`、`Completion::cancelled`、`quiesce`、壊れた driver の箇条と、`crates/eml_rt/src/driver.rs` の `cancel` と `quiesce` の doc (T-6)。

#### D-5: waker と起こし
- 決定
  - 起こしに使う OS の物 (Linux は eventfd、Apple はパイプの両端、Windows は完了ポートのハンドル) を `Arc` の中で waker が持ち、backend はその `Arc` を分け持つ。
  - `wake` は確保も lock もせず、atomic の旗で syscall をまとめ、誤りを捨てる。`Backend::wait` は起こしと時間切れを区別して返す。
  - driver を落とした後の `wake` は害がない。`IoWaker` をシグナルの handler から呼ばない規則は変えない。
- 理由: `IDLE_TICK` をなくす道と、監視のスレッドが worker を起こす道のため。読む端を先に閉じると、埋め込んだ eml では SIGPIPE が既定のままで process が終わりうる。
- 退けた案: macOS の起こしを `EVFILT_USER` にする (waker が kqueue の fd を持ち、driver を落とした後に fd の番号が使い回される)。パイプに `F_SETNOSIGPIPE` を立てて読む端を先に閉じる (読む端を `Arc` に置けば fcntl も EPIPE の扱いも要らない)。
- 影響の範囲: `IoWaker` の実装、`doorbell.rs`、IOCP の `Port`、`Backend::wait`。
- 見直す条件: `IDLE_TICK` をなくすとき (roadmap の「10 ms の上限をなくすこと」)。
- 守る規則
  - G-6 MUST: waker は `Arc` の中に atomic の旗と OS の物 (Linux は eventfd、ほかの unix はパイプの読む端と書く端の両方、Windows は完了ポートのハンドル) を所有し、backend は同じ `Arc` を持って読む。`wake` は確保も lock もせず、誤りを捨てる。`Backend::wait` は起こしと時間切れを区別して返す。driver を落とした後の `wake` は、`Arc` が残る限り OS の物が生きているので、EPIPE にも SIGPIPE にもならない。シグナルの handler から呼ばない。
- 適合の確かめ
  - レビューで、`IoWaker::wake` の本体に `Vec`、`Box`、`Mutex`、`lock` がない
  - `tests/real.rs` の `a_wake_from_another_thread_ends_a_wait_with_a_far_deadline` と `a_wake_after_the_driver_is_dropped_raises_no_sigpipe` (unix) が通る
  - `sys::tests` の5本 (付録1 §13) が4つの対象で通る
- リポジトリに残す所: architecture.md の「`eml_io` の内部」の doorbell の行 (T-6)。

#### D-6: プールと、`Shared` と `Local` の分け方
- 決定
  - プールは driver ごとに持ち、スレッドは `spawn_masked` で起こし、上限 32 本、空き 60 秒で終わる。上限に達した仕事は driver の中の FIFO の列に積み、`submit` は呼んだスレッドを止めない。結果は `Shared` の受け箱に置いて waker を鳴らす。
  - driver の状態を `Shared` (スレッドをまたいでよい。鍵を振る `AtomicU64`、受け箱、waker) と `Local<B>` (表、ハンドル、backend、`done`、プール、解決器) に分ける。別のスレッドからの受け付けの API はこの移行で作らない。作るときは `Arc<Shared>` を持つ別の型が受け箱に受け付けを積み、持ち主のスレッドが OS に出す形を既定にする。
  - 解決器は今のまま自前のスレッドで、結果は同じ受け箱に入れる。上限 (同時 8、手放した 64) と `with_lookup` は変えない。全 OS でコンパイルする。
- 理由: compio の `yield_now` の空回りの間は Ctrl-C も上限も届かない。列にすれば `submit` の契約「失敗しない」はそのままで、呼んだスレッドを止めなくなる。`Shared` と `Local` の分け方は、`async-threads` で受け付けを別のスレッドから出すときに driver の型を作り直さないための最小の準備である。
- 退けた案: プールを process 全体で1つにする (multicore.md の「各 worker の I/O の driver の一部」と食い違う)。`mpsc` と `Mutex<Receiver>` で作る (受ける側が lock を持ったまま待ち、ほかの空きのスレッドの 60 秒の数えが止まる)。
- 影響の範囲: `src/real/pool.rs`、`src/real/shared.rs`、`src/real/resolve.rs`、`Op::Job` の道。仕事の長さの予算は `ffi-blocking` の論点のまま。`IoDriver` の trait は変えない。
- 見直す条件: `async-threads` で受け付けを別のスレッドから出すとき。`ffi-blocking` で長い仕事を載せるとき (列の長さの上限)。
- 守る規則
  - G-7 MUST: プールは driver ごとに持ち、スレッドは `spawn_masked` で起こし、上限は 32 本、上限に達した仕事は FIFO の列に積み、`submit` は呼んだスレッドを止めない。結果は `Shared` の `Inbox` に置いて waker を鳴らす。`Drop` は仕事と名前の解決を待たず、プールを落とすと受け付けを閉じて (旗と `notify_all`、または channel の閉じ) 空きのスレッドはその場で終わり、仕事中のスレッドはその仕事の後で終わる。`quiesce` は列と実行中の仕事をすべて待つ。
  - G-8 MUST: driver の状態を `Shared` (`Send + Sync`。鍵の `AtomicU64`、`Inbox`、waker) と `Local<B>` (表、ハンドル、backend、`done`) に分ける。`Shared` に `RawBuf` を置いてはならない。別のスレッドからの受け付けの API はこの移行で作らない。
- 適合の確かめ
  - `tests/real.rs` の `a_job_beyond_the_pool_limit_does_not_block_submit` (33 本目の `submit` が 10 ms 以内に戻る)、`dropping_a_driver_does_not_wait_for_a_running_job`、`pool_threads_block_interrupt_and_terminate` が通る
  - Miri の `real::` のテストが `-Zmiri-ignore-leaks` なしで通る
  - `src/real/tests/` に、`Shared` に `send_sync` を当てる assert と、`RealDriver` が `Send` でないことの型の assert がある
- リポジトリに残す所: vm.md の「時計と driver」の `Job` の箇条と「上限と割り込み」のプールのスレッド、`crates/eml_rt/src/driver.rs` の `submit` の doc (T-6)。async.md の「未決の論点」の回り道の箇条 (T-6)。

#### D-7: unsafe の囲い
- 決定
  - `crates/eml_io/src/lib.rs` と `crates/eml_io/tests/main.rs` に `#![deny(unsafe_code)]` を置き、`#![allow(unsafe_code)]` は OS を呼ぶモジュールと `buf.rs` と `tests/real.rs` にだけ書く。一覧はソースを読むテストで固定する。
  - `RawBuf` を slice にするのは `buf.rs` の `Lent::bytes` と `Lent::room` だけで、本物の driver もテストの driver もそれを使う。`Lent::new` は安全な関数で、呼び手を限る。
  - `unsafe impl Send` と `unsafe impl Sync` は crate のどこにも書かない。`unsafe` の塊には日本語の SAFETY のコメントで、どの不変条件に頼るかと spec の節を書く。
- 理由: `eml_io` は Verus の外で、Miri も syscall を動かせないので、人が読む範囲を小さく固定するのが守りになる。語の数え方を決めておかないと、コメントの `unsafe` で誤検出し、`allow` だけのファイルを見逃す。
- 退けた案: `Lent::new` を `unsafe fn` にする (`submit` が安全な関数である限り義務の場所は変わらず、`real/mod.rs` と `test.rs` に `unsafe` の塊が入って一覧と両立しない)。
- 影響の範囲: `crates/eml_io/src` と `crates/eml_io/tests` のすべて。`eml_vm` の `jobs.rs` の規則はそのまま。
- 見直す条件: 一覧の外に OS を呼ぶモジュールが要るとき。
- 守る規則
  - G-10 MUST: `crates/eml_io/src/lib.rs` に `#![deny(unsafe_code)]` を置き、`#![allow(unsafe_code)]` は一覧のモジュールだけに書く。ソースを読むテストが、`//` を除いた上で識別子の一部でない語 `unsafe` を持つファイルの集合と `allow(unsafe_code)` を持つファイルの集合を、どちらも一覧と比べる。`unsafe` の塊には日本語の SAFETY のコメントで、どの不変条件に頼るかと spec の節を書く。
    - 読み: 数えるのは、生のポインタを読み書きする所である。`RawBuf` の約束は `IoDriver::submit` の呼び手が負い、`eml_rt` の doc がそれを言う。`Lent::new` は安全な関数で、呼び手は `buf.rs` (テスト用)、`real/mod.rs`、`test.rs` に限り、grep で確かめる。
  - G-11 MUST: `RawBuf` を slice にするのは `src/buf.rs` の関数だけで、本物の driver とテストの driver の両方がそれを使う。`Op::Job` の closure、`Shared`、プールのスレッドに渡す型のどれにも `RawBuf` を置いてはならない。
- 適合の確かめ
  - `cargo nextest run -p eml_io --test integration -E 'test(unsafe_code::)'` → 2本が通る
  - `grep -rn "unsafe impl" crates/eml_io` → 0行
  - `grep -rn "from_raw_parts" crates/eml_io/src` → `buf.rs` だけ (T-7 から)
  - `grep -rn "Lent::new" crates/eml_io/src` → `buf.rs`、`real/mod.rs`、`test.rs` の行だけ
  - `src/real/tests/` に、`pool::Job` と `Message` が `Send` であることの assert がある
- リポジトリに残す所: architecture.md の「`eml_io` の内部」の unsafe の文 (T-2 が書き、T-6 が仕上げる)。

#### D-8: 確かめ方と対象
- 決定
  - 核は OS を呼ばない偽の backend の上の単体テストで確かめ、Miri の2つの設定で流す。OS を呼ぶ部分は4つの対象の CI の本物のソケットのテストと、Linux の ASan のジョブで確かめる。
  - 対象に aarch64-linux を足す。flake の `systems` は x86_64-linux、aarch64-linux、aarch64-darwin の3つにし、Verus と verusfmt は aarch64-linux では定義しない。CI の `test` の matrix は4つにする。
  - ワークスペースに `target_arch` の cfg を書かない。
- 理由: Miri が syscall を動かせず、`pthread_sigmask` の shim もない以上、核と OS の層を分けて、核だけでも Miri に通す。arch の差 (`epoll_event` の packed) は `libc` に閉じる。Verus の配布物は aarch64-linux にない。
- 退けた案: aarch64-linux を別の小さい段にする (flake と CI の変更だけなので、この段の最初のタスクにすれば以後のコミットを arm で確かめられる)。
- 影響の範囲: `src/real/tests/`、CI の `test`、`miri`、`asan` のジョブ、`flake.nix`、`nix/*.nix`、CLAUDE.md、testing.md。
- 見直す条件: 2つ目の Linux の backend を足すとき (D-2)。
- 守る規則
  - G-12 MUST: 核は OS を呼ばない偽の backend の上の単体テストで確かめ、そのテストを Miri の2つの設定で流す。偽の backend は readiness の台本と完了の台本を持ち、EAGAIN の列、短い書き、`ECONNABORTED`、取り消しと完了の競走、`Start::Done`、終わらない操作を台本で起こせる。`mask.rs` は `cfg(miri)` で何もしない。OS を呼ぶ部分は4つの対象の CI の `tests/real.rs` で確かめる。予算は testing.md の「Miri」の規則で決める。
  - G-13 MUST: 対象に aarch64-linux を足す。flake の `systems` は3つ (x86_64-linux、aarch64-linux、aarch64-darwin)、CI の `test` の matrix は4つ、`verus`、`miri`、`fmt` は x86_64 の Linux のまま。aarch64-linux の devShell には verus と verusfmt がなく、`eml_rt` の関門はほかの system で流すことを CLAUDE.md と testing.md に書く。ワークスペースに `target_arch` の cfg を書かず、OS の構造体は `libc` と `windows-sys` のものをそのまま使う。
- 適合の確かめ
  - `cargo miri test -p eml_io --lib real::` が両設定で通り、testing.md と CLAUDE.md と ci.yml に同じコマンドがある
  - `git grep -n target_arch -- '*.rs'` → 0行
  - `nix eval .#devShells.aarch64-linux --apply builtins.attrNames` → `[ "cross" "default" "miri" ]`
  - ci.yml の matrix に `ubuntu-24.04-arm` がある (10 章の Q-2 の場合は T-6 から)
  - `grep -n aarch64-linux CLAUDE.md docs/implementation/testing.md` → Verus の条件の行が出る
- リポジトリに残す所: testing.md の「Miri」「OS ごとのテスト」「CI」「Verus」「よく使うコマンド」、CLAUDE.md の devShell、Commands、Testing の行、`crates/eml_rt/CLAUDE.md`、architecture.md の toolchain の項 (T-1、T-3、T-8)。

#### D-9: 段の分け方と移し方
- 決定
  - 段は `io-backends` (この段。前提 `async-net`) と `net-windows` (IOCP の上のソケット。前提 `io-backends`) の2つにする。io_uring の段は置かない。段の名前 `io-driver` は閉じた段の名前なので使わない。
  - compio と自前の driver を並べる期間は置かない。段の中で新しい driver を別のモジュールに作り、`RealDriver` を差し替えるコミット (T-6) と、その直後に compio を消すコミット (T-7) の2つで切り替える。
  - `async-threads`、`async-files`、`ffi-blocking` の前提に `io-backends` を足す。`worker-threads` の前提は変えない。
- 理由: Windows を途中で壊さずに compio を消すため。最小の IOCP があれば、この段の終わりで Windows も compio なしで今の機能 (時計、仕事、waker) を保てる。ソケットを IOCP に載せる作業は量があり、`net-windows` の目的そのものなので別の段にする。compio の上で `net-windows` を先に行うと IOCP の操作を2回書く。2つの driver を同じ trait の後ろで切り替えると、テストの組と docs と `IoDriver` の契約が2つになる。
- 退けた案: 1段で `net-windows` まで含める (途中のコミットで Windows の CI が赤くなる期間ができる)。compio と自前の driver を feature で切り替える期間を持つ。
- 影響の範囲: roadmap の段の列とこの段のコミットの列。段の中で新しいモジュールを compio の隣に作る一時的な共存は許す。
- 見直す条件: なし (段を閉じれば消える決定)。
- 守る規則
  - G-14 MUST: 段は `io-backends` と `net-windows` の2つで、段の名前は roadmap の中で一意にする (`io-driver` は使わない)。compio は、`RealDriver` を差し替えるコミットの直後のコミットで `Cargo.toml` から消し、その2つの間にほかの作業を入れない。compio と自前の driver を同じ trait の後ろで切り替える feature、型、環境変数を作ってはならない。io_uring の段は置かない。
  - G-15 MUST: compio への言及は、段を閉じるときに `docs/`、コード、コメント、CLAUDE.md、README.md のどこにも残さない。spec の書き換え (vm.md、architecture.md、async.md、roadmap、testing.md、status.md) は規則を実装するコミットと同じコミットで行う。
- 適合の確かめ
  - `git log -p -- crates/eml_io/Cargo.toml` で、T-6 のコミットの次のコミットが `compio-*` を消し、その後に compio が残るコミットがない
  - `git grep -n 'io-driver' -- docs ':!docs/plans'` → 0行
  - `git grep -n -i compio -- . ':!docs/plans'` → 0行 (T-7 から)
- リポジトリに残す所: roadmap の段の列 (T-0)。移し方はコミットの履歴に残る。

### 5.2 短中期の設計

理由があれば実装の中で変えてよいが、変えたらコミットのメッセージに書く。細部は付録にある。

#### D-10: `Backend` の trait と境界の型
- `src/real/backend.rs` の `trait Backend` は、関連型 `Slot: Default` と `Socket`、定数 `KERNEL_WRITES_AFTER_CANCEL`、関数 `new`、`listen`、`socket`、`shutdown_write`、`set_nodelay`、`start`、`cancel`、`wait`、`drain_broken` を持つ。境界の型は `Start`、`Finished`、`Outcome`、`Woke` である。署名と doc の文は付録1 §2 のとおりにする。
- doc の要点: `wait` の `Err` は、OS の待ちの失敗か、backend が覚えた取り消しの失敗である。`cancel` で出せなかった取り消しは次の `wait` の `Err` になる。`drain_broken` は 1 秒まで確かめ、終わらなければ鍵を書いて abort し、readiness は何もしない。`wait` の doc は「割り込み」と書き、`EINTR` と書かない (D-2 の grep)。
- `close` は trait に置かない。記録が `Rc<B::Socket>` を持ち、fd は最後の記録が落ちるときに閉じる。
- module は private にし、中の項目は `pub` にする (付録1 §1)。

#### D-11: 表、記録、核の方針
- `Table<B>` は `HashMap<OpKey, Record<B>>` と I/O の記録の数を持つ。記録は `cancelled` の印と `Work::{Io, Job, Lookup}` を持ち、`IoOp` は `Io<B::Socket>` と `B::Slot` を持つ。`Table::keys` は `quiesce`、`Drop`、`drain_broken` だけが使う。
- 核の方針は `Local::settle` の表 (付録1 §4) に従う。取り消しを頼んだ記録の backend の取り消しの誤りは、核の誤り `cancelled` に替える。印のない取り消しの終わりは debug で落とし、release では `the operation was aborted` にする。
- `cap == 0` の `Read` と `Write` は、核が backend を呼ばずに `Ok(0)` で終える。

#### D-12: `IoDriver` の各メソッドの手順
- 手順は付録1 §5 のとおりである。`wait` は `IDLE_TICK` で1回の待ちを切り、`Woke` の値によらず受け箱を空ける。`completions` は I/O の記録があるときだけ `poll(Some(ZERO))` を呼ぶ。
- `quiesce` の中では仕事の panic をその場で起こし直さず、表が空になってから起こし直す。
- 1つのハンドルの同じ向きに重ねた操作は、試さずに列の後ろに積む。重ねた呼び手にはバイトの順を約束しない。

#### D-13: epoll と kqueue
- 関心は level で、fd ごとの読みと書きの列が空でない間だけ登録する。epoll は ADD、MOD、DEL を、kqueue は向きごとの `EV_ADD` と `EV_DELETE` を使い、`EV_CLEAR`、`EV_ONESHOT`、`EV_DISPATCH`、`EV_RECEIPT` は使わない。
- `Op` ごとの手順は `src/sys/unix/ops.rs` の `first` と `ready` を2つの backend が共有する (付録1 §6)。
- 理由: 登録の寿命が「その fd に保留の操作がある間」に一致し、fd の番号の使い回しに対して安全で、`close` の知らせが要らない。計測のない今は、syscall の数より単純さを取る。
- 退けた案: oneshot と `EV_DISPATCH` (閉じた fd の後始末が要る)、edge (古い印で `Readable` が早く終わる)、登録を外すのを遅らせる (kqueue で止まる)。

#### D-14: doorbell
- Linux は `eventfd`、Apple はパイプの両端を `Doorbell` が持ち、`Arc<Doorbell>` を waker と backend が分け持つ。`clear` は「fd を読む → 旗を下ろす」の順にする (付録1 §7)。
- `eml_vm` の監視のスレッドの自己パイプとは型を分ける。

#### D-15: プールと解決器の形
- プールの状態の表と `Drop` の手順は付録1 §9 のとおりである。受け付けを閉じるのは `closed` の旗と `notify_all` で行う。`busy > 0` のときは join しない。
- 解決器は mpsc の channel と waker を外し、`Arc<Shared>` に結果を積む。全 OS でコンパイルする。Windows の名前の接続は、解決の後に `Backend::socket` の `Unsupported` で終わる。

#### D-16: `buf.rs` の `Lent`
- `Lent(RawBuf)` は `new`、`len`、`bytes(from)`、`room()` を持つ。`#[cfg(test)]` の `Lent::of(&mut Vec<u8>)` を T-3 で足す。ptr と cap は `Vec` の先頭と長さから取り、呼び手は完了まで `Vec` を生かす。`net-windows` の `wsa.rs` のテストがこれを使う。この段では核のテスト `a_lent_slices_nothing_past_its_end` が使う (付録1 §11、§12)。
- テストの driver も `Lent` を使い、`test.rs` の `unsafe` を消す。

#### D-17: unsafe の一覧と数えるテスト
- 一覧、途中のコミットの `LISTED`、前処理と数え方は付録1 §11 のとおりである。テストは `crates/eml_io/tests/unsafe_code.rs` の2本で、Miri では流さない。
- `mask.rs` は `src/` の直下に残す。

#### D-18: 偽の backend と Miri
- `Fake<const COMPLETION: bool>` と台本の約束、核の単体テスト (23 本と ignored の1本) は付録1 §12 のとおりである。`drain_broken` の台本は `drain k` を積むだけで、abort の道は偽の backend で確かめない。
- Miri の予算は Stacked Borrows 30 秒、Tree Borrows 30 秒、1本を足して増える `user` は2つのモードの合計で 2 秒以内とする。T-3 で測り直し、測った値の約3倍に丸めて書く。
- 33 本の仕事は Tree Borrows で重いので、核のテストは上限 2 のプールで流し、32 本の上限は `tests/real.rs` で確かめる。

#### D-19: 差し替えの前の確かめと中間のコミット
- T-4 から、`tests/real.rs` を `#[path]` で `src/sys/tests.rs` の module `driver` に取り込み、新しい driver の上でも流す (`cfg(all(test, unix))`)。公開の型は `RealDriver` の1つのままで、G-14 の「切り替える型」に当たらない。T-6 で取り込みを外す。
- 差し替えまで、核と backend は公開の型から届かないので、`lib.rs` の `mod real;` と `mod sys;` に `#[allow(dead_code, reason = "差し替えるまで公開の型から届かない")]` を付ける。`expect` は test の build で落ちるので使わない。T-6 で外し、`mod compio;` と古い `mod resolve;` に `#[allow(dead_code, reason = "次のコミットで消す")]` を付ける。

#### D-20: aarch64-linux の flake、CI、ASan
- flake は `systems` に `aarch64-linux` を足し、`hasVerus = inputs ? "verus-${system}"` で verus と verusfmt を `lib.optionalAttrs` で外す。`.#cross` に `aarch64-unknown-linux-gnu` と `aarch64-apple-darwin` の rust-std を足す (付録1 §15)。
- CI は `test` の matrix に `ubuntu-24.04-arm` を足し、T-3 で `miri` ジョブに `eml_io` の手順を、T-8 で `asan` ジョブを足す。ASan は `.#miri` の nightly で `RUSTFLAGS=-Zsanitizer=address` にして `cargo test -p eml_io --target x86_64-unknown-linux-gnu` を流す。

#### D-21: この段の最小の IOCP の backend
- `Port` (完了ポートのハンドルと旗) を waker と backend が分け持ち、`Iocp` は `GetQueuedCompletionStatusEx` で 128 件ずつ待つ。`KERNEL_WRITES_AFTER_CANCEL = true`。ソケットの関数は `Unsupported` を返し、`cancel` と `drain_broken` は何もしない。形と起こしの場合分けは付録2 §A のとおりである。
- `windows-sys` の feature はこの段では `Win32_System_IO` と `Win32_System_Threading` の2つだけにする。

#### D-22: 後の段 `net-windows` の設計
- この段では実装しない。設計は付録2 §B にある。要点は、記録ごとの `OVERLAPPED` の箱、ソケットを作るときの結び付けとブロックしない設定、成功のときに packet を省くモードを使わず結果を packet だけで読むこと、`CancelIoEx` の失敗を壊れた状態にすること、`drain_broken` の 1 秒と abort、`WSAGetOverlappedResult` で読む誤りの写し、ループバックで SYN を送り直さない設定である。
- T-9 が要点を roadmap の `net-windows` の項に書き写す (付録2 §C)。`net-windows` の計画はその段の最初のコミットで別に作る。

## 6. 実装の約束

- 各タスクは、実装する規則の `spec/` の節を同じコミットで書く。コードのコメントは、この計画でなく `docs/` の節を引く (CLAUDE.md)。付録の文案とコード片に計画の番号 (D-x、T-x) は書かない。
- 日本語を書くときは、natural-japanese のスキルに従う。付録の文案とコード片のコメントも、写すときにもう一度当てる。
- `src/real/` には語 `EINTR`、`Interrupted`、`SO_ERROR`、`EINPROGRESS` をコメントにも書かない。`src/sys/` には `ECONNABORTED` と `WriteZero` を書かない (D-2)。
- `mem::forget` と `Box::leak` はどこにも書かない。abort の前の書き出しは `let _ = writeln!(std::io::stderr(), …)` で書き、書きの誤りを捨てる。`eprintln!` は書けないと panic するためである (D-3)。
- `unsafe impl` を書かない。`#![allow(unsafe_code)]` を足すファイルは、同じコミットで `tests/unsafe_code.rs` の `LISTED` に足す (D-7)。
- `Lent::new` は `buf.rs`、`real/mod.rs`、`test.rs` のほかで呼ばない (D-7)。
- 新しいテストは、どれも単独で 1 s 以内に終わり、実時間を測るテストは下限だけを確かめる (testing.md の「性能のテスト」)。例外は `#[ignore]` の `quiesce_panics_when_a_cancelled_op_never_finishes` だけである (D-4)。
- テストの成否と期待値は、7 章と付録1 §14 に挙げたテストと範囲の中だけで変える。期待値を変えない機械的な追随は許す。
- `eml_rt` を変えるタスク (T-6。doc コメントだけ) は、`crates/eml_rt/CLAUDE.md` の関門を通す (D-4、D-6)。
- `eml_vm` のコードは変えない。変えるのは `Machine::drop` のコメント1つだけである (D-3)。

## 7. 実装のタスク

各タスクは1つのコミットである。付録の節との対応は表の右端の列にある。

| ID | タスク | 決定 | 依存 | 付録 |
|---|---|---|---|---|
| T-0 | 計画を入れ、roadmap に段を足す (`Plan io-backends`) | D-1、D-2、D-9 | - | 付録1 §16.2 |
| T-1 | aarch64-linux の flake と CI (`Run the tests on aarch64-linux`) | D-8、D-20 | T-0 | 付録1 §15 |
| T-2 | `buf.rs` と unsafe の囲い (`Lend driver memory through buf.rs and deny unsafe code in eml_io`) | D-7、D-16、D-17 | T-1 | 付録1 §11 |
| T-3 | 核と偽の backend (`Add the driver core with a fake backend`) | D-2、D-3、D-4、D-6、D-8、D-10、D-11、D-12、D-15、D-16、D-18 | T-2 | 付録1 §2〜§5、§9、§10、§12 |
| T-4 | epoll と kqueue (`Add the epoll and kqueue backends and run tests/real.rs on them`) | D-2、D-5、D-13、D-14、D-19 | T-3 | 付録1 §6〜§8、§13、§14 |
| T-5 | 最小の IOCP (`Add the minimal IOCP backend`) | D-1、D-3、D-5、D-21 | T-4 | 付録2 §A |
| T-6 | 差し替え (`Run the real driver on the own backends`) | D-3、D-4、D-6、D-9、D-19 | T-5 | 付録1 §14、§16 |
| T-7 | compio を消す (`Remove compio`) | D-1、D-9 | T-6 | 付録1 §16.6 |
| T-8 | ASan (`Check eml_io under AddressSanitizer`) | D-8、D-20 | T-7 | 付録1 §15 |
| T-9 | 文書の手直しと `net-windows` の要点 | D-9、D-22 | T-7 | 付録2 §C |
| T-10 | 段を閉じる (`Close io-backends`) | すべて | T-8、T-9 | - |

### すべてのタスクに共通の完了の条件

次のコマンドがどれも、失敗も警告もなく終わる。T-0 は文書だけなので、最初と最後の行だけでよい。

```sh
cargo nextest run
cargo clippy --all-targets -- -D warnings
cargo fmt --check
nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings
nix develop .#cross -c cargo clippy --target aarch64-apple-darwin --workspace --all-targets -- -D warnings      # T-1 から
nix develop .#cross -c cargo clippy --target aarch64-unknown-linux-gnu --workspace --all-targets -- -D warnings # T-1 から
cargo nextest run -p eml_cli --test integration -E 'test(citations::)'
```

CI は、`test` の行 (T-1 から4行。10 章の Q-2 の場合は T-6 まで3行)、`fmt`、`verus`、`miri` の2行が緑であること。T-8 からは `asan` も。

`eml_io` の核を変えるタスク (T-3 から T-7、T-10) は、Miri の2つも通す。

```sh
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_io --lib real::
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows" cargo miri test -p eml_io --lib real::
```

grep の基準 (T-3 から):

```sh
grep -rn "forget\|Box::leak" crates/eml_io/src          # 0行
grep -rn "unsafe impl" crates/eml_io                    # 0行
grep -rn "SO_ERROR\|EINPROGRESS\|EINTR\|Interrupted" crates/eml_io/src/real   # 0行
```

### T-0: 計画を入れ、roadmap に段を足す
- 目的: 段の設計と計画をリポジトリに入れ、roadmap の段の列を直す (D-9)。
- 対象: `docs/plans/2026-10-29-io-backends.md`、`docs/plans/2026-10-29-io-backends/01-core-unix.md`、`docs/plans/2026-10-29-io-backends/02-iocp.md`、`docs/future/roadmap.md`
- 作業
  - 4 章の前提をすべて確かめる。最初の行が通らなければ止める。
  - roadmap を付録1 §16.2 のとおりに直す。段の列の先頭に `io-backends` を足し、`net-windows` の文と前提を直し、`async-threads`、`async-files`、`ffi-blocking` の前提に `io-backends` を足し、「その後の項目」の「io_uring の backend」を書き換える。
  - `io-backends` の「前提」は「なし」と書く。`async-net` はもう閉じていて、roadmap の「進め方」の手順5が閉じた段の名前を前提から外しているためである。
  - 基点で `cargo nextest run` を流し、通ったテストの数をコミットのメッセージに書く (9 章の基点の確かめ)。
- テストの変更 (この計画の承認で合意したもの。種類は testing.md の「テストの変更の運用」)
  - 成否の変更 (種類1): `crates/eml_io/tests/real.rs` の `dropping_a_driver_waits_for_a_running_job` を T-4 で消す。新しい driver は落とすときに仕事を待たないためである。代わりに T-6 で `dropping_a_driver_does_not_wait_for_a_running_job` を足す。
  - 成否の変更 (種類1): `src/real.rs` (T-3 から `src/compio.rs`) の単体テスト `a_wake_from_another_thread_ends_a_long_poll` を T-6 で消す。compio の driver を消すためである。代わりは T-4 の `sys::tests::a_wake_from_another_thread_ends_a_long_wait` である。
  - 期待値の変更 (種類2): `listen_accept_and_connect_are_unsupported` (Windows) の `Accept { listener: 0 }` と `shutdown_write(0)` の期待値を、T-6 で `Unsupported` から `InvalidInput` (`no such handle`) にする。核は Windows の分岐を持たず、ハンドルの検査が先に当たるためである。
  - 機械的な追随 (種類3): 付録1 §14 の残り (`tests/real.rs` の `allow` と `use`、テストの説明の文、`compio::` への名前の変更)。
- 実装者に任せること: なし
- 完了の条件
  - `cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` → 通る
  - `git grep -n 'io-backends' docs/future/roadmap.md` → 段の列の1行と、`net-windows`、`async-threads`、`async-files`、`ffi-blocking` の前提の行が出る
  - `git grep -n 'fusion\|io_uring で失敗するテスト' docs/future/roadmap.md` → 0行
- 落とし穴: async-net の閉じる作業で roadmap の行が動いている。付録1 §16.2 の文は中身で当て、行番号で探さない。

### T-1: aarch64-linux の flake と CI
- 目的: 以後のコミットを arm の Linux でも確かめる (D-8、D-20)。
- 対象: `flake.nix`、`.github/workflows/ci.yml`、`docs/implementation/testing.md`、`docs/implementation/architecture.md` (toolchain の項)、`CLAUDE.md`、`crates/eml_rt/CLAUDE.md`
- 作業: 付録1 §15 のとおりにする。flake の3つの system、`.#cross` の2つの target、CI の matrix の4つ目、docs の文。compio のままで arm のジョブが通ることを確かめる。
- 実装者に任せること: docs の文の細部
- 条件: CI の `test (ubuntu-24.04-arm)` が落ちたら、付録1 §15 の末尾の形 (arm の行と arm の文を入れない形) でコミットし、入れなかった部分を T-6 に回す (10 章の Q-2)。
- 完了の条件
  - 共通の条件
  - `nix eval .#devShells.aarch64-linux --apply builtins.attrNames` → `[ "cross" "default" "miri" ]`
  - `nix eval .#packages.aarch64-linux --apply builtins.attrNames` → `[ "default" "eml" ]`
  - `nix eval .#packages.x86_64-linux --apply builtins.attrNames` と `nix eval .#packages.aarch64-darwin --apply builtins.attrNames` → どちらも `[ "default" "eml" "verus" "verusfmt" ]`
  - `nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- .github/workflows/ci.yml` → 何も出さずに 0 で終わる
  - `grep -n aarch64-linux CLAUDE.md docs/implementation/testing.md` → Verus の条件の行が出る
  - `git grep -n target_arch -- '*.rs'` → 0行
  - CI の `test (ubuntu-24.04-arm)` → 緑 (条件の場合を除く)
- 落とし穴: `nix/verus.nix` と `nix/verusfmt.nix` の `meta.platforms` は変えない。Verus の input がない system で `inputs."verus-${system}"` を読むと評価が落ちるので、`hasVerus` で包む。

### T-2: `buf.rs` と unsafe の囲い
- 目的: `RawBuf` を slice にする所を `buf.rs` に集め、`eml_io` に `deny(unsafe_code)` を置く (D-7、D-16、D-17)。
- 対象: `crates/eml_io/src/{lib.rs,buf.rs,mask.rs,real.rs,test.rs}` (`real.rs` は compio のまま、頭に `#![allow(unsafe_code)]` を足すだけ)、`crates/eml_io/tests/{main.rs,real.rs,unsafe_code.rs}`、architecture.md の「`eml_io` の内部」の unsafe の文
- 作業
  - 付録1 §11 の `Lent` (`Lent::of` を除く) を足し、テストの driver を `Lent` の上に書き直す。
  - `lib.rs` と `tests/main.rs` に `#![deny(unsafe_code)]`、`buf.rs`、`mask.rs`、`real.rs`、`tests/real.rs` に `#![allow(unsafe_code)]` を置く。
  - `mask.rs` の `Masked` を `cfg(any(not(unix), miri))` で何もしない型にする。`Masked` と `Masked::new` は compio が使うので `pub(crate)` のまま残し、T-7 で private にする。
  - `tests/unsafe_code.rs` の2本を足し、`tests/main.rs` で宣言する。`LISTED` は `src/buf.rs`、`src/mask.rs`、`src/real.rs`、`tests/real.rs`。
- テスト: 新しく足すのは `unsafe_code::` の2本。`test_driver.rs` の期待値は変わらない (種類3)。
- 実装者に任せること: SAFETY の文
- 完了の条件
  - 共通の条件
  - `cargo nextest run -p eml_io --test integration -E 'test(unsafe_code::)'` → 2本が通る
  - `grep -rn unsafe crates/eml_io/src/test.rs crates/eml_io/src/test` → 0行
  - `grep -rn from_raw_parts crates/eml_io/src` → `buf.rs` と `real.rs` の行だけ
  - `grep -rn "Lent::new" crates/eml_io/src` → `buf.rs` と `test.rs` の行だけ
- 落とし穴: 数えるテストの前処理で文字列を空白にするので、テストのファイルに語 `unsafe` を分けて書く必要はない。`LISTED` は在るファイルで絞らない。

### T-3: 核と偽の backend
- 目的: 新しい driver の核を、OS を呼ばない形で入れ、Miri で確かめる (D-2、D-3、D-4、D-6、D-8、D-10 から D-12、D-15、D-16、D-18)。
- 対象
  - `git mv crates/eml_io/src/real.rs crates/eml_io/src/compio.rs` をし、`lib.rs` の `pub use` を `compio::` にする。古い `resolve.rs` は compio の側のまま残す。
  - 足す: `crates/eml_io/src/real/{mod.rs,backend.rs,table.rs,shared.rs,pool.rs,resolve.rs}`、`crates/eml_io/src/real/tests/{mod.rs,fake.rs}`、`buf.rs` の `Lent::of`
  - 直す: `tests/unsafe_code.rs` (`src/real.rs` を `src/compio.rs` に)、`ci.yml` の `miri` の手順、testing.md の「Miri」と「よく使うコマンド」、CLAUDE.md の Commands の Miri の行
- 作業
  - 付録1 §2 から §5、§9、§10、§12 のとおりに書く。`Driver<sys::Backend>` の `impl` (`new`、`with_lookup`) はまだ置かない。
  - `lib.rs` の `mod real;` に `#[allow(dead_code, reason = "差し替えるまで公開の型から届かない")]` を付ける (D-19)。
  - testing.md の「Miri」に `eml_io` の項を足す。中身は、核の単体テストを偽の backend の上で流すこと、コマンド、予算、`cfg(miri)` で `Masked` が何もしないこと、`sys::tests` は流さないこと。「ほかの crate は、次の `eml_vm` を除いて Miri で流さない」の文に `eml_io` を足す。
  - CLAUDE.md の Commands に足す: `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_io --lib real::   # Miri on eml_io's driver core over the fake backend (same two settings)`
  - ci.yml の `miri` ジョブに付録1 §15 の手順を足す。
  - Miri の2つのコマンドを `/usr/bin/time -p` で測り、`user` の約3倍に丸めた値を予算として testing.md に書く。測った数と機械をコミットのメッセージに書く。
- テスト: 付録1 §12 の表の 23 本と ignored の1本。どれも新しく足すテストである。`compio::tests::a_wake_from_another_thread_ends_a_long_poll` への名前の変更は種類3。
- 実装者に任せること: 補助の関数の分け方。予算の値 (測った値の約3倍に丸める。既定は Stacked Borrows 30 秒、Tree Borrows 30 秒)
- 完了の条件
  - 共通の条件、Miri の2つ、grep の基準
  - `cargo nextest run -p eml_io --lib -E 'test(real::)'` → 23 本が通り、1本が ignored
  - `cargo test -p eml_io --lib -- --ignored real::tests::quiesce_panics_when_a_cancelled_op_never_finishes` → 通る (約 5 秒)
  - Miri の2つのコマンドの `/usr/bin/time -p` の `user` が、testing.md に書いた予算の中
  - `grep -rn "Lent::new" crates/eml_io/src` → `buf.rs`、`real/mod.rs`、`test.rs` の行だけ
  - `cargo clippy -p eml_io --all-targets -- -D warnings` と `.#cross` の Windows の clippy が通る (中間の形で dead_code が出ないこと)
- 落とし穴
  - `src/real/backend.rs` の doc に `EINTR` と書くと grep の基準が落ちる。「割り込み」と書く。
  - `cfg_attr(not(test), expect(dead_code))` は test の build で落ちる。`allow` を使う。
  - Miri の下の待つ補助の見張りは 60 秒にする。1 秒だと Miri の仮想の時計で仕事のテストが越える。

### T-4: epoll と kqueue
- 目的: unix の backend を入れ、差し替えの前に `tests/real.rs` を新しい driver の上で流す (D-2、D-5、D-13、D-14、D-19)。
- 対象
  - 足す: `crates/eml_io/src/sys/{mod.rs,socket.rs,tests.rs}`、`crates/eml_io/src/sys/unix/{mod.rs,doorbell.rs,ops.rs,epoll.rs,kqueue.rs}`
  - `lib.rs`: `mod sys;` に T-3 と同じ `allow(dead_code)` を付ける
  - `real/mod.rs`: `#[cfg(unix)] impl Driver<crate::sys::Backend> { new, with_lookup }` を足す
  - `crates/eml_io/Cargo.toml`: `socket2` を全 OS の依存にする
  - `tests/unsafe_code.rs`: `src/sys/unix/doorbell.rs`、`src/sys/unix/epoll.rs`、`src/sys/unix/kqueue.rs` を足す
  - `tests/real.rs`: `use super::RealDriver;` にし、`dropping_a_driver_waits_for_a_running_job` を消す
  - `tests/main.rs`: `use eml_io::RealDriver;` を置く
- 作業: 付録1 §6 から §8、§13 のとおりに書く。`src/sys/mod.rs` の `mod tests` は `#[cfg(all(test, unix))]` にし、`src/sys/tests.rs` の頭に付録1 §13 の取り込みを置く。
- テスト
  - 成否の変更 (種類1): `dropping_a_driver_waits_for_a_running_job` を消す (T-0 で合意済み)。
  - 新しく足すテスト: `sys::tests` の5本。
  - 機械的な追随 (種類3): `tests/real.rs` の `use` と、`a_wake_from_another_thread_leaves_a_wait_working` の説明の文。
- 実装者に任せること: 事象の列の長さ (既定 256)、`ready` の補助の分け方
- 完了の条件
  - 共通の条件、Miri の2つ、grep の基準
  - Linux で `cargo nextest run -p eml_io --lib -E 'test(sys::)'` → `sys::tests` の5本と取り込んだ `sys::tests::driver::` の組が通る
  - `cargo nextest run -p eml_io --test integration` → compio の上で同じ組が通る
  - `grep -rn "ECONNABORTED\|WriteZero" crates/eml_io/src/sys` → 0行
  - CI の macos-15 で、`sys::` の組 (kqueue の上の `tests/real.rs`) が通る
- 落とし穴
  - `#[path]` は `src/sys/` からの相対で `../../tests/real.rs` である。
  - 取り込みの module 名を `driver` にする。`real` を含む名前にすると Miri の絞り込み `real::` に掛かる。
  - Windows では T-5 まで新しい driver を組まない (`cfg(all(test, unix))`)。

### T-5: 最小の IOCP
- 目的: Windows の `sys::Backend` を作り、差し替えの後も Windows で時計、仕事、waker を保つ (D-1、D-3、D-5、D-21)。
- 対象: 付録2 §A.2 の触るファイル
- 作業: 付録2 §A のとおりにする。`src/sys/mod.rs` の `mod tests` を `cfg(test)` にし、取り込みだけを `cfg(all(test, unix))` に残す。`real/mod.rs` の `impl Driver<sys::Backend>` の `cfg(unix)` を外す。
- テスト: `sys::tests` の5本を Windows でも流す。新しいテストは足さない。
- 実装者に任せること: `entries` の数 (既定 128)、関数の名前、SAFETY の文
- 条件: IOCP が核の trait で成り立たない点が出たら、このタスクの前に trait を直し、偽の backend のテストを同じコミットで直す (10 章の Q-13)。
- 完了の条件
  - 共通の条件、Miri の2つ、grep の基準
  - Linux で `cargo nextest run -p eml_io` → 失敗なし
  - CI の `test (windows-2025)` が緑で、出力に付録2 §A.4 の5行の `PASS` がある
- 落とし穴: 生の `HANDLE` を受ける公開の関数は clippy の `not_unsafe_ptr_arg_deref` で落ちる。`&Port` を受ける。docs はこのタスクに入れない (T-6 が書く)。

### T-6: 差し替え
- 目的: `RealDriver` を `real::Driver<sys::Backend>` にし、全体のテストを4つの対象で新しい driver の上に通す (D-3、D-4、D-6、D-9、D-19)。
- 対象
  - `crates/eml_io/src/lib.rs`: `RealDriver` と `IDLE_TICK` を `real::` から出す。新しい module の `allow(dead_code)` を外す。`mod compio;` と古い `mod resolve;` に `#[allow(dead_code, reason = "次のコミットで消す")]` を付ける
  - `crates/eml_io/src/compio.rs`: 単体テストを消す
  - `src/sys/tests.rs`: 取り込みを外す
  - `crates/eml_io/tests/real.rs`: 付録1 §14 の6本を足し、`listen_accept_and_connect_are_unsupported` を直す
  - `crates/eml_rt/src/driver.rs` の doc、`crates/eml_vm/src/lib.rs` の `Machine::drop` のコメント
  - 付録1 §16.1 の T-6 の行の文書と、付録1 §16.3 から §16.5 の文
- 作業: 付録1 §16 の文案を、日本語の規則を当ててから写す。T-1 で arm の部分を回した場合 (10 章の Q-2) は、その部分もここで入れる。
- テスト
  - 成否の変更 (種類1): `compio::tests::a_wake_from_another_thread_ends_a_long_poll` を消す (T-0 で合意済み)。
  - 期待値の変更 (種類2): `listen_accept_and_connect_are_unsupported` (T-0 で合意済み)。
  - 新しく足すテスト: 付録1 §14 の6本。
  - 機械的な追随 (種類3): 取り込みを外すこと (同じテストは結合テストで新しい driver の上を流れる)。
  - `crates/eml_vm/tests/api.rs` の本物の driver のテストは期待値を変えない。
- 実装者に任せること: architecture.md の「`eml_io` の内部」の書き方
- 完了の条件
  - 共通の条件、Miri の2つ、grep の基準
  - `cargo nextest run -p eml_io --test integration` → 失敗なし。付録1 §14 の6本が `PASS`
  - `cargo nextest run -j 1 -p eml_io` → どのテストも 1 s 以内 (ignored を除く)
  - `cargo verus verify -p eml_rt` → 誤りが 0。`verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` → 差分なし
  - `git grep -l -i compio -- . ':!docs/plans'` → 付録1 §16.1 の末尾に挙げたファイルだけ
  - `git grep -n 'CANCEL_LIMIT\|安全点を通らずに待つ\|5秒' -- docs/spec/vm.md crates/eml_rt/src/driver.rs` → 0行
  - CI の4行の `test` が緑 (macos-15 と ubuntu-24.04-arm で `tests/real.rs` の unix の組、windows-2025 で Windows の組)
- 落とし穴
  - async.md と roadmap の行は async-net の閉じる作業で動いている。付録1 §16.1 は中身で当てる。
  - vm.md の「`IDLE_TICK`」の箇条と roadmap の「10 ms の上限をなくすこと」は、async-net が書き換えた文を読んでから直す (10 章の Q-11)。
  - async.md の「ファイルとパイプ」の macOS の箇条は、compio の文だけを消し、ほかの文を残す。

### T-7: compio を消す
- 目的: compio の module、古い解決器、依存を消す。T-6 の直後に置き、間にほかの作業を入れない (D-1、D-9)。
- 対象
  - 消す: `crates/eml_io/src/{compio.rs,resolve.rs}`
  - `lib.rs`、`mask.rs` (`Masked` を private に)
  - `crates/eml_io/Cargo.toml`: `compio-buf`、`compio-driver` を消す
  - ワークスペースの `Cargo.toml` の `compio-*` の2行、`Cargo.lock`
  - `tests/unsafe_code.rs`: `src/compio.rs` を外す
  - 付録1 §16.6 の文書
- テスト: 機械的な追随 (種類3) だけである。
- 実装者に任せること: なし
- 完了の条件
  - 共通の条件、Miri の2つ、grep の基準
  - `git grep -n -i compio -- . ':!docs/plans'` → 0行
  - `grep -n "compio\|polling\|rustix\|mio\|io-uring" crates/eml_io/Cargo.toml` → 0行
  - `grep -n 'name = "compio-\|name = "polling"' Cargo.lock` → 0行
  - `grep -rn "CANCEL_LIMIT" crates/eml_io/src` → 0行
  - `grep -rn from_raw_parts crates/eml_io/src` → `buf.rs` の行だけ
  - `git log -1 --format=%s HEAD~1` → T-6 のコミットの題 (`Run the real driver on the own backends`)
- 落とし穴: `Cargo.lock` から `compio-log` と `compio-send-wrapper` も消える。`cargo build` で作り直し、手で書かない。

### T-8: ASan
- 目的: `eml_io` の OS を呼ぶ部分を Linux の AddressSanitizer で1回流す (D-8、D-20)。
- 対象: `.github/workflows/ci.yml`、testing.md の「CI」と「よく使うコマンド」、CLAUDE.md の Testing の CI の行
- 作業: 付録1 §15 の `asan` ジョブを足す。testing.md の「CI」に、ASan のジョブと壊れたときの扱い (`miri` と同じく `flake.lock` を上げる人が直し、直せなければ手順を外して理由を書く) を書く。CLAUDE.md の CI の行を「…, and Verus, verusfmt, Miri and AddressSanitizer (eml_io) on Linux through nix」にする。
- 実装者に任せること: なし
- 条件: GitHub の runner で組めなければ、このタスクを置かず、理由を T-10 のコミットのメッセージに書く (10 章の Q-4)。
- 完了の条件
  - 共通の条件
  - `nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=address CARGO_TARGET_DIR=target/asan cargo test -p eml_io --target x86_64-unknown-linux-gnu` → 通る
  - actionlint (T-1 のコマンド) → 何も出さない
  - CI の `asan` → 緑

### T-9: 文書の手直しと `net-windows` の要点
- 目的: 計画を消した後も `net-windows` の設計の要点がリポジトリに残るようにし、T-6 と T-7 で書けなかった文書を直す (D-9、D-22)。
- 対象: `docs/future/roadmap.md` の `net-windows` の項。T-6 と T-7 の後に `git grep` で見つかった、古い driver を書いた文
- 作業
  - 付録2 §C の文案を、日本語の規則を当ててから `net-windows` の項の下に写す。
  - `git grep -n 'Asyncify\|Proactor\|CANCEL_LIMIT\|polling (epoll' -- docs CLAUDE.md README.md ':!docs/plans' ':!docs/reports'` を流し、古い driver を書いた文を今の状態に直す。`yield_now` は `Std.Async` の関数の名前なので探さない。
- 実装者に任せること: 付録2 §C から写す細部の量。`net-windows` の設計者が付録2なしで始められることを基準にする
- 完了の条件
  - `cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` → 通る
  - `git grep -n 'Asyncify\|Proactor\|CANCEL_LIMIT\|polling (epoll' -- docs CLAUDE.md README.md ':!docs/plans' ':!docs/reports'` → 0行
  - `git grep -n 'WSAGetOverlappedResult' docs/future/roadmap.md` → `net-windows` の項の1行
  - `uv run <natural-japanese のスキルのディレクトリ>/scripts/lint.py --genre tech docs/future/roadmap.md` の新しい指摘を、直すか理由を付けて残す (スキルが使えないときは手で確かめる)

### T-10: 段を閉じる (`Close io-backends`)
- 目的: roadmap の「進め方」の閉じる手順を行う。
- 作業: roadmap の「進め方」の9つの手順の順に行う。この段で直すものは次のとおりである。
  1. `spec/` を仕上げる。段の名前と「今は〜、後で〜」の予告が残っていないことを確かめる。vm.md の Windows の文 (ソケットを扱わない) は今の状態の記述なので残す。
  2. 計測する。この段は `crates/eml_rt` と `crates/eml_vm` を (コメントだけ) 変えるので対象になる。`bench/scripts.sh`、`bench/run.sh`、`bench/rt.sh` を macOS のきれいな作業ツリーで流し、docs/implementation/benchmarks.md の「今の数値」の3つの表を入れ替える。macOS がなければ、表の入れ替えだけを直後の別のコミット `Measure io-backends` にする。
  3. docs/implementation/status.md: 本物の driver は自前の backend の上にあること、Windows はまだソケットを扱わないこと、aarch64-linux を CI で流すこと。
  4. docs/implementation/architecture.md: crate の構成 (`eml_io` の `real/`、`sys/`、`buf.rs`) と「外部 crate」が T-6 と T-7 のとおりであることを確かめる。
  5. roadmap
     - `io-backends` の項を消す。`net-windows`、`async-threads`、`async-files`、`ffi-blocking` の「前提」から `io-backends` を外す (roadmap の「進め方」の手順5)。
     - 「その後の項目」の「非同期」に、後に回した項目「readiness の backend の書きの続きの syscall を減らすこと」を足す。後に回した理由は「計測のない今は、登録の寿命を保留の操作の寿命に合わせる単純さを取る。書きの続き1回に syscall が2つ多いが、写すバイトに比べて小さい」、見直す条件は「`Std.Net` の計測で、書きの続きの1周が目立ったとき。形は、列が空になった fd の登録を、その fd を持つ最後の記録が落ちるまで外さないことで、登録を外すのを遅らせるだけの形は kqueue で止まるので採らない」にする。
  6. docs/overview.md の決定の表と、docs/README.md の地図の「内容」の欄を、変わった所だけ直す。
  7. `CLAUDE.md` を確かめる (T-1、T-3、T-6、T-8 で直してある)。
  8. `docs/plans/2026-10-29-io-backends.md` と `docs/plans/2026-10-29-io-backends/` を消す。消す前に、`docs/` に残っていない決めがないことを確かめる。
  9. コミットのメッセージに、何をどこへ移したか、テストの変更の種類と理由、計測の目立つ差、後に回した項目を決めた日 (2026-10-29) を書く。Docker Desktop か OrbStack の上の aarch64 の Linux のコンテナで `cargo nextest run -p eml_io` を流した結果を書く (流せる機械がなければその旨)。T-8 を置かなかったときはその理由を書く。
- 完了の条件
  - 共通の条件、Miri の2つ、grep の基準
  - `cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` → 通る
  - `git grep -n -i compio -- . ':!docs/plans'` → 0行
  - `git grep -n target_arch -- '*.rs'` → 0行
  - `grep -rn "EML_IO_BACKEND" crates` → 0行
  - `grep -rn "process::abort" crates/eml_io/src` → 0行
  - `git grep -n 'io-backends' docs` → roadmap の前提の欄に段の名前が残っていない
  - `ls docs/plans` → 何もないか、ディレクトリがない

## 8. 実行の順

T-0 から T-8 は1列に進める。T-6 と T-7 は隣り合わせ、間にほかのコミットを入れない (D-9)。T-9 は文書だけの作業なので、T-7 の後なら T-8 と前後してよく、T-10 の前に終える。

```
T-0 → T-1 → T-2 → T-3 → T-4 → T-5 → T-6 → T-7 → T-8 → T-10
                                            └── T-9 (T-7 の後、T-10 の前) ──┘
```

## 9. 検証の計画

- 基点の確かめ (最初の変更の前): `nix develop -c cargo nextest run` → 失敗なし。通った数を T-0 のコミットのメッセージに書く。`git log origin/main --oneline | grep 'Close async-net'` → 1行
- 全体の検証 (すべてのタスクの後)
  - 7 章の共通の条件、Miri の2つ、grep の基準
  - `cargo clippy -p eml_cli --no-default-features --features types -- -D warnings`、`--features core`、`cargo clippy -p eml_test_support --no-default-features --features hir -- -D warnings` → 警告なし
  - `cargo verus verify -p eml_rt` → 誤りが 0
  - `nix build` → 成功
  - `cargo nextest run -j 1` → `SLOW` の印のあるテストがない
  - GitHub の CI の `test` (4行)、`fmt`、`verus`、`miri` (2行)、`asan` が緑
- 利用者が手で確かめること
  1. Apple Silicon の Mac の上の Docker Desktop か OrbStack の aarch64 の Linux のコンテナで、`timeout 120 cargo nextest run -p eml_io` が通ること (成功の基準の aarch64-linux。結果を T-10 のメッセージに書く)
  2. macOS か Linux で、`Net.listen "127.0.0.1" 0` で受けるサーバを書き、別の端末から `nc 127.0.0.1 <port>` でつなぎ、`read_line` と `write_line` の往復ができること。eml のファイルは scratch に置き、`timeout 20` の下で流す (新しい backend の上の `Std.Net`)
  3. 同じサーバの待ちの間に Ctrl-C を1回押すと、130 で終わること (doorbell と監視のスレッドの起こし)

## 10. 危険と決めていない点

### 10.1 async-net の読み替え

async-net の残りの作業 (T-5 から T-13) が計画どおりに終わらなかったときは、次のように読み替える。どれも着手のときに 4 章の確かめで見つける。

| 崩れ方 | 読み替え |
|---|---|
| `IoDriver::pair` が残る | `Backend::pair(&mut self) -> io::Result<(Socket, Socket)>` を足す。unix は `socket2::Socket::pair` (試作でこの形が通った)、IOCP は `Unsupported` |
| `Op` の腕が増えている | それぞれを `Io` の腕と `ops.rs` の行、`Backend::start` の1つの腕に写す |
| `IoDriver` の trait の形が変わっている | 変わった形を出発点にし、この計画の「trait の形は変えない」をその形に読み替える |
| テストの名前が違う | 付録1 §14 と付録2 §B.8 の表を名前で読み替える |
| 解決器が自前のスレッドでない | 止めて報告する |
| 監視のスレッドが driver の waker を鳴らさない (async-net の D-15 が入っていない) | 設計は変わらない。docs の文 (Q-11) は、その時点の文から compio の理由だけを消す |
| async-net の閉じる作業が roadmap と async.md を別の形にした | 付録1 §16 の文を中身で当てる。当てる所がなければ、その文の書き換えを飛ばしてコミットのメッセージに書く |

### 10.2 決めていない点

| ID | 項目 | 影響 | 決め方と時期 | 決まらないときの既定 |
|---|---|---|---|---|
| Q-1 | Rosetta for Linux の上の epoll の動き (一次の出典がない) | 成功の基準の範囲 | 9 章の手の確かめ1で見る | io_uring は使わないので設計は変わらない。崩れたら x86_64 のイメージを Mac で動かす使い方が対象から外れるだけで、status.md に書く |
| Q-2 | `ubuntu-24.04-arm` の runner がこのリポジトリで使えるか、compio のままで arm のテストが通るか | T-1、T-6 | T-1 の CI で確かめる | 付録1 §15 の末尾の形で T-1 をコミットし、arm の行と文を T-6 に入れる。T-6 でも通らなければ CI では流さず、roadmap の「その後の項目」に runner の条件を書く |
| Q-3 | `taiki-e/install-action` が arm で `cargo-nextest` を入れられるか | T-1 | T-1 の CI で確かめる (固定した manifest に `aarch64_linux_gnu` がある) | 入れられなければ、その行だけ `cargo install --locked cargo-nextest` に替える |
| Q-4 | GitHub の runner の nix で `-Zsanitizer=address` の build が通るか | T-8 | T-8 の CI で確かめる (この container では通った) | 通らなければ T-8 を置かず、T-10 のメッセージに理由を書く |
| Q-5 | Mac の上の Linux コンテナで実際に流すこと | 9 章 | 利用者が T-10 の前に流す | 流せる機械がなければ、T-10 のメッセージにその旨を書く |
| Q-6 | Miri の予算の最終の値 | T-3 | T-3 で測る | Stacked Borrows 30 秒、Tree Borrows 30 秒、1本あたり2つのモードの合計で 2 秒 |
| Q-7 | debug の `quiesce` の見張りの長さ (5 秒) が CI の遅い runner で誤って鳴らないか | T-3 から | CI で見る | 鳴ったら 10 秒に伸ばし、コミットのメッセージに書く。見張りを外さない |
| Q-8 | プールの空きのスレッドの寿命 | T-3 | REPL でスレッドが残ることが問題になったとき | 60 秒 |
| Q-9 | 書きの続きの syscall を減らす最適化、`Readable` で先に試す読み (`recv(MSG_PEEK)` など) | 性能 | `Std.Net` の計測で、書きの続きの1周が目立ったとき | 入れない。書きの続きの項目は T-10 が roadmap の「その後の項目」に書く。形は付録1 §6 にある |
| Q-10 | doorbell と `eml_vm` の監視のスレッドの自己パイプを1つの型にするか、`tests/real.rs` を OS ごとに割るか | T-4 | 実装の中で要が出たとき | どちらもしない |
| Q-11 | async-net の T-5 の後、監視のスレッドが driver の waker を鳴らす。vm.md の `IDLE_TICK` の箇条と roadmap の「10 ms の上限をなくすこと」の後に回した理由は async-net が書き換えている | T-6 | T-6 で、その時点の文を読んで決める | compio と kqueue の通知に依る文だけを消す。残る文が今の状態と食い違えば、止めて報告する。新しい理由を作らない |
| Q-12 | main だけにある文書の箇条 (roadmap の「完了を集める手間」、async.md の「未決の論点」の下の2つの箇条と「ファイルとパイプ」の macOS の箇条) が基点にあるか | T-0、T-6 | 4 章の確かめ | 無い箇条の書き換えを飛ばし、コミットのメッセージに書く |
| Q-13 | IOCP が核の trait で成り立たない点が T-5 で出るか | T-5 | T-5 の実装で見る | T-5 の前に trait を直し、偽の backend のテストを同じコミットで直す。D-10 の変更としてメッセージに書く |
| Q-14 | `net-windows` の未確認の点 (付録2 §B.10 と §B.11) | 後の段 | `net-windows` の計画で決める | 付録2 §B.11 の既定 |

## 付録の外の記録: 検討で決まったこと

蒸し返さないために、検討の中で決まったことだけを書く。

- 利用者の決定
  - backend をどこまで自前で書くかは、設計の担当に任せた。設計は、io_uring を入れず、epoll、kqueue、IOCP をすべて自前で書く形を選んだ。
  - 段の分け方は設計の担当に任せた。設計は `io-backends` と `net-windows` の2段にした。
  - docs/future/async.md の「未決の論点」の、自前の driver に替える条件の文は、この計画どおりに書き換える。
  - 壊れた driver の後始末は、2回の裁定を経て「1 秒で終わらなければ abort し、`Box::leak` と `mem::forget` は書かない」に決まり、利用者が承認した。1回目の裁定は「1 秒の確かめの後に、終わらない箱を `Box::leak` で残す」で、利用者はこれを一度承認した。その根拠は「poisoned の `Machine` がヒープを漏らすので、漏らした箱にカーネルが後から書いても使用後解放にならない」だった。その後、run からの panic の巻き戻しでは driver が `quiesce` を通らずに `Scheduler` の中で落ち、poisoned でない `Machine` がヒープを放す道が見つかった。この道では箱だけを漏らしても使用後解放になり、`Lent` の記憶ごと漏らすことも、`Drop` から漏らしたことを伝えることもできない。そのため、放した記憶にカーネルが書く道を残さない唯一の形として abort を選んだ。壊れた状態は OS の待ちの失敗と OS が取り消しを断ったときだけで、どちらも欠陥でしか起きない。`eml_vm` の変更は要らない。
- 設計で退けた案 (D-1 から D-22 に書いたもののほか)
  - Windows の成功のときに packet を省くモード (`FILE_SKIP_COMPLETION_PORT_ON_SUCCESS`) を使う案は、非 IFS の LSP の下でモードが効かない機械を eml が見分けられず、そのときの packet が放した箱を読むので退けた。同期に失敗した呼び出しのほかは、結果を packet だけで読む。
  - IOCP の `CancelIoEx` の失敗を捨てる案は、取り消せない操作の記録が `quiesce` を終えられず、`Drop` が待ち続ける道が残るので退けた。失敗を覚えて次の `wait` を `Err` にし、壊れた driver の道に入れる。
  - `quiesce` に時間の上限を残して壊れた状態にする案は、backend の欠陥を隠すので退けた。debug の 5 秒の panic と偽の backend のテストで欠陥を見つける。
  - `Drop` の debug の見張りで panic する案は、巻き戻しで表が落ち、カーネルの手の中の箱を放すので退けた。
