//! LIR を実行するレジスタ型のバイトコード VM (docs/spec/vm.md)。`eml_rt` の `Ctx` の上で動き、フロントエンドに
//! 依存しない。

use std::mem::ManuallyDrop;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use eml_lir::Program;
use eml_rt::{Checked, Ctx, CtxStats, Fast, Fault, HeapError, IoDriver, Mode, Stats};

use crate::cells::{Registry, Shared, TaskStats};
use crate::counters::Counters;
use crate::driver::{JOB_QUIESCE_LIMIT, RunClock};
use crate::error::KEPT_PENDING;
use crate::vm::ExecWith;
use crate::workers::{
    Retired, Run, Shutdown, WorkerExit, Workers, rescuable, settle, spawn_workers,
};

mod apply;
mod cells;
mod code;
mod compile;
mod counters;
mod data;
mod disasm;
mod doorbell;
mod driver;
mod error;
mod ffi;
mod hosts;
mod jobs;
mod monitor;
mod op;
mod sched;
mod tls;
mod vm;
mod workers;

pub use code::{CodeBase, CompileError, FnEntry};
pub use disasm::disassemble;
pub use doorbell::Doorbell;
pub use driver::{Clock, Signal, VirtualSignal};
pub use eml_cabi::{Bridge, CCall, Dtor, Linked};
pub use eml_rt::{Control, HostRef, Limits, Stop};
pub use error::{RuntimeError, SourceLocation, VmFault};
pub use hosts::{Capture, CaptureHost, Written, capture_host, script_host, stdio_host};
pub use sched::{
    Memory, Normal, Policy, Rescue, RescueAlways, Sched, Scheduler, Seeded, Sequential,
};
pub use vm::Task;
pub use workers::{Entry, Ready};

/// 実行の仕事の回数と、同時に生きていたヒープの物体の数とスタックの語の数の最大。検査モードだけで数える。
/// 各欄の意味と出どころは docs/spec/vm.md の「`RunStats`」にある。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RunStats {
    pub evidence_steps: u64,
    pub yields: u64,
    pub yield_frames: u64,
    /// 中断の巻き戻しで外したフレームの数。`ApplyRest` の枠も数える。
    pub unwound_frames: u64,
    pub string_bytes_copied: u64,
    pub rc_increments: u64,
    pub rc_decrements: u64,
    pub boxes: u64,
    pub unboxes: u64,
    pub allocations: u64,
    /// token の箱を使った `con` の回数。空の token で確保した `con` は数えない。
    pub reuses: u64,
    pub peak_objects: u64,
    /// タスクのスタックで同時に使った語の数の最大。
    pub peak_stack: u64,
    /// `Async` が作ったタスクのセルの数。
    pub tasks: u64,
    /// `Async` の待ち、`yield_now`、`sleep_ms`、`drain_within`、channel の待ち、driver の操作の待ち、シグナルの待ちの数。
    /// driver の操作の待ちは `io_wait` と `io_wait_until` の呼び出しの数で、操作が終わっていて継続を渡さずに続けた待ちも
    /// 数える。ほかの待ちは継続をランタイムに渡した数である。
    pub parks: u64,
    /// スコープの最初の失敗より後の、タスクの失敗の数。
    pub dropped_failures: u64,
    /// 取り消しが届いた回数。待つ前に取り消しを見つけた `park`、`park_time`、`park_drain`、`park_send`、`park_recv`、
    /// `park_io`、`park_io_until`、`park_signal`、mail の処理が外した待ちと取り消しを頼んだ操作の待ち、取り消しで
    /// 再開する `yield_now` (速い道が譲る `reschedule` は除く)、真を返した `cancel_point`、偽を返した `claim_scope` を
    /// 数える (docs/spec/vm.md の「`RunStats`」)。
    pub cancels: u64,
    /// `await_time` と `await_io_until` が求めた、`ms` が正の待ちの数。待つ前に取り消しが見つかった待ちは数えない。
    /// 操作がもう終わっていて輪に載せなかった待ちと、`io_wait_until` の速い道で続けた待ちも数える。
    pub timers: u64,
    /// `share` が rc を負にした物体の数。
    pub shared_objects: u64,
    /// `Std.Chan` の速い道が品物を積んだ回数と取った回数の和。
    pub channel_ops: u64,
    /// `send` と `recv` が継続を `park_send`、`park_recv` に渡した回数。`parks` にも数える。
    pub channel_waits: u64,
    /// run の終わりに放した channel の数。閉じなかった channel と、閉じても品物の残った channel を数える。
    pub channels_swept: u64,
    /// 終わった driver の操作の数。受け付けで終わった操作と、driver から取り出して記録に当てた完了の和である。
    /// `quiesce` が捨てた完了は数えない。
    pub completions: u64,
}

impl RunStats {
    /// 欄の名前と値を宣言の順に返す。`eml run --stats` の出力はこれを並べるだけなので、欄を足したらここにも足す
    /// (長さの型が数の食い違いを止める)。
    pub fn fields(&self) -> [(&'static str, u64); 23] {
        [
            ("evidence_steps", self.evidence_steps),
            ("yields", self.yields),
            ("yield_frames", self.yield_frames),
            ("unwound_frames", self.unwound_frames),
            ("string_bytes_copied", self.string_bytes_copied),
            ("rc_increments", self.rc_increments),
            ("rc_decrements", self.rc_decrements),
            ("boxes", self.boxes),
            ("unboxes", self.unboxes),
            ("allocations", self.allocations),
            ("reuses", self.reuses),
            ("peak_objects", self.peak_objects),
            ("peak_stack", self.peak_stack),
            ("tasks", self.tasks),
            ("parks", self.parks),
            ("dropped_failures", self.dropped_failures),
            ("cancels", self.cancels),
            ("timers", self.timers),
            ("shared_objects", self.shared_objects),
            ("channel_ops", self.channel_ops),
            ("channel_waits", self.channel_waits),
            ("channels_swept", self.channels_swept),
            ("completions", self.completions),
        ]
    }
}

/// 1回の実行の結果。`exit` で終わったら、その終了コードを持つ (docs/spec/vm.md の「`RunStats`」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Finished {
    pub stats: RunStats,
    pub exit: Option<i64>,
    /// 方針が安全点で始めたタスクの切り替えの数 (docs/spec/vm.md の「タスクとスケジュール」)。
    pub preemptions: u64,
    /// 1つ以上を盗んだ救済の数と、救済で移した項の数。どちらも worker の和である。救済はスケジュールで決まるので
    /// `RunStats` に入れない (docs/spec/vm.md の「`RunStats`」)。
    pub rescues: u64,
    pub rescued: u64,
    /// この run が作った worker の数。worker 0 は数えない (docs/spec/vm.md の「タスクとスケジュール」)。
    pub workers: usize,
    /// この run がほかの worker に置いた `par` の右の子の数。どの worker の fork も数える
    /// (docs/spec/vm.md の「`par` の fork と join」)。
    pub placed: u64,
    /// この run が、`start` を実行した worker と違う worker に置いた `Async` のタスクの数
    /// (docs/spec/vm.md の「`Async` のタスク」)。
    pub placed_tasks: u64,
    /// この run がほかの worker に置いた `compute` の仕事の数。どの worker の `fork_job` も数える
    /// (docs/spec/vm.md の「`compute` の仕事」)。
    pub placed_jobs: u64,
}

