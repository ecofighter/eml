# async-rescue の設計と計画

- 作成: 2026-11-04
- 基準: main @ f969e3c0 (3 節と、すべての `file:line` はこの状態を指す)
- 状態: 計画は確定。既定のある未決の項目が残る (10 節)

## 1. 目的と完了の条件

- 目的: 詰まった worker か過負荷の worker の `Async` の仕事を、空いた worker が引き取る (救済)。今はタスクが `start` で置かれた worker から動かず、長く計算するタスクがいると、ほかの worker が空いていても同じ worker のタスクが時間を分け合う (docs/implementation/status.md:164)。
- 完了の条件: 9 節の全体の確かめがすべて通り、T4 の閉じるコミットで `docs/` が今の状態を書いていること。

## 2. 範囲

- 範囲に入るもの:
  - worker ごとの実行待ちの列を、run の表の欄の lock の下に移すこと
  - タスクのセルの `entries` と、持ち主の変更
  - 盗む手順 `steal`
  - 監視のスレッドの判定と、`Notice::Rescue`
  - CLI から `--sched` を外すこと。`eml run` と `eml repl` はいつも `Normal` で動き、`Sequential`、`Seeded`、新しい `RescueAlways` はライブラリの API から渡すテストのための値になる (D-16、T0)
  - テストのための方針 `Sched::RescueAlways`
  - `Finished` の2つの数
  - テスト、`docs/spec/` の節、段を閉じる作業
- 範囲に入らないもの:
  - 操作か仕事を待つタスク (`Wait::Io`、`Wait::Job`) の救済、`Completion` の知らせ、`resume_io` の項 (10 節の U-7、U-8。閉じるときに roadmap の「その後の項目」へ移す)
  - `par` の子の盗み (`par-steal`)
  - 専用の worker とメインの worker (`main-pinning`。この段では `rescuable` の口だけを作る)
  - タスクごとの上限 (`task-limits`)
  - ネイティブのコード
- `eml_rt` には触れない。Verus の gate は要らない (D-3)。

## 3. 前提になる事実

- 救済のコードは何もない。`stuck`、`overload`、`rescue`、`steal`、`救済`、`盗` で eml_vm を grep しても、該当するものはない。2刻みと 64 の値は docs/future/multicore.md:79-81, 570 にだけある。
- 独立した worker のループはない。worker 0 も 1..N-1 も同じ VM の命令のループを回し、切り替えは `switch_away` (`crates/eml_vm/src/vm.rs:1531-1586`) と `preempt` (`vm.rs:1693`) で起きる。`switch_away` は次の順に進む。
  1. `collect` から `gather` (`crates/eml_vm/src/sched.rs:634-763`) を呼ぶ。`gather` は受け箱を取り、登録表の lock の下で `Async` の知らせを処理する。続けて host の受け付け、mail、driver の完了、自分の区画の時間の満期、`ring_hosts` を処理する
  2. 走れる仕事をエントリの `Task` にして `ready` の後ろに積む
  3. `reclaim_if_due`
  4. `policy.pick(ready)`
  5. 取れなければ `idle_wait` (worker 1..N-1 では `peer_wait`) に入る
- `Scheduler.ready` は lock のない `VecDeque<TaskId>` で (`sched.rs:367`)、スレッド局所の `Scheduler` にある。`par` の子、横取りしたタスク、起きた `par` の親、`Async` のエントリの `Task` (始めるものも再開するものも) が1本に混ざる (`vm.rs:1267, 1713, 2954, 2971`)。
- 受け箱は `Slot` で、次の形である (`crates/eml_vm/src/workers.rs:155-176`)。
  - `Slot{ Mutex<SlotState{notices: Vec<Notice>, woken, parked}>, has_notices: AtomicBool, cv, waker, bin, switches }`
  - 知らせは `Child(Task)`、`Joined`、`Place{cell, worker, closure}`、`Wake`、`Cancel`、`Submit`、`CancelIo`、`Completion` の8種類である (`workers.rs:36-59`)
  - 欄の lock は、登録表の lock の内側で取る
- `TaskCell.owner` は `Registry::start` だけが書き、後で変わらない (`crates/eml_vm/src/cells.rs:236`)。持ち主が変わった後の回送は、すでに次の箇所にある。
  - `take_wake` の `Route::Forward` (`cells.rs:1347`)
  - `placed_owner` と `Place` の積み直し (`sched.rs:54-64`)
  - `cancel_task` の `Cancel` の積み直し (`cells.rs:2416`)
  - `Mail::take(me)` (`cells.rs:618`)
- `Op::Ret` は安全点である (`vm.rs:629-634`)。`park` の命令 (`vm.rs:2184-2192`) からエントリの `Task` の `Finish` (`vm.rs:647-655`) までの間に、持ち主は横取りされてほかのタスクを動かしうる。その間も待ったタスクのフレームは残り、継続 `k` から届く物体を手放す (`entry_finish` が `evv` の残りを decref する、`vm.rs:3035-3037`)。
- すぐ再開する待ち (`ready`、取り消された地点、終わった相手、`park_chan` の準備済み) は、待ったエントリの `Task` がまだ動いている間に `Runnable` を列に積む (`cells.rs:1762-1775, 4419-4440`)。そのため、1つのセルにエントリの `Task` が2つ同時に生きることがある。
- `Registry::ready` と、`ms <= 0` の `park_time` は `wait = Some(Wait::Ready)` を立てる (`cells.rs:1885, 1913`)。`resumed` がそれを取る (`cells.rs:1722`)。
- 印付けは次のとおりである。
  - `share(&mut self, w: Word, receiver: u32)` (`crates/eml_rt/src/heap/share.rs:2316`) に送り手の引数はない
  - 置いたタスクの閉包と根は、`Registry::start` で新しいタスクの論理的な ID へ `share` する (`cells.rs:1646-1651`)
  - 始めるときに `transfer_back(args[0], spawner)` する (`vm.rs:2990-2992`)
  - タスクをまたぐ受け渡しはどれも印を付ける。channel の項とタスクの結果は `Depot::publish` (`cells.rs:4862`)、`par` と `compute` の fork は `vm.rs:1215, 1285-1289, 2234` である
- 確保器は、ほかのヒープのページの物体のうち、共有の物体、不死の物体、持ち主が今の ID の局所の物体を受け付ける (docs/spec/object-model.md:330)。検査モードの持ち主の検査は、ヒープの今の ID と比べる (`crates/eml_rt/src/heap/check.rs:217-236`)。run の終わりでは worker をすべて join してから `reclaim_all` がすべての `RemoteSink` を空にし、その後で検査する (`crates/eml_vm/src/lib.rs:1057-1060`)。
- 置き方と監視は次のとおりである。
  - 置き方は `Run::next_owner` (`workers.rs:269`) で、1, 2, …, 0 の順に回す
  - `Run.idle: Mutex<Vec<usize>>` (LIFO) は、`par` の子と `compute` の仕事だけが使う (`vm.rs:1296`)
  - 監視のスレッド (`crates/eml_vm/src/monitor.rs:131-175`) は 10 ms ごとに `slot.switches` を前の刻みと比べ、変わらなければ `control.preempt(k)` を呼ぶ。受け箱も登録表も読まない
  - `Ticker` のテストは構造体のリテラル `Ticker { last: 3 }` を使う (`monitor.rs:193, 200`)
- 決定性に関わる事実は次のとおりである。
  - `Sequential` は横取りしない。`Seeded` は `1 + rand % 16` 個の安全点ごとに切り替え、走れるものから乱数で選ぶ (`sched.rs:239-267`)
  - `par/` と `async/` の UI テストは、N = 1 の seed [1, 2026] と、仮想の driver の上の N = [2, 8] の `Sequential` で流す (`crates/eml_cli/tests/ui.rs:275-345`)。`async/` の worker の run は `placed_tasks > 0` を確かめる
  - 救済を強いる方針はまだない。`crates/eml_cli/tests/cli.rs:644` は `--sched rescue-always` に exit 2 を、`crates/eml_vm/tests/api.rs:109` は `"rescue-always"` に解析の誤りを期待する
  - CLI の `--sched` は `run` と `repl` のフラグで (`crates/eml_cli/src/main.rs:34-40, 54-60`、既定は `normal`)、`Sched::from_str` (`sched.rs:101-118`) で読み、`normal`、`sequential`、`seeded:<n>` だけを受ける。`main.rs` は `RunConfig::with_sched(sched)` (`main.rs:119`) と `terminal::run(debug_heap, sched, file)` (`main.rs:90`) に渡す。`FromStr` を使うのは CLI と `crates/eml_vm/tests/api.rs` の `a_sched_parses_and_rejects` だけである
  - `--sched` を使うテストと文書は次のとおりである。
    - `crates/eml_cli/tests/cli.rs:596-660` の `sched_flag_is_accepted_and_rejected` (引数の分け方も確かめる)。値を取るフラグの分け方は `workers_flag_is_accepted_and_rejected` (`cli.rs:668-`) も確かめる
    - `cli.rs:1299, 1338, 1396` のシグナルと標準入力のテストは `--workers 1 --sched sequential` で流す
    - `crates/eml_cli/tests/terminal.rs:67-79` の `repl_runs_par_under_a_seeded_schedule`
    - `crates/eml_cli/tests/ui.rs:360` の `seeded_label` は、食い違いを再現するコマンドとして `eml run --debug-heap --sched seeded:{seed}` を出す。`ui.rs:388` の文と、`ui.rs:750, 770` の `should_panic(expected = …)` もこの綴りを含む
    - 文書: docs/spec/stdlib.md:12, 18-20 (「起動」)、docs/spec/repl.md:7、docs/implementation/architecture.md:416-417、docs/implementation/testing.md:473, 491, 697、docs/future/multicore.md:528-531, 545、docs/future/async.md:373、CLAUDE.md の Commands の `eml run` の行
- `RunStats` に worker と置き方の欄はない。`Finished` は `preemptions`、`workers`、`placed`、`placed_tasks`、`placed_jobs` を持つ。
- `cells.rs` は `std` と `eml_rt` だけを import する (`crates/eml_vm/tests/tasks.rs` が確かめる)。
- 基準のテスト: `nix develop -c cargo nextest run` → `4284 tests run: 4284 passed, 20 skipped` (macOS aarch64、2026-11-04 に計画の側で流した)。

## 4. 前提の確かめ

| 要るもの | 用途 | 確かめ方 | ないとき |
|---|---|---|---|
| devShell (nix + direnv) | すべて | `cargo --version` → `cargo 1.99.1` | 止めて利用者に聞く |
| macOS (aarch64) | 計測 (T4) | `uname -sm` → `Darwin arm64` | 表の入れ替えを `Measure async-rescue` の別のコミットに回す (roadmap.md の「進め方」) |
| x86_64 Linux の nix の環境 (例: クラウドのセッション) | TSan (T3b、全体) | `uname -sm` → `Linux x86_64` | T3b のコミットのメッセージに「TSan は CI で確かめる」と書き、T4 の前に CI の TSan の job が通ったことを確かめる |
| 基準の結果 | 期待する要約の行 | 9 節の基準 | 9 節を参照 |

## 5. 決めたこと

### 5.1 長期の方針 (変えるには見直しが要る)

実装がこの方針とぶつかったら、止めて報告する。

#### D-1: 盗めるのはフレームを持たない仕事だけで、実行待ちの列は欄の lock の下の1本にする

- 決めたこと:
  - `Scheduler.ready` をやめ、`Run` の欄に `Slot.ready: Mutex<VecDeque<Ready>>` を置く。受け箱 (`Slot.state`) とは別の mutex にする。workers.rs に次を置く。`Ready` と `Entry` は、公開の `Policy` の引数に出るので (`private_interfaces`)、`pub` にして lib.rs から再公開する。`Entry` の欄と `EntryKind` は `pub(crate)` のままにする (実装の中で決めた)。
    ```rust
    pub(crate) enum Ready { Task(TaskId), Entry(Entry) }
    pub(crate) struct Entry { pub cell: CellRef, pub worker: u32, pub kind: EntryKind, pub word: Word, pub rescued: bool }
    pub(crate) enum EntryKind { Start, Resume { wake: i64 }, ResumeIo { wake: i64, op: i64 } }
    ```
  - `Ready::Task` は持ち主の表にあるフレームを持つ VM の `Task` (`par` の右の子、横取りしたタスク、起きた `par` の親、`Async.run` で待つ `Task`) で、盗まない。`Ready::Entry` はフレームを持たず、列で盗めるのはこれだけである。エントリの `Task` (`Task::entry`、`vm.rs:171-213`) は、持ち主が `pick` の時点で作る。
  - 盗めるもの (元の worker を `victim` とする):
    - (a) 受け箱の `Notice::Place`。条件は `phase == Placed && owner == victim`
    - (b) 受け箱の `Notice::Wake(Woken::Task { op: None, .. })`。条件は `phase == Parked && wait == None && k.is_some() && entries == 0 && owner == victim`
    - (c) 列の `Ready::Entry { rescued: false }`。`Start` なら (a) の条件、`Resume` なら `phase == Parked && matches!(wait, None | Some(Wait::Ready)) && k.is_none() && entries == 0 && owner == victim`。`Wait::Ready` の再開 (譲り) も盗める。CPU を使い続ける worker にたまるのはこれだからである
  - 盗まないもの: `Ready::Task`、`Entry { rescued: true }`、`ResumeIo`、`Woken::Token`、`Woken::Task { op: Some(_) }`、`Child`、`Joined`、`Cancel`、`Submit`、`CancelIo`、`Completion`、`Rescue`、待ちの登録 (`Wait::Cell | Time | Io | Signal | Job`) を持つタスク、セルの持ち主が `victim` でない項。
  - `rescued` は救済した worker の `collect` で付け、その項を `pick` するまで残る。その間はもう一度盗まない。動いてまた待ったタスクは、ふつうのタスクとして盗める。
  - `Async.run` の根のタスクも、ほかのタスクと同じに盗める。worker から動かないのは、スタックを持って待つ `Task` だけである。
