# async-tasks: 1つの worker の上の `Std.Async` のタスク

- 作成: 2026-10-27
- 基準: `main` @ `3bfa56d` (「3. 背景」と、file:line の参照はこの状態を指す)
- 状態: 承認済み (2026-10-27)。D-4 と D-5 で docs/future/multicore.md の設計を変えること (worker 0 の語に書く側が直接書く、終わりの欄の「正常な終わり」と「致命的な誤り」を async-threads に回す) と、UI テストの速いモードを `normal` で流すことも承認を受けた。未決の項目は「10. 危険と未決の項目」にあり、どれも既定の扱いがある
- 付録: `docs/plans/2026-10-27-async-tasks/Async.em` (std/Async.em の下書き)、`docs/plans/2026-10-27-async-tasks/ui-tests.md` (UI テストのプログラム)

## 1. 目的と成功の条件

- 目的: roadmap の段 `async-tasks` を実装する。VM の1つの worker の上で `Std.Async` のタスクを動かし、監視のスレッドの `PREEMPT` で切り替える普通の方針を `eml run` と `eml repl` の既定にする。std に `Task`、`await`、`try_all`、`fail_io`、`Either`、`yield_now`、暗黙のスコープ、`IOErrorKind` の新しい種類を入れる。スコープは失敗を記録するだけで、兄弟のタスクは最後まで動く
- 成功の条件: 「9. 確かめ方」の全体の確認がすべて通ること。とくに `tests/ui/run/async/` と `tests/ui/run-fail/async/` の UI テストが、検査モードの逐次、速いモードの普通の方針、検査モードの seed 1 と 2026 で同じ出力を返し、seed の run ごとに `preemptions > 0` になること

## 2. 範囲

- 範囲に入るもの: 上の目的の項目。タスクごとの論理的な worker ID、`RunStats` の4つの欄 (`tasks`、`parks`、`dropped_failures`、`shared_objects`)、終わりの欄と `exit`、`Fault::Raised` と `Proc.exited` の書き直し、ネイティブ化に備えた意味の核と実行系の分け方
- 範囲の外: 取り消し (`Wake.Cancelled` を届けること、`cancel`、`shield`、`spawn_in`、`scope`、操作の予算。async-cancel)、時間、channel、I/O の driver、`Signal.wait` と `TERMINATE`、複数の worker、救済、タスクごとの上限、自己パイプ。EPIPE を `ConnectionReset` に写すことは後に回す (D-18)

## 3. 背景

- 1つの worker の `par` は `crates/eml_vm/src/sched.rs` の `Scheduler<P: Policy>` で動く。`TaskState` は `Running | Ready | Waiting(JoinId) | Unstarted(Word)` (sched.rs:131)、方針は `Sequential` と `Seeded` で、`Policy` は `tick`、`reload`、`pick`、`wake` を持つ (sched.rs:40-49)。`Scheduler` は `run_scheduled` (crates/eml_vm/src/lib.rs:403) が run ごとに作って捨てる
- 安全点は `ctx.poll(depth)?` の後で `tick() && ctx.yielding() == 0` なら `preempt` する (crates/eml_vm/src/vm.rs:269-280)。タスクを待たせる仕組みは join の待ちだけである。`step` は命令を実行する前に `task.pc = pc + 1` にする (vm.rs:335)
- `Ctx::switch_task(evv, worker, depth_offset)` (crates/eml_rt/src/heap/ctx.rs:1586) は RC に触れずに `evv` と heap の worker を替える。`Ctx::poll` (ctx.rs:3488) は止める語が 0 でなければ `stop_reason(bits)` を返す
- `Control` (crates/eml_rt/src/control.rs:37-41) は `stop: AtomicU32` (`INTERRUPT = 1`、`MEMORY = 2`)、`bytes`、`limits` だけを持ち、どれも Relaxed である。`clear()` はすべてのビットを下ろす。終わりの欄、`PREEMPT`、worker の止める語、監視のスレッドはない
- `exit` は VM の `Task.exit` に入り、`exit_abort` で `FINISH` まで巻き戻す。終了コードは割り込みが 130、ほかの実行時エラーが 1、使い方の誤りが 2、`exit n` が `n & 255` である
- Ctrl-C の handler は `crates/eml_cli/src/interrupt/` にある (unix は `SA_RESTART` なしの SIGINT、Windows は `SetConsoleCtrlHandler`)。`eml_rt` は handler を入れない
- `IO` と `Panic` は Prelude の `pub extern effect` で、操作は never である。std にはほかに `once` の操作を持つ普通のエフェクト (`Yield`、`State`) がある。`once` の継続はヒープのクロージャである (docs/spec/core-ir.md の「evidence passing」)
- ランタイムが std の eml の関数を名前で呼んだ例はない。近い例は生成した `fork$E.op` と `reperform$` (crates/eml_vm/src/compile/forks.rs) と、Prelude の lang item である。translate は入口から届く instance だけを作り (crates/eml_core_ir/src/translate/instances.rs:801)、届かない関数を落とすのは `optimize::reach::retain_reachable` だけである
- `Par.par` は `pub extern par` で、全引数の呼び出しは `Call::Par` になる。子の row `<never e>` は子の中の `Async` を E2020 で断る
- extern の表は `crates/eml_extern/src/lib.rs` にある。std の宣言との結び付けは `crates/eml_hir/tests/externs.rs` と `crates/eml_core_ir/tests/externs.rs` が確かめ、後者は `IOErrorKind` のタグの並び (externs.rs:572-596) と、Lin を持つ行と組を返す行の一覧を固定する。`IOErrorKind` は5つでタグ 0-4。`Fault::Exited` と `ProcExited` の行が `Proc.exited` を支える。`Either` はない
- extern の引数の Kind は Unr に固定される (`Array.make 1 (MutArray.empty ())` は E3001)。extern の引数の中の矢印の row を空に限る決まりは、`crates/eml_hir/tests/externs.rs:109-150` のテストにだけあり、docs/spec/declarations.md の「`extern`」には書いていない
- Prelude の `pub` でない item はほかのモジュールから見えない (crates/eml_hir/src/def_map.rs:664)
- `Session::run` はない。`--sched` は clap (crates/eml_cli/src/main.rs:27-46、既定 `sequential`) から `Sched`、`RunConfig::with_sched`、`eml_vm::execute` へ流れる
- UI テストの harness (crates/eml_cli/tests/ui.rs) は `par/` だけを seed [1, 2026] で流し (`seeded()`、ui.rs:266)、`assert_seeds_agree` は成功した run ごとに `preemptions > 0` を求める。`schedule_independent` は `peak_objects` だけを 0 にする。速いモードの run は `assert_fast_mode_agrees` (ui.rs:250-259) が `Sched::Sequential` で流す
- `RunStats` は13欄で検査モードだけで数え、`crates/eml_vm/tests/bench.rs` の48個の inline スナップショットが固定する
- `eml_vm` はすでに `src/` に 103 か所の `unsafe` を持ち、`forbid(unsafe_code)` はない。`eml_rt` は vstd だけに依存する。Cargo.lock に crossbeam、parking_lot、signal-hook はない
- 試作で確かめたこと (計画の作成のときに scratch の写しで行った)
  - `ctl` の節が `k` を extern に渡して戻り、後で `handle` の外から `k` を適用しても再開でき、続く操作でまた待てる。再開した κ の中の `try_io` と `try_panic` も IO の失敗と Panic を捕まえる
  - `start_task` と `resume_task` の LIR は `fn start_task(f.0: tobj poly) -> unit`、`fn resume_task(k.0: tobj lin, w.1: enum) -> unit` で、隠れた evidence の引数はない。節は `k` を `tobj lin` の所有した語で渡す
  - `Lin` の結果は `once Unit -> a` の thunk なら `try_io`、`try_panic` と多相の関数を通る
  - `Control` に `worker: AtomicU32`、`end: AtomicU32`、`code: AtomicI64` を `verus!` の中で足しても、`cargo verus verify -p eml_rt` は `1095 verified, 0 errors` で、verusfmt も通る

## 4. 前提

| 要るもの | 使う所 | 確かめ方 | ないとき |
|---|---|---|---|
| Nix の devShell (Verus、verusfmt、cargo-nextest、cargo-insta) | すべて | `nix develop -c sh -c 'verus --version && cargo nextest --version'` → `Version: 0.2026.10.26.d17fbd0` を含む | クラウドでは `nix/prefetch-inputs.sh` を流してからやり直す。それでも入れなければ止めて尋ねる |
| Miri の shell | T-1、T-6、T-7 | `nix develop .#miri -c cargo miri --version` → 版が出る | 止めて尋ねる |
| Windows の cross clippy の shell | T-4、T-5 | `nix develop .#cross -c rustc --print target-list \| grep x86_64-pc-windows-msvc` | 止めて尋ねる |
| unix (SIGINT を送れる) | T-5 の CLI の試験 | `uname` → `Linux` か `Darwin` | Windows では CI に任せる |

## 5. 決めたこと

### 5.1 長く効く方針 (変えるときは見直しが要る)

実装がこれと食い違ったら、止めて報告する。

#### D-1: `Async` の ABI

- 決め: `Async` は普通の eml のエフェクトで、この段の生の操作は `await_cell : Cell -> Wake` と `reschedule : Unit -> Wake` の2つ (どちらも once)。`Wake = | Woke | Cancelled` (タグ 0、1) で、`Cancelled` はこの段では届けないが ABI として先に置く。`Cell` は登録表の添字 `(generation << 32) | index` の即値 (`Unr`、`Repr::Int`)、`Task a` は `TaskHandle` を中身に持つ `Lin` の `Foreign`。ランタイムが名前で呼ぶ入口は単相の `Std.Async.start_task : (Unit -> <IO, Async> Unit) -> <IO> Unit` と `Std.Async.resume_task : (once Wake -> <IO> Unit) -> Wake -> <IO> Unit` である。Panic はタスクの底 `bottom` で捕まえるので、`k` の row は `<IO>` になる。タスクの結果は `once Unit -> a` の thunk で渡す。std の全文は付録の `Async.em` にある
- 理由: translate は入口から届く instance と型引数ごとの instance だけを作るので、単相の入口なら名前がそのまま instance の名前になり、translate の変更は根を足すことだけで済む。extern の引数は Unr に固定されるので、値で渡すと `Lin` の結果が E3001 になる
- 退けた案: 多相の `start_task@[_:poly]` を根にする (translate に Flexible のキーの根が要る)。`finish_ok : a -> …` (Lin の結果を断る)。スケジューラのループを eml で書く (安全点で横取りできない)
- 効く範囲: async-cancel、async-time、async-chan、io-driver、async-threads、jit-effects
- 見直す条件: 継続の型が違う生の操作を足すとき。そのときは継続の型ごとに単相の入口を1つ足す (io-driver の `resume_task_io : (once (Wake, OpId) -> <IO> Unit) -> Wake -> OpId -> <IO> Unit`)
- リポジトリに残す所: docs/spec/stdlib.md の「`Std.Async`」(T-9)、docs/spec/core-ir.md の「変換の規則」と「最適化」(T-10)。docs/future/multicore.md:106-108 と docs/future/async.md の「`Async` のエフェクト」の3つ目の項目を単相の入口と thunk に書き直す (T-9)

#### D-2: 待つタスクは継続だけになり、待ちは2種類に限る

