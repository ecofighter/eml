//! 関数の値の適用とクロージャ (docs/spec/vm.md の「呼び出しと戻り」)。引数は `Task::scratch` に置いて渡す。呼ぶ所の
//! シグネチャと関数の `FnEntry` の Repr が合わなければ、引数を実行時に変える
//! (docs/spec/vm.md の「シグネチャ」)。

use eml_lir::Repr;
use eml_rt::{Ctx, Mode, Word};

use crate::RunCounts;
use crate::code::{CodeBase, Conversion, FnEntry};
use crate::data::{self, count_word};
use crate::error::VmFault;
use crate::op::{APPLY_REST, Pc, Slot};
use crate::sig::{SLOW_SITE, SigId, class};
use crate::vm::{self, Task};

const NOT_A_FUNCTION: VmFault = VmFault::Internal("applying a value that is not a function");
pub(crate) const DIFFERENT_TYPES: VmFault =
    VmFault::Internal("an apply converts between the reprs of different types");

/// 適用の結果の行き先。
pub(crate) enum Target {
    /// 今のフレームの上に呼ばれる側を積み、戻り先 `ret` で結果を `dst` に受ける。
    Frame { ret: Pc, dst: Slot },
    /// 今のフレームを置き換える (末尾の位置)。
    Tail,
}

/// `apply` を呼ぶ所。`scratch` の引数は場所のシグネチャの引数の `offset` 番目からである。`offset` が 0 でないのは
/// `ApplyRest` が残りの引数を適用するときである。
#[derive(Clone, Copy)]
pub(crate) struct Site {
    sig: SigId,
    offset: usize,
    /// 場所のシグネチャの種類を詰めた語を、`offset` の分ずらしたもの。
    key: u64,
}

impl Site {
    #[inline]
    pub(crate) fn new<M: Mode>(code: &CodeBase<M>, sig: SigId, offset: usize) -> Site {
        let key = code.signatures.site_key(sig);
        // 詰められない語はずらすと詰めた語に見えうるので、そのまま残す
        let key = if offset == 0 || key == SLOW_SITE {
            key
        } else {
            key >> (4 * offset)
        };
        Site { sig, offset, key }
    }
}

/// 呼ばれる値がヒープのクロージャのときの、その語と走査する語の数。
#[derive(Clone, Copy)]
struct Closure {
    w: Word,
    scanned: usize,
}

/// `callee` を `task.scratch` の引数に適用する。捕まえた値の数と渡す引数の数の和が関数の引数の数と等しく、種類が
/// 合えば、今と同じ道で入る (速い道)。速い道に入るシグネチャが持つ構造体は葉が1つのものだけなので
/// (`crate::sig::pack`)、そこでは語の数と源の引数の数が等しい。
pub(crate) fn apply<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    counts: &mut RunCounts,
    callee: Word,
    site: Site,
    target: Target,
) -> Result<(), VmFault> {
    let (f, k, closure) = decode(ctx, callee)?;
    let entry = lookup(code, f, k, closure)?;
    // 関数の語を捕まえた値の分ずらすと、渡す引数と結果の種類の語になる。`k` は関数の引数の数より小さいので、詰めた
    // 関数ではずらす量が 64 に届かない。詰められない関数では `wrapping_shr` が回り込むが、その語はどの場所の語とも
    // 等しくならない (`crate::sig`)
    if k + task.scratch.len() == usize::from(entry.arity)
        && entry.key.wrapping_shr(4 * k as u32) == site.key
    {
        return enter(code, ctx, task, f, k, closure, target);
    }
    apply_slow(code, ctx, task, counts, (f, k, closure), site, target)
}

