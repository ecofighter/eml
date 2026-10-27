# async-tasks: 1つの worker の上の `Std.Async` のタスク

- 作成: 2026-10-27。改訂: 2026-10-27 (T-9 で止まり、設計し直した。下の段落)
- 基準: `claude/trusting-hawking-hvvx32` @ `9747404` (T-8 までを入れた状態。T-9a から後の task の file:line の参照はこの状態を指す)。「3. 背景」は計画を作った `main` @ `3bfa56d` の状態を書き、T-1〜T-8 が変えた所はそれぞれの task にある。ただし、extern の規則の項目と試作の項目は `9747404` で確かめ直した
- 行番号の注意: T-9b 以降の挿入で、この計画が引く docs/spec の行番号はずれている。行番号でなく、引いた文と節の名前で探す
- 状態: 承認済み (2026-10-27)。D-4 と D-5 で docs/future/multicore.md の設計を変えること (worker 0 の語に書く側が直接書く、終わりの欄の「正常な終わり」と「致命的な誤り」を async-threads に回す) と、UI テストの速いモードを `normal` で流すことも承認を受けた。設計し直した D-1、D-11、D-12、D-19 と T-9a〜T-9d も、抽象型をこの段に入れる形で承認を受けた (2026-10-27)。未決の項目は「10. 危険と未決の項目」にあり、どれも既定の扱いがある
- 進み具合: T-1〜T-8 は済み。T-1 `47ed9ff`、T-2 `5139c15`、T-3 `605e0eb`、T-4 `06f4481` と `5bfdd0f`、T-5 `910babe` と `68ca75b`、T-6 `8c1353f` と `3c68efe`、T-7 `28abd4f`、T-8 `45fbec5` と `9747404`。実行は T-9a から再開する
- 付録: `docs/plans/2026-10-27-async-tasks/Async.em` (std/Async.em の下書き)、`docs/plans/2026-10-27-async-tasks/ui-tests.md` (UI テストのプログラム)

この改訂で変えたこと: もとの T-9 (`Async` の ABI の表と `std/Async.em`) を計画のとおりに実装すると、2つの所で止まった。1つ目は extern の引数の数である。HIR と Kind のスキームはシグネチャの一番外側の矢印をすべて引数に数え (crates/eml_hir/src/program.rs:408-419)、translate は表の行の `params.len()` を使う (crates/eml_core_ir/src/translate/expr.rs:348-353)。結果を `once Unit -> a` の thunk にした `take_result` は結果が関数型の最初の行で、2つの数が食い違った。2つ目は `Lin` の結果である。extern の引数の型変数は `Unr` に固定されるので、`task_cell`、`take_result`、`run_result` を通る `Async.await` と `Async.run` が `Lin` の結果を断った (`Async.run (fn () -> MutArray.empty ())` が E3001)。利用者は、T-9 で止めて設計を見直すこと、extern の設計を根本から変えてもよいこと、抽象エフェクトを足すこと、抽象型もこの段に入れることを順に選び、抽象型を含む設計し直しを承認した。改訂では、タスクの結果を thunk に包まず値のまま渡し、表に引数ごとの「預かる」印 (`held`) を足し、表の印の列を LIR と同じ3値の `Mark` にした (D-1、D-11、D-12)。修飾子 `pub abstract` で抽象型と抽象エフェクトを入れ (D-19)、`Async` を抽象エフェクトに、`Map`、`Set`、`Iter` を抽象型にする。T-9 を T-9a〜T-9d に分け、T-11、T-12、T-14、T-16 に項目を足した。Q-1 と Q-5 は閉じて消し、Q-7〜Q-10 と R-1、R-2、R-4 を足した

## 1. 目的と成功の条件

- 目的: roadmap の段 `async-tasks` を実装する。VM の1つの worker の上で `Std.Async` のタスクを動かし、監視のスレッドの `PREEMPT` で切り替える普通の方針を `eml run` と `eml repl` の既定にする。std に `Task`、`await`、`try_all`、`fail_io`、`Either`、`yield_now`、暗黙のスコープ、`IOErrorKind` の新しい種類を入れる。スコープは失敗を記録するだけで、兄弟のタスクは最後まで動く
- 成功の条件: 「9. 確かめ方」の全体の確認がすべて通ること。とくに `tests/ui/run/async/` と `tests/ui/run-fail/async/` の UI テストが、検査モードの逐次、速いモードの普通の方針、検査モードの seed 1 と 2026 で同じ出力を返し、seed の run ごとに `preemptions > 0` になること

## 2. 範囲

- 範囲に入るもの: 上の目的の項目。タスクごとの論理的な worker ID、`RunStats` の4つの欄 (`tasks`、`parks`、`dropped_failures`、`shared_objects`)、終わりの欄と `exit`、`Fault::Raised` と `Proc.exited` の書き直し、ネイティブ化に備えた意味の核と実行系の分け方、extern の表の3値の印の列 (`Mark`) と預かる印 (`held`)、修飾子 `pub abstract` (抽象型と抽象エフェクト) と、それを std の `Async`、`Map`、`Set`、`Iter` に当てること
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
- extern の引数の型に現れる型変数は、Kind のスキームで Unr に固定される (docs/spec/declarations.md の「`extern`」:121-124。`Array.make 1 (MutArray.empty ())` は E3001)。固定しないのは、結果だけに現れる型変数と、コールバックの最後の結果にだけ現れる型変数である。extern の引数の中の矢印の row を空に限る決まりは、`crates/eml_hir/tests/externs.rs:109-150` のテストにだけあり、declarations.md には書いていない。extern の引数の数は、HIR と Kind のスキームがシグネチャの一番外側の矢印で数え (crates/eml_hir/src/program.rs:408-419)、translate が表の行の `params.len()` で数える (crates/eml_core_ir/src/translate/expr.rs:348-353)。結果が関数型の行はないので、2つは一致する。表の印の列は `param_kinds: &[Linearity]` と `ret_kind` の2値で、LIR の3値の `Mark { Unr, Lin, Poly }` は `eml_lir` にある
- Prelude の `pub` でない item はほかのモジュールから見えない (crates/eml_hir/src/def_map.rs:664)
- `Session::run` はない。`--sched` は clap (crates/eml_cli/src/main.rs:27-46、既定 `sequential`) から `Sched`、`RunConfig::with_sched`、`eml_vm::execute` へ流れる
- UI テストの harness (crates/eml_cli/tests/ui.rs) は `par/` だけを seed [1, 2026] で流し (`seeded()`、ui.rs:266)、`assert_seeds_agree` は成功した run ごとに `preemptions > 0` を求める。`schedule_independent` は `peak_objects` だけを 0 にする。速いモードの run は `assert_fast_mode_agrees` (ui.rs:250-259) が `Sched::Sequential` で流す
- `RunStats` は13欄で検査モードだけで数え、`crates/eml_vm/tests/bench.rs` の48個の inline スナップショットが固定する
- `eml_vm` はすでに `src/` に 103 か所の `unsafe` を持ち、`forbid(unsafe_code)` はない。`eml_rt` は vstd だけに依存する。Cargo.lock に crossbeam、parking_lot、signal-hook はない
- 試作で確かめたこと (計画の作成のときに scratch の写しで行った)
  - `ctl` の節が `k` を extern に渡して戻り、後で `handle` の外から `k` を適用しても再開でき、続く操作でまた待てる。再開した κ の中の `try_io` と `try_panic` も IO の失敗と Panic を捕まえる
  - `start_task` と `resume_task` の LIR は `fn start_task(f.0: tobj poly) -> unit`、`fn resume_task(k.0: tobj lin, w.1: enum) -> unit` で、隠れた evidence の引数はない。節は `k` を `tobj lin` の所有した語で渡す
  - `Control` に `worker: AtomicU32`、`end: AtomicU32`、`code: AtomicI64` を `verus!` の中で足しても、`cargo verus verify -p eml_rt` は `1095 verified, 0 errors` で、verusfmt も通る
- 設計し直すときに `9747404` の写しで確かめたこと (値のまま渡す形。付録の `Async.em` と D-1 の表の行)
  - `Async.start (fn () -> R)`、`Async.await t`、`Async.run (fn () -> R)` を持つプログラムを、`R` が `MutArray.empty ()` と `1` の2通りで `eml_test_support::lir` に通すと、translate、optimize、evidence、`to_lir`、LIR のすべてのパスと verifier を通る。`MutArray Int` の instance は `t.6: obj lin = extern Std.Async.take_result(t.3)` と束縛し、`Int` の instance は `tobj` で束縛する。`finish_ok(x)` は `x` を消費し、`task_cell` は `t.3` を借り、`take_result` が `t.3` を消費する
  - `task_cell` の行を借りない (`borrowed: [false]`) と、Perceus の verifier が `Lin` の instance を `t.3 (lin) is duplicated` で断る。`with_arg` の行は引数を後の `con` に貸すだけだからである (docs/spec/core-ir.md の「組を返す extern」)
  - 値のまま渡す行で、`Lin` の結果の `Async.run` と `Async.await` は型検査を通る (`try_io` の中の `await` で受けた `FileIn` も)。型の付け替え (`let t : Async.Task Int = Async.start (fn () -> "s")`) は E2001、`pub` でない `start_raw` と `take_result` を外から呼ぶと E1001、`Lin` の結果を2回待つと E3002 である
  - `take_result : Task a -> <IO, Panic> a` は `a` が2回現れるので、戻らない呼び出しに分類されない (crates/eml_types/src/diverge.rs:34-45)。`await` の前に `Lin` の値を使い残すと E3003 が出る
  - `transfer_back` と `decref` は即値の語も受け、`decref` はどの種類の物体も放す (`Foreign` の破棄処理を含む。crates/eml_rt/src/heap/mod.rs:2470-2477 の契約)
  - 抽象エフェクトがない今は、利用者の handler が `Async.await_cell` を `Woke` で再開するプログラムが型検査を通る。終わっていないタスクの結果を取りに行く道で、抽象エフェクト (D-19) がこれを名前解決で断る
  - 抽象型の試作 (`pub abstract data` と、定義の可視性の分岐): 外のモジュールでのコンストラクタの作る式とパターン、import の並びの `T(..)`、`Map.Empty` を断り、API だけを使う `tests/ui/run/std/std_map.em` は通る。`std/Map.em` を抽象にしたまま、`eml_test_support::with_std` で `Map.em` の後ろに `pub valid` を足して `std_map_balance.em` の本体を流すと、今のスナップショットと同じ stdout になる (このマシンで 1.2 s)

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

