# io-fastpath

- 作成日: 2026-10-29
- 基点: `claude/eager-archimedes-spdipw` @ `41b4f3d` (async-net を閉じた main `1cf4b1b` を取り込んだもの)。3 章と付録の事実と行番号はこのコミットで読んだ。行番号は探す手がかりにとどめ、関数、テスト、節の名前で引く
- 状態: 計画は確定。決めていない細部は 10 章に既定とともに置く

この計画は、段 `io-fastpath` の設計と計画である。終わった I/O の操作に待ちの費用を払わない形に、`IoDriver` の trait と、VM と std の I/O の約束を変える。本物の driver は `compio-driver` のまま新しい trait に合わせ、driver の差し替えは後の段 `io-backends` が行う。roadmap の「段の列」にはまだこの段がなく、T-0 が足す。roadmap の「進め方」に従い、この文書と付録のディレクトリ `docs/plans/2026-10-29-io-fastpath/` を段の最初のコミット (T-0) で入れ、段を閉じるときに消す。コードと `docs/` はこの文書を引かない。

付録は4つある。本体は決定、作業の順、完了の条件を持ち、作業の中身の細部は付録にある。付録と本体が食い違うときは本体に従う。

- 付録1 `01-fastpath.md`: trait の差分、`cells.rs` と `vm.rs` の変更、集める地点の方針、`settle`、`op_ready` と数え方、予算、貸し方と `MIN_ROOM`、書きの貸しと T10、即値の token、std と `eml_extern` の差分、テストの1本ずつの扱いと種類、spec の文案
- 付録2 `02-bench.md`: `eml run --stats` の形、`bench/net` の物差し、前と後を比べる規則と手順、基点の数字、`bench/net/` に置くファイルの全文
- 付録3 `03-io-backends.md`: 後の段 `io-backends` の設計。この段では実装しない
- 付録4 `04-net-windows.md`: 後の段 `net-windows` の設計。この段では実装しない

付録3と付録4は後の段の計画の元である。T-0 は、その要点を roadmap の `io-backends` と `net-windows` の項に書き写す。`io-backends` と `net-windows` の計画は、それぞれの段の最初のコミットで付録から作る。この段を閉じるときに付録は消えるが、git の履歴に残る。

## 1. 目的と成功の基準

- 目的
  - その場で終わった I/O の操作に、perform、park、再開、完了の収集を払わない。待つのは本当に保留の操作だけにする。
  - データのある読みを driver の操作1回にし、buffer を使い回す呼び手には確保も `Readable` もない道を作る。共有の `Bytes` の書きを写さず、足し続ける読みを線形にする。
  - 操作ごとの `Foreign` の token と、登録表の鍵の `HashMap` と、driver の 10 ms の刻みをなくす。
  - `IoDriver` の trait の変更をこの段で1回だけ行い、後の段 `io-backends` が VM と std に触れずに driver だけを差し替えられる形にする。
  - 本物の I/O の物差し (`eml run --stats` と `bench/net`) を入れ、この段の前と後を同じ機械で測る。
  - 後の段 `io-backends`、`net-windows`、`io-uring` を roadmap に置く。
- 成功の基準 (確かめ方は 9 章)
  - 9 章の全体の検証のコマンドがすべて通る。
  - `crates/eml_vm/tests/io.rs` の `one_task_of_finished_ops_never_waits_on_the_driver` が、`immediate == 5`、driver の `wait` の呼び出し 0、`parks == 5`、`completions == 5` で通る。
  - `bench/net/bulk.em 64 64 fresh accum` が release の eml で 1 s 以内に終わる (基点は 22.8 s)。
  - echo-py-reuse の往復あたりの `allocations` が、T-2b の1点目 (基点で 40.02) より下がる。
  - `grep -rn IDLE_TICK crates docs CLAUDE.md | grep -v docs/plans`、`grep -n "HashMap<OpKey" crates/eml_vm/src`、`grep -n "OpToken\|OpDropped" crates/eml_vm/src` が何も返さない。
  - docs/implementation/benchmarks.md の「ネットワークの計測」に、1点目と2点目の2列の表がある。

## 2. 範囲

- 含むもの
  - `IoDriver` の trait の3つの変更 (呼ぶ側の鍵と `Submit`、1つの読みと状態の問いの `Readable`、刻みのない `wait`) と、compio の driver とテストの driver の追随 (D-2、D-17、D-18)
  - 終わった操作を待たない速い道、`op_ready`、操作の予算、`RunStats` の `parks` と `completions` の意味 (D-3、D-12)
  - 鍵を記録の番号にすること、`settle`、即値の token (D-6、D-11)
  - 空きのある buffer への1回の読み、`MIN_ROOM`、幾何の伸び、`read_line` の写し1回、`Bytes.with_capacity` (D-4、D-13、D-19)
  - `write_bytes` が buffer を返すこと、書きの貸しに一意を求めず写しを 0 にすること (D-4、D-5、D-14)
  - 空きのループの 10 ms の上限を VM に置くこと、監視のスレッドを run の始めに起こすこと、集める地点が OS を見る頻度 (D-7、D-15、D-16)
  - 1操作あたりの確保を減らすこと (D-21)
  - `eml run --stats`、`bench/net`、1点目と2点目の計測 (D-1、D-20)
  - roadmap に後の段を置き、付録3と付録4の要点を書き写すこと (T-0)
- 含まないもの
  - `compio-driver` を外し、epoll、kqueue、IOCP を自前で書くこと。後の段 `io-backends` (付録3)
  - Windows のソケット。後の段 `net-windows` (付録4)。この段の間、Windows の本物の driver はソケットを扱わず、UI テストはテストの driver で流れる
  - io_uring。後の段 `io-uring` (試作と計測が最初のタスク)
  - 別のスレッドから受け付けを出す API (`async-threads`)、非同期のファイル (`async-files`)
  - ネイティブのコードの入り口 (`Ctx.worker` の欄は `jit-core`、`worker` から `&mut Worker` を作る Miri のテストは `jit-effects`)
  - 貸す量の上限 65 536 バイトを外すこと。測定の根拠がない (物差しの bulk-recv-py の件で、`recvfrom` 1回は平均 53 KiB だった)

## 3. 背景

基点 41b4f3d で読んだ事実である。

- `IoDriver` (`crates/eml_rt/src/driver.rs`、108 行) のメソッドは `now`、`wait`、`submit`、`cancel`、`completions`、`pending`、`waker`、`bin`、`shutdown_write`、`set_nodelay`、`close`、`quiesce` の 12 である。`submit(&mut self, op: Op) -> OpKey` で鍵は driver が振る。`Op` は `Readable`、`Read`、`Write`、`Listen`、`Accept`、`Connect`、`Job`。`RawBuf` の doc は「貸した側はその操作の完了を `completions` から取り出すまで、物体を生かし、一意に保ち、動かさない」と書く。`submit` の doc は「プールのスレッドがすべて仕事中なら、仕事の `submit` はスレッドが空くまで、安全点を通らずに呼んだスレッドの上で待つ」、`cancel` の doc は「届かないことがあれば、driver は `quiesce` で壊れた状態になる」と書く。`driver.rs` は `verus!` の外にあり、`trust.rs` の `TRUSTED` に行はない (`FILES` には載る)。
- `IoDriver` の実装は6つ (`RealDriver`、`TestDriver`、`cells.rs` の `NoDriver`、`tests/io.rs` の `HoldingDriver`、`tests/machine.rs` の `FakeDriver` と `StubDriver`) である。`eml_rt` の中で `IoDriver` を使うのは `driver.rs` と `lib.rs` だけである。
- 本物の driver は `crates/eml_io/src/real.rs` (866 行) で、`compio-driver` 0.12.5 の `Proactor` の上にある。
  - 保留の操作は `ops: HashMap<OpKey, Entry>` に compio の `Key` として持つ。`completions` は `poll(ZERO)` の後に `reap` が保留をすべて `pop` して確かめ、O(保留の数) かかる。
  - `submit` は compio の `push` の中で終わった操作 (`Step::Done`) を `done` に積む (real.rs:246、259、281-298、350-357)。`Recv::pre_submit` は先に `recv` を試すので、データのある `Read` は `push` の中で終わる。`PollOnce` (`Readable`) は poll の backend では `push` の中で終わらない。
  - `wait` は1回の待ちを `IDLE_TICK` (10 ms) で切る。`cancel` は `ops` にある鍵だけに `register_cancel` する (real.rs:697-723)。
- テストの driver (`crates/eml_io/src/test.rs`) は、`wait` と `completions` の始めに決着させる。`wait(Some(d))` は時計を `max(now, d)` に進めて `Ok(true)` を返す (test.rs:223-236)。`cancel` は保留の列だけを見る (test.rs:249-255)。
- VM の登録表 (`crates/eml_vm/src/cells.rs`) は、driver の鍵から記録への `ops: HashMap<OpKey, CellRef>` を持つ。`Registry::record` は `Done` の記録を `ops` に入れず、`park_op` は `Done` の記録を即座に `ResumeIo` で返す (cells.rs:2065-2083、2148-2156)。`submit_*` は `worker` を引数に取り、記録に書く (cells.rs:1893-1925、1937-1967)。`unique_bytes` は一意でない `Bytes` を `to_vec` と `alloc_bytes` の2回写す (cells.rs:2711-2722)。
- `OpId` は Kind `Lin`、Repr `Obj` で、中身は `Foreign` の `OpToken` である (eml_extern の lib.rs:253、cells.rs:482-496)。token の `Drop` は受け箱に知らせを積む。core-ir.md の87行付近は「`Lin` と宣言したフィールドのない型の値も、Repr は `enum` で印は `unr` である。値を1回だけ使うことは型検査が守る」と書く。
- `Net.read_chunk` は `Readable` の後に `Read` の2つの driver の操作になる。`Readable` の完了で空きが 65 536 バイトより少なければ、`len + 65 536` の容量の `Bytes` を作って写し、65 536 バイトを貸す。待ちごとに park と再開の2回の切り替えがある。`read_line` はストリームの `Vec` に 64 KiB の0の配列 (`ZERO_ROOM`) を写してから貸し、行を `Vec` から写した後に `String` へもう1回写す。
- 操作の予算はタスクごとに 128 で、`check_cancelled` が `spend_budget` で使う (cells.rs:1521-1524、Async.em:139-145)。再開で 128 に戻る (cells.rs:1003-1015)。
- `RunStats` は検査モードでだけ数え、数をプログラムだけで決める方針である (vm.md の「`RunStats`」、cells.rs:2139-2140)。
- 集める地点 (`collect`) は、安全点の `NOTICE`、`switch_away` の頭と空きのループ、`preempt` の頭、横取りの判定の中にある (vm.rs:418、1264、1287、1433、1448)。空きのループは `listening()` のとき期限を `u64::MAX` にする (vm.rs:1273-1284)。
- 監視のスレッド (`eml-monitor`) は、シグナルの中継の後と止める語が立ったときに driver の waker を鳴らす (monitor.rs:120)。横取りの方針のタスクを足すときと、本物の driver で OS のシグナルを待つときにだけ起きる (sched.rs:368-371、vm.rs:2060)。CLI はいつも自己パイプ (`Doorbell`) を渡す (eml_cli の main.rs:90-101)。自己パイプのない監視のスレッドは刻みごとに起きる (vm.md の「タスクとスケジュール」)。
- `bench/` に I/O のプログラムはなく、`bench/` のどのプログラムも `Async` と `Net` を使わない。`bench.rs` の `every_program_in_bench_has_a_test` は `bench/` の直下の `.em` だけを数える。CLI に時計を選ぶ flag はない (main.rs に `clock` の引数がない) が、CLAUDE.md の Commands は `--clock real|virtual` を書いている。
- 基点の物差しの数字 (付録2 の7節。Linux 6.18 の container、Xeon 2.1 GHz、4 vCPU):
  - 待つ `read_chunk` は syscall が 8 回、両端が同じ run なら空振りの poll で 13 回。データのある `read_chunk` も 9 回。compio の上の1回の poll は、待ちの長さ 0 でも5回の syscall になる。
  - echo-py の1往復は、eml の CPU が約 37 µs、確保が 42 個、命令が約 11 万で、`collect`、`submit`、`poll`、malloc/free に散る。`parks` は待ちの数と同じ 2、`completions` は 3 である。
  - 保留が多いと compio の上の `reap` が命令の多くを占める (前の版の many.em の測りで 85%、今の many.em で 1000 本 78.8%、2000 本 90.7%)。many-self の1接続あたりの CPU は 250 本で 165 µs、2000 本で 488 µs で、比は 2.96 である。
  - 共有の `Bytes` の2回の写しは bulk-shared の命令の 47.9% を占める。1つの buffer に足し続ける読みは、4、16、64 MiB で 0.11、1.16、22.8 s と2乗で伸びる。
  - G-23 の目標と基点の数の対応は付録2 の7.2節にある。
- 文書のうち、このブランチにだけあって main にない箇条がある (d2aa182 と f50e607)。roadmap の「その後の項目」の「完了を集める手間」(中の文は消えた `Std.IoTest` と閉じた async-net に触れていて古い)、async.md の「未決の論点」の「I/O の driver の crate」の下の2つの箇条 (「自前の driver に替えるもう1つの条件」「替えるときの順」)、async.md の「ファイルとパイプ」の macOS の通常のファイルの箇条である。前回の計画書 `docs/plans/2026-10-29-io-backends.md` と付録 (`01-core-unix.md`、`02-iocp.md`) も木にあり、T-0 が消す。
- テストの数: `crates/eml_io/tests/real.rs` は 34 本、`tests/test_driver.rs` は 21 本、`src/real.rs` に単体テストが1本、`crates/eml_vm/tests/io.rs` は 11 本、`api.rs` は 27 本、`signal.rs` は 7 本。`Net` を使う UI テストは 15 ファイル (`tests/ui/run/async/net_*` の 13 と `run-fail/async/` の2)。`Net.write_bytes` を使う UI テストと REPL のトランスクリプトはない。

## 4. 前提の確認

| 要るもの | 用途 | 確かめ方 | ないとき |
|---|---|---|---|
| async-net が閉じていること | すべて | `git log --oneline \| grep 'Close async-net'` → 1行 | 止めて利用者に聞く。着手しない |
| 基点の trait の形 | T-4 | `git grep -n 'fn submit(&mut self, op: Op) -> OpKey' crates/eml_rt/src/driver.rs` → 1行 | 止めて報告する |
| このブランチにだけある文書の箇条 | T-0 | `git grep -n '完了を集める手間' -- docs/future/roadmap.md` → 1行。`git grep -n '自前の driver に替えるもう1つの条件' -- docs/future/async.md` → 1行 | 無い箇条の書き換えを飛ばし、コミットのメッセージに書く |
| nix の devShell | すべて | `nix develop -c cargo --version` → `cargo 1.99.1` | worktree で止まるなら `nix develop "path:$PWD" -c cargo --version`。それでも止まれば利用者に聞く |
| Verus | T-4、T-6 (`eml_rt` を変える) | `nix develop -c verus --version` → 版が出る | 止めて利用者に聞く |
| Miri の shell | T-8 | `nix develop .#miri -c cargo miri --version` → 版が出る | 止めて利用者に聞く |
| `.#cross` の shell | T-4、T-8 (`crates/eml_io` と Windows の分岐) | `nix develop .#cross -c cargo clippy --version` → 版が出る | 止めて利用者に聞く |
| `python3` と `strace` | T-2、T-2b、T-7、T-9、T-10 | `python3 --version` と `strace -V` → 版が出る | `nix shell nixpkgs#strace` で入れる。入らなければ T-2b と T-10 の syscall の表を空けずに止めて報告する |
| 同じ形の Linux の機械 (負荷のない container) | T-2b と T-10 の計測 | T-2b で機械と kernel を計画書に書き、T-10 で `uname -r` と `nproc` が同じこと | T-10 を閉じずに止めて利用者に報告する |
| macOS の機械 | T-11 (`bench/run.sh`、`bench/rt.sh`、`bench/scripts.sh`) | `uname` → `Darwin` | 利用者が流す。流せなければ T-11 を置かず、その旨を利用者に伝える |
| GitHub の CI (macOS、Windows) | すべて | 段の PR で CI の `test` が3行流れる | 段を閉じる前に利用者に聞く |

## 5. 決定

### 5.1 長く効く方針 (変えるときは見直しが要る)

実装がこの節の方針とぶつかったら、止めて報告する。各決定の「守る規則」は、設計の検討で決めた規則 (G-1 から G-25) の最終の文である。io-backends 以後に効く規則も、後の段が同じ文で引けるようにここに置く。

#### D-1: 段の分け方と移し方、3点の計測
- 決定
  - 段は `io-fastpath` (前提: なし)、`io-backends` (前提: `io-fastpath`)、`net-windows` (前提: `io-backends`)、`io-uring` (前提: `io-backends`) の順に進める。閉じた段の名前 `io-driver` と `async-net` は使わない。
  - `IoDriver` の trait の変更はこの段の T-4 で1回だけ行い、`io-backends` 以後は変えない。`io-backends` は VM、std、`eml_extern` のコードに触れず、driver だけを差し替える。
  - `io-uring` の最初のタスクは試作と計測で、`bench/net` の many-self-1000、bulk-1k、echo-py で epoll と比べ、user+sys の CPU が2割以上縮む件がなければ試作を捨てて段を閉じる。
  - この段と `io-backends` を閉じる前に、`bench/net` を同じ機械で流し、3点 (compio の前 (この段の始め)、この段の後、`io-backends` の後) の数字を benchmarks.md に書く。
- 理由
  - この段を先にすると、約束の変更の効果を compio の上で測れ、`io-backends` は VM に触れない純粋な差し替えになる。差し替えのコミットで動くのは driver のテストだけになり、3点の計測が約束の効果と driver の効果を分ける。
  - compio を新しい trait に合わせる作業は `real.rs` の数十行で、使い捨てでも安い。`real.rs` の `Step::Done` の道は `submit` の中にすでにあり、`done` に積む代わりに返すだけである。
  - io_uring の利点は速い道の後に残る待ちの道の束ねで、その大きさは測っていない。Apple Silicon の Mac の上の Linux コンテナでは既定の seccomp が io_uring を塞ぐので、epoll の道はどのみち要る。試作と計測を段の最初のタスクにすれば、「測れないので始まらない」ことはない。
- 退けた案
  - `io-backends` を先にする: VM の変更が driver の差し替えの段に混ざり、原因を切り分けられない。
  - `io-uring` を条件付きの保留にする: 条件を測る主体がない。
  - compio と自前の driver を同じ trait の後ろで切り替える期間を持つ: テストの組と docs と契約が2つになる。
