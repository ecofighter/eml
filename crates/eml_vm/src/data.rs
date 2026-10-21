//! データの命令: `con`、switch、`unpack`、`release`、reuse、box (docs/spec/vm.md の「命令ごとの実行」)。

use eml_lir::{Repr, StructId};
use eml_rt::{Ctx, Heap, HeapError, Kind, Meta, Mode, Token, Word};

use crate::RunCounts;
use crate::code::CodeBase;
use crate::error::VmFault;
use crate::op::{KEEP, NONE, RAW, SKIP, Slot};
use crate::sig::{Scalar, Structs};
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
    unsafe { write_fields(heap, task, w, &ops[5..5 + 2 * n], false) }?;
    task.set(dst, w);
    Ok(())
}

/// [位置 | RAW | SKIP, スロット] の並びの値を `w` のフィールドに書く。`skip` なら、書かない印のフィールドを飛ばす。
///
/// # Safety
/// `w` は作っている途中の物体で、並びはその形の位置である。スロットの値の参照は `w` に移る。
#[inline]
unsafe fn write_fields<M: Mode>(
    heap: &mut Heap<M>,
    task: &Task,
    w: Word,
    items: &[u32],
    skip: bool,
) -> Result<(), HeapError> {
    for pair in items.as_chunks::<2>().0 {
        let (position, value) = (pair[0], task.get(pair[1] as Slot));
        if skip && position & SKIP != 0 {
            continue;
        }
        let index = (position & !(RAW | SKIP)) as usize;
        unsafe {
            if position & RAW == 0 {
                heap.set_field(w, index, value)?;
            } else {
                heap.set_raw(w, index, value.bits() as u64)?;
            }
        }
    }
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
    let w = task.get(src);
    check_data(
        ctx,
        w,
        tag,
        scanned,
        "a release names a tag and number of fields the value does not have",
    )?;
    // VM は `w` の参照を1つ持つ
    unsafe { release_fields(ctx.heap_mut(), w, &ops[3..3 + n]) }?;
    Ok(())
}

/// `release` の部品の列。最後の参照なら残さない子を手放して箱を空け、そうでなければ残す子を `dup` して `w` を手放す。
///
/// # Safety
/// `w` は検査を通った `release` の値で、呼ぶ側がその参照を1つ持つ。`entries` はその形の RC のフィールドの項目である。
#[inline]
unsafe fn release_fields<M: Mode>(
    heap: &mut Heap<M>,
    w: Word,
    entries: &[u32],
) -> Result<(), HeapError> {
    unsafe {
        if heap.is_last(w)? {
            drop_children(heap, w, entries)?;
            heap.free_block(w)
        } else {
            for &entry in entries.iter().filter(|&&e| e & KEEP != 0) {
                let child = heap.field(w, (entry & !KEEP) as usize)?;
                heap.dup(child)?;
            }
            heap.decref(w)
        }
    }
}

/// 残さない子を手放す。残さない子は、呼ぶ側が持つ `w` の最後の参照の分と数える。
///
/// # Safety
/// `release_fields` と同じで、`w` の参照は最後のものである。
#[inline]
unsafe fn drop_children<M: Mode>(
    heap: &mut Heap<M>,
    w: Word,
    entries: &[u32],
) -> Result<(), HeapError> {
    for &entry in entries.iter().filter(|&&e| e & KEEP == 0) {
        unsafe {
            let child = heap.field(w, entry as usize)?;
            heap.decref(child)?;
        }
    }
    Ok(())
}

