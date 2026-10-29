//! 関数の値の適用とクロージャ (docs/spec/vm.md の「呼び出しと戻り」)。引数は `Task::scratch` に置いて渡す。`apply` の
//! 引数と結果はどれも一様な語なので、引数の数だけを比べ、実行時に Repr を変えない (docs/spec/lir.md の「関数の ABI」)。

use eml_lir::Repr;
use eml_rt::{Ctx, Mode, Word};

use crate::code::{CodeBase, FnEntry};
use crate::data::count_word;
use crate::error::VmFault;
use crate::op::{APPLY_REST, Pc, Slot};
use crate::vm::{Exec, Task};

const NOT_A_FUNCTION: VmFault = VmFault::Internal("applying a value that is not a function");

/// 適用の結果の行き先。
pub(crate) enum Target {
    /// 今のフレームの上に呼ばれる側を積み、戻り先 `ret` で結果を `dst` に受ける。
    Frame { ret: Pc, dst: Slot },
    /// 今のフレームを置き換える (末尾の位置)。
    Tail,
}

impl Target {
    /// 大きさ `size` の呼ばれる側のフレームを用意し、その base を返す。末尾の位置では今のフレームを使い回す。
    #[inline(always)]
    fn frame(self, task: &mut Task, size: usize) -> usize {
        match self {
            Target::Frame { ret, dst } => task.push_frame(ret, dst, size),
            Target::Tail => {
                task.reserve(task.base + size);
                task.frame_size = size;
                task.base
            }
        }
    }
}

/// 呼ばれる値がヒープのクロージャのときの、その語と走査する語の数。
#[derive(Clone, Copy)]
struct Closure {
    w: Word,
    scanned: usize,
}

/// `callee` を `task.scratch` の引数に適用する。捕まえた値の数と渡す引数の数の和が関数の引数の数と等しければ、
/// そのまま入る (速い道)。
pub(crate) fn apply<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    callee: Word,
    target: Target,
) -> Result<(), VmFault> {
    let (f, k, closure) = decode::<R>(ctx, callee)?;
    let entry = lookup::<R>(code, f, k, closure)?;
    if k + task.scratch.len() == usize::from(entry.arity) {
        return enter::<R>(code, ctx, task, f, k, closure, target);
    }
    apply_other::<R>(code, ctx, task, (f, k, closure), target)
}

/// 引数の数が合わない適用。足りなければクロージャを作り、余れば `ApplyRest` のフレームに預ける。部分適用は
/// ふつうのプログラムにも現れるので、`#[cold]` にはしない。
#[inline(never)]
fn apply_other<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    (f, k, closure): (usize, usize, Option<Closure>),
    target: Target,
) -> Result<(), VmFault> {
    let entry = &code.functions[f];
    let arity = usize::from(entry.arity);
    let n = task.scratch.len();
    let Some(missing) = arity.checked_sub(k) else {
        return Err(NOT_A_FUNCTION);
    };
    // 部分適用のクロージャの `lin` ビットはもとのクロージャから写す。捕まえた値を取り出すと物体を手放すので、先に読む
    let lin = match closure {
        Some(closure) if n < missing => unsafe { ctx.heap().meta(closure.w) }?.lin(),
        _ => false,
    };
    if let Some(closure) = closure {
        task.scratch
            .splice(0..0, std::iter::repeat_n(Word::UNIT, k));
        take_captures::<R>(ctx, closure, &entry.params[..k], &mut task.scratch[..k])?;
    }
    if n < missing {
        let closure = make_closure::<R>(code, ctx, f as u32, lin, &task.scratch)?;
        task.scratch.clear();
        deliver::<R>(code, task, target, closure);
        return Ok(());
    }
    let rest = n - missing;
    let size = 2 + rest;
    if size > usize::from(u16::MAX) {
        return Err(VmFault::Internal("too many arguments are left over"));
    }
    // 余った引数は結果を受けて適用する `ApplyRest` のフレームに預ける
    let base = target.frame(task, size);
    task.stack[base + 1] = Word::from_bits(rest);
    for (i, w) in task.scratch.drain(arity..).enumerate() {
        task.stack[base + 2 + i] = w;
    }
    let size = usize::from(entry.frame_size);
    task.push_frame(APPLY_REST, 0, size);
    task.enter_in_place::<R>(code, f as u32);
    Ok(())
}

