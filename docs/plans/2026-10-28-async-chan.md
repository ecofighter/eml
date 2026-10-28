# async-chan: 1つの worker の上の channel と `Atomic.Counter`

- 作成: 2026-10-28
- 基点: `main` @ 12c5d40 (「背景」と、すべての `file:line` はこの状態を指す)
- 状態: 計画は確定 (未決の項目は 10 にあり、どれも既定の扱いを決めてある)
- 付録: [2026-10-28-async-chan/](2026-10-28-async-chan/)
  - `files/`: 足す std のモジュール、UI テスト、REPL のトランスクリプト、スナップショット、`crates/eml_vm/tests/chan.rs` の本文。リポジトリと同じ道の名前で置いてあり、そのまま写す。`files/std/Chan.U-2.em` は T-3 で入れる版 (`merge` を除いたもの)、`files/std/Chan.em` は T-6 の後の版である
  - `docs-edits.md`: 直す文書の一覧。作業ごとに入れる文の下書きがある
  - `proto/`: 設計の確認に使った試作の差分 (参考)。`prototype.diff` は実行系の試作で、写さずに 5 で決めたことに従って書く (試作は `Depot` を通さず、`share` と `transfer_back` を直に呼ぶ)。`harness.diff` は `ui.rs` と `eml_test_support` の差分で、rustfmt のほかはそのまま使える。`async-em.diff` は `std/Async.em` の差分、`async-em-line-snapshots.diff` は行の位置が変わる5つのスナップショットの差分である

## 1. 目的と成功の条件

- 目的: 1つの worker の上の `Async` のタスクの間で値を受け渡す `Std.Chan` (容量付きの MPMC) と、共有の数 `Std.Atomic.Counter` を入れる。受け手の決まらない値 (channel の品物とタスクの結果) は、値を預かる構造の論理的な worker ID を途中の持ち主にして移す
- 成功の条件
  - 9 の「全体の確認」のコマンドがすべて期待どおりになる
  - 付録 `files/` の UI テストが、検査モードの逐次、速いモードの `Normal`、seed 1 と 2026 のすべてで、スナップショットと同じ出力を返す
  - `crates/eml_vm/tests/chan.rs` の11のテストが通り、合わせて 1 s 以内に終わる
  - `eml_rt` のソースは変わらない (T-1 で足す単体のテストを除く)

## 2. 範囲

- 範囲: `Std.Chan` (`new`、`send`、`recv`、`try_send`、`try_recv`、`close`、`merge`、3つの結果の型)、`Std.Atomic` (`Counter`、`new`、`add`、`load`、`store`)、`Async` の2つの生の操作 `await_send` と `await_recv`、`Std.Async` の公開の包み `chan_wait_send` と `chan_wait_recv`、登録表の channel のセル、預かり先 `Depot`、タスクの結果に印を付けること、`RunStats` の3つの欄、seed の run の比べ方、ハンドルの型の Kind の規則
- 範囲の外
  - 複数の worker、スレッドをまたぐ channel、ロックフリーのキュー: `async-threads` 段。この段では広げ方を `vm.md` に書くだけにする (D-9)
  - 最後のハンドルを落としたときに channel を放すこと: 「その後の項目」に送る (D-2)
  - 複数の channel の `select`、broadcast と watch の channel、`Counter` のほかの `Atomic`: 作らない (docs/future/async.md の「未決の論点と将来の拡張」のまま)

## 3. 背景

- 登録表 `Registry` は `crates/eml_vm/src/cells.rs:293` にあり、セルの種類はタスクとスコープの2つである。待ちは `Wait::{Cell, Ready, Time}` で、`park`、`park_time`、`park_drain`、`ready` はどれも `suspend` と `parks += 1` を通り、先に取り消しの規則を確かめる (`cells.rs:610-647`)。`unpark` は `Wait::Cell(_)` を受ける (`cells.rs:1183-1212`)。`drain` は受け箱が空になるまで回る (`cells.rs:1216-1234`)
- `cells.rs` は `std::` と `eml_rt::` だけを import する。`crates/eml_vm/tests/tasks.rs:249-266` が確かめ、`crate::` と `super::` を含む行も許さない
- 操作の予算は `BUDGET = 128` で、減らすのは `spend_budget` だけである (`cells.rs:1014-1018`)。呼ぶのは今 `check_cancelled` だけで、`cancel_point` は予算を使わない (`cells.rs:1004-1011`)。`resumed` が予算を 128 に戻す (`cells.rs:576-589`)
- `Op::Async` は `Extern::is_async()` の行で出る。`async_op` (`crates/eml_vm/src/vm.rs:1653` から) が 27 の腕を持ち、汎用の extern の分岐も Async の行をすべて並べて断る (`vm.rs:2702-2729`)。`run_extern` は `Scheduler` を持たない (`vm.rs:2178-2185`)。安全点は `Ret`、`Call`、`TailCall`、`ApplyRest`、`TailSelf` にあり、分岐にはない (`vm.rs:436-502`)
- `Op::Async` の行は、`Async` を読み込めば `Async.run` の外の `main` でも動き、`task.cell` は `None` になる (試作で `Async.cancel_requested ()` を `main` から呼び、両方のモードで `False`、終了コード 0 を確かめた)
- タスクの値の移し方: `Registry::start` が `fork_worker`、`share(root)`、`share(g)` を呼び、`enter` が `transfer_back(閉包, spawner)` を呼ぶ。結果は印を付けずにタスクの ID の持ち物のまま置き、`take` が `transfer_back(w, task_worker)` で受ける (`cells.rs:937`)。`free_task` は受け取られなかった値をタスクの ID の下で落としてから `retire_worker` を呼ぶ。ID を切り替えて落とす関数は `with_worker` (`cells.rs:1302-1314`) と `drop_value` である
- `eml_rt` の `share(w, receiver)` (`crates/eml_rt/src/heap/share.rs:2247`) は `receiver` が今の worker でもよく、そのときは `Unr` の物体に印を付けるだけで所有は動かない。`transfer_back(w, from)` (`heap/transfer.rs:900`) は、検査モードで今の worker でも `from` でもない持ち主の物体に届くと `WrongOwner` を返す。ID を出すのは `fork_worker` (`heap/mod.rs:946`) だけで、`retire_worker` (`mod.rs:983`) は残った物体があれば `TransferLeak` を返す
- `Machine` は誤りと main でない `exit` の後に回復し、`Heap::fresh()` が新しい ID の表で始まる (`crates/eml_vm/src/lib.rs:278-304`、`crates/eml_rt/src/heap/ctx/recover.rs:63-70`、`heap/mod.rs:874-893`)。回復は `Foreign` の物体をすべて断る (`heap/recover.rs:802`)
- `Task a` は `lin` の `Foreign` (`TaskHandle`)、`Cell` と `Scope` は `(generation << 32) | index` の `Int` の即値である。`Cell` と `Scope` の型の行は `type_row(.., Unr, Repr::Int)` である (`crates/eml_extern/src/lib.rs:245-248`)
- 型引数を持つ extern の型 (`Array a`、`Task a`) の Kind は型引数の Kind の結びになる (`crates/eml_types/src/data.rs:62-65`)。extern の引数の型変数を `Unr` に固定する規則は `param_type_vars` (`crates/eml_types/src/check/mod.rs:851-887`) にある
- `generations` は `Start` と `Scheduled` を通って run をまたぐ (`lib.rs:363`、`382`、`508`、`517`、`538`、`547`)。`Scheduled` はどの道でも作る (`lib.rs:541-549`)
- `std/Async.em` の `run` は 59-60 行にあり、5つのスナップショットが `Async.em:60:9` を持つ: `integration__ui__run_fail@async__{await_rethrows_panic,cancelled_run,caught_await_still_fails_run,spawned_failure_fails_run}.em.snap` と `integration__repl__transcripts@async__stale_scope.repl.snap`。`check-fail/names/async_op_handled` は変わらない
- UI の harness は `run/` と `run-fail/` を検査モードの逐次と速いモードの `Normal` で流し、`par/` と `async/` はさらに seed 1 と 2026 で流す。seed の run は少なくとも1回横取りされなければならず、`RunStats` は両方の `cancels` が 0 のときだけ比べる (`crates/eml_cli/tests/ui.rs:261-306`)
- `crates/eml_vm/tests/bench.rs` は 42 のテストで 44 の `RunStats` をインラインのスナップショットで固定している。`bench/` のプログラムは `Async` を使わない
- 文書の extern の数が食い違っている: `stdlib.md:984` と `vm.md:239` は `Std.Async` を27行、`declarations.md:121` は23個としている。この段で正しい数に直す

## 4. 前提

