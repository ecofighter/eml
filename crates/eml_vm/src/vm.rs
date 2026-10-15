//! 命令を実行するループ (docs/spec/vm.md)。状態は `Ctx` と `Task` だけに置き、安全点をまたいで外に持たない。呼び出しの
//! スタックを Rust の再帰でなく `Task` のデータに持つので、後の協調的なタスクの切り替えでタスクごとに取り替えられる
//! (docs/future/multicore.md の「VM の並列化の備え」の制約5)。

use eml_extern::Extern;
use eml_rt::{Ctx, Mode, Word, externs};

use crate::RunCounts;
use crate::apply;
use crate::code::CodeBase;
use crate::data;
use crate::error::VmFault;
use crate::op::{FINISH, Op, Pc, Slot};
use crate::prims;

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
    pub(crate) fn new<M: Mode>(code: &CodeBase<M>) -> Task {
        let entry = &code.functions[code.entry as usize];
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

    pub(crate) fn reserve(&mut self, end: usize) {
        if self.stack.len() < end {
            self.stack.resize(end, Word::UNIT);
        }
    }

    /// scratch の引数を今の base のスロット 0.. に書き、`f` の先頭へ進む。見出しはそのまま使う。
    pub(crate) fn enter_in_place<M: Mode>(&mut self, code: &CodeBase<M>, f: u32) {
        let entry = &code.functions[f as usize];
        self.reserve(self.base + usize::from(entry.frame_size));
        let Task {
            stack,
            scratch,
            base,
            ..
        } = self;
        for (i, w) in scratch.drain(..).enumerate() {
            stack[*base + i] = w;
        }
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
#[inline(always)]
fn step<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    counts: &mut RunCounts,
    pc: Pc,
) -> Result<bool, VmFault> {
    task.pc = pc + 1;
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
            let ops = &code.operands[at as usize..];
            let n = ops[1] as usize;
            task.scratch.clear();
            for &slot in &ops[2..2 + n] {
                let w = task.get(slot as Slot);
                task.scratch.push(w);
            }
            task.enter_in_place(code, ops[0]);
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
            let ops = &code.operands[at as usize..];
            let (f, lin, n) = (ops[0], ops[1] != 0, ops[2] as usize);
            task.scratch.clear();
            for &slot in &ops[3..3 + n] {
                let w = task.get(slot as Slot);
                task.scratch.push(w);
            }
            let w = apply::make_closure(ctx, f, lin, &task.scratch)?;
            task.scratch.clear();
            task.set(dst, w);
        }
        Op::Apply { dst, at } | Op::Clause { dst, at } => {
            let clause = matches!(code.code[pc as usize], Op::Clause { .. });
            let callee = load_args(code, task, at);
            let target = apply::Target::Frame { ret: task.pc, dst };
            apply::apply(code, ctx, task, callee, target, clause)?;
            ctx.poll(task.depth())?;
        }
        Op::TailApply { at } | Op::TailClause { at } => {
            let clause = matches!(code.code[pc as usize], Op::TailClause { .. });
            let callee = load_args(code, task, at);
            apply::apply(code, ctx, task, callee, apply::Target::Tail, clause)?;
            ctx.poll(task.depth())?;
        }
        Op::TailSelf { at } => {
            let ops = &code.operands[at as usize..];
            let n = ops[1] as usize;
            task.scratch.clear();
            for &slot in &ops[2..2 + n] {
                let w = task.get(slot as Slot);
                task.scratch.push(w);
            }
            let Task {
                stack,
                scratch,
                base,
                ..
            } = task;
            for (i, w) in scratch.drain(..).enumerate() {
                stack[*base + i] = w;
            }
            task.pc = ops[0];
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
            let tag = unsafe { ctx.heap().meta(w) }?.tag();
            data::switch_tag(code, ctx, task, w, tag.into(), at)?
        }
        Op::SwitchTObj { src, at } => {
            let w = task.get(src);
            let tag = if w.is_imm() {
                w.imm_value()
            } else {
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
        other => {
            if !prims::step(code, ctx, task, other)? {
                unreachable!("every instruction has an arm");
            }
        }
    }
    Ok(true)
}

/// `[呼ばれる値, n, 引数…]` から引数を scratch に移し、呼ばれる値を返す。全部読んでから書くので、結果のスロットが
/// オペランドのスロットと同じ色でもよい (docs/spec/vm.md の「スロットの割り当て」)。
fn load_args<M: Mode>(code: &CodeBase<M>, task: &mut Task, at: u32) -> Word {
    let ops = &code.operands[at as usize..];
    let n = ops[1] as usize;
    task.scratch.clear();
    for &slot in &ops[2..2 + n] {
        let w = task.get(slot as Slot);
        task.scratch.push(w);
    }
    task.get(ops[0] as Slot)
}

/// 呼ぶ側の上に呼ばれる側のフレームを作り、引数を写して先頭へ進む。呼ぶ側のスロットはスタックに残るので、`saved` は
/// 読まない。
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

/// 入口の関数の結果を手放す。CEK のいちばん下のフレームも、結果を手放してから終わる。
fn finish<M: Mode>(code: &CodeBase<M>, ctx: &mut Ctx<M>, task: &Task) -> Result<(), VmFault> {
    if code.entry_ret_rc {
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
    unsafe {
        match Extern::ALL[ops[0] as usize] {
            Extern::Println => {
                ctx.println(task.get(arg(0)))?;
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
            _ => {
                return Err(VmFault::Internal(
                    "an arithmetic extern reached the generic extern instruction",
                ));
            }
        }
    }
    Ok(())
}
