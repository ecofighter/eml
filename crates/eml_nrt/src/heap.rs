//! 生成したコードが呼ぶヒープのエントリ (docs/spec/native.md の「ヒープのエントリ」)。どれも`eml_rt`のヒープの操作1つの包みで、
//! workerの構造体には触れない。失敗はすべて致命的である (docs/spec/native.md の「失敗の手順」)。
#![allow(unsafe_code)]

use eml_rt::{ArithFault, Fault, HeapError, Token, Word, layout};

use crate::enter::Fatal;
use crate::image::{ARITH_DIVISION_BY_ZERO, ARITH_INTEGER_OVERFLOW};

/// 生成したコードが渡す`tag`、`scanned`、`raw`の範囲を、debugのビルドで確かめる。`Data`の`meta`は、走査する語が
/// 数の語を要しない範囲でだけ作れる (docs/spec/object-model.md の「欄」)。
fn data_shape(tag: i64, scanned: i64, raw: i64) -> (u16, usize, usize) {
    debug_assert!(u16::try_from(tag).is_ok(), "a tag word is out of range");
    debug_assert!(scanned >= 0 && raw >= 0, "a size word is negative");
    (tag as u16, scanned as usize, raw as usize)
}

crate::entries! {
    pub(crate) const HEAP;

    /// `Data`の物体を作る。`scanned`個の走査する語は`()`で埋めてあり、`raw`個の生の語はまだ書いていない。
    ///
    /// # Safety
    /// `ctx`はこのworkerの生きている`Ctx`である。
    pub(crate) shim fn eml_rt_alloc_data(ctx, tag: i64, scanned: i64, raw: i64, lin: i64) -> obj {
        let (tag, scanned, raw) = data_shape(tag, scanned, raw);
        // SAFETY: 新しい物体は、作る関数が走査する語まで書き終える。`Ctx`の語には触れない
        let w = unsafe { ctx.heap_mut() }.alloc_data(tag, scanned, raw, lin != 0);
        Ok::<Word, HeapError>(w)
    }

    /// `Int`を語にする。63ビットに収まれば即値で、収まらなければ`BigInt`の物体である。
    ///
    /// # Safety
    /// `ctx`はこのworkerの生きている`Ctx`である。
    pub(crate) shim fn eml_rt_alloc_int(ctx, n: i64) -> tobj {
        // SAFETY: 上と同じ
        let w = unsafe { ctx.heap_mut() }.alloc_int(n);
        Ok::<Word, HeapError>(w)
    }

    /// `Float`の物体を作る。引数はそのビット列を持つ`f64`である。
    ///
    /// # Safety
    /// `ctx`はこのworkerの生きている`Ctx`である。
    pub(crate) shim fn eml_rt_alloc_float(ctx, x: f64) -> obj {
        // SAFETY: 上と同じ
        let w = unsafe { ctx.heap_mut() }.alloc_float(x.to_bits());
        Ok::<Word, HeapError>(w)
    }

    /// 参照を1つ増やす。生成したコードは、速い道で済まない物体 (共有の物体と、数が張り付いた物体) のために遅い道で呼ぶ。
    ///
    /// # Safety
    /// `w`は即値か、このヒープか付けた塊の生きている物体で、呼ぶ側が参照を持つか、生きている持ち主から借りている。物体は
    /// `lin`ビットを持たない。数が正なら、持ち主は今のworkerである。
    pub(crate) shim fn eml_rt_dup_slow(ctx, w: tobj) {
        // SAFETY: 呼ぶ側の約束。`Ctx`の語を手放さない
        unsafe { ctx.heap_mut().dup(w) }?;
        Ok::<(), HeapError>(())
    }

    /// 参照を1つ手放す。最後なら物体を解放し、子を手放す。
    ///
    /// # Safety
    /// `w`は即値か、このヒープか付けた塊の生きている物体で、呼ぶ側が手放す参照を1つ持つ。不死の物体なら、その参照は
    /// `acquire_immortal`か`dup`で作ったものである。共有の物体は`lin`ビットを持たない。数が正なら、持ち主は今のworkerである。
    pub(crate) shim fn eml_rt_decref(ctx, w: tobj) {
        // SAFETY: 呼ぶ側の約束。`Ctx`の語を手放さない
        unsafe { ctx.heap_mut().decref(w) }?;
        Ok::<(), HeapError>(())
    }

    /// 最後の参照か (数が1か-1) を0か1で返す。
    ///
    /// # Safety
    /// `w`は即値か、このヒープか付けた塊の生きている物体である。数が正なら、持ち主は今のworkerである。
    pub(crate) shim fn eml_rt_is_last(ctx, w: tobj) -> i64 {
        // SAFETY: 呼ぶ側の約束
        let last = unsafe { ctx.heap().is_last(w) }?;
        Ok::<i64, HeapError>(i64::from(last))
    }

    /// 最後の参照を持ち、子をもう処理した物体の箱を空ける。
    ///
    /// # Safety
    /// `w`はこのヒープの生きている物体で、呼ぶ側が最後の参照を持ち (数が1か-1)、子の参照は処理してある。種類は破棄処理を
    /// 持たない (`Foreign`でない)。数が正なら、持ち主は今のworkerである。
    pub(crate) shim fn eml_rt_free_block(ctx, w: obj) {
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.heap_mut().free_block(w) }?;
        Ok::<(), HeapError>(())
    }

    /// 一意な`Data`の箱を、フィールドに触れずにtokenにする。
    ///
    /// # Safety
    /// `w`はこのヒープの生きている`Data`で、呼ぶ側がただ1つの参照を持ち (数が1)、子の参照は処理してある。持ち主は今の
    /// workerである。
    pub(crate) shim fn eml_rt_into_token(ctx, w: obj) -> token {
        // SAFETY: 呼ぶ側の約束
        let token = unsafe { ctx.heap_mut().into_token(w) }?;
        Ok::<Word, HeapError>(token.into_word())
    }

    /// 同じコンストラクタの再利用。ヘッダに何も書かずに箱を物体として返す。
    ///
    /// # Safety
    /// `token`はこのヒープの空でないtokenで、まだ使っていない。`tag`、`scanned`、`lin`は箱の`meta`と同じである。
    pub(crate) shim fn eml_rt_reuse_in_place(ctx, token: token, tag: i64, scanned: i64, lin: i64) -> obj {
        let (tag, scanned, _) = data_shape(tag, scanned, 0);
        debug_assert!(!layout::data_extended(scanned, false), "a scanned count needs a count word");
        let meta = layout::data_meta(tag, scanned, lin != 0);
        // SAFETY: 呼ぶ側の約束。tokenは`eml_rt_into_token`が返した語か、空の語である
        let w = unsafe {
            let token = Token::from_word(token);
            ctx.heap_mut().reuse_in_place(token, meta)
        }?;
        Ok::<Word, HeapError>(w)
    }

    /// tokenの箱に`Data`のヘッダを書く。tokenが空なら確保する。フィールドは呼ぶ側が書く。
    ///
    /// # Safety
    /// `token`はこのヒープのtokenで、まだ使っていない。空でなければ、その箱は`scanned + raw`語の物体の級にある。
    /// 呼ぶ側は、次にヒープを操作する前に、走査する語をすべて書く。
    pub(crate) shim fn eml_rt_con_at(ctx, token: token, tag: i64, scanned: i64, raw: i64, lin: i64) -> obj {
        let (tag, scanned, raw) = data_shape(tag, scanned, raw);
        debug_assert!(!layout::data_extended(scanned, false), "a scanned count needs a count word");
        let meta = layout::data_meta(tag, scanned, lin != 0);
        // SAFETY: 呼ぶ側の約束。上と同じ
        let (w, _reused) = unsafe {
            let token = Token::from_word(token);
            ctx.heap_mut().con_at(token, scanned + raw, meta)
        }?;
        Ok::<Word, HeapError>(w)
    }

    /// 使わなかったtokenの箱を空ける。
    ///
    /// # Safety
    /// `token`はこのヒープのtokenで、まだ使っていない。
    pub(crate) shim fn eml_rt_drop_token(ctx, token: token) {
        // SAFETY: 呼ぶ側の約束
        unsafe {
            let token = Token::from_word(token);
            ctx.heap_mut().drop_token(token)
        }?;
        Ok::<(), HeapError>(())
    }

    /// 算術の失敗を`Ctx`に置く。種類は`ARITH_*`である。置いた失敗は、生成したコードが呼び出しの後で確かめる。
    pub(crate) shim fn eml_rt_fault_arith(ctx, kind: i64) {
        let fault = match kind {
            ARITH_DIVISION_BY_ZERO => ArithFault::DivisionByZero,
            ARITH_INTEGER_OVERFLOW => ArithFault::IntegerOverflow,
            _ => unreachable!("an arithmetic fault kind is unknown"),
        };
        let previous = ctx.set_fault(Fault::Arith(fault));
        debug_assert!(previous.is_none(), "a fault was already parked");
        Ok::<(), Fatal>(())
    }
}
