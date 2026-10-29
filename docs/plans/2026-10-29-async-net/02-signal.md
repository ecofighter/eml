# 付録2: `Std.Signal` の設計

基準は HEAD dd5b3f0 である。確かめた事実には「確かめた」と添え、推定には「(推定)」と添える。

前提として決まっていること。

- **CLI:** CLI は常に本物の driver を使い、`--clock` は廃止する (Net-U1)。
- **テストのハーネス:** 仮想の driver は `RunConfig` か `Machine` を通してだけ選ぶ。
- **`Std.IoTest`** は削除する (Net-U6)。
- **仮想のシグナル**は、ハーネスの予定である。
- **Ctrl-C:** 「前の Ctrl-C をまだ届けていない間に来た Ctrl-C は `_exit(130)`」という規則を採る。

内部の型の名前は `Clock` のままにする。以下の `real_driver` は、今の `Scheduler::real_clock` の旗である (確かめた: sched.rs:305-307)。

## 1 結論

### 1.1 API (`std/Signal.em`、新しい std モジュール `Std.Signal`)

```haskell
pub data Signal =
  | Interrupt      -- Ctrl-C: SIGINT on unix, CTRL_C_EVENT on Windows
  | Terminate      -- SIGTERM on unix
  deriving (Eq, Show)

pub wait : Signal -> <IO, Async> Unit
```

意味は次のとおりである。

- **待つタスクが複数あるとき:** 届いたシグナルは、届いた時点でそのシグナルを待つタスクをすべて、待ち始めた順に `Woke` で起こす。
- **取り消し:** `wait` は `sleep_ms` と同じく取り消しの地点である。待ちに入る前に `point` を確かめる。取り消された待ちは列から外れて `Cancelled` で再開し、ラッパーが `fail_io Prelude.Cancelled "task was cancelled"` を投げる。
- **貯めない:**
  - 届けるのは、その時点で待っているタスクだけである。
  - 待つタスクがなければ既定の動きをする。`Interrupt` は run を止め (終了コード 130)、`Terminate` も同じ止め方で止める (143)。
  - 起こされてから次の `wait` までに来たシグナルも、これに含む。
  - 届ける前に来た同じ種類のシグナルは、1つにまとまる。
- **`eml run` の強制終了:** 前の同じシグナルがまだ効いていない間に来たシグナルは、その場でプロセスを終わらせる。
  - Ctrl-C は、前の Ctrl-C を届けていない (保留の) 間か、止める語に `INTERRUPT` が立っている間は `_exit(130)` である。
  - SIGTERM は、前の SIGTERM を届けていない間か、止める語に `TERMINATE` が立っている間は `_exit(143)` である。
  - 「届けていない」状態は、worker が集める地点でそのシグナルを処理するまで続く。
  - 届けた後のシグナルは1回目として扱う。待つタスクが残っていなければ run を止め、その次の1回で終わらせる。
  - 2つの種類は、互いの強制終了を呼ばない。
- **本物のシグナルが `wait` に届くのは、本物の driver のときだけである。** 仮想の driver では、本物のシグナルは既定の動きをし、`wait` に届くのはハーネスの予定 (1.5) だけである。
- **`eml repl`** は SIGTERM の handler を持たず、強制終了も使わない。`Signal.wait Interrupt` はそこで使える。
- **Windows:** `Interrupt` は共通の仕組みを使う。OS から `Terminate` を起こす出来事はなく、`net-windows` に回す。
- **worker が忙しいとき、ブロックしているとき:**
  - eml のコードを動かしている worker は、次の安全点で (`NOTICE` のビットによって) シグナルを集め、待つタスクを起こす。起こしたタスクは、動いているタスクが次に切り替えるときに走る。
  - 同期の読み (stdin、`FileIn`、`Child`) でブロックしている worker は、読みが戻ってから集める。
  - どちらの場合も、同じシグナルの2回目は強制終了する (上)。

### 1.2 std の配線

`Chan` と同じ型である (確かめた: std/Async.em:11-28, 277-288; std/Chan.em:3-5)。

`std/Async.em`:

- op `await_signal : Int -> Wake` を最後の op として追加し、コメントはちょうど1行にする。
- `start_task` の最後の節に `| ctl await_signal s k -> park_signal s k` を追加する。
- `pub signal_wait : Int -> <IO, Async> Wake` を追加する。0 が Interrupt、1 が Terminate で、ほかの値は eml 側で `fail_io NotAvailable "no such signal"` にする。
- extern `park_signal : Int -> (once Wake -> <IO> Unit) -> <IO> Unit` を追加する。継続は `park_time` と同じく held である。

`std/Signal.em`: `wait s = match Async.signal_wait (code s) with | Async.Woke -> () | Async.Cancelled -> raise_cancelled ()`。`code` と `raise_cancelled` は private である。

### 1.3 実行時の経路

**制御 (eml_rt。eml_rt の関門が掛かる)**

- **止める語のビット `TERMINATE = 8`。** object-model.md:806 が空けてあるビットである (確かめた)。`pub fn terminate()` が止める語と worker の語に立てる。
- **worker の語だけのビット `NOTICE = 1 << 30`。** `PREEMPT` の隣に置く。意味は「次の安全点で集める」である。`notice()` が立て、`take_notice()` が下ろす。
- **`Stop` に `Terminated` (表示は `terminated`) と `Notice` (表示は `a signal notice`。利用者には見せない) を足す。** `stop_reason` の優先順は Interrupted > Terminated > MemoryLimit > Notice > Preempt > DepthLimit である。
- **新しい語 `signals: AtomicU32`:**
  - ビット `b` は保留中を表す。
  - ビット `b << 16` は中継済みを表す。
  - `b` は `INTERRUPT` か `TERMINATE` である。
- **新しい語 `watched: AtomicU32`。**
- 2つの語はどちらも、`verus!` の中の `Control::new` で 0 に初期化する (確かめた: control.rs:48-58)。
- メソッドは安全で Relaxed とし、`verus!` の外の impl に置く。

