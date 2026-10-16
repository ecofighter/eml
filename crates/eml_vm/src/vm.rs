//! 命令を実行するループ (docs/spec/vm.md)。状態は `Ctx` と `Task` だけに置き、安全点をまたいで外に持たない。呼び出しの
//! スタックを Rust の再帰でなく `Task` のデータに持つので、後の協調的なタスクの切り替えでタスクごとに取り替えられる
//! (docs/future/multicore.md の「VM の並列化の備え」の制約5)。

use eml_extern::Extern;
use eml_rt::{Ctx, Mode, Stream, Word, externs};

use crate::RunCounts;
use crate::apply;
use crate::code::CodeBase;
use crate::data;
use crate::error::VmFault;
use crate::op::{FINISH, Op, Pc, Slot};

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
}

/// 誤りと、それが起きた命令の位置。
pub(crate) struct Failure {
    pub(crate) fault: VmFault,
    pub(crate) pc: Pc,
}

// いちばん下に置く空のフレーム。入口の関数の結果を受けるスロットを1つ持つ
const ROOT_BASE: usize = 2;
const ROOT_SIZE: usize = 1;

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

    /// 使っている語の数。安全点の深さである (docs/spec/object-model.md の「実行の制御」)。
    pub(crate) fn depth(&self) -> usize {
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

    /// scratch の語を今の base のスロット 0.. に書き、scratch を空にする。
    pub(crate) fn store_scratch(&mut self) {
        let Task {
            stack,
            scratch,
            base,
            ..
        } = self;
        for (i, w) in scratch.drain(..).enumerate() {
            stack[*base + i] = w;
        }
    }

    /// scratch の引数を今の base のスロット 0.. に書き、`f` の先頭へ進む。見出しはそのまま使う。
    pub(crate) fn enter_in_place<M: Mode>(&mut self, code: &CodeBase<M>, f: u32) {
        let entry = &code.functions[f as usize];
        self.reserve(self.base + usize::from(entry.frame_size));
        self.store_scratch();
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
        let (pc, dst, size) = read_header(self.stack[self.base - 2]);
        self.base = self.stack[self.base - 1].bits();
        self.frame_size = size;
        self.pc = pc;
        self.set(dst, value);
    }
}

/// スタックの容量の上限。深さが `max_depth` を超えると次の安全点で止まり、安全点の前には通常、1つの枠と見出し
/// (`u16::MAX + 2` 語) までを積むので、それより大きい容量は見込まない。余る引数の適用は2つ積みうるが、`reserve` は
/// 求める長さを下回らない (docs/spec/vm.md の「スロットとスタック」)。
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

pub(crate) fn run<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    counts: &mut RunCounts,
) -> Result<(), Failure> {
    loop {
        let pc = task.pc;
        match step(code, ctx, task, counts, pc) {
            Ok(true) => {}
            Ok(false) => return Ok(()),
            Err(fault) => return Err(Failure { fault, pc }),
        }
    }
}