/// VM だけが知る仕事の回数。検査モードだけで数える。
#[derive(Debug, Default)]
pub(crate) struct RunCounts {
    pub(crate) unwound_frames: u64,
    pub(crate) boxes: u64,
    pub(crate) unboxes: u64,
    pub(crate) reuses: u64,
}

impl RunCounts {
    /// ほかの worker の回数を足す (docs/spec/vm.md の「`RunStats`」)。
    fn add(&mut self, other: &RunCounts) {
        self.unwound_frames += other.unwound_frames;
        self.boxes += other.boxes;
        self.unboxes += other.unboxes;
        self.reuses += other.reuses;
    }
}

/// 実行の設定。上限は `Control` が持つので、ここに上限の欄は置かない。`file_root` は Host が持つ
/// (docs/spec/vm.md の「置き場所」)。写した設定は同じ `Control` を分け合うので、写した設定で2つの `Machine` を同時に
/// 動かさない (docs/spec/object-model.md の「実行の制御」)。
#[derive(Clone)]
#[non_exhaustive]
pub struct RunConfig {
    pub debug_heap: bool,
    /// 止める理由のビットは `Control::clear` でしか下りないので、1つの `Control` は1回の実行にだけ使う。終わりの欄は
    /// 実行の始めに開くので、使い回しの妨げにならない。`clone` した設定は同じ `Control` を指す。
    pub control: Arc<Control>,
    /// タスクを切り替える方針。既定は横取りしない `Sched::Sequential` である。
    pub sched: Sched,
    /// タイマーが使う時計。既定はすべてのタスクが待つときにだけ進む `Clock::Virtual` である。
    pub clock: Clock,
    /// 監視のスレッドを起こす自己パイプ。シグナルのハンドラと同じものを渡す。既定はなしで、監視のスレッドは刻みで
    /// シグナルを見に行く。`Clock::Real` の run と worker を2つ以上持てる run に渡すと、run の始めに監視のスレッドが
    /// 起き、VM は空きの待ちを切らない。そのため、止める語を立てる者は時計によらず自己パイプも鳴らす
    /// (docs/spec/vm.md の「タスクとスケジュール」)。
    pub doorbell: Option<Arc<Doorbell>>,
    /// 仮想のシグナルの予定。既定は空で、テストのハーネスだけが使う (docs/spec/vm.md の「時間の待ち」)。
    pub signals: Vec<VirtualSignal>,
    /// 静止が走っている `Op::Job` を待つ長さで、run のすべての静止が分け合う。既定は 1 秒である。過ぎても残る仕事は
    /// 見捨て、`Machine` は poisoned になる (docs/spec/vm.md の「時計と driver」)。
    pub job_budget: Duration,
}

// `Control` は `Default` を持たないので、手で書く
impl Default for RunConfig {
    fn default() -> RunConfig {
        RunConfig {
            debug_heap: false,
            control: Arc::new(Control::new(Limits::default())),
            sched: Sched::Sequential,
            clock: Clock::Virtual,
            doorbell: None,
            signals: Vec::new(),
            job_budget: JOB_QUIESCE_LIMIT,
        }
    }
}

impl RunConfig {
    pub fn with_debug_heap(mut self, debug_heap: bool) -> RunConfig {
        self.debug_heap = debug_heap;
        self
    }

    pub fn with_control(mut self, control: Arc<Control>) -> RunConfig {
        self.control = control;
        self
    }

    pub fn with_sched(mut self, sched: Sched) -> RunConfig {
        self.sched = sched;
        self
    }

    pub fn with_clock(mut self, clock: Clock) -> RunConfig {
        self.clock = clock;
        self
    }

    pub fn with_doorbell(mut self, doorbell: Arc<Doorbell>) -> RunConfig {
        self.doorbell = Some(doorbell);
        self
    }

    pub fn with_signals(mut self, signals: Vec<VirtualSignal>) -> RunConfig {
        self.signals = signals;
        self
    }

    pub fn with_job_budget(mut self, job_budget: Duration) -> RunConfig {
        self.job_budget = job_budget;
        self
    }
}

/// VM の worker のスレッドのスタックの大きさ。VM のスタックは `Task` のデータでヒープの外にあるので、Rust の再帰の
/// 深さだけを受け持てばよい。
pub(crate) const WORKER_STACK: usize = 8 << 20;