| メソッド | 働き |
|---|---|
| `defer(b)` | `fetch_or` |
| `pending_signals()` | 下の半分を返す |
| `claim_signals()` | CAS で `new = pending & !relayed` を求め、`new << 16` を立てて `new` を返す |
| `settle(bits)` | `fetch_and(!(bits | bits << 16))` |
| `take_pending()` | 0 を swap し、下の半分を返す |
| `set_watched(bits)` | store |
| `watched()` | load |

- `clear()` は `signals`、`watched`、`NOTICE` を 0 にする。`begin_run()` は `PREEMPT` と一緒に `NOTICE` を下ろし、シグナルの語には触れない。

**doorbell (新しい `eml_vm::Doorbell`。`Machine` ごとに1つで、static は使わない)**

- `Doorbell::new() -> io::Result<Arc<Doorbell>>`。
  - unix: `libc::pipe` で作り、両端に `O_NONBLOCK | FD_CLOEXEC` を付ける。素の `i32` のフィールドに持ち、`Drop` で閉じる。
  - Windows: `Mutex<bool> + Condvar`。
- `ring(&self)`:
  - unix: async-signal-safe な `write(write_fd, &1, 1)`。errno は保存して戻す (Linux は `__errno_location`、macOS は `__error`)。EAGAIN は構わない。
  - Windows: 旗を立てて `notify_one` を呼ぶ。コンソールの handler は普通のスレッドで動く。
- `pub(crate) fn wait(&self, timeout: Option<Duration>)`:
  - unix: 読み側を `poll` し、EAGAIN まで読む。
  - Windows: condvar で待ち、旗を下ろす。
- `RunConfig::with_doorbell(Arc<Doorbell>)` と `Machine::with_doorbell(Arc<Doorbell>)` を足す。`AnyMachine::new` が渡す。`Scheduler` は `Machine` から `Option<Arc<Doorbell>>` を受け取る。
- 約束: 1つの doorbell は1つの `Machine` に仕える。その run は逐次なので、同時に読む監視のスレッドは多くても1つである。
  - doorbell が要るテストは自分で作る。こうして、1つのプロセスの中の監視のスレッド (`cargo test` のスレッド) が doorbell を共有しないようにする。
  - doorbell のない `Machine` は、10 ms の監視の刻みに戻る。
  - 前の run の ring が残したバイトは、空振りの起床を1回起こすだけである。

**handler (eml_cli の bin `interrupt`)**

- `eml run` と `eml repl` は `Doorbell` を作る。
  - 作れなければ doorbell なしで進み、警告 `warning: cannot create the signal doorbell: {e}` を出す。
  - doorbell は `Installed { control, doorbell: Option<Arc<Doorbell>>, mode }` と、`RunConfig` / `Repl` の設定に入れる。
- `respond(b) -> Exit(code) | Handled` は次の順で決める。
  1. `Mode::Run` で `(stop_bits() | pending_signals()) & b != 0` なら、`Exit(130 if b == INTERRUPT else 143)`。
  2. そうでなく `watched() & b != 0` なら、`defer(b)`。
  3. そうでなければ `interrupt()` か `terminate()`。
  4. 2 と 3 では、doorbell があれば `doorbell.ring()` を呼ぶ。
- `TERMINATE_EXIT_CODE: u8 = 143`。`on_sigterm` は `Exit` なら `_exit(143)` を呼ぶ。
- unix の `register(mode)`: SIGINT は今のまま、SIGTERM は `Mode::Run` のときだけ登録する。どちらも `sa_flags = 0` である。
- `main.rs` は `error.terminated()` を 143 に写す。
- Windows: CTRL_C_EVENT は `respond(INTERRUPT)` を呼ぶ。ほかの出来事は今までどおり FALSE を返す。

**プールのスレッドは SIGINT と SIGTERM を塞ぐ (unix)**

- `RealDriver::submit` は、`Op::Job` の `proactor.push` を guard で包む。guard は `pthread_sigmask(SIG_BLOCK, {SIGINT, SIGTERM}, &old)` で2つを塞ぎ、`SIG_SETMASK` で `old` に戻す。
- 理由: compio は、プールのスレッドを `dispatch` から `std::thread::spawn` で遅延して作る。`dispatch` は driver のスレッドの `push` から呼ばれ、新しいスレッドは塞ぎを受け継ぐ (確かめた: compio-driver 0.12.5 asyncify.rs:113-130; poll/mod.rs:372-392)。
- 塞ぐのは非同期の2つのシグナルだけなので、プールのスレッドで起きる同期のフォールトは届くままである。
- プールを使うのは `Op::Job` だけである。`Listen` は Net の設計で、持ち主のスレッドで動かす。
- `eml_io` は unix 専用の依存として `libc` を足す。同じ行を Net-U2 も足すので、共有する。

**監視 (eml_vm)**

- `Scheduler` が持つ: `control: Arc<Control>`、`doorbell: Option<Arc<Doorbell>>`、`monitor: Option<Monitor>`。
- `Policy` から `task_added` / `switched` を除き、`const PREEMPTS: bool` を足す (`Normal` だけが真。`Normal` は欄のない型になる)。
- `Monitor::start(control, inbox, waker: Arc<dyn IoWaker>, doorbell, preempt: bool)`。起動には既存の `spawn_masked` を使う (Net-U2 のあとの置き場所にある)。1 run に多くても1つである。始めるのは次の場所である。
  - `P::PREEMPTS` のときの `add_task` (タスクの開始はすべてここを通る。確かめた: vm.rs:2245)。
  - `real_driver` のときの `park_signal`。
- `set_current` は `P::PREEMPTS` のときだけ `watch.switches` を増やす。
- 繰り返しの中身:
  1. doorbell を待つ。doorbell がなければ `park_timeout` で待つ。待つ時間は、`preempt` のとき、または doorbell がないときは、次の刻みまでの時間とする。そうでなければ期限なしである。
  2. `stop` なら戻る。
  3. `new = claim_signals()`。`new != 0` なら `Notice::Signal(new)` を積み、`control.notice()` を呼ぶ。
  4. `new != 0 || stop_bits() != 0` なら `waker.wake()` を呼ぶ。
  5. `preempt` で刻みの時刻が来ていれば、刻みの処理を走らせる。
