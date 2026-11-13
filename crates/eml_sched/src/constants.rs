//! 定数の箱を作る関数。VMとネイティブのコードが同じ実装を使うので、定数の箱の表現は1か所で決まる
//! (docs/spec/object-model.md の「核の操作」)。物体はどちらも不死で、検査モードでも参照を数えない。

use eml_rt::{Heap, Kind, Meta, Mode, Word, layout};

/// 即値に収まらない`Int`の不死の`BigInt`の箱。
pub fn immortal_big_int<M: Mode>(heap: &mut Heap<M>, n: i64) -> Word {
    immortal_box(heap, Kind::BigInt, n as u64)
}

/// 不死の`Float`の箱。値はビット列で渡すので、`0.0`と`-0.0`やNaNの中身も保たれる。
pub fn immortal_float<M: Mode>(heap: &mut Heap<M>, bits: u64) -> Word {
    immortal_box(heap, Kind::Float, bits)
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
