# `blocking` の C の呼び出しとプールの仕事

- 作成日: 2026-11-05
- 基点: main の 8d6a813a (`Merge pull request #31 from ecofighter/plan/ffi-c`。`Close ffi-c` の後)。「3. 背景」と、この文書のファイルの行番号は、この木を指す
- 状態: 計画は確定。決めていない細部は 10 章に既定とともに置く
- 付録: 「付録 A: レビューで変わったこと」「付録 B: ffi-c の後の木に合わせて変えたこと」「付録 C: r1 のレビューで変えたこと」 (コードの断片は 5 章と 7 章に入れてある)

この計画は docs/future/roadmap.md の「段の列」にある `ffi-blocking` の段の設計と計画である。roadmap の「進め方」に従い、この文書を段の最初のコミットで入れ、段を閉じるときに消す。コードと `docs/` はこの文書を引かない。

## 1. 目的と成功の基準

- 目的
  - 利用者が `extern "C"` の行に `blocking` を書けるようにする。その C の呼び出しは、タスクを置いた worker の driver のプールのスレッドで動く。呼んだタスクは完了まで止まり、同じ worker のほかのタスクは動き続ける。
  - プールの仕事 (`Op::Job`) が、所有する Rust の値を返せるようにする。次の段 `tls-platform-verifier` は、この道で `Conn` を返す。
  - 戻らない仕事を、run の終わりに1回の予算 (既定は合わせて 1 秒) まで待ち、それでも走っていれば見捨てる。見捨てたときは記憶を放さない。`Machine` は poisoned になってヒープを漏らし、run は終わる。roadmap の「その後の項目」にある「プールの仕事の予算と、放さずに終わる方針」はこれで決まる。
  - VM から `Op::Job` の仕事までを、テストの driver と本物の driver の両方で通すテストを足す (roadmap のこの段の項目が求めている)。
- 成功の基準 (確かめ方は 9 章)
  - 9 章の全体の検証のコマンドがすべて通り、CI が4つの OS と Miri、ASan、TSan の job で、やり直しなしに緑になる。
  - 5.1 の D-1 から D-13 の「適合の確かめ」がすべて成り立つ。
  - roadmap から `ffi-blocking` の行と、「プールの仕事の予算と、放さずに終わる方針」の項目が消えている。

## 2. 範囲

- 含むもの
  - `Op::Job` が値を返す形 (`Done::Job`) と、`eml_io` のプールの2つの列 (D-3、D-7、D-21、D-22)
  - `quiesce` の予算と、仕事を見捨てる規則 (D-8、D-23)
  - VM の run ごとの期限、`exit` の終了コード、poisoned の公開の API (D-8、D-20、D-24)
  - `blocking` の行の下ろし方、行の見え方、VM の2つの命令 (D-1、D-2、D-4、D-5、D-6、D-25 から D-28)
  - 別のスレッドが書いた生の配列の Miri のテスト (D-9)
  - 鍵の fixture と、本物の時計、見捨て、REPL、UI のテスト (D-10、D-11、D-30、D-31)
  - REPL が poisoned の入力の直後に終わること (D-29)
  - `docs/future` の書き直しと、段を閉じる作業
- 含まないもの
  - `tls-platform-verifier` の `Conn` をプールに載せること: 次の段で扱う。この段は、仕事が Rust の値を所有して返せる道 (D-3、D-12) を用意するだけである
  - 走っている C の呼び出しを止めること: 仕事は今のまま取り消せない (D-4)
  - 仮想の時計でほかのタスクを待つ C の関数の診断: しない。そうしたテストは `-- clock: real` で書く (D-10)
  - プールの上限 24/8 と予算 1 秒を測って決め直すこと: 「その後の項目」に書く (D-7、D-8、T-4)
  - プールの仕事を数える `RunStats` の項目: 足さない。完了は今の `completions` が数える (Q-4)
  - `eml_cli` から予算を変える手段: 出さない (D-8)
  - ネイティブのコードから `blocking` を呼ぶこと: `jit-core` 以降の段で扱う

## 3. 背景

行番号は 8d6a813a のものである。`crates/eml_rt/src/driver.rs`、`crates/eml_io/` の全体、`crates/eml_vm/src/{cells,sched,driver}.rs`、`std/`、`crates/eml_core_ir/src/optimize/reach.rs` は、ffi-c の段で変わっていない。

- driver の型 (`crates/eml_rt/src/driver.rs`)。ファイル全体が `verus!` の外にある (:1-4)。
  - `Op::Job(Box<dyn FnOnce() -> io::Result<usize> + Send + 'static>)` (:65)。結果は `Done::Size(usize)` で届く。
  - `Done` は `#[derive(Debug, PartialEq, Eq)]` (:75)。`Submit` は `#[derive(Debug)]` で `Done(io::Result<Done>)` を持つ (:196-203)。だから `Done` は `Debug` を持ち続ける必要がある。`Completion` (:205-212) は derive を持たない。
  - `fn quiesce(&mut self)` (:274) は引数を持たない。`fn now(&self) -> Duration` (:233)。
  - `RawBuf` を使ってよいスレッドの規則は :29-35 にあり、ファイルの仕事のプールのスレッドも使い手に入っている。
- `eml_io` のプール (`crates/eml_io/src/driver/`)
  - `POOL_LIMIT: usize = 32` (pool.rs:15)。プールは driver ごとに1つで、`Pool::new(Arc::clone(&inbox), POOL_LIMIT)` (mod.rs:254) で作る。`Driver::new(backend, lookup)` (mod.rs:251)。列は `State.queue` の1本である (pool.rs:25-36)。
  - `pub(crate) type Job = Box<dyn FnOnce() -> io::Result<usize> + Send + 'static>` (backend.rs:13)。ファイルの仕事も同じ型の closure で、`Backend::file_job` が作る (unix.rs:953、iocp.rs:1226、fake.rs:338)。
  - `submit` (pool.rs:63-107) は列に積むだけで止まらない。スレッドを作るのは `queue.len() > idle && threads < limit` のときだけである。スレッドを作れなかった仕事は `Answer::Job(Ok(Err(e)))` で1回だけ終わる (:103)。
  - `work` (pool.rs:111-140) は `catch_unwind(AssertUnwindSafe(job))` で走らせ、`running` を外す lock の区間の後で答えを積み、lock を取り直して次を取る。空いたスレッドは `POOL_IDLE` (10 秒) で抜ける。
  - 答えは `answer_arrived` (mod.rs:553-) が引き取る。札 (`tickets`) のない答えは捨てる (:560)。`Answer::Job(Ok(result))` は `result.map(Done::Size)` の完了になる (:567)。
  - 仕事は取り消せない (mod.rs:828-838)。`quiesce` (mod.rs:938-985) は名前の解決を手放し、ほかの操作を取り消し、slot が空になるまで上限なしに待つ。待ちを切るのは、OS に出した取り消しの `CANCEL_LIMIT` だけである。
  - `Drop` は `Op::Job` を待たず、ファイルの仕事は `HELD_LIMIT` (1 秒、mod.rs:39) まで待って、終わらなければ abort する (mod.rs:990-)。`Pool::drop` (pool.rs:143-) は列の仕事を走らせずに捨て、走っているスレッドを切り離す。
  - `Kind::Job` を欄なしで照合する所が mod.rs:140、148、244、811、834 にある。
  - fake の backend: `ring_while_threads_run` の欄 (fake.rs:149) は、鳴らされるまで待つ (:432-435)。スレッドが走る間の時間つきの `wait` は assert で落ちる (:437-440)。
  - プールのスレッドは unix で `spawn_masked` で作り、SIGINT と SIGTERM を止める (unix.rs:851)。Windows は同じ関数を通るが何も止めない (iocp.rs:1159)。テストは `real::pool_threads_block_interrupt_and_terminate` (real.rs:512、`#[cfg(unix)]`)。
  - `real::a_job_pool_is_capped` (real.rs:187) が 32 本の上限を確かめる。`driver::tests::pool` は driver/tests/mod.rs:1071 にあり、`Pool::new(Arc::clone(&inbox), 2)` (:1089) で作る。
  - `Done` を `assert_eq!` で比べる所は1行のものと複数行のものがある。数は grep の書き方で変わるので、T-1 はコンパイラの示す所を直す。`Done::` を書くファイルは `eml_io` の `tests/{real,test_driver,tls,tls_off}.rs`、`src/test.rs`、`src/test/net.rs`、`src/driver/{mod,tests/mod}.rs`、`eml_vm` の `tests/{io,machine}.rs`、`src/{cells,sched}.rs` である。
  - `Op::Job(` を書くテストは `driver/tests/mod.rs` 8、`tests/real.rs` 9、`tests/test_driver.rs` 1 である。
- テストの driver (`crates/eml_io/src/test.rs`) は `Op::Job` を `submit` で積み、`wait` の決着のときにその場で動かす (:177 `Op::Job(job) => done(job().map(Done::Size))`、:371-377)。`quiesce` (:448-) は積んだ仕事を `let _ = job();` で動かしてから捨てる (:452)。`thread::spawn` を使わない。
- VM で `Op::Job` を作るコードはない。docs/implementation/status.md:156、architecture.md:376、testing.md:284 は「`Op::Job` を使うのは Rust のテストだけ」の旨を書いている。
- VM の終わり方 (`crates/eml_vm/src/lib.rs`)
  - `quiesce` を呼ぶ所は3つ: `recover` (:516-536) の中 (:520)、worker 0 が join した後 (:1055)、worker `k` (`k` ≥ 1) の `retire_driver` (workers.rs:1252-1265、呼ぶのは :1256)。worker `k` は自分のスレッドで静止させ (workers.rs:1040)、worker 0 は join の後に静止させる。
  - `kept_pending` の枝 (:663-677) は `self.poisoned = true` にし、`ctx.flush()` の後に誤りを返す。ヒープは漏らす。
  - `exit` の道 (:697-724) は `Finished { … }` (:713-722) を作って `Ran::Abandoned` を返す (:724)。成功の道の `Finished { … }` は :743-752 にある。`run_recovering` (:495-512) は `Ran::Abandoned` を `recover` に通し (:505)、`recover` は poisoned なら `poisoned_fault("<recover>")` を返す (:526)。だから `Ran::Abandoned` のままでは `exit` の終了コードが消える。`enum Ran` は非公開である (:864)。
  - `end_check` は成功した run でだけ走る (:1078)。
  - poisoned の `Machine` では、`load` が `CompileError::Poisoned` を返し (:457-459)、`run_entry` が断り (:565-567)、`run_recovering` は `recover` を呼ばず (:507)、`close` は `poisoned_fault("<close>")` を返す (:821-824)。文言は `the driver kept pending ops after quiesce` である (error.rs:28 の `KEPT_PENDING`)。`Machine` の `poisoned` は非公開の欄 (:328) で、`is_poisoned` という公開の関数はない。
  - `execute_here` (:280-292) は `machine.run(loaded.entry)?` の後に `machine.close()?` を呼ぶ。`execute(program, linked, config, host)` は :254。
  - run の表は `Run::new(workers, start.doorbell, start.run_clock, start.counters)` (:931) で作り、`Arc::new` する。
  - `RunConfig` (:180-199) は `#[non_exhaustive]` で、pub の欄 `debug_heap`、`control`、`sched`、`clock`、`doorbell`、`signals` を持つ。
  - `begin_run` は `DriverKind::Injected` の driver では worker を使わせない (sched.rs:793-806)。そのため、2 worker の見捨てのテストには本物の driver が要る。
  - 本物の時計の run の driver は、どれも同じ時間の基点から作る。`Run::new_driver` (workers.rs:430-435) が `self.clock.driver(None)` を呼び、`RunClock::driver` (driver.rs:59-66) が `RealDriver::new_at(*epoch)` (:62) で作る。`Scheduler::own_driver` (sched.rs:829-840) は、本物の時計の worker `k` がまだ driver を持たなければ作る。
- `Std.Async` (`std/Async.em`)
  - `pub abstract effect Async` (:11)。抽象のエフェクトは、宣言したモジュールの外では perform も handle もできない (E1058、docs/spec/effects.md:101)。だから `Async` の handler は `Async.run` だけである。
  - `Async.run` は `ctl await_io op k -> park_io op k` で止める (:25、:58)。
  - `io_wait : OpId -> <IO, Async> (Wake, OpId)` は `pub` で、`op_ready` の速い道、予算、取り消しを持つ (:304-311)。`Wake` は `Woke | Cancelled` (:4-7) で、Repr は `Enum` である (`RuntimeEntry::ResumeTask` の引数が `[TObj, Enum]`)。タグは `eml_lir::WAKE_WOKE`/`WAKE_CANCELLED` (eml_lir/src/lib.rs:1055-1056) と cells.rs の `WOKE`/`CANCELLED` (:69、:71) で、vm.rs:46-48 の const assert が両者を結ぶ。`OpId` は `Lin` の `Int` である (eml_extern/src/lib.rs:262)。
  - 取り消しの IO の失敗は、std の包みが `fail_io Prelude.Cancelled "task was cancelled"` で起こす (std/Net.em:243 ほか)。
- `crates/eml_vm/src/cells.rs`
  - `IO_CANCELLED = 5` (:73)、`OpKind` (:415)、`prefix` (:474)、`cost` の既定の枝は 1 (:524-533)、`OpPhase::Done(io::Result<usize>, bool)` (:536-)、`Registry::point` (非公開、:2136-2144)、`op_ready` (:3313)、`advance` (:3489)、`take_op` (:3764、:3794)、`submit_*` の公開の関数 (:3924-4210、`#[allow(clippy::too_many_arguments)]`)、非公開の `submit(shared, here, ctx, driver, depth_offset, record, op)` (:4240-4262)、`net_record` (:4430)。
  - 流れを持たない記録の host は `here.host` (= `Run::io_worker(k)`) である (:4249-4252)。
  - `OpRecord.cancelling` がある。cells.rs は `std` と `eml_rt` だけを import する (:63-)。
  - `NoDriver` (cells の単体テスト) は :5921 にあり、`fn quiesce(&mut self) {}` は :6034 である。
- `crates/eml_vm/src/vm.rs`
  - `here!` (:33-43) は vm.rs の中の `macro_rules!` で、ffi.rs からは見えない。`Op::Ffi` の dispatch は `step` (:918) の中の :1176 で、`sched` を持たない `ffi::call` を呼ぶ。`step` は `sched` を持つ。
  - `io_kind` (:3418) と `failed` (:3824) と `current_cell` (:3441) は非公開である。`IO_FAILED` (:3760) も非公開で、`PANICKED` (:3761) は `pub(crate)`。文言の IO の失敗は `Fault::Raised(Box::new(Raised { kind, message }))` を `failed` に渡して起こす (`raise_again`、:3466-3471)。
  - `Std.Net` の受け付けの腕は `sched.own_driver()?`、`here!(sched, post, here)`、`cells::submit_*(&sched.cells, &here, ctx, sched.driver.as_deref_mut(), task.worker, task.depth_offset, …)` の形である (:3080-3110)。
- Core IR (`crates/eml_core_ir/src`)
  - `Stmt::Unpack` がある (lib.rs:270)。直接の呼び出しが `Obj` の組を返し、その後に `Unpack` を置く前例は `display` である (`translate/program.rs:729-750`)。
  - `ProgramBuilder` (program.rs) は `c_functions` (:103) と `fn row(&self, ext)` (:138) を持つ。`extern_wrapper` (:537-575) は `builder.finish(name, true, false, …)` で yields を `false` にする (:571)。`extern_call` (:583-623) は `self.row(ext).with_args.is_empty()` で bind か `lending_call` (:626-643) を選ぶ。`extern_call` は `BodyCtx` (expr.rs:462、:1289) と `extern_wrapper` から呼ばれるので、`ProgramBuilder` の上で完結する必要がある。
  - `blocking` の下ろし方と同じ形の前例が std にある。`Std.Net.listen` は translate の後に `extern Std.Net.submit_listen` → `call Std.Async.io_wait(t)` → `unpack t (,) #0(w: enum, op: int)` になり、`io_wait` は `yields fn Std.Async.io_wait(op.0: int) -> (,){enum, int} lin` である (scratch の複製で `Pass::Translate` の表示を見て確かめた)。組 `(Wake, OpId)` は構造体の Repr で、型から決める形も `tuple_struct` も `StructShape { layout: tuple_layout(2), fields }` を intern するので、同じ fields なら同じ `StructId` になる (program.rs:266-286、translate/types.rs:18-36)。
  - evidence passing は evidence を実行時にエフェクトの番号で探し、関数に evidence の引数を足さない (docs/spec/core-ir.md の「evidence passing」)。scratch の複製で `RuntimeEntry::IoWait` を `ALL` に足すと、`Pass::Evidence` の後の `io_wait` は引数 `[Int]`、`split == None`、`internal == false` で、結果は `Struct(_)` (箱の形では `Obj`) だった。
  - `RuntimeEntry` の関数は、translate の instance の収集で根になり (`translate/instances.rs:837-847`、`collector.add(function, vec![], vec![])`)、`retain_reachable` でも名前で根になる (`optimize/reach.rs:13-24`)。instance の `FnIdx` は translate の `indices` (translate/mod.rs:325-341) で決まる。`Instances` (instances.rs:167-177) は `RuntimeEntry` の instance を覚えていない。
  - `par` の右の子と `compute` の仕事の row は `never` で抑えられ、`IO` を含まない (docs/spec/stdlib.md:973)。
  - `lowering::every_program_lowers_to_the_same_text` (`crates/eml_core_ir/tests/lowering.rs:16`) は `tests/ui` のすべてのプログラムを、Core IR と LIR の表示が同じになるか確かめる。
- `eml_extern`
  - `RuntimeEntry` (lib.rs:2232-2268) は `StartTask`、`ResumeTask`、`ResumeIo` で、`ALL`、`name()`、`arity()`、`params()` を持つ。doc は「結果はどれも `Unit` である」と書き、`ret` はない。`#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]`。
  - `externs::every_runtime_entry_is_a_monomorphic_std_function` (`crates/eml_core_ir/tests/externs.rs:738`) と `externs::the_runtime_entries_have_the_declared_abi` (:779) は、どの entry の結果も `Repr::Unit` であることを確かめる。
  - VM の `async_entries` (`crates/eml_vm/src/compile/mod.rs:303-339`) は3つの entry を名前で引き、引けないロードが `Async` の命令を持てば `CompileError::MissingEntry` にする。
  - CI の「dependency edges of eml_native」の step (ci.yml:94-101) は、`crates/eml_extern/src` に `unsafe` の語がないこと (コメントも含む) を確かめる。
- REPL (`crates/eml_cli/src/terminal.rs`、`crates/eml_cli/src/repl.rs`)
  - `enum Flow { Continue, Quit, Broken, Interrupted }` (terminal.rs:254-260)。
  - 主のループ (:86-111) は `Flow::Interrupted` で `lines.interrupt()` を呼んで続け、`Flow::Quit` で `terminal.close(ExitCode::SUCCESS)`、`Flow::Broken` では `close` を呼ばずに 1 で終わる (:109)。`Read::Eof` の枝は `Flow::Continue | Flow::Interrupted` を `Flow::Quit` にし、ほかの flow はそのまま通す (:95-98)。
  - `show` (:301-324) は `Outcome::Interrupted` と、割り込みの `Outcome::Failed` で `Flow::Interrupted` を返す。`Outcome::Unlinked` は `link error:` を書く (:317)。
  - `Terminal::close` (:242-251) は `self.repl.close().leaks` の誤りを `runtime error:` で書いて 1 で終わる。`steps` (:264-298) は `Continue` でない flow で止まる。
  - 起動のときの `load_text` (:74-78) は `Flow::Broken` だけを見る。
  - `Repl` (repl.rs) は `broken` の欄 (:41) と `linker: Linker` を持つ。`Repl::new(config, source, host)` (:124)、`with_linker` (:162)、`eval` (:171)、`close(self) -> Closed { leaks }` (:309)。`Outcome` (:97-114) は `Exited(i64)`、`Failed`、`Unlinked`、`Broken` などを持つ。
  - `crates/eml_cli/tests/terminal.rs` は `eml` のバイナリを動かすテストだけで、そのバイナリは fixture の記号を持たない。
  - トランスクリプトの runner (`crates/eml_cli/tests/repl.rs`) は `fn linker() -> Linker` (:19-22) で、`unsafe { Linker::new(Vec::new()).with_symbols(&eml_test_support::fixture_symbols()) }` を SAFETY のコメントとともに作る。
- 文書
  - docs/future/roadmap.md:31 に `ffi-blocking`、:32 に「前提: `ffi-blocking`」の `tls-platform-verifier`、:436-438 に「プールの仕事の予算と、放さずに終わる方針」がある。
  - docs/future/async.md:263 は「仕事には渡さない」と書き、docs/spec/vm.md:520 (`RawBuf` の箇条) と driver.rs:29-35 (ファイルの仕事のスレッドは `RawBuf` を使う) と食い違っている。
  - docs/future/ffi.md の「長く走る呼び出し」(:82-) は、3段の下ろし方と「仕事の記録が渡した語を数えた参照で持つ」と書き、E0004 で断ると書く (:84)。async.md の「ブロックする呼び出しのプール」(:259-280) と multicore.md の「移し戻し」(:292-、:300) と印付けの表 (:316) は、記録が語を持ち `MutUnboxedArray` を記録へ移す形を書いている。
  - `blocking` の E0004 の文は docs/spec/declarations.md:226、docs/spec/diagnostics.md:56、:172、:175、docs/implementation/status.md:188、docs/future/ffi.md:84 にある。
  - docs/implementation/testing.md:427 は fixture のファイルの規則を書く。「`-- fixtures: only runnable through the test harness` は最初の行の印の後に置く」「`-- fixture ids: A..B` は半開きで空でなく、ファイルどうしで重ならない」「`ffi/` のファイルは 1 から 99、Rust のテストは 100000 から上」「トランスクリプト `fixtures.repl` は tag 3」。この規則を読むコードはない (`grep -rn "fixture ids" crates` が何も出さない)。
  - docs/implementation/testing.md:390 は `-- clock: real (<理由>)` の印の規則を書く。
  - docs/implementation/benchmarks.md の「今の数値」(:207) の3つの表は `コミット: 9949fa6` で測ってある。
- テストの道具
  - `crates/eml_vm/Cargo.toml` は `eml_test_support = { workspace = true, features = ["run"] }` を持つ (:29)。
  - `crates/eml_vm/src/workers.rs` の `mod tests` は :1352 にあり、そこに置く単体テストの名前は `workers::tests::…` になる。
  - `crates/eml_test_support/tests/main.rs` は `mod support;` だけを宣言する。
  - UI テストの `leading_marks` は、印でない最初の行で読むのをやめる (`crates/eml_cli/tests/common/mod.rs:161-193`)。印は `windows:`、`workers:`、`clock:`、`signal:`、`max-depth:`、`max-bytes:` である。
  - `crates/eml_vm/tests/miri.rs` のテストは LIR のテキストを `eml_lir::parse` で読んで流し、パイプラインを通さない。C の関数のテスト (:597、:621) はスカラーと詰めた整数のハンドルだけを渡す。`Async` を使うテストはない。
  - UI テストは、どのプログラムも `eml_test_support::link(program)` でつないで `execute` で流す (`crates/eml_cli/tests/ui.rs:210-215`)。
  - `.config/nextest.toml` の `[profile.timing]` は `slow-timeout = { period = "2s", terminate-after = 1 }` で、ディレクトリをまとめて流すテストに 10 秒を許す。
