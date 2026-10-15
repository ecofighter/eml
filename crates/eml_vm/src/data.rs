//! データの命令: `con`、switch、`unpack`、`release`、box (docs/spec/vm.md の「命令ごとの実行」)。

use eml_rt::{Ctx, Kind, Mode, Word};

use crate::RunCounts;
use crate::code::CodeBase;
use crate::error::VmFault;
use crate::op::{KEEP, NONE, RAW, Slot};
use crate::vm::Task;

/// 走査する語が 255 個以上の物体の、ペイロードの先頭の数の語の分 (docs/spec/object-model.md の「ヘッダ」)。
pub(crate) fn count_word(scanned: u32) -> u32 {
    u32::from(scanned >= 255)
}

/// フィールドを物理的な位置から読んでスロットに書く。読むだけなので、参照の数は変えない。
fn bind_fields<M: Mode>(
    ctx: &Ctx<M>,
    task: &mut Task,
    w: Word,
    fields: &[u32],
) -> Result<(), VmFault> {
    let heap = ctx.heap();
    for pair in fields.as_chunks::<2>().0 {
        let (position, slot) = (pair[0], pair[1] as Slot);
        // 値は verifier を通った IR が名指した配置の物体で、フィールドは持ち主から借りて読む
        if position & RAW == 0 {
            let field = unsafe { heap.field(w, position as usize) }?;
            task.set(slot, field);
        } else {
            let raw = unsafe { heap.raw(w, (position & !RAW) as usize) }?;
            task.set_raw(slot, raw as i64);
        }
    }
    Ok(())
}

/// 検査モードで、`w` がタグ `tag` で走査する語が `scanned` 個のデータであることを確かめる。verifier の R8 は値を
/// 作った配置を追わないので、verifier を通った IR でも外れうる (docs/spec/lir.md の「構造の規則」)。
fn check_data<M: Mode>(
    ctx: &Ctx<M>,
    w: Word,
    tag: u32,
    scanned: u32,
    what: &'static str,
) -> Result<(), VmFault> {
    if !M::CHECKED {
        return Ok(());
    }
    let heap = ctx.heap();
    let ok = w.is_ptr() && {
        let meta = unsafe { heap.meta(w) }?;
        meta.kind() == Kind::Data
            && u32::from(meta.tag()) == tag
            && unsafe { heap.scanned_len(w) }? == scanned as usize
    };
    if ok {
        Ok(())
    } else {
        Err(VmFault::Internal(what))
    }
}

/// 物体を作り、フィールドをすべて書いてから `dst` に置く。`dst` は引数のスロットと同じ色でありうる
/// (docs/spec/vm.md の「スロットの割り当て」)。
#[inline]
pub(crate) fn con<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    dst: Slot,
    at: u32,
) -> Result<(), VmFault> {
    let ops = &code.operands[at as usize..];
    let (tag, scanned, raw, lin, n) = (ops[0], ops[1], ops[2], ops[3] != 0, ops[4] as usize);
    // ヒープに触れるのは作る物体だけで、`Ctx` の持つ語には触れない
    let heap = unsafe { ctx.heap_mut() };
    let w = heap.alloc_data(tag as u16, scanned as usize, raw as usize, lin);
    for pair in ops[5..5 + 2 * n].as_chunks::<2>().0 {
        let (position, value) = (pair[0], task.get(pair[1] as Slot));
        if position & RAW == 0 {
            unsafe { heap.set_field(w, position as usize, value) }?;
        } else {
            unsafe { heap.set_raw(w, (position & !RAW) as usize, value.bits() as u64) }?;
        }
    }
    task.set(dst, w);
    Ok(())
}

/// タグの switch。`w` は scrutinee の語で、`tag` は Repr に従って読んだタグである。
#[inline]
pub(crate) fn switch_tag<M: Mode>(
    code: &CodeBase<M>,
    ctx: &Ctx<M>,
    task: &mut Task,
    w: Word,
    tag: i64,
    at: u32,
) -> Result<(), VmFault> {
    let ops = &code.operands[at as usize..];
    let (m, default) = (ops[0] as usize, ops[1]);
    // タグは配置のタグなので、飛び先の表を1回引けば case が決まる
    let record = usize::try_from(tag)
        .ok()
        .filter(|&t| t < m)
        .map(|t| ops[2 + t])
        .filter(|&r| r != NONE);
    let Some(record) = record else {
        return take_default(task, default);
    };
    let r = &ops[record as usize..];
    let (target, check, n) = (r[0], r[1], r[2] as usize);
    if check != NONE {
        check_data(
            ctx,
            w,
            tag as u32,
            check,
            "a switch case binds a different number of fields than the value has",
        )?;
    }
    bind_fields(ctx, task, w, &r[3..3 + 2 * n])?;
    task.pc = target;
    Ok(())
}