- 理由:
  - フレームと I/O の記録は持ち主の worker に縛られる。始まったタスクを盗む形は、multicore.md の「却下した案」で退けている。
  - 1本の列なら、今の FIFO (横取りしたタスクと届いた継続の順) がそのまま保てる。盗む側は列の途中から外し、持ち主は先頭から取るので、mutex がいちばん小さい正しい形である (multicore.md:125)。
- 退けた案:
  - 列を2本にする: FIFO を戻すのに通し番号が要る。
  - スレッド局所の列を `gather` で公開する: 帳簿が二重になる。
  - 受け箱だけから盗む: 過負荷の worker の仕事は列にたまっているので、救えない。
  - `rescued` の項も `Normal` で盗み直す: `steal` が方針を知る必要がある (U-10)。
- 及ぶ範囲: `steal`、方針、`par-steal`、`main-pinning`。
- 見直す条件: `par-steal` で `par` の子を deque に移すとき (U-9)。
- 適合の確かめ:
  - MUST 救済は `Place`、`Wake(Woken::Task{op: None})`、`Ready::Entry { rescued: false }` の `Start` と `Resume` で、セルの持ち主が `victim` のものだけを動かす (G-1)。`steal_leaves_resume_io_rescued_entries_tokens_and_par_tasks` と `steal_helpers_refuse_a_cell_owned_by_a_third_worker` で確かめる。
  - MUST `par` の子、`compute` の仕事の `Task`、横取りした `Task`、`Async.run` で待つ `Task` は `Ready::Task` のままにする。`steal` は `Ready::Task` の中身を見ない (G-12)。
- リポジトリに残す所: vm.md の「タスクとスケジュール」と「救済」(T2)。

#### D-2: 待っているタスクを動かせるのは `entries == 0` のときだけである

- 決めたこと:
  - `TaskCell.entries: u32` を足す。そのセルのエントリの `Task` のうち、まだ `Finish` していないものの数である。
  - `started` と `resumed` (どちらも `enter` から呼ぶ。`vm.rs:2988-2998`) が 1 増やす。
  - `entry_finish` (`vm.rs:3012`) は、`task.outcome.is_none()` (待ちに入って終わった) のとき `task.cell` を `Scheduler.left: Vec<CellRef>` に積む。
  - 持ち主は次の `gather` の最初の登録表の区間の始めで `Registry::leave(&mut Vec<CellRef>) -> Result<usize, CellFault>` を呼び、セルごとに 1 減らす。0 のセルを減らすと内部の誤りにする。返り値は 0 になったセルの数で、`gather` はそれを `Slot.settled: AtomicU64` に足す。
  - `idle_wait` の前 (`vm.rs:1573`) と worker が抜けるとき (`workers.rs:671`) に `debug_assert!(sched.left.is_empty())` を置く。
- 理由:
  - `Ret` が安全点なので、`park` の後も待ったタスクのフレームが横取りされたまま残る。そのフレームは継続と同じ物体を手放す。
  - すぐ再開する待ちでは、同じセルのエントリの `Task` が2つ生きる。真偽値の印では「生きたフレームがない」を表せない (レビューで見つかった反例)。
  - 数が 0 になった持ち主の区間と救済の区間は、登録表の lock で順序が付く。`entry_finish` の後にはいつも `switch_away` → `collect` → `gather` が続くので (`vm.rs:3066, 1545`)、減らすのに追加の lock は要らない。
- 退けた案:
  - セルごとの真偽値 `left`: 2つのエントリで壊れる。
  - worker ごとの世代: セルごとの生死を表せない。
  - `entry_finish` で lock を取って減らす: 待つたびに lock が1つ増える。
- 及ぶ範囲: `steal_*`、`entry_finish`、`gather`、`idle_wait`、worker の終わり、将来のネイティブのエントリ。
- 見直す条件: エントリの `Task` を作るか終える道を足すとき。
- 適合の確かめ:
  - MUST NOT `entries > 0` のタスクの持ち主を変える。`entries` を増やすのは `started` と `resumed` だけで、減らすのは `Finish` の後に持ち主が `leave` で行う。眠るときと抜けるときに `Scheduler.left` は空である (G-2)。`two_live_entries_keep_a_parked_task_unstealable` と、`steal_resume` と `steal_wake` の成功の道の `debug_assert_eq!(entries, 0)` で確かめる。
- リポジトリに残す所: vm.md の「セルと登録表」と「救済」(T2)。

#### D-3: 救済は印を付けず、`eml_rt` を変えない

- 決めたこと:
  - 盗む手順は `share`、`transfer_back`、`set_worker` を呼ばず、確保もせず、ヒープ、`Ctx`、driver に触れない。
  - 盗んだタスクは自分の論理的な worker ID (`TaskCell.worker`) のまま、救済した worker の上で `Ctx::switch_task` で動く。
  - 盗んだ `Start` は今と同じく `started` → `transfer_back(閉包, spawner)` で入る。
  - `share` に送り手の引数は足さない。multicore.md の「救済」の印付けの文 (MC:159 ほか) は、閉じるときに書き直す (T4)。
- 理由:
  - 印付けが要るのは、2つのスレッドが同じ物体に触れうるときである。救済の後は、印のない物体に触れるスレッドは1つだけである。理由は次の3つになる。
    - タスクをまたぐ受け渡しはどれも印を付ける
    - `entries == 0` なのでフレームが残っていない
    - Rust の側の構造は数えた参照しか持たない
  - その結果の状態 (worker A のページにある rc > 0 の物体を、タスクの ID T が持ち、worker B が T の名で触れる) は、今の置いたタスクが `transfer_back` の後に置かれる状態と同じである。確保器、遠隔の解放、run の終わりの検査はこの状態を受け付ける (3 節)。
  - 印を付けると、救済したタスクの作業の集合が共有になり、reuse を失う。これは Tokio の形の盗みを退けた理由 (MC:177) と同じ費用である。送り手の引数には `share_walk` の中で与える意味がなく、Verus の仕事だけが増える。
- 退けた案:
  - `share(k, T)` を救済する worker で呼ぶ: 安全性は増えず、reuse を失う。
  - `share(w, receiver, sender)`: 意味を与えられない。
- 及ぶ範囲: `steal`、`steal_*`、検査モード、ネイティブの救済。
- 見直す条件: 印を付けない受け渡しの道 (タスクをまたぐ値の道) を足すとき。前提 (印のない物体は1つのタスクからしか届かない、MC:160) が崩れるからである。
- 適合の確かめ:
  - MUST NOT 盗む手順が `share`、`transfer_back`、`set_worker` を呼び、確保し、語のヘッダに触れる。救済したタスクは論理的な ID を保つ (G-4)。
  - `git diff --stat f969e3c0 -- crates/eml_rt` が空であること。
  - 印を付けないことの証拠は、次の検査モードの成功するテストである。multicore.md:549 の失敗するテスト (「印を付けずに救済すると `WrongOwner`」) は、作りから失敗しえないので置かない。
    - `sched::tests::a_rescued_task_runs_under_its_own_id` (T2)
    - `workers::a_rescued_task_holding_a_lin_resource_passes_the_checks` (T3b)
    - `workers::rescue_always_rescues_every_started_task`。N = 2 と 4 で流し、4 では spawner、元の worker、救済する worker がみな違う (T3b)
    - `async/` の UI テストの `RescueAlways` の run (T3b)
- リポジトリに残す所: vm.md の「救済」と「並列化への備え」、object-model.md の「論理的な worker ID」(T2)。future の文書の書き直し (T4)。

#### D-4: 持ち主の変更、lock の順、回送

- 決めたこと:
  - `TaskCell.owner` は、登録表の lock の下で、`Registry::start` と `steal_*` だけが書く。書き換えは `victim` から `me` への変更だけである。
  - 持ち主だけの欄 (`phase`、`k`、`wait`、`shield`、`budget`、`waiters`、`entries`) は、今の持ち主が登録表の lock の下で触れる。ほかに触れるのは、持ち主を変える区間の `steal_*` だけである。
  - lock の順は、登録表 > 欄の `state`、登録表 > 欄の `ready` である。`state` と `ready` は入れ子にせず、2つの worker の欄の lock を同時に持たない。
  - `Slot.ready` を持つ間は、ほかの lock を取らない。`steal` の外では、`Registry`、`Mail`、`Run::send`、driver を呼ばない。`steal` だけは登録表の lock を持ったまま `Registry` の helper を呼んでよい。
  - 救済した worker は、登録表と元の worker の lock をすべて放してから、自分の列に積む。
  - 持ち主でなくなったタスクの知らせか mail の `Cancel` を取り出した worker は、登録表の lock の下で今の持ち主を読み、そこへ回す。自分では処理しない (既存の回送をそのまま使う)。
  - 盗んだタスクは待ちの登録を持たないので、時間の輪、channel と シグナルの待ち手の列、操作の記録、仕事のセルのどれも書き換えない。
- 理由: 時間の区画と待ちの列は持ち主で引く (`cells.rs:1300, 1445`)。lock の順に輪がなければ、救済が交差しても deadlock しない。
- 及ぶ範囲: cells.rs、`steal`、`collect`、`Run` の列の操作のすべての閉包。
- 見直す条件: 登録表の lock を分けるとき。
- 適合の確かめ:
  - MUST `owner` の変更は登録表の lock の下で `Registry::start` と `steal_*` だけで行う (G-3)。`grep -n "\.owner = " crates/eml_vm/src/cells.rs` に、それらとテストの helper だけが出ることで確かめる。
  - MUST 持ち主だけの欄は今の持ち主か `steal_*` だけが触れる (G-6)。
  - MUST lock の順は登録表 > `state`、登録表 > `ready` で、`state` と `ready` は入れ子にしない。救済した worker が自分の列に積むのは lock をすべて放した後である (G-5)。TSan の `workers::` で確かめる。
  - MUST 古い持ち主に届いた知らせと `Cancel` は今の持ち主へ回す (G-7)。既存の回送のテストと `a_stale_cancel_in_the_victim_mail_reaches_the_new_owner` で確かめる。
  - MUST `Slot.ready` を持つ間はほかの lock を取らない。`steal` の外では `Registry`、`Mail`、`Run::send`、driver を呼ばない (G-13)。閉包のどれも `sched.cells` を捕まえないことを読んで確かめる。
- リポジトリに残す所: vm.md の「セルと登録表」と「救済」(T2)。

#### D-5: 盗む手順 `steal` の形