- ffi-c が入れたもの (4 章の確かめ方で確かめる)
  - `eml_cabi` (crates/eml_cabi/src/lib.rs): `Bridge = unsafe extern "C" fn(callee: *const (), args: *const u64) -> u64` (:7)、`Dtor = unsafe extern "C" fn(*mut c_void)` (:9)、`CCall { pub bridge, pub callee }` は `Clone, Copy, Debug` で、SAFETY のコメントつきの `unsafe impl Send/Sync` を持つ (:11-20)。`Linked` (:27-33) の欄 (`functions`、`calls`、`handles`、`dtors`) はどれも非公開で、`#[derive(Clone, Debug, Default)]`。作れるのは `pub unsafe fn new(calls: Vec<(CFunction, CCall)>, dtors: Vec<(CHandleType, Option<Dtor>)>)` (:44、`# Safety` つき) と `Linked::default()` だけである。読み手は `calls()` (:59)、`dtors()` (:64)、`fits(functions, handles)` (:70、宣言を `==` でまるごと比べる)。`eml_vm` は `pub use eml_cabi::{Bridge, CCall, Dtor, Linked};` (lib.rs:43)。
  - `eml_native::Linker` (crates/eml_native/src/linker.rs): `new(overrides)` (:32)、`pub unsafe fn with_symbols(self, symbols)` (:47、`# Safety` つき)、`link(&mut self, functions, handles) -> Result<Linked, LinkError>` (:55)。JIT の module は `static RETIRED: Mutex<Vec<JITModule>>` (bridge.rs:22) に移して、LeakSanitizer が報告しないよう生かしておく。`eml_cli` は `pub use eml_native::{LinkError, Linker};` (lib.rs:22)。
  - `CodeBase::load(program, &Linked)` (compile/mod.rs:70) は `assert!(linked.fits(&program.c_functions, &program.c_handles), …)` (:71-74) で、`Linked` がプログラムの宣言と同じ宣言からつないだものかを確かめる。`Machine::load` (lib.rs:456)、`AnyMachine::load` (:1201)、`eml_vm::execute` (:254) は `linked: &Linked` を受ける。C の関数を持たない呼び出し側は `&Linked::default()` を渡す。`renumbered` (compile/mod.rs:275-298) は `CFunction::new` で関数を作り直してハンドルの番号を付け直す。
  - `eml_test_support` (crates/eml_test_support/src/lib.rs): `#[cfg(feature = "run")] mod fixtures;` は非公開で、`pub use fixtures::fixture_symbols;` だけを出す (:18-21)。`static LINKER` (:390-394) は SAFETY のコメントつきの `unsafe { Linker::new(Vec::new()).with_symbols(&fixture_symbols()) }` で作る。公開の関数は `link(program) -> Linked` (:399) だけで、`try_link` はない。C の表のないプログラムには lock も JIT も触らずに `Linked::default()` を返す。
  - fixture (fixtures.rs): `fixture_symbols()` (:9) は `eml_fixture_add_i32`、`sum12`、`neg`、`dot`、`fill`、`bytes_sum`、`handle_new(tag, value)` (値が負なら null)、`handle_get`、`handle_free`、`frees(tag)` (読むと数を 0 に戻す) の10個を並べる。
  - eml_vm のテストの道具 (`crates/eml_vm/tests/common/c.rs`): `linked(program, calls, dtors) -> Linked` (:17、SAFETY のコメントつきで `unsafe { Linked::new(..) }` を呼ぶ)、`calls()` (:54)、`dtors()` (:70)。呼ばれる関数は自前の `rs_abs`、`rs_add_i32`、`rs_sub_i32`、`rs_half`、`rs_dot`、`rs_fill`、`rs_bytes_sum`、`rs_handle_new`、`rs_handle_get`、`rs_frees` で、橋渡しは署名ごとに手で書く (`i64_to_i64`、`i64_i64_to_i64`、`i32_i32_to_i32`、`f64_to_f64`、`i64_i64_i64_to_f64`、`i64_i64_f64_to_void`、:83-143)。ハンドルは確保せず、tag と値を詰めた整数 `(tag << 32) | value` にする (Miri のため)。dtor の数は自前の `FREES` で数え、`eml_test_support` の数と別である。eml_vm のテストは `eml_fixture_*` を使わない。Rust のテストが使う tag は `ffi.rs` と `miri.rs` の 100002 から 100007 である。
  - `RunConfig` と `Machine` は linker を持たない。`Repl` はセッションごとに `Linker` を1つ持ち、入力ごとに `machine.load` の前につなぐ。リンクの失敗は `Outcome::Unlinked` である。
  - `FunctionRow` (eml_extern/src/lib.rs:2273-2300) は `writes` を持ち、`removable()` (:2307-2316) は `Pure && !writes && すべての Lin の引数を借りる` である。`OwnedRow` は lib.rs:2322 の pub の struct で欄は非公開、`#[derive(Clone, Debug, PartialEq, Eq)]`、`view()` (:2337) を持つ。`c.rs` の `pub fn writes(params: &[CParam]) -> bool` (:243) が `writes` を決める。
  - `ExternRef::{Std, C}` (lib.rs:2356-2360)、`std()` (:2369)、`row(self, c)` (:2377)、`from_name(name, c)` (:2385)。`Program::row(ext)` は eml_lir と eml_core_ir にある。`grep -rln "ExternRef::C" crates/*/src` は `eml_core_ir/src/translate/program.rs`、`eml_extern/src/lib.rs`、`eml_vm/src/compile/emit.rs` を出す。
  - `CFunction` (c.rs:118-131) は `#[derive(Clone, Debug, PartialEq, Eq)]` で、pub の欄 `name`、`symbol`、`links`、`pure`、`blocking`、`params`、`ret` と非公開の `row: OwnedRow` を持つ。`CFunction::new(name, symbol, links, pure, blocking, params, ret)` (:135) が行を作る。`row()` (:192)、`c_signature()` (:197)、`faults()` (:222)。C の行の `borrowed` は `param_repr == Repr::Obj` の引数、`with_args` は `Lin` の引数の位置である。
  - `CodeBase.c_functions: Vec<CEntry { function, call: CCall }>` (code.rs:89-93)、`c_handles: Vec<CHandleEntry { handle, dtor: Option<Dtor> }>` (:96-99)、`Index.c_functions`。
  - `crates/eml_vm/src/ffi.rs`: `CHandle { ptr, dtor }` と、dtor を呼ぶ `Drop` (:17-34)、`enum Raise { Narrowing { arg, int, value }, NullHandle }` (:37) と `Raise::text` (:45)、`enum Out { Raw(i64), Word(Word) }` (:58)、`marshal(ctx, task, function, slots: &[u32]) -> Result<Result<Vec<u64>, Raise>, VmFault>` (:67、先に範囲をすべて確かめ、次にポインタを取る)、`unmarshal(code, ctx, function, ret: u64) -> Result<Result<Out, Raise>, VmFault>` (:111)、`call<R: Exec>(code, ctx, task, dst, at)` (:134、SAFETY のコメントが `Linked::new` と `fits` を引く)、非公開の `raise<R: Exec>(code, ctx, task, dst, function, why)` (:163) と `write_out(task, dst, out)` (:180、`ctx` を取らない)。
  - 命令: `Op::Ffi { dst, at }` (op.rs:399)、被演算子は `[CodeBase の C の関数の番号, 引数の数, スロット…]`。emit.rs:526-534 が `ExternRef::C(id)` を `links.c_functions[id.0]` で `Op::Ffi` にする。逆アセンブラは `ffi s{dst} = {name}({args})` (disasm.rs:401-405) と `cfn {name}` の行 (:16-18) を書く。
  - 型: `TypedProgram.c_shapes`、E2023 (`eml_types/src/ffi.rs` の `expected`、:497-。`pure` でなければ `IO`、狭める引数かハンドルの結果で `Panic`、`blocking` で `Async`)、`pure` と `blocking` の両方は E0018 (:81-82)。`blocking` の E0004 は :95-100 (`"`blocking` C functions are not supported yet"`)。型のテスト `blocking_is_e0004_after_a_correct_row` (`crates/eml_types/tests/ffi.rs:273`)、UI の `tests/ui/check-fail/ffi/blocking.em`。
  - LIR のテキストの `cfn … blocking …` (`crates/eml_lir/src/syntax.rs:610-639` で読み、:1802-1806 で書く)。extern の呼び出しは `extern Mod.f(args)@loc` で、読み手は `ExternRef::from_name` で引く (syntax.rs:917-929)。往復のテストは `crates/eml_lir/tests/text.rs` の `c_tables_read_back` (:1939) と `crates/eml_core_ir/tests/text.rs:700`。
  - fixture の UI のファイル: `tests/ui/run/ffi/fixture_{handle,sum12,dot,fill,bytes}.em` と `tests/ui/run-fail/ffi/fixture_null_handle.em`。どれも1行目が `-- fixtures: only runnable through the test harness`、2行目が `-- fixture ids: A..B` で、範囲は `fixture_handle` 1..2、`fixture_null_handle` 2..3、`fixture_sum12` 4..5、`fixture_dot` 5..6、`fixture_fill` 6..7、`fixture_bytes` 7..8。トランスクリプト `crates/eml_cli/tests/repl/ffi/fixtures.repl` は tag 3 を使う。
  - vm.md の `## C の関数の呼び出し` (:966) と、その下の `### 橋渡しの関数` (:970)、`### ライブラリと記号` (:987)、`### C の関数の命令` (:1007)、`### \`--link\`` (:1032) (この順)。
  - object-model.md の信頼の行 T16 (:1380)。窓は「C の関数を呼ぶ間」で、Miri の欄は同じスレッドのテストを並べる (`c_writes_through_unboxed_ptr_mut_and_the_heap_reads_it`、trusted.rs:2451 など)。`Heap::unboxed_ptr_mut` は `pub unsafe fn unboxed_ptr_mut(&self, w: Word) -> Result<*mut u64, HeapError>` (`crates/eml_rt/src/heap/unboxed.rs:730`、`verus!` の中) で、要素 0 を指す。別のスレッドが書くテストの前例は `a_lent_room_is_written_by_another_thread` (trusted.rs:803、`std::thread::scope` に `Send` の包みごと移す)。
  - CI の `asan` の job (ci.yml:211-221) は `-p eml_io`、`-p eml_vm --test integration -- api:: signal:: io:: workers:: ffi::`、`-p eml_native` を流す。leak の検出は既定のままで、`ASAN_OPTIONS` はどこにもない。`tsan` の job (:240-258) は `-p eml_rt trusted::`、`-p eml_vm --test integration -- workers:: signal:: preempt:: io:: chan:: ffi::`、`-p eml_vm --lib counters::`、`-p eml_io --lib -- driver::tests::` を流す。

## 4. 前提の環境

| 要るもの | 使う所 | 確かめ方 | ないとき |
|---|---|---|---|
| 基点の木 | すべて | `git merge-base --is-ancestor 8d6a813a HEAD` が 0 で終わり、`git log --oneline --grep='^Close ffi-c' -1` が `ae687c1c Close ffi-c` を出し、`` grep -n '^- `ffi-blocking`' docs/future/roadmap.md `` が :31 の1行を出す | 止めて利用者に聞く |
| Nix の devShell の cargo-nextest、cargo-insta | すべて | `nix develop -c cargo nextest --version` が `cargo-nextest 0.9.146` を、`nix develop -c cargo insta --version` が `cargo-insta 1.49.0` を出す (版が上がっていてもよい) | 止めて利用者に聞く |
| cargo-about 0.9.0 | T-11 (`nix/third-party-licenses.sh --check`) | `nix develop -c cargo about --version` が `cargo-about 0.9.0` を出す | 止めて利用者に聞く |
| verus、verusfmt | T-1、T-3、T-5 | `nix develop -c verus --version` が `Version: 0.2026.10.26.d17fbd0` を含む行を、`nix develop -c verusfmt --version` が `verusfmt 0.7.4` を出す (`cargo verus` は `--version` を受け付けない) | 止めて利用者に聞く |
| `.#miri` の shell | T-1、T-2、T-3、T-5、T-7 | `nix develop .#miri -c cargo miri --version` が `miri 0.1.0 (76456a688b 2026-10-25)` を出す | 止めて利用者に聞く |
| actionlint と shellcheck | T-8 | `nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- .github/workflows/ci.yml` が何も書かずに 0 で終わる | 止めて利用者に聞く |
| CI (ubuntu-24.04、ubuntu-24.04-arm、macos-15、windows-2025 と Miri、ASan、TSan の job) | T-3、T-7、T-8、T-11 の後 | ブランチを push して全 job が緑になる。基点では main の CI が緑である | 止めて利用者に聞く |
| macOS (`bench/*.sh` は macOS で測る) | T-11 | `uname` が `Darwin` | 表の入れ替えだけを直後の別のコミット `Measure ffi-blocking` にする (roadmap の「進め方」の 2) |

ThreadSanitizer と AddressSanitizer は x86_64 Linux の CI で確かめる。aarch64 の macOS では ASan の leak 検出が動かないので、手元では確かめられない (Q-1)。

ffi-c が入れた形を、最初の変更の前に確かめる。リポジトリの根で流し、出力が下と同じであること。行番号だけが違うなら読み替えてよい。形そのもの (関数の有無、引数、`unsafe`) が違うときは止めて報告する。

- `grep -n "pub struct Linked\|pub struct CCall" crates/eml_cabi/src/lib.rs` → `12:pub struct CCall {` と `28:pub struct Linked {`
- `grep -n "pub unsafe fn new\|pub fn calls\|pub fn dtors\|pub fn fits" crates/eml_cabi/src/lib.rs` → `44:    pub unsafe fn new(`、`59:    pub fn calls(&self) -> &[CCall] {`、`64:    pub fn dtors(&self) -> &[Option<Dtor>] {`、`70:    pub fn fits(&self, functions: &[CFunction], handles: &[CHandleType]) -> bool {`
- `grep -n "pub unsafe fn with_symbols\|pub fn link" crates/eml_native/src/linker.rs` → `47:    pub unsafe fn with_symbols(mut self, symbols: &[(&str, *const ())]) -> Linker {` と `55:    pub fn link(`
- `grep -n "static RETIRED" crates/eml_native/src/bridge.rs` → `22:static RETIRED: Mutex<Vec<JITModule>> = Mutex::new(Vec::new());`
- `grep -n "pub fn load" crates/eml_vm/src/lib.rs` → `456:` と `1201:` の2行で、どちらも `pub fn load(&mut self, program: &Program, linked: &Linked) -> Result<Loaded, CompileError> {`
- `grep -n -A2 "^pub fn execute(" crates/eml_vm/src/lib.rs` → `254:pub fn execute(`、`255-    program: &Program,`、`256-    linked: &Linked,`
- `grep -n "linked.fits" crates/eml_vm/src/compile/mod.rs` → `72:            linked.fits(&program.c_functions, &program.c_handles),`
- `grep -rn "Linker\|CLinker" crates/eml_vm/src` → 何も出さない
- `grep -n "pub fn link\|pub fn try_link" crates/eml_test_support/src/*.rs` → `crates/eml_test_support/src/lib.rs:399:pub fn link(program: &eml_lir::Program) -> Linked {` の1行
- `grep -n "^mod fixtures\|^pub use fixtures" crates/eml_test_support/src/lib.rs` → `19:mod fixtures;` と `21:pub use fixtures::fixture_symbols;`
- `grep -n "pub fn fixture_symbols" crates/eml_test_support/src/fixtures.rs` → `9:pub fn fixture_symbols() -> Vec<(&'static str, *const ())> {`
- `grep -n "pub fn with_linker" crates/eml_cli/src/repl.rs` → `162:    pub fn with_linker(self, linker: Linker) -> Repl {`
- `grep -n "fixture_symbols" crates/eml_cli/tests/repl.rs` → `21:` の1行 (`unsafe { Linker::new(Vec::new()).with_symbols(&eml_test_support::fixture_symbols()) }`)
- `grep -n "fn linked" crates/eml_vm/tests/common/c.rs` → `17:pub fn linked(`
- `grep -n "pub struct OwnedRow" crates/eml_extern/src/lib.rs` → `2322:pub struct OwnedRow {`。`grep -n "fn writes(params" crates/eml_extern/src/c.rs` → `243:pub fn writes(params: &[CParam]) -> bool {`
- `grep -n "pub(crate) fn marshal\|pub(crate) fn unmarshal\|pub(crate) fn call\|pub(crate) enum Raise\|pub(crate) enum Out" crates/eml_vm/src/ffi.rs` → `37:`、`58:`、`67:`、`111:`、`134:` の5行
- `grep -n "fn raise\|fn write_out" crates/eml_vm/src/ffi.rs` → `163:fn raise<R: Exec>(` と `180:fn write_out(task: &mut Task, dst: Slot, out: Out) {`
- `grep -n "struct CEntry\|struct CHandleEntry" crates/eml_vm/src/code.rs` → `89:pub(crate) struct CEntry {` と `96:pub(crate) struct CHandleEntry {`
- `grep -rn "blocking_is_e0004_after_a_correct_row" crates/eml_types/tests` → `crates/eml_types/tests/ffi.rs:273:fn blocking_is_e0004_after_a_correct_row() {`
- `grep -n "^### C の関数の命令\|^### \`--link\`" docs/spec/vm.md` → `1007:### C の関数の命令` と ``1032:### `--link` ``
- `grep -rn "pub unsafe fn unboxed_ptr_mut" crates/eml_rt/src` → `crates/eml_rt/src/heap/unboxed.rs:730:` の1行
- `git grep -L '^-- fixture ids: ' -- $(git grep -l '^-- fixtures:' -- 'tests/ui/*.em')` → 何も出さない (6つの fixture のファイルがどれも範囲の行を持つ)
- `grep -rn "fixture ids" crates` → 何も出さない (範囲を確かめるコードはまだない)

## 5. 決めたこと

### 5.1 長く効く方針 (変えるときは設計からやり直す)

実装の途中でこれらとぶつかったら、止めて報告する。各項の「適合の確かめ」の MUST と MUST NOT は、守らなければならない規則である。

#### D-1: `blocking` の呼び出しは、受け付け、`io_wait`、取り出しの3段に下ろす

- 決定
  - Core IR の translate の `extern_call` (`lending_call` と同じ所) が、`blocking` の行の飽和した呼び出し `f a1 … an` を次の4つの文に下ろす。
    ```
    let op      = extern submit "Mod.f" a1 … an     -- ExternRef::CSubmit(id): C の引数をすべて借りる
    let (w, op) = call Std.Async.io_wait op          -- 名前で見つけた Call::Direct、続けて Stmt::Unpack
    let r       = extern take "Mod.f" op w o1 … om  -- ExternRef::CTake(id): 記録の番号、Wake、Obj の引数 m 個を順に
    let t       = con (r, lin args …)                -- lending_call の組。CTake 自身の with_args で決まる
    ```
  - `o1 … om` は、C の引数が `Obj` のもの (`String`、`Bytes`、`UnboxedArray`、`MutUnboxedArray`、ハンドル) である。`Int`、`Float`、`Unit` の引数は `take` に渡さない。
  - 部分適用と値としての使い道は、ffi-c の `extern_wrapper` を通る。`blocking` の行の wrapper は `yields` である。
  - `Std.Async.io_wait` を `RuntimeEntry::IoWait` (名前 `Std.Async.io_wait`、引数の Repr `[Int]`) にして、`retain_reachable` の根にする。translate は今の3つの entry と同じく名前で見つける。
- 理由
  - タスクが止まる道は、`Async.run` の下の `perform await_io` だけである (Async.em:25、:58)。`io_wait` はそれを perform し、速い道、予算、取り消しの規則を持つ。
  - `take` に `Wake` を渡すと、種類ごとの後始末 (記録を放すこと、捨てるハンドルの dtor) を、`CFunction` を知っている1か所に置ける。std に関数も行も足さずに済む。
- 退けた案
  - タスクを止める VM の1命令: 継続を捕まえられず、`io_wait` を写すことになる。
  - `blocking_wait` と汎用の `discard_op`: 汎用の解放は種類ごとの後始末を知らず、取り消されたときに返ったハンドルの dtor を失う。
- 及ぶ範囲: `eml_core_ir` の translate、`eml_extern` の行の見え方と `RuntimeEntry::IoWait`。同期の `ExternRef::C` は含まない。
- 見直す条件: タスクの止まり方 (`await_io` と `park_io`) を変えるとき。ネイティブのコードが `blocking` を呼ぶとき。
- 適合の確かめ
  - MUST: translate が上の4つの文に下ろす。`take` は `Wake` を受け取り、記録を放すのは `take` だけである。
  - MUST NOT: タスクを直接止める VM の命令を作らない。この段のために std に関数か行を足さない。
  - 確かめ方: `lowering::a_blocking_call_waits_between_submit_and_take` の inline snapshot に4つの文 (`Unpack` を含む) が出る。`git diff 8d6a813a -- std/` が空。6 章の照合の場所の grep が通る。
- リポジトリに残す所: docs/spec/vm.md の「`blocking` の呼び出し」と docs/spec/core-ir.md の「組を返す extern」(T-7)

#### D-2: 記録はヒープの語を持たず、引数は待つタスクの継続が持つ

- 決定
  - 仕事の記録は語を持たない。`submit` から `take` まで、引数は生きた変数のままで、`await_io` が記録に置く継続 (タスクの論理 ID のヒープの closure で、数えた参照) が捕まえる。仕事の closure が持つのは `CCall` と `u64` のスロットだけである。`transfer_back`、共有の印、`Depot` は使わない。
  - 継続かそのヒープが消えうる道は、どれも先に `quiesce` を通る。`quiesce` は仕事を待つか、`pending() > 0` を残す。後者は解放を禁じる (D-8)。
    1. `main` の普通の終わり: `Async.run` は根の scope にタスクがなくなるまで戻らない。`blocking` の中のタスクもその一員なので、仕事は残っていない。
    2. 待つタスクの取り消し: 操作の待ちは外さず、タスクは完了が当たってから再開する (vm.md の「取り消しの届け方」)。
    3. 救済: 記録を待つタスクは動かさない (vm.md の「救済」)。
    4. 利用者の handler が `k` を捨てること: `Async` は抽象なので書けない (E1058)。
    5. `par` の右の子と `compute` の仕事: row が `never` で `IO` を含まないので、`blocking` を呼べない。
    6. run を止める終わり (`Async.run` の外へ出た実行時エラー、デッドロック、致命的な誤り、Ctrl-C、`exit`): worker は安全点で止まり、静止させてから `Ctx` を返す。待つ継続は巻き戻さず、ヒープとともに放す。ヒープを落とすか回復するのは、仕事を見捨てなかったときだけである。スコープの失敗は run を止めず、2 の道 (取り消し) を通る。
    7. worker の panic: poisoned になり、ヒープは漏らす。
    8. REPL の回復: `recover` は先に静止させ、`pending() > 0` なら poisoned にする。
    9. `Machine` を落とすこと: driver を先に落とす。poisoned の `Machine` はヒープを落とさない。
  - 仕事が走る間、`Lin` の引数 (`MutUnboxedArray`、ハンドル) には待つ継続からしか届かない。`Unr` の引数はほかのタスクも持ちうるが、継続が参照を1つ持つので、ほかに持ち手がいれば RC は 2 以上になる。std のその場の書き換え (`unboxed_thaw`、`unboxed_set`、`Bytes` の操作) は共有なら写すので、ほかのタスクがその場で書くことはない。
  - 仕事の中身は今の規則のまま: eml のヒープにも RC にも触れない。FFI の仕事は `bridge(callee, slots)` と、答えを入れる Rust の `Box<u64>` 1つだけである。検査モードはプールの書き込みを持ち主の検査にかけない (宣言を信頼する)。
- 理由: 生きていることは、止まったタスクについて VM がすでに頼っている不変条件から出る。プールは `Ctx` を要らない。ffi-c の「Lin の引数は借りて組で返す」規則をそのまま守れ、`lending_call` を使い回せる。
- 退けた案: 記録が語を持つ形 (`held` の列、解放の行、分けた取り出し、`transfer_back`): ffi-c の借りて返す規則とぶつかる。
- 及ぶ範囲: `OpKind::Ffi`、`crates/eml_vm/src/ffi.rs`、仕事の closure。`tls-platform-verifier` の仕事の中身にも当てはまる。
- 見直す条件: 救済が記録を待つタスクを動かすようになるとき。継続を巻き戻すようになるとき。
- 適合の確かめ
  - MUST: 記録はヒープの語を持たない。`submit` は C の引数をすべて借り、`take` は `Obj` の引数をすべて借りる。
  - MUST NOT: プールのスレッドが `CCall` と `u64` のスロットのほかを受け取ること。eml のヒープか RC に触れること。
  - 確かめ方: `OpKind::Ffi` に `Word` の欄がない。`c::a_blocking_row_splits_into_submit_and_take` が `CTake` の `borrowed == [false, false, true…]` と、`with_args` が自分の `Lin` の引数の位置であることを確かめる。`grep -rn "transfer_back" crates/eml_vm/src/ffi.rs` が何も出さない。
- リポジトリに残す所: docs/spec/vm.md の「`blocking` の呼び出し」と「操作の記録」(T-7)、docs/future/async.md の「ブロックする呼び出しのプール」(T-10)

#### D-3: `Op::Job` は Rust の値を返し、`Done::Job` で終わる

- 決定
  - `Op::Job(Box<dyn FnOnce() -> Box<dyn Any + Send> + Send + 'static>)`、`Done::Job(Box<dyn Any + Send>)`。
  - 仕事は `io::Result` を返さない。仕事の `Completion.result` が `Err` なのは、driver が仕事を走らせられなかったときだけである。
  - `Done` は `PartialEq` と `Eq` を持たず、手で書いた `Debug` (`Job(..)` と書く) を持つ。
- 理由: C の結果と TLS の状態を、所有した値の1本の道で返せる。仕事そのものは失敗しない。
- 退けた案: `Done::Size(usize)` に `u64` のビットを入れる: TLS では使えず、失敗しない仕事に `io::Result` を付けることになる。
- 及ぶ範囲: `eml_rt::driver`、両方の driver、仕事を作るものとテストのすべて。
- 見直す条件: 仕事が自分で失敗を返す必要が出たとき (そのときも失敗は値の中に書く)。
- 適合の確かめ
  - MUST: 上の型。MUST NOT: 仕事が `io::Result` を返すこと。`Done` が `PartialEq` を実装すること。
  - 確かめ方: driver.rs の型。`grep -n -B3 "^pub enum Done" crates/eml_rt/src/driver.rs | grep -c "PartialEq"` が `0` で、`grep -rn "impl PartialEq for Done" crates` が何も出さない。`driver::tests::a_job_returns_its_boxed_value` と `test_driver::a_job_value_is_delivered_at_wait` が通る。
