//! 命令を実行するループ (docs/spec/vm.md)。状態は `Ctx` と `Task` だけに置き、安全点をまたいで外に持たない。呼び出しの
//! スタックを Rust の再帰でなく `Task` のデータに持つので、後の協調的なタスクの切り替えでタスクごとに取り替えられる
//! (docs/future/multicore.md の「VM の並列化の備え」の制約5)。

use eml_extern::{Extern, ExternEffect, Purity};
use eml_rt::externs::{self, Fault};
use eml_rt::{Ctx, Mode, Stream, Token, Word};

use crate::RunCounts;
use crate::apply;
use crate::code::CodeBase;
use crate::data;
use crate::error::VmFault;
use crate::op::{APPLY_REST, FINISH, Op, Pc, Slot};

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
    /// `exit` の終了コード。立っていれば、中断が `Finish` まで届く (docs/spec/vm.md の「extern の中断」)。
    pub(crate) exit: Option<i64>,
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
            exit: None,
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
            Err(fault) => {
                step_failed(code, ctx, task, pc, fault).map_err(|fault| Failure { fault, pc })?
            }
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
        Op::CallN { at } => call_many(code, ctx, task, at)?,
        Op::RetN { at } => return_many(code, ctx, task, pc, at)?,
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
            let w = apply::make_closure(code, ctx, f, lin, &task.scratch)?;
            task.scratch.clear();
            task.set(dst, w);
        }
        Op::Apply { dst, at } => {
            let target = apply::Target::Frame { ret: task.pc, dst };
            apply_call(code, ctx, task, at, target)?;
        }
        Op::Clause { dst, at } => {
            let target = apply::Target::Frame { ret: task.pc, dst };
            clause_call(code, ctx, task, at, target)?;
        }
        Op::TailApply { at } => apply_call(code, ctx, task, at, apply::Target::Tail)?,
        Op::TailClause { at } => clause_call(code, ctx, task, at, apply::Target::Tail)?,
        Op::TailSelf { at } => {
            let o = ops(at);
            task.load_scratch(slots(&o[2..2 + o[1] as usize]));
            task.store_scratch();
            task.pc = o[0];
            // バイトコードに後ろ向きの辺ができるので、ここも安全点にする (docs/spec/vm.md の「呼び出しと戻り」)
            ctx.poll(task.depth())?;
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
        Op::UnboxStruct { src, at } => data::unbox_struct(code, ctx, task, src, at)?,
        Op::ReleaseUnboxed { src, at } => data::release_unboxed(code, ctx, task, src, at)?,
        Op::Con { dst, at } => data::con(code, ctx, task, dst, at)?,
        Op::ReleaseReuse { src, at } => data::release_reuse(code, ctx, task, src, at)?,
        Op::ConReuse { dst, at } => data::con_reuse(code, ctx, task, counts, dst, at)?,
        // token は verifier を通った IR の、このヒープのまだ使っていない token である
        Op::DropToken { tok } => {
            unsafe { ctx.heap_mut().drop_token(Token::from_word(task.get(tok))) }?
        }
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
        Op::BoxFloat { dst, src } => {
            if M::CHECKED {
                counts.boxes += 1;
            }
            let w = unsafe { ctx.heap_mut() }.alloc_float(task.raw(src) as u64);
            task.set(dst, w);
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
        Op::UnboxFloat { dst, src } => {
            let bits = data::unbox_float(ctx, task.get(src), counts)?;
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
        Op::ArraySet { dst, at } => {
            let o = &code.operands[at as usize..];
            let (a, i, x) = (
                task.get(o[0] as Slot),
                task.raw(o[1] as Slot),
                task.get(o[2] as Slot),
            );
            let w = unsafe { ctx.array_set(a, i, x) }?;
            task.set(dst, w);
        }
        Op::Extern { dst, at } => extern_call(code, ctx, task, counts, dst, at)?,
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
            let items = &code.mask_lists[o[0] as usize..(o[0] + o[1]) as usize];
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
            unwind(code, ctx, task, counts, ev)?;
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

/// `apply` の命令。オペランドは [呼ばれる値, n, 引数…] である。引数は全部読んでから書くので、結果の
/// スロットがオペランドのスロットと同じ色でもよい (docs/spec/vm.md の「スロットの割り当て」)。`step` に展開するのは
/// `bench/apply_int.em` の命令の数がそのほうが少なかったためである。
#[inline(always)]
fn apply_call<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    at: u32,
    target: apply::Target,
) -> Result<(), VmFault> {
    let o = &code.operands[at as usize..];
    task.load_scratch(slots(&o[2..2 + o[1] as usize]));
    let callee = task.get(o[0] as Slot);
    apply::apply(code, ctx, task, callee, target)?;
    ctx.poll(task.depth())?;
    Ok(())
}

/// `clause` の命令。オペランドは [呼ばれる値, n, 引数…] である。
fn clause_call<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    at: u32,
    target: apply::Target,
) -> Result<(), VmFault> {
    let o = &code.operands[at as usize..];
    task.load_scratch(slots(&o[2..2 + o[1] as usize]));
    let callee = task.get(o[0] as Slot);
    apply::apply_clause(code, ctx, task, callee, target)?;
    ctx.poll(task.depth())?;
    Ok(())
}

/// 呼ぶ側の上に呼ばれる側のフレームを作り、引数を写して先頭へ進む。呼ぶ側のスロットはスタックに残る。`CallN` からも
/// 呼ぶので、`Call` の速い道に展開し続けるように `inline(always)` にする。
#[inline(always)]
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

// 複数の値の命令は `step` に展開しない。展開すると `step` が大きくなり、値が1つの `Call` と `Ret` の速い道の命令の数が
// 増えた (`bench/run.sh` の `fib` で約 4%)。値が1つの呼び出しの速さを変えないためである

/// 複数の値を受ける直接の呼び出し。結果のスロットは呼ばれる側の `RetN` が読むので、見出しの結果のスロットは使わない。
#[inline(never)]
fn call_many<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    at: u32,
) -> Result<(), VmFault> {
    call(code, task, 0, at);
    ctx.poll(task.depth())?;
    Ok(())
}

