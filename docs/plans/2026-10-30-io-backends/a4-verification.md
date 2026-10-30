# 付録 A4 (S4): 検証、CI、計測、文書、テストの変更

> この付録は計画の本体 (`../2026-10-30-io-backends.md`) の一部である。本体の「5.3 方針の修正と食い違いの解き方」がこの付録より優先する。`[V]` は設計の時に scratch の試作や probe で確かめた事実を指す。試作と probe のファイル (`s1-probe/` などの名前で引いているもの) は残っていない。S1〜S4 は付録 A1〜A4 を指す。


対象: S1 第1部 (§1〜§8) を所与とし、S2 r2 (epoll/kqueue) と S3 r2 (IOCP、プール、解決器、`sys/mod.rs`) の決定を事実として取り込む。そのうえで、偽の backend と Miri、`unsafe` を読むテスト、CI (aarch64-linux、ASan)、`bench/net` の前後比較と 512 の再計測、文書の一覧、テストの変更の一覧、段のコミットの順を決める。
[V] は確かめた事実 (方法を併記)、[I] は推論。scratch: `repo-s4` (625d088)、`s4-probe/`。r1 からの変更は第2部の表にまとめた。

# 第1部: 改訂した成果物 (全文)

## 1. 結論

### Q1. 偽の backend と Miri

#### 置き場所

- 核のテストは `crates/eml_io/src/driver/tests/` に置く。module の道は `driver::tests::…`。`mod.rs` が `mod fake;`、`mod abort;` と、話題ごとの子 (S3 の `pool` など) を宣言する。どれも Miri のフィルタ `driver::tests::` に乗る。
- 新しいファイルを置いたら、`trust.rs` の `FILES` に1行足す (足さないと `every_source_file_is_listed` が落ちる。Q2)。

#### 偽の backend (`driver/tests/fake.rs`)

```rust
#![allow(unsafe_code)] // RawBuf への読み書き。カーネルの代わりに貸された記憶に触れる (docs/spec/vm.md の「時計と driver」)

pub(super) struct FakeSock(pub u32);            // 行番号だけ。Clone も Copy もしない (G-14 を型で守る)

pub(super) enum Answer {
    Pending(Hold),                               // 台本が空のときの既定は Pending(Hold::None)
    Done(io::Result<Outcome<FakeSock>>),
    Fill(Vec<u8>),                               // Recv: buf の先頭へ写して Done(Size(len))。len <= cap を assert
    Take(usize),                                 // Send: buf の先頭 n バイトを `sent` へ写して Done(Size(n))
}
pub(super) enum StepKind { Peek, Recv, Send, Accept, Connect(SocketAddr) }
pub(super) enum Call {                           // 呼ばれた順の記録
    Listen(SocketAddr), Socket(SocketAddr), Close(u32), ShutdownWrite(u32), SetNodelay(u32, bool),
    Start(OpKey, StepKind), Cancel(OpKey, StepKind), Wait(Option<Duration>),
}

pub(super) struct Bell { rung: Mutex<bool>, cv: Condvar }        // IoWaker。wake は rung = true; notify_all (S3 の名前)

pub(super) struct Fake<const SOCKETS: bool = true> {
    pub now: Duration,                                   // 偽の時計。wait(Some(d)) でだけ進む
    pub bell: Arc<Bell>,
    pub finished: Arc<(Mutex<usize>, Condvar)>,          // spawn した本体が終わった数 (S3)
    pub spawned: usize,
    pub starts: HashMap<OpKey, VecDeque<Answer>>,
    pub cancels: VecDeque<io::Result<Cancel>>,           // 空なら Ok(Cancel::Done)
    pub events: Vec<Event<FakeSock>>,                    // 次の wait で全部渡す
    pub wait_error: Option<io::Error>,
    pub spawn_error: bool,
    pub listen_port: u16, pub next_sock: u32,
    pub sent: Vec<u8>,
    pub calls: Vec<Call>,
}
```

- `wait(timeout, events)` の決まり (S3 のテストの形に合わせた):
  1. `calls` に `Wait` を積む。`wait_error.take()` があれば `Err` を返す。
  2. `events` が空でなければ、全部を渡して `Ok`。
  3. `None`: 鳴るまで `cv.wait_timeout_while(10 s)` で待つ。10 s で鳴らなければ `panic!("the fake wait was never woken")` (見張り。Miri では全スレッドが止まると仮想の時計が進むので、この panic で落ちる)。鳴ったら印を下ろして `Ok`。
  4. `Some(d)`: 鳴っていれば印を下ろす。`now += d` してすぐ `Ok` を返す。ただし `d > 0` で走っているスレッド (`spawned − finished`) があれば `panic!("a timed fake wait while a thread runs")`。
- 4 の assert は、本物のスレッドを持つテストが偽の時計の進み方に依らないことを機械で守る (第2部 C-3)。本物のスレッドがある間に使えるのは、`wait(None)` と、`finished` が期待の数に届いた後の `wait(Some(0))` だけである。`CANCEL_LIMIT` や `HELD_LIMIT` の道 (偽の時計を 1 s 進める) を通すテストは、仕事も解決も出さない。
- `spawn`: `spawn_error` なら `Err(io::Error::other("no threads"))`。そうでなければ `spawned += 1` し、本体を「走らせた後に `finished` を1つ増やして `notify_all`」で包んで `builder.spawn` する。
- `start`: `calls` に積んで台本を取り出す。`Fill`/`Take` は `std::ptr::copy_nonoverlapping` で写し、`from_raw_parts` は書かない。`Take` は核が出した残りの位置 (`ptr` を進めた `RawBuf`) を strict provenance の下で読むので、短い書きの残りの計算の誤りが Miri で落ちる。
- `close(sock)` は `Close(sock.0)` を積む。`listen` と `socket` は `FakeSock(next_sock)` を返す。`Fake<false>` は `SOCKETS = false` の道を確かめる。
- テストは `driver.backend.calls` などを直接読む (`driver::tests` は `driver` の子なので、private のフィールドに届く)。

#### 核のテスト (Miri の対象)

S1 §5 の列のうち、S3 r2 の名前と重なるものは S3 の名前を採る (下の S3 の列)。ここで決めるもの:

| テスト | 台本と確かめること | G |
|---|---|---|
| `a_short_write_is_continued_under_the_same_key` | `Take(3)` → `Pending` → `Retry` → `Take(残り)`。完了は1回で `Size(全長)`、`sent` が全文 | G-2 |
| `an_aborted_accept_is_retried_under_the_same_key` | `Done(Err(ConnectionAborted))` → `Done(Accepted)`。完了は1回で `Stream` | G-2 |
| `a_refused_connect_tries_the_next_address` | lookup が [a1, a2] (with_lookup。答えは `wait(None)` で受ける)。a1 は `Err(ConnectionRefused)`、a2 は `Connected`。`calls` に `Socket(a1)`、`Close(s1)`、`Socket(a2)` | G-2、G-14 |
| `a_cancelled_connect_does_not_try_the_next_address` | a1 を `Pending` のまま取り消す。`cancelled == true` で、`Socket(a2)` がない | G-2 |
| `a_retry_delivered_twice_completes_once` | 同じ鍵に `Retry` を2回。完了は1回 | G-4 |
| `a_cancel_done_completes_at_once` | `cancel` → `Cancel::Done`。`wait` なしで `completions` に `cancelled` の完了 | — |
| `a_wait_after_a_cancel_returns_true` | `cancel` の後の `wait(None)` が `Ok(true)`。`calls` に `Wait` がない | A-1 |
| `a_second_op_in_one_direction_is_invalid_input` | 同じ handle に `Read` を2つ出すと、2つ目が `Done(Err(InvalidInput))`。`Read` と `Write` の組は通る | G-5 |
| `closing_a_handle_with_a_pending_op_closes_after_the_op` | 保留の `Recv` の間に `close`。`Close` は完了の後に1回 | G-14 |
| `quiesce_closes_the_stream_of_a_discarded_accept` | `Accepted` の完了を渡さずに `quiesce`。その `Close` が1回 | G-14 |
| `a_failed_wait_breaks_the_driver` | `wait_error` の後、`wait` は `Err`、`submit` は `Job` を含めて `Done(Err)`、`completions` は空、`cancel` は `calls` に積まない | G-12 |
| `quiesce_breaks_on_a_cancel_that_never_completes` | `Pending(Hold::Buffer)` と `Cancel::Pending` を返し、event を届けない。`quiesce` が偽の時計を 1 s 進めて壊れ (`By::Timeout`) で戻る | G-12、G-13 |
| `a_held_buffer_completed_within_the_limit_lets_drop_return` | `Hold::Buffer` の保留。`Drop` の中の `wait` で `Arrival::Cancelled` を渡すと、`Drop` が戻る | G-13 |

