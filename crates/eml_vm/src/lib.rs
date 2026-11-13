//! LIR を実行するレジスタ型のバイトコード VM (docs/spec/vm.md)。`eml_rt` の `Ctx` の上で動き、フロントエンドに
//! 依存しない。

use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use eml_lir::Program;
use eml_rt::{
    Checked, Control, Ctx, CtxStats, Fast, Fault, HeapError, IoDriver, Mode, Stats, Stop,
};
use eml_sched::config::{RunStats, Sched, Seat};
use eml_sched::doorbell::Doorbell;
use eml_sched::driver::{Clock, VirtualSignal};
use eml_sched::error::RunFault;
use eml_sched::park::{DefaultPark, Park, ParkFactory};

use crate::cells::{Registry, Shared, TaskStats};
use crate::counters::Counters;
use crate::driver::{JOB_QUIESCE_LIMIT, RunClock};
use crate::vm::ExecWith;
use crate::workers::{
    Panicked, Retired, Run, Shutdown, WorkerExit, Workers, settle, spawn_workers,
};

mod apply;
mod code;
mod compile;
mod counters;
mod data;
mod disasm;
mod ffi;
mod gui;
mod hosts;
mod jobs;
mod monitor;
mod op;
mod sched;
mod vm;
mod workers;

pub use code::{CodeBase, CompileError, FnEntry};
pub use disasm::disassemble;
pub use eml_cabi::Linked;
/// GUIのtoolkitの層。`eml run`はここのbackendと筋書きでGUIの`Park`を作る (docs/spec/vm.mdの「GUIのPark」)。
#[cfg(feature = "gui")]
pub use eml_gui;
pub use eml_rt::HostRef;
pub use eml_sched::config::{Finished, RunConfig};
pub use eml_sched::error::RuntimeError;
pub(crate) use eml_sched::{cells, doorbell, driver, error, park};
#[cfg(feature = "gui")]
pub use gui::park as gui_park;
pub use hosts::{Capture, CaptureHost, Written, capture_host};
pub use sched::{Memory, Normal, Policy, Scheduler, Seeded, Sequential};
pub use vm::Task;
pub use workers::{Entry, Ready};

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

/// VM の worker のスレッドのスタックの大きさ。VM のスタックは `Task` のデータでヒープの外にあるので、Rust の再帰の
/// 深さだけを受け持てばよい。
pub(crate) const WORKER_STACK: usize = 8 << 20;