| 要るもの | 用途 | 確かめ方 | ないとき |
|---|---|---|---|
| nix の devShell | すべての T と確認 | `nix develop -c cargo nextest --version` → 版が出る | `nix/prefetch-inputs.sh` を流してやり直す。それでも駄目なら止めて報告する |
| Verus と verusfmt | T-1 (eml_rt のテストを足す) | `nix develop -c cargo verus --version` → 版が出る | 止めて報告する (eml_rt の門を通せない) |
| Miri の shell | T-1 | `nix develop .#miri -c cargo miri --version` → 版が出る | 止めて報告する |
| macOS の機械 | T-7 の計測 | `uname` → `Darwin` | 表の入れ替えだけを別のコミット (`Measure async-chan`) にする (roadmap.md の「進め方」) |

以下のコマンドは devShell の中で流す (`nix develop -c <コマンド>` か、`direnv` で入った shell)。

## 5. 決めたこと

### 5.1 長く効く方針 (変えるときは見直しが要る)

実装がこれと食い違ったら、止めて報告する。

#### D-1: 途中の持ち主は、値を預かる構造の ID にする
- 決め: 受け手の決まらない値は、値を預かる構造の論理的な worker ID に預ける。タスクの結果はそのタスクの ID (`TaskCell.worker`)、channel の品物は channel のセルごとに `fork_worker` で出す ID である。channel の ID は、セルを放すときに残った品物をその ID の下で落としてから `retire_worker` で退かせる。予約した大域の ID も、品物ごとの ID も作らない
- 預かり先の操作は `cells.rs` の `Depot` にまとめる。タスクのセルと channel のセルが同じ6つのメソッドを呼ぶ
  ```rust
  /// 受け手の決まらない値の預かり先。預かる構造 (channel のセル、タスクのセル) の論理的な worker ID を持ち、送り手は
  /// `publish` で値を預け、受け手は `receive` で受け取る (docs/spec/object-model.md の「論理的な worker ID」)。
  pub(crate) struct Depot {
      /// 預かる ID。速いモードでは 0 である
      worker: u32,
  }

  impl Depot {
      /// 新しい預かる ID を出す。`new_chan` が1回呼ぶ。
      pub(crate) fn open<M: Mode>(ctx: &mut Ctx<M>) -> Depot {
          Depot { worker: unsafe { ctx.heap_mut() }.fork_worker() }
      }

      /// ほかで出した ID (タスクのセルの ID) を預かる ID として使う。`close` は呼ぶ側が ID を退かせる所で呼ぶ。
      pub(crate) fn of(worker: u32) -> Depot {
          Depot { worker }
      }

      /// 値 `w` を預ける。送り手の ID のまま、構造に入れる前に呼ぶ。`Unr` の物体に共有の印を付け、`lin` の物体から rc 1 の
      /// 道で届く物体の持ち主を預かる ID にする (docs/spec/object-model.md の「`share`」)。語は変わらない。
      pub(crate) fn publish<M: Mode>(&mut self, ctx: &mut Ctx<M>, w: Word) -> Result<Word, CellFault> {
          unsafe { ctx.heap_mut().share(w, self.worker) }?;
          Ok(w)
      }

      /// 構造から取り出した値 `w` を、今の worker の局所の値として受け取る。取り出した後、ほかに参照がない状態で、受け手の
      /// ID のまま呼ぶ (docs/spec/object-model.md の「`transfer_back`」)。語は変わらない。
      pub(crate) fn receive<M: Mode>(&mut self, ctx: &mut Ctx<M>, w: Word) -> Result<Word, CellFault> {
          unsafe { ctx.heap_mut().transfer_back(w, self.worker) }?;
          Ok(w)
      }

      /// 受け取られない値 `w` を預かる ID の下で落とす。
      pub(crate) fn discard<M: Mode>(&mut self, ctx: &mut Ctx<M>, w: Word, depth_offset: usize) -> Result<(), CellFault> {
          with_worker(ctx, self.worker, depth_offset, |ctx| unsafe { ctx.heap_mut().decref(w) })?;
          Ok(())
      }

      /// 預かる ID を退かせる。値をすべて `discard` した後、セルを放す直前に1回呼ぶ。ID の物体が残っていれば
      /// `TransferLeak` を返す (`par` の join と同じ)。
      pub(crate) fn close<M: Mode>(self, ctx: &mut Ctx<M>) -> Result<(), CellFault> {
          unsafe { ctx.heap_mut() }.retire_worker(self.worker)?;
          Ok(())
      }
  }
  ```
  `discard` の本体は今の `drop_value` の本体で、`drop_value` は消す。`From<HeapError> for CellFault` (`cells.rs:129`) と `From<CellFault> for VmFault` (`crates/eml_vm/src/error.rs:57`) はすでにある
- 呼ぶ所

  | 呼ぶ所 | 呼ぶもの | 今の worker |
  |---|---|---|
  | `finish_ok` の腕 (vm.rs) | `Depot::of(task.worker).publish` を `deposit` の前に | 終わるタスク |
  | `take` (cells.rs:937) | `Depot::of(worker).receive` | 受け取るタスク |
  | `take` の投げる道と `free_task` | `Depot::of(worker).discard`、続けて `free_task` では `close` | どれでも |
  | `new_chan` | `Depot::open` | 作るタスク |
  | `push_raw` (`reserve_send` が 0 を返した後) | `chan.depot.publish`、続けて品物の列に積む | 送り手 |
  | `pop_raw` (`reserve_recv` が 0 を返した後) | 列から取り、`chan.depot.receive` | 受け手 |
  | `free_chan` の残った品物ごと | `chan.depot.discard` | どれでも |
  | `settle` と `free_chan` の終わり | `chan.depot.close` | どれでも |
  | `reserve_*` が 1、2、3 を返したとき、`close_raw`、待っている間に取り消された送り手 | 呼ばない (値は channel に入っていない) | — |

- 規則: `publish` した値は、`receive` か `discard` を呼ぶまで預かる ID の持ち物である。構造が値をタスクの局所の値に渡さずに手放す道は、どれも `discard` を通る。この段では、その道は `free_chan` と `free_task` だけである
- 誤り、`exit`、回復: 何もしない。登録表はヒープに触れずに落とし、回復が新しい ID の表のヒープを作る (3 の背景)。`bad_retire` の panic は起きない (検査モードの預かる ID は `fork_worker` が出した 1 以上の値で、`close` はセルごとに1回。速いモードの `retire_worker` は先に `Ok` を返す)
- 速いモードと検査モード: `publish` は両方のモードで印を付け、`receive` は両方のモードで rc -1 を 1 に裏返す。持ち主の ID、ID ごとの数、`TransferLeak` と `WrongOwner` は検査モードにだけある。rc の遷移は同じなので、reuse の判断は2つのモードで一致する
- 理由: `transfer_back(w, from)` は、multicore.md の「`Lin` の物体の所有者の移し替え」が受け手の歩きに求める動きをすでに持つ。決めるのは `from` がどの ID かだけで、構造の ID ならすでに出し方と退かせ方があり、漏れを構造を放す時点で、その構造の `TransferLeak` として報告できる。`eml_rt` のコード、Verus の証明、信頼の一覧、Miri のテストは変わらない
- 却下
  - 予約した大域の ID 1つ: `eml_rt` に予約の API と「ID を出すのは `fork_worker` だけ」の例外が要る。`u32::MAX` にすると ID ごとの表が 2^32 の項目まで伸びる。漏れは run の終わりに名前のない ID の数として出るだけになる
  - 品物ごとの ID: 表が列の長さとともに伸び、検査モードで送受信のたびに `fork_worker` と `retire_worker` が要る
  - `eml_rt` に `Ctx::entrust` などの入口を足す: 動きの増えない薄い包みに Verus の仕様と節が要る
- 影響: `cells.rs`、`vm.rs`、object-model.md、vm.md
- 見直す条件: worker のスレッドを入れるとき。そのときは ID と数を run の表へ移し (object-model.md:700)、worker の間で共有する構造の ID もその表から出す
- リポジトリに残す所: object-model.md の「`share`」「`transfer_back`」「論理的な worker ID」「報告する誤り」、vm.md の「セルと登録表」「命令」「終わり」(T-1、T-3)。docs/future の「予約した途中の持ち主の ID」は T-7 で直す (付録 `docs-edits.md` の26)

