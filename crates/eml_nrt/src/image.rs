//! ネイティブの実行系がプログラムについて読む事実 (docs/spec/native.md の「`Image`」)。
#![allow(unsafe_code)]

use eml_rt::MaskItem;

use crate::Thunk;
use crate::run::WORKER_STACK;

/// JITのホストが持つUTF-8の文字列。
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Str {
    pub ptr: *const u8,
    pub len: usize,
}

impl Str {
    pub fn new(s: &str) -> Str {
        Str {
            ptr: s.as_ptr(),
            len: s.len(),
        }
    }

    /// # Safety
    /// `ptr`と`len`は、`'a`の間生きている正しいUTF-8を指す。
    pub unsafe fn as_str<'a>(&self) -> &'a str {
        // SAFETY: 呼ぶ側の約束
        unsafe { std::str::from_utf8_unchecked(raw_slice(self.ptr, self.len)) }
    }
}

/// JITのホストが持つ配列。
#[repr(C)]
#[derive(Debug)]
pub struct Slice<T> {
    pub ptr: *const T,
    pub len: usize,
}

// deriveでは`T: Copy`を求めてしまう
impl<T> Clone for Slice<T> {
    fn clone(&self) -> Slice<T> {
        *self
    }
}

impl<T> Copy for Slice<T> {}

impl<T> Slice<T> {
    pub fn new(items: &[T]) -> Slice<T> {
        Slice {
            ptr: items.as_ptr(),
            len: items.len(),
        }
    }

    /// # Safety
    /// `ptr`と`len`は、`'a`の間生きている初期化した要素を指す。
    pub unsafe fn as_slice<'a>(&self) -> &'a [T] {
        // SAFETY: 呼ぶ側の約束
        unsafe { raw_slice(self.ptr, self.len) }
    }
}

/// `from_raw_parts`は要素がなくてもnullでないポインタを求めるが、空の`Slice`はnullを持ちうる。
unsafe fn raw_slice<'a, T>(ptr: *const T, len: usize) -> &'a [T] {
    if len == 0 {
        &[]
    } else {
        // SAFETY: 呼ぶ側の約束
        unsafe { std::slice::from_raw_parts(ptr, len) }
    }
}

/// 位置のない地点の`SiteRow.file`。
pub const NO_FILE: u32 = u32::MAX;

/// faultの地点のソースの位置。`Image.files`の添字、行、列である。
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SiteRow {
    pub file: u32,
    pub line: u32,
    pub column: u32,
}

/// 関数1つ。名前、コードの範囲`[code, code + size)`、その関数のfaultの地点と、関数の静的なクロージャのslot (なければ
/// null) である。
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FnRow {
    pub name: Str,
    pub code: *const u8,
    pub size: u32,
    pub sites: Slice<SiteRow>,
    pub value: *mut u64,
}

pub const DATUM_STRING: u32 = 0;
pub const DATUM_BIG_INT: u32 = 1;
pub const DATUM_FLOAT: u32 = 2;
pub const DATUM_FN: u32 = 3;
pub const DATUM_EMPTY_MUT_ARRAY: u32 = 4;
pub const DATUM_EFFECT: u32 = 5;
pub const DATUM_MASK: u32 = 6;

/// 読み込み時の値1つ。種類、中身 (大きい`int`と`float`とエフェクトの番号は`bits`、文字列は`text`、静的なクロージャは
/// `bits`の引数の数と`code`のエントリのコード、マスクは`items`) と、runが埋めるデータのslotである。runは、ヒープの
/// 種類のslotには物体の語を、エフェクトのslotには番号そのものを、マスクのslotには項目を`MaskItem`として書く。マスクの
/// slotは`mask_bytes(items.len)`バイトで、ほかのslotは1語である。
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DatumRow {
    pub kind: u32,
    pub bits: u64,
    pub text: Str,
    pub code: *const u8,
    pub slot: *mut u64,
    pub items: Slice<MaskRow>,
}

/// マスクの項目1つ。`eml_rt::MaskItem`の欄で、エフェクトはマシンの番号である。
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaskRow {
    pub effect: u16,
    pub keep: u32,
    pub skip: u32,
}

