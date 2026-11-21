//! `Std.Array.concat`と`Std.UnboxedArray.concat`の本体 (docs/spec/stdlib.mdの「`Std.Array`」)。`Ctx`の配列の操作だけで
//! 書くので、検査モードの確かめはそれぞれの操作が持つ。

use eml_rt::externs::Fault;
use eml_rt::{Ctx, Mode, Word};

/// `chunks`の要素の長さの和。和が`i64`に収まらなければ`i64::MAX`にし、その長さは`make`が作れない大きさとして断る。
///
/// # Safety
/// `chunks`は即値か、このヒープか付けた塊の生きている配列で、その要素は`length`が受ける、同じく生きている配列
/// である。
unsafe fn total<M: Mode>(
    ctx: &mut Ctx<M>,
    chunks: Word,
    length: unsafe fn(&mut Ctx<M>, Word) -> Result<i64, Fault>,
) -> Result<i64, Fault> {
    // SAFETY: 呼ぶ側の約束。`array_get`が返した参照は、長さを読んだ後で手放す
    unsafe {
        let mut sum = 0i64;
        for c in 0..ctx.array_length(chunks)? {
            let chunk = ctx.array_get(chunks, c)?;
            let len = length(ctx, chunk);
            ctx.heap_mut().decref(chunk).map_err(Fault::Heap)?;
            sum = sum.saturating_add(len?);
        }
        Ok(sum)
    }
}

/// 確保を断られたら、受け取った`chunks`を手放してから誤りを返す。
///
/// # Safety
/// `chunks`は即値か、このヒープか付けた塊の生きている物体で、呼ぶ側がその参照を1つ持つ。
unsafe fn made<M: Mode>(
    ctx: &mut Ctx<M>,
    chunks: Word,
    made: Result<Word, Fault>,
) -> Result<Word, Fault> {
    if made.is_err() {
        // SAFETY: 呼ぶ側の約束
        unsafe { ctx.heap_mut().decref(chunks) }.map_err(Fault::Heap)?;
    }
    made
}

/// `chunks`の要素を順につないだ新しい`Array`を返す。`chunks`の参照を受け取り、要素ごとに参照を1つ増やしてから
/// `chunks`を手放す。
///
/// # Safety
/// `heap_mut`の前提を守っている。`chunks`は即値か、このヒープか付けた塊の
/// 生きている`Array (Array a)`の値 (その要素も同じ) で、呼ぶ側がその参照を1つ持つ。
pub(crate) unsafe fn array_concat<M: Mode>(ctx: &mut Ctx<M>, chunks: Word) -> Result<Word, Fault> {
    // SAFETY: 呼ぶ側の約束。`out`はここで作った一意の配列なので、`array_set`はその場で書く。`()`は即値なので、
    // 始めの要素にも、`array_set`が手放す前の要素にも参照の数がない
    unsafe {
        let n = total(ctx, chunks, Ctx::array_length)?;
        let first = ctx.array_make(n, Word::UNIT, false);
        let mut out = made(ctx, chunks, first)?;
        let mut at = 0;
        for c in 0..ctx.array_length(chunks)? {
            let chunk = ctx.array_get(chunks, c)?;
            for i in 0..ctx.array_length(chunk)? {
                let x = ctx.array_get(chunk, i)?;
                out = ctx.array_set(out, at, x)?;
                at += 1;
            }
            ctx.heap_mut().decref(chunk).map_err(Fault::Heap)?;
        }
        ctx.heap_mut().decref(chunks).map_err(Fault::Heap)?;
        Ok(out)
    }
}

/// `array_concat`の生の要素の形。要素は語を写すだけで、参照を数えない。
///
/// # Safety
/// `heap_mut`の前提を守っている。`chunks`は即値か、このヒープか付けた
/// 塊の生きている`Array (UnboxedArray a)`の値 (その要素も同じ) で、呼ぶ側がその参照を1つ持つ。
pub(crate) unsafe fn unboxed_concat<M: Mode>(
    ctx: &mut Ctx<M>,
    chunks: Word,
) -> Result<Word, Fault> {
    // SAFETY: 呼ぶ側の約束。`out`はここで作った一意の配列なので、`unboxed_set`はその場で書く
    unsafe {
        let n = total(ctx, chunks, Ctx::unboxed_length)?;
        let first = ctx.unboxed_make(n, 0, false);
        let mut out = made(ctx, chunks, first)?;
        let mut at = 0;
        for c in 0..ctx.array_length(chunks)? {
            let chunk = ctx.array_get(chunks, c)?;
            for i in 0..ctx.unboxed_length(chunk)? {
                let x = ctx.unboxed_get(chunk, i)?;
                out = ctx.unboxed_set(out, at, x)?;
                at += 1;
            }
            ctx.heap_mut().decref(chunk).map_err(Fault::Heap)?;
        }
        ctx.heap_mut().decref(chunks).map_err(Fault::Heap)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::Arc;

    use eml_rt::{Buffering, Checked, Control, HostRef, Limits, StdHost, Stop};

    use super::*;

    fn ctx(max_bytes: usize) -> Ctx<Checked> {
        let host = StdHost::new(
            Box::new(io::sink()),
            Box::new(io::sink()),
            Default::default(),
            Buffering::Line,
        );
        let limits = Limits {
            max_bytes,
            ..Limits::default()
        };
        Ctx::new(Arc::new(Control::new(limits)), HostRef::new(Arc::new(host)))
    }

    /// 断られた長さはメモリの上限の誤りになり、何も残さない。行は受け取った引数を手放す。
    #[test]
    fn a_refused_length_releases_the_argument() {
        let mut ctx = ctx(1 << 24);
        // SAFETY: 語はここで作った生きている配列で、参照はそれぞれ1度だけ渡す
        unsafe {
            let chunk = ctx.array_make(1000, Word::UNIT, false).unwrap();
            let chunks = ctx.array_make(100_000, chunk, false).unwrap();
            assert_eq!(
                array_concat(&mut ctx, chunks),
                Err(Fault::Stopped(Stop::MemoryLimit))
            );
            let chunk = ctx.unboxed_make(1000, 0, false).unwrap();
            let chunks = ctx.array_make(100_000, chunk, false).unwrap();
            assert_eq!(
                unboxed_concat(&mut ctx, chunks),
                Err(Fault::Stopped(Stop::MemoryLimit))
            );
        }
        assert_eq!(ctx.check_invariants(), Ok(()));
        assert!(ctx.leaks().is_empty(), "{:?}", ctx.leaks());
    }

    /// 要素ごとに参照を1つ増やして外側の配列を手放すので、結果を手放せばすべてが解放される。
    #[test]
    fn the_result_owns_one_reference_per_element() {
        let mut ctx = ctx(usize::MAX);
        // SAFETY: 語はここで作った生きている物体で、参照はそれぞれ1度だけ渡す
        unsafe {
            let s = ctx.fresh_string("s");
            let chunk = ctx.array_make(3, s, false).unwrap();
            let chunks = ctx.array_make(2, chunk, false).unwrap();
            let all = array_concat(&mut ctx, chunks).unwrap();
            assert_eq!(ctx.array_length(all), Ok(6));
            let x = ctx.array_get(all, 5).unwrap();
            assert_eq!(ctx.copy_string(x, usize::MAX), Ok("s".to_string()));
            ctx.heap_mut().decref(all).unwrap();
        }
        assert_eq!(ctx.check_invariants(), Ok(()));
        assert!(ctx.leaks().is_empty(), "{:?}", ctx.leaks());
    }
}