- リポジトリに残す所: driver.rs の doc と docs/spec/vm.md の「時計と driver」(T-1)

#### D-4: `blocking` の呼び出しは取り消しの地点である

- 決定
  - `submit` は記録を作るとき、登録表の lock の下で `Registry::point(me)` (`op_ready` と `park_io` が使う関数) を評価する。真なら `Op` を出さず、`cancelling = true` と `Done(Err(other "cancelled"), true)` にする。`stats`、予算、`parks` には触れない。後の `io_wait` は、終わった記録を取り消しの地点で待つときと同じに動き、`Cancelled` で再開する。
  - 偽で、その後に取り消しが来たら、仕事は走り、タスクは本当の完了の後に `Cancelled` で再開する (今の操作の待ちの規則)。2つの確かめは仕事を始めるかだけを決め、待ちが終わるかは決めないので、食い違わない。
  - `take` は `Cancelled` のとき、記録を放し、行がハンドルを返していて答えが 0 でなければ、そのハンドルの型の dtor をその場で (持ち主のスレッドで) 呼び、IO の失敗 (種類 `Cancelled`、文言 `task was cancelled`) を起こす。`Cancelled` で答えが `Ok` のときは仕事が走ったので、C の副作用は残り、結果は捨てる。
  - スコープの失敗 (`fail_scope_io`、`fail_scope_panic`、`cancel_members`) による取り消しも同じ規則に従う。失敗を起こし直す `run` と `scope` は、取り消した子が終わるまで戻らないので (`wait_closed`)、戻らない C の呼び出しを抱えたスコープの失敗は、C の関数が戻るか、run を止める終わり (Ctrl-C など) が来るまで終わらない。run の仕事の予算は静止にだけ効き、取り消しには効かない (D-8)。`Std.Stream` のファイルの仕事と同じ規則である。
- 理由: std のどの待ちとも同じく取り消しの地点になる。仕事は止められない。C の資源を黙って漏らさない。取り消しの途中で継続を再開すると、仕事が読む引数を落とす (D-2 の 2)。予算を取り消しに持ち込むには、記録か `take` が引数を留める必要があり、D-2 を作り直すことになる。
- 退けた案: 取り消しの地点にしない `blocking`。
- 及ぶ範囲: `Op::FfiSubmit`、`Op::FfiTake`、docs/spec/declarations.md。
- 見直す条件: 仕事を取り消せるようにするとき。
- 適合の確かめ
  - MUST: 上の前の確かめ、取り消しの後は本当の完了を待つこと、`take` の `Cancelled` の扱い。取り消された記録の待ちは、仕事の完了でだけ終わる。スコープの失敗も例外にしない。
  - MUST NOT: 取り消しの道 (`cells.rs`、`Async.em`) に仕事の予算や期限を持ち込むこと。仕事の完了の前に、待つ継続を再開すること。
  - 確かめ方: `blocking_fixture_cancelled.em` が `frees` で `1` を書く。`cells::tests::a_cancelled_task_submits_no_job` (名前は cells の `mod tests` に合わせる) と `ffi::a_cancelled_task_submits_no_job` が、取り消された後のタスクが `Op::Job` を出さないことを確かめる。今あるテストの `RunStats` の snapshot が変わらない。`ffi::a_failing_scope_waits_for_its_cancelled_blocking_call` が通る (run は `Panicked("boom")` で終わり、`is_poisoned()` が偽、`frees` が 1)。`grep -n "job_wait\|job_budget\|job_deadline" crates/eml_vm/src/cells.rs` が何も出さない。
- リポジトリに残す所: docs/spec/vm.md の「`blocking` の呼び出し」、docs/spec/declarations.md の「`extern "C"`」(T-7)、docs/spec/stdlib.md の「取り消し」(T-8)

#### D-5: 受け付けの行は狭める引数があるときだけ中断を起こし、取り出しの行はいつも起こす

- 決定
  - `CSubmit.faults` は、狭める `as` の引数があるときだけ true である。範囲の確かめは記録を作る前に走るので、失敗しても放すものがない。
  - `CTake.faults` はいつも true である。IO の失敗は `failed` から `IO` の handler への中断になる (`Effectful` の行)。null のハンドル、IO の失敗、`Cancelled` の失敗は、どれも持ち主の worker の `take` で起こす。
  - E2023 の型の規則: `blocking` の行は `IO` と `Async` をいつも要し、`Panic` は狭める引数かハンドルの結果があるときに要る。
  - 同期の C の行の規則 (「`faults` は狭める引数かハンドルの結果があるとき」) は、同期の `ExternRef::C` と E2023 の `Panic` の述語に当てはまる。`blocking` の2つの行は、ffi-c の方針の理由 (「中断を始めうるか」) に従って上のとおりにする。
- 理由: 巻き戻しの地点は「中断を始めうるか」で決める。
- 及ぶ範囲: 行の見え方、`eml_lir::unwind`、`ffi.rs`。
- 見直す条件: 中断の分類を変えるとき。
- 適合の確かめ
  - MUST: 上の `faults`。範囲の確かめは記録の前。null の確かめ、IO の失敗、`Cancelled` は `take` で。
  - 確かめ方: `unwind::a_submit_without_narrowing_is_no_unwind_point`、`unwind::a_submit_with_narrowing_is_an_unwind_point`、`unwind::a_take_is_an_unwind_point` が通る。`blocking_fixture_range_panic.em` と `blocking_fixture_null_handle.em` が両方のモードで通る。
- リポジトリに残す所: docs/spec/lir.md の「巻き戻しの地点」、docs/spec/vm.md の「`blocking` の呼び出し」(T-7)

#### D-6: 仕事は記録の host に、今の受け付けと完了の道で送る

- 決定: 仕事は記録の `host` (`Run::io_worker(k)`) へ、今の `submit`、`Submit`、`Completion` の道で送る。完了は driver の完了の列と記録を通ってだけ持ち主に届く。`Notice` の種類は足さない。host でない持ち主 (仮想の時計の worker `k` ≥ 1) は今の `Submit` と `Completion` の知らせを使う。`Op::Job` は `Send` である。
- 理由: I/O の道を1本に保つ。仮想の時計では worker 0 がすべてを host する。
- 及ぶ範囲: cells.rs、workers.rs。
- 見直す条件: 記録の host の決め方を変えるとき。
- 適合の確かめ
  - MUST: 上の道。MUST NOT: 新しい `Notice` の種類。
  - 確かめ方: `blocking_fixture_add.em` が仮想の時計の 2 と 8 worker で通る。`ffi::a_blocking_call_runs_on_the_driver_of_worker_one` が本物の時計で通る。`git diff 8d6a813a -- crates/eml_vm/src/workers.rs | grep "^+.*Notice::"` が新しい種類を出さない。
- リポジトリに残す所: docs/spec/vm.md の「`blocking` の呼び出し」(T-7)

#### D-7: プールは driver ごとに 32 本で、`Op::Job` は同時に 24 本まで

- 決定
  - プールは driver ごとに1つで `POOL_LIMIT = 32`。`Op::Job` が同時に使うスレッドは `JOB_LIMIT = 24` までで、ファイルの仕事はどのスレッドも使える。`submit` は決して止まらない。
  - 利用者に見える文 (docs/spec/declarations.md と docs/spec/stdlib.md の「`Std.Stream`」に同じ文を置く):
    「`blocking` の呼び出しは、タスクを置いた worker の driver のプールで走る。1つの driver で同時に走る `blocking` の呼び出しは 24 までで、それを超えた呼び出しは順に待つ。ファイルの読み書きのために、別に 8 本を残す。worker を N 個使う run では上限は driver ごとにあり、タスクをどの worker に置くかはランタイムが決める。仮想の時計 (テスト) では、仕事は worker 0 の上でその場で動き、上限はない。`blocking` の呼び出しの中で、同じ run のほかの `blocking` の呼び出しの結果を待つ C の関数は、上限に達するとデッドロックする。」
- 理由: 戻らない C の呼び出しが std のファイルの仕事を飢えさせないためである。24/8 は測っていない既定で、「その後の項目」で見直す。
- 退けた案: 利用者の別のプール。スレッドの上限なし。
- 及ぶ範囲: `eml_io` のプール、spec の文。テストの driver は含まない。
- 見直す条件: 計測か報告で、ファイルの仕事か C の呼び出しが上限で待つのが見えたとき。
- 適合の確かめ
  - MUST: 上の2つの定数、ファイルの仕事がどのスレッドも使えること、`submit` が止まらないこと、spec の2か所の文。
  - 確かめ方: `driver::tests::pool::jobs_beyond_the_job_limit_wait_while_a_file_job_gets_a_thread` (上限 3、`Op::Job` の上限 2)。`grep -n "JOB_LIMIT: usize = 24" crates/eml_io/src/driver/pool.rs` が1行。`grep -c "同時に走る \`blocking\` の呼び出しは 24 まで" docs/spec/declarations.md docs/spec/stdlib.md` がどちらも 1。
- リポジトリに残す所: docs/spec/vm.md の「時計と driver」(T-2)、declarations.md と stdlib.md の文 (T-7)、roadmap の「その後の項目」(T-4)

#### D-8: 静止は `Op::Job` を run の期限まで待ち、残りを見捨てて `Machine` を poisoned にする

- 決定
  - `IoDriver::quiesce(&mut self, jobs: Duration)`。ファイルの仕事は今のまま上限なしに待つ。`Op::Job` は `jobs` まで待ち、まだ残るものを見捨てる。走っているものは slot と札を外し (遅れた答えは捨てる)、列で待つものは走らせずに捨てる。見捨てた仕事は、それからずっと `pending()` に数える。名前の解決は今のまま。見捨てても driver は壊れない。
  - run は仕事の期限を1つ持つ。最初の `quiesce` で `now + RunConfig::job_budget` に決まり、その run のどの `quiesce` (worker `k` ≥ 1、それから worker 0) も残りを受け取る。予算の既定は 1 秒で、`RunConfig::with_job_budget` と `Machine::with_job_budget` で変える。`Machine::recover` は予算をまるごと渡す。
  - `quiesce` の後に `pending() > 0` なら `Machine` は poisoned になり、ヒープを漏らし、`is_poisoned() == true` を返す。
  - 見える結果
    - 仕事を見捨てた `exit n` の run は、`Machine::run` からも `eml_vm::execute` からも `Ok(Finished { exit: Some(n), .. })` を返し、`Machine` は poisoned になる。
    - 誤りで終わった run は、その誤りを返す。
    - その後の `load`、`run`、`recover`、`close` は、どれも1つの文言 `the driver kept pending ops after quiesce` を返す。
    - REPL は、poisoned にした入力の直後に、その文言でセッションを終える (D-29)。
    - `eml_cli` は予算を出さない。
  - 予算が効くのは、run を止める終わりだけである: `exit`、Ctrl-C と SIGTERM、確保量の上限、致命的な誤り、デッドロック、`Async.run` の外へ出た実行時エラー。どれも静止を通る。スコープの失敗による取り消しは run を止めず、静止に入らない。その `run` は、取り消した `blocking` の呼び出しが終わるまで戻らない (D-4)。予算の間の2回目の Ctrl-C は今のまま `_exit(130)` で終わる。仮想の時計のテストの driver は予算を無視し、仕事をその場で動かす。
- 理由: C の呼び出しが走っている記憶は放さない。それでも run を止める終わりは、利用者から見て1回の予算のうちに届く。失敗したスコープの待ちは Ctrl-C で止められ、そこからは予算が効く。プログラムが選んだ終了コードは届く。`Std.Stream` の意味は変わらない。
- 退けた案
  - ファイルの仕事にも 1 秒の予算: 遅いディスクの `Std.Stream` を後退させ、安全も増えない。
  - worker ごとに 1 秒: N worker で最大 N 秒になる。
  - `Ran::Abandoned` で終了コードを返す: `run_recovering` が `recover` に通し、終了コードが消える。
  - 予算を `Limits` か `Control` に置く: `Limits` は `eml_rt` にあって `Machine` の間で共有され、検証する核を VM の終わり方の方針で広げる。
  - `cfg(test)` か feature の予算: 結合テストから届かず、テストのための欄になる。
- 及ぶ範囲: `eml_rt` の trait、`eml_io` の本物の driver、`eml_vm` の `Run`、`Machine`、`AnyMachine`、`execute`、`eml_cli` の REPL。
- 見直す条件: 予算が、戻るはずの呼び出しを切っている報告か計測が出たとき。
- 適合の確かめ
  - MUST: 上の静止の規則、run ごとの1つの期限、poisoned と漏らすこと、見える結果。MUST NOT: `eml_cli` が予算を出すこと。取り消しの道に予算を持ち込むこと (D-4)。
  - 確かめ方: `driver::tests::quiesce_abandons_a_running_job_after_its_budget`、`driver::tests::quiesce_drops_a_queued_job_unrun`、`driver::tests::quiesce_still_waits_for_a_file_job`、`workers::tests::job_wait_shares_one_deadline`、`machine::every_quiesce_gets_the_job_budget`、`machine::an_exit_with_an_op_left_pending_keeps_its_code_and_poisons`、`ffi_abandon::an_exit_during_a_blocking_call_keeps_its_code_and_poisons` (1 worker) と `…_on_worker_one` (2 worker)、`ffi_abandon::execute_keeps_the_exit_code_of_a_run_that_abandoned_a_job`、`repl::the_machine_is_poisoned_after_an_input_that_abandoned_a_job` が通る。`machine::a_driver_left_pending_poisons_the_machine` (machine.rs:784) は変えずに通る。`grep -rn "job_budget" crates/eml_cli/src` が何も出さない。`grep -c "run を止める終わり" docs/spec/declarations.md docs/spec/vm.md` がどちらも 1 以上。REPL のセッションが終わることは 9 章の手での確かめで見る。
- リポジトリに残す所: docs/spec/vm.md の「時計と driver」の「run の外では」の箇条と「誤り、止めること、終わり」、docs/implementation/architecture.md の「CLI と lib API」、docs/future/multicore.md の「止める手順」(T-3、T-4)、REPL の文 (T-8)

#### D-9: プールの仕事に貸す記憶は、その仕事のスレッドだけが使う

- 決定
  - プールの仕事に貸す記憶 (ファイルの仕事の `RawBuf`、C の仕事のポインタのスロット) を使うのは、その仕事を走らせるプールのスレッドだけである。貸す側は、完了が持ち主に当たるまで物体を生かし、動かさず、書くときは一意に保つ。
  - 前後関係は、プールの列の mutex、inbox の mutex、`wait`/`completions`、登録表の lock でつながる (driver.rs:29-35 がファイルの仕事について同じ鎖を書いている)。docs/future/async.md:263 の「仕事には渡さない」は消す。
  - T16 の窓は「`submit` から、完了が持ち主の worker に当たるまで」になり、別のスレッドの Miri のテストを持つ。本物の driver の `ffi::` のテストを TSan で流し、受け渡しの辺の証拠にする。
- 理由: 2種類の仕事に1つの規則を当てる。
- 及ぶ範囲: vm.md、async.md、object-model.md の T16、`crates/eml_rt/src/heap/tests/trusted.rs`、`crates/eml_vm/src/ffi.rs` の `marshal` の SAFETY のコメント、CI。
- 見直す条件: プールの仕事が持ち主のスレッドの外で完了を当てるようになるとき。
- 適合の確かめ
  - MUST: 上の規則。T16 がこの窓を書き、別のスレッドの Miri のテストを持つ。本物の driver の `ffi::` のテストが TSan で流れる。
  - 確かめ方: `grep -n "仕事には渡さない" docs/future/async.md` が何も出さない。`trusted::another_thread_writes_through_unboxed_ptr_mut_before_the_owner_reads` が Miri の両方の設定で通る。CI の TSan の job が `ffi::` を流して通る。
- リポジトリに残す所: docs/spec/vm.md の「時計と driver」、docs/spec/object-model.md の T16 の文 (T-7) と Miri の欄 (T-5)

#### D-10: 仮想の時計では、`blocking` の仕事は worker 0 でその場で動く

- 決定
  - `Clock::Virtual` では、`blocking` の呼び出しはファイルの仕事と同じ仕事である。`submit` で積み (worker `k` ≥ 1 からは `Submit` の知らせで)、次の `wait` の決着のときに worker 0 の上で受け付けの順に動く。完了の順は seed で混ぜ、結果は seed によらない。
  - ほかのタスクを待つ C の関数は、仮想の時計では動かせない (`wait` か `quiesce` の中で worker 0 を止め、診断はない)。そうしたテストは `-- clock: real` を付け、検査モードと速いモードの 1 worker でだけ流れる。複数の worker の本物の時計は eml_vm のテストで確かめる。
  - テストの driver は仕事をスレッドで動かさず、`JOB_LIMIT` も持たない。
  - declarations.md に「仮想の時計の run (テスト) では、戻らない `blocking` の呼び出しは `exit` でも Ctrl-C でも止まらず、run は終わらない」と書く。
- 理由: 結果がプログラム、方針、seed で決まる。
- 退けた案: テストの driver で仕事をスレッドに載せる。
- 及ぶ範囲: `eml_io::test`、UI の印、testing.md、declarations.md。
- 見直す条件: 仮想の時計で別のタスクを待つ C の呼び出しを試す必要が出たとき。
- 適合の確かめ
  - MUST: 上の動き。MUST NOT: テストの driver が仕事をスレッドで動かすこと。
  - 確かめ方: 鍵の UI のファイル (`gate`、`cancelled`、`many`) が `-- clock: real` を持つ。`grep -n "thread::spawn" crates/eml_io/src/test.rs` が何も出さない。declarations.md の文がある。
- リポジトリに残す所: docs/spec/declarations.md (T-7)、docs/implementation/testing.md の「UI テスト」(T-9)

#### D-11: テストは VM から `Op::Job` までを、眠らず、壁の時間を見ずに通す

- 決定: テストは次のものを覆う。
  - 数えた1つの `Op::Job`
  - 本物の時計の鍵 (待つ間にほかのタスクが動く)
  - 本物の時計の worker 1
  - 仕事が残る `exit` の run と、割り込みで止めた run を、1 と 2 worker で (どちらも poisoned になる)
  - 失敗したスコープが取り消した `blocking` の呼び出しを、テストのスレッドが鍵を開けて終わらせ、`run` が待ちきってから失敗を起こし直すこと (poisoned にならない)
  - poisoned にした入力の後の REPL
  - 2回目の `load` が `CompileError::Poisoned` で断られること
  - 下ろし方の Miri
  - `ffi::` と `ffi_abandon::` の TSan と ASan
  - 鍵の fixture は待ったときに消費し、テストはそれぞれ重ならない番号を使い、`job_budget` は 1 秒より短くする。
- 理由: roadmap の要件。D-2 の記憶の安全の道は、じかに試す必要がある。UI のテストは1つのプロセスの中で、ファイルを2回、並べて流す。スコープの失敗で止まった run は C の関数が戻るまで終わらないので、テストは鍵を外から開ける。UI のテストには鍵を開ける手がないので、この形は eml_vm のテストにだけ置く (T-9 には足さない)。
- 及ぶ範囲: `crates/eml_vm/tests/{ffi,ffi_abandon}.rs`、`crates/eml_vm/tests/common/c.rs`、UI テスト、`eml_test_support`、`crates/eml_cli/tests/repl.rs`、CI。
- 見直す条件: テストの流し方 (同じプロセスで並べて流すこと) が変わるとき。
- 適合の確かめ
  - MUST: 上の範囲。MUST NOT: 眠ること。壁の時間を見ること。
  - 確かめ方: 7 章で名前を挙げたテストがあって通る。`grep -rn "Instant::now\|elapsed" crates/eml_vm/tests/ffi_abandon.rs` が何も出さない。`grep -n "sleep" crates/eml_test_support/src/fixtures.rs crates/eml_vm/tests/common/c.rs` が何も出さない。`ui::fixture_ids_are_disjoint_across_files` が通る。`grep -n "fn an_error" crates/eml_vm/tests/ffi_abandon.rs` が何も出さない (名前は `an_interrupt_…`)。REPL のセッションが終わることは手で確かめる (9 章)。
- リポジトリに残す所: docs/implementation/testing.md の「UI テスト」「AddressSanitizer」「ThreadSanitizer」(T-6、T-8、T-9)

#### D-12: プールの道は、Rust のデータを持つ仕事にも使える形に保つ

- 決定: `eml_rt::driver` と `eml_io` に FFI の型を入れない。
- 理由: この道は `tls-platform-verifier` とネイティブのコードにも使う。
- 及ぶ範囲: `eml_rt`、`eml_io`。
- 見直す条件: なし。
- 適合の確かめ
  - MUST NOT: `eml_rt` か `eml_io` が `CCall`、`CFunction`、`CFnId` を名指すこと。
  - 確かめ方: `grep -rn "CCall\|CFunction\|CFnId" crates/eml_rt/src crates/eml_io/src` が何も出さない (基点でも何も出さない)。
- リポジトリに残す所: 不要 (型の境目そのものが守る)

#### D-13: roadmap の項目は D-8 と同じコミットで消し、`eml_rt` を変えるコミットは門を通す

- 決定: roadmap の「プールの仕事の予算と、放さずに終わる方針」は、D-8 の VM の部分を入れるコミット (T-4) で消す。段の spec の文は、実装するコミットに入れる。`crates/eml_rt` を変えるコミット (T-1、T-3、T-5) は、`crates/eml_rt/CLAUDE.md` の門を通す。
- 理由: 文書は今の状態を書く。`eml_rt` には専用の門がある。
- 及ぶ範囲: 文書とコミットの規律。
- 見直す条件: なし。
- 適合の確かめ
  - MUST: 上のとおり。
  - 確かめ方: T-4 の後に `grep -n "プールの仕事の予算と、放さずに終わる方針" docs/future/roadmap.md` が何も出さない。各タスクの spec の grep が通る。T-1、T-3、T-5 で Verus、verusfmt、Miri の基準が通る。
- リポジトリに残す所: 不要

### 5.2 短中期の設計

理由があれば実装の中で変えてよい。変えたらコミットのメッセージに書く。

#### D-20: 公開の API に足すもの

- 決定: どれも設計の裁定で受け入れた。段を閉じるコミットのメッセージに、受け入れたことを書く
  - `exit` の run で静止の後に操作が残れば、その `exit` の `Finished` を返し (`Machine::run` と `eml_vm::execute` から `Ok(Finished { exit: Some(n), .. })`)、`Machine` を poisoned にする。
  - run ごとに1つの仕事の期限 (`Run::job_wait`) を、その run のすべての静止が分け合う。`recover` は予算をまるごと受け取る。
  - `RunConfig::job_budget` (pub の欄、既定 1 秒) と `RunConfig::with_job_budget(Duration)`、`Machine::with_job_budget(Duration)`。`eml_cli` は出さない。
  - `Machine::is_poisoned(&self) -> bool` と `AnyMachine::is_poisoned(&self) -> bool`。
  - `eml_cli::Repl::is_poisoned(&self) -> bool` (`AnyMachine::is_poisoned` に回す)。`terminal.rs` は `Repl` の非公開の `machine` に届かないので要る。
- 理由
  - 2 worker の見捨てのテストには本物の driver と 1 秒より短い予算が要る (`begin_run` は注入した driver に worker を使わせない)。`cfg(test)` は結合テストから届かず、テストのための欄は CLAUDE.md が禁じるので、公開の設定が正直な形である。
  - `RunConfig` は方針 (`sched`、`clock`、`signals`) を持ち、予算も終わり方の方針である。
  - `execute_here` は `close()` が poisoned の誤りを返して終了コードを隠すので、`is_poisoned` を要る。
- 退けた案: `Repl.broken` を立てる、`Outcome::Poisoned` を足す: どちらも、後の入力に `load` の `CompileError::Poisoned` がすでに与えるものを写し、`Outcome::Poisoned` は poisoned の文言を `eml_cli` に持たせる。

#### D-21: driver の型と `eml_io` の仕事の enum

- 決定
  - `eml_rt::driver` は D-3 の型にする。`Done::Size` の doc (「`Readable`、`Read`、`Write`、`Job` の値」) から `Job` を消す。
  - `eml_io` の内側は `pub(crate) type FileJob = Box<dyn FnOnce() -> io::Result<usize> + Send + 'static>;` と `pub(crate) enum Job { File(FileJob), Value(Box<dyn FnOnce() -> Box<dyn Any + Send> + Send + 'static>) }`。`Job::run(self) -> io::Result<Done>` が両方を `Done` にする。`Answer::Job(thread::Result<io::Result<Done>>)`。
- 理由: 外から見える型は1つで、内側の区別は `eml_io` の中に閉じる。

#### D-22: プールの2つの列

