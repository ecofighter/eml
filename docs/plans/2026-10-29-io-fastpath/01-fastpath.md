# 付録1: io-fastpath の細部 (trait、VM、std、テスト、spec の文案)

この付録は、本体の T-1 から T-11 の細部を持つ。本体と食い違うときは本体に従う。

- 行番号は 41b4f3d (基点) で読んだものである。各タスクで動くので、関数、テスト、節の名前で引く。
- コード片のコメントと doc は、写すときに日本語の規則を当て直し、`docs/` の節を引く形にする。計画の番号 (D-x、T-x) と `docs/plans/` は書かない。
- 「試した」は、計画を作るときに基点の写し (scratch) の上で組んで流したことを表す。試しのファイルはリポジトリに残らないので、要る中身はここに書き写してある。

## 1. `IoDriver` の trait の差分 (`crates/eml_rt/src/driver.rs`、T-4)

差は3つで、T-4 の1つのコミットでだけ変える。io-backends 以後も成り立つ文だけを doc に書き、compio に固有の振る舞い (プールが満ちると `submit` が待つこと、`quiesce` の 5 秒) は trait の doc から外して `real.rs` と vm.md に移す。

1. `submit` は呼ぶ側の鍵を受け、その場で終わったかを `Submit` で返す
2. `Op::Read` は「今すぐ読みを試し、ブロックするなら読めるまで待ってから1回読む」1つの操作で、`Op::Readable` は状態の問いである
3. `wait` は内部の刻みを持たない。OS を見るのは `wait` だけで、`completions` は OS を見ない

変える項目の新しい形 (ほかの項目、`Completion`、`Done`、`IoWaker`、`HandleBin` と、`verus!` の外に置いて `external_body` を持たないことは変えない):

```rust
/// 呼ぶ側が振る操作の名前。1つの driver の中で、保留の操作 (`submit` が `Submit::Pending` を返し、まだ
/// `completions` で返していない操作) の鍵は互いに違う。`Submit::Done` を返した鍵と、完了を `completions` で
/// 返した鍵は、すぐに次の操作に使い回してよい。
pub type OpKey = u64;

/// カーネルが読み書きする記憶。driver を持つスレッドの外へは出さないので `Send` ではない。`cap == 0` は
/// 記憶がないことを表し、`ptr` は空でない任意の番地で、読みも書きもしない。貸した側は、`submit` が
/// `Submit::Done` を返すか、その操作の完了を `completions` から取り出すまで、物体を生かし、動かさない。
/// `Read` に貸した記憶は一意に保ち、`Write` に貸した記憶は書き換えない。
#[derive(Clone, Copy)]
pub struct RawBuf { pub ptr: *mut u8, pub cap: usize }

pub enum Op {
    /// `handle` の次の読みがブロックしない状態 (読めるデータがあるか EOF) なら `Ok(0)` で終わる。状態の問いで、
    /// 辺ではない。受け付けたときにその状態なら、その場で終わってよい。
    Readable { handle: IoHandle },
    /// `buf.ptr[0..cap]` に1回読む。読めるデータか EOF があればその場で読み、なければ読めるようになるまで
    /// 待ってから読む。`Ok(n)` で終わり、n == 0 は EOF である。`cap` は 1 以上。
    Read { handle: IoHandle, buf: RawBuf },
    // Write、Listen、Accept、Connect、Job は今の doc のまま
}

/// `submit` の結果。
pub enum Submit {
    /// その場で終わった。完了は `completions` に届かず、`pending` にも数えない。
    Done(io::Result<Done>),
    /// 後で `completions` にちょうど1回届く。
    Pending,
}

pub trait IoDriver {
    /// `now() >= deadline_ms` か、完了が届くか、waker が鳴るまで待つ。OS を見るのは `wait` だけで、
    /// `deadline_ms` が今以前でも OS を1回確かめ、終わった操作を完了として届けてから戻る。ほかの理由で早く
    /// 戻ってよいが、内部の刻みでは戻らない。`Ok(false)` を返すのは、`deadline_ms` が `None` で、保留の操作も
    /// 届いた完了もないときだけである。`deadline_ms == u64::MAX` は期限なしで待つが、`Ok(false)` は返さず、
    /// waker が鳴れば `Ok(true)` で戻る。`Ok(false)` は外からの入力なしには何も進まないこと、`Err` は driver が
    /// 壊れたことを表す。
    fn wait(&mut self, deadline_ms: Option<u64>) -> io::Result<bool>;
    /// 失敗しない。出せなかった操作は `Submit::Done(Err(_))` で返す。`Op::Job` はいつも `Pending` を返す。
    /// 保留の鍵と同じ鍵で呼んではならない。
    fn submit(&mut self, key: OpKey, op: Op) -> Submit;
    /// 呼んだ時点で保留の鍵 `key` の操作だけを、できるだけ取り消す。保留でない鍵には何もしない。後で同じ鍵で
    /// 出した操作に効いてはならない (取り消しを後で OS に出す driver は、出す時点で同じ操作であることを確かめる)。
    /// 保留の操作の完了は、取り消しで終わったか自分で着いた結果で、それでも1回届く。壊れた driver では届かない
    /// ことがあり、そのとき `quiesce` は保留を残して戻る。`Op::Job` には何もしない。
    fn cancel(&mut self, key: OpKey);
    /// `wait` (と `cancel` の即時の取り消し) が届けた完了をすべて `out` に移す。OS を見ず、保留の表を歩かない。
    /// 順は driver が決め、呼ぶ側は鍵で記録を引く。
    fn completions(&mut self, out: &mut Vec<Completion>);
    /// `Pending` を返して、まだ `completions` で返していない操作の数。
    fn pending(&self) -> usize;
    /// 保留の操作をすべて取り消し、完了がすべて届くまで待って捨て、捨てた完了の中身のハンドルと `bin` の
    /// ハンドルを閉じる。仕事は待ちきり、名前の解決は待たない。driver が壊れているときは保留を残して戻る
    /// (呼ぶ側は `pending()` で確かめる)。
    fn quiesce(&mut self);
    // now、waker、bin、shutdown_write、set_nodelay、close は今の doc のまま
}
```

- `crates/eml_rt/src/lib.rs` の `pub use driver::{...}` に `Submit` を足す。
- `wait` の `u64::MAX` の扱いは、`Ok(false)` の確かめの後に期限なしへ写す。順を逆にすると、保留のない `wait(Some(u64::MAX))` が `Ok(false)` を返し、VM の空きのループ (`switch_away` の `deadline.or(Some(u64::MAX))` の道) がデッドロックの誤りで止まる。
- doc に `trust.rs` の `PATTERNS` の語 (とくに `global`、`axiom`、`uninterp`、`assume(`、`admit(`) を書かない。`trust.rs` は `driver.rs` を走査する。`TRUSTED` に行を足さない。

## 2. VM の変更

### 2.1 鍵と登録表 (`cells.rs`、T-4)

- `OpKey` は記録の `CellRef::encode()` (`(generation << 32) | index`) にする。`OpRecord::key`、`Registry::ops: HashMap<OpKey, CellRef>`、`NO_KEY` を消す。
- 完了は `CellRef::decode(key)` で `live` を引き、`CellKind::Op` で段階が `Done` でなければ当てる。合わなければ内部の誤り `a completion names no op` にする。
- 保留の数 `in_flight: usize` を登録表に置く。`Pending` を返した受け付けで足し、`complete` が当てたときに引く。`complete` は `in_flight == 0` なら driver に触れない (今の `ops.is_empty()` の早い戻りの代わり)。driver の `pending()` を読まないのは、仮想呼び出しで、compio では解決の数の lock を伴うためである。
- `quiesce` の後、`sweep_all` の前に `in_flight` を 0 に戻す (捨てた完了は当てない)。
- 1つの記録は受け付けから取り出しまで1つの添字と generation を持ち、generation は放すときにだけ進む。`Probe` から `Read` への出し直しも、前の完了を取り出した後なので、保留の鍵は一意である。取り出しは `Done` を求めるので、放した記録への完了は来ない。
- 不変条件のコメント「`ops` の項は…」を `in_flight` の文に直す。

### 2.2 決着 `settle` (`cells.rs`、T-4)

`complete` の1件ぶんの本体を切り出す。

```rust
fn settle<M>(
    &mut self,
    ctx: &mut Ctx<M>,
    driver: &mut dyn IoDriver,
    depth_offset: usize,       // 型は今の `task.depth_offset` に合わせる
    op: CellRef,
    result: io::Result<Done>,
    cancelled: bool,
) -> Result<Option<Runnable>, CellFault>
```