- `stop`: 旗を立て、doorbell があれば ring し、`unpark` する。

**worker (cells.rs、vm.rs)**

- `Wait::Signal(kind)` を足す。
- `Registry` に `signal_waiters: [Vec<CellRef>; 2]` と `unheard: u32` を足す。
- 時間の輪の値を `enum Due { Task(CellRef), Signal(u32) }` にする。Net の設計で先に入る輪の項は、Sig-U3 で `Due::Task` に直す。
- `deliver(kind, out)`: 列の順に、待つタスクをすべて `WOKE` で再開し、列を空にする。待つタスクがなければ `unheard |= bit` とする。
- `drain` は `Notice::Signal(bits)` を受けると、各ビットを `deliver` し、続けて `settle(bits)` を呼ぶ。
- `expire` は `Due::Signal(b)` を受けると `deliver(b)` を呼ぶ。
- `unpark` は `Wait::Signal` に対して、列から外し、`CANCELLED` で再開し、`cancels += 1` とする。
- `collect` は `take_notice()` から始める。
- **安全点での `NOTICE`:** `poll` は `Continue | Switch | Collect` の3通りを返す。
  - `polled_stop` は `Stop::Notice` を `Preempt` と同じに扱う。`yielding() != 0` ならビットを残して続ける。
  - そうでなければ `take_notice()` で下ろし、もう一度 `poll` する。`Preempt` なら `Switch` とする。別の止まりは誤りとする。それ以外は `Collect` である。
  - `Collect` は `collect` を走らせて同じタスクを続ける。切り替えないので、Sequential の「`preempt` がなければ切り替えない」(確かめた: vm.md:381) を保つ。
  - 起こしたタスクは、走れるタスクの列の後ろに入れる。
  - 空きのループでは、`wait` の後の `poll` が `PREEMPT` と同じように `NOTICE` を捨てる。続く `collect` が仕事をする。
- `relay_on = real_driver && monitor.is_some()`。
- **`watched` の不変条件:** `relay_on` の間は、`park_signal` が戻った時点から、`watched` は列が空でない種類をすべて含む。
  - `set_watched(if relay_on { signal_bits() } else { 0 })` を、`park_signal` の最後と、すべての `collect` の最後で呼ぶ。
  - 1回の `collect` の中で、列が空になった後もビットが立ったままなら、その分は `unheard` になる。
- **`collect` の後に `unheard != 0` のとき:** そのビットを止める語に立て、`Err(VmFault::Stopped(stop_reason(bits)))` を返す。位置の規則は、空きのループが INTERRUPT で止めるときと同じである。`main` の `Finish` (受け箱だけ) では、`unheard` を立てるが、終わった run は失敗させない。
- **空きのループ**。`listening = relay_on && signal_bits() != 0` とする。
  - デッドロックになるのは `deadline.is_none() && pending == 0 && !listening` のときだけである。
  - listening のときは `driver.wait(deadline.or(Some(u64::MAX)))` を呼ぶ。本物の driver は1回の眠りを `IDLE_TICK` で切り (確かめた: vm.md:459)、届いたときは監視のスレッドの waker が眠りを終わらせる。
- **`Scheduler` の `Drop`** (どの経路でも): `set_watched(0)`、`take_pending()` して捨てる、監視を止める、の順で行う。
  - run の最後の集める地点の後に defer されたシグナルは、run が終わることで消費される。`eml run` は run 自身の終了コードで終わる。

**止まり:** `RuntimeError::terminated()` を足す。`stops_the_run` に `Terminated` を含める (確かめた: vm.rs:1591-1603)。出力は `runtime error: terminated`、終了コードは 143 である。

**IDLE_TICK:** 空きの待ちの上限は残す。waker に初めての本番の呼び手として監視のスレッドが付く。ロードマップの項は「10 ms の上限をなくす」に絞る。

### 1.4 Windows

- `Interrupt` の配送は実装する。`Terminate` (CTRL_CLOSE_EVENT / CTRL_SHUTDOWN_EVENT) は `net-windows` に移す。
- シグナルの印を付けた UI テストは、どの OS でも走らせる。OS にシグナルを送るテストは `#[cfg(unix)]` にする。
- Windows で覆うもの:
  - `windows.rs` の `respond` の単体テスト
  - `Doorbell` のテスト
  - `eml_vm/tests/signal.rs` のテスト (cfg なし)
- 覆えないのは、OS が `console_event` を呼ぶところだけである (推定)。

### 1.5 仮想のシグナルの予定 (ハーネス専用)

**API (eml_vm。eml_cli が再公開する)**

- `pub enum Signal { Interrupt, Terminate }`。`Debug, Clone, Copy, PartialEq, Eq` を導出する。
- `pub struct VirtualSignal { pub at_ms: u64, pub signal: Signal }`。
- `RunConfig` に欄 `signals: Vec<VirtualSignal>` (既定は空) と `with_signals(...)` を足す。`Machine` にも `with_signals(...)` を足す。`AnyMachine::new` が一覧を渡す。

**意味**

- `Machine` は一覧を持ち、次の run にだけ渡す。REPL のセッションは一覧を再生しない。
- その run の始め、最初の命令の前に、`Scheduler` が各項を `Due::Signal` として、期限 `at_ms` で時間の輪に載せる。一覧の順に載せる。
  - `at_ms` は、driver を作ったときからのミリ秒で、driver の時計で数える。
  - `Machine` は、最初の run の始めに、`Scheduler` より先に driver を作る (確かめた: lib.rs:419-436)。したがって新しい `Machine` では、`at_ms` はその run の始めから数える。
- **満ちる順:** 集める地点で、タイマーの順 (期限、次に `seq`) に満ちる。同じミリ秒では、予定のシグナルがプログラムのタイマーより先に満ちる。仮想の時計は、すべてのタスクが待つときだけ進む。そのため、満ちる時点でどのタスクが待っているかは、どの seed でも同じである (確かめた: vm.md:461)。
- **デッドロックの規則:** 予定の項は輪の項なので、`next_deadline()` は `Some` になる。空きのループは、デッドロックを報告せずに時計をその期限へ進める。
  - 満ちると、待つタスクを起こすか、そのビットを `unheard` にして run を止める (130 か 143)。
  - そのため、印によって、デッドロックになるはずの run が `terminated` か `interrupted` になることがある (UI テスト `signal_instead_of_deadlock.em`)。
  - `main` の `Finish` までに満ちなかった項は捨てる。`Finish` は輪を見ない (確かめた: vm.md:455)。