S3 r2 の核のテスト (そのまま受け入れる): `pool::a_job_beyond_the_limit_does_not_block_submit` (`Pool::new(inbox, 2)`)、`pool::two_jobs_submitted_to_one_idle_thread_run_together`、`a_job_panic_is_raised_on_the_driver_thread`、`a_job_whose_thread_cannot_start_completes_once_with_an_error`、`a_late_answer_of_a_cancelled_lookup_does_not_hit_the_reused_key`、`quiesce_abandons_a_lookup_and_drops_its_late_answer`、`a_ninth_lookup_starts_when_an_answer_is_taken`、`sockets_off_rejects_before_looking_up_the_handle`。

- S3 の `two_jobs_…` は `thread::sleep(5 ms)` を検出力のためだけに使い、合否には効かない (S3 の Miri の形の規則 2)。

Miri の形の規則 (testing.md の「`eml_io` の検証」に書く): (1) 出した仕事と解決は、手放したものも含めて、本体が答えを積み、核が引き取るまで進めてから driver を落とす。手放した解決は `finished` を待ってから `wait(Some(0))` を呼ぶ。(2) 順は門 (`mpsc`、`Mutex`+`Condvar`) で作る。(3) 本物の時計を使わない。(4) 切り離したスレッドを使うテストは `tests/real.rs` に置く。(5) 上限は小さな値で確かめ、32 本のスレッドは Miri で起こさない。(6) 偽の時計を進める待ちは、走るスレッドのない間だけ使う (偽の backend の assert)。

#### abort の道 (`driver/tests/abort.rs`)

- テストは2件: `a_broken_driver_with_a_held_buffer_aborts_on_drop` (`By::Wait` の後、即座に abort) と `a_held_buffer_past_the_limit_aborts_on_drop` (偽の時計で `HELD_LIMIT` を越える)。どちらもスレッドを出さない。
- 1つの `#[test]` が親と子を兼ねる。子の名前は `concat!(module_path!(), "::", "<関数名>")` から `"eml_io::"` を取り除いて作り、libtest の道 (`driver::tests::abort::<関数名>`) と一致させる。環境変数 `EML_IO_ABORT_CHILD` があれば子として driver を落とす。なければ `Command::new(current_exe()).args(["--exact", 名前, "--nocapture", "--test-threads=1"]).env(…).output()` で子を起こす。そして、`!status.success()` と、標準エラーに `Drop` の文言の定数 (`driver` の `pub(super) const`) が含まれることを確かめる。名前がずれて子が0件を流したときは、文言がないという誤りで落ちる。
- `.output()` を使う (`.spawn()` を書かない)。`#[cfg_attr(miri, ignore = "Miri はプロセスを起こせない")]` を付ける。Miri の隔離で `current_exe` は止まる [V]。

#### コマンドと予算

```sh
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_io --lib -- driver::tests::
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows" cargo miri test -p eml_io --lib -- driver::tests::
```

- CI の `miri` ジョブ (借用の規則ごとに1つ) に4つ目の step として足す (Q3)。
- 予算 (コマンドの `user`、ビルド済み): 既定は Stacked 120 s、Tree 160 s。核のテストを入れたコミットで測り、testing.md の規則 (およそ測った値の3倍、1テストの増分は Tree で 10 s 以下、機械と数をコミットメッセージに書く) で置き換える。S3 の試作の6件は Stacked 3.6 s、Tree 2.8 s だった。このコンテナには `/usr/bin/time` がない [V] ので、bash の `time` の `user` で測る。
- ふだんの実行: 偽の待ちは実時間を使わず、abort は子のプロセス1つ分なので、1 件 1 s を大きく下回る。

### Q2. `src/trust.rs`

- `lib.rs` に `#![deny(unsafe_code)]` と `#[cfg(test)] mod trust;` を置く。
- `FILES`: `src/` のすべての `.rs` (`include_str!` は cfg に依らないので、Linux でも `kqueue.rs` と `iocp.rs` を読む)。段の終わりには次のとおりで、S2/S3 が足すファイルもここに入れる。
  - `lib.rs`、`flag.rs`、`mask.rs`、`real.rs`、`test.rs`、`test/net.rs`
  - `driver/mod.rs`、`driver/backend.rs`、`driver/inbox.rs`、`driver/pool.rs`、`driver/resolve.rs`、`driver/tests/mod.rs`、`driver/tests/fake.rs`、`driver/tests/abort.rs`、`driver/tests/pool.rs` (S3 が分けたとき)
  - `sys/mod.rs`、`sys/unix.rs`、`sys/epoll.rs`、`sys/kqueue.rs`、`sys/iocp.rs`
- `SKIPPED = ["trust.rs"]`。
- `UNSAFE_FILES = ["mask.rs", "test.rs", "sys/unix.rs", "sys/epoll.rs", "sys/kqueue.rs", "sys/iocp.rs", "driver/tests/fake.rs"]`。
- 範囲 (第2部 B-5): G-15 の「`unsafe` は一覧のファイルにだけ」は `src/` の規則とする。結合テスト (`crates/eml_io/tests/`) は対象外である。前例として、`tests/real.rs:509` (`pthread_sigmask`) に今も `unsafe` がある [V]。S2 の `reset_on_close` (`setsockopt` の `SO_LINGER`) もここに置く。std の `TcpStream::set_linger` は 1.99.1 で unstable (`tcp_linger`) である [V: rustc 1.99.1 で E0658]。この範囲は段の計画と testing.md に書く。
- 消し方: `eml_rt/src/trust.rs` の `blank_comments_and_strings` を写す (`#[cfg(test)]` の中身は crate をまたいで共有できない)。test module は消さない。生の文字列 (`r"`、`r#"`) は扱わないので、それがどのファイルにもないことを自己テストで確かめる。
- テスト (どれも消した後の文で調べる。G の grep と同じ字面で、それより広げない):
  1. `unsafe_stays_in_the_listed_files` (G-15): 識別子 `unsafe` を持つファイルの集合、`#![allow(unsafe_code)]` で始まるファイルの集合、`UNSAFE_FILES` が等しい。`allow(unsafe_code)` はほかにない。`lib.rs` は `#![deny(unsafe_code)]` を持つ。
  2. `every_source_file_is_listed`: `eml_rt` と同じ `read_dir` の歩きで、`FILES ∪ SKIPPED` が `src/` の `.rs` と一致する。
  3. `the_real_driver_makes_no_slices_and_leaks_nothing` (G-13、G-15): どのファイルにも部分文字列 `mem::forget` と `Box::leak` がない。`test.rs` 以外のファイルに `from_raw_parts` がない。
  4. `the_core_names_no_os` (G-1、G-16): 核 (`driver/` の下で `driver/tests/` でないもの。4c からは `real.rs` も) の文に、部分文字列 `libc::`、`windows_sys::`、`Instant`、`thread::spawn`、`Builder::spawn`、`raw_os_error`、`ECANCELED`、`ECONNABORTED`、`EAGAIN` がない。`.spawn(` は、空白を詰めた文で直前が `backend` のときだけ許す。既定の解決 (`sys::lookup`) は S3 r2 で `sys/mod.rs` に移ったので、核の外にある。`to_socket_addrs` の確かめは S3 の grep に任せ、このテストには入れない。
  5. `readiness_backends_report_only_retries` (G-3): `sys/unix.rs`、`sys/epoll.rs`、`sys/kqueue.rs` に `Arrival::Done`、`Arrival::Cancelled`、`Hold::Buffer`、`Cancel::Pending` がない。
  6. `the_core_walks_its_table_only_to_shut_down` (G-8): `driver/mod.rs` の `mem::take`、`slots.iter`、`slots.values`、`slots.keys`、`slots.drain` が、名前が `quiesce` か `drop` の関数の中にだけある (`eml_rt` の `enclosing_fn` を写す)。
  7. `the_scanner_skips_comments_and_strings`: 小さい文 (`// unsafe`、`/* unsafe */`、`"unsafe"`、`'"'`、`<'a>`、`unsafe_code`) で数え方を確かめる自己テスト。