#### D-2: `Chan a` は登録表の添字の `Unr` の即値で、`close` で放す
- 決め: `Chan a` は `Scope` と同じく `(generation << 32) | index` の `Int` の即値 (`Repr::Int`) で、Kind は `a` によらず `Unr` である。channel は、閉じて空になった時点 (`closed && items.is_empty()`) で放す。閉じない channel と、閉じても品物の残った channel は、run の終わりの `sweep_all` で放し、残った品物をそのとき落とす。放した channel と前の run の channel の値は、閉じて空の channel として振る舞う
- 理由 (利用者がアドバイザーの推奨で選んだ。基準はネイティブ化の簡潔さと速さ)
  - ネイティブのコードでは、ハンドルは RC を持たない整数で、`dup`、`decref`、解放の道がない。extern の ABI は `Scope` と同じ形で、コード生成に新しい形が要らない
  - `Foreign` のハンドルにすると、送受信のたびに 4 回ほどの RC の操作と `Foreign` の照らし合わせが入り、channel ごとに1つ確保する
  - `Unr` の `Foreign` は回復で断られる (`recover.rs:802`) ので、REPL で使うには `eml_rt` に信頼する行 (T3)、Verus と Miri の作業が要る。ネイティブのランタイムも同じ信頼する部分を持つことになる
  - 品物が自分の channel のハンドルを持つ輪では、最後のハンドルが落ちない。保証はどちらの形でも「閉じるか run の終わり」になる
- 却下: `Unr` の `Foreign` のハンドルで、最後のハンドルを落とすと放す形 (docs/future/async.md:195 の形)。上の費用のため
- docs/future からの変更として残す: async.md:195 の「channel の最後のハンドルを落とすと、キューに残った品物を記述子で落とす」を、この close の規則に替える
- 後に回した理由 (roadmap.md の「その後の項目」の「非同期」に「最後のハンドルで放す channel」として書く): 「`Chan a` を `Unr` の `Foreign` にして最後のハンドルで放す形は、初めての `Unr` の `Foreign` になる。回復で `Foreign` を移す信頼する行 (T3) と、Verus と Miri の作業を `eml_rt` に要し、送受信のたびにハンドルの `dup`、`decref` と表の照らし合わせが入る。品物が自分の channel のハンドルを持つ輪では最後のハンドルが落ちないので、放す保証はどちらの形でも「閉じるか run の終わり」になる。ネイティブ化の簡潔さと速さを優先し、即値にした」
- 見直す条件: 「接続やリクエストごとに channel を作り、取り消しの巻き戻しで `close` を飛ばす形のサーバが実際に書かれ、長く走る run で登録表の slot が増え続けると測れたとき (`RunStats.channels_swept` か検査モードの終わりの数で)」
- 後で戻す形 (この順で考える)
  1. `Lin` の closer token: `Chan.owned : Int -> <IO> (Chan a, ChanOwner)` のように、`Lin` の `Foreign` の `ChanOwner` を足し、その `Drop` が受け箱に閉じる知らせを積む。`TaskHandle` と同じ仕組みで、`eml_rt` は変わらず、送受信に費用を足さない
  2. `Unr` の `Foreign` のハンドル: 1 で足りないとき。channel の Rust の構造は変わらず、ハンドルの読みと放す引き金だけが変わる
- 影響: stdlib.md、vm.md、modules.md の「REPL のセッション」、status.md の制限
- リポジトリに残す所: stdlib.md の「`Std.Chan`」(T-3)、roadmap.md と async.md (T-7)

#### D-3: channel のセルは登録表に置き、起こすことはただ起こす
- 決め: 登録表のセルの3つ目の種類 `CellKind::Chan(ChanCell)` を足す。新しい `Wait` の種類も、新しいモジュールも作らない
  ```rust
  struct ChanCell {
      cap: u32,
      closed: bool,
      /// 積んだ品物。語は数えた参照で、`depot.publish` で預けたもの。
      items: VecDeque<Word>,
      /// 待っている送り手。項はいつも生きていて、`wait == Some(Wait::Cell(このセル))` である。
      senders: VecDeque<CellRef>,
      /// 待っている受け手。同上。
      receivers: VecDeque<CellRef>,
      /// 品物の預かり先 (D-1)。`new_chan` で `open`、放すときに `close`。
      depot: Depot,
  }
  enum CellKind { Task(TaskCell), Scope(ScopeCell), Chan(ChanCell) }
  ```
  `TaskCell` は変えない。`park`、`park_drain`、`wait` が channel のセルに当たったら `CellFault::Internal("a cell of the wrong kind")` にする
- 不変条件 (cells.rs の冒頭の文書に足す)
  - タスクが `senders` か `receivers` にあるのは、そのタスクが止まっていて `wait == Some(Wait::Cell(その channel))` のときだけで、1回だけ現れる
  - `close_raw` の後、両方の列はいつも空である
  - channel のセルを放すのは `settle` と `sweep_all` だけである
  - W: 品物があるのに受け手が待っている状態は、その受け手を起こす仕事が実行待ちにあるときにだけありうる。送り手も同じ
- 積めるか: `can_send = items.len() < cap + receivers.len()`。待っている受け手1人につき枠が1つ開く (待ち合わせの規則)
- 起こすこと `wake(t)`: `t` は生きていて、止まっていて、`wait == Some(Wait::Cell(この channel))` でなければならない (そうでなければ `Internal("a channel waiter is not waiting")`)。`wait = None` にし、`k` を取り出し、`Resume { wake: WOKE }` を積む。品物や枠は取っておかない
- 起こされたタスクは、取り消しを確かめず、`yields = False` で速い道をやり直す。積めないか空なら、もう一度待つ (待つときに取り消しを確かめる)
- 経路の表

  | 経路 | 変わるもの | 起こすもの |
  |---|---|---|
  | `push_raw` | `items` が1つ増える | 受け手がいれば、先頭を1つ |
  | `pop_raw` | `items` が1つ減る | 積めるなら送り手の先頭を1つ。続けて `settle` |
  | `park_recv` が登録する | `receivers` が1つ増える | 積めるなら送り手の先頭を1つ |
  | `park_send` が登録する | `senders` が1つ増える | なし |
  | 待っている間の取り消し (`unpark`) | その項を列から外す | 自分を `CANCELLED` で |
  | 起こされた後の取り消し | なし (項はもう外れている) | 速い道をやり直す。空なら `park_*` が `CANCELLED` |
  | `close_raw` | `closed`。両方の列を空にする | 全員を `WOKE` で。続けて `settle` |
  | `settle` | `closed && items` が空なら `depot.close`、セルを放す | — |
  | タスクの終わり | なし (待つタスクは終われない) | — |
  | `sweep_all` | 品物を `discard`、`depot.close`、放す。`channels_swept` を1つ数える | — |

- 理由: 起こすときに枠や品物を取っておく形は、取っておいた理由 (待っていた受け手) が取り消しで消えたときに取り消す仕組みが要り、レビューで容量 0 の channel に受け手のいない `Sent` が出る道が見つかった。ただ起こし、起こされたタスクがやり直す形なら、取っておく状態がなく、W が起こしの取りこぼしを防ぐ。代わりに公平さは弱くなる (速い道のタスクが先に取ることがある)。これは仕様に書く
- 却下: 起こすときの予約 (ticket)。起こされた受け手の欄に値を渡す形 (渡した値の持ち主の扱いが要る)。channel の状態を `Foreign` の中の `Arc<Mutex<..>>` に置く形 (語が `sweep_all` と漏れの検査から外れ、multicore.md の「Rust の側が持つ語」に反する)
- 影響: `cells.rs`、vm.md
- 見直す条件: 複数の worker の上で channel を動かすとき (D-9)
- リポジトリに残す所: vm.md の「セルと登録表」と新しい節「channel」(T-3)

#### D-4: 速い道は予約と確定の2つの extern で、値は channel の外で断られる
- 決め: 送りは `reserve_send c yields : Int` の後に `push_raw x c`、受け取りは `reserve_recv c yields : Int` の後に `pop_raw c : a` を呼ぶ。`reserve_*` の値は 0 (確定してよい)、1 (待つ)、2 (閉じている。受け取りでは閉じて空)、3 (先に譲る。`yields` が真のときだけ) である。`Rejected`、`Full`、`Refused` の値と、待っている間に取り消された送り手の値は、channel に入らずに eml の側に残る
- std の約束: `push_raw` と `pop_raw` は、0 を返した `reserve_*` の直後に、`Int` を比べるほかは呼び出しを挟まずに呼ぶ。処理系は確かめない。間に呼び出しが入っても安全は崩れない (ほかのタスクが一時的に1つずれた枠を見るだけで、間に取り消しの地点はない)
- 理由: extern は `TrySendResult a` を作れない。値を対で返すと、受け付けた道で `Lin` の物体を2つの参照が持つ。断った値をタスクごとの欄に置く形は、隠れた状態になりタスクの外で使えない
- 却下: 1つの extern で値を対にして返す形、タスクごとの返しの欄
- 影響: `std/Chan.em`、vm.rs、stdlib.md
- 見直す条件: 複数の worker の上で予約と確定の間が lock の外になるとき (D-9)
- リポジトリに残す所: stdlib.md の「`Std.Chan`」の ABI の部分 (T-3)

