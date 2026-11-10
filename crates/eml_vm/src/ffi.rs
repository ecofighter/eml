//! 利用者の C の関数の呼び出し。引数の語を橋渡しの `u64` の slot にし、結果の語を値に戻す変換は、両方のモードで
//! ここだけが受け持つ (docs/spec/vm.md の「C の関数の命令」、docs/spec/vm.md の「`blocking` の呼び出し」)。

use std::any::Any;
use std::ffi::c_void;
use std::ptr;
use std::sync::Arc;

use eml_cabi::{CCall, Dtor};
use eml_extern::{CFunction, CInt, CParam, CRet, ExternEffect, Purity};
use eml_rt::externs::{Fault, Raised};
use eml_rt::{Ctx, Kind, Mode, PagePin, Word};

use crate::cells::{self, JobSource};
use crate::code::CodeBase;
use crate::error::VmFault;
use crate::op::Slot;
use crate::vm::{Exec, PANICKED, Task, abort_to, failed, io_kind};

/// `extern "C" data`の物体の中身。
pub(crate) struct CHandle {
    cell: Arc<HandleCell>,
}

/// `extern "C" data`の物体のCのポインタと、その型のdtor。物体の`CHandle`と、それを渡したどの`blocking`の
/// 呼び出しもこのセルを共有するので、dtorはその最後の1つが放したときに走る。ふつうは物体を解放するworkerで、
/// 見捨てた呼び出しが物体より長く生きるときは、Cが戻った直後のプールのスレッドである。
struct HandleCell {
    /// Cの側の値の位置。VMは参照外しせず、Cに返すだけなので、来歴を持たない。
    ptr: *mut c_void,
    dtor: Option<Dtor>,
}

// SAFETY: VMは`ptr`を参照外ししない。dtorは最後の参照を落としたスレッドでちょうど1回走り、どのスレッドかは
// 決めない (docs/spec/declarations.md の「`extern "C"`」)。
unsafe impl Send for HandleCell {}
// SAFETY: `HandleCell`は内側の可変性を持たず、`&self`は`ptr`と`dtor`を読むだけである
unsafe impl Sync for HandleCell {}

impl Drop for HandleCell {
    fn drop(&mut self) {
        if let Some(dtor) = self.dtor {
            // SAFETY: `ptr`は、この型を返すCの関数が返したnullでない値で、セルが落ちるのはこの1回だけである
            unsafe { dtor(self.ptr) }
        }
    }
}

/// C の関数の呼び出しが上げる `Panic` の理由。
pub(crate) enum Raise {
    /// 狭める `Int as` の引数が範囲の外にある。`arg` は 1 から数える。
    Narrowing { arg: usize, int: CInt, value: i64 },
    /// ハンドルを返す関数が null を返した。
    NullHandle,
}

impl Raise {
    pub(crate) fn text(&self, function: &CFunction) -> String {
        let name = &function.name;
        match *self {
            Raise::Narrowing { arg, int, value } => format!(
                "argument {arg} of `{name}` does not fit in C `{}`: {value}",
                int.c_name()
            ),
            Raise::NullHandle => format!("`{name}` returned a null handle"),
        }
    }
}

/// C の関数の結果を戻した値。
pub(crate) enum Out {
    /// `Int` と `Float` の生の語。
    Raw(i64),
    /// `()` とハンドルの物体。
    Word(Word),
}

/// 狭める`Int as`の引数の範囲をすべて確かめる。どのポインタを取るよりも、どの写しを作るよりも先に走るので、
/// 範囲の外の引数があっても放すものはない。
fn narrowing(task: &Task, function: &CFunction, slots: &[u32]) -> Option<Raise> {
    let args = function.params.iter().zip(slots.iter().map(|&s| s as Slot));
    for (i, (&param, slot)) in args.enumerate() {
        if let CParam::Int(int) = param {
            let value = task.raw(slot);
            if !int.fits(value) {
                return Some(Raise::Narrowing {
                    arg: i + 1,
                    int,
                    value,
                });
            }
        }
    }
    None
}

