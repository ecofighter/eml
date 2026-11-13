//! 命令を実行するループ (docs/spec/vm.md)。状態は `Ctx`、`Task`、`Scheduler` だけに置き、安全点をまたいで外に
//! 持たない。呼び出しのスタックを Rust の再帰でなく `Task` のデータに持つので、安全点でタスクごとに取り替えられる
//! (docs/future/multicore.md の「VM の並列化の備え」の制約5)。

use std::marker::PhantomData;
use std::sync::atomic::Ordering;
use std::time::Duration;

use eml_extern::{Extern, ExternEffect, Purity};
use eml_rows::rows::{Args, Value};
use eml_rows::{gui, tls};
use eml_rt::externs::{self, Fault, Raised};
use eml_rt::layout::data_first;
use eml_rt::{Ctx, MaskItem, Mode, PipeEnd, RawResource, Stop, Token, Word};

use crate::apply;
use crate::cells::{self, CellRef, JobSource, Raise, Runnable, Take, WaitToken};
use crate::code::{CodeBase, ForkTable, UnwindEntry};
use crate::data;
use crate::driver::DriverKind;
use crate::error::RunFault;
use crate::ffi;
use crate::jobs;
use crate::op::{APPLY_REST, CHILD_DONE, FINISH, Op, Pc, Slot};
use crate::park::{Guard, Park, Pending, PollSource, Progress};
use crate::sched::{
    ENTRY, JOB, JoinId, JoinNode, Memory, Outcome, PLACED, POLL_EVERY, Policy, Scheduler, TaskId,
    TaskState,
};
use crate::workers::{Entry, EntryKind, Notice, Ready};
use crate::{RunCounts, RuntimeError};

/// 登録表を触るときの宛先 `cells::Here` を、`Scheduler` の欄から作る。欄ごとに借りるので、同じ関数の中で driver の
/// 欄を可変で借りられる。
macro_rules! here {
    ($sched:expr, $post:ident, $here:ident) => {
        let $post = crate::workers::Mailer::new(&$sched.run, &$sched.watcher.control);
        let $here = cells::Here {
            me: $sched.index as u32,
            host: $sched.run.io_worker($sched.index) as u32,
            mail: &$sched.cells.mail,
            post: &$post,
        };
    };
}
pub(crate) use here;

// 核は `eml_lir` を知らないので、`Wake` のタグを自分で持つ。表の値と食い違わないことをここで確かめる
const _: () = assert!(cells::WOKE == eml_lir::WAKE_WOKE as i64);
const _: () = assert!(cells::CANCELLED == eml_lir::WAKE_CANCELLED as i64);
const _: () = assert!(cells::IO_CANCELLED == eml_lir::IO_CANCELLED as i64);

/// VM の繰り返しの1つの写しを決める型。モード `M` と方針 `P` を束ねる。`step` から呼ぶ補助の関数をこの型で総称に
/// するので、補助の関数の実体は写しごとにできて呼び手が1つになり、inline の判断が写しごとに決まる
/// (docs/spec/vm.md の「置き場所」)。
pub(crate) trait Exec {
    type M: Mode;
    type P: Policy;
}

/// モード `M` と方針 `P` の `Exec`。
pub(crate) struct ExecWith<M, P>(PhantomData<(M, P)>);

impl<M: Mode, P: Policy> Exec for ExecWith<M, P> {
    type M = M;
    type P = P;
}

/// 1つのタスクの実行の状態。
pub struct Task {
    pub(crate) stack: Vec<Word>,
    pub(crate) pc: Pc,
    pub(crate) base: usize,
    pub(crate) frame_size: usize,
    /// 最後に戻った関数の `Ret` の位置。どの関数にも属さない組み込みの命令で起きた誤りに、その関数の名前を付ける。
    pub(crate) last_ret: Pc,
    /// 末尾の呼び出しと適用で、引数を今のフレームから新しいフレームへ移す間だけ置く所。
    pub(crate) scratch: Vec<Word>,
    /// `reserve` がスタックの容量を伸ばす上限の語の数 (`stack_limit`)。
    pub(crate) stack_limit: usize,
    /// エントリの関数の番号。どの関数にも属さない命令の誤りの名前と、`Finish` が結果を手放すかを決める。
    pub(crate) entry: u32,
    /// `Finish` が結果を手放さずに残す。`Machine::run_shown` が、実行の後に結果の文字列を写して手放す。
    pub(crate) keep_result: bool,
    /// 巻き戻しの間の `exit` の終了コード。立っていれば、中断が `Finish` まで届き、底が終わりの欄に書く
    /// (docs/spec/vm.md の「`exit`」)。
    pub(crate) exit: Option<i64>,
    /// 動いていない間に退避した `evv`。タスクが参照を1つ持つ。動いている間は `()` で、`Ctx` が持つ。
    pub(crate) evv: Word,
    /// ヒープの論理的な worker ID。右の子は fork でもらった ID で動き、左の子は親の ID で動く。
    pub(crate) worker: u32,
    /// fork のときの親の深さ。右の子の深さはここから数える。エントリのタスクは 0 である。
    pub(crate) depth_offset: usize,
    /// 開いている子の境目の join。内側が最後に来る。右の子の先頭は、自分が右の子である join である。
    pub(crate) boundaries: Vec<JoinId>,
    pub(crate) state: TaskState,
    /// `Async` のタスクのセル。エントリの `Task` だけが持ち、`main`、`par` の右の子、`compute` の仕事は `None` である。
    /// `resume_task` のエントリの `Task` は、待っていたタスクのセルを持つ (docs/spec/vm.md の「`Async` のタスク」)。
    pub(crate) cell: Option<CellRef>,
    /// `finish_*` が預けたタスクの終わり方。エントリの `Task` の `Finish` が核の `finish` に渡す。タスクの ID の物体が
    /// すべて片付く前に待ち手を起こさないためである (docs/spec/vm.md の「`Async` のタスク」)。
    pub(crate) outcome: Option<cells::Outcome>,
    /// ほかの worker に置いた右の子の、親の側の記録。join の節点は親の worker にあるので、子の終わりと誤りは
    /// `home` へ知らせで送る (docs/spec/vm.md の「`par` の fork と join」)。
    pub(crate) placed: Option<Placed>,
    /// `compute` の仕事の `Task` の、仕事のセルと `fork_job` の位置。仕事は join の節点を持たず、終わりを仕事のセルに
    /// 置く (docs/spec/vm.md の「`compute` の仕事」)。
    pub(crate) job: Option<JobRef>,
    /// 深さの超過とタスクの旗をどう扱うか。`Ctx` の深さの上限の値と組で決まる (docs/spec/vm.md の「タスクの上限」)。
    pub(crate) limit: Limit,
    /// `Async` のタスクの今の区間より前の数 (取ったページのバイト数)。エントリの `Task` はセルが返した値から数え始める
    /// (docs/spec/vm.md の「タスクとスケジュール」)。
    pub(crate) counted: usize,
    /// このエントリの `Task` が数えた分。待ちに入って `Finish` したら、`leave` がセルの数に足す。
    pub(crate) added: usize,
}

/// タスクごとの上限の状態。状態ごとに `Ctx` の深さの上限を決まった値にしておき、安全点の速い道は変えない
/// (docs/spec/vm.md の「タスクの上限」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Limit {
    /// タスクの外 (`main`、`compute` の仕事、タスクの外の `par` の子)。上限は本当の限度で、超えたら run を止める。
    Stops,
    /// `Async` のタスクの中で、保留はない。上限は本当の限度である。
    Task,
    /// 超過か旗を見て、次の関数のエントリで `ResourceLimit` を起こす。上限は 0 で、どの安全点も冷たい道に入る。
    Pending,
    /// 中断を始めてから着地するまで。上限は本当の限度に `LIMIT_HEADROOM` を足した値で、超えたら run を止める。
    /// `pending` が立っていれば、着地の後に保留へ戻す。
    Aborting { pending: bool },
}

impl Limit {
    /// `par` の右の子が受け継ぐ状態。子は保留も猶予も持たずに始まる。子の `evv` に `IO` の evidence はないので、
    /// `Task` の子の深さの超過も `limit_hit` の手順の終わりで run を止め、`Stops` の子と結果は変わらない。右の子は
    /// セルを持たず旗も受けないので、`Task` はタスクの中の子であることの印である (docs/spec/vm.md の「深さ」)。
    fn inherit(self) -> Limit {
        match self {
            Limit::Stops => Limit::Stops,
            _ => Limit::Task,
        }
    }

    /// 上限の旗を受けた後の状態。`Task` は保留に、中断の間は着地の後に保留へ戻す印だけを付ける。`Pending` はそのままで、
    /// `Stops` のタスクはセルを持たないので旗を受けない。上限の値は呼び手が当てる (docs/spec/vm.md の「タスクの上限」)。
    fn flagged(self) -> Limit {
        match self {
            Limit::Task => Limit::Pending,
            Limit::Aborting { .. } => Limit::Aborting { pending: true },
            Limit::Pending | Limit::Stops => self,
        }
    }
}

/// 巻き戻しの間に本当の限度へ足す猶予の語の数 (2 MiB)。節のエントリが暴走しても、この先で止まる。
const LIMIT_HEADROOM: usize = 1 << 18;

/// 仕事の `Task` が自分の仕事について知ること。
#[derive(Debug, Clone, Copy)]
pub(crate) struct JobRef {
    pub(crate) cell: CellRef,
    /// `fork_job` の命令の位置。仕事を始めるときの誤りに付ける。
    pub(crate) at: Pc,
}

/// 置いた右の子が親について知ること。
#[derive(Debug, Clone, Copy)]
pub(crate) struct Placed {
    /// 親の worker の番号。
    pub(crate) home: usize,
    /// fork した `par` の命令の位置。子を始めるときの誤りに付ける。
    pub(crate) at: Pc,
}

/// 誤りと、それが起きた命令の位置。
pub(crate) struct Failure {
    pub(crate) fault: RunFault,
    pub(crate) pc: Pc,
}

// いちばん下に置く空のフレーム。エントリの関数の結果を受けるスロットを1つ持つ
const ROOT_BASE: usize = 2;
const ROOT_SIZE: usize = 1;
// 子の境目のフレーム。slot 0 が子の結果、slot 1 が join の番号である
const BOUNDARY_SIZE: usize = 2;

impl Task {
    pub(crate) fn new<M: Mode>(code: &CodeBase<M>, index: u32, max_depth: usize) -> Task {
        let entry = &code.functions[index as usize];
        let base = ROOT_BASE + ROOT_SIZE + 2;
        let mut stack = vec![Word::UNIT; base + usize::from(entry.frame_size)];
        stack[base - 2] = header(FINISH, 0, ROOT_SIZE);
        stack[base - 1] = Word::from_bits(ROOT_BASE);
        Task {
            stack,
            pc: entry.start,
            base,
            frame_size: usize::from(entry.frame_size),
            last_ret: FINISH,
            scratch: Vec::new(),
            stack_limit: stack_limit(max_depth),
            entry: index,
            keep_result: false,
            exit: None,
            evv: Word::UNIT,
            worker: 0,
            depth_offset: 0,
            boundaries: Vec::new(),
            state: TaskState::Running,
            cell: None,
            outcome: None,
            placed: None,
            job: None,
            limit: Limit::Stops,
            counted: 0,
            added: 0,
        }
    }

    /// `Async` のタスクのエントリの `Task`。`Task::new` と同じ根のフレームを作り、スタックと scratch は集まりから取った
    /// `spare` を使う。引数は動き始めるときにエントリのフレームに書く (`resume`)。深さは 0 から数え、タスクごとに
    /// `max_depth` のすべてを使う (docs/spec/vm.md の「`Async` のタスク」)。
    #[expect(
        clippy::too_many_arguments,
        reason = "the parts of a task an entry starts from"
    )]
    fn entry<M: Mode>(
        code: &CodeBase<M>,
        function: u32,
        args: [Word; 3],
        argc: u8,
        cell: CellRef,
        worker: u32,
        max_depth: usize,
        (mut stack, mut scratch): (Vec<Word>, Vec<Word>),
    ) -> Task {
        let entry = &code.functions[function as usize];
        let base = ROOT_BASE + ROOT_SIZE + 2;
        stack.clear();
        scratch.clear();
        stack.resize(base + usize::from(entry.frame_size), Word::UNIT);
        stack[base - 2] = header(FINISH, 0, ROOT_SIZE);
        stack[base - 1] = Word::from_bits(ROOT_BASE);
        Task {
            stack,
            pc: FINISH,
            base,
            frame_size: usize::from(entry.frame_size),
            last_ret: FINISH,
            scratch,
            stack_limit: task_stack_limit(max_depth, Limit::Task),
            entry: function,
            keep_result: false,
            exit: None,
            evv: Word::UNIT,
            worker,
            depth_offset: 0,
            boundaries: Vec::new(),
            state: TaskState::Entry {
                function,
                args,
                argc,
            },
            cell: Some(cell),
            outcome: None,
            placed: None,
            job: None,
            limit: Limit::Task,
            counted: 0,
            added: 0,
        }
    }

    /// まだ始まっていない右の子。いちばん下のフレームが子の境目のフレームで、始めるときに閉包 `g` をその上で呼ぶ。
    /// 境目のフレームの戻り先は `Finish` だが、`ChildDone` がフレームを外さずにタスクを捨てるので、戻ることはない
    /// (docs/spec/vm.md の「`par` の fork と join」)。`limit` は親の状態から `Limit::inherit` で決める。
    #[expect(
        clippy::too_many_arguments,
        reason = "the parts of a child a fork starts from"
    )]
    fn right_child(
        entry: u32,
        worker: u32,
        evv: Word,
        depth_offset: usize,
        max_depth: usize,
        j: JoinId,
        g: Word,
        limit: Limit,
    ) -> Task {
        let base = ROOT_BASE + ROOT_SIZE + 2;
        let mut stack = vec![Word::UNIT; base + BOUNDARY_SIZE];
        stack[base - 2] = header(FINISH, 0, ROOT_SIZE);
        stack[base - 1] = Word::from_bits(ROOT_BASE);
        stack[base + 1] = Word::from_bits(j);
        Task {
            stack,
            pc: FINISH,
            base,
            frame_size: BOUNDARY_SIZE,
            last_ret: FINISH,
            scratch: Vec::new(),
            stack_limit: task_stack_limit(max_depth.saturating_sub(depth_offset), limit),
            entry,
            keep_result: false,
            exit: None,
            evv,
            worker,
            depth_offset,
            boundaries: vec![j],
            state: TaskState::Unstarted(g),
            cell: None,
            outcome: None,
            placed: None,
            job: None,
            limit,
            counted: 0,
            added: 0,
        }
    }

    /// まだ始まっていない `compute` の仕事。右の子と同じ形で、境目の join は番兵 `JOB` である。深さはエントリの `Task`
    /// と同じく 0 から数える (docs/spec/vm.md の「`compute` の仕事」)。
    fn job(entry: u32, worker: u32, evv: Word, max_depth: usize, job: JobRef, g: Word) -> Task {
        Task {
            job: Some(job),
            ..Task::right_child(entry, worker, evv, 0, max_depth, JOB, g, Limit::Stops)
        }
    }

    /// worker 1..N-1 で、動くタスクのない間に VM の繰り返しが持つ `Task`。スタックも語も持たず、最初の切り替えで
    /// 表の `ENTRY` の欄に退避したまま、二度と動かさない。
    fn idle() -> Task {
        Task {
            stack: Vec::new(),
            pc: FINISH,
            base: 0,
            frame_size: 0,
            last_ret: FINISH,
            scratch: Vec::new(),
            stack_limit: 0,
            entry: 0,
            keep_result: false,
            exit: None,
            evv: Word::UNIT,
            worker: 0,
            depth_offset: 0,
            boundaries: Vec::new(),
            state: TaskState::Running,
            cell: None,
            outcome: None,
            placed: None,
            job: None,
            limit: Limit::Stops,
            counted: 0,
            added: 0,
        }
    }

    pub(crate) fn get(&self, slot: Slot) -> Word {
        self.stack[self.base + usize::from(slot)]
    }

    pub(crate) fn set(&mut self, slot: Slot, w: Word) {
        let index = self.base + usize::from(slot);
        self.stack[index] = w;
    }

    pub(crate) fn raw(&self, slot: Slot) -> i64 {
        self.get(slot).bits() as i64
    }

    pub(crate) fn set_raw(&mut self, slot: Slot, n: i64) {
        self.set(slot, Word::from_bits(n as usize));
    }

    /// `float` の値は、スロットに `f64` のビット列で置く (docs/spec/vm.md の「スロットとスタック」)。
    pub(crate) fn float(&self, slot: Slot) -> f64 {
        f64::from_bits(self.get(slot).bits() as u64)
    }

    pub(crate) fn set_float(&mut self, slot: Slot, x: f64) {
        self.set(slot, Word::from_bits(x.to_bits() as usize));
    }

    /// 深さ。右の子は fork のときの親の深さから数えるので、`par` を通る再帰も `(f (), g ())` と同じく深さの上限で
    /// 止まる (docs/spec/vm.md の「`par` の fork と join」)。
    pub(crate) fn depth(&self) -> usize {
        self.depth_offset + self.stack_depth()
    }

    /// このタスクのスタックで使っている語の数。安全点はこれを `Ctx::poll` に渡す。深さの起点は `switch_task` が
    /// `Ctx` の上限から引いてあるので、安全点で足さない (docs/spec/object-model.md の「タスクの切り替え」)。
    fn stack_depth(&self) -> usize {
        self.base + self.frame_size
    }

    /// スタックの長さを `end` 語まで伸ばす。容量は倍に伸ばすが、`stack_limit` で止める。`end` が上限を超えるときは、
    /// 容量を `end` ちょうどにする。
    pub(crate) fn reserve(&mut self, end: usize) {
        if self.stack.len() >= end {
            return;
        }
        if end > self.stack.capacity() {
            let capacity = (2 * self.stack.capacity()).min(self.stack_limit).max(end);
            self.stack.reserve_exact(capacity - self.stack.len());
        }
        self.stack.resize(end, Word::UNIT);
    }

    /// `slots` の語を scratch に読む。全部読んでから書くので、書き先が読んだスロットと重なってもよい。
    pub(crate) fn load_scratch(&mut self, slots: impl IntoIterator<Item = Slot>) {
        self.scratch.clear();
        for slot in slots {
            let w = self.get(slot);
            self.scratch.push(w);
        }
    }

    /// scratch の語をスタックの `at..` に書き、scratch を空にする。
    pub(crate) fn store_scratch_at(&mut self, at: usize) {
        let Task { stack, scratch, .. } = self;
        for (i, w) in scratch.drain(..).enumerate() {
            stack[at + i] = w;
        }
    }

    /// scratch の引数を今の base のスロット 0.. に書き、`f` の先頭へ進む。見出しはそのまま使う。
    pub(crate) fn enter_in_place<R: Exec>(&mut self, code: &CodeBase<R::M>, f: u32) {
        let entry = &code.functions[f as usize];
        self.reserve(self.base + usize::from(entry.frame_size));
        self.store_scratch_at(self.base);
        self.frame_size = usize::from(entry.frame_size);
        self.pc = entry.start;
    }

    /// 今のフレームの上に、戻り先 `ret` と結果のスロット `dst` を持つ空のフレームを作り、その base を返す。
    pub(crate) fn push_frame(&mut self, ret: Pc, dst: Slot, size: usize) -> usize {
        let base = self.base + self.frame_size + 2;
        self.reserve(base + size);
        self.stack[base - 2] = header(ret, dst, self.frame_size);
        self.stack[base - 1] = Word::from_bits(self.base);
        self.base = base;
        self.frame_size = size;
        base
    }

    /// 今のフレームを捨てて、呼び出し元の結果のスロットに `value` を書く。
    pub(crate) fn return_value(&mut self, value: Word) {
        let dst = self.pop_frame();
        self.set(dst, value);
    }

    /// 今のフレームを捨てて見出しの呼び出し元に戻り、見出しの結果のスロットを返す。
    #[inline(always)]
    pub(crate) fn pop_frame(&mut self) -> Slot {
        let (pc, dst, size) = read_header(self.stack[self.base - 2]);
        self.base = self.stack[self.base - 1].bits();
        self.frame_size = size;
        self.pc = pc;
        dst
    }
}

/// スタックの容量の上限。深さが `max_depth` を超えると次の安全点で止まるか中断を起こし (中断の間の猶予は
/// `task_stack_limit` が足す)、安全点の前には通常、1つの枠と見出し
/// (`u16::MAX + 2` 語) までを積むので、それより大きい容量は見込まない。余る引数の適用は呼ばれる側の枠と同じ安全点の
/// 前に `ApplyRest` の枠も積むので、求める長さが上限を超えうる。`reserve` は求める長さを下回らないので、そのときも足りる
/// (docs/spec/vm.md の「スロットとスタック」)。
fn stack_limit(max_depth: usize) -> usize {
    max_depth.saturating_add(usize::from(u16::MAX) + 2)
}

/// 状態が `limit` で始まるタスクのスタックの容量の上限。`Stops` でないタスクは中断の間 (`Aborting`) だけ深さの上限が
/// `LIMIT_HEADROOM` だけ高いので、容量の上限にも同じだけ足す。足さないと、猶予の間は積むたびに `reserve` が容量を
/// 長さちょうどに伸ばし直す (docs/spec/vm.md の「スロットとスタック」「タスクの上限」)。
fn task_stack_limit(real: usize, limit: Limit) -> usize {
    match limit {
        Limit::Stops => stack_limit(real),
        _ => stack_limit(real).saturating_add(LIMIT_HEADROOM),
    }
}

/// 見出しの1語目。戻り先、結果を書くスロット、呼び出し元の枠の大きさを1つの生の語に詰める
/// (docs/spec/vm.md の「フレーム」)。
fn header(ret: Pc, dst: Slot, caller_size: usize) -> Word {
    debug_assert!(caller_size <= usize::from(u16::MAX));
    Word::from_bits(((ret as usize) << 32) | (usize::from(dst) << 16) | caller_size)
}

fn read_header(w: Word) -> (Pc, Slot, usize) {
    let bits = w.bits();
    ((bits >> 32) as Pc, (bits >> 16) as Slot, bits & 0xFFFF)
}

/// 安全点の `poll` が求めること。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Polled {
    Continue,
    /// 切り替えずに知らせを集める。
    Collect,
    /// 深さが今の上限を超えた。本当の超過か、保留のタスクの上限か、猶予の超過かは`limit_hit`が決める。
    /// `poll`し直す前に下ろした`NOTICE`を`collect`で持ち、安全点がそれも果たす。
    Limit {
        collect: bool,
    },
}

/// `Ctx::poll` の VM での呼び方。速い道は `Ok(())` の分岐だけで、残りは `polled_stop` に置く
/// (docs/spec/vm.md の「タスクとスケジュール」)。
#[inline(always)]
fn poll<M: Mode>(ctx: &Ctx<M>, depth: usize, k: usize) -> Result<Polled, RunFault> {
    match ctx.poll(depth) {
        Ok(()) => Ok(Polled::Continue),
        Err(stop) => polled_stop(ctx, depth, stop, k),
    }
}

/// `poll`が止める理由を返したときの扱い。`DepthLimit`は`Polled::Limit`にして安全点に任せ、`Notice`以外は誤りとして
/// 返す。`Notice`は、yieldの途中でなければビットを下ろしてからもう一度`poll`する。間に命令がないので、深さの上限は
/// 正確で、同時に立った止めるビットは集めることより先に勝つ。`NOTICE`では切り替えない
/// (docs/spec/vm.mdの「タスクとスケジュール」「タスクの上限」)。
#[cold]
#[inline(never)]
fn polled_stop<M: Mode>(
    ctx: &Ctx<M>,
    depth: usize,
    stop: Stop,
    k: usize,
) -> Result<Polled, RunFault> {
    // `Ctx` は worker `k` の語を読むので、下ろすのも `k` 番の語である
    let control = ctx.heap().control();
    match stop {
        Stop::Notice => {}
        Stop::DepthLimit => return Ok(Polled::Limit { collect: false }),
        _ => return Err(stop.into()),
    }
    // yield の途中では切り替えも集めることもしない。ビットは立てたままにして、次の yield のない安全点で拾う
    if ctx.yielding() != 0 {
        return Ok(Polled::Continue);
    }
    control.take_notice(k);
    match ctx.poll(depth) {
        // 2回目の `poll` の前に立ち直した `NOTICE` も、集める地点が下ろす
        Ok(()) | Err(Stop::Notice) => Ok(Polled::Collect),
        // 下ろした `NOTICE` の知らせは、安全点が `limit_hit` の前に集める
        Err(Stop::DepthLimit) => Ok(Polled::Limit { collect: true }),
        Err(stop) => Err(stop.into()),
    }
}

/// 安全点 (docs/spec/vm.mdの「タスクとスケジュール」)。止める理由と深さを確かめた後で、方針の`tick`が求め、yieldの
/// 途中でなければ切り替えを試みる。yieldの途中の状態は`Ctx`が1つだけ持つので、そこでは切り替えない。`tick`は予算を
/// 数える方針があるので毎回呼ぶ。`NOTICE`は切り替えずに集めるので、今のタスクを続ける。
#[inline(always)]
fn safepoint<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
) -> Result<(), RunFault> {
    let polled = poll(ctx, task.stack_depth(), sched.index)?;
    if let Polled::Limit { collect } = polled {
        if limit_safepoint::<R>(code, ctx, task, sched, collect)? {
            preempt::<R>(code, ctx, task, sched)?;
        }
        return Ok(());
    }
    let tick = sched.policy.tick();
    if polled == Polled::Collect {
        collect_notice::<R>(ctx, task, sched)?;
    }
    if tick && ctx.yielding() == 0 {
        preempt::<R>(code, ctx, task, sched)?;
    }
    Ok(())
}

/// 深さの上限を超えた安全点。`poll`し直す前に下ろした`NOTICE`の知らせを先に集め、`limit_hit`の後で方針の
/// `tick`を数える。切り替えを試みるか (`tick`が求め、yieldの途中でも中断を起こしたばかりでもない) を返し、試みるのは
/// 呼び手である。`preempt`の繰り返しがRustの再帰を重ねないためである。`tick`を飛ばすと、予算で切り替える方針が
/// 保留のタスクを切り替えなくなる
/// (docs/spec/vm.mdの「タスクの上限」)。
#[cold]
#[inline(never)]
fn limit_safepoint<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    collect: bool,
) -> Result<bool, RunFault> {
    if collect {
        collect_notice::<R>(ctx, task, sched)?;
    }
    limit_hit::<R>(code, ctx, task, sched)?;
    let tick = sched.policy.tick();
    Ok(tick && ctx.yielding() == 0 && !matches!(task.limit, Limit::Aborting { .. }))
}

/// 安全点の `poll` が深さの上限を超えたと返した。タスクの外と猶予の間は run を止める。タスクの中では、関数のエントリの
/// 安全点で `ResourceLimit` の IO の失敗を起こし、それ以外の位置では保留のまま続ける。エントリのフレームは RC の対象の
/// 引数をすべて持ち、借りた値を持たないので、エントリの行だけで巻き戻せるためである。EP の補助の関数のエントリでも
/// 保留のまま続ける。どのエントリでも、深さが本当の限度と猶予の和を超えていれば run を止める
/// (docs/spec/vm.md の「タスクの上限」)。
#[cold]
#[inline(never)]
fn limit_hit<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
) -> Result<(), RunFault> {
    if matches!(task.limit, Limit::Stops | Limit::Aborting { .. }) {
        return Err(RunFault::Stopped(Stop::DepthLimit));
    }
    // yield の途中の状態は `Ctx` が1つだけ持ち、中断はそれを使うので、yield を終えるまで待つ
    if ctx.yielding() != 0 {
        return Ok(());
    }
    let Some(row) = code
        .tables
        .unwind_at(task.pc)
        .filter(|row| row.entry != UnwindEntry::No)
    else {
        return Ok(());
    };
    let real = real_limit(ctx, task.depth_offset);
    // 補助の関数のエントリで延ばしても深さの限度がなくならないよう、延ばす前に確かめる
    if task.stack_depth() > real.saturating_add(LIMIT_HEADROOM) {
        return Err(RunFault::Stopped(Stop::DepthLimit));
    }
    // 補助の関数のエントリの `evv` は利用者のプログラムのどの地点のものでもなく、起こすとプログラムに見えない
    // handler に届く
    if row.entry == UnwindEntry::Defers {
        return Ok(());
    }
    let real_over = task.stack_depth() > real;
    let message = if real_over {
        "the call depth limit is exceeded"
    } else {
        "the memory soft limit is exceeded"
    };
    task.limit = Limit::Aborting { pending: false };
    ctx.set_depth_limit(real.saturating_add(LIMIT_HEADROOM));
    let caught = abort_to::<R, _>(code, ctx, task, 0, ExternEffect::Io, IO_FAILED, |ctx| {
        [
            Word::from_bits(eml_lir::IO_RESOURCE_LIMIT as usize),
            ctx.fresh_string(message),
        ]
    })?;
    if !caught {
        // IO の evidence がない (`par` の子の中、底の `try_all` の外)。本当の超過なら run を止める。旗だけなら捨てずに
        // 保留へ戻し、evidence のある所の次のエントリで起こす。旗はまだ下ろしていないので、セルにも立ったままである
        if real_over {
            task.limit = Limit::Task;
            ctx.set_depth_limit(real);
            return Err(RunFault::Stopped(Stop::DepthLimit));
        }
        task.limit = Limit::Pending;
        ctx.set_depth_limit(0);
        return Ok(());
    }
    // 起こしたので、本当の超過で起こすときも、まだ `Task` に届いていない旗も消費する。下ろすのを起こせたときに限るのは、
    // evidence のない間の安全点ごとに lock を取らないためである。セルがもう死んでいれば、旗を見る者もいない
    if let Some(cell) = task.cell {
        sched.cells.lock().clear_limit(cell)?;
        // `clear_limit` はセルの数を 0 にするので、タスクの数もここから数え直す。旗で起こしたときも本当の超過で
        // 起こしたときも同じにして、セルとタスクの数をずらさない (docs/spec/vm.md の「タスクとスケジュール」)
        task.counted = 0;
        task.added = 0;
        sched.mark = ctx.heap().control().taken(sched.index);
        sched.publish(task);
    }
    Ok(())
}