- **本物の driver:** 同じように扱う。これも仮想のシグナルであり、handler と `watched` と監視のスレッドを通らない。
- **RunStats:** 予定の項は `timers` に数えない。

### 1.6 単位の順と共有するファイル

全体の順は Net-U1 → **Sig-U0 → Sig-U1** → Net-U2 → **Sig-U2** → Net-U3..U6 → **Sig-U3 → Sig-U4 → Sig-U5** である。

Net の設計と Signal の設計の両方が編集するファイル:

| ファイル | Net の単位 | Sig の単位 |
|---|---|---|
| `eml_io/src/real.rs` の `submit`、`eml_io/Cargo.toml` (`libc`) | U2 | U0 |
| `eml_vm/src/monitor.rs` (`spawn_masked` の移動) | U2 | U2 |
| `eml_vm/src/cells.rs` | U3–U5 | U3 |
| `crates/eml_cli/tests/ui.rs` (`seeded_label`) | U1 | U3 |
| `crates/eml_vm/tests/io.rs` | U6 | U3 (種類 2 のみ) |
| vm.md の「時計と driver」と「上限と割り込み」 | 両方が編集 | 両方が編集 |
| `std/Async.em` | U6 (`io_wait` / `io_wait_until` を除く場合) | U3 |

## 2 理由

- **handler は `watched` で決める。** 待つタスクがなければ動きは今のままで、`EINTR` による止まりも働く (確かめた: host.rs:1147, 1155)。
- **保留と中継済みを1つの語に持つ。** 1回の出来事に知らせを1つだけ出す。知らせが重なって run を `unheard` で誤って止めることも、起こしの取りこぼしもない (推定)。
- **保留は届けるときに消す。** 採用した Ctrl-C の規則であり、SIGTERM にも同じ規則を使う。defer されたシグナルが詰まりうる経路はすべて、2回目で強制終了できる。
- **`NOTICE` は切り替えずに、忙しい worker を扱う。** 忙しい worker は次の安全点で集める。その後の2回目のシグナルは、`watched` が空なので run を止めるか、1回目がまだ保留なら強制終了する。
- **doorbell と waker** で、async.md の決定 #18 を実装する。handler が監視のスレッドを起こし、監視のスレッドが届けて worker を起こす。`Machine` ごとの doorbell なら、1つのプロセスにいくつ `Machine` があっても正しい。
- **SIGINT と SIGTERM だけを塞ぐ** のが、vm.md:372 の不変条件を戻す最小の変更である。

## 3 退けた案

- プロセス全体で共有する doorbell: 1つのプロセスの監視のスレッドが互いの ring を奪う。
- シグナルだけを見て 10 ms ごとに回る監視: 決定 #18 から外れる。doorbell がないときの代わりとして残す。
- 中継が `preempt()` を呼ぶ: Sequential が壊れる。`NOTICE` は切り替えずに集める。
- 忙しい worker はシグナルを無視してよいとする: プロセスを管理する側が送る SIGTERM が失われる。
- run の終わりに、残った保留のシグナルを立てる: 読む者がいない。
- プールのスレッドですべてのシグナルを塞ぐ: 同期のフォールトの動きが未定義になる。
- handler が常に defer する: `EINTR` による止まりが壊れる。
- std の hook、CLI のフラグ、`TestDriver` の状態としての予定: 前提が除いており、Net の設計は driver に時間の出来事を足さない。
- 本物の driver で予定を拒む: 得るものがない。
- REPL の入力ごとに予定を再生する: 絶対の時刻がもう過ぎている。
- 待つタスクを1つに限る、またはキューにする: 終了の通知は全員に広がり、POSIX もシグナルをまとめる。
- Windows の CLOSE / SHUTDOWN を今入れる: CI で試せない。
- 仮想の driver で本物のシグナルを届ける: テストの driver は眠れず、seed の run に迷い込んだシグナルを見せてはならない。
- REPL に SIGTERM の handler を置く: プロンプトでは止める語を読む者がいない。

## 4 前提にした仮定

- UI の run は仮想の driver (`RunConfig::default()`) を使う (確かめた: ui.rs:184-187; lib.rs:124)。
- Net-U1 が `seeded_label` を「… on the virtual driver」に書き換える。Sig-U3 がそこに追記する。
- compio の poll バックエンドがプールに仕事を渡すのは、driver のスレッドの `push` からだけである (推定)。Sig-U0 のテストが確かめる。
- `Async` を使う bench のプログラムはない (確かめた)。

## 5 実装への影響

### 制約

- **eml_rt の関門 (Sig-U1):** `cargo verus verify -p eml_rt` と `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')`。欄は `verus!` の中の `Control` と `Control::new` に置き、メソッドは `raise` の隣に置く。信頼する一覧への行は足さない。これらは安全な `Control` のアクセサだからである (確かめた: object-model.md:1239)。一覧の文言は広げる。
- **extern:** `Std.Async.park_signal` には、`eml_extern` の行 (held の引数)、std の宣言、`crates/eml_core_ir/tests/externs.rs` の Repr の対応が要る (確かめた: :1077 が `park_time` を挙げている)。`std/Signal.em` を `eml_hir::STD` に足す。
- **日本語の文:** 5.4 の下書きには natural-japanese のスキルを当てる。

### 5.1 単位

**Sig-U0. プールのスレッドが SIGINT と SIGTERM を塞ぐ** (eml_io)

