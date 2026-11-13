//! 利用者のCの関数の呼び出しの前後で生成したコードが呼ぶエントリ (docs/spec/native.mdの「externの失敗」)。externの行の
//! 一覧の行ではない。関数の名前を受けるエントリは、それを不死の文字列として借りるだけで、何も手放さない。行のエントリと
//! 同じく、`run`が入ったコードからだけ呼ぶ (docs/spec/native.mdの「実行の始めと終わり」)。
#![allow(unsafe_code)]

use eml_extern::CInt;
use eml_rows::ffi::{CHandle, Raise, handle_new};
use eml_rt::{Ctx, Fast, Word};
use eml_sched::error::RunFault;

use crate::enter::Fatal;
use crate::worker;

/// 内部の類の誤りをworkerの構造体へ移し、駆動にそれを伝える符号を返す。
///
/// # Safety
/// `worker::record`と同じ。
unsafe fn internal(ctx: &mut Ctx<Fast>, fault: impl Into<RunFault>) -> Fatal {
    // SAFETY: 呼ぶ側の約束
    unsafe { worker::record(ctx, fault.into(), None) }
}

/// 関数`name`の呼び出しが`why`で上げる`Panic`を置く。文言はVMと同じ`Raise::text`が作る。
///
/// # Safety
/// - `worker::park_panic`と同じ。ただしメッセージの語はここで作る
/// - `name`は、この`ctx`のヒープか、それに付けた塊の生きている文字列で、呼ぶ側が借りている
unsafe fn park(ctx: &mut Ctx<Fast>, name: Word, why: Raise) -> Result<(), Fatal> {
    // SAFETY: 呼ぶ側の約束
    let name = match unsafe { ctx.heap().string(name) } {
        Ok(name) => name,
        // SAFETY: 呼ぶ側の約束
        Err(error) => return Err(unsafe { internal(ctx, error) }),
    };
    // SAFETY: `name`は生きている文字列の中身を指し、この借用は文言を作り終えたところで終わる
    let text = why.text(unsafe { &*name });
    let message = ctx.fresh_string(&text);
    // SAFETY: 呼ぶ側の約束。`message`は今作った文字列で、その参照はworkerの構造体へ移る
    unsafe { worker::park_panic(ctx, message) };
    Ok(())
}

crate::entries! {
    pub(crate) const C;

    /// 関数`name`の引数`arg` (1から数える) の値`value`が、番号が`int` (`CInt::code`) のCの幅に収まらないときの
    /// panicを置いて戻る。生成したコードは、その後で行の純粋さで上げる。
    ///
    /// # Safety
    /// `ctx.runtime()`は`worker::of`の約束を満たし、失敗を置いていない。`name`はこの`ctx`のヒープか、それに付けた塊の
    /// 生きている文字列である。
    pub(crate) shim fn eml_x_c_narrow(ctx, name: obj, arg: int, int: int, value: int) -> never {
        let (Some(arg), Some(int)) = (
            usize::try_from(arg).ok().filter(|&arg| arg > 0),
            CInt::from_code(int),
        ) else {
            // SAFETY: このエントリの約束
            return Err(unsafe { internal(ctx, RunFault::Internal("a narrowing argument is malformed")) });
        };
        // SAFETY: このエントリの約束
        unsafe { park(ctx, name, Raise::Narrowing { arg, int, value }) }
    }

    /// ハンドルの物体が持つCのポインタ。生成したコードは、これをCの関数に渡す。読むだけで、確保も解放もせず、参照の
    /// 数も変えない。
    ///
    /// # Safety
    /// `ctx.runtime()`は`worker::of`の約束を満たす。`handle`は、この`ctx`のヒープの生きている物体で、呼ぶ側が借りている。
    /// 返したポインタをCに渡す者は、Cがそのポインタを使う間`handle`を生かしておく。だからその間、セルは落ちず、dtorは
    /// 走らない (`eml_rows::ffi::CHandle::new`の約束が、後で`ptr`をCに渡す者に課す務めである)。
    pub(crate) shim fn eml_x_c_handle_ptr(ctx, handle: obj) -> i64 {
        // SAFETY: このエントリの約束。`foreign`は速いモードでも中身の型を確かめる
        match unsafe { ctx.heap().foreign::<CHandle>(handle) } {
            // SAFETY: `cell`は、生きている物体の中身の`CHandle`を指す
            Ok(cell) => Ok::<i64, Fatal>(unsafe { &*cell }.ptr().addr() as i64),
            // SAFETY: このエントリの約束
            Err(error) => Err(unsafe { internal(ctx, error) }),
        }
    }

    /// Cの関数が返したポインタ`ptr`のハンドルの物体を作る。nullのポインタでは何も作らず、nullのハンドルのpanicを
    /// 置いて仮の値を返す。生成したコードは、呼び出しのすぐ後でfaultの語を確かめる。
    ///
    /// # Safety
    /// `ptr`と`dtor`は`eml_rows::ffi::handle_new`の約束を満たす。生成したコードは、`ptr`に、宣言した結果がそのハンドルの
    /// 型である`eml_c$<name>`の呼び出しの結果を、`dtor`に、0か、取り込み`eml_cd$<そのハンドルの型の名前>`のアドレスを
    /// 渡して、これを満たす。`load`はその取り込みを、`Linked::fits`が成り立った後でだけ`Linked::dtors()`の要素に結ぶ。
    /// `ptr`が0なら、`ctx.runtime()`は`worker::of`の約束を満たし、失敗を置いておらず、`name`はこの`ctx`のヒープか、
    /// それに付けた塊の生きている文字列である。
    pub(crate) shim fn eml_x_c_handle_new(ctx, name: obj, dtor: dtor, ptr: i64) -> obj {
        // SAFETY: `ptr`と`dtor`はこのエントリの約束による。`Ctx::heap_mut`の約束は、`shim`が渡すこのworkerの生きている
        // `Ctx`で満たす。`handle_new`は物体を1つ作るだけで、`Ctx`の持つ語に触れない
        match unsafe { handle_new(ctx, ptr, dtor) } {
            Some(handle) => Ok::<Word, Fatal>(handle),
            None => {
                // SAFETY: このエントリの約束
                unsafe { park(ctx, name, Raise::NullHandle) }?;
                Ok(Word::UNIT)
            }
        }
    }
}