- 決め: `Async` は普通の eml のエフェクトで、この段の生の操作は `await_cell : Cell -> Wake` と `reschedule : Unit -> Wake` の2つ (どちらも once)。`Wake = | Woke | Cancelled` (タグ 0、1) で、`Cancelled` はこの段では届けないが ABI として先に置く。`Cell` は登録表の添字 `(generation << 32) | index` の即値 (`Unr`、`Repr::Int`)、`Task a` は `TaskHandle` を中身に持つ `Lin` の `Foreign`。ランタイムが名前で呼ぶ入口は単相の `Std.Async.start_task : (Unit -> <IO, Async> Unit) -> <IO> Unit` と `Std.Async.resume_task : (once Wake -> <IO> Unit) -> Wake -> <IO> Unit` である。Panic はタスクの底 `bottom` で捕まえるので、`k` の row は `<IO>` になる。タスクの結果は値のまま渡す: `finish_ok : a -> <IO> Unit` が預け、`take_result : Task a -> <IO, Panic> a` と `run_result : Task a -> <IO, Panic> a` が1回返す。ランタイムは預かった語を写さず中身を見ず、セルに持って1回返すか、セルを放すときに `decref` で落とす。`take_result` と `run_result` が返す値の型が `finish_ok` に預けた値の型と合うことは、処理系は確かめない。生の行を `pub` にせず、`start`、`run`、`await` のシグネチャで `a` を結ぶ std が保ち、UI テストで確かめる。`Async` は `pub abstract effect` で、操作 `await_cell` と `reschedule` は `Std.Async` の外から見えない (perform も handler の節も E1058。D-19)。そのため、終わっていないタスクの結果を取りに来る道はなく、`take` のその枝は `Internal` のままでよい。std の全文は付録の `Async.em` にある
- 理由: translate は入口から届く instance と型引数ごとの instance だけを作るので、単相の入口なら名前がそのまま instance の名前になり、translate の変更は根を足すことだけで済む。extern の引数の型変数は `Unr` に固定されるが、表の「預かる」印 (`held`) を付けた引数は固定しない (docs/spec/declarations.md の「`extern`」、T-9b)。そのため `Lin` の結果も thunk に包まずに渡せ、結果ごとの閉包の確保と `await` の apply がない。預かる引数の矢印には閉じた row を書けるので、`start_raw`、`park`、`ready` の閉包と継続の row も同じ印で説明がつく。預かる印は借用の印と独立で、`task_cell` はハンドルを借りて預かる (`with_arg` の行は引数 k を貸すだけで、消費するのは後の `con` である)
- 退けた案: 多相の `start_task@[_:poly]` を根にする (translate に Flexible のキーの根が要る)。`finish_ok : a -> …` を印なしで使う (Lin の結果を断る)。結果を `once Unit -> a` の thunk で渡す (結果ごとに閉包を1つ確保し、extern の引数の数の規則と型の印の規則が別に要る)。`Task a` を単相の `TaskHandle` を包む `pub data` にする (コンストラクタが公開されるので型を付け替えられる)。D-19 の抽象型で包んでも同じで、`finish_ok` には預かる印が要り、`TaskHandle -> a` は戻らない呼び出しに分類され、ハンドルごとに箱が1つ増える。`extern data` の `Task a` もコンストラクタを持たないので、隠して得るものはない。利用者の handler が待たずに再開したときを実行時エラーにする (抽象エフェクトなら名前解決で断れ、後の段の `await_io` の `Lin` の token も守れる)。スケジューラのループを eml で書く (安全点で横取りできない)
- 効く範囲: async-cancel、async-time、async-chan (`send_raw` の値を預かって消費し、`Chan a` を預かって借り、受け取りの token `ChanWait a` を預かって消費する。`Chan a` の Kind を引数によらず `Unr` にする `TypeRow` の形はその段で足す)、io-driver (`submit_*`/`take_*` は型変数を持たないが、buffer とハンドルを持つ行は預かる印を付ける)、async-threads、jit-effects、native (預かる印は語の ABI に現れない)。利用者の `extern "C"` の行 (docs/future/ffi.md) は表の印を書けないので、型変数は `Unr` に固定されたままである。ffi.md が `borrowed` の構文を足すときに `held` も足す
- 見直す条件: 継続の型が違う生の操作を足すとき。そのときは継続の型ごとに単相の入口を1つ足す (io-driver の `resume_task_io : (once (Wake, OpId) -> <IO> Unit) -> Wake -> OpId -> <IO> Unit`)
- リポジトリに残す所: docs/spec/core-ir.md の「表の印の列」と docs/spec/lir.md の「Kind の印」(T-9a)、docs/spec/declarations.md の「`extern`」、docs/spec/stdlib.md の「extern の境界」と「`Std.Async`」(T-9b)、docs/spec/effects.md の「抽象エフェクト」と docs/spec/modules.md の「公開の範囲」(T-9c)、`Async` を抽象エフェクトにすること (T-9d)、docs/spec/core-ir.md の「変換の規則」と「最適化」(T-10)。docs/future/multicore.md:106-108 と docs/future/async.md の「`Async` のエフェクト」の3つ目の項目を単相の入口と値のままの結果に書き直す (T-9b)

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
| `take_result t`、`run_result t` | `take(ctx, handle, Take::Task \| Take::Run) -> Result<Word, Raise>` | `Ok` は値の語を `dst` へ。`Raise` は `failed(Fault::Raised{..})` か `Extern::Panic` の道 | 包みが `ctx.fault` に同じ `Fault` を書き、冷たいブロックが届ける |
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
- リポジトリに残す所: docs/spec/stdlib.md、docs/spec/declarations.md の「Prelude」、docs/spec/modules.md の「Prelude」(T-7、T-8、T-9b)

#### D-19: 抽象型と抽象エフェクトの `abstract`

- 決め: `pub` の後に書く修飾子 `abstract` を1つ入れる。`pub abstract data T …` は型の名前を公開し、コンストラクタと名前付きのフィールドを宣言したモジュールの外から隠す (作る式、パターン、import の並びの `T(..)`、射影、更新がすべて誤り)。`pub abstract effect E where …` はエフェクトの名前を公開し、操作を宣言したモジュールの外から隠す (perform、handler の節、import の並びの `E(..)` が誤り)。隠した部品を名指した位置は E1058、隠したフィールドの射影と更新は E2021 である。`abstract` は `data` と `effect` にだけ付き、`pub` のない宣言、`extern` の宣言 (extern の型はコンストラクタを持たず、extern のエフェクトはいつも隠す)、`type`、シグネチャ、fixity、`class` に付けたら E0011 である。抽象型の Kind、線形性、`deriving`、網羅性の規則は普通の `data` と同じで、違いは名前解決とフィールドの解決だけである。`deriving` した `Eq`、`Ord`、`Show` は構造をそのまま表すので、抽象型の中身を比べ方と表示で見せる。隠すかどうかは宣言する側が `deriving` を書くかで選ぶ。REPL のセッションの入力はどれも同じモジュールとみなし (`same_scope`)、セッションで宣言した抽象型と抽象エフェクトはセッションの中では隠さない。import したファイルのモジュールの抽象型と抽象エフェクトは、セッションでも隠す
- 理由: extern のエフェクトの操作を隠す規則 (modules.md の「標準ライブラリ」) がすでにあり、それを宣言で選べる形に広げるだけで両方が書ける。1つの修飾子にするのは、型とエフェクトで「名前は見えて中身は見えない」が同じ意味だからである。`Std.Map`、`Std.Set`、`Std.Iter` のコンストラクタと `Std.Async` の操作を、約束でなく検査で隠せる。`deriving` を禁じないのは、Haskell と同じく、構造の比べ方と表示を見せるかは型の作者が決めることだからである
- 退けた案: Haskell の export list (`T` と `T(..)`)。eml は import の並びしか持たず、宣言の側に書くほうが eml の `pub` の形に合う。コンストラクタごとの `pub` (隠す理由は型ごとに1つである)。フィールドだけ読める抽象型 (読める射影が要るのは `Cmd` だけで、`Cmd` は別の理由でこの段では抽象にしない)。抽象エフェクトの操作を extern のエフェクトと同じ E1001 にする (「定義がない」は std の `pub` でない item の約束で、利用者も書ける `abstract` には理由を言う診断が要る)。`deriving` の禁止 (`Map`、`Set`、`Iter` は何も導出しないので std には効かず、利用者の選択を奪うだけである)
- 効く範囲: この段の T-9c、T-9d。後の段の `Chan a` (抽象エフェクトの操作 `await_chan`)、`Cmd` の抽象化、`Ref` と `Heap`
- 見直す条件: 読めるが書けないフィールド (`pub` のフィールド) が要る型が出たとき
- リポジトリに残す所: docs/spec/declarations.md 「`pub`」、docs/spec/modules.md 「公開の範囲」と「REPL のセッション」、docs/spec/effects.md 「抽象エフェクト」、docs/spec/grammar.md、docs/spec/lexical.md、docs/spec/diagnostics.md (T-9c)

### 5.2 段の中の設計

理由があれば実装の中で直してよい。直したら、そのコミットのメッセージに書く。

#### D-10: 登録表は VM の実行ごとに1つ

`Registry` とその `Arc<Inbox>` は `vm::run` ごとに作る (`Scheduler` と同じ)。そのため、1つ目の `Async.run` が返したハンドルは、同じ実行の後の `Async.run` で待てる (docs/future/async.md:88)。ハンドルの `Arc<Inbox>` と登録表の `Arc<Inbox>` を `Arc::ptr_eq` で比べ、違えば `VmFault::StaleTask` (表示 `a task handle outlived its run`、壊れた状態にしない) にする。これが起きるのは REPL の入力をまたいだときだけである。古いハンドルの `Drop` は自分の死んだ受け箱に積むだけで、生きている受け箱には届かない。同じ実行の中の添字の使い回しは generation で見つける。

#### D-11: セルの形

```rust
// crates/eml_vm/src/cells.rs。import してよいのは std と eml_rt だけ
pub(crate) struct CellRef { index: u32, generation: u32 }
pub(crate) enum Waiter { Cont { task: CellRef, k: Word }, Token(WaitToken) }
pub(crate) enum Outcome { Value(Word), Io { kind: i64, message: String }, Panic(String) }   // Value は finish_ok が預けた語そのもの (即値でもよい)
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
- `Outcome::Value` の語は、即値のこともある (`()`、引数のないコンストラクタ、捕まえる値のない関数は即値の `tobj` で、`Int`、`Float`、`Enum`、`Struct` の instance は LIR の boxing が箱に入れる)。ランタイムは語を写さず中身を見ない
- タスクのセルを放すとき: `outcome` を `cell.worker` の下で放し (`Value(w)` は `with_worker(ctx, cell.worker, |ctx| ctx.decref(w))` で落とす。`decref` は即値も、破棄処理を持つ `Foreign` を含むどの物体も受ける)、`retire_worker(cell.worker)` を呼び (`TransferLeak` はヒープの誤りとして出る。`par` と同じ)、generation を1つ進めて添字を `free` に戻す
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
- `take(ctx, handle, kind)`: `Arc::ptr_eq` を確かめる。セルは `Finished` でなければならない (違えば `Internal("awaiting an unfinished task")`。`Async` が抽象エフェクトなので、検査を通ったプログラムからは届かない)。`Take::Run` はスコープの `first_failure` を本体の値より先にし、根のスコープを放す。値なら `transfer_back(value, from = cell.worker)` して `Ok(value)`、失敗なら `Raise`。`taken = true` にしてハンドルを手放し、セルを放す
- 受け箱の処理 `drain(ctx)`: lock の下で `Vec` を取り、lock を外して処理する。`HandleDropped(c)` は、セルが `Finished` ならその場で放し、そうでなければ `handle_dropped = true` にする。`main` の `Finish` では、受け箱の処理、`sweep_all` (残ったセルをすべて放し、その間に積まれた知らせは後で受け箱ごと捨てる)、`has_children()`、`run_entry` の `check_finished`、`check_heap`、漏れの検査の順に行う
- 始めるときの `transfer_back(closure, from = spawner)` と、待つときの `transfer_back(value, from = cell.worker)` と、放すときの `retire_worker` は、`par` の fork と join と同じ形をタスクごとに当てたものである

- `finish` を呼ぶ時点 (T-11 の実装で決めた): `finish_ok`、`finish_io`、`finish_panic` の命令は結果を実行系のタスクの欄に置くだけにし、核の `finish` は入口の `Task` の `Op::Finish` (タスクの底を巻き戻し終えた後) で呼ぶ。命令の中で呼ぶと、待つ `Async.run` がタスクの巻き戻しの途中で起き、seed の方針の下で生きた物体を持つ worker の ID を退かせて `TransferLeak` になるためである。ネイティブでも、入口の stub に戻ってから `finish` を呼ぶ

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
    return Err(VmFault::Extern(Fault::Raised(Box::new(Raised { kind: task.raw(arg(0)), message: message.into_boxed_str() }))));
}
```

