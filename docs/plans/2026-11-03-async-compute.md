# async-compute の設計と計画

- 作成: 2026-11-03
- 基準: main @ 08a1e01c (3 節と、すべての `file:line` はこの状態を指す)
- 状態: 計画は確定。既定のある未決の項目が残る (10 節)

## 1. 目的と完了の条件

- 目的: `Std.Async.compute : (Unit -> <never e> a) -> <IO, Async | e> a` を入れる。`compute f` は `f ()` を子が1つの `par` と同じ仕事として動かし、呼んだタスクだけを待たせる。待つ間、その worker はほかのタスクを動かす。
- 完了の条件: 9 節の全体の確かめがすべて通り、T6 の閉じるコミットで `docs/` が今の状態を書いていること。

## 2. 範囲

- 範囲に入るもの: extern の行と std の `compute`、`may_run_par` から `may_fork` への改名、VM の登録表の仕事のセル (`Job`) と3つの行、空いた worker への置き方と `Finished.placed_jobs`、テスト、`docs/spec/` の節、段を閉じる作業。
- 範囲に入らないもの: 仕事の取り消し (`par` の取り消しと一緒に後で足す)、盗み (`par-steal`)、救済 (`async-rescue`)、タスクごとの上限 (`task-limits`)、ネイティブのコード。`eml_rt` には触れない (Verus は要らない)。

## 3. 前提になる事実

- `Std.Par.par` は `pub extern par` (`std/Par.em:6`) で、飽和した呼び出しは Core IR と LIR で `Call::Par { left, right, pair, at }` になり (`crates/eml_core_ir/src/translate/program.rs:487`、`crates/eml_lir/src/lib.rs:530`)、`Op::Par`、`Op::ChildDone`、`Op::ChildAbort` に翻訳される。
- `par` の fork (`crates/eml_vm/src/vm.rs:1170-1268`): 左の子は親の `Task` の上でその場で動く。右の子は、`Workers` が `Started` か `Peer` で `Run::pop_idle()` が worker を返せば `Notice::Child` でその受け箱へ、そうでなければ自分の `ready` の末尾へ行く。evv は `fork_evv` で作り、2つの子が1つずつ参照を持つので `dup` する (`vm.rs:1195-1201`)。
- join の節点 `JoinNode` は worker の `Scheduler.joins` にある Rust の構造で (`crates/eml_vm/src/sched.rs:336`)、登録表のセルではない。登録表のセルの種類は Task、Scope、Chan、Op だけである (`crates/eml_vm/src/cells.rs:292`)。
- join で待つ親は `TaskState::Waiting(j)` でスタックを持ったまま待ち、自分の worker の `wake_parent` でだけ起きる (`sched.rs:854-861`)。`Async` の待ち (`await_cell` など) は継続 `k` をヒープに置いて park し、エントリの `Task` は終わる。
- `park` (`cells.rs:1682-1725`) は Chan と Op を断り、取り消しの地点を先に見て、`Wait::Cell` を登録する。`resumed` (`cells.rs:1650-1668`) は `Wait::Ready` のときだけ `Woke` を `Cancelled` に替える。`unpark` (`cells.rs:2329-2394`) は `matches!` の一覧 (`cells.rs:2345`) にある待ちだけを外す。
- join の完了 (`join_outcomes`、`vm.rs:1681-1763`) は `transfer_back(w, right_worker)`、`retire_worker`、組の作成の順で、中断ならいちばん左のものを取り、深さが 0 でなければ evv を mask してから `apply_to_unit` で `reperform$E.op` を親のフレームで呼ぶ。
- `task_failed` (`vm.rs:1812-1865`) は `sched.current == MAIN || task.cell.is_some() || stops_the_run(&error)` のとき run を止め、それ以外は join に回す。
- `may_run_par` は2か所にあり (`crates/eml_lir/src/lib.rs:69-83`、`crates/eml_core_ir/src/evidence/forks.rs:30-47`)、`Call::Par | Prim::RootGet` を見て `fork$E.op` と `reperform$E.op` を作るかを決める。`retain_reachable` は evidence passing の前に走るので、呼ばれない std の関数のために fork の表は作られない (`crates/eml_core_ir/src/optimize/reach.rs:1-10`)。
- `std/Async.em` の `Async` は `once` の操作を9つ持ち、その1つが `await_cell` である。private な extern は 33 行で (`std/Async.em:333-365`)、`Extern::is_async()` は明示した一覧である (`crates/eml_extern/src/lib.rs:1630`)。`run` の位置 `<std>/Async.em:75:9` はスナップショットに出るので、関数は `shield` の下に、extern は一覧の末尾に足す (`std/Async.em:77-79`)。
- extern は対の値を作れない。`with_arg` の組の2つ目は引数でなければならない (docs/spec/core-ir.md の「組を返す extern」)。そのため `await` は `task_cell` で借りてから `await_cell` する形である (`std/Async.em:117-125`)。
- `std` 以外は `extern` を書けない (E1033、`crates/eml_hir/src/lib.rs:101`)。`current_cell` はタスクのセルを持つ `Task` でしか動かず (`vm.rs:2662`)、それを作るのは std の `start_task` だけである。
- 論理的な worker ID は空きの一覧で使い回し、上限は 2^32-1 で、使い切ると panic する (`crates/eml_rt/src/control.rs:274-292`、`crates/eml_rt/src/heap/mod.rs:241-243`)。速いモードの `fork_worker` は 0 を返して何も確保しない (`heap/mod.rs:893, 905-906`)。
- `async/` の UI テストの `RunStats` は、逐次と seed の run の間で、N = 1 でだけ比べる (docs/implementation/testing.md の「seed で流す」)。N = 1 では `places()` が偽なので (`crates/eml_vm/src/workers.rs:124-126`)、何も置かれない。
- docs/spec/declarations.md の「`extern`」の行の数は基準の時点ですでに古い。文書は 60 行 (Async 32、Chan 6、Atomic 4、Net 18) と書くが、コードは 62 行 (33、6、4、19) である。
- テストの数: 基準では `cargo nextest run` が 4269 本を流し、20 本を飛ばす。設計の段の試し (作業用の worktree) では、`eml_io::integration real::a_cancelled_connect_does_not_try_the_next_address` だけが、ループバックの accept の列を埋める所 (`crates/eml_io/tests/real.rs:616`) で失敗した。ネットワークを絞った環境に依る失敗で、どのタスクも `eml_io` に触れない。

## 4. 前提の確かめ

| 要るもの | 用途 | 確かめ方 | ないとき |
|---|---|---|---|
| devShell (nix + direnv) | すべて | `cargo --version` → `cargo 1.99.1` | 止めて利用者に聞く |
| `.#miri` の shell | T3、T4、全体の確かめ | `nix develop .#miri -c cargo miri --version` → exit 0 | 止めて利用者に聞く |
| macOS (aarch64) | ASan (T4、T5)、計測 (T6) | `uname -sm` → `Darwin arm64` | 計測は `Measure async-compute` の別のコミットに回す (roadmap.md の「進め方」) |
| x86_64 Linux の nix の環境 (例: クラウドのセッション) | TSan (T4、T5) | `uname -sm` → `Linux x86_64` | T4 と T5 のコミットのメッセージに「TSan は CI で確かめる」と書き、T6 の前に CI の TSan の job が通ったことを確かめる |
| 基準の結果 | 期待する要約の行 | 9 節の基準 | 9 節を参照 |

## 5. 決めたこと

### 5.1 長期の方針 (変えるには見直しが要る)

実装がこの方針とぶつかったら、止めて報告する。

#### D-1: 仕事の待ちは登録表の `Job` のセルと `await_cell` で行う

- 決めたこと: `compute` は新しいセルの種類 `CellKind::Job` を作り、既存の `Async.await_cell` で待つ。タスクは継続 `k` をヒープに置いて park し、エントリの `Task` とスタックは終わる。待ちの理由は `Wait::Job(CellRef)` を足す。タスクの中の `par` は今のまま `TaskState::Waiting(j)` で待つ。
- 理由: 継続の待ちだけが、動かせて (救済)、スタックを持たず (タスクごとの上限)、ネイティブのコードでも書ける (docs/future/native.md の「並列の実行」)。join の節点は `wake_parent` でしか起こせず、park したタスクは `Task` を持たない。
- 退けた案: `TaskState::Waiting(j)` でスタックを持って待つ形は、同じ worker でしか起きず、待つタスクの数だけスタックが要り、救済できず、ネイティブでは書けない。仕事を `TaskCell` で表す形は、スコープ、取り消し、`tasks` の数え方、スコープの失敗の伝わり方を、タスクでないものに持ち込む。
- 及ぶ範囲: `compute` の道すべて。
- 見直す条件: 仕事を複数のタスクが待つ API を足すとき、仕事を取り消せるようにするとき。
- 適合の確かめ:
  - MUST `compute` の道 (`fork_job` の腕、`take_job`/`drop_job` の腕、`job_done`) に `TaskState::Waiting` も `add_join` も出てこない。
  - MUST `park` に `CellKind::Job` の枝がある。`cells::a_cancel_leaves_a_task_parked_on_a_job` が、park の後で登録表が `k` を持ち、待ちが `Wait::Job` であることを確かめる。
