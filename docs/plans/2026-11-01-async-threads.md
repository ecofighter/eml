# `async-threads` の設計と計画

- 作成: 2026-11-01
- 基準: `ccr-754c3b4b-mgsd3w` @ `2c8de78` (この文書と付録の行番号は、すべてこのコミットの内容を指す)
- 状態: 計画は確定。Q-1 と Q-2 は利用者が計画のとおりに決めた (2026-11-01)

この文書は本文と付録5つからなる。本文は、決めたこと、単位の順番、完了の条件を書く。細部の根拠、手順、完了のコマンドは付録に英語で書いてある。付録は設計の途中の成果物を収めたもので、そこに出てくる作業用のディレクトリは、実装のときにはもうない。`probes/` だけは付録と一緒に置いてある。

| 付録 | 中身 | 本文での呼び名 |
|---|---|---|
| [s1-concurrency.md](2026-11-01-async-threads/s1-concurrency.md) | worker をまたぐ `Async` の並行の模型。登録表と lock、セルの持ち主、知らせと mail、置き方、driver と受け付けの転送、仮想の時計、シグナル、run の終わり。基準 G-A1〜G-A20。設計の文書の書き換え 1〜10 | S1 |
| [s2-chan-counter.md](2026-11-01-async-threads/s2-chan-counter.md) | worker をまたぐ `Std.Chan` (予約の数と worker ごとの hold) と、lock を取らない `Std.Atomic.Counter` | S2 |
| [s3-tests-measure.md](2026-11-01-async-threads/s3-tests-measure.md) | UI テストのハーネス、ファイルの分類、Rust のテスト、CI、Linux の計測、testing.md と roadmap の文 | S3 |
| [s4-integration.md](2026-11-01-async-threads/s4-integration.md) | S1〜S3 を単位 A0〜A14 と C に並べたもの。単位ごとの完了のコマンド、文書の文の持ち分、テストの変更の一覧 | S4 |
| [x-ruling.md](2026-11-01-async-threads/x-ruling.md) | G-A6、G-A15、G-A16 の読み方の裁定と、書き換えた文 | 裁定 |

付録どうしが食い違うときは、本文 > S4 の 1.4 節 > 裁定 > S4 のほかの節 > S1 > S2 と S3 の順に優先する。S4 の 1.4 節は、S1 を書き換えた文をまとめた所である。書き換えたのは、置き始める worker と `placed_tasks` の定義 (S4 では AM-1、AM-2。下の D-4) と、裁定の文である。

## 1. 目的と完了の条件

- 目的: `Async` のタスクを worker 0 のほかの worker でも動かす。タスクは `start` のときに置く worker を決め、そこから動かない。channel と `Counter` は worker をまたいで使え、本物の時計の run では各 worker が自分の I/O の driver を持つ。後の段 (`async-rescue`、`task-limits`、`main-pinning`、`async-compute`) は、この段の登録表と知らせの模型の上に作る
- 完了の条件: 9 章の全体の確認がすべて通ること。特に次の3つである
  - `async/` の UI テスト (`run` 69 本、`run-fail` 12 本) が、N = 2 と 8 でも N = 1 と同じ出力と終わり方を返す。例外は、S4 の 5 章の K2-d の範囲で印を付けたファイルだけである
  - N = 1 の出力、UI テストの snapshot、`RunStats` の snapshot (`crates/eml_vm/tests/bench.rs` を含む) が、どの単位でもバイト単位で変わらない
  - Linux の `bench/async/scale.sh` で、worker の数に応じて速くなることを確かめる (段を閉じる基準は 10 章の Q-1)

## 2. 範囲

- 範囲に入るもの
  - `eml_vm`: run に1つの登録表と lock、セルの持ち主、知らせの統合と run の mail、どの worker でも同じ集める地点、仮想の時計の全体の静止、`Counter` の atomic の表、channel の予約の数と hold、受け付けの転送 (`Submit`、`CancelIo`、`Completion`)、タスクの置き方、run の終わりの順番、本物の時計での worker ごとの driver、シグナルの届け方
  - `eml_io`: run の時間の基準を共有する driver の作り方 (`RealDriver::new_at`、`TestDriver::new_shared`)
  - `eml_rt`: `RawBuf` を `Send` にすること (`verus!` の外、信頼の一覧の行は増えない)。A14 が入るときは `RawSocket` と `IoDriver::adopt`
  - `eml_cli`: `async/` の UI テストを N = 2 と 8 で流すこと、印 `-- workers: skip (<理由>)`、CLI のテスト
  - `bench/`: `bench/async/` の計測のプログラムと `scale.sh`、`bench/net/` の worker の数の指定と新しい場合
  - CI: TSan と ASan の対象 (`io::`、`chan::`、`counters::` など)
  - 文書: `docs/spec/` の該当の節、multicore.md と async.md の書き換え 1〜11、testing.md、benchmarks.md、roadmap、status.md、architecture.md、`CLAUDE.md`
- 範囲に入らないもの (どの段に回すか)
  - `compute` (新しい段 `async-compute`。A1 で roadmap に足す)
  - 救済、`--sched rescue-always`、`share` の送り手の引数 (`async-rescue`)
  - タスクごとの上限 (`task-limits`)、deque と盗み (`par-steal`)、専用の worker とメインの worker (`main-pinning`)
  - `eml_sched` の crate、REPL の複数の worker、ロックフリーの受け箱と channel
  - `tls/`、`http/`、`std/` の UI テストを N ≥ 2 で流すこと (roadmap の「その後の項目」に回す。S3 の 5.7 節)

## 3. 背景

今のコードについて、実装で前提にする事実を挙げる。根拠の行番号は付録にある。