/// プログラムを1回だけ実行する。worker 0 のスレッドを作り、その上で新しい `Machine` に読み込み、エントリを実行し、
/// 閉じる。呼んだスレッド (host) は worker 0 を join するまで SIGINT と SIGTERM を塞ぎ、2つのシグナルを worker 0 に
/// 届ける。待っている読みに `EINTR` を返すためである (docs/spec/vm.md の「上限と割り込み」)。
pub fn execute(
    program: &Program,
    linked: &Linked,
    config: &RunConfig,
    host: HostRef,
) -> Result<Finished, RuntimeError> {
    thread::scope(|scope| {
        let builder = thread::Builder::new()
            .name("eml-worker-0".into())
            .stack_size(WORKER_STACK);
        // 塞ぐのは作った後である。新しいスレッドは作ったスレッドの塞ぎを受け継ぐので、先に塞ぐと worker 0 も塞がれる
        let worker = builder
            .spawn_scoped(scope, move || execute_here(program, linked, config, host))
            .map_err(|error| RuntimeError::Fault {
                fault: VmFault::Driver(error.to_string()),
                function: "<worker 0>".into(),
                at: None,
            })?;
        let _blocked = eml_io::Masked::new();
        worker
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

/// `execute` の中身。worker 0 のスレッドで動く。
fn execute_here(
    program: &Program,
    linked: &Linked,
    config: &RunConfig,
    host: HostRef,
) -> Result<Finished, RuntimeError> {
    let mut machine = AnyMachine::new(config, host);
    let loaded = machine
        .load(program, linked)
        .map_err(|error| compile_error(program, error))?;
    let finished = machine.run(loaded.entry)?;
    // 仕事を見捨てた `exit` の run でも、終了コードを返す。poisoned の `Machine` はヒープを漏らし、`close` は終了コードの
    // 代わりに poisoned の誤りを返す (docs/spec/vm.md の「時計と driver」)
    if machine.is_poisoned() {
        return Ok(finished);
    }
    machine.close()?;
    Ok(finished)
}

/// `Machine::load` の結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Loaded {
    /// 読み込んだプログラムのエントリの関数の、`CodeBase` での番号。
    pub entry: u32,
}

/// 入力をまたいで `CodeBase` と `Ctx` を持つ実行の状態 (docs/spec/vm.md の「追記」)。`load` でプログラムを追記し、
/// `run` でエントリの関数を実行する。モードごとの確かめは、作るときに `Machine<Fast>::new` と `Machine<Checked>::new` が
/// 決める。渡す `Control` を同時に使う `Machine` は1つだけにする。ID の表と数が `Control` にあるためである
/// (docs/spec/object-model.md の「実行の制御」)。
pub struct Machine<M: Mode> {
    /// 時計と待ち。`with_driver` が渡すか、最初の run が `clock` と方針の seed から作り、run の間は `Scheduler` に
    /// 移す。`with_clock` と `with_sched` は `None` に戻し、次の run が作り直す。作った後は時刻が run をまたいで続く。
    /// driver が貸した記憶はヒープにあるので、`Drop` は `ctx` より先にこの欄を落とす (docs/spec/vm.md の「時計と driver」)。
    driver: Option<Box<dyn IoDriver>>,
    /// `driver`と同じ時間の基準。runの`Run`に渡し、runをまたいで続く時刻を同じdriverと同じ基準で読む。driverの
    /// 出どころもこれが決める。`with_driver`で渡したdriverは、時計の欄によらず基準を`Injected`にする。出どころを
    /// 知らないdriverのrunにはOSのシグナルを中継せず、眠るdriverとして空きの待ちを切る (docs/spec/vm.mdの
    /// 「時計と driver」)。
    run_clock: RunClock,
    code: CodeBase<M>,
    /// `ctx` と同じ Host。worker 1..N-1 の `Ctx` に写す。
    host: HostRef,
    /// poisoned の `Machine` では、driver が貸したままの記憶を放さないように、`Drop` が落とさずに漏らす。
    ctx: ManuallyDrop<Ctx<M>>,
    /// worker 1..N-1 が run の終わりに返した `Ctx`。成功した run では終わりの検査の後に落とし、ほかの道では回復の
    /// 後に落とす。回復はほかのヒープのページから写しうるためである。回復しなかった誤りの後は `Drop` まで残す
    /// (docs/spec/object-model.md の「回復」)。
    others: Vec<Ctx<M>>,
    /// `quiesce` の後にも driver が保留の操作を残した。この後の run、回復、`close` はヒープに触れずに driver の誤りを
    /// 返す (docs/spec/vm.md の「時計と driver」)。
    poisoned: bool,
    max_depth: usize,
    sched: Sched,
    /// 実行と回復の後の確かめ。2つ目の引数はほかの worker の `Ctx` である。検査モードではどのヒープの不変条件も
    /// 確かめ、すべてのヒープで ID ごとの数を数え直す。
    check_heap: CheckHeap<M>,
    /// 成功した run の終わりに、ほかの worker のヒープに残った物体を確かめる。検査モードではリークとして返す。
    at_run_end: fn(&mut [Ctx<M>]) -> Result<(), RuntimeError>,
    /// 閉じるときの確かめ。検査モードでは残った物体をリークとして返す。
    at_close: fn(&mut Ctx<M>) -> Result<(), RuntimeError>,
    /// ほかの worker のヒープが、これまでの run でした仕事の和。worker 0 の `ctx` の積算と同じく、`Machine` を作って
    /// からの値である (docs/spec/vm.md の「`RunStats`」)。
    others_ctx: CtxStats,
    others_heap: HeapWork,
    /// 回復で捨てたヒープが実行でした仕事。
    earlier: HeapWork,
    /// 今のヒープが回復で写すためにした仕事。`RunStats` は実行の仕事だけを数えるので引く。
    copying: HeapWork,
    /// 根の表のうち、印を付け終えた先頭の根の数。実行をまたいで持ち、回復で 0 に戻す
    /// (docs/spec/object-model.md の「セッションの根」)。
    shared_roots: usize,
    /// 次の run の登録表の、添字ごとの generation (docs/spec/vm.md の「セルと登録表」)。
    generations: Vec<u32>,
    /// `Std.Atomic.Counter` の表。run をまたぎ、REPL の入力をまたいで残る (docs/spec/vm.md の「セルと登録表」)。
    counters: Arc<Counters>,
    /// driver を作るときの時計。
    clock: Clock,
    /// 実行ごとの `Scheduler` に渡す自己パイプ。1つの自己パイプは1つの `Machine` だけに渡す
    /// (docs/spec/vm.md の「タスクとスケジュール」)。
    doorbell: Option<Arc<Doorbell>>,
    /// 次の run だけに渡す仮想のシグナルの予定。run が取り出すので、後の run は再生しない
    /// (docs/spec/vm.md の「時間の待ち」)。
    signals: Vec<VirtualSignal>,
    /// 1回の run の静止が `Op::Job` を待てる長さ。`recover` はまるごと受け取る (docs/spec/vm.md の「時計と driver」)。
    job_budget: Duration,
}

impl Machine<Fast> {
    pub fn new(control: Arc<Control>, host: HostRef) -> Machine<Fast> {
        Machine::with_checks(control, host, |_, _| Ok(()), |_| Ok(()), |_| Ok(()))
    }
}

impl Machine<Checked> {
    pub fn new(control: Arc<Control>, host: HostRef) -> Machine<Checked> {
        Machine::with_checks(control, host, check_heaps, check_others, check_leaks)
    }
}

impl<M: Mode> Machine<M> {
    fn with_checks(
        control: Arc<Control>,
        host: HostRef,
        check_heap: CheckHeap<M>,
        at_run_end: fn(&mut [Ctx<M>]) -> Result<(), RuntimeError>,
        at_close: fn(&mut Ctx<M>) -> Result<(), RuntimeError>,
    ) -> Machine<M> {
        Machine {
            code: CodeBase::new(Arc::clone(&control)),
            max_depth: control.limits().max_depth,
            host: host.clone(),
            ctx: ManuallyDrop::new(Ctx::new(control, host)),
            others: Vec::new(),
            poisoned: false,
            sched: Sched::Sequential,
            check_heap,
            at_run_end,
            at_close,
            others_ctx: CtxStats::default(),
            others_heap: HeapWork::default(),
            earlier: HeapWork::default(),
            copying: HeapWork::default(),
            shared_roots: 0,
            generations: Vec::new(),
            counters: Arc::default(),
            clock: Clock::Virtual,
            driver: None,
            run_clock: Clock::Virtual.start(),
            doorbell: None,
            signals: Vec::new(),
            job_budget: JOB_QUIESCE_LIMIT,
        }
    }

    /// 実行ごとに作るスケジューラの方針を選ぶ。既定は `Sched::Sequential` である。seed は driver に渡すので、次の run
    /// が driver を作り直す。
    pub fn with_sched(mut self, sched: Sched) -> Machine<M> {
        self.sched = sched;
        self.driver = None;
        self
    }

    /// タイマーが使う時計を選ぶ。既定は `Clock::Virtual` である。次の run が新しい driver を作る。
    pub fn with_clock(mut self, clock: Clock) -> Machine<M> {
        self.clock = clock;
        self.driver = None;
        self
    }

    /// 監視のスレッドを起こす自己パイプを渡す。ないときは、監視のスレッドが刻みでシグナルを見に行く。`Clock::Real` の
    /// run と worker を2つ以上持てる run に渡すと、run の始めに監視のスレッドが起き、VM は空きの待ちを切らない。
    /// そのため、止める語を立てる者は時計によらず自己パイプも鳴らす (docs/spec/vm.md の「タスクとスケジュール」)。
    pub fn with_doorbell(mut self, doorbell: Arc<Doorbell>) -> Machine<M> {
        self.doorbell = Some(doorbell);
        self
    }

    /// 次の run で時間の輪に載せる仮想のシグナルの予定を渡す。期限は run の時間の基準の始まりからのミリ秒である。
    pub fn with_signals(mut self, signals: Vec<VirtualSignal>) -> Machine<M> {
        self.signals = signals;
        self
    }

    /// 静止が走っている `Op::Job` を待つ長さを選ぶ。既定は 1 秒で、run のすべての静止が分け合う
    /// (docs/spec/vm.md の「時計と driver」)。
    pub fn with_job_budget(mut self, job_budget: Duration) -> Machine<M> {
        self.job_budget = job_budget;
        self
    }

    /// 次の run から使う driver を渡す。後の `with_clock` と `with_sched` はこれを捨てる。driver は `Machine` を使う
    /// スレッドの上で作る。渡すのは眠る driver に限る。VM はその run の空きの待ちを 10 ms ごとに切るので、期限まで
    /// 時計を進める driver では、終わらない保留のある run がデッドロックで止まらない (docs/spec/vm.md の
    /// 「時計と driver」)。
    pub fn with_driver(mut self, driver: Box<dyn IoDriver>) -> Machine<M> {
        self.driver = Some(driver);
        self.run_clock = RunClock::Injected;
        self
    }

    pub fn code(&self) -> &CodeBase<M> {
        &self.code
    }

    /// 静止の後にも driver が保留の操作を残したか。そうなら、この後の `load`、`run`、`close` は driver の誤りを返す
    /// (docs/spec/vm.md の「時計と driver」)。
    pub fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    /// プログラムを追記する。`linked` は `eml_native` がこのプログラムの C の表をつないだ結果である。誤りを返した後の
    /// `Machine` は使い続けない (`CodeBase::load`)。
    pub fn load(&mut self, program: &Program, linked: &Linked) -> Result<Loaded, CompileError> {
        if self.poisoned {
            return Err(CompileError::Poisoned);
        }
        let entry = self.code.load(program, linked)?;
        if let Some(region) = &self.code.region {
            // 入力の境目で、Ctx の持つ語には触れずに塊を付け替えるだけなので、`heap_mut` の前提を崩さない。新しい塊は
            // 前の塊の子孫である (docs/spec/object-model.md の「凍結した領域」)
            unsafe { self.ctx.heap_mut() }.attach(Arc::clone(region));
        }
        Ok(Loaded { entry })
    }

    /// エントリの関数を `()` で実行する。統計は `Machine` を作ってからの積算で、`peak_stack` だけがこの実行の値である。
    /// 根は手放さず、次の実行に残す。続けられる誤りの後と `exit` が実行を捨てた後は、根から届く物体を新しいヒープに
    /// 写して古いヒープを捨て、元の誤りか `exit` の `Finished` を返す。回復が失敗したら `Unrecovered` を返す
    /// (docs/spec/vm.md の「追記」)。
    pub fn run(&mut self, entry: u32) -> Result<Finished, RuntimeError> {
        self.run_recovering(entry, None)
            .map(|(finished, _)| finished)
    }

    /// 文字列を返すエントリの関数を実行し、結果の文字列の先頭 `max_chars` 文字を写して返す。REPL が式の入力の値を書いた
    /// 文字列を受け取るのに使う。LIR はエントリが文字列を返すことを印さないので、呼ぶ側が文字列を返すエントリにだけ使う。誤りと
    /// 回復は `run` と同じである。`exit` で終わった実行は結果を返さないので、文字列は `None` になる
    /// (docs/spec/vm.md の「追記」)。
    pub fn run_shown(
        &mut self,
        entry: u32,
        max_chars: usize,
    ) -> Result<(Finished, Option<String>), RuntimeError> {
        self.run_recovering(entry, Some(max_chars))
    }

    /// `line` を Host の標準出力に1行として書き、書き出す。REPL が整形した結果を書くのに使う。
    pub fn write_line(&mut self, line: &str) -> Result<(), Fault> {
        self.ctx.print_line(line)
    }

    fn run_recovering(
        &mut self,
        entry: u32,
        shown: Option<usize>,
    ) -> Result<(Finished, Option<String>), RuntimeError> {
        match self.run_entry(entry, shown) {
            Ok(Ran::Done(finished, text)) => Ok((finished, text)),
            // 捨てた run のヒープには片付けていないタスクの物体が残るので、誤りの後と同じく入れ替える。`Scheduler` と
            // 登録表は `run_scheduled` が返るときに捨ててあるので、古いヒープの語を持つものは回復の後に残らない
            // (docs/spec/vm.md の「追記」)
            Ok(Ran::Abandoned(finished, cause)) => self.recover(cause).map(|_| (finished, None)),
            // poisoned の `Machine` は回復せず、run の誤りをそのまま返す
            Err(cause) if !cause.is_broken() && !self.poisoned => Err(match self.recover(cause) {
                Ok(cause) | Err(cause) => cause,
            }),
            Err(cause) => Err(cause),
        }
    }

    /// 誤り `cause` の後にヒープを入れ替え、成功したら `cause` を返す。失敗したら、回復の誤りを足した `Unrecovered`
    /// か、poisoned の `Machine` の driver の誤りを返す。
    fn recover(&mut self, cause: RuntimeError) -> Result<RuntimeError, RuntimeError> {
        // カーネルやプールのスレッドが書いている記憶を写したり放したりしないように、ヒープに触れる前に driver を
        // 空にする (docs/spec/object-model.md の「回復」)
        if let Some(driver) = &mut self.driver {
            driver.quiesce(self.job_budget);
            if driver.pending() > 0 {
                self.poisoned = true;
            }
        }
        if self.poisoned {
            return Err(poisoned_fault("<recover>"));
        }
        match self.recover_heap() {
            Ok(()) => Ok(cause),
            Err(reason) => Err(RuntimeError::Unrecovered {
                cause: Box::new(cause),
                reason,
            }),
        }
    }

    fn recover_heap(&mut self) -> Result<(), HeapError> {
        let old = self.heap_work();
        // 根の語は根が持つ参照で、根から届く物体は失敗した実行が書き換えないので、`recover` の前提を守る。ほかの
        // worker のヒープは取り込みを終えて生きていて、どのスレッドも触れない (docs/spec/object-model.md の「回復」)
        unsafe { self.ctx.recover() }?;
        self.others.clear();
        // 回復は共有の根を局所の物体として写すので、次の fork はどの根にも印を付け直す
        self.shared_roots = 0;
        self.earlier = old;
        self.copying = HeapWork::of(self.ctx.heap().stats());
        (self.check_heap)(&mut self.ctx, &mut [])
    }

    /// `Machine` を作ってからの、実行がヒープでした仕事。
    fn heap_work(&self) -> HeapWork {
        let now = HeapWork::of(self.ctx.heap().stats());
        HeapWork {
            rc_increments: self.earlier.rc_increments + now.rc_increments
                - self.copying.rc_increments,
            rc_decrements: self.earlier.rc_decrements + now.rc_decrements
                - self.copying.rc_decrements,
            allocations: self.earlier.allocations + now.allocations - self.copying.allocations,
            peak_objects: self.earlier.peak_objects.max(now.peak_objects),
            shared_objects: self.earlier.shared_objects + now.shared_objects
                - self.copying.shared_objects,
        }
    }

    fn run_entry(&mut self, entry: u32, shown: Option<usize>) -> Result<Ran, RuntimeError> {
        if self.poisoned {
            return Err(poisoned_fault(
                self.code.functions[entry as usize].name.as_str(),
            ));
        }
        // 終わりの欄は run ごとに開く (docs/spec/vm.md の「誤り、止めること、終わり」)
        self.ctx.heap().control().begin_run();
        let (code, ctx) = (&self.code, &mut self.ctx);
        let mut task = Task::new(code, entry, self.max_depth);
        // 実行の後の誤りは、最後に戻った関数の名前を付ける (docs/spec/vm.md の「誤り、止めること、終わり」)
        let after = |fault: VmFault, task: &Task| {
            code.error(
                vm::Failure {
                    fault,
                    pc: task.last_ret,
                },
                task,
            )
        };
        if shown.is_some() {
            if !code.functions[entry as usize].ret_rc {
                return Err(code.error(
                    vm::Failure {
                        fault: VmFault::Internal("a shown entry does not return a string"),
                        pc: task.pc,
                    },
                    &task,
                ));
            }
            task.keep_result = true;
        }
        // driver は run を行うスレッドで作る。作れなければヒープに触れずに終わる (docs/spec/vm.md の「時計と driver」)
        let driver = match self.driver.take() {
            Some(driver) => driver,
            None => {
                self.run_clock = self.clock.start();
                self.run_clock.driver(self.sched.seed()).map_err(|e| {
                    code.error(
                        vm::Failure {
                            fault: VmFault::Driver(e.to_string()),
                            pc: task.pc,
                        },
                        &task,
                    )
                })?
            }
        };
        let mut counts = RunCounts::default();
        let start = Start {
            host: self.host.clone(),
            max_depth: self.max_depth,
            shared_roots: self.shared_roots,
            generations: std::mem::take(&mut self.generations),
            counters: Arc::clone(&self.counters),
            driver,
            run_clock: self.run_clock.clone(),
            doorbell: self.doorbell.clone(),
            signals: std::mem::take(&mut self.signals),
            job_budget: self.job_budget,
        };
        let (result, mut scheduled) = match self.sched {
            Sched::Normal => run_scheduled(code, ctx, &mut task, Normal, start, &mut counts),
            Sched::Sequential => {
                run_scheduled(code, ctx, &mut task, Sequential, start, &mut counts)
            }
            Sched::Seeded(seed) => {
                run_scheduled(code, ctx, &mut task, Seeded::new(seed), start, &mut counts)
            }
            Sched::RescueAlways => run_scheduled(
                code,
                ctx,
                &mut task,
                RescueAlways::default(),
                start,
                &mut counts,
            ),
        };
        self.shared_roots = scheduled.shared_roots;
        // 誤りで終わる run の分も積算に入れる。ほかのヒープは回復で落ちるので、落ちる前に足す
        let o = &scheduled.others_ctx;
        self.others_ctx.evidence_steps += o.evidence_steps;
        self.others_ctx.yields += o.yields;
        self.others_ctx.yield_frames += o.yield_frames;
        self.others_ctx.string_bytes_copied += o.string_bytes_copied;
        self.others_heap.add(&scheduled.others_heap);
        // ほかの worker のヒープは、確かめるか回復するまで `Machine` が持つ。poisoned になる道でも落とさない
        self.others.append(&mut scheduled.others);
        self.generations = std::mem::take(&mut scheduled.generations);
        // 誤りで返る道でも driver を失わないように、run の直後に戻す
        self.driver = scheduled.driver.take();
        if let Some(panic) = scheduled.panic.take() {
            // panic した worker のヒープは途中の状態で漏らしたので、この `Machine` はもう使わない
            // (docs/spec/vm.md の「タスクとスケジュール」)
            self.poisoned = true;
            std::panic::resume_unwind(panic);
        }
        if scheduled.kept_pending {
            // 貸したままの記憶があるので、結果を写さず、終わりの確かめもしない。書き出しはヒープに触れないので、ほかの
            // 誤りと同じく誤りの前に行う。run の誤りがあれば、元の原因としてそれを返す (docs/spec/vm.md の「時計と driver」)
            self.poisoned = true;
            let flushed = ctx.flush();
            let exit = ctx.heap().control().exit_code();
            // 貸したままの仕事があっても `exit` の終了コードは返す。ヒープは写さず、終わりの検査もしない
            if let (
                Some(code),
                Ok(())
                | Err(RuntimeError::Fault {
                    fault: VmFault::Stopped(Stop::Ended),
                    ..
                }),
            ) = (exit, &result)
            {
                flushed.map_err(|fault| after(fault.into(), &task))?;
                return Ok(Ran::Done(
                    self.finished(&counts, &scheduled, Some(code)),
                    None,
                ));
            }
            return Err(match result {
                Ok(())
                | Err(RuntimeError::Fault {
                    fault: VmFault::Stopped(Stop::Ended),
                    ..
                }) => match flushed {
                    Ok(()) => poisoned_fault(code.functions[entry as usize].name.as_str()),
                    Err(fault) => after(fault.into(), &task),
                },
                Err(cause) => cause,
            });
        }
        // 結果は `Finish` が手放さずに残した、VM の持つ参照である。書き出しの前に写すので、写すときの誤りも
        // 書き出しの誤りより先に報告する (docs/spec/vm.md の「誤り、止めること、終わり」)
        // `exit` で終わった実行のエントリの結果はダミーなので写さない
        let exit = ctx.heap().control().exit_code();
        let copied = match (&result, shown, exit) {
            (Ok(()), Some(max_chars), None) => {
                Some(unsafe { ctx.copy_string(task.get(0), max_chars) })
            }
            _ => None,
        };
        // 写して手放した結果は、ほかの worker のヒープの物体でありうる。手放しがそのヒープに積んだ解放を、統計と
        // 検査の前に取り込む (docs/spec/vm.md の「誤り、止めること、終わり」)
        if copied.is_some() && scheduled.workers > 0 {
            reclaim_all(ctx, &mut self.others);
        }
        // 誤りを返す前にも書き出す。`> out 2>&1` のときに出力と誤りの順を保つためである
        let flushed = ctx.flush();
        if let Err(
            cause @ RuntimeError::Fault {
                fault: VmFault::Stopped(Stop::Ended),
                ..
            },
        ) = result
        {
            // タスクの底の `exit` が run を捨てた。終わりの検査はせず、その時点の数で `exit` を報告する
            // (docs/spec/vm.md の「誤り、止めること、終わり」)
            flushed.map_err(|fault| after(fault.into(), &task))?;
            let Some(code) = exit else {
                return Err(after(
                    VmFault::Internal("a run ended without an exit"),
                    &task,
                ));
            };
            let finished = self.finished(&counts, &scheduled, Some(code));
            return Ok(Ran::Abandoned(finished, cause));
        }
        result?;
        let text = copied
            .transpose()
            .map_err(|fault| after(fault.into(), &task))?;
        flushed.map_err(|fault| after(fault.into(), &task))?;
        if !ctx.check_finished() || !self.others.iter().all(Ctx::check_finished) {
            return Err(after(
                VmFault::Internal("the evidence list or a yield is left at the end"),
                &task,
            ));
        }
        // 検査はどのヒープも先にすべて取り込む。ほかの worker のヒープは検査の後に落とす
        // (docs/spec/vm.md の「誤り、止めること、終わり」)
        (self.check_heap)(ctx, &mut self.others)
            .map_err(|error| heap_fault(error, code.function_name(task.last_ret, &task)))?;
        (self.at_run_end)(&mut self.others)?;
        self.others.clear();
        Ok(Ran::Done(self.finished(&counts, &scheduled, exit), text))
    }

    fn finished(
        &self,
        counts: &RunCounts,
        scheduled: &Scheduled<M>,
        exit: Option<i64>,
    ) -> Finished {
        Finished {
            stats: self.run_stats(counts, scheduled),
            exit,
            preemptions: scheduled.preemptions,
            rescues: scheduled.rescues,
            rescued: scheduled.rescued,
            workers: scheduled.workers,
            placed: scheduled.placed,
            placed_tasks: scheduled.placed_tasks,
            placed_jobs: scheduled.placed_jobs,
        }
    }

    /// `Machine` を作ってからの積算の `RunStats`。`peak_stack` と、登録表が数える `tasks`、`parks`、`dropped_failures` だけがこの実行の値である (docs/spec/vm.md の「追記」)。
    fn run_stats(&self, counts: &RunCounts, scheduled: &Scheduled<M>) -> RunStats {
        let c = self.ctx.stats();
        let o = &self.others_ctx;
        let mut h = self.heap_work();
        h.add(&self.others_heap);
        RunStats {
            evidence_steps: c.evidence_steps + o.evidence_steps,
            yields: c.yields + o.yields,
            yield_frames: c.yield_frames + o.yield_frames,
            unwound_frames: counts.unwound_frames,
            string_bytes_copied: c.string_bytes_copied + o.string_bytes_copied,
            rc_increments: h.rc_increments,
            rc_decrements: h.rc_decrements,
            boxes: counts.boxes,
            unboxes: counts.unboxes,
            allocations: h.allocations,
            reuses: counts.reuses,
            peak_objects: h.peak_objects,
            // 統計は検査モードだけで数える
            peak_stack: if M::CHECKED { scheduled.peak_stack } else { 0 },
            tasks: if M::CHECKED { scheduled.tasks.tasks } else { 0 },
            parks: if M::CHECKED { scheduled.tasks.parks } else { 0 },
            dropped_failures: if M::CHECKED {
                scheduled.tasks.dropped_failures
            } else {
                0
            },
            cancels: if M::CHECKED {
                scheduled.tasks.cancels
            } else {
                0
            },
            timers: if M::CHECKED {
                scheduled.tasks.timers
            } else {
                0
            },
            shared_objects: h.shared_objects,
            channel_ops: if M::CHECKED {
                scheduled.tasks.channel_ops
            } else {
                0
            },
            channel_waits: if M::CHECKED {
                scheduled.tasks.channel_waits
            } else {
                0
            },
            channels_swept: if M::CHECKED {
                scheduled.tasks.channels_swept
            } else {
                0
            },
            completions: if M::CHECKED {
                scheduled.tasks.completions
            } else {
                0
            },
        }
    }

    /// 実行の状態を閉じる。根の参照は VM が持つので先に手放す。検査モードでは、その後にヒープの不変条件を確かめ、
    /// 残った物体をリークとして返す。
    pub fn close(mut self) -> Result<(), RuntimeError> {
        if self.poisoned {
            return Err(poisoned_fault("<close>"));
        }
        let close_error = |error| heap_fault(error, "<close>".to_string());
        for root in self.ctx.take_roots().into_iter().flatten() {
            // 根の語は根が持つ参照で、ほかの語には触れないので、`heap_mut` の前提を崩さない
            unsafe { self.ctx.heap_mut().decref(root) }.map_err(close_error)?;
        }
        (self.check_heap)(&mut self.ctx, &mut self.others).map_err(close_error)?;
        self.others.clear();
        (self.at_close)(&mut self.ctx)
    }
}

impl<M: Mode> Drop for Machine<M> {
    fn drop(&mut self) {
        // 本物の driver には、落とすときにカーネルの書き込みの終わりを待つものがあるので、その間はヒープを生かしておく
        self.driver = None;
        if self.poisoned {
            // ほかの worker のヒープの物体も driver に貸されうるので、`ctx` と同じく漏らす
            std::mem::forget(std::mem::take(&mut self.others));
        } else {
            self.others.clear();
            // `ctx` はこの後に使わない
            unsafe { ManuallyDrop::drop(&mut self.ctx) }
        }
    }
}

/// poisoned の `Machine` の誤り。
fn poisoned_fault(function: &str) -> RuntimeError {
    RuntimeError::Fault {
        fault: VmFault::Driver(KEPT_PENDING.to_string()),
        function: function.to_string(),
        at: None,
    }
}

/// 実行と回復の後のヒープの確かめ。2つ目の引数はほかの worker の `Ctx` である。
type CheckHeap<M> = fn(&mut Ctx<M>, &mut [Ctx<M>]) -> Result<(), HeapError>;

/// `run_entry` の結果。
enum Ran {
    /// エントリの関数が戻った。`exit` で終わったときは、`main` の底が終わりの欄に書いてある。
    Done(Finished, Option<String>),
    /// `main` でないタスクの底の `exit` が run を捨てた。誤りは回復が失敗したときの `cause` に使う。
    Abandoned(Finished, RuntimeError),
}

/// 実行が終わった後に、捨てるスケジューラから読むもの。
struct Scheduled<M: Mode> {
    preemptions: u64,
    rescues: u64,
    rescued: u64,
    peak_stack: u64,
    shared_roots: usize,
    tasks: TaskStats,
    /// ほかの worker の `Ctx` が数えた回数の和と、ヒープの仕事の和。
    others_ctx: CtxStats,
    others_heap: HeapWork,
    generations: Vec<u32>,
    /// run の間 `Scheduler` に預けた driver。`Machine` に戻すときに取り出す。
    driver: Option<Box<dyn IoDriver>>,
    /// どれかの worker の driver が、静止させた後にも保留の操作を残した。
    kept_pending: bool,
    /// worker 1..N-1 の `Ctx`。join の後に worker 0 へ返ってきたもの。
    others: Vec<Ctx<M>>,
    /// 作った worker の数。
    workers: usize,
    /// ほかの worker に置いた右の子の数。
    placed: u64,
    /// ほかの worker に置いた `Async` のタスクの数。
    placed_tasks: u64,
    /// ほかの worker に置いた `compute` の仕事の数。
    placed_jobs: u64,
    /// worker 1..N-1 の panic。最初のものを worker 0 で起こし直す。
    panic: Option<Box<dyn std::any::Any + Send>>,
}

/// スケジューラを作るときに `Machine` から渡すもの。
struct Start {
    host: HostRef,
    max_depth: usize,
    shared_roots: usize,
    generations: Vec<u32>,
    counters: Arc<Counters>,
    driver: Box<dyn IoDriver>,
    run_clock: RunClock,
    doorbell: Option<Arc<Doorbell>>,
    signals: Vec<VirtualSignal>,
    job_budget: Duration,
}

/// 方針 `P` のスケジューラを作って `task` から実行する。方針を型の引数にして、方針ごとに VM の繰り返しを作る
/// (docs/spec/vm.md の「タスクとスケジュール」)。
fn run_scheduled<M: Mode, P: Policy>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    policy: P,
    start: Start,
    counts: &mut RunCounts,
) -> (Result<(), RuntimeError>, Scheduled<M>) {
    let main_worker = ctx.heap().worker();
    task.worker = main_worker;
    let control = Arc::clone(ctx.heap().control());
    // run の表は run ごとに作り、監視のスレッドが止まる `Scheduler` の破棄とともに捨てる (docs/spec/vm.md の
    // 「タスクとスケジュール」)
    let workers = control.limits().workers;
    let mut run = Run::new(workers, start.doorbell, start.run_clock, start.counters);
    run.job_budget = start.job_budget;
    let run = Arc::new(run);
    // 登録表も run ごとに1つ作り、worker 0 と、後で作る worker 1..N-1 のすべての `Scheduler` に配る
    // (docs/spec/vm.md の「セルと登録表」)
    let mut registry = Registry::new(start.generations, workers);
    // 予定は最初の命令の前に、どのタイマーよりも先に載せる (docs/spec/vm.md の「時間の待ち」)
    registry.schedule_signals(start.signals.iter().map(|s| (s.at_ms, s.signal.bit())));
    let mut sched = Scheduler::new(
        policy,
        start.driver,
        run,
        control,
        main_worker,
        start.max_depth,
        Arc::new(Shared::new(registry)),
    );
    sched.shared_roots = start.shared_roots;
    // 根は REPL のコードだけが作るので、根に触れるコードを読み込んだ `Machine` は worker を作らない。成功した run の後に
    // 根が、落としたほかの worker のヒープを指しうるためである (docs/spec/vm.md の「タスクとスケジュール」)
    let wanted = sched.begin_run(!code.tables.uses_roots && ctx.roots().len() == 0);
    let host = start.host;
    let (result, exits) = thread::scope(|scope| {
        // worker 1..N-1はrunの最初の命令の前に作るので、命令を2回実行することはない
        // (docs/spec/vm.mdの「タスクとスケジュール」)
        let handles = if wanted {
            spawn_workers::<M, P>(
                scope,
                code,
                &sched.policy,
                &sched.run,
                &sched.watcher.control,
                &sched.cells,
                &host,
                start.max_depth,
            )
        } else {
            Vec::new()
        };
        // workerができたらすぐに見張りを置く。この後でpanicしても、workerを起こしてjoinできるようにするためである
        let shutdown = Shutdown {
            control: Arc::clone(&sched.watcher.control),
            run: Arc::clone(&sched.run),
            armed: !handles.is_empty(),
        };
        // タスクを置くのは作れたworkerだけである
        sched.run.set_live(handles.len() + 1);
        // 1つも作れなければworker 0だけで続ける。救済を強いる方針は、救済するworkerがいるときだけ働く
        sched.workers = match handles.len() {
            0 => Workers::Denied,
            n => {
                sched.policy.set_rescuable(rescuable(&sched.run, 0));
                sched.policy.workers_started();
                Workers::Started(n)
            }
        };
        let result = vm::run::<ExecWith<M, P>>(code, ctx, task, &mut sched, counts);
        // 終わりを決めたのは worker 0 のループか、ほかの worker の致命的な誤りである。どちらでも `END` を立てて
        // すべての worker を起こし、`Ctx` を返させてから join する (docs/spec/vm.md の「タスクとスケジュール」)
        drop(shutdown);
        let exits: Vec<WorkerExit<M>> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap_or_else(Err))
            .collect();
        (result, exits)
    });
    let workers = match sched.workers {
        Workers::Started(n) => n,
        Workers::Denied | Workers::Peer => 0,
    };
    if workers > 0 {
        sched.watcher.control.clear_end();
    }
    let mut others = Vec::with_capacity(exits.len());
    let mut panic = None;
    // ほかの worker の仕事を worker 0 の数に足す。数は足し、山は最大を取る (docs/spec/vm.md の「`RunStats`」)
    let mut preemptions = sched.preemptions();
    let mut rescues = sched.rescues;
    let mut rescued = sched.rescued;
    let mut peak_stack = sched.peak_stack(task);
    let mut bases = Vec::with_capacity(exits.len());
    let mut left = false;
    let mut kept_pending = false;
    for exit in exits {
        match exit {
            Ok(Retired {
                ctx: other,
                counts: worker_counts,
                preemptions: worker_preemptions,
                rescues: worker_rescues,
                rescued: worker_rescued,
                peak_stack: worker_peak,
                base,
                left: worker_left,
                kept_pending: worker_kept,
            }) => {
                counts.add(&worker_counts);
                preemptions += worker_preemptions;
                rescues += worker_rescues;
                rescued += worker_rescued;
                peak_stack = peak_stack.max(worker_peak);
                bases.push(base);
                others.push(other);
                left |= worker_left;
                kept_pending |= worker_kept;
            }
            Err(payload) => {
                panic.get_or_insert(payload);
            }
        }
    }
    let result = settle(result, sched.run.take_fatal());
    // run の外に保留の操作を残さない。誤りの道でも、`Scheduler` と登録表を落とす前に driver を静止させる。どの worker も
    // もう受け付けを送らないので、join の後に行う。ほかの worker の driver は、その worker が抜ける前に静止させてある。
    // 成功した run では、静止させた driver が返した完了を記録に置く。集める地点と同じく、driver に出した操作がなければ
    // 完了を読まない (docs/spec/vm.md の「時計と driver」「誤り、止めること、終わり」)
    kept_pending |= match sched.driver.as_mut() {
        Some(driver) => {
            driver.quiesce(sched.run.job_wait(driver.now()));
            let in_flight = sched.cells.lock().in_flight(sched.index);
            if result.is_ok() && in_flight > 0 {
                let mut completed = Vec::new();
                driver.completions(&mut completed);
                sched.cells.lock().quiesced(sched.index, &mut completed);
            }
            driver.pending() > 0
        }
        None => false,
    };
    // 成功した run だけが、残った知らせと保留の操作を確かめてから、残ったセルを片付ける。誤りと `exit` の run の登録表は、
    // 触れずに `Scheduler` と一緒に落とす。誤りには `Finish` の位置を付ける (docs/spec/vm.md の「誤り、止めること、終わり」)
    let result = result.and_then(|()| {
        let at_finish = |fault: VmFault| {
            code.error(
                vm::Failure {
                    fault,
                    pc: task.pc - 1,
                },
                task,
            )
        };
        workers::end_check(kept_pending, left).map_err(at_finish)?;
        // 予約と確定の間に安全点はないので、`Finish` に来た worker 0 は予約を持たない (docs/spec/vm.md の「channel」)
        debug_assert!(
            sched.hold.is_none(),
            "a channel reservation is held at the end"
        );
        cells::sweep_all(&sched.cells, ctx, task.depth_offset)
            .map_err(|fault| at_finish(fault.into()))
    });
    let (tasks, placed_tasks) = {
        let cells = sched.cells.lock();
        (cells.stats, cells.placed_tasks)
    };
    // join の後に、ほかのスレッドが積んだ解放をすべてのヒープで取り込む。この後はどのスレッドも解放を積まない
    // (docs/spec/vm.md の「誤り、止めること、終わり」)
    if workers > 0 {
        reclaim_all(ctx, &mut others);
    }
    // ほかのヒープの統計は、取り込みの後に読む。`Machine` が `others` を落とす前だからである
    let mut others_ctx = CtxStats::default();
    let mut others_heap = HeapWork::default();
    for (other, base) in others.iter().zip(&bases) {
        let c = other.stats();
        others_ctx.evidence_steps += c.evidence_steps;
        others_ctx.yields += c.yields;
        others_ctx.yield_frames += c.yield_frames;
        others_ctx.string_bytes_copied += c.string_bytes_copied;
        others_heap.add(&HeapWork::of(other.heap().stats()).since(base));
    }
    let generations = sched.cells.lock().next_generations();
    let scheduled = Scheduled {
        preemptions,
        rescues,
        rescued,
        peak_stack,
        shared_roots: sched.shared_roots,
        tasks,
        others_ctx,
        others_heap,
        generations,
        driver: sched.driver.take(),
        kept_pending,
        others,
        workers,
        placed: sched.run.placed.load(std::sync::atomic::Ordering::Relaxed),
        placed_tasks,
        placed_jobs: sched
            .run
            .placed_jobs
            .load(std::sync::atomic::Ordering::Relaxed),
        panic,
    };
    (result, scheduled)
}

