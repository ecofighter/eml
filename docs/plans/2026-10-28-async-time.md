# async-time: 仮想の時計の上のタイマーと期限

- 作成: 2026-10-28
- 基点: `main` @ 0161d97 (「背景」と、すべての `file:line` はこの状態を指す)
- 状態: 計画は確定 (未決の項目は 10 にあり、どれも既定の扱いを決めてある)
- 付録: [2026-10-28-async-time/](2026-10-28-async-time/)
  - `tests/`: 足す UI テストと REPL のトランスクリプトの本文。そのまま写す
  - `proto/s2-prototype.diff`: 設計の確かめに使った試作の差分 (参考)。時計の部分は S1 の設計の代役なので、写さずに 5 の決めたことに従って書く。std と `cells.rs` の E9 以降の部分はそのまま下敷きにしてよい

## 1. 目的と成功の条件

- 目的: 1つの worker の上の `Async` に時間を入れる。`eml run` は本物の単調な時計で待ち、テストは仮想の時計で速く決定的に流れる。`sleep_ms`、`timeout`、`race`、`drain_within` を std に足し、`IoDriver` の時間だけの下書きを `eml_rt` に置く
- 成功の条件
  - 9 の「全体の確かめ」のコマンドがすべて期待どおりになる
  - 足した UI テスト (付録 `tests/`) が検査モードの逐次、速いモードの `Normal`、seed 1 と 2026 のすべてで同じ出力を返す
  - `eml run` で `sleep_ms 50` が 50 ms 以上待ち、眠っている間の Ctrl-C で 130 で終わる (T-5 のテストと 9 の手での確かめ)

## 2. 範囲

- 範囲: 生の操作 `await_time` と `await_drain`、時間の輪、`IoDriver` (時間だけ)、本物の時計と仮想の時計、すべてのタスクが待つときの眠り、`--clock`、`RunStats.timers`、`shield` の境目 (D-10)、`sleep_ms`、`timeout`、`race`、`drain_within`、デッドロックの実行時エラー
- 範囲の外
  - `await_io`、`await_io_until`、`_within` の読み、`eml_io` の crate、I/O の受け付けと完了: `io-driver` 段
  - 複数の worker、ネイティブのコード、時刻を扱う `Time` のモジュール
  - `await_cell_until` (期限付きのセルの待ち): 「その後の項目」に送る (T-10)

## 3. 背景

- 生の操作は今 `await_cell : Cell -> Wake` と `reschedule : Unit -> Wake` の2つで、`start_task` の handler が `park` と `ready` に写す (`std/Async.em:11-15`、`36-40`)。extern の行は 23 で、`Extern::is_async` (`crates/eml_extern/src/lib.rs:1181-1208`) が `Op::Async` に回し、`async_op` (`crates/eml_vm/src/vm.rs:1653`) が実行する。汎用の extern の分岐も Async の行をすべて並べて断る (`vm.rs:2657-2682`)
- セルの核は `crates/eml_vm/src/cells.rs` で、`std` と `eml_rt` だけを import する (`crates/eml_vm/tests/tasks.rs:250-266` が確かめる)。`TaskCell.wait: Option<Wait::{Cell, Ready}>`。取り消しは `mark` (ビットと旗、`Notice::Cancel`) の後に `drain` → `unpark` → `Runnable::Resume { wake: CANCELLED }` で届く (`cells.rs:959-982`)。`unpark` は `point` で門を通し、`shield` の中では外さない (`cells.rs:509-519`)
- 走れるタスクがないと `switch_away` が `VmFault::Internal("no task is ready to run")` を返す (`vm.rs:1223-1226`)。run の終わりに残ったタスクは `Internal("a task is left at the end")` になる
- 眠る仕組みはない。worker は `step` を回し続け、監視のスレッド (`Normal` のときだけ、10 ms ごと) は `PREEMPT` を立てるだけで worker を起こさない (`crates/eml_vm/src/monitor.rs`、`docs/spec/vm.md:383`)。Ctrl-C の handler は atomic と `_exit` しか使わない (`crates/eml_cli/src/interrupt/unix.rs:27-33`)
- `Host` の trait は `verus!` の外にあり、信頼の一覧の行を持たない (`crates/eml_rt/src/host.rs:4-5`、`984`)。`HostRef` と橋の関数が T11 の行を持つ (`crates/eml_rt/src/trust.rs:132-161`)。`trust.rs` の `FILES` に全ソースを載せる (`trust.rs:5-54`)
- `RunConfig { debug_heap, control, sched }` は `#[non_exhaustive]` で、`eml_cli` の `main.rs:82-118` が組み立てて `eml_vm::execute` に渡す。UI の harness、`eml_test_support`、REPL のトランスクリプト (`crates/eml_cli/tests/repl.rs:301`) は `RunConfig::default()` を使う。ライブラリの既定の `Sched` は `Sequential`
- UI の harness は `run/` と `run-fail/` を検査モードの逐次と速いモードの `Normal` で流し、`par/` と `async/` はさらに seed 1 と 2026 で流す。seed の run は少なくとも1回横取りされなければならず、`RunStats` は両方の `cancels` が 0 のときだけ比べる (`crates/eml_cli/tests/ui.rs:261-306`)
- `<std>/Async.em:54:9` (`run` の行) を、次の5つのスナップショットが持つ。`integration__ui__run_fail@async__{await_rethrows_panic,cancelled_run,caught_await_still_fails_run,spawned_failure_fails_run}.em.snap` と `integration__repl__transcripts@async__stale_scope.repl.snap`
- `Prelude` に `Option`、`Either`、`IOErrorKind.TimedOut` がある (`std/Prelude.em:189-212`)
- `bench/` のプログラムは `Async` を使わない。`crates/eml_vm/tests/bench.rs` は 44 の `RunStats` をインラインのスナップショットで固定している

## 4. 前提

| 要るもの | 用途 | 確かめ方 | ないとき |
|---|---|---|---|
| nix の devShell | すべての T と確かめ | `nix develop -c cargo nextest --version` → 版が出る | `nix/prefetch-inputs.sh` を流してやり直す。それでも駄目なら止めて報告 |
| Verus と verusfmt | T-1 | `nix develop -c cargo verus --version` → 版が出る | 止めて報告 (eml_rt の門を通せない) |
| unix の環境 | T-5 の SIGINT のテスト | `uname` → `Linux` か `Darwin` | テストは `#[cfg(unix)]` なので、ほかの OS では流れないだけである |

## 5. 決めたこと

### 5.1 長く効く方針 (変えるときは見直しが要る)

実装がこれと食い違ったら、止めて報告する。

#### D-1: 生の操作 `await_time` と、セルあたり1つの待ちの登録
- 決め: `Async` の3つ目の操作 `await_time : Int -> Wake` を足し、`start_task` の節 `| ctl await_time ms k -> park_time ms k` で extern `park_time` に写す。`Wake` と `resume_task` は変えない。タスクのセルの待ちの登録はいつも高々1つ (`Wait::Cell | Ready | Time`) で、`unpark` はその1つだけを外す。時計を読む操作は置かない
- extern の行 (`crates/eml_extern/src/lib.rs` の `AsyncPark` の直後)
  ```rust
  AsyncParkTime => holding(&[false, true],
      kinded("Std.Async.park_time", &[Int, TObj], &[Unr, Lin], Unit, Unr, Purity::Effectful)),
  ```
  std は `extern park_time : Int -> (once Wake -> <IO> Unit) -> <IO> Unit`
- 理由: 継続はいつも待つタスクのセルが持ち、登録はセルの番号だけを持つ (`docs/future/async.md` の「待ちを外す」)。待ちの種類を1つ足すだけで、取り消しの手順が変わらない。`io-driver` の `await_io_until` も、セルの待ちは I/O の操作の1つで、期限は操作を指す輪の項にする (輪の値の型が2種類の enum になるだけ) ので、この不変条件は保てる
- 却下: `await_cell_until` (セルの待ちと期限の2つの登録) は D-23
- 影響: `cells.rs`、`vm.rs`、`eml_extern`、`std/Async.em`、externs の一覧のテスト
- 見直す条件: 1つのセルに2つの登録が要る操作を足すとき
- リポジトリに残す所: `docs/spec/vm.md` の「`Async` のタスク」と `docs/spec/stdlib.md` の「`Std.Async`」(T-3)

#### D-2: `park_time` の意味
- 決め: `park_time(now_ms, ms, me, k)` を `cells.rs` で次の順に行う
  1. `suspend(me)`、`parks += 1`
  2. `point(me)` なら `cancels += 1`、`Resume { wake: CANCELLED }` (`park` が待つ前に確かめるのと同じ)
  3. `ms <= 0` なら `wait = Some(Wait::Ready)`、`Resume { wake: WOKE }` (`ready` と同じ。輪に載せず、`timers` に数えない)
  4. ほかは `deadline = now_ms.saturating_add(ms as u64)`、`key = TimerKey { deadline, seq: timer_seq }`、`timer_seq += 1`、`timers.insert(key, me)`、`k` を預け、`wait = Some(Wait::Time(key))`、`timers` の数を1つ増やし、`None` を返す
- `now_ms` は登録のときは切り上げ (`ceil_ms`)、満期の判定のときは切り捨て (`floor_ms`) で読む (D-6)。どんな `ms` でも誤りにしない。仮想の時計が `u64::MAX` に届いた後は、どの期限も今と等しくなり、次の空きで満ちる
- VM の腕 (`vm.rs` の `async_op`)
  ```rust
  Extern::AsyncParkTime => {
      let me = current_cell(task)?;
      let now = ceil_ms(sched.driver.now());
      if let Some(run) = sched.cells.park_time(now, task.raw(arg(0)), me, task.get(arg(1)))? {
          schedule(code, sched, entries, run)?;
      }
      task.set(dst, Word::UNIT);
  }
  ```