#### D-5: 型引数が Kind に効かないハンドルの型
- 決め: `TypeRow` に `pub kind_follows_params: bool` を足す (`type_row` と `generic_type_row` では真)。`handle_type_row("Std.Chan.Chan")` は `TypeRow { params: 1, linearity: Unr, repr: Repr::Int, kind_follows_params: false }` を作る。`data.rs:64` の規則に `&& ty.row().kind_follows_params` を足す。`param_type_vars(signature, arity, held, handle: &dyn Fn(TypeDefId) -> bool)` は、`handle(def)` が真の `TypeRefKind::Con(def, ..)` の中に下りない (下りない版の `visit_outside_handles` を足す)。呼ぶ側は `|def| matches!(program[def].kind, TypeDefKind::Extern(Some(ty)) if ty.row().params > 0 && !ty.row().kind_follows_params)` を渡す
- 理由: `Chan (MutArray Int)` を作れ、同じ channel の値を何度も使える (試作で `eml check` が通ることと、対照の `Array.make 1 (MutArray.make 1 1)` がまだ E3001 になることを確かめた)
- 却下: `Chan a` の引数を `held` か `borrowed` にして固定を逃れる形 (`held` は「預かって1回返す」の意味で、表のテストは `Unr` でない印を求める)
- 影響: `eml_extern`、`eml_types`、declarations.md、types.md
- 見直す条件: 中身の値の Kind に効く型引数を持つハンドルを足すとき
- リポジトリに残す所: declarations.md:126 の下の箇条と types.md:23 (T-2)

#### D-6: `Counter` は `Machine` が持つ表の添字で、放さない
- 決め: `Registry.counters: Vec<i64>` を、`generations` と同じく `Start` で受け取り `Scheduled` で返す (`lib.rs:363`、`382`、`508`、`517`、`538`、`547` の隣に欄 `counters` を足す)。回復でも戻さない。`Counter` は `Unr` の `Int` の即値で、表の添字を持つ。4つの行は `Op::Async` で動く (`run_extern` は登録表を持たない)。そのため `std/Atomic.em` は `import Async` を書く。`new n` は表に積んで添字を返す。`add n i` は `v = counters[i].wrapping_add(n); counters[i] = v; v` を返す。`load` と `store` はそのまま読み書きする。添字が表の外なら `Internal("a counter index is out of range")` にする (検査を通ったプログラムからは届かない)
- 理由: `Data` の物体の生の欄は、共有の物体に `set_raw` を断られる (object-model.md:648)。`Foreign` は回復で断られる。run ごとの登録表の項目は REPL の入力をまたげない
- 却下: `Data` の生の欄、`Foreign` の `AtomicI64`、run ごとの登録表の項目
- 影響: `cells.rs`、`lib.rs`、vm.rs、stdlib.md
- 見直す条件: `Counter` を大量に作るプログラムが出たとき (表は伸び続ける)。worker のスレッドを入れるとき (表を `SeqCst` の atomic にする)
- リポジトリに残す所: stdlib.md の「`Std.Atomic`」、vm.md の「セルと登録表」(T-5)

#### D-7: `RunStats` の3つの欄と、seed の run の比べ方
- 決め: `TaskStats` と `RunStats` に `channel_ops` (積んだ回数と取った回数の和)、`channel_waits` (継続を `park_send`、`park_recv` に渡した回数。すぐ再開する待ちも数え、`parks` にも数える)、`channels_swept` (`sweep_all` が放した channel の数。閉じなかった channel と、閉じても品物の残った channel を数える) を足す。どれも検査モードだけで数える。UI の harness の比べ方を3つにする
  - どちらかの run の `cancels` が 0 でない: 比べない
  - どちらかの run の `channel_ops` か `channel_waits` が 0 でない: `tasks`、`timers`、`dropped_failures`、`channels_swept` だけを比べる (新しい `eml_test_support::channel_independent`)
  - そのほか: 今と同じく `schedule_independent` で比べる
  `ui.rs` では `compares_stats` が `enum Compare { Nothing, ScheduleIndependent, ChannelIndependent }` を返し、`assert_agrees` が `Compare` を受ける (関数ポインタを比べると `unpredictable_function_pointer_comparisons` の警告が出るため、enum にする)。差分は付録 `proto/harness.diff` にある
- 理由: channel を使う run では、待つかどうかと、送り手が参照を持ち続けた `Unr` の品物を受け手が裏返すかが順で変わり、`parks`、`yields`、`shared_objects`、仕事の回数がそれに従う。待った後に閉じた channel を見る受け手は `channel_ops` が 0 のままでも `parks` が変わるので、`channel_waits` も見る
- 却下: `channel_ops` が 0 でなければ何も比べない形 (スケジュールによらない欄まで捨てる)
- 影響: `eml_vm`、`eml_test_support`、`crates/eml_cli/tests/ui.rs`、`bench.rs` のスナップショット、vm.md、testing.md
- 見直す条件: channel を使うプログラムで仕事の回数の食い違いを確かめたくなったとき
- リポジトリに残す所: vm.md の「`RunStats`」(T-3)、testing.md の「seed で流す」(T-4)

#### D-8: channel の待ちは `Std.Async` の公開の包みを通す
- 決め: `Async` に生の操作 `await_send : Cell -> Wake` と `await_recv : Cell -> Wake` を足し、`start_task` の節 `| ctl await_send c k -> park_send c k` と `| ctl await_recv c k -> park_recv c k` で extern に写す。`Async` は抽象エフェクトで、外のモジュールからは perform できない (E1058) ので、`std/Async.em` に `pub chan_wait_send : Cell -> <IO, Async> Wake` と `pub chan_wait_recv` を置き、`std/Chan.em` はそれを呼ぶ。`Cell` を返すか渡す公開の関数はないので、利用者は呼べない。channel でないセルや放したセルを渡すと、すぐ `Woke` で戻る
- 理由: 抽象エフェクトの規則 (modules.md:185) は std のモジュールにも当たる。試作でも `std/Chan.em` からの perform は E1058 になった
- 却下: `await_chan : ChanWait -> Wake` と token の `Foreign` (docs/future/async.md:49 の形。待ちごとに確保が要り、1つの worker では得るものがない)
- 影響: `std/Async.em` (`run` が 65-66 行へ動く)、stdlib.md
- 見直す条件: `Cell` を公開の関数に出すとき
- リポジトリに残す所: stdlib.md の「`Std.Async`」(T-3)

#### D-9: 複数の worker への広げ方
- 決め: この段では次の文を vm.md の「並列化への備え」に書くだけにする。「今は `Registry` を1つの worker が持ち、channel のセルもその中にある。worker のスレッドを入れるときは、登録表を run の lock の下に置き (スコープの一覧を mutex で守る予定と同じ)、送り手は lock の下で積む。待ち手を起こすときは、その項を列から外し、そのタスクのセルの持ち主の worker の受け箱へ `Wake` の知らせを送る。継続 `k` を取り出すのは持ち主である。`reserve_*` と `push_raw`、`pop_raw` の間は lock の外になるので、そのときに枠と品物の予約の数を `ChanCell` に足す。ハンドルは添字の即値のまま変わらない。ロックフリーのキューに替えるときは `ChanCell` の中身だけを替える」
- リポジトリに残す所: vm.md の「並列化への備え」(T-3)

### 5.2 短中期の設計

理由があれば実装の中で変えてよいが、変えたら記録する。

#### D-20: 送りと受け取りの意味
- `send x c`: `Async.cancel_requested ()` が真なら投げ、そうでなければ `send_loop x c True`。`send_loop` は `reserve_send` の値で分ける: 0 なら `push_raw` して `Sent`、2 なら `Rejected x`、3 なら `Async.yield_now ()` の後に `send_loop x c True`、1 なら `Async.chan_wait_send (chan_cell c)` で待ち、`Woke` なら `send_loop x c False`、`Cancelled` なら投げる。`recv` も同じ形で、`Some`、`None` を返す。`try_send` は `reserve_send c False` で、0 なら `push_raw` して `Accepted`、2 なら `Refused x`、ほかは `Full x`。`try_recv` は `Got`、`Drained`、`Empty`。本文は付録 `files/std/Chan.em` にある
- 取り消し: `send` と `recv` は取り消しの地点である。待つ前と待っている間に取り消しが立てば投げる。起こされた後は、`await` と同じく、まず操作をやり直し、次の取り消しの地点が投げる。起こされた受け手が `timeout` の中にいても、`timeout` は `Some x` を返す (`std/Async.em:72-83`、`155-167`)。`try_*` は取り消しの地点でない
- 投げる失敗: `std/Chan.em` の非公開の `raise_cancelled` で、`Async` のものと同じく `fail_io Prelude.Cancelled "task was cancelled"`。文は `chan_cancel_points.em` のスナップショットが固定する
- 予算: `reserve_*` は、タスクの中で呼ばれたときに `spend_budget(me)` を1回呼ぶ。3 を返すのは `yields` が真で、予算がちょうど 0 になったときだけである。`try_*` は予算を使うが譲らない (row に `Async` がない)。docs/future/async.md:63 の「`try_send` と `try_recv` が予算を使い切ると切り替える」からの変更として T-7 で直す
- `new n`: `n` を `0..=u32::MAX` に収める。負の数は 0 になる
- 結果の3つの型は `deriving (Eq, Show)`