- `Fault::Raised(Box<Raised>)` で、`Raised { kind: i64, message: Box<str> }` である (欄を直に持つと `Fault` が 32 バイトになるので、`Exited(Box<Exited>)` と同じく箱に入れる)。`io_error_kind() == Some(kind)`、`panics() == false`、表示は `message` である。`Fault` の大きさは 24 バイトのまま (`faults_stay_small` が確かめる)
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

| ID | task | もとの決定 | 先に要るもの | 並行 | 状態 |
|---|---|---|---|---|---|
| T-1 | `eml_rt`: worker の止める語、`PREEMPT`、終わりの欄 | D-4、D-5 | - | T-6、T-8 と並行可 | 済み (`47ed9ff`) |
| T-2 | VM: 終わりの欄 | D-5 | T-1 | 不可 | 済み (`5139c15`) |
| T-3 | VM: `Preempt` の扱い | D-16 | T-1 | T-2 と並行可 | 済み (`605e0eb`) |
| T-4 | VM: `Normal` の方針と監視のスレッド | D-6、D-16 | T-3 | 不可 | 済み (`06f4481`、`5bfdd0f`) |
| T-5 | CLI の既定と UI の速いモード | D-6 | T-4 | 不可 | 済み (`910babe`、`68ca75b`) |
| T-6 | `eml_rt`: `Fault::Raised` と `Stats::shared` | D-14、D-15 | - | 可 | 済み (`8c1353f`、`3c68efe`) |
| T-7 | `fail_io`、新しい種類、`Proc.exited` | D-7、D-15 | T-6 | 可 | 済み (`28abd4f`) |
| T-8 | `Either` | D-7 | - | 可 | 済み (`45fbec5`、`9747404`) |
| T-9a | 印の列を `Mark` にする | D-1 | - | 不可 | 済み (`43fc958`) |
| T-9b | extern の預かる引数、`Async` の ABI の表、`std/Async.em` | D-1、D-7 | T-9a | 不可 | 済み (`b08e47b`、`6dda225`) |
| T-9c | `abstract` (言語) | D-19 | T-9b | T-10、T-11 と並行可 | 済み (`b7d5fe5`、`2da0643`) |
| T-9d | std の抽象エフェクトと抽象型 | D-19、D-1 | T-9c | T-10、T-11 と並行可 | 済み (`3f3834e`、`88c6bc7`) |
| T-10 | ランタイムの入口を根にする | D-1 | T-9b | T-9c、T-9d と並行可 | 済み (`67002b9`) |
| T-11 | VM のタスク (1): 構造、入口、始めと終わり、`Async.run` | D-2、D-3、D-10〜D-13 | T-4、T-6、T-10 | T-9c、T-9d と並行可 | |
| T-12 | VM のタスク (2): 待ち、`await`、失敗、ハンドル | D-10〜D-13 | T-11、T-9d | 不可 | |
| T-13 | `RunStats` の4つの欄 | D-14 | T-12 | 不可 | |
| T-14 | harness と `async/` の UI テスト | D-17、D-18 | T-13、T-5、T-7、T-9d | 不可 | |
| T-15 | タスクの中の `exit` | D-5、D-13 | T-14、T-2 | 不可 | |
| T-16 | 段を閉じる | 全体 | T-15 | 不可 | |

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
- 作業: `Fault::Raised(Box<Raised>)` (`Raised { kind: i64, message: Box<str> }`) を足す (`io_error_kind() == Some(kind)`、`panics() == false`、表示は `message`)。`share` が rc を負にする所で `Stats::shared` を1つ増やす (検査モードだけ。`Stats` の他の欄と同じく証明する不変条件の外。heap/mod.rs:100-101)
- テスト: `Raised` の表示と分類の単体テスト、`share` が数える単体テスト
- 文書: docs/spec/object-model.md の `Fault` の表に `Raised` の行、「仕事の回数」に `shared`
- 実装者に任せること: なし
- 条件: いつも (Verus は `Stats::shared` を受け付けた)
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

### T-9a: 印の列を `Mark` にする

- 目的: extern の行の印の列を LIR の印と同じ3値にし、`Std.Par.par` の行を正直にする。預かる印はまだ足さない
- 対象: `crates/eml_extern/src/lib.rs`、`crates/eml_lir/src/lib.rs`、`crates/eml_lir/src/check.rs`、`crates/eml_core_ir/tests/externs.rs`、docs/spec/core-ir.md の「表の印の列」、docs/spec/lir.md の「Kind の印」
- 作業 (`9747404` のきれいな作業ツリーから始める):
  - `Mark { Unr, Lin, Poly }` を `eml_lir/src/lib.rs` から `eml_extern/src/lib.rs` へ移す (`name`、`join`、`passes_to`、`meet` (pub にする)、`at_least`、`rank`、doc comment ごと)。`eml_lir` は `pub use eml_extern::{…, Mark}` で再公開し、`impl From<Linearity> for Mark` とその doc 「extern は多相にならないので…」を消す
  - `FunctionRow` の `param_kinds: &[Linearity]` を `param_marks: &'static [Mark]`、`ret_kind` を `ret_mark: Mark` にする。doc: 「引数ごとの印。長さは `params` と同じである。LIR の印と同じ3値で、型変数の位置のうちスキームが `Unr` に固定しない位置は `Poly` である (docs/spec/core-ir.md の「表の印の列」)」。表の `use` を `Mark::{Lin, Poly, Unr}` にし、`kinded`/`row`/`writing` と `Exit`/`FailIo`/`Panic` の literal の型を直す。`removable()` は `mark == Mark::Unr`。`Par` の行を `&[Poly, Poly]`、`Poly` にする
  - check.rs:1089 `let expected = kind;`、:1114 `row.ret_mark`
  - core_ir の tie テスト: `every_extern_function_row_has_the_kinds_of_its_std_signature` を `every_extern_function_row_has_the_marks_of_its_std_signature` に改め、`linearity_or_poly` の `Unr/Lin/Poly` を `Mark` に写して直接比べる (`Var` は panic のまま)。`kind_of` とその doc を消す。`only_the_handle_and_mutable_array_rows_carry_a_lin` を `only_the_listed_rows_carry_a_lin_or_poly_mark` に改め、`"Std.Par.par" => (&[Poly, Poly], Poly)` を足す
  - docs/spec/core-ir.md の「表の印の列」の最初の項目を次に置き換え、:199 の「`Std.Par.par` の結果の組の型変数は固定されないが…`Poly` を通して `Unr` と比べる」の文を「固定しない位置は `poly` の行と比べる」に直し、表に `Std.Par.par | poly、poly | poly` の行を足す。docs/spec/lir.md の「Kind の印」に「extern の行の印の列も同じ3値である ([Core IR](core-ir.md) の「表の印の列」)」を足す

````
- extern の行 (`eml_extern::FunctionRow`) は、`params` と `ret` と並べて `param_marks: &'static [Mark]` と `ret_mark: Mark` を持つ。印は LIR の印と同じ3値 (`unr`、`lin`、`poly`) で、`Mark` は `eml_extern` が持ち `eml_lir` が再公開する。extern の関数の引数に現れる型変数はスキームで `Unr` に固定される ([宣言](declarations.md) の「`extern`」) ので、型変数の位置 (`Std.Array.get` の要素) と、Kind が要素に従う `Array a` は `unr` である。固定しない位置 (コールバックの最後の結果、結果だけの型変数、預かる引数) は `poly` で、verifier はどの印の値も通す。`exit` の結果の印は、行の `ret_mark` でなく、translate が具体化した結果の型から決める ([LIR](lir.md) の「戻らない extern」)
````

- 実装者に任せること: `Mark` の impl を1つのブロックに置くか、`meet` だけ `eml_lir` の extension に残すか (既定: 1つのブロック、`meet` は pub)
- 条件: いつも
- 完了の条件:
  - `cargo nextest run` → 通る。UI と IR のスナップショットはバイト単位で変わらない
  - ``rg -n 'param_kinds|ret_kind|impl From<Linearity> for Mark|kind_of\(|多相にならない|Poly` を通して' crates docs/spec`` → 何も出ない
  - 共通の確認
- テストの変更: 種類 2 (一つずつ): `only_the_handle_and_mutable_array_rows_carry_a_lin` (改名と `Std.Par.par` の期待)、`every_extern_function_row_has_the_kinds_of_its_std_signature` (改名と `Mark` の直接比較)。種類 3: 再公開と欄の改名に伴う参照

### T-9b: extern の預かる引数、`Async` の ABI の表、`std/Async.em`

- 目的: 預かる印 `held` をその最初の利用者 (`Std.Async` の8行) と一緒に入れ、`std/Async.em` と `Std.Async` の ABI の文書を書く。D-7 の `Failure`、`try_all` と、最小の `pub` の面 (`run`、`start`、`spawn`、`await`、`yield_now`) もここで書く
- 対象: `crates/eml_extern/src/lib.rs`、`crates/eml_types/src/check/mod.rs`、新しい `std/Async.em`、`crates/eml_hir/src/lib.rs` (`STD`)、`crates/eml_lir/src/lib.rs` (タグ)、`crates/eml_vm` (表の行の腕)、tie のテスト (`crates/eml_hir/tests/externs.rs`、`crates/eml_core_ir/tests/externs.rs`)、新しい `crates/eml_core_ir/tests/async.rs` (`tests/main.rs` に `mod r#async;` か `mod async_rows;`)、docs/spec/declarations.md、docs/spec/stdlib.md、docs/spec/modules.md、docs/implementation/status.md、docs/future/multicore.md、docs/future/async.md
- 作業:
  - `eml_extern`: `FunctionRow.held: &'static [bool]` (doc: 「引数ごとの預かる印。長さは `params` と同じで、借用の印とは独立である。印の立つ引数の型引数の値を、処理系は写さず中身を見ない。消費する引数なら持って後で高々1回返すか落とし、借りる引数なら読むだけで持たない。その型に現れる型変数は `Unr` に固定せず、矢印の row も問わない (docs/spec/declarations.md の「`extern`」)」)。`kinded`/`row` は `held: &[false; N]`。`Exit`/`FailIo`/`Panic` の literal に `held` を足す。`const fn holding<const N: usize>(mask: &'static [bool; N], row: FunctionRow) -> FunctionRow` (長さの `assert!`) を `borrowing` の隣に置く
  - `eml_extern` に `pub enum RuntimeEntry { StartTask, ResumeTask }`、`ALL`、`name()` (`Std.Async.start_task`、`Std.Async.resume_task`)、`arity()` (1、2)、`params()` (`[TObj]`、`[TObj, Enum]`)、`Extern::is_async()`
  - `eml_types` の check/mod.rs: `param_type_vars(signature, arity, held)`。`held` は `FunctionKind::Extern(Some(e)) => e.row().held`、ほかは `&[]`。預かる引数は飛ばす (型変数を集めない)。`#[cfg(test)]` の単体テスト `a_held_parameter_leaves_its_type_variables_free` (`Std.MutArray.read` のシグネチャに `&[true, false]` で空、`&[false, false]` で `a` 1つ)
  - 表の行を `Par` の後に足す (Repr は `kinded` の列、印は `Mark`。`task_cell` の `ret` は組の1つ目を書く)