- `settle` が持つもの: `bytes_commit`、`Done::Stream` と `Done::Listener` の `Foreign` の作成、`Probe` から `Read` への続き (貸して `driver.submit(key, op)`)、行の連鎖の `Again`、待つタスクの `Runnable` を作ること。`with_worker` は `settle` の中に残す。
- 続きの `driver.submit` が `Done` を返したら、同じ記録でループする (再帰しない)。`Pending` なら `in_flight += 1` で戻る。
- `submit_*` は記録を今までどおり組み立てて `alloc` し (鍵が決まる)、`driver.submit(key, op)` を呼ぶ。`Done` なら `settle` を呼ぶ。待つタスクはまだいないので `None` が返る。`Done(Err)` でも記録は `Done` で残し、包みの `finish` が放す。
- `submit_*` は新しい引数 `depth_offset` を取る。vm.rs は `task.depth_offset` を渡す。
- 受け付けたタスクの上で `settle` を呼ぶときは、記録の worker ID と今の ID が同じなので `with_worker` は切り替わらない (`submit_*` は `worker` を引数に取り、記録にそのまま書く)。`complete` から呼ぶときは切り替わる。どちらも `settle` の中の1つの `with_worker` で足りる。

### 2.3 即値の token (`cells.rs`、`vm.rs`、T-5)

- 消すもの: `OpToken` とその `Drop`、`Notice::OpDropped`、`token_op`、`OpRecord::token`、内部の誤り `an op token outlived its run` と `an op token was dropped before its take`。
- token を引く所を1つにする: `fn op_of(&mut self, raw: i64, worker: u32) -> Result<CellRef, CellFault>`。
  - 生きた `Op` のセルでなければ内部の誤り `an op names no record`
  - 記録の `worker` が呼んだタスクの論理的な worker ID と違えば内部の誤り `an op is used by another task`。今の `take_op` の `an op is taken by another task` はこれに入れて消す
  - 速いモードでは worker ID がいつも 0 なので、比べは検査モードでだけ効く
  - `op_ready`、`park_io`、`park_io_until`、`op_*`、`take_*`、`finish` のすべてがこれを通る
- `Runnable::ResumeIo::op` は `i64` にし、`schedule` は `Word::from_bits(op as usize)` で渡す。
- `park_io` と `park_io_until` は継続だけを預かる (今と同じ `holding(&[false, true])`)。
- vm.rs: Net と park の腕で token を `task.raw` と `set_raw` で読み書きする。
- 取り出さずに終わった記録は `sweep_all` が放す。`sweep_all` に新しい確かめは足さない (`exit` と誤りで終わる run は記録を残して正当である)。

### 2.4 速い道の行 `op_ready` (`cells.rs`、`vm.rs`、T-5)

`Std.Async.op_ready : OpId -> Int -> <IO> (Int, OpId)` (token を借りる行。`with_arg: Some(0)`)。結果は 0、1、2 の3値で、速い道の extern を1回にする。channel の `reserve_send` の「3 なら先に譲る」と同じ形である。

| 記録と地点 | 結果 | 数える欄 |
|---|---|---|
| 段階が `Done` でない | 1 (待つ) | 予算を1つ使う。ほかは数えない (`park_io` が数える) |
| `Done` だが、今のタスクの取り消しの地点が投げる | 1 (待つ) | 予算を1つ使う。ほかは数えない。`cancels` も数えない |
| `Done` で地点が投げず、予算が残る | 0 (続ける) | 予算を1つ使う。`parks += 1`。`ms > 0` なら `timers += 1` |
| `Done` で地点が投げず、予算が切れた | 2 (先に譲る) | 予算を1つ使う。`ms > 0` なら `timers += 1`。`parks` は数えず、続く `reschedule` の `ready` が数える。その再開は取り消しを数えない |
| 待つタスクがもういる | 内部の誤り `an op is awaited twice` | |

- 1 のときの `park_io` と `park_io_until` は今の数え方 (`parks`、`cancels`、`timers`) のままである。そのため `parks` は `io_wait` と `io_wait_until` の呼び出しの数、`timers` は今の定義 (地点で取り消されなかった正の期限) のままで、どちらもスケジュールに依らない。
- `completions` は受け付けで終わった操作と集めた完了の和である。driver の結果1つにつき1つ数える (確かめとその後の読みは2つ)。
- 予算は今の操作の予算 (タスクごとに 128。`check_cancelled` と同じ `spend_budget`) で、どの `io_wait` も1つ使う。始まるとき、操作の待ちでない再開 (`resumed`) のとき、予算を使い切った操作の待ちからの再開 (`resume_io`) のときに 128 に戻り、予算の残る操作の待ちからの再開では戻らない。操作の待ちが park するかはスケジュールで変わるので、終わった操作の待ちだけで予算を使ったり、park した待ちの再開で戻したりすると、予算の切れる位置と `parks` がスケジュールに依る。`io_wait` はどの道でも待ちを1つだけ数えるので、予算の切れる位置は `check_cancelled` が譲る `yield_now` の待ちに表れる。`io.rs` の `the_budget_runs_out_at_the_same_point_in_every_schedule` が、ループに `check_cancelled` を入れ、待ちの数の偶奇を変えた2つの形でこれを固定する。io の再開のたびに予算を戻す規則でも、終わった操作の待ちだけで予算を使う規則でも、逐次と seed の `parks` が割れて落ちる。
- `vm.rs` に `AsyncOpReady` の腕を足し、`Extern::is_async()` と、vm.rs の `Async` の命令にする行の一覧 (3090 行付近の、`Extern::AsyncStartRaw` から並ぶ腕) に足す。

### 2.5 空きのループの期限と監視のスレッド (`lib.rs`、`sched.rs`、`vm.rs`、T-3)

- `Machine` の `real_driver: bool` を `DriverKind { Real, Virtual, Injected }` に替え、`Start` と `Scheduler` に渡す。`Machine::with_driver` は `Injected`、`with_clock` と `with_sched` で作り直す driver は `clock` から `Real` か `Virtual` にする。
- `sched.rs`:
  - `const WAIT_CAP_MS: u64 = 10;` (名前に `IDLE_TICK` を使わない)
  - `fn begin_run(&mut self)`: `Real` で自己パイプがあれば `start_monitor()` を呼ぶ。`run_scheduled` が `Scheduler::new` と `schedule_signals` の直後に呼ぶ唯一の入口である
  - `fn wait_deadline(&self, deadline: Option<u64>) -> Option<u64>`: `switch_away` が `wait` に渡す値を作る唯一の関数
    - `Real` で自己パイプがあり、監視のスレッドが動いている: そのまま返す
    - `Virtual`: そのまま返す (テストの driver は時計を進めるので、上限を掛けると終わらない保留のある run がデッドロックで止まらない)
    - ほか (`Real` で自己パイプか監視のスレッドがない、`Injected`): `ceil_ms(now) + 10` との小さいほう。期限がなければ `ceil_ms(now) + 10`
  - `relay_on` は `driver_kind == Real` で読む
- `vm.rs` の `switch_away`: デッドロックの判定は上限を掛ける前の期限で行い、その後に `sched.wait_deadline(deadline)` を `wait` に渡す。`listening()` で `u64::MAX` にした期限にも上限を掛ける (`Real` で監視がないときだけ切れる)。`AsyncParkSignal` の腕の `sched.real_driver` を `driver_kind` で読む。コメントの「待ちは1回ごとに短く切り」を直す。
- T-3 を T-4 より前に置くのは、逆だと自己パイプのない `Clock::Real` の run (`api.rs` の `an_interrupt_wakes_a_sleeping_worker` は自己パイプを渡さない) が、compio の刻みが消えた後に割り込みで起きなくなるためである。
- `Injected` の driver には 10 ms ごとに期限つきの `wait` が届く。`TestDriver` のように `wait(Some(d))` で時計を進めて `Ok(true)` を返す driver を `with_driver` で渡すと、終わらない保留を持つ run はデッドロックで止まらず、時計を進め続ける。そのため、テストの driver を包む Rust のテストの driver (4.2 節の `Recorder`) にはデッドロックするプログラムを流さない。デッドロックのテストは `Machine` が作る `Virtual` の driver で書く。`with_driver(driver, Sleeps::Yes)` のような口は作らない (要るテストがない)。

### 2.6 集める地点が OS を見る頻度 (`vm.rs`、`sched.rs`、T-4)

`completions` は OS を見ないので、OS を見る頻度は VM の集める地点が持つ。driver の契約は1つのままで、将来の `Worker` やネイティブの呼び手は自分のループでこの方針を持つ。