/// token を定義する `release` (docs/spec/vm.md の「reuse」)。一意なら残さない子を手放して箱を token にし、そうで
/// なければ `release` と同じにして空の token を書く。`eml_rt` の `release_reuse` と同じ列を、`release` と同じく
/// この関数の中に並べる。crate をまたぐ呼び出しを速い道に入れないためである。
// 共有された値の道は `release_shared` に出し、`vm::run` に展開するのは一意な道だけにする。全部を展開すると、
// reuse の命令を使わないプログラムの命令の数が増えたためである (docs/spec/vm.md の「置き場所」)
#[inline]
pub(crate) fn release_reuse<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    src: Slot,
    at: u32,
) -> Result<(), VmFault> {
    let ops = &code.operands[at as usize..];
    let (tok, tag, scanned, n) = (ops[0] as Slot, ops[1], ops[2], ops[3] as usize);
    let entries = &ops[4..4 + n];
    let w = task.get(src);
    check_data(
        ctx,
        w,
        tag,
        scanned,
        "a release names a tag and number of fields the value does not have",
    )?;
    // VM は `w` の参照を1つ持つ。token のスロットは `w` のスロットと同じ色でありうるので、`w` を読んだ後で書く
    let heap = unsafe { ctx.heap_mut() };
    let token = unsafe {
        if heap.is_unique(w)? {
            drop_children(heap, w, entries)?;
            heap.into_token(w)?.into_word()
        } else {
            release_shared(heap, w, entries)?;
            Word::EMPTY
        }
    };
    task.set(tok, token);
    Ok(())
}

/// token を定義する `release` の、値が共有されていた道。`release` と同じに振る舞う。
///
/// # Safety
/// `release_fields` と同じである。
#[cold]
#[inline(never)]
unsafe fn release_shared<M: Mode>(
    heap: &mut Heap<M>,
    w: Word,
    entries: &[u32],
) -> Result<(), HeapError> {
    unsafe { release_fields(heap, w, entries) }
}

/// token を受ける `con` (docs/spec/vm.md の「reuse」)。空の token なら `con` と同じに確保してフィールドをみな書く。
/// 空でなければ token の箱を使い、書かない印のフィールドは飛ばす。箱は中身を消さないので、そのフィールドには
/// 元の値が残っている。
// 空の token の道を `con_fresh` に出す理由は、`release_reuse` が共有された値の道を出す理由と同じである
#[inline]
pub(crate) fn con_reuse<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    counts: &mut RunCounts,
    dst: Slot,
    at: u32,
) -> Result<(), VmFault> {
    let ops = &code.operands[at as usize..];
    let (tok, tag, scanned, raw) = (ops[0] as Slot, ops[1], ops[2], ops[3]);
    let (lin, in_place, n) = (ops[4] != 0, ops[5] != 0, ops[6] as usize);
    // token は verifier を通った IR の token で、このヒープのまだ使っていないものである
    let token = unsafe { Token::from_word(task.get(tok)) };
    let heap = unsafe { ctx.heap_mut() };
    let fields = &ops[7..7 + 2 * n];
    if token.is_empty() {
        return con_fresh(heap, task, dst, tag, scanned, raw, lin, fields);
    }
    if M::CHECKED {
        counts.reuses += 1;
    }
    // 組は走査する語が 255 個未満の形に限るので、`scan` に収まり、数の語もない
    // (docs/spec/object-model.md の「ヘッダ」)。物理的な語の数が同じ形の token なので、箱の級も合う
    let meta = Meta::new(tag as u16, scanned as u8, Kind::Data, lin);
    let w = unsafe {
        if in_place {
            heap.reuse_in_place(token, meta)?
        } else {
            heap.con_at(token, (scanned + raw) as usize, meta)?.0
        }
    };
    unsafe { write_fields(heap, task, w, fields, true) }?;
    task.set(dst, w);
    Ok(())
}

