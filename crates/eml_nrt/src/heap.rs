//! 生成したコードが呼ぶヒープのエントリ (docs/spec/native.md の「ヒープのエントリ」)。`eml_rt_release`のほかは
//! `eml_rt`のヒープの操作1つの包みで、`eml_rt_release`はその操作の列である。どれもworkerの構造体には触れない。失敗は
//! すべて致命的である (docs/spec/native.md の「失敗の手順」)。
#![allow(unsafe_code)]

use eml_rt::{ArithFault, Fault, HeapError, Kind, Token, Word, layout};
use eml_sched::error::RunFault;

use crate::enter::{Fatal, Raise};
use crate::image::{ARITH_DIVISION_BY_ZERO, ARITH_INTEGER_OVERFLOW};
use crate::worker::{self, DATA_RELEASE, DATA_STRUCT_UNBOX, DATA_SWITCH, DATA_UNPACK};

/// 生成したコードが渡す`tag`、`scanned`、`raw`の範囲を、debugのビルドで確かめる。`Data`の`meta`は、走査する語が
/// 数の語を要しない範囲でだけ作れる (docs/spec/object-model.md の「欄」)。
fn data_shape(tag: i64, scanned: i64, raw: i64) -> (u16, usize, usize) {
    debug_assert!(u16::try_from(tag).is_ok(), "a tag word is out of range");
    debug_assert!(scanned >= 0 && raw >= 0, "a size word is negative");
    (tag as u16, scanned as usize, raw as usize)
}

/// `eml_rt_audit_data`の`what`が選ぶ内部の誤りの文言。VMの`check_data`の4つと同じである。
fn data_text(what: i64) -> &'static str {
    match what {
        DATA_UNPACK => "an unpack names a tag and number of fields the value does not have",
        DATA_SWITCH => "a switch case binds a different number of fields than the value has",
        DATA_RELEASE => "a release names a tag and number of fields the value does not have",
        DATA_STRUCT_UNBOX => {
            "a struct unbox names a tag and number of fields the value does not have"
        }
        _ => "an audit names a message that does not exist",
    }
}