/// `RunStats` に入るヒープの仕事の数 (docs/spec/vm.md の「`RunStats`」)。
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct HeapWork {
    rc_increments: u64,
    rc_decrements: u64,
    allocations: u64,
    peak_objects: u64,
    shared_objects: u64,
}

impl HeapWork {
    /// ほかのヒープの仕事を足す。回数は足し、物体の数の最大は大きい方を取る。
    fn add(&mut self, other: &HeapWork) {
        self.rc_increments += other.rc_increments;
        self.rc_decrements += other.rc_decrements;
        self.allocations += other.allocations;
        self.peak_objects = self.peak_objects.max(other.peak_objects);
        self.shared_objects += other.shared_objects;
    }

    /// `base` を起点にした仕事。物体の数の最大は、起点を引かずにそのまま残す。
    fn since(self, base: &HeapWork) -> HeapWork {
        HeapWork {
            rc_increments: self.rc_increments - base.rc_increments,
            rc_decrements: self.rc_decrements - base.rc_decrements,
            allocations: self.allocations - base.allocations,
            peak_objects: self.peak_objects,
            shared_objects: self.shared_objects - base.shared_objects,
        }
    }

    pub(crate) fn of(stats: Stats) -> HeapWork {
        HeapWork {
            rc_increments: stats.rc_increments,
            rc_decrements: stats.rc_decrements,
            allocations: stats.allocations,
            peak_objects: stats.peak_live,
            shared_objects: stats.shared,
        }
    }
}