- `Async` のタスク、channel、`Counter`、I/O の driver は worker 0 だけにある。worker 1..N-1 は driver も登録表も使わず、driver を求めると内部の誤り (`NO_DRIVER`) になる (`crates/eml_vm/src/sched.rs:222-233`、`docs/spec/vm.md:590`)
- 登録表 `Registry` は run ごとに `Scheduler.cells` が持ち、`&mut` で触れる。lock はない (`crates/eml_vm/src/cells.rs:658-680`)
- 知らせは2種類ある。worker をまたぐ `workers::Notice` (`Child`、`Joined`) と、worker 0 の登録表の受け箱 `cells::Notice` (`HandleDropped`、`Cancel`、`Signal`) である (`workers.rs:33`、`cells.rs:459`)
- `Counter` の表は `Vec<i64>` で、`Machine` から run へ移し、run の後に戻す (`lib.rs:329, 593, 893, 1011`)
- 仮想の時計を進めるのは worker 0 だけで、ほかのすべての worker が眠っているときに限る (`workers.rs:566-569`)
- 本物の driver の `wake()` はどのスレッドからも呼べるが、受け付けは driver を作ったスレッドからしか出せない。各 backend は作ったときの `Instant::now()` を時刻の起点にする (`crates/eml_io/src/sys/unix.rs:460`、`sys/iocp.rs:464`)
- `par` の子の row は `<never e>` で、`Async`、`Chan`、`Atomic`、`Net`、`Signal` の extern はどれも `<IO>` なので、`par` の子からは届かない (`std/Par.em:4-6`、`std/Atomic.em:10-14`)
- UI のハーネスは `par/` だけを N = 2 と 8 で流し、`async/` を外している。外すことを `ui::workers_runs_cover_par_only` が固定している (`crates/eml_cli/tests/ui.rs:277-284, 620`)
- `compute` はどこにも実装がない (`docs/future/async.md:366-377` の設計だけがある)
- worker の数で速さを比べた記録はない。`bench/run.sh` は `EML_WORKERS=1` に固定している
- 今の 4 vCPU の VM での目安 (S3 の実測、release): 同じ仕事を N 個の1 worker のプロセスで分けると t(1) 926 ms、t(2) 487 ms、t(4) 285 ms。`par` で分けると 853 ms、512 ms、490 ms で、N = 4 で伸びが止まる

## 4. 前提の環境

| 要るもの | 用途 | 確かめ方 | ないとき |
|---|---|---|---|
| nix の devShell | すべて | `nix develop -c sh -c 'cargo nextest --version && cargo insta --version && which verus verusfmt'` → どれも版が出る | `nix/prefetch-inputs.sh` を流してからやり直す。だめなら止めて利用者に聞く |
| `.#miri` の shell | Miri、ASan、TSan | `nix develop .#miri -c cargo miri --version` → 版が出る | 止めて利用者に聞く |
| `.#cross` の shell | Windows と Linux の clippy (A8、A9、A11、A14) | `nix develop .#cross -c cargo clippy --version` → 版が出る | 止めて利用者に聞く |
| actionlint | ci.yml を変える単位 | `nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- --version` → 版が出る | 止めて利用者に聞く |
| python3、`lscpu`、空いた物理コア4つ以上の Linux | A12〜A14 の計測 | `python3 --version; lscpu` | A13 は判定できない。数値を記録して利用者に聞く |
| macOS の機械 | 段を閉じた後の `bench/run.sh`、`bench/rt.sh`、`bench/scripts.sh` | — | 利用者が `Measure async-threads` のコミットで行う (roadmap の「進め方」の手順 2) |

## 5. 決めたこと

### 5.1 長期の指針 (変えるには検討し直しが要る)

実装がこれらとぶつかったら、止めて報告する。各指針の基準 (MUST / MUST NOT と確かめ方) は S1 の 8 章にあり、S4 の 1.4 節と裁定で書き換えた文が優先する。コミットごとのレビューは、それに照らして行う。

#### D-1: 登録表は run に1つで、1つの lock の下に置く

- 決定: `Registry` を `Arc<Shared { cells: Mutex<Registry>, mail: Mail }>` にし、worker 0 が `run_scheduled` で作って、すべての `Scheduler` に配る。worker ごとの部分 (時間の輪、`timer_seq`、driver に出した操作の数 `in_flight`) は、同じ lock の下の `parts[k]` に分ける。lock の順は、登録表 → `Run.slots[k].state` → 葉 (`Run.idle`、`Run.fatal`、`Mail.items`、`HandleBin.dropped`、`Control.workers` の mutex、Host の mutex、driver の中の lock) である。葉か slot の lock を持つ側は、登録表の lock を取らない。`TaskHandle`、`NetStream`、`NetListener` の `Drop` は葉の lock だけを取る
- 登録表の lock の下では、`Ctx` と `Heap` を呼ばない。例外は、今の `start` (`fork_worker`、`share`) と、持ち主の `advance` と `settle_op` (`with_worker` の下で `made` の物体を確保すること) だけである。Rust の `Vec`、`VecDeque`、`BTreeMap` が伸びるのは構わない (裁定の G-A6)
- 理由: セルは1つの添字の空間にあり、スコープの一覧、channel の待ち手の列、取り消しの印付けがセルをまたぐ。セルごとの lock とセルの欄の atomic は cells.rs の書き直しになり、この段では要らない。`Drop` は登録表の lock の下の `decref` の中で走るので、葉の lock しか取れない
- 却下した案: worker ごとの登録表とセルごとの atomic (multicore.md の長期の形。ロックフリーの項目に残す)
- 見直す条件: N が 8 以上で lock の競合を測ったとき (roadmap の「ロックフリーの受け箱と channel」)
- 基準: G-A1、G-A2、G-A6
- リポジトリに残す所: vm.md の「セルと登録表」(A3)

#### D-2: タスクのセルの持ち主と、論理的な ID を使う `Ctx` は1つ

