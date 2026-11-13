//! 関数の値のエントリ (docs/spec/native.md の「関数の値」)。
#![allow(unsafe_code)]

use eml_rt::{Ctx, Fast, Heap, HeapError, Kind, Word, layout};
use eml_sched::error::RunFault;

use crate::Raise;
use crate::worker;

/// コードのアドレスを、クロージャの0番の走査する語にする即値。生成したコードは1ビット右へずらしてアドレスに戻す。
pub(crate) fn code_word(code: *const u8) -> Word {
    Word::int(code.addr() as i64).expect("a code address does not fit an immediate")
}

const NOT_A_FUNCTION: RunFault = RunFault::Internal("applying a value that is not a function");

crate::entries! {
    pub(crate) const CLOSURES;

    /// `callee`を`argv[0..m]`の`m`個の引数に部分適用したPAPを返す。PAPの物体は、まだ待つ引数の数`r`をタグに持ち、
    /// 走査する語に`[eml$pap$m$rのコード, callee, a_1..a_m]`を、`lin`に`callee`の`lin`を持つ。`callee`が引数を
    /// `m`個以下しか待たないか、`(m, r)`のアダプタがなければ、内部の誤りでrunを終える。
    ///
    /// # Safety
    /// - `eml_nrt_safepoint`と同じ
    /// - `callee`はこのヒープか付けた塊の生きている関数の値で、`argv`は`m`個の語を読める。`m`は1以上である
    /// - 呼ぶ側は`callee`と各引数の参照を1つずつ持ち、それらはPAPへ移る。呼ぶ側はその後、これらの語を手放さない
    pub shim fn eml_nrt_pap(ctx, callee: obj, argv: ptr, m: i64) -> obj {
        debug_assert!(m >= 1, "a partial application has no argument");
        let m = m as usize;
        let not_a_function = |ctx: &mut Ctx<Fast>| {
            // SAFETY: 呼ぶ側の約束
            unsafe { worker::record(ctx, NOT_A_FUNCTION, None) }
        };
        if callee.is_imm() {
            return Err(not_a_function(ctx));
        }
        // SAFETY: 呼ぶ側の約束
        let meta = unsafe { ctx.heap().meta(callee) }.map_err(|e| e.park(ctx))?;
        // ほかの種類のタグは引数の数ではない
        if meta.kind() != Kind::Data {
            return Err(not_a_function(ctx));
        }
        let Some(rest) = usize::from(meta.tag()).checked_sub(m).filter(|&r| r > 0) else {
            return Err(not_a_function(ctx));
        };
        // SAFETY: 上と同じ
        let adapter = match (u32::try_from(m), u32::try_from(rest)) {
            (Ok(args), Ok(r)) => unsafe { worker::of(ctx) }.pap(args, r),
            _ => None,
        };
        let Some(adapter) = adapter else {
            return Err(not_a_function(ctx));
        };
        // SAFETY: 呼ぶ側の約束。新しい物体を作って書くだけで、`Ctx`の持つ語には触れない
        let pap = unsafe { build(ctx.heap_mut(), rest, meta.lin(), adapter, callee, argv, m) };
        pap.map_err(|e| e.park(ctx))
    }
}

/// PAPの物体を作り、走査する語をすべて書く。
///
/// # Safety
/// `rest`は16ビットに収まる。`callee`と`argv[0..m]`は`eml_nrt_pap`の約束を満たし、その参照はPAPの欄へ移る。
unsafe fn build(
    heap: &mut Heap<Fast>,
    rest: usize,
    lin: bool,
    adapter: *const u8,
    callee: Word,
    argv: *const Word,
    m: usize,
) -> Result<Word, HeapError> {
    let scanned = 2 + m;
    let first = layout::data_first(scanned, false);
    let pap = heap.alloc_data(rest as u16, scanned, 0, lin);
    // SAFETY: 新しい物体の走査する語を、ほかのヒープの操作の前にすべて書く。呼ぶ側の約束により、`callee`と引数は
    // 生きていて、その参照はPAPの欄へ移る
    unsafe {
        heap.set_field(pap, first, code_word(adapter))?;
        heap.set_field(pap, first + 1, callee)?;
        for i in 0..m {
            heap.set_field(pap, first + 2 + i, argv.add(i).read())?;
        }
    }
    Ok(pap)
}