// マスクのデータの物は8にそろうので、runはそこへ`MaskItem`を書ける
const _: () = assert!(align_of::<MaskItem>() <= 8);

/// 項目を`n`個持つマスクのデータの物のバイト数。
pub fn mask_bytes(n: usize) -> usize {
    n * size_of::<MaskItem>()
}

/// PAPのアダプタ1つ。`rest`個の引数を待つPAPのエントリのコードである。
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PapRow {
    pub rest: u32,
    pub code: *const u8,
}

/// 関数の値1つのエントリのコード。クロージャのエントリ`c<k>:<f>`かPAPのアダプタと、その署名の語 (`sig_word`) である。
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CodeRow {
    pub code: *const u8,
    pub sig: u64,
}

/// `eml_rt_audit_code`の`expect`の語のうち、値自身のタグが待つ引数の数の一様な署名を求めるもの。
pub const CODE_OF_TAG: i64 = -1;

/// エントリの署名`tail (ctx, clo, params..) -> ret`の語。`params`の数に、`F64`の結果ならビット16を、`F64`の引数`i`ならビット
/// `17 + i`を足したものである。引数`n`個の一様な署名は語`n`になる。添字47以降の引数が`F64`なら`None`。
pub fn sig_word(params: &[crate::Kind], ret: crate::Kind) -> Option<u64> {
    let mut word = u64::try_from(params.len())
        .ok()
        .filter(|&n| n < u64::from(u16::MAX))?;
    if ret == crate::Kind::F64 {
        word |= 1 << 16;
    }
    for (i, &kind) in params.iter().enumerate() {
        if kind == crate::Kind::F64 {
            if i > 46 {
                return None;
            }
            word |= 1 << (17 + i);
        }
    }
    Some(word)
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Image {
    pub functions: Slice<FnRow>,
    pub files: Slice<Str>,
    pub data: Slice<DatumRow>,
    pub paps: Slice<PapRow>,
    /// このimageの関数の値が持ちうるエントリのコードすべて。検査モードのマシンが埋め、速いモードのマシンは空のままにする。
    pub entries: Slice<CodeRow>,
    /// 組み込みの`Panic`と`IO`のマシンの番号。プログラムのエフェクトの表になければ`NO_EFFECT`である。
    pub panic_effect: u32,
    pub io_effect: u32,
}

/// `Image.panic_effect`と`Image.io_effect`で、エフェクトがプログラムにないことを表す値。
pub const NO_EFFECT: u32 = u32::MAX;

// SAFETY: 安全なAPIはどれも`Image`のポインタの先を読み書きしない。読み書きするのは`unsafe fn`の`run`だけで、呼ぶ側が
// 待っている間に、workerのスレッドから読む。workerが書くのは`DatumRow.slot`を通してだけで、書くのはrunごとに、その
// runの最初のエントリの前である。1つの`Image`のrunは重ならない (`run`の約束で、JITのホストの`run`と`call`は
// `&mut self`を取る) ので、書く間にslotを読むコードはない。書く大きさは、ヒープの種類とエフェクトでは1語、マスクでは
// `mask_bytes(items.len)`バイトである。ほかはどれも読むだけである。コードはslotとその指す定数を同期なしに読むので、
// runのコードを動かすどのworkerのヒープも、コードを動かす前にそのrunの塊を付ける
unsafe impl Sync for Image {}

/// runがコードに入る所。`eml_enter`の引数と、エントリの結果が数える語かどうかである。
#[derive(Debug, Clone, Copy)]
pub struct Start {
    pub thunk: Thunk,
    pub entry: *const u8,
    pub argv: *mut u64,
    pub result_rc: bool,
}

// SAFETY: 安全なAPIはどれも`Start`のポインタの先を読み書きしない。ポインタを使うのは`unsafe fn`の`run`だけで、呼ぶ側が
// 待っている間のworkerのスレッドから使う
unsafe impl Sync for Start {}

/// 成功したrunの終わりに、ヒープに残った物体を数えるか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Census {
    Skip,
    Check,
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub census: Census,
    /// workerのスタックに予約するバイト数。スタックの上限はこの値から決めるので、どの値も安全の約束を破らない。
    /// 小さすぎる値は深さの誤りかガードページでの中止になり、大きすぎる値はworkerを作れない内部の誤りになる
    /// (docs/spec/lir.mdの「エンジンの約束」)。
    pub stack: usize,
    /// 呼んだスレッドでSIGINTとSIGTERMを塞いだまま`body`を動かすフック。`body`を1回呼ぶ。既定は何も塞がない。
    /// `run`はこの中でworker 0の門を開けてjoinするので、worker 0は呼んだスレッドが塞ぐまでコードを動かさない
    /// (docs/spec/native.md の「実行の始めと終わり」)。塞ぐことは呼ぶ側に任せ、`eml_nrt`はOSのシグナルの関数を
    /// 呼ばない。フックが`body`を呼ばなければrunは内部の誤りで終わり、2回目からの呼び出しは何もしない。これは安全の
    /// 約束ではない。
    pub masked: fn(&mut dyn FnMut()),
}