- 決定: `TaskCell.owner` (物理的な worker) は `Registry::start` が1回だけ書き、この段では変わらない。今の `TaskCell.worker` は論理的な ID のまま残す。持ち主だけが、`Runnable` から `Task` を作ること、`take_wake`、`unpark`、自分の区画の `expire`、動いているタスクの欄、自分のタスクの操作の段階の機械 (`settle_op`、`advance`) を行う。どの worker も、登録表の lock の下で `mark`、待ち手の列からの除き、スコープの終わり、`wake_cell` を行える
- 論理的な ID の下で動く (`with_worker`、`set_worker`、`switch_task` で切り替える) のは、同時に1つの `Ctx` だけで、持ち手の受け渡しは登録表の lock で順序付ける。`share(値, ID)` と `transfer_back(値, ID)` は呼ぶ側が自分の ID のまま行うので、同じ預かる ID へ複数の worker が同時に行ってよい。要るのは、それらがすべて `retire_worker(ID)` より前に起きることだけである (裁定の G-A16)
- 届いた知らせのセルの持ち主が自分でなければ、登録表の lock の下でその持ち主へ送り直す (`Route::Forward`)。`take_wake` は持ち主を assert しない。`async-rescue` が持ち主を変えたときに、途中の知らせを取り残さないためである
- 理由: `eml_rt` の「1つの ID を使う `Ctx` は1つ」という規則を変えずに済む。ID ごとの数は今も Relaxed の `fetch_add`/`fetch_sub` で、ほかのヒープと共有している (`crates/eml_rt/src/control.rs:253-270`)
- 却下した案: driver を持つ worker がタスクの ID の下で操作の結果を確保する案 (2つの `Ctx` が同じ ID の下で同時に動き、`eml_rt` の規則を変えることになる)
- 見直す条件: `async-rescue` の設計に入るとき
- 基準: G-A3、G-A16
- リポジトリに残す所: object-model.md の「論理的な worker ID」(A4、A6)、vm.md の「セルと登録表」(A3)

#### D-3: 知らせは worker ごとの受け箱に、ハンドルとシグナルは run の mail に

- 決定: `workers::Notice` を `Child(Task)`、`Joined`、`Place{cell, worker, closure}`、`Wake(Woken)`、`Cancel(CellRef)`、`Submit(CellRef)`、`CancelIo(CellRef)`、`Completion(CellRef)` にする。`cells::Mail` は run に1つで葉の lock を持ち、`MailItem::{HandleDropped, Signal, Cancel}` を運ぶ。最初に集めた worker が登録表の lock の下で処理する。`mark` は、持ち主が自分なら `Cancel` を mail に、そうでなければ持ち主の受け箱に送る。こうすると、N = 1 の `Cancel`、`HandleDropped`、`Signal` の順は今の1つの FIFO と同じになる
- 宛先が自分の `Wake` と `Place` は受け箱を通さず、その場で処理する。`Wake` は継続を持たず、持ち主がセルから `k` を取り出す。`Place` が運ぶ語は、`start` で印を付けた閉包だけである
- 1回の集める地点で取る登録表の lock は、今の処理の順 (取り消しの `driver.cancel` を完了を読む前に呼ぶこと、すぐ終わる受け付け直しを次の完了より先に片付けることなど) を保つ短い区間に分けてよい。どの区間の中でも、`Ctx` と `Heap` (G-A6 の例外を除く) と driver を呼ばず、捨てる語と `retire_worker` は lock を放してから、今と同じ順に行う。`schedule` は登録表の lock を放してから行う (実行中の決定 E-2 で、1区間にまとめる元の決めを改めた)
- 実行待ちの列は今のまま (`Scheduler.ready: VecDeque<TaskId>`、`Policy` も同じ) にする。救済で盗む形は、まだフレームを持たないエントリの `Task` である
- 理由: 救済は、持ち主の受け箱と実行待ちにある、まだ始まっていない継続を見つけられればよい。継続を知らせに入れないので、知らせの中を探さずに済む。自分宛てをその場で処理するのは、N = 1 の実行待ちの順と `RunStats` を変えないためである
- 却下した案: `Wake` に継続を入れる案。自分宛ても受け箱を通す案 (N = 1 の順が変わる)。実行待ちを `Ready::{Task, Run}` にする案 (`Policy` の trait とタスクの番号の振り方が変わる)
- 見直す条件: `async-rescue` と `main-pinning` の設計に入るとき
- 基準: G-A4、G-A5、G-A6、G-A13
- リポジトリに残す所: vm.md の「タスクとスケジュール」と「`Async` のタスク」(A4、A8)、multicore.md の書き換え (A13)

#### D-4: タスクは置ける worker を順に回して置き、最初の `start` を worker 1 に置く

- 決定: `start_raw` と `start_in_raw` は、`sched.workers.places()` のとき `owner = next_place.fetch_add(1, Relaxed) % N` で持ち主を決め、そうでなければ自分にする。`next_place` は 1 から始める (N = 3 で 1、2、0、1、…)。`Async.run` の根のタスクは呼んだ worker に置く。worker 1..N-1 は、最初の `par` の fork か、最初の置く `start` で作る (`SPAWN_WORKERS` を返して同じ命令をやり直す。今の `par` の fork と同じ仕組み)
- `Finished.placed_tasks` は、持ち主が `start` を実行した worker と違うタスクの数である。両方のモードで登録表の lock の下で数え、`RunStats` には入れない
- 理由: 0 から始めると、`start` を1回だけ呼ぶ約25本の UI テストと `exit_in_task` が、N = 2 でも worker をまたがない。ハーネスの `placed_tasks > 0` の確かめも成り立たない。費用は、`start` が1回のプログラムでも worker のスレッドを作ることで、1本あたり約 0.25 ms である (S3 の実測)
- 却下した案: 0 から始める案 (上の理由)。持ち主が 0 でないタスクを数える案 (worker をまたがないタスクも数え、`main-pinning` で worker 0 の意味が変わる)
- 見直す条件: タスクの偏りを測ったとき (roadmap の「タスクを置く worker の選び方」)
- 基準: G-A7
- リポジトリに残す所: vm.md の「`Async` のタスク」、stdlib.md の「`Std.Async`」(A9)

#### D-5: run の時間の基準は1つで、仮想の時計は全体が静止したときだけ進める

- 決定: `Run.clock: RunClock { Real { epoch }, Virtual(Arc<AtomicU64>), Injected }` を作り、VM は `Run::now_ms_*` からだけ時刻を読む。run のどの driver も、この基準で作る (`RealDriver::new_at`、`TestDriver::new_shared`)。仮想の時計を進めるのは worker 0 だけで、worker 0 が眠り、ほかの worker もすべて眠っているときに、全区画のいちばん早い期限まで進める。進めた後、期限の来た区画の持ち主を起こす。デッドロックを決めるのも worker 0 だけで、登録表を読んだ後に slot の lock の下で `running`、受け箱、`mail.pending`、`NOTICE` を読み直してから決める
- 理由: 遅れて作る driver はそれぞれ別の起点を持ち、driver のない worker は時刻を読めない。1つの worker が自分の空きだけで仮想の時刻を進めると、ほかの worker の忙しい間に時刻が飛ぶ
- 見直す条件: worker ごとに仮想のネットワークを持たせたくなったとき
- 基準: G-A8、G-A12
- リポジトリに残す所: vm.md の「時計と driver」(A2、A4)

