//! 利用者のCの関数の呼び出しの部品。ハンドルの物体の中身、呼び出しが上げる`Panic`の理由、ハンドルを作る関数を持ち、
//! VMとネイティブの実行系が使う (docs/spec/vm.mdの「C の関数の呼び出し」、docs/spec/native.mdの「externの失敗」)。

use std::ffi::c_void;
use std::ptr;
use std::sync::Arc;

pub use eml_cabi::Dtor;
use eml_extern::CInt;
use eml_rt::{Ctx, Mode, Word};

/// `extern "C" data`の物体の中身。
pub struct CHandle {
    cell: Arc<HandleCell>,
}

impl CHandle {
    /// Cが返したポインタと、その型のdtorからハンドルの中身を作る。dtorはセルへの最後の参照を落としたときに1回走り、
    /// 落とさないセルでは走らない。
    ///
    /// # Safety
    /// `dtor`は`None`か、次がすべて成り立つ。`dtor`はプロセスが終わるまで有効なコードを指し、`void f(void*)`として
    /// 呼べる。この呼び出しの後のいつでも、どのスレッドでも、`ptr`を渡して1回呼んでよく、巻き戻しを起こさない。その
    /// 1回はこの呼び出しが受け取る。ほかに`ptr`に`dtor`を呼ぶものはなく、後で`ptr`をCに渡す者は、Cが使う間、
    /// `CHandle`かそのセルの写しを持つ。
    pub unsafe fn new(ptr: *mut c_void, dtor: Option<Dtor>) -> CHandle {
        CHandle {
            cell: Arc::new(HandleCell { ptr, dtor }),
        }
    }

    /// Cのポインタ。
    pub fn ptr(&self) -> *mut c_void {
        self.cell.ptr
    }

    /// セルへの参照。`blocking`の呼び出しが、Cが戻るまで持つ。
    pub fn cell(&self) -> Arc<HandleCell> {
        Arc::clone(&self.cell)
    }
}

/// `extern "C" data`の物体のCのポインタと、その型のdtor。物体の`CHandle`と、それを渡したどの`blocking`の
/// 呼び出しもこのセルを共有するので、dtorはその最後の1つが放したときに走る。ふつうは物体を解放するworkerで、
/// 見捨てた呼び出しが物体より長く生きるときは、Cが戻った直後のプールのスレッドである。
pub struct HandleCell {
    /// Cの側の値の位置。このcrateは参照外しせず、Cに返すだけなので、来歴を持たない。
    ptr: *mut c_void,
    dtor: Option<Dtor>,
}

impl HandleCell {
    /// Cのポインタ。
    pub fn ptr(&self) -> *mut c_void {
        self.ptr
    }
}

// SAFETY: このcrateは`ptr`を参照外ししない。dtorは最後の参照を落としたスレッドで1回走り、`CHandle::new`の約束は
// どのスレッドも許す (docs/spec/declarations.mdの「`extern "C"`」)。
unsafe impl Send for HandleCell {}
// SAFETY: `HandleCell`は内側の可変性を持たず、`&self`は`ptr`と`dtor`を読むだけである
unsafe impl Sync for HandleCell {}

impl Drop for HandleCell {
    fn drop(&mut self) {
        if let Some(dtor) = self.dtor {
            // SAFETY: セルを作った`CHandle::new`の約束により、dtorは`ptr`に対して1回呼んでよい。セルが落ちるのは
            // この1回だけである
            unsafe { dtor(self.ptr) }
        }
    }
}

/// Cの関数の呼び出しが上げる`Panic`の理由。
pub enum Raise {
    /// 狭める`Int as`の引数が範囲の外にある。`arg`は1から数える。
    Narrowing { arg: usize, int: CInt, value: i64 },
    /// ハンドルを返す関数がnullを返した。
    NullHandle,
}

impl Raise {
    /// `name`の関数の呼び出しが上げる`Panic`の文言。
    pub fn text(&self, name: &str) -> String {
        match *self {
            Raise::Narrowing { arg, int, value } => format!(
                "argument {arg} of `{name}` does not fit in C `{}`: {value}",
                int.c_name()
            ),
            Raise::NullHandle => format!("`{name}` returned a null handle"),
        }
    }
}

/// Cが返したハンドルの物体を作る。nullのポインタは何も作らずに`None`を返す。そうでなければ物体を確保する前に
/// `CHandle`を作るので、確保がpanicしても巻き戻しが`CHandle`を落とし、dtorは1回走る。
///
/// # Safety
/// `Ctx::heap_mut`と同じである。加えて、`ptr`が0でないときは、`dtor`と、番地が`ptr`で来歴を持たないポインタに
/// ついて`CHandle::new`と同じである。
pub unsafe fn handle_new<M: Mode>(ctx: &mut Ctx<M>, ptr: i64, dtor: Option<Dtor>) -> Option<Word> {
    if ptr == 0 {
        return None;
    }
    // SAFETY: 呼び手の約束による
    let handle = unsafe { CHandle::new(ptr::without_provenance_mut(ptr as usize), dtor) };
    // SAFETY: 呼び手の約束による。物体を1つ作るだけで、`Ctx`の持つ語に触れない
    Some(unsafe { ctx.heap_mut() }.alloc_foreign(handle, true))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use eml_sched::cells;

    use super::*;

    static NULL_DTORS: AtomicUsize = AtomicUsize::new(0);
    static ONCE_DTORS: AtomicUsize = AtomicUsize::new(0);

    // どちらのdtorも引数を読まないので、番地0と8に対して`CHandle::new`の約束が成り立つ
    unsafe extern "C" fn count_null(_: *mut c_void) {
        NULL_DTORS.fetch_add(1, Ordering::SeqCst);
    }

    unsafe extern "C" fn count_once(_: *mut c_void) {
        ONCE_DTORS.fetch_add(1, Ordering::SeqCst);
    }

    #[test]
    fn a_null_pointer_makes_no_handle() {
        let mut ctx = cells::test_ctx();
        assert!(unsafe { handle_new(&mut ctx, 0, Some(count_null)) }.is_none());
        assert_eq!(NULL_DTORS.load(Ordering::SeqCst), 0);
        let leaks = ctx.leaks();
        assert!(leaks.is_empty(), "{leaks:?}");
    }

    #[test]
    fn a_handle_runs_its_dtor_once() {
        let mut ctx = cells::test_ctx();
        let w = unsafe { handle_new(&mut ctx, 8, Some(count_once)) }.expect("a handle");
        assert_eq!(ONCE_DTORS.load(Ordering::SeqCst), 0);
        unsafe { ctx.heap_mut().decref(w) }.unwrap();
        assert_eq!(ONCE_DTORS.load(Ordering::SeqCst), 1);
        let leaks = ctx.leaks();
        assert!(leaks.is_empty(), "{leaks:?}");
        assert_eq!(ONCE_DTORS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_raise_texts_name_the_function() {
        let narrowing = Raise::Narrowing {
            arg: 2,
            int: CInt::I32,
            value: 1 << 40,
        };
        assert_eq!(
            narrowing.text("Lib.f"),
            "argument 2 of `Lib.f` does not fit in C `int32_t`: 1099511627776"
        );
        assert_eq!(
            Raise::NullHandle.text("Lib.open"),
            "`Lib.open` returned a null handle"
        );
    }
}