/// 速い道に入らない適用。引数の数は源の単位で比べる。構造体の引数は葉の語で並ぶので、語の位置はシグネチャの語の
/// 並び (`Words`) で引く。ちょうどの数なら引数と結果を変えて入り、足りなければクロージャを作り、余れば `ApplyRest`
/// のフレームに預ける。部分適用はふつうのプログラムにも現れるので、`#[cold]` にはしない。
#[inline(never)]
fn apply_slow<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    counts: &mut RunCounts,
    (f, k, closure): (usize, usize, Option<Closure>),
    site: Site,
    target: Target,
) -> Result<(), VmFault> {
    let entry = &code.functions[f];
    let arity = usize::from(entry.arity);
    let n = code.signatures.get(site.sig).params.len() - site.offset;
    let Some(missing) = arity.checked_sub(k) else {
        return Err(NOT_A_FUNCTION);
    };
    let result = convert(
        code,
        ctx,
        task,
        counts,
        entry,
        k,
        site,
        n.min(missing),
        n == missing,
    )?;
    let words = code.signatures.words(entry.sig);
    let captured = words.starts[k];
    if n == missing {
        let target = match result {
            Some(result) => convert_result(code, task, result, target)?,
            None => target,
        };
        return enter(code, ctx, task, f, captured, closure, target);
    }
    // 部分適用のクロージャの `lin` ビットはもとのクロージャから写す。捕まえた値を取り出すと物体を手放すので、先に読む
    let lin = match closure {
        Some(closure) if n < missing => unsafe { ctx.heap().meta(closure.w) }?.lin(),
        _ => false,
    };
    if let Some(closure) = closure {
        task.scratch
            .splice(0..0, std::iter::repeat_n(Word::UNIT, captured));
        take_captures(
            ctx,
            closure,
            &words.params[..captured],
            &mut task.scratch[..captured],
        )?;
    }
    if n < missing {
        let closure = make_closure(code, ctx, f as u32, k + n, lin, &task.scratch)?;
        task.scratch.clear();
        deliver(code, task, target, closure);
        return Ok(());
    }
    let used = words.starts[arity];
    let rest = task.scratch.len() - used;
    let size = 4 + rest;
    if size > usize::from(u16::MAX) {
        return Err(VmFault::Internal("too many arguments are left over"));
    }
    // 余った引数は結果を受けて適用する `ApplyRest` のフレームに預ける。引数は場所の Repr のままなので、後の適用と
    // 巻き戻しが語の Repr を知れるように、場所のシグネチャと残りの引数の最初の位置 (源の単位) も置く
    let base = match target {
        Target::Frame { ret, dst } => task.push_frame(ret, dst, size),
        Target::Tail => {
            task.frame_size = size;
            task.reserve(task.base + size);
            task.base
        }
    };
    task.stack[base + 1] = Word::from_bits(rest);
    task.stack[base + 2] = Word::from_bits(site.sig.0 as usize);
    task.stack[base + 3] = Word::from_bits(site.offset + missing);
    for (i, w) in task.scratch.drain(used..).enumerate() {
        task.stack[base + 4 + i] = w;
    }
    let size = usize::from(entry.frame_size);
    task.push_frame(APPLY_REST, 0, size);
    task.enter_in_place(code, f as u32);
    Ok(())
}

/// `clause` で入口のクロージャ `callee` を呼ぶ。引数は操作の ABI の Repr で、入口の関数の引数と同じ Repr なので比べない。
/// 引数の数はいつも入口の関数の引数の数と等しい。部分適用で生の `int` をクロージャに入れることもない。
pub(crate) fn apply_clause<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    callee: Word,
    target: Target,
) -> Result<(), VmFault> {
    let (f, k, closure) = decode(ctx, callee)?;
    let entry = lookup(code, f, k, closure)?;
    if k + task.scratch.len() != usize::from(entry.arity) {
        return Err(VmFault::Internal(
            "a clause gets a different number of arguments than its function takes",
        ));
    }
    // 入口の関数の捕獲は一様で、操作の引数に構造体は来ないので、語の数は源の引数の数と等しい
    // (docs/spec/lir.md の「関数の ABI」)
    if M::CHECKED && code.signatures.words(entry.sig).starts[k] != k {
        return Err(VmFault::Internal("a clause entry captures a struct"));
    }
    enter(code, ctx, task, f, k, closure, target)
}