- Net-U1 が入っていること。
- 変更: `Op::Job` の `push` を包む guard (unix)。`eml_io/Cargo.toml` に `libc` (unix) を足す。
- 文書:
  - vm.md 「上限と割り込み」: 塞いだスレッドの段落に、driver のプールのスレッドが SIGINT と SIGTERM を塞ぐことと、その理由を書く。Net-U2 がこの段落に名前解決のスレッドを足す。
  - architecture.md の `libc` の行に `eml_io` を足す。
  - architecture.md の compio を上げるときの確認表に、「プールのスレッドは `push` を呼んだスレッドから生まれ、シグナルの塞ぎを受け継ぐ」の行を足す。
- テスト (新規): `crates/eml_io/tests/real.rs` の `#[cfg(unix)] pool_threads_block_interrupt_and_terminate`。`Job` が `pthread_sigmask(SIG_BLOCK, null, &mut set)` で自分のマスクを読み、SIGINT と SIGTERM がどちらも入っていれば 1 を返す。1 を検査する。
- 次が通れば完了:
  - `cargo nextest run -p eml_io`
  - `cargo clippy --all-targets -- -D warnings`
  - `cargo fmt --check`
  - `nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings`

**Sig-U1. 待つタスクのない TERMINATE、NOTICE、SIGTERM** (eml_rt、eml_vm、eml_cli)

- Sig-U0 が入っていること。
- 変更:
  - `Control` への追加すべて (`TERMINATE`、`NOTICE`、`Stop::{Terminated, Notice}`、`signals`、`watched`)。
  - `RuntimeError::terminated`、`stops_the_run`。
  - `polled_stop` は `Notice` を「ビットを下ろして続ける」として扱う。まだ立てる者はいない。
  - `respond` の手順 1 から 3。ring は Sig-U2 で入る。
  - Run での SIGTERM、143、`_exit(143)`。
- 文書:
  - object-model.md 「実行の制御」: 止める語 (`TERMINATE`)、worker の語 (`NOTICE`)、`signals`、`watched` の行。優先順。`clear` / `begin_run`。「8 は空けてある」を除く。
  - vm.md 「上限と割り込み」: SIGTERM、手順、143、強制終了。
  - architecture.md:381 の終了コード。
- テスト (新規):
  - eml_rt の heap/tests/ctx.rs:101 の隣: `poll` が `Terminated` を返すこと。`Notice` を含む優先順。`claim_signals` が新しいビットを1回だけ返すこと。`settle`。`take_pending`。`clear` と `begin_run` の効果。
  - interrupt/mod.rs:
    - `a_watched_ctrl_c_is_deferred`
    - `a_ctrl_c_while_one_is_pending_ends_a_run`
    - `a_ctrl_c_after_a_settled_one_is_a_first_one`
    - `a_watched_terminate_is_deferred`
    - `an_unwatched_terminate_raises_terminate`
    - `a_terminate_while_one_is_pending_ends_a_run_with_143`
    - `a_terminate_after_a_raised_one_ends_a_run_with_143`
    - `the_two_kinds_do_not_arm_each_other`
    - `the_repl_defers_a_watched_ctrl_c_and_never_exits`
  - windows.rs: `a_watched_ctrl_c_is_handled_without_exit`。
  - cli.rs の `#[cfg(unix)]`:
    - `a_terminate_ends_an_idle_program_with_143`: `Async.run` の中で `eprintln "ready"` と `sleep_ms 600000` を行う。`ready` を読んだ後に SIGTERM を1回送る。終了コードは 143 で、stderr は `runtime error: terminated\n` で始まる。
    - `repeated_terminates_end_a_program_waiting_for_the_standard_input_with_143`: cli.rs:922-960 の型で、100 ms ごとに SIGTERM を送る。検査は終了コード 143 だけにする。
- 次が通れば完了:
  - `cargo verus verify -p eml_rt`
  - verusfmt の確認
  - `cargo nextest run`
  - `cargo clippy --all-targets -- -D warnings`
  - `cargo fmt --check`
  - cross の clippy
  - `cargo nextest run -p eml_cli --test integration -E 'test(cli::a_terminate) | test(cli::repeated_terminates)'`

**Sig-U2. doorbell、`Scheduler` の中の監視、中継、`NOTICE` の収集、waker** (eml_vm、eml_cli)

- Net-U2 が入っていること (`spawn_masked` が移り、`real.rs` が書き換わっている)。
- 変更:
  - 1.3 の「doorbell」「監視」、`Collect` の経路、`drain` の `Notice::Signal` (列が空なら、すべてのビットが `unheard` になる)、`Scheduler` の `Drop`。
  - `eml_cli` は doorbell を作って `Installed` と設定に渡す。`respond` が ring する。
- 文書:
  - vm.md 「タスクとスケジュール」: `Scheduler` が監視を持つこと、2つの起動の契機と役目、切り替えない `NOTICE` の収集を書く。「ビットを写さず、worker を起こさず、シグナルを届けない」を除く。
  - vm.md 「時計と driver」の `IDLE_TICK` の段落: 監視のスレッドが waker を鳴らす。
  - architecture.md の `libc` の行に「自己パイプ (`Doorbell`)」を足す。
- テスト:
  - `Doorbell` の単体テスト: `a_ring_wakes_a_waiter`、`rings_merge` (ring を2回行う。1回の wait で空になり、次の wait は時間切れになる)、`two_doorbells_do_not_share_rings`。
  - 監視のテスト: `a_claimed_signal_is_relayed_once`、`nothing_pending_relays_nothing`。
  - 新しい `crates/eml_vm/tests/signal.rs` (tests/main.rs に宣言する):
    - `a_relayed_signal_without_a_waiter_stops_the_run`: 本物の driver、`Sched::Normal`、doorbell なし。`Async.run (fn () -> Async.sleep_ms 2000)` を走らせる。`run` の前に `control.defer(TERMINATE)` を呼ぶ。`begin_run` はこれを消さないので残る。`terminated()` を検査する。3 秒の番犬は失敗の条件にだけ使う。
  - **種類 1:** `monitor::tests::dropping_the_policy_stops_the_monitor` を削除する (確かめた: monitor.rs:149-162)。同じ `strong_count` の確認で、`eml_io::TestDriver::new(None)` で作った `Scheduler` を使う `dropping_the_scheduler_stops_the_monitor` を足す。
  - ほかはすべて期待値がバイト単位で変わらない (種類 3)。
