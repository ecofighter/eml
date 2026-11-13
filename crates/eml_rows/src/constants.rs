//! 定数の箱と空の`MutArray`を作る関数。VMとネイティブのコードが定数の箱に同じ実装を使うので、その表現は1か所で
//! 決まる (docs/spec/object-model.mdの「核の操作」)。空の`MutArray`はVMが使う。物体はどれも不死で、検査モードでも
//! 参照を数えない。

use eml_rt::{Heap, Kind, Meta, Mode, Word, layout};

/// 即値に収まらない`Int`の不死の`BigInt`の箱。
pub fn immortal_big_int<M: Mode>(heap: &mut Heap<M>, n: i64) -> Word {
    immortal_box(heap, Kind::BigInt, n as u64)
}

/// 不死の`Float`の箱。値はビット列で渡すので、`0.0`と`-0.0`やNaNの中身も保たれる。
pub fn immortal_float<M: Mode>(heap: &mut Heap<M>, bits: u64) -> Word {
    immortal_box(heap, Kind::Float, bits)
}

/// 長さ0の`MutArray`。`lin`ビットを立てた不死の`Data`で、ほかの配列と同じく数の語 (0) を持つ。長さが0なので、
/// 添字を確かめて読み書きする所はどれもこの物体を書き換えない (docs/spec/object-model.mdの「凍結した領域」)。
pub fn empty_mut_array<M: Mode>(heap: &mut Heap<M>) -> Word {
    heap.alloc_array_immortal(true)
}

fn immortal_box<M: Mode>(heap: &mut Heap<M>, kind: Kind, value: u64) -> Word {
    // 構築中の物体で、ほかの操作の前に値の語を書き終える (docs/spec/object-model.md の「核の操作」)
    unsafe {
        let w = heap.alloc_immortal(layout::BOX_WORDS, Meta::new(0, 0, kind, false));
        heap.set_raw(w, layout::BOX_VALUE_WORD, value)
            .expect("a fresh box takes its value");
        w
    }
}

#[cfg(test)]
mod tests {
    use eml_rt::{Checked, Heap, Kind};

    use super::empty_mut_array;

    #[test]
    fn the_empty_mut_array_is_immortal_lin_and_empty() {
        let mut heap = Heap::<Checked>::new();
        let w = empty_mut_array(&mut heap);
        unsafe {
            let meta = heap.meta(w).unwrap();
            assert_eq!(meta.kind(), Kind::Data);
            assert!(meta.lin());
            assert_eq!(heap.scanned_len(w).unwrap(), 0);
            assert!(heap.is_immortal(w));
        }
    }
}