/// 深さの起点が `depth_offset` のタスクの本当の深さの限度。`switch_task` が `Ctx` の上限に書く値と同じである
/// (docs/spec/object-model.md の「タスクの切り替え」)。
fn real_limit<M: Mode>(ctx: &Ctx<M>, depth_offset: usize) -> usize {
    ctx.heap()
        .control()
        .limits()
        .max_depth
        .saturating_sub(depth_offset)
}

/// `Ctx` の深さの上限を、`switch_task` が書いた本当の限度から `limit` の状態の値に直す
/// (docs/spec/vm.md の「タスクの上限」)。
fn apply_limit<M: Mode>(ctx: &mut Ctx<M>, limit: Limit, depth_offset: usize) {
    match limit {
        Limit::Stops | Limit::Task => {}
        Limit::Pending => ctx.set_depth_limit(0),
        Limit::Aborting { .. } => {
            let headroom = real_limit(ctx, depth_offset).saturating_add(LIMIT_HEADROOM);
            ctx.set_depth_limit(headroom);
        }
    }
}

/// `Ctx::switch_task` の唯一の呼び口。`switch_task` は上限を本当の限度に書き直すので、続けて `limit` の状態を
/// 当て直す。通さないと保留と猶予が切り替えで消える (docs/spec/vm.md の「タスクの上限」)。
///
/// # Safety
/// `Ctx::switch_task` と同じ。
unsafe fn switch_to<M: Mode>(
    ctx: &mut Ctx<M>,
    evv: Word,
    worker: u32,
    depth_offset: usize,
    limit: Limit,
) -> Word {
    let saved = unsafe { ctx.switch_task(evv, worker, depth_offset) };
    apply_limit(ctx, limit, depth_offset);
    saved
}

/// 今動いているタスクに上限の旗が届いた。状態は `Limit::flagged` で決め、`Task` から保留に変わったときだけ上限を 0 に
/// する。中断の間は着地の後に保留へ戻す印だけが付き、上限は猶予のままである。表に退いたタスクはこれを通らず、
/// `mark_limited` が状態だけを変え、上限は入るときに `switch_to` が当てる (docs/spec/vm.md の「タスクの上限」)。
fn mark_pending<M: Mode>(ctx: &mut Ctx<M>, task: &mut Task) {
    let before = task.limit;
    task.limit = before.flagged();
    if before == Limit::Task {
        apply_limit(ctx, task.limit, task.depth_offset);
    }
}

/// 集める地点が受けた上限の旗を、そのセルの VM の `Task` に付ける。今のタスクは `mark_pending` で上限まで直す。表に
/// 退いたタスクは状態だけを変え、上限は切り替えで入るときに `switch_to` が当てる。同じセルの `Task` は、待ちに入った
/// 後で `Finish` へ戻る前のエントリの `Task` と次のエントリの `Task` の2つがありうるので、すべてに付ける。前のほうは
/// 底の `try_all` の外にいて IO の evidence がないので、旗では起こさない。どれもなければ何もしない。`Finish` に着いた
/// エントリの `Task` で、旗は次の `started` か `resumed` が返すか、セルと一緒に消える (docs/spec/vm.md の「タスクの上限」)。
#[cold]
#[inline(never)]
fn mark_limited<P: Policy, M: Mode>(ctx: &mut Ctx<M>, task: &mut Task, sched: &mut Scheduler<P>) {
    for cell in std::mem::take(&mut sched.limited) {
        if task.cell == Some(cell) {
            mark_pending(ctx, task);
        }
        for waiting in sched.tasks.iter_mut().flatten() {
            if waiting.cell == Some(cell) {
                waiting.limit = waiting.limit.flagged();
            }
        }
    }
}

/// 巻き戻しが着地した。猶予を外し、起こす前に保留だったタスクは保留へ戻す (docs/spec/vm.md の「タスクの上限」)。
fn landed<M: Mode>(ctx: &mut Ctx<M>, task: &mut Task) {
    if let Limit::Aborting { pending } = task.limit {
        if pending {
            task.limit = Limit::Pending;
            ctx.set_depth_limit(0);
        } else {
            task.limit = Limit::Task;
            let real = real_limit(ctx, task.depth_offset);
            ctx.set_depth_limit(real);
        }
    }
}

/// 安全点で `NOTICE` を見たときの集める地点。安全点の速い道に残さないように分けてある。計算の重いタスクが走る
/// worker でも I/O の完了が届くように、OS を1回見てから集める (docs/spec/vm.md の「時計と driver」)。
#[cold]
#[inline(never)]
fn collect_notice<R: Exec>(
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
) -> Result<(), RunFault> {
    collect::<R>(ctx, task, sched, true)
}

/// タスクを実行する。`task` はエントリのタスクで、切り替えるたびに動いているタスクと入れ替わる。成功して戻るときはエントリの
/// タスクに戻っている (`Finish` が確かめる)。誤りで戻る前に、ヒープの worker と `Ctx` の深さの上限をエントリのタスクのものに戻す。返る語は失敗した
/// タスクの `evv` で、手放さずに捨てる。誤りの後のヒープは回復で置き換えるか、`Ctx` ごと落とすので、捨てた語は漏れに
/// 数えない (docs/spec/vm.md の「`par` の fork と join」)。
pub(crate) fn run<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    counts: &mut RunCounts,
) -> Result<(), RuntimeError> {
    match run_tasks::<R>(code, ctx, task, sched, counts) {
        Ok(()) => Ok(()),
        Err(error) => {
            // 検査モードの前提は `wf` と `()` だけで、誤りの後でも成り立つ
            let _abandoned =
                unsafe { switch_to(ctx, Word::UNIT, sched.main_worker, 0, Limit::Stops) };
            Err(error)
        }
    }
}

/// worker `k` (`k` ≥ 1) の実行の繰り返し。動くタスクのない `Task` から始め、置かれた右の子と仕事に切り替えて動かす。
/// 右の子と仕事は `Finish` に着かないので、戻るのは run を止める誤りのときだけで、その誤りを返す。止めた後のタスクは
/// 巻き戻さずに捨てる (docs/spec/vm.md の「タスクとスケジュール」)。
pub(crate) fn work<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    sched: &mut Scheduler<R::P>,
    counts: &mut RunCounts,
) -> (RuntimeError, u64) {
    let mut task = Task::idle();
    let error = work_loop::<R>(code, ctx, &mut task, sched, counts);
    // 2つ目は、この worker のタスクのスタックで同時に使った語の数の最大。worker 0 が `RunStats` に最大を取る
    (error, sched.peak_stack(&task))
}

fn work_loop<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    counts: &mut RunCounts,
) -> RuntimeError {
    // 動くタスクのない `Task` は `ENTRY` の欄に退避し、その番号をどのタスクにも振らない
    if let Err(fault) = switch_away::<R>(code, ctx, task, sched, Some(TaskState::Ready)) {
        let pc = task.pc;
        if let Err(error) = task_failed::<R>(code, ctx, task, sched, Failure { fault, pc }) {
            return error;
        }
    }
    match run_tasks::<R>(code, ctx, task, sched, counts) {
        Ok(()) => code.error(
            Failure {
                fault: RunFault::Internal("a placed child reached the end of a run"),
                pc: task.pc,
            },
            task,
        ),
        Err(error) => error,
    }
}

/// メインのworkerの実行の繰り返し。`park`で待ち、`park`が積まれた仕事か期限の来たタイマーを残して戻れば、
/// `Pending::run`と同じ規則で`park`の代わりにその仕事を動かす。最初の待ちの前には固定したタスクを動かさないので、
/// どの固定したタスクが動く前にも、`park`が先に制御を受け取る。止める語か誤りで抜け、その誤りを返す。止める語で
/// 抜けたときは`None`である (docs/spec/vm.mdの「メインのworker」)。
pub(crate) fn main_work<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    sched: &mut Scheduler<R::P>,
    counts: &mut RunCounts,
    park: &mut dyn Park,
) -> (Option<RuntimeError>, u64) {
    let mut task = Task::idle();
    task.worker = sched.main_worker;
    let error = loop {
        match main_wait::<R>(code, ctx, &mut task, sched, counts, park) {
            Ok(true) => {}
            Ok(false) => break None,
            Err(error) => break Some(error),
        }
    };
    (error, sched.peak_stack(&task))
}

fn main_fault(fault: RunFault) -> RuntimeError {
    RuntimeError::Fault {
        fault,
        function: "<main worker>".into(),
        at: None,
    }
}

/// メインのworkerの1回の待ち。仕事がもう積まれていなければメインのworkerを眠った形にする。積まれた仕事とともに
/// 始まる待ちは鳴らしておくので、`park`はすぐに戻るか`run`を呼ぶ。`park`が戻った後、メインのworkerが起こされたか、
/// 知らせがあるか、もう眠った形でないか、止める語を読んだか、本物の時計で自分の期限を過ぎていれば、`park`の代わりに
/// 仕事を動かす。どれもなければ眠った形のままにする。メインのworkerが止まれば偽を返す。`run`の誤りは待ちの後に返す
/// (docs/spec/vm.mdの「メインのworkerの待ち」)。
fn main_wait<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    counts: &mut RunCounts,
    park: &mut dyn Park,
) -> Result<bool, RuntimeError> {
    let k = sched.index;
    if !sched.main_park().map_err(main_fault)? {
        sched.run.slots[k].ring();
    }
    let timeout = sched.main_timeout();
    sched.guard = Guard::Parked;
    let mut pending = MainPending::<R> {
        code,
        ctx: &mut *ctx,
        task: &mut *task,
        sched: &mut *sched,
        counts: &mut *counts,
        stop: None,
        from_park: true,
    };
    let waited = park.wait(timeout, &mut pending);
    let stop = pending.stop.take();
    if let Some(error) = stop {
        sched.guard = Guard::Outside;
        return Err(error);
    }
    // driverの誤りは`default_wait`が`stop`に残したので、ここに来る誤りは`Park`自身のものである
    if let Err(e) = waited {
        sched.guard = Guard::Outside;
        return Err(main_fault(RunFault::Park(e.to_string())));
    }
    if matches!(sched.guard, Guard::Stopped(_)) {
        sched.guard = Guard::Outside;
        return Ok(false);
    }
    // 本物の時計では、メインのworkerの区画の期限で起こすものはほかにいない。期限を過ぎて戻った`Park`をまた
    // 呼ぶと、残りが0の待ちを繰り返すので、起きた形にしてタイマーを拾う
    let due = sched.main_timeout() == Some(Duration::ZERO);
    if !sched.run.main_woken(&sched.watcher.control, k) && !due {
        sched.guard = Guard::Outside;
        return Ok(true);
    }
    let mut pending = MainPending::<R> {
        code,
        ctx: &mut *ctx,
        task: &mut *task,
        sched: &mut *sched,
        counts: &mut *counts,
        stop: None,
        from_park: false,
    };
    let progress = pending.run();
    let stop = pending.stop.take();
    sched.guard = Guard::Outside;
    match (progress, stop) {
        (_, Some(error)) => Err(error),
        (Progress::Stopping, None) => Ok(false),
        _ => Ok(true),
    }
}

/// `Park::wait`に渡す、メインのworkerの仕事。待ちごとに作り、`Scheduler`にまた触れる前に落とす。
struct MainPending<'a, R: Exec> {
    code: &'a CodeBase<R::M>,
    ctx: &'a mut Ctx<R::M>,
    task: &'a mut Task,
    sched: &'a mut Scheduler<R::P>,
    counts: &'a mut RunCounts,
    /// `run`の`drive`が止まった誤り。待ちの後で`main_wait`が返す。
    stop: Option<RuntimeError>,
    /// `Park`に渡したものか。`Park`はdriverの代わりにdriverのOSの源で待ちうるので、`Park`が呼ぶ`run`は始めに1回、
    /// 待たずにdriverに問う。完了を取り、源に残った起こしも消すためである (docs/spec/vm.mdの「メインのworkerの待ち」)。
    from_park: bool,
}

impl<R: Exec> Pending for MainPending<'_, R> {
    fn run(&mut self) -> Progress {
        if let Some(progress) = self.sched.guard.enter() {
            return progress;
        }
        self.sched.run.unpark(self.sched.index);
        if self.from_park
            && let Err(error) = self.sched.look_once()
        {
            self.stop = Some(main_fault(error));
            self.sched.guard = Guard::Stopped(0);
            return Progress::Stopping;
        }
        loop {
            let driven = drive::<R>(self.code, self.ctx, self.task, self.sched, self.counts)
                .and_then(|go| {
                    if !go {
                        return Ok(None);
                    }
                    self.sched.main_park().map(Some).map_err(main_fault)
                });
            match driven {
                Ok(Some(true)) => {
                    self.sched.guard = Guard::Parked;
                    return Progress::Wait(self.sched.main_timeout());
                }
                // 眠る前に起こされたので、もう一度動かす
                Ok(Some(false)) => {}
                Ok(None) => {
                    self.sched.guard = Guard::Stopped(0);
                    return Progress::Stopping;
                }
                Err(error) => {
                    self.stop = Some(error);
                    self.sched.guard = Guard::Stopped(0);
                    return Progress::Stopping;
                }
            }
        }
    }

    fn default_wait(&mut self) -> std::io::Result<()> {
        if matches!(self.sched.guard, Guard::Stopped(_)) {
            return Ok(());
        }
        let waited = self.sched.main_default_wait();
        if let Err(e) = &waited {
            self.stop = Some(main_fault(RunFault::Driver(e.to_string())));
            self.sched.guard = Guard::Stopped(0);
        }
        waited
    }

    fn set_holds(&mut self, holds: bool) {
        if self.sched.run.swap_holds(holds) && !holds {
            self.sched.run.wake(0);
        }
    }

    fn io(&self) -> Option<PollSource> {
        self.sched.poll
    }

    fn poke(&mut self, gate: i64) {
        if let Err(fault) = poke_gate(self.sched, gate) {
            self.fail(fault);
        }
    }

    fn release_gate(&mut self, gate: i64) {
        let released = self.sched.cells.lock().release_gate(gate);
        if let Err(fault) = released {
            self.fail(fault.into());
        }
    }
}

impl<R: Exec> MainPending<'_, R> {
    /// `Park`が呼んだ登録表の操作の誤りを、`run`の誤りと同じくrunの誤りにする。続く`run`は`Stopping`を返す。
    fn fail(&mut self, fault: RunFault) {
        if self.stop.is_none() {
            self.stop = Some(main_fault(fault));
        }
        self.sched.guard = Guard::Stopped(0);
    }
}

/// ゲート`gate`に知らせ、起こしたタスクを積む。`Std.Gui.poke_raw`の腕とメインのworkerの`Pending::poke`が使う
/// (docs/spec/vm.mdの「セルと登録表」)。
fn poke_gate<P: Policy>(sched: &mut Scheduler<P>, gate: i64) -> Result<(), RunFault> {
    let mut runs = Vec::new();
    {
        here!(sched, post, here);
        let mut cells = sched.cells.lock();
        cells.poke_gate(gate, &mut sched.woken)?;
        cells.route(&mut sched.woken, &here, &mut runs)?;
    }
    for run in runs {
        schedule(sched, run)?;
    }
    Ok(())
}

/// メインのworkerの1回の仕事。受け箱の知らせを取り、走れるタスクを列が空になるまで動かして戻る。列が空になった
/// 地点では`HOST_IDLE`でVMの繰り返しを抜けるので、メインのworkerが待つ間、VMの状態はRustのスタックに残らない。
/// 動くタスクのない`Task`は、ほかのworkerと同じく表の`ENTRY`の欄に退避する。止める語を読んだら`false`を返す。
/// 呼ぶのは`Pending::run`だけである (docs/spec/vm.mdの「メインのworker」)。
fn drive<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    counts: &mut RunCounts,
) -> Result<bool, RuntimeError> {
    if crate::workers::stopping(&sched.watcher.control, sched.index) {
        return Ok(false);
    }
    let mut ran = match switch_away::<R>(code, ctx, task, sched, Some(TaskState::Ready)) {
        Ok(()) => Ok(()),
        Err(fault) => {
            let pc = task.pc;
            task_failed::<R>(code, ctx, task, sched, Failure { fault, pc })
        }
    };
    if ran.is_ok() {
        ran = match run_tasks::<R>(code, ctx, task, sched, counts) {
            Ok(()) => Err(code.error(
                Failure {
                    fault: RunFault::Internal("a pinned task reached the end of a run"),
                    pc: task.pc,
                },
                task,
            )),
            Err(error) => Err(error),
        };
    }
    match ran {
        Err(RuntimeError::Fault { fault, .. }) if fault == HOST_IDLE => Ok(true),
        Ok(()) => Ok(true),
        Err(error) => Err(error),
    }
}

fn run_tasks<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    counts: &mut RunCounts,
) -> Result<(), RuntimeError> {
    loop {
        let pc = task.pc;
        match step::<R>(code, ctx, task, sched, counts, pc) {
            Ok(true) => {}
            Ok(false) => return Ok(()),
            Err(fault) => {
                if let Err(fault) = step_failed::<R>(code, ctx, task, pc, fault) {
                    task_failed::<R>(code, ctx, task, sched, Failure { fault, pc })?;
                }
            }
        }
    }
}

/// 命令を1つ実行する。`Finish` に着いたら `false` を返す。
///
/// 結果のスロットは引数のスロットと同じ色でありうるので (docs/spec/vm.md の「スロットの割り当て」)、どの腕も引数を
/// 読み終えてから結果を書く。
#[inline(always)]
fn step<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    counts: &mut RunCounts,
    pc: Pc,
) -> Result<bool, RunFault> {
    task.pc = pc + 1;
    let ops = |at: u32| &code.tables.operands[at as usize..];
    match code.tables.code[pc as usize] {
        Op::Move { dst, src } => task.set(dst, task.get(src)),
        Op::LoadRaw { dst, value } => task.set_raw(dst, value.into()),
        Op::LoadImm { dst, value } => task.set(dst, Word::imm(value.into())),
        Op::LoadConst { dst, index } => task.set(dst, code.constants[index as usize]),
        Op::Goto { target } => task.pc = target,
        Op::Ret { src } => {
            let value = task.get(src);
            task.last_ret = pc;
            task.return_value(value);
            safepoint::<R>(code, ctx, task, sched)?;
        }
        Op::Call { dst, at } => {
            call::<R>(code, task, dst, at);
            safepoint::<R>(code, ctx, task, sched)?;
        }
        Op::TailCall { at } => {
            let o = ops(at);
            task.load_scratch(slots(&o[2..2 + o[1] as usize]));
            task.enter_in_place::<R>(code, o[0]);
            safepoint::<R>(code, ctx, task, sched)?;
        }
        Op::CallN { at } => call_many::<R>(code, ctx, task, sched, at)?,
        Op::RetN { at } => return_many::<R>(code, ctx, task, sched, pc, at)?,
        Op::Finish => {
            if sched.current != ENTRY {
                entry_finish::<R>(code, ctx, task, sched)?;
                return Ok(true);
            }
            finish::<R>(code, ctx, task, sched)?;
            return Ok(false);
        }
        Op::ApplyRest => {
            apply::apply_rest::<R>(code, ctx, task)?;
            safepoint::<R>(code, ctx, task, sched)?;
        }
        Op::ChildDone => {
            let value = task.get(0);
            child_done::<R>(code, ctx, task, sched, Outcome::Value(value))?;
        }
        Op::Par { dst, at } => par::<R>(code, ctx, task, sched, dst, at)?,
        Op::Async { dst, at } => async_op::<R>(code, ctx, task, sched, dst, at)?,
        Op::Closure { dst, at } => {
            let o = ops(at);
            let (f, lin, n) = (o[0], o[1] != 0, o[2] as usize);
            task.load_scratch(slots(&o[3..3 + n]));
            let w = apply::make_closure::<R>(code, ctx, f, lin, &task.scratch)?;
            task.scratch.clear();
            task.set(dst, w);
        }
        Op::Apply { dst, at } => {
            let target = apply::Target::Frame { ret: task.pc, dst };
            apply_call::<R>(code, ctx, task, sched, at, target)?;
        }
        Op::Clause { dst, at } => {
            let target = apply::Target::Frame { ret: task.pc, dst };
            clause_call::<R>(code, ctx, task, sched, at, target)?;
        }
        Op::TailApply { at } => apply_call::<R>(code, ctx, task, sched, at, apply::Target::Tail)?,
        Op::TailClause { at } => clause_call::<R>(code, ctx, task, sched, at, apply::Target::Tail)?,
        Op::TailSelf { at } => {
            let o = ops(at);
            task.load_scratch(slots(&o[2..2 + o[1] as usize]));
            task.store_scratch_at(task.base);
            task.pc = o[0];
            // バイトコードに後ろ向きの辺ができるので、ここも安全点にする (docs/spec/vm.md の「呼び出しと戻り」)
            safepoint::<R>(code, ctx, task, sched)?;
        }
        // 誤りは `run` が `Panic` に届ける (`step_failed`)
        Op::IntAdd { dst, a, b } => task.set_raw(dst, unsafe {
            eml_rows::rows::IntAdd::<R::M>(ctx, task.raw(a), task.raw(b))
        }?),
        Op::IntSub { dst, a, b } => task.set_raw(dst, unsafe {
            eml_rows::rows::IntSub::<R::M>(ctx, task.raw(a), task.raw(b))
        }?),
        Op::IntMul { dst, a, b } => task.set_raw(dst, unsafe {
            eml_rows::rows::IntMul::<R::M>(ctx, task.raw(a), task.raw(b))
        }?),
        Op::IntDiv { dst, a, b } => task.set_raw(dst, unsafe {
            eml_rows::rows::IntDiv::<R::M>(ctx, task.raw(a), task.raw(b))
        }?),
        Op::IntRem { dst, a, b } => task.set_raw(dst, unsafe {
            eml_rows::rows::IntMod::<R::M>(ctx, task.raw(a), task.raw(b))
        }?),
        Op::IntNeg { dst, a } => task.set_raw(dst, unsafe {
            eml_rows::rows::IntNeg::<R::M>(ctx, task.raw(a))
        }?),
        Op::IntEq { dst, a, b } => task.set_raw(dst, unsafe {
            eml_rows::rows::IntEq::<R::M>(ctx, task.raw(a), task.raw(b))
        }?),
        Op::IntNe { dst, a, b } => task.set_raw(dst, unsafe {
            eml_rows::rows::IntNe::<R::M>(ctx, task.raw(a), task.raw(b))
        }?),
        Op::IntLt { dst, a, b } => task.set_raw(dst, unsafe {
            eml_rows::rows::IntLt::<R::M>(ctx, task.raw(a), task.raw(b))
        }?),
        Op::IntLe { dst, a, b } => task.set_raw(dst, unsafe {
            eml_rows::rows::IntLe::<R::M>(ctx, task.raw(a), task.raw(b))
        }?),
        Op::IntGt { dst, a, b } => task.set_raw(dst, unsafe {
            eml_rows::rows::IntGt::<R::M>(ctx, task.raw(a), task.raw(b))
        }?),
        Op::IntGe { dst, a, b } => task.set_raw(dst, unsafe {
            eml_rows::rows::IntGe::<R::M>(ctx, task.raw(a), task.raw(b))
        }?),
        Op::IntCompare { dst, a, b } => task.set_raw(dst, unsafe {
            eml_rows::rows::IntCompare::<R::M>(ctx, task.raw(a), task.raw(b))
        }?),
        Op::FloatAdd { dst, a, b } => task.set_float(dst, unsafe {
            eml_rows::rows::FloatAdd::<R::M>(ctx, task.float(a), task.float(b))
        }?),
        Op::FloatSub { dst, a, b } => task.set_float(dst, unsafe {
            eml_rows::rows::FloatSub::<R::M>(ctx, task.float(a), task.float(b))
        }?),
        Op::FloatMul { dst, a, b } => task.set_float(dst, unsafe {
            eml_rows::rows::FloatMul::<R::M>(ctx, task.float(a), task.float(b))
        }?),
        Op::FloatDiv { dst, a, b } => task.set_float(dst, unsafe {
            eml_rows::rows::FloatDiv::<R::M>(ctx, task.float(a), task.float(b))
        }?),
        Op::FloatRem { dst, a, b } => task.set_float(dst, unsafe {
            eml_rows::rows::FloatMod::<R::M>(ctx, task.float(a), task.float(b))
        }?),
        Op::FloatNeg { dst, a } => task.set_float(dst, unsafe {
            eml_rows::rows::FloatNeg::<R::M>(ctx, task.float(a))
        }?),
        Op::FloatEq { dst, a, b } => task.set_raw(dst, unsafe {
            eml_rows::rows::FloatEq::<R::M>(ctx, task.float(a), task.float(b))
        }?),
        Op::FloatNe { dst, a, b } => task.set_raw(dst, unsafe {
            eml_rows::rows::FloatNe::<R::M>(ctx, task.float(a), task.float(b))
        }?),
        Op::FloatLt { dst, a, b } => task.set_raw(dst, unsafe {
            eml_rows::rows::FloatLt::<R::M>(ctx, task.float(a), task.float(b))
        }?),
        Op::FloatLe { dst, a, b } => task.set_raw(dst, unsafe {
            eml_rows::rows::FloatLe::<R::M>(ctx, task.float(a), task.float(b))
        }?),
        Op::FloatGt { dst, a, b } => task.set_raw(dst, unsafe {
            eml_rows::rows::FloatGt::<R::M>(ctx, task.float(a), task.float(b))
        }?),
        Op::FloatGe { dst, a, b } => task.set_raw(dst, unsafe {
            eml_rows::rows::FloatGe::<R::M>(ctx, task.float(a), task.float(b))
        }?),
        Op::BoolEq { dst, a, b } => task.set_raw(dst, unsafe {
            eml_rows::rows::BoolEq::<R::M>(ctx, task.raw(a), task.raw(b))
        }?),
        Op::BoolNe { dst, a, b } => task.set_raw(dst, unsafe {
            eml_rows::rows::BoolNe::<R::M>(ctx, task.raw(a), task.raw(b))
        }?),
        Op::SwitchEnum { src, at } => {
            data::switch_tag::<R>(code, ctx, task, task.get(src), task.raw(src), at)?
        }
        Op::SwitchObj { src, at } => {
            let w = task.get(src);
            // scrutinee の Repr は `obj` なので、語はポインタである (docs/spec/vm.md の「switch」)
            let tag = unsafe { ctx.heap().meta(w) }?.tag();
            data::switch_tag::<R>(code, ctx, task, w, tag.into(), at)?
        }
        Op::SwitchTObj { src, at } => {
            let w = task.get(src);
            let tag = if w.is_imm() {
                w.imm_value()
            } else {
                // `is_imm()` の枝を先に取ったので、ここでは語はポインタである (docs/spec/vm.md の「switch」)
                unsafe { ctx.heap().meta(w) }?.tag().into()
            };
            data::switch_tag::<R>(code, ctx, task, w, tag, at)?
        }
        Op::SwitchInt { src, at } => data::switch_int::<R>(code, task, task.raw(src), at)?,
        Op::SwitchString { src, at } => {
            data::switch_string::<R>(code, ctx, task, task.get(src), at)?
        }
        Op::Unpack { src, at } => data::unpack::<R>(code, ctx, task, src, at)?,
        // RC の命令は Repr が `obj` か `tobj` の変数にだけ出す。VM はその参照を持つか借りている
        Op::Dup { src } => unsafe { ctx.heap_mut().dup(task.get(src)) }?,
        Op::Decref { src } => unsafe { ctx.heap_mut().decref(task.get(src)) }?,
        Op::Release { src, at } => data::release::<R>(code, ctx, task, src, at)?,
        Op::UnboxStruct { src, at } => data::unbox_struct::<R>(code, ctx, task, src, at)?,
        Op::ReleaseUnboxed { src, at } => data::release_unboxed::<R>(code, ctx, task, src, at)?,
        Op::Con { dst, at } => data::con::<R>(code, ctx, task, dst, at)?,
        Op::ReleaseReuse { src, at } => data::release_reuse::<R>(code, ctx, task, src, at)?,
        Op::ConReuse { dst, at } => data::con_reuse::<R>(code, ctx, task, counts, dst, at)?,
        // token は verifier を通った IR の、このヒープのまだ使っていない token である
        Op::DropToken { tok } => {
            unsafe { ctx.heap_mut().drop_token(Token::from_word(task.get(tok))) }?
        }
        Op::BoxInt { dst, src } => {
            if R::M::CHECKED {
                counts.boxes += 1;
            }
            let w = unsafe { ctx.heap_mut() }.alloc_int(task.raw(src));
            task.set(dst, w);
        }
        Op::BoxEnum { dst, src } => {
            if R::M::CHECKED {
                counts.boxes += 1;
            }
            task.set(dst, Word::imm(task.raw(src)));
        }
        Op::BoxFloat { dst, src } => {
            if R::M::CHECKED {
                counts.boxes += 1;
            }
            let w = unsafe { ctx.heap_mut() }.alloc_float(task.raw(src) as u64);
            task.set(dst, w);
        }
        Op::BoxConst { dst, index } => {
            if R::M::CHECKED {
                counts.boxes += 1;
            }
            task.set(dst, code.constants[index as usize]);
        }
        Op::Unbox { dst, src } => {
            let n = data::unbox::<R>(ctx, task.get(src), counts)?;
            task.set_raw(dst, n);
        }
        Op::UnboxFloat { dst, src } => {
            let bits = data::unbox_float::<R>(ctx, task.get(src), counts)?;
            task.set_raw(dst, bits as i64);
        }
        // 配列の行は借りる引数を手放さない。誤りは `step_failed` が `Panic` に届ける
        Op::ArrayGet { dst, a, b } => task.set(dst, unsafe {
            eml_rows::rows::ArrayGet::<R::M>(ctx, task.get(a), task.raw(b))
        }?),
        Op::ArrayLength { dst, a } => task.set_raw(dst, unsafe {
            eml_rows::rows::ArrayLength::<R::M>(ctx, task.get(a))
        }?),
        // unboxed の行も借りる引数を手放さない。要素は生のビットなので、値のスロットは `task.raw` で読む
        Op::UnboxedGet { dst, a, b } => task.set_raw(dst, unsafe {
            eml_rows::rows::UnboxedArrayGetInt::<R::M>(ctx, task.get(a), task.raw(b))
        }?),
        Op::UnboxedLength { dst, a } => task.set_raw(dst, unsafe {
            eml_rows::rows::UnboxedArrayLength::<R::M>(ctx, task.get(a))
        }?),
        Op::UnboxedSet { dst, at } => {
            let o = &code.tables.operands[at as usize..];
            let w = unsafe {
                eml_rows::rows::UnboxedArraySetInt::<R::M>(
                    ctx,
                    task.get(o[0] as Slot),
                    task.raw(o[1] as Slot),
                    task.raw(o[2] as Slot),
                )
            }?;
            task.set(dst, w);
        }
        // 文字列の行も借りる引数を手放さない。誤りの扱いは配列の行と同じ
        Op::StrByteAt { dst, i, s } => task.set_raw(dst, unsafe {
            eml_rows::rows::StrByteAt::<R::M>(ctx, task.raw(i), task.get(s))
        }?),
        Op::StrByteLength { dst, s } => task.set_raw(dst, unsafe {
            eml_rows::rows::StrByteLength::<R::M>(ctx, task.get(s))
        }?),
        Op::BytesGet { dst, i, b } => task.set_raw(dst, unsafe {
            eml_rows::rows::BytesGet::<R::M>(ctx, task.raw(i), task.get(b))
        }?),
        Op::BytesLength { dst, b } => task.set_raw(dst, unsafe {
            eml_rows::rows::BytesLength::<R::M>(ctx, task.get(b))
        }?),
        Op::ArraySet { dst, at } => {
            let o = &code.tables.operands[at as usize..];
            let (a, i, x) = (
                task.get(o[0] as Slot),
                task.raw(o[1] as Slot),
                task.get(o[2] as Slot),
            );
            let w = unsafe { eml_rows::rows::ArraySet::<R::M>(ctx, a, i, x) }?;
            task.set(dst, w);
        }
        Op::Extern { dst, at } => extern_call::<R>(code, ctx, task, counts, dst, at)?,
        Op::Ffi { dst, at } => ffi::call::<R>(code, ctx, task, dst, at)?,
        Op::FfiSubmit { dst, at } => ffi_submit::<R>(code, ctx, task, sched, dst, at)?,
        Op::FfiTake { dst, at } => ffi::take::<R>(code, ctx, task, &sched.cells, dst, at)?,
        // 基本操作 (docs/spec/lir.md の「基本操作」) は `Ctx` の同じ名前のメソッドを呼ぶ。引数の語は、LIR の所有の規則に
        // より VM が持つ参照か借りた語で、基本操作の契約 (docs/spec/object-model.md の「基本操作の契約」) のとおりに
        // 渡す。借用の引数は `Ctx` が手放さない。VM は基本操作の合間に `Ctx` の持つ語に触れない
        Op::EvvGet { dst } => {
            let w = unsafe { ctx.evv_get() }?;
            task.set(dst, w);
        }
        Op::EvvSet { dst, l } => {
            unsafe { ctx.evv_set(task.get(l)) }?;
            task.set(dst, Word::UNIT);
        }
        Op::EvvPush { dst, l, e } => {
            let w = unsafe { ctx.evv_push(task.get(l), task.get(e)) }?;
            task.set(dst, w);
        }
        Op::EvvMask { dst, at } => {
            let o = ops(at);
            let items = &code.tables.mask_lists[o[0] as usize..(o[0] + o[1]) as usize];
            let w = unsafe { ctx.evv_mask(items, task.get(o[2] as Slot)) }?;
            task.set(dst, w);
        }
        Op::EvNew { dst, at } => {
            let o = ops(at);
            let (state, outer) = (task.get(o[2] as Slot), task.get(o[3] as Slot));
            let w = unsafe { ctx.ev_new(o[0] as u16, o[1] as usize, state, outer) }?;
            task.set(dst, w);
        }
        Op::EvSetClause { dst, at } => {
            let o = ops(at);
            let (ev, entry) = (task.get(o[0] as Slot), task.get(o[2] as Slot));
            unsafe { ctx.ev_set_clause(ev, o[1] as usize, entry) }?;
            task.set(dst, Word::UNIT);
        }
        Op::EvRenew { dst, ev, state } => {
            let w = unsafe { ctx.ev_renew(task.get(ev), task.get(state)) }?;
            task.set(dst, w);
        }
        Op::EvLookup { dst, effect } => {
            let w = unsafe { ctx.ev_lookup(effect) }?;
            task.set(dst, w);
        }
        Op::EvClause { dst, ev, op } => {
            let w = unsafe { ctx.ev_clause(task.get(ev), op.into()) }?;
            task.set(dst, w);
        }
        Op::EvEvv { dst, ev } => {
            let w = unsafe { ctx.ev_evv(task.get(ev)) }?;
            task.set(dst, w);
        }
        Op::EvTakeEvv { dst, ev } => {
            let w = unsafe { ctx.ev_take_evv(task.get(ev)) }?;
            task.set(dst, w);
        }
        Op::EvTakeState { dst, ev } => {
            let w = unsafe { ctx.ev_take_state(task.get(ev)) }?;
            task.set(dst, w);
        }
        Op::EvSetState { dst, ev, s } => {
            unsafe { ctx.ev_set_state(task.get(ev), task.get(s)) }?;
            task.set(dst, Word::UNIT);
        }
        Op::Yielding { dst } => {
            let r = ctx.yielding();
            task.set_raw(dst, r);
        }
        Op::YieldStatus { dst, ev } => {
            let r = ctx.yield_status(task.get(ev));
            task.set_raw(dst, r);
        }
        Op::YieldTo { dst, at } => {
            let o = ops(at);
            let (ev, clause, k) = (
                task.get(o[0] as Slot),
                task.get(o[1] as Slot),
                task.get(o[2] as Slot),
            );
            unsafe { ctx.yield_to(ev, clause, k) }?;
            task.set(dst, Word::UNIT);
        }
        Op::YieldAbort { ev, clause } => {
            let ev = task.get(ev);
            unsafe { ctx.yield_abort(ev, task.get(clause)) }?;
            unwind::<R>(code, ctx, task, counts, ev)?;
        }
        Op::YieldContTake { dst } => {
            let w = ctx.yield_cont_take()?;
            task.set(dst, w);
        }
        Op::YieldContSet { dst, k } => {
            unsafe { ctx.yield_cont_set(task.get(k)) }?;
            task.set(dst, Word::UNIT);
        }
        Op::YieldFinish { dst } => {
            let w = unsafe { ctx.yield_finish() }?;
            task.set(dst, w);
        }
        Op::Dummy { dst } => {
            let w = ctx.heap().dummy();
            task.set(dst, w);
        }
        Op::RootGet { dst, root } => {
            let w = unsafe { ctx.root_get(root) }?;
            task.set(dst, w);
        }
        Op::ChildAbort { ev, c } => child_abort::<R>(code, ctx, task, sched, counts, ev, c)?,
        Op::RootSet { dst, at } => {
            let o = ops(at);
            unsafe { ctx.root_set(o[0], task.get(o[1] as Slot)) }?;
            // 印を付け終えた根より前に置いた根は、次の fork で印を付ける
            sched.shared_roots = sched.shared_roots.min(o[0] as usize);
            task.set(dst, Word::UNIT);
        }
    }
    Ok(true)
}