/// `clause` でエントリのクロージャ `callee` を呼ぶ。引数は操作の ABI の Repr で、エントリの関数の引数と同じ Repr なので比べない。
/// 引数の数はいつもエントリの関数の引数の数と等しい。部分適用で生の `int` をクロージャに入れることもない。
pub(crate) fn apply_clause<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    callee: Word,
    target: Target,
) -> Result<(), VmFault> {
    let (f, k, closure) = decode::<R>(ctx, callee)?;
    let entry = lookup::<R>(code, f, k, closure)?;
    if k + task.scratch.len() != usize::from(entry.arity) {
        return Err(VmFault::Internal(
            "a clause gets a different number of arguments than its function takes",
        ));
    }
    enter::<R>(code, ctx, task, f, k, closure, target)
}

/// 呼ばれる値から、関数の番号、捕まえた値の数、クロージャの物体を読む。捕まえた値の数は `&f` の即値の上の 32 ビットに
/// 詰めてあるので、1回の読み込みで関数と数が分かる (docs/spec/vm.md の「データとクロージャ」)。
#[inline(always)]
fn decode<R: Exec>(
    ctx: &Ctx<R::M>,
    callee: Word,
) -> Result<(usize, usize, Option<Closure>), VmFault> {
    let split = |imm: i64| {
        let bits = imm as u64;
        ((bits & 0xFFFF_FFFF) as usize, (bits >> 32) as usize)
    };
    if callee.is_imm() {
        let (f, k) = split(callee.imm_value());
        if R::M::CHECKED && k != 0 {
            return Err(NOT_A_FUNCTION);
        }
        return Ok((f, 0, None));
    }
    // `callee` は VM が参照を持つ物体である
    let heap = ctx.heap();
    unsafe {
        let scanned = heap.scanned_len(callee)?;
        // 走査する語のない物体の0番目を読むと、断る誤りが範囲外の誤りになる
        if R::M::CHECKED && scanned == 0 {
            return Err(NOT_A_FUNCTION);
        }
        let function = heap.scanned(callee, 0)?;
        if R::M::CHECKED && !function.is_imm() {
            return Err(NOT_A_FUNCTION);
        }
        let (f, k) = split(function.imm_value());
        Ok((f, k, Some(Closure { w: callee, scanned })))
    }
}

/// 関数の番号の `FnEntry`。検査モードでは、`&f` の即値に詰めた捕まえた値の数が、クロージャの物体の走査する語の数と
/// 合うことも確かめる。
#[inline(always)]
fn lookup<R: Exec>(
    code: &CodeBase<R::M>,
    f: usize,
    k: usize,
    closure: Option<Closure>,
) -> Result<&FnEntry, VmFault> {
    let entry = code.functions.get(f).ok_or(NOT_A_FUNCTION)?;
    if R::M::CHECKED
        && let Some(closure) = closure
    {
        let refs = entry
            .params
            .get(..k)
            .filter(|_| k < usize::from(entry.arity))
            .map(|captured| captured.iter().filter(|repr| repr.is_rc()).count());
        if refs.map(|refs| 1 + refs) != Some(closure.scanned) {
            return Err(VmFault::Internal(
                "the capture count packed with a function does not match its closure",
            ));
        }
    }
    Ok(entry)
}

/// 捕まえた値と scratch の引数で `f` に入る。捕まえた値は一時の列を通さず、クロージャから新しいフレームへ直に写す。
#[inline(always)]
fn enter<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    f: usize,
    k: usize,
    closure: Option<Closure>,
    target: Target,
) -> Result<(), VmFault> {
    let entry = &code.functions[f];
    let size = usize::from(entry.frame_size);
    let base = target.frame(task, size);
    if let Some(closure) = closure {
        take_captures::<R>(
            ctx,
            closure,
            &entry.params[..k],
            &mut task.stack[base..base + k],
        )?;
    }
    task.store_scratch_at(base + k);
    task.pc = entry.start;
    Ok(())
}