- 理由: 0 以下を譲りにすると、`sleep_ms 0` が `yield_now` と同じ意味になり、時計を読まずに済む
- 却下: `ms <= 0` を `deadline = now` の輪の項にする案。時計を読む費用がかかり、次に集める地点まで遅れる
- リポジトリに残す所: `vm.md` の新しい節「時間の待ち」(T-3)

#### D-3: 時間の輪は核に置き、集める地点で満期を見る
- 決め: 輪は `cells.rs` の `Registry` に置く。driver にも `vm.rs` にも置かない
  ```rust
  #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
  struct TimerKey { deadline: u64, seq: u64 }   // ミリ秒。seq で同じ期限を登録の順に並べる
  enum Wait { Cell(CellRef), Ready, Time(TimerKey) }
  // Registry
  timers: BTreeMap<TimerKey, CellRef>,         // 語を持たない
  timer_seq: u64,
  pub(crate) fn park_time(&mut self, now_ms: u64, ms: i64, me: CellRef, k: Word) -> Result<Option<Runnable>, CellFault>;
  pub(crate) fn next_deadline(&self) -> Option<u64>;
  pub(crate) fn expire(&mut self, now_ms: u64, out: &mut Vec<Runnable>) -> Result<(), CellFault>;
  ```
  - `expire`: 先頭の `deadline <= now_ms` の間、先頭を取り出す。タスクが `Parked` で `wait == Some(Time(key))` でなければ `Internal("a timer names no waiting task")`。`wait = None`、`k` を取り出して `Resume { cell, k, wake: WOKE }` を積む。順は期限、次に `seq`
  - `unpark` は `Some(Wait::Cell(_)) | Some(Wait::Time(_))` を受け、`Time(key)` なら `timers.remove(&key)` をしてから今までどおり `k` を取り出す。先に `wait` を下ろした側が勝つ
  - `shield` の中の `sleep_ms` は外さない (`unpark` が `point` を通すので、新しいコードは要らない)
  - `cells.rs` 冒頭の不変条件を「`k.is_some()` ⇔ `wait` が `Some(Cell(_))` か `Some(Time(_))`」に直す。`sweep_all` は `timers.clear()` をする (空であることは確かめない。`exit` や誤りで眠るタスクを残して終わりうる)
  - 集める地点 `collect` = `drain` の後、`timers` が空でなければ `expire(floor_ms(driver.now()))`、出た `Runnable` を `schedule` する。`switch_away` (`vm.rs:1222`) と `preempt` (`vm.rs:1337`) の頭の `drain` を `collect` に替える。MAIN の `finish` (`vm.rs:1616`) は `drain` のままにする
- 理由: `unpark` が driver を往復せずに時間の待ちを外せる。本物と仮想の driver が同じ輪を二重に持たずに済む。タイマーのないプログラムは時計を読まない
- 却下: 輪を driver に置く案 (trait に登録と取り消しが要り、持ち主だけの状態が核の外へ出る)。空きのときだけ満期を見る案 (本物の時計で忙しいプログラムのタイマーが遅れ続ける)。`finish` でも満期を見る案 (残った眠りのタスクが機械の速さで `Internal` になる)
- リポジトリに残す所: `vm.md` の「セルと登録表」「時間の待ち」「取り消しの届け方」(T-2、T-3)。`docs/future/async.md` と `docs/future/multicore.md` の「時間の輪は driver が持つ」は核に置く形に直す (T-3)

#### D-4: `IoDriver` は時間だけの trait で、持ち主だけが触る
- 決め: `crates/eml_rt/src/driver.rs` に `verus!` の外の trait を置き、`eml_rt` から再輸出する
  ```rust
  /// worker ごとの時計と待ち (docs/spec/vm.md の「時計と driver」)。持ち主の worker だけが触る。
  pub trait IoDriver: Send {
      /// 作ったときからの単調な経過時間。仮想の時計はミリ秒の整数で進む。
      fn now(&self) -> Duration;
      /// `now() >= deadline` ミリ秒になるまで待つ。早く戻ってよい (呼ぶ側が確かめ直す)。
      /// 本物の時計はスレッドを止め、仮想の時計は `now` を `deadline` に進める。
      fn wait_until(&mut self, deadline_ms: u64);
  }
  ```
  - `external_body` を持たないので、信頼の一覧の行も Miri のテストも要らない (`Host` の trait と同じ)。`trust.rs` の `FILES` に `("driver.rs", include_str!("driver.rs"))` を足し、`docs/spec/object-model.md:1233` の `verus!` の外の一覧に「`IoDriver` の trait」を足す
  - `io-driver` 段が受け付けと完了のメソッドを足し、`wait_until` が完了を返すようにする。上の2つのメソッドの意味は変えない
- 理由: P3 (時間だけの下書き) と、multicore.md の「時間の輪は持ち主の worker だけが触る」に合わせて、`&mut self` で `Sync` を求めない。将来 `Ctx` に入れるときも排他で触る形になる
- 却下: `Arc<dyn IoDriver + Sync>` と `&self` (持ち主だけの規則と、将来の `DriverRef` の排他に合わない)
- 見直す条件: 複数の worker の仮想の時計 (全体の静止) を設計するとき (Q-3)
- リポジトリに残す所: `vm.md` の新しい節「時計と driver」と `object-model.md` (T-1)

#### D-5: すべてのタスクが待つときは、上限付きで眠る
- 決め: `switch_away` を次の形にする
  ```rust
  collect::<R>(code, ctx, task.depth_offset, sched)?;
  let next = loop {
      if let Some(next) = sched.policy.pick(&mut sched.ready) { break next; }
      let deadline = sched.cells.next_deadline().ok_or(VmFault::Internal("no task is ready to run"))?;
      sched.driver.wait_until(deadline);
      poll(ctx, task.stack_depth())?;    // INTERRUPT と MEMORY は run を止める。PREEMPT は取って捨てる
      collect::<R>(code, ctx, task.depth_offset, sched)?;
  };
  ```
  - 空きとは、集めた後で実行待ちの列が空のことである。`Async.run` の呼び手、入れ子の `run` の呼び手、join で待つ `par` の親は列にいないので空きを妨げない。`par` の右の子は列にいるので、子が残る run は空きにならない。`preempt` は変えない
  - 本物の時計の `wait_until` は1回に高々 `IDLE_TICK` (10 ms) だけ眠る。ループが毎回 `poll` するので、Ctrl-C と `MEMORY` はどの `Sched` でも 10 ms 以内に眠る worker を止める。waker も condvar も `unsafe` も使わない
  - 止まったときの誤りは `switch_away` に入った命令の pc で作る (`code.rs:251-267`)。テストは `interrupted()` と終了コード 130 だけを確かめ、関数の名前は確かめない
  - `ok_or` の `Internal` は T-9 で D-11 の `VmFault::Deadlock` に替える
- 理由: シグナルの handler からは condvar も `unpark` も呼べない。監視のスレッドは `Normal` にしかない。上限付きの眠りは、今のブロックする読みが OS の呼び出しの後で止める語を読み直すのと同じ形である
- 却下: handler や監視のスレッドや self-pipe で起こす案 (async-signal-safe でないか、`Normal` だけか、`unsafe` と OS ごとのコードが要る。`io-driver` 段で考える)
- 見直す条件: Ctrl-C の遅れか、空きの間の CPU の使用が問題になったとき (Q-1)
- リポジトリに残す所: `vm.md` の「時計と driver」(T-2)

#### D-6: 仮想の時計の規則、決定性、本物の時計の約束
- 決め
  - 丸め: 登録は `ceil_ms(now)`、満期の判定と `next_deadline` との比較は `floor_ms(now)` で読む。本物の時計の `sleep_ms ms` は `ms` 以上 `ms + 1` ms 未満と起きる遅れだけ待ち、早く起きない。仮想の時計はいつもミリ秒の整数なので、2つの丸めは一致する
    ```rust
    pub(crate) fn floor_ms(d: Duration) -> u64 { d.as_millis() as u64 }
    pub(crate) fn ceil_ms(d: Duration) -> u64 { d.as_millis() as u64 + u64::from(d.subsec_nanos() % 1_000_000 != 0) }
    ```
  - 仮想の時計は driver ごとに 0 から始まる。driver は `Machine` ごとに1つなので、REPL では入力をまたいで時間が残る (輪は run ごとに作り直す)。時間は空きのループの中でだけ、いちばん早い期限まで進み、越えも戻りもしない
  - 決定性の規則 (テストを書く人が守る): 2つの空きの間の忙しい期間では今の時刻は変わらない。そのため、同じ忙しい期間に登録した長さの違うタイマーは、どのスケジュールでも長さの順に満ちる。同じ期限のタイマーは登録の順に満ちるが、並行するタスクの登録の順はスケジュールで決まる。テストの出力に、同じ期限のタイマーの順 (と、タイマーと関係のない出来事の順) を出さない。期限を `yield_now` や `check_cancelled` の空回しで待たない (空きにならず時間が進まない)。seed の harness のための `spin 40` は今までどおり要る (空きの待ちは横取りに数えない)
  - `Clock` と `Sched` は直交する。時計はタイマーがいつ満ちるかを、方針は実行待ちのどれを動かすかを決める
  - 本物の時計の約束: 単調である (`Instant`)。早く満ちない。満ちたタイマーは満期の後の最初の集める地点で動く (空きの眠りの後なら `IDLE_TICK` と OS の遅れの分、ほかのタスクが動いていれば次の切り替えか横取りの頭。`Sequential` では CPU を使い続けるタスクが切り替わるまで動かない)。同じ期限は登録の順である