/// オペランドの表のスロットの並び。
fn slots(list: &[u32]) -> impl Iterator<Item = Slot> {
    list.iter().map(|&slot| slot as Slot)
}

/// `apply` の命令。オペランドは [呼ばれる値, n, 引数…] である。引数は全部読んでから書くので、結果の
/// スロットがオペランドのスロットと同じ色でもよい (docs/spec/vm.md の「スロットの割り当て」)。`step` に展開するのは
/// `bench/apply_int.em` の命令の数がそのほうが少なかったためである。
#[inline(always)]
fn apply_call<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    at: u32,
    target: apply::Target,
) -> Result<(), RunFault> {
    let callee = load_call::<R>(code, task, at);
    apply::apply::<R>(code, ctx, task, callee, target)?;
    safepoint::<R>(code, ctx, task, sched)?;
    Ok(())
}

/// `clause` の命令。オペランドは [呼ばれる値, n, 引数…] である。
fn clause_call<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    at: u32,
    target: apply::Target,
) -> Result<(), RunFault> {
    let callee = load_call::<R>(code, task, at);
    apply::apply_clause::<R>(code, ctx, task, callee, target)?;
    safepoint::<R>(code, ctx, task, sched)?;
    Ok(())
}

/// 関数の値を呼ぶ命令のオペランド [呼ばれる値, n, 引数…] の引数を scratch に読み、呼ばれる値を返す。
#[inline(always)]
fn load_call<R: Exec>(code: &CodeBase<R::M>, task: &mut Task, at: u32) -> Word {
    let o = &code.tables.operands[at as usize..];
    task.load_scratch(slots(&o[2..2 + o[1] as usize]));
    task.get(o[0] as Slot)
}

/// 呼ぶ側の上に呼ばれる側のフレームを作り、引数を写して先頭へ進む。呼ぶ側のスロットはスタックに残る。`CallN` からも
/// 呼ぶので、`Call` の速い道に展開し続けるように `inline(always)` にする。
#[inline(always)]
fn call<R: Exec>(code: &CodeBase<R::M>, task: &mut Task, dst: Slot, at: u32) {
    let ops = &code.tables.operands[at as usize..];
    let n = ops[1] as usize;
    let entry = &code.functions[ops[0] as usize];
    let caller = task.base;
    let base = task.push_frame(task.pc, dst, usize::from(entry.frame_size));
    for (i, &slot) in ops[2..2 + n].iter().enumerate() {
        task.stack[base + i] = task.stack[caller + slot as usize];
    }
    task.pc = entry.start;
}

// 複数の値の命令は `step` に展開しない。展開すると `step` が大きくなり、値が1つの `Call` と `Ret` の速い道の命令の数が
// 増えた (`bench/run.sh` の `fib` で約 4%)。値が1つの呼び出しの速さを変えないためである

/// 複数の値を受ける直接の呼び出し。結果のスロットは呼ばれる側の `RetN` が読むので、見出しの結果のスロットは使わない。
#[inline(never)]
fn call_many<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    at: u32,
) -> Result<(), RunFault> {
    call::<R>(code, task, 0, at);
    safepoint::<R>(code, ctx, task, sched)?;
    Ok(())
}

/// `RetN` の命令。`pc` はその位置で、`Ret` と同じく最後に戻った関数として覚え、戻りを安全点にする。
#[inline(never)]
fn return_many<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    pc: Pc,
    at: u32,
) -> Result<(), RunFault> {
    ret_many::<R>(code, task, at)?;
    task.last_ret = pc;
    safepoint::<R>(code, ctx, task, sched)?;
    Ok(())
}

/// 複数の値の戻り。値を書くスロットは見出しの戻り先の直前の `CallN` から読む。見出しの
/// 形は変えずに済み、末尾呼び出しが見出しを引き継げば、そのまま末尾で呼んだ側の呼び出し元に書ける
/// (docs/spec/vm.md の「呼び出しと戻り」)。値は呼ばれる側のフレームから
/// 呼び出し元のフレームへ写すので、読む所と書く所は重ならない。
#[inline]
fn ret_many<R: Exec>(code: &CodeBase<R::M>, task: &mut Task, at: u32) -> Result<(), RunFault> {
    let o = &code.tables.operands[at as usize..];
    let n = o[0] as usize;
    let (ret, _, _) = read_header(task.stack[task.base - 2]);
    let dsts = receivers::<R>(code, ret, n)?;
    let callee = task.base;
    task.pop_frame();
    for (&src, &dst) in o[1..1 + n].iter().zip(dsts) {
        task.stack[task.base + dst as usize] = task.stack[callee + src as usize];
    }
    Ok(())
}

/// 戻り先 `ret` の直前の、複数の値を受ける命令の結果のスロット。`Finish` と `ApplyRest` の位置へは値が1つしか
/// 戻らないので、ほかの命令の後への戻りは内部の誤りである。検査モードは値の数も比べる。
#[inline]
fn receivers<R: Exec>(code: &CodeBase<R::M>, ret: Pc, n: usize) -> Result<&[u32], RunFault> {
    let o = |at: u32| &code.tables.operands[at as usize..];
    let dsts = match ret.checked_sub(1).map(|pc| code.tables.code[pc as usize]) {
        Some(Op::CallN { at }) => {
            let o = o(at);
            let count = 2 + o[1] as usize;
            &o[count + 1..count + 1 + o[count] as usize]
        }
        _ => return Err(not_received()),
    };
    if R::M::CHECKED && dsts.len() != n {
        return Err(not_received());
    }
    Ok(dsts)
}

#[cold]
#[inline(never)]
fn not_received() -> RunFault {
    RunFault::Internal("multiple values return to a place that does not receive them")
}

/// 中断を始めた命令の後で、タスクのスタックを巻き戻す (docs/spec/vm.md の「巻き戻しの手順」)。`task.pc` は中断を
/// 始めた命令の次の位置で、今のフレームの表を引く位置である。着地の印の evidence が `target` と同じ語のフレームに
/// 着いたら、呼び出しの結果にダミーを書いてそこから続ける。それまでのフレームは、着地の印があればその evidence の
/// 状態を手放し、表のスロットを手放して外す。
/// 巻き戻しの間は物体を作らず、`target` は着地するフレームが所有し続けるので、解放した物体の番地が `target` と
/// 取り違えられることはない。安全点を置かないのは、仕事が外すフレームの数で終わるためである。
/// `Ok` で戻る所はどれも着地なので、そこでタスクの上限の猶予を外す (docs/spec/vm.md の「タスクの上限」)。
fn unwind<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    counts: &mut RunCounts,
    target: Word,
) -> Result<(), RunFault> {
    // 最後に外したフレームの見出しにある、呼び出し元の結果のスロット
    let mut dst = None;
    loop {
        match task.pc {
            FINISH if task.exit.is_some() => {
                landed(ctx, task);
                return Ok(());
            }
            FINISH => {
                return Err(RunFault::Internal(
                    "an abort reached the bottom of the task",
                ));
            }
            // 子の中の handler は子の中に着地するので、子の境目に着くのは `child_abort` の巻き戻しだけである。宛先は
            // 不死のダミーで、どの着地の印とも合わない
            CHILD_DONE if task.exit.is_none() && target == ctx.heap().dummy() => {
                landed(ctx, task);
                return Ok(());
            }
            CHILD_DONE => {
                return Err(RunFault::Internal(
                    "an abort reached the boundary of a par child",
                ));
            }
            APPLY_REST => {
                // 余った引数はまだ渡していないこの枠の参照である
                for slot in apply::rest_arguments(task) {
                    unsafe { ctx.heap_mut().decref(task.get(slot)) }?;
                }
            }
            pc => {
                let entry = code
                    .tables
                    .unwind_at(pc)
                    .ok_or(RunFault::Internal("a frame stops outside an unwind point"))?;
                if let Some(landing) = entry.landing {
                    let ev = task.get(landing);
                    // 中断を始めたフレーム (`dst` がまだない) は、印の付いた呼び出しの最中ではないので印を持たない
                    debug_assert!(
                        dst.is_some(),
                        "the frame that started the abort carries a landing marker"
                    );
                    if let Some(dst) = dst
                        && ev == target
                    {
                        task.set(dst, ctx.heap().dummy());
                        landed(ctx, task);
                        return Ok(());
                    }
                    // 外す handler の状態を、ここで取り出して手放す。状態は値なので退いた evidence にしか届かず、
                    // evidence を通る輪はない (docs/spec/object-model.md の「evidence の3つの物体」)。取り出すのは
                    // 状態を早く解放するためだけである。生き残った継続は `resume$` で新しい evidence と状態を作るので、
                    // 古い状態はもう読まれない (docs/spec/vm.md の「巻き戻し」)
                    let state = unsafe { ctx.ev_take_state(ev) }?;
                    unsafe { ctx.heap_mut().decref(state) }?;
                }
                // 表のスロットは、LIR の所有の規則によりこのフレームが持つ参照である。`Lin` の物体も手放してよく、
                // 線形なハンドル (`FileIn`、`FileOut` など) はここで閉じる
                for &(slot, count) in &entry.slots {
                    for _ in 0..count {
                        unsafe { ctx.heap_mut().decref(task.get(slot)) }?;
                    }
                }
                // 表の token は、地点の後で生きていてまだ使っていない token である。手放さないと箱が残る
                for &slot in &entry.tokens {
                    unsafe { ctx.heap_mut().drop_token(Token::from_word(task.get(slot))) }?;
                }
            }
        }
        dst = Some(task.pop_frame());
        if R::M::CHECKED {
            counts.unwound_frames += 1;
        }
    }
}

// `par` の fork と join は呼び出しや戻りに比べてまれなので、`step` に展開しない。値が1つの呼び出しと戻りの速い道の
// 命令の列を変えないためである (docs/spec/vm.md の「`par` の fork と join」)

/// join が見つけた子の誤りを `Scheduler::failed` に置いたことを、実行の繰り返しに伝える印。外には出ない。
const CHILD_FAILED: RunFault = RunFault::Internal("a child failure is waiting in the scheduler");

/// メインのworkerの走れるタスクがなくなったことを、実行の繰り返しに伝える印。`drive`が受け取り、外には出ない
/// (docs/spec/vm.mdの「メインのworker」)。
const HOST_IDLE: RunFault = RunFault::Internal("the main worker has nothing to run");

/// `par` の fork。親の文脈で、右の子に ID を出して閉包と子の `evv` に印を付け、右の子をまだ始めずに列に入れ、左の子を
/// 親のスタックの上で始める。mask は evidence passing の包みがすでに当てている。
#[inline(never)]
fn par<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    dst: Slot,
    at: u32,
) -> Result<(), RunFault> {
    let o = &code.tables.operands[at as usize..];
    // 2つの閉包は `par` が消費する、VM の持つ参照である
    let (f, g) = (task.get(o[0] as Slot), task.get(o[1] as Slot));
    let (child, fork_evv) = fork_child::<R>(code, ctx, sched, g)?;
    // 2つの子が1つずつ参照を持つ。左の子は `Ctx` に入れ、右の子はタスクに退避する。入れ替えで出る親の `evv` の参照は
    // join の節点に移す
    let parent_evv = unsafe {
        ctx.heap_mut().dup(fork_evv)?;
        ctx.heap_mut().share(fork_evv, child)?;
        switch_to(ctx, fork_evv, task.worker, task.depth_offset, task.limit)
    };
    let ret = task.pc;
    let j = sched.add_join(JoinNode {
        parent: sched.current,
        right: ENTRY,
        right_worker: child,
        parent_evv,
        left: None,
        right_outcome: None,
        ret,
        dst,
        orphan: false,
    });
    let mut right = Task::right_child(
        task.entry,
        child,
        fork_evv,
        task.depth(),
        sched.max_depth,
        j,
        g,
        task.limit.inherit(),
    );
    // 空いている worker があれば右の子をその受け箱に置き、なければこの worker の列に入れる
    // (docs/spec/vm.md の「fork」)
    let home = idle_home::<R>(ctx, sched)?;
    let local = match home {
        Some(k) => {
            right.placed = Some(Placed {
                home: sched.index,
                at: ret - 1,
            });
            sched.join(j).right = PLACED;
            // 終わりの知らせを受け取るまで、worker 0 はデッドロックを決めない
            sched.run.in_flight.fetch_add(1, Ordering::AcqRel);
            sched.run.placed.fetch_add(1, Ordering::Relaxed);
            let control = std::sync::Arc::clone(&sched.watcher.control);
            sched.run.send(&control, k, Notice::Child(right));
            None
        }
        None => {
            let right = sched.add_task(right);
            sched.join(j).right = right;
            Some(right)
        }
    };
    let base = task.push_frame(ret, dst, BOUNDARY_SIZE);
    task.stack[base + 1] = Word::from_bits(j);
    task.boundaries.push(j);
    apply_to_unit::<R>(code, ctx, task, f, CHILD_DONE, 0)?;
    if let Some(right) = local {
        sched.run.push_ready(sched.index, Ready::Task(right));
    }
    safepoint::<R>(code, ctx, task, sched)
}

/// 右の子の worker ID を出し、子が読む語に印を付ける。`g` の後で子の `evv` を作って返すが、その印は呼び手が付ける。
fn fork_child<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    sched: &mut Scheduler<R::P>,
    g: Word,
) -> Result<(u32, Word), RunFault> {
    // 印付けは右の子が読む前に、送り手の ID のまま行う (docs/future/multicore.md の「印付けの表」)。`Ctx` の持つ語には
    // 触れない
    let child = unsafe { ctx.heap_mut() }.fork_worker();
    // 右の子は REPL の束縛を `root_get` で読むので、根にも印を付ける。印は回復まで残るので、まだ印のない根にだけ
    // 付ける。根の物体を `share` しても `heap_mut` の前提は崩れない (docs/spec/object-model.md の「セッションの根」)
    for r in unshared_roots(ctx, sched) {
        unsafe { ctx.heap_mut().share(r, child) }?;
    }
    // 値から届く evidence は退いたものだけなので、`g` の `share` は親がその場で書き換える evidence に届かない
    // (docs/spec/object-model.md の「evidence の3つの物体」)
    unsafe { ctx.heap_mut().share(g, child) }?;
    let parent = ctx.evv_word();
    let fork_evv = fork_evv::<R>(code, ctx, parent)?;
    Ok((child, fork_evv))
}

/// 右の子か `compute` の仕事を置く、空いている worker の添字。置かないときは `None`。
fn idle_home<R: Exec>(
    ctx: &Ctx<R::M>,
    sched: &mut Scheduler<R::P>,
) -> Result<Option<usize>, RunFault> {
    let home = match sched.workers.places() {
        true => sched.run.pop_idle(),
        false => None,
    };
    // 根に印を付けるのは worker 0 の fork だけなので、置く子が読む根にはもう印が付いている
    // (docs/spec/object-model.md の「セッションの根」)
    if home.is_some() && <R::M as Mode>::CHECKED && sched.shared_roots != ctx.roots().len() {
        return Err(RunFault::Internal(
            "a placed child may read an unshared root",
        ));
    }
    Ok(home)
}

/// まだ印のない根。印は回復まで残るので、返した根はこの後の fork とタスクの始まりで印を付け直さない。
fn unshared_roots<M: Mode, P: Policy>(ctx: &Ctx<M>, sched: &mut Scheduler<P>) -> Vec<Word> {
    let roots = ctx.roots().len();
    if sched.shared_roots >= roots {
        return Vec::new();
    }
    let fresh = ctx
        .roots()
        .skip(sched.shared_roots)
        .flatten()
        .copied()
        .collect();
    sched.shared_roots = roots;
    fresh
}

/// 閉包 `f` に `()` を適用し、戻り先 `ret` で結果を `dst` に受ける。`par` の2つの子の始まりと join の起こし直しは、
/// どれもこの形である (docs/spec/vm.md の「`par` の fork と join」)。
fn apply_to_unit<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    f: Word,
    ret: Pc,
    dst: Slot,
) -> Result<(), RunFault> {
    task.scratch.clear();
    task.scratch.push(Word::UNIT);
    apply::apply::<R>(code, ctx, task, f, apply::Target::Frame { ret, dst })
}

/// 右の子の `evv`。mask を当てた後の親の `evv` を頭からたどり、`Panic` を除く never-only のエフェクトの evidence ごとに、
/// そのエフェクトの evidence のうち何番目かを状態の欄に持つ fork evidence と節点を作り、凍結した末尾につなぐ。子の列が
/// 親の列と同じ形になるので、子の中の `mask` は親の中と同じだけ evidence を飛ばす。該当する evidence がなければ、末尾
/// そのものを返して確保しない (docs/spec/vm.md の「`par` の fork と join」)。
fn fork_evv<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    parent: Word,
) -> Result<Word, RunFault> {
    let forks = &code.forks;
    let mut mirrored: Vec<(u16, Word, u32)> = Vec::new();
    let mut ranks: Vec<(u16, u32)> = Vec::new();
    let heap = ctx.heap();
    let mut node = parent;
    // `parent` は `Ctx` が参照を持つ `evv` のリストで、節点も evidence も生きている
    while node.is_ptr() {
        let effect = unsafe { heap.meta(node) }?
            .tag()
            .checked_sub(1)
            .ok_or(RunFault::Internal("an evv node has tag 0"))?;
        if Some(effect) != forks.panic {
            match forks.tables.get(usize::from(effect)) {
                Some(ForkTable::Resumable) => {}
                Some(&ForkTable::Table(table)) => {
                    let depth = match ranks.iter_mut().find(|(e, _)| *e == effect) {
                        Some((_, rank)) => {
                            *rank += 1;
                            *rank
                        }
                        None => {
                            ranks.push((effect, 0));
                            0
                        }
                    };
                    mirrored.push((effect, table, depth));
                }
                _ => {
                    return Err(RunFault::Internal(
                        "a never-only evidence of a par parent has no fork table",
                    ));
                }
            }
        }
        node = unsafe { heap.scanned(node, 1) }?;
    }
    // 作る物体だけに書き、`Ctx` の持つ語には触れない。表と末尾は凍結した塊の物体なので、参照を数えない
    let heap = unsafe { ctx.heap_mut() };
    let mut next = forks.tail;
    const EV: usize = data_first(3, false);
    const NODE: usize = data_first(2, false);
    for &(effect, table, depth) in mirrored.iter().rev() {
        let ev = heap.alloc_data(effect + 1, 3, 0, false);
        let node = heap.alloc_data(effect + 1, 2, 0, false);
        unsafe {
            heap.set_field(ev, EV, table)?;
            heap.set_field(ev, EV + 1, Word::imm(i64::from(depth)))?;
            heap.set_field(node, NODE, ev)?;
            heap.set_field(node, NODE + 1, next)?;
        }
        next = node;
    }
    Ok(next)
}