- 影響の範囲: roadmap の段の列、各段のコミットの列、`bench/net`、benchmarks.md、testing.md。
- 見直す条件: 段を閉じれば消える決定。trait をもう一度変える必要が `io-backends` の設計で出たときは、止めて利用者に聞く。
- 守る規則
  - G-14 MUST: 段は `io-fastpath`、`io-backends`、`net-windows`、`io-uring` の順で、名前は roadmap と `git log` の `Close` の名前と重ねない。`IoDriver` の trait の変更は `io-fastpath` で1回だけ行い、`io-backends` 以後は変えない。`io-backends` は `crates/eml_vm`、`std/`、`crates/eml_extern` のコードを変えない (docs とコメントの compio の文を除く)。compio は `io-backends` の中の、`RealDriver` を差し替えるコミットの直後のコミットで消し、その間にほかの作業を入れない。同じ trait の後ろで compio と自前の driver を切り替える feature、型、環境変数を作らない。`io-backends` の間、`crates/eml_vm/src` には触れない。driver の振る舞いを述べた `eml_vm` のコメントは `io-fastpath` で中立の文に直しておく。
  - G-15 MUST: compio への言及は、`io-backends` を閉じるときに `docs/`、コード、コメント、CLAUDE.md、README.md のどこにも残さない。spec の書き換えは規則を実装するコミットと同じコミットで行う。
  - G-25 MUST: `io-fastpath` と `io-backends` を閉じる前に、`bench/net` を同じ機械で流し、3点 (compio の前、`io-fastpath` の後、`io-backends` の後) の user+sys、壁、syscall の数、RSS、`RunStats.allocations` を benchmarks.md に書く。主指標は user+sys で、壁は参考にする。CI では流さない。通常のテストに I/O の時間の assert を入れない (G-4 の 1 ms は driver の単体の上限で、I/O を待たない)。
- 適合の確かめ
  - この段: `git log -p --format= -- crates/eml_rt/src/driver.rs` で、この段のコミットのうち `driver.rs` に触れるのが T-4 だけ。
  - この段: `grep -rn "Instant::now\|elapsed" crates/eml_vm/tests/io.rs` → 0行。benchmarks.md の「ネットワークの計測」に1点目と2点目の列がある (T-10)。
  - `io-backends`: `git log -p -- crates/eml_rt/src/driver.rs` に `io-backends` 以後の変更がない。`git log --stat` で `io-backends` のコミットが `crates/eml_vm/src`、`std/`、`crates/eml_extern/src` に触れない。3列目の表 (付録3)。
- リポジトリに残す所: roadmap の段の列 (T-0)。3点の表と比べる規則は benchmarks.md の「ネットワークの計測」(T-2 が節と規則、T-10 が表)。物差しの置き場所と CI で流さないことは testing.md の「よく使うコマンド」と「性能のテスト」(T-2)。

#### D-2: `IoDriver` の契約
- 決定
  - 鍵は呼ぶ側が振る。`submit(key, op)` は `Submit::Done(io::Result<Done>)` か `Submit::Pending` を返す。`Done` の操作の完了は後から届かず、`Pending` の操作の完了はちょうど1回届く。
  - `wait` は完了、期限、起こしでだけ戻り、内部の刻みを持たない。`Ok(false)` を返すのは、期限が `None` で、保留も届いた完了もないときだけである。`Some(u64::MAX)` は期限なしで待つが `Ok(false)` を返さず、起こしで `Ok(true)` を返す。
  - OS を見るのは `wait` だけで、`wait` は期限が今以前でも OS を1回確かめてから戻る。`completions` は届いた完了を移すだけで、OS を見ず、保留の表を歩かない。完了は並べない。
  - `cancel` は呼んだ時点で保留の鍵にだけ効き、後で同じ鍵で出した操作に効かない。
  - trait の doc には io-backends 以後も成り立つ文だけを書く (付録1 の1節)。
- 理由
  - 鍵を VM が振れば、VM の側のハッシュが 0 回になり、完了の当て先が添字で O(1) になる。
  - `Done` を返せれば、終わっている操作に2回の切り替えと継続の捕まえを払わない (D-3)。
  - 刻みをなくすと空の起きが消え、Ctrl-C と止める語を届ける道が waker の1本になる (D-7)。
  - `completions` が OS を見ると、`switch_away` の頭の集める地点と直後の `wait` で OS を2回見る。OS を見る頻度の方針を VM に置けば、driver の契約は1つで、将来の `Worker` やネイティブの呼び手も同じ契約で呼べる。
- 退けた案
  - 鍵を driver が振る (今): VM の側に鍵から記録への表が要る。
  - `completions` が 16 回に1回だけ OS を見る (driver の中の頻度): VM の呼び方に driver を結合し、計算の重い worker では完了が最大 16 刻み (約 160 ms) 遅れる。
  - `wait(Some(u64::MAX))` を期限なしと同じに扱い、保留がなければ `Ok(false)` を返す: シグナルを待つタスクだけの run がデッドロックの誤りで止まる。
- 影響の範囲: `crates/eml_rt/src/driver.rs`、すべての `IoDriver` の実装、`cells.rs` の記録。
- 見直す条件: なし (trait は凍る)。
- 守る規則
  - G-4 MUST: `IoDriver::completions` は届いた完了を移すだけで、OS を見ず、保留の表を歩かない。OS を見るのは `wait` だけで、`wait` は期限が今以前でも OS を1回確かめてから戻る。backend の `wait` は fd から鍵を引く自分の表で鍵を得て、核の表を鍵で引く。完了は並べない (VM は鍵で記録を引く)。対象は `IoDriver::completions`、`IoDriver::wait`、`Backend::wait` で、`quiesce`、`Drop`、`pending` は対象外。
    - 読み: この段の compio の `wait` は `reap` で保留の表を歩く (基点で `completions` が歩いていた分が `wait` に移るだけ)。「`wait` は表を歩かない」は自前の driver の約束で、確かめは `io-backends` から行う。この段の確かめは `completions` が OS を見ないことだけである。compio の `reap` は `io-backends` で消える。
  - G-5 MUST: `submit` が `Pending` を返した操作はちょうど1回 `Completion` になり、`Done` を返した操作は `Completion` にならない。readiness の backend は取り消しの完了をその場で表から作り、完了の backend は OS の取り消しの完了を待つ。`quiesce` に時間の上限を置いて壊れた状態にしてはならない。debug のビルドでは、`quiesce` が 5 秒たっても I/O の操作を残せば、残った鍵を文言に入れて panic する。`Drop` は panic せず、debug では 5 秒で1回 `eprintln!` し、待ち続ける。`broken` にするのは、`wait` の OS の呼び出しそのものが失敗したときと、取り消しの要求 (`CancelIoEx`) が「取り消すものがない」以外の誤りで失敗したときだけである。後者は backend が誤りを覚え、次の `wait` が OS を待たずに `Err` で返す。プールの仕事は取り消せないので対象外。
  - G-6 MUST: waker は `Arc` の中に atomic の旗と OS の物 (Linux は eventfd、ほかの unix はパイプの読む端と書く端の両方、Windows は完了ポートのハンドル) を所有し、backend は同じ `Arc` を持って読む。`wake` は確保も lock もせず、誤りを捨てる。`Backend::wait` は起こしと時間切れを区別して返す。本物の driver の `wait` は、完了、期限、起こしのどれかでだけ戻り、内部の刻み (`IDLE_TICK`) を持たない。driver を落とした後の `wake` は害がない。シグナルの handler から呼ばない。
  - G-17 MUST: `OpKey` は VM が振り、値は操作の記録の `CellRef` (`(generation << 32) | index`) である。登録表は鍵から記録への写像を持たず、完了の鍵を添字で引いて generation で確かめ、合わなければ内部の誤り `a completion names no op` にする。1つの記録は受け付けから取り出しまで1つの鍵を持つ。Rust のテストが driver を直に使うときは任意の鍵でよい。
- 適合の確かめ
  - この段 (T-4): `grep -n "HashMap<OpKey" crates/eml_vm/src` → 0行。`grep -n "next_key" crates/eml_io/src` → 0行。`real.rs` の `fn completions` の本体に `poll` と `reap` がない。`grep -rn "IDLE_TICK" crates docs CLAUDE.md | grep -v docs/plans` → 0行。
  - この段のテスト: `test_driver.rs` の `completions_do_not_settle`、`an_op_decided_at_submit_returns_done_and_leaves_no_completion`、`a_cancel_after_completion_does_not_touch_the_next_op_of_the_key`、`a_pending_key_submitted_twice_panics_in_debug`。`tests/real.rs` の `a_read_after_a_finished_probe_returns_done`、`a_wake_ends_a_wait_with_a_far_deadline`、`a_wait_with_no_ops_and_a_far_deadline_wakes`、`a_wait_without_ops_sleeps_until_its_deadline`。`cells.rs` の `a_completion_with_a_stale_generation_names_no_op`。`signal.rs` の `a_waiter_keeps_an_otherwise_idle_run_alive` (`Some(u64::MAX)` の道)。
  - `io-backends` (付録3): 核の `completions_never_look_at_the_backend`、`completions_and_wait_do_not_walk_the_table` (`walks == 0`)、`a_wait_with_no_ops_and_a_far_deadline_wakes`。`tests/real.rs` の `a_wait_with_two_hundred_pending_ops_returns_only_the_finished_one` (保留 200 のうち1つだけ終わったときに `wait(Some(now))` が 1 ms 以内に戻り、`completions` がその1つだけを返す)。`grep -n "CANCEL_LIMIT\|IDLE_TICK" crates/eml_io/src` → 0行。
- リポジトリに残す所: `crates/eml_rt/src/driver.rs` の doc (T-4)。vm.md の「時計と driver」の trait の項、`submit`、`cancel`、`wait` の箇条 (T-4)。

#### D-3: 終わった操作を待たない速い道と `RunStats`
- 決定
  - `IoDriver::submit` が `Submit::Done` を返した操作の記録は、`Done` の段階で生まれる。std の `io_wait` と `io_wait_until` は、記録が終わっていて取り消しの地点が投げないときは perform しない。予算を1つ使い、切れていれば `reschedule` してから (その `Wake` は見ない) `(Woke, op)` を返す。
  - 保留の操作の待ち (継続を預けて戻り、エントリで再開する) は変えない。ネイティブのコードと同じ形だからである。
  - `RunStats` の `parks` は `io_wait` と `io_wait_until` の呼び出しの数、`completions` は終わった driver の操作の数 (受け付けで終わった操作と集めた完了の和) にする。スケジュールで変わる数 (その場で終わった操作の数、集めた完了の数、切り替えの数) は `RunStats` に入れない。速い道は、テストの driver の `immediate()` と、Rust のテストの driver が持つ `wait` と `completions` の呼び出しの数と、本物の driver の計測で確かめる。
- 理由
  - 待ちのたびに park と再開の2回の切り替えがあり、書きは `sendto` 1回で終わるのに park と再開を払っている (echo-py は往復で `parks` 2)。
  - 速い道を VM の命令でなく eml の `if` にすると、取り消しの地点と予算の規則を std の1か所で書ける。
  - その場で終わるかはスケジュールで変わるので、`parks` と `completions` の意味を「頼んだ待ちの数」「終わった操作の数」にそろえれば seed によらない。driver を使う run では、VM の仕事の回数 (`evidence_steps`、`yields`、`rc_*`、`allocations`、`boxes` など) と `peak_stack` はスケジュールに従う。終わった操作の待ちは perform も切り替えもしないためである。スケジュールによらないのは、`eml_test_support::driver_independent` が比べる欄 (`tasks`、`parks`、`timers`、`dropped_failures`、`shared_objects`、`channels_swept`、`completions`) である。
  - 速い道は切り替えを 0 にするので、`Sequential` では走れる他のタスクが待たされうる。操作の予算は、同じ理由で `check_cancelled` に入っている仕組みである。
  - 予算は、操作が終わっていたかによらず `io_wait` と `io_wait_until` の1回ごとに1つ使う。どの待ちも `parks` を1つだけ数える。終わった操作の待ちが予算を使い切ったときは、`op_ready` は数えずに 2 を返し、続く `reschedule` が数える。予算を使い切った待ちは譲り (終わっていない操作は park して、終わった操作は `reschedule` で)、その再開で予算を戻す。ほかに予算を戻すのは、始まるときと、操作の待ちでない再開のときだけである。操作の待ちが park するかはスケジュールで変わるので、終わった操作の待ちだけで予算を使ったり、park した待ちの再開で予算を戻したりすると、予算の切れる位置と、譲りが数える `parks` がスケジュールに依る。
- 退けた案
  - `RunStats` にその場で終わった数の欄を足す: スケジュールで変わる数を新しい欄として `RunStats` に足さない方針 (G-18) に反する。
  - 予算を使わず、`Normal` の横取りだけに任せる: `Sequential` で走れる他のタスクが飢える。
  - 速い道を VM の命令にする、`await_io` の節で `k` を直接呼ぶ: 取り消しの地点と予算の規則が VM と std に割れる。
- 影響の範囲: `Async.io_wait`、`io_wait_until`、`op_ready`、`Registry::record`、`submit_*`、両方の driver、`RunStats`。`park_io` の取り消しの道は変えない。
- 見直す条件: ネイティブのコードの `Async` の入り口を作るとき (`jit-effects`)。
- 守る規則
  - G-18 MUST: `IoDriver::submit` が `Submit::Done` を返した操作の記録は `Done` の段階で生まれ、std の `io_wait` と `io_wait_until` は、記録が終わっていて取り消しの地点が投げないときは perform せず、予算を1つ使い、切れていれば `reschedule` (その `Wake` は見ない) してから `(Woke, op)` を返す。perform と完了の収集は `Pending` の操作にだけ起き、park と再開は `Pending` の操作と、予算を使い切った終わった操作の待ち (`reschedule` で譲る) にだけ起きる。予算は、操作が終わっていたかによらず `io_wait` と `io_wait_until` の1回ごとに1つ使う。どの待ちも `parks` を1つだけ数える。終わった操作の待ちが予算を使い切ったときは、`op_ready` は数えずに 2 を返し、続く `reschedule` が数える。予算を使い切った待ちは譲り (終わっていない操作は park して、終わった操作は `reschedule` で)、その再開で予算を戻す。ほかに予算を戻すのは、始まるときと、操作の待ちでない再開のときだけである。本物の driver とテストの driver は、受け付けの時点で決められる操作を `Done` で返す。`RunStats` の `parks` は `io_wait`/`io_wait_until` の呼び出しの数、`completions` は終わった driver の操作の数 (`Done` と集めた完了の和) で、どちらもスケジュールに依らない。スケジュールで変わる数を新しい欄として `RunStats` に足さない。driver を使う run では VM の仕事の回数と `peak_stack` がスケジュールに従い、`driver_independent` の欄は従わない。
- 適合の確かめ (この段)
  - (1) `io.rs` の `one_task_of_finished_ops_never_waits_on_the_driver`: 1つのタスクが `listen`、`connect`、`accept`、`write_bytes`、`read_chunk` を順に行い、`TestDriver::immediate() == 5`、driver の `wait` の呼び出しが 0 回、`RunStats.parks == 5`、`completions == 5` (T-5 で中間の値、T-8 で最終の値)。
  - (2) `io.rs` の `echo_counts_do_not_depend_on_the_seed`: 2つのタスクの echo を seed 1、2、3 で流して `parks` と `completions` が同じ。
  - (3) `api.rs` の `#[cfg(unix)]` `a_real_read_after_a_finished_probe_does_not_wait`: 本物の driver で、`Readable` が終わった後の `Read` が `Done` で、その後に `wait` がない (driver を包んで記録する)。
  - (4) `io.rs` の `a_fast_path_loop_yields_to_another_task_under_sequential`: 速い道を 300 回続けるタスクともう1つのタスクを `Sequential` で流し、もう1つのタスクが 300 回の終わりより前に動く。
  - (5) `grep -n "immediate" crates/eml_vm/src/lib.rs` → 0行 (`RunStats` に欄がない)。
- リポジトリに残す所: vm.md の「操作の記録」と「`RunStats`」、stdlib.md の「`Std.Async`」「`Std.Net`」「操作の予算」、testing.md の「seed で流す」(T-5)。

#### D-4: 1つの読みと、`Bytes` の貸し方と `Std.Net` の API
- 決定
  - `Op::Read` は「今すぐ読みを試し、ブロックするなら読めるまで待ってから1回読む」1つの操作で、`Op::Readable` は「次の読みがブロックしない状態か」の問いである。
  - VM は、buffer が一意で空きが 8 バイト以上あれば、空きのうち 65 536 バイトまでを `Read` で貸す。そうでなければ `Readable` を出し、読めると分かってから広げて `Read` を出す。広げる容量は `長さ + max(長さ, 65 536)` で、貸すのは 65 536 バイトである。`read_line` のストリームの `Vec` にも同じ規則 (8 バイトの下限を含む) を当てる。
  - `write_bytes : Bytes -> TcpStream -> <IO, Async> (Bytes, TcpStream)` にし、渡した `Bytes` をそのまま返す。`write` と `write_line` の型は今のまま。
  - `Bytes.with_capacity : Int -> <Panic> Bytes` を足す。
  - `*_within`、`accept`、`connect`、`listen`、`shutdown_write`、`set_nodelay`、`local_port`、`peer_addr` の API は変えない。driver が buffer を持つ形は採らない。
- 理由
  - データのある読みを syscall 1回にし、buffer を使い回せば確保も `Readable` もなくす。足し続ける読みを線形にする。
  - 下限 8 バイトは、確保器が長さを語に切り上げた余り (0〜7 バイト) に読む量を決めさせないためである。下限がないと、`Bytes.of_string "> "` (空き 6) で読む UI テストの出力と syscall の数が変わる。
  - 何も来ない接続は、呼び手が空きを渡さない限り記憶を持たない。`Readable` が状態どおりに答えることでこの性質を保つ。
  - echo の形で buffer を使い回せないと、読みごとに 64 KiB の確保か `Readable` の2段が残る。`Readable` の道は syscall が増えるので、速い道の主な利益がそこで消える。
  - 貸す量の上限 65 536 バイトを保つのは、上限を外す測定の根拠がなく、`read_chunk` の doc の「65 536 バイトまで」を保てるためである。
- 退けた案
  - 貸す量の上限を外す: 測定の根拠がない。
  - 空きが少なければ受け付けの時点で広げて `Read` を出す: 何も来ない接続が 64 KiB を持つ。
  - `write_bytes` の型を変えずに `write_bytes_keep` を足す: API が2つになり、echo の典型の形が遅いほうに残る。
  - `Readable` を記憶だけで答える: 読み尽くした後の `Readable` が `Done` になり、状態の問いの契約に反する。