- 決めたこと: sched.rs の `take_async` の隣に置く (cells.rs は `Run` と `Notice` を知らない)。
  ```rust
  // crates/eml_vm/src/workers.rs
  /// worker `victim` の仕事を救済してよいか。今はいつも真である (docs/spec/vm.md の「救済」)。
  pub(crate) fn rescuable(run: &Run, victim: usize) -> bool { let _ = (run, victim); true }
  /// 1回の救済で見る項の数の上限。実行待ちと受け箱のそれぞれに掛かる。
  pub(crate) const STEAL_SCAN: usize = 1024;

  // crates/eml_vm/src/sched.rs
  pub(crate) struct StealResult { pub victim: usize, pub taken: usize, pub truncated: bool }

  /// 救済する worker `here.me` が、worker `victim` の実行待ちと受け箱から盗めるものを古い順に高々 `budget` 個取り、
  /// それぞれのタスクのセルの持ち主を自分にして、走らせる仕事として `out` の末尾に積む。
  pub(crate) fn steal(
      cells: &mut Registry,   // 登録表の lock の下 (呼ぶ側の MutexGuard から借りる)
      run: &Run,
      here: &Here,            // here.me = 救済する worker
      victim: usize,
      budget: usize,          // タスクの数。usize::MAX はすべて
      out: &mut Vec<Runnable>,
  ) -> Result<StealResult, CellFault>;
  ```
  - 前提: 呼ぶ側は登録表の lock を持ち、欄の lock を持たない。`victim < run.slots.len()` で、`victim == here.me` は `CellFault::Internal` にする。worker `here.me` 自身のスレッドで、`gather` の最初の登録表の区間の中から呼ぶ。
  - 手順:
    0. `!rescuable(run, victim)` なら、欄の lock に触れずに `{ victim, taken: 0, truncated: false }` を返す。
    1. `victim.ready` の lock を取る。先頭から `min(len, STEAL_SCAN)` 個を見る (`truncated = len > STEAL_SCAN`)。
       - 1回目の走査: `!e.rescued` の `Ready::Entry(e)` について、`Start` なら `cells.steal_start(e.cell, victim, here.me)?`、`Resume` なら `cells.steal_resume(e.cell, victim, here.me)?` を呼ぶ。`ResumeIo` は飛ばす。真が返った項 (持ち主はもう `here.me`) の添字を覚え、`budget` 個に達したら止める。
       - 2回目の走査: 覚えた項を `Runnable` として `out` に移しながら列を詰める (添字を数える `retain`)。語はそのままで、RC は変えない。`ready_entries` を移した数だけ減らす。
       - 予算か上限の先の項には触れない。lock を放す。
    2. 予算が残っていれば `victim.state` の lock を取る。先頭から `min(len, STEAL_SCAN)` 個の知らせを、同じ2回の走査で見る (`truncated |= …`)。
       - `Place { cell, worker, closure }` は `steal_start` が真なら `Runnable::Start { cell, worker, closure }` にする。
       - `Wake(Woken::Task { cell, wake, op: None })` は `steal_wake` が `Some((k, worker))` を返せば `Runnable::Resume { cell, worker, k, wake }` にする。
       - ほかの知らせは順を保って残す。`inbox_work` を外した数だけ減らし、`notices` が空になれば `has_notices = false` にする。lock を放す。
    3. `StealResult` を返す。`out[before..]` の順は、列の項を列の順に、次に知らせを届いた順に並べたものである。
  - helper が `Err` (死んだセルか種類の違い、`CellFault::Internal`) を返したら、`steal` はすぐ返す。それまでに持ち主を変えた項は元の構造に残るが、これは run の内部の誤りで、run はそこで終わる。worker k ≥ 1 では `fatal` と END のビットで終わるので、残った項が `pick` されることはない。
  - 後の状態:
    - `out` に積んだタスクはどれも `owner == here.me` で、`entries == 0` である。`Resume` ならセルの `k == None`、`phase == Parked`、`wait ∈ {None, Some(Wait::Ready)}` で、`Start` なら `phase == Placed` である。
    - 盗まなかったタスクの欄は変わらない。語は増えも減りもしない。残った項の順は変わらない。
  - Registry の helper (cells.rs、`pub(crate)`、lock の下、ヒープに触れない):
    ```rust
    fn steal_start(&mut self, cell: CellRef, victim: u32, me: u32) -> Result<bool, CellFault>;
    //  Placed && owner == victim → owner = me, true; else false
    fn steal_resume(&mut self, cell: CellRef, victim: u32, me: u32) -> Result<bool, CellFault>;
    //  Parked && wait ∈ {None, Some(Ready)} && k.is_none() && entries == 0 && owner == victim → owner = me, true; else false
    fn steal_wake(&mut self, cell: CellRef, victim: u32, me: u32) -> Result<Option<(Word, u32)>, CellFault>;
    //  Parked && wait == None && k.is_some() && entries == 0 && owner == victim → owner = me, Some((k.take(), worker)); else None
    fn leave(&mut self, cells: &mut Vec<CellRef>) -> Result<usize, CellFault>;
    //  entries -= 1 for each (Internal at 0); returns the number of cells that reached 0; drains the Vec
    ```
- 理由: 盗む仕事を、世界で1つの登録表の lock の下で O(min(len, STEAL_SCAN)) に抑える。1項ずつ `remove(i)` すると2乗になる。
- 及ぶ範囲: `steal` と、それを呼ぶ `gather` と T3a の後始末。
- 見直す条件: 計測で `STEAL_SCAN` を変えるとき (U-1)。
- 適合の確かめ:
  - MUST `steal` は構造ごとに高々 `STEAL_SCAN` 個を2回の走査で見る。予算か上限の先の項には触れず、`truncated` を返し、helper の `Err` はすぐ返す (G-14)。`steal_reports_truncation_past_the_scan_cap` で確かめる。
  - MUST `rescuable(run, victim)` を `steal` の中で、欄の lock の前に見る。その値は `Policy::set_rescuable` で各方針に届き、予約の判断はその値を使う (G-11)。
- リポジトリに残す所: vm.md の「救済」(T2)。

#### D-6: 列の操作と公開する数

- 決めたこと:
  - `Slot.ready` に触れるのは、次の `Run` のメソッドと `steal` だけである。
    ```rust
    impl Run {
        pub(crate) fn push_ready(&self, k: usize, item: Ready);
        pub(crate) fn pick_ready<P: Policy>(&self, k: usize, policy: &mut P) -> Option<Ready>;
        pub(crate) fn wake_ready<P: Policy>(&self, k: usize, policy: &mut P, id: TaskId);
        pub(crate) fn retain_ready(&self, k: usize, keep: impl FnMut(&Ready) -> bool);
        pub(crate) fn ready_is_empty(&self, k: usize) -> bool;
    }
    ```
  - `Policy::pick(&mut self, &mut VecDeque<Ready>) -> Option<Ready>`、`Policy::wake(&mut self, &mut VecDeque<Ready>, TaskId)` は今の意味を保つ。`Async` の項はどの方針でも `push_back` なので、`Entry` の間の列の順は届いた順である。方針は数に触れない。
  - `Slot.ready_entries: AtomicUsize` は、列の `Ready::Entry` のうち `rescued == false` で、種類が `Start` か `Resume` のものの数である。上のメソッドと `steal` が、列の lock の下で正しく保つ。`ResumeIo` と `rescued` の項は数えない。この数は、`RescueAlways` の予約の数と、監視の目安に使う。
  - 公開する atomic (どれも Relaxed) は次の3つである。
    - `ready_entries`: S1 の部分 (T1、T2) が書く
    - `inbox_work`: `Run::send` が `Place` と `Wake(Woken::Task { op: None })` で 1 足し、`take_notices` が 0 にし、`steal` が減らす (T3a)
    - `settled`: `gather` が `leave` の返り値を足す。単調に増える (T2)
  - 列の数の確かめは、`#[cfg(test)]` の `Run::count_entries(k)` を使うテストで行う。速い道には O(n) の確かめを置かない。
- 理由: `RescueAlways` の頼みと監視の目安を O(1) にし、速い道に O(n) の仕事を入れない。この数の定義は、計画をまとめるときに決めた (付録)。
- 及ぶ範囲: workers.rs、sched.rs、vm.rs、`steal`、監視。
- 見直す条件: 盗める種類を増やすとき (U-7、U-8)。
- 適合の確かめ:
  - MUST `Slot.ready` は上のメソッドと `steal` だけで触れる。それらは `ready_entries` を上の定義の数に保つ。`steal` は外した `Place` と `Wake` の数だけ `inbox_work` を減らす。速い道に O(n) の確かめを置かない (G-8)。`ready_entries_follows_push_pick_and_retain` と `steal_decrements_inbox_work_and_ready_entries` で確かめる。
  - MUST `settled` は、持ち主が `leave` で 0 にしたセルの数だけ増やす。空振りの印は `(ready_entries, inbox_work, settled)` で引き、走査が切れた後には立てない (G-15)。`a_flush_adds_settled_cells_to_the_slot` と `a_futile_victim_is_retried_after_its_entries_settle` で確かめる。
- リポジトリに残す所: vm.md の「タスクとスケジュール」(T2、T3a)。

#### D-7: 救済は `RunStats` を変えず、N = 1 では起きない。操作の待ちの仕組みはこの段で変えない

- 決めたこと:
  - `RunStats` に欄を足さず、`bench.rs` のスナップショットは1バイトも変えない。数は `Finished.rescues` と `Finished.rescued` に置く (D-14)。
  - N = 1 では、どの方針も救済しない。
  - 操作の記録の持ち主 (`OpRecord.owner`) と `Completion` の道は変えない。
  - タスクが Done でない操作の記録を持つ間は、操作の待ちのほかの `Async` の操作をしない (G-9)。std では受け付けと待ちが隣り合い、間にあるのは `op_ready` が Done を言った後の予算の譲りだけである (`cells.rs:2914-2928`)。
  - Done の記録を持つ `Wait::Ready` の再開を盗んでも安全である。理由は次の2つになる。
    - `take_op` は持ち主を確かめない (`cells.rs:3320-3346`)
    - `settle_op` の持ち主の確かめ (`cells.rs:3053`) には `arrival` があるときにしか届かない
- 理由: seed とスナップショットを保つ。救済したタスクが持つ記録の古い `owner` は、記録が Done の間だけ害がない。
- 及ぶ範囲: 監視、方針、std の行、将来の std とネイティブの std。
- 見直す条件: 操作の待ちを救済するとき (U-8)。
- 適合の確かめ:
  - MUST NOT 救済が `RunStats` を変える。MUST N = 1 で救済が起きない (G-10)。`bench.rs` が変わらないこと、seed の run が変わらないこと、`rescue_always_at_one_worker_is_sequential` で確かめる。
  - MUST NOT タスクが Done でない操作の記録を持つ間に、操作の待ちのほかの `Async` の操作をする (G-9)。std/Async.em と std/Net.em を読んで確かめる。
- リポジトリに残す所: vm.md の「救済」と「`RunStats`」(T2、T3a)。

### 5.2 短中期の設計

理由があれば実装の中で変えてよい。変えたらコミットのメッセージに書く。

#### D-10: `gather` の呼び出しの形

T2 が書く。T3a は `request_rescue` と `after_steal` の中身だけを書く。

```rust
// first registry section
let mut cells = shared.lock();
let settled = cells.leave(&mut self.left)?;
if !notices.is_empty() { take_async(&mut cells, notices, &here, ctx, depth_offset, &mut out, &mut work)?; }
if let Some((victim, budget)) = self.rescue.take() {
    let before = out.len();
    let result = steal(&mut cells, run, &here, victim, budget, &mut out)?;
    self.stolen.extend(out.drain(before..));
    self.steal_result = Some(result);
}
cells.watch(control); …
drop(cells);
run.slots[k].settled.fetch_add(settled as u64, Ordering::Relaxed);
if self.steal_result.is_some() { self.after_steal(); }
```

- `collect` は lock をすべて放した後、`runnables` を `rescued: false` で、続けて `stolen` を `rescued: true` で積む。
- `Scheduler` に足す欄: `rescue: Option<(usize, usize)>` (victim、予算。`request_rescue` が書く)、`stolen: Vec<Runnable>`、`steal_result: Option<StealResult>`、`left: Vec<CellRef>`。
- `Runnable::Start/Resume/ResumeIo` に `worker: u32` を足す。セルを lock の下で持っている所 (`start`、`take_wake` `cells.rs:1343`、`park*` のすぐ戻る道、`settle_op`) で埋めるので、登録表の読みは増えない。`schedule` (`vm.rs:2917-2973`) は登録表の lock を取らなくなる。

#### D-11: 誰が救済を頼むか (`Policy::RESCUE`)

- `trait Policy` (sched.rs) に `const RESCUE: Rescue` (`enum Rescue { Never, Watched, Always }`)、`fn workers_started(&mut self) {}`、`fn set_rescuable(&mut self, _: bool) {}` を足す。

| 方針 | `RESCUE` | `PREEMPTS` | `pick` / `wake` |
|---|---|---|---|
| `Normal` | `Watched` (監視のスレッドが頼む) | true | 今のまま |
| `Sequential` | `Never` | false | 今のまま |
| `Seeded` | `Never` | false | 今のまま |
| 新しい `RescueAlways` (ライブラリの値だけ) | `Always` (仕事の持ち主が頼む) | false | `Sequential` と同じ。ただし予約する間、`pick` は予約した項を飛ばす |

- `RescueAlways { active: bool, rescuable: bool }` は、どちらも偽で作る。
  - worker k ≥ 1 では `for_worker(k)` が `active: true` を返す。worker 1..N-1 は作れたときにだけあるので、このとき `live ≥ 2` である (`workers.rs:560-590`)。
  - worker 0 は、`set_live` (`lib.rs:945-946`) の直後に `workers_started()` を1回呼んで `active` を立てる。`set_rescuable(rescuable(&run, k))` は、各 worker の `Scheduler` を作るとき (worker 0 はこの位置、worker 1..N-1 は `Scheduler::worker`) に呼ぶ。
  - 予約するのは `active && rescuable` の間だけで、run の中でどちらも偽に戻らない。
- 別の型にするので、`Sequential` の写しに枝は増えない。`vm::run` のモードごとの写しが1つ増える。

#### D-12: 監視のスレッドの判定 (`Watched` のときだけ)

- 監視は atomic だけを読み (数は Relaxed、`futile` は Acquire)、登録表の lock も欄の lock も取らない。
- 目安は `h(k) = ready_entries(k) + inbox_work(k)` である。次の項目も数えてしまうが、取るものは `steal` が決め、多く数えた分は空振りの印が吸収する。
  - まだ `entries > 0` のタスクの項
  - 持ち主がもう `victim` でない知らせ
  - 走査の上限の先の項