- 次が通れば完了:
  - `cargo nextest run -p eml_vm --test integration bench` (スナップショットの変更なし)
  - `cargo nextest run -p eml_vm --test integration signal::`
  - `cargo test -p eml_vm --lib doorbell` (1つのプロセスなので、テスト間で共有がないことが分かる)
  - `cargo nextest run -p eml_vm --lib monitor::`
  - 全テスト、clippy、fmt、cross の clippy

**Sig-U3. `Std.Signal`、仮想の予定、UI の印** (std、eml_extern、eml_hir、eml_vm、eml_cli のテスト)

- Net-U6 が入っていること (`Std.IoTest` が除かれ、io.rs と `async/` の UI テストが書き換わっている)。Net-U1 の `seeded_label` も入っていること。
- 変更:
  - 1.2。`Wait::Signal`、`signal_waiters`、`Due` (既存の輪への挿入をすべて `Due::Task` に直す)、待つタスクがある場合の `deliver`。
  - `park_signal`: suspend、`parks += 1`、`point`、push。監視はまだ始めない。
  - `unpark` / `expire`。`sweep_all` は列を空にする。`collect` と `Finish` での `unheard` による止まり。
  - 1.5 の API。
- **UI の印** (パーサは `crates/eml_cli/tests/common/mod.rs` の `windows_skip` の隣):
  - 印の行は、エントリのファイルの先頭にある、`<leader> windows:` か `<leader> signal:` で始まる行の最長の並びである (shebang の行があればその後)。今は shebang を除く最初の1行だけを読む (確かめた: common/mod.rs:76-85)。1つのファイルに両方の種類を、この並びの中の任意の順で書いてよい。
  - シグナルの印は `-- signal: <interrupt|terminate> at <ms>` である。小文字、`ms` は `u64` の10進の数字だけ、空白は1つずつとする。例は `-- signal: terminate at 100`。1行に1つで、一覧の順は行の順である。
  - 形の合わない `-- signal:` の行は、``the signal mark is `-- signal: <interrupt|terminate> at <ms>`, not `…` `` で panic する。
  - シグナルの印を許すのは、`tests/ui/run/` と `tests/ui/run-fail/` の `.em` のエントリだけである。ほかの場所 (check-fail、`# signal:` と書いた `.repl`) では `signal marks are not supported here` で panic する。
  - REPL のトランスクリプトはシグナルに対応しない。
  - `execute_in` は、ファイルのすべての run に `.with_signals(list.clone())` を足す。検査モードの逐次、速いモード (`Sched::Normal`)、各 seed の run がそれにあたる。
  - 一覧が空でないとき、`seeded_label` は ` with the signal marks of the file` を末尾に足す。
- 文書:
  - stdlib.md: 新しい「`Std.Signal`」と、「`Std.Async`」の ABI (op、export、extern、held の引数)。
  - vm.md 「待ちの2種類」、「時間の待ち」(`Due`。「値は待つタスクのセルの番号だけ」を変える。予定)、「取り消しの届け方」、「`RunStats`」(`parks` に `park_signal` を足す)。
  - modules.md 「標準ライブラリ」。
  - testing.md 「OS ごとのテスト」(先頭の印の行) と「UI テスト」(シグナルの印)。
- UI テスト (新規。開始か spawn の後に `spin 40` を置く。出力するタスクは一度に1つ。時刻は重ならないようにする):
  - `tests/ui/run/async/signal_race.em`: `terminate at 10`。`sleep_ms 1000` と `Signal.wait Terminate` を競わせる。
  - `signal_many_waiters.em`: `interrupt at 5`。2つのタスクが待ち、順に await する。
  - `signal_repeat.em`: `interrupt at 5` と `interrupt at 10`。1つのタスクが2回待つ。
  - `signal_cancelled_wait.em`: `interrupt at 20`。`timeout 5 (Signal.wait Interrupt)` が `None` を返す。その後の新しい待ちは成功する。
  - `signal_scheduled_keeps_the_run_alive.em`: `terminate at 50`。待ちは `Signal.wait Terminate` だけである。
  - `tests/ui/run-fail/async/signal_terminate_unheard.em`: `terminate at 5`。1つのタスクが `sleep_ms 100` を走らせる。`terminated` になる。
  - `signal_interrupt_unheard.em`: `interrupted` になる。
  - `signal_wait_deadlock.em`: 印なし。`Signal.wait Terminate` だけがある。デッドロックの誤りになる。
  - `signal_instead_of_deadlock.em`: `terminate at 50`。1つのタスクが、誰も送らない `Chan.recv` で待つ。デッドロックの誤りではなく `terminated` になる。
- ハーネスの単体テスト (新規。ui.rs): `signal_marks_are_parsed_in_order`、`a_malformed_signal_mark_panics`、`a_windows_mark_and_signal_marks_can_share_the_leading_lines`、`a_signal_mark_in_a_transcript_panics`。`the_windows_mark_is_the_first_line` は今のケースを保つ。パーサの署名が変わるなら、このテストの書き換えは種類 3 である。
- **種類 2 の値は、この単位の中で一度に決める:**
  1. 最初のコミットの中の最初の段階は、`Async.em` の op とそのコメント行、`start_task` の節、`park_signal` の extern の宣言と行、`park_signal` の VM の処理だけにする。コンパイルできる最小の変更である。
  2. `run` の本体の行の、古い行番号と新しい行番号を求める。
     - `git show HEAD:std/Async.em | grep -n '^run f = '` が OLD を返す。
     - `grep -n '^run f = ' std/Async.em` が NEW を返す。
  3. スナップショットの集合: `git grep -l "<std>/Async.em:OLD:" -- crates/eml_cli/tests/snapshots crates/eml_vm/tests`。これらが書き換えるファイルである。許す差は `<std>/Async.em:OLD:9` → `<std>/Async.em:NEW:9` だけである (`cargo insta review` がほかを見せてはならない)。
  4. RunStats の集合: この段階で `cargo nextest run --no-fail-fast` を走らせる。失敗した `RunStats` の検査すべてが、種類 2 の集合 K である。K に入ってよいのは、プログラムが `Async` を使うテストだけである。`Async` を使わない固定値が K に入ったら不具合である。K のテストの名前と欄の差を、コミットメッセージに記録する。
  5. 単位を仕上げる。新しいテスト以外の固定値をこれ以上変えたら、種類 2 ではなく不具合である。