#### D-6: 仮想の時計では driver は worker 0 だけ、本物の時計では各 worker が持つ。受け付けは転送する

- 決定: `Run::io_worker(k)` は、仮想の時計なら 0、本物の時計なら k である (A8〜A10 は両方とも 0、A11 から本物の時計で k)。本物の時計の worker k は、最初に要るときに自分のスレッドで driver を作り、`Slot.waker` を1回だけ設定する。`Machine::with_driver` の run は worker を作らない
- 操作の記録は `host` (driver を持つ worker) を持つ。持ち主が `Op` を作り (今と同じ場所で `RawBuf` を貸す)、段階の機械を動かし、結果の物体を自分の `Ctx` で確保する。host は `Op` を自分の driver に渡し、届いた生の結果を取り出すことだけをし、ヒープの語に触れない。`submitted` と `cancelling` は登録表の lock の区間の中でだけ読み書きし、host は `submitted` を立てる同じ区間で `cancelling` を読む。そのため、取り消しと受け付けのどちらかが必ず相手の旗を見る
- `RawBuf` は `Send` にする。貸した側が作り、driver のスレッドが使い、結果を記録するまで記録が貸した物体を生かして動かさない、という規則である
- `listen` と `connect` の結果は host に縛る。`accept` の結果は、A14 が入るまで受け付けた host に縛る
- 理由: テストの driver の仮想のネットワークは driver ごとにあるので、仮想の時計の run では1つの driver に集める。こうすると、N ≥ 2 のどの UI テストでも転送の道を通る
- 却下した案: 記録の中身を lock の外で host が処理する案 (取り消しの取りこぼし、語の持ち主、ID の同時使用の問題が出た)
- 見直す条件: IOCP や SQ を lock する io_uring で、受け付けをスレッドをまたいで出せるとき
- 基準: G-A9、G-A10、G-A11、G-A19
- リポジトリに残す所: vm.md の「時計と driver」(A7、A8、A11)、object-model.md の T10 の行 (A7)、async.md の書き換え (A13)

#### D-7: run の終わりの順番

- 決定: 成功した run は、次の順で終える。(1) worker 0 の `main` の `Finish` が、自分の受け箱と mail を空にし、登録表の lock の下で終わっていないタスクと出したままの操作がないことを確かめる。(2) `END` を立ててすべての worker を起こす。(3) worker k は安全点か待ちでだけ `END` に応じ、最後に1回集めて、残った `Place`、`Child`、`Wake` があれば `left` を立てる。`hold` が空であることを確かめ、自分の driver を静止させて結果を記録し、driver を落として `Retired` を返す。(4) worker 0 が join し、自分の driver を静止させる。(5) どれかの `left` があれば `a task is left at the end`、`kept_pending` があれば `Machine` を使えなくする。(6) `sweep_all` → `reclaim_all` → Host の書き出し → 検査。誤りと `exit` の run は、登録表に触れずに join の後で落とす。worker k ≥ 1 で `exit` した run は、`end_run` を呼んでから起こす
- 理由: worker 0 の確かめだけでは、ほかの worker のタスクが確かめの最中に登録表を書き換えうる。`exit` の今の扱いでは、worker k の `Stop::Ended` を worker 0 が見落とす (`workers.rs:494-520`)
- 基準: G-A14、G-A15
- リポジトリに残す所: vm.md の「誤り、止めること、終わり」(A8)、object-model.md の「回復」(A8)

#### D-8: 設計の文書を書き換える

- 決定: multicore.md と async.md の決めのうち、この段の模型と食い違うものを書き換える。書き換えは S1 の 5 章の 1〜10 (受け箱が実行待ちを持つ文、持ち主の読み直しの規則、救済の道、印付けの表など) と、S3 の 5.7 節の multicore.md の「テスト」(11) である。今の事実を述べる文 (multicore.md:72、:488、:523) はそれを偽にする単位 (A9) で直し、後の段のための決めは A13 で書く。roadmap の `async-rescue` の項目は、書き換え 1〜3 を引く (C)
- 理由: 利用者が、よりよい設計のために設計の文書を改めてよいと決めた
- 基準: G-A20
- リポジトリに残す所: multicore.md、async.md (A13)、roadmap (C)

### 5.2 短中期の設計

理由があれば実装の中で調整してよい。調整したら、コミットのメッセージに書く。

#### D-10: `Counter` は lock を取らない表にする (S2 の U4)

- 決定: 新しい `crates/eml_vm/src/counters.rs` に、動かない塊を伸ばしていく表を置く。`add`、`load`、`store` は lock を取らず `SeqCst` で、`add` は桁あふれで回る。`Machine` が `Arc<Counters>` で持ち、`Start` から `Run.counters` に渡す。run の後に戻さないので、失敗した run、`exit`、回復をまたいで値が残る。`new` が新しい塊を開くときだけ、ほかの `new` が待つ
- 基準: G-A17
- リポジトリに残す所: stdlib.md の「`Std.Atomic`」、vm.md (A5)

#### D-11: channel は予約の数と worker ごとの hold で守る (S2 の U7)

- 決定: `ChanCell` に `sends` と `recvs` を持たせる。`reserve_send` は登録表の lock の下で `sends` を数え、channel とその ID を `Scheduler.hold` に預ける。`push_raw` は lock の外で `share` し、1回の lock の区間で要素を積んで受け手を起こす。`reserve_recv` は lock の下で先頭の要素を取り出して `recvs` を数え、hold に預け、空いた送り手を起こす。`pop_raw` は lock の外で `transfer_back` し、1回の区間で数を下ろして片付ける。`new_raw` の `fork_worker` と、片付けの `retire_worker` も lock の外で行う。extern の行、extern の表、`std/Chan.em` のコードは変えない
- 予約から確定までの間の命令は、`load_raw`、`int_eq`、`switch_enum`、`box_int`、`box_const` だけで、安全点も切り替えの地点も捕まえられる失敗もない (S2 の 7 章、逆アセンブルで確かめた)。そのため、hold は安全点ではいつも空で、N = 1 の出力と `RunStats` は変わらない
- 新しく起きうる振る舞いを2つ、意図したものとして spec に書く。`close` の前に予約した送りは届き、受け手はそれを待つ。予約した送りの後に受け手が取り消されると、channel は一時的に容量を超えうる
- 却下した案: `push_raw` で確かめ直して、やり直しの値を返す案 (extern の形が変わる)。lock の下で `share` する案 (G-A6 に反し、送った値の大きさだけ lock を持つ)
- 基準: G-A6、G-A15、G-A16、G-A17
- リポジトリに残す所: stdlib.md の「`Std.Chan`」、vm.md の「channel」(A6、A9)