- 影響の範囲: `read_chunk`、`read_chunk_within`、`read_line`、`write_bytes`、両方の driver の `Readable`、`std/Bytes.em`、`eml_rt` の `bytes_with_capacity`。
- 見直す条件: `bench/net` の計測で、貸す量の上限か下限が目立ったとき。
- 守る規則
  - G-19 MUST: `Op::Read` は「今すぐ読みを試し、ブロックするなら読めるまで待ってから1回読む」1つの操作で、`Op::Readable` は「次の読みがブロックしない状態か」の問いである (記憶だけで答えず、読み尽くした後は `Pending`)。VM は buffer が一意で空きが 8 バイト以上あれば、空きのうち 65 536 バイトまでを `Read` で貸し、そうでなければ `Readable` を出して、読めると分かってから広げて `Read` を出す。広げる容量は `長さ + max(長さ, 65 536)` で、貸すのは 65 536 バイトである。`Readable` が `Submit::Done` なら `submit_read` の中で続ける。何も来ない接続は、呼び手が空きを渡さない限り記憶を持たない。対象は `read_chunk`、`read_chunk_within`、`read_line` (ストリームの `Vec` に同じ規則)、両方の driver の `Readable`。
- 適合の確かめ
  - この段: `io.rs` の `a_read_into_room_does_not_probe`、`a_read_into_word_slack_probes_first` (`Bytes.of_string "> "` の読みが `Readable` の後に `Read` で、貸す大きさが 65 536)、`a_read_into_empty_bytes_probes_first`、`a_probe_done_at_submit_reads_at_once`、`write_bytes_returns_the_same_buffer`。`cells.rs` の `growing_a_full_buffer_doubles_its_length`。`timeout 60 ./target/release/eml run bench/net/bulk.em 64 64 fresh accum` が 1 s 以内 (T-7)。
  - `io-backends`: `sys::tests` の `a_probe_after_a_drain_waits` (読み尽くした後の `Readable` が `Pending`)。strace で、データのある `read_chunk` が Linux で `recvfrom` 1回。
- リポジトリに残す所: vm.md の「操作の記録」と「`RunStats`」、stdlib.md の「`Std.Net`」と「`Bytes`」、object-model.md の「バイト列の操作」(T-6、T-7、T-8)。async.md の「所有ごと渡す buffer」(T-8)。

#### D-5: 貸した記憶の約束 (書きの貸しに一意を求めない)
- 決定
  - `Read` に貸す記憶は一意に保つ (カーネルが書く)。`Write` に貸す記憶は、生かし、書き換えないだけで、一意を求めない。共有の `Bytes` も写さずに書きに貸す。
  - `RawBuf` の約束は「`submit` が `Submit::Done` を返すか、その操作の完了を `completions` から取り出すまで、物体を生かし、動かさない。`Read` に貸した記憶は一意に保ち、`Write` に貸した記憶は書き換えない」にする。
- 理由
  - `Bytes` をその場で書き換える道 (`set`、`push`、`append`、`append_slice`、`truncate`、`bytes_lend`/`bytes_commit`) は、どれも `is_unique` (rc が 1) を要する。`is_unique` は共有、不死、張り付いた数をみな偽にする。書きの記録が参照を1つ持つ間、ほかの持ち手から見た rc は 2 以上 (共有なら負) なので、どの道も写しになる。記録の参照は `take_bytes` まで放さない。
  - ヒープは物体を動かさず、動かす道は REPL の回復の写しだけで、回復は `quiesce` の後なので保留の書きがない。
  - 共有の `Bytes` の2回の写しは bulk-shared の命令の 47.9% だった。
- 退けた案
  - 写しを1回にして元の共有の `Bytes` を返す: 記録に欄が1つ増え、写しも残る。
  - 検査モードで貸し出し中の印を足す: 書き換えはどれも rc 1 を求め、rc が 2 以上の間は写すので、印が守るものがない。
- 影響の範囲: `cells.rs` の `submit_write`、`eml_rt` の `RawBuf` の doc、object-model.md の T10 の行、`real.rs` と `test.rs` の SAFETY、io-backends の `buf.rs`、net-windows の `WSASend`。
- 見直す条件: その場で書き換える操作に、一意を求めないものを足すとき。ヒープが物体を動かすようになるとき。
- 守る規則
  - MUST: `Read` に貸す記憶は、完了を取り出すまで生かし、一意に保ち、動かさない。`Write` に貸す記憶は、完了を取り出すまで生かし、書き換えず、動かさない。`Bytes` をその場で書き換える操作は、どれも一意 (rc が 1) を求める。
  - MUST: 読みの貸しの SAFETY とコメントだけが「一意に保ち」と書く。書きの貸しの文は「生かし、書き換えない」と書く。
- 適合の確かめ
  - この段 (T-8): `grep -rn "一意に保ち" crates docs` の残りが、読みの貸しを述べる所だけ。`crates/eml_vm/src/cells.rs` の `a_write_held_by_the_driver_is_not_changed_by_its_alias` と `a_write_of_a_shared_buffer_makes_no_copy` が、両方のモードと Miri の2つの設定で通る (T-8 の実装で、手で書いた LIR が `Async` のエントリを持たず `Std.Net` の行を呼べないので、`miri.rs` から `cells.rs` に移した)。
  - `io-backends` (付録3): `buf.rs` の `Lent::bytes` の SAFETY が「生かし、書き換えない」、`Lent::room` の SAFETY が「一意に保ち」で、付録3の T-2 の基準に同じ grep がある。
- リポジトリに残す所: object-model.md の T10 の行、vm.md の `RawBuf` の項、`driver.rs` の `RawBuf` の doc (T-4、T-8)。

#### D-6: 即値の token
- 決定
  - `Std.Async.OpId` は Kind `Lin`、Repr `Int` の即値で、値は記録の `CellRef` である。`Foreign` の token、その `Drop` の知らせ、`an op token outlived its run` をなくす。
  - 古い token と二重の取り出しは内部の誤り `an op names no record` にする。検査モードでは、記録の worker ID と呼んだタスクの論理的な worker ID を比べ、違えば `an op is used by another task` にする。
  - `sweep_all` に新しい確かめは足さない。
- 理由
  - 操作ごとの `Foreign` の確保と `Drop` の知らせの費用だけを払っていた。線形性の守りは型検査が持つ。`Lin` のスカラーは core-ir.md の規則にある形で、印は `unr`、1回だけ使うことは型検査が守る。std の包みが token を捨てる書き間違いは E3003 で止まる。
  - `exit` と誤りで終わる run は記録を残して正当なので、取り出さない記録を `sweep_all` で誤りにすると偽陽性になる。
- 退けた案
  - `OpId` を `Unr` の即値にする: 型検査の守りを捨て、`sweep_all` の確かめが偽陽性になる。
- 影響の範囲: `eml_extern` の型の行、`cells.rs` の `OpToken` と `Notice::OpDropped`、stdlib.md の表、`crates/eml_core_ir/tests/externs.rs`。
- 見直す条件: ネイティブのコードで token の ABI を決めるとき。
- 守る規則
  - G-20 MUST: `Std.Async.OpId` は Kind `Lin`、Repr `Int` の即値 (値は記録の `CellRef`) で、`Foreign` の token を持たない。token の `Drop` の知らせと `sweep_all` の token の確かめは持たない。古い token と二重の取り出しは内部の誤り `an op names no record` にする。`park_io`/`park_io_until` は継続だけを預かる。
- 適合の確かめ (この段、T-5)
  - `grep -n "OpToken\|OpDropped" crates/eml_vm/src` → 0行。
  - `grep -n 'OpId => type_row' crates/eml_extern/src/lib.rs` の行が `Lin, Repr::Int`。
  - `crates/eml_core_ir/tests/externs.rs` の印の表で `OpId` の位置が `unr`。
  - `cells.rs` の `a_second_take_of_an_op_is_an_internal_error` (手で書いた LIR は `Async` のエントリを持たないので `cells.rs` に置く) と `an_op_used_by_another_task_is_an_internal_error`。
- リポジトリに残す所: stdlib.md の「`Std.Async`」、vm.md の「操作の記録」、core-ir.md の「表の印の列」(T-5)。

#### D-7: 空きの待ちの上限、監視のスレッド、集める地点が OS を見る頻度
- 決定
  - `Clock::Real` で自己パイプのある run は、run の始めに監視のスレッドを起こし、VM は driver の `wait` の期限を切らない。
  - 眠る driver で中継のない run (`Clock::Real` で自己パイプがないか監視のスレッドを起こせなかった run、`with_driver` で注入した run) では、VM が `wait` に渡す期限を今から 10 ms 先までに切る。`Clock::Virtual` のテストの driver の run には上限を掛けない。
  - 集める地点は、空きの待ちの直前では OS を見ない。切り替えが続くときは 16 回に1回 (`POLL_EVERY = 16`)、横取りと `NOTICE` の集める地点ではいつも、`wait(今)` で OS を1回見てから集める。
- 理由
  - Ctrl-C と止める語を届ける道を waker の1本にし、空の起きをなくす。自己パイプのない監視のスレッドは 100 Hz で起きるので、その run では起こさない。CLI はいつも自己パイプを渡す。
  - テストの driver は `wait(Some(d))` で時計を進めるので、上限を掛けると終わらない保留のある run がデッドロックで止まらない。`Injected` に上限を掛けないと、`with_driver` で渡す本物の driver の run が割り込みで起きない。
  - 横取りの刻みは 10 ms に1回なので、計算の重いタスクが走る worker でも I/O の完了の遅れの上限は今と同じ 10 ms になる。
- 退けた案
  - 監視のスレッドをすべての `Clock::Real` の run で起こす: 自己パイプのない run では 100 Hz で起きる。
  - `Normal` の単一タスクの run では今どおり2つ目のタスクで起こす: run の始めに起こす規則に反する。費用は T-10 で測る (10 章の Q-1)。
  - `with_driver(driver, Sleeps::Yes)` の口を作る: 要るテストがない。
- 影響の範囲: `Machine::run`、`Scheduler::begin_run`、`Scheduler::wait_deadline`、`switch_away` の空きのループ、`collect` と5つの呼ぶ所。
- 見直す条件: `worker-threads` で worker のループを作るとき (同じ方針をそのループに持つ)。`busy-self` の比べで 16 が遅いと分かったとき (T-9)。
- 守る規則
  - G-21 MUST: `Clock::Real` で自己パイプのある run は、run の始めに監視のスレッドを起こし、VM は driver の `wait` の期限を切らない。眠る driver で中継のない run (`Clock::Real` で自己パイプがないか監視のスレッドを起こせなかった run、`with_driver` で注入した run) では、VM が `wait` に渡す期限を今から 10 ms 先までに切る。`Clock::Virtual` のテストの driver の run には上限を掛けない (時計を進める driver に掛けると、終わらない保留のある run がデッドロックで止まらない)。`with_driver` に渡す driver は眠る driver に限り、テストの driver を包んだ driver に終わらない保留のあるプログラムを流さないことを vm.md に書く。driver は刻みを持たない (G-6)。
  - MUST (集める地点): `IoDriver::wait` を OS を見るために呼ぶのは、`switch_away` の切り替えが `POLL_EVERY` 回続いたときと、横取りの頭、横取りのループの `NOTICE`、安全点の `NOTICE` の集める地点と、空きのループだけである。`switch_away` の頭の集める地点は OS を見ない。
- 適合の確かめ (この段)
  - T-3: `sched.rs` の `wait_deadline_caps_only_drivers_that_sleep_without_a_relay` と `begin_run_starts_the_monitor_only_for_a_real_run_with_a_doorbell`。`api.rs` の `an_injected_driver_waits_at_most_10_ms_at_a_time` と、Linux の `a_real_run_with_a_doorbell_has_a_monitor_from_its_start`。`grep -n "real_driver" crates/eml_vm/src` → 0行。
  - T-4: `tests/real.rs` の `a_wait_without_ops_sleeps_until_its_deadline` (driver に刻みがない)。`io-backends` で `a_wait_with_a_pending_probe_does_not_tick` (保留ありの版)。
  - T-9: `busy-self` の比べ (付録2 の8.4節)。
- リポジトリに残す所: vm.md の「時計と driver」の空きのループと集める地点の箇条、`with_driver` の項、「タスクとスケジュール」の監視のスレッドの契機 (T-3、T-4)。

#### D-8: driver の置き場所とネイティブの入り口、プールの完了の道
- 決定
  - driver は実行系が持つ。今は `Scheduler` の欄、`worker-threads` 以後は worker の構造の欄である。`Ctx` には入れず、`eml_rt` の extern は driver を呼ばない。`IoDriver` の trait は `eml_rt` の `verus!` の外に残す。`Box<dyn IoDriver>` のままにする。
  - `Async`、`Chan`、`Atomic`、`Net` の行は、実行系の crate の関数として1回だけ書き、VM の腕とネイティブの `extern "C"` の包みが同じ関数を呼ぶ。ネイティブの包みは `Ctx` の `#[repr(C)]` の欄 `worker` から実行系の状態に届く。この形は `jit-core` と `jit-effects` が作る。
  - プールの仕事と名前の解決の完了は、持ち主の worker の driver の完了の列を通して届く。
- 理由: `eml_rt` を vstd だけに保ち、Verus の外の型を核の構造体に入れない。`Worker` と `Ctx` は互いを含まず、`Ctx.worker` は番地だけなので、2つの `&mut` が別の物体を指す。`Foreign` の `Drop` が触れる受け箱は `Arc` で外にあり、今の VM と同じ形である。driver の完了の道が1本なら `completions` の契約も1つで済む。
- 退けた案: driver を `Ctx` に入れる (Verus の外の型が核に入る)。プールの完了を worker の受け箱で届ける (ffi.md の今の文。driver の完了の道が2本になる)。
- 影響の範囲: `eml_rt`、`eml_vm`、将来の `eml_sched` と `eml_native`、ffi.md の「長く走る呼び出し」。
- 見直す条件: `eml_rt` の extern が I/O を持つことになったとき。
- 守る規則
  - G-22 MUST: driver は実行系 (`Scheduler`、将来の `Worker`) が持ち、`Ctx` には入れない。`IoDriver` の trait と型は `eml_rt` の `verus!` の外に置き、`external_body` も信頼の行も持たない。`Async`、`Chan`、`Atomic`、`Net` の行は実行系の関数 `fn(&mut Worker, &mut Ctx<M>, ...)` として1回だけ書き、VM の腕とネイティブの `extern "C"` の包みが同じ関数を呼ぶ。`Worker` と `Ctx` は互いを含まず、`Ctx.worker` は番地だけで、`Foreign` の `Drop` が触れる受け箱は `Arc` で `Worker` の外に置く。cells.rs の核は std と `eml_rt` だけを import する。
  - G-24 MUST: プールの仕事と名前の解決の完了は、持ち主の worker の driver の完了の列 (`Shared` の `Inbox` から `done`) を通して届く。worker の受け箱 (登録表の `Inbox`) は通さない。
- 適合の確かめ
  - この段: `git grep -n "IoDriver" -- crates/eml_rt/src ':!crates/eml_rt/src/driver.rs' ':!crates/eml_rt/src/lib.rs'` → 0行。`crates/eml_rt/src/trust.rs` の `TRUSTED` に `driver.rs` の行がない。`crates/eml_vm/tests/tasks.rs` の `the_task_cells_import_only_std_and_the_runtime` が通る。`grep -n "受け箱の mutex" docs/future/ffi.md` → 0行 (T-10)。
  - `io-backends`: `pool.rs` と `resolve.rs` が `eml_vm` の型を知らない (依存の向きで保証)。
  - `jit-effects`: `worker` から `&mut Worker` を作って extern を1つ呼ぶ Miri の `trusted::` のテスト。
- リポジトリに残す所: async.md の「`IoDriver` と `eml_io`」と multicore.md (今の文のまま)。ffi.md の「長く走る呼び出し」の完了の道 (T-10)。ネイティブの入り口は native.md と roadmap の `jit-core`、`jit-effects` の項 (今の文のまま)。