- 次が通れば完了:
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::) | test(repl::)'` (seed を含む)
  - `cargo nextest run -p eml_core_ir --test integration externs`
  - `cargo insta review` (段階 3 の差だけ)
  - 全テスト、clippy、fmt、cross の clippy

**Sig-U4. 本物の配送** (eml_vm、eml_cli)

- Sig-U3 が入っていること。
- 変更: `real_driver` のときの `park_signal` での監視の起動。`relay_on`。`set_watched` の同期。`listening` の空きの規則。
- 文書:
  - vm.md 「時計と driver」の空きのループ。
  - vm.md 「上限と割り込み」: 中継、settle、run の終わり。
  - stdlib.md の本物の driver の箇条書き。
- テスト (新規):
  - `crates/eml_vm/tests/signal.rs`:
    - `a_pending_signal_reaches_a_waiter_through_the_monitor`。本物の driver と新しい `Doorbell` で、`Sched::{Sequential, Normal, Seeded(1)}` のそれぞれについて走らせる。テストのスレッドは `control.watched() & TERMINATE != 0` になるまで回り (3 秒の番犬)、`defer(TERMINATE)` と `doorbell.ring()` を呼ぶ。`Ok` と stdout `got\n` を検査する。
    - `a_waiter_keeps_an_otherwise_idle_run_alive`: 同じ形で、待ちがシグナルだけである。
    - `a_scheduled_signal_fires_on_the_real_driver`: 新しい `Machine`、`with_signals([terminate at 20])`、待つタスク1つ。`Ok` と、`run` の直前から測った経過時間が 20 ms 以上であることを検査する。下限だけを見るのは、時間の規則による。
    - `an_unheard_scheduled_terminate_stops_the_run`: 本物の driver、待つタスクなし。`terminated()` を検査する。
    - `a_busy_worker_collects_a_signal_and_a_second_one_stops_it`: `Sched::Sequential`、本物の driver、新しい `Doorbell`。spawn したタスクが `Signal.wait Terminate` で待ち、これが監視を始める。main は `yield_now` を1回呼んで待つタスクが待ちに入れるようにし、その後は yield せずに回り続ける。
      1. テストのスレッドは `watched() & TERMINATE != 0` になるまで待ち、`defer(TERMINATE)` と `ring()` を呼ぶ。
      2. `watched() == 0` になるまで待つ。これが、main が忙しい間に `NOTICE` の収集が行った配送である。
      3. `control.terminate()` を呼ぶ。`watched() == 0` のときの `respond` の動きと同じである。
      4. `terminated()` を検査する。
      - 各待ちに 3 秒の番犬を置く。`NOTICE` がなければ段階 2 は起きない。
  - cli.rs の `#[cfg(unix)]`:
    - `a_terminate_reaches_a_waiting_task`: `--sched sequential`。spawn したタスクが `Signal.wait Terminate; eprintln "got"` を走らせる。main は `yield_now`、`eprintln "ready"` の後に await する。`ready` の後に SIGTERM を送る。終了コード 0 と stderr `ready\ngot\n` を検査する。
    - `an_interrupt_after_a_delivered_one_ends_the_program_with_130`。
    - `a_second_ctrl_c_ends_a_program_whose_waiter_cannot_be_reached`: 待つタスクが1つあり、main は stdin を開いたまま `Stdin.read_line` でブロックする。`ready` の後に 100 ms ごとに SIGINT を送る。130 を検査する。
    - `a_second_terminate_ends_a_program_whose_waiter_cannot_be_reached`: 同じ形で SIGTERM を送る。143 を検査する。
- 次が通れば完了:
  - `cargo nextest run -p eml_vm --test integration signal::`
  - `cargo test -p eml_vm --test integration signal::` (1つのプロセスなので、doorbell の共有がないことが分かる)
  - `cargo nextest run -p eml_cli --test integration -E 'test(cli::)'`
  - 新しいテストが1つずつ 1 秒以内: `cargo nextest run -j 1 -E 'test(signal) | test(terminate) | test(cli::a_second) | test(cli::an_interrupt_after)'`
  - 全テスト、clippy、fmt、cross の clippy

**Sig-U5. 文書の締め**

- Sig-U4 が入っていること。
- 変更:
  - status.md: `Std.Signal`。Windows に `Terminate` がないこと。
  - roadmap.md: waker の項を「10 ms の上限をなくす」にする。`net-windows` に「`Terminate` を CTRL_CLOSE_EVENT と CTRL_SHUTDOWN_EVENT に写す」を足す。
  - async.md: 決定 #18 の文 (5.4)。「シグナル」を Windows に残る分だけに削る。「テストと決定性」には、仮想のシグナルがハーネスの `RunConfig` の予定から来ることを書く。
  - repl.md: SIGTERM は扱わない。
  - testing.md 「テストの置き場所」: `eml_vm/tests/signal.rs` と cli.rs のシグナルのテスト。
- `cargo nextest run -p eml_cli --test integration citations::` が通れば完了 (確かめた: citations.rs が文書の引用を検査する)。全テストも通す。
- ステージを閉じるとき: benchmarks.md の表を更新する (`bench/run.sh`、`bench/rt.sh`、`bench/scripts.sh`。macOS、きれいな木)。

### 5.2 テストの変更の一覧

- **種類 1:** `dropping_the_policy_stops_the_monitor` を `dropping_the_scheduler_stops_the_monitor` に置き換える (Sig-U2)。
- **種類 2:** Sig-U3 だけにある。
  - 段階 3 で見つけたファイルの `<std>/Async.em:OLD:9` → `NEW:9` の書き換え。
  - 段階 4 で見つけた集合 K の RunStats の固定値。
  - 理由: op は `run` より上に置かなければならない (確かめた: Async.em:75-77 の置き場所の規則)。
- **種類 3:** 印のパーサの整理 (ケースも期待値も同じ)。そのほかはすべて、新しいテストか、バイト単位で変わらないものである。