- `collect` に引数 `look: bool` を足す。`look` なら、まず `sched.driver.wait(Some(floor_ms(now)))` を1回呼び (誤りは `VmFault::Driver`)、`since_poll = 0` にしてから、受け箱、`complete`、輪の順で今どおり集める。
- `sched.rs` の `Scheduler` に `POLL_EVERY: u32 = 16` (定数) と `since_poll: u32` (欄。run ごとに 0 から) を置く。

| 呼ぶ所 | `look` | 理由 |
|---|---|---|
| `switch_away` の頭 | `false`。その後、実行待ちの列が空でなければ `since_poll += 1` とし、`since_poll >= POLL_EVERY` なら `collect(true)` を呼び直す。列が空なら空きのループに入る | 直後に `wait` が見るか、走れるタスクがある |
| 空きのループの `wait` の後 | `false` (`since_poll = 0` にする) | 直前の `wait` が見た |
| `preempt` の頭、`preempt` のループの `NOTICE` | `true` | 横取りは1つのタスクが刻みの全体を使ったときだけ起こるので、計算の重いタスクが走る worker では、I/O の完了の遅れの上限は今と同じ刻み2つ分 (20 ms) である |
| 安全点の `NOTICE` (`collect_notice`) | `true` | 同上 |

- 期限は `floor_ms(now)` にする。`ceil_ms(now)` は今より最大 1 ms 先になり、compio の `poll` がその分眠りうるためである。`floor_ms` は今以前なので、`wait` は待たずに OS を1回見る。
- テストの driver の `wait(Some(d))` は、`d <= now` なら決着だけして時計を進めない (今の `max(now, d)` のまま)。
- 試した結果: 基点の上に `collect` の引数と5つの呼ぶ所、`since_poll` と `POLL_EVERY = 16`、テストの driver の `completions` から決着を外すこと、compio の `completions` から `poll(ZERO)` と `reap` を外して `wait` に移すことを入れると、`eml_vm` の結合テスト (io.rs の `RunStats`、bench.rs の snapshot、api.rs の本物の driver を含む 390 本) と単体 47 本、`eml_cli` の UI テストと REPL のトランスクリプト (201 本)、`eml_core_ir` と `eml_test_support` はすべて通った。落ちたのは、`completions` で決着か poll を待つ形の `eml_io` の driver のテスト 21 本 (`tests/test_driver.rs` の 19 本と `tests/real.rs` の `a_cancelled_connect_does_not_try_the_next_address`、`a_cancelled_long_write_stops_resubmitting`) だけだった。この試しは `IDLE_TICK` を消していないので、`live_lookups_beyond_the_limit_wait_in_order` は 5.2 節の直しを当てる。

### 2.7 1操作あたりの確保 (T-9)

- 消える確保: token の `Foreign` (1/操作、T-5)、速い道の継続の捕まえと再開の entry (perform しない、T-5)、`Readable` の後の 64 KiB の作り直し (使い回す buffer では 0、T-7)、書きの写し (T-8)、`read_line` の写し2回を1回に (T-7)。
- T-9 で足す: `collect` の `Vec::new()` を `Scheduler::runnables: Vec<Runnable>` の使い回しにする。残る確保を `--dump` と callgrind で見て、記録と std の包みの局所の組から消せるものを消す。消せない組は理由を計画書に書く。
- `(Wake, OpId)` は2つともスカラーになるので、組が構造体に下りて確保がなくなりうる (推定)。確保の数は T-9 で測る。

## 3. 貸し方 (`cells.rs`、T-7)

### 3.1 `read_chunk` と `read_chunk_within`

- `MIN_ROOM = 8` バイト。空きが 8 バイト未満の buffer は空きなしとみなす。`alloc_bytes` が長さを語に切り上げた余り (0〜7 バイト) を空きに数えないためである。
  - 試した値: `alloc_bytes` の空きは長さ 0、1、2、4、12、100、1000 で 0、7、6、4、4、4、0。`alloc_bytes_room(16)` は 16、`alloc_bytes_room(65 536)` は 65 536
  - 下限がないと、読む量が確保器の余りで決まり、`Bytes.of_string "> "` (空き 6) で読む UI テストの出力と syscall の数が変わる (`net_echo.em:29`、`net_read_line.em:47`、`net_read_chunk_within.em:43`、`api.rs:502` がこの形で読む)
  - 下限を大きく (4 KiB など) すると、小さい buffer を使い回す echo (16 バイト) が速い道を外れる
- `submit_read` は、buffer が一意で空きが 8 バイト以上あれば、空きのうち `min(空き, 65 536)` を `Op::Read` で貸す (段階 `Transfer`)。一意でないか空きが 8 バイト未満なら `Op::Readable` を出す (段階 `Probe`)。
- `Probe` の `Ok` で広げる。新しい容量は `長さ + max(長さ, 65 536)` で、貸すのは 65 536 バイトである。容量を読まずに長さだけで書くので、容量を読む関数を `eml_rt` に足さない。空きが 8 バイト未満の一意な buffer では容量 = 長さ + 余りなので、`max(2 × 容量, 長さ + 65 536)` とは最大 7 バイトしか違わない。一意でない buffer の写しは、広げるときの1回だけになる。
- `Readable` が受け付けで `Submit::Done` になれば、`submit_read` の中で `settle` が続けて貸し、`Read` を出す。
- 一意の判定は `is_unique` で、空きは一意なときだけ `bytes_lend` で読む (検査モードは一意でない buffer の `bytes_lend` を `NotUnique` にする)。
- `serve_rest` (読みすぎたバイトを返す道) は今どおり driver の操作を出さない。
- 呼び手が作った空き (`Bytes.with_capacity n`、使い終えた buffer の `truncate 0`) の大きさは容量で決まる。`Bytes.with_capacity 0` の空きは 0 で、`Readable` の道に進む。`n >= 1` で spec が約束するのは `容量 >= n` だけで、空きが 8 以上になるのは、確保器が容量を語の倍数に切り上げる実行時のふるまいである。docs には「`n >= 1` なら必ず 8 以上」と書かない。
- `ZERO_ROOM` を消す。

### 3.2 `read_line`

- `NetStream::rest` の `Vec` の空き (`capacity - len`。消費済みの先頭を詰めた後) が 8 バイト以上あれば、`min(空き, 65 536)` を `Read` で貸す。未初期化の空きを貸し、完了で `set_len(filled + n)` にする。`commit` の前に `set_len` しない。
- 空きが 8 バイト未満なら、`Readable` の後に `reserve_exact(max(len, 65 536))` で広げて貸す。
- 残りがなくなれば確保を放す規則は保つ (待つ接続に記憶を持たせない)。
- 行は完了の時点で UTF-8 を確かめ、`ctx.fresh_string` で1回だけ写して `OpRecord::made` に持たせる。`take_line` はそれを返す。UTF-8 でなければ今と同じ文言の IO の失敗にする。`OpKind::Line::line: Option<Vec<u8>>` は `not_utf8: bool` に替える。

### 3.3 `RunStats` の `completions` の表 (vm.md)

| 操作 | `completions` |
|---|---|
| `listen`、`accept`、`connect`、`write_bytes` | 1 |
| `read_chunk` (空きが 8 バイト以上の一意な buffer) | 1 |
| `read_chunk` (ほか) | 2。読めるようになる前に取り消した読みなどは 1。ストリームの buffer から返す読みは 0 |
| `read_line` | 周ごとに、`Vec` に空きがあれば 1、なければ 2 |

## 4. 書き (`cells.rs`、`std/Net.em`、T-8)

### 4.1 写しのない書きの貸し

- `submit_write` は `unique_bytes` を呼ばず、受け取った参照を記録に移して `Heap::bytes` の位置を貸す。写しは 0 回である。空の `Bytes` の書きは今どおり `NonNull::dangling` の位置と `cap == 0`。
- 安全の根拠: `Bytes` をその場で書き換える道は `set`、`push`、`append`、`append_slice`、`truncate`、`bytes_lend` と `bytes_commit` で、どれも `is_unique` (rc が 1) を要する。`is_unique` は、共有、不死、張り付いた数をみな偽にする (`eml_rt/src/heap/mod.rs` の `is_unique` の doc と ensures `u == (rc_value(w) == 1)`)。書きの記録が参照を1つ持つ間、ほかの持ち手から見た rc は 2 以上 (共有なら負) なので、どの道も写しになる。記録の参照は `take_bytes` まで放さない。
- 残る前提: `Heap::bytes` の位置が貸している間に動かないこと。ヒープは物体を動かさず、動かす道は REPL の回復の写しだけで、回復は `quiesce` の後なので保留の書きがない。
- 読みの貸しは今どおり一意を求める (カーネルが書くため)。
- `write_bytes : Bytes -> TcpStream -> <IO, Async> (Bytes, TcpStream)` は渡した `Bytes` そのものを返す。写した場合に元を返すための欄は要らない。
- 取り出しの行 `take_read` を `take_bytes` に改め、読みと書きの記録の buffer を返す。Net の extern の行の数は 18 のまま。`unique_bytes` は消し、読みの側は 3.1 節の「一意でなければ `Probe`、広げるときに写す」で置き換える。