#### D-12: `async/` の UI テストを N = 2 と 8 で流す (S3)

- 決定: `par/` と同じく、検査モード、逐次の方針、仮想の時計で、`Limits.workers` だけを変える。N ≥ 2 では出力と終わり方を比べ、`RunStats` は比べない。`run/async` の各ファイルで `Finished.placed_tasks > 0` を確かめる。順序に依存するファイルには先頭の印 `-- workers: skip (<理由>)` を付ける。理由は必ず書き、印は `par/` と `async/` の下の `run/` と `run-fail/` にだけ許す。A10 ですべてのファイルを一度に有効にする
- S3 は81本を読み、79本を順序によらないと分けた。残る `drain_within_cancelled_at_deadline` と `drain_within_deadline_coincides` は、印を付けずに N = 2 と 8 で200回流し、違いが出たときだけ印を付ける。`shield_boundary.em` は、55行目の `yield_now` を `sleep_ms 10` に替える (snapshot は変わらない)
- 速いモードの N ≥ 2 は、UI テストでなく2本のテストで確かめる (`workers.rs` の `the_fast_mode_runs_tasks_on_four_workers`、`cli.rs` の `eml_run_spreads_tasks_over_four_workers`)
- `cli.rs` のシグナルのテスト3本 (`a_terminate_reaches_a_waiting_task`、`an_interrupt_after_a_delivered_one_ends_the_program_with_130`、`a_second_signal_ends_a_program_whose_waiter_cannot_be_reached`) は `--workers 1` に固定する。複数の worker でのシグナルは `signal.rs` と A9 の2本の CLI のテスト (待つタスクなしで 143 と 130) で確かめる
- リポジトリに残す所: testing.md の「UI テスト」「seed で流す」(A10)

#### D-13: Linux で worker の数ごとに測る (S3)

- 決定: `bench/async/scale.sh` が、同じ回の中で N = 1、2、4 について4つを測る。`spread.em` (`fib 30` のタスク8つ)、同じ仕事を `par` で書いた版、機械の対照 (1 worker のプロセスを N 個並べて仕事を分ける)、S2 の `chan_pingpong`。速さの伸び S(N) は、N = 1 の実時間の中央値を N のときの中央値で割った値である。`bench/net` の `bench.py` に `--workers` を足し、`serve-self-32`、`conn-self-32`、`busy-self`、`echo-self-fresh` を測る。結果は benchmarks.md の新しい見出し「worker の数と実時間」に書く。テストは速さを確かめない
- 段を閉じる基準 (10 章の Q-1): N = 2 と 4 のどちらでも、S_spread が S_hw の 0.80 倍以上であること。ただし、その回の S_hw が N = 2 で 1.6 以上、N = 4 で 2.4 以上でなければ数えない
- `chan_pingpong` の t(4) が t(1) より遅ければ、roadmap の「ロックフリーの受け箱と channel」の見直す条件に数値を書く。段は止めない
- A14 を入れる条件: `conn-self-32` が伸び、`serve-self-32` が伸びないとき、または `echo-self-fresh` が N = 2 で 10% 以上遅くなるとき (S3 の 5.6 節)
- リポジトリに残す所: benchmarks.md (A12、A13)

## 6. 実装の決まり

- どのコミットも、テストに限らず、どの eml のプログラムをどの worker の数で動かしても安全でなければならない。A2〜A8 では、タスク、channel の要素、`Counter`、driver の操作は worker 0 から出ない。A9 で置き方を入れる前に、置いたタスクが届くすべての道を A3〜A8 で作っておく (S4 の 1.1 節)
- 単位の ID (A0〜A14、C)、付録の ID (U1〜U9、S3-x)、`docs/plans/` の道を、コード、コメント、`docs/` に書かない (S4 の 1.6 節の G-0 の grep)
- `docs/` と `CLAUDE.md` の文は、それを偽にするコミットで直す。どの文をどの単位が直すかは S4 の 2.0 節の表にある。status.md、architecture.md、overview、README、roadmap は C で直す。例外は A1 と、A5 の roadmap.md:344 である
- 日本語の文は natural-japanese のスキルに従う。コードのコメントは理由を書き、`docs/` の節を引く
- 毎回の確認 K1〜K6 (S4 の 1.6 節) を、どの単位でも流す。K1 は `cargo insta test --test-runner nextest --check` → exit 0、失敗 0、保留の snapshot なし。K2 は `git diff 2c8de78 --stat -- crates/eml_cli/tests/snapshots crates/eml_vm/tests/bench.rs` → 空
- `eml_rt` を変える単位 (A7、A14) は、`crates/eml_rt/CLAUDE.md` の関門を通す
- 計測の数値、stress の結果、起動の費用は、その単位のコミットのメッセージに書く。K2-d に当たるファイルは、見えた違いと選んだ扱いをコミットのメッセージに書き、直後の `Record … in the async-threads plan` のコミットで、この計画の 10 章に書き足す。A14 を入れるかの決定も同じく記録する
- テストの失敗を見たら、まず VM の誤りとして調べる。印を付けるのは最後の手段である

## 7. 実装の単位

各単位は1つのコミットである。作業の中身、触るファイル、spec の文、テストの変更、完了のコマンドは、S4 の 2 章の同じ ID の節にある。下の表の「完了の要点」は、毎回の確認 K1〜K6 に足すものの要約である。