fn take_default(task: &mut Task, default: u32) -> Result<(), VmFault> {
    if default == NONE {
        return Err(VmFault::Internal("a switch without a matching case"));
    }
    task.pc = default;
    Ok(())
}

#[inline]
pub(crate) fn switch_int<M: Mode>(
    code: &CodeBase<M>,
    task: &mut Task,
    n: i64,
    at: u32,
) -> Result<(), VmFault> {
    let ops = &code.operands[at as usize..];
    let (cases, default) = (ops[0] as usize, ops[1]);
    for case in 0..cases {
        let i = 2 + 3 * case;
        let key = i64::from(ops[i]) | (i64::from(ops[i + 1]) << 32);
        if key == n {
            task.pc = ops[i + 2];
            return Ok(());
        }
    }
    take_default(task, default)
}

#[inline]
pub(crate) fn switch_string<M: Mode>(
    code: &CodeBase<M>,
    ctx: &Ctx<M>,
    task: &mut Task,
    w: Word,
    at: u32,
) -> Result<(), VmFault> {
    let ops = &code.operands[at as usize..];
    let (cases, default) = (ops[0] as usize, ops[1]);
    let heap = ctx.heap();
    // scrutinee も定数も借りて読み、手放さない
    let text = unsafe { &*heap.string(w)? };
    for case in 0..cases {
        let i = 2 + 3 * case;
        let constant = unsafe { &*heap.string(code.constants[ops[i] as usize])? };
        if constant == text {
            task.pc = ops[i + 2];
            return Ok(());
        }
    }
    take_default(task, default)
}

#[inline]
pub(crate) fn unpack<M: Mode>(
    code: &CodeBase<M>,
    ctx: &Ctx<M>,
    task: &mut Task,
    src: Slot,
    at: u32,
) -> Result<(), VmFault> {
    let ops = &code.operands[at as usize..];
    let (tag, scanned, n) = (ops[0], ops[1], ops[2] as usize);
    let w = task.get(src);
    check_data(
        ctx,
        w,
        tag,
        scanned,
        "an unpack names a tag and number of fields the value does not have",
    )?;
    bind_fields(ctx, task, w, &ops[3..3 + 2 * n])
}

/// 形の分かっている `release` の展開 (docs/spec/object-model.md の「核の操作」)。残す変数はすでにスロットにあるので、
/// 参照の数だけを動かす。
#[inline]
pub(crate) fn release<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &Task,
    src: Slot,
    at: u32,
) -> Result<(), VmFault> {
    let ops = &code.operands[at as usize..];
    let (tag, scanned, n) = (ops[0], ops[1], ops[2] as usize);
    let entries = &ops[3..3 + n];
    let w = task.get(src);
    check_data(
        ctx,
        w,
        tag,
        scanned,
        "a release names a tag and number of fields the value does not have",
    )?;
    // VM は `w` の参照を1つ持ち、残さない子はその参照と数える
    let heap = unsafe { ctx.heap_mut() };
    unsafe {
        if heap.is_last(w)? {
            for &entry in entries.iter().filter(|&&e| e & KEEP == 0) {
                let child = heap.field(w, entry as usize)?;
                heap.decref(child)?;
            }
            heap.free_block(w)?;
        } else {
            for &entry in entries.iter().filter(|&&e| e & KEEP != 0) {
                let child = heap.field(w, (entry & !KEEP) as usize)?;
                heap.dup(child)?;
            }
            heap.decref(w)?;
        }
    }
    Ok(())
}

#[inline]
pub(crate) fn unbox<M: Mode>(
    ctx: &Ctx<M>,
    w: Word,
    counts: &mut RunCounts,
) -> Result<i64, VmFault> {
    if M::CHECKED {
        counts.unboxes += 1;
    }
    if w.is_imm() {
        Ok(w.imm_value())
    } else {
        // ダミーは値 0 の `BigInt` なので、ここで 0 を読む (docs/spec/object-model.md の「ダミーの値」)
        Ok(unsafe { ctx.heap().unbox_int(w) }?)
    }
}