| 行 | 名前 | params | param marks | held | borrowed | ret と mark | purity | with_arg |
|---|---|---|---|---|---|---|---|---|
| `AsyncStartRaw` | `Std.Async.start_raw` | `[TObj]` | `[Poly]` | `[true]` | `[false]` | `Obj`、Lin | Effectful | |
| `AsyncRunRaw` | `Std.Async.run_raw` | `[TObj]` | `[Poly]` | `[true]` | `[false]` | `Obj`、Lin | Effectful | |
| `AsyncPark` | `Std.Async.park` | `[Int, TObj]` | `[Unr, Lin]` | `[false, true]` | `[false, false]` | `Unit`、Unr | Effectful | |
| `AsyncReady` | `Std.Async.ready` | `[TObj]` | `[Lin]` | `[true]` | `[false]` | `Unit`、Unr | Effectful | |
| `AsyncFinishOk` | `Std.Async.finish_ok` | `[TObj]` | `[Poly]` | `[true]` | `[false]` | `Unit`、Unr | Effectful | |
| `AsyncFinishIo` | `Std.Async.finish_io` | `[Enum, Obj]` | `[Unr, Unr]` | `[false, false]` | `[false, false]` | `Unit`、Unr | Effectful | |
| `AsyncFinishPanic` | `Std.Async.finish_panic` | `[Obj]` | `[Unr]` | `[false]` | `[false]` | `Unit`、Unr | Effectful | |
| `AsyncTaskCell` | `Std.Async.task_cell` | `[Obj]` | `[Lin]` | `[true]` | `[true]` | `Int`、Unr (組の1つ目) | Pure | `Some(0)` |
| `AsyncTakeResult` | `Std.Async.take_result` | `[Obj]` | `[Lin]` | `[true]` | `[false]` | `TObj`、Poly | Effectful | |
| `AsyncRunResult` | `Std.Async.run_result` | `[Obj]` | `[Lin]` | `[true]` | `[false]` | `TObj`、Poly | Effectful | |

  - `ExternType` に `Cell => type_row("Std.Async.Cell", Unr, Repr::Int)` と `Task => generic_type_row("Std.Async.Task", Lin)`
  - `std/Async.em` を付録の `Async.em` で作る。ただし `abstract` はまだ書けないので、エフェクトは `pub effect Async` とし、そのコメントは「タスクの生の操作。どちらも once で、ランタイムとの ABI である」にする (T-9d が `abstract` と付録のコメントを入れる)
  - `eml_lir` に `WAKE_WOKE = 0`、`WAKE_CANCELLED = 1`
  - VM の `run_extern` の `Std.Async.*` の腕は `Internal("Async externs run through Op::Async")` を返す (T-11 までは `Async` のプログラムを動かすとこの誤りになる)
  - `eml_hir::STD`、docs/spec/modules.md:69 の std の一覧、docs/implementation/status.md:28 の一覧に `Async.em` を足す
  - hir の tie テスト: `assert_eq!(row.held.len(), row.params.len())`。預かる引数の型の中の矢印 (`held_arrows`) には閉じた row を許す。`signature.arity() == row.params.len()` と `program.arity(id) == Some(row.params.len())` はそのまま
  - core_ir の tie テスト: `only_the_listed_rows_carry_a_lin_or_poly_mark` に上の10行。`only_the_listed_rows_borrow_or_return_a_pair` に `"Std.Async.task_cell" => (&[true], Some(0))`。新しく `only_the_async_rows_hold_an_argument`: `held` に `true` を持つ行の集合が上の8行 (`finish_io`、`finish_panic` は含まない) と等しく、`held[i]` の行の `param_marks[i]` が `Unr` でない。型の行の Repr を確かめる std の写しに `probe_cell : Async.Cell -> Async.Cell` と `probe_task : Async.Task Int -> Async.Task Int` を足し、`probe` の表に `ExternType::Cell => "probe_cell"` と `ExternType::Task => "probe_task"` を足す
  - 新しい `crates/eml_core_ir/tests/async.rs`: `held_rows_lower_for_a_lin_instance` と `held_rows_lower_for_an_int_instance`。本体が `Async.start (fn () -> R)`、`Async.await t`、`Async.run (fn () -> R)` を含むプログラム (`R` = `MutArray.empty ()` と `1`) を `eml_test_support::lir` に通し (verifier が走る)、`eml_lir::pretty` の出力が `extern Std.Async.finish_ok(` と `extern Std.Async.task_cell(` と `extern Std.Async.take_result(` を含むこと、Lin の instance では `take_result` の束縛が `: obj lin = extern Std.Async.take_result(` で、Int の instance では `: tobj = extern Std.Async.take_result(` であることを `assert!(contains)` で確かめる (スナップショットにしない。500行を超える)
  - docs/spec/declarations.md の「`extern`」の :121 の後、:122-124 を次に置き換える (:122 の本文と最初の2つの下位項目は現行のまま)

````
- extern の関数の引数の数は、シグネチャの一番外側の `->` の数である。処理系の表の行は同じ数の引数を持ち、食い違いはテストで防ぐ。結果の型を関数型にはしない (矢印がそのまま引数に数えられる)
- extern の関数の引数の型に現れる型変数は、Kind のスキームで `Unr` に固定する。本体がないので、extern が引数をどう使うかを Kind に推論できないためである (操作の引数と同じ考えである)。そのため、`Lin` の要素の `Array` にはどの関数も使えない。結果だけに現れる型変数 (`exit` と `panic` の結果、`MutArray.empty` の要素) は固定しない。関数型の引数の矢印の Kind も型変数でないので固定せず、`Std.Par.par` の子の閉包は `Lin` の値を捕まえてよい。
  - 関数型の引数 (コールバック) の最後の結果にだけ現れる型変数も固定しない。(現行のまま)
  - コールバックの引数の型変数と、矢印の row の型引数は固定したままである。(現行のまま)
  - 処理系の表で「預かる」印を付けた引数の型変数も固定しない。預かる引数の型引数の値を、処理系は写さず、中身も見ない。引数を消費する行は、その値を持ち、後でほかの行の結果として高々1回 eml のコードへ返すか、使わずに落とす。引数を借りる行は読むだけで、持たない。返した値は新しい束縛になり、その型の規則で1回使われる。落とすときは、コールバックの結果と同じ後始末になる。`Std.Async.finish_ok : a -> <IO> Unit` はタスクの結果の値を預かって持ち、`take_result : Task a -> <IO, Panic> a` はハンドルを預かって消費し、同じセルに持っていた値を返す。`task_cell : Task a -> (Cell, Task a)` はハンドルを預かって借りる。預かった値を返す行の結果の型が、預けた値の型と合うことは、処理系は確かめない。預ける行と返す行を `pub` にせず、包む関数のシグネチャで型変数を結ぶ標準ライブラリが保つ。どの行がどの引数を預かるかは [標準ライブラリ](stdlib.md) の「extern の境界」にある
  - 預かる引数の型の中の矢印には、閉じた row を書いてよい。処理系は預かった閉包や継続を呼ばず、名前で呼ぶ標準ライブラリの関数が後で呼ぶので、row はその関数の型が決める。預からない引数の矢印の row は空か、`Std.Par.par` のように結果の矢印と共有する row 変数だけである。どちらもテストで防ぐ
````

  - docs/spec/stdlib.md の「extern の境界」に段落を足す: 「extern の行は引数を預かることがある (`FunctionRow::held`。[宣言](declarations.md) の「`extern`」)。預かる行は `Std.Async` の `start_raw`、`run_raw`、`park`、`ready`、`finish_ok`、`task_cell`、`take_result`、`run_result` の8行で、`task_cell` だけは借りて預かる。利用者の extern (将来の `extern "C"`) は表の印を書けないので、型変数は `Unr` に固定されたままである」
  - docs/spec/stdlib.md の新しい節「`Std.Async`」の ABI の部分 (エフェクト、`Wake`、`Cell`、`Task`、名前で引く入口、10行の extern と預かる引数、結果が値のまま渡り型の対応を std が保つこと) と `Failure`、`try_all`。docs/future/multicore.md:106-108 と docs/future/async.md の「`Async` のエフェクト」の3つ目の項目 (D-1)
- 実装者に任せること: `tests/async.rs` のモジュール名 (`async` は予約語)。止めた U9 の作業から始めてもよい (prototype at `/tmp/claude-0/-home-user-eml/4dfe1f80-72d6-5946-9518-fce039bdc290/scratchpad/exec/U9-wip.diff` と `.../exec/U9-wip-Async.em`)。そのときは表の10行を上の表に書き直し (`holding(...)`、`Mark` の名前、`task_cell` は `borrowed: &[true]`)、hir の tie テストの `STORED_CALLBACK_ROWS` と `outer_arrows` の部分を `held_arrows` に替え、`Async.em` を付録に合わせる。3つのファイルを解いた版と新しいテストは prototype at `/tmp/claude-0/-home-user-eml/4dfe1f80-72d6-5946-9518-fce039bdc290/scratchpad/exec/redesign/eml` にある (`crates/eml_core_ir/tests/async_probe.rs` が `tests/async.rs` の元)。どちらもセッションの一時ディレクトリなので、なければこの計画と付録から作る
- 条件: いつも
- 完了の条件:
  - `cargo nextest run -p eml_extern -p eml_hir -p eml_core_ir -p eml_types` → 通る (新しい `a_held_parameter_leaves_its_type_variables_free`、`only_the_async_rows_hold_an_argument`、`held_rows_lower_for_a_lin_instance`、`held_rows_lower_for_an_int_instance`、`every_runtime_entry_is_a_monomorphic_std_function`、`Wake` のタグの試験を含む)
  - 次のプログラムを作業ツリーの外の一時ディレクトリ `$SCRATCH` に置き、それぞれ `timeout 20 cargo run -p eml_cli -- check $SCRATCH/<file>` を流す
    - `x.em`、`int.em`、`use.em`、`lin.em`、`lin2.em`、`lin_io.em`、`handled_await.em` → 何も出さず終了コード 0 (`handled_await.em` は T-9d で E1058 が2つになる)
    - `retype.em` → E2001 だけ。`raw.em` → E1001 が2つ。`twice.em` → E3002。`unused.em` → E3003 (どれも終了コード 1)