#### D-9: 自前の driver の形と安全 (io-backends 以後に効く)
- 決定: 付録3と付録4の設計の土台になる規則である。この段では実装せず、`io-backends`、`net-windows`、`io-uring` の計画がこの文で引く。要点は、核と backend の分担、呼ぶ側の鍵で引く1つの表、壊れた driver を 1 秒で終わらなければ abort すること、`mem::forget` と `Box::leak` を書かないこと、プールと `Shared`/`Local` の分け方、unsafe の囲い、偽の backend と Miri、aarch64-linux、io_uring への当てはめの表、epoll の syscall の目標である。
- 理由: 貸した記憶は VM のヒープの一部で、driver は生かし続けることも漏らすこともできない。run からの panic の巻き戻しでは driver が `quiesce` を通らずに `Scheduler` の中で落ち、poisoned でない `Machine` がヒープを放すので、箱だけを漏らしても使用後解放になる。abort はそれを防ぐ唯一の形で、壊れた状態は欠陥でしか起きない。ほかの理由は付録3。
- 退けた案: 付録3と付録4。
- 影響の範囲: `crates/eml_io`、flake、CI。
- 見直す条件: 付録3と付録4。
- 守る規則
  - G-1 MUST: 本物の driver の backend は、Linux は epoll、Apple は kqueue、Windows は IOCP で、それぞれ `libc`、`libc`、`windows-sys` を直接呼んで書く。`io-backends` 以後は `compio-driver`、`compio-buf`、`polling`、`rustix`、`mio`、`io-uring` を `eml_io` の依存に入れてはならない。`io-uring` crate を足せるのは段 `io-uring` だけである。対象は `crates/eml_io/Cargo.toml` と `Cargo.lock` で、`socket2`、`libc`、`windows-sys` は許す。`io-fastpath` の間の compio は対象外。
  - G-2 MUST: 核 (`src/real/`) と backend (`src/sys/`) の分担は付録3の2.2節の表に従う。backend は核の続き (短く終わった書きの残り、`ECONNABORTED` の受けの出し直し、接続の次の番地) を自分から出してはならない。1つの操作の中で OS の待ちを張り直すこと (readiness の `EAGAIN` の後に次の辺を待つ、IOCP の packet の後の `recv` がブロックするときに長さ 0 の受けを出し直す) は続きに当たらず、backend の仕事である。核は OS の呼び出しを `socket.rs` の安全な補助を通してしか行わない。readiness の backend の `start` は `Read`、`Write`、`Accept` で先に syscall を試し、終われば `Start::Done` を返す。
  - G-3 MUST: 保留の操作は、呼ぶ側が振った `OpKey` で O(1) に引ける1つの表にちょうど1つの記録を持ち、記録を放すのは完了を `done` に積むときか、`quiesce` と `Drop` が完了を取り出して捨てるときだけである。`Submit::Done` で終わった操作は表に入れない。`KERNEL_WRITES_AFTER_CANCEL` が `true` の backend では、`Drop` は取り消しを出して I/O の操作の完了をすべて取り出してから記録を放す。driver が壊れていて完了を取り出せないときは、`Backend::drain_broken` が取り消しを出し、合わせて 1 秒までカーネルの終わりを確かめ、終わらない操作が残れば、残った鍵を標準エラーに書いて process を abort する。カーネルが書きうる記憶を放す道も、放されうる状態に置く道 (漏らす) も作らない。`mem::forget` と `Box::leak` は `eml_io` のどこにも書かない。対象は `src/real/` と `src/sys/` で、テストの driver は対象外。
  - G-7 MUST: プールは driver ごとに持ち、スレッドは `spawn_masked` で起こし、上限は 32 本、上限に達した仕事は FIFO の列に積み、`submit` は呼んだスレッドを止めずに `Pending` を返す。結果は `Shared` の `Inbox` に置いて waker を鳴らし、driver の完了の列から届く。`Drop` は仕事と名前の解決を待たず、プールを落とすと受け付けを閉じて空きのスレッドはその場で終わり、仕事中のスレッドはその仕事の後で終わる。`quiesce` は列と実行中の仕事をすべて待つ。
  - G-8 MUST: driver の状態を `Shared` (`Send + Sync`。`Inbox`、waker) と `Local<B>` (表、ハンドル、backend、`done`) に分ける。`Shared` に `RawBuf` も鍵を振る状態も置いてはならない。別のスレッドからの受け付けの API はこれらの段で作らない。
  - G-9 MUST: backend はコンパイル時に OS で1つに決める。Linux に2つ目の backend (io_uring) を足す段では、`RealDriver::new` が起動時に使えるかを調べて選び、環境変数 `EML_IO_BACKEND` で固定でき、CI がその値ごとに `eml_io` のテストを流し、選ばれた backend の名前を返す関数の値が環境変数と同じことをテストが確かめる。それまでは環境変数も選ぶ関数も作らない。
  - G-10 MUST: `crates/eml_io/src/lib.rs` に `#![deny(unsafe_code)]` を置き、`#![allow(unsafe_code)]` は一覧のモジュール (`buf.rs`、`mask.rs`、`sys/unix/{doorbell,epoll,kqueue}.rs`、`sys/windows/{iocp,wsa}.rs`、`tests/real.rs`) だけに書く。ソースを読むテストが、`//` を除いた上で識別子の一部でない語 `unsafe` を持つファイルの集合と `allow(unsafe_code)` を持つファイルの集合を、どちらも一覧と比べる。`unsafe` の塊には日本語の SAFETY のコメントで、どの不変条件に頼るかと spec の節を書く。
    - 読み: 数えるのは、生のポインタを読み書きする所である。`RawBuf` の約束は `IoDriver::submit` の呼び手が負い、`eml_rt` の doc がそれを言う。`Lent::new` は安全な関数で、呼び手は `buf.rs` (テスト用)、`real/mod.rs`、`test.rs` に限り、grep で確かめる。
  - G-11 MUST: `RawBuf` を slice にするのは `src/buf.rs` の関数だけで、本物の driver とテストの driver の両方がそれを使う。`Op::Job` の closure、`Shared`、プールのスレッドに渡す型のどれにも `RawBuf` を置いてはならない。
  - G-12 MUST: 核は OS を呼ばない偽の backend の上の単体テストで確かめ、そのテストを Miri の2つの設定で流す。偽の backend は readiness の台本と完了の台本を持ち、EAGAIN の列、短い書き、`ECONNABORTED`、取り消しと完了の競走、`Start::Done` (速い道) と `Pending`、終わらない操作を台本で起こせる。`mask.rs` は `cfg(miri)` で何もしない。OS を呼ぶ部分は4つの対象の CI の `tests/real.rs` で確かめる。予算は testing.md の「Miri」の規則で決める。
  - G-13 MUST: 対象に aarch64-linux を足す。flake の `systems` は3つ (x86_64-linux、aarch64-linux、aarch64-darwin)、CI の `test` の matrix は4つ、`verus`、`miri`、`fmt` は x86_64 の Linux のまま。aarch64-linux の devShell には verus と verusfmt がなく、`eml_rt` の関門はほかの system で流すことを CLAUDE.md と testing.md に書く。ワークスペースに `target_arch` の cfg を書かず、OS の構造体は `libc` と `windows-sys` のものをそのまま使う。
  - G-16 MUST: io-backends の設計に「io_uring への当てはめ」の節を置き、`Backend` の各メソッドと `Slot` と `KERNEL_WRITES_AFTER_CANCEL` を io_uring の操作に写す 12 行の表を書く。行は `start` の `Readable` (`POLL_ADD`)、`Read` (`RECV`)、`Write` (`SEND`)、`Accept` (`ACCEPT`)、`Connect` (`CONNECT`)、`cancel` (`ASYNC_CANCEL`)、`wait` (`EXT_ARG` と eventfd の起こしの区別)、`close`、`Slot` (`sockaddr` の所有)、`Drop` (CQ を読み、1 秒で終わらなければ abort)、`submit` の `Done`/`Pending` (SQE を出す前の同期の試しをするか)、鍵と `user_data` (表の番号と、欄を使い回すたびに進める世代。取り消しの `addr` も世代つき) である。`io-uring` の段の試作はこの表に従う。
  - G-23 MUST: Linux の epoll の backend の定常状態で、1つの操作あたりの syscall は次を超えない。書けるソケットへの `write_bytes`: 1 (`sendto`)。データのある `read_chunk` (空きあり): 1 (`recvfrom`)。待つ `read_chunk`: 3 (`recvfrom` の `EAGAIN`、`epoll_wait`、`recvfrom`)。backlog のある `accept`: `accept4` 1 と、受けたソケットの登録の `epoll_ctl` 1。空きのループの1回の待ち: 1 (`epoll_wait`)。操作ごとの `epoll_ctl` は出さず、fd の登録はソケットの作成時の1回である。起こしの eventfd の読みは起こされたときだけ。空きの待ちの直前の集める地点では OS を見ない (1往復は eml が片側なら 4 以下)。保留が多いときの1接続あたりの CPU は本数によらず、many-self の 2000 本 ÷ 250 本の比が 1.5 以下 (基点は 2.96)。対象は `sys/unix/epoll.rs` と核で、kqueue は同じ形を目指し、数は macOS で測ってから決める。IOCP と io_uring は対象外。
- 適合の確かめ: 付録3の18節の共通の基準と各タスクの基準、付録4の5節の各タスクの基準。この段の確かめは G-23 の「空きの待ちの直前の集める地点では OS を見ない」だけで、D-7 の確かめに含まれる。
- リポジトリに残す所: roadmap の `io-backends`、`net-windows`、`io-uring` の項の要点 (T-0)。docs への書き込みは各段の計画が持つ。

### 5.2 短中期の設計

理由があれば実装の中で変えてよいが、変えたらコミットのメッセージと計画書に書く。細部は付録1にある。

#### D-10: trait の doc の文
- 付録1 の1節の doc をそのまま入れる。io-backends 以後も成り立つ文だけで、compio に固有の振る舞い (プールが満ちたときの待ち、`quiesce` の 5 秒) は `real.rs` と vm.md の本物の driver の項に移す。
- `wait` の `u64::MAX` の写しは `Ok(false)` の確かめの後に行う。

#### D-11: 鍵、`in_flight`、`settle`
- 鍵は `CellRef::encode()`。`ops`、`NO_KEY`、`OpRecord::key` を消し、保留の数 `in_flight` を登録表に置く。
- `complete` の1件の本体を `settle` に切り出し、`submit_*` と `complete` の両方から呼ぶ。続きの `submit` が `Done` なら同じ記録でループする。`with_worker` は `settle` の中に残す。`submit_*` は `depth_offset` を受ける。
- 形と手順は付録1 の2.1節と2.2節。

#### D-12: `op_ready` と std の包み
- `Std.Async.op_ready : OpId -> Int -> <IO> (Int, OpId)` の1行で、結果は 0 (続ける)、1 (待つ)、2 (先に譲る) の3値にする。速い道の extern を1回にし、channel の `reserve_send` と同じ形にそろえる。
- 数え方は付録1 の2.4節の表。`parks` と `timers` は `op_ready` が 0 か 2 を返すときだけ足し、1 のときは `park_io` が今の規則で数える。
- 退けた案: `op_ready : OpId -> Int -> <IO> (Bool, OpId)` と別の `spend_budget` (速い道で extern が2回)。`ms` を見ない `op_ready` (操作が終わっていた期限付きの待ちを数えなくなり、`timers` がスケジュールに依る)。

#### D-13: 貸し方の細部
- `MIN_ROOM = 8`。広げる式 `長さ + max(長さ, 65 536)` を長さだけで書き、容量を読む関数を `eml_rt` に足さない。
- `read_line` の `Vec` にも 8 バイトの下限を当て、空きがなければ `Readable` の後に `reserve_exact(max(len, 65 536))`。残りがなくなれば確保を放す。行は完了の時点で `String` に1回だけ写す。
- 細部は付録1 の3節。

#### D-14: 書きの取り出しと型
- `take_read` を `take_bytes` に改め、読みと書きの記録の buffer を返す (Net の extern は 18 行のまま)。`take_write` は足さない。
- `submit_write` は写さず、`unique_bytes` を消す。`write_bytes` は `(take_bytes op, s)` を返す。
- 細部は付録1 の4節。

#### D-15: `DriverKind`、`begin_run`、`wait_deadline`
- `Machine::real_driver` を `DriverKind { Real, Virtual, Injected }` に替える。`begin_run` が run の始めの監視のスレッドの唯一の入口、`wait_deadline` が `wait` に渡す期限の唯一の関数である。定数は `sched.rs` の `WAIT_CAP_MS = 10`。
- デッドロックの判定は上限を掛ける前の期限で行う。`signal.rs` の `an_injected_driver_does_not_listen_for_os_signals` は今どおりデッドロックになる。
- 細部は付録1 の2.5節。

#### D-16: 集める地点の `look`
- `collect(look)`、`Scheduler::since_poll`、`POLL_EVERY = 16`。`look` の `wait` の期限は `floor_ms(now)` にする (`ceil_ms` は今より最大 1 ms 先で、compio の `poll` がその分眠りうる)。
- 呼ぶ所ごとの値は付録1 の2.6節の表。
- `POLL_EVERY` の値は T-9 の `busy-self` の比べで決める。往復あたりの実時間が `POLL_EVERY = 1` の形より 50 µs 以上大きくなければ 16 を保つ。

#### D-17: compio の driver の追随
- `submit` は `Step::Done` を `Submit::Done` で返し、鍵は呼ぶ側のものを `ops` に使う。`completions` は `done` を移すだけにし、`poll(ZERO)`、`receive_lookups`、`reap` を `wait` に移す。`IDLE_TICK` を消し、期限 `u64::MAX` は期限なし、ほかは `MAX_POLL` (`i32::MAX` ミリ秒) で切る。`quiesce` の1回の poll は私的な `CANCEL_POLL` (10 ms)。
- 退けた案: `wait` の早い戻り (`done` があれば poll せずに戻る、解決の答えがあれば戻る) を残す。解決の答えは `reap` が受けないと消えないので、回り続ける。
- 細部は付録1 の5.1節。io-backends の差し替えで丸ごと消える。

#### D-18: テストの driver の決着
- 受け付けの時点で決まる操作は `submit` の中で終えて `Submit::Done` を返し、`immediate()` で数える。ほかの操作は `wait` の始めにだけ決着させ、`completions` は決着させない。
- seed の混ぜは `wait` の決着の組にだけ掛かる。決着の組を混ぜる性質を確かめるテストは、受け付けで決まらない操作で組む。
- 細部は付録1 の5.2節。

#### D-19: `Bytes.with_capacity`
- `std/Bytes.em` の `pub extern with_capacity : Int -> <Panic> Bytes` と、Verus の下の `Ctx::bytes_with_capacity`。誤りの約束は `bytes_make` と同じ (負は `bytes length {n} is negative`、確保量の上限は `Stopped(MemoryLimit)`)。
- 細部は付録1 の6.3節。

#### D-20: `eml run --stats` と `bench/net`
- `--stats` は検査モードで走らせ、`Finished` の run の後に標準エラーに `stats:` の1行を出す。`RunStats::fields()` が名前と値の組を宣言の順で返す。
- `bench/net/` に物差しを置き、入口を `bench/net.sh` にする。`cases.tsv` に `compute-fib` (T-2) と `busy-self` (T-9) を足す。
- 段の始めに入れるのは、1点目 (compio の前) を同じ物差しで取るためである。
- 細部は付録2。

#### D-21: 1操作あたりの確保
- `collect` の `Vec::new()` を `Scheduler::runnables` の使い回しにする。残る確保を `--dump` と callgrind で見て、記録と std の包みの局所の組から消せるものを消す。目標の数は決めず、echo-py-reuse の往復あたりの `allocations` が1点目より下がることだけを基準にする。

## 6. 実装の約束

- 各タスクは、実装する規則の `spec/` の節を同じコミットで書く。コードのコメントは、この計画でなく `docs/` の節を引く (CLAUDE.md)。付録の文案とコード片に計画の番号 (D-x、T-x) を書かない。
- 日本語を書くときは natural-japanese のスキルに従う。付録の文案とコード片のコメントも、写すときにもう一度当てる。
- `crates/eml_rt` を変えるタスク (T-4 は `driver.rs` の doc と型、T-6 は `bytes_with_capacity`) は、`crates/eml_rt/CLAUDE.md` の関門を通す。`driver.rs` の doc に `trust.rs` の `PATTERNS` の語を書かず、`TRUSTED` に行を足さない (D-2、D-8)。
- 通常のテストに実時間の上限を書かない。起こしや割り込みは、結果 (割り込みで終わった、期限より前の driver の時刻で戻った) で確かめ、時間は下限だけを比べる。番犬は失敗のときだけ働かせる (D-1 の G-25)。
- 本物のソケットを使う新しいテストには `#[cfg(unix)]` を付ける。この段の Windows の本物の driver はソケットを扱わない。「書いた直後の読みがその場で終わる」ことは OS が約束しないので確かめず、「`Readable` が終わった後の `Read` が `Submit::Done`」の形で確かめる (D-2、D-3)。
- テストの成否と期待値は、付録1 の7節に挙げたテストと範囲の中だけで変える。期待値を変えない機械的な追随は許す。テストの変更の種類はコミットのメッセージに書く。
- `RunStats` にスケジュールで変わる欄を足さない (D-3)。
- `MIN_ROOM`、`POLL_EVERY`、`WAIT_CAP_MS` の名前に `IDLE_TICK` を使わない (D-2 の grep)。
- `bench/net` を CI で流さず、`cargo nextest run` に I/O の時間の assert を入れない (D-1)。
- ad-hoc の `.em` は `timeout 20 cargo run -p eml_cli -- run ...` か、release の eml を `timeout` の下で流す。
- git の worktree で nix が止まるときは `nix develop "path:$PWD" -c ...` を使う。

## 7. 実装のタスク

各タスクは1つのコミットである。T-2b と T-11 は計測を書くだけのコミットである。

| ID | タスク (コミットの題) | 決定 | 依存 | 付録 |
|---|---|---|---|---|
| T-0 | 計画を入れ、roadmap に段を足す (`Plan io-fastpath`) | D-1、D-9 | - | 付録3、付録4 |
| T-1 | `eml run --stats` (`Add eml run --stats`) | D-20 | T-0 | 付録2 の1節 |
| T-2 | `bench/net` (`Add bench/net`) | D-1、D-20 | T-1 | 付録2 の2〜6節、8.1〜8.2節 |
| T-2b | 1点目の計測 (`Measure the I/O baseline`) | D-1 | T-2 | 付録2 の6節 |
| T-3 | 空きの待ちの上限を VM に置く (`Bound the idle wait in the VM`) | D-7、D-15 | T-2b | 付録1 の2.5節 |
| T-4 | trait の変更と driver の追随 (`Give the driver the key and an immediate result`) | D-2、D-7、D-10、D-11、D-16、D-17、D-18 | T-3 | 付録1 の1節、2.1〜2.2節、2.6節、5節 |
| T-5 | 終わった操作を待たない (`Do not wait for finished ops`) | D-3、D-6、D-12 | T-4 | 付録1 の2.3〜2.4節、6.1節、6.4節 |
| T-6 | `Bytes.with_capacity` (`Add Bytes.with_capacity`) | D-4、D-19 | T-5 | 付録1 の6.3節 |
| T-7 | 空きを1つの読みに貸す (`Lend the room of the buffer to one read`) | D-4、D-13 | T-6 | 付録1 の3節 |
| T-8 | 書いた buffer を返し、写さずに貸す (`Return the written buffer and lend it without a copy`) | D-4、D-5、D-14 | T-7 | 付録1 の4節、6.2節、付録2 の8.3節 |
| T-9 | 1操作の確保を減らし、`POLL_EVERY` を比べる (`Cut the allocations of one op`) | D-16、D-21 | T-8 | 付録1 の2.7節、付録2 の8.4節 |
| T-10 | 段を閉じる (`Close io-fastpath`) | すべて | T-9 | 付録1 の8.9節、付録2 の6節 |
| T-11 | 「今の数値」の3つの表 (`Measure io-fastpath`) | D-1 | T-10 | - |

### すべてのタスクに共通の完了の条件

次のコマンドがどれも、失敗も警告もなく終わる。T-0、T-2b、T-11 は文書だけなので、最後の行だけでよい。

```sh
timeout 3000 cargo nextest run
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo clippy -p eml_cli --no-default-features -- -D warnings
cargo clippy -p eml_cli --no-default-features --features types -- -D warnings
cargo clippy -p eml_cli --no-default-features --features core -- -D warnings
cargo clippy -p eml_test_support --no-default-features --features hir -- -D warnings
cargo nextest run -p eml_cli --test integration -E 'test(citations::)'
```

`crates/eml_io` か Windows の分岐に触れるタスク (T-4、T-8) は、次も通す。

```sh
nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings
```

`crates/eml_rt` に触れるタスク (T-4、T-6) は、`crates/eml_rt/CLAUDE.md` の関門を通す。

