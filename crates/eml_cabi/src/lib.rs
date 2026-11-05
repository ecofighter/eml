//! C の関数を呼ぶコードのポインタ。`eml_native` が作り、`eml_vm` が使う。
//! `Ctx` に関わらないので、`eml_rt` に依存せず、`eml_extern` だけに依存する葉の crate にする。

use eml_extern::{CFunction, CHandleType, CSignature};

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
/// 呼ばないので安全である。呼び出しごとにつないだときの署名を持ち、読み込む側が `fits` でプログラムの表と比べる。
/// 別のプログラムのためにつないだ `Linked` を渡しても、署名の合わない橋渡しを呼ばないためである。
#[derive(Clone, Debug, Default)]
pub struct Linked {
    calls: Vec<CCall>,
    signatures: Vec<CSignature>,
    dtors: Vec<Option<Dtor>>,
}

impl Linked {
    /// `calls` は C の関数の表の順の (署名, 呼び出し)、`dtors` はハンドルの型の表の順の dtor である。
    ///
    /// # Safety
    /// `calls` のそれぞれの `bridge` は、組にした署名の slot を読む橋渡しの関数で、`callee` はその署名の C の
    /// 関数である。`dtors` のそれぞれは `void f(void*)` として呼べる関数である。どのポインタも、プロセスが終わるまで
    /// 有効なコードを指す (docs/spec/vm.md の「橋渡しの関数」、docs/spec/vm.md の「ライブラリと記号」)。並びと署名が
    /// 読み込むプログラムの表に合うことは、読み込む側が `fits` で確かめる。
    pub unsafe fn new(calls: Vec<(CSignature, CCall)>, dtors: Vec<Option<Dtor>>) -> Linked {
        let (signatures, calls) = calls.into_iter().unzip();
        Linked {
            calls,
            signatures,
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

    /// プログラムの表と合うか。長さ、呼び出しごとの署名 (`CFunction::c_signature`)、ハンドルの型ごとの dtor の
    /// 有無を比べる。読み込む側が、呼ぶ前に確かめるために使う。
    pub fn fits(&self, functions: &[CFunction], handles: &[CHandleType]) -> bool {
        self.calls.len() == functions.len()
            && self.dtors.len() == handles.len()
            && self
                .signatures
                .iter()
                .zip(functions)
                .all(|(signature, function)| *signature == function.c_signature())
            && self
                .dtors
                .iter()
                .zip(handles)
                .all(|(dtor, handle)| dtor.is_some() == handle.dtor.is_some())
    }
}
