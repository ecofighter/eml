//! LIR を実行するレジスタ型のバイトコード VM (docs/spec/vm.md)。`eml_rt` の `Ctx` の上で動き、フロントエンドに
//! 依存しない。

use std::mem::ManuallyDrop;
use std::sync::Arc;

use eml_lir::Program;
use eml_rt::{Checked, Ctx, Fast, Fault, HeapError, IoDriver, Mode, Stats};

use crate::cells::TaskStats;
use crate::driver::DriverKind;
use crate::error::KEPT_PENDING;
use crate::vm::ExecWith;

mod apply;
mod cells;
mod code;
mod compile;
mod data;
mod disasm;
mod doorbell;
mod driver;
mod error;
mod hosts;
mod monitor;
mod op;
mod sched;
mod vm;

pub use code::{CodeBase, CompileError, FnEntry};
pub use disasm::disassemble;
pub use doorbell::Doorbell;
pub use driver::{Clock, Signal, VirtualSignal};
pub use eml_rt::{Control, HostRef, Limits, Stop};
pub use error::{RuntimeError, SourceLocation, VmFault};
pub use hosts::{Capture, CaptureHost, Written, capture_host, script_host, stdio_host};
pub use sched::{Normal, Policy, Sched, Scheduler, Seeded, Sequential};
pub use vm::Task;

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
    /// `Async` の待ち、`yield_now`、`sleep_ms`、`drain_within`、channel の待ち、driver の操作の待ちが、継続をランタイムに
    /// 渡した数。
    pub parks: u64,
    /// スコープの最初の失敗より後の、タスクの失敗の数。
    pub dropped_failures: u64,
    /// 取り消しが届いた回数。待つ前に取り消しを見つけた `park`、`park_time`、`park_drain`、`park_send`、`park_recv`、
    /// `park_io`、受け箱の処理が外した待ちと取り消しを頼んだ操作の待ち、取り消しで再開する `yield_now`、真を返した
    /// `cancel_point`、偽を返した `claim_scope` を数える (docs/spec/vm.md の「`RunStats`」)。
    pub cancels: u64,
    /// `await_time` が時間の輪に登録した回数。
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
}

/// VM だけが知る仕事の回数。検査モードだけで数える。
#[derive(Debug, Default)]
pub(crate) struct RunCounts {
    pub(crate) unwound_frames: u64,
    pub(crate) boxes: u64,
    pub(crate) unboxes: u64,
    pub(crate) reuses: u64,
}

/// 実行の設定。上限は `Control` が持つので、ここに上限の欄は置かない。`file_root` は Host が持つ
/// (docs/spec/vm.md の「置き場所」)。
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
    /// シグナルを見に行く。`Clock::Real` の run に渡すと、run の始めに監視のスレッドが起き、VM は空きの待ちを切らない。
    /// そのため、止める語を立てる者は自己パイプも鳴らす (docs/spec/vm.md の「タスクとスケジュール」)。
    pub doorbell: Option<Arc<Doorbell>>,
    /// 仮想のシグナルの予定。既定は空で、テストのハーネスだけが使う (docs/spec/vm.md の「時間の待ち」)。
    pub signals: Vec<VirtualSignal>,
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
}