- リポジトリに残す所: docs/spec/vm.md の「`compute` の仕事」と「待ちの2種類」(T4)。

#### D-2: 仕事は `par` の右の子と同じ置き方で動き、`docs/spec/` に盗みと手伝いを書かない

- 決めたこと: 仕事は `par` の右の子と同じく、空いた worker があればその受け箱に `Notice::Child` で置き、なければ呼んだ worker の `ready` の末尾に積む。worker が1つ (REPL、`--workers 1`、根を持つセッション) なら、仕事はその worker のほかのタスクと交互に動く。呼んだ worker に積んだ仕事は、後でほかの worker が空いても動かない。これは今の `par` と同じ制限で (docs/implementation/status.md:140)、status.md に書く。`par-steal` が入ると、積んだ仕事が deque の項になり、盗まれる。
- 理由: 置き方の道を1つにする。盗みは `par-steal` の段の仕事である。
- 退けた案: 仕事専用の置き方。道が2つになる。
- 及ぶ範囲: `fork_job`、stdlib.md と vm.md の文、status.md。
- 見直す条件: `par-steal` の段。
- 適合の確かめ:
  - MUST `fork_job` は `par` と同じ `fork_child` と `idle_home` を呼ぶ。
  - MUST NOT `git grep -n '盗み\|手伝い' -- docs/spec` に `compute` の話が出る。
  - MUST status.md の `Async` の制限に、呼んだ worker に積んだ仕事が動かないことを書く。
  - `workers::a_job_is_placed_on_an_idle_worker` が N = 2 で `placed_jobs == 1` を確かめる。
- リポジトリに残す所: stdlib.md の「`compute`」と vm.md の「`compute` の仕事」(T5)、status.md (T6)、multicore.md の「`par` の子」と roadmap.md の `par-steal` (T6)。

#### D-3: `compute` は std の関数で、4つの extern の行で書く。LIR の `Call` は足さない

- 決めたこと: `compute` は `std/Async.em` の eml の関数で、`fork_job`、`job_cell`、`take_job`、`drop_job` の4行を使う。`fork_job`、`take_job`、`drop_job` は `Op::Async` の行 (`is_async`) で、`job_cell` は `chan_cell` と同じく即値を写す普通の extern である。`Call::Par` と `Op::Par` は変えない。中断の起こし直しは `take_job` の中で、`task.pc` を戻り先にした `apply_to_unit` で行う。
- 理由: 受け付け、待ち、取り出しの形はすでにある (`submit_*`、`io_wait`、`take_*`)。`Call::Par` は 25 か所ほどで場合分けされている。`step` は `Op::Async` の後に後始末を持たず、命令を読んだ時点の `task.pc` は次の命令を指すので (`vm.rs:640, 649-651`)、行の中で `apply_to_unit` してよい。
- 退けた案: `Call::Compute` などの新しい変種は、場合分けの所が何十も増えるだけで動きは同じである。`Call::Par` の左の子を省けるようにする案は、オペランド、境目のフレーム、`par` を見るすべてのパスを1つの呼び手のために変える。`fork_job` が `(Cell, Job a)` の組を返す案は、extern が対を作れないので書けない (3 節)。
- 及ぶ範囲: eml_extern、`std/Async.em`、eml_vm。
- 見直す条件: ネイティブのエントリを作るとき。
- 適合の確かめ:
  - MUST `Extern::AsyncForkJob`、`AsyncTakeJob`、`AsyncDropJob` が `is_async` にあり、`AsyncJobCell` はない。4行とも std に宣言がある。
  - MUST `take_job` の中断の腕は `reraise(.., task.pc, dst)` で終わる。
  - MUST NOT `git diff 08a1e01c --stat -- crates/eml_lir` が `src/lib.rs`、`src/verify.rs`、`tests/verify.rs` 以外を挙げる。
- リポジトリに残す所: stdlib.md の「`Std.Async`」(T1 で ABI、T4 で動き)、vm.md の「命令」(T4)。

#### D-4: `fork_job` を含むプログラムは fork の表を持つ

- 決めたこと: `may_run_par` を `may_fork` に改め、2か所とも `Rhs::Extern { ext: Extern::AsyncForkJob, .. }` も見る。`fork$E.op` と `reperform$E.op` の作り方はプログラム全体の never だけのエフェクトの操作ごとで、呼び出しの位置ごとではないので、ほかは変えない。`Std.Async` を import するだけのプログラムは、`compute` が `retain_reachable` で落ちるので表を持たない。
- 理由: 仕事の `never` の操作には fork evidence が要る。呼ばない `compute` には費用を払わせない。
- 及ぶ範囲: eml_lir と eml_core_ir の `may_fork`、VM の `MissingFork`。
- 適合の確かめ:
  - MUST 2か所の `may_fork` が `Extern::AsyncForkJob` を見る。
  - MUST `Std.Par` を import しない UI テスト (`compute_abort_reraised.em`) で中断が起こし直される。
  - MUST NOT 既存のスナップショットが変わる。
- リポジトリに残す所: core-ir.md の「`par` の子の中断」(T2)。

#### D-5: 仕事の中断は、記録した深さで mask した evv のもとで、タスクのフレームで起こし直す

- 決めたこと: `take_job` は `join_outcomes` の手順 4 と同じく、深さが 0 でなければ `(effect, keep 0, skip depth)` で mask し、`reperform$E.op` の閉包を `apply_to_unit` で呼ぶ。`drop_job` は閉包を落とす。park の後で再開したフレームの evv は park の前と同じ形なので、`fork_job` で記録した深さは `take_job` でも同じ handler を指す (docs/spec/lir.md:429-430。設計の段の試しで、park の後と別の worker の上で mask の深さ 1 の中断が同じ handler に届くことを確かめた)。
- 適合の確かめ: MUST `join_outcomes` と `take_job` が同じ `reraise` を呼ぶ。`compute_abort_reraised.em` の mask の場合が `Err 7` を出す。
- リポジトリに残す所: vm.md の「`compute` の仕事」、core-ir.md の「`par` の子の中断」(T4)。

#### D-6: 取り消しは仕事が終わってから、再開のときの取り消しの地点の規則で届く

- 決めたこと:
  - `park` の Job の枝は、取り消しの地点を先に見ない。終わった仕事では待ち手を登録しない。
  - `unpark` は `Wait::Job` を外さない。取り消しはビットを立てるだけで、タスクは park したままである。
  - 仕事の終わりは `TaskCell.woke_from_job` を立てて起こす。`resumed` はこの印を取り、`Wait::Ready` と同じく取り消しの地点の規則が成り立てば `Woke` を `Cancelled` に替えて `cancels` を1つ数える。
  - std は `Cancelled` なら `drop_job` してから `raise_cancelled ()` する。
  - そのため `compute` は仕事の後で働く取り消しの地点になる。`timeout` で囲んだ `compute` は仕事が終わってから `None` を返し、`race` で負けた腕の仕事も最後まで動く。
- 理由: 「取り消しは join の後で届く」という設計を、持ち主の worker が規則を確かめる今の形で実現する。前例は `Wait::Io` (`unpark` が外さない)、`Wait::Ready` (`resumed` が替える)、`fast_yield` (`resumed` が取る1ビットの印) である。
- 退けた案: いつも `Woke` で起こして次の取り消しの地点に任せる案では、`timeout` で囲んだ裸の `compute` が期限の後で `Some x` を返す。仕事を終えた側が登録表の lock の下で `Cancelled` を決める案は、ほかの worker からタスクの shield とスコープを読む。
- 適合の確かめ:
  - MUST NOT `park` の Job の枝が `point()` を呼ぶ、終わった仕事に待ち手を登録する。
  - MUST `unpark` の一覧に `Wait::Job` がない。`resumed` が `from_job` を見る。
  - 取り消した `compute` 1回につき `parks` と `cancels` が1つずつ増えることを、`tasks::a_cancelled_compute_counts_one_park_and_one_cancel_under_every_schedule` (逐次と2つの seed) と `workers::a_cancelled_job_on_another_worker_counts_one_cancel` (N = 2) が確かめる。
- リポジトリに残す所: vm.md の「取り消しの届け方」、stdlib.md の「`compute`」と「取り消し」(T4)。

#### D-7: 仕事の実行時エラーはその場で run を止める。捕まえない `Panic` はタスクの `Panicked` になる

- 決めたこと: `task_failed` の止める条件に `task.job.is_some()` を足す。仕事のセルは誤りの結果を持たない (`JobEnd` は `Value` と `Abort` だけ)。入口で失敗した仕事は `fork_job` の位置 (std の中) で報告する。`Panic` は実行時エラーでなく、`reperform$Prelude.Panic.panicked` でタスクの中で起こし直され、タスクの底の `try_all` に `Panicked` として捕まる。配列の範囲外の読みも `Panic` である。
- 理由: タスクの本体の誤りと、タスクが join する `par` の子の誤りは、今も run を止める (`vm.rs:1836`)。止めるのを `take_job` まで遅らせると、道が2つになり、誤りと報告の間にタスクが動く。
- 適合の確かめ: MUST `task_failed` が `task.job.is_some()` を見る。MUST NOT `JobEnd` に誤りの変種がある。`workers::a_depth_error_in_a_job_stops_the_run` と `compute_panic_caught.em` が確かめる。
- リポジトリに残す所: stdlib.md の「`compute`」、vm.md の「`compute` の仕事」(T4)。

