//! 命令を実行するループ (docs/spec/vm.md)。状態は `Ctx`、`Task`、`Scheduler` だけに置き、安全点をまたいで外に
//! 持たない。呼び出しのスタックを Rust の再帰でなく `Task` のデータに持つので、安全点でタスクごとに取り替えられる
//! (docs/future/multicore.md の「VM の並列化の備え」の制約5)。

use std::marker::PhantomData;

use eml_extern::{Extern, ExternEffect, Purity};
use eml_rt::externs::{self, Fault, Raised};
use eml_rt::{
    Ctx, Form, Handle, MaskItem, Mode, Open, ReadAmount, Stop, Stream, Token, Word, Write,
    WriteMode,
};

use crate::apply;
use crate::cells::{self, CellRef, Raise, Runnable, Take, WaitToken};
use crate::code::{AsyncEntries, CodeBase, ForkTable};
use crate::data;
use crate::error::VmFault;
use crate::op::{APPLY_REST, CHILD_DONE, FINISH, Op, Pc, Slot};
use crate::sched::{JoinId, JoinNode, MAIN, Outcome, Policy, Scheduler, TaskId, TaskState};
use crate::{RunCounts, RuntimeError};

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
    /// 入口の関数の番号。どの関数にも属さない命令の誤りの名前と、`Finish` が結果を手放すかを決める。
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
    /// fork のときの親の深さ。右の子の深さはここから数える。入口のタスクは 0 である。
    pub(crate) depth_offset: usize,
    /// 開いている子の境目の join。内側が最後に来る。右の子の先頭は、自分が右の子である join である。
    pub(crate) boundaries: Vec<JoinId>,
    pub(crate) state: TaskState,
    /// `Async` のタスクのセル。入口の `Task` だけが持ち、`main` と `par` の右の子は `None` である。`resume_task` の
    /// 入口の `Task` は、待っていたタスクのセルを持つ (docs/spec/vm.md の「`Async` のタスク」)。
    pub(crate) cell: Option<CellRef>,
    /// `finish_*` が預けたタスクの終わり方。入口の `Task` の `Finish` が核の `finish` に渡す。タスクの ID の物体が
    /// すべて片付く前に待ち手を起こさないためである (docs/spec/vm.md の「`Async` のタスク」)。
    pub(crate) outcome: Option<cells::Outcome>,
}

/// 誤りと、それが起きた命令の位置。
pub(crate) struct Failure {
    pub(crate) fault: VmFault,
    pub(crate) pc: Pc,
}