```haskell
-- x.em
import Async
main : Unit -> <IO> Unit
main () = ()

-- int.em
import Async
main : Unit -> <IO, Panic> Unit
main () = println (show (Async.run (fn () -> 41 + 1)))

-- use.em
import Async
import MutArray
child : Unit -> <IO, Panic, Async.Async> Int
child () =
  println "child"
  Async.yield_now ()
  41 + 1
lin_child : Unit -> <IO, Panic, Async.Async> MutArray Int
lin_child () = MutArray.empty ()
body : Unit -> <IO, Panic, Async.Async> Int
body () =
  let t = Async.start child
  Async.spawn (fn () -> println "spawned")
  let a = Async.await (Async.start lin_child)
  let (n, a) = MutArray.length a
  drop a
  Async.await t + n
main : Unit -> <IO, Panic> Unit
main () =
  let n = Async.run body
  println (show n)
  match Async.try_all (fn () -> Async.run body) with
    | Ok m -> println (show m)
    | Err f -> println (show f)

-- lin.em
import Async
import MutArray
main : Unit -> <IO, Panic> Unit
main () =
  let a = Async.run (fn () -> MutArray.empty ())
  let (n, a) = MutArray.length a
  drop a
  println (show n)

-- lin2.em
import Async
import MutArray
child : Unit -> <IO, Panic, Async.Async> MutArray Int
child () = MutArray.empty ()
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let t = Async.start child
    let a = Async.await t
    let (n, a) = MutArray.length a
    drop a
    println (show n))

-- lin_io.em: Lin のハンドルを try_io の中の await で受け取り、閉じる
import Async
import FileIn
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let t = Async.start (fn () -> FileIn.open "/dev/null")
    match try_io (fn () -> Async.await t) with
      | Ok f -> FileIn.close f
      | Err _ -> ())

-- handled_await.em: 利用者の handler が await_cell を Woke で再開すると、終わっていないタスクの結果を取りに行く
import Async
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let t = Async.start (fn () -> 1)
    let x = handle Async.await t with
      | ctl Async.await_cell c k -> k Async.Woke
      | ctl Async.reschedule () k -> k Async.Woke
      | return x -> x
    println (show x))

-- retype.em: 型を付け替える試み。Task String を Task Int として待つ
import Async
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let t : Async.Task Int = Async.start (fn () -> "s")
    println (show (Async.await t)))

-- raw.em: pub でない口を外から呼ぶ試み
import Async
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let t = Async.start_raw (fn () -> ())
    println (show (Async.take_result t)))

-- twice.em: Lin の結果を2回待つ試み
import Async
import MutArray
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let t = Async.start (fn () -> MutArray.empty ())
    let a = Async.await t
    let b = Async.await t
    drop a
    drop b)

-- unused.em: await の前に Lin の値を使い残す。take_result が戻らない呼び出しに分類されると、この誤りが消える
import Async
import FileIn
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let t = Async.start (fn () -> 1)
    let f = FileIn.open "/dev/null"
    println (show (Async.await t)))
```

  - `rg -n 'STORED_CALLBACK_ROWS' crates` → 何も出ない (止めた U9 の作業の名前が残っていない)
  - 共通の確認
- テストの変更: 種類 2 (一つずつ): `only_the_listed_rows_carry_a_lin_or_poly_mark` (10行)、`only_the_listed_rows_borrow_or_return_a_pair` (`task_cell`)、`every_extern_function_is_declared_once_in_std` (預かる引数の矢印に閉じた row を許す)。追加: 完了の条件に挙げた6つのテストと、`probe_cell`、`probe_task`

### T-9c: `abstract` (言語)

- 目的: D-19 の `pub abstract data` と `pub abstract effect` を構文、名前解決、型検査に入れる。std の宣言はまだ変えない
- 対象: `crates/eml_syntax/src/syntax_kind.rs` (`ABSTRACT_KW`)、`lexer/mod.rs:658`、`grammar/items.rs` (下の5か所)、`ast.rs` (`Item::abstract_keyword()`)、grammar のスナップショット。`crates/eml_hir/src/item_tree.rs` (`DataItem.abstract_ctors`、`EffectItem.abstract_ops`)、`lower/` (HIR の `TypeDef` と `EffectDef` に写す)、`def_map.rs` (`Visibility`、`declare_part`、`qualified`、`T(..)`/`E(..)`、`display_names`、E1032)、`lib.rs` (`codes::ABSTRACT_MEMBER = 1058`)。`crates/eml_types/src/check/field.rs` (`projection`、`update`)、`lib.rs` (`codes::PRIVATE_FIELD = 2021`)。UI テスト、REPL のトランスクリプト、hir と types のテスト。docs/spec/grammar.md、lexical.md、declarations.md、modules.md、effects.md、diagnostics.md、docs/implementation/diagnostics.md
- 作業:
  - 構文: `at_item_start` (items.rs:44) と `at_items_input` (:51) に `p.at(ABSTRACT_KW)` を足す (REPL の入力が `abstract` で始まれば item として読む)。`item()` (:109-122): `pub` の後で `let abstract_ = p.eat(ABSTRACT_KW)`。`abstract pub` は `extern pub` と同じ `misordered` の枝 (E0011 ``` `abstract` cannot be written before `pub` ```、help ``write `pub abstract` ``) で読み直す。`abstract` があって `pub` がない、`extern` が続く、`kind` が `Data`/`Effect` でない、のどれかなら `abstract` のトークンに E0011 を1つ (``` `abstract` is only allowed on `pub data` and `pub effect` ```) 出して続ける (ほかの誤りを重ねない)。`class_member` (:614) と `instance_member` (:664) は `pub` と同じ形で `abstract` を E0011 にして読み飛ばす。`abstract` を予約語にし、docs/spec/lexical.md の並びに足す
  - HIR: `DataItem.abstract_ctors`、`EffectItem.abstract_ops` (`item.abstract_keyword().is_some()`)、`TypeDef`/`EffectDef` にも持つ。`Definition.public: bool` を `visibility: Visibility { Public, Private, Abstract }` にする。`declare_part`: コンストラクタは `data.public && data.abstract_ctors` なら `Abstract`、`data.public` なら `Public`、ほかは `Private`。操作は `effect.extern_keyword.is_some()` なら今までどおり `Private` (E1001 の道のまま)、`effect.public && effect.abstract_ops` なら `Abstract`、`effect.public` なら `Public`、ほかは `Private`。関数、型、エフェクト、クラス、fixity は `Public`/`Private` だけである
  - 解決の優先: `qualified()` の各モジュールのループは今の形のまま (名前ごとに最初の使える定義で `break`)。`Public` は `found` へ、`Abstract` は `abstract.get_or_insert((scope.file, definition.range))`、`Private` は今のとおり (ユーザーのモジュールなら `private`、std なら無視)。ループの後、`decide(found, broken)` が `Some` ならそれ (別のモジュールに `Public` があれば今までどおり勝つ)、次に `abstract` → `Resolved::Abstract(file, range)` (E1058)、次に `private` → `Private`、`unusable` → `Silent`、ほかは `NotFound`。修飾なしの名前 (import の並びと Prelude) は `Public` だけを引く今の道のまま。`fixity_of` (:1237) と `fixities` (:598) は `Public` だけを公開として読む (抽象型の中置コンストラクタの fixity は外へ出ない)。`private_type_item` (:1191、E1032) は `Private` だけを見る。`display_names` (:1010) は `Abstract` を `Public` と同じに扱う (`Definer::Module`)。lower/mod.rs:686 の `Resolved::Private` の隣に `Resolved::Abstract` → E1058 の診断 (定義の位置を secondary に)
  - `T(..)`/`E(..)` (def_map.rs:764): 部品ごとに `scope.values` の定義を引き、`Public` なら今までどおり、`Abstract` なら `(..)` の位置に E1058 を1つ (部品が複数でも1つ) 出して部品を `None` (報告済み) にする。型そのものは取り込む
  - eml_types: `projection()` (field.rs:36) と `update()` (:52) の入口で、`base_ty` の形が `Con(def, ..)` で `program[def].abstract_ctors` かつ `!program.same_scope(def.module, self.id.module)` なら E2021 を1つ出して `Error` を返し、フィールドの解決に進まない (E2014 の help が宣言したフィールドを並べる前に止めるので、隠した名前は出ない。更新のフィールドが複数でも1つ)。`Program::same_scope(a, b)` を足し、`DefMap::same_scope` (def_map.rs:1054) の判定を写す (eml_types は `DefMap` を持たないため)
  - 診断の文言 (文言は実装者が決め、番号と位置は決まり): E1058 の使う位置は ``constructor `Empty` of the abstract type `Map` is not visible outside `Std.Map` `` / ``operation `await_cell` of the abstract effect `Async` is not visible outside `Std.Async` ``、ラベル ``hidden by `abstract` ``、宣言の位置に secondary のラベル `declared here`。`Map(..)` / `Async(..)` の位置は ``` `Map(..)` imports no constructor: `Map` is abstract ```、help ``write `Map` ``。E2021 は ``field `name` of the abstract type `Config` is not visible outside `Lib` `` (射影) / ``the fields of the abstract type `Config` cannot be updated outside `Lib` `` (更新)
  - テスト: hir の `tests/lower.rs` に `an_abstract_constructor_is_hidden_outside_its_module` (E1058 が作る式とパターンで2つ)、`an_abstract_import_takes_only_the_type` (`import Lib (T(..))` で `(..)` に E1058 が1つ、その後 `T` を型に書いても誤りなし)、`an_abstract_operation_is_hidden_outside_its_module` (perform と節で E1058 が2つ、同じモジュールでは0)。`private_in_public_constructor_fields_and_operations` に `pub abstract data Sealed = | Sealed Secret` を足して E1032 が増えないこと。`tests/def_map.rs:619-626` はそのまま通ること。types の `tests/` に `an_abstract_field_is_hidden_outside_its_module` (射影1つと2フィールドの更新1つで E2021 が2つ、help にフィールドの名前がない)。syntax の grammar のスナップショット
  - UI テストと REPL のトランスクリプト (付録の `ui-tests.md` の「`abstract` のテスト」): `check-fail/names/abstract_constructor/` (E1058 が3つ)、`check-fail/names/abstract_operation/` (E1058 が2つ)、`check-fail/types/abstract_field/` (E2021 が2つ)、`check-fail/syntax/abstract_misplaced.em` (E0011 が5つ。ほかの誤りは `abstract` のない宣言と同じ)、`run/data/abstract_api.em`、`crates/eml_cli/tests/repl/declarations/abstract_in_session/`
  - 文書: 次の文をすべて入れる。ただし、modules.md の「公開の範囲」の3つ目の項目 (標準ライブラリの抽象型と抽象エフェクト) と、effects.md の「抽象エフェクト」の最後の文 (`Std.Async` の `Async`) は、std を変える T-9d で入れる (T-9c の時点では std に抽象の宣言がない)。docs/implementation/diagnostics.md に E1058 と E2021 の出し方 (位置、ラベル、help) を1行ずつ足す
    - docs/spec/grammar.md: :19 の `item` を次に置き換え、:118 の `extern` の項目の後に項目を足す

````
item        ::= 'pub'? 'abstract'? ('extern' extern_decl | decl)
````

````
- `abstract` は `pub` の後、`extern` の前に書く修飾子で、`data` と `effect` にだけ付く ([宣言](declarations.md) の「`pub`」)。ほかの宣言に付けたとき、`pub` がないとき、`extern` の宣言に付けたときは、`abstract` のトークンに E0011 を1つ出す (``` `abstract` is only allowed on `pub data` and `pub effect` ```)。`abstract pub` の順は、`extern pub` と同じく E0011 にして読み直す。クラスと instance のメンバーに書いた `abstract` は、`pub` と同じく E0011 にして読み飛ばす
````

    - docs/spec/lexical.md の :34 を次にする