### 4.2 直す文 (コメントと docs)

| 所 | 今の文の中身 | 直す形 |
|---|---|---|
| `object-model.md` の T10 の行 | `bytes` が返す位置を I/O の driver に読ませてよいのは、「物体を生かし、一意に保ち、書き換えない間だけである」 | 「物体を生かし、書き換えない間だけである。一意は求めない。その場で書き換える操作はどれも一意を求めるので、記録が参照を1つ持つ間、ほかの参照からは書き換わらない」 |
| `cells.rs` の `submit_write` のコメント (1903 行付近) | 「一意に保ち」 | 「生かし、書き換えない」 |
| `real.rs` の `Mem::as_init` の SAFETY | 書きと読みを1つの文で述べる | 書きは「生かし、書き換えない」、読みは「一意に保ち」と分ける |
| `test.rs` の書きの SAFETY (121-123 行付近) | 「生かし、書き換えない」 | 今のまま |
| vm.md の `RawBuf` の項 | 「一意に保ち、動かさない」 | 1 節の `RawBuf` の doc の文 |
| 読みの側のコメント (`cells.rs` 2259 行付近、`test.rs` 144 行付近) | 「一意に保ち」 | 今のまま |

T-8 の完了の基準: `grep -rn "一意に保ち" crates docs` の残りが、読みの貸しを述べる所だけである。`trust.rs` の行は変わらない (`TRUSTED` は項目の名前を持ち、文を持たない)。

## 5. driver の追随 (`crates/eml_io`、T-4)

### 5.1 compio の driver (`real.rs`)

- `submit(key, op)` は今の `Step` を作り、`Step::Done(r, _)` を `Submit::Done(r)` に、`Step::Wait(slot)` を `ops.insert(key, Entry)` と `Pending` にする。
- `next_key` と `place` の `Done` の腕を消す (`place` は解決の答えの腕だけが使う)。`done` に積むのは、解決の取り消し (`cancel` の中) と、解決の答えの即時の誤りだけになる。
- 名前の解決は `resolver.submit(key, ...)` に呼ぶ側の鍵を渡し、`Ok` なら `Pending`、`Err` なら `Done(Err)`。
- `debug_assert!(!self.ops.contains_key(&key), "a pending key is submitted again")`。
- `Readable` の `PollOnce` は、compio の poll の backend では `push` の中で終わらないので `Pending` のまま。`Read` は compio の `Recv::pre_submit` が先に `recv` を試すので、データがあれば `Done` になる。
- `completions` から `poll(Duration::ZERO)`、`receive_lookups`、`reap` を外し、`empty_bin` と `out.append(&mut self.done)` だけにする (並べ替えも消してよい。VM は鍵で引く)。
- `wait` が OS を見る。届いた完了か解決の答えがあれば `Duration::ZERO` で、なければ期限の長さで `poll` し、`receive_lookups` と `reap` で終わった操作を `self.done` に積んでから `Ok(true)` で戻る。今の「`done` があれば poll せずに戻る」と「解決の答えがあれば戻る」の早い戻りは消す。解決の答えは `reap` が受けないと消えないので、早く戻ると回り続けるためである。compio の `reap` は保留の表を歩くが、`wait` の中だけになり、io-backends で消える。
- `IDLE_TICK` を消す。`wait` は `poll(Option<Duration>)` にする。期限が `u64::MAX` なら `None`、ほかの期限は残りの長さを `MAX_POLL = Duration::from_millis(i32::MAX as u64)` (約 24.8 日) で切って渡す (切った待ちは早く戻るだけで約束に合う)。期限が `None` で保留も解決もなければ `Ok(false)` を返す (今と同じ)。期限が `None` で保留か解決があれば `None` で眠る。
- `quiesce` の1回の poll は、新しい私的な定数 `CANCEL_POLL: Duration = 10 ms` を使う (`CANCEL_LIMIT` の判定のため。io-backends で消える)。
- `lib.rs` の `pub use real::{IDLE_TICK, RealDriver}` から `IDLE_TICK` を外す。
- `cancel` は `ops` にある鍵 (保留) にだけ `register_cancel` するので、後の同じ鍵の操作に効かない規則をもう満たす。

### 5.2 テストの driver (`test.rs`)

- `completions` は決着をせず、`empty_bin` と `out.append(&mut self.done)` だけにする。決着は受け付け (`submit`) と `wait` の始めだけで行う。
- `submit(key, op)`: `empty_bin()` の後、`Op::Job` なら列に積んで `Pending`。ほかは `try_finish(Pending { key, op, cancelled: false })` を呼び、`Ok(c)` なら `immediate += 1` で `Submit::Done(c.result)`、`Err(p)` なら列の後ろに積んで `Pending`。`debug_assert!` で保留の鍵の重複を断る。`next_key` を消す。
- `pub fn immediate(&self) -> u64` を足す (`cfg(test)` にしない。Rust のテストが使う)。
- 受け付けの時点で決まる操作 (`try_finish` が `Ok` を返すもの): 相手の書いたバイトか EOF がある `Read` と `Readable`、開いた相手への `Write` (閉じた相手なら `BrokenPipe`)、backlog のある `Accept`、待ち受けるリスナーがあり backlog が満ちていない `Connect` (リスナーがなければ `Refused`、名前が仮想のホストでなければ `NotFound`)、`Listen` (使っているポートなら `AddrInUse`)、知らないハンドルの操作 (`no such handle`)。
- 決まらない操作: データも EOF もない `Read` と `Readable`、backlog の空の `Accept`、満ちた backlog への `Connect`、すべての `Job`。
- seed の混ぜは `wait` の決着の組にだけ掛かる。受け付けで決まる操作の順はタスクの交錯 (方針と seed) で決まり、決着の組に残るのは、データの来た `Read` と `Readable`、接続の来た `Accept`、席の空いた `Connect` と `Job` である。
- `cancel` は保留の列だけを見るので、後の同じ鍵の操作に効かない。

### 5.3 テストの stub

`cells.rs` の `NoDriver`、`tests/io.rs` の `HoldingDriver`、`tests/machine.rs` の `FakeDriver` と `StubDriver`、`tests/api.rs` が使う driver を新しい署名に合わせる。`IoDriver` の実装は6つ (`RealDriver`、`TestDriver`、`NoDriver`、`HoldingDriver`、`FakeDriver`、`StubDriver`) である。

## 6. std と `eml_extern` の差分

### 6.1 `std/Async.em` (T-5)

- `OpId` の注釈を「記録の番号 `(generation << 32) | index` を持つ即値。Kind は `Lin`」にする。
- extern の並びの末尾に `extern op_ready : OpId -> Int -> <IO> (Int, OpId)` を足す (`Async` の extern は 33 行になる)。`run` より上に関数を置かない、extern は末尾に足す、という `Async.em` の決まりを守る。
- `io_wait` と `io_wait_until`:

```haskell
pub io_wait : OpId -> <IO, Async> (Wake, OpId)
io_wait op =
  let (step, op) = op_ready op 0
  if step == 0 then (Woke, op)
  else if step == 2 then
    let _ = reschedule ()
    (Woke, op)
  else await_io op

pub io_wait_until : OpId -> Int -> <IO, Async> (Wake, OpId)
io_wait_until op ms =
  let (step, op) = op_ready op ms
  if step == 0 then (Woke, op)
  else if step == 2 then
    let _ = reschedule ()
    (Woke, op)
  else await_io_until op ms
```

- 2 のときの `reschedule` の `Wake` は見ない。`reschedule` の間に取り消されたタスクは、その操作の結果を受け取って続け、次の取り消しの地点で投げる。速い道だけのループでも、次の `io_wait` の `op_ready` が地点を読んで 1 を返すので、取り消しの後に終える操作は高々1つである。
- `Net.read_chunk` の本体の形は今のまま (`cancel_requested` → `submit_read` → `io_wait` → `op_stream` → `op_outcome` → `take_bytes` か `finish`)。`with_arg` の組は開く所で消える。

### 6.2 `std/Net.em` (T-8)