```sh
cargo verus verify -p eml_rt
verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')
cargo test -p eml_rt trust
```

CI は `test` の3行 (macos-15、ubuntu-24.04、windows-2025)、`fmt`、`verus`、`miri` の2行が緑であること。実行の始めに利用者が決めたとおり、各タスクは Linux と `.#cross` の clippy で確かめ、PR は段を閉じる前に作って CI を流す。macOS と Windows で落ちたら、そのときに直すコミットを足す。

### T-0: 計画を入れ、roadmap に段を足す
- 目的: 段の設計と計画をリポジトリに入れ、roadmap の段の列に4つの段を置き、後の段の設計の要点を roadmap に残す (D-1、D-9)。
- 基づく決定: D-1、D-9
- 対象: `docs/plans/2026-10-29-io-fastpath.md` と `docs/plans/2026-10-29-io-fastpath/{01-fastpath,02-bench,03-io-backends,04-net-windows}.md` (足す)、`docs/plans/2026-10-29-io-backends.md` と `docs/plans/2026-10-29-io-backends/{01-core-unix,02-iocp}.md` (消す)、`docs/future/roadmap.md`、`docs/future/async.md`
- 作業
  - 4 章の前提をすべて確かめる。最初の行が通らなければ止める。
  - 前回の計画書と付録を消し、この計画と付録を置く。
  - roadmap の「段の列」の先頭 (`net-tls` の前) に `io-fastpath` と `io-backends` の項を、この順で足す。下の文案を、日本語の規則を当ててから写す。
  - roadmap の `net-windows` の項を下の文案に替え、`std-http` の後の今の位置に置く。その後に `io-uring` の項を足す。
  - `async-threads`、`async-files`、`ffi-blocking` の「前提」に `io-backends` を足す。今の前提があればその後に「、」で続け、「なし」なら `io-backends` に置き換える。
  - roadmap の「その後の項目」の「io_uring の backend」の項目 (下の箇条を含む) を消す。段 `io-uring` に移る。
  - roadmap の「その後の項目」の「完了を集める手間」の項目 (下の箇条を含む) を消す。保留が多いときの集める地点の手間は、段 `io-backends` の項 (`completions` は表を歩かない) と `io-fastpath` の trait の約束に移る。中の文は消えた `Std.IoTest` と閉じた async-net に触れていて古い。
  - async.md の「未決の論点と将来の拡張」の「I/O の driver の crate」の箇条と、その下の2つの箇条 (「自前の driver に替えるもう1つの条件」「替えるときの順」) を下の文案に替える。「ファイルとパイプ」の macOS の通常のファイルの箇条は今のまま残す (中の compio の文は `io-backends` が消す)。
  - 基点で `cargo nextest run` を流し、通ったテストの数をコミットのメッセージに書く (9 章の基点の確かめ)。
- roadmap の文案 (段の列):

```
- `io-fastpath`: 本物の I/O の道を、終わった操作を待たない形にする。`IoDriver` の trait を1回だけ変え (呼ぶ側が振る鍵、`submit` がその場の結果を返すこと、1つの読み、刻みのない `wait`)、VM と std の約束を変える。終わった操作は perform せずに続け、空きのある buffer には1回の読みで読み、token を即値にし、`write_bytes` は buffer を返し、共有の `Bytes` を写さずに書き、足し続ける読みの容量を倍に伸ばす。`Bytes.with_capacity` を足す。本物の driver は `compio-driver` のまま新しい trait に合わせる。`eml run --stats` と `bench/net` を入れ、前と後を測る。前提: なし。設計: [非同期の設計](async.md) の「`IoDriver` と `eml_io`」
- `io-backends`: 本物の I/O の driver を、OS ごとに自前で書く backend (Linux は epoll、Apple は kqueue、Windows は IOCP) に替え、`compio-driver` を外す。VM、std、`eml_extern`、`IoDriver` の trait には触れない。aarch64-linux を対象に足す。Windows の backend はこの段ではソケットを扱わない。前提: `io-fastpath`。設計: [非同期の設計](async.md) の「`IoDriver` と `eml_io`」
  - driver を、呼ぶ側の鍵で引く1つの表を持つ核と、1つの操作を1回ずつ OS に出す backend に分ける。核は OS を呼ばず、偽の backend の上で Miri で確かめる。核の続き (短い書きの残り、受けの出し直し、接続の次の番地) は核だけが出す
  - epoll と kqueue は、ソケットを作るときに1回だけ edge (`EPOLLET`、`EV_CLEAR`) で登録し、操作ごとの登録を出さない。fd ごとに読める記憶と読み・書きの列を持ち、記憶は読みの向きのブロック (`EAGAIN`) でだけ下ろす。`Readable` は記憶が立っていれば1バイトを覗いて確かめ、読み尽くした後は待つ
  - `completions` は OS を見ず、表を歩かない。`wait` は刻みを持たない。名前の解決の答えは driver の中の札で当て、使い回した鍵の操作に当てない
  - 落とすときに仕事と名前の解決を待たない。プールは 32 本までで、満ちたら driver の中の列で待ち、`submit` は呼んだスレッドを止めない。壊れた driver を落とすときは、カーネルがまだ書きうる操作の終わりを 1 秒まで確かめ、終わらなければ process を abort する。`mem::forget` と `Box::leak` を書かない
  - `unsafe` は一覧のファイルに閉じ、ソースを読むテストで固定する。読みの貸しは一意に保ち、書きの貸しは生かして書き換えない。OS を呼ぶ部分は4つの対象 (Linux の x86_64 と aarch64、macOS、Windows) の CI と AddressSanitizer で確かめる
  - 目標: epoll の定常状態で、データのある読みは syscall 1回、書きは 1回、待つ読みは 3回、操作ごとの `epoll_ctl` は 0。保留が多くても1接続あたりの CPU が本数によらない (`bench/net` の many-self の 2000 本 ÷ 250 本が 1.5 以下)。閉じる前に `bench/net` で、差し替えの前の driver と比べる
```

```
- `net-windows`: 自前の IOCP の backend にソケットを載せ、同じネットのテストの組を Windows で流す。`Std.Signal` の `Terminate` を CTRL_CLOSE_EVENT と CTRL_SHUTDOWN_EVENT に写す。前提: `io-backends`。設計: [非同期の設計](async.md) の「テストと決定性」
  - 読みはブロックしない `recv` を先に試し、ブロックするなら長さ 0 の `WSARecv` で読めるまで待ってから、もう一度 `recv` する。読みの buffer をカーネルに渡さない。書きと受けも先に試し、ブロックするときだけ `WSASend` と `AcceptEx` を出す
  - 記録ごとの `OVERLAPPED` の箱は、カーネルが持つ間だけ記録に置き、カーネルが書き終えたと packet で確かめるまで放さない。確かめずに放す道に入った箱は abort する
  - ソケットは作るときにブロックしない設定にして IOCP のポートへ結び付ける。成功のときに packet を省くモードは使わない。非 IFS の LSP の下でこのモードが効かず、放した箱を読む道を残さないためである。同期に失敗した呼び出しのほかは、結果を packet で読み、packet の誤りは `WSAGetOverlappedResult` で読んで WSA の番号にそろえる
  - OS が取り消しの要求を断ったら、driver を壊れた状態にする。壊れた driver を落とすときは、取り消しを出して 1 秒まで終わりを確かめ、終わらなければ process を abort する
  - 文書にない前提 (長さ 0 の `WSARecv` の振る舞い、ブロックしないソケットの重なり合わない呼び出しが packet を積まないこと、`accept` と `AcceptEx` の併用) は、コードを書く前にテストだけのコミットで Windows の CI で確かめる
  - Windows の `SO_REUSEADDR` は立てない。ループバックへの接続では SYN を送り直さない設定を出し、拒まれた接続をすぐ誤りにする
  - Windows で流し始める `tests/real.rs` と `eml_vm/tests/api.rs` のテストは、成否の変更として段の計画に1件ずつ挙げる
- `io-uring`: Linux に io_uring の backend を足すかを、試作と計測で決める。最初のタスクで `Backend` の実装を1つ試作し、`bench/net` の many-self-1000、bulk-1k、echo-py で epoll と比べる。user+sys の CPU が2割以上縮む件がなければ、試作を捨て、数字を [計測の基準](../implementation/benchmarks.md) に書いて段を閉じる。続けるときは、起動時に io_uring が使えるかを調べて選び (使えなければ epoll)、環境変数 `EML_IO_BACKEND` で固定でき、CI が両方の backend で `eml_io` のテストを流す。最低の kernel は 5.11 (`EXT_ARG`)。`user_data` は backend の表の番号と、その欄を使い回すたびに進める世代にする。壊れた driver を落とすときは、取り消しを出して 1 秒まで完了を読み、終わらなければ abort してから ring を閉じる。前提: `io-backends`。設計: [非同期の設計](async.md) の「`IoDriver` と `eml_io`」
```

- async.md の文案 (「未決の論点と将来の拡張」の3つの箇条を替える):

```
- I/O の driver の crate: 今の本物の driver は `compio-driver` の上にある。OS ごとの backend (Linux は epoll、Apple は kqueue、Windows は IOCP) を `libc` と `windows-sys` で自前に書いて替える。mio は Windows で AFD の readiness を真似るので採らない
  - 自前の driver は、保留の操作を呼ぶ側の鍵で引く1つの表で持つ。そのため、driver を落とすときに仕事を待ちきることも、壊れた driver の鍵を漏らすことも要らない。救済の後の受け付けを、縛った worker の受け箱を通さずに別のスレッドから出す形は、`async-threads` の計測で遅れが目立ったときに考える
  - 替える順: 先に `IoDriver` の trait と VM と std の I/O の約束を compio の上で変え、次に driver だけを自前の backend に差し替え、Windows のソケットをその後に載せる。io_uring は試作と計測で入れるかを決め、入れても、io_uring が使えない環境のために epoll の道は残す。自前の driver の syscall の周りの `unsafe` は Miri で動かせないので、4つの対象の CI と AddressSanitizer で確かめる ([ロードマップ](roadmap.md) の `io-backends`、`net-windows`、`io-uring`)
```

- テストの変更 (この計画の承認で合意したもの。種類は testing.md の「テストの変更の運用」。1本ずつの扱いは付録1 の7節)
  - 成否の変更 (種類1): `crates/eml_io/src/real.rs` の単体テスト `a_wake_from_another_thread_ends_a_long_poll` を T-4 で消す。中身は `tests/real.rs` の `a_wake_ends_a_wait_with_a_far_deadline` が `wait` を通す形で引き継ぐ。
  - 期待値の変更 (種類2) の範囲
    - T-4: `tests/test_driver.rs` と `tests/real.rs` のうち、`completions` を呼んで決着か OS の確かめを待っていたテスト (基点で 21 本) を、`wait(Some(now))` を挟んでから `completions` で受ける形に直す。受け付けの時点で決まる操作を `completions` から受け取っていたテストを、`submit` の戻り値から受け取る形に書き直す。seed のテスト `a_seed_does_not_change_which_ops_finish` と取り消しの2本 (`a_cancel_before_return_ends_the_op_at_the_next_settle`、`a_cancel_before_settle_marks_the_completion_cancelled`) を、受け付けの時点で決まらない操作で組み直す。`tests/real.rs` の `live_lookups_beyond_the_limit_wait_in_order` の待ちのループを `poll_for` に替える。どれも完了の値は変えない。理由: 受け付けで終わる操作は完了として届かず、`completions` は OS を見なくなり、`wait` に刻みがなくなる。
    - T-4 と T-5: `tests/ui/run/async/net_*` と `crates/eml_cli/tests/repl/async/*.repl` のうち、操作が受け付けで終わったことと、終わった操作を待たないことで、出力の順が変わるもの。理由: 終わった操作の待ちが切り替えなくなり、タスクの交錯が変わる。
    - T-5: `crates/eml_core_ir/tests/externs.rs` の印の表の `OpId` の位置を `unr` に、2つの一覧の行を直す。理由: 即値の token の印は `unr` である。
    - T-5: `crates/eml_cli/tests/ui.rs` の `compares_stats` で、channel を使わず、どちらかの run の `completions` が 0 でない run は、`tasks`、`parks`、`timers`、`dropped_failures`、`shared_objects`、`channels_swept`、`completions` だけを比べる (新しい `eml_test_support::driver_independent`)。`par/` の run と driver の操作のない run は、すべての欄を比べたままにする。理由: 操作が受け付けで終わって待ちを省くかはスケジュールで変わるので、VM の仕事の回数がスケジュールに従う。D-3 が約束する数は変わらない。これがないと `tests/ui/run/async/net_*` の9本が seed の run で落ちる。
    - T-5 の後追い (予算の規則を決め直した利用者の判断): `cells.rs` の `op_ready_counts_a_wait_only_on_a_finished_op` の期待値 (終わっていない記録でも予算を使う、譲る待ちは `parks` を数えない) と、`io.rs` の `the_budget_runs_out_at_the_same_point_in_every_schedule` の形 (待ちの数の偶奇を変えた2つの形。後の後追いで名前を替え、ループに `check_cancelled` を足した)。理由: どの `io_wait` も予算を1つ使い、`parks` を1つだけ数える規則に替えた。付録1 の7.4節の4本の値は変わらない。
    - T-5 と T-8: `io.rs` の `one_task_of_finished_ops_never_waits_on_the_driver` を T-5 で中間の値 (`immediate == 6`、`completions == 6`) で入れ、T-8 で最終の値 (`immediate == 5`、`completions == 5`) に直す。理由: 空きのある buffer の読みと `write_bytes` の新しい型が T-6 と T-8 で入る。
    - T-7: `cells.rs` の単体テストのうち貸し方を確かめる4本 (`a_read_lends_a_roomy_buffer_in_place_and_grows_a_short_one`、`probed_read` を使うもの、`a_read_line_lends_the_stream_buffer_and_frees_it_when_empty`、`a_read_after_a_read_line_is_served_from_the_stream_buffer`) を新しい規則の値に書き直す。理由: 貸し方と広げる式が変わる。
    - T-8: `io.rs` の `a_read_line_that_is_not_utf8_raises_not_utf8` の `Net.write_bytes ... a` を `let (_, a) = ...` にする。理由: `write_bytes` が組を返す。
  - 機械的な追随 (種類3): `tests/real.rs` と `tests/test_driver.rs` の署名の追随と補助 `run_op`、`IDLE_TICK` を書いたコメント。T-5 の `cells.rs` の単体テストの stub `NoDriver` に、書きを受け付けで終える印を足すこと。
  - ここに挙げたほかに成否が変わるテストが出たら、そのタスクで止め、計画書に種類1として1件ずつ足してから進める。
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件
  - `git diff --name-status HEAD~1` → 次の10行だけ: `A docs/plans/2026-10-29-io-fastpath.md`、`A` の付録4つ、`D docs/plans/2026-10-29-io-backends.md`、`D docs/plans/2026-10-29-io-backends/01-core-unix.md`、`D docs/plans/2026-10-29-io-backends/02-iocp.md`、`M docs/future/async.md`、`M docs/future/roadmap.md`
  - 次の1つ目のコマンドが4行 (4つの段の項)、2つ目が5行 (`net-windows`、`io-uring`、`async-threads`、`async-files`、`ffi-blocking` の項) を出す

    ```sh
    git grep -n -e '^- `io-fastpath`' -e '^- `io-backends`' -e '^- `net-windows`' -e '^- `io-uring`' docs/future/roadmap.md
    git grep -n '前提: .*`io-backends`' docs/future/roadmap.md
    ```

  - `git grep -n '完了を集める手間\|io_uring の backend:\|fusion' docs/future/roadmap.md` → 0行
  - `git grep -n '自前の driver に替えるもう1つの条件' docs/future/async.md` → 0行
  - `cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` → 通る
- 落とし穴: async-net の閉じる作業で roadmap と async.md の行は動いている。文案は中身で当て、行番号で探さない。roadmap の「進め方」の手順5は閉じた段の名前を「前提」から外すので、`io-fastpath` の前提は「なし」である。

### T-1: `eml run --stats`
- 目的: 本物の driver の `RunStats` を読む口を作る (D-20)。1点目を取る前に入れる。
- 基づく決定: D-20
- 対象: `crates/eml_vm/src/lib.rs` (`RunStats::fields`)、`crates/eml_cli/src/main.rs` (`Run` の `stats`)、`crates/eml_cli/tests/cli.rs`、`docs/implementation/architecture.md` の「CLI と lib API」、`CLAUDE.md` の Commands の `run [...]` の行
- 作業: 付録2 の1節のとおりにする。CLAUDE.md の `run` の行に `[--stats]` を足し、`main.rs` にない `--clock real|virtual` の記述を run と repl の両方から消す。
- テスト: 付録1 の7.1節の2本 (新しく足す)。
- 実装者に任せること: 単体テストの名前、出力の並びを作る補助の形
- 条件: いつも
- 完了の条件
  - 共通の条件
  - `cargo build -p eml_cli` の後、`timeout 20 cargo run -p eml_cli -- run --stats tests/ui/run/async/net_echo.em 2>&1 >/dev/null | grep -c '^stats: '` → `1`
  - `timeout 20 cargo run -p eml_cli -- run tests/ui/run/async/net_echo.em 2>&1 >/dev/null | grep -c '^stats: '` → `0`
  - `grep -n -- '--clock' CLAUDE.md` → 0行
- 落とし穴: `fields()` の配列の長さは欄の数と一緒に変える。`--stats` の run は検査モードなので、その run の時間を物差しに使わない。REPL には足さない。

### T-2: `bench/net`
- 目的: 物差しをリポジトリに入れる (D-1、D-20)。
- 基づく決定: D-1、D-20
- 対象: `bench/net/{base,echo,bulk,many,conn}.em`、`bench/net/{cases.tsv,peer.py,run.py,bench.py,report.py}`、`bench/net.sh`、`docs/implementation/benchmarks.md` (「ネットワークの計測」の節)、`docs/implementation/testing.md` (「よく使うコマンド」と「性能のテスト」)、`CLAUDE.md` の Commands
- 作業
  - 付録2 の8.1節と8.2節のファイルを、そのまま置く。`bench/net.sh` に実行の印を付ける。プログラムは今の API (`write_bytes` が `TcpStream` を返す) のまま入れる。
  - benchmarks.md に「ネットワークの計測」の節を足す。物差しの件、主指標、比べる規則 (付録2 の2節と4節の要約) を書き、表はまだ置かない (付録1 の8.2節)。
  - testing.md の「よく使うコマンド」に `bench/net.sh` を足し、「性能のテスト」に、`bench/net` はテストでなく CI で流さないことを1文で書く。CLAUDE.md の Commands に付録1 の8.2節の1行を足す。