/// 子の本体が値で戻ったか (`ChildDone`)、中断した (`child_abort`)。`task.base` は子の境目のフレームである。
#[inline(never)]
fn child_done<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    outcome: Outcome,
) -> Result<(), RunFault> {
    // 子は親の handler に yield できないので、yield の途中で終わる子はない
    if ctx.yielding() != 0 {
        return Err(RunFault::Internal(
            "a par child ends in the middle of a yield",
        ));
    }
    let j = task.get(1).bits();
    let open = task.boundaries.pop();
    debug_assert_eq!(open, Some(j), "the innermost boundary is the one that ends");
    // 仕事の境目は番兵の join なので、表を引かずに仕事のセルを終える
    if task.boundaries.is_empty()
        && let Some(job) = task.job
    {
        return job_done::<R>(code, ctx, task, sched, job, outcome);
    }
    // 置いた右の子の join は親の worker にあるので、この worker の表を引かない
    if task.boundaries.is_empty()
        && let Some(placed) = task.placed
    {
        return placed_done::<R>(code, ctx, task, sched, placed, j, outcome);
    }
    if sched.current == sched.join(j).right {
        let join = sched.join(j);
        join.right_outcome = Some(outcome);
        let right_worker = join.right_worker;
        // 子の残りの `evv` を、子の ID のまま手放す
        let rest =
            unsafe { switch_to(ctx, Word::UNIT, right_worker, task.depth_offset, task.limit) };
        unsafe { ctx.heap_mut().decref(rest) }?;
        sched.wake_parent(j);
        return switch_away::<R>(code, ctx, task, sched, None);
    }
    let join = sched.join(j);
    join.left = Some(outcome);
    let parent_evv = std::mem::replace(&mut join.parent_evv, Word::UNIT);
    let right_done = join.right_outcome.is_some();
    // 境目のフレームを外す。組か起こし直しの結果は join が `par` の結果のスロットに書く
    task.pop_frame();
    let rest = unsafe { switch_to(ctx, parent_evv, task.worker, task.depth_offset, task.limit) };
    unsafe { ctx.heap_mut().decref(rest) }?;
    if right_done {
        let at = complete_join::<R>(code, ctx, task, sched, j)?;
        let result = safepoint::<R>(code, ctx, task, sched);
        at_par(code, task, sched, at, result)
    } else {
        // 待つ親を切り替えの前の集める地点で候補にしない。そのまま動き続けるときは `keep_running` が公開し直す
        // (docs/spec/vm.md の「タスクとスケジュール」)
        sched.unpublish();
        switch_away::<R>(code, ctx, task, sched, Some(TaskState::Waiting(j)))
    }
}

/// 置いた右の子が終わった。残りの参照を子の ID のまま手放し、ヒープの worker を戻してから、結果を親の worker へ
/// 送る。親は知らせを受けた後に join で子の ID を退かせるので、手放すのを公開より先にして、子の ID の物体を残さない
/// (docs/spec/vm.md の「子の終わりと join」)。
fn placed_done<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    placed: Placed,
    j: JoinId,
    outcome: Outcome,
) -> Result<(), RunFault> {
    let rest = unsafe { switch_to(ctx, Word::UNIT, task.worker, task.depth_offset, task.limit) };
    unsafe { ctx.heap_mut().decref(rest) }?;
    // `()` を入れ替えるだけで、返る語も `()` である
    let _unit = unsafe { switch_to(ctx, Word::UNIT, sched.main_worker, 0, Limit::Stops) };
    let control = std::sync::Arc::clone(&sched.watcher.control);
    sched
        .run
        .send(&control, placed.home, Notice::Joined { join: j, outcome });
    switch_away::<R>(code, ctx, task, sched, None)
}

/// `compute` の仕事が終わった。残りの `evv` を仕事の ID のまま手放し、ヒープの worker を戻してから、仕事のセルに
/// 終わりを置いて出したタスクの起こしを route する。route を切り替えより先に行うのは、起こしが届く前に worker 0 が
/// デッドロックを決めないためである (docs/spec/vm.md の「`compute` の仕事」)。
fn job_done<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    job: JobRef,
    outcome: Outcome,
) -> Result<(), RunFault> {
    let end = match outcome {
        Outcome::Value(w) => cells::JobEnd::Value(w),
        Outcome::Abort {
            effect,
            depth,
            clause,
        } => cells::JobEnd::Abort {
            effect,
            depth,
            clause,
        },
        Outcome::Error(_) => return Err(RunFault::Internal("a job ends with an error")),
    };
    let rest = unsafe { switch_to(ctx, Word::UNIT, task.worker, task.depth_offset, task.limit) };
    unsafe { ctx.heap_mut().decref(rest) }?;
    // `()` を入れ替えるだけで、返る語も `()` である
    let _unit = unsafe { switch_to(ctx, Word::UNIT, sched.main_worker, 0, Limit::Stops) };
    let mut out = Vec::new();
    {
        here!(sched, post, here);
        cells::finish_job(&sched.cells, &here, job.cell, end, &mut out)?;
    }
    for run in out {
        schedule(sched, run)?;
    }
    switch_away::<R>(code, ctx, task, sched, None)
}

/// 方針が選ぶ次のタスクに切り替える。`keep` が `None` なら、今のタスクは終わったので捨てる。次のタスクが join を
/// 完了したら、`par` の後の安全点に進む。
fn switch_away<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    keep: Option<TaskState>,
) -> Result<(), RunFault> {
    debug_assert!(
        sched.hold.is_none(),
        "a channel reservation is held at a switch"
    );
    // 落ちたハンドルの知らせは切り替えのたびに処理し、終わったタスクのセルと ID を run の終わりまで残さない
    // (docs/spec/vm.md の「`Async` のタスク」)。走れるタスクがなければ直後の `wait` が OS を見るので、ここでは
    // 見ない。走れるタスクが続くときは `POLL_EVERY` 回に1回だけ見る (docs/spec/vm.md の「時計と driver」)
    collect::<R>(ctx, task, sched, false)?;
    // 切り替えるたびに、ほかのスレッドから受けた解放が溜まっていれば取り込む。溜まっていなければ2回の読みで済む
    // (docs/spec/vm.md の「タスクとスケジュール」)
    ctx.reclaim_if_due();
    if !sched.run.ready_is_empty(sched.index) {
        sched.since_poll += 1;
        if sched.since_poll >= POLL_EVERY {
            collect::<R>(ctx, task, sched, true)?;
        }
    }
    // 走れるタスクがなければ、起こされるか次の期限かdriverの完了まで待って集め直す。戻るたびに止める語を読む。
    // デッドロックは空きの待ちが決める (docs/spec/vm.mdの「時計と driver」)
    let next = loop {
        // 置いた右の子の終わりは知らせで届く。待とうとする親はまだ表にいないので、集めた知らせが親の join に
        // 終わりを届けても起こされない。そのときは待たずにここで join を完了する (docs/spec/vm.md の「子の終わりと join」)
        if let Some(TaskState::Waiting(j)) = keep
            && sched.join(j).right_outcome.is_some()
        {
            sched.keep_running(task);
            let at = complete_join::<R>(code, ctx, task, sched, j)?;
            let result = safepoint::<R>(code, ctx, task, sched);
            return at_par(code, task, sched, at, result);
        }
        if let Some(item) = sched.run.pick_ready(sched.index, &mut sched.policy) {
            break ready_task(code, sched, item)?;
        }
        // 眠る前に、ほかのスレッドから受けた解放が溜まっていれば取り込む (docs/spec/vm.md の「時計と driver」)
        ctx.reclaim_if_due();
        sched.publish_idle();
        debug_assert!(
            sched.added.is_empty(),
            "an entry is left unsettled before a worker sleeps"
        );
        // メインのworkerはここで待たず、動くタスクのない`Task`に戻って`drive`から抜ける。待ちは`drive`の外で行う
        // ので、待つ間にVMの状態をRustのスタックに残さない (docs/spec/vm.mdの「メインのworker」)
        if sched.main_loop {
            if sched.current != ENTRY {
                resume::<R>(code, ctx, task, sched, ENTRY, keep)?;
            }
            return Err(HOST_IDLE);
        }
        sched.idle_wait()?;
        // `NOTICE`は捨てる。その知らせは続く集める地点が集める。直前の`wait`がOSを見たので、
        // 集める地点は見ない。ここは関数のエントリでないので、タスクの中の上限は保留のまま次の安全点に任せる
        // (docs/spec/vm.mdの「タスクの上限」)
        if matches!(
            poll(ctx, task.stack_depth(), sched.index)?,
            Polled::Limit { .. }
        ) && matches!(task.limit, Limit::Stops | Limit::Aborting { .. })
        {
            return Err(RunFault::Stopped(Stop::DepthLimit));
        }
        sched.since_poll = 0;
        collect::<R>(ctx, task, sched, false)?;
    };
    match resume::<R>(code, ctx, task, sched, next, keep)? {
        Some(at) => {
            let result = safepoint::<R>(code, ctx, task, sched);
            at_par(code, task, sched, at, result)
        }
        None => Ok(()),
    }
}

/// タスク`next`に切り替える。`evv`とworkerは`Ctx::switch_task`の1回で替え、RCに触れない
/// (docs/spec/object-model.mdの「タスクの切り替え」)。まだ始まっていない右の子と仕事は、そのIDに替えた後で閉包を呼ぶ。
/// 閉包を手放すと、子のIDの物体を解放しうるためである。待っていた親はjoinを完了し、そのときは`par`の命令の
/// 位置を返す。`par`の後の安全点は呼ぶ側が行い、その誤りにもこの位置を付ける (`at_par`)。ここで安全点に進むと、切り替えが`preempt`と`resume`をRustの再帰で重ねる
/// ためである (docs/spec/vm.mdの「並列化への備え」)。
fn resume<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    next: TaskId,
    keep: Option<TaskState>,
) -> Result<Option<Pc>, RunFault> {
    let mut incoming = sched.tasks[next].take().ok_or(RunFault::Internal(
        "a task to resume is not in the scheduler",
    ))?;
    let evv = std::mem::replace(&mut incoming.evv, Word::UNIT);
    sched.end_slice(task);
    let saved = unsafe {
        switch_to(
            ctx,
            evv,
            incoming.worker,
            incoming.depth_offset,
            incoming.limit,
        )
    };
    let mut prev = std::mem::replace(task, incoming);
    let prev_id = sched.current;
    sched.set_current(next);
    // 入るタスクを公開し、実行待ちを離れたので `top` から消す。まだ動いていないエントリの `Task` は `enter` が公開する
    // (docs/spec/vm.md の「タスクとスケジュール」)
    sched.yielded = false;
    sched.publish(task);
    if let Some(cell) = task.cell
        && !matches!(task.state, TaskState::Entry { .. })
    {
        sched.clear_top(cell);
    }
    sched.note_depth(&prev);
    match keep {
        Some(state) => {
            // フレームを持ったまま横取りされたタスクは実行待ちに入る
            if let (Some(cell), TaskState::Ready) = (prev.cell, &state) {
                sched.offer_top(cell, prev.counted);
            }
            prev.evv = saved;
            prev.state = state;
            sched.tasks[prev_id] = Some(prev);
        }
        None => {
            // 終わったエントリの `Task` のスタックと scratch は、次のエントリの `Task` が使い回す
            if prev.cell.is_some() {
                sched.stacks.push((
                    std::mem::take(&mut prev.stack),
                    std::mem::take(&mut prev.scratch),
                ));
            }
            sched.free_task(prev_id)
        }
    }
    match std::mem::replace(&mut task.state, TaskState::Running) {
        TaskState::Unstarted(g) => {
            // 始めたばかりの子はまだ命令を実行していないので、誤りには fork した `par` の命令の位置を付ける。
            // 横取りされたタスクの位置を付けると、文言がスケジュールで変わる
            // 仕事の境目は番兵の join で表にないので、`job` を先に見る
            let at = match (task.job, task.placed) {
                (Some(job), _) => job.at,
                (None, Some(placed)) => placed.at,
                (None, None) => sched.join(task.boundaries[0]).ret - 1,
            };
            let result = apply_to_unit::<R>(code, ctx, task, g, CHILD_DONE, 0);
            at_par(code, task, sched, at, result).map(|()| None)
        }
        TaskState::Waiting(j) => complete_join::<R>(code, ctx, task, sched, j).map(Some),
        TaskState::Entry {
            function,
            args,
            argc,
        } => {
            // エントリの `Task` はまだ命令を実行していないので、誤りにはエントリの関数の先頭の位置を付ける。`run` が持つ
            // 位置は、切り替える前のタスクのものである
            let at = code.functions[function as usize].start;
            let result = enter::<R>(code, ctx, task, sched, function, args, argc);
            at_par(code, task, sched, at, result).map(|()| None)
        }
        // `Async.run` の待ちは、書いた `Async` の命令の次から続ける。起こすのは根のスコープが閉じたときだけである
        TaskState::WaitingCell(root) => {
            debug_assert!(
                sched.cells.lock().closed(root).unwrap_or(false),
                "a run wakes before its scope closes"
            );
            Ok(None)
        }
        TaskState::Ready | TaskState::Running => Ok(None),
    }
}

/// 集める地点。登録表とほかの worker の知らせを `Scheduler::gather` が集め、待ちを外したタスクを再開する仕事を列の
/// 後ろに積み、受けた上限の旗を VM の `Task` に付ける。`task` は今動いているタスクである。聞く者のいなかった
/// シグナルの誤りも、仕事を積んで旗を付けてから返す (docs/spec/vm.md の「タスクとスケジュール」)。
fn collect<R: Exec>(
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    look: bool,
) -> Result<(), RunFault> {
    let gathered = sched.gather(ctx, task.depth_offset, look);
    if !sched.limited.is_empty() {
        mark_limited(ctx, task, sched);
    }
    if !sched.runnables.is_empty() {
        let mut out = std::mem::take(&mut sched.runnables);
        for run in out.drain(..) {
            schedule(sched, run)?;
        }
        sched.runnables = out;
    }
    gathered
}

/// 方針が安全点で切り替えを求めた。走れるタスクがなければ、切り替えずに予算を詰め直す。切り替えた先がjoinを
/// 完了したら、`par`の後の安全点をここで繰り返す。Rustの再帰を使わないためである
/// (docs/spec/vm.mdの「並列化への備え」)。
#[cold]
#[inline(never)]
fn preempt<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
) -> Result<(), RunFault> {
    debug_assert!(
        sched.hold.is_none(),
        "a channel reservation is held at a switch"
    );
    // 方針が強いる切り替えの頭でもOSを1回見る (docs/spec/vm.mdの「時計と driver」)
    collect::<R>(ctx, task, sched, true)?;
    loop {
        let next = sched.run.pick_ready(sched.index, &mut sched.policy);
        sched.policy.reload();
        let Some(next) = next else {
            return Ok(());
        };
        let next = ready_task(code, sched, next)?;
        let current = sched.current;
        sched
            .run
            .wake_ready(sched.index, &mut sched.policy, current);
        sched.count_preemption();
        let Some(at) = resume::<R>(code, ctx, task, sched, next, Some(TaskState::Ready))? else {
            return Ok(());
        };
        let polled = match poll(ctx, task.stack_depth(), sched.index) {
            Ok(Polled::Collect) => collect::<R>(ctx, task, sched, true),
            Ok(Polled::Limit { collect }) => {
                match limit_safepoint::<R>(code, ctx, task, sched, collect) {
                    Ok(true) => continue,
                    Ok(false) => return Ok(()),
                    Err(fault) => return at_par(code, task, sched, at, Err(fault)),
                }
            }
            Ok(Polled::Continue) => Ok(()),
            Err(fault) => Err(fault),
        };
        if let Err(fault) = polled {
            return at_par(code, task, sched, at, Err(fault));
        }
        let tick = sched.policy.tick();
        if !(tick && ctx.yielding() == 0) {
            return Ok(());
        }
    }
}

/// 両方の子が終わった join を、親の文脈と ID で完了する。誤りには `par` の命令の位置を付けるので、どのスケジュールで
/// 親が join に着いても同じ文言になる。`par` の後の安全点は呼ぶ側が行うので、`par` の命令の位置を返す。
fn complete_join<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    j: JoinId,
) -> Result<Pc, RunFault> {
    let join = sched.take_join(j);
    let at = join.ret - 1;
    let result = join_outcomes::<R>(code, ctx, task, sched, join);
    // join の後にも、ほかのスレッドから受けた解放が溜まっていれば取り込む (docs/spec/vm.md の「子の終わりと join」)
    ctx.reclaim_if_due();
    at_par(code, task, sched, at, result).map(|()| at)
}

/// join の完了、その後の安全点、右の子の始まりで起きた誤りに、`par` の命令 `at` の関数と位置を付ける。どれも
/// 切り替えの後で起きるので、`run` が持つ命令の位置は別のタスクのものでありうる。`par` の位置を付ければ、どの
/// スケジュールでも同じ文言になる (docs/spec/vm.md の「子の終わりと join」)。`Async` のエントリの `Task` の始まりの誤りも、
/// 同じ理由でエントリの関数の先頭の位置 `at` を付ける。すでに記録した誤りはそのまま渡す。
fn at_par<M: Mode, P: Policy>(
    code: &CodeBase<M>,
    task: &Task,
    sched: &mut Scheduler<P>,
    at: Pc,
    result: Result<(), RunFault>,
) -> Result<(), RunFault> {
    match result {
        Err(fault) if sched.failed.is_none() => {
            sched.failed = Some(code.error(Failure { fault, pc: at }, task));
            Err(CHILD_FAILED)
        }
        result => result,
    }
}

/// join の解決の順 (docs/spec/vm.md の「`par` の fork と join」)。誤りは左、右の順に報告し、中断でも値でも救わない。
/// 右の側の語はすべて親の局所に戻してから右の子の ID を退かせ、その後で捨てる語を手放す。ヒープの誤りはそこで実行を
/// 止めるので、退かせも手放しもしない。
fn join_outcomes<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    join: JoinNode,
) -> Result<(), RunFault> {
    let (Some(left), Some(right)) = (join.left, join.right_outcome) else {
        return Err(RunFault::Internal(
            "a join completes before both children end",
        ));
    };
    let (left, right) = match (left, right) {
        (Outcome::Error(error), _) | (_, Outcome::Error(error)) => {
            sched.failed = Some(error);
            return Err(CHILD_FAILED);
        }
        outcomes => outcomes,
    };
    // 右の側の語は右の子が残した参照で、親だけが持つ
    let heap = unsafe { ctx.heap_mut() };
    if let Some(w) = right.word() {
        unsafe { heap.transfer_back(w, join.right_worker) }?;
    }
    heap.retire_worker(join.right_worker)?;
    let (abort, dropped) = match (left, right) {
        (Outcome::Value(l), Outcome::Value(r)) => {
            let Op::Par { at, .. } = code.tables.code[join.ret as usize - 1] else {
                return Err(RunFault::Internal(
                    "a join returns after something other than a par",
                ));
            };
            let tag = code.tables.operands[at as usize + 2] as u16;
            const PAIR: usize = data_first(2, false);
            let pair = heap.alloc_data(tag, 2, 0, false);
            // 組は今作った局所の物体で、2つの語の参照は組に移る
            unsafe {
                heap.set_field(pair, PAIR, l)?;
                heap.set_field(pair, PAIR + 1, r)?;
            }
            task.set(join.dst, pair);
            return Ok(());
        }
        (Outcome::Error(_), _) | (_, Outcome::Error(_)) => {
            return Err(RunFault::Internal(
                "a join raises an error after reporting it",
            ));
        }
        // いちばん左の中断を起こし直し、中断しなかった側の値は捨てる
        (abort @ Outcome::Abort { .. }, other) | (other @ Outcome::Value(_), abort) => {
            (abort, other)
        }
    };
    if let Some(w) = dropped.word() {
        unsafe { heap.decref(w) }?;
    }
    let Outcome::Abort {
        effect,
        depth,
        clause,
    } = abort
    else {
        return Err(RunFault::Internal(
            "a join raises again something other than an abort",
        ));
    };
    reraise::<R>(code, ctx, task, (effect, depth, clause), join.ret, join.dst)
}

/// 中断した子の handler の節を、親の `evv` を mask してから起こし直す。
fn reraise<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    abort: (u16, u32, Word),
    ret: Pc,
    dst: Slot,
) -> Result<(), RunFault> {
    let (effect, depth, clause) = abort;
    // 子が見た `depth` 番目の evidence に届くように、親の `evv` を mask する。`reperform$` は今の `evv` の最初の
    // evidence で perform して戻らないので、mask した `evv` は handler の `prompt$` が手放す
    // (docs/spec/core-ir.md の「`par` の子の中断」)
    if depth > 0 {
        unsafe {
            let w = ctx.evv_get()?;
            let item = MaskItem {
                effect,
                keep: 0,
                skip: depth,
            };
            let m = ctx.evv_mask(&[item], w)?;
            ctx.heap_mut().decref(w)?;
            ctx.evv_set(m)?;
        }
    }
    apply_to_unit::<R>(code, ctx, task, clause, ret, dst)
}

/// `prim child_abort(ev, c)`。fork evidence `ev` からエフェクトと深さを読み、子の境目まで巻き戻す。`ev` は借りる。
#[inline(never)]
fn child_abort<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    counts: &mut RunCounts,
    ev: Slot,
    c: Slot,
) -> Result<(), RunFault> {
    if task.boundaries.is_empty() {
        return Err(RunFault::Internal("a child abort outside a par child"));
    }
    let ev = task.get(ev);
    // `ev` は節のエントリが持つ fork evidence で、状態の欄は深さの即値である
    let heap = ctx.heap();
    let effect = unsafe { heap.meta(ev) }?
        .tag()
        .checked_sub(1)
        .ok_or(RunFault::Internal(
            "a child abort gets something other than evidence",
        ))?;
    let depth = unsafe { heap.scanned(ev, 1) }?;
    let depth = u32::try_from(depth.imm_value())
        .ok()
        .filter(|_| depth.is_imm())
        .ok_or(RunFault::Internal(
            "a child abort gets evidence without a depth",
        ))?;
    let clause = task.get(c);
    let dummy = ctx.heap().dummy();
    unwind::<R>(code, ctx, task, counts, dummy)?;
    let outcome = Outcome::Abort {
        effect,
        depth,
        clause,
    };
    child_done::<R>(code, ctx, task, sched, outcome)
}

/// 実行時エラーの後。すぐに実行を止める誤りと、エントリのタスクの誤り (左の子の誤りを含む) と、`compute` の仕事の誤りは
/// 実行の誤りにする。右の子の
/// 誤りは join に記録し、右のタスクはスタックを巻き戻さずに捨てて、ほかのタスクに切り替える。止めたスタックは
/// 巻き戻しの表のない位置で止まっていて解放できない。記録した誤りは join が必ず報告するので、実行は誤りで終わる
/// (docs/spec/vm.md の「`par` の fork と join」)。
#[cold]
#[inline(never)]
fn task_failed<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    mut failure: Failure,
) -> Result<(), RuntimeError> {
    loop {
        if failure.fault == HOST_IDLE {
            return Err(RuntimeError::Fault {
                fault: HOST_IDLE,
                function: String::new(),
                at: None,
            });
        }
        let error = match sched.failed.take() {
            Some(error) => error,
            None => code.error(failure, task),
        };
        // `Async` のタスクの失敗は底の `try_all` が捕まえるので、ここに来るのは捕まえられない誤りで、run を止める
        // (docs/spec/vm.md の「`Async` のタスク」)。仕事のセルは誤りを持たないので、仕事の誤りもその場で run を止める
        // (docs/spec/vm.md の「`compute` の仕事」)
        if sched.current == ENTRY
            || task.cell.is_some()
            || task.job.is_some()
            || stops_the_run(&error)
        {
            return Err(error);
        }
        let boundaries = std::mem::take(&mut task.boundaries);
        // 右の子が開いていた入れ子の `par` の子は、結果を受ける親がもういないので、走らせずに捨てる
        for &inner in &boundaries[1..] {
            cancel(sched, inner);
        }
        let j = boundaries[0];
        match task.placed {
            // 置いた右の子の join は親の worker にあるので、誤りを知らせで送る
            Some(placed) => {
                let control = std::sync::Arc::clone(&sched.watcher.control);
                let joined = Notice::Joined {
                    join: j,
                    outcome: Outcome::Error(error),
                };
                sched.run.send(&control, placed.home, joined);
            }
            None => {
                sched.join(j).right_outcome = Some(Outcome::Error(error));
                sched.wake_parent(j);
            }
        }
        match switch_away::<R>(code, ctx, task, sched, None) {
            Ok(()) => return Ok(()),
            Err(fault) => failure = Failure { fault, pc: task.pc },
        }
    }
}

/// 子の誤りのうち、評価だけでなく実行そのものを止めるもの (docs/spec/vm.md の「`par` の fork と join」)。
fn stops_the_run(error: &RuntimeError) -> bool {
    matches!(
        error,
        RuntimeError::Fault {
            fault: RunFault::Stopped(
                Stop::Interrupted | Stop::Terminated | Stop::MemoryLimit | Stop::Ended
            ) | RunFault::Extern(eml_rt::Fault::Heap(_))
                | RunFault::Internal(_),
            ..
        } | RuntimeError::Leak(_)
            | RuntimeError::Deadlock
            | RuntimeError::Unrecovered { .. }
    )
}

/// 親が誤りで止まった join の右の子を、その子孫ごと表と列から外す。持つ語は触れずに捨てる。子孫は作業の列でたどり、
/// Rust の再帰を使わない (docs/spec/vm.md の「並列化への備え」)。
fn cancel<P: Policy>(sched: &mut Scheduler<P>, j: JoinId) {
    let mut pending = vec![j];
    while let Some(j) = pending.pop() {
        let node = sched.join(j);
        // 置いた右の子はほかの worker で走り続け、終わりの知らせが必ず届く。届くまで番号を取っておく
        if node.right == PLACED && node.right_outcome.is_none() {
            node.orphan = true;
            continue;
        }
        let join = sched.take_join(j);
        if join.right_outcome.is_some() {
            continue;
        }
        let right = join.right;
        sched.run.retain_ready(
            sched.index,
            |item| !matches!(item, Ready::Task(id) if *id == right),
        );
        if let Some(task) = sched.tasks[right].take() {
            sched.free_task(right);
            pending.extend_from_slice(&task.boundaries[1..]);
            if let TaskState::Waiting(waited) = task.state {
                pending.push(waited);
            }
        }
    }
}

impl Outcome {
    /// 子が残した語。値か、起こし直しの閉包である。
    fn word(&self) -> Option<Word> {
        match *self {
            Outcome::Value(w) | Outcome::Abort { clause: w, .. } => Some(w),
            Outcome::Error(_) => None,
        }
    }
}

/// エントリの関数の結果か、`exit` の中断を手放す (docs/spec/vm.md の「誤り、止めること、終わり」)。その前に、自分の
/// 受け箱の知らせと mail の項を処理し、登録表の lock の下で、終わっていないタスクも driver に出したままの操作も
/// ないことを確かめる。driver の静止と残ったセルの片付けは、worker を join した後に `run_scheduled` が行う。ほかの
/// worker がまだ host へ受け付けを送りうるからである (docs/spec/vm.md の「`Async` のタスク」の「終わり」)。
// run ごとに1回しか通らないので、VM の繰り返しに展開しない。展開すると、driver の有無を確かめる分岐で繰り返しの
// レジスタの割り当てが変わり、`bench/loop.em` の命令の数が 4% 増えた
#[inline(never)]
fn finish<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &Task,
    sched: &mut Scheduler<R::P>,
) -> Result<(), RunFault> {
    let mut out = Vec::new();
    sched.take_slot(ctx, task.depth_offset, &mut out)?;
    {
        here!(sched, post, here);
        // 終わりでは動くタスクがないので、旗を付ける `Task` もない
        cells::drain(
            &sched.cells,
            &here,
            ctx,
            sched.driver.as_deref_mut(),
            task.depth_offset,
            &mut out,
            &mut Vec::new(),
        )?;
    }
    if !out.is_empty() || sched.cells.lock().left_at_end() {
        return Err(RunFault::Internal("a task is left at the end"));
    }
    // 予約と確定の間に安全点はないので、`Finish` に来た worker は予約を持たない (docs/spec/vm.md の「channel」)
    debug_assert!(
        sched.hold.is_none(),
        "a channel reservation is held at the end"
    );
    if sched.has_children() {
        return Err(RunFault::Internal("a task is left at the end"));
    }
    if task.exit.is_some() && !task.boundaries.is_empty() {
        return Err(RunFault::Internal("an exit ends a run with an open par"));
    }
    if let Some(n) = task.exit {
        // 途中のフレームは巻き戻しで片付けてある。エントリの結果はダミーなので手放さない。中断の途中の `evv` には
        // 意味がないので、空に戻す (docs/spec/vm.md の「extern の中断」)
        unsafe {
            ctx.yield_finish()?;
            ctx.evv_set(Word::UNIT)?;
        }
        // 終了コードは終わりの欄だけが運ぶ。run の始めに開いた欄に先に書いた者はいないはずである
        // (docs/spec/vm.md の「`exit`」)
        if !cells::end_with_exit(ctx.heap().control(), n) {
            return Err(RunFault::Internal("an exit after the end of the run"));
        }
        return Ok(());
    }
    if code.functions[task.entry as usize].ret_rc && !task.keep_result {
        // 結果はエントリの関数が返した、VM の持つ参照である
        unsafe { ctx.heap_mut().decref(task.get(0)) }?;
    }
    Ok(())
}