/// 同期の呼び出しの引数のスロットを橋渡しのslotの列にする。先に範囲の検査をすべて済ませ、次にポインタを取る。
/// 範囲の外の引数があれば、どのポインタも取らずに`Raise`を返す。何も確保しない。物体は借りるだけで、参照の数を
/// 変えない。
fn marshal<M: Mode>(
    ctx: &Ctx<M>,
    task: &Task,
    function: &CFunction,
    slots: &[u32],
) -> Result<Result<Vec<u64>, Raise>, VmFault> {
    if let Some(why) = narrowing(task, function, slots) {
        return Ok(Err(why));
    }
    let heap = ctx.heap();
    let mut out = Vec::with_capacity(slots.len());
    for (&param, slot) in function.params.iter().zip(slots.iter().map(|&s| s as Slot)) {
        let w = task.get(slot);
        // SAFETY: LIRの借用の規則により、借りた物体は呼んだ側のスロットが生かしていて、`MutUnboxedArray`は一意で
        // 今のworkerが持つ。Cは呼ぶ間だけこのスレッドでポインタを使い、`MutUnboxedArray`のポインタだけを通して
        // 書く (docs/spec/object-model.md の「`unsafe` の核」のT16)。物体のポインタは来歴を公開してslotに入れる
        let slot = unsafe {
            match param {
                CParam::Int(_) => w.bits() as u64,
                CParam::Float => w.bits() as u64,
                CParam::UnboxedArray => heap.unboxed_ptr(w)?.expose_provenance() as u64,
                CParam::MutUnboxedArray => heap.unboxed_ptr_mut(w)?.expose_provenance() as u64,
                CParam::Text => heap.text_payload(w)?.expose_provenance() as u64,
                CParam::Handle(_) => (&*heap.foreign::<CHandle>(w)?).cell.ptr.addr() as u64,
                CParam::NoArg => continue,
            }
        };
        out.push(slot);
    }
    Ok(Ok(out))
}

/// `blocking`の呼び出しの引数で、仕事が持つ。大きい物体のページにある配列、文字列、バイト列は、そのページを留めて
/// 貸し、ほかの物体は写す。runより長く生きる呼び出しも、貸した範囲のほかはヒープに触れず、そのページは仕事が
/// 留めを落とすまで返らない (docs/spec/vm.mdの「`blocking` の呼び出し」、docs/spec/object-model.mdの「ページの留め」)。
enum Arg {
    /// `Int`と`Float`の生の語。
    Word(u64),
    /// `UnboxedArray`の要素の写し。
    Array(Box<[u64]>),
    /// `MutUnboxedArray`の要素の写し。仕事が返し、取り出しが配列に書き戻す。
    MutArray(Box<[u64]>),
    /// `String`か`Bytes`の中身の写し。終わりのNULは置かない。
    Text(Box<[u8]>),
    /// 大きい物体のページにある配列、文字列、バイト列の、`Op::Ffi`が渡すのと同じ位置と、そのページの留め。
    Lent(u64, PagePin),
    /// ハンドルのセルへの参照。Cが戻るまでdtorを走らせないためである。
    Handle(Arc<HandleCell>),
}

impl Arg {
    /// この引数の橋渡しのslot。写しの位置は来歴を公開してslotに入れる。
    fn slot(&mut self) -> u64 {
        match self {
            Arg::Word(bits) => *bits,
            Arg::Array(elements) => elements.as_ptr().expose_provenance() as u64,
            Arg::MutArray(elements) => elements.as_mut_ptr().expose_provenance() as u64,
            Arg::Text(bytes) => bytes.as_ptr().expose_provenance() as u64,
            Arg::Lent(at, _) => *at,
            Arg::Handle(cell) => cell.ptr.addr() as u64,
        }
    }
}

/// `blocking`の呼び出しの仕事の答え。結果の語と、`MutUnboxedArray`の引数ごとに引数の順で1つずつの項を持つ。
/// 項は写しか、Cがその場で書いた貸した配列の`None`である。
struct CAnswer {
    ret: u64,
    back: Vec<Option<Box<[u64]>>>,
}