#### D-8: 仕事の語はタスクの側で `transfer_back` してから `retire_worker` する

- 決めたこと: `take_job` と `drop_job` は、どちらも登録表の lock の外で `transfer_back(word, job の ID)`、`retire_worker(job の ID)`、`reclaim_if_due` の順に行ってから、語を使うか落とす。取り消した場合も同じである。
- 理由: spec が許す語の受け渡しはこの組だけで、`TransferLeak` の規律を守る。
- 適合の確かめ: MUST 両方の腕でこの順になっている。検査モードの `compute_lin_result.em`、`compute_cancelled.em`、`tasks::a_cancel_reaches_a_task_parked_on_a_job_only_after_the_job` が通る。
- リポジトリに残す所: vm.md の「`compute` の仕事」、object-model.md の「`transfer_back`」と「論理的な worker ID」(T4)。

#### D-9: `RunStats` に欄を足さない。`Finished` に `placed_jobs` を足す

- 決めたこと: `compute` 1回につき `parks` は必ず1つ増え (`park` の Job の枝で数える)、`cancels` は `resumed` が替えたときだけ増え、`tasks` は変わらない。`shared_objects` は fork の共有の数、`allocations` は鏡の evidence、`peak_stack` は仕事の `Task` (`depth_offset` 0) を含む。`Finished` に `placed_jobs: u64` (ほかの worker に置いた仕事の数) を足し、`placed` の意味は変えない。`in_flight` には触れない。
- 理由: スケジュールによらない欄の表をそのまま保つ。`parks` が `compute` を1回ずつ数える。
- 適合の確かめ: MUST NOT `RunStats` に欄が増える、`placed` の定義が変わる。MUST vm.md の「`RunStats`」に `compute` の数え方を書く。
- リポジトリに残す所: vm.md の「`RunStats`」(T4、T5)。

#### D-10: 仕事の終わりは、終えた worker が待つより前に起こしを送る。仕事のセルは物理的な worker を持たない

- 決めたこと: 仕事の終わりの順は、子の残りの evv を仕事の ID で手放す → worker の基準の ID に戻す → 登録表で `finish_job` して起こしを route する → `schedule` → `switch_away` である。route は受け箱に積み、受け手の worker を running にして `NOTICE` を立てる (`workers.rs:340-357`)。worker 0 のデッドロックの判定は `running == 0`、空の受け箱、mail なし、`NOTICE` なしを求めるので (`workers.rs:801-816`)、`switch_away` の前に route すれば誤って止まらない。仕事のセルは `{ worker: 仕事の論理的な ID, submitter: タスクのセル, end, waiter }` で、受け箱の番号を持たない。
- 理由: `par-steal` は仕事を動かし、`async-rescue` は待つタスクを動かす。両者は登録表を通して互いを見つける。
- 適合の確かめ: MUST `job_done` がこの順である。MUST NOT `JobCell` が受け箱の番号を持つ。N = 2 の workers のテストが `Deadlock` なしで終わる。
- リポジトリに残す所: vm.md の「`compute` の仕事」と「セルと登録表」(T4)。

#### D-11: 仕事のセルを放すのは `take_job`、`drop_job`、run の終わりの `sweep_all` だけである

- 決めたこと: この段には park したタスクを途中で捨てる仕組みがないので、出したタスクは必ず取るか落とす。後の段で park した継続を捨てる仕組み (救済、上限、ネイティブ) は、捨てるタスクを `submitter` に持つ仕事のセルを、`with_worker(仕事の ID)` と `retire_worker` の下で落とさなければならない。成功した run の終わりに生きた仕事のセルがあれば、`left_at_end` が「残ったタスク」として報告する。`sweep_all` の Job の腕は網羅のためにあり、成功した run では通らない (誤りの run と `exit` は `sweep_all` を飛ばす。`crates/eml_vm/src/lib.rs:1033-1039`)。
- 理由: 「出したタスクが必ず取るか落とす」はこの段だけで成り立つ。崩しうるのは後の段である。
- 適合の確かめ: MUST `CellKind::Job` の枠を放すのが `Registry::take_job` と `sweep_all` だけである (`git grep -n 'CellKind::Job' crates/eml_vm/src/cells.rs`)。MUST vm.md の「セルと登録表」と cells.rs のモジュールの文書にこの不変条件がある。
- リポジトリに残す所: vm.md の「セルと登録表」(T4)。

#### D-12: 仕事を待てるのは出したタスクだけである

- 決めたこと: 仕事のセルの待ち手は `waiter: Option<CellRef>` の1つだけにする。`park` と `take_job` は呼んだタスクが `submitter` でなければ `Internal` の誤りにする。
- 理由: `job_cell` は `pub` でなく、呼ぶのは `compute` だけなので、仕事の `Cell` はほかに出ない。
- 見直す条件: 仕事の `Cell` を公開するか、ほかのタスクが仕事を待てるようにするとき。そのときは待ち手を列にする。
- 適合の確かめ: vm.md に「仕事を待てるのは出したタスクだけである」がある。
- リポジトリに残す所: vm.md の「セルと登録表」(T4)。閉じるコミットのメッセージにも書く。

### 5.2 短中期の設計

理由があれば実装の途中で変えてよいが、変えたら記録する。

#### D-20: 4つの extern の行と `Job a` の型

- `Job a` は `Lin` のハンドルの型で、即値 `(generation << 32) | index` を表す。Repr は `int`、Kind は `a` によらず `Lin`、型の引数は `Unr` に固定しない (ハンドルの型)。1回だけ使うことは型検査が守る。
- `fork_job` は引数を預からない (`holding` にしない)。保持する引数に `Unr` の印は付けられず (`only_the_listed_rows_hold_an_argument`)、`int` はいつも `unr` になるためである。閉包は `par` と同じく消費され、子として動く。
- `take_job` と `drop_job` の引数の印は `Unr` で、1回だけ使うことは `Job a` の Kind が守る。
- hir のテストの内側の矢印の規則に「ラベルのない、`never` で上限を付けた開いた row」を足す。

#### D-21: `Task.job` と境目の番兵 `JOB`

- `Task.job: Option<JobRef>` (`struct JobRef { cell: CellRef, at: Pc }`) を足し、`boundaries[0]` と境目の枠 1 に番兵 `JOB = usize::MAX` を置く。これで `child_done` の枠の読みと pop、`child_abort` の空でないかの確かめ、`ChildDone` での巻き戻しの停止、`cancel()` の `boundaries[1..]` の歩きは変わらない (`vm.rs:1365-1411, 1767-1908`)。`job` で分かれるのは `child_done`、`resume` の `Unstarted`、`task_failed` の3か所だけである。
- 仕事の `depth_offset` は 0 にする。エントリの `Task` と同じ規則で、park の後に作り直したフレームの数に `peak_stack` が左右されない。

#### D-22: 仕事の evv は `dup` しない

- `fork_evv` は数えない凍結した末尾か、参照が1つの新しい節点を返す (`vm.rs:1305-1361`)。`par` の `dup` は子が2つあるためで、仕事の持ち手は1つである。参照は `Task.evv` に入り、`resume` で RC に触れずに `Ctx` へ移り (`vm.rs:1510`)、`job_done` が `switch_task(UNIT, ..)` と `decref` で手放す。右の子と同じ道である (`vm.rs:1388-1391`)。

#### D-23: T4 と T5 は置き方で分ける

- 登録表と VM の行は1つのコミットにする。登録表だけでは `-D warnings` で使われないコードになり、行は std の `compute` なしでは動かせない (std 以外は `extern` を書けず、`current_cell` は std の `start_task` が作るタスクのセルを要る)。
- T4 は仕事をいつも呼んだ worker に積む。仕事の終わりから起こし、再開までがスレッドをまたがないので、T5 の差分だけが別のスレッドの道になり、TSan と ASan を当てやすい。

#### D-24: S1 の確かめ方を、テストで書ける形に替えた所

- G-1 の「仕事が動く間、待つタスクのエントリの `Task` が消えていることを Rust のテストで見る」は、`sched.tasks` を見るテスト専用の欄が要るので書けない (CLAUDE.md が禁じる)。代わりに、登録表の単体テストで `k` と `Wait::Job` を確かめ、統合テストで `parks` を数える。`TaskState::Waiting` は `Registry::park` に届かないので、数が継続の道を通ったことを示す。
- 「`timeout` のテストで仕事の終わりを `None` より前に出す」は、仕事の row が `never` で上限を付けられていて出力できないので書けない。仮想の時計は仕事が回る間進まない。`compute_timeout.em` は `timeout 0` で `None` を出し、仕事の後で取り消しが届いたことは、`drop_job` が終わっていない仕事を内部の誤りにする規則と run の成功で確かめる。
- 「どのスケジュールでも」は、テストが並べるスケジュール (逐次、Seeded(1)、Seeded(2026) を N = 1 で、T5 で N = 2) の意味である。
- 「仕事の中の範囲外のアクセスの run-fail」は `Panic` なので、タスクで起こし直されて `Async.run` から抜ける形の run-fail になる。`Panic` でない実行時エラー (深さの上限) は、CLI の `max_depth` が `1 << 27` (`crates/eml_cli/src/lib.rs:232`) で 1 秒の規則に収まらないので、Rust のテストにする。