// いちばん下に置く空のフレーム。入口の関数の結果を受けるスロットを1つ持つ
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
        }
    }

    /// `Async` のタスクの入口の `Task`。`Task::new` と同じ根のフレームを作り、スタックは集まりから取った `stack` を
    /// 使う。引数は動き始めるときに入口のフレームに書く (`resume`)。深さは 0 から数え、タスクごとに `max_depth` の
    /// すべてを使う (docs/spec/vm.md の「`Async` のタスク」)。
    #[expect(
        clippy::too_many_arguments,
        reason = "the parts of a task an entry starts from"
    )]
    fn entry<M: Mode>(
        code: &CodeBase<M>,
        function: u32,
        args: [Word; 2],
        argc: u8,
        cell: CellRef,
        worker: u32,
        max_depth: usize,
        mut stack: Vec<Word>,
    ) -> Task {
        let entry = &code.functions[function as usize];
        let base = ROOT_BASE + ROOT_SIZE + 2;
        stack.clear();
        stack.resize(base + usize::from(entry.frame_size), Word::UNIT);
        stack[base - 2] = header(FINISH, 0, ROOT_SIZE);
        stack[base - 1] = Word::from_bits(ROOT_BASE);
        Task {
            stack,
            pc: FINISH,
            base,
            frame_size: usize::from(entry.frame_size),
            last_ret: FINISH,
            scratch: Vec::new(),
            stack_limit: stack_limit(max_depth),
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
        }
    }

    /// まだ始まっていない右の子。いちばん下のフレームが子の境目のフレームで、始めるときに閉包 `g` をその上で呼ぶ。
    /// 境目のフレームの戻り先は `Finish` だが、`ChildDone` がフレームを外さずにタスクを捨てるので、戻ることはない
    /// (docs/spec/vm.md の「`par` の fork と join」)。
    fn right_child(
        entry: u32,
        worker: u32,
        evv: Word,
        depth_offset: usize,
        max_depth: usize,
        j: JoinId,
        g: Word,
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
            stack_limit: stack_limit(max_depth.saturating_sub(depth_offset)),
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

/// スタックの容量の上限。深さが `max_depth` を超えると次の安全点で止まり、安全点の前には通常、1つの枠と見出し
/// (`u16::MAX + 2` 語) までを積むので、それより大きい容量は見込まない。余る引数の適用は呼ばれる側の枠と同じ安全点の
/// 前に `ApplyRest` の枠も積むので、求める長さが上限を超えうる。`reserve` は求める長さを下回らないので、そのときも足りる
/// (docs/spec/vm.md の「スロットとスタック」)。
fn stack_limit(max_depth: usize) -> usize {
    max_depth.saturating_add(usize::from(u16::MAX) + 2)
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

/// `Ctx::poll` の VM での呼び方。切り替えたいなら `true` を返す。速い道は `Ok(())` の分岐だけで、残りは
/// `polled_stop` に置く (docs/spec/vm.md の「タスクとスケジュール」)。
#[inline(always)]
fn poll<M: Mode>(ctx: &Ctx<M>, depth: usize) -> Result<bool, VmFault> {
    match ctx.poll(depth) {
        Ok(()) => Ok(false),
        Err(stop) => polled_stop(ctx, depth, stop),
    }
}

/// `poll` が止める理由を返したときの扱い。`Preempt` 以外は誤りとして返す。`Preempt` は、yield の途中でなければ
/// ビットを下ろしてからもう一度 `poll` する。間に命令がないので、深さの上限は正確で、同時に立った止めるビットは
/// 切り替えより先に勝つ。
#[cold]
#[inline(never)]
fn polled_stop<M: Mode>(ctx: &Ctx<M>, depth: usize, stop: Stop) -> Result<bool, VmFault> {
    if stop != Stop::Preempt {
        return Err(stop.into());
    }
    // yield の途中では切り替えない。ビットは立てたままにして、次の yield のない安全点で拾う
    if ctx.yielding() != 0 {
        return Ok(false);
    }
    ctx.heap().control().take_preempt();
    match ctx.poll(depth) {
        Ok(()) | Err(Stop::Preempt) => Ok(true),
        Err(stop) => Err(stop.into()),
    }
}

/// 安全点 (docs/spec/vm.md の「タスクとスケジュール」)。止める理由と深さを確かめた後で、`PREEMPT` が立っていたか
/// 方針が求め、yield の途中でなければ横取りを試みる。yield の途中の状態は `Ctx` が1つだけ持つので、そこでは
/// 切り替えない。`tick` は予算を数える方針があるので、`PREEMPT` の有無によらず毎回呼ぶ。
#[inline(always)]
fn safepoint<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
) -> Result<(), VmFault> {
    let wants = poll(ctx, task.stack_depth())?;
    let tick = sched.policy.tick();
    if (wants || tick) && ctx.yielding() == 0 {
        preempt::<R>(code, ctx, task, sched)?;
    }
    Ok(())
}

/// タスクを実行する。`task` は入口のタスクで、切り替えるたびに動いているタスクと入れ替わる。成功して戻るときは入口の
/// タスクに戻っている (`Finish` が確かめる)。誤りで戻る前に、ヒープの worker と `Ctx` の深さの上限を入口のタスクのものに戻す。返る語は失敗した
/// タスクの `evv` で、手放さずに捨てる。誤りの後のヒープは回復で置き換えるか、`Ctx` ごと落とすので、捨てた語は漏れに
/// 数えない (docs/spec/vm.md の「`par` の fork と join」)。
pub(crate) fn run<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    counts: &mut RunCounts,
) -> Result<(), RuntimeError> {
    let result = run_tasks::<R>(code, ctx, task, sched, counts);
    if result.is_err() {
        // 検査モードの前提は `wf` と `()` だけで、誤りの後でも成り立つ
        let _abandoned = unsafe { ctx.switch_task(Word::UNIT, sched.main_worker, 0) };
    }
    result
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
) -> Result<bool, VmFault> {
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
            if sched.current != MAIN {
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
        Op::IntAdd { dst, a, b } => task.set_raw(dst, externs::int_add(task.raw(a), task.raw(b))?),
        Op::IntSub { dst, a, b } => task.set_raw(dst, externs::int_sub(task.raw(a), task.raw(b))?),
        Op::IntMul { dst, a, b } => task.set_raw(dst, externs::int_mul(task.raw(a), task.raw(b))?),
        Op::IntDiv { dst, a, b } => task.set_raw(dst, externs::int_div(task.raw(a), task.raw(b))?),
        Op::IntRem { dst, a, b } => task.set_raw(dst, externs::int_rem(task.raw(a), task.raw(b))?),
        Op::IntNeg { dst, a } => task.set_raw(dst, externs::int_negate(task.raw(a))?),
        Op::IntEq { dst, a, b } => task.set_raw(dst, externs::int_eq(task.raw(a), task.raw(b))),
        Op::IntNe { dst, a, b } => task.set_raw(dst, externs::int_ne(task.raw(a), task.raw(b))),
        Op::IntLt { dst, a, b } => task.set_raw(dst, externs::int_lt(task.raw(a), task.raw(b))),
        Op::IntLe { dst, a, b } => task.set_raw(dst, externs::int_le(task.raw(a), task.raw(b))),
        Op::IntGt { dst, a, b } => task.set_raw(dst, externs::int_gt(task.raw(a), task.raw(b))),
        Op::IntGe { dst, a, b } => task.set_raw(dst, externs::int_ge(task.raw(a), task.raw(b))),
        Op::IntCompare { dst, a, b } => {
            task.set_raw(dst, externs::int_compare(task.raw(a), task.raw(b)))
        }
        Op::FloatAdd { dst, a, b } => {
            task.set_float(dst, externs::float_add(task.float(a), task.float(b)))
        }
        Op::FloatSub { dst, a, b } => {
            task.set_float(dst, externs::float_sub(task.float(a), task.float(b)))
        }
        Op::FloatMul { dst, a, b } => {
            task.set_float(dst, externs::float_mul(task.float(a), task.float(b)))
        }
        Op::FloatDiv { dst, a, b } => {
            task.set_float(dst, externs::float_div(task.float(a), task.float(b)))
        }
        Op::FloatRem { dst, a, b } => {
            task.set_float(dst, externs::float_rem(task.float(a), task.float(b)))
        }
        Op::FloatNeg { dst, a } => task.set_float(dst, externs::float_negate(task.float(a))),
        Op::FloatEq { dst, a, b } => {
            task.set_raw(dst, externs::float_eq(task.float(a), task.float(b)))
        }
        Op::FloatNe { dst, a, b } => {
            task.set_raw(dst, externs::float_ne(task.float(a), task.float(b)))
        }
        Op::FloatLt { dst, a, b } => {
            task.set_raw(dst, externs::float_lt(task.float(a), task.float(b)))
        }
        Op::FloatLe { dst, a, b } => {
            task.set_raw(dst, externs::float_le(task.float(a), task.float(b)))
        }
        Op::FloatGt { dst, a, b } => {
            task.set_raw(dst, externs::float_gt(task.float(a), task.float(b)))
        }
        Op::FloatGe { dst, a, b } => {
            task.set_raw(dst, externs::float_ge(task.float(a), task.float(b)))
        }
        Op::BoolEq { dst, a, b } => task.set_raw(dst, externs::bool_eq(task.raw(a), task.raw(b))),
        Op::BoolNe { dst, a, b } => task.set_raw(dst, externs::bool_ne(task.raw(a), task.raw(b))),
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
        Op::ArrayGet { dst, a, b } => {
            let w = unsafe { ctx.array_get(task.get(a), task.raw(b)) }?;
            task.set(dst, w);
        }
        Op::ArrayLength { dst, a } => {
            let n = unsafe { ctx.array_length(task.get(a)) }?;
            task.set_raw(dst, n);
        }
        // 文字列の行も借りる引数を手放さない。誤りの扱いは配列の行と同じ
        Op::StrByteAt { dst, i, s } => {
            let b = unsafe { ctx.string_byte_at(task.raw(i), task.get(s)) }?;
            task.set_raw(dst, b);
        }
        Op::StrByteLength { dst, s } => {
            let n = unsafe { ctx.string_byte_length(task.get(s)) }?;
            task.set_raw(dst, n);
        }
        Op::BytesGet { dst, i, b } => {
            let x = unsafe { ctx.bytes_get(task.raw(i), task.get(b)) }?;
            task.set_raw(dst, x);
        }
        Op::BytesLength { dst, b } => {
            let n = unsafe { ctx.bytes_length(task.get(b)) }?;
            task.set_raw(dst, n);
        }
        Op::ArraySet { dst, at } => {
            let o = &code.tables.operands[at as usize..];
            let (a, i, x) = (
                task.get(o[0] as Slot),
                task.raw(o[1] as Slot),
                task.get(o[2] as Slot),
            );
            let w = unsafe { ctx.array_set(a, i, x) }?;
            task.set(dst, w);
        }
        Op::Extern { dst, at } => extern_call::<R>(code, ctx, task, counts, dst, at)?,
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
) -> Result<(), VmFault> {
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
) -> Result<(), VmFault> {
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
) -> Result<(), VmFault> {
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
) -> Result<(), VmFault> {
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
fn ret_many<R: Exec>(code: &CodeBase<R::M>, task: &mut Task, at: u32) -> Result<(), VmFault> {
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
fn receivers<R: Exec>(code: &CodeBase<R::M>, ret: Pc, n: usize) -> Result<&[u32], VmFault> {
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
fn not_received() -> VmFault {
    VmFault::Internal("multiple values return to a place that does not receive them")
}

/// 中断を始めた命令の後で、タスクのスタックを巻き戻す (docs/spec/vm.md の「巻き戻しの手順」)。`task.pc` は中断を
/// 始めた命令の次の位置で、今のフレームの表を引く位置である。着地の印の evidence が `target` と同じ語のフレームに
/// 着いたら、呼び出しの結果にダミーを書いてそこから続ける。それまでのフレームは、着地の印があればその evidence の
/// 状態を手放し、表のスロットを手放して外す。
/// 巻き戻しの間は物体を作らず、`target` は着地するフレームが所有し続けるので、解放した物体の番地が `target` と
/// 取り違えられることはない。安全点を置かないのは、仕事が外すフレームの数で終わるためである。
fn unwind<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    counts: &mut RunCounts,
    target: Word,
) -> Result<(), VmFault> {
    // 最後に外したフレームの見出しにある、呼び出し元の結果のスロット
    let mut dst = None;
    loop {
        match task.pc {
            FINISH if task.exit.is_some() => return Ok(()),
            FINISH => return Err(VmFault::Internal("an abort reached the bottom of the task")),
            // 子の中の handler は子の中に着地するので、子の境目に着くのは `child_abort` の巻き戻しだけである。宛先は
            // 不死のダミーで、どの着地の印とも合わない
            CHILD_DONE if task.exit.is_none() && target == ctx.heap().dummy() => return Ok(()),
            CHILD_DONE => {
                return Err(VmFault::Internal(
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
                    .ok_or(VmFault::Internal("a frame stops outside an unwind point"))?;
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
const CHILD_FAILED: VmFault = VmFault::Internal("a child failure is waiting in the scheduler");

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
) -> Result<(), VmFault> {
    let o = &code.tables.operands[at as usize..];
    // 2つの閉包は `par` が消費する、VM の持つ参照である
    let (f, g) = (task.get(o[0] as Slot), task.get(o[1] as Slot));
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
    // 2つの子が1つずつ参照を持つ。左の子は `Ctx` に入れ、右の子はタスクに退避する。入れ替えで出る親の `evv` の参照は
    // join の節点に移す
    let parent_evv = unsafe {
        ctx.heap_mut().dup(fork_evv)?;
        ctx.heap_mut().share(fork_evv, child)?;
        ctx.switch_task(fork_evv, task.worker, task.depth_offset)
    };
    let ret = task.pc;
    let j = sched.add_join(JoinNode {
        parent: sched.current,
        right: MAIN,
        right_worker: child,
        parent_evv,
        left: None,
        right_outcome: None,
        ret,
        dst,
    });
    let right = Task::right_child(
        task.entry,
        child,
        fork_evv,
        task.depth(),
        sched.max_depth,
        j,
        g,
    );
    let right = sched.add_task(right);
    sched.join(j).right = right;
    let base = task.push_frame(ret, dst, BOUNDARY_SIZE);
    task.stack[base + 1] = Word::from_bits(j);
    task.boundaries.push(j);
    apply_to_unit::<R>(code, ctx, task, f, CHILD_DONE, 0)?;
    sched.ready.push_back(right);
    safepoint::<R>(code, ctx, task, sched)
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
) -> Result<(), VmFault> {
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
) -> Result<Word, VmFault> {
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
            .ok_or(VmFault::Internal("an evv node has tag 0"))?;
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
                    return Err(VmFault::Internal(
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
    for &(effect, table, depth) in mirrored.iter().rev() {
        let ev = heap.alloc_data(effect + 1, 3, 0, false);
        let node = heap.alloc_data(effect + 1, 2, 0, false);
        unsafe {
            heap.set_field(ev, 0, table)?;
            heap.set_field(ev, 1, Word::imm(i64::from(depth)))?;
            heap.set_field(node, 0, ev)?;
            heap.set_field(node, 1, next)?;
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
) -> Result<(), VmFault> {
    // 子は親の handler に yield できないので、yield の途中で終わる子はない
    if ctx.yielding() != 0 {
        return Err(VmFault::Internal(
            "a par child ends in the middle of a yield",
        ));
    }
    let j = task.get(1).bits();
    let open = task.boundaries.pop();
    debug_assert_eq!(open, Some(j), "the innermost boundary is the one that ends");
    if sched.current == sched.join(j).right {
        let join = sched.join(j);
        join.right_outcome = Some(outcome);
        let right_worker = join.right_worker;
        // 子の残りの `evv` を、子の ID のまま手放す
        let rest = unsafe { ctx.switch_task(Word::UNIT, right_worker, task.depth_offset) };
        unsafe { ctx.heap_mut().decref(rest) }?;
        wake_parent(sched, j);
        return switch_away::<R>(code, ctx, task, sched, None);
    }
    let join = sched.join(j);
    join.left = Some(outcome);
    let parent_evv = std::mem::replace(&mut join.parent_evv, Word::UNIT);
    let right_done = join.right_outcome.is_some();
    // 境目のフレームを外す。組か起こし直しの結果は join が `par` の結果のスロットに書く
    task.pop_frame();
    let rest = unsafe { ctx.switch_task(parent_evv, task.worker, task.depth_offset) };
    unsafe { ctx.heap_mut().decref(rest) }?;
    if right_done {
        let at = complete_join::<R>(code, ctx, task, sched, j)?;
        let result = safepoint::<R>(code, ctx, task, sched);
        at_par(code, task, sched, at, result)
    } else {
        switch_away::<R>(code, ctx, task, sched, Some(TaskState::Waiting(j)))
    }
}

/// 右の子が終わった join の親が、その join を待っていれば起こす。
fn wake_parent<P: Policy>(sched: &mut Scheduler<P>, j: JoinId) {
    let parent = sched.join(j).parent;
    if let Some(task) = &sched.tasks[parent]
        && matches!(task.state, TaskState::Waiting(w) if w == j)
    {
        sched.policy.wake(&mut sched.ready, parent);
    }
}

/// 方針が選ぶ次のタスクに切り替える。`keep` が `None` なら、今のタスクは終わったので捨てる。次のタスクが join を
/// 完了したら、`par` の後の安全点に進む。
fn switch_away<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    keep: Option<TaskState>,
) -> Result<(), VmFault> {
    // 落ちたハンドルの知らせは切り替えのたびに処理し、終わったタスクのセルと ID を run の終わりまで残さない
    // (docs/spec/vm.md の「`Async` のタスク」)
    drain::<R>(code, ctx, task.depth_offset, sched)?;
    let next = sched
        .policy
        .pick(&mut sched.ready)
        .ok_or(VmFault::Internal("no task is ready to run"))?;
    match resume::<R>(code, ctx, task, sched, next, keep)? {
        Some(at) => {
            let result = safepoint::<R>(code, ctx, task, sched);
            at_par(code, task, sched, at, result)
        }
        None => Ok(()),
    }
}

/// タスク `next` に切り替える。`evv` と worker は `Ctx::switch_task` の1回で替え、RC に触れない
/// (docs/spec/object-model.md の「タスクの切り替え」)。まだ始まっていない右の子は、その ID に替えた後で閉包を呼ぶ。
/// 閉包を手放すと、子の ID の物体を解放しうるためである。待っていた親は join を完了し、そのときは `par` の命令の
/// 位置を返す。`par` の後の安全点は呼ぶ側が行い、その誤りにもこの位置を付ける (`at_par`)。ここで安全点に進むと、横取りが `preempt` と `resume` を Rust の再帰で重ねる
/// ためである (docs/spec/vm.md の「並列化への備え」)。
fn resume<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    next: TaskId,
    keep: Option<TaskState>,
) -> Result<Option<Pc>, VmFault> {
    let mut incoming = sched.tasks[next].take().ok_or(VmFault::Internal(
        "a task to resume is not in the scheduler",
    ))?;
    let evv = std::mem::replace(&mut incoming.evv, Word::UNIT);
    let saved = unsafe { ctx.switch_task(evv, incoming.worker, incoming.depth_offset) };
    let mut prev = std::mem::replace(task, incoming);
    let prev_id = sched.current;
    sched.set_current(next);
    sched.note_depth(&prev);
    match keep {
        Some(state) => {
            prev.evv = saved;
            prev.state = state;
            sched.tasks[prev_id] = Some(prev);
        }
        None => {
            // 終わった入口の `Task` のスタックは、次の入口の `Task` が使い回す
            if prev.cell.is_some() {
                sched.stacks.push(std::mem::take(&mut prev.stack));
            }
            sched.free_task(prev_id)
        }
    }
    match std::mem::replace(&mut task.state, TaskState::Running) {
        TaskState::Unstarted(g) => {
            // 始めたばかりの子はまだ命令を実行していないので、誤りには fork した `par` の命令の位置を付ける。
            // 横取りされたタスクの位置を付けると、文言がスケジュールで変わる
            let at = sched.join(task.boundaries[0]).ret - 1;
            let result = apply_to_unit::<R>(code, ctx, task, g, CHILD_DONE, 0);
            at_par(code, task, sched, at, result).map(|()| None)
        }
        TaskState::Waiting(j) => complete_join::<R>(code, ctx, task, sched, j).map(Some),
        TaskState::Entry {
            function,
            args,
            argc,
        } => {
            // 入口の `Task` はまだ命令を実行していないので、誤りには入口の関数の先頭の位置を付ける。`run` が持つ
            // 位置は、切り替える前のタスクのものである
            let at = code.functions[function as usize].start;
            let result = enter::<R>(code, ctx, task, sched, function, args, argc);
            at_par(code, task, sched, at, result).map(|()| None)
        }
        // `Async.run` の待ちは、書いた `Async` の命令の次から続ける。起こすのは根のスコープが閉じたときだけである
        TaskState::WaitingCell(root) => {
            debug_assert!(
                sched.cells.closed(root).unwrap_or(false),
                "a run wakes before its scope closes"
            );
            Ok(None)
        }
        TaskState::Ready | TaskState::Running => Ok(None),
    }
}

/// 受け箱を処理し、取り消しで待ちを外したタスクを再開する仕事を列の後ろに積む。
fn drain<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    depth_offset: usize,
    sched: &mut Scheduler<R::P>,
) -> Result<(), VmFault> {
    let mut out = Vec::new();
    sched.cells.drain(ctx, depth_offset, &mut out)?;
    if out.is_empty() {
        return Ok(());
    }
    let entries = code
        .entries
        .ok_or(VmFault::Internal("the Async entries are not loaded"))?;
    for run in out {
        schedule(code, sched, entries, run)?;
    }
    Ok(())
}

/// 方針か `PREEMPT` が安全点で横取りを求めた。走れるタスクがなければ、切り替えずに予算を詰め直す (`PREEMPT` は
/// 安全点の `poll` がもう下ろしている)。切り替えた先が join を
/// 完了したら、`par` の後の安全点をここで繰り返す。Rust の再帰を使わないためである
/// (docs/spec/vm.md の「並列化への備え」)。
#[cold]
#[inline(never)]
fn preempt<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
) -> Result<(), VmFault> {
    drain::<R>(code, ctx, task.depth_offset, sched)?;
    loop {
        let next = sched.policy.pick(&mut sched.ready);
        sched.policy.reload();
        let Some(next) = next else {
            return Ok(());
        };
        let current = sched.current;
        sched.policy.wake(&mut sched.ready, current);
        sched.count_preemption();
        let Some(at) = resume::<R>(code, ctx, task, sched, next, Some(TaskState::Ready))? else {
            return Ok(());
        };
        let wants = match poll(ctx, task.stack_depth()) {
            Ok(wants) => wants,
            Err(fault) => return at_par(code, task, sched, at, Err(fault)),
        };
        let tick = sched.policy.tick();
        if !((wants || tick) && ctx.yielding() == 0) {
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
) -> Result<Pc, VmFault> {
    let join = sched.take_join(j);
    let at = join.ret - 1;
    let result = join_outcomes::<R>(code, ctx, task, sched, join);
    at_par(code, task, sched, at, result).map(|()| at)
}

/// join の完了、その後の安全点、右の子の始まりで起きた誤りに、`par` の命令 `at` の関数と位置を付ける。どれも
/// 切り替えの後で起きるので、`run` が持つ命令の位置は別のタスクのものでありうる。`par` の位置を付ければ、どの
/// スケジュールでも同じ文言になる (docs/spec/vm.md の「子の終わりと join」)。`Async` の入口の `Task` の始まりの誤りも、
/// 同じ理由で入口の関数の先頭の位置 `at` を付ける。すでに記録した誤りはそのまま渡す。
fn at_par<M: Mode, P: Policy>(
    code: &CodeBase<M>,
    task: &Task,
    sched: &mut Scheduler<P>,
    at: Pc,
    result: Result<(), VmFault>,
) -> Result<(), VmFault> {
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
) -> Result<(), VmFault> {
    let (Some(left), Some(right)) = (join.left, join.right_outcome) else {
        return Err(VmFault::Internal(
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
                return Err(VmFault::Internal(
                    "a join returns after something other than a par",
                ));
            };
            let tag = code.tables.operands[at as usize + 2] as u16;
            let pair = heap.alloc_data(tag, 2, 0, false);
            // 組は今作った局所の物体で、2つの語の参照は組に移る
            unsafe {
                heap.set_field(pair, 0, l)?;
                heap.set_field(pair, 1, r)?;
            }
            task.set(join.dst, pair);
            return Ok(());
        }
        (Outcome::Error(_), _) | (_, Outcome::Error(_)) => {
            return Err(VmFault::Internal(
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
        return Err(VmFault::Internal(
            "a join raises again something other than an abort",
        ));
    };
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
    apply_to_unit::<R>(code, ctx, task, clause, join.ret, join.dst)
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
) -> Result<(), VmFault> {
    if task.boundaries.is_empty() {
        return Err(VmFault::Internal("a child abort outside a par child"));
    }
    let ev = task.get(ev);
    // `ev` は節の入口が持つ fork evidence で、状態の欄は深さの即値である
    let heap = ctx.heap();
    let effect = unsafe { heap.meta(ev) }?
        .tag()
        .checked_sub(1)
        .ok_or(VmFault::Internal(
            "a child abort gets something other than evidence",
        ))?;
    let depth = unsafe { heap.scanned(ev, 1) }?;
    let depth = u32::try_from(depth.imm_value())
        .ok()
        .filter(|_| depth.is_imm())
        .ok_or(VmFault::Internal(
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

/// 実行時エラーの後。すぐに実行を止める誤りと、入口のタスクの誤り (左の子の誤りを含む) は実行の誤りにする。右の子の
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
        let error = match sched.failed.take() {
            Some(error) => error,
            None => code.error(failure, task),
        };
        // `Async` のタスクの失敗は底の `try_all` が捕まえるので、ここに来るのは捕まえられない誤りで、run を止める
        // (docs/spec/vm.md の「`Async` のタスク」)
        if sched.current == MAIN || task.cell.is_some() || stops_the_run(&error) {
            return Err(error);
        }
        let boundaries = std::mem::take(&mut task.boundaries);
        // 右の子が開いていた入れ子の `par` の子は、結果を受ける親がもういないので、走らせずに捨てる
        for &inner in &boundaries[1..] {
            cancel(sched, inner);
        }
        let j = boundaries[0];
        sched.join(j).right_outcome = Some(Outcome::Error(error));
        wake_parent(sched, j);
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
            fault: VmFault::Stopped(Stop::Interrupted | Stop::MemoryLimit | Stop::Ended)
                | VmFault::Extern(eml_rt::Fault::Heap(_))
                | VmFault::Internal(_),
            ..
        } | RuntimeError::Leak(_)
            | RuntimeError::Unrecovered { .. }
    )
}

/// 親が誤りで止まった join の右の子を、その子孫ごと表と列から外す。持つ語は触れずに捨てる。子孫は作業の列でたどり、
/// Rust の再帰を使わない (docs/spec/vm.md の「並列化への備え」)。
fn cancel<P: Policy>(sched: &mut Scheduler<P>, j: JoinId) {
    let mut pending = vec![j];
    while let Some(j) = pending.pop() {
        let join = sched.take_join(j);
        if join.right_outcome.is_some() {
            continue;
        }
        let right = join.right;
        sched.ready.retain(|&id| id != right);
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

/// 入口の関数の結果か、`exit` の中断を手放す (docs/spec/vm.md の「誤り、止めること、終わり」)。その前に、受け箱の
/// 知らせを処理し、残った `Async` のセルを放す (docs/spec/vm.md の「`Async` のタスク」)。
fn finish<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &Task,
    sched: &mut Scheduler<R::P>,
) -> Result<(), VmFault> {
    let mut out = Vec::new();
    sched.cells.drain(ctx, task.depth_offset, &mut out)?;
    if !out.is_empty() {
        return Err(VmFault::Internal("a task is left at the end"));
    }
    sched.cells.sweep_all(ctx, task.depth_offset)?;
    if sched.has_children() {
        return Err(VmFault::Internal("a task is left at the end"));
    }
    if task.exit.is_some() && !task.boundaries.is_empty() {
        return Err(VmFault::Internal("an exit ends a run with an open par"));
    }
    if let Some(n) = task.exit {
        // 途中のフレームは巻き戻しで片付けてある。入口の結果はダミーなので手放さない。中断の途中の `evv` には
        // 意味がないので、空に戻す (docs/spec/vm.md の「extern の中断」)
        unsafe {
            ctx.yield_finish()?;
            ctx.evv_set(Word::UNIT)?;
        }
        // 終了コードは終わりの欄だけが運ぶ。run の始めに開いた欄に先に書いた者はいないはずである
        // (docs/spec/vm.md の「`exit`」)
        if !cells::end_with_exit(ctx.heap().control(), n) {
            return Err(VmFault::Internal("an exit after the end of the run"));
        }
        return Ok(());
    }
    if code.functions[task.entry as usize].ret_rc && !task.keep_result {
        // 結果は入口の関数が返した、VM の持つ参照である
        unsafe { ctx.heap_mut().decref(task.get(0)) }?;
    }
    Ok(())
}

// `Async` の命令は呼び出しや戻りに比べてまれなので、`par` と同じく `step` に展開しない

/// `Std.Async` の行 (docs/spec/vm.md の「`Async` のタスク」)。意味の核を呼び、返った仕事を入口の `Task` にするか、
/// 待っていた `Task` を実行待ちに戻す。安全点を置かない。
#[inline(never)]
fn async_op<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    dst: Slot,
    at: u32,
) -> Result<(), VmFault> {
    let o = &code.tables.operands[at as usize..];
    let arg = |i: usize| o[2 + i] as Slot;
    let entries = code
        .entries
        .ok_or(VmFault::Internal("the Async entries are not loaded"))?;
    // 語の引数は、LIR の所有の規則により VM が持つ参照で、行がそれを預かる
    match Extern::ALL[o[0] as usize] {
        Extern::AsyncStartRaw => {
            let cell = current_cell(task)?;
            let scope = sched.cells.current_of(cell)?;
            let (handle, run) = start_task::<R>(ctx, task, sched, scope, task.get(arg(0)))?
                .ok_or(VmFault::Internal("a task starts in a closed scope"))?;
            task.set(dst, handle);
            schedule(code, sched, entries, run)?;
        }
        Extern::AsyncRunRaw => {
            // 呼んだ `Task` はスタックを持ったまま、根のスコープが閉じるのを待つ
            let parent = match task.cell {
                Some(cell) => Some(sched.cells.current_of(cell)?),
                None => None,
            };
            let root = sched.cells.open_root(parent, task.cell)?;
            let (handle, run) = start_task::<R>(ctx, task, sched, root, task.get(arg(0)))?
                .ok_or(VmFault::Internal("a task starts in a closed scope"))?;
            sched.cells.wait(root, WaitToken(sched.current as u64))?;
            task.set(dst, handle);
            schedule(code, sched, entries, run)?;
            switch_away::<R>(code, ctx, task, sched, Some(TaskState::WaitingCell(root)))?;
        }
        Extern::AsyncFinishOk => {
            let value = task.get(arg(0));
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
            match sched.cells.take(ctx, handle, kind, task.depth_offset)? {
                Ok(value) => task.set(dst, value),
                Err(raise) => return raise_again::<R>(code, ctx, task, dst, raise, ext),
            }
        }
        Extern::AsyncPark => {
            let me = current_cell(task)?;
            let k = task.get(arg(1));
            if let Some(run) = sched.cells.park(task.raw(arg(0)), me, k)? {
                schedule(code, sched, entries, run)?;
            }
            task.set(dst, Word::UNIT);
        }
        Extern::AsyncReady => {
            let me = current_cell(task)?;
            let run = sched.cells.ready(me, task.get(arg(0)))?;
            schedule(code, sched, entries, run)?;
            task.set(dst, Word::UNIT);
        }
        Extern::AsyncTaskCell => {
            // ハンドルは借りるだけで、組を作る後の `con` が消費する (docs/spec/core-ir.md の「組を返す extern」)
            let c = sched.cells.cell_of(ctx, task.get(arg(0)))?;
            task.set_raw(dst, c);
        }
        ext @ (Extern::AsyncDetach | Extern::AsyncCancelRaw) => {
            let cancel = ext == Extern::AsyncCancelRaw;
            sched
                .cells
                .detach(ctx, task.get(arg(0)), cancel, task.depth_offset)?;
            task.set(dst, Word::UNIT);
        }
        Extern::AsyncStartInRaw => {
            current_cell(task)?;
            let g = task.get(arg(1));
            let started = match sched.cells.scope_cell(task.raw(arg(0))) {
                Some(scope) => start_task::<R>(ctx, task, sched, scope, g)?,
                None => None,
            };
            let Some((handle, run)) = started else {
                // 預かった閉包は呼ばずに手放してから起こす
                unsafe { ctx.heap_mut().decref(g) }?;
                let raise = Raise::Panic("the scope is closed".to_string());
                return raise_again::<R>(code, ctx, task, dst, raise, Extern::AsyncStartInRaw);
            };
            task.set(dst, handle);
            schedule(code, sched, entries, run)?;
        }
        Extern::AsyncOpenScope => {
            let me = current_cell(task)?;
            let s = sched.cells.open_scope(me)?;
            task.set_raw(dst, s);
        }
        Extern::AsyncCloseScope => {
            let c = sched.cells.close_scope(task.raw(arg(0)))?;
            task.set_raw(dst, c);
        }
        Extern::AsyncLeaveScope => {
            let me = current_cell(task)?;
            match sched.cells.leave_scope(task.raw(arg(0)), me)? {
                None => task.set(dst, Word::UNIT),
                Some(raise) => {
                    return raise_again::<R>(code, ctx, task, dst, raise, Extern::AsyncLeaveScope);
                }
            }
        }
        Extern::AsyncCancelScopeRaw => {
            sched.cells.cancel_scope(task.raw(arg(0)))?;
            task.set(dst, Word::UNIT);
        }
        Extern::AsyncFailScopeIo => {
            let message = unsafe { ctx.copy_string(task.get(arg(2)), usize::MAX) }?;
            let kind = task.raw(arg(1));
            sched
                .cells
                .fail_scope(task.raw(arg(0)), cells::Outcome::Io { kind, message })?;
            task.set(dst, Word::UNIT);
        }
        Extern::AsyncFailScopePanic => {
            let message = unsafe { ctx.copy_string(task.get(arg(1)), usize::MAX) }?;
            sched
                .cells
                .fail_scope(task.raw(arg(0)), cells::Outcome::Panic(message))?;
            task.set(dst, Word::UNIT);
        }
        Extern::AsyncCancelPoint => {
            let b = sched.cells.cancel_point(task.cell)?;
            task.set_raw(dst, i64::from(b));
        }
        Extern::AsyncSpendBudget => {
            let me = current_cell(task)?;
            let b = sched.cells.spend_budget(me)?;
            task.set_raw(dst, i64::from(b));
        }
        Extern::AsyncShieldEnter => {
            let me = current_cell(task)?;
            sched.cells.shield_enter(me)?;
            task.set(dst, Word::UNIT);
        }
        Extern::AsyncShieldLeave => {
            let me = current_cell(task)?;
            sched.cells.shield_leave(me)?;
            task.set(dst, Word::UNIT);
        }
        _ => {
            return Err(VmFault::Internal(
                "a row outside `Std.Async` reached the Async instruction",
            ));
        }
    }
    Ok(())
}

/// 今の `Task` のタスクのセル。`main` と `par` の右の子は `Async` の操作を実行しない。
fn current_cell(task: &Task) -> Result<CellRef, VmFault> {
    task.cell
        .ok_or(VmFault::Internal("an Async operation runs outside a task"))
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
) -> Result<(), VmFault> {
    match raise {
        Raise::Panic(message) => {
            if abort_to::<R, _>(code, ctx, task, dst, ExternEffect::Panic, PANICKED, |ctx| {
                [ctx.fresh_string(&message)]
            })? {
                return Ok(());
            }
            Err(VmFault::Panicked(message))
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

/// 今の `Task` から、スコープ `scope` に新しいタスクを置く。閉包 `g` とまだ印のない根に、新しいタスクの ID で印を付ける。
fn start_task<R: Exec>(
    ctx: &mut Ctx<R::M>,
    task: &Task,
    sched: &mut Scheduler<R::P>,
    scope: CellRef,
    g: Word,
) -> Result<Option<(Word, Runnable)>, VmFault> {
    let roots = unshared_roots(ctx, sched);
    Ok(sched.cells.start(ctx, scope, task.worker, g, &roots)?)
}

/// `finish_*` の終わり方を入口の `Task` に預ける。核に渡すのは `Task` の `Finish` で、そのときにはタスクの ID の物体は
/// 終わり方の値のほかに片付いている。先に渡すと、起こした側がセルを放してタスクの ID を退かせたときに、まだ動いている
/// タスクの物体が残る (docs/spec/vm.md の「`Async` のタスク」)。
fn deposit(task: &mut Task, outcome: cells::Outcome) -> Result<(), VmFault> {
    if task.cell.is_none() || task.outcome.is_some() {
        return Err(VmFault::Internal("a task finishes twice or outside a task"));
    }
    task.outcome = Some(outcome);
    Ok(())
}

/// 核が返した仕事を実行待ちの列の後ろに積む。どの方針でも FIFO である (docs/spec/vm.md の「タスクとスケジュール」)。
/// 始める仕事と再開する仕事は、集まりのスタックで入口の `Task` にする。token は待っている `Task` の番号である。
fn schedule<M: Mode, P: Policy>(
    code: &CodeBase<M>,
    sched: &mut Scheduler<P>,
    entries: AsyncEntries,
    run: Runnable,
) -> Result<(), VmFault> {
    let (function, args, argc, cell) = match run {
        Runnable::Start { cell, closure } => (entries.start, [closure, Word::UNIT], 1, cell),
        Runnable::Resume { cell, k, wake } => {
            (entries.resume, [k, Word::from_bits(wake as usize)], 2, cell)
        }
        Runnable::Token(WaitToken(token)) => {
            let id = token as TaskId;
            match sched.tasks.get(id) {
                Some(Some(Task {
                    state: TaskState::WaitingCell(_),
                    ..
                })) => {}
                _ => {
                    return Err(VmFault::Internal("a woken token names no waiting task"));
                }
            }
            sched.ready.push_back(id);
            return Ok(());
        }
    };
    let worker = sched.cells.worker(cell)?;
    let stack = sched.stacks.pop().unwrap_or_default();
    let task = Task::entry(
        code,
        function,
        args,
        argc,
        cell,
        worker,
        sched.max_depth,
        stack,
    );
    let id = sched.add_task(task);
    sched.ready.push_back(id);
    Ok(())
}

/// 入口の `Task` が動き始める。始める仕事なら、閉包を始めたタスクの ID から今の ID へ戻す (`par` の join の
/// `transfer_back` と同じ形)。引数を入口のフレームの slot 0.. に書いて入口の先頭へ進む。
fn enter<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
    function: u32,
    mut args: [Word; 2],
    argc: u8,
) -> Result<(), VmFault> {
    let cell = task
        .cell
        .ok_or(VmFault::Internal("an Async entry task has no cell"))?;
    if argc == 1 {
        let spawner = sched.cells.started(cell)?;
        // 閉包は始めたタスクが印を付けて渡した参照で、このタスクだけが持つ
        unsafe { ctx.heap_mut().transfer_back(args[0], spawner) }?;
    } else {
        let wake = sched.cells.resumed(cell, args[1].bits() as i64)?;
        args[1] = Word::from_bits(wake as usize);
    }
    let argc = usize::from(argc);
    let base = task.base;
    task.stack[base..base + argc].copy_from_slice(&args[..argc]);
    task.pc = code.functions[function as usize].start;
    Ok(())
}

/// 入口の `Task` の `Finish`。タスクの残りの `evv` を手放し、預かった終わり方を核に渡して起こす仕事を積み、スタックを
/// 集まりに返してほかのタスクに切り替える。`exit` の巻き戻しが底に着いたら、終わりの欄に書いて run を捨てる
/// (docs/spec/vm.md の「`exit`」)。
#[inline(never)]
fn entry_finish<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    sched: &mut Scheduler<R::P>,
) -> Result<(), VmFault> {
    let Some(cell) = task.cell else {
        return Err(VmFault::Internal("a task is left at the end"));
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
        return Err(VmFault::Stopped(Stop::Ended));
    }
    if ctx.yielding() != 0 || !task.boundaries.is_empty() {
        return Err(VmFault::Internal(
            "an Async task ends in the middle of a yield or a par",
        ));
    }
    // タスクの残りの `evv` を、タスクの ID のまま手放す
    let rest = unsafe { ctx.switch_task(Word::UNIT, task.worker, task.depth_offset) };
    unsafe { ctx.heap_mut().decref(rest) }?;
    if let Some(outcome) = task.outcome.take() {
        let entries = code
            .entries
            .ok_or(VmFault::Internal("the Async entries are not loaded"))?;
        let mut out = Vec::new();
        sched
            .cells
            .finish(ctx, cell, outcome, task.depth_offset, &mut out)?;
        for run in out {
            schedule(code, sched, entries, run)?;
        }
    }
    switch_away::<R>(code, ctx, task, sched, None)
}

/// 中断の操作の番号。Prelude の `IO` (`io_failed`) と `Panic` (`panicked`) は、どちらも操作を1つだけ宣言する
/// (docs/spec/vm.md の「`IO` と `Panic` への中断」)。
const IO_FAILED: usize = 0;
const PANICKED: usize = 0;

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
    fault: VmFault,
) -> Result<(), VmFault> {
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
        | Op::ArraySet { dst, .. } => (dst, Purity::Panics),
        // `length` の行は `Pure` で巻き戻しの地点を持たない
        Op::ArrayLength { dst, .. }
        | Op::StrByteLength { dst, .. }
        | Op::BytesLength { dst, .. } => (dst, Purity::Pure),
        _ => return Err(fault),
    };
    match fault {
        VmFault::Extern(fault) => failed::<R>(code, ctx, task, dst, fault, purity),
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
) -> Result<(), VmFault> {
    match run_extern::<R>(code, ctx, task, counts, dst, at) {
        Err(VmFault::Extern(fault)) => {
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
fn failed<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    dst: Slot,
    fault: Fault,
    purity: Purity,
) -> Result<(), VmFault> {
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
        Err(VmFault::Extern(fault))
    }
}

/// 実行の時点で最も近い `effect` の handler の、`never` の操作 `op` の節の入口を、evidence と `args` で呼ぶ。evidence
/// がなければ何もせずに `false` を返し、`args` も作らない。入口は `never` の節の入口なので、`yield_abort` で巻き戻しを
/// 始めて戻らない。中断を起こした命令のフレームは入口の呼び出し元になり、その命令の地点の表の行で片付く。入口が戻らない
/// ので、安全点も置かない (docs/spec/vm.md の「`IO` と `Panic` への中断」)。
fn abort_to<R: Exec, const N: usize>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    dst: Slot,
    effect: ExternEffect,
    op: usize,
    args: impl FnOnce(&mut Ctx<R::M>) -> [Word; N],
) -> Result<bool, VmFault> {
    let Some(&number) = code.index.effects.get(effect.row().name) else {
        return Ok(false);
    };
    let Some(ev) = (unsafe { ctx.ev_find(number) })? else {
        return Ok(false);
    };
    // `ev_find` が `dup` した evidence を、節の入口の引数として渡す。`ev_clause` はそれを借りる
    let clause = unsafe { ctx.ev_clause(ev, op) }?;
    let args = args(ctx);
    task.scratch.clear();
    task.scratch.push(ev);
    task.scratch.extend(args);
    let ret = task.pc;
    apply::apply_clause::<R>(code, ctx, task, clause, apply::Target::Frame { ret, dst })?;
    Ok(true)
}

/// 行の番号の `match` で `Ctx` のメソッドを呼ぶ (docs/spec/object-model.md の「extern」)。
fn run_extern<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    counts: &mut RunCounts,
    dst: Slot,
    at: u32,
) -> Result<(), VmFault> {
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
        return Err(VmFault::Panicked(text));
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
            Extern::Println => {
                ctx.println(Stream::Out, task.get(arg(0)))?;
                task.set(dst, Word::UNIT);
            }
            Extern::Open | Extern::FileOutCreate | Extern::FileOutAppend => {
                let open = match ext {
                    Extern::Open => Open::Read,
                    Extern::FileOutCreate => Open::Write(WriteMode::Truncate),
                    _ => Open::Write(WriteMode::Append),
                };
                let w = ctx.fs_open(task.get(arg(0)), open)?;
                task.set(dst, w);
            }
            Extern::ReadAll
            | Extern::ReadLine
            | Extern::ReadAllBytes
            | Extern::ReadLineBytes
            | Extern::StdinReadAll
            | Extern::StdinReadLine
            | Extern::StdinReadAllBytes
            | Extern::StdinReadLineBytes
            | Extern::ProcReadAll
            | Extern::ProcReadLine
            | Extern::ProcReadAllBytes
            | Extern::ProcReadLineBytes => {
                let (handle, read, form) = match ext {
                    Extern::ReadAll => (Handle::FileIn, ReadAmount::All, Form::Text),
                    Extern::ReadLine => (Handle::FileIn, ReadAmount::Line, Form::Text),
                    Extern::ReadAllBytes => (Handle::FileIn, ReadAmount::All, Form::Bytes),
                    Extern::ReadLineBytes => (Handle::FileIn, ReadAmount::Line, Form::Bytes),
                    Extern::StdinReadAll => (Handle::Stdin, ReadAmount::All, Form::Text),
                    Extern::StdinReadLine => (Handle::Stdin, ReadAmount::Line, Form::Text),
                    Extern::StdinReadAllBytes => (Handle::Stdin, ReadAmount::All, Form::Bytes),
                    Extern::StdinReadLineBytes => (Handle::Stdin, ReadAmount::Line, Form::Bytes),
                    Extern::ProcReadAll => (Handle::Child, ReadAmount::All, Form::Text),
                    Extern::ProcReadLine => (Handle::Child, ReadAmount::Line, Form::Text),
                    Extern::ProcReadAllBytes => (Handle::Child, ReadAmount::All, Form::Bytes),
                    _ => (Handle::Child, ReadAmount::Line, Form::Bytes),
                };
                let w = ctx.handle_read(task.get(arg(0)), handle, read, form)?;
                task.set(dst, w);
            }
            Extern::ReadChunk | Extern::StdinReadChunk | Extern::ProcReadChunk => {
                let handle = match ext {
                    Extern::ReadChunk => Handle::FileIn,
                    Extern::StdinReadChunk => Handle::Stdin,
                    _ => Handle::Child,
                };
                let chunk = ReadAmount::Chunk(task.get(arg(0)));
                let w = ctx.handle_read(task.get(arg(1)), handle, chunk, Form::Bytes)?;
                task.set(dst, w);
            }
            Extern::Close
            | Extern::FileOutCloseRaw
            | Extern::StdinClose
            | Extern::StdoutClose
            | Extern::StderrClose => {
                let handle = match ext {
                    Extern::Close => Handle::FileIn,
                    Extern::FileOutCloseRaw => Handle::FileOut,
                    Extern::StdinClose => Handle::Stdin,
                    Extern::StdoutClose => Handle::Stdout,
                    _ => Handle::Stderr,
                };
                ctx.handle_close(task.get(arg(0)), handle)?;
                task.set(dst, Word::UNIT);
            }
            Extern::StdinOpen | Extern::StdoutOpen | Extern::StderrOpen => {
                let handle = match ext {
                    Extern::StdinOpen => Handle::Stdin,
                    Extern::StdoutOpen => Handle::Stdout,
                    _ => Handle::Stderr,
                };
                let w = ctx.handle_open(handle)?;
                task.set(dst, w);
            }
            Extern::FileOutWrite
            | Extern::FileOutWriteBytes
            | Extern::FileOutWriteLine
            | Extern::StdoutWrite
            | Extern::StdoutWriteBytes
            | Extern::StdoutWriteLine
            | Extern::StderrWrite
            | Extern::StderrWriteBytes
            | Extern::StderrWriteLine
            | Extern::ProcWrite
            | Extern::ProcWriteBytes
            | Extern::ProcWriteLine => {
                let (handle, write) = match ext {
                    Extern::FileOutWrite => (Handle::FileOut, Write::Text),
                    Extern::FileOutWriteBytes => (Handle::FileOut, Write::Bytes),
                    Extern::FileOutWriteLine => (Handle::FileOut, Write::Line),
                    Extern::StdoutWrite => (Handle::Stdout, Write::Text),
                    Extern::StdoutWriteBytes => (Handle::Stdout, Write::Bytes),
                    Extern::StdoutWriteLine => (Handle::Stdout, Write::Line),
                    Extern::StderrWrite => (Handle::Stderr, Write::Text),
                    Extern::StderrWriteBytes => (Handle::Stderr, Write::Bytes),
                    Extern::StderrWriteLine => (Handle::Stderr, Write::Line),
                    Extern::ProcWrite => (Handle::Child, Write::Text),
                    Extern::ProcWriteBytes => (Handle::Child, Write::Bytes),
                    _ => (Handle::Child, Write::Line),
                };
                let w = ctx.handle_write(task.get(arg(1)), handle, task.get(arg(0)), write)?;
                task.set(dst, w);
            }
            Extern::StdoutFlush | Extern::FileOutFlush => {
                let handle = if ext == Extern::StdoutFlush {
                    Handle::Stdout
                } else {
                    Handle::FileOut
                };
                let w = ctx.handle_flush(task.get(arg(0)), handle)?;
                task.set(dst, w);
            }
            Extern::CharShow | Extern::CharDisplay => {
                let c = task.raw(arg(0));
                // SAFETY: `Char` の語は Unicode のスカラー値である (docs/spec/lir.md の「値の表現」)
                let w = if ext == Extern::CharShow {
                    ctx.char_show(c)
                } else {
                    ctx.char_display(c)
                };
                task.set(dst, w);
            }
            Extern::IntShow | Extern::ByteDisplay => {
                let w = ctx.int_show(task.raw(arg(0)));
                task.set(dst, w);
            }
            Extern::StrConcat => {
                let w = ctx.string_append(task.get(arg(0)), task.get(arg(1)))?;
                task.set(dst, w);
            }
            Extern::StrEq => {
                let r = ctx.string_eq(task.get(arg(0)), task.get(arg(1)))?;
                task.set_raw(dst, r);
            }
            Extern::StrNe => {
                let r = ctx.string_ne(task.get(arg(0)), task.get(arg(1)))?;
                task.set_raw(dst, r);
            }
            Extern::StrCompare => {
                let r = ctx.string_compare(task.get(arg(0)), task.get(arg(1)))?;
                task.set_raw(dst, r);
            }
            Extern::StrShow => {
                let w = ctx.string_show(task.get(arg(0)))?;
                task.set(dst, w);
            }
            Extern::Eprintln => {
                ctx.println(Stream::Err, task.get(arg(0)))?;
                task.set(dst, Word::UNIT);
            }
            Extern::StrCharAt => {
                let c = ctx.string_char_at(task.raw(arg(0)), task.get(arg(1)))?;
                task.set_raw(dst, c);
            }
            Extern::StrSlice => {
                let w = ctx.string_slice(task.raw(arg(0)), task.raw(arg(1)), task.get(arg(2)))?;
                task.set(dst, w);
            }
            Extern::StrPush => {
                // SAFETY: `Char` の語は Unicode のスカラー値である (docs/spec/lir.md の「値の表現」)
                let w = ctx.string_push_char(task.raw(arg(0)), task.get(arg(1)))?;
                task.set(dst, w);
            }
            Extern::StrFindFrom => {
                let i =
                    ctx.string_find_from(task.get(arg(0)), task.raw(arg(1)), task.get(arg(2)))?;
                task.set_raw(dst, i);
            }
            Extern::BytesEmpty => {
                let w = ctx.bytes_empty()?;
                task.set(dst, w);
            }
            Extern::BytesAppend => {
                let w = ctx.bytes_append(task.get(arg(0)), task.get(arg(1)))?;
                task.set(dst, w);
            }
            Extern::BytesAppendSlice => {
                let w = ctx.bytes_append_part(
                    task.raw(arg(0)),
                    task.raw(arg(1)),
                    task.get(arg(2)),
                    task.get(arg(3)),
                )?;
                task.set(dst, w);
            }
            Extern::BytesSlice => {
                let w = ctx.bytes_slice(task.raw(arg(0)), task.raw(arg(1)), task.get(arg(2)))?;
                task.set(dst, w);
            }
            Extern::BytesTruncate => {
                let w = ctx.bytes_truncate(task.raw(arg(0)), task.get(arg(1)))?;
                task.set(dst, w);
            }
            Extern::BytesOfString => {
                let w = ctx.bytes_of_string(task.get(arg(0)))?;
                task.set(dst, w);
            }
            Extern::BytesEq => {
                let r = ctx.bytes_eq(task.get(arg(0)), task.get(arg(1)))?;
                task.set_raw(dst, r);
            }
            Extern::BytesNe => {
                let r = ctx.bytes_ne(task.get(arg(0)), task.get(arg(1)))?;
                task.set_raw(dst, r);
            }
            Extern::BytesCompare => {
                let r = ctx.bytes_compare(task.get(arg(0)), task.get(arg(1)))?;
                task.set_raw(dst, r);
            }
            Extern::BytesFindFrom => {
                let i =
                    ctx.bytes_find_from(task.get(arg(0)), task.raw(arg(1)), task.get(arg(2)))?;
                task.set_raw(dst, i);
            }
            Extern::BytesMake => {
                let w = ctx.bytes_make(task.raw(arg(0)), task.raw(arg(1)))?;
                task.set(dst, w);
            }
            Extern::BytesSet => {
                let w = ctx.bytes_set(task.raw(arg(0)), task.raw(arg(1)), task.get(arg(2)))?;
                task.set(dst, w);
            }
            Extern::BytesPush => {
                let w = ctx.bytes_push(task.raw(arg(0)), task.get(arg(1)))?;
                task.set(dst, w);
            }
            Extern::BytesFindByte => {
                let i =
                    ctx.bytes_find_byte(task.raw(arg(0)), task.raw(arg(1)), task.get(arg(2)))?;
                task.set_raw(dst, i);
            }
            Extern::StringDecode => {
                let w = ctx.string_decode(task.get(arg(0)))?;
                task.set(dst, w);
            }
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
                    task.get(arg(1)),
                    task.get(arg(2)),
                    task.get(arg(3)),
                    task.raw(arg(4)),
                    task.raw(arg(5)),
                    task.raw(arg(6)),
                )?;
                task.set(dst, w);
            }
            Extern::ProcWait => {
                let code = ctx.proc_wait(task.get(arg(0)))?;
                task.set_raw(dst, code);
            }
            Extern::ProcCloseStdin => {
                let w = ctx.proc_close_stdin(task.get(arg(0)))?;
                task.set(dst, w);
            }
            // いつも誤りを返すので、宛先には書かない。届け先は `failed` が決める (docs/spec/vm.md の「extern の中断」)
            Extern::FailIo => {
                let message = ctx.copy_string(task.get(arg(1)), usize::MAX)?;
                return Err(VmFault::Extern(Fault::Raised(Box::new(Raised {
                    kind: task.raw(arg(0)),
                    message: message.into_boxed_str(),
                }))));
            }
            Extern::FloatCompare => task.set_raw(
                dst,
                externs::float_compare(task.float(arg(0)), task.float(arg(1))),
            ),
            Extern::FloatShow => {
                let w = ctx.float_show(task.float(arg(0)));
                task.set(dst, w);
            }
            Extern::FloatShowsMinus => {
                task.set_raw(dst, externs::float_shows_minus(task.float(arg(0))))
            }
            Extern::FloatOfInt => task.set_float(dst, externs::float_of_int(task.raw(arg(0)))),
            Extern::FloatTruncate | Extern::FloatFloor | Extern::FloatCeil | Extern::FloatRound => {
                let convert = match ext {
                    Extern::FloatTruncate => externs::float_truncate,
                    Extern::FloatFloor => externs::float_floor,
                    Extern::FloatCeil => externs::float_ceil,
                    _ => externs::float_round,
                };
                let n = convert(task.float(arg(0))).map_err(Fault::Arith)?;
                task.set_raw(dst, n);
            }
            Extern::CharOfInt => {
                let c = externs::char_of_int(task.raw(arg(0))).map_err(Fault::Arith)?;
                task.set_raw(dst, c);
            }
            Extern::ByteOfInt => {
                let b = externs::byte_of_int(task.raw(arg(0))).map_err(Fault::Arith)?;
                task.set_raw(dst, b);
            }
            Extern::ByteNeg => {
                let r = externs::byte_negate(task.raw(arg(0))).map_err(Fault::Arith)?;
                task.set_raw(dst, r);
            }
            Extern::ByteAdd => {
                let r =
                    externs::byte_add(task.raw(arg(0)), task.raw(arg(1))).map_err(Fault::Arith)?;
                task.set_raw(dst, r);
            }
            Extern::ByteSub => {
                let r =
                    externs::byte_sub(task.raw(arg(0)), task.raw(arg(1))).map_err(Fault::Arith)?;
                task.set_raw(dst, r);
            }
            Extern::ByteMul => {
                let r =
                    externs::byte_mul(task.raw(arg(0)), task.raw(arg(1))).map_err(Fault::Arith)?;
                task.set_raw(dst, r);
            }
            Extern::ByteWrap => task.set_raw(dst, externs::byte_wrap(task.raw(arg(0)))),
            Extern::IntAnd | Extern::ByteAnd => {
                let r = externs::int_and(task.raw(arg(0)), task.raw(arg(1)));
                task.set_raw(dst, r);
            }
            Extern::IntOr | Extern::ByteOr => {
                let r = externs::int_or(task.raw(arg(0)), task.raw(arg(1)));
                task.set_raw(dst, r);
            }
            Extern::IntXor | Extern::ByteXor => {
                let r = externs::int_xor(task.raw(arg(0)), task.raw(arg(1)));
                task.set_raw(dst, r);
            }
            Extern::IntComplement => task.set_raw(dst, externs::int_complement(task.raw(arg(0)))),
            Extern::ByteComplement => task.set_raw(dst, externs::byte_complement(task.raw(arg(0)))),
            Extern::IntShl => {
                let r = externs::int_shl(task.raw(arg(0)), task.raw(arg(1)));
                task.set_raw(dst, r);
            }
            Extern::IntShr => {
                let r = externs::int_shr(task.raw(arg(0)), task.raw(arg(1)));
                task.set_raw(dst, r);
            }
            Extern::ByteShl => {
                let r = externs::byte_shl(task.raw(arg(0)), task.raw(arg(1)));
                task.set_raw(dst, r);
            }
            Extern::ByteShr => {
                let r = externs::byte_shr(task.raw(arg(0)), task.raw(arg(1)));
                task.set_raw(dst, r);
            }
            Extern::FloatSqrt
            | Extern::FloatAbs
            | Extern::FloatSin
            | Extern::FloatCos
            | Extern::FloatExp
            | Extern::FloatLog => {
                let f = match ext {
                    Extern::FloatSqrt => externs::float_sqrt,
                    Extern::FloatAbs => externs::float_abs,
                    Extern::FloatSin => externs::float_sin,
                    Extern::FloatCos => externs::float_cos,
                    Extern::FloatExp => externs::float_exp,
                    _ => externs::float_log,
                };
                task.set_float(dst, f(task.float(arg(0))));
            }
            Extern::FloatPow => task.set_float(
                dst,
                externs::float_pow(task.float(arg(0)), task.float(arg(1))),
            ),
            Extern::FloatInfinity => task.set_float(dst, externs::float_infinity()),
            Extern::FloatNan => task.set_float(dst, externs::float_nan()),
            Extern::FloatToFixed => {
                let w = ctx.float_to_fixed(task.raw(arg(0)), task.float(arg(1)))?;
                task.set(dst, w);
            }
            Extern::FloatCanParse => {
                let parsed = ctx.string_to_float(task.get(arg(0)))?;
                task.set_raw(
                    dst,
                    if parsed.is_some() {
                        externs::TRUE
                    } else {
                        externs::FALSE
                    },
                );
            }
            // `Std.Float.parse` は読める文字列にだけ呼ぶ。読めない文字列には NaN を返す
            Extern::FloatParseRaw => {
                let parsed = ctx.string_to_float(task.get(arg(0)))?;
                task.set_float(dst, parsed.unwrap_or(f64::NAN));
            }
            Extern::ArrayMake | Extern::MutArrayMake => {
                let lin = ext == Extern::MutArrayMake;
                let w = ctx.array_make(task.raw(arg(0)), task.get(arg(1)), lin)?;
                task.set(dst, w);
            }
            Extern::MutArrayEmpty => {
                let w = ctx.array_empty(true)?;
                task.set(dst, w);
            }
            Extern::ArrayThaw => {
                let w = ctx.array_thaw(task.get(arg(0)))?;
                task.set(dst, w);
            }
            Extern::MutArrayFreeze => {
                let w = ctx.array_freeze(task.get(arg(0)))?;
                task.set(dst, w);
            }
            Extern::CheckIndex => {
                externs::check_index(task.raw(arg(0)), task.raw(arg(1)))?;
                task.set(dst, Word::UNIT);
            }
            Extern::Panic
            | Extern::Exit
            | Extern::IntNeg
            | Extern::IntAdd
            | Extern::IntSub
            | Extern::IntMul
            | Extern::IntDiv
            | Extern::IntMod
            | Extern::ByteDiv
            | Extern::ByteMod
            | Extern::IntEq
            | Extern::IntNe
            | Extern::IntCompare
            | Extern::IntLt
            | Extern::IntLe
            | Extern::IntGt
            | Extern::IntGe
            | Extern::CharEq
            | Extern::CharNe
            | Extern::CharCompare
            | Extern::CharLt
            | Extern::CharLe
            | Extern::CharGt
            | Extern::CharGe
            | Extern::ByteEq
            | Extern::ByteNe
            | Extern::ByteCompare
            | Extern::ByteLt
            | Extern::ByteLe
            | Extern::ByteGt
            | Extern::ByteGe
            | Extern::CharToInt
            | Extern::CharOfByte
            | Extern::ByteToInt
            | Extern::FloatNeg
            | Extern::FloatAdd
            | Extern::FloatSub
            | Extern::FloatMul
            | Extern::FloatDiv
            | Extern::FloatMod
            | Extern::FloatEq
            | Extern::FloatNe
            | Extern::FloatLt
            | Extern::FloatLe
            | Extern::FloatGt
            | Extern::FloatGe
            | Extern::BoolEq
            | Extern::BoolNe
            | Extern::StrByteLength
            | Extern::StrByteAt
            | Extern::BytesLength
            | Extern::BytesGet
            | Extern::ShowBytesLength
            | Extern::ShowBytesGet
            | Extern::ArrayLength
            | Extern::MutArrayLength
            | Extern::ArrayGet
            | Extern::MutArrayRead
            | Extern::ArraySet
            | Extern::MutArrayWrite => {
                return Err(VmFault::Internal(
                    "a dedicated-instruction extern reached the generic extern instruction",
                ));
            }
            Extern::Par => {
                return Err(VmFault::Internal(
                    "`Std.Par.par` is not an extern instruction",
                ));
            }
            // コード生成が `Async` の命令にする (docs/spec/vm.md の「`Async` のタスク」)
            Extern::AsyncStartRaw
            | Extern::AsyncRunRaw
            | Extern::AsyncPark
            | Extern::AsyncReady
            | Extern::AsyncFinishOk
            | Extern::AsyncFinishIo
            | Extern::AsyncFinishPanic
            | Extern::AsyncTaskCell
            | Extern::AsyncTakeResult
            | Extern::AsyncRunResult
            | Extern::AsyncDetach
            | Extern::AsyncCancelRaw
            | Extern::AsyncShieldEnter
            | Extern::AsyncShieldLeave
            | Extern::AsyncStartInRaw
            | Extern::AsyncOpenScope
            | Extern::AsyncCloseScope
            | Extern::AsyncLeaveScope
            | Extern::AsyncCancelScopeRaw
            | Extern::AsyncFailScopeIo
            | Extern::AsyncFailScopePanic
            | Extern::AsyncCancelPoint
            | Extern::AsyncSpendBudget => {
                return Err(VmFault::Internal("Async externs run through Op::Async"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        }
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
}
