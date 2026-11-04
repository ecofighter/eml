# task-limits の設計と計画

- 作成: 2026-11-04
- 改訂: 2026-11-04。T-10 の途中で、待ちの中のタスクに届いた旗が利用者の `try_io` に届かない事例が見つかり、設計全体を見直した。改訂した決めと作業には「改訂」と印を付け、T-10 の前に T-9a と T-9b を足した。見直しの記録は末尾の付録にある
- 基準: main @ 837b0fe9。「3. 現状」と T-1〜T-9 の `file:line` はこのコミットを指す。改訂した決めと T-9a、T-9b、T-10 の `file:line` は HEAD 36169ec9 を指す
- 状態: T-1〜T-9 は済み。T-10 の作業ツリーの変更 (印 `-- max-bytes:`、M-1〜M-5 とその snapshot、testing.md と stdlib.md) は残したまま、T-9a から再開する。Q-1 は利用者が既定の扱い (例外を受け入れる) に決めた
- 付録: `docs/plans/2026-11-04-task-limits/eml_rt-prototype.diff` (T-7 の eml_rt の変更の試作。基準のコミットに `git apply` できる)

## 1. 目的と完了の条件

- 目的: `Async` のタスクの中で深さの上限か、メモリの柔らかい上限を超えたとき、そのタスクだけを IO の失敗 `ResourceLimit` で止める。1つの接続が上限に届いてもサーバ全体が止まらないようにする。タスクの外の深さの上限と、メモリの硬い上限は、今と同じく run 全体を止める
- 完了の条件:
  - タスクの中の深さの超過が `ResourceLimit` になり、`Async.try_all` で捕まえられる (UI テスト U-1〜U-8)
  - 確保の多いタスクがメモリの柔らかい上限で `ResourceLimit` になり、run は続く (UI テスト M-1〜M-5、統合テスト P-1〜P-5)
  - タスクの外の上限のテスト (`crates/eml_vm/tests/` の `calls`、`par`、`workers`、`scaling`、`crates/eml_cli/tests/api.rs`) が期待値を変えずに通る
  - 利用者の `try_io` の中で待っているタスクに届いた旗を、再開の後にその `try_io` が捕まえる (統合テスト T-M)。M-1〜M-5 が速いモードの 50 回の繰り返しで通る (T-9a)
  - 「9. 確かめ方」の全体の確かめがすべて通る

## 2. 範囲

- 範囲に入るもの: VM の上の、タスクごとの深さの上限とメモリの柔らかい上限。巻き戻しの表の拡張。`eml_rt` の `Ctx`、`Control`、`Limits`、`Pages` の変更。監視のスレッドの新しい務め。UI テストの印 `-- max-depth:` と `-- max-bytes:`。`spec/` の節と段を閉じる作業
- 範囲に入らないもの:
  - ネイティブのコード。この段の判断がネイティブを妨げないことだけを守る (D-6)。実装は `jit-effects` の段で行う
  - 待ちのタスクが持つメモリを数えること (roadmap の「その後の項目」に既にある)
  - CLI のフラグ (`--max-depth` など)。vm.md の「上限と割り込み」のとおり足さない

## 3. 現状

実装者が知っておくべき今の形を並べる。

### 深さと安全点

- 深さは語の数で、`depth_offset + base + frame_size` である。確かめるのは `Ctx::poll(depth)` (`crates/eml_rt/src/heap/ctx.rs:3593`) だけで、速い道は `depth > self.depth_limit || bits != 0` の2つの比較である
- `depth_limit` は `max_depth - depth_offset` で、書くのは `switch_task` (`ctx.rs:1714`) だけである。`Ctx::wf` (`ctx.rs:1017-1026`) は `depth_limit` に触れない
- `Async` のタスクのエントリの `Task` と `compute` の仕事は、`depth_offset` 0 で `max_depth` をまるごと持つ。`par` の右の子は、分かれたときの親の深さを受け継ぐ (vm.md:697)
- 安全点は `Ret`、`Call`、`TailCall`、`CallN`、`RetN`、`ApplyRest`、`TailSelf`、`Apply`、`Clause` の後と、`par` の終わりにある (`crates/eml_vm/src/vm.rs:482`)。`Call` の安全点では `pc` がもう呼ばれる側の先頭にある (`vm.rs:1016-1026`)。`TailSelf` は引数を `task.base..` に書いて関数の先頭へ飛ぶ (`vm.rs:683-690`、`compile/emit.rs:55-57`)。`par` の安全点の `pc` は左の子の関数の先頭である (`vm.rs:1263-1270`)
- 超えると `Stop::DepthLimit` (`crates/eml_rt/src/control.rs:788`) から `VmFault::Stopped` になり、タスクの中でも run 全体を止める (`vm.rs:1972-1978`)。文は ``the call depth limit is exceeded in `f` ``、終了コードは 1
- `polled_stop` は `PREEMPT` と `NOTICE` を取ってから poll し直す (`vm.rs:460-475`)。yield の途中ではどちらも働かない (`vm.rs:455`)

### 巻き戻しと IO の失敗

- 巻き戻しの表は `eml_lir::unwind::points` と `crates/eml_vm/src/compile/emit.rs:378` が作り、`Tables::unwind_at(pc)` (`code.rs:186`) が戻りの `pc` で引く。行があるのは、中断しうる関数 (`may_abort`) の中の、文の位置の末尾でない call、apply、clause、`par`、`Pure` でない extern だけである
- 巻き戻しは安全点を置かず、poll しない (vm.md:244-291)
- 巻き戻しの呼び手は `Op::YieldAbort` (`vm.rs:933`)、`child_abort` (`vm.rs:1931`)、`exit` (`vm.rs:3320`) の3つである
- IO の失敗は `Extern::FailIo` (`vm.rs:3643`) → `failed` → `abort_to` → `try_io` の `io_failed` の節のエントリ、の順に届く。`abort_to` は元の命令の直後の `pc` と、`ev_find` で見つける IO の evidence を要る。任意の安全点で失敗を起こす仕組みはない
- タスクの底は `bottom f () = match try_all f with Ok x -> finish_ok … | Err (IOFailed …) -> finish_io … | Err (Panicked …) -> finish_panic …` (`std/Async.em:66-72`) である。どのタスクの本体も、`Async.run` に渡した本体も、この `try_all` の下で動く
- `Async.shield` は `shield_enter`、`try_all f`、`shield_leave`、`reraise` の順に動く (`std/Async.em:158-167`)
- `IOErrorKind.ResourceLimit` は tag 9 で既にある (`std/Prelude.em:215`、`crates/eml_core_ir/tests/externs.rs:830`)。Rust から起こす所はまだない。`IO_CANCELLED` は `crates/eml_lir/src/lib.rs:1029` にある。`IO_FAILED = 0` (`vm.rs:3167`)

### メモリと `Control`

- `Control` (`crates/eml_rt/src/control.rs`、`verus!` の中) は worker ごとの止める語 (`AtomicU32`) を持つ。ビットは `INTERRUPT=1`、`MEMORY=2`、`END=4`、`TERMINATE=8`、`NOTICE=1<<30`、`PREEMPT=1<<31`。原子的な操作はすべて `Relaxed`
- `Limits { max_bytes, max_depth, max_evv, workers }` (`control.rs:24-33`) は一度作ったら変えない。`eml_cli::default_limits()` (`crates/eml_cli/src/lib.rs:230`) が `max_depth` 2^27、`max_bytes` 物理メモリの 80% にする
- 確保量を数えるのは `fresh_page` (`crates/eml_rt/src/alloc.rs:2039`、検証済みの関数) だけで、`charge_pages` (`alloc.rs:2107`、T1 の `external_body`) から `Control::charge` (`control.rs:612-620`) を呼ぶ。`bytes` は run 全体で1つの `AtomicUsize` で、`max_bytes` を超えると `raise(MEMORY)` する。数はページのバイト数で、小さい物体のページは run の間 OS に返さない。大きい物体のページだけが `refund` で減る
- `TaskCell` (`crates/eml_vm/src/cells.rs:247-287`) はバイト数も上限の旗も持たない。登録表は1つの `Mutex` の後ろにあり、いちばん外側の lock である。`Async` のタスクと VM の `Task` は1対1でない (起きるたびに新しいエントリの `Task` を作る)
- `Scheduler.current` (`sched.rs:546`) を書くのは `set_current` (`sched.rs:1205`) だけで、それを呼ぶのは `resume` (`vm.rs:1623`) だけである
- 監視のスレッド (`crates/eml_vm/src/monitor.rs`、`eml-monitor`) は壁時計の 10 ms ごとに動き、シグナルの中継、worker を起こすこと、横取り (`Sched::Normal` だけ)、救済の判断をする。読むのは原子的な値と `Mail`、`Run`、`Control` だけで、登録表の lock も `Ctx` もヒープも触らない。mail に積んで全 worker に `NOTICE` を立てる形は `Relay::relay` (`monitor.rs:132-143`) にある
- 決定的なスケジュールの方針は `Sequential`、`Seeded`、`RescueAlways` である。`eml run` と REPL は `Normal` だけを使う (`sched.rs:209`)。`Rescue::Always` は、監視のスレッドの救済の務めを worker の上で行う (`sched.rs:194-203`)

### テスト

- UI テストの harness は `Limits { workers, ..default_limits() }` で動かし (`crates/eml_cli/tests/ui.rs:188-195`)、上限を変える印はない。先頭の行の印は `-- signal:`、`-- workers: skip`、`-- windows: skip` がある
- UI テストの checked の実行は、1 worker の `Sequential` と `Seeded`、2 と 8 worker の `Sequential` と `RescueAlways` である。速いモードの実行は 1 worker の `Normal` である。`RunStats` を比べるのは 1 worker の `Sequential` と `Seeded` の間だけで、複数 worker の `async/` は出力だけを比べる (`ui.rs:262-290, 337, 400-418`)
- 小さい上限は Rust のテストでだけ使っている (`crates/eml_vm/tests/workers.rs` の約 419 行目の `max_depth: 4096`、`par.rs`、`calls.rs`、`crates/eml_cli/tests/api.rs` の `max_bytes: 1<<22`)。上限に届く UI テストはない

## 4. 前提の環境

| 要るもの | 用途 | 確かめ方 | ないとき |
|---|---|---|---|
| nix の devShell (`direnv` か `nix develop`) | すべて | `cargo --version` → `cargo 1.99.1` | 止めて利用者に聞く |
| Verus と verusfmt | T-3、T-7 | `cargo verus --version` と `verusfmt --version` が終了コード 0 | 止めて利用者に聞く (aarch64-linux の devShell には入っていない) |
| `.#miri` の shell | T-7 | `nix develop .#miri -c cargo miri --version` → 終了コード 0 | 止めて利用者に聞く |
| x86_64 Linux | T-8 の ThreadSanitizer | `uname -sm` → `Linux x86_64` | macOS ならこの1つを飛ばし、CI の TSan の job に任せると報告に書く |
| macOS | T-9 の `bench/run.sh`、段を閉じる計測 | `uname -s` → `Darwin` | 計測を `Measure task-limits` の別のコミットに回す (roadmap の「進め方」) |

## 5. 決めたこと

### 5.1 長期の方針 (変えるときは見直しが要る)

実装がこれらとぶつかったら、止めて報告する。

#### D-1 (改訂): 起こすのは、EP の補助の関数を除く関数のエントリの安全点だけ

- 決めたこと: タスクごとの上限は、最上段のフレームが関数のエントリの `pc` で止まっている安全点で、その関数が EP の補助の関数 `id$`、`compose$`、`prompt$`、`resume$`、`rewind$`、`mask$[..]` (docs/spec/core-ir.md の「EP が作る補助の関数」) でないときだけ IO の失敗にする。`Call`/`CallN`、`TailCall`、`TailSelf`、関数に入った `Apply`/`TailApply`/`ApplyRest`/`Clause`/`TailClause`、`par` の安全点がこれに当たる。`Ret`/`RetN` の後、部分適用を返した `apply` の後、補助の関数のエントリでは保留のままにし、次の起こせるエントリで起こす。旗でも本当の深さの超過でも同じ扱いである。判定は「`Tables::unwind_at(task.pc)` の行が、起こすエントリの行か」で、行の種類 `UnwindEntry { No, Raises, Defers }` は、エントリの行を出すときに関数の名前から決める (`eml_lir::ep_helper(name)`: 5つの名前との一致か、`mask$[` で始まること。名前の定数と判定は `eml_lir` に1組だけ置き、EP が作るときも VM が行を出すときも同じものを使う)。どの状態でも、エントリの安全点で深さが `real + LIMIT_HEADROOM` を超えていれば run を止める (硬い限度)
- 理由: エントリのフレームは RC の対象の引数をすべて所有し、借りた値を持たない (lir.md:554) ので、エントリの行だけで巻き戻せる。ただし補助の関数のエントリの `evv` は、利用者のプログラムのどの地点の `evv` でもない。`prompt$` のエントリでは、終わった handler の evidence がまだ `evv` に残っていて、その着地の行はもう戻った `apply body … prompt ev` のものなので、着地がない。`resume$` と `mask$` のエントリでは、これから入り直す handler の evidence をまだ積んでいない。`compose$`、`rewind$`、`id$` はその間の κ の中で動く。そこで起こすと、プログラムに見えない handler に届く。再開の途中なら外側の prompt に、`prompt$` のエントリの後なら着地のない所に届く。T-10 の速いモードで、待ちの中のタスクのセルに立った旗が再開の `rewind$` のエントリで起き、利用者の `try_io` でなく底の `try_all` に届いたのがこれである (U-10 の consult の trace。`Sched::Sequential` でも T-M の形で決まって起きる)。ほかのエントリの `evv` は、その地点のプログラムが見る `evv` である。`f$kN` と `f$jJ` は、κ の下の鎖が巻き戻って戻った後に、`mask$`/`resume$` が戻した `evv` で呼ばれる (core-ir.md:1065)。節のエントリの `evv` は、エントリの本体が替えるまで perform した側のもので (`clause_call` は `apply_clause` の後に安全点を置き、`evv_set` は節の本体の中にある)、起こすと perform した所のいちばん内側の `try_io` に届く。その着地は節のフレームの下に生きている。`$cont$`、`Std.Async.resume_task`、`start_task`、`bottom` のエントリの `evv` は節の文脈か空 (`vm.rs:250`) で、空なら旗は保留のまま残る。`fork$E.op` は子の中で動き、IO の evidence がない。`reperform$E.op` と、`compute` の仕事の失敗を起こし直すコードは親のタスクの上で動き、そのエントリは起こすエントリである。そこに保留の旗が届くと、子の失敗の起こし直しの代わりに `ResourceLimit` が親のいちばん内側の `try_io` に届く (R9)。関数の名前でコードと ABI が決まる規則 (CLAUDE.md) のとおり、行の種類も関数の名前だけで決まる。名前は `eml_lir` の定数1組で、EP も VM も、後のネイティブもそれを使う。T-L が判定を、T-L2 がコンパイルした名前を固定する
- 退けた案:
  - `Ret` の後でも起こす案。呼び出しの行ごとに束縛した値の列が要り、行の意味が2つになる。ネイティブには戻りの位置の安全点がない
  - 補助の関数のエントリで、旗では起こさず本当の超過では起こす案 (U-10 の advice 1)。届き方の誤りは同じである。再開は捕まえた深さに `compose$` のフレームを足して組み直すので、上限の近くで yield したタスクは、まさに巻き戻しの中で本当に超える
  - 再開のときの旗を `Wake` で届ける案。`prompt$` のエントリと、動いているタスクが継続を再開する形を覆わない
  - 実行時に「`evv` が今の地点のものか」を確かめる案。巻き戻しの途中を示す `Ctx` の状態はなく、D-3 が速い道の状態を増やすのを禁じている
  - LIR に印を持たせる案。6つの決まった名前のために、`eml_lir` の構文、text、pretty と `LirFn` のリテラル約 30 か所が変わる。ネイティブの段で LIR に要れば、そのときに入れる