/// クロージャの捕まえた値を、関数の引数の順に `dst` に写し、VM の持つクロージャの参照を手放す。参照の捕獲は `&f` の後の
/// 走査する語に、ほかの捕獲は生の語に、それぞれ引数の順に並ぶ (docs/spec/object-model.md の「クロージャ」)。
#[inline]
fn take_captures<R: Exec>(
    ctx: &mut Ctx<R::M>,
    closure: Closure,
    params: &[Repr],
    dst: &mut [Word],
) -> Result<(), VmFault> {
    // 読むのは `closure` の語だけで、`Ctx` の持つ語には触れない
    let heap = unsafe { ctx.heap_mut() };
    let w = closure.w;
    unsafe {
        let last = heap.is_last(w)?;
        let mut scanned = 1;
        let mut raw = count_word(closure.scanned as u32) as usize + closure.scanned;
        for (slot, &repr) in dst.iter_mut().zip(params) {
            *slot = if repr.is_rc() {
                let field = heap.scanned(w, scanned)?;
                scanned += 1;
                if !last {
                    heap.dup(field)?;
                }
                field
            } else {
                let bits = heap.raw(w, raw)?;
                raw += 1;
                Word::from_bits(bits as usize)
            };
        }
        if last {
            heap.free_block(w)?;
        } else {
            heap.decref(w)?;
        }
    }
    Ok(())
}

fn deliver<R: Exec>(code: &CodeBase<R::M>, task: &mut Task, target: Target, value: Word) {
    match target {
        Target::Frame { ret, dst } => {
            task.set(dst, value);
            task.pc = ret;
        }
        Target::Tail => {
            // `Ret` と同じく、どの関数にも属さない命令の誤りに名前を付けるため (`Task::last_ret`)。`ApplyRest` から
            // 来たときは今の位置がどの関数にも属さないので、最後に戻った関数の位置をそのまま残す
            let at = task.pc - 1;
            if code.function_at(at).is_some() {
                task.last_ret = at;
            }
            task.return_value(value);
        }
    }
}

/// `f` と先頭の引数 `args` からクロージャを作る。走査する語に `&f` と参照の語を、生の語にほかの語を、`f` の引数の順に
/// 並べる。`&f` の即値には引数の数を詰める (docs/spec/object-model.md の「クロージャ」)。
#[inline]
pub(crate) fn make_closure<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    f: u32,
    lin: bool,
    args: &[Word],
) -> Result<Word, VmFault> {
    let k = args.len();
    let params = code.functions[f as usize]
        .params
        .get(..k)
        .ok_or(VmFault::Internal(
            "a closure takes more arguments than its function",
        ))?;
    let refs = params.iter().filter(|repr| repr.is_rc()).count();
    let scanned = 1 + refs;
    let offset = count_word(scanned as u32) as usize;
    // 作る物体だけに書き、`Ctx` の持つ語には触れない
    let heap = unsafe { ctx.heap_mut() };
    let w = heap.alloc_data(0, scanned, args.len() - refs, lin);
    let (mut next_scanned, mut next_raw) = (offset + 1, offset + scanned);
    unsafe {
        heap.set_field(w, offset, Word::imm(i64::from(f) | (k as i64) << 32))?;
        for (&arg, &repr) in args.iter().zip(params) {
            if repr.is_rc() {
                heap.set_field(w, next_scanned, arg)?;
                next_scanned += 1;
            } else {
                heap.set_raw(w, next_raw, arg.bits() as u64)?;
                next_raw += 1;
            }
        }
    }
    Ok(w)
}

/// `ApplyRest` のフレームから、結果に残りの引数を適用する。このフレームの見出しを引き継ぐ末尾の適用である。
#[inline]
pub(crate) fn apply_rest<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
) -> Result<(), VmFault> {
    let callee = task.get(0);
    let n = task.get(1).bits();
    task.load_scratch((2..2 + n).map(|i| i as Slot));
    apply::<R>(code, ctx, task, callee, Target::Tail)
}

/// `ApplyRest` のフレームの、まだ渡していない残りの引数のスロット。引数は一様な語なので、巻き戻しはどれも手放す。
pub(crate) fn rest_arguments(task: &Task) -> impl Iterator<Item = Slot> {
    (0..task.get(1).bits()).map(|i| (2 + i) as Slot)
}