- 理由: P1 (テストは仮想の時計で、すべてのタスクが待つときだけ進む)。`now` は切り捨てると最大 1 ms 早く満ちるので、登録だけを切り上げる
- 却下: 登録の基点を `now_ms() + 1` にする案 (仮想の時計のすべての眠りが 1 ms ずれる)。`yield_now` の空回しでも仮想の時間を進める案 (P1 に反する)
- リポジトリに残す所: `vm.md` の「時計と driver」(T-2)、`docs/implementation/testing.md` の「seed で流す」(T-7 で同じ期限の規則を足す)、`docs/spec/repl.md` (T-5)

#### D-7: `timeout` は本体を呼んだタスクで動かす
- 決め: `timeout ms f` は私的な子のスコープ `p` を開き、`ms > 0` のときだけ `p` に期限のタスクを1つ始め、呼んだタスクで `scope (fn _ -> f ())` を動かす。期限のタスクは眠り切ると `p` に旗を立て、本体と本体が始めたタスクを取り消す。`ms <= 0` ならタスクを始めず、すぐ旗を立てる。`timeout` は `p` のすべてのタスクが終わってから戻る
- 結果の規則

  | 本体の終わり | 期限が過ぎた | `p` を離れた後に外から取り消されている | 結果 |
  |---|---|---|---|
  | 値 `x` (`Lin` も可) | どれでも | どれでも | `Some x` |
  | IO の `Cancelled` | はい | いいえ | `None` |
  | IO の `Cancelled` | はい | はい | 起こし直す |
  | IO の `Cancelled` | いいえ | どれでも | 起こし直す |
  | ほかの失敗 | どれでも | どれでも | 起こし直す。スコープの失敗にならない |

  - 「期限が過ぎた」は期限のタスクが眠り切って終わったか、`ms <= 0` のことである。`None` は期限が過ぎたかだけで決まり、本体の `Cancelled` が何から来たかは見ない
  - `timeout` 自身は取り消しの地点でない (`scope` と同じ)。期限は本体の取り消しの地点でだけ届く。本体が `await` で待つ外のタスクは、`await` の規則で取り消される
  - 本体の失敗は呼んだタスクの失敗として出るので、`timeout` を囲む `try_all` が捕まえる
- 理由: 捕まえた値を共有にせず、`Lin` の結果も預けずに返せ、タスクが1つ少ない。D-10 の下で、`shield` の中でも期限が効く
- 却下: 本体をタスクにする案 (`timeout = race f (sleep_ms ms)`)。タスクが1つ増え、捕まえた値に印が付く。`ms <= 0` で本体を動かさず `None` を返す案 (本体の効果を飛ばす)
- リポジトリに残す所: `stdlib.md` の新しい節「時間」(T-7)

#### D-8: `race` の勝ちは1つの行 `claim_scope` で決める
- 決め: `race f g` は私的なスコープ `p` に2つの腕のタスクを始め、`shield` の中で両方の終わりを待つ。腕は `scope (fn _ -> f ())` を動かした後に `if claim_scope p then x else raise_cancelled ()` をする。`claim_scope` は呼んだタスクのビットと今のスコープの旗を読み (`shield` は見ない)、どちらも立っていなければ同じ行で `p` に旗を立てて真を返す。立っていれば `cancels += 1` で偽を返す
  ```rust
  AsyncClaimScope => row("Std.Async.claim_scope", &[Int], Enum, Purity::Effectful),
  ```
- 失敗の規則
  - 腕の失敗 (種類 `Cancelled` を除く) は `p` の最初の失敗で、もう一方を取り消し、勝った側があっても `race` から起こし直す (`scope` の規則。失敗を黙って落とさない)。後の失敗は `dropped_failures` に数える
  - 種類 `Cancelled` で終わった腕は勝たず、もう一方も取り消さない。両方がそう終われば `f` の失敗を起こし直す (どちらでもよいが、終わった順によらない側に決める)
  - 呼んだタスクが勝つ前に取り消されれば `Cancelled` を起こし直す。同時に勝ちうる2つのどちらが勝つかはスケジュールで決まる
- 理由: 勝ちを確かめることと旗を立てることを1つの行で行えば、横取りがどこに入っても、将来の複数の worker でも、勝つのは1つである。2つの extern を続けて呼ぶ形は、`Op::Async` が安全点でないことに頼り、複数の worker では成り立たない
- 却下: 勝った後の負けた側の失敗を落とす案 (片付けの失敗を黙って落とす)
- リポジトリに残す所: `stdlib.md` の「時間」と `vm.md` の「命令」(T-8)

#### D-9: `drain_within` は「属するタスクがなくなる」のを待つ
- 決め
  - 生の操作 `await_drain : Scope -> Wake` (extern `park_drain`、2つ目の引数を預かる) は、`s` に属するタスクがなくなるまで待つ。`s` を閉じない
  - 行 `cancel_members : Scope -> <IO> Unit` は `s` に属するタスクごとにビットを立てる。`s` にタスクが残る間は、`s` に加わるタスクを作るときに取り消す印 (`cancel_new`) を立て、`s` が空になったら下ろす。`s` の旗は立てないので、呼んだタスクも `s` の本体も取り消さない
  - 期限のタスクは `s` の外の私的なスコープに置き、期限に `cancel_members s` を呼ぶ。呼んだタスクは `s` が空になって起きるので、`shield` の中でも期限が効く
  - 型は `drain_within : Int -> Scope -> <IO, Async> Unit` のまま (`docs/future/async.md` の「時間」)。生の行で書き、`leave_scope` の `Panic` は起きない `try_panic` で消す (私的なスコープのタスクは期限のタスクだけで、記録される失敗で終わらない)
  - `ms > 0` なら、`s` が閉じているか空でも期限のタスクを始める (時間は過ぎない。数回 park するだけ)
  ```rust
  AsyncParkDrain => holding(&[false, true],
      kinded("Std.Async.park_drain", &[Int, TObj], &[Unr, Lin], Unit, Unr, Purity::Effectful)),
  AsyncCancelMembers => row("Std.Async.cancel_members", &[Int], Unit, Purity::Effectful),
  ```
- 意味の表

  | 場合 | 結果 |
  |---|---|
  | `s` が閉じた、離れた、前の run の、空のスコープ | 時間を待たずに戻る |
  | `ms` より前に `s` が空になる | 戻る。期限のタスクは取り消す |
  | `ms` でタスクが残る | 残ったタスクと、`s` が空になるまでに加わるタスクを取り消し、空になるまで待つ。その後に加わるタスクは普通に動く |
  | `ms <= 0` | すぐ取り消してから待つ |
  | 待つ間に呼んだタスクが取り消される | `s` に触れずに `Cancelled` を投げる |
  | 呼んだタスクが `s` に属する | 自分を待つ。`shield` の外なら期限で自分も取り消される。`shield` の中では戻らず、ほかに進むものがなくなると D-11 のデッドロックで止まる |

- 理由: `close_scope` で待つと、本体が後で `start` したときに `Internal("a task starts in a closed scope")` になる。`cancel_scope s` は `s` の本体まで取り消す。取り消してすぐ戻ると `shield` の中で起きない
- リポジトリに残す所: `stdlib.md` の「時間」、`vm.md` の「命令」「セルと登録表」「取り消しの届け方」(T-9)

#### D-10: `shield` の中で開いたスコープは境目になる
- 決め (利用者が承認した): `shield` の中でタスクが最初に開くスコープは境目である。外からの取り消しは境目で止まり、境目とその内側のスコープ自身の旗 (期限、子の失敗、`cancel_scope`) は `shield` の中でも届く
  - 各スコープは開くときの開き手の `shield` の深さ `shield: u32` と `boundary: bool` を持つ。`base` は、親を同じタスクが開いていれば親の深さ、ほかは 0。`boundary = level > base`
  - 開くとき: 境目は何も写さない。境目でないスコープは親の旗を写し、開き手のビットは深さ 0 のときだけ写す
  - 印付け: `mark(Task t)` は鎖 `[t の run の根 (あれば)] ++ [今のスコープ … home まで)` のうち、最後の境目より外側 (`rposition` で探し、その境目を含めて落とす) だけに旗を立てる。`mark(Scope s)` は入れ子の境目に入らない
  - 取り消しの地点: 深さ 0 なら `ビット || 今のスコープの旗`。深さ `k > 0` なら `今のスコープの旗 && 今のスコープの開き手 == 自分 && 今のスコープの深さ == k`
  - 帰結: いちばん内側の `shield` は外側の旗をすべて隠す。`shield` の中から `shield` の外で開いたスコープに加えたタスクは外の取り消しを受ける (守るのはスコープである)。外のタスクが境目の `Scope` に `start_in` で加えたタスクは守られる。`shield` の中の入れ子の `run` の根は境目である
  - `claim_scope` は `shield` を見ない
- 理由: `shield (fn () -> timeout n cleanup)` が後片付けを動かしつつ期限を効かせ、`shield` の中の `race` が外から取り消されないための、いちばん小さい規則である (Trio の shielded cancel scope と同じ形)
- 却下: 今の規則のまま `race` を後片付けに使えないと書く案。開き手のビットを深さによらず写す案 (`shield` に入る前に立ったビットが、`shield` の中で開いたスコープに漏れる。レビューで再現した)
- 影響: 今の `shield` の規則「ビットも旗も見ない」を変える。今の UI テストと eml_vm のテストの期待値は変わらない (試作で確かめた)
- リポジトリに残す所: `stdlib.md` の「取り消し」、`vm.md` の「命令」「取り消しの届け方」、`std/Async.em` のコメント (T-6)