- 範囲: `limit_hit`、`code.rs` の `Unwind`、`compile/emit.rs`、`eml_lir` の名前の定数と `ep_helper`、`eml_core_ir` の `evidence/helpers.rs` と `evidence/mod.rs` の名前のリテラル
- 見直す条件: エントリでない位置で起こさないと困る事例 (エントリに来ない長い直線のコードで、旗が届かないなど) が見つかったとき。EP が `evv` を書き換えるか κ のフレームになる関数を新しく作るとき。6つの名前のどれかが変わるとき
- 適合の確かめ:
  - MUST: `limit_hit` は起こすエントリの行 (`Raises`) でだけ `abort_to` を呼ぶ。エントリでない行と `Defers` の行では、状態も `depth_limit` も変えずに `Ok(())` を返す。テスト T-B、T-C、T-J
  - MUST: どのエントリの行でも、`task.stack_depth() > real + LIMIT_HEADROOM` なら `Err(Stopped(DepthLimit))`。テスト T-K
  - MUST: `Defers` になるのは6つの名前だけ。テスト T-L (名前の判定) と T-L2 (EP が実際にその名前を作る)
  - MUST: 利用者の `try_io` の中で待っているタスクに届いた旗を、再開の後にその `try_io` が捕まえる。テスト T-M (決定的) と、速いモードの 50 回の繰り返し (T-9a の完了の条件)
  - MUST: U-1〜U-8 と M-1〜M-5 が checked mode で漏れなく通る
- リポジトリに残す所: vm.md の「呼び出しと戻り」と「タスクの上限」、core-ir.md の「EP が作る補助の関数」 (T-4、T-9a)

#### D-2: すべての関数に巻き戻しの行を持たせる

- 決めたこと: すべての関数のすべての末尾でない `call` (呼ばれる側を問わない)、`apply`、`clause`、`par`、`Pure` でない extern、`yield_abort`、`child_abort` を巻き戻しの地点にする。`may_abort` は消す。さらに、すべての関数にエントリの行を、関数の開始の `pc` を鍵にして置く。エントリの行の `owned` は RC の表現を持つすべての引数を数 1 で持ち、token も着地も持たない。`Pure` の extern、prim、末尾呼び出しの終端には行を置かない
- 理由: どのフレームも巻き戻しで通りうるようになる。関数がタスクの中で動くかは動的に決まり、「関数の名前がコードを決める」の規則で2つの写しは作れない。VM では行はデータなので実行時の費用はない
- 退けた案: `may_abort` を残してタスクのコードにだけ行を足す案 (上の理由で作れない)
- 範囲: `eml_lir::unwind`、`compile/emit.rs`、`code.rs`
- 見直す条件: 表の大きさが読み込みの時間かメモリで問題になったとき
- 適合の確かめ:
  - MUST: `cargo nextest run -p eml_lir --test integration -E 'test(unwind::)'` が通る
  - MUST: ワークスペースの `cargo nextest run` で `a frame stops outside an unwind point` が一度も出ない
  - MUST: テスト `unwind_pcs_are_unique` が通る (エントリの行と呼び出しの行が同じ `pc` を持たない)
- リポジトリに残す所: lir.md の「巻き戻しの地点」 (T-1)、vm.md の「巻き戻しの表」 (T-2)

#### D-3 (一部改訂): 速い道を変えず、深さの上限の欄で状態を表す

- 決めたこと: `Ctx::poll`、`stop_reason`、`vm::poll` の `Ok` の枝をバイト単位で変えない。タスクごとの状態は `depth_limit` の値で表し、`Ctx` に検証済みの書き手 `set_depth_limit` を1つ足す。VM の `Task` は次の状態を持つ

  ```
  Limit::Stops                        depth_limit = real                  タスクの外。DepthLimit は run を止める
  Limit::Task                         depth_limit = real                  タスクの中で、保留はない
  Limit::Pending                      depth_limit = 0                     旗が立っている。次の安全点で poll が失敗し、次のエントリで起こす
  Limit::Aborting { pending: bool }   depth_limit = real + LIMIT_HEADROOM  abort_to から着地まで
  real = max_depth.saturating_sub(task.depth_offset)
  const LIMIT_HEADROOM: usize = 1 << 18   // 語。2 MiB
  ```

  `Aborting` の間に poll が `DepthLimit` を返したら、今の文で run を止める (硬い限度)。改訂で足した: `Task` と `Pending` でも、関数のエントリの安全点で深さが `real + LIMIT_HEADROOM` を超えていれば `limit_hit` が run を止める。補助の関数のエントリで起こすのを延ばしても、限度がなくならないためである (D-1、T-9a)
- 理由: poll の速い道は2つの比較のままにする (multicore.md:150、object-model.md:861)。`depth_limit` はもともと切り替えのたびに書き直すタスクごとの値で、`Stop::DepthLimit` は「ビットがなく、深さが限度を超えた」を表す。猶予に限度を残すので、巻き戻しの途中の節のエントリが暴走しても止まる
- 退けた案:
  - worker の語にタスクの上限のビットを足す案。ビットは worker を名指し、次に安全点に来たタスクを失敗させてしまう
  - 速い道で3つ目の欄を読む案。poll の形が変わる
  - 猶予を `usize::MAX` にする案。硬い限度がなくなる
- 範囲: `eml_rt` の `Ctx`、`eml_vm` の `vm.rs`
- 見直す条件: ネイティブで `stack_limit` を同じ形で毒にできないと分かったとき
- 適合の確かめ:
  - MUST: `git diff 837b0fe9 -- crates/eml_rt/src/heap/ctx.rs` で `fn poll` の中が変わっていない。`control.rs` の `stop_reason` (788-809) も変わっていない
  - MUST: `depth_limit` の書き手は `switch_task` と `set_depth_limit` だけ。`crates/eml_vm/src` のテストでないコードで `switch_task(` を呼ぶ所は、包み `switch_to` (その後で `Task.limit` を当て直す) と、`cells::with_worker` の行きと帰りの2つだけにする。`with_worker` は切り替える前に `Ctx::depth_limit()` (検証済みの読み手。T-4 で足す) で今の値を取っておき、帰りの `switch_task` の後で `set_depth_limit` で戻す。`grep -n "switch_task(" crates/eml_vm/src/*.rs` で確かめる (実行の途中で、計画が `with_worker` の2つを数えていなかったと分かり、この形に直した)
  - MUST: `bench/run.sh` の `fib` と `empty` の命令の数が docs/implementation/benchmarks.md の値の揺れの範囲に入る
- リポジトリに残す所: object-model.md の「タスクの切り替え」と `Ctx` の欄の表 (T-3)、vm.md の「タスクの上限」 (T-4)

#### D-4 (一部改訂): タスクの外と、着地と、取り消し

- 決めたこと:
  - `main`、`compute` の仕事、タスクの外の `par` の子は `Stops` で、今と同じく `VmFault::Stopped(Stop::DepthLimit)` と今の文で止まる。右の子は親の種類を受け継ぐ (`Stops` なら `Stops`、それ以外なら `Task`)。子が `Pending` か `Aborting` になって始まることはない
  - IO の evidence がないとき: 本当の深さの超過なら `DepthLimit` で run を止める。旗だけなら旗を捨てず `Pending` のまま続け、evidence のある所 (`par` の子から親に戻った後など) の次のエントリで起こす
  - `par` の子 (左も右も) の row は `never e` で IO を断るので、子の evv に IO の evidence はない。そのため、タスクの中でも `par` の子の深さの超過は `compute` の仕事と同じく run を止める (実行の途中で分かり、利用者が 2026-11-04 にこの扱いに決めた。roadmap の「その後の項目」に、親で起こし直す道を見直す条件とともに書く)
  - `abort_to` (IO と Panic のすべての中断) は、エントリで `Pending` を `Aborting { pending: true }` に変える。節のエントリの中では旗で失敗しない
  - 改訂で足した: 節のエントリの安全点 (`Clause`、`TailClause` の後) で起こした失敗は、handler の外側でなく、perform した所のいちばん内側の `try_io` に届く。節のエントリが `evv` を handler の外側のものに替えるのはエントリの本体の中で、安全点はその前にあるためである。着地は節のフレームの下に生きている。VM が `abort_to` で積む `io_failed` と `panicked` のエントリには `Aborting` の間にしか来ないので、そこでは起こさない。利用者が書いた `never` の操作の節は `Clause` の命令で呼ばれ、そのエントリは起こすエントリである。保留の旗がそこで起きると、操作の節を動かす代わりに `ResourceLimit` が perform した所の `try_io` に届く (R9。R1 と同じ順の競合)。この規則を vm.md に書く (T-9a)
  - `vm::unwind` が `Ok` で戻るすべての所 (`exit` の `FINISH`、`CHILD_DONE`、着地) で `landed` を呼ぶ。`Aborting { pending: true }` は `Pending` (毒あり) に、`Aborting { pending: false }` は `Task` (本当の限度) に戻す
  - yield の途中 (`ctx.yielding() != 0`) の超過は保留のままにする
  - `Async.shield` は上限の失敗を後に回さない。保留の旗は取り消しの中断を越えて残り、着地の後の次のエントリで起こす
- 理由: タスクの外を変えない。巻き戻しの区間を状態で囲むので、節のエントリの中では旗で失敗しない。すべての着地は `vm::unwind` を通るので、1か所で戻せる
- 範囲: `limit_hit`、`abort_to`、`unwind`、`Task` の構築
- 見直す条件: 新しい着地の道を足すとき
- 適合の確かめ: テスト T-A、T-C、T-D、T-E、T-F。`calls.rs`、`par.rs`、`workers.rs::a_depth_error_in_a_job_stops_the_run` が変わらずに通る
- リポジトリに残す所: vm.md の「タスクの上限」と「中断と実行時エラーの分担」、stdlib.md の「取り消し」と「`compute`」 (T-4)

#### D-5: 監視のスレッドがタスクに旗を立てる手順

- 決めたこと: 監視のスレッドが持つのは `Arc<Control>`、`Arc<Shared>` (mail)、`Arc<Run>` だけで、新しい `Arc` は足さない。タスクに旗を立てるときは、`CellKind::Task` のセルについて次のとおりにする

  ```rust
  shared.mail.push(MailItem::Limit(cell));
  for k in 0..run.slots.len() { control.notice(k) }
  ```

  `Ctx`、ヒープ、`NOTICE` 以外の worker の語のビット、登録表の lock には触らない。`CellRef` は worker が lock なしで公開した値 (D-11) から得る。古い `CellRef` は世代の確かめで `None` になるので害はない。立っている旗をもう一度立てても何も起きない

  受け手の規則: `MailItem::Limit` はどの worker でも取る。取った worker は登録表の lock の下で `Registry::limit_task` を呼ぶ。`TaskCell.limit = true` にし、タスクがほかの worker で `Running` なら持ち主の受け箱へ `Notice::Limit(cell)` を回す。自分が持ち主なら、生きている VM の `Task` を `Pending` にする (今のタスクならすぐ毒を入れる)。`Placed`/`Parked` のセルは旗だけを立て、`started`/`resumed` が旗を返す。旗を下ろすのは起こした側 (`clear_limit`) である。横取りされてフレームを持ったまま待つタスクは `Phase::Running` のままで、持ち主はその `Task` を持つ worker である (`Running` を書くのは `started`/`resumed` だけ。`Parked` は cells.rs:2121、`Finished` は 2163)
- 理由: タスクを worker と救済をまたいで名指せるのは登録表だけである。mail は監視のスレッドが既に使っている lock を取らない道である
- 例外 (Q-1): 決定的な方針 (`Sequential`、`Seeded`、`RescueAlways`) では、監視のスレッドでなく各 worker が `gather` の始めに同じ関数 `pressure_tick` を呼ぶ (D-12)。読むもの、送る手順は同じである
- 範囲: `monitor.rs`、`cells.rs`、`workers.rs`、`sched.rs`
- 見直す条件: 監視のスレッドに別の種類の旗を足すとき
- 適合の確かめ: テスト T-G〜T-I、V-10。`monitor.rs` に新しい `Arc` がない
- リポジトリに残す所: vm.md の「タスクとスケジュール」と「セルと登録表」 (T-5、T-9)

#### D-6 (改訂): ネイティブが受け継ぐもの

- 決めたこと: ネイティブは、タスクごとの種類を `#[repr(C)]` の `Ctx.limit_class: u32` (0 `Stops`、1 `Task`、2 `Pending`、3 `Aborting`) に置き、タスクを切り替えるたびに `stack_limit` と一緒に書く。`Pending` では `stack_limit` に毒を入れる (SP がいつも下回る値)。エントリの冷たいブロックは、コード生成の時点で関数の種類で選ぶ。補助の関数でない関数のブロックは、`Stops`/`Aborting` なら `eml_fatal(DepthLimit)`、`Task`/`Pending` で関数のエントリかループの戻りなら、戻り値の道で `ResourceLimit` を起こす。6つの補助の関数 (D-1) のブロックは、まず `limit_class` を見て、`Stops`/`Aborting` なら今までどおり `eml_fatal(DepthLimit)` を呼ぶ。`Task`/`Pending` では `ResourceLimit` を起こさず、SP を本当の限度から `LIMIT_HEADROOM_BYTES` を引いた値と比べて、下回れば `eml_fatal(DepthLimit)`、そうでなければ続ける。VM の `limit_hit` が手順 1 で `Stops`/`Aborting` を止めてから 3' と 3'' に進むのと同じ順である。`Pending` の `stack_limit` には毒が入っているので、この比べ方のために `Ctx` は本当の限度 (か `stack_base`) を2つ目の `#[repr(C)]` の欄に持つ。囲みと着地の規則は VM と同じにする。`Aborting` の間の限度は `stack_limit - LIMIT_HEADROOM_BYTES` である。すべての末尾でない呼び出しの位置が中断を確かめる地点になる。補助の関数にループはないので、戻りの辺の安全点に変種は要らない。戻りの位置の安全点も、`Control` のタスクごとの語も要らない
- 理由: VM と同じ規則でネイティブを作れることを、段を閉じる時点で文書に固定する。D-1 の判定は関数ごとの静的な性質なので、ネイティブでもコード生成の時点で決まる。補助の関数でない関数の費用は変わらず、`Ctx` の欄は今のうちに1語増やしておく
- 範囲: native.md (この段では文書だけ)
- 適合の確かめ: native.md の「タスクごとの上限」と「安全点とスタック」と `Ctx` の欄の一覧 (native.md:88) がこの内容になっている
- リポジトリに残す所: native.md (T-11)

#### D-7: テストの形

- 決めたこと: 深さのテストは UI の先頭の行の印 `-- max-depth: <words>` (値は 4096。足りなければ全部を 8192) を使い、既定の 2^27 に頼らない。メモリのテストは印 `-- max-bytes: <bytes> soft <bytes>` を使う。どちらも harness だけの仕組みで、CLI のフラグではない。UI テストは種類と文の文字列だけを出し、深さ、数、繰り返しの回数を出さない。新しいテストはどれも1つで 1 秒未満で終わる
- 理由: 既定の深さは UI テストには大きすぎる (`crates/eml_vm/tests/par.rs:971`)。出力が worker の数や seed で変わらないようにする
- 適合の確かめ: `cargo nextest run --profile timing -j 1` で `SLOW` が出ない
- リポジトリに残す所: testing.md の「UI テスト」 (T-6、T-10)

