//! ヒープの単体テスト (docs/spec/object-model.md の「核の操作」)。モードを型の引数に
//! 取る関数を `both_modes!` で2つのモードのテストにし、同じ列を速いモードと検査モードで流す。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{Heap, Mode, Word};

/// モードを型の引数に取るテストの関数を、速いモードと検査モードの2つのテストにする。
macro_rules! both_modes {
    ($($name:ident),* $(,)?) => {
        mod fast {
            $( #[test] fn $name() { super::$name::<crate::Fast>() } )*
        }
        mod checked {
            $( #[test] fn $name() { super::$name::<crate::Checked>() } )*
        }
    };
}

mod basics;
mod checked;
mod reuse;
mod share;

/// Miri で遅くなりすぎない長さ。
pub(super) const LONG: usize = if cfg!(miri) { 300 } else { 100_000 };

pub(super) const CONS: u16 = 1;

/// cons セル (要素, 残り)。
pub(super) fn cons<M: Mode>(heap: &mut Heap<M>, head: Word, tail: Word) -> Word {
    let cell = heap.alloc_data(CONS, 2, 0, false);
    unsafe {
        heap.set_field(cell, 0, head).unwrap();
        heap.set_field(cell, 1, tail).unwrap();
    }
    cell
}

/// 即値の要素の長さ n のリスト。
pub(super) fn list<M: Mode>(heap: &mut Heap<M>, n: usize) -> Word {
    (0..n).fold(Word::imm(0), |tail, i| {
        cons(heap, Word::imm(i as i64), tail)
    })
}

/// ダミーのほかに生きている箱の数。
pub(super) fn mortal<M: Mode>(heap: &Heap<M>) -> usize {
    heap.live_objects().len() - 1
}

/// 破棄処理が呼ばれた回数を数える `Foreign` の値。
pub(super) struct Probe(pub(super) Arc<AtomicUsize>);

impl Drop for Probe {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

pub(super) fn probe() -> (Probe, Arc<AtomicUsize>) {
    let drops = Arc::new(AtomicUsize::new(0));
    (Probe(drops.clone()), drops)
}