// `Async` の命令は呼び出しや戻りに比べてまれなので、`par` と同じく `step` に展開しない

/// `Std.Async`、`Std.Chan`、`Std.Atomic` の行 (docs/spec/vm.md の「`Async` のタスク」)。意味の核を呼び、返った仕事をエントリの `Task` にするか、
/// 待っていた `Task` を実行待ちに戻す。安全点を置かない。
#[inline(never)]
fn async_op<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    dst: Slot,
    at: u32,
) -> Result<(), RunFault> {
    let o = &code.tables.operands[at as usize..];
    let arg = |i: usize| o[2 + i] as Slot;
    if code.entries.is_none() {
        return Err(RunFault::Internal("the Async entries are not loaded"));
    }
    // 語の引数は、LIR の所有の規則により VM が持つ参照で、行がそれを預かる
    let ext = Extern::ALL[o[0] as usize];
    match ext {
        Extern::AsyncStartRaw | Extern::AsyncStartMainRaw | Extern::AsyncStartOffMainRaw => {
            let cell = current_cell(task)?;
            // 固定したタスクが始めるタスクも固定する (docs/spec/vm.mdの「`Async`のタスク」)
            let (scope, pinned) = {
                let mut cells = sched.cells.lock();
                let pinned = cells.pinned(cell)?;
                (cells.current_of(cell)?, pinned)
            };
            // 固定したタスクの仕事をメインのworkerの外へ出せるよう、`start_off_main_raw`は固定を引き継がない (docs/spec/stdlib.mdの「メインのworker」)
            let pinned =
                (pinned || ext == Extern::AsyncStartMainRaw) && ext != Extern::AsyncStartOffMainRaw;
            let owner = place_owner(sched, pinned);
            let (handle, run) =
                start_task::<R>(ctx, task, sched, scope, owner, task.get(arg(0)), pinned)?
                    .ok_or(RunFault::Internal("a task starts in a closed scope"))?;
            task.set(dst, handle);
            // ほかの worker に置いたタスクは、`Place` を受けた持ち主が始める
            if let Some(run) = run {
                schedule(sched, run)?;
            }
        }
        Extern::AsyncRunRaw => {
            // 呼んだ `Task` はスタックを持ったまま、根のスコープが閉じるのを待つ
            let (root, pinned) = {
                let mut cells = sched.cells.lock();
                let (parent, pinned) = match task.cell {
                    Some(cell) => (Some(cells.current_of(cell)?), cells.pinned(cell)?),
                    None => (None, false),
                };
                (cells.open_root(parent, task.cell)?, pinned)
            };
            // 根のタスクは呼んだworkerに置く。固定したタスクの入れ子の根も固定する (docs/spec/vm.mdの
            // 「`Async`のタスク」)
            let me = sched.me();
            let (handle, run) =
                start_task::<R>(ctx, task, sched, root, me, task.get(arg(0)), pinned)?
                    .ok_or(RunFault::Internal("a task starts in a closed scope"))?;
            let run = run.ok_or(RunFault::Internal("a root task is placed elsewhere"))?;
            sched
                .cells
                .lock()
                .wait(root, WaitToken(sched.current as u64), me)?;
            task.set(dst, handle);
            schedule(sched, run)?;
            // 根のスコープを待つタスクも、待つ親と同じく切り替えの前に公開から外す
            sched.unpublish();
            switch_away::<R>(code, ctx, task, sched, Some(TaskState::WaitingCell(root)))?;
        }
        Extern::AsyncFinishOk => {
            // 結果はタスクの ID に預けた印を付けてから置く。`take` が同じ ID から受け取る
            let value = cells::Depot::of(task.worker).publish(ctx, task.get(arg(0)))?;
            deposit(task, cells::Outcome::Value(value))?;
            task.set(dst, Word::UNIT);
        }
        Extern::AsyncFinishIo => {
            // 文言は Rust の文字列で持つので、失敗の語は持ち主をまたがない。写すと文字列の参照も手放す
            let message = unsafe { ctx.copy_string(task.get(arg(1)), usize::MAX) }?;
            let kind = task.raw(arg(0));
            deposit(task, cells::Outcome::Io { kind, message })?;
            task.set(dst, Word::UNIT);
        }
        Extern::AsyncFinishPanic => {
            let message = unsafe { ctx.copy_string(task.get(arg(0)), usize::MAX) }?;
            deposit(task, cells::Outcome::Panic(message))?;
            task.set(dst, Word::UNIT);
        }
        ext @ (Extern::AsyncTakeResult | Extern::AsyncRunResult) => {
            let kind = if ext == Extern::AsyncRunResult {
                Take::Run
            } else {
                Take::Task
            };
            let handle = task.get(arg(0));
            match cells::take(&sched.cells, ctx, handle, kind, task.depth_offset)? {
                Ok(value) => task.set(dst, value),
                Err(raise) => return raise_again::<R>(code, ctx, task, dst, raise, ext),
            }
        }
        Extern::AsyncPark => {
            let me = current_cell(task)?;
            let k = task.get(arg(1));
            let run = sched.cells.lock().park(task.raw(arg(0)), me, k)?;
            if let Some(run) = run {
                schedule(sched, run)?;
            }
            task.set(dst, Word::UNIT);
        }
        Extern::AsyncParkTime => {
            let me = current_cell(task)?;
            let ms = task.raw(arg(0));
            // 0 以下は譲りなので時計を読まない (docs/spec/vm.md の「時間の待ち」)
            let now = if ms > 0 {
                sched.run.now_ms_ceil(sched.driver.as_deref())?
            } else {
                0
            };
            let run = sched
                .cells
                .lock()
                .park_time(now, ms, me, task.get(arg(1)))?;
            if let Some(run) = run {
                schedule(sched, run)?;
            }
            task.set(dst, Word::UNIT);
        }
        Extern::AsyncReady => {
            let me = current_cell(task)?;
            let run = sched.cells.lock().ready(me, task.get(arg(0)))?;
            schedule(sched, run)?;
            task.set(dst, Word::UNIT);
        }
        Extern::AsyncTaskCell => {
            // ハンドルは借りるだけで、組を作る後の `con` が消費する (docs/spec/core-ir.md の「組を返す extern」)
            let c = cells::cell_of(ctx, &sched.cells, task.get(arg(0)))?;
            task.set_raw(dst, c);
        }
        Extern::AsyncForkJob => {
            let me = current_cell(task)?;
            // 閉包は `fork_job` が消費する、VM の持つ参照で、仕事として動く
            let g = task.get(arg(0));
            let (child, evv) = fork_child::<R>(code, ctx, sched, g)?;
            // 子の `evv` の持ち手は仕事の `Task` だけなので、`par` と違って `dup` しない。`Ctx` の `evv` には触れない
            // (docs/spec/vm.md の「`compute` の仕事」)
            unsafe { ctx.heap_mut().share(evv, child) }?;
            let cell = sched.cells.lock().new_job(child, me);
            let at = task.pc - 1;
            let job = Task::job(
                task.entry,
                child,
                evv,
                sched.max_depth,
                JobRef { cell, at },
                g,
            );
            // `par` の右の子と同じく、空いた worker の受け箱に置き、なければこの worker の列に入れる。終わりは
            // 仕事のセルを通って出したタスクを起こすので、`in_flight` には数えない (docs/spec/vm.md の「`compute` の仕事」)
            match idle_home::<R>(ctx, sched)? {
                Some(k) => {
                    sched.run.placed_jobs.fetch_add(1, Ordering::Relaxed);
                    let control = std::sync::Arc::clone(&sched.watcher.control);
                    sched.run.send(&control, k, Notice::Child(job));
                }
                None => {
                    let id = sched.add_task(job);
                    sched.run.push_ready(sched.index, Ready::Task(id));
                }
            }
            task.set_raw(dst, cell.encode());
        }
        ext @ (Extern::AsyncTakeJob | Extern::AsyncDropJob) => {
            let me = current_cell(task)?;
            let (worker, end) = sched.cells.lock().take_job(task.raw(arg(0)), me)?;
            // 終わりの語は仕事のセルが持っていた、仕事の ID の物体である。登録表の lock を放してから今の ID へ戻し、
            // その後で仕事の ID を退かせる (docs/spec/object-model.md の「`transfer_back`」)
            let w = end.word();
            let heap = unsafe { ctx.heap_mut() };
            unsafe { heap.transfer_back(w, worker) }?;
            heap.retire_worker(worker)?;
            ctx.reclaim_if_due();
            match end {
                // 取り消されたタスクは結果を受け取らない。中断も起こし直さずに落とす
                _ if ext == Extern::AsyncDropJob => {
                    unsafe { ctx.heap_mut().decref(w) }?;
                    task.set(dst, Word::UNIT);
                }
                cells::JobEnd::Value(w) => task.set(dst, w),
                // join の起こし直しと同じく、次の命令を戻り先にして呼んだタスクのフレームで起こし直す
                // (docs/spec/vm.md の「`compute` の仕事」)
                cells::JobEnd::Abort {
                    effect,
                    depth,
                    clause,
                } => return reraise::<R>(code, ctx, task, (effect, depth, clause), task.pc, dst),
            }
        }
        ext @ (Extern::AsyncDetach | Extern::AsyncCancelRaw) => {
            let cancel = ext == Extern::AsyncCancelRaw;
            here!(sched, post, here);
            cells::detach(
                &sched.cells,
                &here,
                ctx,
                task.get(arg(0)),
                cancel,
                task.depth_offset,
            )?;
            task.set(dst, Word::UNIT);
        }
        Extern::AsyncStartInRaw => {
            let cell = current_cell(task)?;
            let g = task.get(arg(1));
            let (scope, pinned) = {
                let mut cells = sched.cells.lock();
                let pinned = cells.pinned(cell)?;
                (cells.scope_cell(task.raw(arg(0))), pinned)
            };
            let owner = place_owner(sched, pinned);
            let started = match scope {
                Some(scope) => start_task::<R>(ctx, task, sched, scope, owner, g, pinned)?,
                None => None,
            };
            let Some((handle, run)) = started else {
                // 預かった閉包は呼ばずに手放してから起こす
                unsafe { ctx.heap_mut().decref(g) }?;
                let raise = Raise::Panic("the scope is closed".to_string());
                return raise_again::<R>(code, ctx, task, dst, raise, Extern::AsyncStartInRaw);
            };
            task.set(dst, handle);
            if let Some(run) = run {
                schedule(sched, run)?;
            }
        }
        Extern::AsyncOpenScope => {
            let me = current_cell(task)?;
            let s = sched.cells.lock().open_scope(me)?;
            task.set_raw(dst, s);
        }
        Extern::AsyncCloseScope => {
            let c = sched.cells.lock().close_scope(task.raw(arg(0)))?;
            task.set_raw(dst, c);
        }
        Extern::AsyncLeaveScope => {
            let me = current_cell(task)?;
            let left = sched.cells.lock().leave_scope(task.raw(arg(0)), me)?;
            match left {
                None => task.set(dst, Word::UNIT),
                Some(raise) => {
                    return raise_again::<R>(code, ctx, task, dst, raise, Extern::AsyncLeaveScope);
                }
            }
        }
        Extern::AsyncCancelScopeRaw => {
            here!(sched, post, here);
            sched.cells.lock().cancel_scope(&here, task.raw(arg(0)))?;
            task.set(dst, Word::UNIT);
        }
        Extern::AsyncFailScopeIo => {
            let message = unsafe { ctx.copy_string(task.get(arg(2)), usize::MAX) }?;
            let kind = task.raw(arg(1));
            here!(sched, post, here);
            sched.cells.lock().fail_scope(
                &here,
                task.raw(arg(0)),
                cells::Outcome::Io { kind, message },
            )?;
            task.set(dst, Word::UNIT);
        }
        Extern::AsyncFailScopePanic => {
            let message = unsafe { ctx.copy_string(task.get(arg(1)), usize::MAX) }?;
            here!(sched, post, here);
            sched.cells.lock().fail_scope(
                &here,
                task.raw(arg(0)),
                cells::Outcome::Panic(message),
            )?;
            task.set(dst, Word::UNIT);
        }
        Extern::AsyncCancelPoint => {
            let b = sched.cells.lock().cancel_point(task.cell)?;
            task.set_raw(dst, i64::from(b));
        }
        Extern::AsyncSpendBudget => {
            let me = current_cell(task)?;
            let b = sched.cells.lock().spend_budget(me, 1)?;
            task.set_raw(dst, i64::from(b));
        }
        Extern::AsyncClaimScope => {
            let me = current_cell(task)?;
            here!(sched, post, here);
            let won = sched
                .cells
                .lock()
                .claim_scope(&here, task.raw(arg(0)), me)?;
            task.set_raw(dst, i64::from(won));
        }
        Extern::AsyncParkDrain => {
            let me = current_cell(task)?;
            let k = task.get(arg(1));
            let run = sched.cells.lock().park_drain(task.raw(arg(0)), me, k)?;
            if let Some(run) = run {
                schedule(sched, run)?;
            }
            task.set(dst, Word::UNIT);
        }
        Extern::AsyncCancelMembers => {
            here!(sched, post, here);
            sched.cells.lock().cancel_members(&here, task.raw(arg(0)))?;
            task.set(dst, Word::UNIT);
        }
        Extern::AsyncShieldEnter => {
            let me = current_cell(task)?;
            sched.cells.lock().shield_enter(me)?;
            task.set(dst, Word::UNIT);
        }
        Extern::AsyncShieldLeave => {
            let me = current_cell(task)?;
            sched.cells.lock().shield_leave(me)?;
            task.set(dst, Word::UNIT);
        }
        ext @ (Extern::AsyncParkSend | Extern::AsyncParkRecv) => {
            let me = current_cell(task)?;
            let (c, k) = (task.raw(arg(0)), task.get(arg(1)));
            let mut runs = Vec::new();
            {
                here!(sched, post, here);
                let mut cells = sched.cells.lock();
                let run = if ext == Extern::AsyncParkSend {
                    cells.park_send(c, me, k, &mut sched.woken)?
                } else {
                    cells.park_recv(c, me, k, &mut sched.woken)?
                };
                runs.extend(run);
                cells.route(&mut sched.woken, &here, &mut runs)?;
            }
            for run in runs {
                schedule(sched, run)?;
            }
            task.set(dst, Word::UNIT);
        }
        // channel の行は、待ちのほかは今のタスクのセルを使わないので、タスクの外でも動く (docs/spec/vm.md の「命令」)。
        // どの行も登録表の lock を1区間だけ取り、預かり先の操作 (`open`、`publish`、`receive`、`close`) は lock の
        // 外で行う。予約から確定までに要るものは worker の `hold` が持つ (docs/spec/vm.md の「channel」)
        Extern::ChanNewRaw => {
            let depot = cells::Depot::open(ctx);
            let c = sched.cells.lock().new_chan(task.raw(arg(0)), depot);
            task.set_raw(dst, c);
        }
        Extern::ChanReserveSend => {
            let (c, yields) = (task.raw(arg(0)), task.raw(arg(1)) != 0);
            let r = sched
                .cells
                .lock()
                .reserve_send(c, task.cell, yields, &mut sched.hold)?;
            task.set_raw(dst, r);
        }
        Extern::ChanReserveRecv => {
            let (c, yields) = (task.raw(arg(0)), task.raw(arg(1)) != 0);
            let mut runs = Vec::new();
            let r = {
                here!(sched, post, here);
                let mut cells = sched.cells.lock();
                let r =
                    cells.reserve_recv(c, task.cell, yields, &mut sched.hold, &mut sched.woken)?;
                cells.route(&mut sched.woken, &here, &mut runs)?;
                r
            };
            for run in runs {
                schedule(sched, run)?;
            }
            task.set_raw(dst, r);
        }
        Extern::ChanPushRaw => {
            let (chan, mut depot) = cells::Hold::take_send(&mut sched.hold, task.raw(arg(1)))?;
            let x = depot.publish(ctx, task.get(arg(0)))?;
            let mut runs = Vec::new();
            {
                here!(sched, post, here);
                let mut cells = sched.cells.lock();
                cells.push_raw(chan, x, &mut sched.woken)?;
                cells.route(&mut sched.woken, &here, &mut runs)?;
            }
            for run in runs {
                schedule(sched, run)?;
            }
            task.set(dst, Word::UNIT);
        }
        Extern::ChanPopRaw => {
            let (chan, mut depot, item) =
                cells::Hold::take_recv(&mut sched.hold, task.raw(arg(0)))?;
            // 品物は予約で列から取ったので、ほかに参照がない
            let x = depot.receive(ctx, item)?;
            let gone = sched.cells.lock().pop_raw(chan)?;
            if let Some(gone) = gone {
                gone.close(ctx)?;
            }
            task.set(dst, x);
        }
        Extern::ChanCloseRaw => {
            let mut runs = Vec::new();
            let gone = {
                here!(sched, post, here);
                let mut cells = sched.cells.lock();
                let gone = cells.close_raw(task.raw(arg(0)), &mut sched.woken)?;
                cells.route(&mut sched.woken, &here, &mut runs)?;
                gone
            };
            if let Some(gone) = gone {
                gone.close(ctx)?;
            }
            for run in runs {
                schedule(sched, run)?;
            }
            task.set(dst, Word::UNIT);
        }
        Extern::AtomicNew => {
            let i = sched.run.counters.push(task.raw(arg(0)))?;
            task.set_raw(dst, i);
        }
        Extern::AtomicAdd => {
            let v = sched.run.counters.add(task.raw(arg(1)), task.raw(arg(0)))?;
            task.set_raw(dst, v);
        }
        Extern::AtomicLoad => {
            let v = sched.run.counters.load(task.raw(arg(0)))?;
            task.set_raw(dst, v);
        }
        Extern::AtomicStore => {
            sched
                .run
                .counters
                .store(task.raw(arg(1)), task.raw(arg(0)))?;
            task.set(dst, Word::UNIT);
        }
        Extern::AsyncOpReady => {
            // token は借りるだけで、組を作る後の `con` が返す
            let me = current_cell(task)?;
            let token = task.raw(arg(0));
            // host が置いた結果は、決める区間と同じ区間で先に当てる (docs/spec/vm.md の「操作の記録」)
            let mut work = cells::IoWork::default();
            let mut step = {
                here!(sched, post, here);
                let mut cells = sched.cells.lock();
                cells.settle_token(ctx, task.depth_offset, token, me, &here, &mut work)?;
                cells.op_ready(token, task.raw(arg(1)), me)?
            };
            work.cancel_on(sched.driver.as_deref_mut())?;
            // 予算を使い切った終わった操作の待ちは、OS を1回見て集める。受け付けで終わる書きを続ける書き手が、OS を
            // 見ないと起きない同じ worker の読み手の先へ走り続けないためである。走れるタスクがなければ譲らずに続ける
            // (docs/spec/stdlib.md の「操作の予算」、docs/spec/vm.md の「時計と driver」)
            if step == cells::FAST_YIELD {
                collect::<R>(ctx, task, sched, true)?;
                if sched.run.ready_is_empty(sched.index) {
                    step = sched.cells.lock().go_on(me)?;
                }
            }
            task.set_raw(dst, step);
        }
        Extern::AsyncParkIo => {
            let me = current_cell(task)?;
            let (token, k) = (task.raw(arg(0)), task.get(arg(1)));
            let run = {
                here!(sched, post, here);
                cells::park_io(
                    &sched.cells,
                    &here,
                    ctx,
                    sched.driver.as_deref_mut(),
                    task.depth_offset,
                    token,
                    me,
                    k,
                )?
            };
            if let Some(run) = run {
                schedule(sched, run)?;
            }
            task.set(dst, Word::UNIT);
        }
        Extern::AsyncParkIoUntil => {
            let me = current_cell(task)?;
            let (token, ms, k) = (task.raw(arg(0)), task.raw(arg(1)), task.get(arg(2)));
            // 0 以下は過ぎた期限なので時計を読まない (docs/spec/vm.md の「時間の待ち」)
            let now = if ms > 0 {
                sched.run.now_ms_ceil(sched.driver.as_deref())?
            } else {
                0
            };
            let run = {
                here!(sched, post, here);
                cells::park_io_until(
                    &sched.cells,
                    &here,
                    ctx,
                    sched.driver.as_deref_mut(),
                    task.depth_offset,
                    now,
                    ms,
                    token,
                    me,
                    k,
                )?
            };
            if let Some(run) = run {
                schedule(sched, run)?;
            }
            task.set(dst, Word::UNIT);
        }
        Extern::AsyncParkSignal => {
            let me = current_cell(task)?;
            // 見張りの語は待ち手の列と同じ区間で書く。ほかの worker の集める地点が書く値と順序を保つためである
            // (docs/spec/vm.md の「上限と割り込み」)
            let run = {
                let mut cells = sched.cells.lock();
                let run = cells.park_signal(task.raw(arg(0)), me, task.get(arg(1)))?;
                cells.watch(&sched.watcher.control);
                run
            };
            if let Some(run) = run {
                schedule(sched, run)?;
            }
            task.set(dst, Word::UNIT);
        }
        Extern::NetSubmitWrite | Extern::StreamSubmitWrite => {
            let (buf, stream) = (task.get(arg(0)), task.get(arg(1)));
            bind_here(sched, ctx, stream)?;
            here!(sched, post, here);
            let token = cells::submit_write(
                &sched.cells,
                &here,
                ctx,
                sched.driver.as_deref_mut(),
                task.worker,
                task.depth_offset,
                buf,
                stream,
            )?;
            task.set_raw(dst, token);
        }
        ext @ (Extern::NetSubmitShutdownWrite | Extern::NetSubmitSetNodelay) => {
            let (on, stream) = if ext == Extern::NetSubmitShutdownWrite {
                (None, task.get(arg(0)))
            } else {
                (Some(task.raw(arg(0)) != 0), task.get(arg(1)))
            };
            bind_here(sched, ctx, stream)?;
            here!(sched, post, here);
            let token = cells::submit_control(
                &sched.cells,
                &here,
                ctx,
                sched.driver.as_deref_mut(),
                task.worker,
                task.depth_offset,
                on,
                stream,
            )?;
            task.set_raw(dst, token);
        }
        Extern::NetSubmitRead | Extern::StreamSubmitRead => {
            let (buf, stream) = (task.get(arg(0)), task.get(arg(1)));
            bind_here(sched, ctx, stream)?;
            here!(sched, post, here);
            let token = cells::submit_read(
                &sched.cells,
                &here,
                ctx,
                sched.driver.as_deref_mut(),
                task.worker,
                task.depth_offset,
                buf,
                stream,
            )?;
            task.set_raw(dst, token);
        }
        ext @ (Extern::NetSubmitListen | Extern::NetSubmitConnect) => {
            let (host, port) = (task.get(arg(0)), task.raw(arg(1)));
            // 新しいハンドルは、本物の時計の run では自分の driver に作る (docs/spec/vm.md の「時計と driver」)
            sched.own_driver()?;
            here!(sched, post, here);
            let submitted = if ext == Extern::NetSubmitListen {
                cells::submit_listen(
                    &sched.cells,
                    &here,
                    ctx,
                    sched.driver.as_deref_mut(),
                    task.worker,
                    task.depth_offset,
                    host,
                    port,
                )?
            } else {
                cells::submit_connect(
                    &sched.cells,
                    &here,
                    ctx,
                    sched.driver.as_deref_mut(),
                    task.worker,
                    task.depth_offset,
                    host,
                    port,
                )?
            };
            match submitted {
                Ok(token) => task.set_raw(dst, token),
                // 受け付けの前に断った引数は、driver の誤りと分けて種類 `Other` にする (docs/spec/stdlib.md の「`Std.Net`」)
                Err(message) => {
                    let raise = Raise::Io {
                        kind: externs::IO_OTHER,
                        message,
                    };
                    return raise_again::<R>(code, ctx, task, dst, raise, ext);
                }
            }
        }
        Extern::NetSubmitAccept => {
            let listener = task.get(arg(0));
            here!(sched, post, here);
            let token = cells::submit_accept(
                &sched.cells,
                &here,
                ctx,
                sched.driver.as_deref_mut(),
                task.worker,
                task.depth_offset,
                listener,
            )?;
            task.set_raw(dst, token);
        }
        Extern::NetOpListener
        | Extern::NetOpStream
        | Extern::StreamOpReader
        | Extern::StreamOpWriter
        | Extern::ExecOpChild => {
            // token は借りるだけで、組を作る後の `con` が消費する
            let w = sched
                .cells
                .lock()
                .op_stream(task.raw(arg(0)), task.worker)?;
            task.set(dst, w);
        }
        Extern::NetOpOutcome | Extern::StreamOpOutcome | Extern::ExecOpOutcome => {
            // token は借りるだけで、組を作る後の `con` が消費する
            let code = sched
                .cells
                .lock()
                .op_outcome(task.raw(arg(0)), task.worker)?;
            task.set_raw(dst, code);
        }
        Extern::NetOpDone => {
            // token は借りるだけで、組を作る後の `con` が消費する
            let done = sched.cells.lock().op_done(task.raw(arg(0)), task.worker)?;
            task.set_raw(dst, i64::from(done));
        }
        ext @ (Extern::NetTakeListener
        | Extern::NetTakeStream
        | Extern::StreamTakeReader
        | Extern::StreamTakeWriter) => {
            let made = match ext {
                Extern::NetTakeListener => cells::Made::Listener,
                Extern::NetTakeStream => cells::Made::Stream,
                Extern::StreamTakeReader => cells::Made::Reader,
                _ => cells::Made::Writer,
            };
            let w = cells::take_made(&sched.cells, ctx, task.raw(arg(0)), task.worker, made)?;
            task.set(dst, w);
        }
        Extern::NetTakeBytes | Extern::StreamTakeBytes => {
            let buf = cells::take_bytes(&sched.cells, ctx, task.raw(arg(0)), task.worker)?;
            task.set(dst, buf);
        }
        Extern::NetSubmitReadLine | Extern::StreamSubmitReadLine => {
            let stream = task.get(arg(0));
            bind_here(sched, ctx, stream)?;
            here!(sched, post, here);
            let token = cells::submit_read_line(
                &sched.cells,
                &here,
                ctx,
                sched.driver.as_deref_mut(),
                task.worker,
                task.depth_offset,
                stream,
            )?;
            task.set_raw(dst, token);
        }
        ext @ (Extern::NetTakeLine | Extern::StreamTakeLine) => {
            match cells::take_line(&sched.cells, ctx, task.raw(arg(0)), task.worker)? {
                Ok(line) => task.set(dst, line),
                // `Stdin.read_line` と同じく、UTF-8 でない行は種類 `NotUtf8` にする (docs/spec/stdlib.md の「`Std.Net`」)
                Err(message) => {
                    let raise = Raise::Io {
                        kind: externs::IO_NOT_UTF8,
                        message,
                    };
                    return raise_again::<R>(code, ctx, task, dst, raise, ext);
                }
            }
        }
        Extern::StreamTakeLineBytes => {
            let line = cells::take_line_bytes(&sched.cells, ctx, task.raw(arg(0)), task.worker)?;
            task.set(dst, line);
        }
        ext @ (Extern::NetFinish | Extern::StreamFinish | Extern::ExecFinish) => {
            let taken = cells::take_op(&sched.cells, ctx, task.raw(arg(0)), task.worker)?;
            // 記録が持っていたものは受け付けたタスクの ID の物体で、操作は終わっているので driver はもう触れない。
            // 作ったハンドルは落とすと bin を通して閉じる (docs/spec/vm.md の「操作の記録」)
            for w in [taken.buf, taken.made].into_iter().flatten() {
                unsafe { ctx.heap_mut().decref(w) }?;
            }
            // 種類を決めた誤りは包みが投げる。取り消しを頼んだ記録の誤りは、包みが投げる取り消しに含める
            // (docs/spec/stdlib.md の「`Std.Net`」)
            if let Some((e, message)) = taken.failure() {
                let raise = Raise::Io {
                    kind: io_kind(e),
                    message,
                };
                return raise_again::<R>(code, ctx, task, dst, raise, ext);
            }
            task.set(dst, Word::UNIT);
        }
        Extern::TlsSubmitClient => {
            // `host`と`pem`は借りるので、写して手放さない。仕事はプールのスレッドで走るので、根はここでrunのdriverの
            // 種類から選ぶ (docs/spec/stdlib.mdの「`Std.Tls`」)
            let host = unsafe { &*ctx.heap().string(task.get(arg(0)))? }.to_owned();
            let pem = unsafe { &*ctx.heap().string(task.get(arg(1)))? }.to_owned();
            let roots = tls::Roots::of(sched.driver_kind);
            jobs::submit::<R, _>(ctx, task, sched, dst, TLS_JOB, move || {
                tls::client_conn(roots, &host, pem.as_bytes())
            })?;
        }
        Extern::TlsSubmitPush => {
            // `Conn`は消費するので、中身を取り出して箱を手放してから出す。記録はヒープの語を持たない。バイト列は
            // 借りるので、手放さずに写す (docs/spec/vm.mdの「プールの仕事」)
            let (bytes, conn) = (task.get(arg(0)), task.get(arg(1)));
            let c = unsafe { ctx.heap().foreign_take::<tls::TlsConn>(conn) }?;
            unsafe { ctx.heap_mut().decref(conn) }?;
            let b = unsafe { &*ctx.heap().bytes(bytes)? }.to_vec();
            // `Ok`は、両方の仕事が`take_conn`の答えの型を分け合うためだけにある
            jobs::submit::<R, _>(ctx, task, sched, dst, TLS_JOB, move || {
                Ok::<_, String>(tls::push_conn(c, b))
            })?;
        }
        ext @ Extern::TlsTakeConn => {
            let (token, wake) = (task.raw(arg(0)), task.raw(arg(1)));
            let outcome = jobs::take::<R, Result<tls::TlsConn, String>>(
                ctx,
                task,
                &sched.cells,
                token,
                wake,
                TLS_JOB,
            )?;
            let raise = match outcome {
                jobs::Outcome::Woke(Ok(c)) => {
                    let w = unsafe { ctx.heap_mut() }.alloc_foreign(c, true);
                    task.set(dst, w);
                    return Ok(());
                }
                // `Err`を返すのは`submit_client`の仕事だけで、接続の設定の誤りである
                jobs::Outcome::Woke(Err(message)) => Raise::Io {
                    kind: tls::SETUP_KIND,
                    message,
                },
                jobs::Outcome::Failed(e) => Raise::Io {
                    kind: io_kind(&e),
                    message: format!("cannot run TLS: {e}"),
                },
                jobs::Outcome::Cancelled => Raise::Io {
                    kind: cells::IO_CANCELLED,
                    message: "task was cancelled".into(),
                },
            };
            return raise_again::<R>(code, ctx, task, dst, raise, ext);
        }
        Extern::NetLocalPort => {
            // リスナーは借りるだけで、組を作る後の `con` が消費する
            let port = cells::local_port(ctx, task.get(arg(0)))?;
            task.set_raw(dst, port);
        }
        Extern::NetPeerAddr => {
            // ストリームは借りるだけで、組を作る後の `con` が消費する
            let peer = cells::peer_addr(ctx, task.get(arg(0)))?;
            task.set(dst, peer);
        }
        Extern::StreamSubmitOpen => {
            let (path, mode) = (task.get(arg(0)), task.raw(arg(1)));
            // 取り込みは、本物の時計の run では自分の driver が行う (docs/spec/vm.md の「時計と driver」)
            sched.own_driver()?;
            here!(sched, post, here);
            let submitted = cells::submit_open(
                &sched.cells,
                &here,
                ctx,
                sched.driver.as_deref_mut(),
                task.worker,
                task.depth_offset,
                path,
                mode,
            )?;
            match submitted {
                Ok(token) => task.set_raw(dst, token),
                // 開けなかったファイルは、記録を作らずに `FileIn.open` と同じ種類で投げる
                // (docs/spec/stdlib.md の「`Std.Stream`」)
                Err((e, message)) => {
                    let raise = Raise::Io {
                        kind: io_kind(&e),
                        message,
                    };
                    return raise_again::<R>(code, ctx, task, dst, raise, Extern::StreamSubmitOpen);
                }
            }
        }
        Extern::StreamOpPrefix | Extern::ExecOpPrefix => {
            // token は借りるだけで、組を作る後の `con` が消費する
            let prefix = sched
                .cells
                .lock()
                .op_prefix(task.raw(arg(0)), task.worker)?;
            let w = ctx.fresh_string(&prefix);
            task.set(dst, w);
        }
        Extern::StreamSource => {
            // 読み手は借りるだけで、組を作る後の `con` が消費する
            let name = cells::stream_source(ctx, task.get(arg(0)))?;
            task.set(dst, name);
        }
        Extern::ExecSpawnRaw => {
            let stages = cells::take_string(ctx, task.get(arg(0)))?;
            let payload = cells::take_string(ctx, task.get(arg(2)))?;
            let path = cells::take_string(ctx, task.get(arg(4)))?;
            let (stdin, stdout) = (task.raw(arg(1)), task.raw(arg(3)));
            // 仮想の時計のテストの driver は子を扱わない。どの worker でも driver に触れる前に断る。worker 1 から先は
            // driver を持たないためである (docs/spec/stdlib.md の「`Std.Exec`」)
            if sched.driver_kind == DriverKind::Virtual {
                let raise = Raise::Io {
                    kind: externs::IO_NOT_AVAILABLE,
                    message: format!(
                        "cannot run `{}`: the test driver has no processes",
                        eml_rt::last_program(&stages)
                    ),
                };
                return raise_again::<R>(code, ctx, task, dst, raise, Extern::ExecSpawnRaw);
            }
            // pipe は起動する worker の driver が作り、端は縛らずに子が持つ (docs/spec/vm.md の「時計と driver」)
            sched.own_driver()?;
            let driver = sched
                .driver
                .as_deref_mut()
                .ok_or(RunFault::Internal("a worker without a driver"))?;
            let mut parents = Vec::new();
            let launched = {
                let mut pipes = |end: PipeEnd| {
                    driver.pipe(end != PipeEnd::Stdin).map(|pair| {
                        parents.push((end, pair.parent));
                        pair.child
                    })
                };
                eml_rt::launch(
                    ctx.host(),
                    &sched.watcher.control,
                    &stages,
                    stdin,
                    &payload,
                    stdout,
                    &path,
                    Some(&mut pipes),
                )
            };
            let launched = match launched {
                Ok(launched) => launched,
                // 起動できなければ、作った pipe の親の端はここで閉じる
                Err(fault) => {
                    drop(parents);
                    return failed::<R>(code, ctx, task, dst, fault, Purity::Effectful);
                }
            };
            let (mut stdin, mut stdout, mut stderr) = (None, None, None);
            for (end, parent) in parents {
                match end {
                    PipeEnd::Stdin => stdin = Some(RawResource::PipeWrite(parent)),
                    PipeEnd::Stdout => stdout = Some(RawResource::PipeRead(parent)),
                    PipeEnd::Stderr => stderr = Some(RawResource::PipeRead(parent)),
                }
            }
            let programs = launched.programs;
            let processes = launched.spawned.processes;
            let fits = !processes.is_empty() && processes.len() == programs.len();
            let last = programs.last().cloned().unwrap_or_default();
            let child = cells::ExecChild::new(programs, processes, stdin, stdout, stderr);
            // 段と子の数が合わない Host の結果は、`Proc.spawn_raw` と同じく子を片づけてから起動の誤りにする
            if !fits {
                drop(child);
                let fault = Fault::Io {
                    op: eml_rt::IoOp::Run,
                    name: last,
                    reason: eml_rt::IoReason::Other,
                };
                return failed::<R>(code, ctx, task, dst, fault, Purity::Effectful);
            }
            // 確保するだけで、`Ctx` の持つ語には触れない。`Child` は `Lin` である
            let w = unsafe { ctx.heap_mut() }.alloc_foreign(child, true);
            task.set(dst, w);
        }
        ext @ (Extern::ExecStdin | Extern::ExecStdout | Extern::ExecStderr) => {
            let pipe = match ext {
                Extern::ExecStdin => cells::ChildPipe::Stdin,
                Extern::ExecStdout => cells::ChildPipe::Stdout,
                _ => cells::ChildPipe::Stderr,
            };
            // 子は借りるだけで、組を作る後の `con` が消費する
            match cells::exec_take(ctx, task.get(arg(0)), pipe)? {
                Ok(stream) => task.set(dst, stream),
                Err(message) => {
                    let raise = Raise::Io {
                        kind: externs::IO_NOT_AVAILABLE,
                        message,
                    };
                    return raise_again::<R>(code, ctx, task, dst, raise, ext);
                }
            }
        }
        Extern::ExecStageCount => {
            // 子は借りるだけで、組を作る後の `con` が消費する
            let n = cells::exec_stage_count(ctx, task.get(arg(0)))?;
            task.set_raw(dst, n);
        }
        Extern::ExecSubmitWait => {
            let (k, child) = (task.raw(arg(0)), task.get(arg(1)));
            // 子の終わりは、持ち主の worker の driver が待つ (docs/spec/vm.md の「操作の記録」)
            sched.own_driver()?;
            here!(sched, post, here);
            let token = cells::submit_wait(
                &sched.cells,
                &here,
                ctx,
                sched.driver.as_deref_mut(),
                task.worker,
                task.depth_offset,
                k,
                child,
            )?;
            task.set_raw(dst, token);
        }
        Extern::ExecReap => {
            // 子は借りるだけで、組を作る後の `con` が消費する
            match cells::exec_reap(ctx, task.raw(arg(0)), task.get(arg(1)))? {
                Ok(code) => task.set_raw(dst, code),
                Err((e, message)) => {
                    let raise = Raise::Io {
                        kind: io_kind(&e),
                        message,
                    };
                    return raise_again::<R>(code, ctx, task, dst, raise, Extern::ExecReap);
                }
            }
        }
        // GUIを開く行と、ゲートの行。ゲートは登録表のセルである (docs/spec/vm.mdの「GUIのPark」)
        Extern::GuiWantRaw => match gui::want(sched)? {
            Ok(gate) => task.set_raw(dst, gate),
            Err(raise) => return raise_again::<R>(code, ctx, task, dst, raise, ext),
        },
        Extern::GuiOpenUiRaw => match gui::open_ui(ctx, sched, task.raw(arg(0)), task.cell)? {
            Ok(ui) => task.set(dst, ui),
            Err(raise) => return raise_again::<R>(code, ctx, task, dst, raise, ext),
        },
        Extern::GuiPokeRaw => {
            poke_gate(sched, task.raw(arg(0)))?;
            task.set(dst, Word::UNIT);
        }
        Extern::GuiAbandonRaw => {
            gui::abandon(sched, task.raw(arg(0)))?;
            task.set(dst, Word::UNIT);
        }
        Extern::GuiIsGuiTaskRaw => {
            let own = gui::is_gui_task(task.raw(arg(0)), task.cell);
            task.set_raw(dst, i64::from(own));
        }
        _ => {
            return Err(RunFault::Internal(
                "a row outside Std.Async, Std.Chan, Std.Atomic, Std.Net, Std.Stream, Std.Exec, Std.Tls and Std.Gui reached the Async instruction",
            ));
        }
    }
    Ok(())
}