### 5.2 短中期の設計

理由があれば実装の中で変えてよい。変えたら記録する。

#### D-10: 深さの値と文

- 決めたこと:
  - タスクごとの限度は今の `max_depth` を、タスク自身の `depth_offset` から数えたもの。新しい欄は足さない。VM の「スタック」と「深さ」は同じ量である
  - 起こす文: 起こした時点で深さが本当に `real` を超えていれば `the call depth limit is exceeded` (旗が立っていても旗は消費して下ろす)。そうでなければ `the memory soft limit is exceeded`
  - 種類は tag 9。`crates/eml_lir/src/lib.rs` の `IO_CANCELLED` の隣に `pub const IO_RESOURCE_LIMIT: u32 = 9;` を置く
  - 捕まえられなかった `ResourceLimit` は、ほかの IO の失敗と同じく底の `try_all` が受け、スコープの失敗になる。`main` で捕まえなければ、その文の実行時エラーになり、位置は `run_result` の位置である (insta が固定する。`spawned_failure_fails_run.snap` と同じ `<std>/Async.em:75:9` の見込み)。終了コード 1
  - `RunStats` に欄は足さない
- 退けた案: タスクごとに別の柔らかい深さを置く案 (要件がなく、`Limits` の欄が増える)

#### D-11: メモリを数える量と、公開する値

- 決めたこと:
  - `Limits` に `soft_bytes: usize` を足す。`Limits::default()` と `Control::unbounded()` では `usize::MAX` (柔らかい上限なし)
  - 7/8 は CLI の方針として `eml_cli` に置く。`default_limits()` と `repl_limits()` はそれぞれ自分の `max_bytes` から `soft_bytes = soft_of(max_bytes)` を求める。`soft_of(m) = if m == usize::MAX { usize::MAX } else { m / 8 * 7 }`
  - タスクの数は「そのタスクが動いている VM の `Task` だった間に、その worker のヒープが `fresh_page` で取ったページのバイト数」である。単調に増え (大きい物体の払い戻しは引かない)、待ちをまたいで足し続け、旗を消費して起こしたときに 0 に戻す
  - `eml_rt`: `Control.taken: Vec<AtomicUsize>` (worker ごと) を `charge` が冷たい道で足す。`Pages` は `slot` (worker の番号) を持ち、`Ctx::for_worker` が決める
  - VM: `Scheduler.mark` は今の区間の始めの `taken(k)`。エントリの `Task` は `counted` (この区間より前のバイト数) と `added` (このエントリの分。セルへ書き戻す) を持つ。`TaskCell.bytes` は待ちをまたぐ合計で、`leave` が足し、`started`/`resumed` が返し、`clear_limit` が 0 にする
  - `Run.slots[k]` に5つの原子的な値を足す: `current: AtomicU64` (動いている `Async` のタスクの `CellRef` を符号にしたもの、なければ `NO_CELL = u64::MAX`)、`bytes` と `mark` (動いているタスクの数は `bytes + taken(k).saturating_sub(mark)`)、`top: AtomicU64` と `top_bytes: AtomicUsize` (その worker の実行待ちで数のいちばん大きいタスク。実行待ちとは、フレームを持ったまま横取りされたタスクか、自分から列に戻ったタスク (`yield_now`、`sleep 0`、速い道の譲り))
  - `par` の右の子と `compute` の仕事はどのタスクにも数えない。左の子は親のタスクに数える
- 理由: 確保の場所は `fresh_page` だけで、`Pages` はタスクも lock も知らない。監視のスレッドがヒープも `Ctx` もなしに読める生きた量は `Control` の原子的な値だけである。費用はページごとに冷たい道での `fetch_add` 1回、読み1回、比較1回
- 退けた案: 確保器がセルに書く案 (`Pages` にタスクの ID と lock が要る)。切り替えでだけ読む案 (切り替えないタスクの数が古いまま)。物体のバイト数を数える案 (速い道が変わる)。払い戻しを引く案 (払い戻しは別のスレッドで起き、帰属できない)。実行待ちのタスクをすべて公開する案 (`Runnable::Resume`/`ResumeIo` の構築子 47 か所に数が要るか、lock が要る)
- `eml_rt` の変更は試作し、`cargo verus verify -p eml_rt` が 1147 verified, 0 errors、verusfmt と `cargo test -p eml_rt` (793) とワークスペースの `cargo check --all-targets` が通った。付録の diff がその試作である。閾値の規則 (D-12) は試作の後に決めたので、diff の `charge` と `Control` の方法は D-12 の形に書き直す

#### D-12 (一部改訂): 閾値と、誰が旗を立てるか

- 決めたこと:
  - `Control.next: AtomicUsize` を `soft_bytes` で始める。`gap(b) = max(64 KiB, (max_bytes - b) / 4)`、`rearm(b) = if b < soft_bytes { soft_bytes } else { b + gap(b) }`
  - 越えたとき: ページの課金が `next` を越え (`old <= next < total`)、`max_bytes` は超えないとき、`charge` は自分の worker の語に `NOTICE` を立てる。柔らかい上限で `MEMORY` は立てない
  - 取るとき: 旗を立てる側は `bytes > next` を見たら候補を探し、候補があるときだけ `next` を CAS で `rearm(bytes)` に進め、旗を1回送る
  - 改訂で足した。候補がないとき: `Normal` (監視のスレッド) では `next` を動かさず、次の刻みでまた探す。決定的な方針 (`gather` の中の `pressure_tick`) では、旗を送らずに `next` を `rearm(bytes)` に進める。確保器の `NOTICE` は `next` を越える1回の課金でしか立たず、切り替えない (`yield_now` も I/O もしない) タスクは `Sequential` では集める地点に来ない。そのため進めないと、候補のないまま越えた後 (タスクの前の `main`、右の子、`compute` の仕事の確保) に増え始めたタスクには旗が立たず、硬い上限で run が止まる (U-9 のレビューの B-2。再現したのが T-9b の P-7)。進めれば、次の閾値を越えたタスクの集める地点でまた探し、そのときの数のいちばん大きいタスクに旗が立つ。`next` を今の `bytes` に戻す形は退けた。次のページを最初に取ったタスク (`Async.run` の本体のように数の小さいタスク) に旗が立つためである (scratchpad の replan で確かめた)。複数 worker の決定的な run では、原因のタスクが救済で移る途中 (`top` を消され、まだ入っていない) に別の worker の集める地点が閾値を越えているのを見ると、候補がないので閾値が1つ進み、旗は次の閾値まで遅れる (RS4)。原因のタスク自身がページを取れば自分の集める地点で候補になるので、この窓に当たるのは、その間にほかのタスクが越えるページを取ったときだけである。遅れても出力は変わらず、checked mode の M-1〜M-4 なら 63 MiB の増加で 1 秒に満たない。進める地点を `NOTICE` を見た集める地点に絞る案は退けた。`polled_stop` が `NOTICE` を先に下ろすので `gather` では見分けられず、印を1つ通す配管が増える割に、この窓は閉じない (越えるページをほかのタスクが取る形は残る)
  - 戻すとき: `bytes < soft_bytes` かつ `next > soft_bytes` なら、`next` を CAS で `soft_bytes` に戻す
  - REPL: `Control::clear()` (入力の始め) で `next = rearm(bytes())` にする
  - 回数の上限はない。毎 tick の送り直しはしない (小さいページは返らず `bytes` は柔らかい上限を超えたままなので、送り直すと tick ごとにタスクを止める)
  - 選び方: `choose(run, taken)` はすべての slot を番号の順に見る。slot の版の数 `seq` (書き手が `current`、`bytes`、`mark` を書く前に奇数に、書いた後に偶数に進める) を読み、奇数なら飛ばす。`current`、`bytes`、`mark` を読み、もう一度 `seq` を読んで変わっていなければ、`current` が `NO_CELL` でないとき候補 `(bytes + taken(k).saturating_sub(mark), current)` にする (同じセルを数え直して公開し直すことがあるので、`current` を2回比べる形では古い組を受け入れてしまう。実行の途中でレビューが見つけ、この形に直した)。次に `top` が `NO_CELL` でなければ候補 `(top_bytes, top)` にする。前の最良より真に大きく、0 より大きいものだけを残す。同点なら番号の小さい worker、同じ worker なら `current` が勝つ
  - 誰が: `Sched::Normal` (`Memory::Watched`) では監視のスレッドが tick ごとに `pressure_tick` を1回呼ぶ。決定的な方針 (`Memory::Gathered`) では、各 worker が `Scheduler::gather` の中で、`take_notice(k)` の直後、最初の `shared.lock()` の前に同じ `pressure_tick` を呼び、監視のスレッドは呼ばない (Q-1)
- 理由: 一定の幅で進めると回数に上限ができ、長く動くサーバが保護を失う。残りの余裕の 1/4 なら尽きない。決定的な方針は壁時計の監視のスレッドに頼れない (1 worker の決定的な run では監視のスレッドが動かない)。`Rescue::Always` も同じ理由で救済を worker の上で行う。確保器の `NOTICE` があるので、1 worker の決定的な run では旗の地点がスケジュールによらない
- 退けた案: 毎 tick の送り直し。増えたら旗を立てる案 (捕まえた直後にまた旗を立てる)。`(max - soft)/4` の固定の幅 (4回で尽きる)。柔らかい上限より上で `next` を下げる案 (揺れのたびに旗を立てる)。`Control` の中で 7/8 を求める案 (方針が `eml_rt` に入る)。`Normal` でも確保器が旗を立てる案 (前提に反する)。候補がないときに確保器が閾値の上でページごとに `NOTICE` を立てる案 (`Normal` もページごとに集める地点を通る。監視のスレッドは次の刻みで探し直せる)。タスクが動き始める所 (`enter`、`resume`) で探し直す案 (その時点の数は 0 で候補にならず、その後の確保に引き金がない)

#### D-13 (一部改訂): 決定性について約束すること

- 1 worker の `Sequential` と `Seeded` (checked) では、越えるページ、旗の地点、起こす地点がどの run でも同じである。ただし増える間に動けるのが原因のタスクだけで、大きい物体を確保しないときに限る。候補のないまま越えた run でも、閾値は決まった幅だけ進み、次に越えた所で旗が立つので、地点は run によらない (D-12)。ただし `Normal` は閾値を進めずに次の刻みで探し直すので、候補のないまま越えた後の旗の地点は checked (次の閾値) と速いモード (次の刻み) で違う。出力は同じである。そのため出力と比べる `RunStats` が一致する (前提: checked mode では1つのヒープのページの数は大きさの種類ごとの合計だけで決まり、順序によらない。推定)
- 2 と 8 worker では出力だけが一致する。テストは種類と文の文字列だけを出す。増える間に数が 0 でないタスクは1つなので、選ぶセルはいつも原因のタスクである
- 速いモードの実行 (`Normal`、1 worker) は壁時計の監視のスレッドを通る。原因のタスクが柔らかい上限から硬い上限まで増える前に、監視のスレッドが1回 tick できればよい。柔らかい 4 MiB、硬い 256 MiB、毎秒 31 MB で約 8 秒の余裕がある。旗は刻みで届くので、`yield_now` で待ちに入ったタスクのセルに立つこともある。その旗は再開の後の、補助の関数でない最初のエントリ (κ のいちばん内側の継続の関数) で起き、利用者の `try_io` が捕まえる (D-1)。M-1〜M-4 の出力は、どちらの届き方でも同じである
- `-- workers: skip` を付けるファイルはない。ある UI テストで seed の `RunStats` が食い違ったら、そのファイルの増える前の段階で違う確保を取り除いて直す。印は付けない

## 6. 実装の約束

- `Ctx::poll`、`stop_reason`、`vm::poll` の `Ok` の枝、ページの中の確保の道を変えない (D-3)
- 新しい処理は `polled_stop` の3つの match の腕の `Stop::DepthLimit` と、`safepoint` の冷たい側にだけ置く (D-3)
- `crates/eml_vm/src` のテストでないコードで `ctx.switch_task` を呼ぶのは包み `switch_to(ctx, task, evv)` だけにする。今呼んでいる所 (vm.rs:530, 1216, 1442, 1453, 1476, 1479, 1511, 1514, 1620, 3138) をすべて置き換える (D-3)
- 柔らかい上限の道が書くビットは `NOTICE` だけ (確保器からは課金した worker の語、旗からは全 worker の語)。`MEMORY`、`INTERRUPT`、`TERMINATE`、`END` は立てない (D-12)
- `eml_rt` の新しい項目は `verus!` の中で検証するか、`verus!` の外の安全な原子的操作 (object-model.md:1292 の種類) にする。trust の表の行は足さない (D-3、D-11)
- コードのコメントと `spec/` は日本語で書き、natural-japanese のスキルに従う。コードは `docs/plans/` もこの文書の ID (D-x、T-x) も引かない。規則を実装するコミットと同じコミットで `spec/` の節を書き、コメントはその節を引く
- 巻き戻しのエントリの行の種類は関数の名前だけから決め、名前の一覧 (`id$`、`compose$`、`prompt$`、`resume$`、`rewind$`、`mask$[`) は `eml_lir` の定数と `ep_helper` に1組だけ置く。EP が関数を作るときも、VM が行を出すときも、それを使う。`functions` を走査して判定しない (D-1)
- 候補のないときに閾値を進めるのは `gather` から呼ぶ `pressure_tick` だけで、監視のスレッドは進めない (D-12)。`eml_rt` は変えない
- アドホックな `.em` は `timeout 20` の下で動かす

## 7. 実装の作業

各作業は1つのコミットで、それだけで確かめられる。

| ID | 作業 | もとの決定 | 依存 | 並べられるか |
|---|---|---|---|---|
| T-1 | `eml_lir` の巻き戻しの表 | D-2 | - | いいえ |
| T-2 | `eml_vm` の巻き戻しの表の生成 | D-2 | T-1 | いいえ |
| T-3 | `Ctx::set_depth_limit` | D-3 | - | T-1、T-2 と並べられる |
| T-4 | 状態の機械と、起こす道 | D-1、D-3、D-4、D-10 | T-2、T-3 | いいえ |
| T-5 | 監視のスレッドからの経路 | D-5 | T-4 | いいえ |
| T-6 | 印 `-- max-depth:` と深さの UI テスト | D-7、D-10 | T-4 | T-5 と並べられる |
| T-7 | `eml_rt` の数と閾値、CLI の既定 | D-11、D-12 | T-3 | T-4〜T-6 と並べられる |
| T-8 | 数えることと公開すること | D-11 | T-5、T-7 | いいえ |
| T-9 | 選ぶことと、旗を立てる引き金 | D-5、D-12 | T-8 | いいえ |
| T-9a (改訂で足した) | EP の補助の関数のエントリでは起こさない | D-1、D-3、D-4 | T-9 | いいえ |
| T-9b (改訂で足した) | 候補のないときに閾値を進める | D-12、D-13 | T-9a | いいえ |
| T-10 (改訂) | 印 `-- max-bytes:` とメモリの UI テスト | D-7、D-13 | T-6、T-9b | いいえ |
| T-11 | `Close task-limits` | すべて | T-10 | いいえ |

### T-1: `eml_lir` の巻き戻しの表