impl Default for Options {
    fn default() -> Options {
        Options {
            census: Census::Skip,
            stack: WORKER_STACK,
            masked: unmasked,
        }
    }
}

fn unmasked(body: &mut dyn FnMut()) {
    body();
}

pub const ACTION_CONTINUE: i64 = 0;
pub const ACTION_UNWIND: i64 = 1;
pub const ACTION_RAISE: i64 = 2;

/// `eml_nrt_failed`の`purity`の語。失敗した行の純粋さで、値は`eml_extern::Purity`の宣言の順である
/// (docs/spec/native.md の「externの失敗」)。
pub const PURITY_PURE: i64 = 0;
pub const PURITY_PANICS: i64 = 1;
pub const PURITY_EFFECTFUL: i64 = 2;

/// `eml_nrt_failed`の`site`で、関数の入口を表す値。入口は地点の表に行を持たない。入口で失敗を上げるときは、これと
/// `PURITY_EFFECTFUL`を渡す (docs/spec/native.md の「安全点」)。
pub const ENTRY_SITE: i64 = -1;

/// `eml_nrt_failed`の結果の類。`RAISE_PANIC`では`out`に`[節, evidence, 文言]`を、`RAISE_IO`では
/// `[節, evidence, 種類, 文言]`を書いてある。`eml$raise`はその節を呼ぶ (docs/spec/native.mdの「externの失敗」)。
pub const RAISE_PANIC: i64 = 0;
pub const RAISE_IO: i64 = 1;

/// `eml_rt_fault_arith`の`kind`の語。算術の失敗の種類で、`eml_rt::ArithFault`のうち生成したコードが出すものである
/// (docs/spec/native.md の「ヒープのエントリ」)。
pub const ARITH_DIVISION_BY_ZERO: i64 = 0;
pub const ARITH_INTEGER_OVERFLOW: i64 = 1;

#[cfg(test)]
mod tests {
    use super::sig_word;
    use crate::Kind::{F64, I64};

    #[test]
    fn the_signature_word_tells_signatures_apart() {
        // 引数`n`個の一様な署名は語`n`である
        assert_eq!(sig_word(&[], I64), Some(0));
        assert_eq!(sig_word(&[I64, I64, I64], I64), Some(3));
        // `F64`の結果と、`F64`の引数のそれぞれが、別のビットを立てる
        assert_eq!(sig_word(&[I64, I64], F64), Some(2 | 1 << 16));
        assert_eq!(sig_word(&[F64, I64], I64), Some(2 | 1 << 17));
        assert_eq!(sig_word(&[I64, F64], I64), Some(2 | 1 << 18));
        let mut words = [
            sig_word(&[I64], I64),
            sig_word(&[I64], F64),
            sig_word(&[F64], I64),
            sig_word(&[I64, I64], I64),
        ];
        words.sort_unstable();
        assert!(words.windows(2).all(|pair| pair[0] != pair[1]));
        // 添字46までの`F64`の引数は語に収まり、47からは収まらない
        let mut params = vec![I64; 47];
        params[46] = F64;
        assert_eq!(sig_word(&params, I64), Some(47 | 1 << 63));
        params.push(I64);
        params[47] = F64;
        assert_eq!(sig_word(&params, I64), None);
    }
}