- `write_bytes` の型を `Bytes -> TcpStream -> <IO, Async> (Bytes, TcpStream)` にし、成功の腕を `(take_bytes op, s)` にする。
- `write str s = let (_, s) = write_bytes (Bytes.of_string str) s` の後に `s` を返す形にする (`write_line` も同じ)。`write` と `write_line` の型は今のまま。
- `read_chunk` と `read_chunk_within` の `take_read` を `take_bytes` にする (T-8)。
- `read_chunk` の doc に「`b` に空きがあれば、その空きの 65 536 バイトまでに読む」を足す (T-7)。

### 6.3 `std/Bytes.em` と `eml_rt` (T-6)

- `pub extern with_capacity : Int -> <Panic> Bytes`。doc は「容量が `n` バイト以上の空のバイト列を返す。`n` が負なら `Panic`」。
- `crates/eml_rt/src/heap/ctx/bytes.rs` に `pub fn bytes_with_capacity(&mut self, n: i64) -> (r: Result<Word, Fault>)` を Verus の下に書く。
  - 本体は `bytes_make` の前半と同じ: `n < 0` なら `Fault::Index(IndexFault::BytesLength { len: n })`、`k > MAX_BYTES || over_budget(&self.control, k + 16)` なら `Stopped(MemoryLimit)`、ほかは `self.heap.alloc_bytes_room(k)`
  - spec は `bytes_make` の ensures から中身の項を `text_of(w).len() == 0` に替え、`capacity(w) >= n` を足したもの。requires `old(self).wf()`、ensures `final(self).wf()`、`evv_spec` と `yield_spec` を保つ、`r is Ok || Index || MemoryLimit`、`n < 0 <==> Index`、`r is Err ==> *final(self) == *old(self)`。`held_ok` と `holds(w)` と `evolved_by` の項は `bytes_make` のまま。`capacity` が `Ctx` の側の spec で読めなければ、`heap` の `capacity` を引く (`alloc_bytes_room` の ensures に `capacity(w) >= room` がある)
- `crates/eml_rt/src/heap/tests/table.rs` に入力の行 (-1、0、1、65 536) を足し、`expected.txt` を `#[ignore]` のテストで作り直す。
- Miri の `trusted::` は要らない (検証したコードだけで、信頼の部分に触れない)。

### 6.4 `crates/eml_extern/src/lib.rs`

| 行 | 変更 | タスク |
|---|---|---|
| `OpId` | `type_row("Std.Async.OpId", Lin, Repr::Int)` | T-5 |
| `OpId` の位置 (`park_io`、`park_io_until` の1つ目、Net の `submit_*` の結果、`op_*`、`take_*`、`finish` の引数) | Repr を `Obj` から `Int`、印を `Lin` から `Unr` | T-5 |
| `RuntimeEntry::ResumeIo` | ABI を `[TObj, Enum, Int]` | T-5 |
| `AsyncOpReady` (新) | `borrowing(&[true, false], FunctionRow { with_arg: Some(0), ..kinded("Std.Async.op_ready", &[Int, Int], &[Unr, Unr], Int, Unr, Purity::Effectful) })` | T-5 |
| `BytesWithCapacity` (新) | `row("Std.Bytes.with_capacity", &[Int], Obj, Purity::Panics)` | T-6 |
| `NetTakeRead` | `NetTakeBytes` (`"Std.Net.take_bytes"`) | T-8 |
| `Net.write_bytes` と `submit_write` の型 | `write_bytes` の結果が組になる。`submit_write` の行は変えない (受けた buffer を記録に移すのは今と同じ) | T-8 |
| `Extern::is_async()` | `AsyncOpReady` を足す | T-5 |

- 試した結果: scratch で `OpId` を `Repr::Int` にし、`OpId` の位置を `Int`/`Unr`、`ResumeIo` を `[TObj, Enum, Int]` にし、`crates/eml_core_ir/tests/externs.rs` の印を読む所に Repr のスカラーの規則 (`declared_mark`) を入れ、2つの一覧 (`only_the_listed_rows_carry_a_lin_or_poly_mark`、`only_the_listed_rows_borrow_or_return_a_pair`) を直すと、`cargo test -p eml_core_ir -p eml_hir -p eml_lir -p eml_extern` がすべて通った (490、379、424、9、9 本)。直す前に落ちたのは印の2つのテストと runtime entry の2つのテストだけで、Repr のテストは通った。同じ変更で `eml_vm` の `io::a_listen_counts_one_completion` も通り、LIR の verifier は即値の token を断らなかった。
- `Lin` と宣言した型の Repr がスカラーなら、LIR の印は `unr` で RC の対象にならず、1回だけ使うことは型検査が守る (core-ir.md の「translate が決める印」の87行付近、`data Token : Lin = | Token` の例)。std の包みが token を捨てる書き間違いは E3003 で止まる。

## 7. テストの1本ずつの扱いと種類

種類は testing.md の「テストの変更の運用」のもの (1 は成否の変更、2 は期待値の変更、3 は機械的な追随)。どのタスクでも、成否が変わるテストがここに挙げたもののほかに出たら止め、計画書に種類 1 として1件ずつ足してから進める。

### 7.1 T-1

| テスト | 扱い | 種類 |
|---|---|---|
| `crates/eml_cli/tests/cli.rs` の `run_with_stats_prints_one_stats_line` | 新しく足す | — |
| `eml_vm` の単体テスト `the_stats_fields_follow_the_declaration` (名前は任せる) | 新しく足す。`fields()` の名前の並びと Debug の欄の並びを比べる | — |

### 7.2 T-3

| テスト | 扱い |
|---|---|
| `sched.rs` の `begin_run_starts_the_monitor_only_for_a_real_run_with_a_doorbell` | 新しく足す。`Real` と自己パイプで `begin_run` の後 `watcher.monitor.is_some()`、自己パイプなしと `Virtual` と `Injected` では `None` |
| `sched.rs` の `wait_deadline_caps_only_drivers_that_sleep_without_a_relay` | 新しく足す。`Real` + 自己パイプ + 監視は `Some(5000) → Some(5000)`、`None → None`、`Some(u64::MAX) → Some(u64::MAX)`。`Real` で自己パイプなしと `Injected` は `None → Some(ceil_ms(now) + 10)`、`Some(5000) → Some(min(5000, ceil_ms(now) + 10))`。`Virtual` はそのまま |
| `api.rs` の `an_injected_driver_waits_at_most_10_ms_at_a_time` | 新しく足す。`wait` の引数と `now` を記録し、`wait(Some(d))` で自分の時計を `d` まで進める driver を `with_driver` で渡し、`Async.sleep_ms 50` の run を流す。`wait` の期限がどれも呼んだときの `now + 10` 以下で、呼び出しが5回以上。実時間を測らない |
| `api.rs` の `#[cfg(target_os = "linux")]` `a_real_run_with_a_doorbell_has_a_monitor_from_its_start` | 新しく足す。別のスレッドで `Machine` を `Clock::Real`、`Sched::Sequential`、自己パイプ付きで作り、`Async.sleep_ms 100000` (タスクは1つ) を流す。テストのスレッドは `/proc/self/task/*/comm` に `eml-monitor` が現れるまで読み、現れたら `control.interrupt()` と `doorbell.ring()` (シグナルのハンドラと同じ手順) を行う。3 s の番犬が見つけられずに割り込んだら落ちる。確かめる値は、監視のスレッドが見つかったことと、run が割り込みで終わったこと。`cargo test` は1つのプロセスで並べて流すのでほかのテストの監視のスレッドで偽に通りうる (落ちはしない)。nextest はテストごとにプロセスを分けるので、そこでの結果を確かめとする |

### 7.3 T-4