- 空振りの印: 欄に `futile: AtomicBool` と `futile_ready: AtomicUsize`、`futile_inbox: AtomicUsize`、`futile_settled: AtomicU64` を置く。`futile(k)` が立ち、今の3つの数が覚えた値と等しい間は、k を飛ばす。
- `Ticker` に `still: u32` (切り替えの数が続けて変わらなかった刻みの数) と `fn stuck(&self) -> bool { self.still >= STUCK_TICKS }` を足す。`tick(switches) -> bool` の意味は変えない。monitor.rs の定数は `STUCK_TICKS = 2`、`OVERLOAD = 64` で、`TICK` は 10 ms のままにする。
- 刻みごとに、横取りのループの後で `Relay::rescue_tick(&mut tickers, &mut cursor)` を回す。回すのは `Monitor::start(…, preempt, rescue)` が `rescue = (P::RESCUE == Rescue::Watched)` を受けたときで、`run.live() ≥ 2` の間だけである。
  ```
  for k in rotation from cursor over 0..live:
      h = ready_entries(k) + inbox_work(k);                if h == 0: continue
      if futile(k) && (ready_entries, inbox_work, settled)(k) == stored(k): continue
      if !(tickers[k].stuck() || h > OVERLOAD): continue
      match run.pop_idle_except(k, live):
          Some(r) => { run.send(control, r, Notice::Rescue { victim: k }); cursor = k + 1 }
          None    => break                                 // no idle worker this tick
  ```
- `Run::pop_idle_except(victim, live) -> Option<usize>` は、`idle` の lock (末端の lock) の下で、`r != victim && r < live` のいちばん新しい `r` を取り出す。監視が2つの lock を同時に持つことはない。
- extern の中で止まった worker を見分ける旗は要らない。`switches` はタスクを切り替えたときにしか変わらない (`sched.rs:890-897`)。安全点を通る VM のタスクは、次のように切り替わるので、2刻み変わらないことはない。
  - 届いた仕事は、`NOTICE` の安全点で列に入る (`vm.rs:441-474`)
  - 列が空でなければ、次の `PREEMPT` で切り替わる (`vm.rs:481-496, 1693-1733`)

#### D-13: 頼みを受ける側 (`Notice::Rescue`)

- `Notice::Rescue { victim: u32 }` は語を持たず、`Run::send` で送る。`in_flight` に入れない。`steal` はこれを取らない。
- I4 の規則は「`idle` から1回取り出すごとに、`Child` か `Rescue` を1つ送る」になる。
- run の終わりでは仕事に数えない。`left_notices` (`workers.rs:937-960`)、`would_run` (`cells.rs:1390-1403`)、worker 0 の `take_slot` は `Rescue` を無視し、残ったものは捨てる。
- `receive` は `Rescue` ごとに `Scheduler::request_rescue(&mut self, victim: usize)` を呼ぶ。
  1. `stopping(control, me)` (止めるビットか `END`) なら捨てる。
  2. `self.rescue.is_some()` なら捨てる。
  3. `Always` なら `slots[victim].asked.store(false, Release)` を書く。
  4. `victim` の `(ready_entries, inbox_work, settled)` を盗む前に読み、`self.rescue_key` に覚える。
  5. 予算を決める。`Watched` では `rescue_budget(h) = max(1, h.div_ceil(2))` で、`Always` では `usize::MAX` である。
  6. `self.rescue = Some((victim, budget))` にする。
- `Scheduler::after_steal(&mut self)` は区間の後で、lock を持たずに動く。
  - `let r = self.steal_result.take()` で結果を取る。
  - `r.taken ≥ 1` なら `rescues += 1`、`rescued += r.taken` とし、`futile(r.victim)` を下ろす。
  - 何も取れず、`Watched` で `!r.truncated` なら、`rescue_key` を `futile_*` に書いてから `futile.store(true, Release)` を書く。
  - `r.truncated` なら `futile(r.victim)` を下ろし、印を立てない。
- 救済は、救済する worker が集めるところならどこでも起きる。`switch_away` の `idle_wait` の後、`NOTICE` の安全点、`preempt` の3つである。
- 元の worker が先に戻ったときは、`taken == 0` になり、空振りの印が次の頼みを止める。

#### D-14: `Finished` の数

- `Finished.rescues: u64` (1つ以上を盗んだ救済の数) と `Finished.rescued: u64` (救済で移した項の数) を足し、`Retired` を通して `preemptions` と同じく worker の和を取る。
- どちらも `RunStats` に入れず、`--stats` にも出さない (`placed` や `placed_tasks` と同じ扱い)。

#### D-15: `Sched::RescueAlways`

- `RescueAlways` はテストのための方針で、`eml_vm` の `Sched` の値としてだけ置く。CLI には出さない (D-16)。
- 予約するのは `active && rescuable` の間だけで、ほかのときは `Sequential` と同じに動く。
- 予約した項は `Ready::Entry { rescued: false, kind: Start | Resume, .. }` で、`ready_entries` が数えるものと同じである。`ResumeIo` は予約せず、持ち主が動かす。
  - まだ `entries > 0` のタスクの `Resume` も予約するが、まだ盗めない。生きているエントリの `Task` は同じ worker にあり、その `Finish` の後には必ず `collect` が続く。そこでの `leave` で `entries` は 0 になり、その後で下の頼みが動く。
- `pick`: `RescueAlways::pick(&mut VecDeque<Ready>)` は、予約した項でない最初の項を取る。`wake` は `Sequential` と同じく前に積む。
- 待つ判定: `pick` が `None` なら `idle_wait` に入る。待つかどうかは受け箱と `woken` だけで決まるので (`workers.rs:468-493, 757-766`)、予約した項が worker を起こしたままにすることはない。
- 頼み: `RescueAlways` の写しの `collect` の終わり (冷たい道) で、次を行う。
  1. `slots[k].ready_entries > 0` を読む。O(1) で、lock は要らない。
  2. 真で、`!slots[k].asked.swap(true, AcqRel)` なら、lock をすべて放した状態で `Rescue { victim: k }` を `r = (k + 1) % live` に送る。`live` は Acquire で読む。この頼みは `idle` から取り出さないので、I4 に関わらない。
- 頼まれた worker は `asked` を下ろしてから、予算 `usize::MAX` で盗み、取ったものを動かす。取った項はどれも `rescued: true` なので、もう盗まれない。そのため、方針が働き始めた後に始めるタスクと再開するタスクは、どれも動く前にほかの worker へ1回移る。根の `Async.run` のタスクは、worker を作る前に始まる。
- 半分でなくすべてを取る。半分だと、`asked` が下りたまま眠った持ち主に残りが残り、もう誰も頼まない。
- 走査が切れても、頼み直しは要らない。上限の数の項が予約した項の前にあるなら、持ち主には予約していない仕事があって起きているので、次の `collect` でまた頼む。
- 順序: 下ろすのは盗む lock の前、持ち主が積むのは `swap` の前である。そのため、盗んだ後に積まれた項は、`asked == false` の `swap` で見つかり、もう一度頼まれる。余分な頼みは空の盗みになるだけである。
- `rescuable == false` の worker では、何も予約せず、頼まない。

#### D-16: UI テストの run と、CLI に出さないこと

- `assert_workers_agree` は、`WORKER_COUNTS` の代わりに、分類ごとの表 `WORKER_RUNS` を読む。

| 分類 | run (検査モード、仮想の driver) |
|---|---|
| `run/par/`、`run-fail/par/` | `(2, Sequential)`、`(8, Sequential)`。今のまま。`par/` のファイルは `Async` のタスクを始めない |
| `run/async/`、`run-fail/async/` | `(2, Sequential)`、`(2, RescueAlways)`、`(8, RescueAlways)` |

- `run/async/` の `RescueAlways` の run では `finished.rescued > 0` を確かめる。外れたときの文は、置いたタスクを始めるようにファイルを直すことを促す。
- `-- workers: skip` のファイルは、3つの run をすべて飛ばす。
- CLI から `--sched` を外す (T0):
  - `eml run` と `eml repl` から `--sched` のフラグを消す。どちらもいつも `Sched::Normal` で動く (今の既定と同じ)。`--sched` はほかの知らないフラグと同じく、ファイルより前なら使い方の誤り (exit 2)、ファイルより後ろならスクリプトの引数になる。
  - `impl FromStr for Sched` を消す。使う所がなくなるからである。
  - `Sequential`、`Seeded`、`RescueAlways` は、`RunConfig::with_sched` と `Session`、`Machine` の API から渡すテストのための値になる。ライブラリの既定は今のまま `Sequential` である。`Sched::seed()` は `RescueAlways` で `None` を返す。
  - seed の run の食い違いは、CLI でなく UI テストを1つのファイルに絞って流して再現する (`EML_UI_FILTER=<file> cargo nextest run -p eml_cli --test integration -E 'test(=ui::run)'`)。`seeded_label` の文はこのコマンドを出す。
  - 理由: `Sequential`、`Seeded`、`RescueAlways` はどれも決定性と救済の道を確かめるためのもので、利用者に意味がない。CLI と文書の面が減り、`--sched` の値の誤りの規則も要らなくなる。
  - 退けた案: `rescue-always` だけを CLI に出さない。残る2つの値も利用者に意味がない点は同じである。隠しフラグか環境変数で残す: テストは API から渡せば足り、CLI の決まりが1つ増える。
  - 失うもの: CLI から手で seed の run を再現する道 (testing.md:697)。上の UI テストの絞り込みで置き換える。

## 6. 実装の制約と取り決め

- cells.rs は `std` と `eml_rt` だけを import する。各要素の置き場所は次のとおりである (D-5)。
  - `steal` と `StealResult` は sched.rs
  - `rescuable`、`STEAL_SCAN`、`Ready` の型、列のメソッドは workers.rs
  - `Registry` に足すのは `entries`、`steal_start`、`steal_resume`、`steal_wake`、`leave` だけ
- 盗む手順の中では、登録表の lock の下で `Ctx`、`Heap`、driver を呼ばない。cells.rs:5-9 の例外の一覧は増やさない (D-3)。
- `Run` が語を持つのは欄の lock の中だけで、`Ctx` もヒープも持たない。workers.rs:1-3 のコメントをこれに合わせて直す。`Ready: Send` と `Entry: Send` を const の確かめ (`workers.rs:24-31`) と `crates/eml_vm/tests/api.rs` の型の確かめに足す (D-1)。
- 列の操作の閉包は列だけを編集する。D-4 の lock の規則の対象は、次の全部の所である。
  - `switch_away` の `pick` と `is_empty` (`vm.rs:1549, 1567`)
  - `preempt` (`vm.rs:1707, 1713`)
  - 自分の worker に積む `par` の子 (`vm.rs:1267`)
  - `fork_job` の置けなかったとき (`vm.rs:2255`)
  - abort の `retain` (`vm.rs:2012`。`Ready::Task(id)` についての `retain_ready` になる)
  - `op_ready` の `FAST_YIELD` の確かめ (`vm.rs:2542`。`collect` が戻った後で行う)
  - `schedule` (`vm.rs:2917-2973`)
  - `receive` の `Child` (`sched.rs:776`)
  - `wake_parent` (`sched.rs:863`)
- 残った仕事の確かめは、`sched.tasks` に加えて列の `Entry` も見る。対象は `has_children` (`sched.rs:930`)、`main` の `Finish` (`vm.rs:2059-2068`)、worker の終わりの `left_notices` (`workers.rs:937-960`) で、残った `Entry` は `Child` と同じく仕事に数える。`Word` は `Drop` のない `Copy` なので (`crates/eml_rt/src/word.rs:23-25`)、捨てた run の列を落としてもヒープに触れない (D-1)。
- コードのコメントと `docs/spec/` は今の状態だけを書き、段の名前を出さない。`rescuable` のコメントは「今はいつも真である」にする。計画の ID (D-x、T-x) も引かない (roadmap.md の「進め方」)。
- 監視のスレッドが持つのは `Arc<Control>`、`Arc<Shared>` (mail)、`Arc<Run>`、`Arc<Watch>` だけである。取る lock は `idle` の lock と、`send` の中の欄の lock だけで、2つを同時に持たない (D-12)。
- テストのためだけの口は作らない (CLAUDE.md の「テスト」)。
- 日本語の文 (spec、コメント) は natural-japanese のスキルに従って書く (CLAUDE.md)。

## 7. 実装の作業

| ID | 作業 | 基づく決め | 先に要る作業 | 並べられるか |
|---|---|---|---|---|
| T0 | CLI から `--sched` を外し、`Sched::from_str` を消す | D-16 | - | いいえ (T1 の前に入れる) |
| T1 | 実行待ちの列を欄の lock の下へ移し、`Entry` を項にする | D-1, D-6, D-10 | - | いいえ |
| T2 | `entries`、`steal`、持ち主の変更と spec の節 | D-2, D-3, D-4, D-5, D-6, D-10 | T1 | いいえ |
| T3a | `Rescue` の知らせ、目安、空振りの印、`request_rescue`、`after_steal`、`Finished` の数 | D-6, D-7, D-13, D-14 | T2 | いいえ |
| T3b | `RescueAlways` と UI テストの表 | D-11, D-15, D-16, D-3, D-7 | T3a | いいえ |
| T3c | 監視のスレッドの判定 | D-12, D-11 | T3a | T3b と並べられる (触れるファイルが monitor.rs、workers.rs の `pop_idle_except` に限られる。workers.rs の衝突に注意) |
| T4 | 段を閉じる (`Close async-rescue`) | すべて | T3b, T3c | いいえ |