- 決め: `Async` の操作で待つタスクは VM の `Task` を持たず、継続のクロージャだけになる。再開は、スタックの集まりから取った新しい入口の `Task` で `resume_task` に入る。待ちの登録は2種類だけである。継続の待ち (`Waiter::Cont`) は動かせて取り消せる。token の待ち (`Waiter::Token`) は `Async.run` を呼んだ VM の `Task` がスタックを持ったまま待つもので、worker を移らず、救済も盗みもされず、取り消しの地点でもない (join で待つ `par` の親と同じ扱い)
- 理由: 継続だけにすると、スタックを切り替えられないネイティブでも同じ手順になる (節が `k` を預けて戻り、`start_task` も戻る)。`main` の `Async.run` は必ずスタックを持ったまま待つので、2種類目はなくせない
- 退けた案: 待つタスクが VM の `Task` を持ち続ける (`await` のたびに extern の中で切り替わる。ネイティブで同じにできない)
- 効く範囲: async-cancel (取り消しは継続の待ちにだけ届く)、async-threads、async-rescue
- 見直す条件: 入れ子の `Async.run` を取り消しの地点にしたくなったとき
- リポジトリに残す所: docs/spec/vm.md の新しい節「`Async` のタスク」(T-11)

#### D-3: 意味の核と実行系を分ける

- 決め: タスクとスコープの意味は `crates/eml_vm/src/cells.rs` に置き、このファイルは `std` と `eml_rt` だけに依存する (`use crate::` を書かない。テストで固定する)。核の関数は VM の `Task`、`TaskId`、`Scheduler`、フレームを知らず、走らせる仕事を `Runnable` と不透明な `WaitToken` で返す。`Op::Async` は核を呼んで返り値を VM の `Task` に変えるだけにする。VM だけの部分は、`run_raw` が呼んだ側のスタックを待たせること (`WaitingCell`)、横取りした `Task` がスタックを持つこと、誤りと `exit` で `vm::run` から `Err` で出ることの3つである

```rust
pub(crate) enum Runnable { Start { cell: CellRef, closure: Word }, Resume { cell: CellRef, k: Word, wake: i64 }, Token(WaitToken) }
#[derive(Clone, Copy, PartialEq, Eq, Hash)] pub(crate) struct WaitToken(pub u64);   // 実行系が自分の単位に写す
```

| 生の操作 | 核 (`cells.rs`) | VM (`Op::Async`) | ネイティブで対応するもの |
|---|---|---|---|
| `start_raw g` | `Registry::start(ctx, scope, spawner_worker, g) -> (handle, Runnable::Start)` | ハンドルを `dst` に書き、`Task::entry(start, [g])` を `add_task` して実行待ちの後ろに積む | `extern "C"` の包み。worker のループが `eml_enter(ctx, start_task, [g])` で動かす |
| `run_raw g` | `open_root()`、`start(..)`、`wait(root, token)` | ハンドルを書き、`switch_away(Some(WaitingCell(root)))`。token は `TaskId` | token が出るまで、マシンのスタックの上で worker のループを入れ子にする |
| `park c k` | `park(c, current, k) -> Option<Runnable::Resume>` | 返った仕事を積む | 同じ核。節は入口の stub へ戻る |
| `ready k` | `ready(current, k) -> Runnable::Resume` | 積む | 同じ |
| `finish_*` | `finish(ctx, cell, outcome, out)` | 仕事を積む。`Token(t)` は `t` の `Task` を実行待ちに戻す | `Token` は待っている入れ子のループを終える |
| `task_cell t` | `cell_of(ctx, handle) -> i64` | `dst` に組を作る | 同じ |
| `take_result t`、`run_result t` | `take(ctx, handle, Take::Task \| Take::Run) -> Result<Word, Raise>` | `Ok` は `dst` へ。`Raise` は `failed(Fault::Raised{..})` か `Extern::Panic` の道 | 包みが `ctx.fault` に同じ `Fault` を書き、冷たいブロックが届ける |
| 受け箱の処理 | `drain(ctx)` | `switch_away`、`preempt` の頭と、`main` の `Finish` の片付けの前 | worker のループの1回ごと |
| セルを放す | `free(ctx, cell)`、`sweep_all(ctx)` | `main` の `Finish` | 入れ子の `Async.run` の終わりと run の終わり |

- 理由: ネイティブ化のときに `cells.rs` をそのまま `eml_sched` へ移し、`extern "C"` の入口から同じ核を呼べるようにする (利用者の求め)
- 退けた案: `Op::Async` の処理の中に意味を書く (ネイティブで書き直しになる)。今 `eml_sched` を作る (共有するものがまだない。docs/future/multicore.md の「層の構成」)
- 効く範囲: async-threads (`eml_sched` へ移す)、jit-effects
- 見直す条件: 2つ目の worker を入れるとき
- リポジトリに残す所: docs/spec/vm.md の「`Async` のタスク」(T-11)。`cells.rs` の import の制限はテストで固定する (T-11)

#### D-4: 止める語と `PREEMPT`