````
data type effect where pub extern abstract import as infixl infixr infix
````

    - docs/spec/declarations.md の「`pub`」: :419 の3つ目の項目を次の1つ目に置き換え、2つ目と3つ目を足す

````
- `pub data` は、型とコンストラクタを公開する。`pub abstract data` は型の名前だけを公開し、コンストラクタと名前付きのフィールドを宣言したモジュールの中だけで見えるようにする (抽象型。[モジュールと名前解決](modules.md) の「公開の範囲」)。`pub abstract effect` は同じくエフェクトの名前だけを公開する ([エフェクトと handler](effects.md) の「抽象エフェクト」)
- 抽象型の Kind はフィールドから普通の規則で決まり ([型と Kind](types.md))、`deriving` も書ける。導出した `Eq`、`Ord`、`Show` は構造で比べて表示するので、抽象型の中身をそのまま見せる (`show` はコンストラクタの名前を書くが、その名前は外から書けない)。隠したい型には `deriving` を書かず、instance を自分で書く
- `abstract` を付けられるのは `pub data` と `pub effect` だけで、`extern data` (コンストラクタがない)、`extern effect` (いつも隠す)、`type`、シグネチャ、fixity、`class` に付けると E0011 である。`=` のない `pub abstract data` は、ほかの `data` と同じく E1025 である
````

    - docs/spec/modules.md の「公開の範囲」: :183 の後に次を足し、:184 の「理由は2つある。1つは、コンストラクタを隠した抽象型を、後で `pub data` の形で意図して入れるためである (…)。非公開の型を返す公開の関数を許すと、その前に裏口の抽象型ができてしまう。」を「理由は2つある。1つは、抽象型は `pub abstract data` で明示して作るものであり、非公開の型を返す公開の関数という裏口を残さないためである。」に置き換える

````
- `pub abstract data` の型のコンストラクタと、そのコンストラクタのフィールドは、宣言したモジュールの外から見えない。外のモジュールでは、修飾してもしなくても、作る式とパターンに書くと E1058 になる。import の並びの `T(..)` は型だけを取り込み、`(..)` の位置に E1058 を出す (help で `T` と書くよう示す)。型の名前そのものは `pub` の型と同じに扱い、その後の型の名前の使いに誤りは重ねない。射影 `e.f` と更新 `{ e | f = v }` は、`e` の型が外のモジュールの抽象型なら、フィールドの名前によらず型検査で E2021 になる (式ごとに1つ。宣言したフィールドの名前は示さない)
- `pub abstract effect` の操作も同じで、外のモジュールの perform と handler の節に書くと E1058 になり、import の並びの `E(..)` はエフェクトだけを取り込んで E1058 を出す。extern のエフェクトの操作 (Prelude の `io_failed`) は今までどおり「定義がない」扱い (E1001) である (上の「標準ライブラリ」)
- 標準ライブラリの抽象型と抽象エフェクト (`Std.Map.Map`、`Std.Set.Set`、`Std.Iter.Iter`、`Std.Async.Async`) も同じで、`pub` でない item のように「定義がない」とは扱わない。診断には定義したモジュールの正式な名前を書く (`Std.Map`)
- `pub abstract data` のコンストラクタのフィールドには、E1032 を当てない。フィールドは外から見えないので、`pub` でない型が現れても裏口にならない
- 同じモジュールかの判定は、セッションの入力どうしを同じモジュールとみなす (下の「REPL のセッション」)
````

    - docs/spec/modules.md の「REPL のセッション」: :170 の「セッションの中では `pub` を見ず、E1032 も当てない。セッションのモジュールはどこからも import されないためである」を「セッションの中では `pub` を見ず、E1032 も当てない。セッションの入力で宣言した抽象型と抽象エフェクトも、セッションの入力どうしでは隠さない (入力はどれも同じモジュールとみなす)。セッションのモジュールはどこからも import されないためである。import したファイルのモジュールの抽象型と抽象エフェクトは、セッションでも隠す」に置き換える
    - docs/spec/effects.md に新しい節を足す

````
## 抽象エフェクト

`pub abstract effect E where …` は、エフェクトの名前を公開し、操作を宣言したモジュールの外から隠す。外のモジュールは row に `E` を書き、`E` を持つ関数を呼べるが、操作を perform できず、`handle` の節に書けず (どちらも E1058)、import の並びの `E(..)` でも取り込めない。extern のエフェクトはいつも隠す (上の「組み込みの `IO`」)。型検査、evidence passing、`mask` は普通のエフェクトと同じで、違いは名前解決だけである。`Std.Async` の `Async` は抽象エフェクトで、生の操作はランタイムとの ABI として [標準ライブラリ](stdlib.md) に書くが、利用者は包みの関数を呼ぶ ([モジュールと名前解決](modules.md) の「公開の範囲」)。
````

    - docs/spec/diagnostics.md: 表に次の2行を足し、E0011 の説明に「`abstract` の位置の誤り」を足す

````
| E1058 | `ABSTRACT_MEMBER` | 抽象型 (`pub abstract data`) のコンストラクタか、抽象エフェクト (`pub abstract effect`) の操作を、宣言したモジュールの外で作る式、パターン、perform、handler の節、import の並びの `T(..)` と `E(..)` に書いた ([モジュールと名前解決](modules.md) の「公開の範囲」) |
| E2021 | `PRIVATE_FIELD` | 抽象型のフィールドを、宣言したモジュールの外で射影か更新した ([モジュールと名前解決](modules.md) の「公開の範囲」) |
````

- 実装者に任せること: E0011、E1058、E2021 の文言。`Visibility` の名前。試作は prototype at `/tmp/claude-0/-home-user-eml/4dfe1f80-72d6-5946-9518-fce039bdc290/scratchpad/exec/redesign/eml` にある (セッションの一時ディレクトリ)
- 条件: いつも
- 完了の条件:
  - `cargo nextest run -p eml_syntax -p eml_hir -p eml_types -p eml_cli` → 通る。既存の UI と REPL のスナップショットはバイト単位で変わらない (`abstract` を使う宣言がまだ std にないため)
  - 次のファイルを作業ツリーの外の一時ディレクトリ `$SCRATCH/abs/` に置き、`timeout 20 cargo run -p eml_cli -- check $SCRATCH/abs/<file>` を流す: `rewrap.em` → E1058 が2つ、終了コード 1。`matching.em` → E1058 が1つ (`(..)` の位置)、終了コード 1。`inside.em` → 何も出さず終了コード 0

```haskell
-- Lib.em: コンストラクタはこのモジュールの中だけで見える
pub abstract data Handle a = | Handle Int
pub make : Int -> Handle a
make n = Handle n
pub get : Handle a -> Int
get (Handle n) = n

-- rewrap.em: 外から型を付け替える試み
import Lib
main : Unit -> <IO> Unit
main () =
  let h : Lib.Handle String = Lib.make 1
  let Lib.Handle n = h
  let h2 : Lib.Handle Int = Lib.Handle n
  println (show (Lib.get h2))

-- matching.em: import の並びで取り込む試みと、パターンで開く試み
import Lib (Handle(..))
main : Unit -> <IO> Unit
main () =
  match Lib.make 1 with
    | Handle n -> println (show n)

-- inside.em: 公開の関数だけを使う正しい使い方
import Lib
main : Unit -> <IO> Unit
main () = println (show (Lib.get (Lib.make 1)))
```

  - `rg -c '^\| E1058 \|' docs/spec/diagnostics.md` → 1。`rg -c '^\| E2021 \|' docs/spec/diagnostics.md` → 1
  - 共通の確認
- テストの変更: 追加だけ

### T-9d: std の抽象エフェクトと抽象型

- 目的: `Async` を `pub abstract effect` に、`Map`、`Set`、`Iter` を `pub abstract data` にし、依存する std、テスト、文書を直す
- 対象: `std/Async.em`、`std/Map.em`、`std/Set.em`、`std/Iter.em`、`std/String.em`、`crates/eml_core_ir/tests/optimize.rs`、`tests/ui/run/std/std_map_balance.em` と `std_set_operations.em` (削除)、新しい `crates/eml_cli/tests/std_invariants.rs`、新しい `tests/ui/check-fail/names/async_op_handled.em` と `abstract_std_constructor.em`、`crates/eml_hir/tests/externs.rs`、docs/spec/stdlib.md、docs/spec/modules.md、docs/spec/effects.md、docs/future/async.md
- 作業:
  - `std/Async.em` を付録の `Async.em` のとおり `pub abstract effect Async where …` にし、コメントを「操作はランタイムとの ABI で、利用者は包みの関数を呼ぶ (docs/spec/effects.md の「抽象エフェクト」)」にする
  - `std/Map.em` の `Map`、`std/Set.em` の `Set`、`std/Iter.em` の `Iter` を `pub abstract data` にする。`Prelude.Cmd` と `StderrMode` は抽象にしない (`Std.Proc` が別のモジュールから作って更新する)。ほかの Prelude の型、`Failure`、`Wake` も抽象にしない (利用者がパターンで分けるか、`Async` が抽象なら外から届かない)
  - `std/String.em:5` と `crates/eml_core_ir/tests/optimize.rs:1899`、`:1908` の `import Iter (Iter(..))` を `import Iter (Iter)` にする (optimize.rs のテストは2つの下ろし方を比べるだけで、スナップショットはない)。`crates/eml_hir/tests/def_map.rs:619-626` は変えない (`display_names` が `Abstract` を `Public` と同じに扱うため)
  - `tests/ui/run/std/std_map_balance.em` と `std_set_operations.em` を消し、新しい `crates/eml_cli/tests/std_invariants.rs` (`tests/main.rs` に `mod std_invariants;`) に `the_map_avl_invariant_holds_through_the_api` と `the_set_avl_invariant_holds_through_the_api` を置く。それぞれ `std = eml_hir::STD` の `Map.em` (または `Set.em`) を、`include_str!("../../../std/Map.em")` の後ろに `pub valid : Map Int v -> Int -> Int -> <Panic> Option Int` (今の UI テストの `valid` を、モジュールの中なので修飾なしで書いたもの。Set は `pub valid : Set Int -> Int -> Int -> <Panic> Option Int`) を足した文に替え、UI テストのプログラム (自分の `valid` を消し、`Map.valid` を呼ぶ) を `eml_test_support::run(with_std(&std, ENTRY))` で流し、stdout を完全に比べる。期待は Map が `random: 199 keys, 0 bad, height Some 9\nascending: 1000 keys, height Some 10\ndescending: 1000 keys, height Some 10\nhalved: 500 keys, height Some 9\n`、Set が `1000 1000: union True, intersection True, difference True, delete True\n1000 20: union True, intersection True, difference True, delete True\n20 1000: union True, intersection True, difference True, delete True\n0 500: union True, intersection True, difference True, delete True\n` である (どちらも今のスナップショットの stdout)。1 s の決まり (docs/implementation/testing.md の「性能のテスト」) に収まらなければ、`run 2000` を `run 1000` に、3つの組み立てを 1000 から 500 のキーに縮め、期待の行を1回の run で測り直す。縮めても速くできないときだけ `#[ignore = "…"]` にする
  - `crates/eml_hir/tests/externs.rs` に `the_async_effect_is_abstract` (`Std.Async.Async` の `abstract_ops` が真、`IO`/`Panic` は extern、`Std.State.State` は偽)
  - UI テスト (付録の `ui-tests.md` の「`abstract` のテスト」): `check-fail/names/async_op_handled.em` (`handled_await.em` の本体) → E1058 が2つ。`check-fail/names/abstract_std_constructor.em` (パターンの `Map.Empty`) → E1058 が1つ
  - 文書: T-9c が残した2つの文 (docs/spec/modules.md の「公開の範囲」の標準ライブラリの項目と、docs/spec/effects.md の「抽象エフェクト」の `Std.Async` の文) を入れる。docs/spec/stdlib.md の :446-447 と :483-484 を「コンストラクタは隠す (`pub abstract data`)。API は上の関数の表だけである」にする (`Map(..)` と `Set(..)` で名前が重なる文は消す)。:838 の宣言を `pub abstract data Iter …` に、:858 を「コンストラクタ `Iter` は隠す」にする。:681 の見直す条件を「`Cmd` の設定の関数を Prelude へ移すか、読めるフィールドの `pub` を入れたときに隠す」にする。docs/future/async.md:57 の「std の外からも見え」を「名前は見えるが操作は見えない。ランタイムの ABI として文書に書く」に直す