| ID | 単位 | 基づく決定 | 依存 | 完了の要点 |
|---|---|---|---|---|
| A0 | 計画のコミット (`Plan async-threads`) | — | — | `git show --stat HEAD` が `docs/plans/` の下だけ。`cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` → 4 passed |
| A1 | roadmap で `compute` を `async-compute` に分ける | — | A0 | `grep -c 'async-compute' docs/future/roadmap.md` → 1。`async-threads` の行に `compute` がない。基準の数値をコミットのメッセージに書く (9 章) |
| A2 | run の時間の基準を1つにする | D-5 | A1 | eml_io の単体テスト2本 (`a_driver_made_at_an_earlier_epoch_counts_from_it`、`test_drivers_on_one_clock_see_each_others_waits`) |
| A3 | 登録表を run に1つにし、lock の下に置く | D-1、D-2 | A2 | 取り消しと受け付けの順の表のテスト `a_cancel_and_a_submit_are_ordered_by_the_lock`。`the_task_cells_import_only_std_and_the_runtime` |
| A4 | 知らせを受け箱と mail に分け、どの worker でも集め、シグナルを全体で届ける | D-3、D-5 | A3 | worker 1 の集める地点が mail のシグナルを処理する単体テスト2本。TSan の `workers::` と `signal::` |
| A5 | `Counter` を lock を取らない表にする | D-10 | A4 | `counters::` の単体テスト5本。TSan と Miri の `--lib counters::` |
| A6 | channel の予約と hold (1つの worker の上) | D-11 | A5 | `cells.rs` の単体テスト11本。今の `chan::` 11本は変わらない |
| A7 | `RawBuf` を `Send` にする | D-6 | A6 | `cargo verus verify -p eml_rt`、verusfmt、`trust::` |
| A8 | 操作を host へ転送し、run を新しい順で終える (置き方はまだ入れない) | D-6、D-7 | A7 | 転送の両側を手で動かす lib の単体テスト3本 (`a_forwarded_read_is_submitted_by_the_host_and_settled_by_the_owner` など)。TSan。Windows の cross clippy |
| A9 | `Async` のタスクをすべての worker に置く | D-4 | A8 | `placement_rotates_over_every_worker` (N = 3 で 1、2、0、1、2、0)、`the_first_start_starts_the_workers`、`exit_in_a_task_on_another_worker`、S2 の N ≥ 2 の `chan::` のテスト、CLI の 143 と 130。TSan と ASan に `io::`、`chan::`、`workers::`。`signal::` を20回流す |
| A10 | `async/` の UI テストを N = 2 と 8 で流す | D-12 | A9 | `ui::run` と `ui::run_fail` が N = 2 と 8 で通る。`workers_runs_cover_par_and_async`。`drain_within_*` 2本を200回、ほかを50回流す。`--profile timing -j 1` で SLOW なし |
| A11 | 本物の時計で各 worker が自分の driver を持つ | D-6 | A10 | `an_echo_between_two_workers_completes`、`eml_run_spreads_tasks_over_four_workers`。ASan と TSan の `io::` |
| A12 | worker の数を測る道具を足す | D-13 | A11 | S3 の 5.8 節の S3-9a の確かめ。`target/release/eml run --workers 4 bench/async/chan_pingpong.em` → `800040000`。`bench::every_program_in_bench_has_a_test` が通る |
| A13 | Linux で測り、設計の文書を書き換える | D-8、D-13 | A12 | benchmarks.md の「worker の数と実時間」。段を閉じる基準 (Q-1) の判定。A14 を入れるかの決定。書き換え 1〜11 |
| A14 | `accept` で受けたソケットを最初に使うタスクの worker に縛る (条件付き) | D-6、D-13 | A13 | 条件: D-13 の A14 を入れる条件を満たすときだけ。満たさなければ飛ばし、roadmap の項目にする。入れたら `scale.sh` を流し直して Q-1 を判定し直す。`eml_rt` の関門 |
| C | 段を閉じる (`Close async-threads`) | すべて | A13 (A14 があれば A14) | roadmap の「進め方」の手順 1〜9 (S4 の 3.3 節)。`grep -rn 'async-threads' docs CLAUDE.md` が `docs/plans/` のほかに出ない。この計画を同じコミットで消す |

単位ごとの「実装者に任せること」は次のとおりである。ほかの単位では、付録が決めていないことは止めて報告する。

- A3、A4: 内部の関数の名前と分け方。S1 の 5 章の型と欄の名前は変えない
- A9: 大きくなったら、コードのコミットとテストだけのコミットに分けてよい。どちらも完了の条件を通すこと
- A10: K2-d の範囲で印、書き換え、仕事の削減のどれを選ぶか。選ぶ基準は S4 の 5 章の K2-d の行にある
- A12: スクリプトの内部の作り。出力の形は S3 の 5.6 節に従う

テストの変更のうち、種類1と2は次のとおりである (S4 の 5 章)。承認はこの計画の承認で行う。

| ID | 単位 | 種類 | テスト | 変更と理由 |
|---|---|---|---|---|
| K1-a | A10 | 1 | `ui::workers_runs_cover_par_only` | 消して `ui::workers_runs_cover_par_and_async` に替える。固定していた規則 (`async/` は N ≥ 2 で流さない) が逆になる |
| K2-a | A10 | 2 | `tests/ui/run/async/shield_boundary.em` | 55行目の `Async.yield_now ()` を `Async.sleep_ms 10` にする。N ≥ 2 で "unreachable" を出しうる競合を除く。snapshot は変わらない |
| K2-b | A9 | 2 | `workers::exit_abandons_a_child_on_another_worker` | 確かめる値を `finished.placed` から `finished.placed_tasks` にする。目的 (`exit` がほかの worker の仕事を捨てる) は同じで、置いたタスクの `par` の fork が `exit` より先に起きるかが競合になるため |
| K2-c | A9 | 2 | `workers::a_timer_on_worker_0_and_a_child_on_worker_1_agree` | 先頭に、ダミーのタスクを1つ `start` して `await` し、`sleep_ms 1` する前置きを足す。タイマーのタスクを worker 0 に、子を worker 1 に保つため。確かめる値は同じで、`placed_tasks == 1` を足す |
| K2-d | A10 以降、CI | 2 (範囲) | `run/async/*`、`run-fail/async/*` | N = 2 か 8 の stress で出力の違いが見えたか、CI で失敗して VM の誤りでないと分かったファイルに、(1) snapshot を変えない書き換え、(2) 出力を変えない仕事の削減、(3) 印 `-- workers: skip (<見えた違い>)` のどれかを当ててよい。段の全体で5本まで。超えるなら計画を改めて利用者の承認を得る |