## 6. 実装の制約と取り決め

- コードと `docs/` はこの計画 (`docs/plans/`) と D-x、T-x を引かない。コメントと文書は日本語で、natural-japanese のスキルに従い、理由を書き、`docs/` の節を引く。
- 規則を実装するコミットに、その `docs/spec/` の節を書く。T1 は ABI の事実だけを書き、`fork_job`、`take_job`、`drop_job` の動きは T4 で書く (T1 から T3 では、行は `pub` でなく std に呼び手もないので、どのコードからも届かない)。
- テストを先に書く。既存のテストの期待値は、7 節に挙げた種類 2 と 3 のものだけが変わる。種類 1 (成否の変更) はない。それ以外のスナップショットは、std の位置 (`Async.em:75:9`、`Async.em:13-19`、`Par.em:6:28`) を含むものも含め、1バイトも変わらない。変わったら設計からのずれとして調べる (D-3)。
- `std/Async.em` では、`pub extern data Job a` を `pub extern data OpId` (l.87) の後に、4つの extern を `op_ready` の後に、`compute` を `shield` と `sleep_ms` の間に置く。`run` の位置を動かさない。
- 登録表の lock の下で `Ctx` と `Heap` を呼ばない (D-8)。
- `fork_job` の腕では、`SPAWN_WORKERS` の確かめを `current_cell` より先に行う。
- 新しいテストは単独で 1 秒以内に終わる。`async/` の UI テストの規則 (1つのタスクだけが出力する、`spin 40` で横取りを起こす、seed と N = 2/8 で流す) に従う。

## 7. 実装の作業

すべて直列で行う (T1 と T3 はどちらも `vm.rs` を変えるため)。

| ID | 作業 | 基づく決め | 前提 | 並行 |
|---|---|---|---|---|
| T0 | この計画をコミットする | - | - | - |
| T1 | 行、`Job a`、std の生の宣言、つなぎのテスト、ABI の spec | D-3、D-20 | T0 | no |
| T2 | `may_run_par` を `may_fork` にし、`fork_job` を数える | D-4 | T1 | no |
| T3 | 振る舞いを変えない整理: `fork_child`、`idle_home`、`reraise` | D-2、D-5 | T2 | no |
| T4 | 呼んだ worker の上の仕事: 登録表、VM の行、仕事の終わり、誤り、取り消し、std の `compute`、テスト、spec | D-1、D-5〜D-12、D-21〜D-24 | T3 | no |
| T5 | 空いた worker への置き方、`Finished.placed_jobs`、worker をまたぐテスト | D-2、D-9、D-10 | T4 | no |
| T6 | 段を閉じる (`Close async-compute`) | D-2、D-12 | T5 | no |

以下の期待する要約の行は、3 節の環境に依る失敗が1つ残る場合のものである。9 節の基準でその失敗が出なければ、「N tests run: N passed, 20 skipped」と読み替える。UI とトランスクリプトのファイルは、ファイルを集めるテストが1つなので数を変えない。

### T0: 計画のコミット

- 作業: この文書を `docs/plans/2026-11-03-async-compute.md` としてコミットする。
- 完了の条件: `git log -1 --stat` → このファイルだけ。

### T1: 行、`Job a`、生の宣言

- 目的: 4つの行と型を、ABI だけが確かな状態で入れる。
- 対象: `crates/eml_extern/src/lib.rs`、`std/Async.em` (`compute` はまだ入れない)、`crates/eml_vm/src/vm.rs` (汎用の振り分けだけ)、`crates/eml_hir/tests/externs.rs`、`crates/eml_core_ir/tests/externs.rs`、docs。
- 作業:
  - eml_extern: `OpId` の後に型の行を足す。`handle_type_row` の文書の「Unr のハンドル」を、`Job` が線形性を上書きすることに合わせて直す。
    ```rust
    Job => TypeRow { linearity: Lin, ..handle_type_row("Std.Async.Job") },
    ```
    `AsyncOpReady` の後、`ChanNewRaw` の前に4行を足す。
    ```rust
    AsyncForkJob => kinded("Std.Async.fork_job", &[TObj], &[Poly], Int, Unr, Purity::Effectful),
    AsyncJobCell => borrowing(&[true], FunctionRow { with_arg: Some(0), ..row("Std.Async.job_cell", &[Int], Int, Purity::Pure) }),
    AsyncTakeJob => kinded("Std.Async.take_job", &[Int], &[Unr], TObj, Poly, Purity::Effectful),
    AsyncDropJob => row("Std.Async.drop_job", &[Int], Unit, Purity::Effectful),
    ```
    `Extern::is_async` に `AsyncForkJob | AsyncTakeJob | AsyncDropJob` を足し (`AsyncJobCell` は足さない)、その文書に `job_cell` と `chan_cell` が例外であることを書く。
  - `std/Async.em`: l.87 の `pub extern data OpId` の後に `pub extern data Job a`。`op_ready` の後に次の4行。
    ```haskell
    extern fork_job         : (Unit -> <never e> a) -> <IO> Job a
    extern job_cell         : Job a -> (Cell, Job a)
    extern take_job         : Job a -> <IO> a
    extern drop_job         : Job a -> <IO> Unit
    ```
  - vm.rs の汎用の振り分け: `Extern::ChanToCell | Extern::AsyncJobCell => task.set_raw(dst, task.raw(arg(0)))`。3つの `is_async` の行を届かない一覧に足す (T4 までは `async_op` の `_ =>` の腕が断る)。
  - `crates/eml_hir/tests/externs.rs` の `every_extern_function_is_declared_once_in_std` の内側の矢印の腕を次にする。
    ```rust
    RowRef::Open { effects, tail, .. } => {
        effects.is_empty()
            && (Some(*tail) == shared_tail
                || signature.generics.row_vars[*tail].never.is_some())
    }
    ```
  - `crates/eml_core_ir/tests/externs.rs`: `probe_job` の probe と `probe()` の腕を足す。借用と組の表に `"Std.Async.job_cell" => (&[true], Some(0))`、印の表に `"Std.Async.fork_job" => (&[Poly], Unr)` と `"Std.Async.take_job" => (&[Unr], Poly)` を足し、表の文書の「18行」を「20行」にする。
- spec (ABI の事実だけ):
  - stdlib.md の「`Std.Async`」: ABI の塊に `pub extern data Job a`。`Job a` の項 (即値、Kind は `a` によらず `Lin`、Repr は `int`、型の引数は `Unr` に固定しない、1回だけの使用は型検査が守る)。extern の表を「33行」から「37行」にし、`fork_job`、`job_cell` (token を借りる)、`take_job`、`drop_job` の行を足す。「VM はこれらの行を…`Async` の命令で実行する」に「`job_cell` は `chan_cell` と同じく即値を写す普通の extern」を添える。`task_cell` の項の隣に `job_cell` の項 (借りて組を返す。core-ir.md の「組を返す extern」を引く)。
  - declarations.md の「`extern`」: l.122 の数を直す。基準の 62 行 (Async 33、Chan 6、Atomic 4、Net 19) が T1 の後に 65 行 (Async 36) になる。l.127 のハンドルの型の一覧に `Std.Async.Job a` (Kind は `Lin`)。l.128 の内側の矢印の規則に「`Std.Async.fork_job` のように `never` で上限を付けた row 変数」。
  - types.md l.21、l.23: ハンドルの例に `Std.Async.Job a` (Kind は `a` によらず `Lin`)。
  - core-ir.md: 印の表に `fork_job | poly | unr` と `take_job | unr | poly`。`with_arg` の一覧 (l.76、l.232) に `job_cell`。「組を返す extern」に `job_cell` の項。
- テストの変更:
  - 種類 2: `eml_hir` の `externs::every_extern_function_is_declared_once_in_std` (許す内側の矢印の形を広げる。理由は `fork_job` の callback)。
  - 種類 2: `eml_core_ir` の `externs::only_the_listed_rows_borrow_or_return_a_pair` (`job_cell` を足す) と `externs::only_the_listed_rows_carry_a_lin_or_poly_mark` (`fork_job`、`take_job` を足す)。
  - 種類 3: `probe_job`。
  - `only_the_listed_rows_hold_an_argument` は変えない。
- 実装者に任せること: なし。
- 完了の条件:
  - `cargo nextest run -p eml_hir -p eml_core_ir --test integration -E 'test(externs::)'` → `29 tests run: 29 passed`
  - `cargo nextest run` → `4269 tests run: 4268 passed, 1 failed, 20 skipped`
  - `cargo clippy --all-targets -- -D warnings` → 警告なし、exit 0
  - `cargo fmt --check` → exit 0
  - `git status --short '*.snap'` → 空
- 落とし穴: `Job` の行を `holding` にしない (D-20)。

### T2: `may_fork`

- 目的: `fork_job` を含むプログラムに fork の表を作る。
- 対象: `crates/eml_lir/src/{lib.rs,verify.rs}`、`crates/eml_core_ir/src/evidence/{forks.rs,mod.rs}`、`crates/eml_vm/src/{compile/forks.rs,code.rs}`、`crates/eml_vm/tests/machine.rs:331`、`crates/eml_lir/tests/verify.rs`、`crates/eml_core_ir/tests/evidence.rs`、docs。
- 作業:
  - `Program::may_run_par` (eml_lir) と `may_run_par` (eml_core_ir の forks.rs) を `may_fork` にし、`| Rhs::Extern { ext: Extern::AsyncForkJob, .. }` を足す。モジュールの文書と `src/evidence/mod.rs:4` を直す。
  - `verify.rs` の `forks()` の文言「but the program may run par」を「but the program may fork」にする。
  - VM の `compile/forks.rs` と `code.rs:223` を `may_fork()` にする。