/// `blocking`の呼び出しの仕事の引数を作る。`marshal`と同じく先に範囲の検査をすべて済ませ、範囲の外の引数が
/// あれば何も留めず、何も写さずに`Raise`を返す。物体は借りるだけで、参照の数を変えない。大きい物体のページにある
/// 配列、文字列、バイト列は留めて貸し、ほかの物体は写す。
fn job_args<M: Mode>(
    ctx: &Ctx<M>,
    task: &Task,
    function: &CFunction,
    slots: &[u32],
) -> Result<Result<Vec<Arg>, Raise>, VmFault> {
    if let Some(why) = narrowing(task, function, slots) {
        return Ok(Err(why));
    }
    let heap = ctx.heap();
    let mut out = Vec::with_capacity(slots.len());
    for (&param, slot) in function.params.iter().zip(slots.iter().map(|&s| s as Slot)) {
        let w = task.get(slot);
        // SAFETY: LIRの借用の規則により、借りた物体は呼んだ側のスロットが生かしていて、`MutUnboxedArray`は一意で
        // 今のworkerが持つ。留めと写しはここで持ち主のworkerが作る。貸した物体は、待つタスクの継続が数えた参照を
        // 持つので`FfiTake`が戻るまで生き、仕事を見捨てても留めがそのページを返させない
        // (docs/spec/object-model.mdの「`unsafe` の核」のT16とT17)
        let arg = unsafe {
            match param {
                CParam::Int(_) | CParam::Float => Arg::Word(w.bits() as u64),
                CParam::UnboxedArray => {
                    let at = heap.unboxed_ptr(w)?;
                    match heap.pin(w)? {
                        Some(pin) => Arg::Lent(at.expose_provenance() as u64, pin),
                        None => Arg::Array(elements(at, heap.unboxed_len(w)?)),
                    }
                }
                CParam::MutUnboxedArray => {
                    let at = heap.unboxed_ptr_mut(w)?;
                    match heap.pin(w)? {
                        Some(pin) => Arg::Lent(at.expose_provenance() as u64, pin),
                        None => Arg::MutArray(elements(at.cast_const(), heap.unboxed_len(w)?)),
                    }
                }
                CParam::Text => {
                    let at = heap.text_payload(w)?;
                    match heap.pin(w)? {
                        Some(pin) => Arg::Lent(at.expose_provenance() as u64, pin),
                        None => {
                            let text: *const [u8] = if heap.meta(w)?.kind() == Kind::String {
                                heap.string(w)? as *const [u8]
                            } else {
                                heap.bytes(w)?
                            };
                            Arg::Text(Box::from(&*text))
                        }
                    }
                }
                CParam::Handle(_) => Arg::Handle(Arc::clone(&(&*heap.foreign::<CHandle>(w)?).cell)),
                CParam::NoArg => continue,
            }
        };
        out.push(arg);
    }
    Ok(Ok(out))
}

/// 位置`at`から`n`個の要素の写し。
///
/// # Safety
/// `at`は生きている配列の要素0の位置で、配列は`n`個の要素を持つ。
unsafe fn elements(at: *const u64, n: usize) -> Box<[u64]> {
    // SAFETY: 呼び手の約束による
    Box::from(unsafe { std::slice::from_raw_parts(at, n) })
}

/// 橋渡しの結果の語を値に戻す。null のハンドルは何も確保せずに `Raise` を返す。`function` のハンドルの型の番号は
/// `code` の番号である。
pub(crate) fn unmarshal<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    function: &CFunction,
    ret: u64,
) -> Result<Result<Out, Raise>, VmFault> {
    Ok(Ok(match function.ret {
        CRet::Void => Out::Word(Word::UNIT),
        // 橋渡しは `int32_t` の結果の符号を広げて返し、`size_t` の結果は読み替えたままにする
        CRet::Int(_) | CRet::Float => Out::Raw(ret as i64),
        CRet::Handle(_) if ret == 0 => return Ok(Err(Raise::NullHandle)),
        CRet::Handle(id) => {
            let handle = CHandle {
                cell: Arc::new(HandleCell {
                    ptr: ptr::without_provenance_mut(ret as usize),
                    dtor: code.c_handles[id.0 as usize].dtor,
                }),
            };
            // SAFETY: 物体を1つ作るだけで、`Ctx` の持つ語に触れない
            Out::Word(unsafe { ctx.heap_mut() }.alloc_foreign(handle, true))
        }
    }))
}