### T0: CLI から `--sched` を外し、`Sched::from_str` を消す

- 目的: テストのための方針を利用者の面から外す。後の T3b で足す `RescueAlways` も CLI に出ない形にしておく。
- 対象: `crates/eml_cli/src/main.rs`、`crates/eml_cli/src/terminal.rs` (`run` の引数)、`crates/eml_vm/src/sched.rs` (`FromStr`)、`crates/eml_cli/tests/{cli.rs,terminal.rs,ui.rs}`、`crates/eml_vm/tests/api.rs`、docs/spec/stdlib.md、docs/spec/repl.md、docs/spec/vm.md、docs/implementation/architecture.md、docs/implementation/testing.md、docs/future/multicore.md、docs/future/async.md、CLAUDE.md。
- 作業:
  - main.rs: `Run` と `Repl` から `sched` のフィールドと `#[arg]` を消す。`RunConfig` には `.with_sched(Sched::Normal)` を渡し、`terminal::run` は `sched` の引数をなくして中で `Sched::Normal` を使う。`split_script_args` は値を取るフラグを clap の定義から読むので (`main.rs:234`)、規則は直さない。
  - sched.rs: `impl FromStr for Sched` と、それだけが使う import を消す。
  - テスト (7 節の最後の表):
    - `cli.rs` の `sched_flag_is_accepted_and_rejected` を消す。引数の分け方の場合のうち、`workers_flag_is_accepted_and_rejected` にないもの (ファイルより後ろの値を取るフラグをスクリプトに渡す、`=` の形、ファイルのない run、2回書いたフラグ) は、`--sched X` を `--workers 1` に替えてそちらへ移す。`["run", "--sched", "normal", "main.em"]` → exit 2 と、`["run", "main.em", "--sched", "bogus"]` → exit 0、stdout `["--sched", "bogus"]` の2つを足す。
    - `cli.rs:1299, 1338, 1396` から `"--sched", "sequential"` を消し、`--workers 1` は残す。
    - `terminal.rs` の `repl_runs_par_under_a_seeded_schedule` を消す。
    - `api.rs` の `a_sched_parses_and_rejects` を消す。`the_default_sched_is_sequential` は残す。
    - `ui.rs` の `seeded_label` の文を ``seed {seed} in checked mode on the virtual driver; rerun with `EML_UI_FILTER=<file> cargo nextest run -p eml_cli --test integration -E 'test(=ui::run)'` `` の形にし (シグナルの印の注は残す)、`ui.rs:388` の文の `--sched seeded:{seed}` を `seed {seed}` にする。`ui.rs:750, 770` の `should_panic(expected = …)` を新しい文に合わせる。
  - 文書:
    - stdlib.md「起動」: l.12 の例を消し、l.18-19 の値を取るフラグの一覧から `--sched` を外し、例 `eml run main.em --sched bogus` を `--workers` の例に替える。l.20 の `--sched` の項を消す。`eml run` はいつも監視のスレッドの刻みで横取りする方針で動く、と1文で書く。
    - repl.md:7: 概要から `[--sched <normal|sequential|seeded:N>]` を消し、REPL も同じ方針で動くと書く。
    - vm.md「タスクとスケジュール」: `FromStr` と綴りの文を消し、次の文にする。
      > - 方針は `Sched` の値で選ぶ。`Normal`、`Sequential` (既定)、`Seeded(seed)` の3つで、`Copy` である。`RunConfig::with_sched` で渡す。`eml run` と `eml repl` はいつも `Normal` で動き、CLI でほかの方針は選べない。`Sequential` と `Seeded` は、決定的なテストのための方針である
    - architecture.md:416-417: `--sched` の文を消し、`eml run` と `eml repl` はいつも `Normal` で動くと書く。
    - testing.md: l.473 を `workers_flag_is_accepted_and_rejected` が分け方を確かめる文にする。l.491 の4本のテストは `--workers 1` で流すと直す。l.697 の再現のコマンドを、上の `EML_UI_FILTER` のコマンドに替える。
    - multicore.md:528-531, 545 と async.md:373: 方針を `--sched <綴り>` でなく `Sched` の値の名前 (`Normal`、`Sequential`、`Seeded`、`RescueAlways`) で書く。救済の中身の書き直しは T4 で行う。
    - CLAUDE.md の Commands: `run [--debug-heap] [--stats] [--sched normal|sequential|seeded:N] [--workers N]` から `--sched …` と、その説明 (`--sched picks how …`) を消す。`repl [--debug-heap] [--sched ...] [file.em]` を `repl [--debug-heap] [file.em]` にする。
- 実装者に任せる所: `sched_flag_is_accepted_and_rejected` から移す場合の書き方。移した後も、今の場合が確かめていた分け方の規則 (値を取るフラグ、`=` の形、ファイルより後ろの引数、ファイルのない run、2回書いたフラグ) が1つずつ残っていること。
- 条件: いつも。
- 完了の条件:
  - `cargo nextest run -p eml_cli --test integration -E 'test(=cli::workers_flag_is_accepted_and_rejected)'` → `1 test run: 1 passed`
  - `cargo nextest run -p eml_cli --test integration -E 'test(/^cli::/) | test(/^terminal::/) | test(/^ui::/)'` → `0 failed`
  - `cargo nextest run -p eml_vm --test integration -E 'test(/^api::/)'` → `0 failed`
  - `cargo run -p eml_cli -- run --sched normal x.em` → exit 2
  - `git grep -n -- '--sched' -- ':!docs/plans' ':!docs/reports' ':!.claude'` → `cli.rs` の新しい2つの場合だけ
  - `git grep -n 'FromStr' -- crates/eml_vm/src/sched.rs` → 何も出ない
  - `cargo nextest run` → `0 failed`。走った数は基準の 4284 から、消した2本 (`cli::sched_flag_is_accepted_and_rejected`、`terminal::repl_runs_par_under_a_seeded_schedule`) と `api::a_sched_parses_and_rejects` を引いたもの (4281)
  - `cargo nextest run -p eml_cli --test integration citations::` → `0 failed`
  - `cargo clippy --all-targets -- -D warnings` → exit 0、`cargo clippy -p eml_cli --no-default-features --features types -- -D warnings` → exit 0、`cargo fmt --check` → exit 0
- 落とし穴:
  - `cli.rs:1299, 1338, 1396` のテストは、`Normal` では眠る driver の run なので監視のスレッドが動き、横取りが起きうる。どのプログラムも、タスクが `Signal.wait` か `Stdin.read_line` に入るまでの数マイクロ秒の間に 10 ms の刻みで横取りされなければ、今と同じ順で動く。T0 の後に `for i in $(seq 20); do cargo nextest run -p eml_cli --test integration -E 'test(/^cli::/)' || break; done` を流し、20 回とも `0 failed` で終わることを確かめる。落ちたら止めて報告する。
  - `terminal.rs` の `repl_with` は `--sched` を渡さない形が残る。消すのはその1本だけである。

### T1: 実行待ちの列を欄の lock の下へ移し、`Entry` を項にする

- 目的: 盗む側が触れられる場所に、フレームを持たない仕事を自己完結した項として置く。
- 対象: `crates/eml_vm/src/workers.rs`、`cells.rs`、`sched.rs`、`vm.rs`、`crates/eml_vm/tests/api.rs`。
- 作業:
  - workers.rs: `Slot.ready`、`Slot.ready_entries`、`Slot.settled` を置く。`Ready`、`Entry`、`EntryKind` (`rescued` はここではいつも偽)、D-6 の列のメソッド、`count_entries` (`#[cfg(test)]`)、const の確かめ、`rescuable`、`STEAL_SCAN` を足す。
  - cells.rs: `Runnable` の各腕に `worker` を足す (D-10)。
  - sched.rs: `Policy::pick`、`Policy::wake` を `VecDeque<Ready>` の上にし、`Policy::set_rescuable` の既定を足す。`Scheduler.ready` を消し、`receive`、`wake_parent`、`has_children` を直す。
  - vm.rs: `schedule` は `Ready::Entry` を積む。`switch_away` と `preempt` は `Ready` を取り、`Entry` なら `Task::entry` と `add_task` で `Task` を作ってから `resume` する。6 節のほかの所も直す。
  - `Runnable::*` と `sched.ready` を使う cells.rs、sched.rs、vm.rs の単体テストを機械的に追随させる (種類 3)。
  - 新しい単体テスト: workers.rs `ready_entries_follows_push_pick_and_retain` (`count_entries` を使う。`ResumeIo` と `rescued: true` の項は数えないことも確かめる)。
- 実装者に任せる所: 列のメソッドの内部の書き方。`Seeded::pick` の添字で外す形は保つこと。
- 条件: いつも。
- 完了の条件:
  - `cargo nextest run` → `0 failed` で終わり、走った数は T0 の後の 4281 に新しいテストの数を足したもの
  - `cargo nextest run -p eml_vm --test integration -E 'test(bench::)'` → `0 failed`
  - `git diff --stat -- crates/eml_vm/tests/bench.rs tests/ui` → 何も出ない
  - `cargo clippy --all-targets -- -D warnings` → exit 0、`cargo fmt --check` → exit 0
- 落とし穴: `pick` の時点で `Task` を作るので、ある時点の `sched.tasks` の中身が変わる。`Task::entry` はスタックの長さを戻すので `RunStats` と `peak_stack` は変わらない見込みだが、bench のテストで確かめる。

### T2: `entries`、`steal`、持ち主の変更と spec の節