種類3 (承認は要らず、コミットのメッセージに書く): `cli.rs` のシグナルのテスト3本の `--workers 1` (A9)、`Finished` のリテラル、`Run::new` と `Scheduler` と監視のスレッドの呼び出しの形、ハーネスの内部の関数、既存のテストの doc コメント。足すテストは並べない。

## 8. 実行の順番

A0 → A1 → A2 → A3 → A4 → A5 → A6 → A7 → A8 → A9 → A10 → A11 → A12 → A13 → (A14) → C の一列で、並行に進める単位はない。A2〜A8 は置き方を入れる前の土台で、A9 で初めてタスクが worker 0 の外に出る。この順は 6 章の安全の決まりから来ていて、入れ替えない。C の後に、利用者が macOS で `Measure async-threads` のコミットを作る。

## 9. 確かめの計画

- 基準 (最初の変更の前に流し、A1 のコミットのメッセージに書く)
  - `cargo insta test --test-runner nextest --check` → exit 0。nextest の要約の数を書く
  - テストの数: `crates/eml_vm/tests/workers.rs` 16、`io.rs` 22、`signal.rs` 10、`chan.rs` 11、`citations.rs` 4。UI テスト: `run/async` 69、`run-fail/async` 12
  - `CLAUDE.md` の TSan、ASan、Miri のコマンドと `cargo verus verify -p eml_rt` → 通る
  - `ui::run` の実時間と、`probes/onetask.em`、`probes/onefork.em` の起動の時間の中央値
- 全体の確認 (A13 か A14 の後、C の前)
  - 先頭のコミットで K1〜K6
  - `cargo nextest run --profile timing -j 1` → SLOW も失敗もない
  - `CLAUDE.md` の最後の形の、すべての sanitizer と Miri のコマンド → 通る
  - `cargo verus verify -p eml_rt` と verusfmt → 通る
  - Windows と Linux の cross clippy、既定でない feature の clippy、actionlint → 通る
  - `EML_UI_FILTER=/async/ cargo test -q -p eml_cli --test integration -- --exact ui::run ui::run_fail` を50回 → 失敗なし
- 利用者が手で確かめること
  - A13 (と A14) の数値と、A14 を入れるかの決定を見る
  - macOS で `bench/run.sh`、`bench/rt.sh`、`bench/scripts.sh` を流し、`Measure async-threads` のコミットを作る。範囲を外れた行があれば、その前に `Resize bench rows` のコミットを作る。`bench/net.sh` を `--workers 1,2,4` で流す
  - A8、A9、A11、A14 の後に Windows の CI が通ることを見る
  - 手元の機械で、既定の worker の数でネットワークのプログラムを動かし、Ctrl-C で止まることを見る

## 10. リスクと未決の項目

| ID | 項目 | 影響 | 決め方と時期 | 決まらないときの既定 |
|---|---|---|---|---|
| Q-1 | 段を閉じる速さの基準 (D-13: S_spread ≥ 0.80 × S_hw、N = 2 と 4) | A13、A14、C | 決定済み (2026-11-01、利用者): 計画のとおり | — |
| Q-2 | K2-d の上限 (段の全体で5本) | A10 以降 | 決定済み (2026-11-01、利用者): 5本 | — |
| R-1 | 待ちの取りこぼしやデッドロックの判定の誤り (A4、A8、A9) | 止まる、または誤ってデッドロックになる | S1 の Q6 の (d) と Q8 の手順、単体テスト、TSan | VM の誤りとして調べる。印を先に付けない |
| R-2 | 記録が動いた後か解放された後に `RawBuf` を使う (A9) | メモリの破壊 | S1 の Q5 の I1〜I4、`take_op` は `Done` を要する、A9 からの ASan の `io::` と `workers::` | — |
| R-3 | 仮想の時計の `net_*` が N ≥ 2 で1秒を超える、`drain_within_*` が分かれる (A10) | 時間の確かめで失敗 | K2-d | K2-d の範囲で扱う |
| R-4 | CI でだけたまに失敗する (CI はやり直さない) | CI が赤くなる | S3 の 5.2 節の手順 5、その後 K2-d | — |
| R-5 | IOCP と kqueue の worker ごとの driver (A11) | Windows か macOS で失敗 | cross clippy、CI の macOS の ASan | A12 の前に直す |
| R-6 | 機械の対照の伸び S_hw が足りない (A13) | Q-1 を判定できない | 別の機械で測るか、利用者に聞く | 利用者に聞く |
| R-7 | A14 で IOCP がソケットを結び付けずに受けられない (S1 の A4) | Windows で縛らずに受けられない | Windows では受け付けた host に縛る形に戻し、spec に1文書く | その形 |
| R-8 | `next_place` = 1 の起動の費用 (A9) | `start` が1回のプログラムが遅くなる | 費用を測ってコミットのメッセージに書く | 後で考える |
| R-9 | std を変えて予約と確定の間に命令が入る (A6 以降) | hold が安全点で空でなくなる | hold の debug assert (切り替え、worker を抜けるとき、`sweep_all` の前) | — |

A13 の計測の決定 (2026-11-03、Linux の 4 CPU の KVM、release、`089dcc9`):

- 段を閉じる基準 (Q-1) は満たした。機械の対照の伸びは N = 2 で 1.960、N = 4 で 3.171 で有効。`spread` の伸びは 1.873 と 3.308 で、それぞれ 0.80 倍の 1.568 と 2.537 を超えた
- A14 は入れる。`conn-self-32` は伸びず (w4÷w1 = 1.58)、`echo-self-fresh` が N = 2 で 2.02 倍遅くなった (差)。受けたストリームが listener の worker 0 に縛られ、worker 1 に置いたサーバのタスクの読み書きがすべて転送されるためである。A14 の後に `scale.sh` と `bench/net.sh` を流し直し、Q-1 を判定し直す
- `chan_pingpong` は t(4) が t(1) の 12.4 倍なので、C で roadmap の「ロックフリーの受け箱と channel」の見直す条件を書き換える
- `spread_par` の伸びは N = 2 と 4 でどちらも 1.94 なので、C で `par` の頭打ちの文を roadmap に足す