/// `Op::Ffi`。引数を組み、橋渡しで C の関数を呼び、結果を `dst` に書くか `Panic` を上げる。
pub(crate) fn call<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    dst: Slot,
    at: u32,
) -> Result<(), VmFault> {
    let ops = &code.tables.operands[at as usize..];
    let entry = &code.c_functions[ops[0] as usize];
    let slots = &ops[2..2 + ops[1] as usize];
    let args = match marshal(ctx, task, &entry.function, slots)? {
        Ok(args) => args,
        Err(why) => return raise::<R>(code, ctx, task, dst, &entry.function, why),
    };
    // SAFETY: `Linked::new` の約束により、`callee` は組にした宣言を解決した関数で、`bridge` はその宣言の
    // `c_signature()` の橋渡しであり、どちらのコードもプロセスが終わるまで有効である。`load` が `fits` でその宣言を
    // プログラムの表と比べた。`entry.function` は表の関数を番号だけ付け直したもので、`args` は橋渡しの読む slot
    // の数だけある (docs/spec/vm.md の「橋渡しの関数」)
    let ret = unsafe { (entry.call.bridge)(entry.call.callee, args.as_ptr()) };
    match unmarshal(code, ctx, &entry.function, ret)? {
        Ok(out) => {
            write_out(task, dst, out);
            Ok(())
        }
        Err(why) => raise::<R>(code, ctx, task, dst, &entry.function, why),
    }
}

/// `Op::FfiSubmit`の前半。引数を写すか留めて貸し、プールでCの関数を呼ぶ仕事を作って、その出どころ (`CodeBase`の
/// Cの関数の番号を持つ`JobSource::C`) とともに返す。範囲の外の引数は記録を作らずに`Panic`を上げ、`None`を返す
/// (docs/spec/vm.mdの「`blocking` の呼び出し」)。
pub(crate) fn job<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    dst: Slot,
    at: u32,
) -> Result<Option<(JobSource, eml_rt::Op)>, VmFault> {
    let ops = &code.tables.operands[at as usize..];
    let c = ops[0];
    let entry = &code.c_functions[c as usize];
    let mut args = match job_args(ctx, task, &entry.function, &ops[2..2 + ops[1] as usize])? {
        Ok(args) => args,
        Err(why) => {
            raise::<R>(code, ctx, task, dst, &entry.function, why)?;
            return Ok(None);
        }
    };
    let call: CCall = entry.call;
    let params: Vec<CParam> = entry
        .function
        .params
        .iter()
        .copied()
        .filter(|&param| param != CParam::NoArg)
        .collect();
    let job = eml_rt::Op::Job(Box::new(move || -> Box<dyn Any + Send> {
        // 欄ごとに捕まえると `callee` の生のポインタだけが移り、`Send` を失う。`CCall` をまるごと移す
        let call = call;
        let slots: Vec<u64> = args.iter_mut().map(Arg::slot).collect();
        // SAFETY: `call`は`Op::Ffi`と同じく`Linked::new`の約束と`fits`の比べで宣言の橋渡しと関数であり、`slots`は
        // 橋渡しの読むslotの数だけある (docs/spec/vm.md の「橋渡しの関数」)。`slots`の物体の位置はどれも、この閉包が
        // 持つ写しか、`args`の留めがページを返させない貸した範囲を指す。ハンドルのslotはCのポインタで、`args`の
        // `Arc<HandleCell>`がそのdtorを走らせない
        let ret = unsafe { (call.bridge)(call.callee, slots.as_ptr()) };
        // 引数は、留めも含めてここで落とす。Cが戻った後で、答えを届ける前である
        let mut back = Vec::new();
        for (param, arg) in params.into_iter().zip(args) {
            let copy = match arg {
                Arg::MutArray(elements) => Some(elements),
                Arg::Lent(_, pin) => {
                    drop(pin);
                    None
                }
                _ => None,
            };
            if param == CParam::MutUnboxedArray {
                back.push(copy);
            }
        }
        Box::new(CAnswer { ret, back })
    }));
    Ok(Some((JobSource::C(c), job)))
}