- 決定
  - `State` は `queue` の代わりに `files` と `values` の2つの `VecDeque<(Ticket, Job)>` と、`running_values: usize` を持つ。
  - 空いたスレッドは lock の下で、ファイルの仕事があればいちばん古いものを、なければ `running_values < job_limit` のときいちばん古い値の仕事を取る (そのとき `+= 1`)。どちらもなければ空いたまま待つ。`POOL_IDLE` で起きたときに抜けるのは、始められる仕事がないときである (今の `queue.is_empty()` の条件を置き換える)。
  - `submit` は種類で積み分け、`startable > idle && threads < limit` のときだけスレッドを作る (`startable = files.len() + min(values.len(), job_limit.saturating_sub(running_values))`)。それから `notify_one`。
  - 値の仕事を終えたスレッドは、`running` を外す lock の区間 (pool.rs:136) で `running_values -= 1` する。答えを積んだ後に lock を取り直す loop の頭で、次の始められる仕事を自分で取る。起こし損ねがない。
  - ファイルの仕事は列で待つ `Op::Job` より先に取る。多くのファイルの仕事が C の呼び出しを遅らせることはあるが、逆はない。vm.md にこう書く。
  - スレッドを作れなかったときは両方の列から札で探し、`Drop` は両方の列を取る。
- 理由: 1つの mutex のまま、`submit` が止まらない性質を保つ。

#### D-23: 静止の見捨て方

- 決定
  - `Kind::Job { ticket: Ticket }`。driver に `abandoned: usize` を足し、`pending() = slots.len() + done.len() + abandoned` (mod.rs:892)。`abandoned` は戻さない (VM は driver を落とす)。
  - `Pool::forget(&mut self, ticket) -> Option<Job>` が列で待つ値の仕事を外す。
  - `Driver::with_limits(backend, lookup, limit, job_limit)` を足し、`Driver::new` (mod.rs:251) は定数でそれを呼ぶ。
  - fake の backend に `advance_while_threads_run: bool` を足す。立てると、スレッドが走る間の時間つきの `wait(Some(d))` は `now` を `d` だけ進めてすぐ戻る。`ring_while_threads_run` は、鍵で止めた仕事が鳴らさないベルを待つので使えない。テストは `quiesce` が戻るまで鍵を閉じたままにするので、`quiesce` の中で起きるのは時間が進むことだけで、結果は決まる。
  - `quiesce` が走るのは driver のスレッドで、そのとき VM のコードは動かないので、`quiesce` の間に新しい `Kind::Job` の slot は増えない。
  - 見捨てた仕事の遅れた答え (panic の payload を含む) は、札がないので `answer_arrived` の `continue` (mod.rs:560) で捨てる。
- 理由: 走っている仕事の答えは札を外して捨て (名前の解決と同じ)、列の仕事は始めないので、見捨てた後に仕事が始まることはない。

#### D-24: run の期限と、`exit` の終了コードを返す枝

- 決定
  - `Run` (workers.rs:299-327) に `pub(crate) job_budget: Duration` と `job_deadline: OnceLock<Duration>` を足す。`job_budget` は `Run::new` で `JOB_QUIESCE_LIMIT` にし (`Run::new` の引数は変えない)、`Machine` が `Arc::new(run)` (lib.rs:931) の前に自分の値を入れる。`job_deadline` は run の時間の基点の値で、`driver.now()` で読む。
    ```rust
    /// 静止で仕事を待てる残り。run の最初の静止が期限を決め、ほかの worker はそれを分け合う。
    pub(crate) fn job_wait(&self, now: Duration) -> Duration {
        self.job_deadline.get_or_init(|| now.saturating_add(self.job_budget)).saturating_sub(now)
    }
    ```
  - `retire_driver` (workers.rs:1256) は `driver.quiesce(self.run.job_wait(driver.now()))`、worker 0 (lib.rs:1055) は `driver.quiesce(sched.run.job_wait(driver.now()))`、`Machine::recover` (lib.rs:520) は `self.job_budget` をまるごと渡す。
  - `kept_pending` の枝 (lib.rs:663-677) は、`self.poisoned = true; let flushed = ctx.flush();` の後に次を置く。
    ```rust
    let exit = ctx.heap().control().exit_code();
    // 貸したままの仕事があっても `exit` の終了コードは返す。ヒープは写さず、終わりの検査もしない
    if let (Some(code), Ok(()) | Err(RuntimeError::Fault { fault: VmFault::Stopped(Stop::Ended), .. })) = (exit, &result) {
        flushed.map_err(|fault| after(fault.into(), &task))?;
        return Ok(Ran::Done(self.finished(&counts, &scheduled, Some(code)), None));
    }
    ```
    lib.rs:713-722 と :743-752 の2つの `Finished { … }` を `fn finished(&self, counts: &RunCounts, scheduled: &Scheduled<M>, exit: Option<i64>) -> Finished` にまとめ、3か所で使う。前の木 (e97567b2) の試しでは、この枝が今の `Err("… kept pending ops after quiesce in `entry$main`")` を `Ok(Some(3))` に変え、`machine::` はすべて通った。8d6a813a では試していない。T-4 の完了の基準 (`machine::` が通り、`an_exit_with_an_op_left_pending_keeps_its_code_and_poisons` が `Some(3)` を見る) が確かめる。
  - `execute_here` (lib.rs:280-292): `let finished = machine.run(loaded.entry)?; if machine.is_poisoned() { return Ok(finished); } machine.close()?;`。
- 理由: `Ran` は非公開なので、規則は見える結果で書き、実装はこの枝で満たす。`recover` は poisoned の `Machine` では呼ばれない。

#### D-25: `OpKind::Ffi` と cells の2つの関数

- 決定
  ```rust
  /// `blocking` の C の呼び出し。`call` は `CodeBase` の C の関数の番号、`ret` はプールが返した語。記録はヒープの語を
  /// 持たず、引数は待つタスクの継続が持つ (docs/spec/vm.md の「`blocking` の呼び出し」)
  Ffi { call: u32, ret: Option<u64> },
  ```
  ```rust
  #[allow(clippy::too_many_arguments)]
  pub(crate) fn submit_job<M: Mode>(shared: &Shared, here: &Here, ctx: &mut Ctx<M>,
      driver: Option<&mut (dyn IoDriver + 'static)>, worker: u32, depth_offset: usize,
      me: CellRef, call: u32, job: Op) -> Result<i64, CellFault>
  pub(crate) struct FfiTaken { pub(crate) call: u32, pub(crate) result: io::Result<u64> }
  pub(crate) fn take_ffi<M: Mode>(shared: &Shared, ctx: &mut Ctx<M>, token: i64, worker: u32) -> Result<FfiTaken, CellFault>
  ```
  - `prefix()` (:474) は `"cannot call a C function"`、`cost()` は既定の枝 (1)。
  - `advance` (:3489) は `(OpPhase::Transfer, OpKind::Ffi { .. }, Ok(Done::Job(v)))` を `u64` に downcast し (失敗は `CellFault::Internal("an op completes with another kind of result")`)、`ret = Some(r)`、結果 `Ok(0)` にする。`Err(e)` はほかの種類と同じ。
  - `submit_job` の記録は `net_record(OpKind::Ffi { call, ret: None }, worker, None)` で、host は `here.host`。`record(..)` と同じ lock の区間で `if cells.point(me)? { record.phase = OpPhase::Done(Err(io::Error::other("cancelled")), true); record.cancelling = true; op = None }`。それから `run_io` を呼ぶ。非公開の `submit` (:4240) に、取り消しの地点を確かめるタスク `Option<CellRef>` を渡す形で書く (今の呼び手は `None` を渡す)。
  - `take_ffi`: ほかの種類の記録は Internal。`Done(Ok(_))` で `ret: Some(r)` なら `Ok(r)`、`Done(Err(e))` なら `Err(e)`。記録を放す。
  - cells.rs が import するのは `std` と `eml_rt` だけのまま。
- 理由: 今の記録と待ちの道に、種類を1つ足すだけで済む。

#### D-26: 行の見え方 `CSubmit` と `CTake`

- 決定
  - `ExternRef` に `CSubmit(CFnId)` と `CTake(CFnId)` を足す。`std()` は両方に `None` を返す。
  - `CFunction` に非公開の欄 `submit_row: Option<OwnedRow>` と `take_row: Option<OwnedRow>` を足し、`CFunction::new` が `blocking` のときだけ `c.rs` の中で作る。読み手は `pub fn submit_row(&self) -> Option<FunctionRow<'_>>` と `pub fn take_row(&self) -> Option<FunctionRow<'_>>` (`self.x_row.as_ref().map(OwnedRow::view)`)。`ExternRef::row` の枝は `CSubmit(id) => c[id.0 as usize].submit_row().expect("a blocking C function")`、`CTake` も同じ。2つの欄は `CFunction::new` がほかの欄から決めるので、`Linked::fits` と `CompileError::Mismatch` の `==` の比べは変わらない。`renumbered` (compile/mod.rs:275-298) も `CFunction::new` を通るので、作り直した関数も2つの行を持つ。
  - 表

    | | 引数 | 借用 | 結果 | 印 | Purity | faults | with_args | writes |
    |---|---|---|---|---|---|---|---|---|
    | `CSubmit` | C の行の引数 | C の行と同じ (`Obj` の引数をすべて借りる) | `Int` (`Std.Async.OpId`、`Lin`) | C の行と同じ | `Effectful` | 狭める引数があるとき | `[]` | C の行と同じ |
    | `CTake` | `[Int, Enum] ++ [Obj; m]` | `[false, false] ++ [true; m]` | C の行の結果と印 | `[Lin, Unr] ++` C の行の `Obj` の引数の印 | `Effectful` | いつも | C の行の `Lin` の引数 (どれも `Obj`) の、`take` の引数の中の位置 (2 + `Obj` の引数の中の順位) | C の行と同じ |

    `held` はどちらもすべて false。`writes` は3つの見え方とも `c.rs` の `writes(&params)` から取る。`CSubmit` と `CTake` は `Effectful` なので、`writes` によらず `removable()` は false である。
  - `CTake` の `Obj` の引数を VM は読まない。すべての物体の最後の使い道を `take` 以後に置き、プールが読む間に Perceus が使わない `Unr` の引数を落とさないためである。
  - `lib.rs`: `RuntimeEntry::IoWait` (`Std.Async.io_wait`、引数 `[Repr::Int]`) を `ALL` に足す。`pub fn ret(self) -> Repr` を足す。値は箱の形の Repr (`boxed_repr` と同じ規則) で、今の3つは `Unit`、`IoWait` は `Obj` である。下ろした後の `io_wait` の `ret` は構造体 `Repr::Struct(_)` (fields `[(Enum, Unr), (Int, Lin)]`) になる。`params` の doc の「結果はどれも `Unit` である」を消す。型の doc は「ランタイムか処理系が名前で呼ぶ」にする。
  - `crates/eml_extern/src` には `unsafe` の語を書かない (コメントも含む。CI の dependency edges の step が確かめる)。
- 理由: 2つの値のために型 (`BlockingRows`) を足すより、非公開の欄2つと読み手2つのほうが表に出る面が小さい。

#### D-27: translate の下ろし方

- 決定
  - `Instances` (instances.rs:167) に `runtime: FxHashMap<RuntimeEntry, InstanceId>` を足し、`RuntimeEntry::ALL` を根にする所 (:837-847) で `collector.add` の返す番号を入れる。`translate` (translate/mod.rs:325-341) は `indices` を決めた後に `builder.runtime = instances.runtime の各項を indices で FnIdx にした表` を入れる。`ProgramBuilder` に `runtime: FxHashMap<RuntimeEntry, FnIdx>` と `fn runtime_target(&self, RuntimeEntry) -> Option<FnIdx>` を足す。`extern_call` が `ProgramBuilder` の関数で、`extern_wrapper` からも呼ばれるので、表は `BodyCtx` でなく `ProgramBuilder` に置く。
  - `extern_call` (program.rs:583) で、`par` の枝の後、`ext` が `ExternRef::C(id)` で `self.c_functions[id.0 as usize].blocking` のとき:
    ```rust
    let op = builder.bind(VarInfo::new("op", Repr::Int, Mark::Lin),
                          Rhs::Extern { ext: ExternRef::CSubmit(id), args: args.clone(), at });
    let io_wait = self.runtime_target(RuntimeEntry::IoWait)
        .expect("a blocking row names `Async`, so `Std.Async` is loaded");
    // io_wait の結果 (Wake, OpId) は構造体の Repr なので、型を持たない組の形で同じ StructId を作る
    let pair_repr = self.tuple_struct(self.tuple_layout(2), vec![(Repr::Enum, Mark::Unr), (Repr::Int, Mark::Lin)]);
    let pair = builder.var(VarInfo::new("r", pair_repr, Mark::Lin));
    builder.emit(Stmt::Let { var: pair, rhs: Rhs::call(Call::Direct(io_wait, vec![Atom::Var(op)])) });
    let (w, op) = (builder.var(VarInfo::new("w", Repr::Enum, Mark::Unr)), builder.var(VarInfo::new("op", Repr::Int, Mark::Lin)));
    builder.emit(Stmt::Unpack { value: pair, ctor: Ctor { layout: self.tuple_layout(2), tag: TUPLE }, fields: vec![w, op] });
    // take の引数は [op, w] に、C の行の borrowed が true の引数 (物体) を順に続けたもの。
    // その後は今の extern_call の続きを ext = ExternRef::CTake(id) で使う。with_args が空なら bind、そうでなければ lending_call
    ```
  - `MutUnboxedArray` の引数があれば `CTake.with_args` がそれを指すので、`take` の後の `con` は C が書いた配列を返す。
  - `extern_wrapper` (:537) は `blocking` の行に `yields = true` を渡す (:571 の `builder.finish(name, true, false, …)` の3つ目)。:569-570 のコメント (「包む関数は extern を呼ぶだけなので yield しない」) を、`blocking` の行の例外を含む形に直す。
  - `std` のモジュールは import したときだけ読み込み、行に `Async` を書くには `Std.Async` を import する必要があるので、`runtime_target` は見つからないことがない。`expect` はこの不変条件を書く。
  - テキストの形 (`crates/eml_lir/src/syntax.rs` の共通の Reader/Writer): `extern submit Mod.f(args)@loc` と `extern take Mod.f(op, w, objs…)@loc`。Reader の `extern_call` (syntax.rs:917-929) は、名前の前の語 `submit` か `take` を読み、名前を `ExternRef::from_name` で引いて `CFnId` を包む。`submit` と `take` は修飾のない語なので、std の extern の名前 (`Std.…`) と取り違えない。往復できること。Core IR と LIR の表示が同じ形を書くこと (`lowering::every_program_lowers_to_the_same_text` が T-9 の UI のファイルで確かめる)。

#### D-28: VM の2つの命令と `ffi.rs`

- 決定
  - `op.rs`: `FfiSubmit { dst: Slot, at: u32 }` と `FfiTake { dst: Slot, at: u32 }`。被演算子は `Op::Ffi` と同じく `[CodeBase の C の関数の番号, 引数の数, スロット…]`。`emit.rs` (:526-534) は `CSubmit(id)` と `CTake(id)` を `links.c_functions[id.0 as usize]` で同じ形に出す。
  - `disasm.rs`: `ffi submit s{dst} = {name}({args})` と `ffi take s{dst} = {name}({args})`。
  - `here!` は vm.rs の中の `macro_rules!` で ffi.rs から見えないので、受け付けは2つに分ける。仕事を作る前半を ffi.rs に、登録表に出す後半を vm.rs に置く。
    ```rust
    // crates/eml_vm/src/ffi.rs
    /// `Op::FfiSubmit` の前半。範囲の外の引数は記録を作らずに `Panic` を上げ、`None` を返す。
    pub(crate) fn job<R: Exec>(code: &CodeBase<R::M>, ctx: &mut Ctx<R::M>, task: &mut Task,
        dst: Slot, at: u32) -> Result<Option<(u32, eml_rt::Op)>, VmFault>;
    /// `Op::FfiTake`。記録を放し、`Wake` で分ける。
    pub(crate) fn take<R: Exec>(code: &CodeBase<R::M>, ctx: &mut Ctx<R::M>, task: &mut Task,
        cells: &cells::Shared, dst: Slot, at: u32) -> Result<(), VmFault>;

    // crates/eml_vm/src/vm.rs の step (:918) の腕
    Op::FfiSubmit { dst, at } => ffi_submit::<R>(code, ctx, task, sched, dst, at)?,
    Op::FfiTake { dst, at } => ffi::take::<R>(code, ctx, task, &sched.cells, dst, at)?,
    // vm.rs の ffi_submit
    let Some((c, job)) = ffi::job::<R>(code, ctx, task, dst, at)? else { return Ok(()) };
    let me = current_cell(task)?;
    sched.own_driver()?;
    here!(sched, post, here);
    let token = cells::submit_job(&sched.cells, &here, ctx, sched.driver.as_deref_mut(),
        task.worker, task.depth_offset, me, c, job)?;
    task.set_raw(dst, token);
    ```
    - `job`: `let ops = &code.tables.operands[at as usize..]; let c = ops[0]; let entry = &code.c_functions[c as usize];`。`let args = match marshal(ctx, task, &entry.function, &ops[2..2 + ops[1] as usize])? { Ok(args) => args, Err(why) => { raise::<R>(code, ctx, task, dst, &entry.function, why)?; return Ok(None) } };` (記録はまだないので、範囲の Panic は記録を作らない)。`let call: CCall = entry.call;`。仕事は `eml_rt::Op::Job(Box::new(move || -> Box<dyn Any + Send> { Box::new(unsafe { (call.bridge)(call.callee, args.as_ptr()) }) }))` で、SAFETY のコメントは `call` のもの (`Linked::new` の約束、`fits`、slot の数) に、ポインタの slot の窓が T16 の「`submit` から、完了が持ち主に当たるまで」であることを足す。命令の `Op` と名前が重なるので、`eml_rt` の `Op` は修飾して書くか別名で import する。
    - `take`: 被演算子は `[c, argc, op, w, objs…]`。先に `cells::take_ffi(cells, ctx, token, task.worker)` (記録を放す)。`w` の `Enum` のスロットは、vm.rs の既存の `Wake` の読み方に合わせて読み、`cells::WOKE` と `cells::CANCELLED` で比べる。それから次の表で分ける。

      | `w` | 答え | 動き |
      |---|---|---|
      | `Woke` | `Ok(r)` | `match unmarshal(code, ctx, &entry.function, r)? { Ok(out) => write_out(task, dst, out), Err(why) => return raise::<R>(code, ctx, task, dst, &entry.function, why) }`。null のハンドルは `Raise::NullHandle` で、何も確保しない |
      | `Woke` | `Err(e)` | `failed::<R>(code, ctx, task, dst, Fault::Raised(Box::new(Raised { kind: io_kind(&e), message: format!("cannot call `{}`: {e}", entry.function.name).into() })), Purity::Effectful)` |
      | `Cancelled` | どれも | `entry.function.ret` が `CRet::Handle(h)` で答えが `Ok(r)`、`r != 0` なら、`CHandle { ptr: ptr::without_provenance_mut(r as usize), dtor: code.c_handles[h.0 as usize].dtor }` を作ってすぐ落とす (`CHandle` の `Drop` が dtor を呼ぶ。ハンドルの番号は `CEntry.function` で付け替え済み)。それから `failed::<R>(…, Fault::Raised(Box::new(Raised { kind: cells::IO_CANCELLED, message: "task was cancelled".into() })), Purity::Effectful)` |

    - `Woke` の `take` はハンドルを普通の `Lin` の物体で返すので、その後の取り消しは次の取り消しの地点で届き、巻き戻しが dtor ごと落とす。
  - `vm.rs` の `failed`、`io_kind`、`current_cell` と `trait Exec` の可視性は、ffi.rs から呼べる `pub(crate)` にする (今 `Exec` は ffi.rs が使っているので見えている)。
  - 仕事が捕まえる `CCall` は `CEntry.call` の `Copy` の値である。`CCall` は `Send` (eml_cabi:17-20) なので、closure は `Send` になる。
- 理由: ffi-c の `marshal`、`unmarshal`、`raise`、`write_out`、`CHandle` をそのまま使い回し、変換を1か所に保つ。登録表に触れる所は、今の `Std.Net` の受け付けと同じく vm.rs に置く。

#### D-29: REPL は poisoned にした入力の直後に終わる

- 決定
  - `repl.rs`: `pub fn is_poisoned(&self) -> bool { self.machine.is_poisoned() }`。入力そのものの結果は変えない (`exit` なら `Exited(n)`、誤りなら `Failed` か `Broken`)。`Repl` は新しい状態を持たない。後の `eval` は `load` で `CompileError::Poisoned` になり、`Outcome::Broken` で poisoned の文言を返す。
  - `terminal.rs`: `Flow::Poisoned` を足す。`Terminal::eval` と `Terminal::load_text` は、`show(..)` がどの flow を返しても (`Flow::Interrupted` も `Flow::Broken` も含めて)、その後に `self.repl.is_poisoned()` を確かめ、真なら `Flow::Poisoned` を返す。主のループ (:86-111) は `Flow::Poisoned => return terminal.close(ExitCode::from(1))`、起動のときの `load_text` (:74-78) も同じにする。`steps` は `Continue` でない flow で止まり、`Read::Eof` の枝 (:95-98) もほかの flow を通すので、`Flow::Poisoned` は外へ出る。
  - `close` は `Repl::close` を1回呼び、poisoned の `Machine::close` は `poisoned_fault("<close>")` を返すので、セッションは `runtime error: the I/O driver failed: the driver kept pending ops after quiesce in `<close>`` を1回書いて 1 で終わる。poisoned の `close` はヒープに触れないので、`Flow::Broken` と重なっても `close` を呼んでよい。
  - いちばんよくある道は、戻らない `blocking` の呼び出しの間の Ctrl-C である。run は割り込みの誤りで終わり、見捨てた仕事が `Machine` を poisoned にし、`show` は `Flow::Interrupted` を返す。だから flow によらずに確かめる。
- 理由: poisoned の `Machine` は後の入力をすべて断るので、その入力でセッションを終える。`Terminal::close` は `close` の誤りを `runtime error:` で書いて 1 で終わる唯一の道で、`eml_cli` は文言を写さずに済む。
- 退けた案: `Flow::Broken` を使う: その道は意図して `close` を呼ばず、poisoned の誤りを書かない。

#### D-30: 鍵の fixture と番号の範囲

- 決定
  - UI テストとトランスクリプトの鍵 (`crates/eml_test_support/src/fixtures.rs`):
    ```rust
    /// 鍵 `id` を開く。開いた鍵は1回の `gate_wait` が消費する
    pub extern "C" fn eml_fixture_gate_open(id: i64);
    /// 鍵 `id` が開くまで待ち、消費して 0 を返す。10 秒の見張りで開かなければ -1
    pub extern "C" fn eml_fixture_gate_wait(id: i64) -> i64;
    /// 鍵 `id` を待ってから、ハンドル `h` の値 (`eml_fixture_handle_get` と同じ) を返す
    pub unsafe extern "C" fn eml_fixture_handle_wait(h: *mut c_void, id: i64) -> i64;
    ```
  - 状態は `static GATES: Mutex<HashSet<i64>>` と `Condvar` で、`wait_timeout_while` で待つ (眠らない)。開くと番号を入れて知らせ、待つと番号が入るまで待って外す。開くと待つは、どちらが先でも1回ずつ対になり、2回目の run に状態を残さない。
  - 3つとも `fixture_symbols()` に入れる。UI テストとトランスクリプトは `eml_test_support::link` の `LINKER` で解決する。`eml_test_support` は `pub use fixtures::{eml_fixture_gate_open, eml_fixture_gate_wait, fixture_symbols};` で、Rust のテストが鍵を開け待ちできるようにする (`fixtures` の module は非公開のまま)。
  - eml_vm のテストは ffi-c の形に従い、`crates/eml_vm/tests/common/c.rs` に自前の `pub extern "C" fn rs_gate_open(id: i64)`、`extern "C" fn rs_gate_wait(id: i64) -> i64`、`extern "C" fn rs_handle_wait(h: i64, id: i64) -> i64` (詰めた整数のハンドル) と、橋渡し `i64_to_void` を足して `calls()` に並べる (T-8)。`eml_test_support` の fixture のハンドルは本物のポインタで、eml_vm のテストのハンドル (詰めた整数) と混ぜられないためである。
  - ハンドルとその数は ffi-c の `eml_fixture_handle_new(tag, value)` と `eml_fixture_frees(tag)` (eml_vm では `rs_handle_new` と `rs_frees`) をそのまま使う。`blocking` の足し算は ffi-c の `eml_fixture_add_i32` (eml_vm では `rs_add_i32`) を `blocking` で宣言する。
  - 番号の規則 (testing.md:427 の今の規則に足す)
    - `tests/ui` の fixture のファイル (`-- fixtures:` の行を持つファイル) は、どれも `-- fixture ids: A..B` の行を1つ持つ。範囲は半開きで空でなく、ファイルどうしで重ならない。ファイルが使うハンドルの tag と鍵の番号は、その範囲の中にある。これは今の規則で、T-9 がテスト `ui::fixture_ids_are_disjoint_across_files` で確かめるようにする。
    - ffi-c のファイル (`ffi/`) は 1 から 99 を使う (今の規則)。ffi-blocking のファイル (`async/` の `blocking_fixture_*.em`) は 1000 以上を使う。Rust のテストは 100000 以上を使い、テストごとに重ねない (testing.md の取り決めだけで、検査はしない)。eml_vm のテストが使う 100002 から 100007 は避ける。