| テスト | 扱い | 種類 |
|---|---|---|
| `crates/eml_io/src/real.rs` の単体テスト `a_wake_from_another_thread_ends_a_long_poll` | 消す。中身は `wait` を通す形で `tests/real.rs` の `a_wake_ends_a_wait_with_a_far_deadline` が引き継ぐ | 1 |
| `tests/test_driver.rs` と `tests/real.rs` のうち、`completions` を呼んで決着か OS の確かめを待っていたテスト (基点で 21 本: `test_driver.rs` の 19 本、`real.rs` の `a_cancelled_connect_does_not_try_the_next_address` と `a_cancelled_long_write_stops_resubmitting`) | `wait(Some(now))` を挟んでから `completions` で受ける形に直す。完了の値は変えない | 2 (範囲) |
| `tests/test_driver.rs` と `tests/real.rs` のうち、受け付けの時点で決まる操作 (5.2 節の一覧。compio では `push` の中で終わる操作) を `completions` から受け取っていたテスト | `submit` の戻り値から受け取る形に書き直す。完了の値は変えない | 2 (範囲) |
| `tests/test_driver.rs` の `a_seed_does_not_change_which_ops_finish` | 3つの端のデータのない `Readable` と backlog の空の `Accept` を保留にしてから、3つの `Write` と1つの `Connect` を受け付け、次の `wait` の1回の決着に4つの完了がそろい、seed 1、2、3 で中身の組が同じで順だけが変わりうることを確かめる形に組み直す (決着の組を混ぜる性質を保つ) | 2 |
| `tests/test_driver.rs` の `a_cancel_before_return_ends_the_op_at_the_next_settle`、`a_cancel_before_settle_marks_the_completion_cancelled` | 受け付けの時点で決まらない操作 (データのない `Readable`、backlog の空の `Accept`) で組み直す | 2 |
| `tests/real.rs` の `live_lookups_beyond_the_limit_wait_in_order` | 9つ目の解決が始まるのを待つループの `driver.wait(None).unwrap()` を `poll_for(&mut driver, 5)` に替える。刻みがないと、9つ目のスレッドが始まる前に戻った `wait(None)` の次の呼び出しを起こす者がいず、止まるためである。値は変えない | 2 |
| `tests/ui/run/async/net_*` と `crates/eml_cli/tests/repl/async/*.repl` のうち、操作が受け付けで終わったことで出力の順が変わるもの | 新しい順に直す。成否が変わるものが出たら止める | 2 (範囲。T-5 も同じ) |
| `tests/real.rs` と `tests/test_driver.rs` の署名の追随 | 補助 `fn run_op(driver, stash, key, op) -> io::Result<Done>` (`Submit::Done` ならそれを、`Pending` なら `complete` で待つ) を置き、鍵はテストが 1 から振る。値の比べは変えない。`complete` は今も `wait(None)` を挟むので形を保つ | 3 |
| `tests/real.rs` の `a_timed_wait_returns_at_or_after_its_deadline` と `a_wake_from_another_thread_leaves_a_wait_working` のコメント | `IDLE_TICK` の文を直す | 3 |
| `test_driver.rs` の `completions_do_not_settle` | 新しく足す。相手が書いた後の保留の `Read` は、`completions` を何度呼んでも返らず、`wait(Some(now))` の後の `completions` で1つ返り、時計は進まない | — |
| `test_driver.rs` の `an_op_decided_at_submit_returns_done_and_leaves_no_completion` | 新しく足す。開いた相手への `Write` が `Submit::Done(Ok(Size(n)))`、続く `completions` が空、`pending() == 0`、`immediate() == 1` | — |
| `test_driver.rs` の `a_read_without_data_stays_pending_until_a_write` | 新しく足す。`Pending`、相手の `Write` の後の `wait` と `completions` に1つ | — |
| `real.rs` の `#[cfg(unix)]` `a_read_after_a_finished_probe_returns_done` | 新しく足す。5 バイト書き、`Readable` を出して終わるまで待ち (`Done` でも `Pending` の完了でもよい)、続く `Read` が `Submit::Done(Ok(Size(5)))`、その後の `completions` が空で `pending() == 0`。書いた直後の `Read` がその場で終わることは確かめない (OS が約束しない) | — |
| `real.rs` の `a_wake_ends_a_wait_with_a_far_deadline` | 新しく足す。保留なし、期限 `d = now + 5000` で `wait(Some(d))`、20 ms 後に別のスレッドの `wake`。戻り値が `Ok(true)` で、戻ったときの `driver.now()` が `d` より前。実時間の上限は比べない | — |
| `real.rs` の `a_wait_with_no_ops_and_a_far_deadline_wakes` | 新しく足す。保留なしで `wait(Some(u64::MAX))` を待ち、別のスレッドの `wake` で `Ok(true)` で戻る。戻ること自体が確かめで、`Ok(false)` なら落ちる | — |
| `real.rs` の `a_wait_without_ops_sleeps_until_its_deadline` | 新しく足す。`wait(Some(now + 200))` を1回、150 ms より前に戻らない (下限だけを比べる) | — |
| `test_driver.rs` と `real.rs` (unix) の `a_cancel_after_completion_does_not_touch_the_next_op_of_the_key` | 新しく足す。鍵 1 の `Readable` を終わらせて受け取り、`cancel(1)` を呼び、同じ鍵 1 で `Read` を保留にして相手が書くと、`Read` の完了が `cancelled: false` で `Ok` | — |
| `test_driver.rs` と `real.rs` の `a_pending_key_submitted_twice_panics_in_debug` | 新しく足す。`#[cfg(debug_assertions)]`、`#[should_panic]` | — |
| `cells.rs` の単体テスト `a_completion_with_a_stale_generation_names_no_op` | 新しく足す。記録を作って放し、同じ添字の新しい記録を作り、古い鍵の完了を `NoDriver` が返すと `complete` が `CellFault::Internal("a completion names no op")` | — |
| `crates/eml_vm/tests/signal.rs` の `a_waiter_keeps_an_otherwise_idle_run_alive` | 変えない。シグナルを待つタスクだけの run (`switch_away` の `deadline.or(Some(u64::MAX))` の道) が本物の driver でデッドロックにならないことを確かめる既存のテストで、T-4 の完了の基準に名前で入れる | — |
| `eml_vm/tests/io.rs` の値 | 変わらない見込み (待ちは今どおり perform し、読みは今どおり `Readable` から始まる)。変わったら止める (`completions` の数え方は受け付けで終わった操作も数えるので、数が変われば設計の誤りである) | — |

`tests/real.rs` の新しいテストは、どれも単独で 1 s 以内に終わる。

### 7.4 T-5

| テスト | 扱い | 種類 |
|---|---|---|
| `crates/eml_core_ir/tests/externs.rs` の印の表の期待値 | `OpId` の位置が `Unr` になる (即値の token の印は `unr`)。印を読む関数 `declared_mark` が Repr のスカラーを `Unr` にし、2つの一覧の行を直す | 2 |
| `io.rs` の `one_task_of_finished_ops_never_waits_on_the_driver` | 新しく足す (中間の値)。`TestDriver` を包んで `wait` と `completions` の呼び出しと受け付けた `Op` の種類を記録し、`quiesce` で `immediate()` を写す driver `Recorder` を `with_driver` で渡す。1つのタスクで `listen`、`local_port`、`connect`、`accept`、`write_bytes (Bytes.of_string "hi")`、`read_chunk (Bytes.empty ())` を順に行う (読みの buffer を空にして、出力を書いた `hi` にする)。確かめる値: `immediate == 6` (読みは空きがないので `Readable` と `Read`)、`wait` の呼び出し 0、`RunStats.parks == 5`、`completions == 6`、出力 `hi`。T-8 で最終の値にする (7.7 節) | 新規 (値は T-8 で種類 2) |
| `io.rs` の `echo_counts_do_not_depend_on_the_seed` | 新しく足す。2つのタスクの 20 往復の echo を `Sched::Seeded(1)`、`(2)`、`(3)` と検査モードで流し、`parks` と `completions` が3つで同じ | — |
| `io.rs` の `a_fast_path_loop_yields_to_another_task_under_sequential` | 新しく足す。`Sequential` で、`main` のタスクが自分の2つの端で書きと読みを 300 回 (`Bytes.truncate 0` で buffer を使い回す)、先に `Async.start` したタスクが `println "other"`。出力が `other\nloop done\n` | — |
| `io.rs` の `a_cancelled_fast_path_loop_stops_at_its_next_wait` | 新しく足す。`Sequential` で、タスク `t` が自分の2つの端で書きと読みを 1000 回繰り返し、回るたびに `Atomic.add` で数を足す。`main` は名前付きのスコープ `s` に `t` を始めた後 `Async.yield_now ()` で `t` を動かし (`t` は予算の 128 回で譲る)、戻ったら `Async.cancel_scope s` と `Async.await t` を行い、スコープの全体を `try_io` で受ける (`Async.cancel` はハンドルを消費して待てないため)。確かめる値: 結果が種類 `Cancelled` の失敗、数が 64 以上 65 以下 (128 回の `io_wait` は 64 周で、取り消しの後に終える操作は高々1つ)。値は実装で数えて、この範囲で固定する | — |
| `io.rs` の `the_budget_runs_out_at_the_same_point_in_every_schedule` | 新しく足す。2つ目の受けが、始めたタスクの接続が先に走ったかで受け付けで終わるか待つかが変わる。その後、書き、`check_cancelled`、読みのループが予算を越える。書きを0か1つ足した形と、61、62、200 周の形で、`parks` が `Sequential` と seed 1、2、7 で同じ。io の再開のたびに予算を戻す規則と、終わった操作の待ちだけで予算を使う規則では落ちる | — |
| `cells.rs` の単体テスト `an_op_used_by_another_task_is_an_internal_error` | 新しく足す。worker 1 で受け付けた記録を worker 2 の `op_of` に渡すと `an op is used by another task` | — |
| `cells.rs` の単体テスト `a_second_take_of_an_op_is_an_internal_error` | 新しく足す。受け付けで終わった記録を `take_op` で2回取り出すと、2回目が `an op names no record`。LIR の本文からは書けない。`Async` の命令はどれも std の3つのエントリを要し、手で書いた LIR はそれを持たないためである | — |
| `cells.rs` の単体テスト `op_ready_counts_a_wait_only_on_a_finished_op` | 新しく足す。2.4節の表の4行 (終わっていない、地点が投げる、続ける、先に譲る) の結果と数える欄 | — |
| `cells.rs` の `NoDriver` | 書きを受け付けで終える印 `finish_writes` を足す (上の2本と `an_op_used_by_another_task_is_an_internal_error` が使う) | 3 |
| `crates/eml_cli/tests/ui.rs` の `compares_stats` | channel を使わず、どちらかの run の `completions` が 0 でない run は、`eml_test_support::driver_independent` を通した欄 (`tasks`、`parks`、`timers`、`dropped_failures`、`shared_objects`、`channels_swept`、`completions`) だけを比べる。新しい単体テスト `stats_of_driver_runs_are_compared_without_the_work_counts` が選び方を確かめる。理由: 操作が受け付けで終わるかが順で変わり、終わった操作の待ちは切り替えないので、VM の仕事の回数が順に従う | 2 |
| `io.rs` の既存の値 | 変わらない (`parks` は `io_wait` の数で、今の park の数と同じ) | — |
| UI テストと REPL のトランスクリプト | 7.3 節の範囲のとおり | 2 (範囲) |