#### D-11: すべてが待つデッドロックは利用者に見える実行時エラーにする
- 決め: 空きのループで実行待ちもタイマーもないとき `VmFault::Deadlock` を返す。`CodeBase::error` はこれを関数の名前も位置も持たない `RuntimeError::Deadlock` にし、表示はちょうど `deadlock: every task is waiting and no timer is pending` である (CLI は前に `runtime error: ` を付ける)。`stops_the_run` に入れ、`is_broken` は偽のままにする (REPL は回復して続ける)。ほかのタスクが動くか眠る間は、互いを待つタスクも待ち続ける。文書はそれだけを約束する
- 理由: `drain_within` で、`shield` の中の `s` のタスクが自分を待つ形が作れる。静止の検出は互いを待つ形 (と後の channel) も覆う唯一の検出である。関数の名前は内部のコードとスケジュールで変わるので、出さない
- 却下: 自分を待つ形を `Panic` で弾く案 (`<IO>` の handler の中から失敗にできず、自分を待つ形しか捕まえない)
- リポジトリに残す所: `vm.md` の「時計と driver」と `RuntimeError` の一覧 (T-9)

### 5.2 短中期の設計

理由があれば実装の中で変えてよい。変えたらコミットのメッセージに書く。

#### D-20: driver は実行系が持ち、`Ctx` には入れない
- `Machine<M>` が `driver: Option<Box<dyn IoDriver>>` を持ち (`with_clock` で `Clock` から作る)、`run_scheduled` が `Scheduler.driver: Box<dyn IoDriver>` に移し、`Scheduled` が run の後に `Machine` へ返す。`vm.rs` は `sched.driver` を使う。`Scheduler: Send` と `Machine: Send` の assert (`crates/eml_vm/tests/api.rs:30-46`) はそのまま通る
- `io-driver` 段で eml_rt の extern が I/O を持つときに `Ctx` へ `DriverRef` として移す。`docs/future/async.md` の「`Ctx` を通して届く」と `docs/future/multicore.md` の該当の文を「eml_rt の extern が要るまでは実行系が持つ」に直す (T-3)

#### D-21: driver と `Clock` は `crates/eml_vm/src/driver.rs` に置く
```rust
pub struct RealClock { epoch: Instant }
const IDLE_TICK: Duration = Duration::from_millis(10);
// now: self.epoch.elapsed()
// wait_until(d): let now = self.now(); let target = Duration::from_millis(d);
//                if now < target { thread::sleep(min(target - now, IDLE_TICK)) }
pub struct VirtualClock { now_ms: u64 }     // 0 から。now: Duration::from_millis(now_ms)。wait_until(d): now_ms = max(now_ms, d)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Clock { Real, #[default] Virtual } // FromStr は "real" と "virtual"。誤りの文は "expected `real` or `virtual`, found `{text}`"
```
- `RunConfig` に `pub clock: Clock` (既定 `Virtual`) と `with_clock(Clock)` を足す。`Machine::with_clock(Clock)` が box を作り、`AnyMachine::new` が `with_sched` と同じく当てる。`eml_cli` の lib から `Clock` を再輸出する
- `eml run` と `eml repl` に `--clock <real|virtual>` (clap の値 `Clock`、`default_value = "real"`) を足し、`--sched` と同じく `RunConfig` に組む。`split_script_args` は clap の値を取るフラグを読むので変えない (`main.rs:158-169`)
- 時計の選び方: UI の harness、`eml_test_support`、REPL のトランスクリプト、プロセス内のテストは `RunConfig::default()` で仮想の時計になり、直さない。`eml` のバイナリを起こすテスト (`cli.rs`、`terminal.rs`) は本物の時計になるので、タイマーを使うプログラムには `--clock virtual` を渡す。SIGINT のテスト (T-5) だけは本物の時計で長く眠らせる。`seeded_label` は `` eml run --debug-heap --sched seeded:{seed} --clock virtual `` にする

#### D-22: `RunStats.timers` を1つ足す
- `RunStats` と `TaskStats` に `timers: u64` (`await_time` が時間の輪に登録した回数。D-2 の4だけで数える) を足す。ほかのタスクの欄と同じく検査モードだけで写す (`lib.rs:431-443`)。`cancels == 0` ならスケジュールによらないので、`schedule_independent` は0にしない。`vm.md` の欄の数は 18 から 19 になる

#### D-23: `await_cell_until` は入れない
- `timeout` と `drain_within` (`ms > 0`) は期限のタスクを1つ、`race` は腕のタスクを2つ使う。費用が計測で目立ったら、D-1 の形で `await_cell_until : Cell -> Int -> Wake` を足す (セルの待ちと、同じセルの輪の項。`unpark` と `wake` がもう一方を外す)。「その後の項目」に送る (T-10)

#### D-24: `std/Async.em` の置き場所
- `run` より上に置くのは `Async` の操作の宣言と `start_task` の節だけにする。関数は `run` より下 (時間の関数は `shield` の後のまとまり) に、extern は extern のまとまりの最後に置く
- そのため、`<std>/Async.em` の `run` の位置 (今 `54:9`) が動くのは T-3 (`await_time` で3行。コメント、操作、節) と T-9 (`await_drain` で3行) だけで、`57:9`、`60:9` と動く。動く単位は3の5つのスナップショットを期待値の変更 (種類2) として持つ

## 6. 実装の制約と約束

- テストを先に書く。コードのコメントとドキュメントは日本語で、`yomiyasu:yomiyasu` のスキルに従う。コメントは理由を書き、`docs/spec/` の節を引く。引く節は同じコミットで書く (`citations::every_cited_heading_exists` が確かめる)。`docs/plans/` は引かない
- `spec/` の節は規則を実装するコミットで書く。段の名前や「今は〜、後で〜」は書かない
- `cells.rs` は `std` と `eml_rt` だけを import する。新しいスレッド、`unsafe`、輪の中の `Word` は足さない (D-3、D-5)
- `crates/eml_rt` を変えるのは T-1 だけである。T-1 は `crates/eml_rt/CLAUDE.md` の門 (Verus、verusfmt、clippy、fmt、テスト) を通す
- どのテストも単独で 1 秒以内に終わり、機械の速さで結果が変わらない。本物の時計のテストは「以上」と「終わること」だけを確かめ、止まったことを見つけるための上限 (測った時間の 10 倍以上) だけを置く (D-6)
- タイマーのテストは D-6 の決定性の規則を守る。期限を空回しで待たない。同じ期限の順を出力に出さない。`start` と `spawn` の後には `spin 40` を置く
- 単位ごとの共通の確かめ (完了の条件の「共通」はこれを指す)
  - `nix develop -c cargo nextest run` → 失敗 0
  - `nix develop -c cargo clippy --all-targets -- -D warnings` → 終了コード 0
  - `nix develop -c cargo fmt --check` → 終了コード 0
  - `nix develop -c cargo clippy -p eml_cli --no-default-features -- -D warnings`、同じく `--features types`、`--features core` → どれも終了コード 0
- insta は受け入れ済みのスナップショットの `assertion_line` を書き換えることがある。中身の確かめは `git diff -U0 <file> | grep '^[-+] '` で行う

## 7. 実装の作業

| ID | 作業 | 基づく決め | 依存 | 並べられるか |
|---|---|---|---|---|
| T-0 | この計画を入れる | - | - | - |
| T-1 | `IoDriver` を eml_rt に置く | D-4 | T-0 | いいえ |
| T-2 | driver、`Clock`、輪の骨組み、空きのループ | D-3、D-5、D-6、D-20、D-21 | T-1 | いいえ |
| T-3 | `await_time`、`park_time`、仮の `sleep_ms` | D-1、D-2、D-3、D-24 | T-2 | いいえ |
| T-4 | `RunStats.timers` | D-22 | T-3 | いいえ |
| T-5 | CLI の `--clock` と割り込みのテスト | D-5、D-6、D-21 | T-4 | いいえ |
| T-6 | `shield` の境目 | D-10 | T-5 | いいえ |
| T-7 | `sleep_ms` と `timeout` | D-7、D-6 | T-6 | いいえ |
| T-8 | `claim_scope` と `race` | D-8 | T-7 | いいえ |
| T-9 | `drain_within` とデッドロック | D-9、D-11、D-24 | T-8 | いいえ |
| T-10 | 段を閉じる | D-23 ほか | T-9 | いいえ |

`N`、`M`、`R` は T-5 の後の `ls tests/ui/run/async | wc -l`、`ls tests/ui/run-fail/async | wc -l`、`ls crates/eml_cli/tests/repl/async | wc -l` である。T-1 から T-5 は UI のファイルを足さない。

### T-0: 計画を入れる
- 目的: 段の最初のコミット (`docs/future/roadmap.md` の「進め方」)
- 対象: この文書と付録
- 完了の条件: `git show --stat HEAD` → `docs/plans/` の下のファイルだけ

### T-1: `IoDriver` を eml_rt に置く
- 目的: D-4 の trait を、eml_rt の門を通して入れる
- 対象: `crates/eml_rt/src/driver.rs` (新しい)、`crates/eml_rt/src/lib.rs` (`pub use driver::IoDriver`)、`crates/eml_rt/src/trust.rs` (`FILES`)、`docs/spec/object-model.md:1233`、`docs/spec/vm.md` (新しい節「時計と driver」: trait、2つの約束、持ち主だけが触ること、実行系が持つこと)
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件
  - `nix develop -c cargo verus verify -p eml_rt` → 誤り 0
  - `nix develop -c sh -c 'verusfmt --verus-only --check $(git ls-files "crates/eml_rt/src/*.rs")'` → 終了コード 0
  - `nix develop -c cargo nextest run -p eml_rt` → 失敗 0 (`trust::every_source_file_is_listed` を含む)
  - 共通
- 落とし穴: Verus が `verus!` の外の trait を拒んだら、`#[verifier::external]` を付けて T11 の行を足す (Q-4)