- 理由: UI テストは1つのプロセスで並べて流れ、`frees` の数は tag で分かれるので、番号が重なると数が狂う。鍵も同じプロセスの1つの表を共有する。

#### D-31: テストの置き場所と sanitizer

- 決定
  - 見捨てのテストは `crates/eml_vm/tests/ffi_abandon.rs` に分ける (`tests/main.rs` で宣言する)。poisoned の `Machine` を意図して漏らすので、ASan はこのモジュールだけを `ASAN_OPTIONS=detect_leaks=0` の別の step で流す。記憶の誤りは確かめ、leak の検出だけを切る。`ffi::` の step は leak の検出を既定のままにする。TSan は eml_vm の step に `ffi_abandon::` を足す。
  - 本物の時計のテストは `Clock::Real`、`with_job_budget(Duration::from_millis(20))`、100000 以上でテストごとに違う鍵と tag を使い、終わりに鍵を開けて切り離したスレッドを終わらせる。壁の時間は読まない。
  - REPL の確かめは、本物の時計と短い予算が要るので、トランスクリプトでなく `crates/eml_cli/tests/repl.rs` の Rust のテストにする。
- 理由: 1秒の規則と、機械の速さによらない結果の規則を守る。

## 6. 実装の決まり

- 共通の完了の基準 (どのタスクも、自分の完了の基準に加えて満たす)
  - `cargo nextest run`、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check` が通る。
  - タスクの前に `cargo nextest list --workspace --message-format oneline | sort > $SCRATCH/tests-before.txt`、後に同じものを `tests-after.txt` に取る (`$SCRATCH` はリポジトリの外の作業用のディレクトリ)。`comm -23 tests-before.txt tests-after.txt` はタスクが消すか改名すると書いたテストだけを、`comm -13` はタスクが名前を挙げた新しいテストだけを出す。ディレクトリをまとめて流す UI のテストは、ファイルごとには一覧に出ない。
  - 新しい item は、`pub` にするか同じコミットで使う。`dead_code` の lint を `allow` や `expect` で消さない。
- 見出しを引く規則: コメントか文書が `docs/….md の「見出し」` を引くのは、その見出しを作るコミットか、その後のコミットだけである。`crates/eml_cli/tests/citations.rs::every_cited_heading_exists` (:153) が確かめる。この段が作る見出しは vm.md の `### \`blocking\` の呼び出し` だけで、T-7 が `### C の関数の命令` の直後、`### \`--link\`` の前に作る。基点にある見出しはどのタスクからも引ける (D-13)。
- 途中の panic: この段では、どのコミットの後にも、正しいプログラムが `unimplemented!` か assert に届く状態は生まれない見込みである。`blocking` は T-7 まで E0004 で断られ、T-7 が型、translate、emit、VM を1つのコミットで入れるためである。実装の途中でそうした状態が見つかったら、そのコミットのメッセージに書き、直すタスクを決めてから進める (D-1)。
- テストの変更の種類 (docs/implementation/testing.md の「テストの変更の運用」)。この文書の承認が、種類1と種類2の合意になる。理由はコミットのメッセージにも書く。
  - 種類1 (成否の変更)
    - `tests/ui/check-fail/ffi/blocking.em` を消し、`tests/ui/check-fail/ffi/blocking_effects.em` を足す (T-7)。`blocking` が書けるようになり、E0004 の確かめが意味を失うため。
    - ffi-c の型のテスト `blocking_is_e0004_after_a_correct_row` (`crates/eml_types/tests/ffi.rs:273`) を消し、同じファイルに `a_blocking_row_needs_io_and_async` を足す (T-7)。同じ理由。
  - 種類2 (期待値の変更)
    - `externs::every_runtime_entry_is_a_monomorphic_std_function` と `externs::the_runtime_entries_have_the_declared_abi` (`crates/eml_core_ir/tests/externs.rs:738`、:779) が、結果の Repr を `Repr::Unit` の決め打ちでなく `entry.ret()` と比べる (T-7)。`io_wait` を根にするため。`every_runtime_entry_is_a_monomorphic_std_function` は `boxed_repr` の結果を `entry.ret()` と比べる (`IoWait` は `Obj`)。`the_runtime_entries_have_the_declared_abi` は、`entry.ret()` が `Obj` の entry では `function.ret` が `Repr::Struct(id)` で `core.structs.shape(id).fields` が `[(Enum, Unr), (Int, Lin)]` であることを、ほかの entry では `function.ret == entry.ret()` を確かめる。8d6a813a の scratch の複製で `IoWait` を `ALL` に足すと、`eml_core_ir` の結合テストで落ちたのはこの2件だけで (497 件が通った。`lowering::` と stage-dump の snapshot を含む)、落ちた値は `Obj` (:767) と `Struct(StructId(0))` (:798) だった。`eml_vm` の `bench::`、`io::`、`tasks::` (127 件) も変わらずに通った。
    - `io_wait` を根にしたことで、`Std.Async` を読み込むが `io_wait` を呼ばないプログラムの Core IR と LIR の stage-dump の snapshot に `Std.Async.io_wait` の関数が残る場合は、その snapshot の書き換え (T-7)。どの snapshot が変わったかをコミットのメッセージに挙げる。それ以外の snapshot が変われば止めて報告する。
    - `real::a_job_pool_is_capped` を 32 から 24 にする (T-2)。
    - `Op::Job` が `io::Result<usize>` を返す `eml_io` のテスト (`driver/tests/mod.rs` 8、`tests/real.rs` 9 で `pool_threads_block_interrupt_and_terminate` を含む、`tests/test_driver.rs` 1) が、箱の値を返して downcast する (T-1)。確かめる数は変えない。
  - 種類3 (機械的な追随)
    - `Done` を `assert_eq!` で比べる所 (1行のもの 36 と、複数行のもの。コンパイラが示すすべて) を `assert!(matches!(..))` にする。`Answer::Job(Ok(Ok(n)))` を `Answer::Job(Ok(Ok(Done::Size(n))))` にする (T-1)
    - `Kind::Job` の照合を `Kind::Job { .. }` に (T-3)
    - `Pool::new(Arc::clone(&inbox), 2)` を `Pool::new(Arc::clone(&inbox), 2, 2)` に (T-2)
    - `eml_io` のテストの `quiesce()` を `quiesce(Duration::from_secs(10))` に、eml_vm のテストの `IoDriver` の stub と `NoDriver` を `_jobs: Duration` に、委ねる包みは `quiesce(jobs)` に (T-3)
- 設計を既存のテストに合わせて曲げない。並べた enum の種類、旗、テストのための欄、spec の例外を作らず、テストの変更をここに書く (CLAUDE.md)。
- どのテストも、ふつうの run (`cargo nextest run`) で1つだけ流して1秒以内に終わり、結果が機械の速さによらない。新しい本物の時計のテストは 20 ms の予算を使う (D-11、D-31)。
- spec の文は、規則を実装するコミットに入れる。コードのコメントはその節を引き、この計画を引かない (roadmap の「進め方」)。
- 日本語のコメントと文書は、書く前に `natural-japanese:natural-japanese` の skill を呼び、その規則で書く (CLAUDE.md)。コメントは why を書き、規則は `docs/` の道で引く。
- `unsafe` を書く所は、ffi-c と同じく SAFETY のコメントを付ける。`Linked::new` を呼ぶのは `crates/eml_vm/tests/common/c.rs::linked` だけのままにし、テストは `Linked` を自分で組まない。`Linker::with_symbols` を呼ぶテストは、`crates/eml_cli/tests/repl.rs` の `linker()` を使う。
- `crates/eml_rt` を変えるコミット (T-1、T-3、T-5) は `crates/eml_rt/CLAUDE.md` の門を通す: `cargo verus verify -p eml_rt`、`verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')`、Miri の2つの設定 (`-Zmiri-strict-provenance` と、それに `-Zmiri-tree-borrows` を足したもの) と時間の予算 (D-13)。
- ffi-c の方針のうち、この段のコードにも当てはまるもの
  - 橋渡しの関数は `CSignature` で決まり、callee を最初の引数にとり、`Ctx`、`Word`、ヒープの配置、スレッドローカル、記号の名前を引かない。`Bridge`、`Dtor`、`CCall`、`Linked` は `eml_cabi` にある。プールの仕事はこの `CCall` を使う (D-2)。
  - 変換 (marshal と unmarshal) は `crates/eml_vm/src/ffi.rs` に1回だけ書き、両方のモードで共有する。範囲の確かめは呼ぶ前、null の確かめは呼んだ直後で、どちらも `abort_to` で `Panic` を起こす (D-28)。
  - C 由来の `ExternRef` (`C`、`CSubmit`、`CTake`) を照合してよい所は、`crates/eml_extern/src/`、`crates/eml_core_ir/src/translate/`、Core IR と LIR のテキスト (`crates/eml_core_ir/src/text.rs`、`crates/eml_lir/src/{syntax,text}.rs`)、`crates/eml_vm/src/{compile/emit.rs,ffi.rs,disasm.rs}` だけである。確かめ方: `grep -rln "ExternRef::C" crates/*/src` がこれらの下のファイルだけを出す (基点では `eml_core_ir/src/translate/program.rs`、`eml_extern/src/lib.rs`、`eml_vm/src/compile/emit.rs`)。
  - 中断の判断は `row.faults`、消すかは `removable()`、VM の中断の振り分けは `purity` で決める (D-5)。`writes` の行は、消す、まとめる、写す、並べ替えるパスから見て、借りた `MutUnboxedArray` の引数への書き込みとして扱う。
  - `Lin` の引数は借りて `(R, lin args)` の組で返し、C の行は引数を消費しない。値を返す行 (同期の C と `CTake`) の `with_args` は自分の `Lin` の引数の位置で、`CSubmit` は `[]` である。`MutUnboxedArray` を貸した後の組は、C が書いた配列を返す。
  - eml_vm のテストと Miri は、`common::c::linked` で作った `Linked` (Rust の橋渡しの関数と自前の `rs_*`) で C の関数を呼び、Cranelift を使わない。確かめ方: `grep -rn "eml_native\|cranelift" crates/eml_vm/Cargo.toml crates/eml_vm/tests` が何も出さない。
  - 命令の被演算子は `Index.c_functions` で名前から決めた `CodeBase` の番号で、逆アセンブラは正規の名前を書く。
  - `eml_vm` はリンクした code を `load` に渡す `Linked` の値でだけ受け取り、linker、factory、trait object を持たない。`RunConfig` と `Machine` に linker の欄はない。確かめ方: `grep -rn "Linker\|CLinker" crates/eml_vm/src` が何も出さない (`crates/eml_vm/src` の識別子とコメントに `Linker` を書かない)。
- そのほかの約束
  - cells.rs が import するのは `std` と `eml_rt` だけ (D-25)。
  - 仕事の中身は `Ctx`、ヒープ、RC に触れない (D-2)。
  - `IoDriver` は `!Send` のまま。
  - `JOB_QUIESCE_LIMIT` は `eml_vm`、`JOB_LIMIT` と `POOL_LIMIT` は `eml_io` に置く (D-7、D-8)。
  - 新しい enum の種類 (`ExternRef::CSubmit`/`CTake`、`OpKind::Ffi`、`Done::Job`) を入れるとき、`ExternRef::Std(..)` を irrefutable に分解する所を作らない。

## 7. タスク

| ID | タスク | もとの決定 | 前のタスク | 並べて進められるか |
|---|---|---|---|---|
| T-1 | 仕事が箱の Rust の値を返す (`Done::Job`) | D-3、D-12、D-21 | - | いいえ |
| T-2 | `Op::Job` をプールの 24 本に抑える | D-7、D-22 | T-1 | いいえ |
| T-3 | `quiesce(jobs)`: `Op::Job` の待ちの上限と見捨て | D-8、D-12、D-23 | T-2 | いいえ |
| T-4 | VM の仕事の予算、run ごとの期限、`exit` の終了コード、`is_poisoned` | D-8、D-13、D-20、D-24 | T-3 | いいえ |
| T-5 | `unboxed_ptr_mut` を通した別のスレッドの書き込みの Miri のテスト | D-9 | - | A 列 |
| T-6 | 鍵の fixture | D-11、D-30 | - | A 列 |
| T-7 | 記録、行、下ろし方、VM の命令: `blocking` がプールで動く | D-1、D-2、D-4、D-5、D-6、D-9、D-10、D-25 から D-28 | T-1 | いいえ |
| T-8 | 本物の時計と見捨てのテスト、`execute` の終了コード、REPL の終わり | D-6、D-8、D-9、D-11、D-20、D-24、D-29、D-30、D-31 | T-4、T-6、T-7 | いいえ |
| T-9 | `blocking` の UI テストと番号の範囲の検査 | D-5、D-10、D-11、D-30 | T-6、T-7 | いいえ |
| T-10 | `docs/future` のプールの記述の書き直し | D-2、D-8、D-9 | T-7 | いいえ |
| T-11 | 段を閉じる (`Close ffi-blocking`) | D-7、D-8、D-13 | すべて | いいえ |

A 列 (T-5、T-6) は T-1 から T-4 と独立に進められる。1本のブランチで1つずつ commit する。既定の順は表の上から下である。

### T-1: 仕事が箱の Rust の値を返す (`Done::Job`)

- 目的: `Op::Job` が `Box<dyn Any + Send>` を返し、`Done::Job` で終わるようにする (D-3、D-12)。型 (このタスク) と意味 (T-3) を分け、`Done` の比べの書き換えが見捨ての論理を埋もれさせないようにする。
- 対象: `crates/eml_rt/src/driver.rs` (:65、:73-76)、`crates/eml_io/src/driver/{backend,inbox,pool,mod}.rs`、`crates/eml_io/src/driver/tests/{fake,mod}.rs`、`crates/eml_io/src/sys/unix.rs` (`file_job` :953)、`crates/eml_io/src/sys/iocp.rs` (:1226)、`crates/eml_io/src/test.rs` (:177)、`crates/eml_io/tests/{real,test_driver}.rs` と、`Done` を比べるほかのテスト (3 章の一覧)、`docs/spec/vm.md` の「時計と driver」
- 作業
  1. `eml_rt::driver`:
     ```rust
     /// … 値を返し、`Done::Job` で終わる。失敗は値の中に書く。
     Job(Box<dyn FnOnce() -> Box<dyn Any + Send> + Send + 'static>),
     pub enum Done {   // PartialEq と Eq は持たない
         Size(usize), Listener { .. }, Stream { .. }, Accepted { .. }, Handle(IoHandle),
         /// `Job` が返した値。受け取る側が型を知っていて、`downcast` で取り出す。
         Job(Box<dyn Any + Send>),
     }
     impl fmt::Debug for Done { /* derive と同じ。Done::Job(_) は "Job(..)" */ }
     ```
     doc には、仕事の `Err` は「driver が仕事を走らせられなかった」ことだけを指すと書き、`Done::Size` の doc から `Job` を消す。
  2. `eml_io` (backend.rs:13 の `pub(crate) type Job` を置き換える):
     ```rust
     pub(crate) type FileJob = Box<dyn FnOnce() -> io::Result<usize> + Send + 'static>;
     pub(crate) enum Job { File(FileJob), Value(Box<dyn FnOnce() -> Box<dyn Any + Send> + Send + 'static>) }
     impl Job {
         pub(crate) fn run(self) -> io::Result<Done> {
             match self { Job::File(f) => f().map(Done::Size), Job::Value(f) => Ok(Done::Job(f())) }
         }
     }
     ```
     - `Backend::file_job(..) -> FileJob` (unix、iocp、fake)。
     - `Answer::Job(thread::Result<io::Result<Done>>)`。スレッドを作れなかったときは今のまま `Answer::Job(Ok(Err(e)))` (pool.rs:103)。
     - `pool::work` は `catch_unwind(AssertUnwindSafe(|| job.run()))` を走らせる。
     - `Driver::submit` は `Job::File(..)` か `Job::Value(job)` で包む。
     - `answer_arrived` (mod.rs:567): `Answer::Job(Ok(result)) => self.done.push(Completion { key, result, cancelled: false })`。
     - `test.rs:177`: `Op::Job(job) => done(Ok(Done::Job(job())))`。`test.rs:452` の `let _ = job();` はそのまま通る。
     - eml_vm に `Done` を網羅する `match` があれば (cells.rs、sched.rs)、`Done::Job(_)` を「ほかの種類の結果」の枝に入れる (コンパイラが示す。種類3)。
  3. vm.md の「時計と driver」: `Op::Job` は所有した Rust の値を `Done::Job` で返し、`Err` は driver が仕事を走らせられないときだけであると書く。
  4. テスト
     - 新規: `driver::tests::a_job_returns_its_boxed_value` (fake の backend。`Box::new(42u64)` を返し、`Ok(Done::Job(v))` を 42 に downcast する) と `test_driver::a_job_value_is_delivered_at_wait`。
     - 種類3と種類2の書き換えは 6 章のとおり。
