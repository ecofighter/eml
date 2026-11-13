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

    /// `callee`を`argv[0..m]`の`m`個の引数に部分適用したPAPを返す。PAPは引数を1つだけ持つので、`m`個のPAPの鎖を作り、
    /// 最後のものを返す。`i`番目 (1から) のPAPは、まだ待つ引数の数`n - i`をタグに持ち、走査する語に
    /// `[eml$pap$(n-i)のコード, 1つ前, a_i]`を、`lin`に`callee`の`lin`を持つ。1つ前は、最初は`callee`で、その後は前に
    /// 作ったPAPである。`n`は`callee`のタグである。`callee`が引数を`m`個以下しか待たないか、鎖のどれかのアダプタがなければ、
    /// 内部の誤りでrunを終える。
    ///
    /// # Safety
    /// - `eml_nrt_safepoint`と同じ
    /// - `callee`はこのヒープか付けた塊の生きている関数の値で、`argv`は`m`個の語を読める。`m`は1以上である
    /// - 呼ぶ側は`callee`と各引数の参照を1つずつ持ち、それらはPAPの鎖へ移る。呼ぶ側はその後、これらの語を手放さない
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
        let n = usize::from(meta.tag());
        if n <= m {
            return Err(not_a_function(ctx));
        }
        // 鎖の残りの数は`n - m..n`である。作り始める前にどのアダプタもあることを確かめるので、断るときに作りかけの鎖は
        // ない
        // SAFETY: 呼ぶ側の約束
        let adapter = |ctx: &mut Ctx<Fast>, rest: usize| unsafe { worker::of(ctx) }.pap(rest as u32);
        if (n - m..n).any(|rest| adapter(ctx, rest).is_none()) {
            return Err(not_a_function(ctx));
        }
        let mut previous = callee;
        for i in 1..=m {
            let rest = n - i;
            let code = adapter(ctx, rest).expect("every adapter of the chain is there");
            // SAFETY: 呼ぶ側の約束。`argv[i - 1]`は読め、`previous`は`callee`か前に作ったPAPで、どちらの参照も新しい
            // PAPへ移る。新しい物体を作って書くだけで、`Ctx`の持つ語には触れない
            let pap = unsafe {
                build(ctx.heap_mut(), rest, meta.lin(), code, previous, argv.add(i - 1).read())
            };
            previous = pap.map_err(|e| e.park(ctx))?;
        }
        Ok(previous)
    }
}

/// 引数を1つ持つPAPの物体を作り、走査する語をすべて書く。
///
/// # Safety
/// `rest`は16ビットに収まる。`callee`と`arg`は生きている語で、その参照はPAPの欄へ移る。
unsafe fn build(
    heap: &mut Heap<Fast>,
    rest: usize,
    lin: bool,
    adapter: *const u8,
    callee: Word,
    arg: Word,
) -> Result<Word, HeapError> {
    let first = layout::data_first(PAP_SCANNED, false);
    let pap = heap.alloc_data(rest as u16, PAP_SCANNED, 0, lin);
    // SAFETY: 新しい物体の走査する語を、ほかのヒープの操作の前にすべて書く。呼ぶ側の約束により、`callee`と`arg`は
    // 生きていて、その参照はPAPの欄へ移る
    unsafe {
        heap.set_field(pap, first, code_word(adapter))?;
        heap.set_field(pap, first + 1, callee)?;
        heap.set_field(pap, first + 2, arg)?;
    }
    Ok(pap)
}

/// PAPの走査する語の数。アダプタのコード、呼ぶ値、引数1つである。
const PAP_SCANNED: usize = 3;