### T-2: driver、`Clock`、輪の骨組み、空きのループ
- 目的: 振る舞いを変えずに、時計と空きのループを入れる (輪はいつも空)
- 対象: `crates/eml_vm/src/driver.rs` (新しい、D-21 と D-6 の丸め)、`lib.rs` (`RunConfig.clock`、`with_clock`、`Machine.driver`、`with_clock`、`run_scheduled` と `Scheduled` での受け渡し、`AnyMachine::new`、再輸出)、`sched.rs` (`Scheduler.driver`)、`cells.rs` (`timers`、`timer_seq`、`next_deadline`、`expire`。まだ登録はない)、`vm.rs` (`collect`、`switch_away` の空きのループ)、`crates/eml_cli/src/lib.rs` (`Clock` の再輸出)
- 作業
  - テスト `crates/eml_vm/tests/api.rs`: `RunConfig::default().clock == Clock::Virtual`、`Clock::default() == Clock::Virtual`、`"real"` と `"virtual"` を読む、`""`、`"Real"`、`"wall"`、`"virtual "` を誤りの文ちょうどで拒む、`send::<Scheduler<Normal>>()` と `send::<Machine<Fast>>()` がまだ通る
  - `driver.rs` の単体テスト: `VirtualClock` は `now() == 0`、`wait_until(5)` で `now() == 5 ms`、続く `wait_until(3)` で 5 のまま。`RealClock` は2回読んで減らない。`let d = ceil_ms(now()) + 5; loop { wait_until(d); if floor_ms(now()) >= d { break } }` が終わる。`ceil_ms(Duration::from_micros(900)) == 1`、`floor_ms(Duration::from_micros(900)) == 0`、`ceil_ms(Duration::from_millis(7)) == 7`
  - spec: `vm.md` の「時計と driver」に `Clock`、既定 (ライブラリは仮想、CLI は本物)、`IDLE_TICK`、丸め、D-6 の規則と約束、空きのループ、残す `Internal` を書く。`docs/implementation/architecture.md` の eml_vm のファイルの一覧に `driver.rs` を足す
- 実装者に任せること: `collect` の引数の形 (`drain` に合わせる)
- 条件: いつも
- 完了の条件
  - `nix develop -c cargo nextest run -p eml_vm` → 失敗 0
  - `git diff --stat -- crates/eml_vm/tests/bench.rs` → 出力なし
  - 共通
- 落とし穴: MAIN の `finish` は `drain` のままにする (D-3)

### T-3: `await_time`、`park_time`、仮の `sleep_ms`
- 目的: D-1 から D-3 の核と ABI を入れ、仮の `sleep_ms` で確かめる
- 対象: `cells.rs` (`Wait::Time`、`TimerKey`、`park_time`、`unpark` の腕、`expire` の本体、冒頭の不変条件、`sweep_all`)、`vm.rs` (`AsyncParkTime` の腕、断る腕)、`eml_extern` (行と `is_async`。24 行)、`std/Async.em` (操作、節、extern、仮の `sleep_ms`)、`crates/eml_core_ir/tests/externs.rs` (印の一覧に `AsyncParkTime => (&[Unr, Lin], Unr)`、預かる行の一覧に `Std.Async.park_time` を表の順で、コメントの 11 を 12 に)、`tests/ui/check-fail/names/async_op_handled.em` (`| ctl Async.await_time ms k -> k Async.Woke` を足す)
- 作業
  - `std/Async.em`: 操作は `reschedule` の後にコメント1行と `await_time : Int -> Wake` を置き、節は `start_task` の `reschedule` の節の後に置く (`run` より上はこの3行だけ。D-24)。仮の `sleep_ms` は `run` より下に置く
    ```haskell
    pub sleep_ms : Int -> <IO, Async> Unit
    sleep_ms ms =
      match await_time ms with
        | Woke -> ()
        | Cancelled -> raise_cancelled ()
    ```
  - テスト (`crates/eml_vm/tests/tasks.rs`。検査モード、仮想の時計)
    - (a) `Async.run (fn () -> Async.sleep_ms 100; println "a")` が `a` を出す
    - (b) 一緒に始めた3つのタスクが 50、100、70 ms 眠ってから長さを出し、本体が3つを待つ。`Sequential`、seed 1、seed 2026 のどれでも `50\n70\n100\n`
    - (c) `sleep_ms 0` と `sleep_ms (-1)` が終わって出力する
    - (d) 10000 ms 眠るタスクのハンドルを落とすと、親の行だけを出してすぐ終わる
    - (e) ハンドルをすぐ落としたタスクの中の `shield (fn () -> sleep_ms 10; println "slept")` が `slept` を出し、その後タスクは `Cancelled` で終わる
    - (f) `the_task_cells_import_only_std_and_the_runtime` が通る
    - (g) `sleep_ms 9223372036854775807` を2回、`sleep_ms 1` の後に `println "end"` が `end` を出す
    - (h) `Async.run (fn () -> Async.sleep_ms 60000; println "woke")` が `woke` を出す (仮想の時計なので待たない)
    - `cells.rs` に `#[cfg(test)] mod tests` を足し、輪だけを確かめる。同じ `now` と `ms` で2つのセルを `park_time` すると、`expire` は先に登録したセルを先に返す。1つ目を `unpark` すると、`expire` は2つ目だけを返す。単体テストでセルを作るのが重ければ、1つのタスクが `start t1` (50 ms 眠って "1")、`await t1`、`start t2` (同じく "2")、`await t2` で `1\n2\n` を出すプログラムで替える
  - spec: `vm.md` の「`Async` のタスク」の「セルと登録表」(待ちの種類に時間、`timers` と `timer_seq`)、「命令」(`park_time`、`vm.md:523` の「今の `Task` のセルが要る」一覧)、新しい節「時間の待ち」(D-2、D-3)、「取り消しの届け方」(時間の待ちを外す)。`stdlib.md` の「`Std.Async`」(3つの操作、24 行、`park_time` の行、仮の `sleep_ms` の行)。`docs/spec/core-ir.md:199`。`docs/implementation/testing.md:70` (11 行を 12 行に)。`docs/future/async.md` の「`Async` のエフェクト」(`await_time` はある)、「待ちを外す」と「時間」(輪は核にある)、「`IoDriver` と `eml_io`」(D-20)。`docs/future/multicore.md` の「`Async` のタスク」の時間の輪の文 (D-3、D-20)
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件
  - 共通
  - `nix develop -c cargo nextest run -p eml_hir --test integration externs` と `-p eml_core_ir --test integration externs` → 失敗 0
  - `nix develop -c cargo insta accept && git status --short crates/eml_cli/tests/snapshots` → 中身の変わるファイルは `async_op_handled` と3の5つだけ。5つの `git diff -U0 | grep '^[-+] '` は `at <std>/Async.em:54:9` → `57:9` だけ
  - `nix develop -c cargo nextest run -p eml_vm --test integration tasks::` → 失敗 0 で、(h) が 1 秒以内に終わる (CLI での確かめは `--clock` を足す T-5 で行う)
- テストの変更: 種類2 は `async_op_handled` のスナップショット (節の行が増える) と5つの位置のスナップショット (`run` の行が動く)。理由はコミットのメッセージに書く。ほかは種類3
- 落とし穴: `run` より上に置く行を3行より増やすと、T-9 の期待値もずれる

### T-4: `RunStats.timers`
- 目的: D-22
- 対象: `cells.rs` (`TaskStats.timers`)、`lib.rs` (`RunStats.timers`、写す所)、`crates/eml_vm/tests/tasks.rs`、`crates/eml_vm/tests/bench.rs`、`vm.md` の「`RunStats`」(19 の欄、`timers`、スケジュールによらない欄の一覧)、`testing.md:361`
- 作業: T-3 のテストに数を足す。(a) `(parks, timers) == (1, 1)`、(c) `(parks, timers, cancels) == (2, 0, 0)`、(d) `(timers, cancels) == (1, 1)`、(b) `(tasks, parks, timers)` が3つのスケジュールで等しい
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件
  - 共通
  - `nix develop -c cargo insta accept` の後、`git diff -U0 crates/eml_vm/tests/bench.rs | grep '^[+-] ' | grep -vc 'timers: 0,'` → `0`
  - `git diff -U0 crates/eml_vm/tests/bench.rs | grep -c '^+.*timers: 0,'` → `44`
- テストの変更: 種類2 は `bench.rs` の 44 のスナップショットに `timers: 0` が増えること

### T-5: CLI の `--clock` と割り込みのテスト
- 目的: `eml run` と `eml repl` で時計を選べるようにし、眠る worker を Ctrl-C が止めることを確かめる
- 対象: `crates/eml_cli/src/main.rs` (`--clock`。help は "Which clock timers use: `real` sleeps on the monotonic clock, `virtual` advances only when every task waits")、REPL への受け渡し、`crates/eml_cli/tests/ui.rs` (`seeded_label`)、`docs/spec/stdlib.md` の「起動」、`docs/spec/repl.md` (`--clock` と、仮想の時間が入力をまたいで残ること)、`testing.md` の `seeded_label` を書いた所、`docs/implementation/architecture.md` の CLI の節
- 作業
  - `clock_flag_is_accepted_and_rejected` (`sched` のフラグのテストと同じ形): `--clock real`、`--clock=virtual` を受け、ファイルの後の `--clock` はスクリプトの引数になり、`--clock bogus` と `check --clock real` と `repl --clock bogus` は終了コード 2
  - プロセス内のテスト `crates/eml_vm/tests/api.rs` の `an_interrupt_wakes_a_sleeping_worker`: `RunConfig::default().with_clock(Clock::Real)`、`Sched::Sequential`、100000 ms 眠るプログラム、別のスレッドが 20 ms 後に `control.interrupt()`。`error.interrupted()` が真で、2 秒以内に戻る (止まりの見張りだけ)
  - `#[cfg(unix)]` の `crates/eml_cli/tests/cli.rs` の `an_interrupt_stops_a_sleeping_program_with_130`: `eprintln "sleeping"` の後に `Async.run (fn () -> Async.sleep_ms 100000)`。stderr の行を待って `kill(pid, SIGINT)`、終了コード 130 と stderr の `interrupted`、`DEADLINE` の中で (`cli.rs:361-399` の形)
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件
  - 共通
  - `printf 'import Async\nmain : Unit -> <IO, Panic> Unit\nmain () = Async.run (fn () ->\n  Async.sleep_ms 60000\n  println "woke")\n' > /tmp/sleep.em` の後、`/usr/bin/time -p nix develop -c timeout 20 ./target/debug/eml run --clock virtual /tmp/sleep.em` → stdout はちょうど `woke`、終了コード 0、`real` は 1.00 未満 (`cargo build -p eml_cli` の後)
