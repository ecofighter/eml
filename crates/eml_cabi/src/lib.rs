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

/// `Program::c_functions` と `Program::c_handles` に並びをそろえる。
#[derive(Clone, Debug, Default)]
pub struct Linked {
    pub calls: Vec<CCall>,
    pub dtors: Vec<Option<Dtor>>,
}

impl Linked {
    /// 長さがプログラムの表と合うか。呼ぶ前に並びが合っていることを、読み込む側が確かめるために使う。
    pub fn fits(&self, functions: &[CFunction], handles: &[CHandleType]) -> bool {
        self.calls.len() == functions.len() && self.dtors.len() == handles.len()
    }
}