### 7.5 T-6

| テスト | 扱い |
|---|---|
| `crates/eml_rt/src/heap/tests/bytes.rs` (両方のモード) | 新しく足す。`bytes_with_capacity(16)` の `bytes_lend` の `cap` が 16 以上、長さ 0 |
| `crates/eml_rt/src/heap/tests/table.rs` の入力 (-1、0、1、65 536) と `expected.txt` | 入力の行を足し、`expected.txt` を `#[ignore]` のテストで作り直す |
| `tests/ui/run/std/bytes_with_capacity.em` | 新しく足す。長さ 0、`push` の後の長さ |
| `tests/ui/run-fail/std/bytes_with_capacity_negative.em` | 新しく足す。負の `n` の `Panic` の文言 |

### 7.6 T-7

| テスト | 扱い | 種類 |
|---|---|---|
| `cells.rs` の単体テスト `a_read_lends_a_roomy_buffer_in_place_and_grows_a_short_one`、`probed_read` を使うもの、`a_read_line_lends_the_stream_buffer_and_frees_it_when_empty`、`a_read_after_a_read_line_is_served_from_the_stream_buffer` | 新しい規則の値に書き直す | 2 |
| `io.rs` の既存の値 | 変わらない。読みの buffer の `Bytes.of_string "keep"`、`"> "`、`"ab"` は空きが 8 バイト未満なので、今どおり `Readable` から始まる | — |
| UI テストと REPL のトランスクリプト | 変わらない。同じ道を通るので、このコミットは UI の期待値を変えない。変わったら止めて、計画書に種類 2 の範囲を足す | — |
| `io.rs` の `a_read_into_room_does_not_probe` | 新しく足す。`Recorder` で、`Bytes.with_capacity 16` の読みの受け付けの列に `Readable` がない | — |
| `io.rs` の `a_read_into_word_slack_probes_first` | 新しく足す。`Bytes.of_string "> "` の読みが `Readable` の後に `Read` で、`Read` に貸す大きさが 65 536 | — |
| `io.rs` の `a_read_into_empty_bytes_probes_first` | 新しく足す。`Bytes.empty ()` の読みが `Readable` の後に `Read` | — |
| `io.rs` の `a_probe_done_at_submit_reads_at_once` | 新しく足す。相手が書いた後の `Bytes.empty ()` の読みで、`Readable` と `Read` がどちらも受け付けで終わり、バイトが返る | — |
| `cells.rs` の単体テスト `growing_a_full_buffer_doubles_its_length` | 新しく足す。長さ 200 000 の空きのない buffer の `Probe` の後の容量が 400 000 以上で、貸す大きさが 65 536 | — |
| `api.rs` の `#[cfg(unix)]` `a_real_read_after_a_finished_probe_does_not_wait` | 新しく足す。`RealDriver` を包み、受け付けの `Op` の種類と結果 (`Done`/`Pending`)、`wait` の呼び出し、`completions` が返した鍵を順に記録する driver を `with_driver` で渡す (`Injected` だが、この run はデッドロックしない)。プログラムはつないだ後に `write` と `read_chunk (Bytes.empty ())` を行う。確かめる値: 読みの記録の `Readable` が終わった後 (受け付けの `Done` か、その鍵の完了の後) に出た `Read` が `Done` で、その `Read` の後に `wait` がない | — |

### 7.7 T-8

| テスト | 扱い | 種類 |
|---|---|---|
| `io.rs` の `a_read_line_that_is_not_utf8_raises_not_utf8` | `Net.write_bytes ... a` を `let (_, a) = ...` に | 2 |
| `io.rs` の `one_task_of_finished_ops_never_waits_on_the_driver` | 最終の形にする。`write_bytes` の組、`read_chunk (Bytes.with_capacity 16)`、`immediate == 5`、`wait` の呼び出し 0、`parks == 5`、`completions == 5` | 2 |
| `crates/eml_vm/src/cells.rs` の `a_write_of_a_shared_buffer_makes_no_copy` (実装で `miri.rs` の `a_shared_buffer_is_written_without_a_copy` から移した。手で書いた LIR は `Async` のエントリを持たず、`Std.Net` の行を呼べない。中身: `dup` した `Bytes` を `submit_write` に渡し、受け付けで終える stub の driver が貸された位置から読んだ中身と位置がもう1つの参照のものと同じで、`take_bytes` が同じ参照を返す) | 新しく足す。元の案: LIR の本文で、`Async.run` なしの `main` が `submit_listen`、`submit_connect`、`submit_accept` (テストの driver は受け付けで終える) で端を作り、`dup` した `Bytes` を `submit_write` に渡して `take_bytes` で受け、もう1つの参照と相手の端の読み (`submit_read` と `take_bytes`) の中身を比べる。両方のモード。Miri の `eml_vm miri::` の2つの設定で通る | — |
| `crates/eml_vm/src/cells.rs` の `a_write_held_by_the_driver_is_not_changed_by_its_alias` (実装で `miri.rs` の `a_held_write_is_not_changed_by_its_alias` から移した。理由は上の行と同じ。保留にした書きの記憶を、stub の `NoDriver` がその鍵の完了を返すときに読む。保留の間にもう1つの参照で `Bytes.set`、`push`、`append`、`truncate` をしてどれも写しになることと、同じ物体への `submit_read` が空きを貸さず `Probe` になることも確かめる。Miri は `cargo miri test -p eml_vm --lib -- cells::a_read cells::a_write`) | 新しく足す。元の案: 書きを `completions` まで保留にし、完了を返すときに貸された記憶を読んで写す driver を miri.rs に置き、`with_driver` で使う。LIR の本文で `dup` した `Bytes` を `submit_write` に渡し、保留の間にもう1つの参照に `Bytes.set` を行い (rc 2 なので写す)、完了の後に driver が読んだ中身が元の中身で、`set` した側が新しい中身であることを確かめる。両方のモードと Miri の2つの設定 | — |
| `io.rs` の `write_bytes_returns_the_same_buffer` | 新しく足す。読みの buffer を `Bytes.with_capacity 16` で作って `truncate 0` で渡し直す echo を、返った buffer を次の書きに渡す一意の形と、返りを捨てて引数で持ち続ける同じ buffer を毎回書く共有の形で 1、2、3 往復流し、往復ごとの `allocations` と `string_bytes_copied` が一定で、2つの形で等しいことを確かめる。書きが一意でない buffer を写すと共有の形だけ往復ごとに確保が増えるので、写しを捕まえられる。元の案 (1往復と3往復の差が2往復の差の倍) は写しがあっても通るので、レビューで強めた (T-8 の範囲の種類2) | — |
| UI テストと REPL のトランスクリプト | `Net.write_bytes` を使うものはない (`grep -rn write_bytes tests/ui crates/eml_cli/tests/repl` は `Proc`、`Stdout`、`FileOut` の行だけ) ので変わらない | — |

### 7.8 T-9