- 目的: すべての関数に巻き戻しの行を持たせる (D-2)
- 対象: `crates/eml_lir/src/unwind.rs`、`crates/eml_lir/src/verify.rs`、`crates/eml_lir/tests/unwind.rs`、`crates/eml_core_ir/tests/unwind.rs`、docs/spec/lir.md
- 作業:
  - 型を次にする

    ```rust
    pub struct Unwinds { pub entry: Vec<(VarId, u32)>, pub points: Vec<UnwindPoint> }
    pub fn points(program: &Program, wanted: impl Fn(FnIdx) -> bool) -> Vec<Option<Result<Unwinds, VerifyError>>>
    ```
  - `point(stmt)` から `aborts` の引数を外す。`Rhs::Call(_)`、`Pure` でない `Rhs::Extern`、`Prim::YieldAbort|ChildAbort`、呼び出しを含む `Stmt::LetMany` を地点にする。`may_abort`、`Abort`、`rhs_abort`、`call_abort` を消す
  - `Recorder` から `aborts` を外し、`entry` を足す。`check_block` でブロック 0 の文 0 の前に、始めの所有の表を RC の表現 (`function.repr(p).is_rc()`) に絞って、数 1 で記録する
  - `unwind_points` は既に地点の有無によらず所有の `Checker` をすべての関数に流している (verify.rs:85-112)。変わるのは記録の絞り込みだけなので、新しい `VerifyError` は出ない
  - lir.md の「巻き戻しの地点」を新しい定義 (末尾でないすべての呼び出し、エントリの行、`may_abort` はない) で書き直す。「verifier」の節が `Recorder` を挙げていれば合わせる
- テストの変更:
  - 種類1 (削除): `crates/eml_core_ir/tests/unwind.rs::map_with_pure_function_has_no_unwind_point_at_the_call` (ファイルごと消し、`tests/main.rs` の宣言も消す)。`crates/eml_lir/tests/unwind.rs` の `recursive_pure_functions_do_not_abort`、`tail_call_propagates_abort`、`call_to_pure_function_is_not_a_point`、`call_to_function_that_calls_an_effectful_extern_is_a_point`
  - 種類2: `crates/eml_lir/tests/unwind.rs` の補助を `Unwinds` に合わせる (`points_of` は `.points` を見て、`entry_of` を足す)
  - 新しいテスト: `every_direct_call_is_a_point` (純粋な関数の呼び出しも地点)、`the_entry_row_owns_the_rc_parameters` (`fn f(a.0: obj, n.1: int, t.2: tobj)` のエントリが `[(0,1),(2,1)]`)。`pure_extern_and_tail_call_are_not` は残す
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `cargo nextest run -p eml_lir -p eml_core_ir` → すべて通る
  - `cargo clippy -p eml_lir -p eml_core_ir --all-targets -- -D warnings` → 終了コード 0
- 落とし穴: この時点では `eml_vm` はまだ `points` の古い形を使うので、T-1 のコミットでは `eml_vm` の呼び手を最小限に合わせて、ワークスペースが組めるようにする (`cargo build` → 終了コード 0)。エントリの行を VM で使うのは T-2 である

### T-2: `eml_vm` の巻き戻しの表の生成

- 目的: エントリの行を VM の表に載せる (D-2)
- 対象: `crates/eml_vm/src/compile/emit.rs`、`code.rs`、`disasm.rs`、`crates/eml_vm/tests/codegen.rs`、`crates/eml_vm/tests/bench.rs`、docs/spec/vm.md
- 作業:
  - `Unwind { slots, tokens, landing, entry: bool }` にする。`function()` はブロック 0 を出す前に `(start, Unwind { slots: エントリの引数, entry: true, .. })` を積む
  - 表を仕上げるとき、`pc` が真に増えていくことを `debug_assert!` で確かめる。点の命令の後にはいつも同じ関数の終端が続き (emit.rs:767-790)、終端は点でないので、エントリの行と呼び出しの行の `pc` は重ならない
  - `disasm` は関数の見出しの行に `entry unwind [s0, s2]` を出す
  - codegen のテスト `unwind_pcs_are_unique` を足す (std と bench のプログラムで、表の `pc` が重ならない)
  - vm.md の「巻き戻しの表」を書く (エントリの行、`Unwind.entry`)
- テストの変更: 種類2。`crates/eml_vm/tests/codegen.rs` の `unwind [` を含む5つの snapshot にエントリの行が増える。`crates/eml_vm/tests/bench.rs::unwind_points` (`points.len()` だけを数えるので `empty: 0` は変わらない見込み。変われば受け入れる)
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `cargo nextest run` (ワークスペース全体) → すべて通る。上の snapshot のほかに変わった snapshot がない (`git status --porcelain` に `.snap.new` がなく、変わった `.snap` は上のものだけ)
  - `cargo clippy --all-targets -- -D warnings && cargo fmt --check` → 終了コード 0
- 落とし穴: ワークスペース全体を流すのは、std のすべての関数で表が作れることを VM と UI テストで確かめるためである。`-p eml_vm` だけでは足りない

### T-3: `Ctx::set_depth_limit`

- 目的: `depth_limit` の2つ目の書き手を検証済みで足す (D-3)
- 対象: `crates/eml_rt/src/heap/ctx.rs`、docs/spec/object-model.md
- 作業: `verus!` の中に次を足す。コメントの節の名前は実際に書いた節に合わせる

  ```rust
  /// 今のタスクの深さの上限を書く。VM がタスクごとの上限の保留と猶予に使う (docs/spec/object-model.md の「タスクの切り替え」)。
  pub fn set_depth_limit(&mut self, limit: usize)
      requires old(self).wf(),
      ensures final(self).wf(), final(self) == Ctx { depth_limit: limit, ..*old(self) },
  { self.depth_limit = limit; }
  ```

  object-model.md の「タスクの切り替え」に2つ目の書き手と値の意味 (0 は保留、`real + LIMIT_HEADROOM` は猶予) を書き、`Ctx` の欄の表の `depth_limit` を直す。trust の表と「`unsafe` の核」は変えない
- 実装者に任せること: Verus が `ensures` の書き方を受け付けないときの書き換え。`rebased` の証明の形に倣う
- 条件: いつも
- 完了の条件:
  - `cargo verus verify -p eml_rt` → `eml_rt` の `verification results` が `0 errors`
  - `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` → 終了コード 0
  - `cargo nextest run -p eml_rt` → すべて通る

### T-4: 状態の機械と、起こす道

- 目的: タスクの中の深さの超過を `ResourceLimit` にする (D-1、D-3、D-4、D-10)
- 対象: `crates/eml_vm/src/vm.rs`、`cells.rs`、`crates/eml_lir/src/lib.rs`、docs/spec/vm.md、stdlib.md、object-model.md、docs/implementation/status.md
- 作業:
  - `Task.limit: Limit` を足す (D-3 の4つの状態)。`Task::new` は `Stops`、`Task::entry` は `Task`、`right_child(.., parent.limit.inherit())` で `inherit()` は `Stops` なら `Stops`、それ以外は `Task`。`Task::job` は `Stops`
  - `enum Polled { Continue, Switch, Collect, Limit }`。`polled_stop` の `Err(Stop::DepthLimit)` (最初の poll と2つの poll し直し) をすべて `Ok(Polled::Limit)` にする
  - `safepoint` で `tick` の前に `if polled == Polled::Limit { return limit_hit::<R>(code, ctx, task, sched); }`
  - `limit_hit` (`#[cold] #[inline(never)]`) は次の順に決める
    1. `Stops` か `Aborting { .. }` → `Err(VmFault::Stopped(Stop::DepthLimit))`
    2. `ctx.yielding() != 0` → `Ok(())`
    3. `code.tables.unwind_at(task.pc)` がエントリの行でない → `Ok(())` (保留)
    4. `real_over = task.stack_depth() > real`。文を D-10 で決める。`was_pending = task.limit == Pending`
    5. `task.limit = Aborting { pending: false }; ctx.set_depth_limit(real.saturating_add(LIMIT_HEADROOM))`。`was_pending` で `task.cell == Some(c)` なら、lock の下で `clear_limit(c)` (死んだセルは無視)
    6. `caught = abort_to(code, ctx, task, 0, ExternEffect::Io, IO_FAILED, |ctx| [Word::from_bits(IO_RESOURCE_LIMIT), ctx.fresh_string(msg)])?`
    7. `!caught`: `real_over` なら `task.limit = Task; ctx.set_depth_limit(real)` にして `Err(Stopped(DepthLimit))`。そうでなければ (旗だけ) `task.limit = Pending; ctx.set_depth_limit(0)` に戻し、手順5で下ろしたセルの旗も立て直して `Ok(())` (旗を捨てない)
  - `abort_to` の始めで `if task.limit == Pending { task.limit = Aborting { pending: true }; ctx.set_depth_limit(real + LIMIT_HEADROOM) }`
  - `unwind` の `Ok(())` で戻る直前のすべての所で `landed(ctx, task)` を呼ぶ (D-4)
  - `apply_limit(ctx, task)`: `Pending` → 0、`Aborting { .. }` → `real + LIMIT_HEADROOM`、ほかは何もしない (`switch_task` が本当の限度を書いた後なので)。`switch_to` と `enter` が呼ぶ
  - `switch_to(ctx, task, evv)` を作り、今の `switch_task` の呼び出しをすべて置き換える
  - `TaskCell.limit: bool` (共有の種類。lock の下で誰でも立て、起こした側が `Registry::clear_limit` で下ろす) を足す。`started`/`resumed` はセルの `limit` を返す (後で T-8 が `bytes` を足すので、`struct TaskMarks { limit: bool }` の形にしておく)。`enter` は立っていれば `task.limit = Pending; apply_limit`
  - `crates/eml_lir/src/lib.rs` に `pub const IO_RESOURCE_LIMIT: u32 = 9;`
  - 起こす道の確かめ (実装の参考): 呼び手 `f` の戻りの `pc` を `r` とする。`call` は `g` のフレームを見出し `(ret = r, dst, f.frame_size)` で積み、`task.pc = g.start` にする。安全点で `Polled::Limit` → `limit_hit` → エントリの行 → `Aborting` → `abort_to` が節のフレームを見出し `(ret = g.start, dst = 0, g.frame_size)` で積む (apply.rs:113-128 は節の関数自身のフレームの大きさを使う)。節のエントリは `yield_abort` を実行し、`unwind` が節のフレームを外して `task.pc = g.start` に戻す (vm.rs:387-393)。`unwind_at(g.start)` がエントリの行なので `g` の RC の引数を減らして外し、`task.pc = r` で `f` の呼び出しの行に進み、`try_io` の着地まで続く。節のフレームの `dst = 0` は読まれない
  - spec: vm.md の「呼び出しと戻り」(エントリでだけ起こす規則)、「巻き戻しの手順」(エントリのフレームはエントリの行で放す)、「中断と実行時エラーの分担」(タスクの深さの上限と旗は中断。タスクの外は実行時エラーのまま)、「上限と割り込み」(タスクごとの段落と2つの文)、「`Async` のタスク」の下に新しい「タスクの上限」(`Limit` の状態、猶予、着地で戻すこと、evidence のないとき、yield の途中、`par` の子、入れ子の `run`、取り消しとの競合、残る危険 R1 と R2)、「深さ」(タスクの中でも `par` の子の深さの超過は run を止める。子の evv に IO の evidence がないため)。stdlib.md の「`try_io`」(「深さの上限を捕まえない」に条件を付ける)、「`Std.Async`」(タスクの上限、`ResourceLimit`、文)、「実行時エラー」、「取り消し」(`shield` は後に回さない)、「`compute`」(変わらないことを書く)。object-model.md の「実行の制御」(`DepthLimit` は「深さか保留のタスクの上限」、poll は変わらない)。status.md:171
- テスト (手で書いた LIR を `eml_lir::parse` で読み、`a_running_task_collects_a_notice_at_its_safepoint` (vm.rs:3984) の `Ctx`/`Scheduler` の用意を使う):
  - T-A `Stops` で `depth_limit = 0` → `limit_hit` が `Err(Stopped(DepthLimit))`
  - T-B `Task` で `Ret` の `pc` (エントリでない) に `depth_limit = 0` → `Ok(())`、状態は変わらず、限度は 0 のまま
  - T-C IO の handler のないプログラムで、`TailSelf` の先頭で `Pending` → `Ok(())`、状態は `Pending` のまま、`depth_limit == 0` (旗は捨てない)
  - T-D `Aborting { .. }` で `DepthLimit` の poll → `Err(Stopped(DepthLimit))`
  - T-E `Pending` の後で Panic の `abort_to` (`try_panic` の handler を LIR で書く) → `Aborting { pending: true }`。`landed` の後 → `Pending` で `depth_limit == 0`
  - T-F `Pending` の親の `right_child` は `Limit::Task`、`Stops` の親の子は `Stops`
- 実装者に任せること: `Limit`、`Aborting`、`LIMIT_HEADROOM`、`switch_to`、`apply_limit`、`landed` の名前 (より分かりやすい名前があれば変えてよい)
- 条件: いつも
- 完了の条件:
  - `cargo nextest run -p eml_vm` → すべて通る
  - `cargo nextest run -p eml_vm -E 'test(calls::) | test(par::) | test(workers::) | test(scaling::)'` → 期待値を変えずに通る
  - `grep -n "switch_task(" crates/eml_vm/src/*.rs` → テストでない呼び出しは `switch_to` の1か所と `cells::with_worker` の2か所だけで、`with_worker` は帰りの後で取っておいた `depth_limit` を `set_depth_limit` で戻す
  - `cargo verus verify -p eml_rt` → `0 errors` (`Ctx::depth_limit()` を足すため)
  - `cargo clippy --all-targets -- -D warnings && cargo fmt --check` → 終了コード 0
- 落とし穴: `par` の worker ID の切り替えも同じ `Task` で `switch_task` を呼ぶので、`switch_to` を通さないと `Pending` と `Aborting` が消える。`Ret` が続く間は保留のまま冷たい道を通るが、着地が `real` より深いときだけで、次の `Call` で起きる

### T-5: 監視のスレッドからの経路

- 目的: 監視のスレッドが lock なしでタスクに旗を立てられるようにする (D-5)
- 対象: `crates/eml_vm/src/cells.rs`、`workers.rs`、`sched.rs`、docs/spec/vm.md
- 作業:
  - `MailItem::Limit(CellRef)`: `for_worker` はどの worker にも真。`take_mail` で `self.limit_task(cell, here, out)` を呼ぶ
  - `Notice::Limit(CellRef)`: 語を持たず、`stealable() == false`、`in_flight` に数えず、run の終わりに捨てる (`Rescue` と同じ)。`take_async` の `Async` の腕で同じ `limit_task` を呼ぶ
  - `Registry::limit_task(&mut self, cell, here) -> Result<Option<CellRef>, CellFault>`: 死んだか `Finished` なら `None`。`limit = true` にする。`Running` で持ち主がほかの worker なら `here.post.send(owner, Notice::Limit(cell))` して `None`。`Running` で持ち主が自分なら `Some(cell)`。`Placed`/`Parked` なら `None`
  - 集める地点は `Some` のセルを VM に返し、VM が印を付ける。今のタスクが `task.cell == Some(cell)` なら `Pending` と `apply_limit`。そうでなければ `sched.tasks` の中のそのセルの `Task` を `Pending` にする。どれもなければ何もしない (`Finish` に着いたエントリの `Task`。次の `started`/`resumed` が旗を見るか、セルが死ぬ)
  - `flag_cancel` の隣にテストの補助 `#[cfg(test)] pub(crate) fn limit_task(shared, cell)`
  - vm.md の「タスクとスケジュール」(mail の項目と知らせ) と「セルと登録表」(`limit` の欄と種類)