- 追加の決定 (実行の途中で利用者と決めた): `Iter` のコンストラクタを隠すと、`Iter` を手で組む `crates/eml_core_ir/tests/optimize.rs` の `CYCLES` と `bench/iter_pipeline_by_hand.em` が書けなくなる。`std/Iter.em` に `pub from_fn : (once Unit -> <e> Option (a, Iter <e> a)) -> Iter <e> a` を足し (docs/spec/stdlib.md の「`Iter`」に1行)、2か所をこれで書き直す。ベンチの `RunStats` が変わったときは、`crates/eml_vm/tests/bench.rs` と `crates/eml_core_ir/tests/lowering.rs` の該当するスナップショットの書き直しを種類 2 としてこの task で行い、コミットのメッセージに差を書く (変わらなければ書き直さない)。`rg` の確かめは `bench/` も対象にする
- 実装者に任せること: `std_invariants.rs` の大きさ (1 s の予算) と、それに合わせて測った期待の行。試作は prototype at `/tmp/claude-0/-home-user-eml/4dfe1f80-72d6-5946-9518-fce039bdc290/scratchpad/exec/redesign/eml/crates/eml_cli/tests/abs_probe.rs` にある (セッションの一時ディレクトリ)
- 条件: いつも
- 完了の条件:
  - `cargo nextest run` → 通る (`cargo insta review` で、消した2つのスナップショットを片付けた後)
  - `cargo nextest run -j 1 -p eml_cli --test integration -E 'test(std_invariants::)'` → 2つ通り、どちらも 1 s 未満 (`SLOW` なし)
  - 次の `map_ctor.em` と、T-9b の `handled_await.em` を `$SCRATCH` に置き、`timeout 20 cargo run -p eml_cli -- check` を流す: `map_ctor.em` → E1058 が1つ、終了コード 1。`handled_await.em` → E1058 が2つ、終了コード 1

```haskell
-- map_ctor.em: std の抽象型のコンストラクタを外から使う試み
import Map
main : Unit -> <IO> Unit
main () =
  match Map.insert 1 2 Map.empty with
    | Map.Empty -> println "empty"
    | _ -> println "node"
```

  - `rg -n 'Map\.(Empty|Even|LeftHigh|RightHigh)|Set\.(Empty|Even|LeftHigh|RightHigh)|Iter\(\.\.\)|Map\(\.\.\)|Set\(\.\.\)|Async\(\.\.\)' crates std tests docs --glob '!docs/plans/**'` → 2行だけ。`tests/ui/check-fail/names/abstract_std_constructor.em` の `Map.Empty` と、`tests/ui/run/kinds/imported_row_parameters/main.em:3` (利用者のモジュール `Gen` の `Iter(..)` で、std の `Iter` ではない)
  - `rg -n '抽象型がまだ|抽象型がない|コンストラクタは公開される' docs/spec` → 何も出ない
  - 共通の確認
- テストの変更: 種類 1: `tests/ui/run/std/std_map_balance.em`、`tests/ui/run/std/std_set_operations.em` を UI テストとしては消す。外から隠すコンストラクタでパターンを書いているためで、確かめていた木の釣り合いはモジュールの中の性質なので、本体を `std_invariants.rs` へ移して同じ stdout を確かめる (縮めたときは測り直した stdout)。種類 3: `std/String.em:5`、`crates/eml_core_ir/tests/optimize.rs:1899,1908` の import。追加: `std_invariants.rs` の2つ、UI の2つ、hir の1つ

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
- 作業: 5.2 の D-10〜D-13 のとおり。`take` は預かった値の語をそのまま返し、受け取らずに放すセルの `Outcome::Value(w)` は `with_worker(ctx, cell.worker, |ctx| ctx.decref(w))` で落とす (D-11)
- テスト (`tasks.rs`。各試験は `eml_test_support::run` の検査モードと、`eml_test_support::execute(&lir(src), &RunConfig::default())` の速いモードの両方で流し、stdout を比べる):
  - (a) `run (fn () -> 41 + 1)` を出力 → `42\n`
  - (b) タスクの中の入れ子の `run` → `inner\nouter\n`
  - (c) 本体が `a`、`b` を出力する2つの子を始めて両方を待つ → 逐次で `main\na\nb\n`
  - (d) (c) を `Sched::Seeded(1)` と `Seeded(2026)` で流す → stdout が同じで、`schedule_independent` の統計が等しい
  - (e) `cells.rs` に `use crate::` の行がない (ファイルを読んで確かめる)
- 文書 (T-9b が残した文): docs/implementation/status.md の「まだないもの」の `Std.Async` を呼ぶと内部の誤りで止まるという項目を消し、docs/spec/stdlib.md の「`Std.Async`」の「VM の extern の命令はこれらの行を実行しない」の文と、extern の行のコメントを `Op::Async` の説明に替える
- 文書: docs/spec/vm.md の新しい節「`Async` のタスク」(状態、入口の `Task`、セル、登録表、受け箱、run の終わり、D-2 の2種類の待ち、D-3 の核と実行系の分け方)、「命令」の一覧に `Async`、「タスクとスケジュール」に `Async` の仕事は FIFO で積むこと、「追記」に `MissingEntry`
- 実装者に任せること: (b) と (c) のプログラムの細部 (期待する stdout を変えない範囲)
- 条件: いつも
- 完了の条件: `cargo nextest run -p eml_vm --test integration -E 'test(tasks::)'` → 5つ通る。共通の確認
- 落とし穴: `transfer_back(closure, from = spawner)` が `WrongOwner` を出したら Q-2 の既定に従う

### T-12: VM のタスク (2) 待ち、`await`、失敗、ハンドル

- 目的: `park`、`ready`、`task_cell`、`take_result` と `run_result` の失敗の道、`TaskHandle` の `Drop` と受け箱の処理、`StaleTask`、D-11 の不変条件の `debug_assert!`
- 対象: `cells.rs`、`vm.rs`、`tasks.rs`、REPL のトランスクリプト
- 作業: 5.2 のとおり。`take_result` の `Raise::Panic(m)` は `Extern::Panic` の道 (`abort_to(Panic, PANICKED, [fresh_string(m)])`、なければ `VmFault::Panicked(m)`)、`Raise::Io { kind, m }` は `failed(Fault::Raised(Box::new(Raised { kind, message })))` (vm.rs:1533-1558)。`take` の `Finished` でない枝は `Internal("awaiting an unfinished task")` のままにする (D-12。T-9d で `Async` が抽象エフェクトになり、操作は `Std.Async` の外から E1058 になるので、検査を通ったプログラムからは届かない)
- テスト (`tasks.rs`、2つのモード):
  - (f) 相手が終わる前と後に待つ → どちらも `x\n`
  - (g) 3つのタスクが `yield_now` を2回ずつ → `a1\nb1\nc1\na2\nb2\nc2\n`
  - (h) 子の `panic "boom"` を待つ → `RuntimeError` の表示が `` runtime error: panicked: boom in `main` `` と等しい。`await` を `try_panic` で囲むと → `Err boom\n`
  - (j) `MutArray` を返すタスクを `spawn` して待たない → 検査モードの run が `Leak` なしで終わる
  - (l) `Lin` の結果: `MutArray.empty ()` を返すタスクを待って `MutArray.length` → `0\n` (結果は thunk に包まず、値のまま預かる行を通る)
  - (k) は REPL のトランスクリプト `crates/eml_cli/tests/repl/async/stale_handle.repl` にする。1つ目の入力で `Async.run` が返したハンドルを束縛し、2つ目の入力で `Async.run (fn () -> Async.await t)` → `a task handle outlived its run` の実行時エラー