- 目的: 救済の核の手順を入れ、規則を spec に書く。まだ誰も頼まないので、振る舞いは変わらない。
- 対象: `cells.rs`、`sched.rs`、`vm.rs`、`workers.rs`、`docs/spec/vm.md`、`docs/spec/object-model.md`。
- 作業:
  - cells.rs: `TaskCell.entries` と、`started` と `resumed` での `+= 1` を足す。`leave`、`steal_start`、`steal_resume`、`steal_wake` を足し、`TaskCell.owner` の doc を「`start` と救済が登録表の lock の下で書く」にする。
  - sched.rs と vm.rs:
    - `Scheduler.left` を足し、`entry_finish` で `outcome.is_none()` のとき積む。
    - `gather` で `leave` を呼び、`settled` に足す (D-10)。
    - `idle_wait` の前と worker の終わりに `debug_assert!(left.is_empty())` を置く。
    - `steal` と `StealResult`、`Scheduler.rescue`、`stolen`、`steal_result`、`gather` の呼び出しを足す。`collect` は `stolen` を `rescued: true` で積む。
    - `request_rescue` と `after_steal` は空の stub にする (T3a が中身を書く)。
  - 単体テスト、cells.rs (`Registry` だけ):
    - `an_entry_count_follows_start_park_resume_and_finish`: start で 1、`park` の後も 1、`leave` で 0、すぐ戻る `ready` と `resumed` で 1、`leave` で 0、0 で `leave` すると `Internal`
    - `two_live_entries_keep_a_parked_task_unstealable`: すぐ再開する `park`、`resumed` で entries 2、`park_chan`、`leave` で 1、`steal_wake` は None、`leave` で 0、`steal_wake` は Some
    - `steal_helpers_refuse_a_cell_owned_by_a_third_worker`
  - 単体テスト、sched.rs (1つの `Run`/`Shared` の上の2つの `Scheduler`、1つのスレッド。`sched.rs:1396-1620` の作り方と `cells.rs:7383-7470` の helper を使う):
    - `steal_takes_a_place_and_the_new_owner_starts_it`
    - `steal_takes_a_wake_only_when_no_entry_of_its_task_is_live`
    - `steal_takes_start_and_resume_entries_in_queue_order_within_the_budget`
    - `steal_takes_a_yielded_resume`
    - `steal_leaves_resume_io_rescued_entries_tokens_and_par_tasks`
    - `steal_decrements_inbox_work_and_ready_entries`: 2つの数と `has_notices` を確かめる。`inbox_work` の増やし方は T3a なので、ここでは欄に直接書いてから盗む
    - `steal_reports_truncation_past_the_scan_cap`: `STEAL_SCAN + 1` 個の `ResumeIo` の後に `Start` を1つ置き、`taken == 0` と `truncated` を確かめる
    - `a_stale_cancel_in_the_victim_mail_reaches_the_new_owner`
    - `a_rescued_task_runs_under_its_own_id`: 検査モード。継続が印のない局所の物体を持つ待ったタスクを、2つ目の scheduler が盗んで再開する。持ち主の検査と漏れの検査が通る
    - `a_flush_adds_settled_cells_to_the_slot`
  - spec: 下の文を書く。文は natural-japanese のスキルで整えてよいが、規則の中身は変えない。
    - vm.md「タスクとスケジュール」の、`Scheduler` の列についての文を次に置き換える。
      > - 走れるタスクの列は run の表の worker ごとの欄 `Slot.ready` にあり、受け箱とは別の mutex の下にある。項は、この worker の表にある VM の `Task` の番号 (`Ready::Task`。`par` の右の子、横取りしたタスク、join で待つ親、`Async.run` で待つ `Task`) か、フレームを持たない `Async` の仕事 (`Ready::Entry`。始める仕事と再開する仕事で、セルの番号、タスクの ID、種類、語を1つ、救済で受けた印 `rescued` を持つ) である。列は1本で、方針の `pick` と `wake` はこの列に対して働く。列に触れるのは run の表の積む、取る、残す、盗むの操作だけである。欄の `ready_entries` は、列の `Entry` のうち救済で受けた印のない始める仕事と再開する仕事の数で、それらの操作が列の lock の下で書く。列の lock を持つ間はほかの lock を取らず、救済のほかでは登録表にも触れない。持ち主は `pick` で取った `Entry` からエントリの `Task` を作って入る。`Entry` だけが救済で動く (下の「救済」)
    - vm.md「`Async` のタスク」の導入 (vm.md:632) の「そこから動かない」を「救済されるまでそこから動かない」にする。
    - 「セルと登録表」(vm.md:645) の「`start` が1回だけ書く」を「`start` と救済が登録表の lock の下で書く」にし、セルの欄の一覧に「エントリの `Task` のうちまだ `Finish` していないものの数 (`entries`)」を足す。
    - 「エントリの `Task`」に、`pick` のときに作ることを書く。
    - 「取り消しの届け方」の後に、次の節を足す。
      > ### 救済
      >
      > - 救済は、空いた worker (救済する worker) が、ほかの worker (元の worker) の受け箱と実行待ちから、フレームを持たない仕事を引き取ることである。引き取るのは、受け箱の `Place`、受け箱の `Wake` のうち継続がセルにあって `op` を持たないもの、実行待ちの `Ready::Entry` のうち救済で受けた印のない始める仕事と `resume_task` で再開する仕事 (譲り `ready` の再開を含む) で、どれもセルの持ち主が元の worker のものに限る。VM の `Task` (`Ready::Task`)、`Async.run` の token、`resume_io` の仕事、救済で受けた印の付いた項、操作か仕事を待っているタスクと待ちの登録 (セル、時間、操作、シグナル、仕事) を持つタスク、持ち主がすでにほかの worker のセルの知らせ、`Completion`、`Submit`、`CancelIo`、`Cancel`、`Child`、`Joined` は引き取らない。いつ誰を救済するかは「タスクとスケジュール」の方針と監視のスレッドが決める。`Async.run` の根のタスクもほかのタスクと同じに引き取る。worker から動かないのは、スタックを持って待つ `Task` だけである。救済で受けた印は、救済した worker がその項を `pick` するまで残り、その間はもう一度引き取らない。動いてまた待ったタスクは、ふつうのタスクとして引き取れる
      > - 待っているタスクを引き取れるのは、そのセルの `entries` が 0 のときだけである。`entries` は、そのタスクのエントリの `Task` のうちまだ `Finish` していないものの数である。始める仕事と再開する仕事が動き始めるとき (`started`、`resumed`) に 1 増やし、待ちに入ったタスクのエントリの `Task` が `Finish` した後、持ち主の次の集める地点が登録表の lock の下で 1 減らす。待ちに入った命令の後にも `ret` の安全点があり、持ち主はそこで横取りされてほかのタスクを動かしうる。すぐ再開する仕事を列に積んだ待ちでは、前のエントリの `Task` がまだ残る間に、同じタスクの次のエントリの `Task` が動き始めることもある。そうしたフレームは継続と同じ物体を手放すので、`entries` が 0 になる前に継続を動かすとデータ競合になる。集める地点は、0 になったセルの数を欄の `settled` に足す。救済の判定はこの数も見る (「タスクとスケジュール」)。眠る前と抜ける前には、減らす前のセルが残っていないことを確かめる
      > - 救済は、救済する worker の集める地点の最初の登録表の区間の中で行う。登録表の lock を持ったまま、元の worker の実行待ちの lock を取って見終えたら放し、次に受け箱の lock を取って見終えたら放す。欄の lock を2つ同時に持つことはなく、欄の lock を持つ間に登録表の lock を取ることもない。見るのはそれぞれ先頭から `STEAL_SCAN` (1024) 個までで、予算 (引き取るタスクの数) に達したら止める。引き取る項をまず選んで持ち主 (`owner`) を自分にし、次に1回の走査で列から外す。`Wake` なら継続をセルから取り出す。取った仕事は、lock をすべて放した後に、救済で受けた印を付けて自分の列の後ろに積む。実行待ちの古い項から、次に受け箱の届いた順に取る。救済してよい worker かは `rescuable` が決める (今はいつも真である)。途中で内部の誤りになれば、run はそこで終わる
      > - 救済は `share` も `transfer_back` も呼ばず、登録表の lock の下でヒープにも `Ctx` にも触れない。引き取ったタスクは、自分の論理的な worker ID のまま救済した worker で動く。待ったタスクの印のない物体は、そのタスクの継続からしか届かない (タスクをまたぐ受け渡しはどれも印を付ける)。`entries` を 0 にした持ち主の区間と救済の区間は登録表の lock で順序が付くので、物体に触れるスレッドは移る前も後も1つである。始める仕事の閉包は、置いた worker で始めるときと同じく `transfer_back(閉包, 始めたタスクの ID)` で受ける
      > - 持ち主が変わった後に元の worker の受け箱と mail に残った知らせは、取り出した worker が今の持ち主へ回す (「セルと登録表」)。引き取るタスクは待ちの登録を持たないので、時間の輪、channel の待ち手の列、シグナルの待ち手の列、操作の記録、仕事のセルのどれも書き換えない。救済の後にタスクが待つときは、新しい持ち主の区画に登録する
      > - 引き取ったタスクが新しいハンドルを作る操作では、持ち主の I/O を受け付ける worker が host になる。本物の時計では救済した worker 自身、仮想の時計では worker 0 である。すでに縛ったハンドルの操作は、縛った worker へ `Submit` で回す (「時計と driver」)。救済は `Submit` を処理しない
    - vm.md「並列化への備え」(vm.md:937) の「実行待ちの待った継続には救済のほかでは印を付けない」を「実行待ちの待った継続には印を付けない。救済も付けない (「救済」)」にし、語を渡す道の一覧 (vm.md:926) に「救済 (持ち主の変更。語は欄の列と受け箱の中を動くだけで、印は付けない)」を足す。
    - object-model.md「論理的な worker ID」(object-model.md:740 の後) に次の文を足す。
      > 救済でタスクが別の worker へ移っても、タスクの ID は変わらず、印も付けない ([VM](vm.md) の「救済」)。
- 実装者に任せる所: 2回の走査の書き方 (`retain` と添字の数え方)。単体テストの組み立ての詳細。
- 条件: いつも。
- 完了の条件:
  - `cargo nextest run -p eml_vm --lib -E 'test(/^(cells|sched)::tests::(an_entry_count_follows_start_park_resume_and_finish|two_live_entries_keep_a_parked_task_unstealable|steal_helpers_refuse_a_cell_owned_by_a_third_worker|steal_takes_a_place_and_the_new_owner_starts_it|steal_takes_a_wake_only_when_no_entry_of_its_task_is_live|steal_takes_start_and_resume_entries_in_queue_order_within_the_budget|steal_takes_a_yielded_resume|steal_leaves_resume_io_rescued_entries_tokens_and_par_tasks|steal_decrements_inbox_work_and_ready_entries|steal_reports_truncation_past_the_scan_cap|a_stale_cancel_in_the_victim_mail_reaches_the_new_owner|a_rescued_task_runs_under_its_own_id|a_flush_adds_settled_cells_to_the_slot)$/)'` → `13 tests run: 13 passed`
  - `cargo nextest run` → `0 failed`
  - `git diff --stat f969e3c0 -- crates/eml_rt tests/ui crates/eml_vm/tests/bench.rs` → 何も出ない
  - `cargo nextest run -p eml_cli --test integration citations::` → `0 failed`
  - `grep -n "\.owner = " crates/eml_vm/src/cells.rs` → `start`、3つの `steal_*`、テストの helper だけ
- 落とし穴:
  - `leave` は `take_async` より前に呼ぶ。後にすると、同じ区間で盗める項を見落とす。
  - `exit` と `Stopped` の道で終わったエントリは減らさないが、run は捨てられるので害はない。

### T3a: `Rescue` の知らせ、目安、空振りの印、`request_rescue`、`after_steal`、`Finished` の数

- 目的: 救済を頼み、受け、結果を数える道を作る。
- 対象: `workers.rs`、`sched.rs`、`cells.rs` (`would_run`)、`lib.rs`、`crates/eml_cli/tests/ui.rs`、`docs/spec/vm.md`。
- 作業:
  - `Notice::Rescue`、`Slot.inbox_work`、`futile_*` の欄を足す。`send` と `take_notices` で `inbox_work` を書く。
  - `Scheduler::{request_rescue, after_steal}`、`rescue_key`、`rescue_budget` を D-13 のとおりに書く。
  - `Finished.rescues` と `Finished.rescued` を `Retired` を通して集める。
  - `left_notices`、`would_run`、`take_slot` が `Rescue` を無視するようにし、I4 の文を直す。
  - spec: vm.md「タスクとスケジュール」に次を足す。
    > - `Rescue { victim }` は救済の頼みで、語を持たない。受けた worker は、止めるビットか `END` が立っていなければ、`victim` の3つの数をその場で読んで覚え、予算を決めて、集める地点の最初の登録表の区間で盗む (「`Async` のタスク」の「救済」)。予算は、`Watched` では目安 `h` の半分 (切り上げ、1 以上)、`Always` では限らない。区間の後で、何も取れず走査も切れていなければ、覚えた数を `victim` の欄に書いて空振りの印 `futile` を立てる。取れたか走査が切れていれば、印を下ろす。まだ処理していない頼みがあるときに届いた頼みは捨てる。終わりに受け箱に残った `Rescue` は仕事に数えず、捨てる
    - I4 の文: 「置く側と監視のスレッドは、1回取り出すごとに知らせ (`Child` か `Rescue`) を1つ送る」
    - 欄の項に足す文: 「盗めるかもしれない知らせ (`Place` と、`op` を持たない `Wake`) の数 `inbox_work`。知らせを積むときと取り出すとき、救済が受け箱から外すときに、欄の lock の下で書く。空振りの印 `futile` と、そのときの3つの数。`RescueAlways` の頼んだ印 `asked`」
    - 「`RunStats`」の近くに足す文: 「`Finished.rescues` は1つ以上を盗んだ救済の数、`Finished.rescued` は救済で移した項の数で、どちらも worker の和である。救済はスケジュールで決まるので `RunStats` には入れず、`--stats` も出さない」
  - 単体テスト:
    - workers.rs `inbox_work_counts_places_and_plain_wakes`: `Place` と `Wake(Task, op None)` で 1 足し、ほかの種類では足さず、`take_notices` の後は 0
    - sched.rs `rescue_budget_takes_half_rounded_up_and_at_least_one`: h = 0, 1, 2, 3, 5 で 1, 1, 1, 2, 3
    - sched.rs `a_rescue_that_finds_nothing_marks_the_victim_futile`: `ResumeIo` の項が1つの victim では `taken == 0` で、走査は切れない。`futile` が鍵 `(0, 0, settled)` で立ち、`rescues == 0`。`ready_entries` は `ResumeIo` を数えないので、鍵の1つ目は 0 である
    - sched.rs `a_truncated_rescue_never_marks_the_victim_futile`: `STEAL_SCAN + 1` 個の `ResumeIo` の後に `Start` を置き、`Watched` で `truncated` になり、`futile` は偽
    - sched.rs `a_rescue_request_after_the_end_is_dropped`: `END` が立つと `rescue` は `None` のまま
  - ui.rs の `Finished` のリテラル (`ui.rs:441, 486`) に `rescues: 0, rescued: 0` を足す (種類 3)。
- 実装者に任せる所: なし。
- 条件: いつも。
- 完了の条件:
  - `cargo nextest run -p eml_vm --lib -E 'test(/^(workers::tests::inbox_work_counts_places_and_plain_wakes|sched::tests::(rescue_budget_takes_half_rounded_up_and_at_least_one|a_rescue_that_finds_nothing_marks_the_victim_futile|a_truncated_rescue_never_marks_the_victim_futile|a_rescue_request_after_the_end_is_dropped))$/)'` → `5 tests run: 5 passed`
  - `cargo nextest run` → `0 failed`
  - `git diff --stat -- crates/eml_vm/tests/bench.rs` → 何も出ない
- 落とし穴: `a_rescue_that_finds_nothing_marks_the_victim_futile` の鍵の1つ目は、D-6 の数の定義で 0 になる。設計の途中の版では 1 と書いていた (付録)。

### T3b: `RescueAlways` と UI テストの表