- 実装者に任せること: ほかの種類の `Debug` の文字 (既定は derive と同じ)。`Job::run` の置き場所 (既定は backend.rs)。
- 条件: いつも行う。実装者は Sonnet。
- 完了の基準
  - `cargo nextest run -p eml_rt -p eml_io` → 失敗なし。`-E 'test(a_job_returns_its_boxed_value) | test(a_job_value_is_delivered_at_wait)'` で 2 件が通る
  - `cargo clippy -p eml_io -p eml_vm --all-targets --no-default-features -- -D warnings` → 0 で終わる
  - `cargo verus verify -p eml_rt` → `0 errors`。`verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` → 0 で終わる
  - `grep -n -B3 "^pub enum Done" crates/eml_rt/src/driver.rs | grep -c "PartialEq"` → `0`
  - `grep -rn "CCall\|CFunction\|CFnId" crates/eml_rt/src crates/eml_io/src` → 何も出さない
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_io --lib -- driver::tests::` → 通る。`-Zmiri-tree-borrows` を足しても通る
  - 6 章の共通の完了の基準
- 落とし穴: `Done` から `PartialEq` を外すと、1行の `assert_eq!` だけでなく複数行の `assert_eq!` も落ちる。grep でなくコンパイラの示す所をすべて直す。`Completion` (driver.rs:205-212) はもとから derive を持たないので変えない。`Submit` は `Debug` を derive して `io::Result<Done>` を持つので、手で書いた `Debug` が要る。

### T-2: `Op::Job` をプールの 24 本に抑える

- 目的: 2つの列にする。値の仕事が同時に走るのは `JOB_LIMIT = 24` までで、ファイルの仕事は 32 本のどれも使える (D-7、D-22)。
- 対象: `crates/eml_io/src/driver/pool.rs`、`crates/eml_io/src/driver/mod.rs` (`Pool::new` :254)、`crates/eml_io/src/driver/tests/mod.rs` (`mod pool` :1071)、`crates/eml_io/tests/real.rs` (`a_job_pool_is_capped` :187)、`docs/spec/vm.md` の「時計と driver」、`docs/implementation/architecture.md` (:376)、`docs/implementation/testing.md` (:250)
- 作業
  1. pool.rs:
     ```rust
     pub(crate) const POOL_LIMIT: usize = 32;
     /// 同時に走る `Op::Job` の上限。残りのスレッドは、戻らない C の呼び出しの下でもファイルの仕事に残す。
     pub(crate) const JOB_LIMIT: usize = 24;
     struct State {
         files: VecDeque<(Ticket, Job)>, values: VecDeque<(Ticket, Job)>,
         running_values: usize,
         threads: usize, idle: usize, running: HashSet<ThreadId>, closed: bool,
     }
     pub(crate) fn new(inbox: Arc<Inbox>, limit: usize, job_limit: usize) -> Pool
     ```
     取り方、`submit` のスレッドの作り方、`work` の数え方、空いたスレッドの抜け方は D-22 のとおり。スレッドを作れなかったときは両方の列から探し、`Drop` は両方の列を取る。
  2. vm.md: プールは driver ごとに1つで 32 本、`Op::Job` は同時に 24 本まで、それを超えると順に待つ。空いたスレッドは、列で待つ `Op::Job` より先にファイルの仕事を取るので、多くのファイルの仕事が C の呼び出しを遅らせることはあるが、逆はない。
  3. architecture.md:376 の `driver/pool.rs` の箇条に `JOB_LIMIT` を足す。testing.md:250 の「5. 上限の振る舞いは小さな上限で確かめる」に、プールの上限を 3/2 で試すことを書く。
  4. テスト
     - 新規 `driver::tests::pool::jobs_beyond_the_job_limit_wait_while_a_file_job_gets_a_thread`: `Pool::new(inbox, 3, 2)`。鍵で止めた値の仕事を3つ出すと2つが始まり、3つ目は始まらずスレッドも作らない。ファイルの仕事は3本目のスレッドを得て、2つの鍵が閉じたまま答える。鍵を1つ開けると、待っていた値の仕事が始まる。`fake.spawned.get() == 3`。
     - 種類3: `Pool::new(Arc::clone(&inbox), 2)` → `Pool::new(Arc::clone(&inbox), 2, 2)`。
     - 種類2: `real::a_job_pool_is_capped` を 32 から 24 に (24 を鍵で止め、25 個目は列で待ち、`submit` は戻る)。
- 実装者に任せること: 欄の名前。`startable` を `State` の method にするか関数にするか。
- 条件: T-1 の後。実装者は Opus (起こし損ねのない数え方に判断が要る)。
- 完了の基準
  - `cargo nextest run -p eml_io -E 'test(pool::) | test(a_job_pool_is_capped)'` → 失敗なし
  - `cargo nextest run -p eml_io` → 失敗なし
  - `grep -n "JOB_LIMIT: usize = 24" crates/eml_io/src/driver/pool.rs` → 1 行
  - Miri の `eml_io --lib -- driver::tests::` が両方の設定で通る (CI は user 時間を 30 秒と 35 秒の予算と比べる)
  - 6 章の共通の完了の基準
- 落とし穴: 値の仕事を終えたスレッドが `running_values` を減らすのを `running` を外す区間の外に置くと、ほかのスレッドが古い数を見て起こし損ねる。`running` を外す区間で減らし、答えを積んだ後の loop の頭で次を取る。

### T-3: `quiesce(jobs)`: `Op::Job` の待ちの上限と見捨て

- 目的: `IoDriver::quiesce(&mut self, jobs: Duration)` を入れ、ファイルの仕事は上限なしに、`Op::Job` は `jobs` まで待ち、残りを見捨てる (D-8 の driver の部分、D-12、D-23)。
- 対象
  - `crates/eml_rt/src/driver.rs` (:274)
  - `crates/eml_io/src/driver/mod.rs` (`Kind::Job` の照合 :140、:148、:244、:811、:834、`pending` :892、`quiesce` :938-985)、`crates/eml_io/src/driver/pool.rs`、`crates/eml_io/src/driver/tests/{fake,mod}.rs`、`crates/eml_io/src/real.rs` (:91)、`crates/eml_io/src/test.rs` (:448)、`crates/eml_io/tests/{real,test_driver}.rs`
  - `crates/eml_vm/src/driver.rs`、`crates/eml_vm/src/lib.rs` (:520、:1055)、`crates/eml_vm/src/workers.rs` (:1256)、`crates/eml_vm/src/cells.rs` (`NoDriver` の `quiesce` :6034)
  - テストの stub: `crates/eml_vm/tests/io.rs` (`fn quiesce` :340、:561、:1806。委ねる呼び出し :562、:1807)、`tests/machine.rs` (:685、:886)、`tests/api.rs` (:430、:719。委ねる呼び出し :720)。`grep -rn "fn quiesce" crates/eml_vm/tests` で漏れを探す
  - `docs/spec/vm.md` の「時計と driver」(`quiesce` の項と、:531 の `Drop` の箇条は変えない)
- 作業
  1. trait の doc: 取り消せるものを取り消す。ファイルの仕事は上限なしに待つ。`Op::Job` は `jobs` まで待ち、まだ残るものをそれぞれ見捨てる。走っているものはそのスレッドで走り続け、答えは捨てる。列で待つものは走らせずに捨てる。見捨てた仕事は `pending()` に残る。見捨てても driver は壊れない。
  2. `driver/mod.rs`: `Kind::Job { ticket: Ticket }` にし (:811 で `self.ticket(key)` の札を入れる)、ほかの照合を `Kind::Job { .. }` にする (種類3)。`abandoned: usize` を足し、`pending() = slots.len() + done.len() + abandoned`。`quiesce` の取り消しの loop を次の形にする。
     ```rust
     let job_deadline = self.backend.now().checked_add(jobs);
     loop {
         self.discard_done();
         if self.broken.is_some() || self.slots.is_empty() { break; }
         let now = self.backend.now();
         if job_deadline.is_some_and(|d| now >= d) && self.abandon_jobs() { continue; }
         // timeout は、今の CANCEL_LIMIT からの値と、Kind::Job の slot が残る間の job_deadline - now の小さいほう
         if self.turn(timeout).is_err() { break; }
     }
     ```
     `abandon_jobs(&mut self) -> bool` は何か外したら true を返す。`Kind::Job { ticket }` の slot ごとに、slot を外し、`self.tickets.remove(&ticket)`、pool の lock の外で `drop(self.pool.forget(ticket))`、`self.abandoned += 1`。
  3. `pool.rs`: `pub(crate) fn forget(&mut self, ticket: Ticket) -> Option<Job>`。
  4. `Driver::with_limits(backend, lookup, limit, job_limit)` を足し、`Driver::new` は定数でそれを呼ぶ。
  5. `fake.rs`: 設定 `advance_while_threads_run: bool` (D-23)。
  6. `test.rs`: `fn quiesce(&mut self, _jobs: Duration)`。動きは変えない。`real.rs:91` は `self.0.quiesce(jobs)`。
  7. `crates/eml_vm/src/driver.rs`: `pub(crate) const JOB_QUIESCE_LIMIT: Duration = Duration::from_secs(1);` を lib.rs:520、lib.rs:1055、workers.rs:1256 に渡す (T-4 が3つとも置き換える)。
  8. vm.md の `quiesce` の項に上の規則を書く。`Drop` は変えない。
  9. テスト (`driver/tests/mod.rs`)
     - `quiesce_abandons_a_running_job_after_its_budget`: `advance_while_threads_run`。鍵で止めた仕事を1つ出し、`quiesce(10 ms)` が戻る。`pending() == 1`。鍵を開けて `fake.finished` を待ち、`wait(Some(0))` の後に完了は空で `pending() == 1`。
     - `quiesce_drops_a_queued_job_unrun`: `with_limits(.., 1, 1)`。A は鍵で止めて走らせ、B は `AtomicBool` を立てる仕事。`quiesce(10 ms)`。`pending() == 2`。A を開けて join し、旗は false のまま。
     - `quiesce_still_waits_for_a_file_job`: 鍵で止めた fake のファイルの仕事。`quiesce(Duration::ZERO)`。始まった後に開け手が鍵を開ける。`quiesce` は `pending() == 0` で戻り、仕事は終わっている。
     - 種類3は 6 章のとおり。`quiesce_waits_for_a_running_job` (driver/tests/mod.rs:840) と `real::quiesce_drains_a_pending_probe_a_full_write_and_a_running_job` (real.rs:469) は `quiesce(Duration::from_secs(10))` で今の期待のまま通る。
     - Miri で流すテストの形は testing.md の「核の Miri」の 1 から 4 に従う (見捨てた仕事も、本体が答えを積み核が引き取るまで進めてから driver を落とす)。
- 実装者に任せること: なし (`abandoned` は戻さない)。
- 条件: T-2 の後。実装者は Opus。
- 完了の基準
  - `cargo nextest run -p eml_io -E 'test(quiesce_)'` → 失敗なし。上の3つの名前が通る
  - `cargo verus verify -p eml_rt` → `0 errors`。verusfmt の確かめ → 0 で終わる
  - `grep -n "fn quiesce(&mut self, jobs: Duration)" crates/eml_rt/src/driver.rs` → 1 行
  - `grep -n "JOB_QUIESCE_LIMIT" crates/eml_vm/src/*.rs` → 定義と3つの使い道
  - Miri の `eml_io --lib -- driver::tests::` が両方の設定で通る
  - 6 章の共通の完了の基準
  - このコミットの後、CI が4つの OS と Miri の job で緑になる
- 落とし穴
  - 見捨てた後も、ファイルの仕事は上限なしに待つ。見捨てたスレッドもファイルの仕事も同じ 32 本を使うが、ファイルの仕事は 8 本を保つので遅れない。
  - fake の時間を進める設定は、鍵が閉じている間だけ結果を決める。テストは `quiesce` が戻るまで鍵を開けない。

### T-4: VM の仕事の予算、run ごとの期限、`exit` の終了コード、`is_poisoned`

- 目的: run ごとに1つの仕事の期限を持たせる。静止の後に操作が残る `exit` の run は終了コードを返し、`Machine` を poisoned にする (D-8 の VM の部分、D-13、D-20、D-24)。
- 対象: `crates/eml_vm/src/lib.rs` (`RunConfig` :180-199、`Machine` :306-、`AnyMachine`、`recover` :516-536、`kept_pending` の枝 :663-677、`exit` の道 :697-724、成功の `Finished` :743-752、run を作る所 :931、worker 0 の静止 :1055)、`crates/eml_vm/src/workers.rs` (`Run` :299-327、`retire_driver` :1252-1265、`mod tests` :1352)、`crates/eml_vm/tests/machine.rs`。文書: `docs/spec/vm.md` の「時計と driver」の「run の外では」の箇条 (:541) と「誤り、止めること、終わり」、`docs/future/roadmap.md` (:436-438 を消し、「その後の項目」に1つ足す)、`docs/future/multicore.md` の「止める手順」の 3、`docs/future/async.md` の「ブロックする呼び出しのプール」の表の `exit` の行、`docs/implementation/architecture.md` の「CLI と lib API」
- 作業
  1. D-24 の `job_budget`、`job_deadline`、`job_wait` を `Run` に足し、3か所の静止を置き換える。
  2. `RunConfig { pub job_budget: Duration }` (既定 `JOB_QUIESCE_LIMIT`) と `with_job_budget`、`Machine { job_budget }` と `with_job_budget`。`AnyMachine::new` が渡す。
  3. `Machine` と `AnyMachine` に `pub fn is_poisoned(&self) -> bool`。
  4. D-24 の `kept_pending` の枝と `fn finished`。
  5. 文書
     - vm.md の「run の外では」: run は走っている `Op::Job` を合わせて 1 秒 (`RunConfig::job_budget`) まで待つ。期限は run の最初の静止から数え、すべての worker が分け合う。`recover` は予算をまるごと受け取る。それでも走る仕事は見捨て、`Machine` は poisoned になってヒープを漏らす。`exit` の run は終了コードを返す。その後の `load`、`run`、`recover`、`close` は `the driver kept pending ops after quiesce` を返す。Ctrl-C、致命的な誤り、デッドロックも同じ道を通る。
     - vm.md の「誤り、止めること、終わり」: 仕事を見捨てた `exit`。
     - roadmap: 「プールの仕事の予算と、放さずに終わる方針」(:436-438) を消す。「その後の項目」に「プールの上限 24/8 と静止の予算 1 秒」を足し、入れ子の箇条に「後に回した理由: 既定は計測なしで選んだ。」と「見直す条件: 計測か報告で、ファイルの仕事か C の呼び出しが上限で待つのが見えたとき、または予算が戻るはずの呼び出しを切っているのが見えたとき。」を置く (決めた日は書かない)。
     - multicore.md の「止める手順」の 3: 「プールの仕事は取り消せないので、終わるまで待つ」を「`Op::Job` は run で合わせて予算 (1 秒) まで待ち、残りを見捨てる」にする。async.md の表の `exit` の行: 予算の後に見捨て、poisoned、ヒープを漏らし、終了コードは返す。
     - architecture.md の「CLI と lib API」: `RunConfig::job_budget`、`with_job_budget`、`Machine::with_job_budget`、`Machine::is_poisoned`、`AnyMachine::is_poisoned`。
     - REPL の文 (vm.md の「追記」、modules.md、multicore.md、async.md の REPL の行) は T-8 で REPL のコードと一緒に入れる。
  6. テスト
     - `machine::every_quiesce_gets_the_job_budget`: `Machine::with_driver` で注入する記録の driver。`now()` は固定で、`quiesce(jobs)` が `jobs` を記録する。既定の run の最初の静止は `1 s`、`with_job_budget(5 ms)` なら `5 ms`。誤る run (Panic) で `run_recovering` が `recover` を呼び、`recover` は予算をまるごと受け取る。
     - `machine::an_exit_with_an_op_left_pending_keeps_its_code_and_poisons`: 注入した driver (`wait` → `Ok(true)`、`submit` → `Pending`、`quiesce` は `pending` を残す) と次のプログラムで、タスク x が止まったまま `kept_pending` の枝に届く見込みである (前の木 e97567b2 の試しで確かめ、8d6a813a では `eml check` が通ることだけを確かめた)。届かなければ止めて報告する。`f.exit == Some(3)`、`is_poisoned()`、`close()` の誤りの文字に `the driver kept pending ops after quiesce` が入ることを確かめる。
       ```
       import Async (Async)
       import Async
       import Net
       body : Unit -> <IO, Panic, Async> Unit
       body () =
         let x = Async.start (fn () -> Net.close_listener (Net.listen "127.0.0.1" 0))
         let y = Async.start (fn () -> (exit 3 : Unit))
         Async.await x
         Async.await y
       main : Unit -> <IO, Panic> Unit
       main () = Async.run body
       ```
     - `machine::a_main_exit_with_an_op_left_pending_keeps_its_code_and_poisons`: x を始めた後の `body` で `exit 3`。
     - `workers::tests::job_wait_shares_one_deadline` (`src/workers.rs` の `mod tests`): 予算 1 秒。`job_wait(10 ms)` = 1 s、`job_wait(400 ms)` = 610 ms、`job_wait(2 s)` = 0。
     - `machine::a_driver_left_pending_poisons_the_machine` (machine.rs:784) は変えない。
- 実装者に任せること: テストの driver の名前。
- 条件: T-3 の後。実装者は Opus。
- 完了の基準
  - `cargo nextest run -p eml_vm --test integration -E 'test(machine::)'` → 失敗なし
  - `cargo nextest run -p eml_vm --lib -E 'test(job_wait_shares_one_deadline)'` → 1 件が通る
  - `cargo nextest run -p eml_vm --test integration -E 'test(bench::)'` → 通り、`bench.rs` の snapshot は変わらない
  - `cargo clippy -p eml_vm --all-targets --no-default-features -- -D warnings` → 0 で終わる
  - `grep -n "プールの仕事の予算と、放さずに終わる方針" docs/future/roadmap.md` → 何も出さない
  - `grep -rn "job_budget" crates/eml_cli/src` → 何も出さない
  - `grep -n "Instant::now\|elapsed" crates/eml_vm/tests/machine.rs | grep -i job` → 何も出さない
  - 6 章の共通の完了の基準
- 落とし穴: 期限を worker ごとに持たせると N worker で N 倍になる。`OnceLock` を `Run` に1つだけ置く。

### T-5: `unboxed_ptr_mut` を通した別のスレッドの書き込みの Miri のテスト

- 目的: 持ち主が join の後に読む生の配列に、別のスレッドが書いてよいことの Miri の証拠を置く (D-9)。
- 対象: `crates/eml_rt/src/heap/tests/trusted.rs`、`docs/spec/object-model.md` の T16 の行 (:1380) の Miri の欄
- 作業
  1. テストだけを足し、`both_modes!` に登録する (trusted.rs:2626 の `c_writes_through_unboxed_ptr_mut_and_the_heap_reads_it` の隣)。形は `c_writes_through_unboxed_ptr_mut_and_the_heap_reads_it` (trusted.rs:2451) と `a_lent_room_is_written_by_another_thread` (:803) に合わせる: `n in [1, 3, LARGE_ELEMENTS]` で `heap.alloc_unboxed(n, 0, false)`、`let p = shared.unboxed_ptr_mut(w).unwrap();` (`unsafe`、`&self`、要素 0 を指す `Result<*mut u64, HeapError>`)。
  2. `p` をテストの中の `struct SendPtr(*mut u64); unsafe impl Send for SendPtr {}` (SAFETY のコメントつき) に入れ、`std::thread::scope` で spawn したスレッドに包みごと移す (provenance を保つ)。
  3. そのスレッドが `i in 0..n` で `p.add(i)` に書く。scope の終わり (join) の後、持ち主が `unboxed_len` と `unboxed_word` ですべての要素を読む。`decref` の後に `mortal(&mut heap) == 0`。
  4. object-model.md の T16 の Miri の欄に、「別のスレッドが `unboxed_ptr_mut` の位置から書いた要素を、join の後に持ち主のヒープの操作が読むこと」を足す。T16 の窓の文は T-7 で広げる。
- 実装者に任せること: なし。
- 条件: いつも行う (ffi-c が `unboxed_ptr_mut` を入れている)。実装者は Sonnet。
- 完了の基準
  - `cargo nextest run -p eml_rt -E 'test(another_thread_writes_through_unboxed_ptr_mut)'` → 2 件 (両方のモード) が通る
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::` → 通る。`-Zmiri-tree-borrows` を足しても通る
  - 足した user 時間が `crates/eml_rt/CLAUDE.md` の予算に収まる (Tree Borrows で 5 秒以下。`trusted::` 全体は 280 s と 365 s 以下)。数をコミットのメッセージに書く
  - `cargo verus verify -p eml_rt` → `0 errors`。verusfmt の確かめ → 0 で終わる
  - 6 章の共通の完了の基準
- 落とし穴: ポインタを `usize` にして渡すと、`-Zmiri-strict-provenance` で落ちる。閉包が欄ごとに捕まえると生のポインタだけが移って `Send` でなくなるので、包みごと移す。

### T-6: 鍵の fixture

- 目的: 待ったときに消費する鍵を、ffi-c の fixture の一覧に足す (D-11、D-30)。
- 対象: `crates/eml_test_support/src/fixtures.rs`、`crates/eml_test_support/src/lib.rs` (:21 の `pub use`)、`crates/eml_test_support/tests/support.rs`、`docs/implementation/testing.md` の「UI テスト」の `ffi/` の段落 (:427)
- 作業
  1. D-30 の3つの関数と `GATES` を足し、`fixture_symbols()` に入れる。`eml_fixture_handle_wait` は `eml_fixture_handle_get` と同じく `unsafe extern "C"` で、SAFETY のコメントを付ける。
  2. lib.rs:21 を `pub use fixtures::{eml_fixture_gate_open, eml_fixture_gate_wait, fixture_symbols};` にする (:20 の `#[cfg(feature = "run")]` の下のまま)。
  3. testing.md:427 に1文を足す。「`async/` の `blocking_fixture_*.em` は 1000 から上の番号を使い、ハンドルの tag にも鍵の番号 (`eml_fixture_gate_open`、`eml_fixture_gate_wait`、`eml_fixture_handle_wait` の番号) にも、ファイルに書いた範囲 `-- fixture ids: A..B` の中の番号を使う。鍵の表もプロセスで1つなので、tag と同じく番号を分ける。」
  4. テストは `support.rs` に置き、3つとも `#[cfg(feature = "run")]` を付ける (support.rs は `run` の import をすでにこの cfg で囲んでいる)。`tests/main.rs` は `mod support;` を宣言済みなので、名前は `support::…` になる。番号は 100200 から 100299 の、テストごとに違うものを使う。
     - `support::an_open_pairs_with_one_wait`: 開いてから待つと 0 が返る。同じ番号をもう一度開いて待つと、また 0 が返る。消費 (2回目の待ちが2回目の開けまで戻らないこと) は時間を見ずには観測できないので、このテストは確かめない。消費は `GATES` から外す作りで決まる。
     - `support::a_wait_returns_after_another_thread_opens`: スレッドが待ち、主のスレッドが開け、`join` の結果が 0。
     - `support::fixture_symbols_lists_the_gates`: `fixture_symbols()` が3つの名前を含む。
- 実装者に任せること: 名前のほかはない。
- 条件: いつも行う。実装者は Sonnet。
- 完了の基準
  - `cargo nextest run -p eml_test_support` → 失敗なし。上の3件が通る
  - `grep -n "sleep" crates/eml_test_support/src/fixtures.rs` → 何も出さない
  - `cargo clippy -p eml_test_support --no-default-features --features core -- -D warnings` と `cargo clippy -p eml_test_support --all-targets -- -D warnings` → 0 で終わる
  - 6 章の共通の完了の基準
- 落とし穴: 10 秒の見張りは、テストが壊れたときにプロセスを止めないためのもので、ふつうの run では届かない。見張りの時間でテストの成否を決めない。

### T-7: 記録、行、下ろし方、VM の命令: `blocking` がプールで動く

- 目的: `blocking` を書けるようにし、端から端まで動かす (D-1、D-2、D-4、D-5、D-6、D-9、D-10、D-25 から D-28)。E0004 を外すと emit も新しい行を扱う必要があり、VM のテストはパイプライン全体を要し、cells の関数は VM の命令より前にはテストのほかに使い手がなく `-D warnings` で落ちるので、1つのコミットにする。
- 対象
  - `crates/eml_vm/src/cells.rs` (`OpKind` :415、`prefix` :474、`cost` :524、`advance` :3489、`submit` :4240、`net_record` :4430、`take_op` :3764/:3794、テスト)
  - `crates/eml_extern/src/{c.rs,lib.rs}` (`CFunction` c.rs:118-185、`RuntimeEntry` lib.rs:2232-2268、`ExternRef` :2356-2395)、`crates/eml_extern/tests/c.rs`
  - `crates/eml_types/src/ffi.rs` (:95-100 の E0004) とそのテスト `crates/eml_types/tests/ffi.rs`
  - `crates/eml_core_ir/src/translate/{program,instances,mod}.rs` (`extern_call` :583、`extern_wrapper` :537、`lending_call` :626、`Instances` :167、根 :837-847、`indices` :325-341)
  - `crates/eml_lir/src/syntax.rs` (:917-929 の読み手と、extern の書き手)
  - `crates/eml_vm/src/{op,vm,disasm,ffi}.rs`、`crates/eml_vm/src/compile/emit.rs` (:526-534)
  - テスト: `crates/eml_core_ir/tests/{externs,c_functions,lowering,text}.rs`、`crates/eml_lir/tests/{unwind,text}.rs`、`crates/eml_vm/tests/{ffi,miri}.rs`、`tests/ui/check-fail/ffi/blocking.em` と `blocking_effects.em`
  - 文書: `docs/spec/vm.md`、`docs/spec/declarations.md` の「`extern "C"`」(:160-226)、`docs/spec/diagnostics.md` (:56、:172、:175)、`docs/spec/stdlib.md` の「`Std.Stream`」と「`Std.Async`」、`docs/spec/core-ir.md` の「組を返す extern」、`docs/spec/lir.md` の「巻き戻しの地点」、`docs/spec/object-model.md` (T16 :1380)、`docs/future/async.md` (:263)
- 作業 (コミットの中では a0 から e の順に進め、それぞれの後に `cargo nextest run -p <crate>` を流す)
  - a0. 最初の確かめ (D-1 の前提)。`RuntimeEntry::IoWait` を `ALL`、`name`、`params` にだけ足し、`cargo nextest run -p eml_core_ir --test integration` を流す。落ちるのが `externs::every_runtime_entry_is_a_monomorphic_std_function` (左 `Obj`) と `externs::the_runtime_entries_have_the_declared_abi` (左 `Struct(_)`) の2件だけで、後者の `params` と `split` と `internal` の assert は通ることを確かめる。ほかが落ちるか、`io_wait` が evidence の引数を持つなら、止めて報告する (D-1 を変えない)。
  - a. cells.rs: D-25 のとおり。
  - b. eml_extern: D-26 のとおり。
  - c. 型と translate
    - ffi-c の `blocking` の E0004 (eml_types/src/ffi.rs:94-104) を消し、正しい形をそのまま返す。E2023 は今の `expected` (:497-) のままで D-5 の規則に合っている (`pure` と `blocking` は E0018 で一緒に書けないので、`blocking` の行はいつも `IO` を要する)。
    - D-27 のとおりに下ろす。`display` (program.rs:729-750) が、直接の呼び出しが `Obj` の組を返して `Unpack` が続く前例である。
  - d. テキストと VM: D-27 のテキストの形、D-28 の命令、`ffi.rs`、`vm.rs` の dispatch。eml_vm のテストは `common::c::linked(&program, &calls(), &dtors())` (ffi.rs の `linked_all` と同じ) で `Linked` を作り、`blocking` で宣言した既存の `rs_add_i32`、`rs_handle_new`、`rs_fill` を呼ぶ。このタスクのテストは仮想の時計だけなので、鍵の関数は要らない (T-8 が足す)。eml_vm のテストに Cranelift と `eml_native` を入れない。
  - e. spec
    - vm.md: `## C の関数の呼び出し` の下、`### C の関数の命令` (:1007) の直後で `### \`--link\`` (:1032) の前に `### \`blocking\` の呼び出し` を作る。中身は、下ろし方、`take` の表、IO の文言、`RuntimeEntry::IoWait`、前の確かめ、`Op::FfiSubmit` と `Op::FfiTake`、仕事が `CCall` と `u64` のスロットを捕まえること。
    - vm.md の「操作の記録」: `OpKind::Ffi`。
    - vm.md の「時計と driver」の `RawBuf` の箇条 (:520) の隣: プールの仕事に貸す記憶は、それを走らせるスレッドだけが使い、貸す側は完了が持ち主に当たるまで物体を生かし、動かさず、書くときは一意に保つ (D-9)。
    - declarations.md の「`extern "C"`」の `blocking` の段落 (:226 の E0004 の文を置き換える)
      - 取り消しの地点である。
      - プールのスレッドで走り、unix では SIGINT と SIGTERM が止められている (今の `spawn_masked` の動き。`real::pool_threads_block_interrupt_and_terminate` が確かめている)。
      - C の関数は戻らなければならず、eml を呼んではならない。ポインタは戻るまで有効である。
      - 取り消された呼び出しの副作用は残り、返ったハンドルは壊す。
      - `MutUnboxedArray` を渡すと、呼んだ側が受け取る配列は C が書いた配列である。
      - D-7 の文。
      - 戻らない呼び出しは run の終わりを予算 (合わせて 1 秒) だけ遅らせ、その後プロセスは放さずに終わり、`exit` は終了コードを保つ。
      - D-10 の仮想の時計の文。
    - diagnostics.md: E0004 の一覧 (:56、:172) から `blocking` を外し、:175 の「型検査は、行が正しい `blocking` の C の関数の、`blocking` の語に出す」を消して「E0004 を出すのは HIR である」の形に直す。
    - stdlib.md の「`Std.Stream`」: D-7 の文。「`Std.Async`」: 処理系は `blocking` の行のためにも `io_wait` を呼ぶ。
    - core-ir.md の「組を返す extern」: 4つの文の下ろし方とテキストの形。lir.md の「巻き戻しの地点」: `submit` は狭める引数があるときだけ、`take` はいつも巻き戻しの地点である。lir.md の「縮約」には文を足さない (`CSubmit` と `CTake` は `Effectful` なので `removable()` は成り立たない)。
    - object-model.md の T16: 窓「C の関数を呼ぶ間」と「呼び出しの間」を「同期の呼び出しでは呼ぶ間、`blocking` の呼び出しでは `submit` から、完了が持ち主の worker に当たるまで」に広げ、プールのスレッドを読み手と書き手に入れる。`ffi.rs` の `marshal` の SAFETY のコメント (「C は呼び出しの間だけポインタを使い」) も同じ窓に直す。
    - async.md: 「仕事には渡さない」(:263) の文を消す (節の残りは T-10)。
  - テスト
    - 種類1と種類2は 6 章のとおり。`blocking_effects.em` には E2023 だけを出すケースを書く (たとえば `Async` のない `blocking` の行)。
    - cells の単体テスト (cells.rs の `mod tests`): `a_job_record_takes_the_boxed_answer`、`a_cancelled_task_submits_no_job` (数える driver が `submit` を見ない。記録は `Done(Err, true)` で `cancelling`。stats は変わらない)、`a_job_answer_of_another_type_is_an_internal_error`。
    - `crates/eml_extern/tests/c.rs`: `c::a_blocking_row_splits_into_submit_and_take`。両方の見え方の引数、`borrowed`、印、`purity == Effectful`、`faults`、`with_args` (submit は `[]`、take は D-26 の位置。`Int` の引数が `Lin` の引数より前にある行で、位置が `2 + Obj の引数の中の順位` になること)、`writes` (`MutUnboxedArray` の引数を持つ行では `C`、`CSubmit`、`CTake` の3つとも true、持たない行では3つとも false)、両方の見え方の `removable()` が false。同期の行 (`blocking` でない) の `submit_row()` と `take_row()` が `None`。
    - `crates/eml_core_ir/tests/lowering.rs`: `lowering::a_blocking_call_waits_between_submit_and_take` (`Pass::Translate` の inline snapshot。関数は `yields fn`) と `lowering::a_blocking_wrapper_yields`。
    - テキストの往復: `crates/eml_lir/tests/text.rs` と `crates/eml_core_ir/tests/text.rs` に `c_tables_with_blocking_calls_read_back`。
    - `crates/eml_lir/tests/unwind.rs`: `a_submit_without_narrowing_is_no_unwind_point`、`a_submit_with_narrowing_is_an_unwind_point`、`a_take_is_an_unwind_point` (既存の `an_io_c_row_without_narrowing_is_no_unwind_point` :330 と `a_narrowing_c_row_is_an_unwind_point` :343 の隣)。
    - `crates/eml_types/tests/ffi.rs`: `a_blocking_row_needs_io_and_async` (正しい行は診断なし、`Async` のない行は E2023)。
    - eml_vm の `ffi::` (テストの driver。それぞれ `common::c::linked` で作った `Linked` を `Machine::load(&program, &linked)` に渡す): `ffi::a_blocking_call_is_submitted_as_a_job` (`Machine::with_driver` の数える包み。`Op::Job` 1つ、`Done::Job` 1つ)、`ffi::a_cancelled_task_submits_no_job`、`ffi::a_cancelled_blocking_call_destroys_its_handle_result` (`rs_handle_new` を `blocking` で宣言し、tag 100011。`rs_frees 100011` が 1)、`ffi::a_blocking_call_whose_job_cannot_run_fails_with_io` (`Op::Job` に `Submit::Done(Err)` を答える包み。文言 `` cannot call `Main.f`: … `` を確かめる)、`ffi::a_blocking_range_panic_creates_no_record` (`rs_add_i32` を `Int as int` の引数で宣言する)、`ffi::a_blocking_null_handle_panics` (tag 100012、値が負)、`ffi::two_loads_of_a_blocking_row_compare_equal` (同じプログラムを同じ `Linked` で2回読み込む。`fits` の assert も `Mismatch` も起きない)。
    - `miri::a_blocking_call_runs_on_the_test_driver` (`crates/eml_vm/tests/miri.rs:597` の `a_c_call_passes_scalars_in_both_modes` の隣)。呼ぶのはスカラーだけの `rs_add_i32` で、`Linked` は `common::c::linked`。`Async.run` の機構を LIR のテキストで手で書けないので、このテストだけは LIR を `eml_test_support::lir(src)` で作る。Miri の下でパイプラインを流す時間を、`/usr/bin/time -p` の user で、このテストを足す前と後の `cargo miri test -p eml_vm --test integration miri::` (両方の設定) で測り、コミットのメッセージに書く。足した時間が1つの設定で 60 秒を超えるなら、止めて報告する (Q-11)。
- 実装者に任せること: `blocking_effects.em` の E2023 のケース。`Enum` のスロットの読み方 (vm.rs の既存の形に合わせる)。
- 条件: T-1 の後。実装者は Opus。段でいちばん大きいコミットである。
- 完了の基準
  - `cargo nextest run -p eml_extern -p eml_types -p eml_core_ir -p eml_lir` → 失敗なし
  - `cargo nextest run -p eml_vm --lib -E 'test(cells::)'` と `cargo nextest run -p eml_vm --test integration -E 'test(ffi::) | test(miri::)'` → 失敗なし
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::)'` → 失敗なし。`blocking_effects.em` があり `blocking.em` がない
  - `cargo nextest run --profile timing -j 1` → 通る。CLAUDE.md の feature の組の clippy が 0 で終わる
  - `grep -rln "ExternRef::C" crates/*/src` → 6 章の照合の場所の下のファイルだけ
  - `grep -rn "unsafe" crates/eml_extern/src` → 何も出さない
  - `grep -rn "Linker\|CLinker" crates/eml_vm/src` → 何も出さない
  - `grep -rn "eml_native\|cranelift" crates/eml_vm/Cargo.toml crates/eml_vm/tests` → 何も出さない
  - `grep -n "writes" crates/eml_extern/src/c.rs` → 共通の関数と3つの使い道
  - `grep -rn "transfer_back\|Done::Size" crates/eml_vm/src/ffi.rs` → 何も出さない。`git diff --stat HEAD~1 -- std/` → 何も出さない
  - Miri の `eml_vm --test integration miri::` (両方の設定) と `eml_vm --lib -- cells::a_read cells::a_write counters::` → 通る
  - `grep -n "仕事には渡さない" docs/future/async.md` → 何も出さない
  - `grep -n "^### C の関数の命令\|^### \`blocking\` の呼び出し\|^### \`--link\`" docs/spec/vm.md` → この3つの見出しがこの順に出る
  - `grep -n "blocking" docs/spec/diagnostics.md | grep -c E0004` → `0`
  - 6 章の共通の完了の基準
  - このコミットの後、CI のすべての job が緑になる
- 落とし穴
  - 型の検査が `blocking` の行の呼び手を `<Async>` の呼び出しと同じく yields にする、と見込んでいる。snapshot に `yields fn` が出ないなら、そこで止めて直す。
  - 完了と `take` の間に持ち主が取り消されても、`Woke` の `take` はハンドルを普通の `Lin` の物体で返し、次の取り消しの地点で巻き戻しが dtor ごと落とす。この窓で漏れは起きない。
  - 前の確かめで終えた記録の答えは `Err` なので、`take` はハンドルを壊さなくてよい。
  - Miri の `-Zmiri-strict-provenance` では、呼ばれる関数が slot の整数をポインタに戻せない (`with_exposed_provenance`)。そのため Miri のテストはスカラーの関数だけを呼ぶ。ポインタを持つ仕事が後の `wait` で走る形 (仮想の時計) は `ffi::` の検査モードと ASan で、別のスレッドの書き込みは T-5 の Miri と TSan で確かめる。Miri が後の呼び出しを拒んだら、D-9 を弱めず、仕事の時点でポインタを取り直すか、その形を T-5 のテストに足して止めて報告する。
  - `RuntimeEntry::ALL` に `IoWait` を足すと、VM の `async_entries` (compile/mod.rs:303-339) の「足りない entry」の報告も `io_wait` を見る。`Std.Async` を読み込めば `io_wait` もあるので、報告は変わらない。

### T-8: 本物の時計と見捨てのテスト、`execute` の終了コード、REPL の終わり

- 目的: 本物の driver の上でプールの道を 1 と 2 worker で試し、仕事が残るときの poisoned と `exit` の道を試す。`eml_vm::execute` が終了コードを保つようにし、REPL が poisoned にした入力の直後に終わるようにする (D-6、D-8、D-9、D-11、D-20、D-24、D-29、D-30、D-31)。REPL のコード、テスト、文を一緒に入れるのは、文書が今の状態を書き、REPL のテストが T-7 から書ける本物の時計の `blocking` の呼び出しを要るためである。
- 対象
  - `crates/eml_vm/src/lib.rs` (`execute_here` :280-292)、`crates/eml_vm/tests/common/c.rs`、`crates/eml_vm/tests/ffi.rs`、新しい `crates/eml_vm/tests/ffi_abandon.rs`、`crates/eml_vm/tests/main.rs`
  - `crates/eml_cli/src/repl.rs` (`Repl`)、`crates/eml_cli/src/terminal.rs` (`Flow` :254-260、主のループ :86-111、起動の `load_text` :74-78、`Terminal::eval` と `Terminal::load_text` の `show` の呼び手)、`crates/eml_cli/tests/repl.rs`
  - `.github/workflows/ci.yml` (`asan` :211-221 と `tsan` :240-258 の job)
  - 文書: `docs/spec/vm.md` の「追記」、`docs/spec/modules.md` の「REPL のセッション」、`docs/spec/repl.md` の「終わり方」、`docs/future/multicore.md` (REPL の箇条 :480)、`docs/future/async.md` の「ブロックする呼び出しのプール」の表の REPL の行、`docs/implementation/architecture.md` の「CLI と lib API」(`Repl::is_poisoned`)、`docs/implementation/testing.md` の「AddressSanitizer」(:222-229)「ThreadSanitizer」(:231-)、`docs/spec/declarations.md` の「`extern "C"`」(予算の箇条 :235)、`docs/spec/vm.md` の「時計と driver」(:544) と「`blocking` の呼び出し」(:1055)、`docs/spec/stdlib.md` の「取り消し」
- 作業
  1. `execute_here` を D-24 のとおりにする。コメント (why): poisoned の `Machine` はヒープを漏らし、`close` は終了コードの代わりに poisoned の誤りを返す。`execute_here` は受け取った `&Linked` を `machine.load(program, linked)` に渡したままにする。
  2. REPL を D-29 のとおりにする。`terminal.rs` のコメント (why): poisoned の `Machine` は後の入力をすべて断るので、その入力でセッションを終える (docs/spec/vm.md の「追記」を引く)。`eml_cli` は予算を出さない (`main.rs` にオプションを足さず、REPL は `RunConfig` の既定を使う)。
  3. `crates/eml_vm/tests/common/c.rs` に D-30 の `rs_gate_open`、`rs_gate_wait`、`rs_handle_wait`、`GATES` (`Mutex<HashSet<i64>>` と `Condvar`、`wait_timeout_while` で 10 秒の見張り)、橋渡し `i64_to_void` を足し、`calls()` に並べる。`grep -n "sleep" crates/eml_vm/tests/common/c.rs` が何も出さないこと。
  4. 文書
     - vm.md の「追記」: poisoned の `Machine` (見捨てた仕事か、残った操作による) は、その入力の直後に REPL のセッションを終える。端末は poisoned の誤りを1回書いて 1 で終わる。
     - modules.md の「REPL のセッション」: `Machine` を poisoned にした入力の後にセッションが終わることを1文で。
     - repl.md の「終わり方」: 書く文字と終了コード 1 の箇条。
     - multicore.md :480 の REPL の箇条 (「実行中のプールの仕事と操作を待ちきってから回復する」) と async.md の表の REPL の行: 予算の後もまだ走る仕事は見捨て、セッションはその入力の後に終わる。
     - architecture.md の「CLI と lib API」: `Repl::is_poisoned`。
     - testing.md の「AddressSanitizer」: `ffi_abandon::` を `detect_leaks=0` の別の step で流す理由 (poisoned の `Machine` を意図して漏らす) と、ほかの step の leak の検出は既定のままであること。「ThreadSanitizer」: 一覧に `ffi_abandon::`。
     - declarations.md の「`extern "C"`」の予算の箇条 (:235): 予算が遅らせるのは run を止める終わり (`exit`、Ctrl-C、致命的な誤り、デッドロック、`Async.run` の外へ出た実行時エラー) だけで、ほかのタスクの失敗で `run` や `scope` が子を取り消すだけのときは、`run` は C の関数が戻るまで戻らず、待ちきれないときは Ctrl-C で止める (D-4、D-8)。
     - vm.md の「時計と driver」(:544): 予算は静止の長さであって取り消しの長さではない。スコープの失敗で取り消された `blocking` の呼び出しやファイルの仕事は完了を待つので、その `run` は仕事が終わるまで戻らず、静止には入らない。
     - vm.md の「`blocking` の呼び出し」(:1055): スコープの失敗による取り消しも同じで、失敗を起こし直す `run` と `scope` はその仕事が終わるまで戻らない。run の仕事の予算は静止にだけ効く。
     - stdlib.md の「取り消し」の `Std.Net`/`Std.Stream`/`Std.Exec` の箇条: `blocking` の C の関数の呼び出しも同じであること。
  5. CI: ffi-c が両方の sanitizer に入れた `ffi::` に加え、ASan に `ASAN_OPTIONS=detect_leaks=0 … -p eml_vm --test integration -- ffi_abandon::` の step を足し、TSan の eml_vm の step に `ffi_abandon::` を足す。
  6. テスト (D-31 の決まりで書く。番号は 100020 から 100099 の、テストごとに違うもの)
     - `ffi.rs` (`common::c::linked`)
       - `ffi::a_blocking_call_writes_an_array_on_the_pool` (本物の時計、1 worker、検査モード。`rs_fill` を `blocking` で宣言し、呼んだ後に読む配列が C の書いた値を持つ)
       - `ffi::another_task_runs_during_a_blocking_call` (A が `rs_gate_wait` で鍵を待つ間に B が動いて `rs_gate_open` で鍵を開ける)
       - `ffi::a_blocking_call_runs_on_the_driver_of_worker_one` (2 worker。最初に `start` したタスクは worker 1 に置かれる。結果と `RunStats.completions`)
       - `ffi::a_cancelled_blocking_call_resumes_after_its_job` (`rs_handle_wait` で鍵を待つ A を B が取り消してから鍵を開ける。A は `Cancelled` を受け、`rs_frees` が 1)
       - `ffi::a_failing_scope_waits_for_its_cancelled_blocking_call` (本物の時計、1 worker。A はハンドルを作って合図の鍵を開け、`rs_handle_wait` で鍵を待つ。B は合図を待ってから `panic "boom"` を呼ぶ。`main` は `Async.run body` で、`try_*` を使わない。テストのスレッドは合図を待ってから鍵を開ける。`run` は `boom` の `Panic` の誤りを返し (`interrupted()` でない)、`is_poisoned()` は偽、`rs_frees` が 1、`close()` は `Ok`。鍵を外から開けなければ run は終わらないことを doc に書き、docs/spec/declarations.md の「`extern "C"`」を引く。取り消しと完了のどちらが先に当たっても結果は同じ)
     - `ffi_abandon.rs` (`common::c::linked`。`Machine::load(&program, &linked)`)
       - `ffi_abandon::an_exit_during_a_blocking_call_keeps_its_code_and_poisons` (1 worker) と `ffi_abandon::an_exit_during_a_blocking_call_keeps_its_code_and_poisons_on_worker_one` (2 worker): 開かない鍵を待つタスクと、`exit 3` を呼ぶタスク。`run` は `Ok(f)` で `f.exit == Some(3)`、`is_poisoned()`、`close()` の誤りに `the driver kept pending ops after quiesce` が入る。
       - `ffi_abandon::two_hung_workers_share_one_budget` (2 worker、worker ごとに戻らない呼び出しが1つ): 同じ3つの結果だけを確かめ、時間は何も確かめない。
       - `ffi_abandon::an_interrupt_during_a_blocking_call_poisons` と `ffi_abandon::an_interrupt_during_a_blocking_call_poisons_on_worker_one` (助けの関数は `an_interrupt_poisons`): テストのスレッドが合図の鍵を待ってから `Control::interrupt` で run を止める。`run` は run 自身の誤り (`RuntimeError::Unrecovered` でない、`interrupted()`) を返し、`is_poisoned()`。スコープの中の `Panic` では run は仕事を待って止まらないので (D-4)、割り込みを使うことを doc に書き、docs/spec/vm.md の「`blocking` の呼び出し」を引く。T-8 の最初の版の `an_error_during_a_blocking_call_poisons*` と `an_error_poisons` からの改名である (種類3。コミットの前の名前なので、一覧の差には出ない)
       - `ffi_abandon::a_machine_that_abandoned_a_job_refuses_recovery`: 戻らない呼び出しのある、割り込みで止めた run は自分の誤りを返す (doc に割り込みを使う理由を同じく書く) (poisoned の `Machine` では `run_recovering` が `recover` を飛ばすので、回復は試みられない)。それから `is_poisoned()`、2回目の `load(&program2, &Linked::default())` が `CompileError::Poisoned`、前の entry の `run` が poisoned の誤り。`recover` 自身の断り (lib.rs:525-527) には、run が `Machine` を poisoned にした後では届かない。
       - `ffi_abandon::execute_keeps_the_exit_code_of_a_run_that_abandoned_a_job`: `eml_vm::execute(&program, &linked, &RunConfig::default().with_clock(Clock::Real).with_job_budget(Duration::from_millis(20)), host)` が `Ok(f)` で `f.exit == Some(3)`。
       - 各テストは終わりに `rs_gate_open` で鍵を開け、切り離したスレッドを終わらせる。
     - `crates/eml_cli/tests/repl.rs`: `repl::the_machine_is_poisoned_after_an_input_that_abandoned_a_job`
       - 設定: `RunConfig::default().with_clock(Clock::Real).with_job_budget(Duration::from_millis(20))`。`Repl::new(&config, Box::new(FsProvider::new(root)), host).with_linker(linker())`。`linker()` はこのファイルの :19-22 のもので、`with_symbols` の `unsafe` と SAFETY のコメントはそこにある。
       - 入力: `extern "C" blocking symbol "eml_fixture_gate_wait" gate_wait : Int -> <IO, Async> Int` を宣言する入力 (`import Async (Async)` と `import Async` を含む)。次に、`Async.run` で、`gate_wait` で鍵 100100 を待つタスクと `exit 3` を呼ぶタスクを走らせる式の入力。
       - 確かめること: その入力の結果は `Outcome::Exited(3)`。`repl.is_poisoned()`。次の `eval("1")` は `Outcome::Broken(e)` で、`e.to_string()` に `the driver kept pending ops after quiesce` が入る。`repl.close().leaks` は同じ文言の `Some(e)`。最後に `eml_test_support::eml_fixture_gate_open(100100)` で鍵を開ける。
     - 端末の `Flow::Poisoned` の道 (Ctrl-C の `Flow::Interrupted` の場合を含む) には、自動のテストを置かない。`terminal.rs` に単体テストはなく、`crates/eml_cli/tests/terminal.rs` は fixture の記号を持たない `eml` のバイナリを動かすだけだからである。9 章の手での確かめで見る。
     - 期限の共有は T-4 の `workers::tests::job_wait_shares_one_deadline` と `machine::every_quiesce_gets_the_job_budget` が確かめる。時間を測る結合テストは置かない。
- 実装者に任せること: プログラムの文。`is_poisoned` の確かめを `show` の呼び手に置くか、助けの関数にまとめるか (どちらでも、すべての評価の後に flow によらず確かめること)。
- 条件: T-4、T-6、T-7 の後。実装者は Opus。
- 完了の基準
  - `cargo nextest run -p eml_vm --test integration -E 'test(ffi::) | test(ffi_abandon::)'` → 失敗なし
  - `cargo nextest run -p eml_cli --test integration -E 'test(repl::)'` → 失敗なし。`the_machine_is_poisoned_after_an_input_that_abandoned_a_job` が通る
  - `cargo nextest run --profile timing -j 1` → 通る
  - `grep -rn "Instant::now\|elapsed" crates/eml_vm/tests/ffi_abandon.rs` → 何も出さない
  - `grep -n "sleep" crates/eml_vm/tests/common/c.rs` → 何も出さない
  - `grep -rn "job_budget" crates/eml_cli/src` → 何も出さない
  - `grep -n "Flow::Poisoned" crates/eml_cli/src/terminal.rs` → 種類、その2つの作り手、主のループの枝、起動の `load_text` の枝
  - `nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- .github/workflows/ci.yml` → 0 で終わる
  - CI だけで: TSan の `-p eml_vm --test integration -- … ffi:: ffi_abandon::` が通る。2つの ASan の step が x86_64 と aarch64 の Linux と macOS で通る
  - 6 章の共通の完了の基準
- 落とし穴
  - `show` が `Continue` を返したときだけ確かめると、Ctrl-C で poisoned になった入力の後もセッションが続く。flow によらずに確かめる。
  - TSan が切り離したスレッドを thread leak と報告したら、テストは確かめる前に鍵を通して仕事のスレッドを終わらせる (Q-5)。
  - eml_cli の結合テストは1つのバイナリで、UI テストと鍵の表 (`eml_test_support` の `GATES`) を共有する。REPL のテストの鍵は 100000 以上にし、UI のファイルの範囲と重ねない。

### T-9: `blocking` の UI テストと番号の範囲の検査

- 目的: 両方の時計で、利用者から見た動きを確かめる。ffi-c が規則だけを書いた fixture の番号の範囲を、テストで確かめるようにする (D-5、D-10、D-11、D-30)。
- 対象: 新しい `tests/ui/run/async/blocking_fixture_{add,fill,gate,cancelled,many}.em`、`tests/ui/run-fail/async/blocking_fixture_null_handle.em`、`tests/ui/run-fail/async/blocking_fixture_range_panic.em` (期待する出力とともに)。`crates/eml_cli/tests/ui.rs` (範囲の検査)。`docs/implementation/testing.md` の「UI テスト」(:390 の `-- clock: real` の段落、:427 の fixture の段落)
- 作業
  1. ファイルはすべて `async/` に置く。worker と seed の run は `par/` と `async/` だけを覆うからである。UI の runner は、どのプログラムも `eml_test_support::link` でつなぐ (ui.rs:210-215)。
  2. 行の順: `leading_marks` は印でない最初の行で読むのをやめるので、ファイルは印 (`gate`、`cancelled`、`many` では `-- clock: real (waits for another task inside a C call)`) で始め、次に `-- fixtures: only runnable through the test harness`、次に `-- fixture ids: A..B` を置く。testing.md:427 の「最初の行の印の後に `-- fixtures:`」の規則のとおりで、ffi-c のファイルは印を持たないので1行目が `-- fixtures:` である。
  3. 範囲 (ハンドルの tag と鍵の番号の両方): `add` 1000..1001、`fill` 1010..1011、`gate` 1020..1022、`cancelled` 1030..1032、`many` 1100..1141、`null_handle` 1200..1201、`range_panic` 1210..1211。ffi-c のファイル (3 章の範囲) は変えず、範囲の検査の対象になるだけである。
  4. 中身
     - `add`: 2つ以上のタスクが `add` (`eml_fixture_add_i32` を `blocking` で宣言) を呼ぶ (`run/async/` の worker の run は `placed_tasks > 0` を求める)。部分適用を1つ使う。仮想の時計、seed、2/8 worker で、worker `k` ≥ 1 の host でない持ち主の道を通る。
     - `fill`: `MutUnboxedArray Float` にプールで書き、組で返り、読んだ値を書く (`writes` の道)。
     - `gate` (`-- clock: real`): A が `a: calling` を書いて鍵 1020 を待ち、`a: done` を書く。B が `b: opening` を書いて鍵 1020 を開ける。どの順でも `a: done` は `b: opening` の後に出る。
     - `cancelled` (`-- clock: real`): A がハンドル (tag 1030) を引数に `eml_fixture_handle_wait` で鍵 1031 を待つ。B が A を取り消してから鍵を開ける。A は `Cancelled` を受け取り、`frees 1030` が `1` を書く。
     - `many` (`-- clock: real`): 40 のタスクが 1100 から 1139 の鍵を待ち、41 番目のタスクが 40 の鍵を順に開け、数 `40` を書く。40 の同時の呼び出し (`JOB_LIMIT` を 16 超える) がどの順でも終わることの煙のテストで、列の順の証明ではない (それは T-2 の pool の単体テスト)。鍵を計算で作るので、範囲の検査は literal の番号だけを見る。開ける側の `eml_fixture_gate_open` は `blocking` でない同期の行で宣言する (その場で動き、止まらない)。
     - `null_handle`、`range_panic`: run-fail の文言。
  5. `ui.rs` に `ui::fixture_ids_are_disjoint_across_files` を足す。`tests/ui` の下の `.em` で `-- fixtures:` の行を持つものは、どれも `-- fixture ids: A..B` に合う行をちょうど1つ持つ (`strip_prefix` と `split_once("..")` で読む)。範囲は半開きで空でなく、ファイルどうしで重ならない。ファイルが literal で書くハンドルの tag と鍵の番号 (`handle_new`、`frees`、`gate_open`、`gate_wait` の最初の引数と、`handle_wait` の2番目の引数) は、その範囲の中にある。範囲は 3 を含まない (トランスクリプト `fixtures.repl` の tag 3。testing.md:427)。
  6. testing.md
     - :390: 「子のプロセスを起こすテストのための印で、テストの driver は子を扱わない (…)。」の1文だけを「子のプロセスを起こすテストと、C の関数の中でほかのタスクを待つ `blocking` の呼び出しのテストのための印である。テストの driver は子を扱わず、ほかのタスクを待つ `blocking` の呼び出しでは worker 0 を止めるためである (…)。」に替える。ほかの文 (理由を空にしない、流す run、`signal:` と一緒に書かない、子は3回までで `eml` 自身、時間に頼る語を使わない、どの子が先に終わるかに預けない、panic する場合、`git grep` で挙げる、`a_real_clock_file_has_no_seed_or_workers_runs`) はそのまま残す。
     - :427: 範囲の規則を `ui::fixture_ids_are_disjoint_across_files` が確かめること。
- 実装者に任せること: プログラムの文。`fixture_ids_are_disjoint_across_files` が literal の番号を見つける方法 (基準: ffi-c と ffi-blocking のすべての fixture のファイルで、literal の tag と鍵の番号を漏らさず見つけ、ハンドルの値のような別の数を番号と取り違えないこと)。
- 条件: T-6 と T-7 の後。実装者は Sonnet。`many` が schedule によって結果を変えるなら Opus に替える。
- 完了の基準
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::)'` → 失敗なし。`fixture_ids_are_disjoint_across_files` が通る
  - `cargo nextest run -p eml_core_ir --test integration -E 'test(every_program_lowers_to_the_same_text)'` → 通る (新しいファイルの `extern submit` と `extern take` の表示が Core IR と LIR で同じ)
  - `cargo nextest run --profile timing -j 1` → 通る
  - コミットの後に `git grep -n '^-- clock: real (' -- tests/ui | grep blocking` → `gate`、`cancelled`、`many` を出す
  - コミットの後に `git grep -L '^-- fixture ids: ' -- $(git grep -l '^-- fixtures:' -- 'tests/ui/*.em')` → 何も出さない
  - 6 章の共通の完了の基準