/// 呼ばれる値から、関数の番号、捕まえた値の数、クロージャの物体を読む。捕まえた値の数は `&f` の即値の上の 32 ビットに
/// 詰めてあるので、1回の読み込みで関数と数が分かる (docs/spec/vm.md の「データとクロージャ」)。
#[inline(always)]
fn decode<M: Mode>(ctx: &Ctx<M>, callee: Word) -> Result<(usize, usize, Option<Closure>), VmFault> {
    let split = |imm: i64| {
        let bits = imm as u64;
        ((bits & 0xFFFF_FFFF) as usize, (bits >> 32) as usize)
    };
    if callee.is_imm() {
        let (f, k) = split(callee.imm_value());
        if M::CHECKED && k != 0 {
            return Err(NOT_A_FUNCTION);
        }
        return Ok((f, 0, None));
    }
    // `callee` は VM が参照を持つ物体である
    let heap = ctx.heap();
    unsafe {
        let scanned = heap.scanned_len(callee)?;
        // 走査する語のない物体の0番目を読むと、断る誤りが範囲外の誤りになる
        if M::CHECKED && scanned == 0 {
            return Err(NOT_A_FUNCTION);
        }
        let function = heap.scanned(callee, 0)?;
        if M::CHECKED && !function.is_imm() {
            return Err(NOT_A_FUNCTION);
        }
        let (f, k) = split(function.imm_value());
        Ok((f, k, Some(Closure { w: callee, scanned })))
    }
}

/// 関数の番号の `FnEntry`。検査モードでは、`&f` の即値に詰めた捕まえた値の数が、クロージャの物体の走査する語の数と
/// 合うことも確かめる。
#[inline(always)]
fn lookup<M: Mode>(
    code: &CodeBase<M>,
    f: usize,
    k: usize,
    closure: Option<Closure>,
) -> Result<&FnEntry, VmFault> {
    let entry = code.functions.get(f).ok_or(NOT_A_FUNCTION)?;
    if M::CHECKED
        && let Some(closure) = closure
    {
        let words = code.signatures.words(entry.sig);
        let refs = words
            .starts
            .get(k)
            .filter(|_| k < usize::from(entry.arity))
            .map(|&captured| {
                words.params[..captured]
                    .iter()
                    .filter(|repr| repr.is_rc())
                    .count()
            });
        if refs.map(|refs| 1 + refs) != Some(closure.scanned) {
            return Err(VmFault::Internal(
                "the capture count packed with a function does not match its closure",
            ));
        }
    }
    Ok(entry)
}

/// 捕まえた値と scratch の引数で `f` に入る。捕まえた値は一時の列を通さず、クロージャから新しいフレームへ直に写す。
/// `k` は捕まえた値の語の数である。
#[inline(always)]
fn enter<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    f: usize,
    k: usize,
    closure: Option<Closure>,
    target: Target,
) -> Result<(), VmFault> {
    let entry = &code.functions[f];
    let size = usize::from(entry.frame_size);
    let base = match target {
        Target::Frame { ret, dst } => task.push_frame(ret, dst, size),
        Target::Tail => {
            task.reserve(task.base + size);
            task.frame_size = size;
            task.base
        }
    };
    if let Some(closure) = closure {
        let params = &code.signatures.words(entry.sig).params[..k];
        take_captures(ctx, closure, params, &mut task.stack[base..base + k])?;
    }
    let Task { stack, scratch, .. } = task;
    for (i, w) in scratch.drain(..).enumerate() {
        stack[base + k + i] = w;
    }
    task.pc = entry.start;
    Ok(())
}