- spec: core-ir.md の「`par` の子の中断」の最初の項に「fork しうる」の定義 (`Call::Par`、`extern Std.Async.fork_job`、`prim root_get` のどれかを含む。`eml_lir::Program::may_fork`)。core-ir.md l.864 を「fork しうる」に。lir.md l.746 に新しい名前と文言。vm.md l.33、l.534、l.537。docs/future/multicore.md l.378。
- テストの変更:
  - 新規: `eml_core_ir` の `evidence::a_program_with_fork_job_gets_the_forks`。`a_program_with_root_get_gets_the_forks` と同じ形で `let j.1: int = extern Std.Async.fork_job(f.0)` を使い、`fork$` の名前が `["fork$Prelude.Panic.panicked", "fork$One.one", "fork$Two.two"]` であることを確かめる。
  - 種類 3: lib の単体テスト `a_program_may_run_par_with_a_par_or_a_root_get` を `a_program_may_fork_with_a_par_a_fork_job_or_a_root_get` に改名し、`extern Std.Async.fork_job` の確かめを1つ足す。
  - 種類 2: `eml_lir` の `verify::a_program_that_may_run_par_has_every_fork_entry` の2つの文言。種類 3: その名前を `a_program_that_may_fork_has_every_fork_entry` に。
  - 種類 3: `machine.rs:331`。
- 実装者に任せること: なし。
- 完了の条件:
  - `git grep -n may_run_par` → 出力なし
  - `git grep -n '走らせうる' -- docs crates` → 出力なし
  - `cargo nextest run -p eml_core_ir --test integration evidence::a_program_with_fork_job_gets_the_forks` → `1 test run: 1 passed`
  - `cargo nextest run -p eml_lir --test integration verify::a_program_that_may_fork_has_every_fork_entry` → `1 test run: 1 passed`
  - `cargo nextest run -p eml_lir --lib a_program_may_fork` → `1 test run: 1 passed`
  - `cargo nextest run` → `4270 tests run: 4269 passed, 1 failed, 20 skipped`
  - clippy と fmt は T1 と同じ。`git status --short '*.snap'` → 空

### T3: 振る舞いを変えない整理

- 目的: `par` の fork と join の手順を、T4 と T5 が使える関数に切り出す。
- 対象: `crates/eml_vm/src/vm.rs` だけ。spec とテストは変えない。
- 作業:
  - `fn fork_child<R: Exec>(code, ctx, sched, g: Word) -> Result<(u32, Word), VmFault>`: `fork_worker`、`unshared_roots` の共有、`share(g, child)`、`fork_evv(ctx.evv_word())`。`(child, fork_evv)` を返し、`fork_evv` はまだ共有しない。
  - `fn idle_home<R: Exec>(ctx, sched) -> Result<Option<usize>, VmFault>`: `places()` なら `pop_idle()`。検査モードの「置いた子が共有していない根を読むかもしれない」の確かめも含む。
  - `fn reraise<R: Exec>(code, ctx, task, abort: (u16, u32, Word), ret: Pc, dst: Slot) -> Result<(), VmFault>`: 深さが 0 でなければ `(effect, keep 0, skip depth)` で mask し、`apply_to_unit(clause, ret, dst)`。`join_outcomes` は `(join.ret, join.dst)` で呼ぶ。
- 実装者に任せること: 関数の中の書き方。引数の型と順は上のとおりにする。
- 完了の条件:
  - `git diff --stat HEAD~1` → `crates/eml_vm/src/vm.rs` だけ
  - `cargo nextest run` → `4270 tests run: 4269 passed, 1 failed, 20 skipped` (`bench::` のスナップショットは変わらない)
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::` → 成功。`-Zmiri-tree-borrows` を足してもう1回 → 成功
- 落とし穴: `par` の中の順 `share(g)` → `fork_evv` → `dup` → `share(fork_evv)` → `switch_task` を変えない。

### T4: 呼んだ worker の上の仕事と `compute`

- 目的: `compute` の動きをすべて入れる。仕事はいつも呼んだ worker の `ready` に積む。
- 対象: `crates/eml_vm/src/{cells.rs,vm.rs,sched.rs}`、`std/Async.em`、テスト、docs。
- 作業の順 (テストを先に): 登録表の単体テスト → cells.rs → VM の `Task.job`、`job_done`、分かれる3か所、3つの行 → std の `compute` → UI と Rust の統合テスト → spec。
- 作業:
  - sched.rs: `PLACED` の隣に `pub(crate) const JOB: JoinId = usize::MAX;`。
  - cells.rs (`std` と `eml_rt` だけを import する):
    - 型: `pub(crate) enum JobEnd { Value(Word), Abort { effect: u16, depth: u32, clause: Word } }` と `fn word(&self) -> Word`。`struct JobCell { worker: u32 /* 仕事の論理的な ID */, submitter: CellRef, end: Option<JobEnd>, waiter: Option<CellRef> }` と `CellKind::Job(JobCell)`。`Wait::Job(CellRef)`。`TaskCell.woke_from_job: bool` を、テストの helper を含めて `TaskCell` を作るすべての所で `false` にする。
    - `Registry::new_job`、`job_mut`、`Registry::finish_job(cell, end, out: &mut Vec<Woken>)`、`Registry::take_job(raw, me) -> Result<(u32, JobEnd), CellFault>` と、lock を取って終えて route するモジュールの関数 `finish_job(shared, here, cell, end, out: &mut Vec<Runnable>)` (`cells.rs:4453` の `finish` と同じ形)。`take_job` の誤りは `Internal` で、文言は「a task takes the job of another task」と「a job is taken before it ends」。`take_job` は枠を放す。
    - `park` の Job の枝: submitter でなければ `Internal("a task waits for the job of another task")`。`suspend` して `parks += 1`。`point()` を先に見ない。仕事が終わっていれば `woke_from_job` を立てて `Resume { wake: WOKE }` を返し、待ち手を登録しない。そうでなければ `k = Some(k)`、`wait = Some(Wait::Job(c))`、`waiter = Some(me)` にして `None` を返す。
    - `finish_job`: `end` を置く。`waiter.take()` が `Some(t)` なら、`t.wait` が `Some(Wait::Job(cell))` でなければ `Internal("a job wakes a task that does not wait for it")`。`woke_from_job` を立てて `wake_cell(t, WOKE, None)` を積む。
    - `resumed`: `let from_job = take(&mut task.woke_from_job)` とし、替える条件を `(ready || from_job) && !fast && wake == WOKE && point(cell)` にする。
    - `unpark` は変えない (`Wait::Job` は `cells.rs:2345` の一覧にない)。
    - `left_at_end`: `Job(_) => true`。`sweep_all`: Job の腕は `end` の語を `worker` の下で落とし、`retire` して `release` する。
    - モジュールの文書: `k` と待ちの不変条件に `Job` を足し、仕事のセルを放すのは `take_job`/`drop_job` の行と `sweep_all` だけという不変条件を書く。
  - vm.rs:
    - `Task.job: Option<JobRef>`、`struct JobRef { cell: CellRef, at: Pc }`。すべての作り手は `None` にする。`Task::job(entry, worker, evv, max_depth, job, g)` は `Task { job: Some(job), ..Task::right_child(entry, worker, evv, 0, max_depth, JOB, g) }`。
    - `async_op` の `Extern::AsyncForkJob` の腕: (1) `Wanted` なら何より先に `return Err(SPAWN_WORKERS)`。(2) `me = current_cell(task)?`、`g = task.get(arg(0))`。(3) `(child, evv) = fork_child(..)` の後 `share(evv, child)`。`dup` しない。(4) `cell = sched.cells.lock().new_job(child, me)`。(5) `job = Task::job(task.entry, child, evv, sched.max_depth, JobRef { cell, at: task.pc - 1 }, g)`。(6) `let id = sched.add_task(job); sched.ready.push_back(id)`。(7) `task.set_raw(dst, cell.encode())`。
    - `ext @ (AsyncTakeJob | AsyncDropJob)` の腕: `me = current_cell(task)?`、`(worker, end) = sched.cells.lock().take_job(task.raw(arg(0)), me)?`。lock の外で `transfer_back(end.word(), worker)`、`retire_worker(worker)`、`ctx.reclaim_if_due()`。`TakeJob` で `Value(w)` なら `task.set(dst, w)`、`Abort { effect, depth, clause }` なら `return reraise(.., (effect, depth, clause), task.pc, dst)`。`DropJob` なら `decref(end.word())` の後 `task.set(dst, Word::UNIT)`。
    - `child_done`: pop の後で `task.boundaries.is_empty()` なら、`task.placed` より先に `task.job` を見て `job_done` を呼ぶ。
    - `fn job_done<R: Exec>(code, ctx, task, sched, job: JobRef, outcome: Outcome)`: (1) `Value`/`Abort` を `cells::JobEnd` にし、`Error` は `Internal("a job ends with an error")`。(2) `decref(switch_task(UNIT, task.worker, task.depth_offset))`。(3) `switch_task(UNIT, sched.main_worker, 0)`。(4) `cells::finish_job(&sched.cells, &here, job.cell, end, &mut out)?` (中で route する)。(5) `out` の runnable をそれぞれ `schedule`。(6) `switch_away(.., None)`。
    - `resume` の `Unstarted`: `at` を `task.job`、`task.placed`、`sched.join(boundaries[0]).ret - 1` の順に取る。`JOB` で join を引くと範囲外なので、`job` を先に見る。
    - `task_failed`: 止める条件に `|| task.job.is_some()` を足す。
  - `std/Async.em`: `shield` と `sleep_ms` の間に次を入れ、l.328-332 のコメント (結果の型 `a` を縛るシグネチャの一覧) に `compute` を足す。
    ```haskell
    pub compute : (Unit -> <never e> a) -> <IO, Async | e> a
    compute f =
      let (c, j) = job_cell (fork_job f)
      match await_cell c with
        | Woke -> take_job j
        | Cancelled ->
          drop_job j
          raise_cancelled ()
    ```
- spec (すべての動き。置き方は「呼んだ worker の実行待ちに積む」と書く):
  - stdlib.md: 表に `compute | (Unit -> <never e> a) -> <IO, Async | e> a | eml` の行。「時間」の後に新しい `### `compute``。書く規則は、(a) `compute f` は `f ()` を仕事として動かし、呼んだタスクだけが待ち、worker はほかのタスクを動かす、(b) 置き方 (T4 では呼んだ worker の実行待ち)、(c) `f` の row は `never` で上限を付ける (`IO`、`Async`、`var` のエフェクト、`once`/`multi` は E2020)、`Lin` の捕捉と結果は許す、(d) 仕事の `never` の操作は仕事が終わった後に呼んだタスクで、仕事が届いた handler に起こし直す (`mask` を守る)、捕まえない `Panic` はタスクの `Panicked` になる、(e) ほかの実行時エラーは run を止める、(f) `compute` は仕事の後でだけ働く取り消しの地点で、仕事の前や途中の取り消しは仕事を止めず、仕事の後で結果を落として `Cancelled` を投げる、`shield` の中では外の取り消しは届かない、(g) `timeout` で囲んだ `compute` は仕事の後でだけ `None` を返し、負けた `race` の腕の仕事も最後まで動く。盗みと手伝いは書かない。「取り消し」の取り消しの地点の一覧に `compute` を足す (仕事が終わってから届く)。
  - types.md l.61: 「まだ実装していない」を消し、stdlib.md の「`compute`」を引く。
  - vm.md の「`par` の fork と join」の下に新しい `### `compute` の仕事`: `fork_job` の手順 (`SPAWN_WORKERS`、左の子のない fork の手順 2〜4、子の evv は1回だけ共有し `Ctx` の evv には触れない、仕事のセル、`depth_offset` 0 と `boundaries = [JOB]` の仕事の `Task`、実行待ちへ積む、token を `dst` へ)。仕事の終わりの順 (D-10。route を switch away の前に行う理由はデッドロックの判定)。仕事の実行時エラーはすべて run を止め、入口の誤りは `fork_job` の位置で報告する。仕事の中の入れ子の `par` は動く。`take_job`/`drop_job` の手順 (D-8) と、中断を join の手順 4 と同じく次の命令を戻り先にして起こし直すこと、`drop_job` は落とすこと。
  - vm.md の「命令」: 3つの行と、即値を写す `job_cell`。
  - vm.md の「セルと登録表」: 仕事のセル (論理的な ID、submitter、終わり (値か中断で、誤りはない)、待ち手)。「仕事を待てるのは出したタスクだけである」(D-12)。放すのは `take_job`/`drop_job` の行と `sweep_all` だけで、park した継続を捨てる仕組みは、捨てるタスクを submitter に持つ仕事のセルを `with_worker(仕事の ID)` と `retire_worker` の下で落とす (D-11)。
  - vm.md の「待ちの2種類」: 仕事の待ちは継続の待ちである。
  - vm.md の「取り消しの届け方」: `Wait::Job` の規則 (D-6)。
  - vm.md の `left_at_end` を書いた所: 生きた仕事のセルは残ったものとして数える。
  - vm.md の「タスクとスケジュール」l.404: worker は最初の `fork_job` でも作る。
  - vm.md の「`RunStats`」: `compute` 1回の数え方 (D-9)。
  - core-ir.md の「`par` の子の中断」: `compute` の仕事の操作は、`take_job` が同じ `reperform$E.op` で起こし直す。l.334 (never だけの handler を残す) に `compute` を足す。
  - object-model.md の「`transfer_back`」(l.292、l.309) と「論理的な worker ID」(l.735、l.739): 仕事の ID は `fork_job` で出し、`take_job`/`drop_job` で退かせる。happens-before は登録表の lock (終える → 取る) を通る。
  - testing.md: 「分類」の `async/` に `compute`。「seed で流す」の `tasks`/`parks` の項に「`parks` は `await`、`yield_now`、`compute` の呼び出しの数」。書く前に、`tasks` が根を数えるという今の文言と `the_task_counts_do_not_depend_on_the_schedule` (start 2回で `tasks == 3`、await 2回で `parks == 2`) を確かめ、根を数えることが残る書き方にする。