### 5.3 実装者に任せること

Rust の名前の細部。`Registry` の配置。`Doorbell` のフィールド。パーサの戻り値の型。UI テストの本体と出力の文言。`Arc<Inbox>`、waker、doorbell を `Monitor::start` へ渡す配線。

### 5.4 仕様の文の下書き

async.md 決定 #18:

```
シグナルの handler は `Control` のビットを立て、自己パイプで監視のスレッドを起こす。監視のスレッドは、`Signal.wait` で待つタスクのセルの持ち主の受け箱へ知らせを積み、worker が次の安全点で集めるよう印を立てて、driver の waker を鳴らす。待つタスクがなければ、`INTERRUPT` は今と同じく致命的で、`TERMINATE` は同じ止め方で run を止めて 143 で終わる。`eml run` で、前の同じシグナルをまだ届けていないか、それがもう run を止めている間に来たシグナルは、その場で終わらせる (Ctrl-C は `_exit(130)`、SIGTERM は `_exit(143)`)
```

stdlib.md 「`Std.Signal`」:

```
## `Std.Signal`

- `data Signal = | Interrupt | Terminate` は `Eq` と `Show` を持つ。`Interrupt` は Ctrl-C で、unix の SIGINT と Windows の `CTRL_C_EVENT` である。`Terminate` は unix の SIGTERM である
- `wait : Signal -> <IO, Async> Unit` は、そのシグナルが来るまでタスクを待たせる。取り消しの地点で、取り消されると IO の失敗 `Prelude.Cancelled` を投げる
- シグナルが来ると、そのとき同じシグナルを待つタスクをすべて、待ち始めた順に起こす。シグナルは貯めない。待つタスクがなければ既定の動きをする。`Interrupt` は run を止めて終了コードを 130 にし、`Terminate` も同じ止め方で 143 にする ([VM](vm.md) の「上限と割り込み」)。起こされてから次の `wait` までに来たシグナルも既定の動きをする
- 届ける前に同じシグナルが続けて来ると、1つにまとまる。`eml run` では、前の同じシグナルをまだ届けていない間に来たシグナルは、その場で終わらせる。届けた後のシグナルは1回目として扱う
- worker が別のタスクを動かしていても、シグナルは次の安全点で届き、待つタスクは列の後ろに入る。同期の読みで worker が待つ間に来たシグナルは、読みが戻ってから届く
- 本物のシグナルが `wait` に届くのは、本物の driver の run だけである。テストの driver の run では、本物のシグナルはいつも既定の動きをする。テストは仮想のシグナルを `RunConfig` の予定で起こす ([VM](vm.md) の「時間の待ち」)
- `eml repl` は SIGTERM を受けない。Windows には `Terminate` を起こす OS の出来事がない
```

vm.md 「時間の待ち」に足す箇条書き:

```
- 仮想のシグナルは `RunConfig::with_signals` か `Machine::with_signals` の予定で起こす。予定は次の run だけに渡し、run の始めに時間の輪へ載せる。期限は driver を作ったときからのミリ秒で、`Machine` は最初の run の始めに driver を作る。並びの順に登録するので、同じ期限ではプログラムのタイマーより先に満ちる。輪の項なので、予定が残る間はデッドロックにならない。満ちると、待つタスクを起こすか、待つタスクがなければ run を止める。`main` の `Finish` までに満ちなかった予定は捨てる。`timers` には数えない
```

testing.md 「UI テスト」に足す箇条書き:

```
- `run/` と `run-fail/` のエントリの `.em` は、先頭の印の行に `-- signal: <interrupt|terminate> at <ms>` を書いて、仮想のシグナルを予定できる。1行に1つで、書いた順に予定する。先頭の印の行は、shebang の後に続く `-- windows:` か `-- signal:` で始まる行の並びで、2種類を混ぜてよい。ハーネスは同じ予定で、検査モードの run、速いモードの run、seed の run のすべてを流す。形の合わない `-- signal:` の行と、`check-fail/` とトランスクリプトの中の印は panic する
```

## 6 決めていない点と既定

1. **採用した Ctrl-C の規則に従う対称な SIGTERM の規則。** `eml run` で、前の SIGTERM をまだ届けていない間か、`TERMINATE` がすでに止める語に立っている間に来た SIGTERM は `_exit(143)` である。既定: 採る。
2. 解釈 (確度は低い): 「待つタスクがなければ既定の動き」を決めるのは、監視ではなく handler である。worker は `unheard` を通して同じ確認を繰り返す。
3. 範囲外の Int を渡した `Async.signal_wait`: `fail_io NotAvailable "no such signal"`。
4. 同じ `ms` に来る予定のシグナルとプログラムのタイマー: 予定のシグナルが先に満ちる。テストのファイルは同時刻を避ける。
5. run の最後の集める地点の後に defer されたシグナル: run が終わることで消費される。`eml run` は run 自身の終了コードで終わる。

## 7 追加で確かめたこと

- compio-driver 0.12.5 では、`AsyncifyPool::dispatch` が `std::thread::spawn` でスレッドを作る (確かめた: asyncify.rs:113-130)。poll の `push_blocking` は driver のスレッドから dispatch する (確かめた: poll/mod.rs:372-392)。
- `polled_stop` が特別に扱うのは `Preempt` だけで、`take_preempt` の後に `poll` をやり直す (確かめた: vm.rs:343-361)。`Notice` も同じ場所に入れる。
- `Machine` は `run_entry` の中で、`Start` の前に driver を作る (確かめた: lib.rs:419-436)。
- `common/mod.rs:62-109` は先頭の1行の `windows_skip` である (確かめた)。`ui.rs:174-187` は `execute_in` の設定である (確かめた)。`seeded_label` は ui.rs:274 にある (確かめた)。
- `add_task` はすべてのタスクの入口で走る (確かめた: vm.rs:2245)。`citations.rs` はある (確かめた)。
- ビルドもテストも走らせていない。`start_task` に節を1つ足すと `RunStats` の固定値が動くかどうかは測っていない。Sig-U3 の段階 4 が決める。
