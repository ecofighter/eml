//! 実行の設定と結果の型 (docs/spec/vm.md の「`RunStats`」「置き場所」)。VMとネイティブのコードが同じ型で実行を頼み、結果を受け取る。

use std::sync::Arc;
use std::time::Duration;

use eml_rt::{Control, Limits};

use crate::doorbell::Doorbell;
use crate::driver::{Clock, JOB_QUIESCE_LIMIT, VirtualSignal};
use crate::park::ParkFactory;

/// 実行の仕事の回数と、同時に生きていたヒープの物体の数とスタックの語の数の最大。検査モードだけで数える。
/// 各欄の意味と出どころは docs/spec/vm.md の「`RunStats`」にある。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
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
    /// このrunが始めに作ったworkerの数。worker 0とメインのworkerは数えない
    /// (docs/spec/vm.mdの「タスクとスケジュール」)。
    pub workers: usize,
    /// この run がほかの worker に置いた `par` の右の子の数。どの worker の fork も数える
    /// (docs/spec/vm.md の「`par` の fork と join」)。
    pub placed: u64,
    /// この run が、`start` を実行した worker と違う worker に置いた `Async` のタスクの数
    /// (docs/spec/vm.md の「`Async` のタスク」)。
    pub placed_tasks: u64,
    /// このrunが始めた固定したタスクの数。`Async.start_on_main`で始めたタスクと、固定したタスクが始めたタスクを
    /// 数える (docs/spec/vm.mdの「`Async`のタスク」)。
    pub pinned_tasks: u64,
    /// この run がほかの worker に置いた `compute` の仕事の数。どの worker の `fork_job` も数える
    /// (docs/spec/vm.md の「`compute` の仕事」)。
    pub placed_jobs: u64,
    /// runの外の待ちの印(`Run.holds`)を立てた回数と、下ろした回数。立てるのは`want_gui_raw`、下ろすのは回収、
    /// `abandon_raw`、作るときの誤りを返す`open_ui_raw`である。期間がつながる2つの`Gui.run`では1回立てて1回下ろす
    /// (docs/spec/vm.mdの「GUIのPark」)。
    pub holds_raised: u64,
    pub holds_lowered: u64,
}
/// スケジュールの方針の選び方。`RunConfig::with_sched` で選ぶ。`eml run` と `eml repl` はいつも `Normal` で動く。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sched {
    /// 自分からは切り替えない。起こしたタスクを列の後ろに入れ、メモリは監視のスレッドが見る。
    Normal,
    /// 自分からは切り替えない。起こしたタスクを列の先頭に入れ、メモリは集める地点が見る。
    #[default]
    Sequential,
    /// seedで決まる擬似乱数で、切り替える安全点と次のタスクを選ぶ。
    Seeded(u64),
}

impl Sched {
    /// テストの driver に渡す seed。seed の方針のときだけある (docs/spec/vm.md の「時計と driver」)。
    pub fn seed(self) -> Option<u64> {
        match self {
            Sched::Seeded(seed) => Some(seed),
            Sched::Normal | Sched::Sequential => None,
        }
    }
}

/// `Machine`を持つスレッドと、`main`を動かすworker 0の関係。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seat {
    /// `Machine`を持つスレッドがworker 0を兼ねる。`eml repl`と`Machine`の既定である。
    Merged,
    /// `Machine`を持つスレッドがメインのworkerになり、worker 0は別のスレッドで動く。
    Separate,
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
    /// 静止が走っている`Op::Job`を待つ長さで、runのすべての静止が分け合う。既定は1秒である。過ぎても残る仕事は
    /// 見捨て、runはそれでもほかと同じ終わり方で終わる (docs/spec/vm.mdの「時計と driver」)。
    pub job_budget: Duration,
    /// `Machine`を持つスレッドとworker 0の関係。既定は`Seat::Merged`で、`execute`はいつも`Seat::Separate`にする
    /// (docs/spec/vm.mdの「メインのworker」)。
    pub seat: Seat,
    /// 分かれたrunのメインのworkerの`Park`を作るもの。既定は`None`で、`DefaultPark`を使う。合わせたrunは使わない
    /// (docs/spec/vm.mdの「メインのworkerの待ち」)。
    pub main_park: Option<ParkFactory>,
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
            seat: Seat::Merged,
            main_park: None,
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

    pub fn with_seat(mut self, seat: Seat) -> RunConfig {
        self.seat = seat;
        self
    }

    pub fn with_main_park(mut self, main_park: ParkFactory) -> RunConfig {
        self.main_park = Some(main_park);
        self
    }
}