- テスト (すべて新規。既存の期待値は変わらない):
  - UI `tests/ui/run/async/`。どのファイルも、先頭の行に日本語のコメント、再帰の `spin`、次の `Async` の操作の前に `let t = Async.start (fn () -> spin 40)` と `let _ = spin 40`、出力するタスクは一度に1つ、最後に `println (show (Async.await t))` で `0` を出す。

    | ファイル | 出力 |
    |---|---|
    | `compute_value.em` | `a 55`、`t 5050`、`0` |
    | `compute_abort_reraised.em` (`Std.Par` を import しない。mask は run/par/mask_inside_the_child.em の `foo` と同じ) | `Err 1`、`Err 7`、`Ok 2`、`0` |
    | `compute_panic_caught.em` | `Err (Panicked "division by zero")`、`Err "boom"`、`0` |
    | `compute_lin_result.em` | `3 7`、`0` |
    | `compute_timeout.em` | `Some 1`、`None`、`Some 2`、`0` |
    | `compute_cancelled.em` | `shielded 5`、`compute: Cancelled: task was cancelled`、`requested False`、`0` |
    | `compute_many_tasks.em` | `55`、`210`、`465`、`820`、`0` |
    | `compute_nested_par.em` | `8`、`0` |
    | `compute_after_par_wait.em` (worker が1つで、`par` の join で待つタスクと、ほかのタスクの積んだ仕事が並ぶ) | `3`、`10`、`0` |

    `compute_value.em` は never だけの handler を持たないので仕事の evv は凍結した末尾、`compute_abort_reraised.em` は `Fail` の evidence を鏡に写す。
  - UI `tests/ui/run-fail/async/compute_out_of_range.em`: `println "before"` の後、本体が `spin 40` で始まり `spin 40` を回す `Async.run` の中で `compute (fn () -> let (v, m) = MutArray.read (MutArray.make 2 1) 5; drop m; v)`。標準出力は `before`、実行時エラーは `panicked: array index 5 is out of range for length 2` と `  at <std>/Async.em:75:9` (`run_fail@async__await_rethrows_panic` と同じ道)。
  - UI `tests/ui/check-fail/types/`: `compute_performs_io.em` (E2020 が1つ)、`compute_performs_async.em` (E2002 と E2020)、`compute_in_par_child.em` (E2020 が1つ)。
  - REPL `crates/eml_cli/tests/repl/async/compute.repl`: (1) `import Async (Async)`、`import Fail (Fail(..))`。(2) どの `compute` と `par` より先に `effect Stop where never stop : Unit -> a`。(3) `let xs = [1, 2, 3]`。(4) `Async.run (fn () -> Async.compute (fn () -> 40 + 2))`。(5) `Async.run (fn () -> handle Async.compute (fn () -> stop ()) with | ctl stop () -> 0)` (宣言から2つ後の入力で `Stop` が初めて fork する)。(6) `xs` (根) を読む `compute`。(7) `compute` を使った後で `effect Stop2 ...` を宣言し、`compute` を囲んで handle する。
  - Rust の `crates/eml_vm/src/cells.rs` の `#[cfg(test)]` (名前を `a_read` と `a_write` で始めない):
    - `a_cancel_leaves_a_task_parked_on_a_job`: 終わっていない仕事で `park` → `None`、`task.k.is_some()` と `wait == Some(Wait::Job(c))`。`cancel_task` → 起こしなし、`cancels == 0`。`finish_job` → 起こしが1つ。`resumed(.., WOKE)` → `CANCELLED`、`cancels == 1`。
    - `a_task_parked_on_a_finished_job_resumes_at_once`: 先に `finish_job`、次に `park` → `Resume { wake: WOKE }`、`parks == 1`、`waiter.is_none()`。`resumed` → `WOKE`。
    - `a_job_of_another_task_is_an_internal_error`: submitter でないタスクの `park` と `take_job` → `Err`。
  - Rust の `crates/eml_vm/tests/tasks.rs` (断りがなければ逐次と検査モード):
    - `a_task_parked_on_a_job_lets_the_worker_run_other_tasks`: 本体で `start (fn () -> println "t")`、`println (show (compute (fn () -> 41)))`、`await`。出力 `"t\n41\n"`、`(tasks, parks, cancels) == (2, 2, 0)`。
    - `a_cancel_reaches_a_task_parked_on_a_job_only_after_the_job`: `scope` の中で `spawn_in s (fn () -> match try_io (fn () -> compute (fn () -> MutArray.make 2 7)) with ...)` が `job: Cancelled` を出す。main は `yield_now`、`cancel_scope s`、`println "after"`。逐次だけ。出力 `"job: Cancelled\nafter\n"`、`cancels == 1`、検査モードで `Ok` (`drop_job` が終わった仕事を見て、`Lin` の結果が漏れなかったことを示す)。
    - `a_cancelled_compute_counts_one_park_and_one_cancel_under_every_schedule`: タスクが `scope (fn s -> cancel_scope s; try_io (fn () -> compute (fn () -> MutArray.make 2 (spin 40))))` を動かす。`Sequential`、`Seeded(1)`、`Seeded(2026)` のそれぞれで `Ok`、出力と `parks` が逐次と同じ、`cancels == 1`。
    - `a_job_releases_its_fork_evv`: `run_both` (検査と速い) で、never だけの handler のない `compute (fn () -> 1)` と、それを `Fail.to_result` の中に置いたものの2つ。出力を確かめる (検査モードは RC のずれと漏れで失敗する)。
    - `exit_abandons_a_queued_job`: N = 1、逐次。main が `println (show (compute (fn () -> 1)))` を動かす T を `spawn` し、`yield_now`、`exit 3`。出力 `""`、`exit == Some(3)`、`parks == 2`。
  - Rust の `crates/eml_vm/tests/workers.rs`:
    - `a_depth_error_in_a_job_stops_the_run`: `limits` で `max_depth: 4096`。空きを作る手順 (no-op を `start` して `await`、`sleep_ms 1`)、`println "before"`、`compute (fn () -> deep 100000)`。N = 1 と N = 2 のどちらも `before\n` を出し、誤りの文言が等しく、`the call depth limit is exceeded` で始まる。T4 では仕事は呼んだ worker で動き、T5 の後は worker 1 に置かれる。テストは変えない。