/// `RetN` の命令。`pc` はその位置で、`Ret` と同じく最後に戻った関数として覚え、戻りを安全点にする。
#[inline(never)]
fn return_many<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    pc: Pc,
    at: u32,
) -> Result<(), VmFault> {
    ret_many::<M>(code, task, at)?;
    task.last_ret = pc;
    ctx.poll(task.depth())?;
    Ok(())
}

/// 複数の値の戻り。値を書くスロットは見出しの戻り先の直前の `CallN` から読む。見出しの
/// 形は変えずに済み、末尾呼び出しが見出しを引き継げば、そのまま末尾で呼んだ側の呼び出し元に書ける
/// (docs/spec/vm.md の「呼び出しと戻り」)。値は呼ばれる側のフレームから
/// 呼び出し元のフレームへ写すので、読む所と書く所は重ならない。
#[inline]
fn ret_many<M: Mode>(code: &CodeBase<M>, task: &mut Task, at: u32) -> Result<(), VmFault> {
    let o = &code.operands[at as usize..];
    let n = o[0] as usize;
    let (ret, _, size) = read_header(task.stack[task.base - 2]);
    let dsts = receivers::<M>(code, ret, n)?;
    let caller = task.stack[task.base - 1].bits();
    for (&src, &dst) in o[1..1 + n].iter().zip(dsts) {
        task.stack[caller + dst as usize] = task.stack[task.base + src as usize];
    }
    task.base = caller;
    task.frame_size = size;
    task.pc = ret;
    Ok(())
}

/// 戻り先 `ret` の直前の、複数の値を受ける命令の結果のスロット。`Finish` と `ApplyRest` の位置へは値が1つしか
/// 戻らないので、ほかの命令の後への戻りは内部の誤りである。検査モードは値の数も比べる。
#[inline]
fn receivers<M: Mode>(code: &CodeBase<M>, ret: Pc, n: usize) -> Result<&[u32], VmFault> {
    let o = |at: u32| &code.operands[at as usize..];
    let dsts = match ret.checked_sub(1).map(|pc| code.code[pc as usize]) {
        Some(Op::CallN { at }) => {
            let o = o(at);
            let count = 2 + o[1] as usize;
            &o[count + 1..count + 1 + o[count] as usize]
        }
        _ => return Err(not_received()),
    };
    if M::CHECKED && dsts.len() != n {
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
fn unwind<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
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
            APPLY_REST => {
                // 余った引数はまだ渡していないこの枠の参照である
                for slot in apply::rest_arguments(task) {
                    unsafe { ctx.heap_mut().decref(task.get(slot)) }?;
                }
            }
            pc => {
                let entry = code
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
                    // 外す handler の状態は、その handler の継続を持てば evidence と輪になる。ここで取り出して
                    // 手放し、輪を切る。生き残った継続は `resume$` で新しい evidence と状態を作るので、古い状態は
                    // もう読まれない (docs/spec/vm.md の「巻き戻し」)
                    let state = unsafe { ctx.ev_take_state(ev) }?;
                    unsafe { ctx.heap_mut().decref(state) }?;
                }
                // 表のスロットは、LIR の所有の規則によりこのフレームが持つ参照である。`Lin` の物体も手放してよく、
                // `File` はここで閉じる
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
        let (ret, slot, size) = read_header(task.stack[task.base - 2]);
        task.base = task.stack[task.base - 1].bits();
        task.frame_size = size;
        task.pc = ret;
        dst = Some(slot);
        if M::CHECKED {
            counts.unwound_frames += 1;
        }
    }
}