/// token を受ける `con` の、token が空の道。`con` と同じに確保して、書かない印のフィールドも書く。
#[cold]
#[inline(never)]
#[expect(clippy::too_many_arguments, reason = "the decoded operands of one con")]
fn con_fresh<M: Mode>(
    heap: &mut Heap<M>,
    task: &mut Task,
    dst: Slot,
    tag: u32,
    scanned: u32,
    raw: u32,
    lin: bool,
    fields: &[u32],
) -> Result<(), VmFault> {
    let w = heap.alloc_data(tag as u16, scanned as usize, raw as usize, lin);
    unsafe { write_fields(heap, task, w, fields, false) }?;
    task.set(dst, w);
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

/// `float` の `unbox`。`Float` の箱はいつも物体で、ダミーはビット列 0 (`0.0`) を読む。
#[inline]
pub(crate) fn unbox_float<M: Mode>(
    ctx: &Ctx<M>,
    w: Word,
    counts: &mut RunCounts,
) -> Result<u64, VmFault> {
    if M::CHECKED {
        counts.unboxes += 1;
        // verifier を通った IR の `float` の `unbox` は、`box` した `Float` かダミーを受ける。検査モードは前提を
        // 確かめるので、即値を物体として読む前に断る
        if !w.is_ptr() {
            return Err(VmFault::Internal("an unbox of float reads an immediate"));
        }
    }
    Ok(unsafe { ctx.heap().unbox_float(w) }?)
}

/// 生の語のスカラー `w` を箱に入れる。`apply` の変換と変換のフレームが使う。`enum` の箱は即値である。
pub(crate) fn box_scalar<M: Mode>(
    ctx: &mut Ctx<M>,
    counts: &mut RunCounts,
    w: Word,
    scalar: Scalar,
) -> Word {
    if M::CHECKED {
        counts.boxes += 1;
    }
    // 作る箱は新しい物体なので、`Ctx` の持つ語には触れない
    let heap = unsafe { ctx.heap_mut() };
    match scalar {
        Scalar::Int => heap.alloc_int(w.bits() as i64),
        Scalar::Float => heap.alloc_float(w.bits() as u64),
        Scalar::Enum => Word::imm(w.bits() as i64),
    }
}

/// 箱 `w` からスカラーを取り出し、箱を手放す。一様な値から取り出す変換は値を消費する。スカラーは中身に参照を持たない
/// ので、取り出した後に箱を `decref` すれば、一意なら箱を空けることになる
/// (docs/spec/vm.md の「変換のフレーム」)。
pub(crate) fn unbox_scalar<M: Mode>(
    ctx: &mut Ctx<M>,
    counts: &mut RunCounts,
    w: Word,
    scalar: Scalar,
) -> Result<Word, VmFault> {
    let bits = match scalar {
        Scalar::Float => unbox_float(ctx, w, counts)?,
        Scalar::Int | Scalar::Enum => unbox(ctx, w, counts)? as u64,
    };
    // 箱は VM が参照を持つ値で、手放すのはその箱だけである
    unsafe { ctx.heap_mut().decref(w) }?;
    Ok(Word::from_bits(bits as usize))
}

/// 構造体の箱 `w` がダミーか (docs/spec/vm.md の「データとクロージャ」)。
/// 速いモードは物体の種類で見分ける。ダミーは `BigInt` の物体で、構造体の箱はいつも `Data` である。検査モードは
/// ヒープのダミーと同じ語であることも確かめ、ダミーでない `BigInt` を断る。1回の実行の中ではダミーは1つで、回復は
/// スタックを捨てるので、語で比べてよい。
#[inline]
fn is_dummy<M: Mode>(ctx: &Ctx<M>, w: Word) -> Result<bool, VmFault> {
    if M::CHECKED && !w.is_ptr() {
        return Err(VmFault::Internal("a struct box is not an object"));
    }
    // 構造体の箱かダミーを受けるのは verifier を通った IR の約束で、どちらも物体である
    let kind = unsafe { ctx.heap().meta(w) }?.kind();
    if kind != Kind::BigInt {
        return Ok(false);
    }
    if M::CHECKED && w != ctx.heap().dummy() {
        return Err(VmFault::Internal(
            "a struct box is a `BigInt` that is not the dummy",
        ));
    }
    Ok(true)
}

/// 構造体の `unbox` (docs/spec/vm.md の「データとクロージャ」)。フィールドは箱から借りて
/// 読む。箱がダミーなら、参照のフィールドにはダミーを、生のフィールドにはビット列 0 を書く。box の挿入は呼び出しの
/// 結果を受け直す `unbox` を yield の確認の前に置くので、ダミーを受けうるためである。`vm::step` に展開しないのは、
/// 複数の値の命令と同じ理由である。
#[inline(never)]
pub(crate) fn unbox_struct<M: Mode>(
    code: &CodeBase<M>,
    ctx: &Ctx<M>,
    task: &mut Task,
    src: Slot,
    at: u32,
) -> Result<(), VmFault> {
    let ops = &code.operands[at as usize..];
    let (tag, scanned, n) = (ops[0], ops[1], ops[2] as usize);
    let fields = &ops[3..3 + 2 * n];
    let w = task.get(src);
    if is_dummy(ctx, w)? {
        let dummy = ctx.heap().dummy();
        for pair in fields.as_chunks::<2>().0 {
            let slot = pair[1] as Slot;
            if pair[0] & RAW == 0 {
                task.set(slot, dummy);
            } else {
                task.set_raw(slot, 0);
            }
        }
        return Ok(());
    }
    check_data(
        ctx,
        w,
        tag,
        scanned,
        "a struct unbox names a tag and number of fields the value does not have",
    )?;
    bind_fields(ctx, task, w, fields)
}

/// 構造体の `unbox` で葉を借りた箱の `release`。ダミーなら何もせず、token の形は空の token を書く。名前を書いた葉は
/// すでにダミーである。ほかは `release` と token を作る `release` と同じである。
#[inline(never)]
pub(crate) fn release_unboxed<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    src: Slot,
    at: u32,
) -> Result<(), VmFault> {
    let tok = code.operands[at as usize];
    if is_dummy(ctx, task.get(src))? {
        if tok != NONE {
            task.set(tok as Slot, Word::EMPTY);
        }
        return Ok(());
    }
    if tok == NONE {
        release(code, ctx, task, src, at + 1)
    } else {
        release_reuse(code, ctx, task, src, at)
    }
}

