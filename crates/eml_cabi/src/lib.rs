//! C の関数を呼ぶコードのポインタ。`eml_native` が作り、`eml_vm` が使う。
//! `Ctx` に関わらないので、`eml_rt` に依存せず、`eml_extern` だけに依存する葉の crate にする。

use eml_extern::{CFunction, CHandleType};

/// 引数の語の列を読んで C の関数を呼び、結果を1語で返す。
pub type Bridge = unsafe extern "C" fn(callee: *const (), args: *const u64) -> u64;

pub type Dtor = unsafe extern "C" fn(*mut core::ffi::c_void);

#[derive(Clone, Copy, Debug)]
pub struct CCall {
    pub bridge: Bridge,
    pub callee: *const (),
}

// SAFETY: 2つのポインタが指すコードは、プロセスが終わるまで外さない。橋渡しの関数は状態を持たないので、
// どのスレッドから呼んでもよい (docs/spec/vm.md の「橋渡しの関数」、docs/spec/vm.md の「ライブラリと記号」)。
unsafe impl Send for CCall {}
unsafe impl Sync for CCall {}

/// `Program::c_functions` と `Program::c_handles` に並びをそろえた、C の関数の呼び出しとハンドルの型の dtor。
/// VM はこのポインタをそのまま呼ぶので、欄を非公開にし、作るのを `unsafe` の `new` に限る。空の `default` は何も
/// 呼ばないので安全である。
#[derive(Clone, Debug, Default)]
pub struct Linked {
    calls: Vec<CCall>,
    dtors: Vec<Option<Dtor>>,
}

impl Linked {
    /// # Safety
    /// `calls` と `dtors` は、読み込むプログラムの C の関数の表とハンドルの型の表の順に並ぶ。`calls` のそれぞれの
    /// `bridge` は、表のその C の関数の署名 (`CFunction::c_signature`) を読む橋渡しの関数で、`callee` はその署名の
    /// C の関数である。`dtors` のそれぞれは `void f(void*)` として呼べる関数である。どのポインタも、プロセスが
    /// 終わるまで有効なコードを指す (docs/spec/vm.md の「橋渡しの関数」、docs/spec/vm.md の「ライブラリと記号」)。
    pub unsafe fn new(calls: Vec<CCall>, dtors: Vec<Option<Dtor>>) -> Linked {
        Linked { calls, dtors }
    }

    /// C の関数の表の順の呼び出し。
    pub fn calls(&self) -> &[CCall] {
        &self.calls
    }

    /// ハンドルの型の表の順の dtor。
    pub fn dtors(&self) -> &[Option<Dtor>] {
        &self.dtors
    }

    /// 長さがプログラムの表と合うか。呼ぶ前に並びが合っていることを、読み込む側が確かめるために使う。
    pub fn fits(&self, functions: &[CFunction], handles: &[CHandleType]) -> bool {
        self.calls.len() == functions.len() && self.dtors.len() == handles.len()
    }
}