- 実装者に任せること: benchmarks.md の節の文の細部
- 条件: いつも
- 完了の条件
  - 共通の条件
  - `timeout 1800 bench/net.sh --only '^base$' cur=./target/release/eml > target/net-base.jsonl` → 終了コード 0 (Linux)
  - `python3 -I bench/net/report.py target/net-base.jsonl | grep -c '^## 時間: cur'` → `1`
  - `cargo nextest run -p eml_vm --test integration -E 'test(bench::)'` → 通る (`bench/net/*.em` は `every_program_in_bench_has_a_test` にかからない)
  - `git grep -n 'immediate' bench/net` → 0行
- 落とし穴: `bench/run.sh` は `bench/*.em` だけを流すので、`bench/net/` のプログラムは流れない。`compute-fib` の行は `../fib.em` を引くので、`bench/fib.em` の名前を変えない。

### T-2b: 1点目の計測
- 目的: compio の前 (この段の始め) の数字を取る (D-1 の G-25)。
- 基づく決定: D-1
- 対象: `docs/plans/2026-10-29-io-fastpath.md` の「計測の記録」の節だけ
- 作業
  - T-2 のコミットを別の worktree に取り出し、`CARGO_TARGET_DIR` を分けて `timeout 1200 cargo build --release -p eml_cli` し、`eml-t2` に写す。
  - `python3 -I bench/net/bench.py --runs 11 --strace-runs 3 --stats before=./eml-t2 > results-1.jsonl` と `python3 -I bench/net/report.py results-1.jsonl > summary-1.md` を流す (付録2 の6節)。
  - 「計測の記録」に、機械 (CPU、vCPU の数、メモリ)、kernel (`uname -r`)、コミット、`summary-1.md` の時間、syscall、`RunStats` の表を写す。macOS があれば `timeout 1200 bench/scripts.sh` の行も控える。
- 実装者に任せること: なし
- 条件: 4 章の Linux の機械がなければ止めて利用者に報告する。
- 完了の条件
  - `git diff --name-only HEAD~1` → `docs/plans/2026-10-29-io-fastpath.md` だけ
  - 「計測の記録」に3つの表と、機械、kernel、コミットがある
- 落とし穴: `--stats` の run と時間の run は別の run である。`busy-self` はまだないので、1点目には値がない (付録2 の6節)。

### T-3: 空きの待ちの上限を VM に置く
- 目的: driver の刻みを消す前に、Ctrl-C と止める語が眠る worker に届く道を VM に置く (D-7、D-15)。
- 基づく決定: D-7、D-15
- 対象: `crates/eml_vm/src/lib.rs` (`DriverKind`、`Machine::real_driver` を `driver_kind` に、`Start`)、`crates/eml_vm/src/sched.rs` (`driver_kind` の欄、`WAIT_CAP_MS`、`begin_run`、`wait_deadline`、`relay_on`)、`crates/eml_vm/src/vm.rs` (`switch_away` の `wait` の引数、`AsyncParkSignal` の腕、`run_scheduled` の `begin_run`、コメント)、`crates/eml_vm/tests/api.rs`、`docs/spec/vm.md` (「時計と driver」の空きのループの項、`with_driver` の項、「タスクとスケジュール」の監視のスレッドの契機)
- 作業: 付録1 の2.5節のとおりにする。docs は付録1 の8.3節の文案を、日本語の規則を当ててから写す。
- テスト: 付録1 の7.2節の4本 (新しく足す)。
- 実装者に任せること: `DriverKind` を置く module、補助の関数の分け方
- 条件: いつも
- 完了の条件
  - 共通の条件
  - `grep -n "real_driver" crates/eml_vm/src` → 0行
  - `grep -n "start_monitor" crates/eml_vm/src/vm.rs crates/eml_vm/src/lib.rs` → `park_signal` の腕の1行だけ (run の始めは `begin_run` が受け持つ)
  - `cargo nextest run -p eml_vm --test integration -E 'test(api::an_injected_driver_waits_at_most_10_ms_at_a_time) | test(api::a_real_run_with_a_doorbell_has_a_monitor_from_its_start)'` → Linux で2本が通る
  - `cargo nextest run -p eml_vm --lib -E 'test(wait_deadline_caps_only_drivers_that_sleep_without_a_relay) | test(begin_run_starts_the_monitor_only_for_a_real_run_with_a_doorbell)'` → 2本が通る
- 落とし穴: テストの driver を包む driver を `with_driver` で渡すテストに、デッドロックするプログラムを流さない (付録1 の2.5節)。デッドロックの判定は上限を掛ける前の期限で行う。`/proc` を読むテストは `cargo test` では偽に通りうるので、nextest の結果を確かめとする。

### T-4: trait の変更と driver の追随
- 目的: trait の3つの変更を、trait とすべての実装と VM の鍵と集める地点で1つのコミットにする (D-2、D-7、D-10、D-11、D-16、D-17、D-18)。
- 基づく決定: D-2、D-7、D-10、D-11、D-16、D-17、D-18
- 対象
  - `crates/eml_rt/src/driver.rs` と `lib.rs` (付録1 の1節)
  - `crates/eml_io/src/real.rs`、`lib.rs`、`test.rs` (付録1 の5節)、`crates/eml_io/tests/real.rs`、`tests/test_driver.rs`
  - `crates/eml_vm/src/cells.rs` (鍵、`in_flight`、`settle`、`ops` と `NO_KEY` と `OpRecord::key` の削除、`submit_*` の `depth_offset`、不変条件のコメント)、`vm.rs` (`submit_*` の呼び出しに `task.depth_offset`、`collect` の `look` と5つの呼ぶ所)、`sched.rs` (`POLL_EVERY`、`since_poll`)、`lib.rs` の `Machine::drop` のコメント
  - テストの stub (`cells.rs` の `NoDriver`、`tests/io.rs` の `HoldingDriver`、`tests/machine.rs` の `FakeDriver` と `StubDriver`、`tests/api.rs` が使う driver)
  - docs: 付録1 の8.4節の一覧
- 作業: 付録1 の1節、2.1節、2.2節、2.6節、5節のとおりにする。docs は8.4節の文案を写す。
- テスト: 付録1 の7.3節 (種類1の1本、種類2の範囲、種類3、新しく足すテスト)。
- 実装者に任せること: `settle` の内部の分け方、補助 `run_op` の名前
- 条件: いつも
- 完了の条件
  - 共通の条件、cross の clippy、`eml_rt` の関門
  - `grep -rn "IDLE_TICK" crates docs CLAUDE.md | grep -v docs/plans` → 0行
  - `grep -n "HashMap<OpKey" crates/eml_vm/src` → 0行
  - `grep -n "next_key" crates/eml_io/src` → 0行
  - `grep -n "仕事を待ちきる" crates/eml_vm/src` → 0行
  - `real.rs` の `fn completions` の本体に `poll` と `reap` がない (レビューで読む)
  - `git diff --stat HEAD~1 -- crates/eml_rt/src` → `driver.rs` と `lib.rs` だけ
  - `cargo nextest run -p eml_io --test integration -E 'test(a_wait_with_no_ops_and_a_far_deadline_wakes) | test(a_wake_ends_a_wait_with_a_far_deadline) | test(a_wait_without_ops_sleeps_until_its_deadline) | test(live_lookups_beyond_the_limit_wait_in_order)'` → 4本が通る
  - `cargo nextest run -p eml_vm --test integration -E 'test(signal::a_waiter_keeps_an_otherwise_idle_run_alive)'` → 通る (`switch_away` の `deadline.or(Some(u64::MAX))` の道がデッドロックにならない)
  - `cargo nextest run -j 1 -p eml_io` → どのテストも 1 s 以内
- 落とし穴
  - `settle` の中で `driver.submit` が `Done` を返したら、同じ記録でループする (再帰しない)。
  - `submit_*` で `alloc` した後に `driver.submit` が `Done(Err)` でも、記録は `Done` で残し、包みの `finish` が放す。
  - `in_flight` は `Pending` のときだけ足し、`complete` が当てたときに引く。`quiesce` の後の `sweep_all` の前に 0 に戻す。
  - テストの driver の `submit` は `empty_bin` を先に呼び、決着と同じく閉じた後の状態を見る。
  - compio の `wait(None)` は保留も解決もなければ `Ok(false)` を返す。`wait(Some(u64::MAX))` は保留がなくても `poll(None)` で起こしを待つ。
  - `trust.rs` は `driver.rs` を走査する。新しい doc に `PATTERNS` の語を書かない。

### T-5: 終わった操作を待たない
- 目的: 速い道、即値の token、`op_ready` と数え方 (D-3、D-6、D-12)。
- 基づく決定: D-3、D-6、D-12
- 対象: `crates/eml_extern/src/lib.rs` (付録1 の6.4節の T-5 の行)、`std/Async.em` (6.1節)、`crates/eml_vm/src/cells.rs` (2.3節と2.4節。`TaskStats` の doc)、`vm.rs` (Net と park の腕、`AsyncOpReady` の腕、`schedule`、`Async` の命令の一覧)、`crates/eml_vm/src/lib.rs` (`RunStats::parks` の doc。`completions` の doc は T-4 で書いた)、`crates/eml_core_ir/tests/externs.rs`、docs (付録1 の8.5節)
- 作業: 付録1 の2.3節、2.4節、6.1節、6.4節のとおりにする。
- テスト: 付録1 の7.4節。
- 実装者に任せること: `op_ready` の本体の分け方。`a_second_take_of_an_op_is_an_internal_error` を LIR で書けないときの置き場所 (`cells.rs` の単体テストにし、計画書に書く)
- 条件: いつも
- 完了の条件
  - 共通の条件
  - `grep -n "OpToken\|OpDropped" crates/eml_vm/src` → 0行
  - `grep -n "immediate" crates/eml_vm/src/lib.rs` → 0行
  - `grep -n 'OpId => type_row' crates/eml_extern/src/lib.rs` の行が `Lin, Repr::Int` を含む
  - `cargo nextest run -p eml_vm --test integration -E 'test(io::one_task_of_finished_ops_never_waits_on_the_driver) | test(io::echo_counts_do_not_depend_on_the_seed) | test(io::a_fast_path_loop_yields_to_another_task_under_sequential) | test(io::a_cancelled_fast_path_loop_stops_at_its_next_wait)'` → 4本が通る
- 落とし穴: `op_ready` は `Done` でない記録で数えない (待つ側の `park_io` が数える)。`op_ready` は取り消しの地点を読むだけで `cancels` を数えない。`AsyncOpReady` は `Op::Async` で出るよう `is_async` に足す。`std/Async.em` の決まり (`run` より上に関数を置かない、extern は末尾に足す) を守る。

### T-6: `Bytes.with_capacity`
- 目的: 空きのある buffer を作る口 (D-4、D-19)。
- 基づく決定: D-4、D-19
- 対象: `crates/eml_rt/src/heap/ctx/bytes.rs`、`crates/eml_rt/src/heap/tests/{table.rs,expected.txt,bytes.rs}`、`crates/eml_extern/src/lib.rs` (`BytesWithCapacity`)、`std/Bytes.em`、`crates/eml_vm/src/vm.rs` (腕)、`tests/ui/run/std/bytes_with_capacity.em`、`tests/ui/run-fail/std/bytes_with_capacity_negative.em`、docs (付録1 の8.6節)
- 作業: 付録1 の6.3節のとおりにする。
- テスト: 付録1 の7.5節。
- 実装者に任せること: spec の項の並び (6.3節の項はすべて持つ)
- 条件: いつも
- 完了の条件
  - 共通の条件、`eml_rt` の関門
  - `cargo nextest run -p eml_cli --test integration -E 'test(=ui::run) | test(=ui::run_fail)'` → 2本が通る。UI のテストはディレクトリごとに1本で、ファイルで絞れない。この2本の走査が新しい2つのファイルを含む (snapshot を受け入れる前は、どちらも新しいファイルだけで落ちる)
- 落とし穴: Miri の `trusted::` は流さなくてよい (検証したコードだけで、信頼の部分に触れない)。`expected.txt` は手で書かず、`#[ignore]` のテストで作り直す。

### T-7: 空きを1つの読みに貸す
- 目的: 1つの読み、`MIN_ROOM`、幾何の伸び、`read_line` の写し1回 (D-4、D-13)。
- 基づく決定: D-4、D-13
- 対象: `crates/eml_vm/src/cells.rs` (`submit_read`、空きの判定と広げる関数、`NetStream::lend_room` と `commit_room`、`ZERO_ROOM` の削除、`OpKind::Line` の `not_utf8`、`take_line`)、`crates/eml_vm/tests/io.rs`、`crates/eml_vm/tests/api.rs`、`std/Net.em` の `read_chunk` の doc、docs (付録1 の8.7節)
- 作業: 付録1 の3節のとおりにする。
- テスト: 付録1 の7.6節。
- 実装者に任せること: 空きの判定と広げる関数の分け方
- 条件: いつも
- 完了の条件
  - 共通の条件
  - `grep -n "ZERO_ROOM" crates/eml_vm/src` → 0行
  - `cargo build --release -p eml_cli` の後、bash で `TIMEFORMAT=%R; time timeout 60 ./target/release/eml run bench/net/bulk.em 64 64 fresh accum` → 標準出力が `bulk done 67108864`、`time` が出す秒数が 1.00 以下
  - `git diff --stat HEAD~1 -- tests/ui crates/eml_cli/tests/repl` → 何も出ない (UI の期待値を変えない)
- 落とし穴: 一意の判定は `is_unique` で、空きは一意なときだけ `bytes_lend` で読む。`Vec` の空きは未初期化なので、`commit` の前に `set_len` しない。`Bytes.of_string` の写しが `alloc_bytes` と同じ切り上げを使うことを、`bytes_lend` の `cap` を読んで確かめる (10 章の Q-9)。

### T-8: 書いた buffer を返し、写さずに貸す
- 目的: `write_bytes` の型と、書きの写し 0 回 (D-4、D-5、D-14)。
- 基づく決定: D-4、D-5、D-14
- 対象: `crates/eml_vm/src/cells.rs` (`submit_write`、`unique_bytes` の削除、`take_bytes`)、`vm.rs` (`NetTakeBytes`)、`crates/eml_extern/src/lib.rs` (`NetTakeBytes`)、`std/Net.em` (付録1 の6.2節)、`bench/net/{echo,bulk,many}.em` (付録2 の8.3節)、`crates/eml_io/src/real.rs` と `test.rs` の SAFETY、`crates/eml_vm/tests/{io,miri}.rs`、docs (付録1 の4.2節と8.8節)
- 作業: 付録1 の4節と6.2節のとおりにする。`bench/net` の3つのプログラムに付録2 の8.3節の差分を当てる。
- テスト: 付録1 の7.7節。
- 実装者に任せること: miri.rs の driver の形 (実装で決めたこと: 手で書いた LIR は `Async` のエントリを持たず、`Std.Net` の行がどれも `is_async` なので `Async.run` なしに呼べない。T-5 の `a_second_take_of_an_op_is_an_internal_error` と同じく、2本を `cells.rs` の単体テストにし、stub の `NoDriver` が書きに貸された記憶を、書きを終える時点で読む。Miri は `cells::a_read` の実行に `cells::a_write` を足して流す)
- 条件: いつも
- 完了の条件
  - 共通の条件、cross の clippy
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::` → 通る
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows" cargo miri test -p eml_vm --test integration miri::` → 通る
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --lib -- cells::a_read cells::a_write` → 通る
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows" cargo miri test -p eml_vm --lib -- cells::a_read cells::a_write` → 通る
  - `grep -rn "unique_bytes\|take_read" crates/eml_vm/src std` → 0行
  - `grep -rn "一意に保ち" crates docs | grep -v docs/plans` の行が、どれも読みの貸しを述べる所 (レビューで読む)
  - `cargo build --release -p eml_cli` の後、`timeout 60 ./target/release/eml run bench/net/echo.em 200 16 reuse` → `echo done 200`。`bulk.em 4 64 shared fresh` → `bulk done 4194304`。`many.em 50` → `many done 50`
- 落とし穴: 書きの記録は buffer の参照を1つ持つので、`take_bytes` まで rc は 1 以上である。空の `Bytes` の書きは今どおり `NonNull::dangling` の位置と `cap == 0`。

### T-9: 1操作の確保を減らし、`POLL_EVERY` を比べる
- 目的: 1操作あたりの確保の残りを減らし、`POLL_EVERY` の値を決める (D-16、D-21)。
- 基づく決定: D-16、D-21
- 対象: `crates/eml_vm/src/{vm.rs,sched.rs,cells.rs}`、`std/Async.em` と `std/Net.em` (局所の組を消すとき)、`bench/net/busy.em`、`bench/net/cases.tsv`、`docs/plans/2026-10-29-io-fastpath.md` の「計測の記録」
- 作業
  - `collect` の `Vec::new()` を `Scheduler::runnables: Vec<Runnable>` の使い回しにする。
  - `bench/net` の echo-py-reuse と echo-self-reuse を `--stats` で流し、往復あたりの `allocations` を T-2b の表と比べる。残る確保を `--dump` と callgrind で見て、記録と std の包みの局所の組から消せるものを消す。消せない組は理由を「計測の記録」に書く。
  - 付録2 の8.4節の `busy.em` と `busy-self` の行を足す。release の eml で `busy.em 200` を1回流し、10 s を超えるなら8.4節のとおり往復の数を下げる。
  - `POLL_EVERY` を 1 にした release の eml (scratch で定数だけ替えたもの) と 16 の eml を `python3 -I bench/net/bench.py --only busy-self --runs 11 --strace-runs 3 one=<1 の eml> sixteen=<16 の eml>` で比べる。往復あたりの実時間が 16 で 50 µs 以上大きくなければ 16 を保つ。大きければ 1 にする。数字と決めた値を「計測の記録」に書く。
- テスト: 付録1 の7.8節。期待値を変えない。
- 実装者に任せること: 消す確保の選び方 (`RunStats` の値を変えないもの)
- 条件: いつも
- 完了の条件
  - 共通の条件
  - echo-py-reuse の往復あたりの `allocations` が T-2b の値より小さく、両方の値が「計測の記録」にある
  - 「計測の記録」に `busy-self` の 1 と 16 の比べの数字と、決めた `POLL_EVERY` がある
  - `grep -n "POLL_EVERY" crates/eml_vm/src/sched.rs` → 定数の行が1つ
- 落とし穴: 確保を減らす変更で `bench.rs` の snapshot が変われば止める (`bench/` のプログラムは `Async` と `Net` を使わないので、変わらないはずである)。`busy-self` の比べは時間の判定で、`minflt` の規則 (付録2 の4節) もそのまま当てる。