/// プログラムを1回だけ実行する。呼んだスレッドで`Seat::Separate`の新しい`Machine`を作り、読み込み、エントリを
/// 実行して閉じる。runの間、呼んだスレッドはメインのworkerで、`main`はrunが作るworker 0で動く
/// (docs/spec/vm.mdの「上限と割り込み」)。
pub fn execute(
    program: &Program,
    linked: &Linked,
    config: &RunConfig,
    host: HostRef,
) -> Result<Finished, RuntimeError> {
    let config = config.clone().with_seat(Seat::Separate);
    let mut machine = AnyMachine::new(&config, host);
    let loaded = machine
        .load(program, linked)
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
/// 決める。渡す `Control` を同時に使う `Machine` は1つだけにする。ID の表と数が `Control` にあるためである
/// (docs/spec/object-model.md の「実行の制御」)。
pub struct Machine<M: Mode> {
    /// 時計と待ち。`with_driver` が渡すか、最初の run が `clock` と方針の seed から作り、run の間は `Scheduler` に
    /// 移す。`with_clock` と `with_sched` は `None` に戻し、次の run が作り直す。作った後は時刻が run をまたいで続く。
    /// 分かれた run の driver は worker 0 が作って落とすので、この欄は `None` のままである。
    /// driver が貸した記憶はヒープにあるので、`Drop` は `ctx` より先にこの欄を落とす (docs/spec/vm.md の「時計と driver」)。
    driver: Option<Box<dyn IoDriver>>,
    /// `driver`の時間の基準。`new`、`with_clock`、`with_sched`の後の最初のrunが作り、後のrunはどちらのseatでも
    /// 持ち続けるので、続けたrunは1つの基準で時刻を読む。runの`Run`に渡し、driverの出どころもこれが決める。
    /// `with_driver`で渡したdriverは、時計の欄によらず基準を`Injected`にする。出どころを知らないdriverのrunには
    /// OSのシグナルを中継せず、眠るdriverとして空きの待ちを切る (docs/spec/vm.mdの「時計と driver」)。
    run_clock: Option<RunClock>,
    code: CodeBase<M>,
    /// `ctx` と同じ Host。worker 1..N-1 の `Ctx` に写す。
    host: HostRef,
    /// worker 0の`Ctx`。
    ctx: Ctx<M>,
    /// worker 1..N-1 が run の終わりに返した `Ctx`。成功した run では終わりの検査の後に落とし、ほかの道では回復の
    /// 後に落とす。回復はほかのヒープのページから写しうるためである。回復しなかった誤りの後は `Drop` まで残す
    /// (docs/spec/object-model.md の「回復」)。
    others: Vec<Ctx<M>>,
    /// runの間にworkerがpanicした。この後の`load`、`run`、`close`は、ヒープに触れずにpanicする
    /// (docs/spec/vm.mdの「タスクとスケジュール」)。
    panicked: bool,
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
    seat: Seat,
    main_park: Option<ParkFactory>,
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
            ctx: Ctx::new(control, host),
            others: Vec::new(),
            panicked: false,
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
            run_clock: None,
            doorbell: None,
            signals: Vec::new(),
            job_budget: JOB_QUIESCE_LIMIT,
            seat: Seat::Merged,
            main_park: None,
        }
    }

    /// 実行ごとに作るスケジューラの方針を選ぶ。既定は `Sched::Sequential` である。seed は driver に渡すので、次の run
    /// が時間の基準と driver を作り直す。
    pub fn with_sched(mut self, sched: Sched) -> Machine<M> {
        self.sched = sched;
        self.driver = None;
        self.run_clock = None;
        self
    }

    /// タイマーが使う時計を選ぶ。既定は `Clock::Virtual` である。次の run が新しい時間の基準と driver を作る。
    pub fn with_clock(mut self, clock: Clock) -> Machine<M> {
        self.clock = clock;
        self.driver = None;
        self.run_clock = None;
        self
    }

    /// `Machine`を持つスレッドとworker 0の関係を選ぶ。既定は`Seat::Merged`である。`Seat::Separate`のrunは、
    /// セッションの根に触れるコードと渡されたdriverを拒む (docs/spec/vm.mdの「メインのworker」)。
    pub fn with_seat(mut self, seat: Seat) -> Machine<M> {
        self.seat = seat;
        self
    }

    /// 分かれたrunのメインのworkerの`Park`を作るものを渡す。runごとに`Machine`を持つスレッドで1つ作る。渡さなければ
    /// `DefaultPark`で、合わせたrunは使わない (docs/spec/vm.mdの「メインのworkerの待ち」)。
    pub fn with_main_park(mut self, main_park: ParkFactory) -> Machine<M> {
        self.main_park = Some(main_park);
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
        self.run_clock = Some(RunClock::Injected);
        self
    }

    pub fn code(&self) -> &CodeBase<M> {
        &self.code
    }

    /// プログラムを追記する。`linked` は `eml_native` がこのプログラムの C の表をつないだ結果である。誤りを返した後の
    /// `Machine` は使い続けない (`CodeBase::load`)。
    pub fn load(&mut self, program: &Program, linked: &Linked) -> Result<Loaded, CompileError> {
        self.refuse_after_panic();
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

    /// runがpanicした`Machine`はもう使わない。runのタスクとセルが途中のまま残っているためである。
    fn refuse_after_panic(&self) {
        assert!(!self.panicked, "a Machine is used after a run panicked");
    }

    fn run_recovering(
        &mut self,
        entry: u32,
        shown: Option<usize>,
    ) -> Result<(Finished, Option<String>), RuntimeError> {
        self.refuse_after_panic();
        match self.run_entry(entry, shown) {
            Ok(Ran::Done(finished, text)) => Ok((finished, text)),
            // 捨てた run のヒープには片付けていないタスクの物体が残るので、誤りの後と同じく入れ替える。`Scheduler` と
            // 登録表は `run_scheduled` が返るときに捨ててあるので、古いヒープの語を持つものは回復の後に残らない
            // (docs/spec/vm.md の「追記」)
            Ok(Ran::Abandoned(finished, cause)) => self.recover(cause).map(|_| (finished, None)),
            Err(cause) if !cause.is_broken() => Err(match self.recover(cause) {
                Ok(cause) | Err(cause) => cause,
            }),
            Err(cause) => Err(cause),
        }
    }

    /// 誤り`cause`の後にヒープを入れ替え、成功したら`cause`を返す。失敗したら、回復の誤りを足した`Unrecovered`を
    /// 返す。
    fn recover(&mut self, cause: RuntimeError) -> Result<RuntimeError, RuntimeError> {
        // カーネルやプールのスレッドが書いている記憶を写したり放したりしないように、ヒープに触れる前にdriverを
        // 空にする。それでも操作を残すdriverは、ここで落とす (docs/spec/object-model.mdの「回復」)
        if let Some(driver) = &mut self.driver {
            driver.quiesce(self.job_budget);
            if driver.pending() > 0 {
                self.drop_kept_driver();
            }
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

    /// 静止の後にも操作を残したdriverを落とす。渡されたdriverの代わりに、次のrunは`clock`からdriverを作る
    /// (docs/spec/vm.mdの「時計と driver」)。
    fn drop_kept_driver(&mut self) {
        self.driver = None;
        if matches!(self.run_clock, Some(RunClock::Injected)) {
            self.run_clock = None;
        }
    }

    fn run_entry(&mut self, entry: u32, shown: Option<usize>) -> Result<Ran, RuntimeError> {
        // 終わりの欄は run ごとに開く (docs/spec/vm.md の「誤り、止めること、終わり」)
        self.ctx.heap().control().begin_run();
        let (code, ctx) = (&self.code, &mut self.ctx);
        let mut task = Task::new(code, entry, self.max_depth);
        // 実行の後の誤りは、最後に戻った関数の名前を付ける (docs/spec/vm.md の「誤り、止めること、終わり」)
        let after = |fault: RunFault, task: &Task| {
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
                        fault: RunFault::Internal("a shown entry does not return a string"),
                        pc: task.pc,
                    },
                    &task,
                ));
            }
            task.keep_result = true;
        }
        let separate = self.seat == Seat::Separate;
        let refused = if !separate {
            None
        } else if code.tables.uses_roots || ctx.roots().len() > 0 {
            // runの後に根が、落としたメインのworkerのヒープを指しうる
            // (docs/spec/vm.mdの「誤り、止めること、終わり」)
            Some("code reaching session roots cannot run with a main worker on another thread")
        } else if self.driver.is_some() {
            // driver は作ったスレッドを離れられないので、分かれた run の driver は worker 0 が作る
            Some("a separate run makes its driver on worker 0")
        } else {
            None
        };
        if let Some(what) = refused {
            return Err(code.error(
                vm::Failure {
                    fault: RunFault::Internal(what),
                    pc: task.pc,
                },
                &task,
            ));
        }
        let clock = self.clock;
        let run_clock = self.run_clock.get_or_insert_with(|| clock.start()).clone();
        // driver は run を行うスレッドで作る。作れなければヒープに触れずに終わる。分かれた run では worker 0 が自分の
        // スレッドで作る (docs/spec/vm.md の「時計と driver」)
        let driver = match (separate, self.driver.take()) {
            (true, _) => None,
            (false, Some(driver)) => Some(driver),
            (false, None) => Some(
                run_clock
                    .driver(self.sched.seed())
                    .map(|(driver, _)| driver)
                    .map_err(|e| {
                        code.error(
                            vm::Failure {
                                fault: RunFault::Driver(e.to_string()),
                                pc: task.pc,
                            },
                            &task,
                        )
                    })?,
            ),
        };
        let mut counts = RunCounts::default();
        let start = Start {
            host: self.host.clone(),
            max_depth: self.max_depth,
            shared_roots: self.shared_roots,
            generations: std::mem::take(&mut self.generations),
            counters: Arc::clone(&self.counters),
            driver,
            seed: self.sched.seed(),
            seat: self.seat,
            main_park: self.main_park.clone(),
            run_clock,
            doorbell: self.doorbell.clone(),
            signals: std::mem::take(&mut self.signals),
            job_budget: self.job_budget,
        };
        let sched = self.sched;
        let outcome = catch_unwind(AssertUnwindSafe(|| match sched {
            Sched::Normal => run_scheduled(code, ctx, &mut task, Normal, start, &mut counts),
            Sched::Sequential => {
                run_scheduled(code, ctx, &mut task, Sequential, start, &mut counts)
            }
            Sched::Seeded(seed) => {
                run_scheduled(code, ctx, &mut task, Seeded::new(seed), start, &mut counts)
            }
        }));
        let (result, mut scheduled, driver) = match outcome {
            Ok(outcome) => outcome,
            Err(payload) => {
                // ここに届くpanicは、`entry_body`が捕まえる区間の外のworker 0のスレッドか、runの終わりから来る。もうrunの
                // どのスレッドもjoinしてある (docs/spec/vm.mdの「タスクとスケジュール」)
                self.panicked = true;
                resume_unwind(payload);
            }
        };
        self.shared_roots = scheduled.shared_roots;
        // 誤りで終わる run の分も積算に入れる。ほかのヒープは回復で落ちるので、落ちる前に足す
        let o = &scheduled.others_ctx;
        self.others_ctx.evidence_steps += o.evidence_steps;
        self.others_ctx.yields += o.yields;
        self.others_ctx.yield_frames += o.yield_frames;
        self.others_ctx.string_bytes_copied += o.string_bytes_copied;
        self.others_heap.add(&scheduled.others_heap);
        // ほかのworkerのヒープは、確かめるか回復するまで`Machine`が持つ
        self.others.append(&mut scheduled.others);
        self.generations = std::mem::take(&mut scheduled.generations);
        // 誤りで返る道でも driver を失わないように、run の直後に戻す。分かれた run は worker 0 で落としたので返さない
        self.driver = driver;
        if !separate && self.driver.is_none() {
            // worker 0は静止の後にも操作を残したdriverを落とした。次のrunは時間の基準からdriverを作り直す。runは
            // それでもほかと同じく終わり、`exit`なら終了コードを返して回復し、誤りならそれを返す
            // (docs/spec/vm.mdの「時計と driver」)
            if matches!(self.run_clock, Some(RunClock::Injected)) {
                self.run_clock = None;
            }
        }
        if let Some(panic) = scheduled.panic.take() {
            // この`Machine`はもう使わない。`Drop`は、panicしたworkerのものも含めてどのworkerのヒープも、ほかのrunの後と
            // 同じく落とす (docs/spec/vm.mdの「タスクとスケジュール」)
            self.panicked = true;
            resume_unwind(panic);
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
        if copied.is_some() && (scheduled.workers > 0 || separate) {
            reclaim_all(ctx, &mut self.others);
        }
        // 誤りを返す前にも書き出す。`> out 2>&1` のときに出力と誤りの順を保つためである
        let flushed = ctx.flush();
        if let Err(
            cause @ RuntimeError::Fault {
                fault: RunFault::Stopped(Stop::Ended),
                ..
            },
        ) = result
        {
            // タスクの底の `exit` が run を捨てた。終わりの検査はせず、その時点の数で `exit` を報告する
            // (docs/spec/vm.md の「誤り、止めること、終わり」)
            flushed.map_err(|fault| after(fault.into(), &task))?;
            let Some(code) = exit else {
                return Err(after(
                    RunFault::Internal("a run ended without an exit"),
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
                RunFault::Internal("the evidence list or a yield is left at the end"),
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
            workers: scheduled.workers,
            placed: scheduled.placed,
            placed_tasks: scheduled.placed_tasks,
            pinned_tasks: scheduled.pinned_tasks,
            placed_jobs: scheduled.placed_jobs,
            holds_raised: scheduled.holds_raised,
            holds_lowered: scheduled.holds_lowered,
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
        self.refuse_after_panic();
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
        // どのヒープも落とすときにほかのヒープのページに触れないので、どのrunの後もこの順で落とせる。panicしたworkerの
        // ヒープも、巻き戻しの後に落とせる (docs/spec/object-model.mdの「`unsafe` の核」のT7)。`ctx`はこの後に欄の
        // 後始末で落ちる
        self.others.clear();
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
    peak_stack: u64,
    shared_roots: usize,
    tasks: TaskStats,
    /// ほかの worker の `Ctx` が数えた回数の和と、ヒープの仕事の和。
    others_ctx: CtxStats,
    others_heap: HeapWork,
    generations: Vec<u32>,
    /// worker 1..N-1と、分かれたrunのメインのworkerの`Ctx`。joinの後に返ってきたもの。
    others: Vec<Ctx<M>>,
    /// 作ったworkerの数。
    workers: usize,
    /// ほかの worker に置いた右の子の数。
    placed: u64,
    /// ほかの worker に置いた `Async` のタスクの数。
    placed_tasks: u64,
    /// 始めた固定したタスクの数。
    pinned_tasks: u64,
    /// ほかの worker に置いた `compute` の仕事の数。
    placed_jobs: u64,
    /// `Run.holds`を立てた回数と下ろした回数。
    holds_raised: u64,
    holds_lowered: u64,
    /// runの最初に記録したpanic。`Machine`のスレッドで起こし直す。
    panic: Option<Box<dyn std::any::Any + Send>>,
}

impl<M: Mode> Scheduled<M> {
    /// スケジューラを作る前に終わった run の結果。
    fn empty(cells: &Shared, shared_roots: usize) -> Scheduled<M> {
        Scheduled::ended(shared_roots, cells.lock().next_generations())
    }

    /// 何も数えずに終わったrunの結果。
    fn ended(shared_roots: usize, generations: Vec<u32>) -> Scheduled<M> {
        Scheduled {
            preemptions: 0,
            peak_stack: 0,
            shared_roots,
            tasks: TaskStats::default(),
            others_ctx: CtxStats::default(),
            others_heap: HeapWork::default(),
            generations,
            others: Vec::new(),
            workers: 0,
            placed: 0,
            placed_tasks: 0,
            pinned_tasks: 0,
            placed_jobs: 0,
            holds_raised: 0,
            holds_lowered: 0,
            panic: None,
        }
    }
}

/// スケジューラを作るときに `Machine` から渡すもの。
struct Start {
    host: HostRef,
    max_depth: usize,
    shared_roots: usize,
    generations: Vec<u32>,
    counters: Arc<Counters>,
    /// 合わせた run の driver。分かれた run では `None` で、worker 0 が作る。
    driver: Option<Box<dyn IoDriver>>,
    seed: Option<u64>,
    seat: Seat,
    main_park: Option<ParkFactory>,
    run_clock: RunClock,
    doorbell: Option<Arc<Doorbell>>,
    signals: Vec<VirtualSignal>,
    job_budget: Duration,
}

/// run の結果と、合わせた run の後に `Machine` へ戻す driver。
type Outcome<M> = (
    Result<(), RuntimeError>,
    Scheduled<M>,
    Option<Box<dyn IoDriver>>,
);

/// 方針 `P` の run の表を作り、worker 0 の本体を `task` から実行する。方針を型の引数にして、方針ごとに VM の繰り返しを
/// 作る (docs/spec/vm.md の「タスクとスケジュール」)。合わせた run では本体をこのスレッドで動かす。分かれた run では、
/// このスレッドが本体のためにworker 0を作り、runが終わるまでメインのworkerとして動く。その後にworker 0をjoinし、
/// runの終わりを自分で行い、最後に`END`を下ろす (docs/spec/vm.mdの「誤り、止めること、終わり」)。
fn run_scheduled<M: Mode, P: Policy>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    policy: P,
    start: Start,
    counts: &mut RunCounts,
) -> Outcome<M> {
    task.worker = ctx.heap().worker();
    let control = Arc::clone(ctx.heap().control());
    // run の表は run ごとに作り、監視のスレッドが止まる `Scheduler` の破棄とともに捨てる (docs/spec/vm.md の
    // 「タスクとスケジュール」)
    let mut run = Run::new(
        control.limits().workers,
        start.seat,
        start.doorbell,
        start.run_clock,
        start.counters,
    );
    run.job_budget = start.job_budget;
    let run = Arc::new(run);
    // 登録表も run ごとに1つ作り、run のすべての `Scheduler` に配る (docs/spec/vm.md の「セルと登録表」)
    let mut registry = Registry::new(start.generations, run.slots.len());
    // 予定は最初の命令の前に、どのタイマーよりも先に載せる (docs/spec/vm.md の「時間の待ち」)
    registry.schedule_signals(start.signals.iter().map(|s| (s.at_ms, s.signal.bit())));
    let cells = Arc::new(Shared::new(registry));
    // 中継したシグナルを届けるのはworker 0..N-1だけである。シグナルを届けることはrunの全体の務めで、すぐに戻らない
    // メインのworkerの待ちがそれを遅らせてはならないためである (docs/spec/vm.mdの「メインのworker」)
    if run.separate() {
        cells.mail.set_main(run.main() as u32);
    }
    let args = EntryArgs {
        run: Arc::clone(&run),
        control: Arc::clone(&control),
        cells: Arc::clone(&cells),
        host: start.host.clone(),
        seed: start.seed,
        max_depth: start.max_depth,
        shared_roots: start.shared_roots,
        // 根は REPL のコードだけが作るので、根に触れるコードを読み込んだ `Machine` は worker を作らない。成功した run の
        // 後に根が、落としたほかの worker のヒープを指しうるためである (docs/spec/vm.md の「タスクとスケジュール」)
        roots_free: !code.tables.uses_roots && ctx.roots().len() == 0,
    };
    if !run.separate() {
        let (body, driver) = entry_body(code, ctx, task, policy, start.driver, args, counts);
        let workers = body.workers;
        let (result, scheduled) = end_run(code, ctx, task, &run, &cells, body);
        if workers > 0 {
            control.clear_end();
        }
        return (result, scheduled, driver);
    }
    let main_policy = policy.for_worker(run.main());
    // `Park`はメインのworkerのスレッドで作り、そこで落とす。wakerは待ちに入る前に置くので、どの起こしも鳴らす
    // (docs/spec/vm.mdの「メインのworkerの待ち」)
    let park: Box<dyn Park> = match &start.main_park {
        Some(factory) => factory(),
        None => Box::new(DefaultPark),
    };
    if let Some(waker) = park.waker() {
        let _ = run.slots[run.main()].park_waker.set(waker);
    }
    // メインのworkerの`Ctx`はworker 0を作る前に作る。検査モードでは作るときにページを1枚取り、確保量に数えるので、
    // その課金がプログラムの確保とどう前後するかをスレッドの速さで決めないためである。決定的な方針では柔らかい上限の
    // 旗の地点が確保量で決まる (docs/spec/vm.mdの「メインのworker」「タスクとスケジュール」)
    let main_ctx = Ctx::<M>::for_worker(Arc::clone(&control), start.host.clone(), run.main());
    let (worker_ctx, worker_task, worker_counts) = (&mut *ctx, &mut *task, &mut *counts);
    let (joined, main) = thread::scope(|scope| {
        // 呼んだスレッドの panic でも `END` を立ててすべての worker を起こし、scope が worker 0 を join できるようにする
        let mut guard = Shutdown {
            control: Arc::clone(&control),
            run: Arc::clone(&run),
            armed: true,
        };
        let builder = thread::Builder::new()
            .name("eml-worker-0".into())
            .stack_size(WORKER_STACK);
        // worker 0は、呼んだスレッドが2つのシグナルを塞ぐまで何も動かさない。作る前に塞ぐと、新しいスレッドは作った
        // スレッドの塞ぎを受け継ぐのでworker 0も塞がれる。動き出してから塞ぐと、worker 0がプログラムを動かす間に
        // シグナルがこのスレッドへ行きうる (docs/spec/vm.md の「上限と割り込み」)
        let (open, gate) = std::sync::mpsc::channel::<()>();
        let spawned = builder.spawn_scoped(scope, move || {
            let _ = gate.recv();
            let (mut body, driver) = entry_body(
                code,
                worker_ctx,
                worker_task,
                policy,
                None,
                args,
                worker_counts,
            );
            // driverは作ったworker 0のスレッドで落とす
            drop_recording(driver, &mut body.panic);
            body
        });
        let worker = match spawned {
            Ok(worker) => worker,
            Err(error) => {
                guard.armed = false;
                return (Err(error), None);
            }
        };
        let _blocked = eml_io::Masked::new();
        // 門を開ける。送り手を落とすとworker 0の待ちが終わる。このスレッドのpanicでも落ちる
        drop(open);
        let main = workers::run_main::<M, P>(
            code,
            main_policy,
            &run,
            &control,
            Arc::clone(&cells),
            main_ctx,
            start.max_depth,
            park,
        );
        let joined = worker.join();
        drop(guard);
        (Ok(joined), Some(main))
    });
    let mut body = match joined {
        Ok(Ok(body)) => body,
        Ok(Err(payload)) => {
            // worker 0は、本体がpanicを捕まえ始める前にpanicしたので、workerを作っていない
            let mut body = Body::failed(worker_panicked(), start.shared_roots);
            body.panic = Some(payload);
            body
        }
        Err(error) => {
            control.clear_end();
            let error = RuntimeError::Fault {
                fault: RunFault::Driver(error.to_string()),
                function: "<worker 0>".into(),
                at: None,
            };
            return (
                Err(error),
                Scheduled::empty(&cells, start.shared_roots),
                None,
            );
        }
    };
    // 分かれたrunの終わりは、両方のjoinの後にここで行う。メインのworkerはworker 0が結果を決めた後に致命的な誤りを
    // 書きうるので、もう一度決める。メインのworkerのヒープは、ほかのworkerのヒープと同じく最後に畳み込む
    body.result = settle(body.result, run.take_fatal());
    if let Some(exit) = main {
        body.retire(exit, counts);
    }
    let (result, scheduled) = end_run(code, ctx, task, &run, &cells, body);
    // `END`を下ろすのは`Machine`を持つスレッドだけで、両方のjoinとrunの終わりの後である。worker 0が下ろすと、
    // メインのworkerが見逃して眠り続けうる (docs/spec/vm.mdの「誤り、止めること、終わり」)
    control.clear_end();
    (result, scheduled, None)
}

/// worker 0 の本体が `run_scheduled` から受け取るもの。分かれた run では worker 0 へ移すので、どれも `Send` である。
struct EntryArgs {
    run: Arc<Run>,
    control: Arc<Control>,
    cells: Arc<Shared>,
    host: HostRef,
    seed: Option<u64>,
    max_depth: usize,
    shared_roots: usize,
    roots_free: bool,
}

/// worker 0の本体がrunの終わりに残すもの。結果と、joinしたworkerの仕事である。分かれたrunでは`Machine`を
/// 持つスレッドに戻り、そのスレッドがメインのworkerの分を足してからrunの終わりを行う。
struct Body<M: Mode> {
    result: Result<(), RuntimeError>,
    preemptions: u64,
    peak_stack: u64,
    shared_roots: usize,
    /// 作ったworkerの数。
    workers: usize,
    /// joinしたworkerの`Ctx`と、それを作った直後のヒープの仕事。同じ順に並ぶ。
    others: Vec<Ctx<M>>,
    bases: Vec<HeapWork>,
    left: bool,
    kept_pending: bool,
    panic: Option<Box<dyn std::any::Any + Send>>,
}

impl<M: Mode> Body<M> {
    /// スケジューラを作る前に終わった本体。
    fn failed(error: RuntimeError, shared_roots: usize) -> Body<M> {
        Body {
            result: Err(error),
            preemptions: 0,
            peak_stack: 0,
            shared_roots,
            workers: 0,
            others: Vec::new(),
            bases: Vec::new(),
            left: false,
            kept_pending: false,
            panic: None,
        }
    }

    /// 抜けたworkerを受け取る。panicしたworkerは最初のpanicの中身を残し、その`Ctx`はほかの`Ctx`に加わる。起点は
    /// この時点のヒープの仕事なので、panicしたworkerは`RunStats`に仕事を足さない。
    fn retire(&mut self, exit: WorkerExit<M>, counts: &mut RunCounts) {
        match exit {
            Ok(retired) => self.add(retired, counts),
            Err(panicked) => {
                let Panicked { payload, ctx } = *panicked;
                if let Some(ctx) = ctx {
                    let base = HeapWork::of(ctx.heap().stats());
                    self.others.push(ctx);
                    self.bases.push(base);
                }
                self.panic.get_or_insert(payload);
            }
        }
    }

    /// 出口の`Ctx`を、その仕事を数えずに`body`に移す。
    fn keep(&mut self, exit: WorkerExit<M>) {
        match exit {
            Ok(retired) => {
                self.others.push(retired.ctx);
                self.bases.push(retired.base);
            }
            Err(panicked) => {
                let Panicked { payload, ctx } = *panicked;
                self.others.extend(ctx);
                self.panic.get_or_insert(payload);
            }
        }
    }

    /// 抜けたworkerの仕事を足す。数は足し、山は最大を取る (docs/spec/vm.mdの「`RunStats`」)。
    fn add(&mut self, retired: Retired<M>, counts: &mut RunCounts) {
        // 和でpanicしても`Ctx`を落とさないよう、先に入れる
        self.others.push(retired.ctx);
        self.bases.push(retired.base);
        counts.add(&retired.counts);
        self.preemptions += retired.preemptions;
        self.peak_stack = self.peak_stack.max(retired.peak_stack);
        self.left |= retired.left;
        self.kept_pending |= retired.kept_pending;
    }
}

/// worker 0 の本体。渡されなければ driver を作り、スケジューラを作り、worker 1..N-1 を作り、エントリを実行して
/// joinし、結果を決めてdriverを静止させる。`Box<dyn IoDriver>`は作ったスレッドを離れないので、driverは本体の
/// 結果と並べて返す (docs/spec/vm.mdの「時計とdriver」)。
fn entry_body<M: Mode, P: Policy>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    policy: P,
    driver: Option<Box<dyn IoDriver>>,
    args: EntryArgs,
    counts: &mut RunCounts,
) -> (Body<M>, Option<Box<dyn IoDriver>>) {
    // 分かれたrunでは、この本体のどの出口も`END`を立ててメインのworkerを起こすので、何かが失敗しうる前に見張りを
    // 置く (docs/spec/vm.mdの「誤り、止めること、終わり」)
    let mut shutdown = Shutdown {
        control: Arc::clone(&args.control),
        run: Arc::clone(&args.run),
        armed: args.run.separate(),
    };
    let EntryArgs {
        run,
        control,
        cells,
        host,
        seed,
        max_depth,
        shared_roots,
        roots_free,
    } = args;
    let driver = match driver.map_or_else(|| run.entry_driver(seed), Ok) {
        Ok(driver) => driver,
        Err(e) => {
            let error = code.error(
                vm::Failure {
                    fault: RunFault::Driver(e.to_string()),
                    pc: task.pc,
                },
                task,
            );
            return (Body::failed(error, shared_roots), None);
        }
    };
    let mut sched = Scheduler::new(policy, driver, run, control, task.worker, max_depth, cells);
    sched.shared_roots = shared_roots;
    let wanted = sched.begin_run(roots_free);
    // 結果は、worker 0がpanicしなければ後で決める
    let mut body = Body::failed(worker_panicked(), shared_roots);
    let mut exits: Vec<WorkerExit<M>> = Vec::new();
    let ran = thread::scope(|scope| {
        let mut handles = Vec::new();
        // ここからjoinまでのpanicを捕まえる。workerがまだ動く間に`thread::scope`を抜けると、scopeはどのworkerもjoin
        // する前に、返ってきた`Ctx`を落としてしまう (docs/spec/vm.mdの「タスクとスケジュール」)
        let ran = catch_unwind(AssertUnwindSafe(|| {
            // worker 1..N-1はrunの最初の命令の前に作るので、命令を2回実行することはない
            // (docs/spec/vm.mdの「タスクとスケジュール」)
            if wanted {
                spawn_workers::<M, P>(
                    scope,
                    code,
                    &sched.policy,
                    &sched.run,
                    &sched.watcher.control,
                    &sched.cells,
                    &host,
                    max_depth,
                    &mut handles,
                );
            }
            // workerができたらすぐに見張りを立てる。この後でpanicしても、workerを起こしてjoinできるようにするためである
            shutdown.armed |= !handles.is_empty();
            // タスクを置くのは作れたworkerだけである
            sched.run.set_live(handles.len() + 1);
            // 1つも作れなければworker 0だけで続ける
            sched.workers = match handles.len() {
                0 => Workers::Denied,
                n => Workers::Started(n),
            };
            vm::run::<ExecWith<M, P>>(code, ctx, task, &mut sched, counts)
        }));
        // 終わりを決めたのはworker 0のループか、ほかのworkerの致命的な誤りか、worker 0のpanicである。どれでも`END`を
        // 立ててすべてのworkerを起こし、`Ctx`を返させてからjoinする (docs/spec/vm.mdの「タスクとスケジュール」)
        shutdown.armed |= !handles.is_empty();
        drop(shutdown);
        exits.extend(handles.into_iter().map(|handle| {
            handle
                .join()
                .unwrap_or_else(|payload| Err(Box::new(Panicked { payload, ctx: None })))
        }));
        ran
    });
    body.workers = exits.len();
    let result = match ran {
        Ok(result) => result,
        Err(payload) => {
            // worker 0がpanicした。まず`Scheduler`とdriverを静止させずに落とす。driverの`Drop`は、記憶を貸した操作を
            // 待つか止める
            body.panic = Some(payload);
            drop_recording(sched, &mut body.panic);
            retire_all(&mut body, exits, counts);
            return (body, None);
        }
    };
    // ほかのworkerの仕事をworker 0の数に足す (docs/spec/vm.mdの「`RunStats`」)
    retire_all(&mut body, exits, counts);
    // joinの後のpanicも同じに扱う。分かれたrunではメインのworkerがまだ動きうるので、巻き戻すとjoinした`Ctx`を落として
    // しまう
    let finished = catch_unwind(AssertUnwindSafe(|| {
        body.preemptions += sched.preemptions();
        body.peak_stack = body.peak_stack.max(sched.peak_stack(task));
        body.shared_roots = sched.shared_roots;
        body.result = settle(result, sched.run.take_fatal());
        // runの外に保留の操作を残さない。誤りの道でも、`Scheduler`と登録表を落とす前にdriverを静止させる。どのworkerも
        // もう受け付けを送らないので、joinの後に行う。ほかのworkerのdriverは、そのworkerが抜ける前に静止させてある。
        // 成功したrunでは、静止させたdriverが返した完了を記録に置く。集める地点と同じく、driverに出した操作がなければ
        // 完了を読まない (docs/spec/vm.mdの「時計と driver」「誤り、止めること、終わり」)
        let kept = match sched.driver.as_mut() {
            Some(driver) => {
                driver.quiesce(sched.run.job_wait(driver.now()));
                let in_flight = sched.cells.lock().in_flight(sched.index);
                if body.result.is_ok() && in_flight > 0 {
                    let mut completed = Vec::new();
                    driver.completions(&mut completed);
                    sched.cells.lock().quiesced(sched.index, &mut completed);
                }
                driver.pending() > 0
            }
            None => false,
        };
        body.kept_pending |= kept;
        // 予約と確定の間に安全点はないので、`Finish`に来たworker 0は予約を持たない (docs/spec/vm.mdの「channel」)
        debug_assert!(
            body.result.is_err() || sched.hold.is_none(),
            "a channel reservation is held at the end"
        );
        let driver = sched.driver.take();
        if kept {
            // 操作を残したdriverは、どのヒープにも触れる前に、作ったこのスレッドで落とし、`Machine`には返さない
            // (docs/spec/vm.mdの「時計と driver」)
            drop(driver);
            return None;
        }
        driver
    }));
    let driver = match finished {
        Ok(driver) => driver,
        Err(payload) => {
            body.result = Err(worker_panicked());
            body.panic.get_or_insert(payload);
            None
        }
    };
    drop_recording(sched, &mut body.panic);
    (body, driver)
}

/// `exits`の出口をすべて`body`に受け取る。途中でpanicしたら、それを記録し、残りの`Ctx`は数えずに`body`に移す。分かれた
/// runのメインのworkerがまだ動きうる間に、どれも落とさないためである。
fn retire_all<M: Mode>(body: &mut Body<M>, exits: Vec<WorkerExit<M>>, counts: &mut RunCounts) {
    let mut exits = exits.into_iter();
    let retired = catch_unwind(AssertUnwindSafe(|| {
        for exit in exits.by_ref() {
            body.retire(exit, counts);
        }
    }));
    if let Err(payload) = retired {
        body.panic.get_or_insert(payload);
        for exit in exits {
            body.keep(exit);
        }
    }
}

/// `value`を落とし、その`Drop`のpanicを`panic`に記録する。先に記録したpanicがあればそれを残す。分かれたrunでは
/// メインのworkerがまだ動きうるので、worker 0の本体から巻き戻すと、joinした`Ctx`を落としてしまう。
fn drop_recording<T>(value: T, panic: &mut Option<Box<dyn std::any::Any + Send>>) {
    if let Err(payload) = catch_unwind(AssertUnwindSafe(move || drop(value))) {
        panic.get_or_insert(payload);
    }
}

/// panicしたworker 0の本体の結果。runは`Machine`のスレッドでpanicを起こし直すので、この誤りは返らない。
fn worker_panicked() -> RuntimeError {
    RuntimeError::Fault {
        fault: RunFault::Internal("a worker thread panicked"),
        function: "<worker 0>".into(),
        at: None,
    }
}

/// 記録したpanicを失わずに`finish_run`を動かす。`others`はその外に置く。runの終わりがpanicしたら、ヒープは巻き戻しの
/// 間に落とさずに結果に入れ、`Machine`がdriverの後で落とす。最初のpanicを残し、後のものは捨てる。
fn end_run<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &Task,
    run: &Run,
    cells: &Shared,
    mut body: Body<M>,
) -> (Result<(), RuntimeError>, Scheduled<M>) {
    let first = body.panic.take();
    let panicked = first.is_some();
    let shared_roots = body.shared_roots;
    let workers = body.workers;
    let mut others = std::mem::take(&mut body.others);
    let finished = catch_unwind(AssertUnwindSafe(|| {
        finish_run(code, ctx, task, run, cells, body, &mut others, panicked)
    }));
    match finished {
        Ok((result, mut scheduled)) => {
            scheduled.others = others;
            scheduled.panic = first;
            (result, scheduled)
        }
        Err(payload) => {
            let mut scheduled = Scheduled::ended(shared_roots, Vec::new());
            scheduled.others = others;
            scheduled.workers = workers;
            scheduled.panic = Some(first.unwrap_or(payload));
            (Err(worker_panicked()), scheduled)
        }
    }
}

/// どのworkerもjoinした後のrunの終わり。終わりの確かめ、残ったセルの片付け、登録表の数、ほかのスレッドが積んだ
/// 解放の取り込み、ほかのヒープの統計である。分かれたrunではこの間ずっと`END`が立っているので、どの手順も止める語を
/// 読まない (docs/spec/vm.mdの「誤り、止めること、終わり」)。`panicked`はrunでpanicを記録したことで、その中身は
/// `end_run`が持つ。
#[allow(clippy::too_many_arguments)]
fn finish_run<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &Task,
    run: &Run,
    cells: &Shared,
    body: Body<M>,
    others: &mut [Ctx<M>],
    panicked: bool,
) -> (Result<(), RuntimeError>, Scheduled<M>) {
    let Body {
        result,
        preemptions,
        peak_stack,
        shared_roots,
        workers,
        others: _,
        bases,
        left,
        kept_pending,
        panic: _,
    } = body;
    // panicの後は、runのタスクとセルが途中のまま残っているので、終わりの確かめも取り込みもしない。ヒープはいつもどおり
    // 落とす
    // 成功した run だけが、残った知らせと保留の操作を確かめてから、残ったセルを片付ける。誤りと `exit` の run の登録表は、
    // 触れずに `Scheduler` と一緒に落とす。誤りには `Finish` の位置を付ける (docs/spec/vm.md の「誤り、止めること、終わり」)
    let result = result.and_then(|()| {
        if panicked {
            return Ok(());
        }
        let at_finish = |fault: RunFault| {
            code.error(
                vm::Failure {
                    fault,
                    pc: task.pc - 1,
                },
                task,
            )
        };
        if kept_pending && ctx.heap().control().exit_code().is_some() {
            // 操作を残したdriverがあっても、`main`の底の`exit`は終了コードを保つ。ほかのタスクの`exit`と同じくrunを
            // 捨てたことにし、終わりの確かめで誤りにせずに回復する (docs/spec/vm.mdの「時計と driver」)
            return Err(at_finish(RunFault::Stopped(Stop::Ended)));
        }
        workers::end_check(kept_pending, left).map_err(at_finish)?;
        cells::sweep_all(cells, ctx, task.depth_offset).map_err(|fault| at_finish(fault.into()))
    });
    let (tasks, placed_tasks, pinned_tasks) = {
        let cells = cells.lock();
        (cells.stats, cells.placed_tasks, cells.pinned_tasks)
    };
    // join の後に、ほかのスレッドが積んだ解放をすべてのヒープで取り込む。この後はどのスレッドも解放を積まない
    // (docs/spec/vm.md の「誤り、止めること、終わり」)
    if !panicked && (workers > 0 || run.separate()) {
        reclaim_all(ctx, others);
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
    let generations = cells.lock().next_generations();
    let scheduled = Scheduled {
        preemptions,
        peak_stack,
        shared_roots,
        tasks,
        others_ctx,
        others_heap,
        generations,
        others: Vec::new(),
        workers,
        placed: run.placed.load(std::sync::atomic::Ordering::Relaxed),
        placed_tasks,
        pinned_tasks,
        placed_jobs: run.placed_jobs.load(std::sync::atomic::Ordering::Relaxed),
        holds_raised: run.holds_raised.load(std::sync::atomic::Ordering::Relaxed),
        holds_lowered: run.holds_lowered.load(std::sync::atomic::Ordering::Relaxed),
        panic: None,
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
                .with_job_budget(config.job_budget)
                .with_seat(config.seat);
            machine.doorbell = config.doorbell.clone();
            machine.main_park = config.main_park.clone();
            AnyMachine::Checked(machine)
        } else {
            let mut machine = Machine::<Fast>::new(control, host)
                .with_sched(config.sched)
                .with_clock(config.clock)
                .with_signals(config.signals.clone())
                .with_job_budget(config.job_budget)
                .with_seat(config.seat);
            machine.doorbell = config.doorbell.clone();
            machine.main_park = config.main_park.clone();
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

/// コード生成の誤りは、正しいプログラムでは起きない内部の誤りである。REPLは`Machine::load`の誤りをこれで報告する。
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
    };
    RuntimeError::Fault {
        fault: RunFault::Internal(what),
        function,
        at: None,
    }
}