/// プログラムを1回だけ実行する。新しい `Machine` に読み込み、エントリを実行し、閉じる。
pub fn execute(
    program: &Program,
    config: &RunConfig,
    host: HostRef,
) -> Result<Finished, RuntimeError> {
    let mut machine = AnyMachine::new(config, host);
    let loaded = machine
        .load(program)
        .map_err(|error| compile_error(program, error))?;
    let finished = machine.run(loaded.entry)?;
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
/// 決める。
pub struct Machine<M: Mode> {
    /// 時計と待ち。`with_driver` が渡すか、最初の run が `clock` と方針の seed から作り、run の間は `Scheduler` に
    /// 移す。`with_clock` と `with_sched` は `None` に戻し、次の run が作り直す。作った後は時刻が run をまたいで続く。
    /// driver が貸した記憶はヒープにあるので、`Drop` は `ctx` より先にこの欄を落とす (docs/spec/vm.md の「時計と driver」)。
    driver: Option<Box<dyn IoDriver>>,
    /// 持っている driver の出どころ。`with_driver` で渡した driver は、時計の欄によらず `Injected` にする。出どころを
    /// 知らない driver の run には OS のシグナルを中継せず、眠る driver として空きの待ちを切る
    /// (docs/spec/vm.md の「時計と driver」)。
    driver_kind: DriverKind,
    code: CodeBase<M>,
    /// poisoned の `Machine` では、driver が貸したままの記憶を放さないように、`Drop` が落とさずに漏らす。
    ctx: ManuallyDrop<Ctx<M>>,
    /// `quiesce` の後にも driver が保留の操作を残した。この後の run、回復、`close` はヒープに触れずに driver の誤りを
    /// 返す (docs/spec/vm.md の「時計と driver」)。
    poisoned: bool,
    max_depth: usize,
    sched: Sched,
    /// 実行と回復の後の確かめ。検査モードではヒープの不変条件を確かめる。
    check_heap: fn(&Ctx<M>) -> Result<(), HeapError>,
    /// 閉じるときの確かめ。検査モードでは残った物体をリークとして返す。
    at_close: fn(&Ctx<M>) -> Result<(), RuntimeError>,
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
    counters: Vec<i64>,
    /// driver を作るときの時計。
    clock: Clock,
    /// 実行ごとの `Scheduler` に渡す自己パイプ。1つの自己パイプは1つの `Machine` だけに渡す
    /// (docs/spec/vm.md の「タスクとスケジュール」)。
    doorbell: Option<Arc<Doorbell>>,
    /// 次の run だけに渡す仮想のシグナルの予定。run が取り出すので、後の run は再生しない
    /// (docs/spec/vm.md の「時間の待ち」)。
    signals: Vec<VirtualSignal>,
}

impl Machine<Fast> {
    pub fn new(control: Arc<Control>, host: HostRef) -> Machine<Fast> {
        Machine::with_checks(control, host, |_| Ok(()), |_| Ok(()))
    }
}

impl Machine<Checked> {
    pub fn new(control: Arc<Control>, host: HostRef) -> Machine<Checked> {
        Machine::with_checks(
            control,
            host,
            |ctx| ctx.heap().check_invariants(),
            check_leaks,
        )
    }
}

impl<M: Mode> Machine<M> {
    fn with_checks(
        control: Arc<Control>,
        host: HostRef,
        check_heap: fn(&Ctx<M>) -> Result<(), HeapError>,
        at_close: fn(&Ctx<M>) -> Result<(), RuntimeError>,
    ) -> Machine<M> {
        Machine {
            code: CodeBase::new(Arc::clone(&control)),
            max_depth: control.limits().max_depth,
            ctx: ManuallyDrop::new(Ctx::new(control, host)),
            poisoned: false,
            sched: Sched::Sequential,
            check_heap,
            at_close,
            earlier: HeapWork::default(),
            copying: HeapWork::default(),
            shared_roots: 0,
            generations: Vec::new(),
            counters: Vec::new(),
            clock: Clock::Virtual,
            driver: None,
            driver_kind: DriverKind::Virtual,
            doorbell: None,
            signals: Vec::new(),
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
    /// run に渡すと、run の始めに監視のスレッドが起き、VM は空きの待ちを切らない。そのため、止める語を立てる者は
    /// 自己パイプも鳴らす (docs/spec/vm.md の「タスクとスケジュール」)。
    pub fn with_doorbell(mut self, doorbell: Arc<Doorbell>) -> Machine<M> {
        self.doorbell = Some(doorbell);
        self
    }

    /// 次の run で時間の輪に載せる仮想のシグナルの予定を渡す。期限は driver を作ったときからのミリ秒である。
    pub fn with_signals(mut self, signals: Vec<VirtualSignal>) -> Machine<M> {
        self.signals = signals;
        self
    }

    /// 次の run から使う driver を渡す。後の `with_clock` と `with_sched` はこれを捨てる。driver は `Machine` を使う
    /// スレッドの上で作る。渡すのは眠る driver に限る。VM はその run の空きの待ちを 10 ms ごとに切るので、期限まで
    /// 時計を進める driver では、終わらない保留のある run がデッドロックで止まらない (docs/spec/vm.md の
    /// 「時計と driver」)。
    pub fn with_driver(mut self, driver: Box<dyn IoDriver>) -> Machine<M> {
        self.driver = Some(driver);
        self.driver_kind = DriverKind::Injected;
        self
    }

    pub fn code(&self) -> &CodeBase<M> {
        &self.code
    }

    /// プログラムを追記する。誤りを返した後の `Machine` は使い続けない (`CodeBase::load`)。
    pub fn load(&mut self, program: &Program) -> Result<Loaded, CompileError> {
        if self.poisoned {
            return Err(CompileError::Poisoned);
        }
        let entry = self.code.load(program)?;
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
            driver.quiesce();
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
        // 根の語は根が持つ参照で、根から届く物体は失敗した実行が書き換えないので、`recover` の前提を守る
        unsafe { self.ctx.recover() }?;
        // 回復は共有の根を局所の物体として写すので、次の fork はどの根にも印を付け直す
        self.shared_roots = 0;
        self.earlier = old;
        self.copying = HeapWork::of(self.ctx.heap().stats());
        (self.check_heap)(&self.ctx)
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
                self.driver_kind = self.clock.kind();
                self.clock.driver(self.sched.seed()).map_err(|e| {
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
            max_depth: self.max_depth,
            shared_roots: self.shared_roots,
            generations: std::mem::take(&mut self.generations),
            counters: std::mem::take(&mut self.counters),
            driver,
            driver_kind: self.driver_kind,
            doorbell: self.doorbell.clone(),
            signals: std::mem::take(&mut self.signals),
        };
        let (result, mut scheduled) = match self.sched {
            Sched::Normal => run_scheduled(code, ctx, &mut task, Normal, start, &mut counts),
            Sched::Sequential => {
                run_scheduled(code, ctx, &mut task, Sequential, start, &mut counts)
            }
            Sched::Seeded(seed) => {
                run_scheduled(code, ctx, &mut task, Seeded::new(seed), start, &mut counts)
            }
        };
        self.shared_roots = scheduled.shared_roots;
        self.generations = std::mem::take(&mut scheduled.generations);
        self.counters = std::mem::take(&mut scheduled.counters);
        // 誤りで返る道でも driver を失わないように、run の直後に戻す
        self.driver = scheduled.driver.take();
        if scheduled.kept_pending {
            // 貸したままの記憶があるので、結果を写さず、終わりの確かめもしない。書き出しはヒープに触れないので、ほかの
            // 誤りと同じく誤りの前に行う。run の誤りがあれば、元の原因としてそれを返す (docs/spec/vm.md の「時計と driver」)
            self.poisoned = true;
            let flushed = ctx.flush();
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
            let finished = Finished {
                stats: self.run_stats(&counts, &scheduled),
                exit: Some(code),
                preemptions: scheduled.preemptions,
            };
            return Ok(Ran::Abandoned(finished, cause));
        }
        result?;
        let text = copied
            .transpose()
            .map_err(|fault| after(fault.into(), &task))?;
        flushed.map_err(|fault| after(fault.into(), &task))?;
        if !ctx.check_finished() {
            return Err(after(
                VmFault::Internal("the evidence list or a yield is left at the end"),
                &task,
            ));
        }
        (self.check_heap)(ctx)
            .map_err(|error| heap_fault(error, code.function_name(task.last_ret, &task)))?;
        let finished = Finished {
            stats: self.run_stats(&counts, &scheduled),
            exit,
            preemptions: scheduled.preemptions,
        };
        Ok(Ran::Done(finished, text))
    }

    /// `Machine` を作ってからの積算の `RunStats`。`peak_stack` と、登録表が数える `tasks`、`parks`、`dropped_failures` だけがこの実行の値である (docs/spec/vm.md の「追記」)。
    fn run_stats(&self, counts: &RunCounts, scheduled: &Scheduled) -> RunStats {
        let c = self.ctx.stats();
        let h = self.heap_work();
        RunStats {
            evidence_steps: c.evidence_steps,
            yields: c.yields,
            yield_frames: c.yield_frames,
            unwound_frames: counts.unwound_frames,
            string_bytes_copied: c.string_bytes_copied,
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
        (self.check_heap)(&self.ctx).map_err(close_error)?;
        (self.at_close)(&self.ctx)
    }
}

impl<M: Mode> Drop for Machine<M> {
    fn drop(&mut self) {
        // 本物の driver には、落とすときにカーネルの書き込みの終わりを待つものがあるので、その間はヒープを生かしておく
        self.driver = None;
        if !self.poisoned {
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

/// `run_entry` の結果。
enum Ran {
    /// エントリの関数が戻った。`exit` で終わったときは、`main` の底が終わりの欄に書いてある。
    Done(Finished, Option<String>),
    /// `main` でないタスクの底の `exit` が run を捨てた。誤りは回復が失敗したときの `cause` に使う。
    Abandoned(Finished, RuntimeError),
}

/// 実行が終わった後に、捨てるスケジューラから読むもの。
struct Scheduled {
    preemptions: u64,
    peak_stack: u64,
    shared_roots: usize,
    tasks: TaskStats,
    generations: Vec<u32>,
    counters: Vec<i64>,
    /// run の間 `Scheduler` に預けた driver。`Machine` に戻すときに取り出す。
    driver: Option<Box<dyn IoDriver>>,
    /// run の直後の `quiesce` の後にも、driver が保留の操作を残した。
    kept_pending: bool,
}

/// スケジューラを作るときに `Machine` から渡すもの。
struct Start {
    max_depth: usize,
    shared_roots: usize,
    generations: Vec<u32>,
    counters: Vec<i64>,
    driver: Box<dyn IoDriver>,
    driver_kind: DriverKind,
    doorbell: Option<Arc<Doorbell>>,
    signals: Vec<VirtualSignal>,
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
) -> (Result<(), RuntimeError>, Scheduled) {
    let main_worker = ctx.heap().worker();
    task.worker = main_worker;
    let mut sched = Scheduler::new(
        policy,
        start.driver,
        Arc::clone(ctx.heap().control()),
        start.doorbell,
        main_worker,
        start.max_depth,
        start.generations,
    );
    sched.shared_roots = start.shared_roots;
    sched.driver_kind = start.driver_kind;
    sched.cells.counters = start.counters;
    // 予定は最初の命令の前に、どのタイマーよりも先に載せる (docs/spec/vm.md の「時間の待ち」)
    sched
        .cells
        .schedule_signals(start.signals.iter().map(|s| (s.at_ms, s.signal.bit())));
    sched.begin_run();
    let result = vm::run::<ExecWith<M, P>>(code, ctx, task, &mut sched, counts);
    // run の外に保留の操作を残さない。誤りの道でも、`Scheduler` と登録表を落とす前に片付ける
    // (docs/spec/vm.md の「時計と driver」)
    sched.driver.quiesce();
    let kept_pending = sched.driver.pending() > 0;
    let scheduled = Scheduled {
        preemptions: sched.preemptions(),
        peak_stack: sched.peak_stack(task),
        shared_roots: sched.shared_roots,
        tasks: sched.cells.stats,
        generations: sched.cells.next_generations(),
        counters: std::mem::take(&mut sched.cells.counters),
        driver: Some(sched.driver),
        kept_pending,
    };
    (result, scheduled)
}

/// `RunStats` に入るヒープの仕事の数 (docs/spec/vm.md の「`RunStats`」)。
#[derive(Debug, Clone, Copy, Default)]
struct HeapWork {
    rc_increments: u64,
    rc_decrements: u64,
    allocations: u64,
    peak_objects: u64,
    shared_objects: u64,
}

impl HeapWork {
    fn of(stats: Stats) -> HeapWork {
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
                .with_signals(config.signals.clone());
            machine.doorbell = config.doorbell.clone();
            AnyMachine::Checked(machine)
        } else {
            let mut machine = Machine::<Fast>::new(control, host)
                .with_sched(config.sched)
                .with_clock(config.clock)
                .with_signals(config.signals.clone());
            machine.doorbell = config.doorbell.clone();
            AnyMachine::Fast(machine)
        }
    }

    pub fn load(&mut self, program: &Program) -> Result<Loaded, CompileError> {
        match self {
            AnyMachine::Fast(machine) => machine.load(program),
            AnyMachine::Checked(machine) => machine.load(program),
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

fn check_leaks(ctx: &Ctx<Checked>) -> Result<(), RuntimeError> {
    let leaks = ctx.heap().leaks();
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
        CompileError::MissingFork { function } => (
            "a program that may run a par lacks a fork function",
            function,
        ),
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