/// `Std.Tls`のプールの仕事の出どころ。受け付けの行はどれも、ただ1つの取り出しの行`take_conn`の名前で出す。
const TLS_JOB: JobSource = JobSource::Std(Extern::TlsTakeConn as u32);

/// driver の誤りの種類を `IOErrorKind` のタグにする。extern の誤りと同じく、最初の5種類のうち I/O の4つだけを出す
/// (docs/spec/stdlib.md の「`Std.Net`」)。
pub(crate) fn io_kind(e: &std::io::Error) -> i64 {
    match e.kind() {
        std::io::ErrorKind::NotFound => externs::IO_NOT_FOUND,
        std::io::ErrorKind::PermissionDenied => externs::IO_PERMISSION_DENIED,
        std::io::ErrorKind::Unsupported => externs::IO_NOT_AVAILABLE,
        _ => externs::IO_OTHER,
    }
}

/// ストリーム `stream` の操作を受け付ける前に、ストリームが受けたまま縛っていなければ自分の driver を用意する。
/// 受け付けがストリームを自分の driver に取り込むためである (docs/spec/vm.md の「時計と driver」)。
fn bind_here<M: Mode, P: Policy>(
    sched: &mut Scheduler<P>,
    ctx: &Ctx<M>,
    stream: Word,
) -> Result<(), RunFault> {
    if cells::stream_unbound(ctx, stream)? {
        sched.own_driver()?;
    }
    Ok(())
}

/// `Op::FfiSubmit`。仕事は`ffi::job`が作り、stdのプールの仕事と同じ`jobs::submit_op`で記録のhostのdriverに出す
/// (docs/spec/vm.md の「`blocking` の呼び出し」)。
fn ffi_submit<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    dst: Slot,
    at: u32,
) -> Result<(), RunFault> {
    let Some((from, job)) = ffi::job::<R>(code, ctx, task, dst, at)? else {
        return Ok(());
    };
    jobs::submit_op::<R>(ctx, task, sched, dst, from, job)
}

/// 今の `Task` のタスクのセル。`main`、`par` の右の子、`compute` の仕事は `Async` の操作を実行しない。
pub(crate) fn current_cell(task: &Task) -> Result<CellRef, RunFault> {
    task.cell
        .ok_or(RunFault::Internal("an Async operation runs outside a task"))
}

/// `take_result` と `run_result` が受け取ったタスクの失敗を、今のタスクで起こし直す。`Panic` は `panic` の extern と
/// 同じく `try_panic` に届け、なければ実行時エラーにする。IO の失敗は失敗した extern と同じく `failed` が届ける。
/// 文言はセルが Rust の文字列で持っていたので、今の worker で新しい文字列を作る (docs/spec/vm.md の「`Async` のタスク」)。
fn raise_again<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    dst: Slot,
    raise: Raise,
    ext: Extern,
) -> Result<(), RunFault> {
    match raise {
        Raise::Panic(message) => {
            if abort_to::<R, _>(code, ctx, task, dst, ExternEffect::Panic, PANICKED, |ctx| {
                [ctx.fresh_string(&message)]
            })? {
                return Ok(());
            }
            Err(RunFault::Panicked(message))
        }
        Raise::Io { kind, message } => {
            let fault = Fault::Raised(Box::new(Raised {
                kind,
                message: message.into_boxed_str(),
            }));
            failed::<R>(code, ctx, task, dst, fault, ext.row().purity)
        }
    }
}

/// `start`が新しいタスクを置くworker (`Run::place_owner`)。
fn place_owner<P: Policy>(sched: &Scheduler<P>, pinned: bool) -> u32 {
    sched
        .run
        .place_owner(sched.workers.places(), sched.me(), pinned)
}

/// 今の `Task` から、スコープ `scope` に worker `owner` を持ち主とする新しいタスクを置く。閉包 `g` とまだ印のない根に、
/// 新しいタスクの ID で印を付ける。返す仕事は、持ち主が自分のときだけある。
fn start_task<R: Exec>(
    ctx: &mut Ctx<R::M>,
    task: &Task,
    sched: &mut Scheduler<R::P>,
    scope: CellRef,
    owner: u32,
    g: Word,
    pinned: bool,
) -> Result<Option<(Word, Option<Runnable>)>, RunFault> {
    let roots = unshared_roots(ctx, sched);
    let started = {
        here!(sched, post, here);
        sched
            .cells
            .lock()
            .start(ctx, &here, scope, task.worker, owner, g, &roots, pinned)?
    };
    Ok(started.map(|(cell, run)| (cells::task_handle(ctx, &sched.cells, cell), run)))
}

/// `finish_*` の終わり方をエントリの `Task` に預ける。核に渡すのは `Task` の `Finish` で、そのときにはタスクの ID の物体は
/// 終わり方の値のほかに片付いている。先に渡すと、起こした側がセルを放してタスクの ID を退かせたときに、まだ動いている
/// タスクの物体が残る (docs/spec/vm.md の「`Async` のタスク」)。
fn deposit(task: &mut Task, outcome: cells::Outcome) -> Result<(), RunFault> {
    if task.cell.is_none() || task.outcome.is_some() {
        return Err(RunFault::Internal(
            "a task finishes twice or outside a task",
        ));
    }
    task.outcome = Some(outcome);
    Ok(())
}

/// 核が返した仕事を実行待ちの列の後ろに積む。どの方針でも FIFO である (docs/spec/vm.md の「タスクとスケジュール」)。
/// 始める仕事と再開する仕事はフレームを持たない `Entry` のまま積み、持ち主が `pick` で取ったときにエントリの `Task` に
/// する。token は待っている `Task` の番号である。
fn schedule<P: Policy>(sched: &mut Scheduler<P>, run: Runnable) -> Result<(), RunFault> {
    // 今のタスクが自分の再開を積んだ。待ちに入っても実行待ちにいるので、今の数で `top` に出す
    // (docs/spec/vm.md の「タスクとスケジュール」)
    if let Runnable::Resume { cell, .. } | Runnable::ResumeIo { cell, .. } = run
        && let Some(count) = sched.published_count(cell)
    {
        sched.offer_top(cell, count);
        sched.yielded = true;
    }
    let entry = match run {
        Runnable::Start {
            cell,
            worker,
            closure,
        } => Entry {
            cell,
            worker,
            kind: EntryKind::Start,
            word: closure,
        },
        Runnable::Resume {
            cell,
            worker,
            k,
            wake,
        } => Entry {
            cell,
            worker,
            kind: EntryKind::Resume { wake },
            word: k,
        },
        Runnable::ResumeIo {
            cell,
            worker,
            k,
            wake,
            op,
        } => Entry {
            cell,
            worker,
            kind: EntryKind::ResumeIo { wake, op },
            word: k,
        },
        Runnable::Token(WaitToken(token)) => {
            let id = token as TaskId;
            match sched.tasks.get(id) {
                Some(Some(Task {
                    state: TaskState::WaitingCell(_),
                    ..
                })) => {}
                _ => {
                    return Err(RunFault::Internal("a woken token names no waiting task"));
                }
            }
            sched.run.push_ready(sched.index, Ready::Task(id));
            return Ok(());
        }
    };
    // メモリを見張る方針では、最初の`Async`のタスクで監視のスレッドを始める。どのタスクもここで列に入り、タスクの
    // ないrunは監視のスレッドを作らない (docs/spec/vm.mdの「タスクとスケジュール」)
    if P::MEMORY == Memory::Watched {
        sched.start_monitor();
    }
    sched.run.push_ready(sched.index, Ready::Entry(entry));
    Ok(())
}

/// 実行待ちの列から取った項を、表にあるタスクの番号にする。`Entry` はここで、集まりのスタックを使うエントリの `Task` に
/// する (docs/spec/vm.md の「`Async` のタスク」)。
fn ready_task<M: Mode, P: Policy>(
    code: &CodeBase<M>,
    sched: &mut Scheduler<P>,
    item: Ready,
) -> Result<TaskId, RunFault> {
    let entry = match item {
        Ready::Task(id) => return Ok(id),
        Ready::Entry(entry) => entry,
    };
    let entries = code
        .entries
        .ok_or(RunFault::Internal("the Async entries are not loaded"))?;
    let (function, args, argc) = match entry.kind {
        EntryKind::Start => (entries.start, [entry.word, Word::UNIT, Word::UNIT], 1),
        EntryKind::Resume { wake } => (
            entries.resume,
            [entry.word, Word::from_bits(wake as usize), Word::UNIT],
            2,
        ),
        EntryKind::ResumeIo { wake, op } => (
            entries.resume_io,
            [
                entry.word,
                Word::from_bits(wake as usize),
                Word::from_bits(op as usize),
            ],
            3,
        ),
    };
    let spare = sched.stacks.pop().unwrap_or_default();
    let task = Task::entry(
        code,
        function,
        args,
        argc,
        entry.cell,
        entry.worker,
        sched.max_depth,
        spare,
    );
    Ok(sched.add_task(task))
}

/// エントリの `Task` が動き始める。始める仕事なら、閉包を始めたタスクの ID から今の ID へ戻す (`par` の join の
/// `transfer_back` と同じ形)。引数をエントリのフレームの slot 0.. に書いてエントリの先頭へ進む。セルにタスクの上限の
/// 旗が立っていれば、保留にする (docs/spec/vm.md の「タスクの上限」)。
fn enter<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    function: u32,
    mut args: [Word; 3],
    argc: u8,
) -> Result<(), RunFault> {
    let cell = task
        .cell
        .ok_or(RunFault::Internal("an Async entry task has no cell"))?;
    let marks = if argc == 1 {
        let (spawner, marks) = sched.cells.lock().started(cell)?;
        // 閉包は始めたタスクが印を付けて渡した参照で、このタスクだけが持つ
        unsafe { ctx.heap_mut().transfer_back(args[0], spawner) }?;
        marks
    } else {
        // 引数が3つのエントリは `resume_io` で、操作の待ちからの再開である
        let (wake, marks) = sched
            .cells
            .lock()
            .resumed(cell, args[1].bits() as i64, argc == 3)?;
        args[1] = Word::from_bits(wake as usize);
        marks
    };
    if marks.limit {
        mark_pending(ctx, task);
    }
    // タスクの数は待ちをまたいでセルが持ち、このエントリはそこから数え始める (docs/spec/vm.md の「タスクとスケジュール」)
    task.counted = marks.bytes;
    task.added = 0;
    sched.mark = ctx.heap().control().taken(sched.index);
    sched.publish(task);
    sched.clear_top(cell);
    let argc = usize::from(argc);
    let base = task.base;
    task.stack[base..base + argc].copy_from_slice(&args[..argc]);
    task.pc = code.functions[function as usize].start;
    Ok(())
}

/// エントリの `Task` の `Finish`。タスクの残りの `evv` を手放し、預かった終わり方を核に渡して起こす仕事を積み、スタックを
/// 集まりに返してほかのタスクに切り替える。`exit` の巻き戻しが底に着いたら、終わりの欄に書いて run を捨てる
/// (docs/spec/vm.md の「`exit`」)。
#[inline(never)]
fn entry_finish<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
) -> Result<(), RunFault> {
    let Some(cell) = task.cell else {
        return Err(RunFault::Internal("a task is left at the end"));
    };
    if let Some(n) = task.exit {
        // 途中のフレームは巻き戻しで片付けてある。セルにもスコープにも書かず、ほかのタスクも動かさずに run ごと
        // 捨てる。先に書いた者がいても、その終了コードが勝つだけなので、結果は見ない
        // (docs/spec/vm.md の「`Async` のタスク」)
        unsafe {
            ctx.yield_finish()?;
            ctx.evv_set(Word::UNIT)?;
        }
        cells::end_with_exit(ctx.heap().control(), n);
        // ほかの worker の `exit` は、worker 0 に止まらせるために `END` を立てる。抜けるときにすべての worker を起こすので、
        // 眠っている worker 0 もそれを読む (docs/spec/vm.md の「誤り、止めること、終わり」)
        if sched.index != 0 {
            ctx.heap().control().end_run();
        }
        return Err(RunFault::Stopped(Stop::Ended));
    }
    if ctx.yielding() != 0 || !task.boundaries.is_empty() {
        return Err(RunFault::Internal(
            "an Async task ends in the middle of a yield or a par",
        ));
    }
    sched.end_slice(task);
    sched.unpublish();
    // タスクの残りの `evv` を、タスクの ID のまま手放す
    let rest = unsafe { switch_to(ctx, Word::UNIT, task.worker, task.depth_offset, task.limit) };
    unsafe { ctx.heap_mut().decref(rest) }?;
    if let Some(outcome) = task.outcome.take() {
        let mut out = Vec::new();
        {
            here!(sched, post, here);
            cells::finish(
                &sched.cells,
                &here,
                ctx,
                cell,
                outcome,
                task.depth_offset,
                &mut out,
            )?;
        }
        for run in out {
            schedule(sched, run)?;
        }
        sched.clear_top(cell);
    } else {
        sched.left_entry(cell, task.added);
    }
    switch_away::<R>(code, ctx, task, sched, None)
}

/// 中断の操作の番号。Prelude の `IO` (`io_failed`) と `Panic` (`panicked`) は、どちらも操作を1つだけ宣言する
/// (docs/spec/vm.md の「`IO` と `Panic` への中断」)。
const IO_FAILED: usize = 0;
pub(crate) const PANICKED: usize = 0;

/// 命令 `pc` が返した誤り。算術の専用の命令の誤りなら、ほかの extern の誤りと同じく `Panic` に届け、届かなければ
/// 誤りを返す。ほかの命令の誤りはそのまま返す。命令の中でなく `run` の誤りの道で届けるのは、`step` の速い道の命令の
/// 列を変えないためである (docs/spec/vm.md の「`IO` と `Panic` への中断」)。
#[cold]
#[inline(never)]
fn step_failed<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    pc: Pc,
    fault: RunFault,
) -> Result<(), RunFault> {
    let (dst, purity) = match code.tables.code[pc as usize] {
        Op::IntAdd { dst, .. }
        | Op::IntSub { dst, .. }
        | Op::IntMul { dst, .. }
        | Op::IntDiv { dst, .. }
        | Op::IntRem { dst, .. }
        | Op::IntNeg { dst, .. }
        | Op::ArrayGet { dst, .. }
        | Op::StrByteAt { dst, .. }
        | Op::BytesGet { dst, .. }
        | Op::ArraySet { dst, .. }
        | Op::UnboxedGet { dst, .. }
        | Op::UnboxedSet { dst, .. } => (dst, Purity::Panics),
        // `length` の行は `Pure` で巻き戻しの地点を持たない
        Op::ArrayLength { dst, .. }
        | Op::UnboxedLength { dst, .. }
        | Op::StrByteLength { dst, .. }
        | Op::BytesLength { dst, .. } => (dst, Purity::Pure),
        _ => return Err(fault),
    };
    match fault {
        RunFault::Extern(fault) => failed::<R>(code, ctx, task, dst, fault, purity),
        other => Err(other),
    }
}

/// 算術と比較以外の extern。
fn extern_call<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    counts: &mut RunCounts,
    dst: Slot,
    at: u32,
) -> Result<(), RunFault> {
    match run_extern::<R>(code, ctx, task, counts, dst, at) {
        Err(RunFault::Extern(fault)) => {
            let ext = Extern::ALL[code.tables.operands[at as usize] as usize];
            failed::<R>(code, ctx, task, dst, fault, ext.row().purity)
        }
        other => other,
    }
}

/// extern の誤りを、誤りの種類で決まる中断に変える。`Panic` に渡す誤りのうち、`Pure` でない行のものを `try_panic` に
/// 届ける。`Pure` の行は巻き戻しの地点を持たないので届けない (`Env.arg` は `Effectful` だが、範囲の外の位置は
/// `Panic` である)。`try_io` の捕まえる誤りは、`Effectful` の行のものだけを届ける。捕まえる handler がなければ、
/// 実行時エラーにする。失敗した extern は引数をすでに消費している。docs/spec/vm.md の「`IO` と `Panic` への中断」を
/// 見る。
pub(crate) fn failed<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    dst: Slot,
    fault: Fault,
    purity: Purity,
) -> Result<(), RunFault> {
    // `Pure` の行の extern は巻き戻しの地点を持たないので、誤りを返しても中断にしない (docs/spec/vm.md の「extern の中断」)
    let caught = if fault.panics() && purity != Purity::Pure {
        abort_to::<R, _>(code, ctx, task, dst, ExternEffect::Panic, PANICKED, |ctx| {
            [ctx.fresh_string(&fault.to_string())]
        })?
    } else if let Some(kind) = fault
        .io_error_kind()
        .filter(|_| purity == Purity::Effectful)
    {
        abort_to::<R, _>(code, ctx, task, dst, ExternEffect::Io, IO_FAILED, |ctx| {
            [
                Word::from_bits(kind as usize),
                ctx.fresh_string(&fault.to_string()),
            ]
        })?
    } else {
        false
    };
    if caught {
        Ok(())
    } else {
        Err(RunFault::Extern(fault))
    }
}

/// 実行の時点で最も近い `effect` の handler の、`never` の操作 `op` の節のエントリを、evidence と `args` で呼ぶ。evidence
/// がなければ何もせずに `false` を返し、`args` も作らない。エントリは `never` の節のエントリなので、`yield_abort` で巻き戻しを
/// 始めて戻らない。中断を起こした命令のフレームはエントリの呼び出し元になり、その命令の地点の表の行で片付く。エントリが戻らない
/// ので、安全点も置かない (docs/spec/vm.md の「`IO` と `Panic` への中断」)。
pub(crate) fn abort_to<R: Exec, const N: usize>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    dst: Slot,
    effect: ExternEffect,
    op: usize,
    args: impl FnOnce(&mut Ctx<R::M>) -> [Word; N],
) -> Result<bool, RunFault> {
    let Some(&number) = code.index.effects.get(effect.row().name) else {
        return Ok(false);
    };
    let Some(ev) = (unsafe { ctx.ev_find(number) })? else {
        return Ok(false);
    };
    // 保留のタスクが着地までの節のエントリで旗によって失敗しないように、猶予に替えて保留を覚えておく
    // (docs/spec/vm.md の「タスクの上限」)
    if task.limit == Limit::Pending {
        task.limit = Limit::Aborting { pending: true };
        apply_limit(ctx, task.limit, task.depth_offset);
    }
    // `ev_find` が `dup` した evidence を、節のエントリの引数として渡す。`ev_clause` はそれを借りる
    let clause = unsafe { ctx.ev_clause(ev, op) }?;
    let args = args(ctx);
    task.scratch.clear();
    task.scratch.push(ev);
    task.scratch.extend(args);
    let ret = task.pc;
    apply::apply_clause::<R>(code, ctx, task, clause, apply::Target::Frame { ret, dst })?;
    Ok(true)
}