- 適用の時点 (第2部 C-4): 1、2、7 はコミット 2 から。3 と 4 はコミット 3 から、`real.rs` を除くすべてのファイルに当てる (`sys/` はできた時点で入る)。`real.rs` は 4c で加える。5 は `sys/` ができるコミット (4a) から。6 は 3 から。

### Q3. CI と flake

#### `ci.yml` (`s4-probe/ci-flake.diff`。actionlint で出力なし [V])

```yaml
  test:
    name: test (${{ matrix.os }})
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-24.04, ubuntu-24.04-arm, macos-15, windows-2025]
    # (ほかは変えない)

  miri:
    # (既存の3つの step の後に)
      - name: "cargo miri test -p eml_io --lib -- driver::tests::"
        if: ${{ !cancelled() && steps.rt.outcome != 'skipped' }}
        run: |
          nix develop .#miri -c cargo miri test -p eml_io --lib -- driver::tests::

  # OS を呼ぶ eml_io の unsafe と、VM が本物の driver に貸す記憶を AddressSanitizer の下で流す。Rust の ASan は
  # Windows の msvc を対象にしないので3つの対象で流す (docs/implementation/testing.md の「AddressSanitizer」)
  asan:
    name: asan (${{ matrix.target }})
    strategy:
      fail-fast: false
      matrix:
        include:
          - os: ubuntu-24.04
            target: x86_64-unknown-linux-gnu
          - os: ubuntu-24.04-arm
            target: aarch64-unknown-linux-gnu
          - os: macos-15
            target: aarch64-apple-darwin
    runs-on: ${{ matrix.os }}
    timeout-minutes: 45
    env:
      RUSTFLAGS: -Zsanitizer=address
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false
      - uses: cachix/install-nix-action@13d8dd58da0234aa297dedd986986ccb8e7f3e24 # v31.11.1
        with:
          github_access_token: ${{ secrets.GITHUB_TOKEN }}
      - name: cargo test -p eml_io
        id: io
        run: |
          nix develop .#miri -c cargo test -Zbuild-std --target ${{ matrix.target }} -p eml_io
      - name: "cargo test -p eml_vm --test integration -- api:: signal::"
        if: ${{ !cancelled() && steps.io.outcome != 'skipped' }}
        run: |
          nix develop .#miri -c cargo test -Zbuild-std --target ${{ matrix.target }} -p eml_vm --test integration -- api:: signal::
```

- 流すテスト: `eml_io` の全部 (偽の backend の核、`sys` の単体テスト、`tests/real.rs`、`test_driver.rs`、`trust.rs`) と、`eml_vm` の `api::` と `signal::` (VM のヒープを本物の driver に貸す道)。今のコードの x86_64 で 68 件と 37 件が通り、時間はビルドを含めて約 1.2 分と 2 分 (4 CPU) [V]。
- nightly は `.#miri` の fenix `complete` (日付は `flake.lock` で固まる) から取る。x86_64-linux では `librustc-nightly_rt.asan.a` と `rust-src` があり、`-Zbuild-std` で通る [V]。
- leak の検出: Linux は ASan の既定 (有効) のまま。`ASAN_OPTIONS` は書かない。macOS (arm64) は LeakSanitizer を持たないと見ている [I]。
- arm と macOS の脚の退路 (第2部 A-1): 手元で確かめられたのは x86_64 だけなので、コミット 1 の CI で次の順に判断する。
  1. ASan の報告 (heap-buffer-overflow など) が出たら、コードの不具合として直す。退路は使わない。
  2. 道具の失敗 (runtime がない、link できない、dylib の rpath、45 分の時間切れ) は、まずその脚だけ rustup の nightly にする。日付は fenix と同じにし (`nix develop .#miri -c rustc -V` の commit-date の翌日の `nightly-YYYY-MM-DD`)、`rust-src` を足す。時間切れなら、その脚の `timeout-minutes` を 90 にする。
  3. それでも緑にならなければ、その脚を matrix から外す。外したことを testing.md の「AddressSanitizer」と段の計画に書き、roadmap の「その後の項目」に足す (見直す条件: fenix か Rust の側の直り)。G-17 の ASan の対象は残った脚に減り、計画の承認でその変更を受ける。
- `ubuntu-24.04-arm`: リポジトリは public [V: `gh api`] で、image に Rustup 1.29.1 と Cargo が入っている [V: runner-images のリリースノート ubuntu24-arm64/20260906.116。検索の要約]。label が使えなくなったら、arm の行を `test` と `asan` から外し、aarch64-linux は `.#cross` の clippy だけにする (同じく計画と testing.md と roadmap に書く)。
- aarch64 で落ちたテストの仕分け (第2部 A-2): コミット 1 は、arm と ASan で落ちたテストを1件ずつ仕分けて終わる。仕分けは次の3つで、どれにするかは失敗を見てから決める。
  - (i) コードの不具合 (FMA の縮約、`c_char` の符号、整列、スタックの大きさなど): 段の中の別のコミットで直す。テストは変えない。
  - (ii) 機械に依る値 (libm の最後のビットなど): testing.md の「印を付けずに直すもの」の規則で期待値を書き直す。期待値の変更として、1件ずつ計画に足して承認を取る。
  - (iii) 動きがその OS にない: 成否の変更 (cfg の印) として、1件ずつ計画に足して承認を取る。
  - 見張りが切れた (下の B-1 の一覧): その runner での時間を測り、10 倍の規則に合わせて見張りを延ばす。期待値の変更として1件ずつ計画に足す。
  - 今の段の計画は、この仕分けの規則を持つだけで、まだ見ていない失敗の書き直しを前もって許さない。

#### `flake.nix` ([V]: scratch で変更を入れ、`nix eval` で aarch64-linux の `default`、`miri`、`cross` の devShell と packages が評価できることを確かめた。変更の前は `default` が verus の `meta.platforms` で評価に失敗する)

```nix
systems = [ "x86_64-linux" "aarch64-darwin" "aarch64-linux" ];
hasVerus = inputs ? "verus-${system}";   # Verus と verusfmt の release は x86_64-linux と aarch64-darwin だけにある
# crossToolchain の target に "aarch64-unknown-linux-gnu"
packages = rec { eml = …; default = eml; } // pkgs.lib.optionalAttrs hasVerus { inherit verus verusfmt; };
# devShells.default.packages
[ devToolchain pkgs.cargo-insta pkgs.cargo-nextest ] ++ pkgs.lib.optionals hasVerus [ verus verusfmt ];
```