crate::entries! {
    pub(crate) const HEAP;

    /// `Data`の物体を作る。`scanned`個の走査する語は`()`で埋めてあり、`raw`個の生の語はまだ書いていない。
    ///
    /// # Safety
    /// `ctx`はこのworkerの生きている`Ctx`である。
    pub(crate) shim[Alloc] fn eml_rt_alloc_data(ctx, tag: i64, scanned: i64, raw: i64, lin: i64) -> obj {
        let (tag, scanned, raw) = data_shape(tag, scanned, raw);
        // SAFETY: 新しい物体は、作る関数が走査する語まで書き終える。`Ctx`の語には触れない
        let w = unsafe { ctx.heap_mut() }.alloc_data(tag, scanned, raw, lin != 0);
        Ok::<Word, HeapError>(w)
    }

    /// `Int`を語にする。63ビットに収まれば即値で、収まらなければ`BigInt`の物体である。
    ///
    /// # Safety
    /// `ctx`はこのworkerの生きている`Ctx`である。
    pub(crate) shim[Alloc] fn eml_rt_alloc_int(ctx, n: i64) -> tobj {
        // SAFETY: 上と同じ
        let w = unsafe { ctx.heap_mut() }.alloc_int(n);
        Ok::<Word, HeapError>(w)
    }

    /// `Float`の物体を作る。引数はそのビット列を持つ`f64`である。
    ///
    /// # Safety
    /// `ctx`はこのworkerの生きている`Ctx`である。
    pub(crate) shim[Alloc] fn eml_rt_alloc_float(ctx, x: f64) -> obj {
        // SAFETY: 上と同じ
        let w = unsafe { ctx.heap_mut() }.alloc_float(x.to_bits());
        Ok::<Word, HeapError>(w)
    }

    /// 参照を1つ増やす。生成したコードは、速い道で済まない物体 (共有の物体と、数が張り付いた物体) のために遅い道で呼ぶ。
    ///
    /// # Safety
    /// `w`は即値か、このヒープか付けた塊の生きている物体で、呼ぶ側が参照を持つか、生きている持ち主から借りている。物体は
    /// `lin`ビットを持たない。数が正なら、持ち主は今のworkerである。検査モードでは、`w`について`eml_rt_audit_dup`が通った
    /// ばかりで、その後にヒープの操作を挟んでいない。
    pub(crate) shim[RcSlow] fn eml_rt_dup_slow(ctx, w: tobj) {
        // SAFETY: 呼ぶ側の約束。`Ctx`の語を手放さない
        unsafe { ctx.heap_mut().dup_body(w) }?;
        Ok::<(), HeapError>(())
    }

    /// インラインの`decref`の冷たい道。`eml_rt_audit_decref`が済ませた検査と数えを繰り返さずに、参照を手放す。速いモード
    /// では`eml_rt_decref`と同じである。
    ///
    /// # Safety
    /// `eml_rt_decref`と同じ。検査モードでは、`w`について`eml_rt_audit_decref`が通ったばかりである。
    pub(crate) shim[RcSlow] fn eml_rt_decref_slow(ctx, w: tobj) {
        // SAFETY: 呼ぶ側の約束。`Ctx`の語を手放さない
        unsafe { ctx.heap_mut().decref_body(w) }?;
        Ok::<(), HeapError>(())
    }

    /// インラインの数の加算の前の監査。
    ///
    /// # Safety
    /// `w`は`eml_rt_dup_slow`と同じ。
    pub(crate) shim[Audit] fn eml_rt_audit_dup(ctx, w: obj) {
        // 監査の述語は即値について何も言わないので、ポインタでない語はここで断る
        if !w.is_ptr() {
            return Err(HeapError::WrongKind);
        }
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.heap_mut().audit_dup(w) }?;
        Ok::<(), HeapError>(())
    }

    /// インラインの数の減算の前の監査。
    ///
    /// # Safety
    /// `w`は`eml_rt_decref`と同じ。
    pub(crate) shim[Audit] fn eml_rt_audit_decref(ctx, w: obj) {
        // 監査の述語は即値について何も言わないので、ポインタでない語はここで断る
        if !w.is_ptr() {
            return Err(HeapError::WrongKind);
        }
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.heap_mut().audit_decref(w) }?;
        Ok::<(), HeapError>(())
    }

    /// 物理添字`i`の走査する語のインラインの読みの前の監査。検査付きの読みで、値は捨てる。
    ///
    /// # Safety
    /// `w`は`Heap::field`と同じ。
    pub(crate) shim[Audit] fn eml_rt_audit_field(ctx, w: obj, i: i64) {
        // 検査付きの操作はポインタを前提にするので、即値はここで断る
        if !w.is_ptr() {
            return Err(HeapError::WrongKind);
        }
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.heap().field(w, i as usize) }?;
        Ok::<(), HeapError>(())
    }

    /// 生の語についての同じ監査。
    ///
    /// # Safety
    /// `w`は`Heap::raw`と同じ。
    pub(crate) shim[Audit] fn eml_rt_audit_raw(ctx, w: obj, i: i64) {
        // 検査付きの操作はポインタを前提にするので、即値はここで断る
        if !w.is_ptr() {
            return Err(HeapError::WrongKind);
        }
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.heap().raw(w, i as usize) }?;
        Ok::<(), HeapError>(())
    }

    /// 物理添字`i`の走査する語へ`v`をインラインで書く前の監査。検査付きの書き込みそのものである。インラインの書き込みは
    /// 同じ語をもう一度書く。
    ///
    /// # Safety
    /// `w`と`v`は`Heap::set_field`と同じ。
    pub(crate) shim[Audit] fn eml_rt_audit_store(ctx, w: obj, i: i64, v: tobj) {
        // 検査付きの操作はポインタを前提にするので、即値はここで断る
        if !w.is_ptr() {
            return Err(HeapError::WrongKind);
        }
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.heap_mut().set_field(w, i as usize, v) }?;
        Ok::<(), HeapError>(())
    }

    /// 生の語についての同じ監査。
    ///
    /// # Safety
    /// `w`は`Heap::set_raw`と同じ。
    pub(crate) shim[Audit] fn eml_rt_audit_store_raw(ctx, w: obj, i: i64, v: i64) {
        // 検査付きの操作はポインタを前提にするので、即値はここで断る
        if !w.is_ptr() {
            return Err(HeapError::WrongKind);
        }
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.heap_mut().set_raw(w, i as usize, v as u64) }?;
        Ok::<(), HeapError>(())
    }

    /// インラインの一意性の検査の前の監査。
    ///
    /// # Safety
    /// `w`は`Heap::is_unique`と同じ。
    pub(crate) shim[Audit] fn eml_rt_audit_unique(ctx, w: obj) {
        // 監査の述語は即値について何も言わないので、ポインタでない語はここで断る
        if !w.is_ptr() {
            return Err(HeapError::WrongKind);
        }
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.heap().is_unique(w) }?;
        Ok::<(), HeapError>(())
    }

    /// インラインのヘッダの読みの前の監査。
    ///
    /// # Safety
    /// `w`は`Heap::meta`と同じ。
    pub(crate) shim[Audit] fn eml_rt_audit_header(ctx, w: obj) {
        if !w.is_ptr() {
            return Err(HeapError::WrongKind);
        }
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.heap().meta(w) }?;
        Ok::<(), HeapError>(())
    }

    /// 関数の値の`meta`とコードの語のインラインの読みの前の監査。
    ///
    /// # Safety
    /// `w`は`Heap::meta`と`Heap::scanned(w, 0)`と同じ。生成したコードはrunの中でだけ呼ぶ。そこでは`ctx.runtime()`が
    /// runの`Worker`を指す。
    pub(crate) shim[Audit] fn eml_rt_audit_code(ctx, w: obj, expect: i64) {
        if !w.is_ptr() {
            return Err(HeapError::WrongKind);
        }
        let heap = ctx.heap();
        // SAFETY: 呼ぶ側の約束
        let (meta, code) = unsafe { (heap.meta(w)?, heap.scanned(w, 0)?) };
        if meta.kind() != Kind::Data || !code.is_imm() {
            return Err(HeapError::WrongKind);
        }
        // 続く呼び出しは、呼び出しの地点が選んだ署名でコードの語へ跳ぶ。だから語は、このimageのエントリで、その署名を
        // ちょうど持つものでなければならない
        let wanted = if expect == crate::image::CODE_OF_TAG {
            u64::from(meta.tag())
        } else {
            expect as u64
        };
        // SAFETY: 呼ぶ側の約束
        let found = unsafe { crate::worker::of(ctx) }.entry_sig(code.imm_value() as usize);
        if wanted == 0 || found != Some(wanted) {
            return Err(HeapError::WrongKind);
        }
        Ok::<(), HeapError>(())
    }

    /// 箱の値のインラインの読みの前の監査。`float`は`Int`の箱なら0、`Float`の箱なら1である。
    ///
    /// # Safety
    /// `w`は`Heap::unbox_int` (`float`が0) か`Heap::unbox_float` (1) と同じ。
    pub(crate) shim[Audit] fn eml_rt_audit_box(ctx, w: obj, float: i64) {
        if float != 0 {
            if !w.is_ptr() {
                return Err(HeapError::WrongKind);
            }
            // SAFETY: 呼ぶ側の約束。`w`はポインタである
            unsafe { ctx.heap().unbox_float(w) }?;
        } else {
            // `unbox_int`の前提は、即値かポインタであることである。ほかの語 (nullなど) はここで断る
            if !w.is_imm() && !w.is_ptr() {
                return Err(HeapError::WrongKind);
            }
            // SAFETY: 呼ぶ側の約束。`w`は即値かポインタである
            unsafe { ctx.heap().unbox_int(w) }?;
        }
        Ok::<(), HeapError>(())
    }

    /// 構成子を名指す文のフィールドの読みの前の監査。値は、この`tag`と`scanned`個の走査する語を持つ`Data`の物体である。
    /// 食い違えば、`what` (`DATA_*`) が選ぶ、VMと同じ文言の内部の誤りを置く。
    ///
    /// # Safety
    /// `w`は`Heap::meta`と同じ。生成したコードはrunの中でだけ呼ぶ。そこでは`ctx.runtime()`がrunの`Worker`を指す。
    pub(crate) shim[Audit] fn eml_rt_audit_data(ctx, w: obj, tag: i64, scanned: i64, what: i64) {
        let heap = ctx.heap();
        let shape = || -> Result<bool, HeapError> {
            if !w.is_ptr() {
                return Ok(false);
            }
            // SAFETY: 呼ぶ側の約束
            let (meta, n) = unsafe { (heap.meta(w)?, heap.scanned_len(w)?) };
            Ok(meta.kind() == Kind::Data && i64::from(meta.tag()) == tag && n as i64 == scanned)
        };
        match shape() {
            Ok(true) => Ok(()),
            // SAFETY: 呼ぶ側の約束
            Ok(false) => Err(unsafe { worker::record(ctx, RunFault::Internal(data_text(what)), None) }),
            Err(error) => Err(Raise::park(error, ctx)),
        }
    }

    /// 一意な箱のトークンを作る。検査モードの下ろし方だけが呼ぶ (速いコードは箱の語を使う)。
    ///
    /// # Safety
    /// `w`は`Heap::into_token`と同じ。
    pub(crate) shim[Reuse] fn eml_rt_into_token(ctx, w: obj) -> token {
        // 検査付きの操作はポインタを前提にするので、即値はここで断る
        if !w.is_ptr() {
            return Err(HeapError::WrongKind);
        }
        // SAFETY: 呼ぶ側の約束
        let token = unsafe { ctx.heap_mut().into_token(w) }?;
        Ok::<Word, HeapError>(token.into_word())
    }

    /// トークンの箱を取り、その場で書き直す。検査モードの下ろし方だけが呼ぶ。
    ///
    /// # Safety
    /// `token`は`Heap::reuse_in_place`と同じ。
    pub(crate) shim[Reuse] fn eml_rt_reuse_in_place(ctx, token: token, tag: i64, scanned: i64, lin: i64) -> obj {
        let (tag, scanned, _) = data_shape(tag, scanned, 0);
        debug_assert!(!layout::data_extended(scanned, false), "a scanned count needs a count word");
        let meta = layout::data_meta(tag, scanned, lin != 0);
        // SAFETY: 呼ぶ側の約束
        let w = unsafe {
            let token = Token::from_word(token);
            ctx.heap_mut().reuse_in_place(token, meta)
        }?;
        Ok::<Word, HeapError>(w)
    }

    /// workerの数え`which` (`COUNT_*`) に1を足す。
    ///
    /// # Safety
    /// 生成したコードはrunの中でだけ呼ぶ。そこでは`ctx.runtime()`がrunの`Worker`を指す。
    pub(crate) shim[Audit] fn eml_rt_audit_count(ctx, which: i64) {
        // SAFETY: 呼ぶ側の約束
        let worker = unsafe { worker::of(ctx) };
        worker.counts[which as usize] += 1;
        Ok::<(), Fatal>(())
    }

    /// 参照を1つ手放す。最後なら物体を解放し、子を手放す。
    ///
    /// # Safety
    /// `w`は即値か、このヒープか付けた塊の生きている物体で、呼ぶ側が手放す参照を1つ持つ。不死の物体なら、その参照は
    /// `acquire_immortal`か`dup`で作ったものである。共有の物体は`lin`ビットを持たない。数が正なら、持ち主は今のworkerである。
    pub(crate) shim[RcSlow] fn eml_rt_decref(ctx, w: tobj) {
        // SAFETY: 呼ぶ側の約束。`Ctx`の語を手放さない
        unsafe { ctx.heap_mut().decref(w) }?;
        Ok::<(), HeapError>(())
    }

    /// 最後の参照か (数が1か-1) を0か1で返す。
    ///
    /// # Safety
    /// `w`は即値か、このヒープか付けた塊の生きている物体である。数が正なら、持ち主は今のworkerである。
    pub(crate) shim[RcSlow] fn eml_rt_is_last(ctx, w: tobj) -> i64 {
        // SAFETY: 呼ぶ側の約束
        let last = unsafe { ctx.heap().is_last(w) }?;
        Ok::<i64, HeapError>(i64::from(last))
    }

    /// 最後の参照を持ち、子をもう処理した物体の箱を空ける。
    ///
    /// # Safety
    /// `w`はこのヒープの生きている物体で、呼ぶ側が最後の参照を持ち (数が1か-1)、子の参照は処理してある。種類は破棄処理を
    /// 持たない (`Foreign`でない)。数が正なら、持ち主は今のworkerである。
    pub(crate) shim[Free] fn eml_rt_free_block(ctx, w: obj) {
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.heap_mut().free_block(w) }?;
        Ok::<(), HeapError>(())
    }

    /// 箱を呼ぶ側だけが持つのでないときの`release`と同じに、呼ぶ側の`x`の参照を手放す。参照が最後なら、`keep`のバイトが
    /// 0の走査する語を小さい順に手放し、その後に箱を空ける。そうでなければ、バイトが0でない走査する語を小さい順に`dup`し、
    /// その後に`x`を手放す。ほかのヒープのエントリが包むヒープの操作の列で、VMの`release`と同じ順に呼ぶ
    /// (docs/spec/native.md の「ヒープのエントリ」)。
    ///
    /// # Safety
    /// `ctx`はこのworkerの生きている`Ctx`である。`x`は、このヒープか付けた塊の生きている`Data`の物体で、走査する語を
    /// `n`個持ち、呼ぶ側がその参照を1つ持つ。呼び出しはその参照を消費する。`x`が不死の物体なら、その参照は
    /// `acquire_immortal`か`dup`で作ったものである。`x`の数が正なら、持ち主は今のworkerである。共有の`x`は`lin`ビットを
    /// 持たない。`keep`は、呼び出しの間変わらない、読める`n`バイトのアドレスで、どのバイトも0か1である。`n`が0でもnullで
    /// ない。バイトが1の走査する語は、呼ぶ側が参照を取らずに`x`からもう束縛した子である。`x`の参照が最後でなければ、
    /// 呼び出しはその子に呼ぶ側の参照を1つ足すので、その子は`lin`ビットを持たない。最後なら、その子の参照は呼ぶ側に渡る。
    /// バイトが0の走査する語は呼ぶ側の`x`の参照の分と数え、その参照が最後なら呼び出しが手放す。
    pub(crate) shim[Free] fn eml_rt_release(ctx, x: obj, keep: ptr, n: i64) {
        let n = usize::try_from(n).map_err(|_| HeapError::WrongLayout)?;
        // SAFETY: 呼ぶ側の約束。バイトとして読むので、どの値も妥当である
        let keep = unsafe { std::slice::from_raw_parts(keep.cast::<u8>(), n) };
        // SAFETY: `Ctx`の語には触れない
        let heap = unsafe { ctx.heap_mut() };
        if cfg!(debug_assertions) {
            // SAFETY: 呼ぶ側の約束
            let (meta, scanned) = unsafe { (heap.meta(x)?, heap.scanned_len(x)?) };
            assert!(
                meta.kind() == Kind::Data && scanned == n,
                "a release names a shape the value does not have"
            );
            assert!(keep.iter().all(|&byte| byte <= 1), "a keep byte is not 0 or 1");
        }
        // SAFETY: 呼ぶ側の約束。どの欄も`x`を手放す前に読み、残す子は箱を手放す前に`dup`するので、呼ぶ側の参照が
        // 読む間ずっと`x`を生かしている。残さない子は呼ぶ側の最後の参照の分と数える
        // (docs/spec/object-model.md の「核の操作」)
        unsafe {
            if heap.is_last(x)? {
                for i in (0..n).filter(|&i| keep[i] == 0) {
                    let child = heap.scanned(x, i)?;
                    heap.decref(child)?;
                }
                heap.free_block(x)
            } else {
                for i in (0..n).filter(|&i| keep[i] != 0) {
                    let child = heap.scanned(x, i)?;
                    heap.dup(child)?;
                }
                heap.decref(x)
            }
        }
    }

    /// tokenの箱に`Data`のヘッダを書く。tokenが空なら確保する。フィールドは呼ぶ側が書く。
    ///
    /// # Safety
    /// `token`はこのヒープのtokenで、まだ使っていない。空でなければ、その箱は`scanned + raw`語の物体の級にある。
    /// 呼ぶ側は、次にヒープを操作する前に、走査する語をすべて書く。
    pub(crate) shim[Reuse] fn eml_rt_con_at(ctx, token: token, tag: i64, scanned: i64, raw: i64, lin: i64) -> obj {
        let (tag, scanned, raw) = data_shape(tag, scanned, raw);
        debug_assert!(!layout::data_extended(scanned, false), "a scanned count needs a count word");
        let meta = layout::data_meta(tag, scanned, lin != 0);
        // SAFETY: 呼ぶ側の約束。速いモードのtokenは箱の語か空の語なので、生成したコードが一意な箱の語をそのまま渡した
        // tokenは`Heap::into_token`が返す語と同じである (docs/spec/object-model.md の「検査モードの token」)
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
    pub(crate) shim[Reuse] fn eml_rt_drop_token(ctx, token: token) {
        // SAFETY: 呼ぶ側の約束。tokenの語は`eml_rt_con_at`と同じである
        unsafe {
            let token = Token::from_word(token);
            ctx.heap_mut().drop_token(token)
        }?;
        Ok::<(), HeapError>(())
    }

    /// 文字列かバイト列の先頭のバイトの位置。来歴を公開した番地で返す。生成したコードは、これをCの関数に貸す。読むだけで、
    /// 確保も解放もせず、参照の数も変えない。
    ///
    /// # Safety
    /// `Heap::text_payload`と同じ。`w`は呼ぶ側が借りている。
    pub(crate) shim[Service] fn eml_rt_text_payload(ctx, w: obj) -> i64 {
        // SAFETY: 呼ぶ側の約束
        let at = unsafe { ctx.heap().text_payload(w) }?;
        Ok::<i64, HeapError>(at.expose_provenance() as i64)
    }

    /// `UnboxedArray`の要素0の位置。来歴を公開した番地で返す。生成したコードは、これを読むだけのCの関数に貸す。読むだけで、
    /// 確保も解放もせず、参照の数も変えない。
    ///
    /// # Safety
    /// `Heap::unboxed_ptr`と同じ。`w`は呼ぶ側が借りている。
    pub(crate) shim[Service] fn eml_rt_unboxed_ptr(ctx, w: obj) -> i64 {
        // SAFETY: 呼ぶ側の約束
        let at = unsafe { ctx.heap().unboxed_ptr(w) }?;
        Ok::<i64, HeapError>(at.expose_provenance() as i64)
    }

    /// `MutUnboxedArray`の要素0の位置。来歴を公開した番地で返す。生成したコードは、これを要素を書いてよいCの関数に貸す。
    /// 読むだけで、確保も解放もせず、参照の数も変えない。
    ///
    /// # Safety
    /// `Heap::unboxed_ptr_mut`と同じ。`w`は呼ぶ側が借りている。
    pub(crate) shim[Service] fn eml_rt_unboxed_ptr_mut(ctx, w: obj) -> i64 {
        // SAFETY: 呼ぶ側の約束
        let at = unsafe { ctx.heap().unboxed_ptr_mut(w) }?;
        Ok::<i64, HeapError>(at.expose_provenance() as i64)
    }

    /// 算術の失敗を`Ctx`に置く。種類は`ARITH_*`である。置いた失敗は、生成したコードが呼び出しの後で確かめる。
    pub(crate) shim[Service] fn eml_rt_fault_arith(ctx, kind: i64) {
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