#### D-21: `close` と、`Chan` の行がタスクの外で動くこと
- `close_raw`: 放したか前の run の channel、またはもう閉じていれば何もしない。そうでなければ `closed = true`、待っている受け手をすべて起こし、送り手をすべて起こし、`settle` を呼ぶ。同じ channel で受け手と送り手が同時に待つことはない (待っている受け手がいれば積めるので、送り手は待たない) ので、列の間の順は見えない
- 放したセルか前の run の値の `Chan` は、閉じて空の channel として振る舞う: `reserve_send` と `reserve_recv` は 2、`close_raw` は何もしない、`park_*` はすぐ `Woke`
- `park_send` と `park_recv` のほかの行は今のタスクのセルを使わず、タスクの外では予算も使わない

#### D-22: `merge` は終わりを `Chan Unit` で集める
- 決め: `merge srcs` は容量 0 の出力を作る。空の並びなら出力を閉じて返し、タスクを作らない。そうでなければ容量 0 の `done : Chan Unit` を作り、入力ごとに転送のタスクを1つ `spawn` し、`close_after` のタスクを1つ `spawn` する。転送は、入力が閉じて空になるか、出力に `Rejected` が返ると (品物を落として) `send () done` をして終わる。`close_after` は入力の並びをもう一度たどって終わりの知らせを1つずつ受け取り、出力と `done` を閉じる。タスクは呼んだタスクの今のスコープに加わる
- 理由: `Counter` で数えると、`Counter` は放さないので `merge` を呼ぶたびに表が伸びる。`List.length` は `Panic` を持つ。この形なら `done` は閉じて空になり、その場で放す
- 費用: 空でない `merge` は、入力の数より1つ多いタスクを作る
- 本文は付録 `files/std/Chan.em` の `merge`、`start_forwarders`、`forward`、`close_after`

#### D-23: タスクの結果に印を付ける
- `vm.rs` の `Extern::AsyncFinishOk` の腕で、`let value = cells::Depot::of(task.worker).publish(ctx, task.get(arg(0)))?;` を `deposit` の前に置く。`task.worker` はここで今のヒープの worker に等しいので、所有は動かず `Unr` の物体に印が付くだけである。始めと `enter` は変えない (`Lin` の値を捕まえた閉包が `WrongOwner` なしに動くことを試作で確かめた)
- 試作で、この1行を足しても `tasks::` の24のテストと UI テストのすべて (seed の run と、統計を比べる所を含む) がバイトまで同じに通ることを確かめた

#### D-24: テストの置き場所
- seed で流す UI テスト: `tests/ui/run/async/chan_*.em` (11)、`counter.em`、`tests/ui/run-fail/async/chan_deadlock.em`
- タスクの外の UI テスト: `tests/ui/run/std/std_chan.em`、`std_atomic.em`
- REPL のトランスクリプト: `crates/eml_cli/tests/repl/async/chan_stale.repl`、`counter.repl`
- 順と `RunStats` を固定するテスト: `crates/eml_vm/tests/chan.rs` (11)。どれも `Sched::Sequential` で流す。`counter.rs` は作らない (`Counter` の事実はどれも利用者に見える)
- 各テストの中身と期待は付録 `files/` の本文とスナップショットにある。どの UI テストも `start` か `spawn` の後に `spin 40` を置く (seed の run が少なくとも1回横取りされるため)。`chan_close.em` と `chan_cancelled_sender.em` は `close` と取り消しの前に `Async.yield_now ()` を置き、逐次の run で待ちの道を通す

#### D-25: 行の数と、作業ごとの分け方
- 数は2つを名前で分けて書く。「`Async` の命令で動く行」: T-3 の後は `Std.Async` 29 と `Std.Chan` 6 (`chan_cell` を除く) の35、T-5 の後は `Std.Atomic` 4 を足して39。「`Std.Chan` の extern の行」: 7。預かる行は16 (`Std.Async` の15と `Std.Chan.push_raw`)
- 行の表 (T-3 は上の9行、T-5 は下の4行)

  | 行 (enum) | std の宣言 (`pub` でない。Atomic は `pub`) | 引数の Repr / 印 → 結果 | 純粋さ | 預かる | 命令 |
  |---|---|---|---|---|---|
  | `AsyncParkSend` | `Std.Async.park_send : Cell -> (once Wake -> <IO> Unit) -> <IO> Unit` | `[Int, TObj]` / `[Unr, Lin]` → `Unit` | Effectful | `[false, true]` | Async |
  | `AsyncParkRecv` | `Std.Async.park_recv` (同じ型) | 同じ | Effectful | `[false, true]` | Async |
  | `ChanNewRaw` | `Std.Chan.new_raw : Int -> <IO> Chan a` | `[Int]` / `[Unr]` → `Int` / `Unr` | Effectful | – | Async |
  | `ChanToCell` | `Std.Chan.chan_cell : Chan a -> Cell` | `[Int]` / `[Unr]` → `Int` / `Unr` | Pure | – | Extern (即値を写す) |
  | `ChanReserveSend` | `Std.Chan.reserve_send : Chan a -> Bool -> <IO> Int` | `[Int, Enum]` / `[Unr, Unr]` → `Int` | Effectful | – | Async |
  | `ChanPushRaw` | `Std.Chan.push_raw : a -> Chan a -> <IO> Unit` | `[TObj, Int]` / `[Poly, Unr]` → `Unit` | Effectful | `[true, false]` | Async |
  | `ChanReserveRecv` | `Std.Chan.reserve_recv : Chan a -> Bool -> <IO> Int` | `[Int, Enum]` / `[Unr, Unr]` → `Int` | Effectful | – | Async |
  | `ChanPopRaw` | `Std.Chan.pop_raw : Chan a -> <IO> a` | `[Int]` / `[Unr]` → `TObj` / `Poly` | Effectful | – | Async |
  | `ChanCloseRaw` | `Std.Chan.close_raw : Chan a -> <IO> Unit` | `[Int]` / `[Unr]` → `Unit` | Effectful | – | Async |
  | `AtomicNew` | `Std.Atomic.new : Int -> <IO> Counter` | `[Int]` → `Int` | Effectful | – | Async |
  | `AtomicAdd` | `Std.Atomic.add : Int -> Counter -> <IO> Int` | `[Int, Int]` → `Int` | Effectful | – | Async |
  | `AtomicLoad` | `Std.Atomic.load : Counter -> <IO> Int` | `[Int]` → `Int` | Effectful | – | Async |
  | `AtomicStore` | `Std.Atomic.store : Int -> Counter -> <IO> Unit` | `[Int, Int]` → `Unit` | Effectful | – | Async |

  型の行: T-3 で `Chan => handle_type_row("Std.Chan.Chan")`、T-5 で `Counter => type_row("Std.Atomic.Counter", Unr, Repr::Int)`
- `async_op` の既定の腕の文は `"a row outside Std.Async, Std.Chan and Std.Atomic reached the Async instruction"` にする (T-3 では Atomic を書かない)。`ChanToCell` は `run_extern` に `task.set_raw(dst, task.raw(arg(0)))` の腕を持つ。channel と `Counter` の引数は即値で、RC を持たない
- `crates/eml_core_ir/tests/externs.rs`: T-3 で `PROBES` と `probe()` に `probe_chan : Chan.Chan Int -> Chan.Chan Int`、印の一覧に `"Std.Chan.push_raw" => (&[Poly, Unr], Unr)`、`"Std.Chan.pop_raw" => (&[Unr], Poly)`、`"Std.Async.park_send" | "Std.Async.park_recv" => (&[Unr, Lin], Unr)`、預かる行の一覧に `park_send`、`park_recv`、`push_raw` を表の順で足し、テストの名前を `only_the_listed_rows_hold_an_argument` にする。T-5 で `probe_counter : Atomic.Counter -> Atomic.Counter` を足す

## 6. 実装の制約と約束