- nightly のコメントを「Miri と AddressSanitizer だけが nightly を要る」に直す。devShell の名前 `.#miri` は変えない。
- `nix develop .#cross -c cargo clippy --target aarch64-unknown-linux-gnu -p eml_io -p eml_vm -p eml_cli --all-targets -- -D warnings` は今のコードで通る [V]。`libc::epoll_event` は x86_64 でだけ `packed` であり、`c_char` の符号も違う。S2 のコードはこの target の clippy でも確かめる。

### Q4. 計測

#### `bench/net` の変更 (計測の前のコミット B)

- `run.py`: `recvfrom` の引数に `MSG_PEEK` があれば、鍵を `recvfrom(peek)` にする (errno の鍵も `recvfrom(peek)!EAGAIN`)。scratch で入れて動いた [V: `s4-probe/bench-peek.diff`]。
- `report.py`: `SYS` に `recvfrom(peek)`、`recvfrom(peek)!EAGAIN`、`sendto!EAGAIN`、`epoll_pwait2`、`getsockopt`、`getpeername` を足す。ラベルごとに「目標」の表 (下の式。strace の3回の最小〜最大と、時間の中央値から出す比) を出す。

#### 目標の判定

記号: mem = `brk` + `mmap` + `munmap`。E = `recvfrom!EAGAIN` + `recvfrom(peek)!EAGAIN`。R = `recvfrom` − `recvfrom!EAGAIN` (データを読んだ読み)。P = `recvfrom(peek)` − `recvfrom(peek)!EAGAIN` (当たった覗き)。どれも床を引いた単位あたりで、strace の3回すべてで満たす。625d088 の列は、覗きを分けた scratch の `run.py` で測った [V]。

| 項目 | 件 | 式 | 合格 | 625d088 |
|---|---|---|---|---|
| 書き 1 | bulk-send-py | (main の計 − mem) ÷ (`sendto` − `sendto!EAGAIN`) | ≤ 1.05 | 1.00 |
| データのある読み (空きあり) 1 | echo-py-reuse | (main の計 − `sendto` − mem − 2E) ÷ 往復 | ≤ 1.05 | 約 1.05〜1.23 |
| 1往復 | echo-py-reuse | main の計 ÷ 往復 | ≤ 4 | 2.06〜2.30 |
| データのある読み (空きなし) は覗きと読みの 2 まで | bulk-recv-py、echo-py-fresh | (R + P) ÷ R | ≤ 2.02 | bulk 約 1.97、echo 1.98 |
| 待つ読み 3 (辺の直後の空きなしの読みは覗かない) | echo-self-fresh | (main の計 − `sendto` − mem) ÷ E | ≤ 3.05、かつ E ≥ 1.5 /往復 | 11.0 (E 2.00) |
| 操作ごとの `epoll_ctl` 0 | echo-self-fresh、echo-py-reuse | `epoll_ctl` ÷ 往復 | ≤ 0.005 | 8.00、0.02〜0.12 |
| `epoll_ctl` の総数 | echo-py-reuse | 生の main の `epoll_ctl` (床を引かない) | ≤ 2 (eventfd 1 + 接続のソケット 1) | 72〜132 |
| 受けと登録 | conn-self-32 | `accept4` ÷ 接続、`epoll_ctl` ÷ 接続 | ≤ 1.05、≤ 2.005 | 1.03、2.70 |
| 待ち1回の費用 | echo-self-fresh | (`timerfd_settime` + `read(eventfd)` + `epoll_ctl`) ÷ (`epoll_wait` + `epoll_pwait` + `epoll_pwait2`) | ≤ 0.05 | 6.0 |
| 本数によらない費用 | many-self-250、-2000 | 1接続あたり CPU (床を引いた中央値) の 2000 ÷ 250 | ≤ 1.5 | 1.53 (benchmarks.md) |

- 空きなしの読みの行 (第2部 C-1) は「覗き + 読みより多くない」ことを確かめる。「辺の直後は覗かない (1)」は、待つ読みの行が確かめる。待つ読みは、`Retry` の後に記憶で答える `Peek` + 読みで終わる (S2 Q5 の表) ので、覗けば 4 になり、3.05 を越えて落ちる。P ÷ R は表に情報として出し、前と後を並べる。
- 待つ読みの行の E は、時機で 2.00 から下がりうる。そこで等しさは求めず、比の分母が十分にあること (≥ 1.5) だけを求める。
- 外れたとき: syscall の目標は仕組みの性質なので、外れは不具合として扱う。手順は次の3つ。
  1. 小さい run を `strace -f -o` で取り、余分な呼び出しの出どころを特定する。
  2. 核か backend にあれば、直してから閉じる。
  3. driver の外 (VM の集める地点など) にあれば、原因と数を benchmarks.md の目標の表の行に書き、roadmap の「その後の項目」に見直す条件付きで足す。目標の数を書き換えるときは、計画に書いて承認を取る。
- many-self の比が 1.5 を越えたら、`perf record -g` で many-self-2000 を見て、上の 2 か 3 にする。
- 前後の CPU は benchmarks.md の「比べる規則」で判定する。後が「差」で遅くなった件は、原因を特定するまで閉じない。

#### 書きの重みの単位 (512) を先に決め、最後に前後を測る (第2部 B-3)

- 候補は 256 と 1024。64 KiB の書きは、単位が 512 以下ならどれも1回で予算を使い切るので、256 は bulk-1k だけを変え、1024 はすべての bulk を変える。
- 変種のビルド (Linux でも macOS でも同じ):

```sh
for u in 256 512 1024; do
  d=$S/wu-$u; rm -rf $d; mkdir -p $d; git archive HEAD | tar -x -C $d
  sed -i.bak "s/^const WRITE_UNIT: usize = 512;/const WRITE_UNIT: usize = $u;/" $d/crates/eml_vm/src/cells.rs
  grep -q "^const WRITE_UNIT: usize = $u;" $d/crates/eml_vm/src/cells.rs
  cargo build --release -p eml_cli --manifest-path $d/Cargo.toml
done
W='^(base|bulk-(self|shared|reuse|1k|send-py|recv-py)|echo-self-(fresh|reuse)|many-self-1000)$'
bench/net.sh u512=$S/wu-512/target/release/eml u256=$S/wu-256/target/release/eml --only "$W" --no-strace --stats > $S/wu-256.jsonl
bench/net.sh u512=$S/wu-512/target/release/eml u1024=$S/wu-1024/target/release/eml --only "$W" --no-strace --stats > $S/wu-1024.jsonl
```

- Linux: 差し替えた後のコードの上で、この container で流す。macOS: 同じ手順を利用者の Mac で流す。`bench.py` は Linux でだけ strace を流し、`run.py` は macOS の `ru_maxrss` を KiB に直す [V]。実装の agent は Linux の container で動くので、macOS の run は利用者に頼む。
- 判定 (第2部 C-2)。候補 U が 512 を置き換えるのは、Linux と macOS の両方で次の3つが成り立つときだけである。
  - (a) bulk-self、bulk-shared、bulk-reuse、bulk-1k のどれか1つ以上で、U の側に「差」がある (比 ≤ 0.90 で四分位が重ならず、「確保の揺れ」の印がない)。
  - (b) 測った件のどれにも、CPU で U に不利な「差」がない。
  - (c) 読み手を飢えさせない。bulk-self、bulk-shared、bulk-reuse の単位あたりの sys が 1.10 倍を越えない。echo-self-fresh と echo-self-reuse の実時間の比が 1.10 以下である。