- 文書: docs/spec/vm.md の「`Async` のタスク」を仕上げる (`Async` の操作は `Std.Async` の外から見えないので、待たずに再開する道がないことを含む)。docs/spec/stdlib.md の「実行時エラー」に `a task handle outlived its run` の行 (REPL の入力をまたいだとき)
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
- 作業: D-17 の harness の変更。付録の `ui-tests.md` のうち、`run/async/` の8つ (`exit_in_task.em` を除く)、`run-fail/async/` の3つ、`check-fail/types/task_captures_var.em`、`check-fail/types/async_in_par_child.em`、`check-fail/linearity/task_awaited_twice.em`。`check-fail/names/async_op_handled.em` は T-9d が置くので、ここでは触らない。`run/async/lin_result.em` と `drop_handle.em` の `Lin` の結果は、値のまま預かる行を通る
- 文書: docs/spec/stdlib.md の「`Std.Async`」に `pub` の面の表 (`run`、`start`、`spawn`、`await`、`yield_now`、`try_all`、`Failure`) と、次の9つの規則
  1. 暗黙のスコープ: `Async.run f` は根のスコープを開き、`f ()` をその中の最初のタスクとして動かす。`start` と `spawn` は新しいタスクを、呼んだタスクのスコープ (呼び出しを動的に囲むいちばん内側の `run` の根) に加える。孫も同じ根に入る。タスクの中の入れ子の `run` は自分の根を開く
  2. `run` の終わり: `run` は根のスコープのすべてのタスクが終わってから戻る (待ったかどうかによらない)。失敗がなければ本体の値を返し、あれば終わった順で最初の失敗を起こし直して本体の値を捨てる。IO の失敗は種類と文言を、Panic は文言を保つ。後の失敗は報告せず、1つずつ `RunStats.dropped_failures` に数える。失敗したタスクを待って、起こし直した失敗を捕まえなかったタスクも数える
  3. `Task a` は `Lin` で、`await t` が消費する。`await` は値を返すか、タスクの失敗を起こし直す。失敗はスコープの失敗のままで、`await` の周りで捕まえても `run` は起こし直す。失敗を分けるには、タスクの中で `try_all` を使う
  4. `drop t` はハンドルを放すだけで、タスクは最後まで動き、`run` はそれも待つ。値は終わったときに放し、失敗はスコープの失敗になる
  5. ハンドルは、1つのプログラムの実行の中なら、作った `run` の外へ出てよい。そのときタスクは終わっているので、`await` はすぐ値を返すか失敗を起こし直す。REPL の入力をまたいだハンドルは実行時エラー ``a task handle outlived its run`` である
  6. どのタスクの `exit n` も、プログラムを `n` で終える (T-15 で書く)
  7. `yield_now ()` はタスクを実行待ちの列の後ろへ回す
  8. タスクは閉じた row `<IO, Panic, Async>` で始まる。始めた側の handler と `var` は見えず、`var` を捕まえると型の誤りになる
  9. `Async` は抽象エフェクトで、操作 `await_cell` と `reschedule` は `Std.Async` の外から perform も handle もできない (E1058)。利用者は `await`、`yield_now` などの包みを呼ぶ
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
  3. docs/implementation/status.md の「今の言語の範囲」に `Async` の項目を足し、制限の節に、取り消しがないこと (`drop` は取り消さない)、`scope`、`spawn_in`、時間、channel、I/O の driver がないこと、入れ子の `run` はそのタスクを待たせること、終わった順で最初の失敗が勝つこと、新しい種類は `fail_io` だけが出すことを書く。「今の言語の範囲」に「抽象型と抽象エフェクト (`pub abstract`)」を足し、:50 のまだない機能の並びから「抽象型」を消し、:141 の `Cmd` の文の理由を「`Std.Proc` が別のモジュールから作って更新するので、抽象型にしていない」に直す
  4. docs/implementation/architecture.md を、`cells.rs` と `monitor.rs` と libc の依存に合わせて直す (T-4 で足した所を確かめる)
  5. docs/future/roadmap.md から `async-tasks` の項目と、「言語機能と構文」の抽象型の項目 (:125-126。`Std.Map` と `Std.Set` の下位項目を含む) を消す。「その後の項目」の「非同期」に次の項目を足す。docs/future/async.md の「スコープとタスク」「失敗と `exit`」「テストと決定性」から、stdlib.md と vm.md が書いた内容を消し (`Failure` と `try_all`、`fail_io`、種類の一覧、`Either`、`Prelude.Cancelled` の重なり、1つの worker の `exit`、seed を既定にする文)、`scope`、`spawn_in`/`start_in`、`cancel_scope`、`fail_scope`、`cancel`、スコープの終わりの手順、`reraise`、取り消しとしての `drop`、取り消しの項目、`ResourceLimit` を作る所、複数の worker の `exit`、テストの driver と仮想の時計を残す。EPIPE の項目は「ネットワークとシグナル」へ移す。「前提」に `async-tasks` を書いた段の名前を項目の名前に直す
     - EPIPE を `ConnectionReset` に写すこと
       - 後に回した理由: タスクと関係のない `Child` の書き込みの誤りの種類を変えるため。
       - 見直す条件: ソケットを足す段 (`TcpStream` の書き込みが EPIPE を返す段)。
  6. docs/overview.md の確定した設計判断の表と、docs/README.md の「文書の地図」の「内容」の欄を、変わった所だけ直す (overview.md の表が公開の範囲を挙げていれば `abstract` の行を足す)
  7. CLAUDE.md を、規則か道しるべが変わったときだけ直す (`--sched` の行は T-5 で直した。Architecture の表の `eml_vm` の行に `Async` のタスクを足すかを確かめる)
  8. この計画書と付録のディレクトリを消す。消す前に、5章の決定のうち「リポジトリに残す所」に書いたものが `docs/` にあることを確かめる
  9. コミットのメッセージに、文書で何をどこへ移したか、テストの変更の種類と理由 (T-4、T-5、T-7、T-8、T-9a、T-9b、T-9c、T-9d、T-13、T-14 のもの)、計測の目立つ差、大きさを変えた行、EPIPE を後に回すと決めた日 (2026-10-27) を書く
- 完了の条件:
  - `cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` → 通る
  - `rg -n 'async-tasks' docs --glob '!docs/reports/**'` → 何も出ない
  - `rg -n '抽象型がまだ|抽象型がない' docs --glob '!docs/reports/**'` → 何も出ない
  - `ls docs/plans` → この計画のファイルがない
  - 「9. 確かめ方」の全体の確認

## 8. 進める順

T-1〜T-8 は済んでいる (表の「状態」)。残りの順は次のとおりである。

```
T-8 まで (9747404)
  └─ T-9a → T-9b ─┬─ T-9c → T-9d
                  └─ T-10
                     T-11 ← (T-4, T-6, T-10)
                     T-12 ← (T-11, T-9d)
                     T-13
                     T-14 ← (T-13, T-5, T-7, T-9d)
                     T-15 ← (T-14, T-2)
                     T-16 ← すべて
```

- 並べて進められる組: {T-9c、T-9d} と {T-10、T-11}。T-9d と T-11 が済むと、T-12 から後は一列である
- 1つずつ進めるときの既定の順: T-9a、T-9b、T-9c、T-9d、T-10、T-11、T-12、T-13、T-14、T-15、T-16

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
| Q-2 | 始めるときの `transfer_back(closure, from = spawner)` が `WrongOwner` を出さないこと、タスクを放すときの `retire_worker` が `TransferLeak` を出さないこと (推定) | T-11、T-12 | 実装のテストで分かる | `WrongOwner` なら始めるときの `transfer_back` をやめる (`par` の右の子と同じ)。`TransferLeak` なら `retire_worker` を run の終わりに回し、async-threads の見直しの項目に書く |
| Q-3 | `RunStats` の4つの欄がスケジュールによらないこと (推定) | T-13、T-14 | T-13 の (m) と T-14 の seed の run で分かる | 違った欄は `schedule_independent` で 0 にし、理由を docs/implementation/testing.md に書く |
| Q-4 | `Async` の UI テストの期待する出力と `<std>/Async.em:L:C` は推定である | T-14、T-15 | 実装の後で流して確かめる | 違ったら、ランタイムの不具合かプログラムの誤りかを確かめてから受け入れる。9つの規則と食い違う出力は受け入れない |
| Q-6 | Miri の新しいテストの費用 | T-1 | T-1 で測る | 予算を超えたら、そのテストの本体を小さくする (`cfg!(miri)` で縮めない) |
| Q-7 | 抽象エフェクトを入れない場合の、利用者の handler が待たずに `await_cell` を再開する道 (閉じた) | - | T-9c と T-9d を入れたので使わない | 記録だけ残す。入れない場合は `take` の終わっていない枝を `VmFault::UnfinishedTask` (表示 `a task was awaited before it finished`) にし、run-fail のテストと「実行時エラー」の行を足す案だった |
| Q-8 | `=` のない `pub abstract data` | T-9c | 決めた | 今の `data` と同じく E1025 |
| Q-9 | `pub abstract extern data` | T-9c | 決めた | E0011 (D-19) |
| Q-10 | E1058 の文言がモジュールを正式な名前 (`Std.Map`) で書くこと | T-9c、T-9d | 実装のときに docs/implementation/diagnostics.md を読む | 診断の書き方の決まりが std のモジュールを短い名前で書く方を選ぶなら (modules.md:87 は、隠されない限り短い名前を使う)、それに従う |
| R-1 | `Map` と `Set` の木の釣り合いを確かめるテストが、1 s の予算に収まること (試作は `run 2000` と 1000 のキーで 1.2 s) | T-9d | T-9d で `-j 1` で測る | 大きさを縮めて期待の行を測り直す。最後の手段として理由を書いて `#[ignore]` にする |
| R-2 | `Visibility` が `Definition` を読むすべての所に触れる (def_map.rs :366-476、:598、:618、:664、:683、:691、:750-768、:1010-1027、:1191、:1237、:1393、:1456) | T-9c | T-9c の作業がそれぞれの読む所を決めている。既存の UI と REPL のスナップショットがバイト単位で変わらないことで確かめる | 変わったスナップショットがあれば、抽象でない item の扱いが変わったので直す |
| R-4 | 抽象型の `T(..)` は、黙って型だけを取り込まず、誤り (`(..)` に E1058) にする | T-9c | 決めた | 試作で、黙って部品を `None` にすると後の使う位置も黙ることが分かったので、誤りにする |

## 付録: レビューで変わった主な点

- 待ったタスクが VM の `Task` を持ち続ける案は退けた。継続だけにし、`Async.run` を呼んだ `Task` だけがスタックを持って待つ (D-2)
- ネイティブ化に備え、意味の核を `cells.rs` に分け、VM の型に依らない形にした (利用者の求め。D-3)
- `Task` の結果は値のまま渡す。表の預かる印が `Lin` の結果を通す (D-1)。最初の版は結果を `once Unit -> a` の thunk で渡したが、T-9 の実装で extern の引数の数の食い違いと、`Lin` の結果の E3001 に当たった。thunk に包む案は、結果ごとの閉包の確保と、引数の数の規則と型の印の規則が別に要るので退けた
- 預かる印 `held` は引数ごとに表に書き、借用の印と独立にした。型ごとの印にしないのは、写すかどうかが行で決まるからである (`MutArray.read` は写し、`MutArray.length` は写さない)。`task_cell` はハンドルを借りて預かる。借りない行にすると Perceus の verifier が `t.3 (lin) is duplicated` で断ることを試作で確かめた (D-1、T-9b)
- 表の印の列を、2値の `Linearity` から LIR と同じ3値の `Mark` にした。2値のまま `Poly` の位置を約束で `Lin` か `Unr` と書く案は、tie のテストに2通りの写し方と、表が `Lin` と書く理由の文が要るので退けた。`Mark` を移す変更は機械的なので、独立した T-9a にした
- `Async` を抽象エフェクトにした。利用者の handler が `await_cell` を待たずに再開すると、終わっていないタスクの結果を取りに行く道があった。実行時エラーにする案は、後の段の生の操作ごとに実行時の扱いが要るので退け、名前解決で断る (D-19、T-9d)
- 抽象型をこの段に入れた。`Map`、`Set`、`Iter` のコンストラクタを、約束でなく検査で隠す。型とエフェクトで同じ修飾子 `abstract` を使う (D-19)
- 隠した部品を名指す位置は、extern のエフェクトと同じ E1001 でなく、新しい E1058 にした。「定義がない」は std の `pub` でない item の約束で、利用者も書ける `abstract` には理由を言う診断が要る。extern のエフェクトの操作は今までどおり E1001 である (D-19)
- `Map` と `Set` の木の釣り合いは、コンストラクタでパターンを書く UI テストを消したあとも、`eml_test_support::with_std` で std のファイルに `pub valid` を足して確かめる (T-9d)
- 抽象型があっても `Task a` は `extern data` のままにした。単相のハンドルを抽象型で包んでも `finish_ok` には預かる印が要り、`TaskHandle -> a` は戻らない呼び出しに分類され、ハンドルごとに箱が1つ増える。`extern data` もコンストラクタを持たないので、隠して得るものはない (D-1)
- `VmFault::Deadlocked` は足さない。この段では待ちの輪を作れない (D-13)
- 止める語に `END` のビットを足す案は退けた。host の読み手の振る舞いが変わる。終わりの欄は `exit` だけを持つ (D-4、D-5)
- `Finished.exit` の出どころは `control.exit_code()` だけにした (D-5)
- ハンドルは同じ実行の中なら `run` を越えて待てる。`StaleTask` は REPL の入力をまたいだときだけである (D-10)
- check-fail のテストは誤りの番号の範囲で `types/` と `linearity/` に置き、`check-fail/async/` は作らない (D-17)
- 手で `cargo run` を流して確かめることはやめ、UI テストの harness だけで確かめる (D-17)