- `cells.rs` は `std::` と `eml_rt::` だけを import する。`Depot` も `cells.rs` に置く (D-1、D-3)
- コードのコメントと文書は日本語で書き、yomiyasu を当てる。付録の std のコメント、テストの頭のコメント、`docs-edits.md` の文は下書きで、写すときに yomiyasu を当てる。意味、数、条件は変えない (CLAUDE.md)
- コードと `docs/` から `docs/plans/` を引かない。コードに D-n や T-n を書かない
- 規則を入れるコミットで、その `spec/` の節を書く。見出しの名前は `docs-edits.md` の字のまま入れる (`citations::every_cited_heading_exists` が確かめる): stdlib.md の `## \`Std.Chan\`` と `## \`Std.Atomic\``、vm.md の `### channel`
- `push_raw` と `pop_raw` は `reserve_*` の直後に呼ぶ (D-4)。`std/Chan.em` の `send_loop` と `recv_loop` の形を変えない
- 起こされた後のやり直しで取り消しを確かめない (D-3、D-20)
- 待ちの列の項はいつも生きている。`unpark` は channel の待ちを取り消すとき、その項を列から外す (D-3)
- `eml_rt` のソースを変えない (D-1、D-2)。T-1 で足すテストは `src/heap/tests/transfer.rs` に置く
- 通常の run のテストは、単独で 1 s 以内に終わる
- `.em` を試しに流すときは `timeout 20` の下で流す

## 7. 実装の作業

| ID | 作業 | 基づく決め | 先に要るもの | 並べてよいか |
|---|---|---|---|---|
| T-0 | 計画を入れる | — | — | — |
| T-1 | `Depot` とタスクの結果の印 | D-1、D-23 | T-0 | T-2 と並べてよい |
| T-2 | ハンドルの型の Kind と固定の規則 | D-5 | T-0 | T-1 と並べてよい |
| T-3 | channel の核、行、`std/Chan.em`、`Async.em` | D-1〜D-4、D-7、D-8、D-9、D-20、D-21、D-24、D-25 | T-1、T-2 | いいえ |
| T-4 | UI の harness、UI テスト、`chan_stale.repl` | D-7、D-24 | T-3 | いいえ |
| T-5 | `Atomic.Counter` | D-6、D-25 | T-4 | T-6 と並べてよい |
| T-6 | `merge` | D-22 | T-4 | T-5 と並べてよい |
| T-7 | 段を閉じる | D-2、D-9 と全体 | T-5、T-6 | いいえ |

### T-0: 計画を入れる
- 目的: 承認した計画を段の最初のコミットにする (roadmap.md の「進め方」)
- 対象: `docs/plans/2026-10-28-async-chan.md` と付録のディレクトリ
- 作業: 2つをコミットする (`Plan async-chan`)
- 実装に任せること: なし
- 完了の条件: `git show --stat HEAD` → この2つだけが入っている

### T-1: `Depot` とタスクの結果の印
- 目的: 受け手の決まらない値の預かり先を1か所にまとめ、タスクの結果に送り手の印を付ける
- 対象: `crates/eml_vm/src/cells.rs`、`crates/eml_vm/src/vm.rs`、`crates/eml_vm/tests/tasks.rs`、`crates/eml_rt/src/heap/tests/transfer.rs`、object-model.md、vm.md
- 作業
  - D-1 の `Depot` を `cells.rs` に置き、`take` と `free_task` を `Depot::of` で書き直し、`drop_value` を消す。`free_task` が `retire_worker` を直に呼ぶ所は `Depot::of(worker).close(ctx)?` にする
  - D-23 の1行を `finish_ok` の腕に置く
  - 文書: `docs-edits.md` の7 (`Depot` の文)、8 (`finish_ok`)、10 (タスクのセルの言い方)、11 (`shared_objects` のタスクの分)、14 (タスクの分。channel の文はまだ書かない)
  - テスト `crates/eml_vm/tests/tasks.rs` の `a_task_result_is_marked_before_it_is_deposited`: 次の2つのプログラムを `run_stats` (検査モード) で流し、`Int` の結果の `shared_objects` が 0、`String` の結果が 1 であることを確かめ、`run_both` の出力が `"1\n"` と `"a1\n"` であることを確かめる
    ```
    import Async
    main : Unit -> <IO, Panic> Unit
    main () =
      Async.run (fn () ->
        let t = Async.start (fn () -> 1)            -- 2つ目: fn () -> "a" ++ show 1
        println (show (Async.await t)))              -- 2つ目: println (Async.await t)
    ```
  - テスト `crates/eml_rt/src/heap/tests/transfer.rs` の `a_value_shared_to_the_current_id_is_received_by_another_id` (`both_modes!`): `a = heap.fork_worker(); heap.set_worker(a);` で `lin` でない物体 `x` (rc 1) を確保し、`share(x, a)` の後に両方のモードで `rc(x) == -1`、検査モードで `owned_by(a)` が1つ減る。`b = heap.fork_worker(); heap.set_worker(b);` で `transfer_back(x, a) == Ok(())`、`rc(x) == 1`、検査モードで `owned_by(b)` が1つ増え、`check_invariants() == Ok(())`、`retire_worker(a) == Ok(())`。最後に `x` を `decref` し、`retire_worker(b) == Ok(())`。速いモードでは rc と `Ok` だけを確かめる