/// 構造体 `id` の葉 `leaves` を、配置の `con` と同じ形の箱に入れる (docs/spec/lir.md の「開く」)。葉は配置の宣言したフィールドの Repr に合わせ、`tobj` のフィールドに入るスカラーは箱に入れ、
/// 入れ子の構造体は再帰で箱にする。`unit` の葉は `()` を書く。葉の参照は箱に移る。実行時に作る箱には `lin` ビットを
/// 立てない。`FnEntry` と呼ぶ所は印を持たないためで、安全な側である。箱は `boxes` に数える。
pub(crate) fn box_struct<M: Mode>(
    structs: &Structs,
    ctx: &mut Ctx<M>,
    counts: &mut RunCounts,
    id: StructId,
    leaves: &[Word],
) -> Result<Word, VmFault> {
    if M::CHECKED {
        counts.boxes += 1;
    }
    let info = structs.get(id);
    let shape = &info.shape;
    let offset = count_word(shape.scanned) as usize;
    let mut values = Vec::with_capacity(info.key.fields.len());
    let mut next = 0;
    for (&field, &declared) in info.key.fields.iter().zip(&info.key.declared) {
        let value = match field {
            Repr::Unit => Word::UNIT,
            Repr::Struct(inner) => {
                let count = structs.get(inner).leaves.len();
                let boxed = box_struct(structs, ctx, counts, inner, &leaves[next..next + count])?;
                next += count;
                boxed
            }
            leaf => {
                let word = leaves[next];
                next += 1;
                match leaf {
                    Repr::Int | Repr::Float | Repr::Enum if declared.is_rc() => {
                        let scalar = match leaf {
                            Repr::Int => Scalar::Int,
                            Repr::Float => Scalar::Float,
                            _ => Scalar::Enum,
                        };
                        box_scalar(ctx, counts, word, scalar)
                    }
                    _ => word,
                }
            }
        };
        values.push(value);
    }
    // 作る物体だけに書き、`Ctx` の持つ語には触れない
    let heap = unsafe { ctx.heap_mut() };
    let w = heap.alloc_data(0, shape.scanned as usize, shape.raw as usize, false);
    for ((value, &position), declared) in values
        .into_iter()
        .zip(&shape.positions)
        .zip(&info.key.declared)
    {
        let index = position as usize + offset;
        unsafe {
            if declared.is_rc() {
                heap.set_field(w, index, value)?;
            } else {
                heap.set_raw(w, index, value.bits() as u64)?;
            }
        }
    }
    Ok(w)
}