- テストの変更: 種類3

### T-6: `shield` の境目
- 目的: D-10
- 対象: `cells.rs` (`ScopeCell.shield`、`ScopeCell.boundary`、`new_scope`、`inherit_flag`、`mark`、`point`、冒頭のコメント)、`std/Async.em` の `shield` と `cancel_requested` のコメント、付録 `tests/shield_boundary.em` を `tests/ui/run/async/` へ
- 作業
  - 試作の差分 (`proto/s2-prototype.diff` の `cells.rs`) の `shield`、`boundary`、`inherit_flag`、`mark`、`point` の部分を下敷きにする
  - `std/Async.em` のコメント: `shield` は「中の取り消しの地点は、ビットと、shield の外で開いたスコープの旗を見ない。中で開いたスコープには外の取り消しが届かない」、`cancel_requested` は「取り消しの地点が投げるときだけ真を返す。shield の中では、いちばん内側の shield の中で開いたスコープの旗だけを読む」の趣旨で書く
  - spec の `stdlib.md` の「取り消し」: `shield` の箇条を次の趣旨で書き替える
    - `shield f` の中の取り消しの地点はタスクのビットと、`shield` の外で開いたスコープの旗を見ない。`shield` の中の待ちは外の取り消しで外れない
    - `shield` に入ってからタスクが最初に開くスコープ (`scope`、`timeout`、`race`、`drain_within` の子のスコープ、入れ子の `run` の根) は境目である。境目は開くときに親の旗もタスクのビットも写さず、外の印付けは境目で止まる。境目の内側で開くスコープは親の旗を写すが、タスクのビットは写さない
    - 境目とその内側のスコープの旗は `shield` の中でもその中の取り消しの地点に届く。いちばん内側の `shield` の中で開いたスコープの旗だけを見るので、さらに内側の `shield` の中では届かない
    - 守るのはスコープである。`shield` の中から外のスコープに加えたタスクは外の取り消しを受ける。外のタスクが境目に `start_in` で加えたタスクは受けない
    - `shield` を出た後の最初の取り消しの地点が、外の取り消しで投げる
  - `stdlib.md` のほかの所: `cancel_requested` の箇条 (いちばん内側の `shield` の中で開いたスコープの旗だけを読む)、ビットの伝わり方の箇条 (`shield` の中で開いたスコープを除く)、「`await` を `shield` で囲む」の助言 (相手のスコープを `shield` の外で開いたとき)
  - `vm.md`: 「命令」の `cancel_point` の箇条 (規則は「取り消しの届け方」と同じ)、「取り消しの届け方」の地点の規則、ビットの伝わり方 (境目とその内側を除く。印付けは入れ子の境目に入らない)、順序の箇条 (タスクのビットは深さ 0 で開くスコープだけが写す)、セルの状態の区分 (深さと境目の印は開くときに1回書き、後は読むだけ)
  - `docs/future/roadmap.md` の「その後の項目」の「`shield` の中で取り消しの要求を読む確かめ」の理由を「`cancel_requested` は `shield` の中では外から来た取り消しを読まない」に直す。`testing.md` の `async/` の分類に「`shield` の境目」を足す
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件
  - 共通
  - `ls tests/ui/run/async | wc -l` → `N+1`
  - `nix develop -c cargo insta accept && git status --short crates/eml_cli/tests/snapshots` → 新しいファイルは `integration__ui__run@async__shield_boundary.em.snap` の1つだけ、中身の変わるファイルはなし
  - そのスナップショットの stdout はちょうど `the shielded body runs\nchild: done\nback outside\nafter the shield: Cancelled\ninner flag: Cancelled\nnested scopes in the shield run\nafter the shield: Cancelled\n`
- テストの変更: 種類3 (今の期待値は変わらない)
- 落とし穴: 最後の2行は `shield` に入る前に立ったビットの場合である。開き手のビットを深さによらず写すと `nested scopes in the shield run` が消える

### T-7: `sleep_ms` と `timeout`
- 目的: D-7。仮の `sleep_ms` を最終の形と置き場所にする
- 対象: `std/Async.em` (時間のまとまりを `shield` の後に置き、`sleep_ms` をそこへ移す)、`crates/eml_vm/tests/tasks.rs`、付録 `tests/timeout.em`、`timeout_nested.em`、`timeout_cancelled.em` を `tests/ui/run/async/` へ、`stdlib.md`、`testing.md`
- 作業
  - std (コメントは yomiyasu で整える)
    ```haskell
    -- `ms` ミリ秒待つ取り消しの地点。0 以下なら実行待ちの列の後ろへ回るだけである (docs/spec/stdlib.md の「時間」)
    pub sleep_ms : Int -> <IO, Async> Unit
    sleep_ms ms =
      match await_time ms with
        | Woke -> ()
        | Cancelled -> raise_cancelled ()

    -- 本体は呼んだタスクで動かし、期限は子のスコープに置いたタスクが旗で届ける。本体を `scope` で包むのは、本体が
    -- 始めたタスクにも期限を届け、終わるまで待つためである (docs/spec/stdlib.md の「時間」)
    pub timeout : Int -> (Unit -> <IO, Panic, Async> a) -> <IO, Panic, Async> Option a
    timeout ms f =
      let s = open_scope ()
      let timer = deadline s ms
      let r = try_all (fn () -> scope (fn _ -> f ()))
      -- 残る期限のタスクを取り消す。旗はスコープを離れるまで呼んだタスクにも届くが、この後に取り消しの地点はない
      cancel_scope_raw s
      wait_closed s
      let late = expired timer
      match r with
        | Ok x -> Some x
        -- 外から取り消された呼び手は、期限が過ぎていても `None` で取り消しを飲み込まない
        | Err e -> if late && cancelled e && not (cancel_point ()) then None else reraise e

    -- 過ぎた期限は、タスクを始めずにすぐ旗を立てる
    deadline : Scope -> Int -> <IO, Panic, Async> Option (Task Unit)
    deadline s ms =
      if ms <= 0 then
        cancel_scope_raw s
        None
      else Some (start_in s (fn () ->
        sleep_ms ms
        cancel_scope_raw s))

    -- 期限のタスクが眠り切って終わっていれば、期限は過ぎている
    expired : Option (Task Unit) -> <IO> Bool
    expired timer = match timer with
      | None -> True
      | Some t -> match try_all (fn () -> take_result t) with
        | Ok () -> True
        | Err _ -> False

    cancelled : Failure -> Bool
    cancelled e = match e with
      | IOFailed (IOError { kind = k, message = _ }) -> k == Prelude.Cancelled
      | Panicked _ -> False
    ```
  - `tasks.rs` の数 `(tasks, parks, timers, cancels)` (`Sequential`)。プログラムは `proto/s2-prototype.diff` の `tasks.rs` の `timeout_value`、`timeout_late`、`timeout_early`、`timeout_zero` を使う
    - `timeout 100 (fn () -> 42)` → `Some 42`、`(2, 3, 1, 1)`
    - `timeout 50 (sleep 100; 1)` → `None`、`(2, 4, 2, 1)`
    - `timeout 100 (sleep 50; 1)` → `Some 1`、`(2, 4, 2, 1)`
    - `timeout 0 (sleep 50; 1)` → `None`、`(1, 3, 0, 1)`
    - 数え方: `tasks` は根と始めたタスク。`parks` は `park`、`ready`、`park_time`、`park_drain` ごとに1つで、すぐ戻るものも数える (`await`、`sleep_ms`、`await_drain`、内側を含む `scope` の出口の `wait_closed`)。`timers` は地点で取り消されなかった `ms > 0` の `park_time`。`cancels` は `Cancelled` の再開ごと (地点、`unpark`、`resumed`) と、真を返す `cancel_point` と、偽を返す `claim_scope` ごとに1つ。`timeout 100 (fn () -> 42)` の `timers` だけはスケジュールで変わる (呼び手を先に動かす seed では 0)。数が合わないとき、`tasks` か `parks` の違いは std がこの計画と違うこと、`timers` か `cancels` の違いは T-3 が D-2 と違うことを示す
  - UI の期待の stdout
    - `timeout.em` → `None\nSome "early"\nSome 1\nNone\nfailed: IOFailed IOError { kind = NotFound, message = "no such host" }\narray 7\n`
    - `timeout_nested.em` → `scoped: Cancelled\ntimeout: None\nspawned: Cancelled\ntimeout: Some 2\n`
    - `timeout_cancelled.em` → `cancelled while waiting: Cancelled\ncancelled with a past deadline: Cancelled\nshielded: Some 3 None\nafter the shield: Cancelled\nbit first: Some 1 None\nafter the shield: Cancelled\n`
  - spec の `stdlib.md`: API の表に `sleep_ms` と `timeout` の行。「操作の予算」の後に新しい節「時間」を置き、次の趣旨を書く
    - 時間は今からの長さを `Int` のミリ秒で表す。0 以下は過ぎた期限である。時計は [VM](vm.md) の「時計と driver」にある
    - `sleep_ms ms` は `ms` ミリ秒待つ取り消しの地点である。0 以下なら `yield_now` と同じく列の後ろへ回る
    - `timeout ms f` は D-7 で決めたとおりに動く (子のスコープ、期限のタスク、`ms <= 0`、期限は本体の取り消しの地点でだけ届く、`await` の規則)
    - 結果は D-7 の表のとおりで、`None` は期限が過ぎたかだけで決まる。どの場合も、本体が始めたタスクと期限のタスクが終わってから戻る
    - 本体の失敗は呼んだタスクの失敗として出る。`shield` の中の `timeout` には外の取り消しが届かず、期限は届く (本体の中でさらに `shield` を開けば、その中には届かない)。`ms` が正のとき `RunStats.tasks` に1つ数える
    - 「取り消し」の取り消しの地点の一覧に `sleep_ms` を足す。`testing.md` の「seed で流す」に D-6 の決定性の規則 (同じ期限の順を出力に出さない、空回しで待たない) を足す
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件
  - 共通
  - `ls tests/ui/run/async | wc -l` → `N+4`
  - `git status --short crates/eml_cli/tests/snapshots` → 新しいファイル3つ、中身の変わるファイルなし