- 目的: 救済の道を、機械の速さに左右されずに通るテストの方針を入れ、印を付けないことの証拠を揃える。
- 対象: `sched.rs`、`lib.rs` (`lib.rs:613` の振り分け、`lib.rs:945-946`)、`workers.rs` (`Slot.asked`)、`crates/eml_cli/tests/ui.rs`、`crates/eml_vm/tests/{workers.rs,io.rs}`、`docs/spec/vm.md`、`docs/implementation/testing.md`。CLI には触れない (D-16。`--sched` は T0 で消えている)。
- 作業:
  - D-11、D-15、D-16 のとおりに書く。対象は `Sched::RescueAlways`、方針の型、`Rescue::Always`、`workers_started` と `set_rescuable` の呼び出し、`Slot.asked`、`collect` の終わりの頼みである。
  - spec: vm.md「タスクとスケジュール」の方針の段落を次にし、続けて `RESCUE` と `RescueAlways` の段落を足す。
    > - 方針は `Sched` の値で選ぶ。`Normal`、`Sequential` (既定)、`Seeded(seed)`、`RescueAlways` の4つで、`Copy` である。`RunConfig::with_sched` で渡す。`eml run` と `eml repl` はいつも `Normal` で動き、CLI でほかの方針は選べない。`Sequential`、`Seeded`、`RescueAlways` は、決定的なテストと救済の道を確かめるテストのための方針である
    > - `Policy` の定数 `RESCUE` は、救済を頼む者を決める。`Never` (`Sequential` と `Seeded`) では救済しない。`Watched` (`Normal`) では監視のスレッドが頼み、`Always` (`RescueAlways`) では盗めるものを持つ worker が自分で頼む。`workers_started` は、worker 0 が worker 1..N-1 を作り、作れた数を書いた直後に1回呼ぶ
    > - `RescueAlways` は、救済の道を確かめるテストのための方針である。横取りと `pick` と `wake` は `Sequential` と同じである。救済を強いるのは、worker 1..N-1 を作った後 (worker 0 では `workers_started` から、worker 1..N-1 では始めから) で、その worker が救済してよい worker のとき (`set_rescuable`) だけである。ほかのときは `Sequential` と同じに動くので、worker が1つの run では最後まで `Sequential` と同じである。強いる間は、救済で受けた印のない始める仕事と再開する仕事 (`Ready::Entry` の `Start` と `Resume`) を `pick` が飛ばし、持ち主は自分では動かさない。集める地点の終わりに欄の `ready_entries` を読み、0 でなく、頼んだ印 `asked` が下りていれば、印を立てて次の worker (`(k + 1) % 作った worker の数`) に `Rescue` を送る。頼まれた worker は、印を下ろしてから予算を限らずに盗む。盗んだ項には救済で受けた印が付くので、ほかの worker はそれを盗まない。そのため、worker を作った後に始めるタスクと再開するタスクは、どれも動く前にほかの worker へ1回移る。半分でなくすべてを取るのは、残りを持ったまま眠った worker に頼み直す者がいなくなるためである
  - 単体テスト、sched.rs:
    - `rescue_always_asks_its_successor_once`: live 3 の worker 1 が予約した項を持つと、worker 2 に `Rescue { victim: 1 }` を1つ送る。盗む前の2回目の `collect` は何も送らない
    - `rescue_always_pick_skips_reserved_entries`: 列 `[Entry Start unrescued, Task t]` から、予約する間は `t` を、働く前は項を取る
    - `rescue_always_reserves_nothing_on_an_unrescuable_worker`: `set_rescuable(false)` の後は `pick` が項を返し、頼まない
  - 結合テスト、`crates/eml_vm/tests/workers.rs` (検査モード、仮想の driver、ふつうの run):
    - `rescue_always_rescues_every_started_task`: N = 2 と N = 4 で流す。出力は N = 1 と同じで、`rescues ≥ 1`、`rescued ≥ stats.tasks − 1`
    - `a_rescued_task_holding_a_lin_resource_passes_the_checks`: N = 2。タスクが `FileIn` を開き、唯一の `Bytes` を持ったまま `yield_now` と channel の待ちをくぐって終わる。検査モードの持ち主の検査と漏れの検査が通り、`rescued ≥ 1`
    - `sequential_never_rescues`: N = 2 の `Sequential` で `rescues == 0`
    - `rescue_always_at_one_worker_is_sequential`: N = 1 で出力と `RunStats` が `Sequential` と同じで、`rescues == 0`
    - `rescued_tasks_sleep_and_take_from_a_channel_on_their_new_worker`: N = 2 で出力が N = 1 と同じ
    - `a_cancel_reaches_a_rescued_waiting_task`: 眠るタスクを `timeout` で囲み、N = 2 で流す
  - 結合テスト、`crates/eml_vm/tests/io.rs`: `a_rescued_task_does_io_through_its_host` (`RescueAlways` N = 2、テストの driver。echo の出力が N = 1 と同じ)。
  - UI の harness: `WORKER_RUNS` (D-16)。
  - testing.md の「seed で流す」に、`async/` の worker の run の表と、`Sched::RescueAlways` は API から渡す方針で CLI では選べないことを書く。
  - 7 節の最後の表の種類 1 と 2 のテストの変更を行う。
  - 最初に `EML_UI_FILTER=/async/` を新しい表で流し、`RescueAlways` の run で落ちるファイルを見つける (U-3)。
- 実装者に任せる所:
  - `RescueAlways` の run だけで落ちる `async/` のファイルの直し方。まずスナップショットを変えずに書き直す。それができなければ `-- workers: skip (<見た理由>)` を付け、コミットのメッセージにファイル名を挙げる (種類 1)。
  - `rescued > 0` を満たさないファイルも同じ扱いにする。
- 条件: いつも。
- 完了の条件:
  - `cargo nextest run -p eml_vm --lib -E 'test(/^sched::tests::rescue_always_/)'` → `3 tests run: 3 passed`
  - `cargo nextest run -p eml_vm --test integration -E 'test(/^(workers::(rescue_always_rescues_every_started_task|a_rescued_task_holding_a_lin_resource_passes_the_checks|sequential_never_rescues|rescue_always_at_one_worker_is_sequential|rescued_tasks_sleep_and_take_from_a_channel_on_their_new_worker|a_cancel_reaches_a_rescued_waiting_task)|io::a_rescued_task_does_io_through_its_host)$/)'` → `7 tests run: 7 passed`
  - `EML_UI_FILTER=/async/ cargo nextest run -p eml_cli --test integration -E 'test(=ui::run) | test(=ui::run_fail)'` → `2 tests run: 2 passed`
  - `cargo nextest run -p eml_cli --test integration -E 'test(=ui::run)' -j 1` → `1 test run: 1 passed`。`SLOW` の行が出ず、時間は 5 s 未満
  - `git diff --stat HEAD~1 -- crates/eml_cli/src crates/eml_cli/tests/cli.rs docs/spec/stdlib.md` → 何も出ない (T3b のコミットは CLI に触れない)
  - `cargo nextest run` → `0 failed`。`cargo nextest run --profile timing -j 1` → `0 failed`
  - x86_64 Linux: `nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=thread TSAN_OPTIONS=halt_on_error=1 CARGO_TARGET_DIR=target/tsan cargo test -Zbuild-std --target x86_64-unknown-linux-gnu -p eml_vm --test integration -- workers:: io::` → exit 0 (環境がなければ 4 節のとおり)
- 落とし穴:
  - `ui::run` が 5 s を超えたら、`async/` の `(2, Sequential)` を外す (U-2)。種類 2 の変更なので、コミットのメッセージに書く。
  - 同期の extern の中で救済する worker が止まり、それが元の worker の予約した項にしか作れないものを待つと、両方が止まる (U-3)。

### T3c: 監視のスレッドの判定

- 目的: `Normal` の run で、詰まった worker と過負荷の worker を見つけて救済を頼む。
- 対象: `monitor.rs`、`workers.rs` (`pop_idle_except`、`live`)、`crates/eml_vm/tests/workers.rs`、`docs/spec/vm.md`、`docs/implementation/testing.md`。
- 作業:
  - D-12 のとおりに書く。対象は `Ticker.still` と `stuck`、`STUCK_TICKS`、`OVERLOAD`、`Run::pop_idle_except`、`Run::live`、`Relay::rescue_tick`、`Monitor::start(control, cells, run, preempt, rescue)` である。
  - 既存の `Ticker` のテストのリテラル (`monitor.rs:193, 200`) に `still: 0` を足す (種類 3。期待する値は変わらない)。
  - spec: vm.md「タスクとスケジュール」の監視のスレッドの役目に、4つ目として次を足す。
    > - 監視のスレッドの役目の4つ目は救済の判定で、`RESCUE` が `Watched` の方針で、作った worker が2つ以上のときだけ行う。刻みごとに worker `k` について、切り替えの数が続けて変わらなかった刻みの数を数え、欄の `ready_entries` と `inbox_work` の和を目安 `h` として読む。どれも atomic の読みで、登録表の lock も欄の lock も取らない。`h` が 0 でなく、切り替えの数が2刻み続けて変わらないか (詰まった)、`h` が 64 を超えていれば (過負荷)、`idle` から `k` でない worker を1つ取り出して `Rescue { victim: k }` を送る。取り出せなければ、その刻みの判定をやめる。1刻みに1つの worker への頼みは高々1つで、判定は前に頼んだ worker の次から回す。前の救済が何も取れず、走査が上限で切れていなかった worker は、`ready_entries`、`inbox_work`、`settled` のどれかがそのときから変わるまで飛ばす。`settled` が変わるのは、救済できなかった理由がまだ終わっていないエントリの `Task` で、それが終わったときである。extern の中で止まった worker も切り替えの数が変わらないので、同じ判定で見つかる。安全点を通る VM のタスクは、`NOTICE` で知らせを列に移し、列が空でなければ次の `PREEMPT` で切り替わるので、2刻み変わらないことはない
  - 単体テスト、monitor.rs (既存の `relay()` の helper と同じく `Run::new` を作り、atomic を手で書き、`tick` と `rescue_tick` を直接呼ぶ。スレッドも時計も使わず、テストの刻みで数える):
    - `a_worker_is_stuck_after_two_ticks_without_a_switch`
    - `a_stuck_worker_without_a_hint_is_left_alone`
    - `an_overloaded_worker_is_rescued_while_it_switches`: `switches` が刻みごとに変わる間、h = 65 で送り、h = 64 では送らない
    - `a_rescue_goes_to_an_idle_worker_other_than_the_victim`: `idle = [victim]` では送らず、`idle = [r, victim]` では r に送る
    - `without_an_idle_worker_nothing_is_sent`
    - `a_futile_victim_is_skipped_until_its_counts_change`: 数が等しい間は飛ばし、`inbox_work` が変わると送る
    - `a_futile_victim_is_retried_after_its_entries_settle`: `settled` だけが変わっても送る
    - `a_blocked_worker_is_rescued_within_three_ticks`: 刻み 0 の前に切り替えの数を変え、その後は止めて h = 1 にする。3回の呼び出しのうちに `Rescue` を1つ送る
  - 無視するテスト、`crates/eml_vm/tests/workers.rs`: `#[ignore = "depends on the monitor's 10 ms tick"] the_monitor_rescues_a_worker_blocked_in_an_extern`。
    - `Normal`、N = 2、release で流す。
    - `BlockingHost` の `write_line("a")` は、別のスレッドから "b" が書かれるまで condvar で待つ。lock は持たず、10 s で自分から抜ける。
    - プログラムは T1 (`println "a"`、w1)、X (何もしない、w0)、T2 (`println "b"`、w1) を始め、すべてを待つ。
    - 出力が `b\na\n` で、`rescues ≥ 1` であることを確かめる。"a" が `write_line` に入ってから "b" までの時間が 3 × `TICK` + 20 ms 未満であることも確かめる。
    - testing.md の「よく使うコマンド」と「性能のテスト」に、このコマンドを書く。
- 実装者に任せる所: なし。
- 条件: いつも。
- 完了の条件:
  - `cargo nextest run -p eml_vm --lib -E 'test(/^monitor::tests::(a_worker_is_stuck_after_two_ticks_without_a_switch|a_stuck_worker_without_a_hint_is_left_alone|an_overloaded_worker_is_rescued_while_it_switches|a_rescue_goes_to_an_idle_worker_other_than_the_victim|without_an_idle_worker_nothing_is_sent|a_futile_victim_is_skipped_until_its_counts_change|a_futile_victim_is_retried_after_its_entries_settle|a_blocked_worker_is_rescued_within_three_ticks)$/)'` → `8 tests run: 8 passed`
  - `cargo nextest run --release -p eml_vm --test integration --run-ignored ignored-only -E 'test(=workers::the_monitor_rescues_a_worker_blocked_in_an_extern)'` → `1 test run: 1 passed`
  - `cargo nextest run` → `0 failed`
- 落とし穴: 長く待っていた worker に `Place` を送った直後は、数マイクロ秒だけ詰まって見える。余分な救済が1回起きるだけで、空振りの印が吸収する (U-4)。

### T4: 段を閉じる (`Close async-rescue`)