/// 構造体 `id` の箱 `w` から葉を取り出して `out` に足し、箱を消費する。一意なら箱を空けて葉の参照を受け取り、そうで
/// なければ葉を `dup` して箱を手放す。`tobj` のフィールドのスカラーは取り出してその箱も手放し、入れ子の構造体は再帰で
/// 取り出す。箱がダミーなら葉のダミーを足す。取り出しは `unboxes` に数える。
pub(crate) fn take_struct<M: Mode>(
    structs: &Structs,
    ctx: &mut Ctx<M>,
    counts: &mut RunCounts,
    id: StructId,
    w: Word,
    out: &mut Vec<Word>,
) -> Result<(), VmFault> {
    if M::CHECKED {
        counts.unboxes += 1;
    }
    let info = structs.get(id);
    if is_dummy(ctx, w)? {
        leaf_dummies(ctx, &info.leaves, out);
        return Ok(());
    }
    let shape = &info.shape;
    check_data(
        ctx,
        w,
        0,
        shape.scanned,
        "an apply unboxes a struct from a value of another shape",
    )?;
    let offset = count_word(shape.scanned) as usize;
    // VM は `w` の参照を1つ持つ
    let last = unsafe { ctx.heap().is_last(w) }?;
    for ((&field, &declared), &position) in info
        .key
        .fields
        .iter()
        .zip(&info.key.declared)
        .zip(&shape.positions)
    {
        let index = position as usize + offset;
        if field == Repr::Unit {
            continue;
        }
        if !declared.is_rc() {
            let raw = unsafe { ctx.heap().raw(w, index) }?;
            out.push(Word::from_bits(raw as usize));
            continue;
        }
        let child = unsafe { ctx.heap().field(w, index) }?;
        match field {
            Repr::Struct(inner) => {
                if !last {
                    unsafe { ctx.heap_mut().dup(child) }?;
                }
                take_struct(structs, ctx, counts, inner, child, out)?;
            }
            Repr::Int | Repr::Enum => {
                out.push(Word::from_bits(unbox(ctx, child, counts)? as usize));
                if last {
                    unsafe { ctx.heap_mut().decref(child) }?;
                }
            }
            Repr::Float => {
                out.push(Word::from_bits(unbox_float(ctx, child, counts)? as usize));
                if last {
                    unsafe { ctx.heap_mut().decref(child) }?;
                }
            }
            _ => {
                if !last {
                    unsafe { ctx.heap_mut().dup(child) }?;
                }
                out.push(child);
            }
        }
    }
    // 一意なら、子の参照は上で受け取ったので箱だけを空ける
    unsafe {
        if last {
            ctx.heap_mut().free_block(w)?;
        } else {
            ctx.heap_mut().decref(w)?;
        }
    }
    Ok(())
}

/// 葉の Repr ごとのダミー。参照の葉は `prim dummy()` の値、生の葉はビット列 0 (`0`、`0.0`、`#0`) である
/// (docs/spec/lir.md の「ダミーの値」)。
pub(crate) fn leaf_dummies<M: Mode>(ctx: &Ctx<M>, leaves: &[Repr], out: &mut Vec<Word>) {
    let dummy = ctx.heap().dummy();
    out.extend(leaves.iter().map(|repr| {
        if repr.is_rc() {
            dummy
        } else {
            Word::from_bits(0)
        }
    }));
}