/// クロージャの捕まえた値を、関数の引数の順に `dst` に写し、VM の持つクロージャの参照を手放す。参照の捕獲は `&f` の後の
/// 走査する語に、ほかの捕獲は生の語に、それぞれ引数の順に並ぶ (docs/spec/object-model.md の「クロージャ」)。
#[inline]
fn take_captures<M: Mode>(
    ctx: &mut Ctx<M>,
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

/// 結果を変換 `result` で受ける行き先。今のフレームの上に呼ぶなら、変換のフレームを積んで、その受けるスロットを
/// 行き先にする。末尾の位置なら、今のフレームの戻り先と合成する (`vm::tail_convert`)。
fn convert_result<M: Mode>(
    code: &CodeBase<M>,
    task: &mut Task,
    result: Conversion,
    target: Target,
) -> Result<Target, VmFault> {
    match target {
        Target::Frame { ret, dst } => {
            task.push_frame(ret, dst, result.size);
            Ok(Target::Frame {
                ret: result.start,
                dst: 0,
            })
        }
        Target::Tail => {
            vm::tail_convert(code, task, result)?;
            Ok(Target::Tail)
        }
    }
}

/// 渡す引数のうち先頭の `take` 個 (源の単位) を、関数の引数の Repr へ変える。`whole` (ちょうどの数の適用) なら結果の
/// 種類も比べ、合わなければ結果の変換を返す。合わない組が1つでもあれば `conversions` に1回数える。構造体と箱の間の
/// 変換は語の数を変えるので、そのときだけ scratch を作り直す。種類の等しい構造体の組は同じ形なので、語の位置は
/// 変わらない。
#[cold]
#[inline(never)]
#[expect(clippy::too_many_arguments, reason = "the decoded parts of one apply")]
fn convert<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    counts: &mut RunCounts,
    entry: &FnEntry,
    k: usize,
    site: Site,
    take: usize,
    whole: bool,
) -> Result<Option<Conversion>, VmFault> {
    let callee = code.signatures.get(entry.sig);
    let at = code.signatures.get(site.sig);
    let mut result = None;
    if whole {
        for (&from, &to) in callee.ret.iter().zip(&at.ret) {
            if class(from) != class(to) {
                check_pair(from, to)?;
                result = Some(
                    code.conversions
                        .get(class(from), class(to))
                        .ok_or(DIFFERENT_TYPES)?,
                );
            }
        }
    }
    let mut mismatched = result.is_some();
    let pairs = at.params[site.offset..site.offset + take]
        .iter()
        .zip(&callee.params[k..k + take]);
    let reshaped = pairs.clone().any(|(&from, &to)| {
        class(from) != class(to)
            && (matches!(from, Repr::Struct(_)) || matches!(to, Repr::Struct(_)))
    });
    if reshaped {
        // 作り直す並びは取っておいた領域に書き、読み終えた並びを次の作り直しのために取っておく。適用ごとに確保しない
        // ためである
        let spare = std::mem::take(&mut task.spare);
        let mut words = std::mem::replace(&mut task.scratch, spare);
        task.scratch.clear();
        let mut at = 0;
        for (&from, &to) in pairs {
            let width = code.structs.words(&from).len();
            let value = &words[at..at + width];
            if class(from) == class(to) {
                task.scratch.extend_from_slice(value);
            } else {
                check_pair(from, to)?;
                convert_value(code, ctx, counts, value, from, to, &mut task.scratch)?;
                mismatched = true;
            }
            at += width;
        }
        task.scratch.extend_from_slice(&words[at..]);
        words.clear();
        task.spare = words;
    } else {
        let mut at = 0;
        for (&from, &to) in pairs {
            if class(from) != class(to) {
                task.scratch[at] = convert_word(ctx, counts, task.scratch[at], from, to)?;
                mismatched = true;
            }
            at += code.structs.words(&from).len();
        }
    }
    if M::CHECKED && mismatched {
        counts.conversions += 1;
    }
    Ok(result)
}

/// 合わない組は同じ型の一様な Repr と箱を要する Repr の組である。単相化した2つの場所は同じ型を見ているためである
/// (docs/spec/vm.md の「シグネチャ」)。構造体は箱 (`obj` か `tobj`) と
/// だけ組になる。違う形の構造体どうしは種類が違うので、ここで断る。箱が同じ型の構造体の箱かは、取り出すときに検査
/// モードが配置で確かめる。
fn check_pair(from: Repr, to: Repr) -> Result<(), VmFault> {
    let boxes = |a: Repr, b: Repr| match b {
        Repr::Struct(_) => matches!(a, Repr::TObj | Repr::Obj),
        _ => a == Repr::TObj && b.needs_box(),
    };
    if boxes(from, to) || boxes(to, from) {
        Ok(())
    } else {
        Err(DIFFERENT_TYPES)
    }
}