### T-10: 段を閉じる
- 目的: roadmap の「進め方」の閉じる手順を行い、2点目を benchmarks.md に書く (すべての決定)。
- 基づく決定: すべて
- 作業: 「進め方」の9つの手順の順に行う。この段で直すものは次のとおりである。
  1. `spec/` を仕上げる。段の名前と「今は〜、後で〜」の予告が残っていないことを確かめる。
  2. 計測する。この段は `crates/eml_vm`、`crates/eml_rt`、`std/`、`bench/` を変えるので対象である。
     - 2点目: `before=<T-2 の eml>`、`after=<T-9 の eml>` で `bench/net/bench.py --runs 11 --strace-runs 3 --stats` を T-2b と同じ機械で流し、benchmarks.md の「ネットワークの計測」に、T-2b の1点目と並べた2列の表 (列はコミット。user+sys、実時間、syscall、RSS、`allocations`)、syscall の目標の対応の表 (付録2 の7.2節の形)、T-9 の `POLL_EVERY` の比べの数字を置く。3列目は io-backends が足す。
     - 監視のスレッドの費用: 同じ比べで `base` と `compute-fib` の user+sys を前後で並べる。付録2 の4節の規則で「差」が出たら、数字を benchmarks.md とコミットのメッセージに書き、利用者に報告する (10 章の Q-1)。段は D-7 のまま閉じる。
     - 「今の数値」の3つの表は macOS でしか流せないので、T-11 に回す。
  3. docs/implementation/status.md: 速い道、即値の token、`write_bytes` の型、10 ms の上限は監視のスレッドのない run だけであること。
  4. docs/implementation/architecture.md: `eml_vm` の I/O の記録と `eml_io` の節が今の形であることを確かめる。
  5. roadmap: `io-fastpath` の項を消す。`io-backends` の「前提」から `io-fastpath` を外して「なし」にする。async.md の「未決の論点」の「替える順」の箇条から、終わった最初の手順 (trait と約束を compio の上で変えること) を消す。
  6. docs/overview.md の決定の表と docs/README.md の地図の「内容」の欄を、変わった所だけ直す。
  7. `CLAUDE.md` を確かめる (T-1 と T-2 で直してある)。
  8. 付録1 の8.9節の文 (ffi.md の完了の道、async.md の「受け付け、待ち、取り出し」の古い文) を直す。`docs/plans/2026-10-29-io-fastpath.md` と `docs/plans/2026-10-29-io-fastpath/` を消す。付録3と付録4は git の履歴に残り、`io-backends` と `net-windows` の最初のコミットが計画書を作るときに読む (`git show <このコミットの親>:docs/plans/2026-10-29-io-fastpath/03-io-backends.md`)。消す前に、`docs/` に残っていない決めがないことを確かめる。
  9. コミットのメッセージに、何をどこへ移したか、テストの変更の種類と理由、計測の目立つ差、後に回した項目、付録3と付録4を読むコミットの SHA を書く。
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件
  - 共通の条件
  - `grep -rn IDLE_TICK crates docs CLAUDE.md` → 0行
  - `grep -rn "Instant::now\|elapsed" crates/eml_vm/tests/io.rs` → 0行
  - `grep -n "受け箱の mutex" docs/future/ffi.md` → 0行
  - `git grep -n 'io-fastpath' docs CLAUDE.md` → 0行
  - benchmarks.md の「ネットワークの計測」に I/O の表が2列ある
  - `ls docs/plans` → 何もないか、ディレクトリがない
- 落とし穴: 2点目と1点目は同じ機械で取る。機械が違えば閉じずに止める。「今の数値」の3つの表はこのコミットの基準に入れない。benchmarks.md の表の列はコミットで名付け、段の名前を書かない (docs は段の名前を書かない)。

### T-11: 「今の数値」の3つの表
- 目的: roadmap の「進め方」の手順2のうち、macOS でしか流せない3つの表を入れ替える (D-1)。閉じるコミットの直後の別のコミットにする。
- 基づく決定: D-1
- 作業: T-10 の作業ツリーを macOS で取り出し、`bench/scripts.sh` を流す。範囲を外れた行があれば、先に `Resize bench rows` の別のコミットで大きさを変える。続けて `bench/run.sh`、`bench/rt.sh`、`bench/scripts.sh` を流し、benchmarks.md の「今の数値」の3つの表を入れ替える。macOS の機械を持つのは利用者なので、利用者が流す。
- 実装者に任せること: なし
- 条件: 利用者が macOS で流せないときは置かない。その旨を利用者に伝える。
- 完了の条件
  - benchmarks.md の3つの表が T-10 のコミットの数字になっている
  - `git diff --name-only HEAD~1` → `docs/implementation/benchmarks.md` だけ (`Resize bench rows` は別のコミット)
- 落とし穴: なし

## 8. 実行の順

T-0 から T-11 は1列に進める。T-3 は T-4 より前に置く (刻みを消す前に VM の上限を入れる)。T-6 は T-7 より前に置く (`with_capacity` を T-7 のテストが使う)。T-2b は T-3 より前に置く (1点目は driver と VM を変える前の数字である)。

```
T-0 → T-1 → T-2 → T-2b → T-3 → T-4 → T-5 → T-6 → T-7 → T-8 → T-9 → T-10 → T-11
```

## 9. 検証の計画

- 基点の確かめ (最初の変更の前): `nix develop -c cargo nextest run` → 失敗なし。通った数を T-0 のコミットのメッセージに書く。`git log --oneline | grep 'Close async-net'` → 1行。
- 全体の検証 (すべてのタスクの後)
  - 7 章の共通の条件、cross の clippy、`eml_rt` の関門
  - Miri の `eml_vm miri::` と `cells::a_read cells::a_write` の2つの設定 (T-8 のコマンド) → 通る
  - `nix build` → 成功
  - `cargo nextest run -j 1` → `SLOW` の印のあるテストがない
  - `cargo nextest run -p eml_vm --test integration -E 'test(io::one_task_of_finished_ops_never_waits_on_the_driver)'` → 通る (`immediate == 5`、`wait` 0、`parks == 5`、`completions == 5`)
  - `timeout 60 ./target/release/eml run bench/net/bulk.em 64 64 fresh accum` → 1 s 以内
  - `grep -rn IDLE_TICK crates docs CLAUDE.md`、`grep -n "HashMap<OpKey" crates/eml_vm/src`、`grep -n "OpToken\|OpDropped" crates/eml_vm/src` → どれも0行
  - GitHub の CI の `test` (3行)、`fmt`、`verus`、`miri` (2行) が緑
- 利用者が手で確かめること
  1. macOS か Linux で、`Net.listen "127.0.0.1" 0` で受けるサーバを書き、別の端末から `nc 127.0.0.1 <port>` でつなぎ、`read_line` と `write_line` の往復ができること。eml のファイルは scratch に置き、`timeout 20` の下で流す (速い道と1つの読みの上の `Std.Net`)
  2. 同じサーバの待ちの間に Ctrl-C を1回押すと、130 で終わること (刻みのない `wait` と監視のスレッドの起こし)
  3. macOS で T-11 の3つの表を流すこと

## 10. 危険と決めていない点

### 10.1 危険

| 危険 | 影響 | 備え |
|---|---|---|
| compio の `wait` は `reap` で保留の表を歩く | G-4 の「`wait` は表を歩かない」をこの段では満たさない | D-2 の G-4 の読みのとおり、確かめは io-backends から行う。この段の確かめは `completions` が OS を見ないことだけである。io-backends で `reap` ごと消える |
| UI テストの出力の順が、終わった操作を待たないことで変わる | T-4、T-5 | 種類2の範囲 (T-0)。成否が変われば止める |
| `Some(u64::MAX)` の `wait` を `Ok(false)` にする実装の誤り | シグナルを待つ run がデッドロックの誤りで止まる | trait の doc の順 (付録1 の1節)、`a_wait_with_no_ops_and_a_far_deadline_wakes`、`signal::a_waiter_keeps_an_otherwise_idle_run_alive` |
| 書きの貸しに一意を求めない規則の誤り | 書きの途中で中身が変わる | D-5 の論証、Miri の2本 (T-8)、`grep -rn "一意に保ち"` |

### 10.2 決めていない点

| ID | 項目 | 影響 | 決め方と時期 | 決まらないときの既定 |
|---|---|---|---|---|
| Q-1 | 監視のスレッドを run の始めに起こす費用 (`Normal` の単一タスクの run にスレッドの起こしと join と 100 Hz の刻みが足される) | T-10 | T-10 で `base` と `compute-fib` を前後で比べる | D-7 のとおり起こす。「差」が出たら数字を残して利用者に報告し、段は閉じる |
| Q-2 | `POLL_EVERY` の値 | T-9 | T-9 の `busy-self` の比べ。ただし横取りの集める地点はいつも OS を見るので、計算のタスクが1つの `busy-self` の往復は横取りの刻み (10 ms) で決まり、`POLL_EVERY` の差が出にくい (基点で 50 往復 2.1 s)。差が出なければ、走れる I/O のタスクが多い many-self で 1 と 16 の CPU を比べ、数字を計測の記録に残す | 16 |
| Q-3 | `busy-self` の往復の数 | T-9 | T-9 で `busy.em 200` の1回の実時間を測る。基点の API に直した同じプログラムは 50 往復で 2.1 s だった | 2000 と 200。1回の run が 10 s を超えれば、1〜3 s に収まる数に下げる (付録2 の8.4節) |
| Q-4 | `read_line` の `Vec` を空になっても持つか | 性能 | T-10 の計測で、行ごとに往復する接続の `Readable` と確保が目立ったとき | 放す (待つ接続に記憶を持たせない) |
| Q-5 | `read_line` の周の数は、バイトの届き方で変わる | テストの決定性 | 今と同じ性質 | testing.md の「seed で流す」に、`async/` のファイルは周がプログラムで決まるように書くと書く (T-5) |
| Q-6 | 監視のスレッドを `Machine` が run をまたいで持つか | REPL の入力ごとの費用 | REPL で費用が目立ったとき | run ごと |
| Q-7 | 速い道の `reschedule` が `Cancelled` で戻ったときの `cancels` の数え方 (1つの取り消しが2回数えられうる) | `RunStats` | `cancels` はもとからスケジュールに依る欄で、比べない | そのまま。vm.md の `cancels` の項に「速い道の `reschedule` も数える」と書く (T-5) |
| Q-8 | `a_second_take_of_an_op_is_an_internal_error` を LIR の本文で書けるか | T-5 | T-5 の実装で見る | 書けなければ `cells.rs` の単体テストにし、計画書に書く |
| Q-9 | `Bytes.of_string` の写しが `alloc_bytes` と同じ切り上げを使うか | T-7 の「UI の期待値を変えない」 | T-7 で `bytes_lend` の `cap` を読んで確かめる | 違って UI の読みが変われば止め、計画書に種類2の範囲を足す |
| Q-10 | `bench/net` の macOS と Windows | 計測 | macOS は時間と RSS と `RunStats` だけ。Windows は net-windows の後 | 流さない |
| Q-11 | many-self の RSS が本数とともに段になる理由 | io-backends の driver の表の形 | T-10 と io-backends の3点目の計測で見る | 推論のまま |
| Q-12 | 1点目と同じ形の Linux の機械が T-10 で使えるか | T-10 | 4 章の確かめ | 閉じずに止めて利用者に報告する |
| Q-13 | 核の関数を `fn(&mut Worker, &mut Ctx, …)` の形にすること (D-8 の G-22) | ネイティブの入り口 | 型 `Worker` を足す `worker-threads` の段で行う。この段に作業は置かない | この段では、`cells.rs` の核が std と `eml_rt` だけを import し、driver を `Ctx` に置かないことだけを確かめる (`crates/eml_vm/tests/tasks.rs` の import の制限のテストと、`git grep -n IoDriver crates/eml_rt/src -- ':!driver.rs' ':!lib.rs'` が0行) |

## 計測の記録

T-2b、T-9 の数字をここに書く。T-10 が benchmarks.md に移してから、この文書ごと消す。

