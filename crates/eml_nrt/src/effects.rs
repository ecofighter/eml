//! 生成したコードが呼ぶエフェクトのエントリ (docs/spec/native.mdの「エフェクト」)。`Ctx`の基本操作1つずつの薄い包みで、
//! 記号は`eml_rt_`に`Prim::name()`を続けたものである。workerの構造体には触れず、失敗はすべて致命的である。
#![allow(unsafe_code)]

use eml_rt::{HeapError, MaskItem, Word};

/// エフェクトの番号。65535は番号ではない (`Ctx::ev_new`の前提で、`+ 1`が0の印に回り込むため)。
fn effect_number(effect: i64) -> Result<u16, HeapError> {
    u16::try_from(effect)
        .ok()
        .filter(|effect| *effect != u16::MAX)
        .ok_or(HeapError::BadEvidence)
}

fn operation_index(op: i64) -> Result<usize, HeapError> {
    usize::try_from(op).map_err(|_| HeapError::BadEvidence)
}

fn root_index(index: i64) -> Result<u32, HeapError> {
    u32::try_from(index).map_err(|_| HeapError::RootMisuse)
}

crate::entries! {
    pub(crate) const EFFECTS = "eml_rt_";

    /// 今の`evv`を`dup`して返す。
    ///
    /// # Safety
    /// `Ctx::evv_get`の前提を守っている (docs/spec/object-model.mdの「基本操作の契約」)。
    pub(crate) shim fn evv_get(ctx) -> tobj {
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.evv_get() }
    }

    /// `evv`を`l`に替え、元の`evv`を手放す。`l`の参照は`Ctx`に移る。
    ///
    /// # Safety
    /// `Ctx::evv_set`の前提を守っている (docs/spec/object-model.mdの「基本操作の契約」)。
    pub(crate) shim fn evv_set(ctx, l: tobj) {
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.evv_set(l) }
    }

    /// リスト`l`の先頭にevidence `e`を足した`evv`を返す。`l`と`e`の参照は消費する。
    ///
    /// # Safety
    /// `Ctx::evv_push`の前提を守っている (docs/spec/object-model.mdの「基本操作の契約」)。
    pub(crate) shim fn evv_push(ctx, l: tobj, e: obj) -> tobj {
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.evv_push(l, e) }
    }

    /// リスト`w`から`items`の項目に当たるevidenceを飛ばした`evv`を返す。
    ///
    /// # Safety
    /// `Ctx::evv_mask`の前提を守っている (docs/spec/object-model.mdの「基本操作の契約」)。加えて、`items`は
    /// 生きている`MaskItem`を`n`個並べた領域の先頭で、`n`個の項目はこの呼び出しの間変わらない。
    pub(crate) shim fn evv_mask(ctx, items: ptr, n: i64, w: tobj) -> tobj {
        let n = usize::try_from(n).map_err(|_| HeapError::BadEvidence)?;
        // SAFETY: 呼ぶ側の約束。生成したコードは`MaskItem`と同じ並びの行を、8に揃えた領域に置く
        let items = unsafe { std::slice::from_raw_parts(items.cast::<MaskItem>(), n) };
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.evv_mask(items, w) }
    }

    /// エフェクト`effect`の新しいevidenceを作る。`state`と`outer`の参照はevidenceに移る。
    ///
    /// # Safety
    /// `Ctx::ev_new`の前提を守っている (docs/spec/object-model.mdの「基本操作の契約」)。`effect`が65535以上の番号なら
    /// 参照を手放さずに致命的な誤りで終わる。
    pub(crate) shim fn ev_new(ctx, effect: i64, n_ops: i64, state: tobj, outer: tobj) -> obj {
        let effect = effect_number(effect)?;
        let n_ops = operation_index(n_ops)?;
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.ev_new(effect, n_ops, state, outer) }
    }

    /// evidence `ev`の表の欄`op`に節の本体`c`を入れる。`c`は消費し、`ev`は借りる。
    ///
    /// # Safety
    /// `Ctx::ev_set_clause`の前提を守っている (docs/spec/object-model.mdの「基本操作の契約」)。
    pub(crate) shim fn ev_set_clause(ctx, ev: obj, op: i64, c: tobj) {
        let op = operation_index(op)?;
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.ev_set_clause(ev, op, c) }
    }

    /// `ev`と同じ表を指し、状態を`state`に、外側の`evv`を今の`evv`にした新しいevidenceを返す。`ev`は借りる。
    ///
    /// # Safety
    /// `Ctx::ev_renew`の前提を守っている (docs/spec/object-model.mdの「基本操作の契約」)。
    pub(crate) shim fn ev_renew(ctx, ev: obj, state: tobj) -> obj {
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.ev_renew(ev, state) }
    }

    /// 今の`evv`をたどり、エフェクト`effect`の最初のevidenceを`dup`して返す。
    ///
    /// # Safety
    /// `Ctx::ev_lookup`の前提を守っている (docs/spec/object-model.mdの「基本操作の契約」)。`effect`が65535以上の
    /// 番号なら致命的な誤りで終わる。
    pub(crate) shim fn ev_lookup(ctx, effect: i64) -> obj {
        let effect = effect_number(effect)?;
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.ev_lookup(effect) }
    }

    /// evidence `ev`の表の欄`op`を`dup`して返す。`ev`は借りる。
    ///
    /// # Safety
    /// `Ctx::ev_clause`の前提を守っている (docs/spec/object-model.mdの「基本操作の契約」)。
    pub(crate) shim fn ev_clause(ctx, ev: obj, op: i64) -> tobj {
        let op = operation_index(op)?;
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.ev_clause(ev, op) }
    }

    /// evidence `ev`の外側の`evv`を`dup`して返す。`ev`は借りる。
    ///
    /// # Safety
    /// `Ctx::ev_evv`の前提を守っている (docs/spec/object-model.mdの「基本操作の契約」)。
    pub(crate) shim fn ev_evv(ctx, ev: obj) -> tobj {
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.ev_evv(ev) }
    }

    /// evidence `ev`の外側の`evv`を返し、欄を`()`にする。外側の参照は呼ぶ側に移る。
    ///
    /// # Safety
    /// `Ctx::ev_take_evv`の前提を守っている (docs/spec/object-model.mdの「基本操作の契約」)。
    pub(crate) shim fn ev_take_evv(ctx, ev: obj) -> tobj {
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.ev_take_evv(ev) }
    }

    /// evidence `ev`の状態を返し、欄を`()`にする。状態の参照は呼ぶ側に移る。`ev`は借りる。
    ///
    /// # Safety
    /// `Ctx::ev_take_state`の前提を守っている (docs/spec/object-model.mdの「基本操作の契約」)。
    pub(crate) shim fn ev_take_state(ctx, ev: obj) -> tobj {
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.ev_take_state(ev) }
    }

    /// evidence `ev`の状態を`s`にし、元の状態を手放す。`s`は消費し、`ev`は借りる。
    ///
    /// # Safety
    /// `Ctx::ev_set_state`の前提を守っている (docs/spec/object-model.mdの「基本操作の契約」)。
    pub(crate) shim fn ev_set_state(ctx, ev: obj, s: tobj) {
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.ev_set_state(ev, s) }
    }

    /// yieldを始める。`ev`は宛先のevidence、`clause`は節の本体、`k`は最初のκで、どれも消費する。
    ///
    /// # Safety
    /// `Ctx::yield_to`の前提を守っている (docs/spec/object-model.mdの「基本操作の契約」)。
    pub(crate) shim fn yield_to(ctx, ev: obj, clause: tobj, k: tobj) {
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.yield_to(ev, clause, k) }
    }

    /// 中断を始める。`ev`は宛先のevidence、`clause`は節の本体で、どちらも消費する。κの欄は`()`にする。
    ///
    /// # Safety
    /// `Ctx::yield_abort`の前提を守っている (docs/spec/object-model.mdの「基本操作の契約」)。
    pub(crate) shim fn yield_abort(ctx, ev: obj, clause: tobj) {
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.yield_abort(ev, clause) }
    }

    /// κを取り出し、欄を`()`にする。ヒープに触れないので、前提はない。
    ///
    /// # Safety
    /// `ctx`はこのworkerの生きている`Ctx`である。
    pub(crate) shim fn yield_cont_take(ctx) -> tobj {
        ctx.yield_cont_take()
    }

    /// κの欄に`k`を置く。`k`の参照は`Ctx`に移る。
    ///
    /// # Safety
    /// `Ctx::yield_cont_set`の前提を守っている (docs/spec/object-model.mdの「基本操作の契約」)。
    pub(crate) shim fn yield_cont_set(ctx, k: tobj) {
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.yield_cont_set(k) }
    }

    /// yieldを終える。節の本体を返し、宛先の参照を手放し、yieldの状態をなしに戻す。
    ///
    /// # Safety
    /// `Ctx::yield_finish`の前提を守っている (docs/spec/object-model.mdの「基本操作の契約」)。
    pub(crate) shim fn yield_finish(ctx) -> tobj {
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.yield_finish() }
    }

    /// ダミーの値を返す。ヒープに触れず、不死の物体を読むだけである。
    ///
    /// # Safety
    /// `ctx`はこのworkerの生きている`Ctx`である。
    pub(crate) shim fn dummy(ctx) -> obj {
        Ok::<Word, HeapError>(ctx.heap().dummy())
    }

    /// 根`index`の語を`dup`して返す。
    ///
    /// # Safety
    /// `Ctx::root_get`の前提を守っている (docs/spec/object-model.mdの「基本操作の契約」)。空の根や表の外の根は
    /// 致命的な誤りで終わる。
    pub(crate) shim fn root_get(ctx, index: i64) -> tobj {
        let index = root_index(index)?;
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.root_get(index) }
    }

    /// 根`index`に`w`を置く。`w`の参照は根に移る。
    ///
    /// # Safety
    /// `Ctx::root_set`の前提を守っている (docs/spec/object-model.mdの「基本操作の契約」)。置いてある根への書き込みは
    /// 致命的な誤りで終わる。
    pub(crate) shim fn root_set(ctx, index: i64, w: tobj) {
        let index = root_index(index)?;
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.root_set(index, w) }
    }
}