/// `Op::FfiTake`。記録を放し、`MutUnboxedArray`の写しを配列に書き戻し、`Wake`と答えで分ける。取り消されたタスクは、
/// 仕事が返したハンドルをその場で壊し、取り消しのIOの失敗を起こす (docs/spec/vm.md の「`blocking` の呼び出し」)。
pub(crate) fn take<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    shared: &cells::Shared,
    dst: Slot,
    at: u32,
) -> Result<(), VmFault> {
    let ops = &code.tables.operands[at as usize..];
    let c = ops[0];
    let entry = &code.c_functions[c as usize];
    let (token, wake) = (task.raw(ops[2] as Slot), task.raw(ops[3] as Slot));
    let taken = cells::take_job(shared, ctx, token, task.worker)?;
    if taken.from != JobSource::C(c) {
        return Err(VmFault::Internal("an op is taken by another function"));
    }
    let function = &entry.function;
    let result = match taken.answer::<CAnswer>()? {
        Ok(CAnswer { ret, back }) => {
            // 仕事は走り終えたので、取り消されたタスクでもCの副作用は残る。そのため写しはどちらの分かれ道でも
            // 書き戻す
            write_back(ctx, task, function, &ops[4..2 + ops[1] as usize], back)?;
            Ok(ret)
        }
        Err(e) => Err(e),
    };
    match (wake, result) {
        (cells::WOKE, Ok(r)) => match unmarshal(code, ctx, function, r)? {
            Ok(out) => {
                write_out(task, dst, out);
                Ok(())
            }
            Err(why) => raise::<R>(code, ctx, task, dst, function, why),
        },
        (cells::WOKE, Err(e)) => {
            let fault = Fault::Raised(Box::new(Raised {
                kind: io_kind(&e),
                message: format!("cannot call `{}`: {e}", function.name).into(),
            }));
            failed::<R>(code, ctx, task, dst, fault, Purity::Effectful)
        }
        (cells::CANCELLED, result) => {
            // 仕事は走り終えたので C の副作用は残る。返したハンドルだけは、持ち主のスレッドで型の dtor に渡して壊す
            if let (CRet::Handle(h), Ok(r)) = (function.ret, result)
                && r != 0
            {
                drop(HandleCell {
                    ptr: ptr::without_provenance_mut(r as usize),
                    dtor: code.c_handles[h.0 as usize].dtor,
                });
            }
            let fault = Fault::Raised(Box::new(Raised {
                kind: cells::IO_CANCELLED,
                message: "task was cancelled".into(),
            }));
            failed::<R>(code, ctx, task, dst, fault, Purity::Effectful)
        }
        _ => Err(VmFault::Internal("an op is taken with another wake")),
    }
}

const MISMATCH: VmFault = VmFault::Internal("a blocking call's copy does not match its array");

/// `MutUnboxedArray`の写し`back`を、取り出しの物体の引数`objects`のうち`MutUnboxedArray`のものに、引数の順に
/// 組にして書き戻す。`None`はCがその場で書いた貸した配列なので、飛ばす。
fn write_back<M: Mode>(
    ctx: &Ctx<M>,
    task: &Task,
    function: &CFunction,
    objects: &[u32],
    back: Vec<Option<Box<[u64]>>>,
) -> Result<(), VmFault> {
    let params = function.params.iter().filter(|param| {
        matches!(
            param,
            CParam::UnboxedArray | CParam::MutUnboxedArray | CParam::Text | CParam::Handle(_)
        )
    });
    let arrays: Vec<Word> = params
        .zip(objects)
        .filter(|&(&param, _)| param == CParam::MutUnboxedArray)
        .map(|(_, &slot)| task.get(slot as Slot))
        .collect();
    if arrays.len() != back.len() {
        return Err(MISMATCH);
    }
    let heap = ctx.heap();
    for (w, copy) in arrays.into_iter().zip(back) {
        let Some(copy) = copy else { continue };
        // SAFETY: 取り出しの物体の引数は呼び出しの引数で、待つタスクの継続が生かしている。`MutUnboxedArray`は一意で
        // このworker (持ち主) が持ち、写しの長さは書く前に配列の要素の数と比べる
        unsafe {
            let at = heap.unboxed_ptr_mut(w)?;
            if heap.unboxed_len(w)? != copy.len() {
                return Err(MISMATCH);
            }
            ptr::copy_nonoverlapping(copy.as_ptr(), at, copy.len());
        }
    }
    Ok(())
}

/// `Raise` を `Panic` への中断にする。捕まえる handler がなければ、文言の実行時エラーにする。
fn raise<R: Exec>(
    code: &CodeBase<R::M>,
    ctx: &mut Ctx<R::M>,
    task: &mut Task,
    dst: Slot,
    function: &CFunction,
    why: Raise,
) -> Result<(), VmFault> {
    let text = why.text(function);
    if abort_to::<R, _>(code, ctx, task, dst, ExternEffect::Panic, PANICKED, |ctx| {
        [ctx.fresh_string(&text)]
    })? {
        return Ok(());
    }
    Err(VmFault::Panicked(text))
}

fn write_out(task: &mut Task, dst: Slot, out: Out) {
    match out {
        Out::Raw(n) => task.set_raw(dst, n),
        Out::Word(w) => task.set(dst, w),
    }
}
