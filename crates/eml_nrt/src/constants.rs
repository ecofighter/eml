//! ネイティブのrunの読み込み時の値。凍結した領域に不死の物体として作る (docs/spec/native.md の「`Image`」)。
#![allow(unsafe_code)]

use std::sync::Arc;

use eml_rt::{Control, Fast, Frozen, Heap, HeapError, Word};
use eml_sched::constants::{immortal_big_int, immortal_float};

/// 中身で表した読み込み時の値1つ。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Datum<'a> {
    String(&'a str),
    BigInt(i64),
    Float(u64),
}

/// 値の語 (値と同じ順) と、その物体を持つ凍結した塊。値がなければ塊もない。
pub(crate) struct Built {
    pub(crate) words: Vec<Word>,
    pub(crate) region: Option<Arc<Frozen<Fast>>>,
}

/// `data`を、VMが定数を作るのと同じ関数で作る。だから定数の表現は、どちらのエンジンでも同じ1つである。
pub(crate) fn build(control: &Arc<Control>, data: &[Datum]) -> Result<Built, HeapError> {
    if data.is_empty() {
        return Ok(Built {
            words: Vec::new(),
            region: None,
        });
    }
    let mut heap = Heap::<Fast>::with_control(Arc::clone(control));
    let words = data
        .iter()
        .map(|datum| match *datum {
            Datum::String(text) => heap.alloc_string_immortal(text),
            Datum::BigInt(n) => immortal_big_int(&mut heap, n),
            Datum::Float(bits) => immortal_float(&mut heap, bits),
        })
        .collect();
    Ok(Built {
        words,
        region: Some(Arc::new(heap.freeze()?)),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use eml_rt::{Buffering, Control, Ctx, Fast, HostRef, Kind, Limits, StdHost, layout};

    use super::{Built, Datum, build};

    fn ctx(control: &Arc<Control>) -> Ctx<Fast> {
        let host = StdHost::new(
            Box::new(std::io::sink()),
            Box::new(std::io::sink()),
            std::path::PathBuf::new(),
            Buffering::Block,
        );
        Ctx::new(Arc::clone(control), HostRef::new(Arc::new(host)))
    }

    #[test]
    fn no_data_builds_no_region() {
        let control = Arc::new(Control::new(Limits::default()));
        let built = build(&control, &[]).unwrap();
        assert!(built.words.is_empty() && built.region.is_none());
    }

    #[test]
    fn each_datum_becomes_its_object() {
        let control = Arc::new(Control::new(Limits::default()));
        let data = [
            Datum::String("héllo"),
            Datum::BigInt(i64::MAX),
            Datum::Float((-0.0f64).to_bits()),
        ];
        // `freeze`は、どの物体も不死のときだけ成功する
        let Built { words, region } = build(&control, &data).unwrap();
        let mut ctx = ctx(&control);
        // SAFETY: 塊を付けても、`Ctx`の持つ語は変わらない
        unsafe { ctx.heap_mut() }.attach(region.unwrap());
        // SAFETY: どの語も、付けた塊の物体である
        unsafe {
            assert_eq!(ctx.copy_string(words[0], usize::MAX).unwrap(), "héllo");
            let heap = ctx.heap();
            for (w, kind, value) in [
                (words[1], Kind::BigInt, i64::MAX as u64),
                (words[2], Kind::Float, (-0.0f64).to_bits()),
            ] {
                assert_eq!(heap.meta(w).unwrap().kind(), kind);
                assert_eq!(heap.raw(w, layout::BOX_VALUE_WORD).unwrap(), value);
            }
        }
    }
}