- (c) を置く理由: 単位の役目は、同じ worker の読み手を起こすことである (stdlib.md の「操作の予算」)。読み手が遅れると、送りの列がたまって sys が倍になったという記録がある (benchmarks.md:447)。そこで、その症状 (sys) と往復の遅れ (実時間) を見る。
- 256 と 1024 の両方が通れば、bulk-* の比の幾何平均が小さいほうを選ぶ。どちらも通らないか、OS で結果が分かれたら、512 のままにする。
- 数は benchmarks.md の節「書きの重みの単位」に、OS ごとの表 (件、512、候補、比、判定) で書く。
- macOS の run が段の中でできないとき: 512 のままにし、Linux の数だけを書く。roadmap の「その後の項目」に「macOS での書きの重みの単位」を足す (後に回した理由: 段の中で macOS の環境がなかった。見直す条件: macOS で `bench/net` を流せるとき)。計画にこの退路を書いて承認を取る。

#### 前と後の比べ (512 を決めた後、コードの最後のコミットで)

```sh
S=<scratch>; BASE=<段の計画のコミット>   # コードは段を始めたときの main と同じ
mkdir -p $S/eml-before && git archive $BASE | tar -x -C $S/eml-before
cargo build --release -p eml_cli --manifest-path $S/eml-before/Cargo.toml
cargo build --release -p eml_cli
bench/net.sh before=$S/eml-before/target/release/eml after=target/release/eml --stats > $S/net-swap.jsonl
python3 -I bench/net/report.py $S/net-swap.jsonl
```

- 全件を流す (`busy-self` は1回 約 80 s なので、全体で約 1 時間 [I])。作業ツリーは clean にし、同じ機械でほかの負荷のない状態で流す。機械の情報 (CPU、vCPU、MemTotal、kernel) を記録する。`std/` と `bench/net/*.em` を段で変えないので `@DIR` は要らない。
- この run は 512 の決定 (と、変えるならコミット 6) の後に流すので、benchmarks.md の数は閉じるときのコードと一致する。
- 結果の置き場所 (第2部 C-5): 目標の表の「今」の列 (後の値) は benchmarks.md に書く。前と後の比べの表 (compio との差) は計測のコミットのメッセージに書き、閉じるコミットのメッセージで目立つ差を挙げる。

### Q5. 文書 (書くコミットは Q7 の番号)

| 文書の節 | 何を書くか | コミット |
|---|---|---|
| `docs/implementation/testing.md` 「CI」 | `test` は4つの runner、`asan` ジョブ、`miri` の4つ目の step。ジョブの数は5つ | 1、3 |
| testing.md 「OS ごとのテスト」 | 対象は macOS、Linux (x86_64 と aarch64)、Windows。`.#cross` に `aarch64-unknown-linux-gnu`。新しい対象で落ちたテストの仕分けの規則 (Q3) | 1 |
| testing.md 新しい節 `## eml_io の検証` (「`eml_vm` のテスト」の後) | `### AddressSanitizer` (対象、流すテスト、nightly、leak、外した脚があればその例外)、`### unsafe の一覧` (`trust.rs` の7つと、範囲が `src/` であること)、`### 核の Miri` (偽の backend の待ちの決まり、Miri の形の規則 (1)〜(6)、コマンド、予算、abort を Miri で ignore すること) | 1、2、3 |
| testing.md 「Miri」 | 「ほかの crate は、次の `eml_vm` を除いて Miri で流さない」を `eml_io` の節を引く文に直す。同時に流せる実行の数を 6 から 8 に | 3 |
| testing.md 「テストの置き場所」 | `src/` の単体テストの行に「`eml_io` の核は偽の backend の上、backend は `sys/unix.rs` の中」 | 3、4a |
| testing.md 「よく使うコマンド」 | ASan、`eml_io` の Miri、`.#cross` の aarch64 | 1、3 |
| `docs/implementation/architecture.md` 65 (toolchain) | nightly は Miri と ASan が使う | 1 |
| `docs/README.md` 「文書の地図」の testing.md の行 | 「`eml_io` の検証」を足す | 1 |
| `CLAUDE.md` Commands と Testing | ASan と `eml_io` の Miri の行。`.#cross` に aarch64 Linux。CI の項に aarch64 Linux と ASan。verus は x86_64-linux と aarch64-darwin だけ | 1、3 |
| `docs/spec/vm.md` 「時計と driver」 (460〜485、trait のコードブロック) と「上限と割り込み」 379 | S1 §5 の列のとおり | 4c |
| `crates/eml_rt/src/driver.rs` のコメント | S1 Q6 | 4c |
| architecture.md 47、77、「`eml_io` の内部」 (全文)、「外部 crate」 | 新しい木。`unix.rs` が表と歩を持ち、`epoll.rs`/`kqueue.rs` は待ちと起こしだけ (S2 U-0)。`sys/mod.rs` の `Os`、`millis`、`lookup` (S3 Q0)。compio-driver、compio-buf、socket2 の行と 425〜431 の更新の手順を消す。`windows-sys` の行に `eml_io`、`libc` の行も更新 | 4c |
| `docs/implementation/status.md` 146、147、149 | 自前の backend。Windows はソケットなし。プールは列で待つ。147 の性能の文は計測で数に合わせる | 4c、7 |
| `docs/future/async.md` 28 (表の行 10)、「`IoDriver` と `eml_io`」 (243、244、265、325)、447〜448、「未決の論点と将来の拡張」 479〜481 | compio の記述を自前の核と backend にする。479 の項は解決したので消す | 4c |
| `CLAUDE.md` Architecture の `eml_io` の行 | compio と `Asyncify` を消し、核と backend と自前のプールを書く | 4c |
| `docs/implementation/benchmarks.md` 「ネットワークの計測」の本文と「主指標」 | 覗きを分けて数えること、目標の表を report が出すこと | B |
| benchmarks.md 新しい節 `### driver の syscall の目標` | 目標の表 (項目、目標、今の値、件と式)。「34cad91 と e942eeb」の目標の表をここへ移す (その節の過去の列は持ってこない)。その節の「driver を差し替えたら、同じ機械で列を足す」の文を消す | 7 |
| benchmarks.md 新しい節 `### 書きの重みの単位` | Q4 の OS ごとの表 | 5 |
| `docs/spec/stdlib.md` 「操作の予算」、status.md 156、`cells.rs` の `WRITE_UNIT` のコメント | 512 を変えるときだけ | 6 |
| benchmarks.md 「今の数値」の3つの表 | 閉じる手順の 2 (`bench/` を変えたので対象)。`bench/*.sh` は macOS なので、測れなければ `Measure io-backends` を後のコミットにする | 8 か Measure |
| `docs/future/roadmap.md` | `io-backends` の項を消す。「`compio-driver` の 0.13 の系列」の項を消す。「プールの仕事の予算と、放さずに終わる方針」は、受け付けが止まる半分を消して `quiesce` の上限なしの待ちだけにする。`net-windows`、`io-uring`、`async-files` の前提から `io-backends` を外す。後に回した項目 (macOS の 512、外した ASan の脚など) を足す | 8 |
| `docs/overview.md` | 変える行はない [V: grep]。閉じるときに確かめる | 8 |