- 実装者に任せること: 上の出力に合うテストの中身、日本語の文とコメント。期待した出力と違ったら、スナップショットを受け入れる前に VM を調べる。`tasks`/`parks` の数 (`(2, 2, 0)` と `parks == 2`) は類推の値なので、違えば数を直し、設計は変えない。
- 完了の条件:
  - `cargo insta review` で新しいスナップショット 14 個 (run 9、run-fail 1、check-fail 3、REPL 1) を、中身が上の行と合うことを確かめて受け入れる
  - `git status --short '*.snap'` → `compute` を含む `??` の行だけ
  - `cargo nextest run -p eml_vm --lib cells::a_cancel_leaves_a_task_parked_on_a_job cells::a_task_parked_on_a_finished_job_resumes_at_once cells::a_job_of_another_task_is_an_internal_error` → `3 tests run: 3 passed`
  - `cargo nextest run -p eml_vm --test integration tasks::a_task_parked_on_a_job tasks::a_cancel_reaches tasks::a_cancelled_compute tasks::a_job_releases tasks::exit_abandons_a_queued_job workers::a_depth_error_in_a_job` → `6 tests run: 6 passed`
  - `cargo nextest run` → `4279 tests run: 4278 passed, 1 failed, 20 skipped`
  - `cargo nextest run -j 1 -p eml_vm` → `SLOW` の行なし
  - `cargo clippy --all-targets -- -D warnings` と `cargo clippy -p eml_io -p eml_vm --all-targets --no-default-features -- -D warnings` → 警告なし。`cargo fmt --check` → exit 0
  - Miri の `eml_vm` `miri::` を2つの設定で → 成功
  - ASan (macOS): `nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=address CARGO_TARGET_DIR=target/asan cargo test -Zbuild-std --target aarch64-apple-darwin -p eml_vm --test integration -- workers:: tasks::` → `test result: ok`
  - TSan: 9 節のコマンドを x86_64 Linux で → exit 0 (4 節の「ないとき」に従う)
- 落とし穴: `SPAWN_WORKERS` を `current_cell` より先に。仕事の evv を `dup` しない。`finish_job` の前に基準の ID に戻す。`switch_away` の前に route する。登録表の lock の下で `Ctx` と `Heap` を呼ばない。`resume` では `job` を先に読む。`woke_from_job` を立てるのは2か所で、取るのは `resumed` だけ。`park` の Job の枝で `point()` を呼ばない。`compute` で `run` の位置を動かさない。

### T5: 置き方と `placed_jobs`

- 目的: 仕事を空いた worker に置き、数える。
- 対象: `crates/eml_vm/src/{vm.rs,workers.rs,lib.rs}`、`crates/eml_cli/tests/ui.rs`、`crates/eml_vm/tests/workers.rs`、docs。
- 作業: `fork_job` の腕の手順 (6) を `match idle_home(ctx, sched)? { Some(k) => { placed_jobs += 1; send(k, Notice::Child(job)) } None => T4 と同じ }` にする。`in_flight` には触れない。`Run.placed_jobs: AtomicU64`、`Scheduled.placed_jobs`、`Finished.placed_jobs: u64` を足し、`lib.rs:693, 720, 1075` で埋める。文書の文字列は vm.md の「`compute` の仕事」を引く。
- spec: stdlib.md の「`compute`」の置き方を「`par` の右の子と同じく、空いた worker の受け箱、なければ呼んだ worker の実行待ち。worker が1つ (REPL、`--workers 1`) なら、その worker のタスクと交互に動く」にする。vm.md の「`compute` の仕事」に、`in_flight` なしの fork の手順 7 と同じ置き方、`placed_jobs` で数えること、ほかの worker で終わった仕事は submitter の worker に起こしを route すること。vm.md の「タスクとスケジュール」の `Run` の表 (l.399) と `Finished.placed_jobs` (l.404)。vm.md の「`RunStats`」l.827、l.872 に `placed_jobs`。vm.md の「並列化への備え」の項 4 に、`Notice::Child` が仕事のタスクも運ぶこと、仕事の語は登録表の lock の下の仕事のセルを通り、submitter が `transfer_back` すること。
- テスト:
  - 種類 3: `ui::stats_are_compared_by_cancellations_and_channels` と `ui::stats_of_driver_runs_are_compared_without_the_work_counts` の `Finished` の literal (ui.rs l.446、l.490) に `placed_jobs: 0`。
  - 新規 (`workers.rs`):
    - `a_job_is_placed_on_an_idle_worker`: 空きを作る手順 (`a_timer_on_worker_0_and_a_child_on_worker_1_agree` と同じ) の後 `compute (fn () -> count 20000 0)`。N = 1 と N = 2 のどちらも `20000` を出す。N = 1 で `placed_jobs == 0`、N = 2 で `(workers, placed_tasks, placed, placed_jobs) == (1, 1, 0, 1)`。
    - `exit_abandons_a_job_on_another_worker`: N = 2。`Async.spawn (fn () -> println (show (Async.compute (fn () -> spin 0))))` (`spin 0` は止まらない)、`yield_now`、`exit 3`。出力 `""`、`exit == Some(3)`、`workers == 1`。確かめるのは「止まらない仕事が生きている run が `exit` で終わる」ことだけで、その時点で仕事が積まれているか動いているかは問わない。
    - `a_cancelled_job_on_another_worker_counts_one_cancel`: `tasks::a_cancelled_compute_...` の自分を取り消すプログラムを、空きを作る手順の後に N = 2 で。出力は同じ、`cancels == 1`、`placed_jobs == 1`。
- 実装者に任せること: 空きを作る手順で worker 1 が `fork_job` の時点で空いていないと分かったら、`a_child_runs_on_another_worker` の手順に替える。確かめる値は弱めない。
- 完了の条件:
  - `cargo nextest run -p eml_vm --test integration workers::a_job_is_placed_on_an_idle_worker workers::exit_abandons_a_job_on_another_worker workers::a_cancelled_job_on_another_worker_counts_one_cancel` → `3 tests run: 3 passed`
  - `cargo nextest run -p eml_cli --test integration ui::stats_` → `2 tests run: 2 passed`
  - `cargo nextest run` → `4282 tests run: 4281 passed, 1 failed, 20 skipped`
  - ASan は T4 と同じ。TSan は 9 節のコマンド。clippy と fmt は T4 と同じ

### T6: 段を閉じる (`Close async-compute`)