- テスト:
  - T-G ほかの worker が持つ `Running` のセルに `limit_task` → その worker へ `Notice::Limit` を回し `None`。自分が持つなら `Some(cell)`
  - T-H `Parked` のセルに `limit_task` → 旗が立ち、`resumed` が返し、`clear_limit` で下りる。死んだセルは `None`
  - T-I `MailItem::Limit` は worker 0 と 1 のどちらでも取れる (`for_worker` が真)
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `cargo nextest run -p eml_vm --lib` → すべて通る
  - `cargo clippy --all-targets -- -D warnings && cargo fmt --check` → 終了コード 0

### T-6: 印 `-- max-depth:` と深さの UI テスト

- 目的: 深さの上限を UI テストで確かめる (D-7、D-10)
- 対象: `crates/eml_cli/tests/ui.rs`、`tests/ui/`、docs/implementation/testing.md
- 作業:
  - 先頭の行の印 `-- max-depth: <words>` (正の整数。`-- signal:` と同じ印の塊と読み方) を足し、そのファイルのすべての実行に `Limits { workers, max_depth, ..default_limits() }` で当てる。形が違えば ``the max-depth mark is `-- max-depth: <words>`, not `{line}` `` で panic する。REPL のトランスクリプトでは断る
  - testing.md の「UI テスト」に印を書く
  - UI テスト (どれも `-- max-depth: 4096`。`run/async/` のファイルはタスクを始め、`spin 40` の約束に従う。出すのは種類と文の文字列だけ):
    - U-1 `tests/ui/run/async/limit_depth_caught.em`: 始めたタスクが1段ごとに `String` を持って `try_all` の下で再帰し、`ResourceLimit: the call depth limit is exceeded` を出す (checked mode の漏れの確かめが行の正しさを確かめる)
    - U-2 `tests/ui/run/async/limit_depth_fails_scope.em`: spawn したタスクが超え、`main` の `try_io (fn () -> Async.run body)` が種類と文を出す
    - U-3 `tests/ui/run-fail/async/limit_depth_uncaught.em`: `--- runtime error ---`、`the call depth limit is exceeded`、`run_result` の位置 (insta が固定)。終了コード 1
    - U-4 `tests/ui/run-fail/async/limit_depth_par_child.em`: タスクの中の `par` の右の子が再帰し、`try_all` があっても run が深さの上限の実行時エラーで止まる (2 と 8 worker でも。子の evv に IO の evidence がないため)
    - U-5 `tests/ui/run/async/limit_depth_nested_run.em`: タスクの中の入れ子の `Async.run` の本体が再帰し、外のタスクで捕まえる
    - U-6 `tests/ui/run/async/limit_depth_repeated.em`: 1つのタスクが3回続けて捕まえ、`1 2 3` を出す
    - U-7 `tests/ui/run/async/limit_depth_in_shield.em`: `Async.shield` の中の再帰が起こし、`shield` の `reraise` が外の `try_all` に届ける
    - U-8 `tests/ui/run/async/limit_depth_after_cancel.em`: スコープに取り消されたタスクが `try_all` で `Cancelled` を受けて出し、その後で再帰して `ResourceLimit` を受ける
  - harness のテスト: `the_max_depth_mark_sets_the_limit`、形の違う印の panic、トランスクリプトでの拒否
- 実装者に任せること: 4096 でどれかの実行の底が収まらなければ、すべての印を 8192 にする
- 条件: いつも
- 完了の条件:
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::)'` → すべて通り、新しい snapshot は新しいファイルのものだけ
  - `cargo nextest run --profile timing -j 1` → `SLOW` が出ない

### T-7: `eml_rt` の数と閾値、CLI の既定

- 目的: worker ごとの数と柔らかい上限の閾値を `eml_rt` に置く (D-11、D-12)
- 対象: `crates/eml_rt/src/control.rs`、`alloc.rs`、`heap/mod.rs`、`heap/ctx.rs`、`heap/tests/control.rs`、`heap/tests/trusted.rs`、`crates/eml_cli/src/lib.rs`、`crates/eml_cli/tests/api.rs`、docs/spec/object-model.md、vm.md
- 作業:
  - 付録の `eml_rt-prototype.diff` から始める (`git apply docs/plans/2026-11-04-task-limits/eml_rt-prototype.diff`)。試作の中身: `Limits.soft_bytes`、`Control.taken` (検証済みの `byte_counters` の輪で作る)、`Control.next`、`Pages.slot` と `Pages::new(.., slot)`、`fresh_page` が `charge_pages(&self.control, self.slot, bytes)` を呼ぶこと、`Heap::for_worker` と非公開の `with_slot` (`with_control(c)` は `with_slot(c, 0)`)、`empty_core(control, slot)`、`Heap::fresh` (heap/mod.rs:842) と freeze の道 (heap/mod.rs:1853) が slot を受け継ぐこと、`Ctx::for_worker` が `Heap::for_worker(control.clone(), k)` を使うこと
  - その上で `impl Control` (安全、`Relaxed`) を D-12 の形にする

    ```rust
    const MIN_GAP: usize = 1 << 16;
    pub(crate) fn charge(&self, slot: usize, bytes: usize) {
        let old = self.bytes.fetch_add(bytes, Ordering::Relaxed);
        let total = old.saturating_add(bytes);
        if let Some(t) = self.taken.get(slot) { t.fetch_add(bytes, Ordering::Relaxed); }
        if total > self.limits.max_bytes { self.raise(Self::MEMORY); return; }
        let next = self.next.load(Ordering::Relaxed);
        if old <= next && next < total && slot < self.words.len() { self.notice(slot); }
    }
    pub fn taken(&self, k: usize) -> usize                     // 範囲の外は 0
    fn rearm(&self, b: usize) -> usize {
        let soft = self.limits.soft_bytes;
        if b < soft { soft }
        else { b.saturating_add(MIN_GAP.max(self.limits.max_bytes.saturating_sub(b) / 4)) }
    }
    /// bytes > next なら Some((next, bytes))。そうでなく bytes < soft < next なら next を soft に戻す (CAS)
    pub fn pressure(&self) -> Option<(usize, usize)>
    /// next を rearm(bytes) へ CAS で進める。ほかが先に動かしていれば false
    pub fn claim_pressure(&self, next: usize, bytes: usize) -> bool
    // clear(): さらに next.store(self.rearm(self.bytes()))
    ```

    `soft_bytes == usize::MAX` なら `rearm` はいつも `usize::MAX`、`pressure()` はいつも `None` になる
  - `Limits.soft_bytes` の doc: 「確保量の柔らかい上限。超えると、確保の多いタスクに `ResourceLimit` を起こさせる。`usize::MAX` なら置かない」
  - `eml_cli`: `default_limits()` に `soft_bytes: soft_of(max_bytes)`。`repl_limits()` は自分の `max_bytes` を求めてから同じく求める
  - spec: object-model.md の「実行の制御」(`Limits` の欄、`taken`、`next`、越えたときの `NOTICE`、`rearm`、`clear`)、「確保と解放」(worker ごとの数、`Pages.slot`)、「`unsafe` の核」(1292 行目の `verus!` の外の一覧に「worker ごとの取った量と柔らかい上限の閾値」を足し、T1 の行に `charge_pages` が課金した worker の数も足し、その語に `NOTICE` を立てうることを書く)。vm.md の「上限と割り込み」の表に `soft_bytes` = `max_bytes` の 7/8 の行
- テストの変更: 種類3 (値はバイト単位で同じ): `heap/tests/control.rs` の `control.charge(..)` の3か所 (109, 112, 317 行目) に slot `0`。`alloc.rs` のテストの補助 `new_pages` (約 3419 行目) に `Pages::new(.., 0)`。ほかの `Limits` の literal はどれも `..Limits::default()` なので変えない
- 新しいテスト:
  - `heap/tests/control.rs`: R-1 `crossing_the_soft_limit_notices_only_the_charging_worker_once` (workers 2、max 1 MiB、soft 128 KiB。`charge(1, 64 KiB)` を2回では両方の語が 0、3回目で語 1 にだけ `NOTICE`、`take_notice(1)` の後の課金では立たない)。R-2 `the_soft_limit_never_stops_the_run` (越えた後も `stop_bits() == 0`)。R-3 `taken_counts_each_worker_and_ignores_refunds`。R-4 `claim_pressure_moves_by_a_quarter_of_the_headroom` (`next == b + (1 MiB - b)/4`、古い claim は失敗、新しい `next` を越えるまで `pressure()` は `None`)。R-5 `the_hard_limit_wins_over_the_soft_limit` (両方を1回で越えると `MEMORY` だけ)。R-6 `clear_rearms_a_gap_above_the_current_bytes` (柔らかい上限より下なら `next == soft`)。R-7 `pressure_rearms_at_soft_when_bytes_fall_below_it`。`Limits::default()` では `pressure()` が `None`
  - `heap/tests/trusted.rs`: A-1 `a_worker_heap_charges_its_own_slot` (両方のモード。`Ctx::for_worker(control, host, 2)` で大きい物体を確保すると `taken(2) >= 64 KiB`、`taken(0) == 0`)
  - `crates/eml_cli/tests/api.rs`: `default_limits_put_the_soft_limit_at_seven_eighths` (run と REPL)