- 実装に任せること: なし
- 完了の条件
  - `cargo nextest run -p eml_vm --test integration -E 'test(tasks::)'` → 25 passed、0 failed
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::)'` → すべて通り、`git status --porcelain crates/eml_cli/tests/snapshots tests/` が何も出さない
  - `cargo nextest run -p eml_vm --test integration -E 'test(bench::)'` → すべて通り、スナップショットは変わらない
  - `cargo nextest run -p eml_rt` → 新しいテストを含めて通る
  - `cargo verus verify -p eml_rt` → verified。`verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` → 差分なし
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::` → 通る (足したテストは `trusted` に入れないので、数は変わらない)
  - `timeout 20 cargo run -p eml_cli -- run --debug-heap tests/ui/run/async/lin_result.em` → `3 7`、終了コード 0
  - `grep -rn 'drop_value' crates/eml_vm/src` → 何も出ない。`grep -rn 'retire_worker' crates/eml_vm/src` → `Depot::close` と `par` の join だけ
  - `cargo clippy --all-targets -- -D warnings && cargo fmt --check` → 何も出ない
- 落とし穴: `Depot::of` の `close` を `free_task` のほかで呼ぶと、同じ ID を2回退かせて panic する

### T-2: ハンドルの型の Kind と固定の規則
- 目的: `Chan a` を `a` によらず `Unr` にし、extern の引数の型変数の固定がハンドルの中に下りないようにする
- 対象: `crates/eml_extern/src/lib.rs`、`crates/eml_types/src/data.rs`、`crates/eml_types/src/check/mod.rs`、declarations.md、types.md
- 作業
  - D-5 の `kind_follows_params`、`handle_type_row`、`data.rs` の規則、`param_type_vars` の `handle` の引数と `visit_outside_handles` を入れる。試作の差分 (`proto/prototype.diff` の `eml_extern`、`eml_types` の部分) が下敷きになる
  - `Chan` の型の行は、std の宣言 (`std/Chan.em`) がないとテストできないので T-3 で足す。この作業では、`handle_type_row` を使う単体のテストを `eml_types` に足す: `a_handle_parameter_leaves_its_type_variables_free`。`params > 0` で `kind_follows_params` が偽の型の中の型変数を、`param_type_vars` が数えないこと (`|_| false` を渡すと数えること) を確かめる。この形でテストが書けなければ、テストを T-3 へ移し、`Std.Chan.pop_raw : Chan a -> <IO> a` で `param_type_vars(signature, 1, &[false], &handle)` が空、`&|_| false` で長さ 1 を確かめる
  - 文書: `docs-edits.md` の16、17
- 実装に任せること: 単体のテストを T-2 に置くか T-3 に移すか (上の基準で決める)
- 完了の条件
  - `cargo nextest run -p eml_types -p eml_extern -p eml_hir -p eml_core_ir` → 失敗 0
  - `cargo clippy --all-targets -- -D warnings && cargo fmt --check` → 何も出ない

### T-3: channel の核、行、`std/Chan.em`、`Async.em`
- 目的: channel を1つの worker の上で動かす
- 対象: `cells.rs`、`vm.rs`、`crates/eml_vm/src/lib.rs` (`RunStats`)、`crates/eml_vm/src/disasm.rs:366` と `emit.rs` のコメント、`crates/eml_extern/src/lib.rs`、`crates/eml_hir/src/lib.rs` (`STD`)、`std/Async.em`、`std/Chan.em`、`crates/eml_core_ir/tests/externs.rs`、`crates/eml_vm/tests/chan.rs`、`crates/eml_vm/tests/main.rs`、`crates/eml_vm/tests/bench.rs` のスナップショット、5つの行の位置のスナップショット、stdlib.md、vm.md、object-model.md、declarations.md、modules.md、architecture.md
- 作業
  - D-3、D-4、D-20、D-21 の核を `cells.rs` に書く: `ChanCell`、`new_chan`、`chan_mut` (生きた channel のセルでなければ `None`)、`reserve_send`、`push_raw`、`reserve_recv`、`pop_raw`、`close_raw`、`wake`、`park_send`、`park_recv`、`settle`、`free_chan`、`unpark` で channel の列から外すこと、`sweep_all` の `Chan` の腕
  - `park_send(c, me, k)`: `suspend(me)`、`parks += 1`、`channel_waits += 1`。`point(me)` なら `cancels += 1` で `Resume{CANCELLED}`。放したか閉じているか積めるなら `Resume{WOKE}` (1つの worker では届かない守り)。そうでなければ `k` をセルに預け、`wait = Cell(chan)`、`senders.push_back(me)` で `None`。`park_recv` は同じ形で、品物があればすぐ `WOKE`。登録の後、積めるなら送り手の先頭を1つ起こす
  - D-25 の9行、`Chan` の型の行、`is_async()`、vm.rs の腕、汎用の分岐の一覧 (`vm.rs:2702-2729`) を足す
  - D-7 の3つの欄を `TaskStats` と `RunStats` に足す。`bench.rs` の 44 のスナップショットに `channel_ops: 0, channel_waits: 0, channels_swept: 0` が増える (期待値の変更)
  - `std/Async.em` を付録 `proto/async-em.diff` のとおりに直す (`files/std/Async.em` が直した後の全文)。`run` は 65-66 行へ動き、5つのスナップショットが `Async.em:60:9` から `Async.em:66:9` になる (`proto/async-em-line-snapshots.diff`)。`Async.em:62-64` の置き場所の規則はこの動きを許す
  - `std/Chan.em` に付録 `files/std/Chan.U-2.em` を写し、`eml_hir` の `STD` に足す
  - `crates/eml_core_ir/tests/externs.rs` を D-25 のとおりに直す
  - `crates/eml_vm/tests/chan.rs` に付録 `files/crates/eml_vm/tests/chan.rs` を写し、`tests/main.rs` に `mod chan;` を足す。テストの一覧と、数の出どころは下の表のとおり。「定義」の数が合わなければ実装の誤りである。「順」の数は `Sequential` の起こす順に依るので、理由があって変わるなら記録して書き直す (期待値の変更)

    | テスト | 確かめること | 数の出どころ |
    |---|---|---|
    | `send_waits_for_room_and_recv_waits_for_items` | 出力 `6`、`(tasks, parks, channel_waits, channel_ops, channels_swept) == (2, 5, 5, 6, 0)` | 待ちの5は順。操作の6、`parks` と待ちが等しいこと、放した数の0は定義 |
    | `waiting_senders_are_woken_in_order` | `Sent`、`Some "a"`、`Sent`、`Some "b"` | 定義 (側ごとの FIFO) |
    | `waiting_receivers_are_woken_in_order` | `a Some 1`、`b Some 2` | 定義 |
    | `close_wakes_every_waiter` | `None`、`None`、`Rejected "kept"`、`(channel_waits, channel_ops, cancels) == (3, 0, 0)` | 定義 |
    | `a_cancelled_waiting_sender_leaves_the_queue_and_frees_its_value` | `None`、漏れなし、`(2, 0, 1)` | 定義 |
    | `a_woken_receiver_waits_again_when_the_item_was_taken` | `Got 1`、`Some 2`、`(channel_waits, channel_ops) == (2, 4)` | 定義 |
    | `a_sender_woken_for_a_cancelled_receiver_waits_again` | `Empty`、`Some 1`、`Sent`、`(4, 2, 1)` | 順 (main のタスクが譲ったときに受け箱を集めること) |
    | `a_woken_receiver_receives_before_its_cancellation_is_raised` | `Some 1`、`Empty` | 定義 |
    | `a_refused_value_is_not_marked` | `shared_objects` が (1, 1)、`Got "a1"` | 定義 (D-4) |
    | `the_fast_paths_spend_the_budget` | `(parks, channel_waits)`: `try_send` 100回 → (0, 0)、300回 → (1, 0)。`send` 126回 → (0, 0)、127回 → (1, 0)、254回 → (2, 0) | 定義 (最後の `recv` が1つ使い、譲った後のやり直しがもう1つ使う) |
    | `the_channels_left_at_the_end_are_swept` | `done`、漏れなし、`(channel_ops, channels_swept) == (2, 2)` | 定義 |

  - 文書: `docs-edits.md` の1 (`merge` を除く)、3、4、5、6、7 (channel のセル)、8、9、10、11、12、13、14 (channel の分)、15、18 (`Chan.em` と REPL の `Chan`)、20
- 実装に任せること
  - `cells.rs` の中の関数の並びと、補助の関数の名前 (`chan_mut`、`settle`、`free_chan`、`wake` は文書が名前で引くので、この名前にする)
  - `chan.rs` の「順」の数が変わったときの書き直し (理由をコミットのメッセージに書く)
- 完了の条件
  - `cargo nextest run -p eml_vm --test integration -E 'test(chan::)'` → 11 passed、0 failed
  - `cargo nextest run -p eml_vm --test integration -E 'test(tasks::)'` → 25 passed (`the_task_cells_import_only_std_and_the_runtime` を含む)
  - `cargo insta review` で、`bench.rs` の 44 のスナップショットと5つの行の位置のスナップショットだけを受け入れる
  - `cargo nextest run` → 失敗 0
  - `cargo clippy --all-targets -- -D warnings && cargo fmt --check` → 何も出ない
  - `cargo clippy -p eml_cli --no-default-features --features types -- -D warnings` → 何も出ない (`core`、`none` も同じ)
- 落とし穴
  - `chan_cell` は Pure の extern で、`Op::Async` にしない
  - `wait` を確かめずに起こすと、取り消された送り手が列に残る誤りを `a_cancelled_waiting_sender_leaves_the_queue_and_frees_its_value` が `Internal` で見つける
  - generation の照らし合わせを省くと、放した channel の値が同じ slot の新しい channel を指す。`std_chan.em` (T-4) が `Got 9` と `Empty` を出して見つける

### T-4: UI の harness、UI テスト、`chan_stale.repl`
- 目的: 利用者に見える channel の振る舞いを、4つの流し方で固定する
- 対象: `crates/eml_cli/tests/ui.rs`、`crates/eml_test_support/src/lib.rs`、`tests/ui/`、`crates/eml_cli/tests/repl/async/`、`crates/eml_cli/tests/snapshots/`、testing.md
- 作業
  - 付録 `proto/harness.diff` を当てる (D-7 の `Compare`、`channel_independent`)。自己テスト `stats_are_compared_only_without_cancellations` は `stats_are_compared_by_cancellations_and_channels` になり、6つの assert を持つ
  - 付録 `files/tests/ui/run/async/chan_*.em` (`chan_merge.em` を除く10)、`files/tests/ui/run/std/std_chan.em`、`files/tests/ui/run-fail/async/chan_deadlock.em`、`files/crates/eml_cli/tests/repl/async/chan_stale.repl` と、それぞれのスナップショットを写す
  - 文書: `docs-edits.md` の19
- 実装に任せること: なし
- 完了の条件
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::)'` → 失敗 0。写したスナップショットと出力が同じで、`git status --porcelain crates/eml_cli/tests/snapshots` は新しく写したファイルのほかを出さない
  - `cargo nextest run -p eml_cli --test integration -E 'test(repl::)'` → 失敗 0
  - seed の run が `never preempted` で落ちない (上の `ui::` が通ることで確かめられる)
  - `cargo clippy --all-targets -- -D warnings && cargo fmt --check` → 何も出ない
- 落とし穴: `chan_woken_retry.em` の逐次の run は、速い道が先に取る道を通らない。その道は `chan.rs` の `a_woken_receiver_waits_again_when_the_item_was_taken` が固定する

### T-5: `Atomic.Counter`
- 目的: タスクと REPL の入力をまたいで共有する数を入れる
- 対象: `cells.rs`、`crates/eml_vm/src/lib.rs` (`Start`、`Scheduled`、`Machine`)、vm.rs、`crates/eml_extern/src/lib.rs`、`crates/eml_hir/src/lib.rs`、`std/Atomic.em`、`crates/eml_core_ir/tests/externs.rs`、UI テスト、REPL のトランスクリプト、stdlib.md、vm.md、declarations.md、modules.md、architecture.md
- 作業
  - D-6 を入れる。D-25 の4行と `Counter` の型の行、`probe_counter`、`STD` の `Atomic.em`、`is_async()` と vm.rs の腕を足す
  - 付録 `files/std/Atomic.em`、`files/tests/ui/run/async/counter.em`、`files/tests/ui/run/std/std_atomic.em`、`files/crates/eml_cli/tests/repl/async/counter.repl` と、それぞれのスナップショットを写す
  - 文書: `docs-edits.md` の2、7 (`counters`)、8 (`Atomic` の行)、13 (39行)、15 (39個)、18 (`Atomic.em` と `Counter`)、20 (`Counter` の表)