- 歴史の書き方 (第2部 C-5): CLAUDE.md の規則 (docs は今の状態を書き、段の名前や前の形を書かない。例外は `docs/reports/`) を優先する。benchmarks.md の「34cad91 と e942eeb」は既にある例外で、消しも広げもしない。新しい節は測るものの名前 (「driver の syscall の目標」「書きの重みの単位」) で立て、今の値だけを書く。compio との比べはコミットメッセージに置く。理由は、CLAUDE.md が benchmarks.md について「段ごとの履歴はファイルの git log」と明記していることにある。前例を増やすほど、今の状態の文書に過去の列が積もる。
- 閉じる手順 (roadmap の「進め方」、`Close io-backends` の1コミット):
  1. spec に段の名前と予告がないことを確かめる (`grep -rn "io-backends\|compio" docs/spec` が空)。
  2. 計測: `bench/scripts.sh` → `bench/run.sh`、`bench/rt.sh`、`bench/scripts.sh`。macOS がなければ `Measure io-backends` を後のコミットにする。
  3. status.md。
  4. architecture.md (4c で済んでいることを確かめる)。
  5. roadmap。
  6. overview.md と docs/README.md。
  7. CLAUDE.md。
  8. `docs/plans/` を消す (消す前に、決めがすべて docs/ に移っていることを確かめる)。
  9. メッセージ (テストの変更の種類と理由、前後の比べの目立つ差、機種、後に回した項目の決めた日)。

### Q6. テストの変更 (計画に書く一覧)

成否の変更 (1件ずつ):

- P1: `crates/eml_io/tests/real.rs` の `dropping_a_driver_waits_for_a_running_job` を消し、`dropping_a_driver_does_not_wait_for_a_running_job` を足す (全 OS、Miri の対象外)。理由: 落とすときに仕事を待たない (S1 Q3/Q4)。形は S3 r2 の順 (`started.recv()`、`drop(driver)`、門を開ける、終わりを受ける) に、testing.md の見張りの規則を足したものである。

```rust
/// 走っている仕事を持つ driver は、落とすときに仕事の終わりを待たない。仕事は driver が消えた後も走り終える。
#[test]
fn dropping_a_driver_does_not_wait_for_a_running_job() {
    use std::sync::mpsc;
    let (started_tx, started) = mpsc::channel();
    let (gate, gate_rx) = mpsc::channel::<()>();
    let (end_tx, end) = mpsc::channel();
    let mut driver = RealDriver::new().unwrap();
    submit(&mut driver, Op::Job(Box::new(move || {
        started_tx.send(()).unwrap();
        // `drop` が待てば門は開かず、10 s の見張りで false を送る
        end_tx.send(gate_rx.recv_timeout(Duration::from_secs(10)).is_ok()).unwrap();
        Ok(0)
    })));
    started.recv().unwrap();
    drop(driver);
    gate.send(()).unwrap();
    assert!(end.recv().unwrap());
}
```

- S2 と S3 が今のテストを消したり移したりすることはない (S3: 解決のテスト6件と `pool_threads_block_interrupt_and_terminate` は `tests/real.rs` に残す。S2: 既存のテストは変えない)。新しく足すテストはこの一覧に載せない。
- aarch64 と ASan の対象を足すこと自体は、テストの変更ではない。そこで落ちたテストは Q3 の仕分けで決め、(ii) と (iii) と見張りの延長だけを1件ずつここに足す。

期待値の変更 (範囲):

- E2 (条件付き): 512 を変えるときは、次のものを変える。`crates/eml_vm/src/cells.rs` の `writes_spend_budget_by_their_length` の長さと `spent` の期待値。それと、書きの重みで変わる UI テストの出力と `RunStats` の snapshot (`crates/eml_vm/tests/*.rs`、`tests/ui/run/`)。
- r1 の E1 (`a_probe_on_a_reset_stream_fails_at_submit`) は取り下げる。S2 r2 の U-1 で、記憶で答えた `Peek` が `fresh` を下ろすので、2つ目の `Readable` は覗いてリセットの誤りを返す。テストは変えずに通る (S2)。
- r1 の E3 (aarch64 の libm の書き直しを前もって範囲で許す) は取り下げる。Q3 の仕分けで、見た失敗を1件ずつ足す。

機械的な追随 (合意は要らない。コミットメッセージに書く):

- M1: `real.rs:394` の `socket2::SockRef::from(&peer).set_linger(Some(Duration::ZERO))` を、S2 の `fn reset_on_close(s: &std::net::TcpStream)` (`libc::setsockopt` の `SO_LINGER`、`// SAFETY:` 付き) に替える。G-15 の範囲は `src/` (Q2) である。
- M2: `real.rs:930` のコメント「接続を compio の中で終わらせ」を「接続を driver の中で終わらせ」に替える。
- M3: `a_job_pool_is_capped` の説明を「それを超える仕事は driver の中の列で待ち、スレッドが空いてから動く」に替える。

残すものと理由 (第2部 B-1): `tests/real.rs` の上限の比べ (73 `wait_for`、151、872、984 `Gate::wait_started`、1099、1204) は変えない。どれも ms の桁で終わる待ちの見張り (1 s) で、testing.md の「見張りの長さは、測った時間の 10 倍以上」を満たす。151 は起こしで待ちが終わったことの証拠も兼ねるが、鳴らすのは 20 ms 後で、1 s はその 50 倍である。新しい runner で見張りが切れたら、Q3 の仕分けの「見張りの延長」にする。`eml_vm/tests/api.rs:376` (`< 2 s`) も同じ理由で残す。

保つもの (変えるなら計画に足す): 誤りの文言 `cancelled`、`too many name lookups are stuck`、`no such handle`、`Unsupported` の `Display` である `unsupported` (`eml_vm/tests/api.rs:998`) [V: grep]。

S2/S3 のテストの置き場所 (この計画は名前に依らず受け入れる):

- (a) 核、プール、解決器、受け箱: `src/driver/tests/` (Miri の対象)。S3 の8件。
- (b) 本物の backend の振る舞い: `crates/eml_io/tests/real.rs` (または `tests/main.rs` に宣言した新しいファイル)。S2 の6件 (`a_probe_on_a_stream_that_had_data_before_accept_finishes`、`a_pending_read_and_a_pending_write_on_one_stream_both_finish`、`queued_connections_are_accepted_one_per_op`、`a_refused_connect_completes_once`、`a_reset_ends_a_pending_write_with_an_error`、`a_signal_during_a_wait_leaves_the_driver_working`) と、S3 の2件 (`a_wake_after_the_driver_drops_is_harmless`、`wakes_before_a_wait_end_only_that_wait`)。
- (c) backend の中の単体のテスト: `sys/unix.rs` の `#[cfg(test)] mod tests` (S2 の9件。一覧のファイルの中なので `unsafe` を書ける)。`sys/mod.rs` の `millis` のテスト (S3)。Miri の対象外で、Linux と macOS の CI と ASan で流れる。

### Q7. コミットの順と始める前の基準

始める前 (計画のコミットの上で):

```sh
cargo nextest run                                   # 全件が通る
cargo test -p eml_io 2>&1 | grep "test result"      # integration 68 passed [V]
cargo nextest run -j 1 -p eml_io                    # SLOW なし
cargo clippy --all-targets -- -D warnings
nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings
nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=address CARGO_TARGET_DIR=target/asan cargo test -Zbuild-std --target x86_64-unknown-linux-gnu -p eml_io      # 68 passed [V]
nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=address CARGO_TARGET_DIR=target/asan cargo test -Zbuild-std --target x86_64-unknown-linux-gnu -p eml_vm --test integration -- api:: signal::   # 37 passed [V]
```

コミット (検証の道具を差し替えより先に入れ、compio の driver の上で緑になった CI を基準にする):