/// 値 `w` を Repr `from` から `to` へ変える。一様な値から取り出すときは値を消費する。どちらもスカラーか一様な値の組に
/// 使う。
fn convert_word<M: Mode>(
    ctx: &mut Ctx<M>,
    counts: &mut RunCounts,
    w: Word,
    from: Repr,
    to: Repr,
) -> Result<Word, VmFault> {
    check_pair(from, to)?;
    // `check_pair` を通った組は、片方が `tobj` で、もう片方が箱を要するスカラーである
    match (class(from).scalar(), class(to).scalar()) {
        (None, Some(scalar)) => data::unbox_scalar(ctx, counts, w, scalar),
        (Some(scalar), _) => Ok(data::box_scalar(ctx, counts, w, scalar)),
        (None, None) => Ok(w),
    }
}

/// 語の並び `value` を Repr `from` から `to` へ変え、`out` に足す。構造体の葉は箱に入れ、箱は葉に開いて消費する。
fn convert_value<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    counts: &mut RunCounts,
    value: &[Word],
    from: Repr,
    to: Repr,
    out: &mut Vec<Word>,
) -> Result<(), VmFault> {
    match (from, to) {
        (Repr::Struct(id), _) => out.push(data::box_struct(&code.structs, ctx, counts, id, value)?),
        (_, Repr::Struct(id)) => data::take_struct(&code.structs, ctx, counts, id, value[0], out)?,
        _ => out.push(convert_word(ctx, counts, value[0], from, to)?),
    }
    Ok(())
}

fn deliver<M: Mode>(code: &CodeBase<M>, task: &mut Task, target: Target, value: Word) {
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

/// `f` と先頭の `k` 個 (源の単位) の引数からクロージャを作る。引数 `args` は構造体を葉に開いた語の並びである。
/// 走査する語に `&f` と参照の語を、生の語にほかの語を、`f` の引数の順に並べる。`&f` の即値には `k` を詰める
/// (docs/spec/object-model.md の「クロージャ」)。`apply` は引数の数を源の単位で比べるためである。
#[inline]
pub(crate) fn make_closure<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    f: u32,
    k: usize,
    lin: bool,
    args: &[Word],
) -> Result<Word, VmFault> {
    let entry = &code.functions[f as usize];
    let words = code.signatures.words(entry.sig);
    let params = words
        .starts
        .get(k)
        .filter(|&&captured| k <= usize::from(entry.arity) && captured == args.len())
        .map(|_| &words.params[..args.len()])
        .ok_or(VmFault::Internal(
            "a closure takes other arguments than its function's leading ones",
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
pub(crate) fn apply_rest<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    counts: &mut RunCounts,
) -> Result<(), VmFault> {
    let callee = task.get(0);
    let n = task.get(1).bits();
    let site = Site::new(code, SigId(task.get(2).bits() as u32), task.get(3).bits());
    task.load_scratch((4..4 + n).map(|i| i as Slot));
    apply(code, ctx, task, counts, callee, site, Target::Tail)
}

/// `ApplyRest` のフレームの引数のうち、参照の語のスロット。巻き戻しはまだ渡していないこれらだけを手放す。生の語を
/// `decref` すると、ビット列をポインタとして読んでしまう。残りの引数は源の単位の位置から始まる語の並びである。
pub(crate) fn rest_references<'c, M: Mode>(
    code: &'c CodeBase<M>,
    task: &Task,
) -> impl Iterator<Item = Slot> + 'c {
    let n = task.get(1).bits();
    let words = code.signatures.words(SigId(task.get(2).bits() as u32));
    let first = words.starts[task.get(3).bits()];
    (0..n)
        .filter(move |&i| words.params[first + i].is_rc())
        .map(|i| (4 + i) as Slot)
}