/// 命令を1つ実行する。`Finish` に着いたら `false` を返す。
///
/// 結果のスロットは引数のスロットと同じ色でありうるので (docs/spec/vm.md の「スロットの割り当て」)、どの腕も引数を
/// 読み終えてから結果を書く。
#[inline(always)]
fn step<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    counts: &mut RunCounts,
    pc: Pc,
) -> Result<bool, VmFault> {
    task.pc = pc + 1;
    let ops = |at: u32| &code.operands[at as usize..];
    match code.code[pc as usize] {
        Op::Move { dst, src } => task.set(dst, task.get(src)),
        Op::LoadRaw { dst, value } => task.set_raw(dst, value.into()),
        Op::LoadImm { dst, value } => task.set(dst, Word::imm(value.into())),
        Op::LoadConst { dst, index } => task.set(dst, code.constants[index as usize]),
        Op::Goto { target } => task.pc = target,
        Op::Ret { src } => {
            let value = task.get(src);
            task.last_ret = pc;
            task.return_value(value);
            ctx.poll(task.depth())?;
        }
        Op::Call { dst, at } => {
            call(code, task, dst, at);
            ctx.poll(task.depth())?;
        }
        Op::TailCall { at } => {
            let o = ops(at);
            task.load_scratch(slots(&o[2..2 + o[1] as usize]));
            task.enter_in_place(code, o[0]);
            ctx.poll(task.depth())?;
        }
        Op::Finish => {
            finish(code, ctx, task)?;
            return Ok(false);
        }
        Op::ApplyRest => {
            apply::apply_rest(code, ctx, task)?;
            ctx.poll(task.depth())?;
        }
        Op::Closure { dst, at } => {
            let o = ops(at);
            let (f, lin, n) = (o[0], o[1] != 0, o[2] as usize);
            task.load_scratch(slots(&o[3..3 + n]));
            let w = apply::make_closure(ctx, f, lin, &task.scratch)?;
            task.scratch.clear();
            task.set(dst, w);
        }
        Op::Apply { dst, at } => {
            let target = apply::Target::Frame { ret: task.pc, dst };
            apply_call(code, ctx, task, at, target, false)?;
        }
        Op::Clause { dst, at } => {
            let target = apply::Target::Frame { ret: task.pc, dst };
            apply_call(code, ctx, task, at, target, true)?;
        }
        Op::TailApply { at } => apply_call(code, ctx, task, at, apply::Target::Tail, false)?,
        Op::TailClause { at } => apply_call(code, ctx, task, at, apply::Target::Tail, true)?,
        Op::TailSelf { at } => {
            let o = ops(at);
            task.load_scratch(slots(&o[2..2 + o[1] as usize]));
            task.store_scratch();
            task.pc = o[0];
            // バイトコードに後ろ向きの辺ができるので、ここも安全点にする (docs/spec/vm.md の「呼び出しと戻り」)
            ctx.poll(task.depth())?;
        }
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
        Op::BoolEq { dst, a, b } => task.set_raw(dst, externs::bool_eq(task.raw(a), task.raw(b))),
        Op::BoolNe { dst, a, b } => task.set_raw(dst, externs::bool_ne(task.raw(a), task.raw(b))),
        Op::SwitchEnum { src, at } => {
            data::switch_tag(code, ctx, task, task.get(src), task.raw(src), at)?
        }
        Op::SwitchObj { src, at } => {
            let w = task.get(src);
            // scrutinee の Repr は `obj` なので、語はポインタである (docs/spec/vm.md の「switch」)
            let tag = unsafe { ctx.heap().meta(w) }?.tag();
            data::switch_tag(code, ctx, task, w, tag.into(), at)?
        }
        Op::SwitchTObj { src, at } => {
            let w = task.get(src);
            let tag = if w.is_imm() {
                w.imm_value()
            } else {
                // `is_imm()` の枝を先に取ったので、ここでは語はポインタである (docs/spec/vm.md の「switch」)
                unsafe { ctx.heap().meta(w) }?.tag().into()
            };
            data::switch_tag(code, ctx, task, w, tag, at)?
        }
        Op::SwitchInt { src, at } => data::switch_int(code, task, task.raw(src), at)?,
        Op::SwitchString { src, at } => data::switch_string(code, ctx, task, task.get(src), at)?,
        Op::Unpack { src, at } => data::unpack(code, ctx, task, src, at)?,
        // RC の命令は Repr が `obj` か `tobj` の変数にだけ出す。VM はその参照を持つか借りている
        Op::Dup { src } => unsafe { ctx.heap_mut().dup(task.get(src)) }?,
        Op::Decref { src } => unsafe { ctx.heap_mut().decref(task.get(src)) }?,
        Op::Release { src, at } => data::release(code, ctx, task, src, at)?,
        Op::Con { dst, at } => data::con(code, ctx, task, dst, at)?,
        Op::BoxInt { dst, src } => {
            if M::CHECKED {
                counts.boxes += 1;
            }
            let w = unsafe { ctx.heap_mut() }.alloc_int(task.raw(src));
            task.set(dst, w);
        }
        Op::BoxEnum { dst, src } => {
            if M::CHECKED {
                counts.boxes += 1;
            }
            task.set(dst, Word::imm(task.raw(src)));
        }
        Op::BoxConst { dst, index } => {
            if M::CHECKED {
                counts.boxes += 1;
            }
            task.set(dst, code.constants[index as usize]);
        }
        Op::Unbox { dst, src } => {
            let n = data::unbox(ctx, task.get(src), counts)?;
            task.set_raw(dst, n);
        }
        Op::Extern { dst, at } => extern_call(code, ctx, task, dst, at)?,
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
            let list = &code.effect_lists[o[0] as usize..(o[0] + o[1]) as usize];
            let w = unsafe { ctx.evv_mask(list, task.get(o[2] as Slot)) }?;
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
        Op::YieldAbort { dst, at } => {
            let o = ops(at);
            let (ev, clause, k) = (
                task.get(o[0] as Slot),
                task.get(o[1] as Slot),
                task.get(o[2] as Slot),
            );
            unsafe { ctx.yield_abort(ev, clause, k) }?;
            task.set(dst, Word::UNIT);
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
        Op::RootSet { dst, at } => {
            let o = ops(at);
            unsafe { ctx.root_set(o[0], task.get(o[1] as Slot)) }?;
            task.set(dst, Word::UNIT);
        }
    }
    Ok(true)
}

/// オペランドの表のスロットの並び。
fn slots(list: &[u32]) -> impl Iterator<Item = Slot> {
    list.iter().map(|&slot| slot as Slot)
}

/// `apply` と `clause` の命令。`clause` は照合した命令の種類から受ける。
fn apply_call<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    at: u32,
    target: apply::Target,
    clause: bool,
) -> Result<(), VmFault> {
    let callee = load_args(code, task, at);
    apply::apply(code, ctx, task, callee, target, clause)?;
    ctx.poll(task.depth())?;
    Ok(())
}