- 0. `Plan io-backends`: `docs/plans/<日付>-io-backends.md`。S1 の逸脱3つと S2 U-1 の補い、Q6 の一覧、Q3 の仕分けと退路、Q4 の退路、§6 の挑戦への答えを書く。
- 1. `Run the tests on aarch64 Linux and under AddressSanitizer`: `ci.yml` (`test` の arm、`asan`)、`flake.nix`、Q5 の 1 の文書。arm と ASan で落ちたものを Q3 で仕分け、(i) を直すコミットを続け、(ii) と (iii) を計画に足して承認を取るまでを、この単位とする。arm と ASan に固有の失敗を、差し替えの失敗と混ぜないためである。
- 2. `Pin unsafe in eml_io to listed files`: `trust.rs` のテスト 1、2、7 と `#![deny(unsafe_code)]`。この時点の `UNSAFE_FILES` は `mask.rs`、`test.rs`、`real.rs` (compio の `Mem`)。
- 3. `Add the I/O driver core on a fake backend`: `#[cfg(test)] mod driver;`、核、受け箱、プール、解決器、偽の backend、核のテスト、abort のテストを入れる (S3 の単位で 3a/3b に分けてよい。コードを足すコミットにその Miri のテストを入れる)。同じコミットで次のものも入れる: `miri` の step、testing.md の「Miri」と「核の Miri」、CLAUDE.md の Miri の行、`trust.rs` の `FILES` と `UNSAFE_FILES` (`fake.rs`)、テスト 3、4、6 (`real.rs` を除く全ファイル)。
- 4a. `Add the epoll and kqueue backends` (S2): `#[cfg(test)] mod sys;` の下に、(c) のテストとテスト 5 を入れる。
- 4b. `Add the IOCP backend` (S3): `sys/mod.rs` の `Os`、`millis`、`lookup` と IOCP。
- 4c. `Replace compio-driver with the own driver`: `real.rs` を `Driver<sys::Os>` にし、`cfg(test)` を外す。compio-driver、compio-buf、socket2 を消す。P1、M1〜M3、S2/S3 の (b) のテスト、Q5 の 4c の文書を入れる。`trust.rs`: `real.rs` を `UNSAFE_FILES` から外し、`sys/*` を入れ、テスト 3 と 4 に `real.rs` を入れる。
- B. `Count peeks and targets in bench/net`: `run.py`、`report.py`、benchmarks.md の「ネットワークの計測」の本文。5 より前ならどこでもよい。
- 5. `Measure the write weight unit`: Linux の 512 の run と、macOS の run (利用者)。判定と benchmarks.md の「書きの重みの単位」。
- 6. (変えるときだけ) `Change the write weight unit to <U>`: `cells.rs`、stdlib.md、status.md 156、E2。
- 7. `Measure the own driver`: Q4 の前と後の全件の run (コードの最後のコミットで)。目標の表の全行の判定、benchmarks.md の「driver の syscall の目標」、status.md 147。前と後の比べの表はコミットメッセージに書く。
- 8. `Close io-backends` (Q5 の手順)。`bench/*.sh` の表に macOS が要るなら、後に `Measure io-backends`。

## 2. 根拠

- 偽の時計を `wait(Some(d))` でだけ進め、走るスレッドがある間はそれを禁じる。こうすると、1 s の上限の道 (`CANCEL_LIMIT`、`HELD_LIMIT`) を実時間なしに決定的に通せる。また、本物のスレッドを持つテストがスケジュールで偽の時計を膨らませる道も、assert で閉じられる。
- `trust.rs` のテストを G の grep と同じ字面にしたのは、S2/S3 が設計の時点で知っている規則以上のものを、テストで後から課さないためである。
- 検証の道具 (arm、ASan、`trust.rs`、Miri) を差し替えより先に入れるのは、道具や新しい環境の失敗を、差し替えの失敗と切り分けるためである。今のコードで x86_64 の ASan が通ることは確かめた。
- 目標の式に E と P を入れたのは、相手の速さで待つ読みの割合が揺れても、「データのある読みは 1」「覗きは高々1回」「待つ読みは 3」を仕組みとして確かめるためである。
- 512 の判定に (c) を置いたのは、単位の役目が読み手を起こすことで、それが崩れたときの症状 (sys の倍増) が記録にあるからである。
- 512 を決めてから前後を測るのは、benchmarks.md の数を閉じるときのコードと一致させるためである。

## 3. 退けた案

- 偽の待ちに実時間を使う: 1 s の上限のテストが 1 件 1 s の規則を破る。
- 偽の待ちを、本物のスレッドが走る間は鳴るまで待たせる (review C-3 の代案の2つ目): S3 のテストの形 (`finished` を待ってから `wait(Some(0))`) で足り、偽の backend に分岐を増やすだけになる。assert で禁じるほうが単純である。
- abort のテストを `driver/abort.rs` に置く (S1): 核のファイルになり、`Command` が核に入る。
- ASan に、はじめから rustup の nightly を使う: 日付が動く。退路でだけ、日付を固めて使う。
- `.#miri` を `.#nightly` に改名する: 写しの書き換えの費用に見合わない。
- ASan で `eml_vm` の全テストを流す: I/O と関係のない VM の `unsafe` まで、この段の門に入る。
- `trust.rs` で `ManuallyDrop`、`sleep`、`to_socket_addrs` なども禁じる (r1): G の字面を越え、`sys/*` のふつうの所有の受け渡しを妨げうる。
- aarch64 の失敗を、前もって範囲で期待値の変更として許す (r1 の E3): 見ていない失敗の書き直しを許すことになる。
- 前と後の compio との比べの表を benchmarks.md に残す: CLAUDE.md の「docs は今の状態」に反する前例を増やす。
- `tests/real.rs` の 1 s の見張りを今のうちに延ばす: 10 倍の規則を既に満たしており、根拠のない変更になる。切れたら測って延ばす。

## 4. 置いた仮定

- fenix の nightly は aarch64-linux と aarch64-darwin でも ASan の runtime を持つ [I。x86_64 だけを確かめた]。偽なら Q3 の退路に入る。
- macOS の arm64 の ASan は LeakSanitizer を持たない [I]。偽でも既定のまま流すだけで、害はない。
- cargo-nextest 0.9.146 に aarch64-linux の配布物がある [I]。偽なら `fallback: none` で落ちるので、版を上げる。
- S2 は close で `EPOLL_CTL_DEL` を出さない (S2 Q5 の数え方。`epoll_ctl(` は `new` と `register` の2か所)。
- このコンテナは benchmarks.md の 34cad91 の計測と同じ kernel の機械である [V: 6.18.44-fc-v185、4 vCPU]。違っても、前と後を交互に流すので判定は成り立つ。

## 5. 実装への含意 (作業の単位と完了の基準)