/// 実行の前にモードを選ぶ呼び出し側のための `Machine`。
pub enum AnyMachine {
    Fast(Machine<Fast>),
    Checked(Machine<Checked>),
}

impl AnyMachine {
    pub fn new(config: &RunConfig, host: HostRef) -> AnyMachine {
        let control = Arc::clone(&config.control);
        if config.debug_heap {
            let mut machine = Machine::<Checked>::new(control, host)
                .with_sched(config.sched)
                .with_clock(config.clock)
                .with_signals(config.signals.clone())
                .with_job_budget(config.job_budget);
            machine.doorbell = config.doorbell.clone();
            AnyMachine::Checked(machine)
        } else {
            let mut machine = Machine::<Fast>::new(control, host)
                .with_sched(config.sched)
                .with_clock(config.clock)
                .with_signals(config.signals.clone())
                .with_job_budget(config.job_budget);
            machine.doorbell = config.doorbell.clone();
            AnyMachine::Fast(machine)
        }
    }

    pub fn load(&mut self, program: &Program, linked: &Linked) -> Result<Loaded, CompileError> {
        match self {
            AnyMachine::Fast(machine) => machine.load(program, linked),
            AnyMachine::Checked(machine) => machine.load(program, linked),
        }
    }

    pub fn run(&mut self, entry: u32) -> Result<Finished, RuntimeError> {
        match self {
            AnyMachine::Fast(machine) => machine.run(entry),
            AnyMachine::Checked(machine) => machine.run(entry),
        }
    }