/// `[呼ばれる値, n, 引数…]` から引数を scratch に移し、呼ばれる値を返す。全部読んでから書くので、結果のスロットが
/// オペランドのスロットと同じ色でもよい (docs/spec/vm.md の「スロットの割り当て」)。
fn load_args<M: Mode>(code: &CodeBase<M>, task: &mut Task, at: u32) -> Word {
    let o = &code.operands[at as usize..];
    task.load_scratch(slots(&o[2..2 + o[1] as usize]));
    task.get(o[0] as Slot)
}

/// 呼ぶ側の上に呼ばれる側のフレームを作り、引数を写して先頭へ進む。呼ぶ側のスロットはスタックに残る。
fn call<M: Mode>(code: &CodeBase<M>, task: &mut Task, dst: Slot, at: u32) {
    let ops = &code.operands[at as usize..];
    let n = ops[1] as usize;
    let entry = &code.functions[ops[0] as usize];
    let caller = task.base;
    let base = task.push_frame(task.pc, dst, usize::from(entry.frame_size));
    for (i, &slot) in ops[2..2 + n].iter().enumerate() {
        task.stack[base + i] = task.stack[caller + slot as usize];
    }
    task.pc = entry.start;
}

/// 入口の関数の結果を手放す (docs/spec/vm.md の「誤り、止めること、終わり」)。
fn finish<M: Mode>(code: &CodeBase<M>, ctx: &mut Ctx<M>, task: &Task) -> Result<(), VmFault> {
    if code.functions[task.entry as usize].ret_rc && !task.keep_result {
        // 結果は入口の関数が返した、VM の持つ参照である
        unsafe { ctx.heap_mut().decref(task.get(0)) }?;
    }
    Ok(())
}

/// 算術と比較以外の extern。行の番号の `match` で `Ctx` のメソッドを呼ぶ (docs/spec/object-model.md の「extern」)。
fn extern_call<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    dst: Slot,
    at: u32,
) -> Result<(), VmFault> {
    let ops = &code.operands[at as usize..];
    let arg = |i: usize| ops[2 + i] as Slot;
    // 語の引数は、LIR の所有の規則により VM が持つ参照で、extern がそれを消費する
    let ext = Extern::ALL[ops[0] as usize];
    unsafe {
        match ext {
            Extern::Println => {
                ctx.write_string(Stream::Out, task.get(arg(0)))?;
                task.set(dst, Word::UNIT);
            }
            Extern::Open => {
                let w = ctx.fs_open(task.get(arg(0)))?;
                task.set(dst, w);
            }
            Extern::ReadAll => {
                let w = ctx.fs_read_all(task.get(arg(0)))?;
                task.set(dst, w);
            }
            Extern::Close => {
                ctx.fs_close(task.get(arg(0)))?;
                task.set(dst, Word::UNIT);
            }
            Extern::IntShow => {
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
                ctx.write_string(Stream::Err, task.get(arg(0)))?;
                task.set(dst, Word::UNIT);
            }
            Extern::StrByteLength => {
                let n = ctx.string_byte_length(task.get(arg(0)))?;
                task.set_raw(dst, n);
            }
            Extern::StrByteAt => {
                let b = ctx.string_byte_at(task.raw(arg(0)), task.get(arg(1)))?;
                task.set_raw(dst, b);
            }
            Extern::StrSlice => {
                let w = ctx.string_slice(task.raw(arg(0)), task.raw(arg(1)), task.get(arg(2)))?;
                task.set(dst, w);
            }
            Extern::StrFindFrom => {
                let i =
                    ctx.string_find_from(task.get(arg(0)), task.raw(arg(1)), task.get(arg(2)))?;
                task.set_raw(dst, i);
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
            Extern::EnvVar => {
                let w = ctx.env_var(task.get(arg(0)))?;
                task.set(dst, w);
            }
            Extern::StdinReadAll | Extern::StdinReadLine => {
                let w = ctx.stdin_read(matches!(ext, Extern::StdinReadLine))?;
                task.set(dst, w);
            }
            Extern::WriteText => {
                ctx.fs_write_text(task.get(arg(0)), task.get(arg(1)))?;
                task.set(dst, Word::UNIT);
            }
            Extern::ProcStatus => {
                let code = ctx.proc_status(task.get(arg(0)), task.get(arg(1)))?;
                task.set_raw(dst, code);
            }
            Extern::IntNeg
            | Extern::IntAdd
            | Extern::IntSub
            | Extern::IntMul
            | Extern::IntDiv
            | Extern::IntMod
            | Extern::IntEq
            | Extern::IntNe
            | Extern::IntCompare
            | Extern::IntLt
            | Extern::IntLe
            | Extern::IntGt
            | Extern::IntGe
            | Extern::BoolEq
            | Extern::BoolNe => {
                return Err(VmFault::Internal(
                    "an arithmetic extern reached the generic extern instruction",
                ));
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