- 目的: docs/future/roadmap.md の「進め方」の順で、文書を今の状態に直す。
- 作業:
  1. spec を仕上げる: `git grep -n 'async-rescue\|後で\|今は' -- docs/spec` に、この段の予告が残っていないことを確かめる。
  2. 計測 (macOS):
     - まず `bench/scripts.sh` を流し、範囲を外れた行があれば、別のコミット (`Resize bench rows`) で大きさを変える。
     - 次に T3c のコミットのきれいな作業ツリーで `bench/run.sh`、`bench/rt.sh`、`bench/scripts.sh` を流し、docs/implementation/benchmarks.md の「今の数値」の3つの表を入れ替える。
     - 列の mutex の切り替えごとの費用を、`par` の bench の行で前の表と比べて「所見」に書く (U-9)。
     - `bench/async/scale.sh` を流し、救済の前後の差を「所見」に足す (U-1)。
     - macOS がなければ、表の入れ替えを直後の `Measure async-rescue` に回す。
  3. status.md: l.164 の制限を、救済するものとしないものの文に書き換える。しないものは次のとおりで、roadmap の「その後の項目」を引く。
     - 操作か仕事を待つタスク
     - `resume_io` の項
     - 救済で受けて列にある項
     - `par` の子
  4. architecture.md: crate とパスの構成は変わらない。eml_vm の節に救済の一文があれば直す。
  5. roadmap.md:
     - `async-rescue` の項 (l.31) を消す。
     - `jit-effects` の「前提」から `async-rescue` を外す。
     - 「その後の項目」に次の2つを、「後に回した理由」と「見直す条件」を添えて足す。
       - 操作と仕事の待ちの救済 (U-7、U-8)
       - 救済で受けた項の盗み直し (U-10)
     - `par-steal` の項に、`par` の子を deque に移した後の列の形 (U-9) を一文で足す。
  6. future の文書:
     - multicore.md の書き直しは次のとおりである。
       - MC:122, 125 (救済されるまで動かない。vm.md の「救済」を引く)
       - MC:144 (「その代わり、救済したタスクには印が付く」を消す)
       - MC:156-163 の「救済」(印付けの文を vm.md の「救済」への道しるべに替える。判定の文は vm.md の「タスクとスケジュール」を引く)
       - MC:243 (例外の箇条を消す)
       - MC:278 (2文目)
       - MC:307/316 (表の行を「救済 | なし | なし (持ち主が変わるだけ)」に)
       - MC:426 の1番目、MC:440-442、MC:518-520
       - MC:531 と MC:547-549 (救済を強いる方針の説明を `RescueAlways` の今の形にし、失敗するテストを D-3 の成功するテストに替える)
       - MC:570 (監視と救済の値。決まった値は vm.md を引く)
     - async.md の書き直しは次のとおりである。
       - 「ハンドルと worker」の4番目の箇条
       - プールの表の「待っている間にタスクが救済された」の行を「この待ちのタスクは救済しない」に
       - async.md:373 の救済を強いる方針の文
  7. overview.md と docs/README.md: 確定した設計判断の表と「内容」の欄で、変わった所 (救済の行) だけを直す。
  8. CLAUDE.md (道しるべだけ):
     - Commands の `--sched` は T0 で消えている。
     - Architecture の eml_vm の行と、`Async` のタスクの不変条件の「never move」を「move only by rescue」にする。
     - トピックの表の `Std.Async` の行に vm.md の「救済」を足す。
  9. この計画を消す。消す前に、5 節の決めがすべて「リポジトリに残す所」に書かれていることを確かめる。
  10. コミットのメッセージに次を書く。
      - future の文書から何をどこへ移したか
      - テストの変更の種類と理由 (7 節の最後の表)
      - 計測の目立つ差と、大きさを変えた行
      - multicore.md の印付けの設計を変えたこと (D-3)
      - 後に回した項目の決めた日 (2026-11-04)
- 実装者に任せる所: 書き直す文の言い回し。中身は 5 節に従う。
- 条件: いつも。
- 完了の条件:
  - `git grep -n "async-rescue" -- docs ':!docs/plans'` → 何も出ない
  - `git grep -n "rescue-always" -- docs ':!docs/plans'` → 何も出ない (綴りがないので、文書は `RescueAlways` と書く)
  - `git grep -n "RescueAlways" -- docs ':!docs/plans'` → vm.md、testing.md、multicore.md、async.md の書き直した文だけ
  - `cargo nextest run -p eml_cli --test integration citations::` → `0 failed`
  - `ls docs/plans` → 空 (またはディレクトリがない)
  - 9 節の全体の確かめ

### テストの変更の一覧 (種類ごと)

- 種類 1 (成否の変更):
  1. `crates/eml_cli/tests/cli.rs` の `sched_flag_is_accepted_and_rejected` を消す。理由: `--sched` がなくなる。分け方の場合は `workers_flag_is_accepted_and_rejected` へ移す (T0)。
  2. `crates/eml_cli/tests/terminal.rs` の `repl_runs_par_under_a_seeded_schedule` を消す。理由: REPL が `--sched` を受けなくなる (T0)。
  3. `crates/eml_vm/tests/api.rs` の `a_sched_parses_and_rejects` を消す。理由: `Sched::from_str` がなくなる (T0)。
  4. `workers_flag_is_accepted_and_rejected` に、`["run", "--sched", "normal", "main.em"]` → exit 2 と、`["run", "main.em", "--sched", "bogus"]` → exit 0 を足す。前者は今は成功する引数で、`--sched` を消したことを確かめる (T0)。
  5. 条件付き: `RescueAlways` の run でだけ落ちる `async/` のファイルに `-- workers: skip (<見た理由>)` を付ける。付けたファイルは T3b のコミットのメッセージに名前で挙げる。
- 種類 2 (期待値の変更、範囲で書く):
  1. `cli.rs:1299, 1338, 1396` の3本を、`Sequential` でなく `Normal` で流す (引数から `--sched sequential` を消す)。期待する出力は変えない (T0)。`ui.rs` の `seeded_label` と `ui.rs:388` の文、`ui.rs:750, 770` の `should_panic(expected = …)` を、`--sched seeded:` を含まない文に替える (T0)。
  2. UI の harness: `run/async/` と `run-fail/async/` に `(2, RescueAlways)` と `(8, RescueAlways)` を足し、`(8, Sequential)` を外す。`run/async/` は `RescueAlways` で `Finished.rescued > 0` を示す (T3b)。U-2 の場合は `(2, Sequential)` も外す。
  3. `ui.rs` の `workers_runs_cover_par_and_async` の `assert_eq!(WORKER_COUNTS, [2, 8])` を、`WORKER_RUNS` の確かめに替える (T3b)。
- 種類 3 (機械的な追随):
  - `Runnable` と `sched.ready` を使う単体テスト (T1)。
  - `ui.rs:441, 486` の `Finished` のリテラル (T3a)。
  - 新しい `Sched` の腕 (T3b)。
  - `monitor.rs:193, 200` の `Ticker` のリテラル (T3c)。

## 8. 進める順

```
T0 → T1 → T2 → T3a → { T3b, T3c } → T4
```

- T0 は救済の作業とは独立しているが、T3b の `RescueAlways` を CLI に出さない前提になるので最初に入れる。

- T3b と T3c は互いに要らない。ただし、どちらも `crates/eml_vm/tests/workers.rs` と vm.md の「タスクとスケジュール」に書くので、同じ worktree で順に進めるのを既定にする。並べるなら、後のほうが前のほうの上に rebase する。
- 段の最初のコミットは、この計画の文書である (roadmap.md の「進め方」)。

## 9. 確かめの計画

- 基準 (最初の変更の前): `cargo nextest run` → `4284 tests run: 4284 passed, 20 skipped` (macOS aarch64)。環境によって `eml_io::integration real::*` がネットワークの制限で落ちることがある (前の段で観察した)。そのときは基準の失敗として記録し、どの作業も `eml_io` に触れない。
- 全体の確かめ (すべての作業の後):
  - `cargo nextest run` → `0 failed`
  - `cargo nextest run --profile timing -j 1` → `0 failed` (x86_64 Linux の CI と同じ設定。macOS では参考)
  - `cargo clippy --all-targets -- -D warnings` → exit 0、`cargo fmt --check` → exit 0
  - testing.md の「feature の組み合わせの確認」の clippy のコマンドすべて → exit 0
  - `git diff --stat f969e3c0 -- crates/eml_rt crates/eml_vm/tests/bench.rs` → 何も出ない
  - Miri: `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::` → exit 0 (同じ2つの設定で。列の項が語を持つ形を確かめる)
  - TSan (x86_64 Linux): `nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=thread TSAN_OPTIONS=halt_on_error=1 CARGO_TARGET_DIR=target/tsan cargo test -Zbuild-std --target x86_64-unknown-linux-gnu -p eml_vm --test integration -- workers:: signal:: preempt:: io:: chan::` → exit 0
  - ASan (macOS): `nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=address CARGO_TARGET_DIR=target/asan cargo test -Zbuild-std --target aarch64-apple-darwin -p eml_vm --test integration -- api:: signal:: io:: workers::` → exit 0
  - `cargo nextest run --release -p eml_vm --test integration --run-ignored ignored-only -E 'test(=workers::the_monitor_rescues_a_worker_blocked_in_an_extern)'` → `1 test run: 1 passed`
- 利用者の手での確かめ: なし。

## 10. リスクと未決の項目

| ID | 項目 | 影響 | 決め方と時期 | 決まらないときの既定 |
|---|---|---|---|---|
| U-1 | 判定の値 (2刻み、64、半分、`STEAL_SCAN` 1024) | 救済の遅れと余分な救済 | T4 で `bench/async/scale.sh` と、1つの worker の CPU を使う2つのタスクの後ろに待つタスクを置く測りで決める。N = 4 で始めるまでの遅れの中央値が 10 刻みを超えたら 64 を見直す | 今の値 |
| U-2 | UI テストの時間 | `ui::run` の 5 s | T3b の完了の条件で測る | 超えたら `async/` の `(2, Sequential)` を外す |
| U-3 | `RescueAlways` の run で止まるか、`rescued > 0` を満たさない `async/` のファイル | T3b | T3b の最初に `EML_UI_FILTER=/async/` を流して見つける | 書き直し、できなければ種類 1 の 3 |
| U-4 | 起こした直後の worker が数マイクロ秒だけ詰まって見える | 余分な救済が1回 | 受け入れる | 何もしない |
| U-5 | 1回の集める地点に `Rescue` が複数届く | 後の頼みを捨てる | 頼みが捨てられて victim が飢えると測れたとき | 最初だけを残す |
| U-6 | `Seeded` の N ≥ 2 での救済 | seed の run は救済を通らない | 必要になったとき | `Never` |
| U-7 | `resume_io` の項を、記録の持ち主ごと移して盗む | 詰まった worker の I/O の再開が遅れる | bench/net で、詰まった worker に `resume_io` がたまると示されたとき | 盗まない。T4 で roadmap の「その後の項目」へ |
| U-8 | 操作か仕事を待つタスクと `Completion` の救済 | 本物の時計で owner ≠ host の接続が遅れる | 本物の時計の server の計測で示されたとき。`OpRecord.owner` をタスクに付いて動かす必要がある | 盗まない。T4 で「その後の項目」へ |
| U-9 | 列の形 | 切り替えごとに mutex を1回取る | `par-steal` で `par` の子を deque に移すとき。T4 で費用を測って benchmarks.md に書く | 1本の mutex の列 |
| U-10 | 救済で受けて列にある項の、`Normal` での盗み直し | 救済した worker が自分の CPU を使うタスクで塞がると、受けた項は次の `PREEMPT` (1刻み以内) まで待つ | `bench/async/scale.sh` の 4 worker で `Finished.rescued` が少ないのに救済した worker の `ready_entries` が多いままのとき | 盗まない。T4 で「その後の項目」へ |
| R-1 | 印を付けない判断の前提 (印のない物体は1つのタスクからしか届かない、MC:160) を崩す道が将来足される | データ競合 | タスクをまたぐ値の道を足す段で、D-3 の見直す条件を確かめる | - |

## 付録: レビューで変わった主な所

- 待ったタスクを盗める条件は、最初はセルごとの真偽値 `left` だった。すぐ再開する待ちで同じセルのエントリの `Task` が2つ生きる反例が見つかり、数 `entries` に替えた。2回目のレビューで、`entries` が増える道と減る道の漏れがないことを確かめた。
- 譲り (`Wait::Ready`) の再開は、最初は盗まなかった。過負荷の worker にたまるのがこれなので、盗むようにした。
- 盗む手順の helper は `owner != me` を見ていたが、第三の worker のセルを奪いうるので `owner == victim` を見るようにした。
- 盗む手順に走査の上限 `STEAL_SCAN` と `truncated` を足し、走査が切れたときは空振りの印を立てないようにした。
- 空振りの印は、最初は目安の数だけで引いていた。`entries` が 0 になってもその数は変わらないので、`settled` を鍵に足した。
- multicore.md の失敗するテスト (印なしで `WrongOwner`) は、印を付けないので作りから失敗しえない。成功する検査モードのテスト (D-3) に替えた。
- `RescueAlways` の頼みは、最初は列を全部見る O(n) の走査だった。適合の確かめで、切り替えのたびに O(n) になると指摘された。計画をまとめるときに `ready_entries` の定義を「救済で受けた印のない `Start` と `Resume` の数」に絞り、頼みを O(1) の読みにした。この定義では監視の目安も `ResumeIo` と受けた項を数えなくなる。
- 監視が実際の monitor の刻みに頼るテストは、ふつうの run から外して `#[ignore]` にした。判定はテストの刻みの単体テストで確かめる。
- 設計の段では `rescue-always` を `--sched` の値として読む形だった。計画をまとめた後に、利用者の判断で `--sched` そのものを CLI から外した (D-16、T0)。`Sequential`、`Seeded`、`RescueAlways` は API から渡すテストのための値になり、`eml run` と `eml repl` はいつも `Normal` で動く。