- 落とし穴: `gate`、`cancelled`、`many` の3つのファイルは、本文、コメント、`-- clock: real (…)` の理由のどこにも `sleep_ms`、`timeout`、`_within`、`drain_within` の語を書かない (`a_real_clock_file_does_not_wait_on_time`、ui.rs:830-847 がファイルの文字列を調べる)。鍵は待つと消費されるので、開ける側と待つ側がどちらに先に走っても1回ずつ対になる。同じ番号を2つのファイルで使うと、並べて流したときに相手の鍵を消費する。`git grep` は追跡したファイルだけを見るので、新しいファイルは `git add` の後に確かめる。

### T-10: `docs/future` のプールの記述の書き直し

- 目的: future の文書を、実装した形に合わせる (D-2、D-8、D-9)。
- 対象
  - `docs/future/ffi.md` の「長く走る呼び出し」(:82-97) と「VM からの呼び出し」(:98-)。:84 の E0004 の文と、「仕事の記録が渡した語を数えた参照で持つ」の箇条を直す
  - `docs/future/async.md` の「その場で動く同期の行」(:252-): 3つ目の箇条を vm.md を指す文にする
  - `docs/future/async.md` の「ブロックする呼び出しのプール」(:259-280): 箇条の 1、3 から 5 と 8 を書き直す。表では「移し戻し」を書く行を直し、「持ち主が入力をその場で書き換える」の行を「待つタスクの継続が数えた参照を持つ」にする
  - `docs/future/multicore.md`: 「移し戻し」の一覧 (:300) から「ブロックする呼び出しの取り出し」と「取り出しはプールへ移した…」の文を消し、印付けの表の行 (:316) を「印を付けない。待つタスクの継続が参照を持つ / なし」にする