- 実装者に任せること: 付録の diff が当たらなくなっていたら、上の中身の一覧に従って書き直す
- 条件: いつも
- 完了の条件:
  - `cargo verus verify -p eml_rt` → `eml_rt` の `verification results` が `0 errors`
  - `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` → 終了コード 0
  - `cargo nextest run` (ワークスペース全体) → すべて通り、`git status --porcelain` に `.snap.new` も変わった `.snap` もない
  - `cargo clippy --all-targets -- -D warnings && cargo fmt --check` → 終了コード 0
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::` → 通る。`-Zmiri-tree-borrows` を足してもう一度 → 通る
  - `git diff 837b0fe9 -- crates/eml_rt/src/heap/ctx.rs` → `fn poll` の中に変更がない

### T-8: 数えることと公開すること

- 目的: タスクごとの数を切り替えの地点で帰属し、lock なしで公開する (D-11)
- 対象: `crates/eml_vm/src/workers.rs`、`vm.rs`、`sched.rs`、`cells.rs`、docs/spec/vm.md
- 作業:
  - `workers.rs` の `Slot` に `current`、`bytes`、`mark`、`top`、`top_bytes` を足し、`Run::new` で `NO_CELL`、0、0、`NO_CELL`、0 にする
  - `Task` に `counted: usize` と `added: usize` (どの構築子でも 0)
  - `Scheduler` に `mark: usize` と `yielded: bool`、補助を足す
    - `end_slice(&mut self, task)`: `now = control.taken(k)`。`task.cell.is_some()` なら `d = now.saturating_sub(self.mark)` を `counted` と `added` に足す。`self.mark = now`
    - `publish(&self, task)`: `task.cell` が `Some(c)` で状態が `TaskState::Entry` でなければ、`current = NO_CELL`、`bytes = task.counted`、`mark = self.mark`、`current = c.encode() as u64` の順に書く。そうでなければ `current = NO_CELL`。変わらない書き込みは手元の写しで飛ばす
    - `offer_top(&self, c, count)`: `top == NO_CELL || top == c || count >= top_bytes` なら `top_bytes`、`top` の順に書く
    - `clear_top(&self, c)`: 自分の `top` を `c` から `NO_CELL` へ CAS
  - `resume` (vm.rs:1608): `switch_task` の前に `sched.end_slice(task)`。`prev.cell == Some(c)` で `keep == Some(TaskState::Ready)` なら `offer_top(c, prev.counted)`。`set_current` の後で `sched.yielded = false; publish(task)`。入るタスクが `Some(c)` で `Entry` でなければ `clear_top(c)`
  - `enter` (vm.rs:3072): `started`/`resumed` が T-4 の `limit` の隣に `bytes` を返す (`TaskMarks { limit, bytes }`)。`task.counted = bytes; task.added = 0; sched.mark = taken(k); sched.publish(task); sched.clear_top(cell)`
  - `enqueue` (vm.rs:2954): `Resume`/`ResumeIo` のセルがこの worker の公開した `current` と同じなら、`offer_top(cell, bytes + taken(k) - mark)` と `sched.yielded = true`
  - `entry_finish`: `sched.end_slice(task)`。結果の道では `clear_top(cell)`。待ちの道では `left` に `(cell, task.added)` を積み、`!sched.yielded` なら `clear_top(cell)`
  - `switch_away`: `pick_ready` が `None` を返したら (`idle_wait` の前に) `top = NO_CELL`
  - 救済: sched.rs の `steal_ready` の呼び手で、盗んだ `Resume`/`ResumeIo` のセル `c` ごとに `run.slots[victim].top` を `c` から `NO_CELL` へ CAS
  - `cells.rs`: `TaskCell.bytes: usize` (持ち主だけの種類)。`left: Vec<(CellRef, usize)>` にし、`leave` は生きたセルに `bytes = bytes.saturating_add(added)`。`clear_limit` は `bytes = 0` もする
  - T-4 の `limit_hit` の手順5で、`was_pending` なら `clear_limit` の後に `task.counted = 0; task.added = 0; sched.mark = taken(k); sched.publish(task)`
  - vm.md の「セルと登録表」(`bytes`、持ち主だけ) と「タスクとスケジュール」(worker が何をいつ公開するか、`top` を消す地点)
- テストの変更: 種類3: `cells.rs` のテストの `.started(` 4か所、`.resumed(` 8か所を `TaskMarks` に合わせ、`.leave(` 8か所 (8074-8115 行目) に `(cell, 0)` を渡す。`sched.rs` のテストの `.started(` 4か所、`.resumed(` 4か所、`.leave(` も同じく。2460-2466 行目の `left` の列を `(cell, 0)` にする
- 新しいテスト: V-1 `end_slice_adds_the_slice_to_counted_and_added`、V-2 `a_preempted_task_is_offered_as_top_and_cleared_when_resumed`、V-3 `a_self_yield_offers_top_and_a_real_wait_clears_it`、V-4 `leave_adds_the_bytes_and_resumed_returns_them`、V-5 `clear_limit_zeroes_the_bytes`
- 実装者に任せること: 補助の名前
- 条件: いつも
- 完了の条件:
  - `cargo nextest run -p eml_vm` → すべて通り、`crates/eml_vm/tests/bench.rs` の snapshot はバイト単位で同じ
  - x86_64 Linux なら `nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=thread TSAN_OPTIONS=halt_on_error=1 CARGO_TARGET_DIR=target/tsan cargo test -Zbuild-std --target x86_64-unknown-linux-gnu -p eml_vm --test integration -- workers:: preempt::` → 通る (macOS なら飛ばし、報告に書く)
  - `cargo clippy --all-targets -- -D warnings && cargo fmt --check` → 終了コード 0

### T-9: 選ぶことと、旗を立てる引き金

- 目的: 柔らかい上限を越えたら、数のいちばん大きいタスクに旗を立てる (D-5、D-12)
- 対象: `crates/eml_vm/src/monitor.rs`、`sched.rs`、`crates/eml_vm/tests/memory.rs` (新しいファイル)、`crates/eml_vm/tests/main.rs`、docs/spec/vm.md
- 作業:
  - `monitor.rs` に足す

    ```rust
    pub(crate) fn choose(run: &Run, taken: impl Fn(usize) -> usize) -> Option<CellRef>
    pub(crate) fn pressure_tick(control: &Control, shared: &Shared, run: &Run) -> Option<CellRef> {
        let (next, bytes) = control.pressure()?;
        let cell = choose(run, |k| control.taken(k))?;
        if !control.claim_pressure(next, bytes) { return None; }
        shared.mail.push(MailItem::Limit(cell));
        for k in 0..run.slots.len() { control.notice(k) }
        Some(cell)
    }
    ```

    `choose` は D-12 の読み方と順位で選ぶ
  - `Monitor::start(.., pressure: bool)`。`watch_loop` は tick ごとに、横取りと救済の隣で `if pressure { pressure_tick(..) }` を1回呼ぶ。`bell_timeout` の最初の引数を `preempt || pressure` にする
  - `sched.rs`: `pub enum Memory { Watched, Gathered }`。`Policy::MEMORY` は `Normal` で `Watched`、ほかで `Gathered`。`start_monitor` は `P::MEMORY == Memory::Watched` を渡す。`gather` で `take_notice(k)` の直後、最初の `shared.lock()` の前に `if P::MEMORY == Memory::Gathered { pressure_tick(control, shared, run); }`。積んだ mail は同じ gather で取る
  - spec: vm.md の「タスクとスケジュール」(監視のスレッドの務め。`Gathered` は前提からの例外としてそう書く)、「上限と割り込み」(柔らかい上限、閾値の規則、run を止めないこと)、T-4 の「タスクの上限」にメモリの部分と残る危険 RS1〜RS6 (「10. 危険と未決の項目」)
- テストの変更: 種類3: `monitor.rs` のテストの `Monitor::start(` 2か所 (565, 604 行目) に `false`。`bell_timeout` の assert (553-555) は変えない
- 新しいテスト:
  - 単体: V-6 `choose_picks_the_largest_count` (`current` と `top`、同点の順位、全部 0 なら `None`、`NO_CELL` は飛ばす)。V-7 `pressure_tick_flags_once_per_threshold` (soft 64 KiB、max 64 MiB。worker 0 の `Ctx` で 200 KiB 確保し、1回の tick で `MailItem::Limit(cell)` と全語の `NOTICE`。2回目の tick は何もしない。`rearm` を越えるとまた立てる)。V-8 `pressure_tick_without_a_candidate_keeps_the_threshold`。V-9 `only_normal_watches_memory`。V-10 `the_monitor_thread_flags_the_published_task` (本番の道。monitor.rs:592-616 の形。slot 1つ、doorbell なしの `Run` で slot 0 が `current = cell` を公開し、worker 0 の `Ctx` で柔らかい上限を越える。`Monitor::start(.., false, false, true)` で始め、最大 5 秒 (1 ms ずつ) `shared.mail.take(0)` が `Limit(cell)` を出し語 0 に `NOTICE` が立つのを待ち、`monitor.stop()`。`bell_timeout(true, 1, tick)` も確かめる)。V-11 `a_gathered_policy_flags_in_gather` (`Scheduler<Sequential>` の用意で、始めたセルを `current` に公開し、柔らかい上限を越えて `collect` → セルの旗が立ち、`next` が進んでいる)
  - 統合 `crates/eml_vm/tests/memory.rs` (`tests/main.rs` に `mod memory;`。`eml_test_support::lir` で組み、明示した `Limits` で `Sched::Sequential`、checked mode、特に書かなければ 1 と 2 worker):
    - P-1 `a_left_child_allocation_counts_to_its_task`: タスクの中の `par` の左が list を伸ばし、`ResourceLimit: the memory soft limit is exceeded` を出す (max 64 MiB、soft 2 MiB)
    - P-2 `a_right_child_allocation_counts_to_no_task`: 右が伸ばし、run が `VmFault::Stopped(Stop::MemoryLimit)` で止まる (max 16 MiB、soft 2 MiB)。文でなく variant を確かめる
    - P-3 `a_compute_job_counts_to_no_task`: `Async.compute` が伸ばし、P-2 と同じ結果
    - P-4 `a_task_in_a_nested_run_is_flagged`: 入れ子の `Async.run` のタスクが伸ばし、外のタスクの `try_io` が出す
    - P-5 `a_task_moved_by_rescue_is_flagged`: `Sched::RescueAlways`、2 worker、`yield_now` の輪。文を出し、`finished.rescued > 0`
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `cargo nextest run -p eml_vm -E 'test(monitor::) | test(sched::) | test(cells::) | test(memory::)'` → すべて通る
  - `cargo nextest run` → すべて通る
  - macOS なら `bench/run.sh` → `fib` と `empty` の命令の数が docs/implementation/benchmarks.md の値の揺れの範囲 (macOS でなければ T-11 に回す)
  - `cargo nextest run --profile timing -j 1` → `SLOW` が出ない

### T-9a (改訂で足した): EP の補助の関数のエントリでは起こさない

- 目的: 補助の関数のエントリで起こした失敗がプログラムに見えない handler に届くのを止め、どのエントリでも硬い限度を確かめる (D-1、D-3、D-4)。T-4 のコードを直す別のコミットで、T-10 の作業ツリーの変更は入れない
- 対象: `crates/eml_lir/src/lib.rs`、`crates/eml_core_ir/src/evidence/helpers.rs`、`evidence/mod.rs`、`crates/eml_vm/src/code.rs`、`compile/emit.rs`、`disasm.rs`、`vm.rs`、`crates/eml_vm/tests/memory.rs`、`crates/eml_vm/tests/codegen.rs`、docs/spec/vm.md、core-ir.md、stdlib.md
- 作業の前に (T-10 の作業ツリーの変更をよける手順。T-9a が直す stdlib.md:1072 の隣の 1073 行目に T-10 の変更があり、`git add -p` は使えない):
  1. `git diff -- docs/spec/stdlib.md > <scratchpad>/u10-stdlib.patch` (3つの hunk。T-10 の分)
  2. `git checkout -- docs/spec/stdlib.md`
  3. T-9a と T-9b では、`git add` に自分のファイルの path を並べて書き、`git add -A` と `git add -u` と `git commit -a` を使わない。コミットの前に `git diff --cached --stat` がその作業のファイルだけを挙げ、`git status --porcelain` に T-10 の ` M crates/eml_cli/tests/common/mod.rs`、` M crates/eml_cli/tests/ui.rs`、` M docs/implementation/testing.md` と `??` の 10 個のファイルが残っていることを確かめる
  4. T-9b のコミットの後に `git apply --3way <scratchpad>/u10-stdlib.patch` で戻し、`git diff -- docs/spec/stdlib.md` に T-10 の3つの hunk が戻っていることを確かめる。`--3way` が失敗したら `git apply --reject` で当て、`.rej` に残った 1073 行目の1項目を手で書き写す
  T-10 の harness と `.em` と snapshot はそのまま置いておく。T-9a と T-9b のテストはそれらと一緒に流れるが、その状態で通れば T-10 を足した状態でも通る (T-10 は harness と UI テストの入力しか変えない)
- 作業 (`file:line` は HEAD 36169ec9):
  - `crates/eml_lir/src/lib.rs` の `IO_RESOURCE_LIMIT` (1030 行目) の隣に、EP の補助の関数の名前の定数 (`EP_ID = "id$"`、`EP_COMPOSE = "compose$"`、`EP_PROMPT = "prompt$"`、`EP_RESUME = "resume$"`、`EP_REWIND = "rewind$"`、`EP_MASK_PREFIX = "mask$["`) と `pub fn ep_helper(name: &str) -> bool` (5つとの一致か、`EP_MASK_PREFIX` で始まること) を置く。doc コメントは core-ir.md の「EP が作る補助の関数」と vm.md の「タスクの上限」を引く。`eml_core_ir` の `evidence/helpers.rs` (21、41、135、181、204、209 行目) と `evidence/mod.rs` (73、80、89、102、111 行目) の名前のリテラルをこの定数にする (種類3。出力はバイト単位で同じ)。`eml_lir` は葉の crate なので、EP と VM と後のネイティブが同じ1組を使える
  - `code.rs:40` の `Unwind.entry: bool` を `entry: UnwindEntry` にし、`#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub(crate) enum UnwindEntry { No, Raises, Defers }` を隣に置く。名前を `Entry` や `EntryKind` にしない。どちらも `workers.rs` にあり、`vm.rs` が import している。巻き戻し (`vm.rs:1399` の `unwind`) はこの欄を読まないので、「エントリの行は所有している引数を手放す」読み方は変わらない
  - `compile/emit.rs` の `entry_unwind` (`emit.rs:408`) に `defers: bool` を足して `UnwindEntry::Defers` か `Raises` を書き、`function()` (`emit.rs:47`) は `eml_lir::ep_helper(&function.name)` を渡す。呼び出しの行 (`emit.rs:400`) は `UnwindEntry::No`。`emit.rs:64` の `debug_assert!` は `!= UnwindEntry::No` で比べる
  - `disasm.rs:23` と `disasm.rs:394` の `entry.entry` の読みを `UnwindEntry::No` との比較にする。出力は変えない (codegen の snapshot を変えないため)
  - `vm.rs:657` からの `limit_hit` を次の順にする。手順 1 (`Stops`/`Aborting` は止める) と 2 (yield の途中は保留) はそのまま
    3. `code.tables.unwind_at(task.pc)` を `row.entry != UnwindEntry::No` で絞り、なければ `Ok(())` (保留)
    3'. `real = real_limit(..)`。`task.stack_depth() > real.saturating_add(LIMIT_HEADROOM)` なら `Err(VmFault::Stopped(Stop::DepthLimit))` (硬い限度)
    3''. `row.entry == UnwindEntry::Defers` なら `Ok(())` (保留)。状態も `depth_limit` も変えない
    4〜7 はそのまま。`functions` は走査しない
  - spec: vm.md の「呼び出しと戻り」(153 行目。起こすエントリから補助の関数を除く)、「巻き戻しの表」(行の種類 `UnwindEntry` と、エントリの行が起こすかの区別)、「タスクの上限」の `LIMIT_HEADROOM` の項 (882 行目。どの状態でもエントリで `real + LIMIT_HEADROOM` を超えたら止める) と `limit_hit` の手順 (894 行目の 3 を上の 3、3'、3'' にする。補助の関数のエントリの `evv` がプログラムの地点のものでない理由を、`prompt$`、`resume$`/`mask$`、`compose$`/`rewind$`/`id$` の3つに分けて書き、core-ir.md を引く)、「IO の evidence のない所」の隣に「節のエントリで起こしたとき」(perform した所のいちばん内側の `try_io` に届く。D-4)、「残る危険」(907 行目。R8: 再開は利用者のフレーム1段ごとに `compose$` のフレームを積み直すので、再開の後の深さは yield の時点の深さにフレームの大きさの比を掛けたほどになる。猶予を超えると run の誤りで止まり、`real` と猶予の間なら κ のいちばん内側の `f$kN` で `the call depth limit is exceeded` になる)。「節のエントリで起こしたとき」には、利用者の `never` の操作の節と `reperform$E.op` と `compute` の仕事の起こし直しのエントリで、保留の旗が進行中の失敗に取って代わること (R9) も書く。core-ir.md の「EP が作る補助の関数」の表の下に1文 (この関数のエントリの `evv` はプログラムの地点の `evv` でなく、VM のタスクの上限はそこでは起こさない。vm.md の「タスクの上限」を引く)。stdlib.md:1072 (「失敗は関数に入るところでだけ起きる」に、処理系の補助の関数に入るところでは起きず次の関数で起きることを足す)
- テスト (TDD。先に書いて落ちることを見る):
  - 単体 (`vm.rs` のテストの module。T-A〜T-F と同じ `limit_fixture` と手で書いた LIR。LIR の text は `fn "compose$"(..)` のように引用した名前を受け付ける。`crates/eml_vm/tests/ir/mask.lir:271` に例がある):
    - T-J `a_hit_at_a_helper_entry_stays_pending`: `main` が `"compose$"` を呼び、`"compose$"` が `"prompt$"` を呼ぶ LIR。`Pending` で `pc` を `"compose$"` の先頭にして `limit_hit` → `Ok(())`、状態は `Pending`、`depth_limit == 0`。`"prompt$"` の先頭でも同じ。`Task` で `pc` を `"compose$"` の先頭、`stack_depth > real` (`DEPTH` を小さくするか `depth_offset` で作る) でも `Ok(())`、状態は `Task`
    - T-K `a_helper_entry_beyond_the_headroom_stops_the_run`: `Task` で `"compose$"` の先頭、`stack_depth > real + LIMIT_HEADROOM` → `Err(Stopped(DepthLimit))`。`Pending` でも同じ
    - T-L `only_the_ep_helpers_defer`: 関数 `"id$"`、`"compose$"`、`"prompt$"`、`"resume$"`、`"rewind$"`、`"mask$[State]"`、`"mask$[Yield@1]"`、`f`、`"$cont$@[X]"`、`"fork$E.op"`、`"compose$x"` を持つ LIR で、`unwind_at(start).entry` が `Defers` なのは最初の7つだけで、ほかは `Raises`
    - 既存の `row.entry` を読む4か所 (`vm.rs:4522, 4550, 5003, 5029`) を `!= UnwindEntry::No` か `== UnwindEntry::No` に合わせる (種類3)
  - 統合:
    - T-L2 `crates/eml_vm/tests/codegen.rs::the_ep_helpers_are_compiled_under_their_names`: `tests/ui/run/effects/mask_multi.em` (`mask` を含む区間を持つ `multi` の継続) の text を `eml_test_support::lir` で組み、`compile(..).functions()` の名前に `id$`、`compose$`、`prompt$`、`resume$`、`rewind$`、`mask$[Name]` の6つがあること (scratchpad の replan で、このファイルがちょうどこの6つを作ることを確かめた。`mask_behind_multi.em` なら `mask$[Choice]` と `mask$[Choice@1]` の2つになる) と、それ以外に `id$`、`compose$`、`prompt$`、`resume$`、`rewind$`、`mask$[` を含む名前 (`compose$$uniform` のような `eml_lir::variants` の写し) がないことを確かめる。写しができると `Raises` に分類されて、この不具合が戻るためである。T-L と合わせて、EP の名前と VM の判定を結ぶ
    - T-M `crates/eml_vm/tests/memory.rs::a_flag_delivered_to_a_parked_task_is_caught_by_its_own_try_io` (`Sched::Sequential`、1 worker、max 64 MiB、soft 2 MiB、`program(..)` の補助を使う)。数の大きいタスク `a` が `yield_now` で列に戻って `top` に出ている間に、数の小さいタスク `b` が閾値を越えるので、旗は `Parked` の `a` のセルに立ち、再開の後に `a` の `try_io` が捕まえる。今は `a` の失敗が底の `try_all` に届き、`Async.run` が `run_result` の位置で `ResourceLimit` を起こし直して出力が `b caught: Cancelled: task was cancelled\n` だけになる (scratchpad の replan で確かめた)。直した後の出力は `a caught: ResourceLimit: the memory soft limit is exceeded\nb ok 80000\ndone\n` で、結果は `Ok`。プログラム:

      ```
      hold : Int -> List Int -> <IO, Panic, Async> Int
      hold n acc =
        if n == 0 then count acc 0
        else
          Async.yield_now ()
          hold (n - 1) (1 :: acc)

      churn : Int -> List Int -> <IO, Panic, Async> Int
      churn n acc =
        if n == 0 then count acc 0
        else
          Async.yield_now ()
          churn (n - 1) (build 200 acc)

      body : Unit -> <IO, Panic, Async> Unit
      body () =
        let a = Async.start (fn () ->
          match try_io (fn () -> hold 400 (build 40000 [])) with
            | Ok n -> println "a ok \{show n}"
            | Err e -> println "a caught: \{show e.kind}: \{e}")
        let b = Async.start (fn () ->
          match try_io (fn () -> churn 400 []) with
            | Ok n -> println "b ok \{show n}"
            | Err e -> println "b caught: \{show e.kind}: \{e}")
        Async.await a
        Async.await b
        println "done"

      main : Unit -> <IO, Panic> Unit
      main () = Async.run body
      ```
- 実装者に任せること: `UnwindEntry` の変種の名前。T-J〜T-L の LIR の形
- 条件: いつも
- 完了の条件:
  - `cargo nextest run -p eml_vm` → すべて通る (T-J〜T-M を含む)
  - `cargo nextest run` (ワークスペース全体) → すべて通り、`git status --porcelain` に `.snap.new` がなく、` M` の `.snap` がない (T-10 の `??` の5つの snapshot は別。disasm の出力を変えないので、codegen の snapshot は変わらない)
  - `cargo nextest run -p eml_core_ir` → すべて通る (名前の定数への置き換えで出力が変わらないこと)
  - `for i in $(seq 50); do EML_UI_FILTER=limit_memory cargo nextest run -p eml_cli --test integration -E 'test(ui::run) | test(ui::run_fail)' || { echo "failed at run $i"; exit 1; }; done; echo "50 runs passed"` → 最後の行が `50 runs passed` で、終了コード 0 (T-10 の作業ツリーの M-1〜M-5 を、速いモードの `Normal` を含めて流す)。1回が数秒なので 10 分を超えうる。`run_in_background` で流すか、`seq 1 25` と `seq 26 50` に分けて流す
  - `cargo clippy --all-targets -- -D warnings && cargo fmt --check` → 終了コード 0
  - `eml_rt` は触らないので、Verus の確かめは要らない
- 落とし穴: `limit_hit` の 3' (硬い限度) は 3'' (`Defers`) より前に置く。後に置くと、補助の関数のエントリで深さが猶予を超えても止まらない。`Pending` では `depth_limit` が 0 なので、どの安全点も `limit_hit` に来る。そのため 3' は `Pending` の間、補助の関数のエントリでも効く

### T-9b (改訂で足した): 候補のないときに閾値を進める

- 目的: 候補のないまま柔らかい上限を越えた後に増え始めたタスクにも、決定的な方針で旗が立つようにする (D-12、D-13。U-9 のレビューの B-2)
- 対象: `crates/eml_vm/src/monitor.rs`、`sched.rs`、`vm.rs` (テストだけ)、`crates/eml_vm/tests/memory.rs`、docs/spec/vm.md
- 作業 (`file:line` は HEAD 36169ec9):
  - `monitor.rs:224` の `pressure_tick` に引数 `retry: bool` を足す。`choose` が `None` のとき、`retry` なら `control.claim_pressure(next, bytes)` を呼んでから `None` を返す (旗は送らず、`NOTICE` も立てない)。`retry` でなければ今までどおり閾値を進めない。doc コメントを D-12 の改訂の内容にする
  - 呼び手: `monitor.rs:295` (監視のスレッド) は `false`、`sched.rs:901` (`gather`) は `true`、`monitor.rs` のテスト 5 か所 (705〜734 行目) は `false` (種類3)。`gather` は `NOTICE` を見た集める地点かを区別しない (`polled_stop` が `NOTICE` を先に下ろすので区別できず、D-12 のとおり絞らない)
  - `eml_rt` は変えない。`claim_pressure` と `rearm` は T-7 のままである
  - spec: vm.md の「上限と割り込み」(375 行目の「候補のタスクがなければ旗は立たず、確保が続けば硬い上限 `max_bytes` で run を止める」を、`Normal` では次の刻みで探し直し、決定的な方針では閾値を同じ幅だけ進めて次に越えた所で探し直す、に直す)、「タスクとスケジュール」の `MEMORY` の段 (405 行目。worker が1つの決定的な run で旗の地点がスケジュールによらないことの条件に、候補のないまま越えた場合を足す)、「タスクの上限」の「メモリの残る危険」(906 行目。候補のないまま越えた分は次の閾値まで旗が遅れる)
- テスト (TDD):
  - 単体 `monitor.rs`: V-8 `pressure_tick_without_a_candidate_keeps_the_threshold` は `retry: false` で残す。V-12 `a_gathered_tick_without_a_candidate_moves_the_threshold` を足す。`pressured()` で 200 KiB 持ち、`pressure_tick(.., true)` → `None`、mail は空、`control.pressure()` は `None` (閾値が `rearm` の分だけ進んだ)。その後セルを `current` に公開し、閾値を越えるまで確保すると `pressure_tick(.., true)` → `Some(cell)`
  - 単体 `vm.rs`: `an_entry_that_waits_is_not_chosen_in_the_gather_of_its_switch` (`vm.rs:4940`) の `assert!(control.pressure().is_some(), "a waiting task is chosen")` を、旗が立たないことの assert (`sched.cells.mail.take(0)` に `Limit` の項がなく、`sched.limited` が空) に替え、doc コメント (4937〜4938 行目) の「候補がないので閾値も進まない」を「候補がないので旗は立たず、閾値は進む」にする (種類2。理由: 決定的な方針では候補のないとき閾値が進むので、閾値で見ると偽になる。確かめたいのは待ちのタスクが選ばれないことである)
  - 統合 `crates/eml_vm/tests/memory.rs`:
    - P-7 `a_task_that_grows_without_switching_after_a_crossing_without_a_candidate_is_flagged` (`Sched::Sequential`、1 worker、max 16 MiB、soft 2 MiB)。`main` がタスクの前に `build 100000 []` で越え (候補なし)、`Async.run` の中で始めたタスクが `try_io` の下で切り替えずに `grow` する。今は `Stopped(MemoryLimit)` で run が止まり出力は空 (scratchpad の replan で確かめた)。直した後の出力は `caught: ResourceLimit: the memory soft limit is exceeded\nheld 100000\n` で、結果は `Ok`。プログラム:

      ```
      body : List Int -> <IO, Panic, Async> Unit
      body held =
        let t = Async.start (fn () ->
          match try_io (fn () -> count (grow 0 []) 0) with
            | Ok n -> println "ok \{show n}"
            | Err e -> println "caught: \{show e.kind}: \{e}")
        Async.await t
        println "held \{show (count held 0)}"

      main : Unit -> <IO, Panic> Unit
      main () =
        let held = build 100000 []
        Async.run (fn () -> body held)
      ```
    - `a_parent_waiting_on_its_join_is_not_chosen` (`memory.rs:219`) を書き直す (種類2)。今の形は `main` がタスクの前に越えるので、T-9b の後は `main` の集める地点で閾値が約 17 MiB 先に進み、親が待ちに入る集める地点では `pressure()` が `None` になって、待つ親が候補になるかの道を通らない。越えるのを `par` の右の子の中に移し、親が join で待っている間に越えるようにする。右の子は親の worker で動き、セルを持たないので候補にならず、待つ親は公開を外されている。そのため旗はどこにも立たず、閾値だけが進み、親は join の後に確保せずに値を返す。待つ親を候補にする退行があれば、親に旗が立って join の後の最初のエントリで `ResourceLimit` になり、タスクの失敗がスコープの失敗になって出力が変わる。期待する出力は `ok 105001\n` で結果は `Ok` (scratchpad の replan で、T-9a と T-9b を当てた写しで通ることを確かめた)。doc コメントもこの説明にする。プログラム:

      ```
      body : Unit -> <IO, Panic, Async> Int
      body () =
        let t = Async.start (fn () ->
          let mine = build 5000 []
          let (a, b) = Par.par (fn () -> 1) (fn () -> count (build 100000 []) 0)
          count mine (a + b))
        Async.await t

      main : Unit -> <IO, Panic> Unit
      main () =
        let n = Async.run body
        println "ok \{show n}"
      ```
- 実装者に任せること: `retry` の名前と、V-12 の確保の量
- 条件: いつも
- 完了の条件:
  - `cargo nextest run -p eml_vm` → すべて通る (V-12、P-7 を含む)
  - `cargo nextest run` (ワークスペース全体) → すべて通り、`git status --porcelain` に `.snap.new` がなく、` M` の `.snap` がない (T-10 の `??` の5つは別)
  - 2 と 8 worker の決定的な run の確かめ: `EML_UI_FILTER=limit_memory cargo nextest run -p eml_cli --test integration -E 'test(ui::run) | test(ui::run_fail)'` を 5 回 (`for i in $(seq 5); do … || { echo "failed at run $i"; exit 1; }; done; echo "5 runs passed"`) と、`cargo nextest run -p eml_vm --test integration -E 'test(a_task_moved_by_rescue_is_flagged)'` を 5 回、どれも通り、`SLOW` が出ない (D-12 の救済の途中の窓が、閾値を使い切って時間を伸ばしていないこと)
  - `cargo clippy --all-targets -- -D warnings && cargo fmt --check` → 終了コード 0
  - T-9b のコミットの後に、T-9a の「作業の前に」の手順 4 で stdlib.md の T-10 の変更を戻す
- 落とし穴: `retry` で進めるとき、`claim_pressure` が偽 (ほかの worker が先に進めた) でも何もしなくてよい。閾値はもう進んでいる。M-5 (`run/limits/soft_limit_without_a_task.em`) はタスクのない run なので、`main` の確保のたびに閾値が進むだけで、出力も `RunStats` も変わらない

### T-10 (改訂): 印 `-- max-bytes:` とメモリの UI テスト

- 目的: メモリの柔らかい上限を UI テストで端から端まで確かめる (D-7、D-13)
- 対象: `crates/eml_cli/tests/ui.rs`、`crates/eml_cli/tests/common/mod.rs`、`tests/ui/`、docs/implementation/testing.md、docs/spec/stdlib.md
- 状態: 作業は作業ツリーにある (印、harness のテスト、M-1〜M-5 とその snapshot、testing.md と stdlib.md の編集)。T-9a と T-9b のコミットの後に、stdlib.md の変更を戻してから (T-9a の「作業の前に」の手順 4)、そのまま1つのコミットにする。速いモードで間欠的に落ちた原因は T-9a で直すので、テストのファイルと snapshot は変えない
- 作業:
  - 印 `-- max-bytes: <bytes> soft <bytes>` (正の10進の整数で soft < bytes)。先頭の印の塊で `-- signal:` と `-- max-depth:` の隣に置き、すべての実行に `Limits { workers, max_bytes, soft_bytes, ..default_limits() }` で当てる (`-- max-depth:` があれば一緒に)。形が違えば ``the max-bytes mark is `-- max-bytes: <bytes> soft <bytes>`, not `{line}` `` で panic。`-- max-depth:` と `-- max-bytes:` を重ねて書いたら panic する (U-6 のレビュー)。REPL のトランスクリプトでは断る
  - testing.md の「UI テスト」に印と、`limits/` の分類と、`async/` のメモリのテストの書き方を書く。stdlib.md の「`Std.Async`」と「実行時エラー」にメモリの柔らかい上限を書く
  - UI テスト (印 `-- max-bytes: 268435456 soft 4194304`。出すのは種類と文の文字列だけ。`run/async/` のファイルは、増える前に `spin 40` だけの spawn したタスクを2つ置き、原因のタスクはその後に始め、大きい物体は作らない。原因のタスクは `grow n acc = Async.yield_now (); grow (n + 1) (build 1000 acc)` で、1回の `yield_now` ごとに 1000 個の cons を積む。checked mode では `yield_now` 1回の確保 (約 14 回) も数に入るので、1 cons ずつでは増え方が `yield_now` の輪と変わらない (U-9 のレビューの B-1)。`ResourceLimit` の行は原因のタスク自身の catch だけが出し、旗を取り違えると出力が変わる):
    - M-1 `tests/ui/run/async/limit_memory_caught.em`: `grow` を `try_io` の下で動かし、`ResourceLimit: the memory soft limit is exceeded`、続いて `main` が `done`
    - M-2 `tests/ui/run/async/limit_memory_fails_scope.em`: `run: ResourceLimit: the memory soft limit is exceeded`
    - M-3 `tests/ui/run-fail/async/limit_memory_uncaught.em`: `before` の後に `run_result` の位置 (`<std>/Async.em:75:9`) の実行時エラー (insta が固定)
    - M-4 `tests/ui/run/async/limit_memory_after_catch.em`: 捕まえた後で 1000 個の list を作って `1000` を出し、`main` が `done`。`rearm` の下では2回目の旗が立たない
    - M-5 `tests/ui/run/limits/soft_limit_without_a_task.em` (新しい分類): `main` が 300000 個の cons を作って長さを出す。タスクのない run は柔らかい上限で止まらない
  - harness のテスト: `the_max_bytes_mark_sets_both_limits`、形の違う印の panic (soft がない、soft >= bytes、数でない)、重ねた印の panic、トランスクリプトでの拒否
- 実装者に任せること: どれかのファイルが 1 秒を超えたら、hard を 128 MiB に下げる (余裕は約 4 秒)
- 条件: いつも
- 完了の条件:
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::)'` → すべて通り、新しい snapshot は新しい5つのファイルのものだけ
  - `cargo nextest run --profile timing -j 1` → `SLOW` が出ない
  - `for i in $(seq 20); do EML_UI_FILTER=limit_memory cargo nextest run -p eml_cli --test integration -E 'test(ui::run) | test(ui::run_fail)' || { echo "failed at run $i"; exit 1; }; done; echo "20 runs passed"` → 最後の行が `20 runs passed` で、終了コード 0 (`run_in_background` で流す)。改訂の前の `-E 'test(ui::) & test(limit_memory)'` は、テストの名前が `ui::run` と `ui::run_fail` なので何にも当たらず、何も流していなかった
  - `cargo clippy --all-targets -- -D warnings && cargo fmt --check` → 終了コード 0

### T-11 (一部改訂): `Close task-limits`

- 目的: 段を閉じる (docs/future/roadmap.md の「進め方」の手順)
- 作業: roadmap の「進め方」の手順1〜9をその順に行い、1つのコミット `Close task-limits` にする。この段で特に直すもの:
  - `spec/` に段の名前と「今は〜、後で〜」が残っていないか確かめる
  - 計測: `bench/scripts.sh`、`bench/run.sh`、`bench/rt.sh` を最後のコードのコミットの作業ツリーで流し、benchmarks.md の「今の数値」を入れ替える (macOS でなければ `Measure task-limits` の別のコミット)
  - status.md の範囲と制限
  - docs/future/async.md:104-112 と multicore.md:170-172 を、実装した形 (worker ごとの数を切り替えで帰属、閾値の規則、テストの方針での `Gathered`) に合わせるか、spec へ移った所を消す
  - native.md:73-75 と 81-88 に D-6 (`limit_class`、毒、猶予、補助の関数ごとの冷たいブロック、本当の限度を持つ2つ目の `#[repr(C)]` の欄) と、ネイティブのスケジューラが同じ slot の値を公開することを書く
  - roadmap からこの段の項目を消し、「その後の項目」に RS1、RS2、RS6、RS7、U3、R7、R8 (下の表) を「後に回した理由」と「見直す条件」(柔らかい上限が実際に違うタスクを選んだとき、など) を添えて書く。`jit-effects` の「前提」の `task-limits` を項目の名前に直す
  - overview.md の表と README.md の「文書の地図」を、変わった所だけ直す
  - CLAUDE.md は、規則か道しるべが変わったときだけ直す (UI テストの印を CLAUDE.md の Testing に足すかを確かめる)
  - `docs/plans/2026-11-04-task-limits.md` と `docs/plans/2026-11-04-task-limits/` を消す。消す前に、この文書の決めがすべて `docs/` に残っていることを確かめる
  - コミットのメッセージに、テストの変更の種類と理由 (下の一覧)、計測の目立つ差、後に回した項目の決めた日 (2026-11-04) を書く
- 完了の条件:
  - `cargo nextest run` → すべて通る
  - `cargo clippy --all-targets -- -D warnings && cargo fmt --check` → 終了コード 0
  - `grep -rn "task-limits" docs CLAUDE.md` → `docs/reports/` のほかに出ない
  - `ls docs/plans` → この段のファイルがない

## 8. 実行の順番

```
T-1 → T-2 ─┐
T-3 ───────┼→ T-4 → T-5 ─┐
           │        └→ T-6 ┼→ T-10 → T-11
T-3 → T-7 ─────────────────┤
           T-5 + T-7 → T-8 → T-9 → T-9a → T-9b ┘
```

- この文書は段の最初のコミットで入れる (roadmap の「進め方」)
- T-3 と T-7 は `eml_rt` だけを触るので、T-1 と T-2 と並べられる。ただし T-7 は T-3 の後 (同じ `ctx.rs` を触る)
- T-6 と T-5 は並べられる
- T-8 は T-5 (`TaskMarks`、`clear_limit`) と T-7 (`Control.taken`) の両方の後
- T-10 は T-6 の印の読み方と T-9 の引き金の後。改訂で、T-9 の後に T-9a (起こす地点の直し) と T-9b (閾値の直し) を挟み、T-10 はその後にした。T-10 の作業ツリーの変更はそのまま残し、T-9a と T-9b のコミットには入れない

テストの変更の一覧 (コミットのメッセージに書く):
- 種類1: T-1 の5つの削除
- 種類2: T-1 の `crates/eml_lir/tests/unwind.rs` の補助、T-2 の codegen の5つの snapshot と `bench.rs::unwind_points`
- 種類2 (改訂で足した): T-9b の `an_entry_that_waits_is_not_chosen_in_the_gather_of_its_switch` の assert と doc コメント (閾値が進まないこと → 旗が立たないこと)、`memory.rs::a_parent_waiting_on_its_join_is_not_chosen` の書き直し (越えるのを右の子の中に移す)
- 種類3: T-7、T-8、T-9 に挙げた呼び出しの形の追随。T-9a の `row.entry` を読むテストと `eml_core_ir` の名前のリテラルの定数への置き換え、T-9b の `pressure_tick` の呼び出しの追随

## 9. 確かめ方

- 基準 (最初の変更の前): `cargo nextest run` → 基準のコミットですべて通る。通らないテストがあれば、作業を始める前に記録して利用者に知らせる
- 全体の確かめ (すべての作業の後):
  - `cargo nextest run` → すべて通る
  - `cargo nextest run --profile timing -j 1` → `SLOW` が出ない
  - `cargo clippy --all-targets -- -D warnings && cargo fmt --check` → 終了コード 0
  - `cargo clippy -p eml_cli --no-default-features --features types -- -D warnings` と、CLAUDE.md の feature の組み合わせの clippy → 終了コード 0
  - `cargo verus verify -p eml_rt` → `0 errors`。`verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` → 終了コード 0
  - Miri: CLAUDE.md の4つの Miri のコマンド (2つの設定で) → 通る
- 利用者が見るもの:
  - `eml run` で、接続ごとのタスクの1つが深く再帰するサーバのような小さいプログラムを動かし、そのタスクだけが `ResourceLimit` で終わり、ほかが続くことを見る (速いモードの本番の道。既定の `max_depth` は大きいので、再帰の深さを十分大きくする)

## 10. 危険と未決の項目

| ID | 項目 | 影響 | 決め方と時期 | 決まらないときの既定 |
|---|---|---|---|---|
| Q-1 | 決定的な方針では worker が旗を立てる (D-12)。D-5 の「監視のスレッドが立てる」からの例外である | T-9 | 決定済み (2026-11-04、利用者が承認) | 例外を受け入れる |
| R1 | 取り消しと上限の順: 先に来た地点が先に巻き戻す。旗の立ったタスクが取り消されると、`Cancelled` でなく `ResourceLimit` (スコープの失敗) で終わりうる | 利用者に見える結果 | 受け入れる。vm.md の「タスクの上限」に書く (T-4) | - |
| R2 | IO の evidence のない所 (`start_task`/`bottom` の `try_all` の外、`try_all` が返った後、`par` の子の中) で見た旗は、捨てずに `Pending` のまま残し、evidence のある所の次のエントリで起こす。保留の間は安全点ごとに冷たい道を通る | 遅れと、保留の間の費用 | 受け入れる。vm.md の「タスクの上限」に書く (T-4) | - |
| R7 | タスクの中でも `par` の子の深さの超過は run を止める (子の evv に IO の evidence がない) | サーバの中の並列の計算は守られない | roadmap の「その後の項目」に書く (T-11)。見直す条件: タスクの中の `par` で深さの上限に届く使い方が出てきたとき | - |
| U3 | 右の子が別の worker で動く親への旗は、合流の後に親で起きる (子の `Task` はセルを持たない)。子の深さの超過は子で起きる | メモリの旗が遅れる | roadmap の「その後の項目」に書く (T-11) | - |
| RS1 | 本当の待ちから起きてまだ動いていないタスクは、動くまで候補にならない。待ちのタスクは候補にならない (前提) | 選び方 | roadmap に書く (T-11) | - |
| RS2 | `par` の右の子と `compute` の仕事はどのタスクにも数えない。左の子は数える。P-2 と P-3 が固定する | そのような増え方は硬い上限に届く | roadmap に書く (T-11) | - |
| RS3 | 数は区間の中のランタイムの仕事と、checked mode では使い回さないごみを含み、起こすときのほかは減らない。昔多く取ったタスクは今少ししか持たなくてもいちばん大きいままである。越えたかは正味の `Control.bytes` で見て、候補は単調な数で選ぶ | 違うタスクを選びうる | vm.md に書く (T-9) | - |
| RS4 | 旗は失われうる (古いセル、R2)。次の閾値まで1つの幅の分だけ増える | 遅れ | vm.md に書く (T-9) | - |
| RS5 | `Normal` で監視のスレッドを始められなければ、柔らかい上限はない。硬い上限は残る | 保護がない | vm.md に書く (T-9) | - |
| RS6 | `top` は worker ごとに1つ。消えた後、小さい実行待ちのタスクはもう一度 CPU を離れるまで候補にならない | 選び方 | roadmap に書く (T-11) | - |
| A-1 | checked mode では1つのヒープのページの数が大きさの種類ごとの合計だけで決まる (推定)。崩れると D-13 の seed の `RunStats` の一致が崩れる | T-10 | T-10 の UI テストが確かめる | 崩れたら、そのファイルの増える前の段階の確保を直す (D-13) |
| R8 (改訂で足した) | 再開は捕まえた利用者のフレーム1段ごとに `compose$` のフレームを積み直すので、再開の後の深さは、yield の時点の深さに `compose$` のフレームと利用者のフレームの大きさの比を掛けたほどになる (比は測っていない)。`real + LIMIT_HEADROOM` を超えると、タスクの失敗でなく run の誤りで止まる。`real` と `real + LIMIT_HEADROOM` の間なら、κ のいちばん内側の `f$kN` で `the call depth limit is exceeded` の `ResourceLimit` になり、論理的な深さが上限に届いていなくても利用者の `try_io` に見える | run が止まる。見かけの深さの超過 | 受け入れる。`LIMIT_HEADROOM` は変えない。vm.md の「残る危険」に比と2つの現れ方を書く (T-9a)。roadmap に書く (T-11) | - |
| R9 (改訂で足した) | 節のエントリの安全点で起きた失敗は、handler の外側でなく perform した所のいちばん内側の `try_io` に届く。利用者の `never` の操作の節のエントリ、`reperform$E.op` のエントリ、`compute` の仕事の失敗を起こし直すコードのエントリでも同じで、保留の旗が進行中の失敗 (操作、子の失敗) に取って代わる (R1 と同じ順の競合)。どの着地も生きていて矛盾はないが、仕様で決めておく | 利用者に見える結果 | vm.md の「タスクの上限」に書く (T-9a) | - |
| RS7 (改訂で足した) | 同じセルのエントリの `Task` が2つ重なると、次のエントリは前のエントリの `top` を消し、まだ `leave` していない数を含まずに数え始める (U-8 のレビューの B-3)。選び方が偏るだけで、旗は生きたセルへ行く | 選び方 | vm.md の「残る危険」に書いてある (T-9)。roadmap に書く (T-11) | - |
| B-2 (改訂で足した) | 候補のないまま閾値を越えると、決定的な方針では切り替えないタスクに旗が立たず、硬い上限で run が止まる (U-9 のレビューの B-2) | checked では硬い上限で止まり、速いモードでは捕まえる | T-9b で、決定的な方針でも旗が立つようにする。旗の地点は `Normal` と違ったまま (D-13) | - |

## 付録: レビューで変わった主なこと

蒸し返さないために必要な分だけを書く。

- 猶予を `usize::MAX` にすると硬い限度がなくなる → 受け入れ、`real + LIMIT_HEADROOM` にし、`Aborting` の間の超過は run を止める (D-3)
- フレームを持ったまま別の worker で待つタスクに旗が届かない → 退けた。そのタスクは `Phase::Running` のままで、持ち主へ回って `sched.tasks` から見つかる (D-5)
- 監視のスレッドが `CellRef` と持ち主を lock なしに知れない → 受け入れ、mail と `NOTICE` の手順と、worker が公開する値を決めた (D-5、D-11)
- エントリで起こすとき、どの行を巻き戻しが見るか → 起こす道を T-4 に書いた
- すべての呼び出しに行を足すと std で `VerifyError` が出うる → 退けた。所有の `Checker` は既にすべての関数に流れている。ただし T-2 の完了の条件をワークスペース全体にした
- 柔らかい上限の旗を 4回までに限ると、長く動くサーバが保護を失う → 受け入れ、残りの余裕の 1/4 で進め、柔らかい上限の下に戻れば元に戻す閾値にした (D-12)
- 決定的な方針の worker 側の旗は前提からの例外 → 受け入れ、例外として書き、本番の道に V-10 を足した (Q-1)
- 決定性の約束が広すぎる → 受け入れ、D-13 の3つに絞った
- 帰属が試されていない → 受け入れ、P-1〜P-5 を足した
- 毎 tick 送り直さないことは S1 の修復の手段 (送り直し) に反する → 退けた。小さいページは返らないので、送り直すと tick ごとにタスクを止める。閾値ごとの送り直しで同じ修復をする
- `RunStats` に欄を足さない → 足すと bench の snapshot がすべて変わり、要るテストがない

改訂 (2026-11-04、T-10 の途中) で変わったこと:

- 補助の関数 (`id$`、`compose$`、`prompt$`、`resume$`、`rewind$`、`mask$[..]`) のエントリの `evv` は、プログラムのどの地点の `evv` でもない。T-10 の速いモードで、待ちの中のタスクのセルに立った旗が再開の `rewind$` のエントリで起き、底の `try_all` に届いた → 受け入れ、D-1 を改訂した。6つの名前のエントリでは旗でも本当の超過でも起こさず、どのエントリでも `real + LIMIT_HEADROOM` の硬い限度を確かめる (T-9a)。旗だけ延ばす案は、本当の超過も同じ所に届くので退けた
- この事例は `Sched::Sequential` でも決まって起こせる。数の大きいタスクが `yield_now` で列に戻って `top` に出ている間に、数の小さいタスクが閾値を越えると、旗は `Parked` のセルに立つ → T-M をこの形にし、`Normal` の繰り返しは補助の確かめにした
- 候補のないまま閾値を越えると、決定的な方針では切り替えないタスクに旗が立たない (U-9 の B-2) → 受け入れ、`gather` の `pressure_tick` が候補のないとき旗を送らずに閾値を `rearm(bytes)` に進めることにした (T-9b)。閾値を今の確保量に戻す形は、次のページを最初に取った数の小さいタスク (`Async.run` の本体) に旗が立つので退けた
- T-10 の完了の条件の filter `test(ui::) & test(limit_memory)` はテストの名前 (`ui::run`、`ui::run_fail`) に当たらず、何も流していなかった → `EML_UI_FILTER=limit_memory` と `test(ui::run) | test(ui::run_fail)` に直した
- `Entry` と `EntryKind` は `workers.rs` にあり、`vm.rs` が import している → 行の種類の名前を `UnwindEntry` にした
- 同じセルのエントリの `Task` が重なるときの数の偏り (U-8 の B-3) は、vm.md の「残る危険」に書いてある → 受け入れ、RS7 として roadmap に回す
- 節のエントリの安全点で起こした失敗の届き先 (perform した所の `try_io`) は、どちらの `try_io` も生きていて矛盾はないが仕様に決めがなかった → vm.md に書く (T-9a)

2回目のレビュー (2026-11-04) で変わったこと:

- T-9b は checked と速いモードの食い違いをなくさない。候補のないまま越えた後、checked は次の閾値で、速いモードは次の刻みで旗を立てる → 受け入れ、D-13 と B-2 の行にそう書いた
- `never` の節のエントリに `Aborting` の間にしか来ないのは、VM が積む `io_failed` と `panicked` だけで、利用者の `never` の操作の節は `Clause` で呼ばれる起こすエントリである。`reperform$E.op` も子でなく親で動く → 受け入れ、D-1 と D-4 の文を直し、R9 に含めた
- R8 の条件「上限の近く」は狭すぎる。再開の後の深さは yield の時点の深さにフレームの大きさの比を掛けたほどで、猶予の内側でも最初の `f$kN` で見かけの深さの超過になる → 受け入れ、R8 と vm.md に書くことにした
- `a_parent_waiting_on_its_join_is_not_chosen` は T-9b の後は待つ親が候補になるかの道を通らない → 受け入れ、越えるのを右の子の中に移した形 (scratchpad で通ることを確かめた) に書き直す (種類2)
- T-9a の stdlib.md:1072 の隣に T-10 の変更があり、対話なしに分けて stage できない → 受け入れ、T-10 の stdlib.md の変更だけを patch によけ、path を並べた `git add` で stage し、T-9b の後に `git apply --3way` で戻す手順を T-9a に書いた
- `|| break` の繰り返しは失敗しても終了コードが 0 になる → 受け入れ、`exit 1` と最後の行で確かめる形にし、background で流すと書いた
- 名前の一覧を `compile/emit.rs` に置くと EP と二重になる → 受け入れ、`eml_lir` の定数と `ep_helper` に1組だけ置き、EP も VM も使う形にした。T-L2 には `$uniform` の写しがないことの確かめを足した
- 候補のない窓で、ほかの worker の集める地点が閾値を使い切る (C-1) → 一部受け入れ。窓は認めて D-12 と RS4 に書き、2 と 8 worker の繰り返しを T-9b の完了の条件に足した。進める地点を `NOTICE` を見た集める地点に絞る案は退けた。`polled_stop` が `NOTICE` を先に下ろすので `gather` では見分けられず (試した写しでは B-2 のテストがまた落ちた)、印の配管を足しても、越えるページをほかのタスクが取る窓は閉じない。遅れても出力は変わらず、checked mode で 1 秒に満たない
- ネイティブの補助の関数の冷たいブロックが `Stops` でも猶予まで進む → 受け入れ、`limit_class` を先に見る順を D-6 に書いた
