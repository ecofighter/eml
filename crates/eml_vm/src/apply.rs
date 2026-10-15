//! 関数の値の適用とクロージャ (docs/spec/vm.md の「呼び出しと戻り」)。引数は `Task::scratch` に置いて渡す。

use eml_rt::{Ctx, Mode, Word};

use crate::code::CodeBase;
use crate::data::count_word;
use crate::error::VmFault;
use crate::op::{APPLY_REST, Pc, Slot};
use crate::vm::Task;

/// 適用の結果の行き先。
pub(crate) enum Target {
    /// 今のフレームの上に呼ばれる側を積み、戻り先 `ret` で結果を `dst` に受ける。
    Frame { ret: Pc, dst: Slot },
    /// 今のフレームを置き換える (末尾の位置)。
    Tail,
}

/// `callee` を `task.scratch` の引数に適用する。`clause` なら、引数の数はいつも入口の関数の引数の数と等しい。
/// `clause` の引数は操作の ABI の Repr (生の `int` を含む) を持つので、部分適用でクロージャの走査する語に入れては
/// ならないためである。
pub(crate) fn apply<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    callee: Word,
    target: Target,
    clause: bool,
) -> Result<(), VmFault> {
    let not_a_function = VmFault::Internal("applying a value that is not a function");
    let (f, lin) = if callee.is_imm() {
        (callee.imm_value(), false)
    } else {
        // VM は `callee` の参照を1つ持つ。中身を取り出した後、その参照を手放すか箱を空ける
        let heap = unsafe { ctx.heap_mut() };
        unsafe {
            let lin = heap.meta(callee)?.lin();
            let len = heap.scanned_len(callee)?;
            // 走査する語のない物体の0番目を読むと、断る誤りが範囲外の誤りになる
            if M::CHECKED && len == 0 {
                return Err(not_a_function);
            }
            let function = heap.scanned(callee, 0)?;
            if M::CHECKED && !function.is_imm() {
                return Err(not_a_function);
            }
            let captured: Result<Vec<Word>, _> =
                (1..len).map(|i| heap.scanned(callee, i)).collect();
            let captured = captured?;
            if heap.is_last(callee)? {
                heap.free_block(callee)?;
            } else {
                for &w in &captured {
                    heap.dup(w)?;
                }
                heap.decref(callee)?;
            }
            task.scratch.splice(0..0, captured);
            (function.imm_value(), lin)
        }
    };
    let entry = usize::try_from(f)
        .ok()
        .and_then(|f| code.functions.get(f))
        .ok_or(not_a_function)?;
    let (f, arity, total) = (f as u32, usize::from(entry.arity), task.scratch.len());
    if clause && total != arity {
        return Err(VmFault::Internal(
            "a clause gets a different number of arguments than its function takes",
        ));
    }
    match total.cmp(&arity) {
        std::cmp::Ordering::Equal => enter(code, task, f, target),
        std::cmp::Ordering::Less => {
            let closure = make_closure(ctx, f, lin, &task.scratch)?;
            task.scratch.clear();
            deliver(task, target, closure);
        }
        std::cmp::Ordering::Greater => {
            let n = total - arity;
            let size = 2 + n;
            if size > usize::from(u16::MAX) {
                return Err(VmFault::Internal("too many arguments are left over"));
            }
            // 余った引数は、結果を受けて適用する `ApplyRest` のフレームに預ける
            let base = match target {
                Target::Frame { ret, dst } => task.push_frame(ret, dst, size),
                Target::Tail => {
                    task.frame_size = size;
                    task.reserve(task.base + size);
                    task.base
                }
            };
            task.stack[base + 1] = Word::from_bits(n);
            for (i, w) in task.scratch.drain(arity..).enumerate() {
                task.stack[base + 2 + i] = w;
            }
            let rest = Target::Frame {
                ret: APPLY_REST,
                dst: 0,
            };
            enter(code, task, f, rest);
        }
    }
    Ok(())
}

/// scratch の引数で `f` に入る。
fn enter<M: Mode>(code: &CodeBase<M>, task: &mut Task, f: u32, target: Target) {
    if let Target::Frame { ret, dst } = target {
        let size = usize::from(code.functions[f as usize].frame_size);
        task.push_frame(ret, dst, size);
    }
    task.enter_in_place(code, f);
}

fn deliver(task: &mut Task, target: Target, value: Word) {
    match target {
        Target::Frame { ret, dst } => {
            task.set(dst, value);
            task.pc = ret;
        }
        Target::Tail => {
            // `Ret` と同じく、どの関数にも属さない命令の誤りに名前を付けるため (`Task::last_ret`)
            task.last_ret = task.pc - 1;
            task.return_value(value);
        }
    }
}

/// `f` と先頭の引数からクロージャを作る (docs/spec/object-model.md の「クロージャ」)。
pub(crate) fn make_closure<M: Mode>(
    ctx: &mut Ctx<M>,
    f: u32,
    lin: bool,
    args: &[Word],
) -> Result<Word, VmFault> {
    let scanned = 1 + args.len();
    let offset = count_word(scanned as u32) as usize;
    // 作る物体だけに書き、`Ctx` の持つ語には触れない
    let heap = unsafe { ctx.heap_mut() };
    let w = heap.alloc_data(0, scanned, 0, lin);
    unsafe {
        heap.set_field(w, offset, Word::imm(f.into()))?;
        for (i, &arg) in args.iter().enumerate() {
            heap.set_field(w, offset + 1 + i, arg)?;
        }
    }
    Ok(w)
}

/// `ApplyRest` のフレームから、結果に残りの引数を適用する。このフレームの見出しを引き継ぐ末尾の適用である。
pub(crate) fn apply_rest<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
) -> Result<(), VmFault> {
    let callee = task.get(0);
    let n = task.get(1).bits();
    task.scratch.clear();
    for i in 0..n {
        let w = task.get((2 + i) as Slot);
        task.scratch.push(w);
    }
    apply(code, ctx, task, callee, Target::Tail, false)
}