- 決め: worker 0 の止める語は `Control` の新しい欄 `worker: AtomicU32` にする。ビットは `INTERRUPT = 1`、`MEMORY = 2` (今と同じ) と、`worker` にだけ立つ `PREEMPT = 1 << 31` である。8 は `TERMINATE` に空けておく。書く側 (Ctrl-C の handler の `interrupt()`、確保器の `charge`) は `stop` と `worker` の両方に直接書き、監視のスレッドは写さない。`Ctx::poll` は `worker_bits()` を読み、host の読み手 (host.rs の `stop_bits()`) は今と同じく `stop` を読む。`END` のビットと、終わりの欄の「正常な終わり」と「致命的な誤り」の種類は足さない。新しいメソッドはどれも `verus!` の外で Relaxed だけを使う。信頼の一覧の行は要らない
- 理由: worker が1つなので、書く側が直接書けば止まる時点が決まり、配線も要らない。`stop` に新しいビットを足さないので、host の読み手、EINTR のやり直し、書き出し、ファイナライザ、回復の振る舞いが変わらない
- 退けた案: `PREEMPT` を `Control.stop` に置く (読み手がすべて隠す必要があり、docs/future/multicore.md の決定 #15 に反する)。`Ctx` に `Arc<StopWord>` を持たせる (N 個の worker の段で行う)。監視のスレッドがビットを写す (止まる時点が時間に依る)
- 効く範囲: async-threads (worker ごとの語を足し、監視のスレッドが写す形に広げる)、async-net (`TERMINATE`)
- 見直す条件: 2つ目の worker を入れるとき
- リポジトリに残す所: docs/spec/object-model.md の「実行の制御」と「`unsafe` の核」(T-1)。docs/future/multicore.md の「スレッドと worker」を「書く側が worker 0 の語に直接書き、ほかの worker の語に写すのは監視のスレッド」に直す (T-4)

#### D-5: `exit` の約束 (VM とネイティブで同じ)

- 決め: 終わりの欄は `exit` だけを持つ (`end: AtomicU32` は 0 なら開いていて 1 なら書いた。`code: AtomicI64` が終了コード)。手順は次の4つである
  1. 開く: host は worker を始める前に `control.begin_run()` を呼ぶ (VM では `Machine::run_entry` の最初の行)。REPL は今と同じく入力ごとに `clear()` も呼ぶ
  2. 書く: `exit n` の巻き戻しがタスクの底に着くと、底は `control.exit(n)` を呼ぶ (最初の書き手が勝つ)。`main` の底も同じである。VM では `yield_finish` と `evv` の `()` への書き換えを先に済ませる
  3. 出る: `main` の底は普通に戻る。ほかのタスクの底はほかのタスクを動かさずにすぐ worker の入口へ戻る。VM では `vm::run` が `Err(VmFault::Stopped(Stop::Ended))` を返し、ネイティブでは `eml_enter` へ longjmp する。後始末はしない
  4. 読む: worker が戻った後で、host は `control.exit_code()` を読む。これだけが `Finished.exit` の出どころである。worker が `Ended` で戻ったときは run を捨てたので、終わりの検査 (`check_finished`、`check_heap`、漏れの検査) をせず、ヒープを入れ替えて (VM は今の `recover()`) `exit n` を報告する
- 理由: 実行系の型を使わずに書けるので、ネイティブは出方 (longjmp) だけが違う。捨てた run の状態は、今 `par` の親が待つ間に実行時エラーで止めた run と同じ種類で、`recover` の前提を満たす
- 退けた案: 終了コードを VM の `Scheduler` に置く (実行系に依り、docs/future/multicore.md の決定 #17 から外れる)。今 `Normal`/`Failed` の種類と `END` のビットを足す (読み手がない)
- 効く範囲: async-threads、jit-effects
- 見直す条件: 2つ目の worker を入れるとき (ほかの worker を止める `END` のビットと種類を足す)
- リポジトリに残す所: docs/spec/object-model.md の「実行の制御」(T-1)、docs/spec/vm.md の「`exit`」と「誤り、止めること、終わり」(T-2)

#### D-6: スケジュールの方針と既定

- 決め: `Sched` は `Normal`、`Sequential`、`Seeded(u64)` の3つで、`FromStr` は `normal`、`sequential`、`seeded:<n>` を読む。`eml run` と `eml repl` の `--sched` の既定は `normal` にし、ライブラリの既定 (`Sched::default()`、`RunConfig::default()`、`Machine`、`eml_test_support`) は `Sequential` のままにする。`Normal` は監視のスレッドの `PREEMPT` だけで横取りし (`tick` は定数の偽)、FIFO である。UI テストは、検査モードの逐次がスナップショットと `RunStats` を固定し、速いモードの run を `Normal` で流し (出力だけを比べる)、`par/` と `async/` を seed [1, 2026] でも流す
- 理由: ベンチと `RunStats` のテストを時間によらないものに保ちながら、出荷する既定を UI テストの全体で流せる
- 退けた案: `Sched::default() = Normal` (スナップショットが時間に依る)。監視のスレッドをすべての方針で動かす (決まったテストにスレッドが入る)
- 効く範囲: async-threads、bench の計測
- 見直す条件: 複数の worker のテストの組を足すとき
- リポジトリに残す所: docs/spec/vm.md の「タスクとスケジュール」(T-4)、docs/spec/stdlib.md の「起動」と docs/implementation/testing.md (T-5)

#### D-7: 失敗の型と Prelude の置き場所

- 決め: `Failure = | IOFailed IOError | Panicked String` と `try_all` は `Std.Async` に置く (docs/future/async.md の決定 #4)。`fail_io : IOErrorKind -> String -> <IO> a` は Prelude の `pub extern` にする (std の別のモジュールから見えるには `pub` が要る。`Proc` は `Async` を import しない)。`IOErrorKind` は `Other` の後ろに `Cancelled`、`TimedOut`、`ConnectionReset`、`AddrInUse`、`ResourceLimit` (タグ 5-9) を足す。`Either a b = | Left a | Right b` は Prelude に置き、`Eq`、`Ord`、`Show` を導く。docs/spec/modules.md の「Prelude」の5つ目の項目を、次の文に直す
  「どのプログラムでも使う汎用の型 (`Option`、`Either`) と、標準ライブラリのいくつものモジュールがシグネチャで共有する型 (`Bytes`、`Array`、`MutArray`)、およびその instance。型を `Std.Bytes` のような自分のモジュールに置くと、その型を返すほかのモジュールの関数 (`Stdin.read_all_bytes`) を呼ぶだけのモジュールも、型の名前を書くたびにそのモジュールを import することになるためである」
- 理由: 置き場所は docs/future/async.md の決定 #4、#8 と :129 が決めている。`fail_io` は `exit` と `panic` に並ぶ IO の制御で、4つ目の項目に入る
- 退けた案: `fail_io` を `pub` にしない (std のほかのモジュールから見えない)。`fail_io` を `Std.Async` に置く (`Proc` が `Async` を import することになる)。新しい種類を `Other` の前に入れる (`Other` のタグが変わる)
- 効く範囲: async-cancel 以降の段 (`Prelude.Cancelled`、`TimedOut` などを Rust の側が作る)
- 見直す条件: Rust の側が新しい種類を作るとき (その段で `IO_*` の定数と `IoReason` を足す)
- リポジトリに残す所: docs/spec/stdlib.md、docs/spec/declarations.md の「Prelude」、docs/spec/modules.md の「Prelude」(T-7、T-8、T-9)

### 5.2 段の中の設計

理由があれば実装の中で直してよい。直したら、そのコミットのメッセージに書く。

#### D-10: 登録表は VM の実行ごとに1つ

`Registry` とその `Arc<Inbox>` は `vm::run` ごとに作る (`Scheduler` と同じ)。そのため、1つ目の `Async.run` が返したハンドルは、同じ実行の後の `Async.run` で待てる (docs/future/async.md:88)。ハンドルの `Arc<Inbox>` と登録表の `Arc<Inbox>` を `Arc::ptr_eq` で比べ、違えば `VmFault::StaleTask` (表示 `a task handle outlived its run`、壊れた状態にしない) にする。これが起きるのは REPL の入力をまたいだときだけである。古いハンドルの `Drop` は自分の死んだ受け箱に積むだけで、生きている受け箱には届かない。同じ実行の中の添字の使い回しは generation で見つける。

#### D-11: セルの形

```rust
// crates/eml_vm/src/cells.rs。import してよいのは std と eml_rt だけ
pub(crate) struct CellRef { index: u32, generation: u32 }
pub(crate) enum Waiter { Cont { task: CellRef, k: Word }, Token(WaitToken) }
pub(crate) enum Outcome { Value(Word /* thunk */), Io { kind: i64, message: String }, Panic(String) }
pub(crate) struct TaskCell {
    phase: Phase,                 // Placed | Running | Parked | Finished
    outcome: Option<Outcome>,
    scope: CellRef,
    worker: u32,                  // タスクの論理的な worker ID (検査モード)。速いモードでは 0
    spawner: u32,                 // 始めたタスクの論理的な ID。始めるときの transfer_back に使う
    waiters: Vec<Waiter>,
    handle_dropped: bool,
}
pub(crate) struct ScopeCell { parent: Option<CellRef>, live: u32, first_failure: Option<Outcome>, waiters: Vec<Waiter>, closed: bool }
pub(crate) struct Registry { slots: Vec<(u32, Option<CellKind>)>, free: Vec<u32>, pub(crate) inbox: Arc<Inbox>, pub(crate) stats: TaskStats }
pub(crate) struct Inbox { notices: Mutex<Vec<Notice>> }
pub(crate) enum Notice { HandleDropped(CellRef) }      // 取り消しの段が Cancel を、複数の worker の段が Wake と Place を足す
pub(crate) struct TaskHandle { cell: CellRef, inbox: Arc<Inbox>, taken: bool }   // `Task a` の Foreign の中身
impl Drop for TaskHandle { fn drop(&mut self) { if !self.taken { self.inbox.push(Notice::HandleDropped(self.cell)) } } }
```

- セルが持つ語は数えた参照で、VM が1回入れて1回出す。`Registry` はヒープに触れる `Drop` を持たない。誤りで終わった run と捨てた run では、`Scheduler` と一緒に触れずに捨てる
- 失敗の文言は Rust の `String` で持つ (記録するときに `copy_string` で写し、起こし直すときに起こす側の worker で新しい文字列を作る)。そのため、失敗の語は持ち主をまたがない
- 持ち主の規律: ほかのタスクの ID の結果を放すときは `with_worker(ctx, id, f)` (`let e = ctx.switch_task(Word::UNIT, id, 0); f(ctx); ctx.switch_task(e, prev_worker, prev_offset)`) だけを使う
- タスクのセルを放すとき: `outcome` を `cell.worker` の下で放し、`retire_worker(cell.worker)` を呼び (`TransferLeak` はヒープの誤りとして出る。`par` と同じ)、generation を1つ進めて添字を `free` に戻す
- 不変条件 (`cells.rs` にコメントで書き、`finish` で generation を `debug_assert!` で確かめる): タスクのセルが `scope` を読むのは `start` と `finish` だけで、`Finished` の後は読まない。スコープのセルを放すのは `live == 0` のとき (`Take::Run` は根の待ちが `closed` で起きた後、`sweep_all` は run の終わり) だけである。セルを放すのは、ハンドルを持つ側の `take`、自分の `HandleDropped` の処理、`sweep_all` の3つだけで、待つ側はハンドルを持つので、待つ相手のセルはその `take` の前に放されない
- 再開したタスクは `Waiter::Cont.task` (待った側のセル) の worker で動く。すでに終わったセルへの `park` も同じである。token の待ちは自分の VM の worker のまま戻る
- 検査モードでは、まだ待たれていないタスクの数だけ論理的な ID が増える (上限は `u32::MAX`。heap/mod.rs:287 の `too_many_workers`)。これは受け入れる

#### D-12: 核の操作の意味

- `start(ctx, scope, spawner, g)`: `stats.tasks += 1`。`worker = heap.fork_worker()`。まだ印のないセッションの根と `g` に `share(.., worker)` で印を付ける (vm.rs:916-931 の `par` の処理を、根の slice を引数にして核へ写す)。セルを `Placed` で作り、`scope.live += 1`。`alloc_foreign(TaskHandle, lin = true)`。`(handle, Runnable::Start)` を返す
- `open_root()`: `ScopeCell { parent: None, live: 0 }`
- `wait(cell, token)`: `Waiter::Token` を積む
- `park(c, me, k)`: `stats.parks += 1`。相手が `Finished` か `closed` なら `Some(Resume { cell: me, k, wake: WOKE })`。そうでなければ `Cont` を積んで自分を `Parked` にし、`None`。終わったセルへの近道は作らない (`parks` をプログラムで決まる数に保つ)
- `ready(me, k)`: `stats.parks += 1`。`Resume` を返す
- `finish(ctx, cell, outcome, out)`: `Finished` にする。失敗なら `scope.first_failure` に最初の書き手として書き、2つ目からは `stats.dropped_failures += 1`。`scope.live -= 1`。待つ側を `out` へ。`live == 0` なら `closed` にしてスコープの待つ側も `out` へ。`handle_dropped` ならすぐ放す
- `take(ctx, handle, kind)`: `Arc::ptr_eq` を確かめる。セルは `Finished` でなければならない (違えば `Internal("awaiting an unfinished task")`)。`Take::Run` はスコープの `first_failure` を本体の値より先にし、根のスコープを放す。値なら `transfer_back(thunk, from = cell.worker)`、失敗なら `Raise`。`taken = true` にしてハンドルを手放し、セルを放す
- 受け箱の処理 `drain(ctx)`: lock の下で `Vec` を取り、lock を外して処理する。`HandleDropped(c)` は、セルが `Finished` ならその場で放し、そうでなければ `handle_dropped = true` にする。`main` の `Finish` では、受け箱の処理、`sweep_all` (残ったセルをすべて放し、その間に積まれた知らせは後で受け箱ごと捨てる)、`has_children()`、`run_entry` の `check_finished`、`check_heap`、漏れの検査の順に行う
- 始めるときの `transfer_back(closure, from = spawner)` と、待つときの `transfer_back(thunk, from = cell.worker)` と、放すときの `retire_worker` は、`par` の fork と join と同じ形をタスクごとに当てたものである

#### D-13: VM の側

- `Task.cell: Option<CellRef>` (`main` と `par` の右の子は `None`)。`TaskState` に `Entry { function: u32, args: [Word; 2], argc: u8 }` と `WaitingCell(CellRef)` を足す。`Entry` を `resume()` するときは、`start` (`argc == 1`) なら先に `transfer_back(args[0], from = spawner)` を行い、引数を入口のフレームの slot 0..argc に書いて入る。`WaitingCell` を `resume()` すると `Ok(None)` で、`Op::Async` の次から続ける
- `Task::entry(code, function, args, cell, worker, max_depth)` は `Task::new` と同じ根のフレームを作り、`depth_offset = 0`、スタックは `Scheduler.stacks` (`Vec<Vec<Word>>`) から取る
- `Op::Async { dst, at }`: codegen は `Std.Async.*` の行をすべてこの命令にする (`Par` と同じ)。`run_extern` は `sched` を持たないままで、`Std.Async.*` の腕は `Internal` を返す。`Op::Async` の中に安全点を置かない
- 入口の `Task` の `Op::Finish`: `task.exit` があれば D-5 の手順。なければスタックを集まりに返して `switch_away(None)`。`finish()` の `sched.current != MAIN` の確認はこちらの腕へ移す
- 実行待ちの順: `Runnable` と起こした token はどちらの方針でも `ready.push_back` に積む (FIFO)。`Policy::wake` (逐次では先頭) は `par` の親にだけ使う。seed の方針の `pick` は今と同じく乱数で選ぶ
- `Scheduler::current` を書くのは新しい `Scheduler::set_current(id)` だけにし、これが `policy.switched()` を呼ぶ。対象は `resume`、`start_task` の新しい入口の `Task`、`resume_task` の入口の `Task`、`Async.run` で待つ `Task` へ戻るときである。同じ `Task` の中の `evv` だけの切り替え (`par` の左の子の終わり) では呼ばない
- 2つ目の仕事が走れるようになる所 (`Scheduler::add_task` と `start_raw`、`run_raw`) で `policy.task_added()` を呼ぶ
- 待つタスクが残ったまま実行待ちが空になることは、この段では起きない (ハンドルは線形で、`await` はいつも先に作ったタスクを待つ)。今の `Internal("no task is ready to run")` のままにし、待ちの輪を作れる段 (async-chan、async-time) でプログラムの誤りを足す
- `CodeBase::load` は読み込みごとに2つの入口の名前を `CodeBase.entries: Option<AsyncEntries { start: u32, resume: u32 }>` に引く。`Op::Async` を出すプログラムで名前がなければ `CompileError::MissingEntry { function }` (`MissingFork` と同じ形)。`entries` が `None` のまま `Op::Async` に来たら `Internal("the Async entries are not loaded")`。`Std.Async` は import したときだけ読むので、REPL では最初に `Async` を import した入力の `load` で引く

#### D-14: `RunStats` の4つの欄

`tasks` (`start_raw` と `run_raw` が作ったセルの数)、`parks` (`park` と `ready` がランタイムに渡した継続の数)、`dropped_failures` (スコープの最初の失敗より後の失敗の数)、`shared_objects` (`share` が rc を負にした物体の数。`crates/eml_rt/src/heap/share.rs` の負にする所で `Stats::shared` を数える) を足し、13欄を17欄にする。どれも検査モードだけで数える。`dropped_failures` は失敗の原因の数でなく、タスクの失敗のうち最初より後の数で、捕まえずに起こし直した `await` の失敗も数える (docs/spec/stdlib.md に書く)。4つともスケジュールによらないと推定しているので、seed の run で比べる (D-17)。違う欄があれば Q-3 の既定に従う。`completions`、`cancels`、`channel_ops` はそれぞれの段で足す。

#### D-15: `Proc.exited` と `fail_io` の VM の扱い

- `Proc.exited` は `std/Proc.em` の `pub` でない eml の関数にする

```haskell
-- 0 でない終了コードを、種類が `Other` の IO の失敗にする (docs/spec/stdlib.md の「`Proc`」)
exited : String -> Int -> <IO> a
exited program code = fail_io Other "command `\{program}` exited with code \{show code}"
```

- `ProcExited` の行、`Fault::Exited`、`struct Exited` とその re-export (crates/eml_rt/src/lib.rs:31)、`Ctx::proc_exited` (proc.rs:449-483)、`Extern::ProcExited` の腕、`io_error_kind`、`panics`、`Display` の `Exited` の腕と、そのテストを消す
- `fail_io` の表の行は `Exit` の直後に置き、`Exit` と同じ形の literal にする

```rust
FailIo => FunctionRow {
    name: "Prelude.fail_io",
    params: &[Repr::Enum, Repr::Obj],
    ret: OpRet::Never,
    param_kinds: &[Linearity::Unr, Linearity::Unr],
    ret_kind: Linearity::Unr,
    purity: Purity::Effectful,
    borrowed: &[false, false],
    with_arg: None,
},
```

- VM の腕は `Err(VmFault::Extern(Fault::Raised { .. }))` を返すだけにし、届けるのは今の `failed()` に任せる (`extern_call` が `Err(VmFault::Extern(_))` を `failed()` に渡す。vm.rs:1519-1524)

```rust
// いつも誤りを返すので、宛先には書かない。届け先は `failed` が決める (docs/spec/vm.md の「extern の中断」)
Extern::FailIo => {
    let message = ctx.copy_string(task.get(arg(1)), usize::MAX)?;
    return Err(VmFault::Extern(Fault::Raised { kind: task.raw(arg(0)), message: message.into_boxed_str() }));
}
```

- `Fault::Raised { kind: i64, message: Box<str> }` は `io_error_kind() == Some(kind)`、`panics() == false`、表示は `message` である。`Fault` の大きさは 24 バイトのまま (`faults_stay_small` が確かめる)
- 新しい種類のための `IO_*` の定数と `IoReason` の変更は足さない。タグは `fail_io` がそのまま渡す

#### D-16: 監視のスレッド

- 置き場所は `crates/eml_vm/src/monitor.rs` で、`Normal` の方針が持つ。run の最初の `policy.task_added()` で1回だけ作り、`Normal` の `Drop` (run の終わりに `Scheduler` を捨てるとき。成功でも誤りでも) で `watch.stop.store(true, Relaxed)`、`thread.unpark()`、`join()` の順に止める。2つ目の仕事を持たないプログラムはスレッドを作らない。REPL では入力ごとに高々1本で、入力の間には残らない

```rust
struct Watch { switches: AtomicU64, stop: AtomicBool }                 // eml_vm の中だけ
pub struct Normal { control: Arc<Control>, monitor: Option<Monitor> }
struct Monitor { watch: Arc<Watch>, thread: JoinHandle<()> }
struct Ticker { last: u64 }
impl Ticker { fn tick(&mut self, switches: u64) -> bool { let same = switches == self.last; self.last = switches; same } }
```

- ループ: `TICK = 10 ms`。`ticker.last = switches`、`next = now + TICK` から始め、`park_timeout(next.saturating_duration_since(now))`、`stop` なら抜ける、`now < next` なら続ける、`ticker.tick(switches.load(Relaxed))` が真なら `control.preempt()`、`next += TICK` を繰り返す
- スレッドが持つのは `Arc<Control>` と `Arc<Watch>` だけで、ヒープの語も `Ctx` も持たない。ビットを写さず、worker を起こさず、シグナルを届けない
- unix では VM のスレッドの上で `spawn` を `pthread_sigmask(SIG_SETMASK, &all, &old)` と `pthread_sigmask(SIG_SETMASK, &old, null)` で囲み、監視のスレッドがすべてのシグナルを塞いだまま始まるようにする。名前は `eml-monitor`、スタックは 64 KiB。`pthread_sigmask` の結果は `assert_eq!(.., 0)` で確かめる。`crates/eml_vm/Cargo.toml` に `[target.'cfg(unix)'.dependencies] libc = { workspace = true }` を足す。Windows では何もしない
- `spawn` が失敗したら、監視のスレッドなしで続ける (FIFO の逐次と同じ振る舞い)。報告はしない
- VM の `Ctx::poll` の呼び出しはすべて次の補助を通す

```rust
#[inline(always)]
fn poll<M: Mode>(ctx: &Ctx<M>, depth: usize) -> Result<bool /*切り替えたい*/, VmFault> {
    match ctx.poll(depth) { Ok(()) => Ok(false), Err(stop) => polled_stop(ctx, depth, stop) }
}
#[cold] #[inline(never)]
fn polled_stop<M: Mode>(ctx: &Ctx<M>, depth: usize, stop: Stop) -> Result<bool, VmFault> {
    if stop != Stop::Preempt { return Err(stop.into()); }
    if ctx.yielding() != 0 { return Ok(false); }   // PREEMPT は立てたまま、次の yield のない安全点で拾う
    ctx.heap().control().take_preempt();
    match ctx.poll(depth) {
        Ok(()) | Err(Stop::Preempt) => Ok(true),
        Err(stop) => Err(stop.into()),
    }
}
```

- 2回の `poll` の間には命令がないので、深さの上限の確認は正確で、間に立った止めるビットは切り替えより先に勝つ。`safepoint` は `let wants = poll(ctx, depth)?; let tick = sched.policy.tick(); if (wants || tick) && ctx.yielding() == 0 { preempt::<R>(..)? }` にする (`Seeded` は `tick` で予算を数えるので毎回呼ぶ)。`preempt` のループ (vm.rs:1190) も補助を使い、`wants || tick` でまた回る。走れるタスクがなければ切り替えずに戻る (ビットはもう下りている)。`Preempt` による切り替えは `Finished.preemptions` に数える

#### D-17: UI テストの harness

- `seeded(suffix)` を `suffix.starts_with("par/") || suffix.starts_with("async/")` にし、テスト `seeded_runs_cover_only_par` を `seeded_runs_cover_par_and_async` に改め、`seeded("par/x.em")`、`seeded("par/dir")`、`seeded("async/x.em")`、`seeded("async/dir")`、`!seeded("panic/x.em")`、`!seeded("parse/x.em")`、`!seeded("io/x.em")` を確かめる
- `assert_seeds_agree` は、`async/` の run が `exit` で終わったときだけ `RunStats` を比べない (`let compare_stats = !(suffix.starts_with("async/") && matches!(checked.result, Ok(Some(_))));`)。タスクの `exit` はほかのタスクをスケジュールで決まる位置で捨て、`Finished.stats` はその時点の数だからである。`par/` は今までどおりすべて比べる。`preemptions > 0` は `exit` の run を含むすべての `Ok` の run に求める
- 横取りがないときの文言は ``"{suffix}: never preempted under `--sched seeded:{seed}`; give a par left child, or an Async task while another task is ready, 16 or more safepoints (docs/implementation/testing.md の「seed で流す」)"`` にする
- 速いモードの run (`assert_fast_mode_agrees`) は `Sched::Normal` で流す
- `async/` のプログラムの約束 (横取りのための `spin 40`、一度に出力するタスクは1つ、失敗の文言は1つ、ファイルはテストのディレクトリからの相対パス) は付録の `ui-tests.md` にある。harness は確かめず、レビューで確かめる

#### D-18: この段で決めた範囲

- `Task a` の `drop` は取り消さない。タスクは最後まで動き、`run` はそれも待ち、値は終わったときに放し、失敗はスコープの失敗になる (取り消しは async-cancel)
- `run` は根のスコープのすべてのタスクを待つ。失敗がなければ本体の値を返し、あれば終わった順で最初の失敗を起こし直して本体の値を捨てる
- タスクの `depth_offset` は 0 (タスクごとに `max_depth` のすべてを使う)
- EPIPE を `ConnectionReset` に写すことは後に回す。今写すと、タスクと関係のない `Child` の書き込みの誤りの種類 (docs/spec/stdlib.md:734) が変わる。閉じるときに roadmap の「その後の項目」へ移す (T-16)

## 6. 実装の約束

- 日本語のコメントと文書は yomiyasu に従い、コメントは理由を書いて `docs/` の節を引く。計画の ID (D-x、T-x) と `docs/plans/` はコードにも `docs/` にも書かない (CLAUDE.md)
- `docs/spec/` の節はその規則を実装するコミットに入れる (roadmap の「進め方」)
- `eml_rt` を変える task は Verus、verusfmt、信頼の一覧のテスト、Miri の予算を通す (crates/eml_rt/CLAUDE.md)。`eml_rt` に `SeqCst` と fence を入れない (D-4)
- `cells.rs` は `std` と `eml_rt` だけを import する (D-3)。VM の `Task` を切り替えるのは `resume` と `with_worker` だけで、`Scheduler::current` は `set_current` だけが書く (D-11、D-13)
- `Op::Async` の中に安全点を置かない。VM の `poll` は補助を通す (D-16)
- `RunStats` のスナップショットを書き直すのは T-13 の1回だけにする。ほかの task は bench の数を変えない
- 各 task の終わりに、共通の確認 `cargo nextest run`、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check` を通す (どれも終了コード 0)
- テストの変更の種類 (docs/implementation/testing.md の「テストの変更の運用」) は各 task に書いたとおりで、コミットのメッセージにも理由を書く

## 7. 実装の task

| ID | task | もとの決定 | 先に要るもの | 並行 |
|---|---|---|---|---|
| T-1 | `eml_rt`: worker の止める語、`PREEMPT`、終わりの欄 | D-4、D-5 | - | T-6、T-8、T-9 と並行可 |
| T-2 | VM: 終わりの欄 | D-5 | T-1 | 不可 |
| T-3 | VM: `Preempt` の扱い | D-16 | T-1 | T-2 と並行可 |
| T-4 | VM: `Normal` の方針と監視のスレッド | D-6、D-16 | T-3 | 不可 |
| T-5 | CLI の既定と UI の速いモード | D-6 | T-4 | 不可 |
| T-6 | `eml_rt`: `Fault::Raised` と `Stats::shared` | D-14、D-15 | - | 可 |
| T-7 | `fail_io`、新しい種類、`Proc.exited` | D-7、D-15 | T-6 | 可 |
| T-8 | `Either` | D-7 | - | 可 |
| T-9 | `Async` の ABI の表と `std/Async.em` | D-1、D-7 | - | 可 |
| T-10 | ランタイムの入口を根にする | D-1 | T-9 | 不可 |
| T-11 | VM のタスク (1): 構造、入口、始めと終わり、`Async.run` | D-2、D-3、D-10〜D-13 | T-4、T-6、T-10 | 不可 |
| T-12 | VM のタスク (2): 待ち、`await`、失敗、ハンドル | D-10〜D-13 | T-11 | 不可 |
| T-13 | `RunStats` の4つの欄 | D-14 | T-12 | 不可 |
| T-14 | harness と `async/` の UI テスト | D-17、D-18 | T-13、T-5、T-7 | 不可 |
| T-15 | タスクの中の `exit` | D-5、D-13 | T-14、T-2 | 不可 |
| T-16 | 段を閉じる | 全体 | T-15 | 不可 |

T-1 の前に、この計画書 (付録を含む) を段の最初のコミットとして入れる (roadmap の「進め方」)。

### T-1: `eml_rt` の worker の止める語、`PREEMPT`、終わりの欄

- 目的: D-4 と D-5 の `eml_rt` の部分を入れる
- 対象: `crates/eml_rt/src/control.rs`、`crates/eml_rt/src/heap/ctx.rs` (`poll`)、`crates/eml_rt/src/heap/tests/control.rs`、`crates/eml_rt/src/heap/tests/trusted.rs`
- 作業:
  - `Control` の欄を次にする (`verus!` の struct の中。新しい `::new` も `Control::new` の中)

```rust
pub struct Control {
    stop: AtomicU32,    // Control の止める語: INTERRUPT | MEMORY (意味は今と同じ)
    worker: AtomicU32,  // worker 0 の止める語: 止めるビットの写しと PREEMPT
    end: AtomicU32,     // 終わりの欄: 0 は開いている、1 は exit を書いた
    code: AtomicI64,    // end を勝ち取った者が書く終了コード
    bytes: AtomicUsize,
    limits: Limits,
}
pub enum Stop { Interrupted, DepthLimit, MemoryLimit, Ended, Preempt }   // verus! の中
```

  - メソッド (どれも `verus!` の外、安全、Relaxed): `interrupt()` は `stop` と `worker` に `fetch_or(INTERRUPT)`。`charge()` は上限を超えたら両方に `MEMORY`。`preempt()` は `worker.fetch_or(PREEMPT)`。`take_preempt()` は `worker.fetch_and(!PREEMPT)`。`worker_bits()` は `#[inline]` の load。`stop_bits()` は今のまま。`exit(code) -> bool` は `end` の CAS (0 → 1) に勝ったときだけ `code` を書いて真を返す。`exit_code() -> Option<i64>`。`begin_run()` は `end.store(0)` と `worker.fetch_and(!PREEMPT)`。`clear()` は `stop`、`worker`、`end` に 0 を書く
  - `stop_reason(bits)` は `#[cold]` のまま、`INTERRUPT` → `Interrupted`、`MEMORY` → `MemoryLimit`、`PREEMPT` → `Preempt`、ほかは `DepthLimit` の順に決める。`Display` は `Ended` → `the run has ended`、`Preempt` → `preempted`
  - `Ctx::poll` は `worker_bits()` を読む
  - テスト: `heap/tests/control.rs` に `preempt_reaches_only_the_worker_word`、`the_memory_limit_reaches_the_worker_word`、`begin_run_reopens_the_end_and_lowers_preempt`、`clear_lowers_every_bit_and_the_exit`、`stop_reason_puts_preempt_after_the_stops`、`the_first_exit_is_kept`。`trusted.rs` に `a_preempt_from_another_thread_is_seen` (スレッドを始めるので Miri の対象)
  - 文書: docs/spec/object-model.md の「実行の制御」(worker の止める語と終わりの欄の表、`poll` は worker の語を読み host の読み手は `Control` の語を読むこと、ビットと優先、誰がどれを下ろすか、Relaxed で足りる理由、D-5 の実行系によらない `exit` の約束) と、「`unsafe` の核」の `verus!` の外の文
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `nix develop -c cargo verus verify -p eml_rt` → `0 errors`、警告なし
  - `nix develop -c sh -c 'verusfmt --verus-only --check $(git ls-files "crates/eml_rt/src/*.rs")'` → 終了コード 0
  - `cargo nextest run -p eml_rt` → すべて通る (上の新しいテストを含む)
  - `/usr/bin/time -p nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::` → 通り、`user` ≤ 60 s。`-Zmiri-tree-borrows` を足して → 通り、`user` ≤ 120 s。新しいテストが Tree Borrows で足す時間は両モードで 5 s 以下
  - 共通の確認
- 落とし穴: 新しいビットを `Control.stop` に足さない。host.rs の `stop_bits()` の読み手 (443、567、591、659、685、739、1134、1147、1155) の振る舞いが変わる
- テストの変更: 追加だけ

### T-2: VM の終わりの欄

- 目的: D-5 の VM の部分。`Finished.exit` の出どころを `control.exit_code()` にする
- 対象: `crates/eml_vm/src/lib.rs` (`run_entry`、`run_recovering`)、`crates/eml_vm/src/vm.rs` (`finish()`、`stops_the_run` (vm.rs:1401))、`crates/eml_vm/tests/api.rs`
- 作業: `run_entry` の最初で `begin_run()`。`main` の `finish()` は今の確認の後で `task.exit == Some(n)` なら `control.exit(n)` を呼び、偽なら `Internal("an exit after the end of the run")`。`Finished.exit` と lib.rs:325 の `copied` の判断は `exit_code()` を使う。`vm::run` が `Stopped(Ended)` の `Err` を返したら、`exit_code().is_some()` を確かめ (違えば `Internal`)、その時点の数から `Finished { exit: Some(n), stats, preemptions }` を作り、`check_finished` と `check_heap` を飛ばす。`run_recovering` は `recover()` を呼んで `Ok(Finished)` を返し、回復に失敗したら誤りと同じく `Unrecovered { cause, .. }` を返す。`stops_the_run` に `Stop::Ended` を足す。`Task.exit` は巻き戻しの間の印としてだけ残す
- テスト: `api.rs` に `an_exit_is_written_to_the_end_field` (`exit 3` の後で `control.exit_code() == Some(3)`) と `a_machine_runs_again_after_an_exit` (同じ `Machine` の2回目の run の `exit` は `None`)
- 文書: docs/spec/vm.md の「`exit`」(底が終わりの欄に書くこと、`Finished.exit` はそこから来ること)、「誤り、止めること、終わり」(`begin_run`、捨てた run)、「追記」(捨てた `exit` の run の回復)、`RunConfig` の説明と vm.md:23
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件: `cargo nextest run -p eml_vm --test integration -E 'test(api::)'` → 通る。共通の確認。UI テストのスナップショットはバイト単位で変わらない
- テストの変更: 追加だけ

### T-3: VM の `Preempt` の扱い

- 目的: D-16 の補助と、安全点と `preempt` での `Stop::Preempt` の扱いを入れる
- 対象: `crates/eml_vm/src/vm.rs`、新しい `crates/eml_vm/tests/preempt.rs` (`tests/main.rs` に `mod preempt;`)
- 作業: D-16 の `poll` と `polled_stop`、`safepoint` と `preempt` の書き換え
- テスト (どちらも `Sequential` で、`write_line` でビットを立てる試しの Host で `CaptureHost` を包む): `a_preempt_without_a_ready_task_only_lowers_the_bit` (タスクが1つ。run は `Ok`、`preemptions == 0`、後で `worker_bits() & PREEMPT == 0`)、`a_stop_raised_with_preempt_wins` (Host が `PREEMPT` と `INTERRUPT` を一緒に立て、run は `interrupted()` で終わる)
- 文書: docs/spec/vm.md の「タスクとスケジュール」の安全点の段落と、2回の `poll` が正しい理由
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件: `cargo nextest run -p eml_vm --test integration -E 'test(preempt::)'` → 2つ通る。共通の確認
- テストの変更: 追加だけ

### T-4: `Normal` の方針と監視のスレッド

- 目的: D-6 と D-16 の方針と監視のスレッドを入れる
- 対象: `crates/eml_vm/src/sched.rs`、新しい `crates/eml_vm/src/monitor.rs`、`crates/eml_vm/src/lib.rs`、`crates/eml_vm/Cargo.toml`、`crates/eml_vm/tests/api.rs`、`crates/eml_vm/tests/par.rs`
- 作業: `Sched::{Normal, Sequential, Seeded(u64)}` (`#[default]` は `Sequential`)。`FromStr` の誤りの文は ``expected `normal`, `sequential` or `seeded:<n>`, found `{text}` ``。`Policy` に `fn task_added(&mut self)` と `fn switched(&mut self)` を足し、`Sequential` と `Seeded` では空の `#[inline(always)]`。`Normal` は `tick` が定数の偽、`reload` は何もしない、`pick` は `pop_front`、`wake` は `push_back`。`task_added` は監視のスレッドがなければ作り、`switched` はあれば `watch.switches.fetch_add(1, Relaxed)`。`Scheduler::set_current` を足し、`resume` の中の `current` の書き込みをこれに替える。`Scheduler::add_task` で `task_added()` を呼ぶ。`run_entry` は `Sched::Normal => run_scheduled(.., Normal::new(Arc::clone(ctx.heap().control())), ..)`。方針ごとに VM のループの写しを1つ持つ
- テスト: `monitor.rs` の `#[cfg(test)]` に `a_tick_without_a_switch_asks_for_a_switch`、`a_tick_after_a_switch_does_not`、`dropping_the_policy_stops_the_monitor`。`api.rs` に `send::<Normal>()`、`send::<Scheduler<Normal>>()`、`"normal".parse() == Ok(Sched::Normal)`、誤りの文の期待値の更新。`par.rs` の `run_par_program` に `Sched::Normal` (検査モード。出力と `schedule_independent` の統計を比べる) を足す。`par.rs` に `#[ignore = "監視のスレッドの 10 ms の刻みを数えるので、マシンの速さに依る"] fn the_monitor_preempts_a_long_par_child` (200 ms 以上動かして `preemptions > 0`)
- 文書: docs/spec/vm.md の「タスクとスケジュール」(3つの方針、監視のスレッドの寿命と刻み、`preemptions` が監視の切り替えも数えること) と「上限と割り込み」の最後の項目 (監視のスレッドはすべてのシグナルを塞ぐ)。docs/implementation/architecture.md (eml_vm のファイルと依存、libc の行)。docs/future/multicore.md の「1つの worker での実行」(普通の方針があること) と「スレッドと worker」(D-4 の書き方)
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `cargo nextest run -p eml_vm` → すべて通る (`#[ignore]` は流さない)
  - `nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings` → 終了コード 0
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::` → 通り、`user` ≤ 100 s。Tree Borrows を足して ≤ 150 s
  - 共通の確認
- テストの変更: 種類 2 (期待値): `crates/eml_vm/tests/api.rs` の `Sched` の解析の誤りの文
- 落とし穴: `pthread_sigmask` で囲まずに `spawn` すると、SIGINT が監視のスレッドに届いて、VM のスレッドの読みに `EINTR` が来なくなる

### T-5: CLI の既定と UI の速いモード

- 目的: D-6 の既定を CLI と UI テストに入れる
- 対象: `crates/eml_cli/src/main.rs`、`crates/eml_cli/tests/ui.rs`、`crates/eml_cli/tests/cli.rs`
- 作業: `run` と `repl` の `--sched` を `#[arg(long, value_name = "normal|sequential|seeded:N", default_value = "normal")]` にする。`assert_fast_mode_agrees` は `Sched::Normal` で流す。CLI の試験 `an_interrupt_stops_a_par_program_with_130` (unix) を足す。既定の方針で `par` の子が回り続け (監視のスレッドが動く)、SIGINT 1回で終了コード 130、stderr に `interrupted` を含む。cli.rs:255-338 の `spawn_run`、`sigint`、`DEADLINE` を使う
- 文書: docs/spec/stdlib.md の「起動」(:19)、repl.md (:7)、docs/implementation/architecture.md (:362-363)、docs/implementation/status.md (:35)、docs/implementation/testing.md (:387。「2つのモードで流す」に速いモードは普通の方針で流すこと、「性能のテスト」にその刻みに依るものはないこと)、CLAUDE.md の Commands の `--sched` の行
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `cargo nextest run -p eml_cli` → すべて通る
  - `cargo run -p eml_cli -- run --help` → `[default: normal]` を含む
  - 共通の確認
- テストの変更: 種類 2: `crates/eml_cli/tests/cli.rs` の `sched_flag_is_accepted_and_rejected` (`--sched normal` を受ける場合と、誤りの文)。`crates/eml_cli/tests/ui.rs` の速いモードの run を `Sequential` から `Normal` へ (期待する出力は変わらないが、既存のテストの流し方が変わるので挙げる)

### T-6: `eml_rt` の `Fault::Raised` と `Stats::shared`

- 目的: D-15 の `Fault::Raised` と D-14 の `shared_objects` の数え先を入れる
- 対象: `crates/eml_rt/src/externs.rs` (`Fault`)、`crates/eml_rt/src/heap/share.rs`、`crates/eml_rt/src/heap/mod.rs` (`Stats`)、`heap/tests/`
- 作業: `Fault::Raised { kind: i64, message: Box<str> }` を足す (`io_error_kind() == Some(kind)`、`panics() == false`、表示は `message`)。`share` が rc を負にする所で `Stats::shared` を1つ増やす (検査モードだけ。`Stats` の他の欄と同じく証明する不変条件の外。heap/mod.rs:100-101)
- テスト: `Raised` の表示と分類の単体テスト、`share` が数える単体テスト
- 文書: docs/spec/object-model.md の `Fault` の表に `Raised` の行、「仕事の回数」に `shared`
- 実装者に任せること: なし
- 条件: いつも。`Stats::shared` を Verus が受け付けなければ Q-5 に従う
- 完了の条件: T-1 と同じ Verus、verusfmt、`cargo nextest run -p eml_rt`、Miri の2つの設定。`faults_stay_small` が通る。共通の確認
- テストの変更: 追加だけ

### T-7: `fail_io`、新しい種類、`Proc.exited`

- 目的: D-7 と D-15 の `fail_io`、`IOErrorKind` の5つの種類、`Proc.exited` の書き直し
- 対象: `std/Prelude.em`、`std/Proc.em`、`crates/eml_extern/src/lib.rs`、`crates/eml_vm/src/vm.rs`、`crates/eml_rt` (消すもの)、`crates/eml_core_ir/tests/externs.rs`、UI テスト
- 作業:
  - Prelude の `exit` の直後に、次を置く

```haskell
-- IO の失敗を種類と文言で起こす。戻らない (docs/spec/stdlib.md の「`fail_io`」)
pub extern fail_io : IOErrorKind -> String -> <IO> a
```

  - `IOErrorKind` を次にする (1行に1つの構築子で書いている今のファイルの形に合わせる)

```haskell
pub data IOErrorKind =
  | NotFound | PermissionDenied | NotAvailable | NotUtf8 | Other
  | Cancelled | TimedOut | ConnectionReset | AddrInUse | ResourceLimit
  deriving (Eq, Show)
```

  - D-15 の表の行、VM の腕、`Proc.exited`、消すものすべて
  - UI テスト `run/io/fail_io.em` と `run-fail/io/fail_io_uncaught.em` (付録の `ui-tests.md`)
- 文書: docs/spec/stdlib.md の「`try_io`」(10の構築子、この実装の extern の誤りは最初の5つだけを出し `fail_io` はどれでも出せること、`Fault` の表の `Exited` を `Raised` に替えること (`fail_io` と、`Async.await`/`Async.run` がタスクの IO の失敗を起こし直すときに出る)、`Prelude.Cancelled` と `Wake.Cancelled` の名前の重なり)、新しい節「`fail_io`」(「`try_panic`」の後。シグネチャ、戻らないこと、`try_io` が種類と文言を受けること、`try_panic` は捕まえないこと、捕まえなければ呼んだ位置の実行時エラーになること)、「`Proc`」(:703 の `exited` の説明、:732 の `Exited`)。docs/spec/vm.md の「extern の中断」(`fail_io` の項目)、docs/spec/effects.md:63、docs/spec/lir.md:254、docs/spec/object-model.md (:950 の `Exited` の行、:918、:970、:1188 の `proc_exited`、T11 のテストの一覧)、docs/implementation/testing.md:175
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `rg -n 'Fault::Exited|proc_exited|ProcExited|Std\.Proc\.exited|struct Exited' crates docs/spec docs/implementation` → 何も出ない
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::)'` → 通る
  - Verus、verusfmt、Miri の2つの設定 (T-1 と同じ)
  - 共通の確認
- テストの変更:
  - 種類 1: `crates/eml_rt/src/heap/tests/trusted.rs::proc_exited_releases_its_string` と、:1790 の一覧の項目を消す (消す関数を確かめるテストのため)
  - 種類 2 (一つずつ): `the_io_error_kind_tags_match_the_prelude_declaration` に `("Cancelled", 5)` から `("ResourceLimit", 9)` までの5組を literal で足す (Rust の定数がないことをコメントに書く)。`crates/eml_rt/src/heap/tests/externs.rs::child_faults_are_shown_and_caught` から `Exited` の場合を除く。`run-fail/std/proc_capture_nonzero.em` のスナップショットの `at` の行 (`<std>/Proc.em:58:30` から `exited` の中の `fail_io` の呼び出しへ)
  - 種類 2 (範囲): Prelude の新しい名前で名前の候補が変わる check-fail のスナップショット

### T-8: `Either`

- 目的: D-7 の `Either` と modules.md の項目を直すこと
- 対象: `std/Prelude.em` (`Result` の後)、UI テスト
- 作業:

```haskell
pub data Either a b =
  | Left a
  | Right b
  deriving (Eq, Ord, Show)
```

  - UI テスト `run/data/prelude_either.em` と `check-fail/linearity/either_holding_a_file.em` (付録)
- 文書: docs/spec/declarations.md の「Prelude」(`Either` のブロックと項目、:365 の `Either` の文が Prelude の `Either` を指すこと)、docs/spec/modules.md の「Prelude」の5つ目の項目 (D-7 の文)
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件: `cargo nextest run -p eml_cli --test integration -E 'test(ui::)'` → 通り、`prelude_either` のスナップショットは `Left 1`、`Right "x"`、`True`、`0`、`5` の5行。共通の確認
- テストの変更: 種類 2 (範囲): `Either`、`Left`、`Right` で名前の候補が変わる check-fail のスナップショット

### T-9: `Async` の ABI の表と `std/Async.em`

- 目的: D-1 の ABI と D-7 の `Failure`、`try_all` を std と表に入れる。最小の `pub` の面 (`run`、`start`、`spawn`、`await`、`yield_now`) もここで書く
- 対象: `crates/eml_extern/src/lib.rs`、新しい `std/Async.em`、`crates/eml_hir/src/lib.rs` (`STD`)、`crates/eml_lir/src/lib.rs` (タグ)、`crates/eml_vm` (表の行の腕)、tie のテスト
- 作業:
  - `std/Async.em` を付録の `Async.em` で作る
  - `eml_extern` に `pub enum RuntimeEntry { StartTask, ResumeTask }`、`ALL`、`name()` (`Std.Async.start_task`、`Std.Async.resume_task`)、`arity()` (1、2)、`params()` (`[TObj]`、`[TObj, Enum]`)、`Extern::is_async()`
  - 表の行を `Par` の後に足す

| 行 | 名前 | params | param kinds | ret と Kind | purity | with_arg |
|---|---|---|---|---|---|---|
| `AsyncStartRaw` | `Std.Async.start_raw` | `[TObj]` | `[Lin]` | `Obj`、Lin | Effectful | |
| `AsyncRunRaw` | `Std.Async.run_raw` | `[TObj]` | `[Lin]` | `Obj`、Lin | Effectful | |
| `AsyncPark` | `Std.Async.park` | `[Int, TObj]` | `[Unr, Lin]` | `Unit`、Unr | Effectful | |
| `AsyncReady` | `Std.Async.ready` | `[TObj]` | `[Lin]` | `Unit`、Unr | Effectful | |
| `AsyncFinishOk` | `Std.Async.finish_ok` | `[TObj]` | `[Lin]` | `Unit`、Unr | Effectful | |
| `AsyncFinishIo` | `Std.Async.finish_io` | `[Enum, Obj]` | `[Unr, Unr]` | `Unit`、Unr | Effectful | |
| `AsyncFinishPanic` | `Std.Async.finish_panic` | `[Obj]` | `[Unr]` | `Unit`、Unr | Effectful | |
| `AsyncTaskCell` | `Std.Async.task_cell` | `[Obj]` | `[Lin]` | `Obj` (組。1つ目は `Int`)、Lin | Pure | `Some(0)` |
| `AsyncTakeResult` | `Std.Async.take_result` | `[Obj]` | `[Lin]` | `TObj`、Lin | Effectful | |
| `AsyncRunResult` | `Std.Async.run_result` | `[Obj]` | `[Lin]` | `TObj`、Lin | Effectful | |

  - `ExternType` に `Cell => type_row("Std.Async.Cell", Unr, Repr::Int)` と `Task => generic_type_row("Std.Async.Task", Lin)`
  - `eml_lir` に `WAKE_WOKE = 0`、`WAKE_CANCELLED = 1`
  - VM の `run_extern` の `Std.Async.*` の腕は `Internal("Async externs run through Op::Async")` を返す (T-11 までは `Async` のプログラムを動かすとこの誤りになる)
  - `eml_hir::STD`、docs/spec/modules.md:69 の std の一覧、docs/implementation/status.md:28 の一覧に `Async.em` を足す
- テスト:
  - 新しく: `crates/eml_hir/tests/externs.rs::every_runtime_entry_is_a_monomorphic_std_function` (名前が `std/Async.em` で引け、型変数と row 変数がなく、引数の数が `RuntimeEntry::arity`)。`crates/eml_core_ir/tests/externs.rs` に `Wake` のタグの試験 (`the_ordering_tags_match_the_prelude_declaration` と同じ形)
  - 種類 2: `every_extern_function_is_declared_once_in_std` の決まりを、`STORED_CALLBACK_ROWS` に載せた行 (`start_raw`、`run_raw`、`park`、`ready`、`finish_ok`) だけは引数の矢印に閉じた row を許す形に緩める。`only_the_handle_and_mutable_array_rows_carry_a_lin` に Lin の行 (`start_raw`、`run_raw`、`park`、`ready`、`finish_ok`、`task_cell`、`take_result`、`run_result`) を、`only_the_listed_rows_borrow_or_return_a_pair` に `task_cell` を足す
- 文書: docs/spec/declarations.md の「`extern`」(継続を預かる行の引数の矢印に閉じた row を許すこと)、docs/spec/stdlib.md の新しい節「`Std.Async`」の ABI の部分 (エフェクト、`Wake`、`Cell`、`Task`、名前で引く入口、extern) と `Failure`、`try_all`、docs/spec/types.md:61-66 の注記、docs/future/multicore.md:106-108 と docs/future/async.md の「`Async` のエフェクト」の3つ目の項目 (D-1)
- 実装者に任せること: なし
- 条件: いつも。決まりを緩めることが受け入れられないときは Q-1 の既定に従う
- 完了の条件:
  - `cargo nextest run -p eml_extern -p eml_hir -p eml_core_ir` → 通る
  - 次の内容の `$SCRATCH/x.em` を `timeout 20 cargo run -p eml_cli -- check $SCRATCH/x.em` → 何も出さず終了コード 0 (`$SCRATCH` は作業ツリーの外の一時ディレクトリ)

```haskell
import Async
main : Unit -> <IO> Unit
main () = ()
```

  - 共通の確認

### T-10: ランタイムの入口を根にする

- 目的: D-1 の入口を translate と reach の根にする
- 対象: `crates/eml_core_ir/src/translate/instances.rs:801`、`crates/eml_core_ir/src/translate/program.rs:765` 付近、`crates/eml_core_ir/src/optimize/reach.rs`
- 作業: `HirProgram::function_by_canonical_name(&str) -> Option<FunctionId>` を `core_name` の隣に足す。`instances::collect` は入口の後で、`RuntimeEntry` ごとに名前が HIR の関数に引ければ `collector.add(f, vec![], vec![])` を呼び、型変数と row 変数がないことを `debug_assert!` で確かめる。`retain_reachable` は `pending` を入口と、名前が `RuntimeEntry::name()` の関数で始める
- テスト: `crates/eml_core_ir/tests/externs.rs::the_runtime_entries_have_the_declared_abi` (`import Async\nmain () = ()` を下ろし、2つの関数を名前で見つけ、`params` の Repr が `RuntimeEntry::params()` と等しく、`split == None`、`!internal`、`ret == Repr::Unit`)。`crates/eml_core_ir/tests/lowering.rs` に、`import Async` があって何も呼ばないときは2つの名前が残り、import がなければどちらもないことの試験
- 文書: docs/spec/core-ir.md の「変換の規則」(入口のほかに、ランタイムが名前で呼ぶ関数を instance にすること) と「最適化」の reach の段落
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件: `cargo nextest run -p eml_core_ir --test integration -E 'test(/runtime_entr/)'` → 3つ通る。共通の確認
- 落とし穴: eml のコードは入口を呼ばないので、閉包を特殊化した写しはできない。写しができたら、どこかが入口を呼んでいる

### T-11: VM のタスク (1) 構造、入口、始めと終わり、`Async.run`

- 目的: D-2、D-3、D-10〜D-13 のうち、`cells.rs` の型と `start`、`open_root`、`wait`、`finish`、`take` の値の道、`drain`、`sweep_all`、`with_worker`、VM の `Task.cell`、`Entry`、`WaitingCell`、`Task::entry`、スタックの集まり、`Op::Async` と codegen、`CodeBase.entries`、`MissingEntry`、`main` の `Finish` の片付けを入れる
- 対象: 新しい `crates/eml_vm/src/cells.rs`、`sched.rs`、`vm.rs`、`code.rs`、`compile/`、新しい `crates/eml_vm/tests/tasks.rs` (`tests/main.rs` に `mod tasks;`)
- 作業: 5.2 の D-10〜D-13 のとおり
- テスト (`tasks.rs`。各試験は `eml_test_support::run` の検査モードと、`eml_test_support::execute(&lir(src), &RunConfig::default())` の速いモードの両方で流し、stdout を比べる):
  - (a) `run (fn () -> 41 + 1)` を出力 → `42\n`
  - (b) タスクの中の入れ子の `run` → `inner\nouter\n`
  - (c) 本体が `a`、`b` を出力する2つの子を始めて両方を待つ → 逐次で `main\na\nb\n`
  - (d) (c) を `Sched::Seeded(1)` と `Seeded(2026)` で流す → stdout が同じで、`schedule_independent` の統計が等しい
  - (e) `cells.rs` に `use crate::` の行がない (ファイルを読んで確かめる)
- 文書: docs/spec/vm.md の新しい節「`Async` のタスク」(状態、入口の `Task`、セル、登録表、受け箱、run の終わり、D-2 の2種類の待ち、D-3 の核と実行系の分け方)、「命令」の一覧に `Async`、「タスクとスケジュール」に `Async` の仕事は FIFO で積むこと、「追記」に `MissingEntry`
- 実装者に任せること: (b) と (c) のプログラムの細部 (期待する stdout を変えない範囲)
- 条件: いつも
- 完了の条件: `cargo nextest run -p eml_vm --test integration -E 'test(tasks::)'` → 5つ通る。共通の確認
- 落とし穴: `transfer_back(closure, from = spawner)` が `WrongOwner` を出したら Q-2 の既定に従う

### T-12: VM のタスク (2) 待ち、`await`、失敗、ハンドル

- 目的: `park`、`ready`、`task_cell`、`take_result` と `run_result` の失敗の道、`TaskHandle` の `Drop` と受け箱の処理、`StaleTask`、D-11 の不変条件の `debug_assert!`
- 対象: `cells.rs`、`vm.rs`、`tasks.rs`、REPL のトランスクリプト
- 作業: 5.2 のとおり。`take_result` の `Raise::Panic(m)` は `Extern::Panic` の道 (`abort_to(Panic, PANICKED, [fresh_string(m)])`、なければ `VmFault::Panicked(m)`)、`Raise::Io { kind, m }` は `failed(Fault::Raised { kind, message })` (vm.rs:1533-1558)
- テスト (`tasks.rs`、2つのモード):
  - (f) 相手が終わる前と後に待つ → どちらも `x\n`
  - (g) 3つのタスクが `yield_now` を2回ずつ → `a1\nb1\nc1\na2\nb2\nc2\n`
  - (h) 子の `panic "boom"` を待つ → `RuntimeError` の表示が `` runtime error: panicked: boom in `main` `` と等しい。`await` を `try_panic` で囲むと → `Err boom\n`
  - (j) `MutArray` を返すタスクを `spawn` して待たない → 検査モードの run が `Leak` なしで終わる
  - (l) `Lin` の結果: `MutArray.empty ()` を返すタスクを待って `MutArray.length` → `0\n`
  - (k) は REPL のトランスクリプト `crates/eml_cli/tests/repl/async/stale_handle.repl` にする。1つ目の入力で `Async.run` が返したハンドルを束縛し、2つ目の入力で `Async.run (fn () -> Async.await t)` → `a task handle outlived its run` の実行時エラー
- 文書: docs/spec/vm.md の「`Async` のタスク」を仕上げる。docs/spec/stdlib.md の「実行時エラー」に `a task handle outlived its run` の行 (REPL の入力をまたいだとき)
- 実装者に任せること: (f)〜(l) (欠番の (i) は T-13) のプログラムの細部 (期待を変えない範囲)。(k) の REPL の束縛の書き方
- 条件: いつも
- 完了の条件: `cargo nextest run -p eml_vm --test integration -E 'test(tasks::)'` → 10 個通る。`cargo nextest run -p eml_cli --test integration -E 'test(repl::)'` → 通る。共通の確認
- 落とし穴: タスクを放すときの `retire_worker` が正しいプログラムで `TransferLeak` を出したら Q-2 の既定に従う

### T-13: `RunStats` の4つの欄

- 目的: D-14 の欄を `RunStats` に足す。スナップショットの書き直しはこの1回だけにする
- 対象: `crates/eml_vm/src/lib.rs` (`RunStats`、`run_entry`)、`RunStats` を固定するスナップショット
- 作業: `tasks`、`parks`、`dropped_failures`、`shared_objects` を足す (`Registry.stats` と `Stats::shared` から写す)
- テスト: (i) 失敗する子が2つ → `RunStats.dropped_failures == 1`。(m) `tasks.rs` の (c) を2つの seed で流し、4つの欄が seed の間で等しく、検査モードで `tasks == 3`、`parks == 2`、`shared_objects > 0` であること
- 文書: docs/spec/vm.md の「`RunStats`」(17欄と定義。`dropped_failures` の数え方)、docs/implementation/testing.md の「seed で流す」の理由の一覧
- 実装者に任せること: なし
- 条件: いつも。欄がスケジュールによって変わったら Q-3 の既定に従う
- 完了の条件:
  - `cargo nextest run` → 通る
  - `rg -c 'shared_objects: 0' crates/eml_vm/tests/bench.rs` の数が、同じファイルの `RunStats {` の数と等しい
- テストの変更: 種類 2 (範囲): `rg -l 'RunStats \{' crates/*/tests` で見つかるすべての inline の `RunStats` のスナップショット (`crates/eml_vm/tests/bench.rs` の48個を含む)。既存のプログラムでは新しい欄がどれも 0 になる。`cargo insta review` で受け入れる

### T-14: harness と `async/` の UI テスト

- 目的: D-17 の harness と、`Std.Async` の利用者向けの規則と UI テストを入れる
- 対象: `crates/eml_cli/tests/ui.rs`、`tests/ui/run/async/`、`tests/ui/run-fail/async/`、`tests/ui/check-fail/types/`、`tests/ui/check-fail/linearity/`
- 作業: D-17 の harness の変更。付録の `ui-tests.md` のうち、`run/async/` の8つ (`exit_in_task.em` を除く)、`run-fail/async/` の3つ、`check-fail/types/task_captures_var.em`、`check-fail/types/async_in_par_child.em`、`check-fail/linearity/task_awaited_twice.em`
- 文書: docs/spec/stdlib.md の「`Std.Async`」に `pub` の面の表 (`run`、`start`、`spawn`、`await`、`yield_now`、`try_all`、`Failure`) と、次の8つの規則
  1. 暗黙のスコープ: `Async.run f` は根のスコープを開き、`f ()` をその中の最初のタスクとして動かす。`start` と `spawn` は新しいタスクを、呼んだタスクのスコープ (呼び出しを動的に囲むいちばん内側の `run` の根) に加える。孫も同じ根に入る。タスクの中の入れ子の `run` は自分の根を開く
  2. `run` の終わり: `run` は根のスコープのすべてのタスクが終わってから戻る (待ったかどうかによらない)。失敗がなければ本体の値を返し、あれば終わった順で最初の失敗を起こし直して本体の値を捨てる。IO の失敗は種類と文言を、Panic は文言を保つ。後の失敗は報告せず、1つずつ `RunStats.dropped_failures` に数える。失敗したタスクを待って、起こし直した失敗を捕まえなかったタスクも数える
  3. `Task a` は `Lin` で、`await t` が消費する。`await` は値を返すか、タスクの失敗を起こし直す。失敗はスコープの失敗のままで、`await` の周りで捕まえても `run` は起こし直す。失敗を分けるには、タスクの中で `try_all` を使う
  4. `drop t` はハンドルを放すだけで、タスクは最後まで動き、`run` はそれも待つ。値は終わったときに放し、失敗はスコープの失敗になる
  5. ハンドルは、1つのプログラムの実行の中なら、作った `run` の外へ出てよい。そのときタスクは終わっているので、`await` はすぐ値を返すか失敗を起こし直す。REPL の入力をまたいだハンドルは実行時エラー ``a task handle outlived its run`` である
  6. どのタスクの `exit n` も、プログラムを `n` で終える (T-15 で書く)
  7. `yield_now ()` はタスクを実行待ちの列の後ろへ回す
  8. タスクは閉じた row `<IO, Panic, Async>` で始まる。始めた側の handler と `var` は見えず、`var` を捕まえると型の誤りになる
  - docs/spec/stdlib.md の「実行時エラー」に、`Async.run` と `Async.await` が起こし直す失敗は文言を保ち、位置が std の呼び出しになること。docs/implementation/testing.md の「UI テスト」の run と run-fail の分類に `async/` の項目 (`par/` の隣、:320)、「seed で流す」に `par/` と `async/`、横取りの約束、出力とファイルの約束 (レビューで確かめる)、`exit` で終わる `async/` の run は統計を比べないこと
- 実装者に任せること: 各 UI テストの1行目の日本語のコメントの文。期待と違う出力が出たときは、ランタイムの不具合かプログラムの誤りかを確かめる (Q-4)
- 条件: いつも
- 完了の条件:
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::) | test(seeded_runs_cover_par_and_async)'` → 通る。`run/async/` の各ファイルが、検査モードの逐次 (スナップショット)、速いモードの普通の方針、検査モードの seed 1 と 2026 (`preemptions > 0`) で流れる
  - `cargo nextest run -p eml_vm --test integration -E 'test(tasks::)'` → 通る
  - `cargo nextest run -j 1 -p eml_cli --test integration -E 'test(ui::run)'` → `SLOW` が出ない
  - 共通の確認
- テストの変更: 種類 2: `seeded_runs_cover_only_par` を `seeded_runs_cover_par_and_async` に改める。`async/` の `exit` の run で統計を比べない。横取りがないときの文言

### T-15: タスクの中の `exit`

- 目的: D-5 の手順をタスクの底に入れる
- 対象: `cells.rs` (`end_with_exit`)、`vm.rs`、`crates/eml_vm/tests/api.rs`、`crates/eml_vm/tests/preempt.rs`、`tests/ui/run/async/exit_in_task.em`、REPL のトランスクリプト
- 作業: `cells::end_with_exit(control, n)` を `control.exit(n)` を呼ぶ唯一の口として足す (実行系によらない約束の1か所)。入口の `Task` の `Op::Finish` で `task.exit == Some(n)` なら、`ctx.yield_finish()`、`evv` を `()` にし、`end_with_exit` を呼び (結果は見ない)、すぐ `Err(VmFault::Stopped(Stop::Ended))` を返す。タスクのセルにもスコープにも記録せず、ほかのタスクを動かさない。`run_recovering` は `Scheduler` (と `Registry`) を先に捨ててから `recover()` を呼ぶ
- テスト:
  - `api.rs` の `an_exit_from_a_task_recovers_and_reports_the_code`: 検査モードの `Machine` で、本体が `FileIn` を持ち別のタスクが待つ間にタスク `t` が `exit 3` → `Finished.exit == Some(3)`、`close()` が `Ok` (`Leak` なし)、同じ `Machine` の次の run も成功する
  - `preempt.rs` の `a_preempt_request_switches_at_the_next_safepoint`: `Async.run` の本体がタスク `t` (`println "b"`) を始め、`"a"` を出力し、回り、`"c"` を出力する。`Sequential` で、`"a"` のときに `control.preempt()` を呼ぶ Host を使う → stdout `a\nb\nc\n`、`preemptions == 1`
  - UI テスト `run/async/exit_in_task.em` (付録)
  - REPL のトランスクリプト `crates/eml_cli/tests/repl/async/exit_in_task.repl`: タスクの `exit 3` で `exited with code 3` と出て、次の入力が評価される
- 文書: docs/spec/stdlib.md の「`exit`」にどのタスクの `exit` もプログラムを終えること (ほかのタスクは巻き戻さず、その `Lin` の資源はヒープを落とすときに後始末すること、`try_*` は捕まえないこと)、「`Std.Async`」の規則 6
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `cargo nextest run -p eml_vm --test integration -E 'test(api::) | test(preempt::)'` → 通る
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::) | test(repl::)'` → 通る。`exit_in_task.em` のスナップショットは `--- stdout ---\nbefore\nexiting\n--- stderr ---\n--- exit: 3 ---` で、`Leak` がなく、速いモードと2つの seed の run が出力と終了コードで一致し、`preemptions > 0`
  - 共通の確認