- テストの変更: 種類3

### T-8: `claim_scope` と `race`
- 目的: D-8
- 対象: `eml_extern` (`AsyncClaimScope` と `is_async`)、`cells.rs` (`claim_scope`)、`vm.rs` (腕と断る腕)、`std/Async.em`、`tasks.rs`、付録 `tests/race.em` と `race_failure.em`、`stdlib.md`、`vm.md`
- 作業
  - std
    ```haskell
    -- 2つの本体を子のスコープのタスクで競わせ、shield の中で両方の終わりを待つ。本体の失敗はスコープの最初の失敗
    -- として `wait_closed` が起こし直す (docs/spec/stdlib.md の「時間」)
    pub race : (Unit -> <IO, Panic, Async> a) -> (Unit -> <IO, Panic, Async> b) -> <IO, Panic, Async> Either a b
    race f g =
      let s = open_scope ()
      let ta = start_in s (arm s f)
      let tb = start_in s (arm s g)
      wait_closed s
      match try_all (fn () -> take_result ta) with
        | Ok x ->
          drop tb
          Left x
        | Err e -> match try_all (fn () -> take_result tb) with
          | Ok y -> Right y
          | Err _ -> reraise e

    -- 値を返したときにまだ取り消されていなければ、勝って旗を立てる。負けた側の値は巻き戻しが落とす
    arm : Scope -> (Unit -> <IO, Panic, Async> a) -> Unit -> <IO, Panic, Async> a
    arm s f () =
      let x = scope (fn _ -> f ())
      if claim_scope s then x else raise_cancelled ()

    extern claim_scope      : Scope -> <IO> Bool
    ```
  - `tasks.rs` の数 `(tasks, parks, timers, cancels, dropped_failures)`。プログラムは試作の `race` と `race_panic`
    - `race` (30 ms で 1、60 ms で 2) → `Left 1`、`(3, 5, 2, 1, 0)`
    - `race_panic` → `Err (Panicked "a")`、`(3, 5, 2, 1, 1)`
  - UI の期待の stdout
    - `race.em` → `Right 2\nLeft 1\narray 9\nloser: Cancelled\nRight "winner"\nshielded: Left "cleanup"\nbit first: Left "cleanup"\nafter the shield: Cancelled\n`
    - `race_failure.em` → `other: Cancelled\npanic: Panicked "arm failed"\none gives up: Right 2\nboth give up: IOFailed IOError { kind = Cancelled, message = "a" }\ncaller cancelled: IOFailed IOError { kind = Cancelled, message = "task was cancelled" }\n`
  - spec: `stdlib.md` の API の表に `race`、「時間」に D-8 で決めたことと失敗の規則、ABI の表に `claim_scope` の行 (行の数を1つ増やす)。`vm.md` の「命令」に `claim_scope` (ビットか今のスコープの旗が立っていれば偽を返し、取り消しが届いたものとして数える。`shield` は見ない。そうでなければ `cancel_scope` と同じく旗を立てて真を返す) を足し、今の `Task` のセルが要る一覧に加える
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件
  - 共通 (externs の一覧のテストは直さずに通る。`claim_scope` は預かる引数も `Lin` の印も持たない)
  - `ls tests/ui/run/async | wc -l` → `N+6`
  - `git status --short crates/eml_cli/tests/snapshots` → 新しいファイル2つ、中身の変わるファイルなし
- テストの変更: 種類3

### T-9: `drain_within` とデッドロック
- 目的: D-9 と D-11
- 対象: `std/Async.em` (操作、節、extern、`drain_within`、`drained`)、`eml_extern` (2行と `is_async`)、`cells.rs` (`park_drain`、`cancel_members`、`cancel_new`、`start`、`finish`)、`vm.rs` (2つの腕、断る腕、空きのループの `VmFault::Deadlock`、`stops_the_run`)、`code.rs` (`error` の写し)、`error.rs` (`VmFault::Deadlock`、`RuntimeError::Deadlock`)、`crates/eml_core_ir/tests/externs.rs`、`async_op_handled.em`、付録 `tests/drain_within.em`、`drain_within_cancelled.em` を `tests/ui/run/async/` へ、`drain_own_scope.em` を `tests/ui/run-fail/async/` へ、`deadlock.repl` を `crates/eml_cli/tests/repl/async/` へ
- 作業
  - std: 操作は `await_time` の後にコメント1行と `await_drain : Scope -> Wake` を、節は `| ctl await_drain s k -> park_drain s k` を `await_time` の節の後に置く (3行。D-24)
    ```haskell
    -- 期限のタスクは `s` の外の子のスコープに置き、期限に `s` のタスクを取り消す。呼んだタスクは `s` が空になるのを
    -- 待つだけなので、`shield` の中でも期限が効く (docs/spec/stdlib.md の「時間」)
    pub drain_within : Int -> Scope -> <IO, Async> Unit
    drain_within ms s =
      if ms <= 0 then
        cancel_members s
        drained s
      else
        let p = open_scope ()
        -- 今のスコープは `p` なので、期限のタスクは `p` に加わる
        let timer : Task Unit = start_raw (bottom (fn () ->
          sleep_ms ms
          cancel_members s))
        let r = try_io (fn () -> drained s)
        cancel_raw timer
        let c = close_scope p
        shield_enter ()
        let _ = await_cell c
        shield_leave ()
        -- `p` のタスクは期限のタスクだけで、種類 `Cancelled` でしか失敗しないので、離れるときに起こす失敗はない
        let _ = try_panic (fn () -> leave_scope p)
        match r with
          | Ok () -> ()
          | Err e -> fail_io e.kind e.message

    drained : Scope -> <IO, Async> Unit
    drained s =
      match await_drain s with
        | Woke -> ()
        | Cancelled -> raise_cancelled ()

    extern park_drain       : Scope -> (once Wake -> <IO> Unit) -> <IO> Unit
    extern cancel_members   : Scope -> <IO> Unit
    ```
    預かる引数のコメントに「`park_drain` の2つ目の引数」を足す
  - `cells.rs`
    - `park_drain(s, me, k)`: 対象 `scope_cell(s)` を引く。`done` は「生きていない、閉じた、属するタスクがない」。`suspend`、`parks += 1`。`point` なら `cancels += 1`、`CANCELLED`。`done` なら `WOKE`。ほかは `k` を預け、`wait = Cell(target)`、もう `Cell(target)` で待っていない待ち手と `me` の古い項を除いてから `Waiter::Task(me)` を積む
    - `cancel_members(s)`: `s` が生きていて閉じていなければ、`cancel_new = !members.is_empty()` を一覧を読む前に立て、属するタスクごとに `mark(Task m)`
    - `start`: 新しいタスクは `cancel: flag || cancel_new`
    - `finish`: 属するタスクの一覧が空になったら `closed |= closing; cancel_new = false;` の後、待ち手をすべて起こす (閉じかけでないスコープの待ち手も起こす)
    - 試作の差分の該当の部分を下敷きにしてよい
  - デッドロック: 空きのループの `ok_or` を `VmFault::Deadlock` にする。`CodeBase::error` の先頭で `if matches!(fault, VmFault::Deadlock) { return RuntimeError::Deadlock; }`。`RuntimeError::Deadlock` の表示は `deadlock: every task is waiting and no timer is pending`。`stops_the_run` に足す
  - `externs.rs`: 印の一覧に `AsyncParkDrain => (&[Unr, Lin], Unr)`、預かる行の一覧に `"Std.Async.park_drain"` を表の順で足し、コメントの数を1つ増やす。`async_op_handled.em` に `| ctl Async.await_drain s k -> k Async.Woke` を足す
  - `tasks.rs` の数 `(tasks, parks, timers, cancels)`。プログラムは試作の `drain_done`、`drain_deadline`、`drain_closed`
    - `drain_done` → `5`、`(4, 6, 2, 1)`
    - `drain_deadline` → `after`、`(3, 5, 2, 1)`
    - `drain_closed` → `at once`、`(2, 5, 1, 1)` (`timers` はスケジュールで変わる)
  - UI と REPL の期待
    - `drain_within.em` → `a: done\ndrained before the deadline\nb: done\nc: Cancelled\nlate: Cancelled\nafter the deadline\nagain: done\nthe body is not cancelled\na left scope returns at once\nd: Cancelled\na past deadline cancels at once\n`
    - `drain_within_cancelled.em` → `drainer: Cancelled\ndrainer: Cancelled\ndrainer: Cancelled\nkept: done\ndrained after 3 cancelled drains\ne: Cancelled\nthe shielded drain returned\n`
    - `drain_own_scope.em` → stdout は `before\n`、実行時エラーの欄はちょうど `deadlock: every task is waiting and no timer is pending`
    - `deadlock.repl` → `> import Async`、入力、`runtime error: deadlock: every task is waiting and no timer is pending`、`> println "next"`、`next`、`--- close ---`、`no leaks`
  - spec
    - `stdlib.md`: API の表に `drain_within`。ABI は4つの操作 (コードの塊と本文、E1058 と `start_task` の文に `await_drain`)、行を2つ増やす (`park_drain` は2つ目の引数の継続を預かり、`cancel_members` は預からない)、預かる矢印の箇条に `park_drain`。「時間」に D-9 で決めたことと表の趣旨を書く。「取り消し」の地点の一覧に `drain_within`
    - `vm.md`: 「命令」に `park_drain` と `cancel_members`、今のセルが要る一覧に `park_drain`、`start_task` が扱う操作の一覧に `await_drain`。「セルと登録表」(スコープの待ち手は属するタスクがなくなったときに起こす。閉じる印があれば閉じ、加わるタスクを取り消す印を下ろす)。「取り消しの届け方」(`cancel_members` はタスクごとにビットから印付けを始め、スコープの旗と本体への知らせは作らない。`vm.md:547` の O(1) の文を「取り消しで項を探して消さないので、1回の取り消しは待ち手の数によらず O(1) で済む。スコープの待ち手だけは、`park_drain` が登録するときに、もう待っていない項を除く」に替える。セルの状態の区分に「加わるタスクを取り消す印は共有である。`cancel_members` は印を立ててから一覧を読み、`finish` は一覧が空になったのと同じ操作の中で印を下ろす。複数の worker ではスコープの lock の下で行う」)。「時計と driver」の残した `Internal` の文を D-11 に替える。`vm.md:324` の `RuntimeError` の箇条に `Deadlock`、`VmFault` の一覧を 4 から 5 に
    - `core-ir.md` の印の表で `park_drain` を `park` と並べる。`testing.md` の預かる行の数を1つ増やす
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件
  - 共通
  - `ls tests/ui/run/async | wc -l` → `N+8`、`ls tests/ui/run-fail/async | wc -l` → `M+1`、`ls crates/eml_cli/tests/repl/async | wc -l` → `R+1`
  - `nix develop -c timeout 20 ./target/debug/eml run --clock virtual tests/ui/run-fail/async/drain_own_scope.em` → stdout `before`、stderr に `runtime error: deadlock: every task is waiting and no timer is pending`、終了コード 1
  - `git status --short crates/eml_cli/tests/snapshots` → 新しいファイル4つ (`drain_within`、`drain_within_cancelled`、`drain_own_scope`、`async__deadlock.repl`)。中身の変わるファイルは `async_op_handled` と3の5つ。5つの `git diff -U0 | grep '^[-+] '` は `57:9` → `60:9` だけ