    pub fn run_shown(
        &mut self,
        entry: u32,
        max_chars: usize,
    ) -> Result<(Finished, Option<String>), RuntimeError> {
        match self {
            AnyMachine::Fast(machine) => machine.run_shown(entry, max_chars),
            AnyMachine::Checked(machine) => machine.run_shown(entry, max_chars),
        }
    }

    pub fn write_line(&mut self, line: &str) -> Result<(), Fault> {
        match self {
            AnyMachine::Fast(machine) => machine.write_line(line),
            AnyMachine::Checked(machine) => machine.write_line(line),
        }
    }

    pub fn is_poisoned(&self) -> bool {
        match self {
            AnyMachine::Fast(machine) => machine.is_poisoned(),
            AnyMachine::Checked(machine) => machine.is_poisoned(),
        }
    }

    pub fn close(self) -> Result<(), RuntimeError> {
        match self {
            AnyMachine::Fast(machine) => machine.close(),
            AnyMachine::Checked(machine) => machine.close(),
        }
    }
}

/// ヒープの不変条件の誤りには、最後に戻った関数の名前を付ける (docs/spec/vm.md の「誤り、止めること、終わり」)。
fn heap_fault(error: HeapError, last_function: String) -> RuntimeError {
    RuntimeError::Fault {
        fault: error.into(),
        function: last_function,
        at: None,
    }
}

