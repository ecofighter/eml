//! 利用者の C の関数の呼び出し。引数の語を橋渡しの `u64` の slot にし、結果の語を値に戻す変換は、両方のモードで
//! ここだけが受け持つ (docs/spec/vm.md の「C の関数の命令」)。

use std::ffi::c_void;
use std::ptr;

use eml_cabi::Dtor;
use eml_extern::{CFunction, CInt, CParam, CRet, ExternEffect};
use eml_rt::{Ctx, Mode, Word};

use crate::code::CodeBase;
use crate::error::VmFault;
use crate::op::Slot;
use crate::vm::{Exec, PANICKED, Task, abort_to};

/// `extern "C" data` の物体の中身。C が返したポインタと、その型の dtor を持つ。
pub(crate) struct CHandle {
    /// C の側の値の位置。VM は参照外しせず、C に返すだけなので、来歴を持たない。
    ptr: *mut c_void,
    dtor: Option<Dtor>,
}

// SAFETY: VM は `ptr` を参照外ししない。dtor は物体を解放する worker のスレッドで走り、どのスレッドかは決めない
// (docs/spec/declarations.md の「`extern "C"`」)。
unsafe impl Send for CHandle {}

impl Drop for CHandle {
    fn drop(&mut self) {
        if let Some(dtor) = self.dtor {
            // SAFETY: `ptr` は、この型を返す C の関数が返した null でない値で、物体が解放されるのはこの1回だけである
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

/// 引数のスロットを橋渡しの slot の列にする。先に範囲の検査をすべて済ませ、次にポインタを取る。範囲の外の引数が
/// あれば、どのポインタも取らずに `Raise` を返す。何も確保しない。物体は借りるだけで、参照の数を変えない。
pub(crate) fn marshal<M: Mode>(
    ctx: &Ctx<M>,
    task: &Task,
    function: &CFunction,
    slots: &[u32],
) -> Result<Result<Vec<u64>, Raise>, VmFault> {
    let args = || function.params.iter().zip(slots.iter().map(|&s| s as Slot));
    for (i, (&param, slot)) in args().enumerate() {
        if let CParam::Int(int) = param {
            let value = task.raw(slot);
            if !int.fits(value) {
                return Ok(Err(Raise::Narrowing {
                    arg: i + 1,
                    int,
                    value,
                }));
            }
        }
    }
    let heap = ctx.heap();
    let mut out = Vec::with_capacity(slots.len());
    for (&param, slot) in args() {
        let w = task.get(slot);
        // SAFETY: LIR の借用の規則により、借りた物体は呼んだ側のスロットが生かしていて、`MutUnboxedArray` は一意で
        // 今の worker が持つ。C は呼び出しの間だけポインタを使い、`MutUnboxedArray` のポインタだけを通して書く
        // (docs/spec/object-model.md の「`unsafe` の核」の T16)。物体のポインタは来歴を公開して slot に入れる
        let slot = unsafe {
            match param {
                CParam::Int(_) => w.bits() as u64,
                CParam::Float => w.bits() as u64,
                CParam::UnboxedArray => heap.unboxed_ptr(w)?.expose_provenance() as u64,
                CParam::MutUnboxedArray => heap.unboxed_ptr_mut(w)?.expose_provenance() as u64,
                CParam::Text => heap.text_payload(w)?.expose_provenance() as u64,
                CParam::Handle(_) => (*heap.foreign::<CHandle>(w)?).ptr.addr() as u64,
                CParam::NoArg => continue,
            }
        };
        out.push(slot);
    }
    Ok(Ok(out))
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
                ptr: ptr::without_provenance_mut(ret as usize),
                dtor: code.c_handles[id.0 as usize].dtor,
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