/// externの行の引数を、命令のオペランドが指すスロットから読む。引数 `i` はスロット `ops[2 + i]` にある。
struct At<'a> {
    task: &'a Task,
    ops: &'a [u32],
}

impl Args for At<'_> {
    fn word(&self, i: usize) -> Word {
        self.task.get(self.ops[2 + i] as Slot)
    }

    fn int(&self, i: usize) -> i64 {
        self.task.raw(self.ops[2 + i] as Slot)
    }

    fn float(&self, i: usize) -> f64 {
        self.task.float(self.ops[2 + i] as Slot)
    }
}

/// 一覧にまだ共有の本体がない行はここで `match` して `Ctx` のメソッドを呼び、それ以外の行はすべて
/// `eml_rows::rows::call` を通す (docs/spec/object-model.md の「extern」)。
fn run_extern<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    counts: &mut RunCounts,
    dst: Slot,
    at: u32,
) -> Result<(), RunFault> {
    let ops = &code.tables.operands[at as usize..];
    let arg = |i: usize| ops[2 + i] as Slot;
    // 語の引数は、LIR の所有の規則により VM が持つ参照で、extern がそれを消費する
    let ext = Extern::ALL[ops[0] as usize];
    if ext == Extern::Panic {
        let message = task.get(arg(0));
        if abort_to::<R, _>(code, ctx, task, dst, ExternEffect::Panic, PANICKED, |_| {
            [message]
        })? {
            return Ok(());
        }
        // 捕まえる `try_panic` がないので、文言を写して実行時エラーにする。写すと文字列の参照も手放す
        let text = unsafe { ctx.copy_string(message, usize::MAX) }?;
        return Err(RunFault::Panicked(text));
    }
    if ext == Extern::Exit {
        task.exit = Some(task.raw(arg(0)));
        unsafe { ctx.exit_abort() }?;
        // 宛先は不死のダミーなので、どの着地の印とも合わずに `Finish` まで巻き戻す
        let dummy = ctx.heap().dummy();
        return unwind::<R>(code, ctx, task, counts, dummy);
    }
    unsafe {
        // 借りる引数 (`FunctionRow::borrowed`) の行は、引数を手放さない。失敗しても、借りた値は呼んだ側の
        // スロットに残り、巻き戻しの表が手放す (docs/spec/lir.md の「extern の借用」)
        match ext {
            // channel の即値をそのまま `Cell` の即値にする (docs/spec/vm.md の「命令」)
            Extern::ChanToCell | Extern::AsyncJobCell => task.set_raw(dst, task.raw(arg(0))),
            Extern::EnvArgCount => task.set_raw(dst, ctx.env_arg_count()),
            Extern::EnvArg => {
                let w = ctx.env_arg(task.raw(arg(0)))?;
                task.set(dst, w);
            }
            Extern::EnvHas => {
                let b = ctx.env_has(task.get(arg(0)))?;
                task.set_raw(dst, b);
            }
            Extern::EnvLookup => {
                let w = ctx.env_var(task.get(arg(0)))?;
                task.set(dst, w);
            }
            Extern::ProcSpawn => {
                let w = ctx.proc_spawn(
                    task.get(arg(0)),
                    task.raw(arg(1)),
                    task.get(arg(2)),
                    task.raw(arg(3)),
                    task.get(arg(4)),
                )?;
                task.set(dst, w);
            }
            Extern::ProcWait => {
                let code = ctx.proc_wait(task.get(arg(0)))?;
                task.set_raw(dst, code);
            }
            Extern::ProcStageCount => {
                let n = ctx.proc_stage_count(task.get(arg(0)))?;
                task.set_raw(dst, n);
            }
            Extern::ProcWaitStage => {
                let code = ctx.proc_wait_stage(task.get(arg(1)), task.raw(arg(0)))?;
                task.set_raw(dst, code);
            }
            Extern::ProcCloseStdin => {
                let w = ctx.proc_close_stdin(task.get(arg(0)))?;
                task.set(dst, w);
            }
            // TLSの状態の行。待たないので普通のexternである (docs/spec/stdlib.mdの「`Std.Tls`」)
            Extern::TlsPushTls => {
                let w = tls::push_tls(ctx, task.get(arg(0)), task.get(arg(1)))?;
                task.set(dst, w);
            }
            Extern::TlsWritePlain => {
                let w = tls::write_plain(ctx, task.get(arg(0)), task.get(arg(1)))?;
                task.set(dst, w);
            }
            Extern::TlsPushEof => {
                let w = tls::push_eof(ctx, task.get(arg(0)))?;
                task.set(dst, w);
            }
            Extern::TlsSendCloseNotify => {
                let w = tls::send_close_notify(ctx, task.get(arg(0)))?;
                task.set(dst, w);
            }
            Extern::TlsPullTls => {
                let w = tls::pull_tls(ctx, task.get(arg(0)))?;
                task.set(dst, w);
            }
            Extern::TlsReadPlain => {
                let w = tls::read_plain(ctx, task.get(arg(0)), task.get(arg(1)))?;
                task.set(dst, w);
            }
            Extern::TlsStatus => task.set_raw(dst, tls::status(ctx, task.get(arg(0)))?),
            Extern::TlsErrorMessage => {
                let w = tls::error_message(ctx, task.get(arg(0)))?;
                task.set(dst, w);
            }
            Extern::TlsLineReady => task.set_raw(dst, tls::line_ready(ctx, task.get(arg(0)))?),
            Extern::TlsTakeLine => {
                let w = tls::take_line(ctx, task.get(arg(0)))?;
                task.set(dst, w);
            }
            // toolkitに触れる行。待たないので普通のexternである (docs/spec/stdlib.mdの「`Std.Gui`」)
            Extern::GuiCloseUi
            | Extern::GuiBeginWindowRaw
            | Extern::GuiBeginGridRaw
            | Extern::GuiEndRaw
            | Extern::GuiNextCellRaw
            | Extern::GuiCreateRaw
            | Extern::GuiAddItemRaw
            | Extern::GuiSetTextRaw
            | Extern::GuiSetIntRaw
            | Extern::GuiCloseWindowRaw
            | Extern::GuiOpenWindowsRaw
            | Extern::GuiFlushRaw
            | Extern::GuiPollRaw
            | Extern::GuiEventIntRaw
            | Extern::GuiEventTextRaw => {
                let n = ext.row().params.len();
                let mut args = [Word::UNIT; 5];
                for (i, w) in args.iter_mut().enumerate().take(n) {
                    *w = task.get(arg(i));
                }
                let w = gui::arm(ctx, ext, &args[..n])?;
                task.set(dst, w);
            }
            // いつも誤りを返すので、宛先には書かない。届け先は `failed` が決める (docs/spec/vm.md の「extern の中断」)
            Extern::FailIo => {
                let message = ctx.copy_string(task.get(arg(1)), usize::MAX)?;
                return Err(RunFault::Extern(Fault::Raised(Box::new(Raised {
                    kind: task.raw(arg(0)),
                    message: message.into_boxed_str(),
                }))));
            }
            _ => {
                let value = eml_rows::rows::call(ext, ctx, &At { task: &*task, ops })?;
                match value {
                    Value::Word(w) => task.set(dst, w),
                    Value::Int(n) => task.set_raw(dst, n),
                    Value::Float(x) => task.set_float(dst, x),
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use eml_rt::Control;

    use crate::cells::{MailItem, Registry, Shared};
    use crate::workers::{NO_CELL, cell_word};

    fn empty_task(max_depth: usize) -> Task {
        Task {
            stack: Vec::new(),
            pc: FINISH,
            base: 0,
            frame_size: 0,
            last_ret: FINISH,
            scratch: Vec::new(),
            stack_limit: stack_limit(max_depth),
            entry: 0,
            keep_result: false,
            exit: None,
            evv: Word::UNIT,
            worker: 0,
            depth_offset: 0,
            boundaries: Vec::new(),
            state: TaskState::Running,
            cell: None,
            outcome: None,
            placed: None,
            job: None,
            limit: Limit::Stops,
            counted: 0,
            added: 0,
        }
    }

    /// `NOTICE`は切り替えずに集める。
    #[test]
    fn a_notice_collects_without_switching() {
        let control = Arc::new(Control::new(eml_rt::Limits::default()));
        let (host, _) = crate::capture_host(Default::default());
        let ctx = Ctx::<eml_rt::Checked>::new(Arc::clone(&control), host);
        control.notice(0);
        assert!(poll(&ctx, 0, 0).ok() == Some(Polled::Collect));
        assert_eq!(control.worker_bits(0), 0);
    }

    /// 動いているタスクの安全点が `NOTICE` を見て集め、聞く者のいないシグナルで run を止める。逐次の方針なので、
    /// 切り替えはしない。届けたシグナルは集める地点が保留から下ろす。`Scheduler` を捨てると保留は消えるので、捨てる前に
    /// 確かめる (docs/spec/vm.md の「タスクとスケジュール」)。
    #[test]
    fn a_running_task_collects_a_notice_at_its_safepoint() {
        let program = eml_lir::parse(
            "\
fn main() -> unit {
  let u.0: unit = call f(())
  return u.0
}
fn f(p.0: unit) -> unit {
  return p.0
}
",
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let control = Arc::new(Control::new(eml_rt::Limits::default()));
        let mut code = CodeBase::<eml_rt::Checked>::new(Arc::clone(&control));
        let entry = code.load(&program, &crate::Linked::default()).unwrap();
        let (host, _) = crate::capture_host(Default::default());
        let mut ctx = Ctx::new(Arc::clone(&control), host);
        let mut task = Task::new(&code, entry, usize::MAX);
        task.worker = ctx.heap().worker();
        let driver = eml_io::TestDriver::new(None);
        let clock = crate::driver::RunClock::Virtual(driver.clock());
        let mut sched = Scheduler::new(
            crate::Sequential,
            Box::new(driver),
            Arc::new(crate::workers::Run::new(
                1,
                eml_sched::config::Seat::Merged,
                None,
                clock,
                Arc::default(),
            )),
            Arc::clone(&control),
            task.worker,
            usize::MAX,
            Arc::new(Shared::new(Registry::new(Vec::new(), 1))),
        );
        // 監視のスレッドの中継と同じことをする
        control.defer(Control::TERMINATE);
        assert_eq!(control.claim_signals(), Control::TERMINATE);
        sched.cells.mail.push(MailItem::Signal(Control::TERMINATE));
        control.notice(0);
        let mut counts = crate::RunCounts::default();
        let result = run::<ExecWith<eml_rt::Checked, crate::Sequential>>(
            &code,
            &mut ctx,
            &mut task,
            &mut sched,
            &mut counts,
        );
        let error = result.unwrap_err();
        assert!(error.terminated(), "{error:?}");
        assert_eq!(control.pending_signals(), 0);
        assert_eq!(control.stop_bits(), Control::TERMINATE);
        assert_eq!(sched.preemptions(), 0);
    }

    type Checked = ExecWith<eml_rt::Checked, crate::Sequential>;

    /// タスクの上限のテストの深さの上限。
    const DEPTH: usize = 4096;

    /// `text` を読み込み、深さの上限が `DEPTH` の `Ctx` と逐次の `Scheduler` を用意する。
    fn limit_fixture(
        text: &str,
    ) -> (
        CodeBase<eml_rt::Checked>,
        Ctx<eml_rt::Checked>,
        Scheduler<crate::Sequential>,
    ) {
        limit_fixture_with(text, crate::Sequential)
    }

    /// `limit_fixture` の、方針を選べる形。
    fn limit_fixture_with<P: Policy>(
        text: &str,
        policy: P,
    ) -> (
        CodeBase<eml_rt::Checked>,
        Ctx<eml_rt::Checked>,
        Scheduler<P>,
    ) {
        let limits = eml_rt::Limits {
            max_depth: DEPTH,
            ..eml_rt::Limits::default()
        };
        fixture_with_limits(text, policy, limits)
    }

    /// `limit_fixture_with` の、上限を選べる形。
    fn fixture_with_limits<P: Policy>(
        text: &str,
        policy: P,
        limits: eml_rt::Limits,
    ) -> (
        CodeBase<eml_rt::Checked>,
        Ctx<eml_rt::Checked>,
        Scheduler<P>,
    ) {
        let program = eml_lir::parse(text).unwrap_or_else(|error| panic!("{error}"));
        let control = Arc::new(Control::new(limits));
        let mut code = CodeBase::<eml_rt::Checked>::new(Arc::clone(&control));
        code.load(&program, &crate::Linked::default()).unwrap();
        let (host, _) = crate::capture_host(Default::default());
        let ctx = Ctx::new(Arc::clone(&control), host);
        let driver = eml_io::TestDriver::new(None);
        let clock = crate::driver::RunClock::Virtual(driver.clock());
        let sched = Scheduler::new(
            policy,
            Box::new(driver),
            Arc::new(crate::workers::Run::new(
                1,
                eml_sched::config::Seat::Merged,
                None,
                clock,
                Arc::default(),
            )),
            control,
            ctx.heap().worker(),
            DEPTH,
            Arc::new(Shared::new(Registry::new(Vec::new(), 1))),
        );
        (code, ctx, sched)
    }

    fn function(code: &CodeBase<eml_rt::Checked>, name: &str) -> u32 {
        code.index.functions[name]
    }

    const CALL: &str = "\
fn main() -> unit {
  let u.0: unit = call f(())
  return u.0
}
fn f(p.0: unit) -> unit {
  return p.0
}
";

    /// どの関数も、先頭の位置にエントリの行を持つ。RC の対象の引数を持たない関数と、verifier の拒む関数の行は空である
    /// (docs/spec/vm.md の「巻き戻しの表」)。
    #[test]
    fn every_function_has_an_entry_row_at_its_start() {
        let text = "\
fn main() -> unit {
  let n.0: int = call count(3)
  return ()
}
fn count(n.0: int) -> int {
  return n.0
}
fn keep(a.0: obj, n.1: int) -> unit {
  decref a.0
  return ()
}
fn leaks(a.0: obj) -> unit {
  return ()
}
";
        let program = eml_lir::parse(text).unwrap_or_else(|error| panic!("{error}"));
        assert!(eml_lir::verify(&program).is_err(), "`leaks` is rejected");
        let (code, _, _) = limit_fixture(text);
        for (name, slots) in [
            ("main", vec![]),
            ("count", vec![]),
            ("keep", vec![(0, 1)]),
            ("leaks", vec![]),
        ] {
            let start = code.functions[function(&code, name) as usize].start;
            let row = code.tables.unwind_at(start).expect(name);
            assert_ne!(row.entry, UnwindEntry::No, "{name}");
            assert_eq!(row.slots, slots, "{name}");
        }
    }

    /// タスクの外で深さの上限を超えたら、run を止める。
    #[test]
    fn a_depth_hit_outside_a_task_stops_the_run() {
        let (code, mut ctx, mut sched) = limit_fixture(CALL);
        let mut task = Task::new(&code, function(&code, "main"), DEPTH);
        task.pc = code.functions[function(&code, "f") as usize].start;
        ctx.set_depth_limit(0);
        let result = limit_hit::<Checked>(&code, &mut ctx, &mut task, &mut sched);
        assert!(
            matches!(result, Err(RunFault::Stopped(Stop::DepthLimit))),
            "{result:?}"
        );
    }

    /// 呼び出しの行の位置 (戻りの後) はエントリでないので、タスクの中の超過は保留のまま続ける。
    #[test]
    fn a_hit_after_a_return_stays_pending() {
        let (code, mut ctx, mut sched) = limit_fixture(CALL);
        let main = code.functions[function(&code, "main") as usize].start;
        let &(after_call, _) = code
            .tables
            .unwinds
            .iter()
            .find(|(pc, row)| *pc > main && row.entry == UnwindEntry::No)
            .unwrap();
        let mut task = Task::new(&code, function(&code, "main"), DEPTH);
        task.limit = Limit::Task;
        task.pc = after_call;
        ctx.set_depth_limit(0);
        let result = limit_hit::<Checked>(&code, &mut ctx, &mut task, &mut sched);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(task.limit, Limit::Task);
        assert_eq!(ctx.depth_limit(), 0);
    }

    /// 自己末尾呼び出しの先頭は関数のエントリなので起こそうとするが、IO の handler がなければ旗を捨てずに保留のまま
    /// 続ける。
    #[test]
    fn a_pending_task_without_an_io_handler_keeps_the_flag() {
        let text = "\
fn main() -> int {
  let n.0: int = call spin(3)
  return n.0
}
fn spin(n.0: int) -> int {
  switch n.0 { 0 -> b1, _ -> b2 }
b1:
  return 0
b2:
  let m.1: int = extern \"Prelude.Num Int.-\"(n.0, 1)
  tail call spin(m.1)
}
";
        let (code, mut ctx, mut sched) = limit_fixture(text);
        let spin = function(&code, "spin");
        assert!(
            code.tables
                .code
                .iter()
                .any(|op| matches!(op, Op::TailSelf { .. })),
            "`spin` jumps back to its start"
        );
        let mut task = Task::new(&code, spin, DEPTH);
        assert_eq!(task.pc, code.functions[spin as usize].start);
        task.limit = Limit::Pending;
        ctx.set_depth_limit(0);
        let result = limit_hit::<Checked>(&code, &mut ctx, &mut task, &mut sched);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(task.limit, Limit::Pending);
        assert_eq!(ctx.depth_limit(), 0);
    }

    /// `main` が `compose$` を、`compose$` が `prompt$` を呼ぶ。名前だけが EP の補助の関数のもので、本体は空に近い。
    const HELPERS: &str = "\
fn main() -> unit {
  let u.0: unit = call \"compose$\"(())
  return u.0
}
fn \"compose$\"(p.0: unit) -> unit {
  let u.1: unit = call \"prompt$\"(p.0)
  return u.1
}
fn \"prompt$\"(p.0: unit) -> unit {
  return p.0
}
";

    /// EP の補助の関数のエントリでは、旗でも本当の深さの超過でも起こさず、状態を変えずに保留のまま続ける。そこの `evv`
    /// はプログラムの地点の `evv` でないためである (docs/spec/vm.md の「タスクの上限」)。
    #[test]
    fn a_hit_at_a_helper_entry_stays_pending() {
        let (code, mut ctx, mut sched) = limit_fixture(HELPERS);
        for name in ["compose$", "prompt$"] {
            let mut task = Task::new(&code, function(&code, "main"), DEPTH);
            task.worker = ctx.heap().worker();
            task.pc = code.functions[function(&code, name) as usize].start;
            task.limit = Limit::Pending;
            ctx.set_depth_limit(0);
            let result = limit_hit::<Checked>(&code, &mut ctx, &mut task, &mut sched);
            assert!(result.is_ok(), "{name}: {result:?}");
            assert_eq!(task.limit, Limit::Pending, "{name}");
            assert_eq!(ctx.depth_limit(), 0, "{name}");
        }
        let mut task = Task::new(&code, function(&code, "main"), DEPTH);
        task.worker = ctx.heap().worker();
        task.pc = code.functions[function(&code, "compose$") as usize].start;
        task.limit = Limit::Task;
        // 起点を上げて本当の超過にする
        task.depth_offset = DEPTH - 1;
        assert!(task.stack_depth() > real_limit(&ctx, task.depth_offset));
        ctx.set_depth_limit(0);
        let result = limit_hit::<Checked>(&code, &mut ctx, &mut task, &mut sched);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(task.limit, Limit::Task);
        assert_eq!(ctx.depth_limit(), 0);
    }

    /// 補助の関数のエントリでも、深さが本当の限度と猶予の和を超えていれば run を止める。起こすのを延ばしても限度は
    /// なくならない。起こすエントリ (`main`) でも、起こさずに run を止める (docs/spec/vm.md の「タスクの上限」)。
    #[test]
    fn a_helper_entry_beyond_the_headroom_stops_the_run() {
        let (code, mut ctx, mut sched) = limit_fixture(HELPERS);
        for name in ["compose$", "main"] {
            for limit in [Limit::Task, Limit::Pending] {
                let mut task = Task::new(&code, function(&code, "main"), DEPTH);
                task.worker = ctx.heap().worker();
                task.pc = code.functions[function(&code, name) as usize].start;
                task.limit = limit;
                task.base = DEPTH + LIMIT_HEADROOM;
                ctx.set_depth_limit(0);
                let result = limit_hit::<Checked>(&code, &mut ctx, &mut task, &mut sched);
                assert!(
                    matches!(result, Err(RunFault::Stopped(Stop::DepthLimit))),
                    "{name}, {limit:?}: {result:?}"
                );
            }
        }
    }

    /// エントリの行が起こすのを延ばすのは、EP の補助の関数の6つの名前 (`mask$[..]` は項目の並びごとに1つ) だけである。
    /// 名前の似たほかの関数のエントリは起こす (docs/spec/vm.md の「巻き戻しの表」)。
    #[test]
    fn only_the_ep_helpers_defer() {
        let defers = [
            "id$",
            "compose$",
            "prompt$",
            "resume$",
            "rewind$",
            "mask$[State]",
            "mask$[Yield@1]",
        ];
        let raises = ["f", "$cont$@[X]", "fork$E.op", "compose$x"];
        let text: String = defers
            .iter()
            .chain(&raises)
            .map(|name| format!("fn \"{name}\"(p.0: unit) -> unit {{\n  return p.0\n}}\n"))
            .collect();
        let (code, _, _) = limit_fixture(&text);
        for (names, kind) in [
            (&defers[..], UnwindEntry::Defers),
            (&raises[..], UnwindEntry::Raises),
        ] {
            for name in names {
                let start = code.functions[function(&code, name) as usize].start;
                let row = code.tables.unwind_at(start).expect(name);
                assert_eq!(row.entry, kind, "{name}");
            }
        }
    }

    /// 猶予の間に深さの上限を超えたら、安全点が run を止める。
    #[test]
    fn a_depth_hit_while_aborting_stops_the_run() {
        let (code, mut ctx, mut sched) = limit_fixture(CALL);
        let mut task = Task::new(&code, function(&code, "main"), DEPTH);
        task.worker = ctx.heap().worker();
        task.pc = code.functions[function(&code, "f") as usize].start;
        task.limit = Limit::Aborting { pending: true };
        ctx.set_depth_limit(0);
        let result = safepoint::<Checked>(&code, &mut ctx, &mut task, &mut sched);
        assert!(
            matches!(result, Err(RunFault::Stopped(Stop::DepthLimit))),
            "{result:?}"
        );
    }

    /// `Panic` の `try_panic` と `prompt$` を evidence passing の出力の形で書いたプログラム。`boom` が `panic` する。
    const CAUGHT_PANIC: &str = "\
effect Prelude.Panic { never panicked(obj) -> never }
fn main() -> unit {
  let c.0: tobj = box 42
  let r.1: tobj = call try_panic(&boom, c.0)
  decref r.1
  return ()
}
fn boom(p.0: unit) -> tobj {
  let m.1: obj = const \"boom\"
  let r.2: tobj = extern Prelude.panic(m.1)
  return r.2
}
fn try_panic(f.0: tobj, c.1: tobj) -> tobj {
  let w.2: tobj = prim evv_get()
  dup w.2
  let ev.3: obj = prim ev_new[Prelude.Panic](c.1, w.2)
  let u.4: unit = prim ev_set_clause[Prelude.Panic.panicked](ev.3, &panicked$entry)
  dup ev.3
  let w.5: tobj = prim evv_push(w.2, ev.3)
  let u.6: unit = prim evv_set(w.5)
  let x.7: tobj poly = yields apply f.0(()) prompt ev.3
  tail call prompt$(ev.3, &return$, x.7)
}
clause Prelude.Panic.panicked fn panicked$entry(e.0: obj, m.1: obj) -> obj poly {
  decref m.1
  let u.2: unit = prim yield_abort(e.0, &panicked$clause)
  let d.3: obj poly = prim dummy()
  return d.3
}
fn panicked$clause(c.0: tobj) -> tobj {
  return c.0
}
yields fn prompt$(ev.0: obj, ret.1: tobj poly, x.2: tobj poly) -> tobj poly {
  let w.3: tobj = prim ev_take_evv(ev.0)
  let u.4: unit = prim evv_set(w.3)
  let s.5: int = prim yield_status(ev.0)
  switch s.5 { 0 -> b1, 1 -> b2, 2 -> b3, _ -> b4 }
b1:
  let st.6: tobj poly = prim ev_take_state(ev.0)
  decref ev.0
  tail yields apply ret.1(x.2, st.6)
b2:
  let u.8: unit = drop x.2
  let k.9: tobj poly = prim yield_cont_take()
  let c.10: tobj poly = prim yield_finish()
  let st.11: tobj poly = prim ev_take_state(ev.0)
  let k.12: tobj poly = closure resume$(ev.0, ret.1, k.9)
  tail yields apply c.10(k.12, st.11)
b3:
  let u.14: unit = drop ret.1
  let u.15: unit = drop x.2
  let c.16: tobj poly = prim yield_finish()
  let st.17: tobj poly = prim ev_take_state(ev.0)
  decref ev.0
  tail yields apply c.16(st.17)
b4:
  let u.19: unit = drop x.2
  let st.20: tobj poly = prim ev_take_state(ev.0)
  let k.21: tobj poly = prim yield_cont_take()
  let k.22: tobj poly = closure rewind$(ev.0, ret.1, st.20, k.21)
  let u.23: unit = prim yield_cont_set(k.22)
  let d.24: obj poly = prim dummy()
  return d.24
}
yields fn resume$(ev.0: obj, ret.1: tobj poly, k.2: tobj poly, v.3: tobj poly, s.4: tobj poly) -> tobj poly {
  let e.5: obj = prim ev_renew(ev.0, s.4)
  decref ev.0
  let w.6: tobj = prim evv_get()
  dup e.5
  let w.7: tobj = prim evv_push(w.6, e.5)
  let u.8: unit = prim evv_set(w.7)
  let x.9: tobj poly = yields apply k.2(v.3) prompt e.5
  tail call prompt$(e.5, ret.1, x.9)
}
yields fn rewind$(ev.0: obj, ret.1: tobj poly, s.2: tobj poly, k.3: tobj poly, v.4: tobj poly) -> tobj poly {
  tail call resume$(ev.0, ret.1, k.3, v.4, s.2)
}
fn return$(r.0: tobj, s.1: unit) -> tobj {
  return r.0
}
";

    /// 保留のタスクの `Panic` の中断は猶予に替わり、着地すると保留と毒の上限に戻る。
    #[test]
    fn a_pending_task_returns_to_pending_after_a_panic_lands() {
        let program = eml_lir::parse(CAUGHT_PANIC).unwrap_or_else(|error| panic!("{error}"));
        eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
        let (code, mut ctx, mut sched) = limit_fixture(CAUGHT_PANIC);
        let mut task = Task::new(&code, function(&code, "main"), DEPTH);
        task.worker = ctx.heap().worker();
        task.limit = Limit::Task;
        let mut counts = crate::RunCounts::default();
        let mut step_until = |task: &mut Task,
                              ctx: &mut Ctx<eml_rt::Checked>,
                              done: &dyn Fn(&Task) -> bool| {
            for _ in 0..1000 {
                if done(task) {
                    return;
                }
                let pc = task.pc;
                if let Err(fault) = step::<Checked>(&code, ctx, task, &mut sched, &mut counts, pc) {
                    panic!("{fault:?}");
                }
            }
            panic!("the program does not reach the expected state");
        };
        let boom = code.functions[function(&code, "boom") as usize].start;
        step_until(&mut task, &mut ctx, &|task| task.pc == boom);
        task.limit = Limit::Pending;
        ctx.set_depth_limit(0);
        step_until(&mut task, &mut ctx, &|task| task.limit != Limit::Pending);
        assert_eq!(task.limit, Limit::Aborting { pending: true });
        assert_eq!(ctx.depth_limit(), DEPTH + LIMIT_HEADROOM);
        step_until(&mut task, &mut ctx, &|task| {
            !matches!(task.limit, Limit::Aborting { .. })
        });
        assert_eq!(task.limit, Limit::Pending);
        assert_eq!(ctx.depth_limit(), 0);
    }

    const CAUGHT_IO: &str = "\
effect Prelude.IO { never io_failed(enum, obj) -> never }
fn main() -> unit {
  let c.0: tobj = box 42
  let r.1: tobj = call try_io(&boom, c.0)
  decref r.1
  return ()
}
fn boom(p.0: unit) -> tobj {
  let r.1: tobj = call leaf(())
  return r.1
}
fn leaf(p.0: unit) -> tobj {
  let b.1: tobj = box 1
  return b.1
}
fn try_io(f.0: tobj, c.1: tobj) -> tobj {
  let w.2: tobj = prim evv_get()
  dup w.2
  let ev.3: obj = prim ev_new[Prelude.IO](c.1, w.2)
  let u.4: unit = prim ev_set_clause[Prelude.IO.io_failed](ev.3, &io_failed$entry)
  dup ev.3
  let w.5: tobj = prim evv_push(w.2, ev.3)
  let u.6: unit = prim evv_set(w.5)
  let x.7: tobj poly = yields apply f.0(()) prompt ev.3
  tail call prompt$(ev.3, &return$, x.7)
}
clause Prelude.IO.io_failed fn io_failed$entry(e.0: obj, k.1: enum, m.2: obj) -> obj poly {
  decref m.2
  let u.3: unit = prim yield_abort(e.0, &io_failed$clause)
  let d.4: obj poly = prim dummy()
  return d.4
}
fn io_failed$clause(c.0: tobj) -> tobj {
  return c.0
}
yields fn prompt$(ev.0: obj, ret.1: tobj poly, x.2: tobj poly) -> tobj poly {
  let w.3: tobj = prim ev_take_evv(ev.0)
  let u.4: unit = prim evv_set(w.3)
  let s.5: int = prim yield_status(ev.0)
  switch s.5 { 0 -> b1, 1 -> b2, 2 -> b3, _ -> b4 }
b1:
  let st.6: tobj poly = prim ev_take_state(ev.0)
  decref ev.0
  tail yields apply ret.1(x.2, st.6)
b2:
  let u.8: unit = drop x.2
  let k.9: tobj poly = prim yield_cont_take()
  let c.10: tobj poly = prim yield_finish()
  let st.11: tobj poly = prim ev_take_state(ev.0)
  let k.12: tobj poly = closure resume$(ev.0, ret.1, k.9)
  tail yields apply c.10(k.12, st.11)
b3:
  let u.14: unit = drop ret.1
  let u.15: unit = drop x.2
  let c.16: tobj poly = prim yield_finish()
  let st.17: tobj poly = prim ev_take_state(ev.0)
  decref ev.0
  tail yields apply c.16(st.17)
b4:
  let u.19: unit = drop x.2
  let st.20: tobj poly = prim ev_take_state(ev.0)
  let k.21: tobj poly = prim yield_cont_take()
  let k.22: tobj poly = closure rewind$(ev.0, ret.1, st.20, k.21)
  let u.23: unit = prim yield_cont_set(k.22)
  let d.24: obj poly = prim dummy()
  return d.24
}
yields fn resume$(ev.0: obj, ret.1: tobj poly, k.2: tobj poly, v.3: tobj poly, s.4: tobj poly) -> tobj poly {
  let e.5: obj = prim ev_renew(ev.0, s.4)
  decref ev.0
  let w.6: tobj = prim evv_get()
  dup e.5
  let w.7: tobj = prim evv_push(w.6, e.5)
  let u.8: unit = prim evv_set(w.7)
  let x.9: tobj poly = yields apply k.2(v.3) prompt e.5
  tail call prompt$(e.5, ret.1, x.9)
}
yields fn rewind$(ev.0: obj, ret.1: tobj poly, s.2: tobj poly, k.3: tobj poly, v.4: tobj poly) -> tobj poly {
  tail call resume$(ev.0, ret.1, k.3, v.4, s.2)
}
fn return$(r.0: tobj, s.1: unit) -> tobj {
  return r.0
}
";

    /// 本当の超過で `ResourceLimit` を起こしても、セルの数と一緒にタスクの数を 0 に戻し、区間の始めを進めて公開し直す。
    /// 旗で起こしたときと同じにして、セルとタスクの数をずらさない。
    #[test]
    fn a_raise_for_a_real_overrun_resets_the_count() {
        let program = eml_lir::parse(CAUGHT_IO).unwrap_or_else(|error| panic!("{error}"));
        eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
        let (code, mut ctx, mut sched) = limit_fixture(CAUGHT_IO);
        let cell = cells::task_on(&sched.cells, 0, 0, false);
        let mut task = Task::new(&code, function(&code, "main"), DEPTH);
        task.worker = ctx.heap().worker();
        task.cell = Some(cell);
        task.limit = Limit::Task;
        let mut counts = crate::RunCounts::default();
        let leaf = code.functions[function(&code, "leaf") as usize].start;
        for _ in 0..1000 {
            if task.pc == leaf {
                break;
            }
            let pc = task.pc;
            if let Err(fault) =
                step::<Checked>(&code, &mut ctx, &mut task, &mut sched, &mut counts, pc)
            {
                panic!("{fault:?}");
            }
        }
        assert_eq!(task.pc, leaf, "the program does not reach `leaf`");
        take_pages(&mut ctx);
        task.counted = 700;
        task.added = 70;
        sched.mark = 0;
        sched.publish(&task);
        // セルに旗が立っていても `Task` にはまだ届いていない。起こすとこの旗も消費する
        assert_eq!(cells::limit_task(&sched.cells, cell), Some(cell));
        // 起点を上げて本当の超過にする
        task.depth_offset = DEPTH - 1;
        limit_hit::<Checked>(&code, &mut ctx, &mut task, &mut sched).unwrap();
        assert_eq!(task.limit, Limit::Aborting { pending: false });
        let now = ctx.heap().control().taken(0);
        assert_eq!((task.counted, task.added, sched.mark), (0, 0, now));
        let slot = &sched.run.slots[0];
        assert_eq!(
            (
                slot.current.load(Ordering::Relaxed),
                slot.bytes.load(Ordering::Relaxed),
                slot.mark.load(Ordering::Relaxed),
            ),
            (cell_word(cell), 0, now)
        );
    }

    /// 眠った後で同じタスクが動き続けるときは、区間を閉じて公開し直す。眠る前に `current` を消したためである。
    #[test]
    fn a_task_that_keeps_running_after_a_sleep_is_published_again() {
        let (code, mut ctx, mut sched) = limit_fixture(CALL);
        let cell = cells::task_on(&sched.cells, 0, 0, false);
        let mut task = Task::new(&code, function(&code, "main"), DEPTH);
        task.cell = Some(cell);
        task.counted = 40;
        sched.mark = ctx.heap().control().taken(0);
        sched.publish(&task);
        sched.publish_idle();
        assert_eq!(sched.run.slots[0].current.load(Ordering::Relaxed), NO_CELL);
        let grown = take_pages(&mut ctx);
        sched.keep_running(&mut task);
        let slot = &sched.run.slots[0];
        assert_eq!(
            (
                slot.current.load(Ordering::Relaxed),
                slot.bytes.load(Ordering::Relaxed),
                slot.mark.load(Ordering::Relaxed),
            ),
            (cell_word(cell), 40 + grown, ctx.heap().control().taken(0))
        );
        assert_eq!(slot.seq.load(Ordering::Relaxed) % 2, 0);
    }

    /// 柔らかい上限 64 KiB、硬い上限 64 MiB の、逐次の方針の用意。
    fn soft_fixture() -> (
        CodeBase<eml_rt::Checked>,
        Ctx<eml_rt::Checked>,
        Scheduler<crate::Sequential>,
    ) {
        let limits = eml_rt::Limits {
            max_depth: DEPTH,
            max_bytes: 64 << 20,
            soft_bytes: 64 << 10,
            ..eml_rt::Limits::default()
        };
        fixture_with_limits(CALL, crate::Sequential, limits)
    }

    /// 決定的な方針では、集める地点が柔らかい上限を見て、公開された今のタスクに旗を立てる。旗の項は同じ集める地点が
    /// 取り、今のタスクを保留にする。閾値は進む。
    #[test]
    fn a_gathered_policy_flags_in_gather() {
        let (code, mut ctx, mut sched) = soft_fixture();
        let control = Arc::clone(ctx.heap().control());
        let cell = cells::task_on(&sched.cells, 0, 0, false);
        let mut task = Task::new(&code, function(&code, "main"), DEPTH);
        task.worker = ctx.heap().worker();
        task.cell = Some(cell);
        task.limit = Limit::Task;
        sched.mark = control.taken(0);
        sched.publish(&task);
        let held = ctx.fresh_string(&"x".repeat(200 << 10));
        assert!(control.pressure().is_some());
        collect::<Checked>(&mut ctx, &mut task, &mut sched, false).unwrap();
        assert_eq!(task.limit, Limit::Pending);
        assert_eq!(ctx.depth_limit(), 0);
        assert!(control.pressure().is_none(), "the threshold does not move");
        assert!(sched.cells.mail.take(0).is_empty());
        unsafe { ctx.heap_mut().decref(held) }.unwrap();
    }

    /// 待ちに入ったエントリの `Task` は、`Finish` の切り替えの前の集める地点で候補にならない。区間を閉じた直後に公開を
    /// 外すためである。候補がないので旗は立たず、閾値は進む。
    #[test]
    fn an_entry_that_waits_is_not_chosen_in_the_gather_of_its_switch() {
        let (code, mut ctx, mut sched) = soft_fixture();
        let control = Arc::clone(ctx.heap().control());
        let cell = cells::placed_task(&sched.cells, 0);
        sched.cells.lock().started(cell).unwrap();
        let mut task = Task::new(&code, function(&code, "main"), DEPTH);
        task.worker = ctx.heap().worker();
        task.cell = Some(cell);
        task.limit = Limit::Task;
        let mut other = Task::new(&code, function(&code, "main"), DEPTH);
        other.worker = ctx.heap().worker();
        other.state = TaskState::Ready;
        let other_id = sched.add_task(other);
        sched.run.push_ready(0, Ready::Task(other_id));
        sched.mark = control.taken(0);
        sched.publish(&task);
        let held = ctx.fresh_string(&"x".repeat(200 << 10));
        assert!(control.pressure().is_some());
        entry_finish::<Checked>(&code, &mut ctx, &mut task, &mut sched).unwrap();
        assert!(
            task.cell.is_none(),
            "the switch does not reach the other task"
        );
        // 旗の項は同じ集める地点の mail の処理が取るので、mail と `limited` は選ばれても空になる。セルの旗で見る
        assert!(
            !cells::limit_flag(&sched.cells, cell),
            "a waiting task is chosen"
        );
        assert_eq!(sched.run.slots[0].current.load(Ordering::Relaxed), NO_CELL);
        unsafe { ctx.heap_mut().decref(held) }.unwrap();
    }

    /// 左の子が終わって join で右の子を待つ `par` の親は、待ちに入る切り替えの集める地点で候補にならない。待ちに入る
    /// 直前に公開を外すためである。閾値を越えたまま集める地点に来ても、旗は立たない。
    #[test]
    fn a_parent_waiting_on_its_join_is_not_chosen_in_the_gather_of_its_switch() {
        let (code, mut ctx, mut sched) = soft_fixture();
        let control = Arc::clone(ctx.heap().control());
        let cell = cells::placed_task(&sched.cells, 0);
        sched.cells.lock().started(cell).unwrap();
        let mut task = Task::new(&code, function(&code, "main"), DEPTH);
        task.worker = ctx.heap().worker();
        task.cell = Some(cell);
        task.limit = Limit::Task;
        let mut right = Task::new(&code, function(&code, "main"), DEPTH);
        right.worker = ctx.heap().worker();
        right.state = TaskState::Ready;
        let right = sched.add_task(right);
        let j = sched.add_join(JoinNode {
            parent: sched.current,
            right,
            right_worker: ctx.heap().worker(),
            parent_evv: Word::UNIT,
            left: None,
            right_outcome: None,
            ret: task.pc,
            dst: 0,
            orphan: false,
        });
        sched.run.push_ready(0, Ready::Task(right));
        let base = task.push_frame(task.pc, 0, BOUNDARY_SIZE);
        task.stack[base + 1] = Word::from_bits(j);
        task.boundaries.push(j);
        sched.mark = control.taken(0);
        sched.publish(&task);
        let held = ctx.fresh_string(&"x".repeat(200 << 10));
        assert!(control.pressure().is_some());
        child_done::<Checked>(
            &code,
            &mut ctx,
            &mut task,
            &mut sched,
            Outcome::Value(Word::imm(1)),
        )
        .unwrap();
        assert!(
            task.cell.is_none(),
            "the switch does not reach the right child"
        );
        assert!(
            !cells::limit_flag(&sched.cells, cell),
            "a waiting parent is chosen"
        );
        unsafe { ctx.heap_mut().decref(held) }.unwrap();
    }

    /// `par` の右の子は、タスクの外の親からは `Stops` を、タスクの中の親からは保留も猶予もない `Task` を受け継ぐ。
    #[test]
    fn a_right_child_inherits_the_kind_of_its_parent() {
        for (parent, child) in [
            (Limit::Stops, Limit::Stops),
            (Limit::Task, Limit::Task),
            (Limit::Pending, Limit::Task),
            (Limit::Aborting { pending: true }, Limit::Task),
            (Limit::Aborting { pending: false }, Limit::Task),
        ] {
            let right = Task::right_child(
                0,
                0,
                Word::UNIT,
                0,
                DEPTH,
                JOB,
                Word::UNIT,
                parent.inherit(),
            );
            assert_eq!(right.limit, child, "{parent:?}");
        }
    }

    /// 保留のタスクの安全点で `NOTICE` も立っていれば、上限を扱う前に知らせを集める。`poll` し直す前に下ろしたビットの
    /// 知らせを捨てないためである。聞く者のいないシグナルを集めて run を止めることで、集めたことを確かめる。
    #[test]
    fn a_pending_task_collects_a_notice_at_its_safepoint() {
        let (code, mut ctx, mut sched) = limit_fixture(CALL);
        let control = Arc::clone(ctx.heap().control());
        let main = code.functions[function(&code, "main") as usize].start;
        let &(after_call, _) = code
            .tables
            .unwinds
            .iter()
            .find(|(pc, row)| *pc > main && row.entry == UnwindEntry::No)
            .unwrap();
        let mut task = Task::new(&code, function(&code, "main"), DEPTH);
        task.worker = ctx.heap().worker();
        task.limit = Limit::Pending;
        task.pc = after_call;
        ctx.set_depth_limit(0);
        control.defer(Control::TERMINATE);
        assert_eq!(control.claim_signals(), Control::TERMINATE);
        sched.cells.mail.push(MailItem::Signal(Control::TERMINATE));
        control.notice(0);
        let result = safepoint::<Checked>(&code, &mut ctx, &mut task, &mut sched);
        assert!(result.is_err(), "the unheard signal stops the run");
        assert_eq!(control.pending_signals(), 0);
        assert_eq!(control.worker_bits(0) & Control::NOTICE, 0);
    }

    /// 予算で横取りする方針は、深さの上限を超えたままの安全点でも予算を数え、使い切れば横取りを求める。
    #[test]
    fn a_pending_task_still_spends_the_preemption_budget() {
        let (code, mut ctx, mut sched) = limit_fixture_with(CALL, crate::Seeded::new(1));
        let main = code.functions[function(&code, "main") as usize].start;
        let &(after_call, _) = code
            .tables
            .unwinds
            .iter()
            .find(|(pc, row)| *pc > main && row.entry == UnwindEntry::No)
            .unwrap();
        let mut task = Task::new(&code, function(&code, "main"), DEPTH);
        task.worker = ctx.heap().worker();
        task.limit = Limit::Pending;
        task.pc = after_call;
        ctx.set_depth_limit(0);
        type Seeded = ExecWith<eml_rt::Checked, crate::Seeded>;
        let switches = (0..16)
            .map(|_| {
                limit_safepoint::<Seeded>(&code, &mut ctx, &mut task, &mut sched, false).unwrap()
            })
            .position(|switch| switch);
        assert!(
            switches.is_some(),
            "the budget of at most 16 safepoints runs out"
        );
        assert_eq!(task.limit, Limit::Pending);
    }

    /// 旗を受けると、`Task` は保留になり、中断の間は着地の後に保留へ戻す印だけが付いて猶予の上限は変わらない。
    #[test]
    fn mark_pending_keeps_the_abort_bracket() {
        let (code, mut ctx, _) = limit_fixture(CALL);
        let mut task = Task::new(&code, function(&code, "main"), DEPTH);
        for (before, after, limit) in [
            (Limit::Task, Limit::Pending, 0),
            (Limit::Pending, Limit::Pending, 0),
            (
                Limit::Aborting { pending: false },
                Limit::Aborting { pending: true },
                DEPTH + LIMIT_HEADROOM,
            ),
            (
                Limit::Aborting { pending: true },
                Limit::Aborting { pending: true },
                DEPTH + LIMIT_HEADROOM,
            ),
            (Limit::Stops, Limit::Stops, DEPTH),
        ] {
            task.limit = before;
            ctx.set_depth_limit(DEPTH);
            apply_limit(&mut ctx, before, 0);
            mark_pending(&mut ctx, &mut task);
            assert_eq!(task.limit, after, "{before:?}");
            assert_eq!(ctx.depth_limit(), limit, "{before:?}");
        }
    }

    /// 集める地点が mail の上限の旗を受けると、表に退いたタスクは状態だけが保留になり、`Ctx` の上限は今のタスクの
    /// ままである。今のタスクの旗では、今のタスクが保留になって上限が 0 になる。
    #[test]
    fn collect_marks_the_task_of_a_limited_cell() {
        let (code, mut ctx, mut sched) = limit_fixture(CALL);
        let current = cells::task_on(&sched.cells, 0, 0, false);
        let waiting = cells::task_on(&sched.cells, 0, 0, false);
        let mut task = Task::new(&code, function(&code, "main"), DEPTH);
        task.worker = ctx.heap().worker();
        task.cell = Some(current);
        task.limit = Limit::Task;
        let mut other = Task::new(&code, function(&code, "main"), DEPTH);
        other.cell = Some(waiting);
        other.limit = Limit::Task;
        let id = sched.add_task(other);
        ctx.set_depth_limit(DEPTH);
        sched.cells.mail.push(MailItem::Limit(waiting));
        collect::<Checked>(&mut ctx, &mut task, &mut sched, false).unwrap();
        assert_eq!(sched.tasks[id].as_ref().unwrap().limit, Limit::Pending);
        assert_eq!(task.limit, Limit::Task);
        assert_eq!(ctx.depth_limit(), DEPTH);
        sched.cells.mail.push(MailItem::Limit(current));
        collect::<Checked>(&mut ctx, &mut task, &mut sched, false).unwrap();
        assert_eq!(task.limit, Limit::Pending);
        assert_eq!(ctx.depth_limit(), 0);
        assert!(sched.limited.is_empty());
    }

    /// 同じセルの `Task` が今のタスクと表の両方にあれば、どちらにも旗を付ける。待ちに入った後で `Finish` へ戻る前の
    /// エントリの `Task` と次のエントリの `Task` が並びうるためである。上限を 0 にするのは今のタスクの分だけである。
    #[test]
    fn collect_marks_every_task_of_the_cell() {
        let (code, mut ctx, mut sched) = limit_fixture(CALL);
        let cell = cells::task_on(&sched.cells, 0, 0, false);
        let mut task = Task::new(&code, function(&code, "main"), DEPTH);
        task.worker = ctx.heap().worker();
        task.cell = Some(cell);
        task.limit = Limit::Task;
        let mut other = Task::new(&code, function(&code, "main"), DEPTH);
        other.cell = Some(cell);
        other.limit = Limit::Aborting { pending: false };
        let id = sched.add_task(other);
        ctx.set_depth_limit(DEPTH);
        sched.cells.mail.push(MailItem::Limit(cell));
        collect::<Checked>(&mut ctx, &mut task, &mut sched, false).unwrap();
        assert_eq!(task.limit, Limit::Pending);
        assert_eq!(
            sched.tasks[id].as_ref().unwrap().limit,
            Limit::Aborting { pending: true }
        );
        assert_eq!(ctx.depth_limit(), 0);
    }

    /// ほかの worker が回した `Notice::Limit` は、集める地点が受け箱から取り、旗がまだ立っていれば今のタスクに付ける。
    /// 届く前に旗が下りていれば付けない。
    #[test]
    fn collect_marks_the_task_of_a_forwarded_limit_only_while_flagged() {
        let (code, mut ctx, mut sched) = limit_fixture(CALL);
        let control = Arc::clone(ctx.heap().control());
        let cell = cells::task_on(&sched.cells, 0, 0, false);
        let mut task = Task::new(&code, function(&code, "main"), DEPTH);
        task.worker = ctx.heap().worker();
        task.cell = Some(cell);
        task.limit = Limit::Task;
        ctx.set_depth_limit(DEPTH);
        assert_eq!(cells::limit_task(&sched.cells, cell), Some(cell));
        sched.cells.lock().clear_limit(cell).unwrap();
        sched.run.send(&control, 0, Notice::Limit(cell));
        collect::<Checked>(&mut ctx, &mut task, &mut sched, false).unwrap();
        assert_eq!(task.limit, Limit::Task);
        assert_eq!(ctx.depth_limit(), DEPTH);
        assert_eq!(cells::limit_task(&sched.cells, cell), Some(cell));
        sched.run.send(&control, 0, Notice::Limit(cell));
        collect::<Checked>(&mut ctx, &mut task, &mut sched, false).unwrap();
        assert_eq!(task.limit, Limit::Pending);
        assert_eq!(ctx.depth_limit(), 0);
    }

    /// `Stops` でないタスクのスタックの容量の上限は、中断の間の猶予も覆う。
    #[test]
    fn a_task_stack_limit_covers_the_headroom() {
        assert_eq!(task_stack_limit(1000, Limit::Stops), stack_limit(1000));
        assert_eq!(
            task_stack_limit(1000, Limit::Task),
            stack_limit(1000) + LIMIT_HEADROOM
        );
    }

    #[test]
    fn the_stack_limit_is_the_depth_limit_and_one_frame() {
        assert_eq!(stack_limit(1000), 1000 + 65535 + 2);
        assert_eq!(stack_limit(usize::MAX), usize::MAX);
    }

    /// 深さの上限で止まる実行でも、スタックの容量は上限と1つの枠の分を超えない。
    #[test]
    fn reserve_doubles_the_capacity_up_to_the_stack_limit() {
        let mut task = empty_task(0);
        let limit = task.stack_limit;
        task.reserve(30_000);
        assert_eq!(task.stack.capacity(), 30_000);
        task.reserve(30_001);
        assert_eq!(task.stack.capacity(), 60_000);
        task.reserve(60_001);
        assert_eq!((task.stack.len(), task.stack.capacity()), (60_001, limit));
        task.reserve(limit);
        assert_eq!((task.stack.len(), task.stack.capacity()), (limit, limit));
    }

    /// 上限を超える長さを求められたら、容量をその長さちょうどにする。`ApplyRest` の枠と呼ばれる側の枠を、同じ安全点の
    /// 前に積むときに起きる。
    #[test]
    fn reserve_beyond_the_stack_limit_takes_exactly_the_length() {
        let mut task = empty_task(0);
        let end = task.stack_limit + 100;
        task.reserve(end);
        assert_eq!((task.stack.len(), task.stack.capacity()), (end, end));
    }

    /// worker 0 のヒープにページを取らせ、`Control::taken` が増えた分を返す。
    fn take_pages(ctx: &mut Ctx<eml_rt::Checked>) -> usize {
        let before = ctx.heap().control().taken(0);
        let word = ctx.fresh_string(&"x".repeat(1 << 17));
        unsafe { ctx.heap_mut().decref(word) }.unwrap();
        let grown = ctx.heap().control().taken(0) - before;
        assert!(grown > 0, "a large string takes no page");
        grown
    }

    /// `end_slice` は区間の始めから取ったページの分を `Async` のタスクの `counted` と `added` に足し、区間の始めを
    /// 進める。セルを持たないタスクには数えない。
    #[test]
    fn end_slice_adds_the_slice_to_counted_and_added() {
        let (code, mut ctx, mut sched) = limit_fixture(CALL);
        let cell = cells::task_on(&sched.cells, 0, 0, false);
        let mut task = Task::new(&code, function(&code, "main"), DEPTH);
        task.cell = Some(cell);
        task.counted = 1000;
        sched.mark = ctx.heap().control().taken(0);
        let grown = take_pages(&mut ctx);
        sched.end_slice(&mut task);
        assert_eq!((task.counted, task.added), (1000 + grown, grown));
        assert_eq!(sched.mark, ctx.heap().control().taken(0));
        let mut outside = Task::new(&code, function(&code, "main"), DEPTH);
        take_pages(&mut ctx);
        sched.end_slice(&mut outside);
        assert_eq!((outside.counted, outside.added), (0, 0));
        assert_eq!(sched.mark, ctx.heap().control().taken(0));
    }

    /// フレームを持ったまま横取りされたタスクはその数で `top` に出し、入るタスクは `current` に公開する。横取りされた
    /// タスクに戻ると、`current` がそのタスクになり、`top` から消える。
    #[test]
    fn a_preempted_task_is_offered_as_top_and_cleared_when_resumed() {
        let (code, mut ctx, mut sched) = limit_fixture(CALL);
        let slot = |sched: &Scheduler<crate::Sequential>| {
            let slot = &sched.run.slots[0];
            (
                slot.current.load(Ordering::Relaxed),
                slot.bytes.load(Ordering::Relaxed),
                slot.top.load(Ordering::Relaxed),
                slot.top_bytes.load(Ordering::Relaxed),
            )
        };
        let cell = cells::task_on(&sched.cells, 0, 0, false);
        let mut task = Task::new(&code, function(&code, "main"), DEPTH);
        task.worker = ctx.heap().worker();
        task.cell = Some(cell);
        task.counted = 500;
        sched.mark = ctx.heap().control().taken(0);
        sched.publish(&task);
        assert_eq!(slot(&sched), (cell_word(cell), 500, NO_CELL, 0));
        let mut other = Task::new(&code, function(&code, "main"), DEPTH);
        other.worker = ctx.heap().worker();
        other.state = TaskState::Ready;
        let other_id = sched.add_task(other);
        let grown = take_pages(&mut ctx);
        let mine = sched.current;
        resume::<Checked>(
            &code,
            &mut ctx,
            &mut task,
            &mut sched,
            other_id,
            Some(TaskState::Ready),
        )
        .unwrap();
        assert_eq!(slot(&sched), (NO_CELL, 500, cell_word(cell), 500 + grown));
        resume::<Checked>(
            &code,
            &mut ctx,
            &mut task,
            &mut sched,
            mine,
            Some(TaskState::Ready),
        )
        .unwrap();
        assert_eq!(task.cell, Some(cell));
        assert_eq!(
            slot(&sched),
            (cell_word(cell), 500 + grown, NO_CELL, 500 + grown)
        );
    }

    /// 自分の再開を積んだタスクは今の数で`top`に出し、待ちに入っても残す。本当に待つタスクは`top`から消す。
    /// どちらもこのエントリの数を`added`に積む。
    #[test]
    fn a_self_yield_offers_top_and_a_real_wait_clears_it() {
        let (code, mut ctx, mut sched) = limit_fixture(CALL);
        let cell = cells::task_on(&sched.cells, 0, 0, false);
        let mut task = Task::new(&code, function(&code, "main"), DEPTH);
        task.cell = Some(cell);
        task.counted = 300;
        sched.mark = ctx.heap().control().taken(0);
        sched.publish(&task);
        let grown = take_pages(&mut ctx);
        let top = |sched: &Scheduler<crate::Sequential>| {
            let slot = &sched.run.slots[0];
            (
                slot.top.load(Ordering::Relaxed),
                slot.top_bytes.load(Ordering::Relaxed),
            )
        };
        let wake = Runnable::Resume {
            cell,
            worker: 0,
            k: Word::UNIT,
            wake: cells::WOKE,
        };
        schedule(&mut sched, wake).unwrap();
        assert!(sched.yielded);
        assert_eq!(top(&sched), (cell_word(cell), 300 + grown));
        sched.end_slice(&mut task);
        sched.left_entry(cell, task.added);
        assert_eq!(top(&sched), (cell_word(cell), 300 + grown));
        // 次の区間で本当に待つ
        sched.yielded = false;
        sched.offer_top(cell, 300 + grown);
        task.added = 0;
        sched.end_slice(&mut task);
        sched.left_entry(cell, task.added);
        assert_eq!(top(&sched).0, NO_CELL);
        assert_eq!(sched.added, [(cell, grown), (cell, 0)]);
        // ほかのセルの再開は今のタスクの譲りでない
        let other = cells::task_on(&sched.cells, 0, 0, false);
        let wake = Runnable::Resume {
            cell: other,
            worker: 0,
            k: Word::UNIT,
            wake: cells::WOKE,
        };
        schedule(&mut sched, wake).unwrap();
        assert!(!sched.yielded);
        assert_eq!(top(&sched).0, NO_CELL);
        sched.added.clear();
    }
}