- テストの変更: 種類2 は (a) `async_op_handled` のスナップショット (`await_drain` の節)、(b) 5つの位置のスナップショット。ほかは種類3

### T-10: 段を閉じる
- 目的: `docs/future/roadmap.md` の「進め方」の閉じる手順 (1 から 9) を行い、`Close async-time` の1つのコミットにする
- 段に固有の作業 (見出しと文で場所を探す。行の番号は使わない)
  - `docs/future/async.md` の「時間」: 型の塊と、「`timeout ms f` は」「`drain_within ms s` は」で始まる箇条を「`sleep_ms`、`timeout`、`race`、`drain_within` は [標準ライブラリ](../spec/stdlib.md) の「時間」にある。ここには I/O の期限を書く」に替える。期限付きの読みの箇条と例 (a)、(b) は残す。「テストと決定性」から「`timeout` の下の入れ子のスコープが親のない子を残さないこと」と、「テストの driver は仮想の時計を持つ。`sleep_ms` と期限は」で始まる文を消す。サーバの例の「最大 30 秒待ってから取り消す」を「最大 30 秒待ち、残りを取り消して終わるまで待つ」にする
  - `docs/implementation/status.md` の「`Async`」: 「時間 (`sleep_ms`、`timeout`)、」を消し、取り消しの地点に `sleep_ms` と `drain_within` を足す。制限に、`shield` の中で自分のスコープを待つと、ほかに進むものがなくなったときにだけ `deadlock` で止まることと、`race` の勝ちは1つの worker の登録表で決めることを足す
  - `docs/future/roadmap.md`: 段の列から `async-time` を消し、`io-driver` の「前提: `async-time`」を項目の名前に直す。「その後の項目」に「期限のタスクを使わない待ち (`await_cell_until`)」を足し、「後に回した理由: 期限のタスク1つで書ける。」と「見直す条件: その費用が計測で目立つとき。」を入れ子で置く
  - 計測: `crates/eml_vm`、`crates/eml_rt`、`std/` が変わったので、`bench/scripts.sh`、`bench/run.sh`、`bench/rt.sh` を流して `docs/implementation/benchmarks.md` の「今の数値」を入れ替える。macOS がなければ表の入れ替えだけを直後の `Measure async-time` にする
  - `docs/overview.md` の確定した設計判断の表に D-10 (`shield` の境目) と時間 (本物の時計と仮想の時計) を、変わった所だけ足す。`CLAUDE.md` の Commands の `run` の説明に `--clock real|virtual` を足す
  - `docs/plans/` のこの文書と付録を消す。消す前に、5 で決めたことがすべて `docs/` か `std/` のコメントにあることを確かめる
- 完了の条件
  - 共通
  - `ls docs/plans` → この計画の文書も付録もない
  - `grep -rn "async-time" docs CLAUDE.md` → 出力なし

## 8. 実行の順

T-0 → T-1 → T-2 → T-3 → T-4 → T-5 → T-6 → T-7 → T-8 → T-9 → T-10 の一本道である。並べられる作業はない。T-6 (`shield` の境目) は T-1 から T-5 に依らないが、`cells.rs` を同じ順に積むために後に置く。

## 9. 確かめの計画

- 基準 (最初の変更の前): `nix develop -c cargo nextest run` → 失敗 0
- 全体の確かめ (T-9 の後)
  - 6 の共通の確かめ → すべて通る
  - `nix develop -c cargo verus verify -p eml_rt` → 誤り 0
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::` → 失敗 0 (eml_vm の Miri のテストの run が時計を読まないことの確かめ)
  - `nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings` → 終了コード 0 (`driver.rs` の `thread::sleep` が Windows でも通ること)
  - `ls tests/ui/run/async | wc -l` → `N+8`
- 利用者が手で確かめること (macOS か Linux の端末で)
  - `sleep_ms 50` の後に `println "woke"` をするプログラムを `eml run` で流すと、`time` の `real` が 0.05 秒以上で `woke` を出す (成功の条件の3つ目)
  - `Async.sleep_ms 100000` で眠るプログラムを `eml run` で流して Ctrl-C を押すと、すぐに `interrupted` で 130 で終わる

## 10. 危うさと未決の項目

| ID | 項目 | 影響 | いつ、どう決めるか | 決まらないときの既定 |
|---|---|---|---|---|
| Q-1 | `IDLE_TICK` の値 | D-5、Ctrl-C の遅れと空きの CPU | 遅れか CPU が問題になったとき | 10 ms |
| Q-2 | 両方の腕が `Cancelled` で終わったときに起こし直す側 | D-8 | 変える理由が出たとき | `f` の失敗 |
| Q-3 | 複数の worker の仮想の時計 (全体の静止) | D-4、D-6 | worker を足す段の設計 | 1つの worker の規則のまま |
| Q-4 | Verus が `verus!` の外の trait を受けるか | T-1 | T-1 の Verus の確かめ | `#[verifier::external]` と T11 の行を足す |
| Q-5 | 本物の時計の `Sequential` で CPU を使い続けるタスクがいるときのタイマーの遅れ | D-6 | 苦情が出たとき | `vm.md` に書くだけで仕組みは足さない |
| Q-6 | `drain_within` が閉じたか空のスコープでも期限のタスクを始めること | D-9 | 費用が目立ったとき | 始める (2つ目の生の操作か3つ目の `Wake` が要るため) |

## 付録: レビューで変わった主な点

- 期限を `as_millis()` で切り捨てると最大 1 ms 早く満ちる → 登録は切り上げ、満期は切り捨て (D-6)
- 同じ期限のタイマーの順がスケジュールで変わる → テストの出力に出さない規則にした (D-6)
- `IoDriver` を `Arc` と `&self` で共有する形は持ち主だけの規則に合わない → `Box` と `&mut self` (D-4)
- MAIN の `finish` で満期を見ると、残った眠りのタスクが機械の速さで `Internal` になる → `finish` は `drain` のまま (D-3)
- `drain_within` でデッドロックが作れる → 利用者に見える実行時エラーにし、関数の名前を出さない (D-11)
- `shield` の中の `race` と `timeout` が外の取り消しで止まる → `shield` の境目を入れた (D-10。利用者が承認)。開き手のビットが `shield` の中の入れ子のスコープに漏れる誤りをレビューで再現して直した
- `timeout` が外の取り消しと期限の重なりで `None` を返して取り消しを飲み込む → `p` を離れた後に `cancel_point ()` を見る (D-7)
- 勝った後の負けた側の失敗を落とす案 → 採らない (`scope` の規則。D-8)
- `drain_within` の型に `Panic` を足す案 → 採らない。`<IO, Async>` のまま (D-9)