/// 入口の関数の結果か、`exit` の中断を手放す (docs/spec/vm.md の「誤り、止めること、終わり」)。
fn finish<M: Mode>(code: &CodeBase<M>, ctx: &mut Ctx<M>, task: &Task) -> Result<(), VmFault> {
    if task.exit.is_some() {
        // 途中のフレームは巻き戻しで片付けてある。入口の結果はダミーなので手放さない。中断の途中の `evv` には
        // 意味がないので、空に戻す (docs/spec/vm.md の「extern の中断」)
        unsafe {
            ctx.yield_finish()?;
            ctx.evv_set(Word::UNIT)?;
        }
        return Ok(());
    }
    if code.functions[task.entry as usize].ret_rc && !task.keep_result {
        // 結果は入口の関数が返した、VM の持つ参照である
        unsafe { ctx.heap_mut().decref(task.get(0)) }?;
    }
    Ok(())
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
fn step_failed<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    pc: Pc,
    fault: VmFault,
) -> Result<(), VmFault> {
    let (dst, purity) = match code.code[pc as usize] {
        Op::IntAdd { dst, .. }
        | Op::IntSub { dst, .. }
        | Op::IntMul { dst, .. }
        | Op::IntDiv { dst, .. }
        | Op::IntRem { dst, .. }
        | Op::IntNeg { dst, .. }
        | Op::ArrayGet { dst, .. }
        | Op::ArraySet { dst, .. } => (dst, Purity::Panics),
        // `length` の行は `Pure` で巻き戻しの地点を持たない
        Op::ArrayLength { dst, .. } => (dst, Purity::Pure),
        _ => return Err(fault),
    };
    match fault {
        VmFault::Extern(fault) => failed(code, ctx, task, dst, fault, purity),
        other => Err(other),
    }
}

/// 算術と比較以外の extern。
fn extern_call<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    counts: &mut RunCounts,
    dst: Slot,
    at: u32,
) -> Result<(), VmFault> {
    match run_extern(code, ctx, task, counts, dst, at) {
        Err(VmFault::Extern(fault)) => {
            let ext = Extern::ALL[code.operands[at as usize] as usize];
            failed(code, ctx, task, dst, fault, ext.row().purity)
        }
        other => other,
    }
}

/// extern の誤りを、誤りの種類で決まる中断に変える。`Panic` に渡す誤りのうち、`Pure` でない行のものを `try_panic` に
/// 届ける。`Pure` の行は巻き戻しの地点を持たないので届けない (`Env.arg` は `Effectful` だが、範囲の外の位置は
/// `Panic` である)。`try_io` の捕まえる誤りは、`Effectful` の行のものだけを届ける。捕まえる handler がなければ、
/// 実行時エラーにする。失敗した extern は引数をすでに消費している。docs/spec/vm.md の「`IO` と `Panic` への中断」を
/// 見る。
fn failed<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    dst: Slot,
    fault: Fault,
    purity: Purity,
) -> Result<(), VmFault> {
    // `Pure` の行の extern は巻き戻しの地点を持たないので、誤りを返しても中断にしない (docs/spec/vm.md の「extern の中断」)
    let caught = if fault.panics() && purity != Purity::Pure {
        abort_to(code, ctx, task, dst, ExternEffect::Panic, PANICKED, |ctx| {
            [ctx.fresh_string(&fault.to_string())]
        })?
    } else if let Some(kind) = fault
        .io_error_kind()
        .filter(|_| purity == Purity::Effectful)
    {
        abort_to(code, ctx, task, dst, ExternEffect::Io, IO_FAILED, |ctx| {
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
fn abort_to<M: Mode, const N: usize>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    dst: Slot,
    effect: ExternEffect,
    op: usize,
    args: impl FnOnce(&mut Ctx<M>) -> [Word; N],
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
    apply::apply_clause(code, ctx, task, clause, apply::Target::Frame { ret, dst })?;
    Ok(true)
}

/// 行の番号の `match` で `Ctx` のメソッドを呼ぶ (docs/spec/object-model.md の「extern」)。
fn run_extern<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    counts: &mut RunCounts,
    dst: Slot,
    at: u32,
) -> Result<(), VmFault> {
    let ops = &code.operands[at as usize..];
    let arg = |i: usize| ops[2 + i] as Slot;
    // 語の引数は、LIR の所有の規則により VM が持つ参照で、extern がそれを消費する
    let ext = Extern::ALL[ops[0] as usize];
    if ext == Extern::Panic {
        let message = task.get(arg(0));
        if abort_to(code, ctx, task, dst, ExternEffect::Panic, PANICKED, |_| {
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
        return unwind(code, ctx, task, counts, dummy);
    }
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
            Extern::ReadAll | Extern::ReadLine => {
                let w = ctx.fs_read(task.get(arg(0)), matches!(ext, Extern::ReadLine))?;
                task.set(dst, w);
            }
            Extern::Close => {
                ctx.fs_close(task.get(arg(0)))?;
                task.set(dst, Word::UNIT);
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
            Extern::EnvLookup => {
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
            // 配列の行は、借りる引数 (`FunctionRow::borrowed`) を手放さない。失敗しても、借りた配列は呼んだ側の
            // スロットに残り、巻き戻しの表が手放す (docs/spec/lir.md の「extern の借用」)
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