### T-16: 段を閉じる

- 目的: roadmap の「進め方」の閉じる手順を1つのコミット `Close async-tasks` で行う
- 作業 (roadmap の手順の順):
  1. `spec/` を仕上げる。段の名前と「今は〜、後で〜」の予告が残っていないことを確かめる。docs/spec/expressions.md の「並行処理」に `Async` だけで書いた例を置き、「並行処理は実装していない」の文を消す
  2. 計測する。まず `bench/scripts.sh` を流し、実時間が範囲を外れた行があれば大きさを変えて別のコミット (`Resize bench rows`) にする。次に `bench/run.sh`、`bench/rt.sh`、`bench/scripts.sh` をコードの最後のコミットの作業ツリーで流し、docs/implementation/benchmarks.md の「今の数値」の3つの表を入れ替える。`bench/scripts.sh` は `eml run` の既定 (普通の方針) を測る。測れる環境 (macOS) がなければ、表の入れ替えだけを直後の別のコミット (`Measure async-tasks`) にする
  3. docs/implementation/status.md の「今の言語の範囲」に `Async` の項目を足し、制限の節に、取り消しがないこと (`drop` は取り消さない)、`scope`、`spawn_in`、時間、channel、I/O の driver がないこと、入れ子の `run` はそのタスクを待たせること、終わった順で最初の失敗が勝つこと、新しい種類は `fail_io` だけが出すことを書く
  4. docs/implementation/architecture.md を、`cells.rs` と `monitor.rs` と libc の依存に合わせて直す (T-4 で足した所を確かめる)
  5. docs/future/roadmap.md から `async-tasks` の項目を消す。「その後の項目」の「非同期」に次の項目を足す。docs/future/async.md の「スコープとタスク」「失敗と `exit`」「テストと決定性」から、stdlib.md と vm.md が書いた内容を消し (`Failure` と `try_all`、`fail_io`、種類の一覧、`Either`、`Prelude.Cancelled` の重なり、1つの worker の `exit`、seed を既定にする文)、`scope`、`spawn_in`/`start_in`、`cancel_scope`、`fail_scope`、`cancel`、スコープの終わりの手順、`reraise`、取り消しとしての `drop`、取り消しの項目、`ResourceLimit` を作る所、複数の worker の `exit`、テストの driver と仮想の時計を残す。EPIPE の項目は「ネットワークとシグナル」へ移す。「前提」に `async-tasks` を書いた段の名前を項目の名前に直す
     - EPIPE を `ConnectionReset` に写すこと
       - 後に回した理由: タスクと関係のない `Child` の書き込みの誤りの種類を変えるため。
       - 見直す条件: ソケットを足す段 (`TcpStream` の書き込みが EPIPE を返す段)。
  6. docs/overview.md の確定した設計判断の表と、docs/README.md の「文書の地図」の「内容」の欄を、変わった所だけ直す
  7. CLAUDE.md を、規則か道しるべが変わったときだけ直す (`--sched` の行は T-5 で直した。Architecture の表の `eml_vm` の行に `Async` のタスクを足すかを確かめる)
  8. この計画書と付録のディレクトリを消す。消す前に、5章の決定のうち「リポジトリに残す所」に書いたものが `docs/` にあることを確かめる
  9. コミットのメッセージに、文書で何をどこへ移したか、テストの変更の種類と理由 (T-4、T-5、T-7、T-8、T-9、T-13、T-14 のもの)、計測の目立つ差、大きさを変えた行、EPIPE を後に回すと決めた日 (2026-10-27) を書く