/// worker 0 とほかの worker のヒープで、ほかのスレッドから受けた解放をすべて取り込む。
fn reclaim_all<M: Mode>(ctx: &mut Ctx<M>, others: &mut [Ctx<M>]) {
    ctx.reclaim();
    for other in others {
        other.reclaim();
    }
}

/// 検査モードの `check_heap`。どのヒープの不変条件も確かめてから (どれも先に取り込む)、すべてのヒープで ID ごとの
/// 数を数え直す。数え直しは取り込みを終えたヒープにしか使えない (docs/spec/object-model.md の「論理的な worker ID」)。
fn check_heaps(ctx: &mut Ctx<Checked>, others: &mut [Ctx<Checked>]) -> Result<(), HeapError> {
    ctx.check_invariants()?;
    for other in others.iter_mut() {
        other.check_invariants()?;
    }
    let heaps: Vec<_> = std::iter::once(ctx.heap())
        .chain(others.iter().map(Ctx::heap))
        .collect();
    ctx.heap().control().check_owned(&heaps)
}

/// 検査モードの `at_run_end`。成功した run の後のほかの worker のヒープには、生きている物体が残らない。根を持つ
/// セッションは worker を作らず、どの子の結果も親が手放しているためである。
fn check_others(others: &mut [Ctx<Checked>]) -> Result<(), RuntimeError> {
    others.iter_mut().try_for_each(check_leaks)
}

