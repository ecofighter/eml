//! ネイティブのrunの読み込み時の値。凍結した領域に不死の物体として作る (docs/spec/native.md の「`Image`」)。
#![allow(unsafe_code)]

use std::sync::Arc;

use eml_rows::constants::{empty_mut_array, immortal_big_int, immortal_float};
use eml_rt::{Control, Fast, Frozen, Heap, HeapError, MaskItem, Word, layout};

use crate::closures::code_word;
use crate::image::{DATUM_EFFECT, DatumRow};

/// 中身で表した読み込み時の値1つ。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Datum<'a> {
    String(&'a str),
    BigInt(i64),
    Float(u64),
    /// 関数の静的なクロージャ。`arity`個の引数を待ち、エントリのコードは`code`にある。
    Fn {
        arity: u16,
        code: *const u8,
    },
    /// 長さ0の`MutArray`。VMの定数と同じ関数で作る。
    EmptyMutArray,
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
            Datum::Fn { arity, code } => static_closure(&mut heap, arity, code),
            Datum::EmptyMutArray => empty_mut_array(&mut heap),
        })
        .collect();
    Ok(Built {
        words,
        region: Some(Arc::new(heap.freeze()?)),
    })
}

/// 不死のクロージャ`{ tag = arity, [code] }`。捕まえた値はない (docs/spec/native.md の「関数の値」)。
fn static_closure(heap: &mut Heap<Fast>, arity: u16, code: *const u8) -> Word {
    // SAFETY: 構築中の物体で、ほかの操作の前にコードの語を書き終える。コードの語は即値なので、参照を移さない
    unsafe {
        let w = heap.alloc_immortal(1, layout::data_meta(arity, 1, false));
        heap.set_field(w, layout::data_first(1, false), code_word(code))
            .expect("a fresh closure takes its code word");
        w
    }
}

/// runが物体にしない値 (`DATUM_EFFECT`か`DATUM_MASK`の行) をそのslotに書く。エフェクトは番号そのものを、マスクは
/// 項目を順に`MaskItem`として書く (docs/spec/native.mdの「`Image`」)。
///
/// # Safety
/// `row.kind`は`DATUM_EFFECT`か`DATUM_MASK`である。`row.slot`は、ほかの誰も使わない書ける領域で、8にそろい、
/// `DATUM_MASK`では`mask_bytes(row.items.len)`バイト、`DATUM_EFFECT`では1語である。`DATUM_MASK`の行の`items`は、
/// 生きている行を指す。
pub(crate) unsafe fn write_raw(row: &DatumRow) {
    if row.kind == DATUM_EFFECT {
        // SAFETY: 呼ぶ側の約束
        unsafe { row.slot.write(row.bits) };
        return;
    }
    let first = row.slot.cast::<MaskItem>();
    // SAFETY: 呼ぶ側の約束。slotは8にそろい、`MaskItem`のそろえはそれ以下である (`image.rs`)
    for (i, item) in unsafe { row.items.as_slice() }.iter().enumerate() {
        let item = MaskItem {
            effect: item.effect,
            keep: item.keep,
            skip: item.skip,
        };
        // SAFETY: 呼ぶ側の約束。slotは`mask_bytes(items.len)`バイトである
        unsafe { first.add(i).write(item) };
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use eml_rt::{Buffering, Control, Ctx, Fast, HostRef, Kind, Limits, StdHost, layout};

    use eml_rt::MaskItem;

    use super::{Built, Datum, build, write_raw};
    use crate::image::{DATUM_EFFECT, DATUM_MASK, DatumRow, MaskRow, Slice, Str, mask_bytes};

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

    #[test]
    fn an_empty_mut_array_datum_is_immortal() {
        let control = Arc::new(Control::new(Limits::default()));
        // `freeze`は、どの物体も不死のときだけ成功する
        let Built { words, region } = build(&control, &[Datum::EmptyMutArray]).unwrap();
        let mut ctx = ctx(&control);
        // SAFETY: 塊を付けても、`Ctx`の持つ語は変わらない
        unsafe { ctx.heap_mut() }.attach(region.unwrap());
        // SAFETY: 語は付けた塊の物体である
        unsafe {
            let heap = ctx.heap();
            let meta = heap.meta(words[0]).unwrap();
            assert_eq!(meta.kind(), Kind::Data);
            assert!(meta.lin());
            assert_eq!(heap.scanned_len(words[0]).unwrap(), 0);
            assert!(heap.is_immortal(words[0]));
        }
    }

    /// エフェクトの行は番号そのものを1語に、項目が2つのマスクの行は項目を`MaskItem`として順に書く。slotはRustの
    /// バッファなので、Miriが書く範囲とそろえを確かめる。
    #[test]
    fn raw_rows_are_written_in_place() {
        let mut effect = vec![0u64; 1];
        let items = [
            MaskRow {
                effect: 3,
                keep: 0,
                skip: 1,
            },
            MaskRow {
                effect: 7,
                keep: 1,
                skip: 2,
            },
        ];
        let mut mask = vec![0u64; mask_bytes(items.len()).div_ceil(8)];
        let row = |kind, bits, slot, items| DatumRow {
            kind,
            bits,
            text: Str::new(""),
            code: std::ptr::null(),
            slot,
            items,
        };
        // SAFETY: どちらのslotもこのテストのバッファで、8にそろい、種類の求める大きさがある
        unsafe {
            write_raw(&row(DATUM_EFFECT, 5, effect.as_mut_ptr(), Slice::new(&[])));
            write_raw(&row(DATUM_MASK, 0, mask.as_mut_ptr(), Slice::new(&items)));
        }
        assert_eq!(effect, [5]);
        // SAFETY: `write_raw`が`MaskItem`を2つ書いた
        let written = unsafe { std::slice::from_raw_parts(mask.as_ptr().cast::<MaskItem>(), 2) };
        assert_eq!(
            written,
            [
                MaskItem {
                    effect: 3,
                    keep: 0,
                    skip: 1
                },
                MaskItem {
                    effect: 7,
                    keep: 1,
                    skip: 2
                },
            ]
        );
    }
}