| # | 単位 | 完了の基準 (コマンド → 期待する結果) |
|---|---|---|
| 1 | arm、ASan、flake | `nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- .github/workflows/ci.yml` → 出力なしで 0。`nix eval --raw .#devShells.aarch64-linux.default.drvPath` (`miri`、`cross` も) → `/nix/store/…-nix-shell.drv`。`nix develop .#cross -c cargo clippy --target aarch64-unknown-linux-gnu --workspace --all-targets -- -D warnings` → 0。Q7 の ASan の2つ → 68 passed、37 passed。`cargo nextest run -p eml_cli --test integration citations::` → 通る。PR の CI で `test` の4行と `asan` の3行 (退路で外した脚を除く) が緑で、落ちたテストはすべて仕分けて計画に載っている (push は利用者が許したときだけ) |
| 2 | trust (3つ) | `cargo nextest run -p eml_io trust::` → 3 passed。clippy → 0 |
| 3 | 核、偽、Miri | `cargo nextest run -p eml_io` → 全件。`cargo nextest run -j 1 -p eml_io` → SLOW なし。Miri の2つ → `test result: ok`、`ignored` は abort の2件だけ、`the main thread terminated without waiting` が出ない。`user` を測ってメッセージに書く。`cargo nextest run -p eml_io trust::` → 6 passed |
| 4a/4b | backend | `cargo nextest run -p eml_io --lib sys::` → Linux で S2 の8件 (Apple の1件を除く) と `millis`。`trust::` → 7 passed。`nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc -p eml_io --all-targets -- -D warnings` と aarch64 の同じ clippy → 0。macOS と Windows の CI が緑 |
| 4c | 差し替え | `cargo nextest run` → 全件。`trust::` → 7 passed。`grep -n "compio\|socket2" Cargo.toml crates/*/Cargo.toml Cargo.lock crates/eml_io/tests/real.rs` → 空。`grep -rn "mem::forget\|Box::leak" crates/eml_io/src --exclude=trust.rs` → 空。`grep -rn "from_raw_parts" crates/eml_io/src --exclude=trust.rs --exclude=test.rs` → 空。`grep -rn compio docs CLAUDE.md --exclude-dir=reports --exclude-dir=plans --exclude=benchmarks.md` → 空。`grep -n compio docs/implementation/benchmarks.md` → 「34cad91 と e942eeb」の節の中だけ。`citations::` → 通る。CI の全ジョブが緑 |
| B | bench | `python3 -I bench/net/bench.py --no-time --only '^(base\|echo-self-fresh)$' x=target/release/eml > $S/x.jsonl && python3 -I bench/net/report.py $S/x.jsonl` → syscall の表に `recvfrom(peek)` の列があり、目標の表が出る |
| 5 | 512 | 2つの OS (か、退路で Linux だけ) の表が benchmarks.md にあり、判定 (a)(b)(c) の結果を書いてある |
| 6 | 512 を変える (条件付き) | `cargo nextest run` → 全件。E2 の範囲の中だけで期待値が変わっている |
| 7 | 前と後 | Q4 の目標の表の全行が合格するか、「外れたとき」で処理してある。後が遅い「差」の件には原因が書いてある |
| 8 | 閉じる | `grep -rn "io-backends" docs --exclude-dir=plans --exclude-dir=reports` → 空。`docs/plans` に段のファイルがない。`cargo nextest run -j 1` → SLOW は集めて流す5つだけ |

実装者に任せること: `report.py` の目標の表の見た目、`trust.rs` の補助関数、偽の backend の台本を積む小さな関数、testing.md の新しい節の文。

## 6. 未決の項目と S1 への挑戦

- C1 (S1 への挑戦。維持): 偽の backend は `RawBuf` に書くので `unsafe` を持つ。既定: `driver/tests/` を調べる対象に入れ、`driver/tests/fake.rs` を `UNSAFE_FILES` に足す。S1 の形 (`driver/tests/` を外す) を採っても、ほかは変わらない。
- C2 (S1 への挑戦。維持): G-19 の `grep -rn compio docs/spec docs/implementation CLAUDE.md` は、benchmarks.md の「34cad91 と e942eeb」の記録に当たるので、空にならない。既定: 4c の基準の式 (`--exclude=benchmarks.md` と、その節の中だけを確かめる別の grep)。
- C3 (解決済み): G-7 の echo-py-reuse の内訳は「eventfd 1 + 接続のソケット 1」(S2 Q5 と一致)。
- C4 (解決済み): 既定の解決は S3 r2 で `sys::lookup` に移った。
- C5 (解決済み): S2 U-1 で E1 は要らない。
- G-15 の範囲 (B-5): 結合テストは対象外とし、計画と testing.md に書く。G-15 の字面 (「`eml_io` 全体」) を「`src/`」に読み替える1文を計画に入れる。
- 未決: Miri の予算の値 (既定 120 s / 160 s。コミット 3 で測る)。macOS の 512 の run の日程 (既定は Q4 の退路)。ASan の arm と macOS の脚の成否 (既定は Q3 の退路)。

## 7. 追加で調べたこと

- Miri (`.#miri`、rustc 1.101.0-nightly 2026-10-25、`s4-probe/miriprobe`) [V]:
  - 動くもの: `Condvar::wait_timeout`、隔離の下の `Instant::now`、33 本のスレッド、`catch_unwind`、`stack_size(256 KiB)`。
  - 止まるもの: `sigemptyset` と `current_exe` は unsupported。切り離したスレッドが残ると、"the main thread terminated without waiting for all remaining threads" で落ちる。
- ASan: 625d088 の `eml_io` 68 件と、`eml_vm` の `api::` と `signal::` の 37 件が、`.#miri` の nightly、`-Zbuild-std`、x86_64 で通った。報告も leak もなかった [V]。
- flake: aarch64-linux の評価と aarch64 の clippy [V]。actionlint [V]。`gh api` → public [V]。
- `bench/net`: 625d088 の release で strace の run をした [V]。覗きを分けた `run.py` の scratch の版で、Q4 の 625d088 の列を出した [V: `s4-probe/before-peek.jsonl`]。主な値は次のとおり。
  - bulk-recv-py: `recvfrom` 17.19〜22.36、`recvfrom(peek)` 16.75〜21.76 /MiB。
  - echo-py-fresh: `recvfrom` 1.01、`recvfrom(peek)` 0.99 /往復。
  - echo-self-fresh: 覗きの EAGAIN 1.97〜1.98 /往復。
- `TcpStream::set_linger` は rustc 1.99.1 で E0658 (`tcp_linger`) [V]。
- `tests/real.rs` の `elapsed()` の上限は6か所で、どれも 1 s の見張りである [V: grep と読み]。`tests/real.rs:509` に今も `unsafe` がある [V]。
- `docs/future/async.md:28` に compio の記述がある [V]。
- `natural-japanese` の skill はこの session の道具になかったので呼べなかった。文は docs の文体に合わせた。

## 8. 適合の自己点検

- G-1: 適合。`trust.rs` のテスト 4 が G-1 の grep と同じ字面で確かめる。既定の解決は `sys/` にある (S3)。
- G-2: 適合。核のテストの最初の4行。
- G-3: 適合。テスト 5 と S3 の `sockets_off_rejects_before_looking_up_the_handle`。
- G-4: 適合。`a_retry_delivered_twice_completes_once` と S2 の `a_refused_connect_completes_once`。
- G-5: 適合。`a_second_op_in_one_direction_is_invalid_input`。逸脱は計画に書く。
- G-6: 対象外 (S2/S3)。テストの置き場所 (c) を用意した。
- G-7: 適合。目標の表の `epoll_ctl` の3行。
- G-8: 適合。テスト 6 と many-self の比の行。
- G-9: 適合。S3 の `a_late_answer_of_a_cancelled_lookup_does_not_hit_the_reused_key`。
- G-10: 適合。S3 のプールの4件と P1。
- G-11: 適合。S3 の2件と今の `a_wake_*`。
- G-12: 適合。`a_failed_wait_breaks_the_driver`、`quiesce_breaks_on_a_cancel_that_never_completes`。
- G-13: 適合。テスト 3 (G の字面)、P1、abort の2件、`a_held_buffer_completed_within_the_limit_lets_drop_return`。
- G-14: 適合。`FakeSock` は Clone を持たない。G-14 の2件。
- G-15: 適合 (C1 と B-5 の範囲)。テスト 1、2、3。偽の backend も slice を作らない。
- G-16: 適合。テスト 4。
- G-17: 適合。Miri の2つ、arm の `test`、ASan (退路で減ったときは計画で承認)、`-j 1`、見張り付きの P1、Miri の形の規則。
- G-18: 適合。4c の grep と `.#cross` の clippy。
- G-19: 適合 (C2 の式)。Q5 の表をコミットに割り当て、各単位の基準に `citations::` を置いた。

出典 (Web): [GitHub runner-images ubuntu24-arm64/20260906.116](https://github.com/actions/runner-images/releases/tag/ubuntu24-arm64%2F20260906.116)、[partner-runner-images の arm-ubuntu-24-image.md](https://github.com/actions/partner-runner-images/blob/main/images/arm-ubuntu-24-image.md)