- 作業: 次の点を書く。4つの文の下ろし方。継続が引数を持ち、記録は語を持たない。`Done::Job`。仕事は `CCall` と `u64` のスロットを受け取る。24/32。run ごとに1つの 1 秒の予算 (「ブロックする呼び出しのプール」の表の予算の行に、予算は静止にだけ効き、失敗したスコープの取り消しは完了を待つことを書く)。poisoned。REPL のセッションが終わる。プールは同じ橋渡しの関数を呼ぶ (「VM からの呼び出し」)。
- 実装者に任せること: 文 (natural-japanese の skill で書く)。
- 条件: T-7 の後。実装者は Sonnet。
- 完了の基準
  - `grep -n "数えた参照で持つ\|記録へ移す\|移した \`MutUnboxedArray\`" docs/future/ffi.md docs/future/async.md docs/future/multicore.md` → 何も出さない
  - `grep -n "transfer_back\|移し戻し" docs/future/ffi.md` → 何も出さない (基点でも何も出さない)
  - `grep -n "ブロックする呼び出しの取り出し" docs/future/multicore.md` → 何も出さない
  - `grep -n "E0004" docs/future/ffi.md` → 何も出さない
  - `grep -rn "docs/plans/[^ ]*ffi-blocking" docs crates | grep -v '^docs/plans/'` → 何も出さない
  - `grep -rn "CLinker\|SharedLinker\|RunConfig::with_linker\|Machine::with_linker" docs | grep -v '^docs/plans/'` → 何も出さない
  - 6 章の共通の完了の基準
- 落とし穴: `grep -rn "docs/plans" docs crates` は、ディレクトリの一般の言及 (docs/README.md、testing.md、roadmap.md、citations.rs など) にも当たるので、完了の基準に使わない。

### T-11: 段を閉じる (`Close ffi-blocking`)

- 目的: roadmap の「進め方」の閉じる作業を、1つのコミット `Close ffi-blocking` にまとめる (D-7、D-8、D-13)。
- 作業 (roadmap の「進め方」の順)
  1. `spec/` を仕上げる。`grep -rn "ffi-blocking" docs/spec docs/implementation` が何も出さず、declarations.md と diagnostics.md に `blocking` の E0004 の文が残っていない。
  2. 計測する (`eml_vm`、`eml_rt`、`eml_core_ir`、`eml_lir` が変わった)。基点の表は benchmarks.md の「今の数値」の3つ (`コミット: 9949fa6`) である。まず `bench/scripts.sh` を流し、範囲を外れた行があれば別のコミット `Resize bench rows` で大きさを変える。次に `bench/run.sh`、`bench/rt.sh`、`bench/scripts.sh` を、T-10 の後のきれいな作業ツリー (コードの最後のコミット) で流し直し、3つの表を入れ替える。表の `コミット:` はそのコミットにする。macOS がなければ表の入れ替えだけを直後の `Measure ffi-blocking` にする。
  3. status.md: `blocking` が使えること。:156 の「`Op::Job` を使うのは Rust のテストだけ」と :188 の E0004 の箇条を消す。制限 (24/8、run ごとに 1 秒、仮想の時計では戻らない呼び出しで run が終わらない、仕事を見捨てた入力の後に REPL のセッションが終わる、失敗したスコープの中の `blocking` の呼び出しは C の関数が戻るまで待ち、戻らない呼び出しを抱えて失敗した `run` は Ctrl-C でしか終わらない (そこからは予算が効く)) を書く。
  4. architecture.md: eml_vm の `ffi.rs` (`FfiSubmit`/`FfiTake`)、cells の `OpKind::Ffi`、eml_io のプールの列 (:376 の「`Op::Job` を使うのは Rust のテストだけ」を消す)。testing.md:284 の同じ旨の文も直す。
  5. roadmap: `ffi-blocking` の行 (:31) を消す。`tls-platform-verifier` (:32) の「前提: `ffi-blocking`」を、「進め方」の 5 のとおり項目の名前に直すか消し、「仕事は `Done::Job` で `Conn` を値で返す」を足す。
  6. overview.md の 13 行と、docs/README.md の「文書の地図」は変わったときだけ直す。
  7. CLAUDE.md
     - sanitizer のコマンドのコメント: ASan の `ffi_abandon::` の step (`ASAN_OPTIONS=detect_leaks=0`) と、TSan の一覧の `ffi_abandon::`。
     - eml_io の行: 「(at most 32 threads, 24 of them for `Op::Job`, then queued)」。
     - Topics の表に「`blocking` C calls」の行を足し、declarations.md の「`extern "C"`」と vm.md の「`blocking` の呼び出し」を指す。
  8. この文書を消す。消す前に、5 章の決めたことがすべて `docs/` かコードにあることを確かめる。
  9. コミットのメッセージ: 文書で何をどこへ移したか、テストの変更の種類と理由、計測の差、後に回した項目 (24/8 と 1 秒) を決めた日 (2026-11-05)、D-20 の公開の API を設計の裁定で受け入れたこと (REPL の変更と `Repl::is_poisoned` を含む)。
- 実装者に任せること: benchmarks.md の「所見」のうち、この段が変えた規則を根拠にするものの測り直しの範囲。
- 条件: すべてのタスクの後。実装者は Opus。
- 完了の基準
  - 9 章の全体の検証 → すべて通る
  - `ls docs/plans | grep ffi-blocking` → 何も出さない
  - `grep -n "ffi-blocking" docs/future/roadmap.md` → 何も出さない
  - `grep -n "コミット: 9949fa6" docs/implementation/benchmarks.md` → 何も出さない (表を測り直した)
  - `nix develop -c nix/third-party-licenses.sh --check` → 0 で終わる
  - 6 章の共通の完了の基準

## 8. 順番

```
T-1 → T-2 → T-3 → T-4 ──────┐
T-1 ───────────────→ T-7 ───┼→ T-8 → T-9 → T-10 → T-11
T-5 (A 列)                  │
T-6 (A 列) ─────────────────┘
```

- T-7 は T-1 (`Done::Job`) を要る。T-8 は T-4 (予算と `is_poisoned`)、T-6 (`eml_test_support` の鍵)、T-7 を要る。T-9 は T-6 と T-7 を要る。
- T-5 と T-6 は T-1 から T-4 と入れ替えてよい。
- 既定の順は T-1、T-2、T-3、T-4、T-5、T-6、T-7、T-8、T-9、T-10、T-11 である。
- CI を T-3、T-7、T-8、T-11 の後に緑で確かめる。

## 9. 検証

- ベースライン (最初の変更の前、基点 8d6a813a のきれいな木で)
  - `cargo nextest run` → すべて通る。通った件数と飛ばした件数を記録する
  - `cargo clippy --all-targets -- -D warnings`、`cargo fmt --check` → 通る
  - `cargo verus verify -p eml_rt` → `0 errors`。verusfmt の確かめ → 0 で終わる
  - CLAUDE.md の Miri のコマンド (`eml_rt trusted::`、`eml_vm --test integration miri::`、`eml_vm --lib -- cells::a_read cells::a_write counters::`、`eml_io --lib -- driver::tests::`) → 両方の設定で通る
  - main の CI が4つの OS で緑である
  - 4 章の ffi-c の形の確かめ → すべて合う
- 全体の検証 (T-10 の後のきれいな木で)
  - `cargo nextest run` → 失敗なし。`cargo nextest run --profile timing -j 1` → 通る
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::) | test(repl::)'`、`cargo nextest run -p eml_vm --test integration -E 'test(ffi::) | test(ffi_abandon::) | test(machine::)'`、`cargo nextest run -p eml_io` → 失敗なし
  - Miri: `eml_vm --test integration miri::`、`eml_rt trusted::`、`eml_io --lib -- driver::tests::` を両方の設定で → 通り、時間の予算に収まる
  - CI の sanitizer: TSan の `ffi:: ffi_abandon::` と、2つの ASan の step → 通る
  - `cargo verus verify -p eml_rt` と verusfmt → 通る
  - `cargo clippy --all-targets -- -D warnings`、CLAUDE.md の feature の組の clippy、`.#cross` の clippy (Windows と Linux の2つ) → 0 で終わる。`cargo fmt --check` → 通る
  - grep
    - `grep -rln "ExternRef::C" crates/*/src` → 6 章の照合の場所の下だけ
    - `grep -rn "unsafe" crates/eml_extern/src` → 何も出さない
    - `grep -rn "Linker\|CLinker" crates/eml_vm/src` → 何も出さない
    - `grep -rn "CCall\|CFunction\|CFnId" crates/eml_rt/src crates/eml_io/src` → 何も出さない
    - `grep -n -B3 "^pub enum Done" crates/eml_rt/src/driver.rs | grep -c "PartialEq"` → `0`
    - `grep -rn "Instant::now\|elapsed" crates/eml_vm/tests/ffi_abandon.rs` → 何も出さない
    - `grep -rn "job_budget" crates/eml_cli/src` → 何も出さない
    - `grep -n "プールの仕事の予算と、放さずに終わる方針" docs/future/roadmap.md` と `grep -n "仕事には渡さない" docs/future/async.md` → 何も出さない
  - `nix build` → 通る
- 利用者が手で確かめること (macOS)
  - `run`: 作業用のプログラムで `extern "C" blocking symbol "sleep" sleep_s : Int as int -> <IO, Panic, Async> Int as int` (プロセスの libc。`Int as int` は狭めるので `Panic` が要る) を宣言する。1つのタスクが `sleep_s 30` を、もう1つが `exit 3` を呼ぶ。`timeout 20 cargo run -p eml_cli -- run <file>` が約 1 秒後に 3 で終わる (D-8)。
  - REPL と `exit`: 同じ宣言で `timeout 20 cargo run -p eml_cli -- repl <file>` を起こし、2つのタスクを走らせる入力が `exited with code 3` を書き、続けて `runtime error: …the driver kept pending ops after quiesce…` を書いて、次の行を読まずに 1 で終わる (D-8、D-29)。
  - REPL と Ctrl-C: `sleep_s 30` だけを呼ぶ入力の間に Ctrl-C を押す。`interrupted` の後に poisoned の文言を書き、セッションは次の行を読まずに 1 で終わる (D-29)。Ctrl-C の後の `Flow::Interrupted` の道で、自動のテストがない所である。
  - BLAS: ffi-c の `cli::blas_*` の `#[ignore]` のテストがまだ通る。
  - CI が ubuntu-24.04、ubuntu-24.04-arm、macos-15、windows-2025 と Miri、ASan、TSan の job で、やり直しなしに緑になる (T-3、T-7、T-8、T-11 の後)。

## 10. 危険と未決の項目

| ID | 項目 | 影響 | 決め方と時期 | 決まらないときの既定 |
|---|---|---|---|---|
| Q-1 | LeakSanitizer が poisoned の `Machine` のヒープを Linux で報告するか (この機械では試せない)。asan の job は leak の検出を既定のまま動かし、ffi-c は JIT の module を `RETIRED` に生かして報告を止めている | T-8 | T-8 の CI | `ffi_abandon::` を `detect_leaks=0` の別の step で流す。その step を変数なしで1回流して leak の報告が出なければ、変数を外して `ffi::` の step にまとめる |
| Q-2 | テキストの語 `extern submit` と `extern take` | T-7 | 実装者 | D-27 の形。往復できること |
| Q-3 | `Flow::Poisoned` の名前と、確かめを置く場所 | T-8 | 実装者 | `Flow::Poisoned`。すべての評価の後に flow によらず確かめる |
| Q-4 | プールの仕事を数える `RunStats` の項目 | なし | 段の外 | 足さない (`completions` が数える) |
| Q-5 | TSan が `ffi_abandon::` の切り離したスレッドを報告するか | T-8 | T-8 の CI | テストは戻る前に鍵をすべて開ける。報告が出たら、確かめる前に鍵を通して仕事のスレッドを終わらせる |
| Q-6 | 24/8 と 1 秒の値 | D-7、D-8 | 計測か報告が出たとき | 今の既定。roadmap の「その後の項目」で見直す |
| Q-7 | 番号の範囲の検査が `tests/ui` の `.em` だけを見ること | T-9 | 決めてある | トランスクリプト (`fixtures.repl` の tag 3) と Rust のテストは testing.md の取り決めだけで守る |
| Q-8 | 端末の `Flow::Poisoned` の道の自動のテスト | T-8 | 決めてある | 置かない。9 章の手での確かめで見る |
| Q-9 | 3 章と 4 章の名前や行番号が、実装の途中の木と違う | すべて | 実装者 | 行番号だけの違いは読み替え、コミットのメッセージに書く。形そのもの (関数の有無、引数、`unsafe`) が違えば止めて報告する |
| Q-10 | `io_wait` を根にしたことで変わる stage-dump の snapshot | T-7 | T-7 | 8d6a813a の scratch の複製では `eml_core_ir` の snapshot は変わらなかった。6 章の種類2の範囲の中なら書き換えてコミットのメッセージに挙げる。範囲の外が変われば止めて報告する |
| Q-11 | `miri::a_blocking_call_runs_on_the_test_driver` が Miri の下でパイプラインを流す時間 | T-7 | 決めた (利用者、T-7 の途中) | Miri の下のパイプラインは Prelude の構文解析で rowan の中の UB に当たる (両方の設定)。そのためテストは LIR を `crates/eml_vm/tests/miri/blocking.lir` (`include_str!`) から `eml_lir::parse` で読む。Miri でないときは同じテストが `eml_test_support::lir(src)` の出力を `eml_lir::pretty` で書き、fixture と一致することを確かめる。std/Async か translate の出力が変われば fixture を書き換える (種類2)。fixture 版でも足した user 時間は +65 秒 (Stacked) と +75 秒 (Tree) で 60 秒と `miri.rs` の1つあたり 10 秒を超えたので、利用者の判断で、テストを検査モードだけにし Miri の側の verify を外して測り直し、`miri.rs` の上限を超えるテストとして testing.md の「Miri」と roadmap の「`miri.rs` のテストの1つあたりの Miri の費用」に足す。`miri::` 全体の予算 (300 秒、400 秒) は守る |

## 付録 A: レビューで変わったこと (蒸し返さないために)

- 記録が語を持つ形 (`held` の列、`transfer_back`) は、ffi-c の「借りて組で返す」規則とぶつかるので、継続が引数を持つ形にした (D-2)。
- `blocking_wait` と汎用の `discard_op` は、取り消しで返ったハンドルの dtor を失うので、`take` が `Wake` を受け取る形にした (D-1、D-4)。
- 予算をファイルの仕事にもかける案は、遅いディスクの `Std.Stream` を後退させるので、`Op::Job` だけにした (D-8)。
- 静止ごとに 1 秒の案は、N worker で N 秒になるので、run ごとに1つの期限にした (D-8、D-24)。
- `exit` の run を `Ran::Abandoned` で返す案は、`run_recovering` が `recover` に通して終了コードを失うので、`exit` の `Finished` を返す枝にした。規則は非公開の `Ran` を名指さず、見える結果で書く (D-8、D-24)。
- REPL を変えない案は退け、poisoned にした入力の直後に終わる形にした。確かめは、Ctrl-C の後の `Flow::Interrupted` を含めて、すべての評価の後に flow によらず行う (D-29)。
- `pure` の C の行が `MutUnboxedArray` に書くとき、使われない呼び出しを縮約が消して書き込みを失う。ffi-c で `FunctionRow::writes` を足し、`removable()` に `!writes` を入れた。`CSubmit` と `CTake` も同じ関数から `writes` を取る。書く行を `Effectful` にする案は、「IO を起こしうる」と混ざるので退けた (D-26)。
- linker を trait (`CLinker`、`RunConfig::with_linker`) で VM に渡す案は、VM が作り手を持たないよう、`eml_cabi` の `Linked` を `load` に渡す形に変わった。仕事は `CEntry` の `CCall` を写す (6 章)。
- C 由来の `ExternRef` を照合してよい所に、`CSubmit` と `CTake`、定義の crate と逆アセンブラを含めた。同期の C の行の `faults` の規則は `blocking` の2つの行に当てず、D-5 の規則にした。`CSubmit` の `with_args` は `[]` にした (6 章、D-5、D-26)。
- 2 worker の期限の共有を壁の時間で確かめるテストは、機械の速さに左右されるので、`job_wait` の単体テストと記録の driver に替えた (D-11、T-4)。
- cells の関数だけのコミットは、使い手がなく `-D warnings` で落ちるので、T-7 にまとめた。
- `call` の最後の2歩を切り出すタスクは、ffi-c が `raise` と `write_out` を定めたので落とした (D-28)。
- 番号の範囲の検査は、鍵を使うファイルだけでなく、`-- fixtures:` を持つすべてのファイルに広げ、検査の名前を `fixture_ids_are_disjoint_across_files` にした (D-30、T-9)。
- REPL のテストの名前は、確かめる中身 (`Machine` の状態) に合わせて `the_machine_is_poisoned_after_an_input_that_abandoned_a_job` にした。セッションが終わることは手で確かめる (T-8、9 章)。
- T-10 の `docs/plans` の grep は、ディレクトリの一般の言及に当たって最初から成り立たないので、この段の計画の名前に絞った。

## 付録 B: ffi-c の後の木 (8d6a813a) に合わせて変えたこと

- 3 章と 4 章: 行番号を 8d6a813a に合わせ、e97567b2 の但し書きを消した。`Completion` は derive を持たない (r0 は `Debug` と書いていた)。同じ時間の基点から driver を作るのは `RunClock::driver` の `RealDriver::new_at` (eml_vm/src/driver.rs:62) で、workers.rs:430-435 は `Run::new_driver` である。`Wake` の Repr の根拠を eml_extern の誤った行から `RuntimeEntry::ResumeTask` の引数に替えた。
- 4 章: 確かめのコマンドを、8d6a813a で流した出力どおりにした。`try_link` はない (`link` だけ)、`OwnedRow` は lib.rs にある、`execute` の `linked` は次の行にある、`cargo verus` は `--version` を受けない (`verus --version` にした)。`Linked::new`、`with_symbols` の `unsafe`、`RETIRED`、非公開の `fixtures` の確かめを足した。
- D-3 と T-1: `Done` の比べの grep (`assert_eq!` の行の数え) は複数行の `assert_eq!` を数えないので、derive の grep とコンパイラに替えた。`Completion` の落とし穴を直した。
- D-9: Miri のテストを書く文書を testing.md の「Miri」から object-model.md の T16 の Miri の欄に替えた。ffi-c の同じスレッドのテストはそこに並んでいる。`marshal` の SAFETY のコメントの窓も直す対象に入れた。
- D-26: `CTake.with_args` を「2 + `Obj` の引数の中の順位」と正確にした (`Int` の引数は `take` に渡さないので、単に 2 ずらすと違う)。`RuntimeEntry::ret` を足すとき `params` の doc の「結果はどれも `Unit`」を消すこと、`fits` の `==` が変わらないこと、`crates/eml_extern/src` に `unsafe` を書かないことを足した。
- D-27: `runtime_target` を `BodyCtx` でなく `ProgramBuilder` に置いた。`extern_call` は `ProgramBuilder` の関数で、`extern_wrapper` からも呼ばれるためである。`Instances` に `RuntimeEntry` の instance の表を足す形にした。Core IR の行番号を直した。
- D-28: `here!` が ffi.rs から見えないので、受け付けを ffi.rs の `job` (仕事を作る) と vm.rs の `ffi_submit` (登録表に出す) に分けた。`take` は `sched` の代わりに `&cells::Shared` を受ける。`write_out(task, dst, out)` は `ctx` を取らない。IO の失敗は `Fault::Raised` を `failed` に渡して起こす。取り消しで返ったハンドルは `CHandle` を作って落とし、dtor の `unsafe` を ffi-c の `Drop` に任せる。
- D-30: `eml_test_support::fixtures` は非公開なので、`eml_test_support` が鍵の関数を `pub use` で出す (T-6)。eml_vm のテストは ffi-c の形に従い、`tests/common/c.rs` の自前の `rs_gate_*`、`rs_handle_wait` を使う (T-8)。fixture のハンドル (本物のポインタ) と eml_vm のハンドル (詰めた整数) は混ぜられないためである。
- D-30 と T-9: ffi-c はすでに `-- fixture ids:` の行と testing.md:427 の規則を入れているが、確かめるコードはない。T-9 の `fixture_ids_are_disjoint_across_files` がそれを足す。「`-- fixtures:` を1行目に求める検査を緩める」種類3の項は、そうした検査がないので消した。
- Rust のテストの番号を、eml_vm が使う 100002 から 100007 を避けて具体にした (T-6 は 100200 台、T-7 は 100011 と 100012、T-8 は 100020 から 100099、REPL は 100100)。
- T-3: テストの stub の行番号を直した。ffi-c が足した eml_vm のテストの `IoDriver` はないので、その項を消した。
- T-7: 鍵の橋渡しを T-8 に移した (T-7 のテストは仮想の時計だけで、鍵を使わない)。diagnostics.md の :175 の文も直す対象に入れた。`io_wait` を根にしたことで変わる snapshot を種類2の範囲に書いた (6 章、Q-10)。
- T-8: 前のタスクに T-6 を足した (REPL のテストが `eml_test_support` の鍵を開ける)。REPL のテストは `with_symbols` を直接呼ばず、`repl.rs` の `linker()` (`unsafe` と SAFETY のコメントつき) を使う。
- T-10: `transfer_back|移し戻し` の grep は ffi.md で基点から何も出さないので、「数えた参照で持つ」などの grep を足した。ffi.md:84 の E0004 の文も対象に入れた。
- T-11: 計測の基点を benchmarks.md の `9949fa6` の表と書き、きれいな木で測り直すこと、`コミット: 9949fa6` が残らないことを完了の基準にした。status.md:188 と testing.md:284 を対象に入れた。
- 9 章: 手での確かめの `sleep` の宣言に `Panic` を足した (`Int as int` は狭めるので、ないと E2023 になる)。

## 付録 C: r1 のレビューで変えたこと

- O-1: `io_wait` の ABI と evidence を 8d6a813a の scratch の複製で確かめた (3 章、6 章)。D-1 はそのまま成り立つ。D-27 の組の変数の Repr を `Obj` から構造体 (`tuple_struct`) に直した。`RuntimeEntry::ret` は箱の形の Repr と決め、2つの `externs::` のテストの書き換えを具体にした。T-7 の最初に a0 の確かめを置いた。
- O-2: 前の木の試しを確かめたことのように書いた所 (D-24、6 章、T-4) を、「8d6a813a では試していない、完了の基準が確かめる」の形に直した。6 章の `externs::` の項は 8d6a813a で試し直した結果に替えた。
- I-1: 3 章の `assert_eq!` の数を消した (grep の書き方で 34 にも 36 にもなる)。
- I-2: T-6 の `pub use` とテストを `#[cfg(feature = "run")]` の下に置くと書いた。消費を確かめられないテストの名前を `an_open_pairs_with_one_wait` にした。
- I-3: T-9 に、時間に頼る語を書かないことと、testing.md:390 の替える1文と残す文を書いた。
- I-4: Miri のテストはスカラーの関数だけを呼ぶと決め、`eml_test_support::lir` で LIR を作ること、時間を測ること、超えたら止めること (Q-11) を書いた。
- T-8 のレビューで、失敗したスコープが取り消した `blocking` の呼び出しは予算で終わらず、C の関数が戻るまで待つことが分かった。利用者が Fable に決めさせ、その結論の (a) をとった。予算は静止にだけ効き、取り消しは完了を待つ (D-2 の 6、D-4、D-8、D-11、T-8、T-10、T-11)。レビューの A-5 (テストの名前) もこの改名で閉じる。

