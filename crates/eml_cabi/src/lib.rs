//! C の関数を呼ぶコードのポインタ。`eml_clink` が作り、`eml_vm` が使う。
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
/// 呼ばないので安全である。ポインタを保証するのはつないだときの宣言なので、`Linked` はその宣言の写しを持ち、読み込む
/// 側が `fits` でプログラムの表と比べる。別のプログラムのためにつないだ `Linked` を渡しても、その宣言が保証しない
/// 呼び方で C の関数や dtor を呼ばないためである。
#[derive(Clone, Debug, Default)]
pub struct Linked {
    functions: Vec<CFunction>,
    calls: Vec<CCall>,
    handles: Vec<CHandleType>,
    dtors: Vec<Option<Dtor>>,
}

impl Linked {
    /// `calls` は C の関数の表の順の (宣言, 呼び出し)、`dtors` はハンドルの型の表の順の (宣言, dtor) である。
    ///
    /// # Safety
    /// `calls` のそれぞれの `callee` は、組にした宣言の記号と `links` を解決して得た関数で、`bridge` はその宣言の
    /// `c_signature()` の slot を読む橋渡しの関数である。`dtors` のそれぞれは、宣言が dtor を持つときに限り `Some`
    /// で、その dtor の記号と `links` を解決して得た、`void f(void*)` として呼べる関数である。どのポインタも、
    /// プロセスが終わるまで有効なコードを指す (docs/spec/vm.md の「橋渡しの関数」、docs/spec/vm.md の
    /// 「ライブラリと記号」)。宣言が読み込むプログラムの表と同じことは、読み込む側が `fits` で確かめる。
    pub unsafe fn new(
        calls: Vec<(CFunction, CCall)>,
        dtors: Vec<(CHandleType, Option<Dtor>)>,
    ) -> Linked {
        let (functions, calls) = calls.into_iter().unzip();
        let (handles, dtors) = dtors.into_iter().unzip();
        Linked {
            functions,
            calls,
            handles,
            dtors,
        }
    }

    /// C の関数の表の順の呼び出し。
    pub fn calls(&self) -> &[CCall] {
        &self.calls
    }

    /// ハンドルの型の表の順の dtor。
    pub fn dtors(&self) -> &[Option<Dtor>] {
        &self.dtors
    }

    /// つないだときの宣言がプログラムの表と同じか。ハンドルの型の番号は表の位置なので、`load` が番号を付け直す前の
    /// 表と比べる。読み込む側が、呼ぶ前に確かめるために使う。
    pub fn fits(&self, functions: &[CFunction], handles: &[CHandleType]) -> bool {
        self.functions == functions && self.handles == handles
    }
}