- 完了の条件:
  - `cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` → 通る
  - `rg -n 'async-tasks' docs --glob '!docs/reports/**'` → 何も出ない
  - `ls docs/plans` → この計画のファイルがない
  - 「9. 確かめ方」の全体の確認

## 8. 進める順

```
計画のコミット
  ├─ T-1 ─┬─ T-2 ───────────────────────────────┐
  │       └─ T-3 → T-4 → T-5 ─────────────┐     │
  ├─ T-6 → T-7 ───────────────────────┐   │     │
  ├─ T-8                              │   │     │
  └─ T-9 → T-10 ─┐                    │   │     │
                 T-11 ← (T-4, T-6)    │   │     │
                 T-12                 │   │     │
                 T-13                 │   │     │
                 T-14 ← (T-5, T-7) ───┴───┘     │
                 T-15 ← T-2 ────────────────────┘
                 T-16 ← すべて
```

- 並べて進められる組: {T-1〜T-5}、{T-6、T-7}、{T-8}、{T-9、T-10}。T-11 から後は一列である
- 1つずつ進めるときの既定の順: T-1、T-2、T-3、T-4、T-5、T-6、T-7、T-8、T-9、T-10、T-11、T-12、T-13、T-14、T-15、T-16

## 9. 確かめ方

- 基準 (最初の変更の前): `cargo nextest run` → すべて通る。`nix develop -c cargo verus verify -p eml_rt` → `0 errors`
- 全体の確認 (すべての task の後、devShell の中で):
  - `cargo nextest run` → すべて通る (`#[ignore]` は流さない)
  - `cargo clippy --all-targets -- -D warnings` → 終了コード 0
  - `cargo fmt --check` → 終了コード 0
  - `cargo clippy -p eml_cli --no-default-features --features types -- -D warnings` (`none`、`core` でも) → 終了コード 0
  - `nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings` → 終了コード 0
  - `cargo verus verify -p eml_rt` → `0 errors`、警告なし
  - `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` → 終了コード 0
  - Miri: `eml_rt trusted::` と `eml_vm --test integration miri::` を2つの設定で (予算は T-1 と T-4)
  - `nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- .github/workflows/ci.yml` → 終了コード 0 (CI の定義を変えたときだけ)