fn check_leaks(ctx: &mut Ctx<Checked>) -> Result<(), RuntimeError> {
    let leaks = ctx.leaks();
    if leaks.is_empty() {
        Ok(())
    } else {
        Err(RuntimeError::Leak(
            leaks
                .into_iter()
                .map(|(name, n)| (name.to_string(), n))
                .collect(),
        ))
    }
}

/// コード生成の誤りは、正しいプログラムでは起きない内部の誤りである。poisoned の `Machine` の `Poisoned` だけは driver
/// の誤りにする。REPL は `Machine::load` の誤りをこれで報告する。
pub fn compile_error(program: &Program, error: CompileError) -> RuntimeError {
    let (what, function) = match error {
        CompileError::TooManySlots { function } => {
            ("a function needs more than 65535 slots", function)
        }
        CompileError::Freeze(_) => (
            "the constants cannot be frozen",
            program.function(program.entry).name.clone(),
        ),
        CompileError::Mismatch { function } => {
            ("a function with a known name has other code", function)
        }
        CompileError::MultiValueEntry { function } => {
            ("an entry function returns more than one value", function)
        }
        CompileError::MissingFork { function } => {
            ("a program that may fork lacks a fork function", function)
        }
        CompileError::MissingEntry { function } => (
            "a program that runs Async tasks lacks a runtime entry",
            function,
        ),
        CompileError::Poisoned => {
            return poisoned_fault(program.function(program.entry).name.as_str());
        }
    };
    RuntimeError::Fault {
        fault: VmFault::Internal(what),
        function,
        at: None,
    }
}