- E-6 (2026-11-03、利用者): A14 で `eml_io` の核のテスト3本の期待値が変わった。`an_aborted_accept_is_retried_under_the_same_key` (`Done::Accepted` を期待する)、`quiesce_closes_the_stream_of_a_discarded_accept` と `an_event_for_an_unknown_key_closes_only_an_accepted_sock` (閉じたことを偽の `Call::Close` でなく pipe で確かめる。受けたソケットは driver に登録されないので、driver の close の呼び出しが出ない)。どれも目的は変わらず、計画で決めた `Done::Accepted` の API から来る種類2の変更として、利用者が受け入れた。あわせて、テストの driver は受け付けを `Done::Stream` で終えたままにする (仮想のネットワークは driver に1つで、仮想の時計ではどの I/O も worker 0 が受け付けるため、縛り先は同じになる)。Windows の仮の形は使わず、IOCP のポートへの結び付けを `adopt` に移した

K2-d を当てたファイル (段の全体で5本まで。今 2 本):

| ファイル | 単位 | 見えた違い | 扱い |
|---|---|---|---|
| `tests/ui/run/async/drain_within_cancelled_at_deadline.em` | A10 | N = 8 の 200 回のうち 10 回。`m: Cancelled` / `drainer: returned` (7 回) と `drainer: Cancelled` / `m: Cancelled` (3 回)。同じ期限のタイマーは worker ごとに満ちるため | 印 |
| `tests/ui/run/async/drain_within_deadline_coincides.em` | A10 | N = 8 の 200 回のうち 4 回。`a: Cancelled` / `drained` / `after: done`。同じ理由 | 印 |

## 実行中の決定

実行の途中で利用者と決めたことを書く。付録と食い違うときは、ここが優先する。

- E-1 (2026-11-01): A7 (`RawBuf` を `Send` にする) を A3 の前に行う。A3 が `OpRecord.pending_op: Option<Op>` を足すと、`Op` が `RawBuf` を持つので `Registry` が `Send` でなくなり、`Arc<Shared>` を worker 1..N-1 のスレッドに渡せないためである。A7 は A3〜A6 に依らない。順は A1 → A2 → A7 → A3 → A4 → A5 → A6 → A8 → … になる
- E-2 (2026-11-01、利用者): G-A6 の「1回の集める地点で登録表の lock は1区間」を、「今の処理の順を保つ短い区間に分けてよい。区間の中では `Ctx`、`Heap` (G-A6 の例外を除く)、driver を呼ばない」に改める。1区間にまとめると、取り消しと受け付け直しの順が変わり、N = 1 の実行待ちの順が変わって G-A5 を破るためである。G-A5 を優先する。`Discards` は、捨てる語と `retire_worker` を今と同じ順に並べた列にする
- E-3 (2026-11-01): channel の extern のヒープの呼び出しは、A3 では lock の下に残してよい。A6 が lock の外に出す
- E-4 (2026-11-01、利用者): A8 と A9 の間に単位 A8b を足す。中身と完了の条件は付録 [a8b-net-controls.md](2026-11-01-async-threads/a8b-net-controls.md) の 2 章と 3 章にある。(1) `Std.Net.shutdown_write` と `set_nodelay` を、受け付けですぐ終わる転送の操作 (`Op::ShutdownWrite`、`Op::SetNodelay`、`Submit::Done`) にし、std の型を `<IO, Async>` にする。今はタスクのスレッドで driver を直接呼んでいて、別の worker に置いたタスクでは動かないためである。`IoDriver` の2つのメソッドは消す。`eml_rt` の関門、eml_io の `driver::tests::` の Miri と ASan、Windows の cross clippy を通す。(2) 持ち主でない worker でソケットを落としたら、host の worker を起こす (worker ごとの atomic の旗を `Drop` が立て、各 worker がループの地点で見て、lock の外で起こす)。今は host が起きず、ソケットが閉じられないままになりうる。(3) 2つの操作は取り消しの地点にしない (利用者)。N = 1 の振る舞いを変えないためである。A9 の完了の条件に `git grep driver_mut crates/eml_vm/src/vm.rs` → 出力なし を足す
- E-5 (2026-11-01): E-4 (3) を守るため、2つの操作の std の包みは `write_bytes` の形 (`io_wait`) にしない。`io_wait` は受け付けで終わった操作でも取り消しの地点になり、予算と `parks` を使うためである。借りるだけの extern `Std.Net.op_done : OpId -> (Bool, OpId)` を1行足し (表の行と std の宣言と spec の1文)、受け付けで終わっていれば待たずに取り出し、終わっていないとき (別の worker の driver に回したとき) だけ、取り消しを見ない待ち (`Async.io_wait_shielded`。`Async.shield` は本体に `Panic` を要するので使わない) で待つ。`op_done` は記録を読むだけで、host が置いた結果を当てない。N = 1 の出力と振る舞いは今と同じで、`RunStats` は `completions` が操作ごとに1増え、`peak_stack` が1増えうる (固定した snapshot はない)。Opus のアドバイザーが利用者の求めで確かめた (2026-11-01)


## 付録: レビューで変わった主なこと

議論を繰り返さないために、決め手になったものだけを書く。

- 登録表の lock の下で I/O の記録の中身を host が処理する案は、取り消しの取りこぼし、語の持ち主、2つの `Ctx` が同じ ID の下で同時に動くことの3つの問題を生んだ。host は `Op` を運ぶだけにし、語には持ち主だけが触れる形に改めた (D-2、D-6)
- 実行待ちを `Ready::{Task, Run}` にする案は、`Policy` の trait とタスクの番号を変えるのでやめた。救済で盗む形は、まだフレームのないエントリの `Task` にした (D-3)
- channel で lock の下で `share` する案は G-A6 に反するのでやめ、予約の数が片付けを防ぐことを使って lock の外で行う形にした (D-11)
- タスクを置き始める worker は、0 では `start` が1回のテストが worker をまたがないので、1 にした (D-4)
- G-A16 の「ID を使う」は ID の下で動くことと読み、ID ごとの数を複数の worker が同時に増減するのは認めた。G-A6 の「確保」は eml のヒープを指すと読んだ。hold を run の終わりに確かめる地点を足した (裁定)
- 置き方を channel と転送より先に入れる順は、途中のコミットで要素が印なしにヒープをまたぐので、置き方を A9 に遅らせた (6 章)