- 実装に任せること: なし
- 完了の条件
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::) | test(repl::)'` → 失敗 0
  - `cargo nextest run -p eml_hir -p eml_core_ir` → 失敗 0
  - `cargo nextest run` → 失敗 0
  - `cargo clippy --all-targets -- -D warnings && cargo fmt --check` → 何も出ない
- 落とし穴: `counter.repl` の1つ目の入力は `import Atomic` だけを書く。`Atomic.em` が `Async` を読み込むので、入口の関数がある

### T-6: `merge`
- 目的: いくつかの channel を1つに集める
- 対象: `std/Chan.em`、`tests/ui/run/async/chan_merge.em` とスナップショット、stdlib.md
- 作業: `std/Chan.em` を付録 `files/std/Chan.em` にする (`merge`、`start_forwarders`、`forward`、`close_after` が増える)。`chan_merge.em` とスナップショットを写す。文書: `docs-edits.md` の1の `merge` の箇条と API の表の行
- 実装に任せること: なし
- 完了の条件
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::)'` → 失敗 0
  - `cargo nextest run -p eml_hir -p eml_core_ir` → 失敗 0
  - `cargo clippy --all-targets -- -D warnings && cargo fmt --check` → 何も出ない

### T-7: 段を閉じる
- 目的: roadmap.md の「進め方」の閉じる手順を行う
- 対象: status.md、benchmarks.md、architecture.md、docs/README.md、overview.md、docs/future/async.md、multicore.md、roadmap.md、CLAUDE.md、`docs/plans/`
- 作業: roadmap.md の「進め方」の1〜9の順に行う。直す文は `docs-edits.md` の21〜33。計測は `bench/scripts.sh`、`bench/run.sh`、`bench/rt.sh` を、コードの最後のコミットのきれいな作業ツリーで流す。docs/future を直すときは、どれも「この段で変えた」とは書かず、今の設計として書く。コミットのメッセージに、docs/future からの変更 (D-1 の途中の持ち主、D-2 の放す時点、D-8 の待ちの操作、D-20 の予算、`completions` の代わりの3つの欄、負の容量)、テストの変更の種類と理由 (8 の後の「テストの変更」)、後に回した項目 (最後のハンドルで放す channel) を決めた日 2026-10-28 を書く
- 実装に任せること: 計測の表の「所見」を更新するか消すか (roadmap.md の「進め方」の2の基準で決める)
- 条件: macOS の機械がなければ、表の入れ替えだけを直後の別のコミット (`Measure async-chan`) にする
- 完了の条件
  - `test -e docs/plans/2026-10-28-async-chan.md` → 終了コード 1
  - `grep -rn 'async-chan' docs CLAUDE.md` → 何も出ない
  - `grep -rn 'docs/plans' crates std docs --include='*.rs' --include='*.em' --include='*.md'` → 何も出ない
  - `cargo nextest run` → 失敗 0 (`citations::` を含む)

## 8. 実行の順

T-0 → (T-1 と T-2 を並べて) → T-3 → T-4 → (T-5 と T-6 を並べて) → T-7。T-5 と T-6 は同じファイルを触らない (`std/Chan.em` は T-6 だけ、`std/Atomic.em` は T-5 だけ) が、`docs/spec/stdlib.md` の隣り合う節を直すので、後から入れる側が先の側に合わせる。

## 9. 確認の計画

- 基点での確認 (最初の変更の前): `cargo nextest run` → 失敗 0
- 全体の確認 (すべての作業の後)
  - `cargo nextest run` → 失敗 0
  - `cargo clippy --all-targets -- -D warnings && cargo fmt --check` → 何も出ない
  - `cargo clippy -p eml_cli --no-default-features --features types -- -D warnings` → 何も出ない (`core`、`none` も同じ)
  - `cargo verus verify -p eml_rt` → verified
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::` → 通る
  - `nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings` → 何も出ない
  - `git diff 12c5d40 -- crates/eml_rt/src ':!crates/eml_rt/src/heap/tests'` → 何も出ない
- 利用者が手で確かめること
  - `eml repl` で `import Chan` と `import Async` を書き、ある入力で作った channel が次の入力で閉じて空の channel として振る舞うこと (`chan_stale.repl` と同じ形)。成功の条件の2つ目を、手で見て確かめる

## 10. 危うさと未決の項目

| ID | 項目 | 影響 | どう決めるか | 決まらないときの既定 |
|---|---|---|---|---|
| Q-1 | `channels_swept` は、品物が残るかが競いに依るプログラムではスケジュールで変わりうる | T-4 で seed の run を比べること | テストを書く人が、残る品物が順によらない形に書く | 比べる。破るプログラムは seed の run で落ちるので、書き直す |
| Q-2 | `std/Chan.em` の `"task was cancelled"` は `Async` の文と二重に持つ | 文がずれる | `chan_cancel_points.em` のスナップショットが固定する | 二重のまま持つ |
| Q-3 | `chan.rs` の「順」の数は `Sequential` の起こす順に依る | T-3 | 実装が理由をもって違えば書き直す | 期待値の変更としてコミットのメッセージに書く |
| Q-4 | 付録の期待値は試作の実行系で確かめたもので、試作は `Depot` を通さない | T-3、T-4 | タスクの結果の印に依る期待値は付録にない (`a_refused_value_is_not_marked` は D-4 の規則だけに依る) | 付録のとおりに写し、食い違えば止めて報告する |
| Q-5 | 閉じない channel は run の終わりまで残る | 長く走るサーバ | D-2 の見直す条件 | status.md の制限と roadmap.md の項目に書く (T-7) |

### テストの変更 (CLAUDE.md の種類)

- 成否の変更 (種類1): なし
- 期待値の変更 (種類2)
  - `crates/eml_vm/tests/bench.rs` の 44 の `RunStats` のスナップショットに、3つの欄 (`channel_ops: 0, channel_waits: 0, channels_swept: 0`) が増える (T-3)。理由: D-7 で欄を足す
  - `Async.em:60:9` を持つ5つのスナップショットが `Async.em:66:9` になる (T-3)。理由: D-8 で `Async` に2つの操作と2つの節を足す
  - `crates/eml_cli/tests/ui.rs` の比べ方を `Compare` に替え、自己テストを `stats_are_compared_by_cancellations_and_channels` にする (T-4)。範囲: channel を使う `async/` の seed の run は、`schedule_independent` の全体でなく4つの欄を比べる。理由: D-7
  - `crates/eml_core_ir/tests/externs.rs` の一覧のテスト (T-3、T-5)。範囲: D-25 の印、預かる行、`PROBES`、テストの名前の変更。理由: 行を足す
- 機械的な追随 (種類3): `crates/eml_vm/tests/main.rs` の `mod chan;`
- 新しいテスト: `chan.rs` の11、UI テストの15、トランスクリプトの2、`tasks.rs` の1、`eml_rt` の1、`eml_types` の1

## 付録: レビューで変わった主な点

- 起こすときに枠や品物を取っておく形 (ticket) → 取り消された受け手のための枠が残り、容量 0 で受け手のいない `Sent` が出る道があった。ただ起こしてやり直す形にした (D-3)
- 起こされた受け手が `timeout` の中で品物を取ると品物が消える、という指摘 → 退けた。`scope` と `timeout` は本体が返した値をそのまま `Some x` で返す (D-20)
- `std/Chan.em` から `Async` の操作を perform する形 → 抽象エフェクトの規則で E1058 になる。`Std.Async` に公開の包みを置いた (D-8)
- seed の run で `channel_ops` が 0 なら比べる形 → 待った後に閉じた channel を見る受け手で `parks` が変わる。3つに分けた比べ方にした (D-7)
- `merge` を `Counter` で数える形 → 呼ぶたびに表が伸びる。`Chan Unit` で終わりを集める形にした (D-22)
- `Chan a` を `Unr` の `Foreign` にして最後のハンドルで放す形 → 利用者がアドバイザーの推奨 (ネイティブ化の簡潔さと速さ) で即値の形を選んだ (D-2)
- `close` が待ち手を起こす道と、待っている送り手が取り消される道が seed の運でしか通らなかった → `chan.rs` の決まった順のテストと、UI テストの `yield_now` で通すようにした (D-24)