- 目的: docs/future/roadmap.md の「進め方」の順で文書を今の状態に直す。
- 作業:
  1. spec を仕上げる: `git grep -n 'まだ実装していない' -- docs/spec/types.md` に `compute` の話がない。`git grep -n '盗み\|手伝い' -- docs/spec` に `compute` の話がない。段の名前と予告が残っていない。
  2. 計測 (macOS): まず `bench/scripts.sh` を流し、範囲を外れた行があれば別のコミット (`Resize bench rows`) で大きさを変える。次に T5 のコミットのきれいな作業ツリーで `bench/run.sh`、`bench/rt.sh`、`bench/scripts.sh` を流し、docs/implementation/benchmarks.md の「今の数値」の3つの表を入れ替える。`bench/async/scale.sh` に行は足さない。macOS がなければ表の入れ替えを直後の `Measure async-compute` に回す。
  3. status.md: l.36 の `Async` の範囲に `compute`。`### Async` の制限に、呼んだ worker に積んだ仕事が動かないこと (roadmap の `par-steal` を引く)、仕事を取り消さず `timeout` は仕事の後で戻ること、`Panic` でない仕事の実行時エラーは run を止めること、タスクの中の `par` はスタックを持ったままで取り消しの地点でないこと。l.156 の一覧に `compute`。
  4. architecture.md: l.294 を「fork しうる」に。l.316 に `compute` の仕事と、登録表の一覧に仕事のセル。l.317 に worker は最初の `fork_job` でも作ること。l.426 に `placed_jobs`。
  5. roadmap.md: `async-compute` の項 (l.31) を消す。`par-steal` の項 (l.64) に「呼んだ worker に積んだ `compute` の仕事も、右の子と同じく盗まれる」。
  6. future の文書: async.md の「`compute`」(l.366-376) と l.216 の `compute` の節を消し、l.10 から `compute` を外し、l.121 を stdlib.md の「`compute`」を引く文にする。multicore.md の「`par` の子」に「仕事は join するフレームを持たない fork の子で、`par-steal` では deque の項になり、終わりで仕事のセルを終える」。multicore.md:415 を stdlib.md の「`compute`」に向け直す (直さないと `citations.rs` が落ちる)。native.md の「並列の実行」に「`compute` の仕事は join するフレームを持たない fork の子で、盗む worker か手伝う worker が動かし、終わりで仕事のセルを終える。出したタスクはほかの `Async` の待ちと同じく `resume_task` で再開する」。
  7. overview.md: 「取り消しと時間」の後に「CPU の仕事」の行 (stdlib.md の「`compute`」を引く)。
  8. docs/README.md: stdlib の行を「(取り消し、時間、`compute` を含む)」に。
  9. CLAUDE.md (道しるべだけ): `Std.Async` の行に stdlib.md の「`compute`」と vm.md の「`compute` の仕事」。
  10. この計画を消す。消す前に、5 節の決めがすべて「リポジトリに残す所」に書かれていることを確かめる。
  11. コミットのメッセージ: async.md から何をどこへ移したか、テストの変更の種類と理由 (7 節の T1、T2、T5)、計測の目立つ差、大きさを変えた行、declarations.md の extern の数を直したこと、D-12 の制約 (仕事を待てるのは出したタスクだけで、`Cell` を公開するなら列にする)、10 節の U-7。
- 完了の条件:
  - `git grep -n compute -- docs/future` → async.md:121、ffi.md:149、multicore.md:167、multicore.md の「`par` の子」の新しい文、multicore.md:415、multicore.md:559、roadmap.md の `par-steal` の項、roadmap.md:274、native.md の新しい文だけ
  - `cargo nextest run -p eml_cli --test integration citations::` → 全部成功
  - `ls docs/plans` → 空 (またはディレクトリがない)
  - 9 節の全体の確かめ

## 8. 進める順

T0 → T1 → T2 → T3 → T4 → T5 → T6 の直列。T1 と T3 はどちらも `vm.rs` を変え、T4 は T3 の関数を、T5 は T4 の行を使うので、並行にできるものはない。TSan は T4 と T5 のコミットの前に流す (4 節)。

## 9. 確かめの計画

- 基準 (最初の変更の前、08a1e01c で): `cargo nextest run` → `4269 tests run: 4269 passed, 20 skipped`。3 節の `eml_io` のテストが失敗するなら `4268 passed, 1 failed` で、そのときは 7 節の期待する行がそのまま当てはまる。ほかの失敗があれば止めて利用者に聞く。
- 全体の確かめ (T5 の後と T6 の後):
  - `cargo nextest run` → `4282 tests run: 4282 passed, 20 skipped` (環境に依る失敗が出る所では `4281 passed, 1 failed`)
  - `cargo clippy --all-targets -- -D warnings`、`cargo fmt --check` → exit 0
  - feature の組み合わせ (testing.md の「feature の組み合わせの確認」): `cargo clippy -p eml_cli --no-default-features [--features types|core|run] -- -D warnings`、`cargo clippy -p eml_test_support --no-default-features [--features hir|types|core] -- -D warnings`、`cargo clippy -p eml_io -p eml_vm --all-targets --no-default-features -- -D warnings` → どれも警告なし
  - `cargo nextest run -p eml_io -p eml_vm -E 'test(tls_off::)'` → 全部成功
  - Windows msvc、x86_64 Linux、aarch64 Linux の cross clippy (CLAUDE.md のコマンド) → 警告なし
  - Miri: `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::` と `... cargo miri test -p eml_vm --lib -- cells::a_read cells::a_write counters::` を、それぞれ `-Zmiri-tree-borrows` ありとなしで → 成功
  - ASan (macOS、T4 と同じコマンド) → `test result: ok`
  - TSan (x86_64 Linux): `nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=thread TSAN_OPTIONS=halt_on_error=1 CARGO_TARGET_DIR=target/tsan cargo test -Zbuild-std --target x86_64-unknown-linux-gnu -p eml_vm --test integration -- workers:: tasks:: signal:: preempt:: io:: chan::` → exit 0。CI の TSan の組は変えない
  - CI の timing の確かめ (`cargo nextest run --profile timing -j 1`、x86_64 Linux) → exit 0
  - Verus と verusfmt は `eml_rt` に触れないので流さない
- 利用者の手での確かめ: なし。TSan を流せる Linux の環境がないときは、T6 の前に CI の TSan の job が通ったことを利用者が見る。

## 10. リスクと未決の項目

| ID | 項目 | 影響 | 決め方と時期 | 決まらないときの既定 |
|---|---|---|---|---|
| U-1 | 入口で失敗した仕事の誤りの位置は std の中 (`fork_job`) になる | T4 の spec | 実装者 | 受け入れる (`run` の std の位置と同じ前例) |
| U-2 | 仕事の row が `Async` を含むと E2020 に E2002 が並ぶ | `compute_performs_async.em` | 実装者 | スナップショットで受け入れる |
| U-3 | `drop_job` は中断を落とし、起こし直さない | T4 | 実装者 | 落とす (直後に `raise_cancelled` が続く) |
| U-4 | `bench/async/scale.sh` に `compute` の行を足さない | T6 | 実装者 | 足さない |
| U-5 | `compute` 専用の Miri のテストを作らない (`miri::` は std のエントリなしの LIR を流し、新しい unsafe の手順は `par` のものを使い回す) | T4 | 実装者 | 作らない |
| U-6 | `job_cell` は普通の extern | T1 | 実装者 | 普通の extern |
| U-7 | REPL で、前の読み込みで翻訳したコードを通してだけ `fork_job` に届く入力があると、その間に宣言した never だけのエフェクトに fork の表がなく、`Internal("a never-only evidence of a par parent has no fork table")` で止まりうる。`par` も同じ。設計の段の試しでは、`compute` も利用者の `par` の関数も使う入力に写されたので再現しなかった | REPL | 実装者が再現したとき | この段では直さない。再現したら roadmap.md の「その後の項目」に `par` と `compute` の両方の項目として書く |
| R-1 | `tasks`/`parks` の期待値と UI の出力は類推を含む | T4 | 実装者 | 違えば VM を調べ、正しければ数と出力を直す。設計は変えない |
| R-2 | TSan を流せる x86_64 Linux の環境 | T4、T5 | 決定済み: 利用者が CI で確かめることを選んだ | 4 節の「ないとき」に従い、T6 の前に CI の TSan の job を見る |

## 付録: レビューで変わった主な所

蒸し返さないための記録である。

- 仕事の実行時エラーを仕事のセルに記録して `take_job` で報告する案 → 退けた。道が2つになり報告が遅れるので、その場で run を止める (D-7)。
- `Finished.placed` に仕事を含める案 → 退けた。既存の欄の意味を変えず、`placed_jobs` を足す (D-9)。
- 呼んだ worker に積んだ仕事が動かないこと → 制限として受け入れ、status.md に書く (D-2)。
- 論理的な worker ID が尽きる心配 → 空きの一覧で使い回し、上限は 2^32-1、生きた仕事の数は生きたタスクの数以下なので、この段で上限の仕組みは足さない。
- `wake_cell` が `wait` を消すので、`resumed` は `Wait::Job` を見られない → `woke_from_job` の印で決める (D-6)。
- `fork_job` が組を返す案 → 書けないので `job_cell` を足して4行にする (D-3)。
- T4 を登録表と VM に分ける案 → 動かせるテストの道がないので分けず、置き方で T4 と T5 に分けた (D-23)。
- 仕事の evv の `dup` → 持ち手が1つなので要らない。検査モードのテストで確かめる (D-22)。
- `async/` の `RunStats` が置き方に左右される心配 → 比べるのは N = 1 だけで、N = 1 では置かれないので起きない。もし seed の run が食い違えば、まず VM の誤りとして調べ、本当にスケジュールに左右される欄なら testing.md と `schedule_independent` を期待値の変更として直す。
