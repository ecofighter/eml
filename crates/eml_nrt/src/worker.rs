//! `ctx.runtime`の後ろのworkerの構造体と、そこに失敗を記録する関数 (docs/spec/native.md の「実行の始めと終わり」)。
#![allow(unsafe_code)]

use eml_rt::{Ctx, Fast, Fault, Word};
use eml_sched::error::{RunFault, RuntimeError, SourceLocation};

use crate::enter::{FATAL_RECORDED, Fatal};
use crate::image::{FnRow, Image, NO_FILE};

/// コードの中の場所が分からない誤りの、関数の名前。
pub(crate) const NATIVE_CODE: &str = "<native code>";

/// emlのコードが動く間、runが`Ctx`の外に持つもの。`run`はエントリの間これを`ctx.runtime`の後ろに置き、エントリが
/// 戻った後に読む。
pub struct Worker {
    image: *const Image,
    /// `image.functions`の添字をコードのアドレスの順に並べたもの。アドレスから関数を探す冷たい道が使う。
    by_code: Vec<usize>,
    /// `image.paps`の添字を`rest`の順に並べたもの。部分適用がアダプタを探す。
    by_pap: Vec<usize>,
    pub(crate) exit: Option<i64>,
    /// 置いたpanicのメッセージの語。`Some(w)`なら、`w`は生きている文字列で、workerの構造体がその参照を1つ持つ。
    /// 書くのは`park_panic`だけで、取り出した者がその参照を持つ。
    pub(crate) panic: Option<Word>,
    pub(crate) error: Option<RuntimeError>,
}

/// ネイティブのコードの中の場所。関数のコードのアドレスと、その関数の地点の1つである。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct At {
    pub function: usize,
    pub site: Option<u32>,
}

impl Worker {
    /// # Safety
    /// `image`の表は、workerが生きている間生きていて変わらない。
    pub(crate) unsafe fn new(image: &Image) -> Worker {
        // SAFETY: 呼ぶ側の約束
        let functions = unsafe { image.functions.as_slice() };
        let mut by_code: Vec<usize> = (0..functions.len()).collect();
        by_code.sort_by_key(|&i| functions[i].code.addr());
        // SAFETY: 呼ぶ側の約束
        let paps = unsafe { image.paps.as_slice() };
        let mut by_pap: Vec<usize> = (0..paps.len()).collect();
        by_pap.sort_by_key(|&i| paps[i].rest);
        Worker {
            image,
            by_code,
            by_pap,
            exit: None,
            panic: None,
            error: None,
        }
    }

    fn function(&self, address: usize) -> Option<&FnRow> {
        // SAFETY: `new`の約束
        let functions = unsafe { (*self.image).functions.as_slice() };
        let after = self
            .by_code
            .partition_point(|&i| functions[i].code.addr() <= address);
        let row = &functions[*self.by_code.get(after.checked_sub(1)?)?];
        (address - row.code.addr() < row.size as usize).then_some(row)
    }

    /// `rest`個の引数を待つPAPのアダプタのコード。
    pub(crate) fn pap(&self, rest: u32) -> Option<*const u8> {
        // SAFETY: `new`の約束
        let paps = unsafe { (*self.image).paps.as_slice() };
        let i = self
            .by_pap
            .binary_search_by_key(&rest, |&i| paps[i].rest)
            .ok()?;
        Some(paps[self.by_pap[i]].code)
    }

    /// `at`での`fault`の誤り。関数の名前は、そのアドレスをコードに含む関数のものである。
    fn error_at(&self, fault: RunFault, at: Option<At>) -> RuntimeError {
        let Some(row) = at.and_then(|at| self.function(at.function)) else {
            return RuntimeError::Fault {
                fault,
                function: NATIVE_CODE.to_string(),
                at: None,
            };
        };
        // SAFETY: `new`の約束
        let (image, name, sites) =
            unsafe { (&*self.image, row.name.as_str(), row.sites.as_slice()) };
        let site = at
            .and_then(|at| at.site)
            .and_then(|i| sites.get(i as usize));
        let at = site.filter(|s| s.file != NO_FILE).map(|s| {
            // SAFETY: `new`の約束
            let files = unsafe { image.files.as_slice() };
            SourceLocation {
                // SAFETY: `new`の約束
                path: unsafe { files[s.file as usize].as_str() }.to_string(),
                line: s.line,
                column: s.column,
            }
        });
        RuntimeError::Fault {
            fault,
            function: name.to_string(),
            at,
        }
    }
}

/// `ctx.runtime`の後ろのworker。
///
/// # Safety
/// `ctx.runtime()`は生きている`Worker`を指す。それは`Worker::new`で作り、その約束がまだ成り立つもので、書き込める
/// ポインタを通して置いてある。返した借用が生きている間、ほかの誰もその`Worker`を読み書きしない。ほかの参照はなく、
/// 持ち主もそれに触れない。`run`は、`enter`の間だけworkerを置き、それをまたいで`ctx`とworkerへの参照を持たないことで
/// これを満たす。テストは、自分の`Worker`を同じように置いて満たす。`Ctx::set_runtime`は安全な関数で、ほかのコードも
/// 任意のポインタを置けるので、この前提は型では守れない。
pub(crate) unsafe fn of(ctx: &mut Ctx<Fast>) -> &mut Worker {
    let worker = ctx.runtime().cast::<Worker>();
    assert!(!worker.is_null(), "no native run holds this context");
    // SAFETY: 呼ぶ側の約束。この借用は`ctx`の借用と一緒に終わるので、2つが同時にあることはない
    unsafe { &mut *worker }
}

/// `at`での`fault`の誤りをworkerの構造体へ移し、駆動にそれを伝える符号を返す。誤りは`Err`が`shim`に届く前に移す。
/// 致命的な誤りの道は、デストラクタを持たないフレームしかまたがないためである (docs/spec/native.md の「`eml_fatal`」)。
///
/// # Safety
/// `of`と同じ。
pub unsafe fn record(ctx: &mut Ctx<Fast>, fault: RunFault, at: Option<At>) -> Fatal {
    // SAFETY: 呼ぶ側の約束
    let worker = unsafe { of(ctx) };
    let error = worker.error_at(fault, at);
    let previous = worker.error.replace(error);
    debug_assert!(previous.is_none(), "an error was already recorded");
    Fatal {
        code: FATAL_RECORDED,
        site: 0,
    }
}

/// メッセージの語をworkerが持つpanicを置く。語はworkerの構造体に、`Fault::Engine`は`ctx`に置く。だから
/// `ctx.native.fault`が0でないのは、ちょうど失敗を置いている間である。
///
/// # Safety
/// - `of`と同じ
/// - `message`は、この`ctx`のヒープか、それに付けた塊の生きている文字列である
/// - 呼ぶ側は`message`の参照を1つ持ち、その参照はworkerの構造体へ移る。呼ぶ側はその後、この語を使わず手放さない
/// - 失敗を置いていない。faultの語は0で、workerの構造体はメッセージを持たない。これを破ると前の語が漏れるが、
///   未定義の動作にはならない
pub unsafe fn park_panic(ctx: &mut Ctx<Fast>, message: Word) {
    // SAFETY: 呼ぶ側の約束
    let previous = unsafe { of(ctx) }.panic.replace(message);
    debug_assert!(previous.is_none(), "a panic was already parked");
    let previous = ctx.set_fault(Fault::Engine);
    debug_assert!(previous.is_none(), "a fault was already parked");
}