- 利用者に頼む手で確かめること:
  - macOS で `bench/run.sh`、`bench/rt.sh`、`bench/scripts.sh` を流す (T-16 の計測。クラウドでは流せない)
  - 端末で `eml run` の既定の方針の `par` のプログラムに Ctrl-C を1回送り、130 で終わることを見る (T-5 の CLI の試験を手で確かめる。成功の条件のうち、既定の方針を変えたことを確かめる)
  - `eml repl` で `import Async` の後に `Async.run` を使い、タスクの `exit 3` で `exited with code 3` が出て次の入力が動くこと (T-15)

## 10. 危険と未決の項目

| ID | 項目 | 影響 | どう決めるか | 決まらないときの既定 |
|---|---|---|---|---|
| Q-1 | extern の引数の矢印に閉じた row を許すよう tie のテストを緩めること | T-9 | T-9 の実装者がレビューで確かめる | 緩めない形にする。std で `data Parked = \| Parked (once Wake -> <IO> Unit)` に包み、行は `Obj` を受ける。VM のコードは同じで、待つたびに確保が1つ増える |
| Q-2 | 始めるときの `transfer_back(closure, from = spawner)` が `WrongOwner` を出さないこと、タスクを放すときの `retire_worker` が `TransferLeak` を出さないこと (推定) | T-11、T-12 | 実装のテストで分かる | `WrongOwner` なら始めるときの `transfer_back` をやめる (`par` の右の子と同じ)。`TransferLeak` なら `retire_worker` を run の終わりに回し、async-threads の見直しの項目に書く |
| Q-3 | `RunStats` の4つの欄がスケジュールによらないこと (推定) | T-13、T-14 | T-13 の (m) と T-14 の seed の run で分かる | 違った欄は `schedule_independent` で 0 にし、理由を docs/implementation/testing.md に書く |
| Q-4 | `Async` の UI テストの期待する出力と `<std>/Async.em:L:C` は推定である | T-14、T-15 | 実装の後で流して確かめる | 違ったら、ランタイムの不具合かプログラムの誤りかを確かめてから受け入れる。8つの規則と食い違う出力は受け入れない |
| Q-5 | `Stats::shared` を Verus が受け付けること | T-6、T-13 | T-6 の Verus で分かる | 受け付けなければ、T-13 より前に `shared_objects` を外し、async-threads の項目に回す。T-13 の後には外さない (スナップショットを2回書き直さないため) |
| Q-6 | Miri の新しいテストの費用 | T-1 | T-1 で測る | 予算を超えたら、そのテストの本体を小さくする (`cfg!(miri)` で縮めない) |

## 付録: レビューで変わった主な点

- 待ったタスクが VM の `Task` を持ち続ける案は退けた。継続だけにし、`Async.run` を呼んだ `Task` だけがスタックを持って待つ (D-2)
- ネイティブ化に備え、意味の核を `cells.rs` に分け、VM の型に依らない形にした (利用者の求め。D-3)
- `Task` の結果は thunk で渡す。値で渡すと `Lin` の結果が通らない (D-1)
- `VmFault::Deadlocked` は足さない。この段では待ちの輪を作れない (D-13)
- 止める語に `END` のビットを足す案は退けた。host の読み手の振る舞いが変わる。終わりの欄は `exit` だけを持つ (D-4、D-5)
- `Finished.exit` の出どころは `control.exit_code()` だけにした (D-5)
- ハンドルは同じ実行の中なら `run` を越えて待てる。`StaleTask` は REPL の入力をまたいだときだけである (D-10)
- check-fail のテストは誤りの番号の範囲で `types/` と `linearity/` に置き、`check-fail/async/` は作らない (D-17)
- 手で `cargo run` を流して確かめることはやめ、UI テストの harness だけで確かめる (D-17)