| テスト | 扱い |
|---|---|
| 既存のテスト | 期待値を変えない。`bench.rs` の snapshot は、`bench/` のプログラムが `Async` と `Net` を使わないので変わらない。変われば止める |

## 8. spec と docs の文案

文案は要点と、決めた文だけを持つ。写すときに日本語の規則を当てる。

### 8.1 T-1

- architecture.md「CLI と lib API」: `eml run --stats` (検査モードで走らせ、`Finished` で終わったら標準エラーに `stats: <欄>=<値> ... preemptions=<値>` の1行を出す。実行時エラーでは出さない)。
- CLAUDE.md の Commands の `run [...]` の行に `[--stats]` を足す。あわせて `main.rs` にない `--clock real|virtual` の記述を run と repl の両方から消す (vm.md「CLI に時計を選ぶフラグはない」と同じ状態にする)。

### 8.2 T-2

- benchmarks.md に「ネットワークの計測」の節を足す。物差しの件 (付録2 の2節の表の要約)、主指標、比べる規則 (付録2 の4節) を書き、表はまだ置かない。
- testing.md「よく使うコマンド」と CLAUDE.md の Commands に `bench/net.sh` の1行を足す。CLAUDE.md の行の形: `bench/net.sh LABEL=EML [LABEL=EML] [--only REGEX] [--stats]   # Std.Net on the real driver: CPU, syscalls (strace, Linux), RSS and RunStats per case as JSONL; bench/net/report.py makes the tables`。

### 8.3 T-3

- vm.md「時計と driver」の空きのループの項:
  - 「監視のスレッドがあり自己パイプのある `Clock::Real` の run では、VM は driver の `wait` の期限を切らない。眠る driver で中継のない run (`Clock::Real` で自己パイプがないか監視のスレッドを起こせなかった run と、`with_driver` で注入した run) では、VM が `wait` に渡す期限を今から 10 ms 先までに切る。`Clock::Virtual` のテストの driver の run には上限を掛けない。テストの driver は時計を進めるので、上限を掛けると、終わらない保留のある run がデッドロックで止まらない。デッドロックの判定は上限を掛ける前の期限で行う」
  - `with_driver` の項: 「`with_driver` に渡す driver は眠る driver に限る。テストの driver を包んだ driver に、終わらない保留のあるプログラムを流さない」
- vm.md「タスクとスケジュール」の監視のスレッドの契機: 「`Clock::Real` で自己パイプのある run は、run の始めに監視のスレッドを起こす。ほかの run では、横取りの方針のタスクを足すときと、OS のシグナルを待つときに起こす」。

### 8.4 T-4

- vm.md「時計と driver」:
  - trait のブロックを1節の形に直す。`submit` と鍵と `Submit` の項、`cancel` は保留の鍵にだけ効き後の同じ鍵の操作に効かないこと、`wait` の `Ok(false)` の契約と `u64::MAX` の期限、`wait` に刻みがないこと
  - 集める地点の箇条: 「集める地点は、空きの待ちの直前では OS を見ない。切り替えが続くときは 16 回に1回、横取りと `NOTICE` の集める地点ではいつも、`wait(今)` で OS を1回見てから集める」。「`completions` は OS を見ず、`wait` は期限が今以前でも OS を1回見る」
  - `IDLE_TICK` の項を消し、10 ms は VM の上限として 8.3 節の文に寄せる
  - テストの driver の項: 「テストの driver は、受け付けの時点で決まる操作を `submit` の中で終え、ほかの操作は `wait` の始めにだけ決着させる。`completions` は決着させない」
  - 本物の driver の `submit` の箇条: compio の `submit` は、`push` の中で終わった操作を `Submit::Done` で返す。プールが満ちたときに `submit` が待つことは、trait の doc からここ (本物の driver の項) に移す
- vm.md「操作の記録」: 鍵は記録の番号 (`CellRef`) であること、`in_flight`、`settle` が受け付けと集める地点の両方から呼ばれること、古い generation の完了は内部の誤りであること。
- object-model.md に `RawBuf` を述べる所があれば、1節の doc の文にそろえる。
- architecture.md「`eml_io` の内部」: `IDLE_TICK` の文 (352、365、368 行付近) を、刻みのない `wait` と `CANCEL_POLL` に替える。
- async.md「`IoDriver` と `eml_io`」(242 行付近): 刻みの文を直す。
- multicore.md (465 行付近): 「今の VM は、監視のスレッドのない run では driver の待ちを起こさず、代わりに1回の待ちを `IDLE_TICK` で切る」を「監視のスレッドのない run では、VM が1回の待ちを 10 ms で切る」にする。
- roadmap「その後の項目」の「10 ms の上限をなくすこと」を消す。監視のスレッドのない run の 10 ms の上限は vm.md にある。
- status.md (147 行付近) の 10 ms の文を「監視のスレッドのない run では、VM が1回の待ちを 10 ms で切る」にする。
- `crates/eml_vm/src/lib.rs` の `Machine::drop` のコメント (652 行) 「本物の driver は落とすときに仕事を待ちきるので」を「本物の driver には、落とすときにカーネルの書き込みの終わりを待つものがあるので」にする。io-backends は `crates/eml_vm/src` に触れないので、driver の振る舞いを述べた `eml_vm` のコメントはここで中立の文にしておく。

### 8.5 T-5

- stdlib.md「`Std.Async`」: `OpId` の行 (即値、Kind `Lin`、Repr `Int`)、extern の表 (33 行) に `op_ready`。
- stdlib.md「`Std.Net`」: 受け付け、待ち、取り出しの文に「操作が終わっていれば待たない」。
- stdlib.md「操作の予算」: 速い道の `io_wait` も予算を使う。
- vm.md「操作の記録」: token の項を即値に。`op_ready` の3値と数え方。`op_of` の確かめ。
- vm.md「`RunStats`」: `parks` は `io_wait` と `io_wait_until` の呼び出しの数 (速い道でも数える)。`completions` は受け付けで終わった操作と集めた完了の和で、「集める地点が」を外す。`cancels` の項に「速い道の `reschedule` も数える」。
- vm.md「取り消しの届け方」: 速い道だけのループでも、取り消しの後に終える操作は高々1つである。
- core-ir.md「表の印の列」の `OpId` の行と、`with_arg` の一覧 (76、232、249 行付近) に `op_ready`。
- testing.md「seed で流す」: `parks` と `completions` はスケジュールに依らない。`read_line` の周は届き方で決まるので、`async/` のファイルは周がプログラムで決まるように書く。

### 8.6 T-6

- stdlib.md「`Bytes`」に `with_capacity`。object-model.md「バイト列の操作」に1行。

### 8.7 T-7

- vm.md「操作の記録」: 貸し方 (空きが 8 バイト以上の一意な buffer は空きの 65 536 バイトまでを `Read` で貸す。ほかは `Readable` の後に広げる)、広げる式 `長さ + max(長さ, 65 536)`、`Readable` が受け付けで終われば続けて貸すこと、`read_line` の `Vec` の同じ規則。
- vm.md「`RunStats`」の表を 3.3 節の形に。
- stdlib.md「`Std.Net`」の `read_chunk`: 「`b` の空きが 8 バイト以上あれば、その空きの 65 536 バイトまでに読む。なければ容量を広げて 65 536 バイトまで読む」。

### 8.8 T-8

- stdlib.md「`Std.Net`」: `write_bytes` の型と「渡した `Bytes` をそのまま返す」、extern の表の `take_bytes`。
- vm.md「操作の記録」: 書きの受け付けは写さない。
- object-model.md の T10 の行 (4.2 節)。
- async.md「所有ごと渡す buffer」: 書きも buffer を返すこと。async.md「未決の論点」の「`write_bytes` が buffer を返して使い回す形: 既定は返さない」の箇条を消す。

### 8.9 T-10

- benchmarks.md「ネットワークの計測」に、T-2b の1点目と2点目を並べた2列の表 (列はコミット。user+sys、実時間、syscall、RSS、`allocations`)、syscall の目標の対応の表、T-9 の `POLL_EVERY` の比べの数字を置く。3列目は io-backends が足す。
- status.md: 速い道、即値の token、`write_bytes` の型。
- architecture.md: `eml_vm` の I/O の記録の節と `eml_io` の節が今の形であることを確かめる。
- ffi.md の 87 行付近: 「完了は持ち主の worker の受け箱の mutex を通して届く」を、プールの仕事の完了は持ち主の worker の driver の完了の列を通して届く形に直す (async.md の「ブロックする呼び出しのプール」と同じ)。
- async.md「受け付け、待ち、取り出し」に残る古い文 (終わった操作も待つこと、`OpId` の `Foreign`)。