- 1点目 (T-2b): 次のとおり。`python3 -I bench/net/bench.py --runs 11 --strace-runs 3 --stats before=./eml-t2` の出力を `report.py` に通した表を写した。全体で約 5.5 分かかった。
  - 機械: Intel(R) Xeon(R) Processor @ 2.10GHz、4 vCPU (`nproc`)、メモリ 16477352 kB (`/proc/meminfo` の MemTotal。約 15.7 GiB)
  - kernel: 6.18.44-fc-v185 (`uname -r`)
  - コミット: 34cad91 (T-2、`Add bench/net`) の release の eml
  - 単位あたりの値は、大きい run と小さい run の差を単位の数の差で割った値である。時間の表の「単位」の列は大きい run の単位の数で、syscall と `RunStats` の表の「単位」の列は小さい run の単位の数である。
  - macOS がないので `bench/scripts.sh` は取っていない。
  - `base` は床の run である。`compute-fib` は床より速いので、単位あたりの値が負になる。この2行は比べに使わない。
  - `busy-self` はまだないので、この表にない。

  #### 時間: before (中央値。括弧は最小-最大。単位あたりは床を引く。µs)

  | 件 | 単位 | 実時間 s | CPU s (四分位) | 単位あたり 実時間 | 単位あたり CPU | 単位あたり sys | 最大 RSS MiB | minflt/単位 | 自発/非自発 |
  |---|---|---|---|---|---|---|---|---|---|
  | base | 1 run | 0.025 (0.020-0.034) | 0.024 (0.021-0.025) | 24633.56 | 23860.00 | 3950.00 | 12.7 | 1240.00 | 2/0 |
  | compute-fib | 1 run | 0.014 (0.012-0.018) | 0.013 (0.013-0.014) | -10682.03 | -10510.00 | -500.00 | 12.3 | -732.00 | 1/0 |
  | echo-self-fresh | 20000 往復 | 0.547 (0.403-0.683) | 0.531 (0.461-0.550) | 26.12 | 25.37 | 10.59 | 13.0 | 0.00 | 53/9 |
  | echo-self-reuse | 20000 往復 | 0.486 (0.450-0.624) | 0.486 (0.462-0.511) | 23.06 | 23.12 | 9.99 | 13.0 | 0.00 | 48/8 |
  | echo-py-fresh | 20000 往復 | 0.797 (0.695-0.882) | 0.715 (0.700-0.743) | 38.63 | 34.55 | 18.89 | 13.0 | 0.00 | 3523/46 |
  | echo-py-reuse | 20000 往復 | 0.787 (0.683-0.867) | 0.696 (0.681-0.754) | 38.11 | 33.62 | 19.42 | 12.9 | 0.00 | 4354/46 |
  | bulk-self | 1024 MiB | 0.404 (0.342-0.491) | 0.391 (0.356-0.458) | 370.06 | 358.07 | 181.03 | 13.1 | 0.09 | 40/8 |
  | bulk-shared | 1024 MiB | 0.384 (0.323-0.430) | 0.376 (0.346-0.421) | 350.85 | 343.49 | 167.49 | 13.2 | 0.11 | 38/7 |
  | bulk-reuse | 1024 MiB | 0.360 (0.289-0.429) | 0.352 (0.344-0.415) | 327.86 | 320.04 | 169.33 | 13.1 | 0.09 | 35/7 |
  | bulk-1k | 64 MiB | 0.808 (0.680-1.209) | 0.787 (0.706-0.830) | 12239.59 | 11929.50 | 4971.56 | 13.1 | 0.97 | 78/17 |
  | bulk-send-py | 1024 MiB | 0.632 (0.545-0.687) | 0.622 (0.562-0.646) | 593.39 | 584.51 | 410.71 | 13.0 | 0.08 | 63/37 |
  | bulk-recv-py | 1024 MiB | 0.767 (0.614-0.991) | 0.527 (0.441-0.557) | 725.17 | 491.53 | 260.71 | 13.0 | 0.08 | 12008/2 |
  | bulk-accum | 16 MiB | 1.094 (1.043-1.400) | 1.093 (1.039-1.178) | 66843.96 | 66834.50 | 56497.81 | 44.9 | 33198.56 | 108/2 |
  | many-self-250 | 250 接続 | 0.107 (0.086-0.137) | 0.069 (0.062-0.077) | 331.45 | 181.43 | 83.84 | 19.4 | 6.88 | 13/0 |
  | many-self-1000 | 1000 接続 | 0.383 (0.333-0.431) | 0.308 (0.281-0.350) | 358.38 | 283.91 | 89.53 | 52.9 | 10.38 | 42/5 |
  | many-self-2000 | 2000 接続 | 1.085 (0.879-1.367) | 0.998 (0.949-1.200) | 530.22 | 487.11 | 128.35 | 222.8 | 27.05 | 113/15 |
  | many-py-1000 | 1000 接続 | 0.159 (0.132-0.183) | 0.110 (0.097-0.116) | 133.91 | 85.73 | 39.34 | 25.9 | 3.45 | 19/1 |
  | conn-self-1 | 10000 接続 | 0.436 (0.323-0.557) | 0.426 (0.388-0.512) | 41.13 | 40.26 | 24.78 | 13.1 | 0.01 | 43/59 |
  | conn-self-32 | 10240 接続 | 0.471 (0.390-0.551) | 0.458 (0.416-0.508) | 43.55 | 42.36 | 22.09 | 13.1 | 0.01 | 46/58 |
  | conn-py | 10000 接続 | 0.594 (0.562-0.686) | 0.574 (0.538-0.628) | 56.90 | 55.04 | 35.09 | 13.0 | 0.01 | 541/74 |

  #### 時間: python (中央値。括弧は最小-最大。単位あたりは床を引く。µs)

  | 件 | 単位 | 実時間 s | CPU s (四分位) | 単位あたり 実時間 | 単位あたり CPU | 単位あたり sys | 最大 RSS MiB | minflt/単位 | 自発/非自発 |
  |---|---|---|---|---|---|---|---|---|---|
  | py:echo | 20000 往復 | 0.871 (0.749-0.947) | 0.453 (0.410-0.473) | 43.55 | 22.67 | 17.97 | 12.3 | 0.07 | 19922/41 |
  | py:bulk | 1024 MiB | 0.506 (0.431-0.624) | 0.499 (0.475-0.534) | 494.53 | 487.31 | 440.62 | 12.3 | 1.31 | 2/32 |
  | py:conn | 10000 接続 | 0.803 (0.761-0.865) | 0.519 (0.513-0.569) | 80.30 | 51.93 | 36.23 | 12.3 | 0.13 | 9974/50 |

  #### syscall: before (strace -f、main のスレッド、床を引いて単位あたり。複数回なら最小-最大)

  | 件 | 単位 | epoll_ctl | epoll_pwait | epoll_wait | timerfd_settime | read(eventfd) | recvfrom | recvfrom!EAGAIN | sendto | accept4 | accept4!EAGAIN | connect | brk | mmap | munmap | main の計 | ほかのスレッドの計 |
  |---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
  | compute-fib | 1 run | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | -23.00 | -1.00 | 0 | -67.00 | -23.00 |
  | echo-self-fresh | 2000 往復 | 12.00 | 4.00 | 0 | 4.00 | 4.00 | 2.00 | 0 | 2.00 | 0 | 0 | 0 | 0.01 | 0 | 0 | 32.02 | 0.52-0.63 |
  | echo-self-reuse | 2000 往復 | 12.00 | 4.00 | 0 | 4.00 | 4.00 | 2.00 | 0 | 2.00 | 0 | 0 | 0 | 0.01 | 0 | 0 | 32.02 | 0.55-0.58 |
  | echo-py-fresh | 2000 往復 | 4.01-4.04 | 1.01-1.02 | 0 | 1.01-1.02 | 1.01-1.02 | 1.00 | 0 | 1.00 | 0 | 0 | 0 | 0.01 | 0 | 0 | 10.05-10.13 | 0.19-0.21 |
  | echo-py-reuse | 2000 往復 | 4.02 | 1.01 | 0 | 1.01 | 1.01 | 1.00 | 0 | 1.00 | 0 | 0 | 0 | 0.01 | 0 | 0 | 10.07-10.08 | 0.18-0.21 |
  | bulk-self | 100 MiB | 64.08 | 16.02 | 0 | 16.02 | 16.02 | 16.01 | 0 | 16.00 | 0.01 | 0 | 0.01 | 0.17 | 0 | 0 | 160.43 | 2.97-3.24 |
  | bulk-shared | 100 MiB | 64.08 | 16.02 | 0 | 16.02 | 16.02 | 16.01 | 0 | 16.00 | 0.01 | 0 | 0.01 | 0.17 | 0 | 0 | 160.43 | 2.85-3.45 |
  | bulk-reuse | 100 MiB | 64.08 | 16.02 | 0 | 16.02 | 16.02 | 16.01 | 0 | 16.00 | 0.01 | 0 | 0.01 | 0.17 | 0 | 0 | 160.43 | 3.09-3.39 |
  | bulk-1k | 16 MiB | 4096.50 | 1024.12 | 0 | 1024.12 | 1024.12 | 1024.06 | 0 | 1024.00 | 0.06 | 0 | 0.06 | 0.94 | 0 | 0 | 10242.56 | 192.00-194.62 |
  | bulk-send-py | 100 MiB | 0.04 | 0.01 | 0 | 0.01 | 0.01 | 0 | 0 | 16.00 | 0 | 0 | 0.01 | 0.14 | 0 | 0 | 16.21 | 0.45-0.51 |
  | bulk-recv-py | 100 MiB | 72.36-76.28 | 18.14-19.07 | 0 | 18.14-19.07 | 18.14-19.07 | 18.03-19.06 | 0 | 0 | 0 | 0 | 0.01 | 0.14 | 0 | 0 | 163.08-171.75 | 3.12-3.75 |
  | bulk-accum | 16 MiB | 64.62-64.75 | 16.19-16.25 | 0 | 16.19-16.25 | 16.19-16.25 | 16.06 | 0 | 16.00 | 0.06 | 0 | 0.06 | 0.94 | 16.00 | 16.00 | 194.94-195.31 | 26.62-28.81 |
  | many-self-250 | 250 接続 | 24.00 | 8.00 | 0 | 8.00 | 8.00 | 3.00 | 0 | 2.00 | 1.00 | 0 | 1.00 | 1.06 | 0 | 0 | 71.06 | 1.33-1.57 |
  | many-self-1000 | 1000 接続 | 24.00-25.13 | 8.00-8.57 | 0 | 8.00-8.57 | 8.00-8.57 | 3.00 | 0 | 2.00 | 1.00 | 0 | 1.00 | 1.03 | 0 | 0 | 71.04-74.44 | 1.35-1.58 |
  | many-self-2000 | 2000 接続 | 24.00-27.68 | 8.00-8.99 | 0 | 8.00-8.99 | 8.00-8.99 | 3.00 | 0 | 2.00 | 1.00-1.85 | 0-0.85 | 1.00 | 1.05-1.05 | 0 | 0 | 71.06-80.40 | 1.56-1.69 |
  | many-py-1000 | 1000 接続 | 8.00 | 2.00 | 0 | 2.00 | 2.00 | 1.00 | 0 | 1.00 | 0 | 0 | 1.00 | 0.52 | 0 | 0 | 25.51 | 0.47-0.52 |
  | conn-self-1 | 1000 接続 | 13.99 | 4.00 | 0 | 4.00 | 4.00 | 1.00 | 0 | 0 | 2.00 | 1.00 | 1.00 | 0.01 | 0 | 0 | 41.99 | 0.75-0.78 |
  | conn-self-32 | 1024 接続 | 10.06 | 3.03 | 0 | 3.03 | 3.03 | 1.00 | 0 | 0 | 1.00 | 0 | 1.00 | 0.01 | 0 | 0 | 32.19 | 0.53-0.62 |
  | conn-py | 1000 接続 | 8.00-8.02 | 2.00-2.01 | 0 | 2.00-2.01 | 2.00-2.01 | 1.00 | 0 | 0 | 0 | 0 | 1.00 | 0.01 | 0 | 0 | 24.01-24.07 | 0.45-0.46 |

  #### RunStats: before (--stats、床を引いて単位あたり。床: parks=1, completions=1, tasks=1, allocations=34, string_bytes_copied=12, rc_increments=18, rc_decrements=51, preemptions=0)

  | 件 | 単位 | parks | completions | tasks | allocations | string_bytes_copied | rc_increments | rc_decrements | preemptions |
  |---|---|---|---|---|---|---|---|---|---|
  | compute-fib | 1 run | -1.00 | -1.00 | -1.00 | -32.00 | -7.00 | -18.00 | -50.00 | 0 |
  | echo-self-fresh | 2000 往復 | 4.00 | 6.00 | 0 | 76.06 | 16.02 | 44.03 | 120.08 | 0 |
  | echo-self-reuse | 2000 往復 | 4.00 | 6.00 | 0 | 74.06 | 0.03 | 46.03 | 120.09 | 0 |
  | echo-py-fresh | 2000 往復 | 2.00 | 3.00 | 0 | 42.02 | 16.04 | 22.00 | 64.02 | 0 |
  | echo-py-reuse | 2000 往復 | 2.00 | 3.00 | 0 | 40.02 | 0.04 | 23.00 | 63.02 | 0 |
  | bulk-self | 100 MiB | 32.04 | 48.04 | 0.01 | 609.34 | 1048576.91 | 352.51 | 961.85 | 0 |
  | bulk-shared | 100 MiB | 32.04 | 48.04 | 0.01 | 609.35 | 656.32 | 368.51 | 977.86 | 0 |
  | bulk-reuse | 100 MiB | 32.04 | 48.04 | 0.01 | 577.34 | 1048576.91 | 352.51 | 929.85 | 0 |
  | bulk-1k | 16 MiB | 2048.25 | 3072.25 | 0.06 | 38920.38 | 1048580.94 | 22531.19 | 61451.56 | 0 |
  | bulk-send-py | 100 MiB | 16.00 | 16.00 | 0 | 304.60 | 1048577.16 | 176.00 | 480.60 | 0 |
  | bulk-recv-py | 100 MiB | 20.91 | 41.82 | 0 | 418.79 | 1.21 | 230.01 | 648.80 | 0 |
  | bulk-accum | 16 MiB | 32.25 | 48.25 | 0.06 | 600.38 | 1048581.25 | 355.19 | 955.56 | 0.62 |
  | many-self-250 | 250 接続 | 7.01 | 10.00 | 2.00 | 165.36 | 16.04 | 99.21 | 264.56 | 0 |
  | many-self-1000 | 1000 接続 | 7.00 | 10.00 | 2.00 | 165.09 | 16.01 | 99.05 | 264.14 | 0 |
  | many-self-2000 | 2000 接続 | 7.00 | 10.00 | 2.00 | 165.04 | 16.01 | 99.03 | 264.07 | 0 |
  | many-py-1000 | 1000 接続 | 3.00 | 4.00 | 1.00 | 72.02 | 16.02 | 40.01 | 112.03 | 0 |
  | conn-self-1 | 1000 接続 | 3.00 | 4.00 | 0 | 58.05 | 0.02 | 33.02 | 91.07 | 0 |
  | conn-self-32 | 1024 接続 | 3.00 | 4.00 | 0.03 | 56.57 | 0.02 | 33.25 | 89.82 | 0 |
  | conn-py | 1000 接続 | 2.00 | 3.00 | 0 | 40.01 | 0.04 | 21.99 | 62.00 | 0 |
- `busy-self` の比べと `POLL_EVERY` (T-9): 次のとおり。`POLL_EVERY` を 16 に保つ。
  - 機械と kernel は1点目と同じである。eml は e7e0df1 の上に T-9 の変更を当てた release の eml で、`one` は `POLL_EVERY` だけを 1 にしたもの、`sixteen` は 16 のままのものである。
  - `busy.em 200` の1回の実時間は 8.1 s だった。10 s を超えないので、往復の数は 2000 と 200 のままにした。2000 往復の run は1回約 80 s かかり、`python3 -I bench/net/bench.py --only busy-self --runs 11 --strace-runs 3 one=... sixteen=...` の全体で約 30 分かかった。
  - この run は `base` を流していないので、単位あたりの値は大きい run と小さい run の差から出している。

  | ラベル | 実時間 s | CPU s (四分位) | 1往復の実時間 µs | 1往復の CPU µs | 1往復の sys µs | 最大 RSS MiB | minflt/往復 | 自発/非自発 |
  |---|---|---|---|---|---|---|---|---|
  | one | 80.165 (80.095-80.267) | 79.570 (79.535-79.655) | 40082.26 | 39784.87 | 29.17 | 13.0 | 0.65 | 8011/130 |
  | sixteen | 80.205 (80.094-80.519) | 79.623 (79.532-79.685) | 40102.63 | 39811.65 | 15.52 | 13.0 | 0.65 | 8010/113 |

  - 比べの表: CPU の比 1.00、四分位は重なり、判定は「差なし」。実時間の比は 1.00。`minflt` は同じなので、確保の揺れの印は付かない。
  - syscall (strace -f の 200 往復の run の main のスレッド。床を引かず、総数を 200 で割った値。3 回とも同じか 0.03 以内): `one` は epoll_pwait 4.02、epoll_ctl 12.08、timerfd_settime 4.02、main の計 37.49-37.58。`sixteen` は epoll_pwait 2.01、epoll_ctl 8.06、timerfd_settime 2.01、main の計 25.42-25.43。recvfrom 4.00 と sendto 2.00 は両方で同じ。
  - 決めた値: 1往復の実時間は 16 のほうが 20.37 µs 大きいが、50 µs に届かないので `POLL_EVERY = 16` を保つ。1往復の約 40 ms は横取りの刻み (10 ms) 約4回分で、OS を見る頻度ではほとんど変わらない (10.2 節の Q-2 の見込みどおり)。
  - 差が出なかったので、走れる I/O のタスクが多い many-self でも 1 と 16 を比べた (`--only '^(base|many-self-.*)$' --runs 11 --strace-runs 3`)。CPU (床を引いて1接続あたり µs) は、250 本で 222.54 → 138.55 (比 0.62)、1000 本で 454.96 → 155.61 (0.34)、2000 本で 847.05 → 237.19 (0.28) で、どれも四分位が重ならない。ただし `minflt` の1接続あたりの値が 3.75 → 4.75、36.22 → 9.48、53.52 → 18.11 と 1 以上違うので、比べの規則 (付録2 の4節) では3件とも確保の揺れとして時間の判定から外す。RSS も 1000 本で 153.5 MiB → 49.3 MiB と違った。syscall は規則の外で読める: 1接続あたりの epoll_pwait は 5.66-6.39 → 1.19、epoll_ctl は 20.64-22.12 → 11.69-11.78、main の計は 59.28-63.67 → 32.29-32.53 で、16 のほうが OS を見る回数が少ない。どちらの比べも 16 を保つ判断と合う。
- echo-py-reuse の往復あたりの `allocations` (T-2b と T-9)
  - T-2b (1点目、34cad91): 40.02 (1往復あたり。床を引いた値)。
  - T-9 (e7e0df1 の上に T-9 の変更を当てた release の eml、`--stats` を1回): 21.11。T-2b の 40.02 より小さい。同じ eml の前の run では 20.85 だった。Python の相手との間で、読みが受け付けで終わるかどうかで揺れる。同じ run の echo-self-reuse は 38.05、echo-self-fresh は 43.05、echo-py-fresh は 24.31 だった。
  - `allocations` (検査モードのヒープの確保) は T-9 の変更では動かない。40.02 からの減りは T-5 から T-8 の分である。T-9 は Rust の側の確保を減らした。速いモードの release の eml で echo.em の reuse を 200 往復と 2200 往復で callgrind に通し、差を 2000 で割ると、1往復の `malloc` は 12、`realloc` は 2 だった。`collect` の列と、エントリの `Task` の scratch (待ちから戻るたびに空の `Vec` から伸びていた。1往復 4) を使い回すと、`malloc` が 10、`realloc` が 0 になった。
  - 残した Rust の側の確保 (1往復 10): compio の `Proactor::push` の鍵の箱 (Send と Recv で 2 ずつ)、`polling::epoll::Events::with_capacity` (poll ごと、2)、`eml_io::real` の操作の表 `HashMap<u64, Entry>` の `reserve_rehash` (2)、compio の `VecDeque<ErasedKey>` の伸び (2)。どれも compio の driver とその包みの中にあり、io-backends の差し替えで driver ごと替わるので、この段では触らない。
  - 残したヒープの確保: 速い道 (受け付けで終わる操作) の std の包みは、LIR で組がすべて構造体に下りていて、確保がない。待つ道の確保は次のとおりで、どれも std の包みの局所の組ではない。(1) yield で継続を組む `compose$` と各フレームの継続の閉包。フレームの深さに比例し、エフェクトの実装の形で決まる。(2) ハンドラの節が `park_io` に渡す `$cont$` の閉包。(3) 再開した継続が一様な位置で返す `(Bytes, TcpStream)` の箱。(4) `resume_io` が継続に渡す `(Wake, OpId)` の組 (1回の待ちに1つ)。2つともスカラーだが、継続の引数は一様な位置なので箱になる。消すには `await_io` の結果の型と、ランタイムのエントリ `resume_io` の形 (`eml_extern` の表と VM) を変える必要があり、局所の組を消す範囲を超えるので残した。

## 付録の外の記録: 検討で決まったこと

蒸し返さないために、検討の中で決まったことだけを書く。

- 利用者の決定
  - 見直しの範囲は4つすべてである。driver と `eml_rt::IoDriver` の trait、VM の I/O の約束 (三段、登録表、`Readable` と `Read`、タスクの切り替え)、std の `Net` の API と `Bytes` の貸し方、ネイティブ化の準備 (driver の置き場所、`Async` の入り口) を見直した。
  - ぶつかったときの優先は性能である。簡潔な実装、性能、ネイティブ化への準備のためなら、これまでに決めた言語機能と VM と std の spec を変えてよいとした。後方互換は考えない。
  - 設計の前に、本物の I/O の物差しと今の数字 (付録2) を先に立てた。数字は、費用が2か所にあることを示した。保留が多い場面は compio の上の `reap` で、これは driver の移行が消す。1操作ごとの費用は約束の形 (待つ読みの 8 回の syscall、終わっている操作の perform と2回の切り替え、1往復 42 個の確保、共有の `Bytes` の2回の写し、足し続ける読みの2乗の伸び) で、これは VM と std の約束の変更が消す。
  - 前回の計画 (`io-backends` を先にし、VM と std の約束を変えない) は、その数字と方針の見直しで、この計画に置き換えた。前回の計画書はこの段の T-0 で消し、その中身のうち今も成り立つものは付録3と付録4に移した。
  - 前回の利用者の決定のうち、次のものを前提として保つ。`compio-driver` を外して自前の driver にする。aarch64-linux (Apple Silicon の Mac の上の Linux コンテナ) を対象に足す。macOS の通常のファイルの読み書きはプールの仕事にする。
  - 壊れた driver の後始末は、前回の2回の裁定を経て「1 秒で終わらなければ abort し、`Box::leak` と `mem::forget` は書かない」に決まり、利用者が承認した。1回目の裁定は「1 秒の確かめの後に、終わらない箱を `Box::leak` で残す」だった。その後、run からの panic の巻き戻しでは driver が `quiesce` を通らずに `Scheduler` の中で落ち、poisoned でない `Machine` がヒープを放す道が見つかった。この道では箱だけを漏らしても使用後解放になるので、abort を選んだ。この決定を前提として保つ。
  - backend の範囲 (io_uring を入れるか) と段の分け方は、設計に任せた。設計は、io_uring を試作と計測を最初のタスクに持つ段 `io-uring` に分け、段の順を `io-fastpath`、`io-backends`、`net-windows`、`io-uring` にした。
  - メモリ安全は設計の検討で確かめさせ、書きの貸しに一意を求めない規則 (D-5) を採った。その場で書き換える操作はどれも一意を求めるので、書きの記録が参照を1つ持つ間はほかの参照から書き換わらない、という論証である。
- 設計で退けた案 (D-1 から D-21 に書いたもののほか)
  - 三段 (受け付け、待ち、取り出し) を1つの extern か VM の命令にする案: 取り消しの地点と予算の規則が VM と std に割れ、ネイティブのコードの待ちの形 (節が継続を預けて戻る) と違う道ができる。
  - `Readable` を消し、読みをいつも1つの操作にする案: 空きのない buffer の読みで、何も来ない接続に 64 KiB を持たせることになる。
  - 鍵から記録への表を登録表の中に driver の状態として置く案: 鍵を記録の番号にすれば表が要らない。
  - `completions` が OS を見る頻度を driver が持つ案 (`POLL_EVERY` を driver の中に置く): 計算の重い worker で完了が最大 16 刻み遅れ、driver が VM の呼び方に結び付く。
  - `Readable` を readiness の記憶だけで答える案: 読み尽くした後の `Readable` が `Done` になり、状態の問いの契約に反し、待つ接続が 64 KiB を持つ。backend は記憶が立っていれば覗いて確かめる。
